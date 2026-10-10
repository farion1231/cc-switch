//! Codex 供应商编辑器：底部的 config.toml 就是「切到这个供应商之后 config.toml 的样子」。
//!
//! - 显示：在内存里对当前 live 做一次切换投影（和切换用同一个补丁），关键字段、独有字段
//!   换成这个供应商的，其余部分是 live 原样。Key 显示在 API Key 输入框里（行的 `auth`），
//!   不在 TOML 里重复。
//! - 保存：关键字段、独有字段写回这个供应商的行（行里其余内容原样保留）；其余部分的改动
//!   是 Codex 的全局设置，经引擎写进 live，只改用户动过的键。编辑的是直连模式下的当前
//!   供应商时，关键字段和独有字段在同一次写入里也换进 live。
//! - 三方比较：每个改动都带着打开编辑器时的原值，live 里这个键已经被别的程序改成了第三个
//!   值就算冲突，由用户选保留哪一边。
//!
//! 改动的粒度：顶层的值；顶层表里的每个键（`[mcp_servers.fs]` 这类子表按整张算）；
//! `[model_providers]` 下 CC Switch 路由表以外的每张表。嵌在用户表里的模型名是关键字段，
//! 不算全局改动。

use std::sync::Arc;

use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item};

use crate::app_config::AppType;
use crate::codex_config::get_codex_config_path;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{read_current, LiveFile};
use crate::live::floor;
use crate::live::patch::toml::parse;
use crate::live::project::codex::{
    is_keyless_fallback, CodexProjection, Route, RowInput, OFFICIAL_PROXY_ROUTE_ID, ROUTE_ID,
};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};
use crate::provider::{Provider, ProviderMeta};
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::store::AppState;

use super::claude_editor::{ConflictPolicy, EditorView, InactiveField};
use super::codex_direct::{self, Owner, Prepared, Target};
use super::editor_toml::{self, config_text, insert_at, render, Entry, TomlEdits};

fn app() -> &'static str {
    AppType::Codex.as_str()
}

fn is_nested_floor(parent: &str, key: &str) -> bool {
    floor::CODEX_FLOOR_NESTED
        .iter()
        .any(|segments| segments.len() == 2 && segments[0] == parent && segments[1] == key)
}

/// 全局设置的每个位置：关键字段、独有字段、CC Switch 的路由表不算。`skip_routes` 是
/// 配置选中的路由表：它归供应商（投影时按内容收成 custom 表），也不算。
fn entries(doc: &DocumentMut, skip_routes: &[&str]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (key, item) in doc.as_table().iter() {
        if floor::CODEX_FLOOR_TOP.contains(&key) || floor::CODEX_EXCLUSIVE_TOP.contains(&key) {
            continue;
        }
        if key == "model_providers" {
            if let Some(providers) = item.as_table_like() {
                for (id, table) in providers.iter() {
                    if id == ROUTE_ID || id == OFFICIAL_PROXY_ROUTE_ID || skip_routes.contains(&id)
                    {
                        continue;
                    }
                    entries.push(Entry {
                        path: vec![key.to_string(), id.to_string()],
                        item: table.clone(),
                    });
                }
            }
            continue;
        }
        match item.as_table_like() {
            Some(table) => {
                for (child, child_item) in table.iter() {
                    if is_nested_floor(key, child) {
                        continue;
                    }
                    entries.push(Entry {
                        path: vec![key.to_string(), child.to_string()],
                        item: child_item.clone(),
                    });
                }
            }
            None => entries.push(Entry {
                path: vec![key.to_string()],
                item: item.clone(),
            }),
        }
    }
    entries
}

fn parse_text(text: &str, what: &str) -> Result<DocumentMut, AppError> {
    editor_toml::parse_text(text, "provider.codex.editor.invalid_toml", "Codex", what)
}

fn selected_route(doc: &DocumentMut) -> Option<&str> {
    doc.get("model_provider").and_then(Item::as_str)
}

/// 编辑器显示的内容。`settings_config` 是这个供应商的行（新增时是空对象）。
pub fn view(
    state: &AppState,
    settings_config: &Value,
    category: Option<&str>,
    meta: Option<&ProviderMeta>,
) -> Result<EditorView, AppError> {
    let path = get_codex_config_path();
    let pre = read_current(&path)?;
    let mut doc = parse(&path, pre.as_deref())?;

    let mut provider =
        Provider::with_id(String::new(), String::new(), settings_config.clone(), None);
    provider.category = category.map(str::to_string);
    provider.meta = meta.cloned();
    let live_owner = LiveOwner::read(state)?;
    let planned = plan_for_view(&state.db, &live_owner.owner(), &provider)?;
    planned.config().apply_to(&path, &mut doc)?;

    // Key 在 API Key 输入框里（行的 auth），TOML 里不再重复显示。
    let row_key = settings_config
        .get("auth")
        .and_then(crate::codex_config::extract_codex_auth_api_key);
    if let Some(route) = doc
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .and_then(|providers| providers.get_mut(ROUTE_ID))
        .and_then(Item::as_table_like_mut)
    {
        let injected = route
            .get("experimental_bearer_token")
            .and_then(Item::as_str)
            .map(str::to_string);
        if injected.is_some() && (injected == row_key || injected.as_deref() == Some(PENDING_KEY)) {
            route.remove("experimental_bearer_token");
        }
    }

    let display = doc.to_string();
    let mut settings = settings_config
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);
    settings.insert("config".to_string(), Value::String(display.clone()));
    settings
        .entry("auth".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    Ok(EditorView {
        inactive: inactive_fields(config_text(settings_config), &doc),
        settings: Value::Object(settings),
    })
}

/// 还没填 Key 的行按「填了 Key」投影时用的占位 Key。只出现在内存里的投影中：显示前、存行
/// 前都去掉，从不写盘。
const PENDING_KEY: &str = "cc-switch-editor-pending-key";

/// `settings` 换上占位 Key（没有 `auth` 就补一个）。
fn with_pending_key(settings: &Value) -> Option<Value> {
    let mut settings = settings.clone();
    let auth = settings
        .as_object_mut()?
        .entry("auth")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    auth.insert(
        "OPENAI_API_KEY".to_string(),
        Value::String(PENDING_KEY.to_string()),
    );
    Some(settings)
}

/// 编辑器显示用的投影。新增对话框一打开就投影自定义模板，选第三方预设也会投影，这时 Key
/// 还没填：行里的 `requires_openai_auth = true`（或顶层 `openai_base_url`）会被切换的
/// 安全闸拒绝。显示不该拒：Key 本来就不在 TOML 里显示，填没填显示都一样。只对这一个错误
/// 按占位 Key 再投影一次；再投影也不行就报原来的错。安全闸留在写 live 的地方（切换、编辑
/// 当前供应商、重写代理契约），它们用的都是真实的行。
fn plan_for_view(
    db: &Database,
    owner: &Owner<'_>,
    provider: &Provider,
) -> Result<codex_direct::Planned, AppError> {
    let plan = |provider: &Provider| {
        codex_direct::plan(
            db,
            owner,
            &Target::Direct(Some(provider)),
            &Prepared::default(),
        )
    };
    let error = match plan(provider) {
        Err(error) if is_keyless_fallback(&error) => error,
        other => return other,
    };
    let Some(settings) = with_pending_key(&provider.settings_config) else {
        return Err(error);
    };
    let mut pending = provider.clone();
    pending.settings_config = settings;
    plan(&pending).map_err(|_| error)
}

/// 保存时拆行用的投影。没填 Key 的行照样能存（和不经编辑器的新增一样，表单会先确认一次）：
/// 存行不写 live。行要进 live 时（编辑直连的当前供应商、新增第一个供应商、它是代理路由那
/// 家）写入按真实的行再投影一次，安全闸在那里拦，行跟着撤回。
fn project_for_save(input: &RowInput<'_>) -> Result<CodexProjection, AppError> {
    let error = match CodexProjection::of(input) {
        Err(error) if is_keyless_fallback(&error) => error,
        other => return other,
    };
    let Some(settings) = with_pending_key(input.settings) else {
        return Err(error);
    };
    let mut projection = CodexProjection::of(&RowInput {
        settings: &settings,
        official: input.official,
        proxy_injected_oauth: input.proxy_injected_oauth,
    })
    .map_err(|_| error)?;
    if let Route::Custom { table, .. } = &mut projection.route {
        if table
            .get("experimental_bearer_token")
            .and_then(Item::as_str)
            == Some(PENDING_KEY)
        {
            table.remove("experimental_bearer_token");
        }
    }
    Ok(projection)
}

/// 行里保存着、但不随切换生效的全局设置。
fn inactive_fields(row_text: &str, display: &DocumentMut) -> Vec<InactiveField> {
    let Ok(row) = row_text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    editor_toml::inactive_fields(entries(&row, selected_route(&row).as_slice()), display)
}

/// 一次编辑器保存：存进行的内容，和要写进 live 的全局改动。
pub(crate) struct CodexEditorPlan {
    pub row_settings: Value,
    pub edits: TomlEdits,
}

/// live 现在归谁：接上代理时是契约，否则是直连指针那家。
struct LiveOwner {
    mode: crate::mode::state::ModeState,
    direct: Option<Provider>,
}

impl LiveOwner {
    fn read(state: &AppState) -> Result<Self, AppError> {
        Ok(Self {
            mode: crate::mode::current::mode_state(&AppType::Codex),
            direct: crate::mode::current::direct_provider(&state.db, &AppType::Codex)?,
        })
    }

    fn owner(&self) -> Owner<'_> {
        match (&self.mode.contract, self.mode.attached) {
            (Some(contract), true) => Owner::Contract {
                contract,
                route: None,
            },
            _ => self.direct.as_ref().map_or(Owner::None, Owner::Provider),
        }
    }
}

/// live 里用户自己的独有字段：live 现在对应的那家带进来的（值还相同的）不算。只给不知道
/// 草稿的新增用（见 [`Origin::Live`]）。
///
/// 只读 live、按值去掉那一家的，不走投影：投影会校验生效的 profile，而空行不写
/// `model_provider`，profile 选了路由表就会被当成覆盖路由拒绝。
pub(crate) fn live_exclusive(state: &AppState) -> Result<Vec<Entry>, AppError> {
    let path = get_codex_config_path();
    let pre = read_current(&path)?;
    let doc = parse(&path, pre.as_deref())?;
    let owned = codex_direct::outgoing_exclusive(&LiveOwner::read(state)?.owner());
    Ok(exclusive_entries(&doc)
        .into_iter()
        .filter(|entry| {
            !owned.iter().any(|(key, value)| {
                entry.path[0] == *key && render(&entry.item) == render(&Item::Value(value.clone()))
            })
        })
        .collect())
}

fn exclusive_entries(doc: &DocumentMut) -> Vec<Entry> {
    floor::CODEX_EXCLUSIVE_TOP
        .iter()
        .filter_map(|key| {
            Some(Entry {
                path: vec![(*key).to_string()],
                item: doc.get(key)?.clone(),
            })
        })
        .collect()
}

/// 编辑器显示里的独有字段是从哪来的：显示的是某份配置投影到 live 上的样子，这份配置里没有
/// 的独有字段就是从 live 带进来的。
pub(crate) enum Origin {
    /// 投影成底的那份配置：编辑已有供应商时是它的行，新增时是预设草稿。
    Row(DocumentMut),
    /// 新增时不知道草稿（旧的调用方）：和 live 里用户自己的独有字段比值，打开之后 live 被
    /// 改过就分不准。
    Live(Vec<Entry>),
}

impl Origin {
    /// `settings` 是行或草稿的 `settings_config`。
    pub(crate) fn row(settings: &Value) -> Result<Self, AppError> {
        Ok(Self::Row(parse_text(config_text(settings), "origin")?))
    }
}

/// 把编辑器里的完整配置拆开：关键字段、独有字段换进行（行里其余内容原样保留），其余部分
/// 和 `base` 比，得出用户改过的全局设置。行本身解析不了在这里报错；没填 Key 不拦（见
/// [`project_for_save`]）。
///
/// 从 live 带进来的独有字段不归这个供应商：用户没动就不收进行、也不写（否则切走时会把
/// 用户自己的设置删掉，打开编辑器之后客户端改的值也会被盖回去），用户删了就从 live 删。
/// 哪些是从 live 带进来的见 [`Origin`]。
pub(crate) fn plan_save(
    stored_row: Option<&Value>,
    edited: &Value,
    base: &Value,
    origin: &Origin,
    official: bool,
    proxy_injected_oauth: bool,
    on_conflict: ConflictPolicy,
) -> Result<CodexEditorPlan, AppError> {
    let edited_doc = parse_text(config_text(edited), "edited")?;
    let base_doc = parse_text(config_text(base), "base")?;
    let mut projection = project_for_save(&RowInput {
        settings: edited,
        official,
        proxy_injected_oauth,
    })?;

    let rendered = |doc: &DocumentMut, key: &str| doc.get(key).map(render);
    let from_live: Vec<Entry> = exclusive_entries(&base_doc)
        .into_iter()
        .filter(|entry| match origin {
            Origin::Row(row) => row.get(&entry.path[0]).is_none(),
            Origin::Live(live_exclusive) => live_exclusive
                .iter()
                .any(|live| live.path == entry.path && render(&live.item) == render(&entry.item)),
        })
        .collect();
    projection.exclusive.retain(|(key, _)| {
        !from_live.iter().any(|entry| entry.path[0] == *key)
            || rendered(&edited_doc, key) != rendered(&base_doc, key)
    });
    let removed_from_live = from_live
        .into_iter()
        .filter(|entry| edited_doc.get(&entry.path[0]).is_none());

    // 打开时和保存时选中的路由表都归供应商：用户在编辑器里把 custom 改名成别的表，那张表
    // 连同里面的 Key 不能当成全局设置留在 live 里。
    let routes: Vec<&str> = [selected_route(&base_doc), selected_route(&edited_doc)]
        .into_iter()
        .flatten()
        .collect();
    let mut base_entries = entries(&base_doc, &routes);
    base_entries.extend(removed_from_live);
    Ok(CodexEditorPlan {
        row_settings: store_into_row(stored_row, edited, &projection)?,
        edits: TomlEdits::between(&base_entries, &entries(&edited_doc, &routes), on_conflict),
    })
}

/// 把编辑器里的关键字段、独有字段存回行：行的 `config` 里这两类键换成编辑器的，其余内容
/// 原样保留（降级后旧版会整份使用这些行）；`auth`、模型目录等表单字段取编辑器的。
fn store_into_row(
    stored_row: Option<&Value>,
    edited: &Value,
    projection: &CodexProjection,
) -> Result<Value, AppError> {
    let mut row = edited.clone();
    let stored_text = stored_row.map(config_text).unwrap_or("");
    let mut doc = parse_text(stored_text, "stored")?;

    let stored_route = doc
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::to_string);
    let root = doc.as_table_mut();
    for key in floor::CODEX_FLOOR_TOP
        .iter()
        .chain(floor::CODEX_EXCLUSIVE_TOP.iter())
    {
        root.remove(key);
    }
    for segments in floor::CODEX_FLOOR_NESTED {
        if let Some(table) = root.get_mut(segments[0]).and_then(Item::as_table_like_mut) {
            table.remove(segments[1]);
        }
    }
    if let Some(providers) = root
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
    {
        for id in [Some(ROUTE_ID), stored_route.as_deref()]
            .into_iter()
            .flatten()
        {
            providers.remove(id);
        }
        if providers.is_empty() {
            root.remove("model_providers");
        }
    }

    for (key, value) in projection.top.iter().chain(&projection.exclusive) {
        root.insert(key, Item::Value(value.clone()));
    }
    for (path, value) in &projection.nested {
        let segments: Vec<String> = path.clone();
        insert_at(&mut doc, &segments, Item::Value(value.clone()));
    }
    let key = edited
        .get("auth")
        .and_then(crate::codex_config::extract_codex_auth_api_key);
    match &projection.route {
        Route::Custom { table, .. } => {
            let mut table = table.clone();
            let token = table
                .get("experimental_bearer_token")
                .and_then(Item::as_str)
                .map(str::to_string);
            if token.is_some() && token == key {
                table.remove("experimental_bearer_token");
            }
            doc["model_provider"] = toml_edit::value(ROUTE_ID);
            insert_at(
                &mut doc,
                &["model_providers".to_string(), ROUTE_ID.to_string()],
                Item::Table(table),
            );
        }
        Route::BuiltIn { id, table } => {
            doc["model_provider"] = toml_edit::value(id.as_str());
            if let Some(table) = table {
                insert_at(
                    &mut doc,
                    &["model_providers".to_string(), id.clone()],
                    Item::Table(table.clone()),
                );
            }
        }
        Route::Official | Route::Default => {}
    }
    row["config"] = Value::String(doc.to_string());
    Ok(row)
}

/// 编辑器改动之外，同一次写入里要不要把关键字段也换进 live。
pub(crate) enum KeyFields<'a> {
    /// 只写全局改动。
    None,
    /// 直连模式下编辑当前供应商：`prev` 是编辑前的行，`set_pointer` 为新增第一个供应商。
    Direct {
        prev: Option<&'a Provider>,
        target: &'a Provider,
        set_pointer: bool,
    },
}

/// 把编辑器保存的改动写进 live。没有要写的就什么都不做。
pub(crate) fn write_live(
    db: &Database,
    manager: &Arc<CodexOAuthManager>,
    edits: &TomlEdits,
    key_fields: KeyFields<'_>,
) -> Result<(), AppError> {
    match key_fields {
        KeyFields::Direct {
            prev,
            target,
            set_pointer,
        } => {
            let owner = prev.map_or(Owner::None, Owner::Provider);
            let spec = Target::Direct(Some(target));
            let prepared = codex_direct::prepare(manager, &owner, &spec)?;
            let planned = codex_direct::plan(db, &owner, &spec, &prepared)?;
            codex_direct::run_with_edits(
                db,
                if set_pointer { op::SWITCH } else { op::APPLY },
                planned,
                &prepared,
                PendingTarget::pointer(set_pointer.then(|| target.id.clone())),
                Some(edits),
            )?;
            Ok(())
        }
        KeyFields::None => {
            if edits.is_empty() {
                return Ok(());
            }
            AppWrite::begin(db, app())?.run(
                op::APPLY,
                &[FileChange {
                    file: LiveFile::private(get_codex_config_path()),
                    patch: edits,
                }],
                PendingTarget::default(),
            )?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::patch::LiveWriteError;
    use serde_json::json;
    use std::path::Path;

    fn doc(text: &str) -> DocumentMut {
        text.parse().unwrap()
    }

    fn global_changes(base: &DocumentMut, edited: &DocumentMut) -> TomlEdits {
        TomlEdits::between(
            &entries(base, &[]),
            &entries(edited, &[]),
            ConflictPolicy::Refuse,
        )
    }

    const LIVE: &str = "approval_policy = \"on-request\"\nmodel = \"gpt-a\"\n\n[agents]\ndefault_subagent_model = \"mini\"\nmax_threads = 4\n\n[mcp_servers.fs]\ncommand = \"fs\"\n";

    #[test]
    fn global_changes_skip_key_fields_and_nested_model_names() {
        let base = doc(LIVE);
        let edited = doc(&LIVE
            .replace("gpt-a", "gpt-b")
            .replace("\"mini\"", "\"maxi\"")
            .replace("max_threads = 4", "max_threads = 8")
            .replace("command = \"fs\"", "command = \"fs2\""));
        let changes = global_changes(&base, &edited);
        let paths = changes.paths();
        assert_eq!(paths, vec!["agents.max_threads", "mcp_servers.fs"]);
    }

    #[test]
    fn edits_detect_three_way_conflicts() {
        let base = doc(LIVE);
        let edited = doc(&LIVE.replace("on-request", "never"));
        let edits = global_changes(&base, &edited);
        // 编辑期间别的程序把它改成了第三个值。
        let mut live = doc(&LIVE.replace("on-request", "untrusted"));
        let err = edits
            .apply_to(Path::new("config.toml"), &mut live)
            .expect_err("conflict");
        assert!(matches!(err, LiveWriteError::EditConflict { .. }));

        // 没被别人改过：照写，其余字节不动。
        let mut live = doc(LIVE);
        edits.apply_to(Path::new("config.toml"), &mut live).unwrap();
        assert_eq!(live.to_string(), LIVE.replace("on-request", "never"));
    }

    #[test]
    fn saving_puts_key_fields_into_the_row_and_keeps_the_rest_of_it() {
        let stored = json!({
            "auth": { "OPENAI_API_KEY": "sk-old" },
            "config": "model_provider = \"relay\"\nmodel = \"gpt-a\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://old.example/v1\"\n\n[mcp_servers.legacy]\ncommand = \"x\"\n"
        });
        let edited = json!({
            "auth": { "OPENAI_API_KEY": "sk-new" },
            "config": "approval_policy = \"never\"\nmodel_provider = \"custom\"\nmodel = \"gpt-b\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://new.example/v1\"\n"
        });
        let plan = plan_save(
            Some(&stored),
            &edited,
            &edited,
            &Origin::row(&stored).unwrap(),
            false,
            false,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        let row = &plan.row_settings;
        assert_eq!(row["auth"]["OPENAI_API_KEY"], "sk-new");
        let text = row["config"].as_str().unwrap();
        let parsed: toml::Table = toml::from_str(text).unwrap();
        assert_eq!(parsed["model"].as_str(), Some("gpt-b"));
        assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
        assert_eq!(
            parsed["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://new.example/v1")
        );
        assert!(parsed["model_providers"].get("relay").is_none());
        assert!(
            !text.contains("sk-new"),
            "the key stays in auth, not the config: {text}"
        );
        assert!(
            text.contains("[mcp_servers.legacy]"),
            "the row's other content stays for older versions: {text}"
        );
        assert!(
            !text.contains("approval_policy"),
            "global settings go to live, not the row: {text}"
        );
    }

    /// #8039：顶层 base_url 的旧形态行原样保存，地址不能丢，归一成 custom 表。
    #[test]
    fn saving_a_top_level_base_url_row_keeps_the_address() {
        let stored = json!({
            "auth": { "OPENAI_API_KEY": "sk-relay" },
            "config": "base_url = \"https://relay.example.com/v1\"\nmodel = \"gpt-a\"\nwire_api = \"responses\"\n"
        });
        let plan = plan_save(
            Some(&stored),
            &stored,
            &stored,
            &Origin::row(&stored).unwrap(),
            false,
            false,
            ConflictPolicy::Refuse,
        )
        .unwrap();
        let text = plan.row_settings["config"].as_str().unwrap();
        let parsed: toml::Table = toml::from_str(text).unwrap();
        assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
        assert_eq!(
            parsed["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://relay.example.com/v1")
        );
        assert!(parsed.get("base_url").is_none(), "{text}");
        assert!(parsed.get("wire_api").is_none(), "{text}");
        assert_eq!(parsed["model"].as_str(), Some("gpt-a"));
        assert_eq!(
            crate::codex_config::extract_codex_base_url(text).as_deref(),
            Some("https://relay.example.com/v1"),
            "the proxy still finds the address"
        );
    }
}
