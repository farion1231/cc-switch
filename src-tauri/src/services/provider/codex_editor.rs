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
use super::editor_toml::{self, config_text, insert_at, item_at, render, Entry, TomlEdits};

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

/// 选中的路由表（`model_provider` 指的那张）；选不上（没选 / 表不存在）为 `None`。
fn selected_table(doc: &DocumentMut) -> Option<(&str, &dyn toml_edit::TableLike)> {
    let id = selected_route(doc)?;
    doc.get("model_providers")?
        .as_table_like()?
        .get(id)?
        .as_table_like()
        .map(|table| (id, table))
}

/// 一次编辑器保存里关键字段改动的**类别**，按「证明过会当场生效」的口径划分
///（issue #7948 三审：只豁免证实即时生效的字段，未证实的一律保留提醒）：
/// - `endpoint_or_key`：代理**逐请求**消费的路由表键（`base_url`、`wire_api`、
///   `experimental_bearer_token`，见 `proxy/providers/codex.rs` 的读取点）和行的 Key
///   （`auth`）。直连当前卡当场进 live；代理路由随契约重写；活跃 Stack 成员的请求按
///   最新行直达那家——都即时。
/// - `route_table_other`：路由表里代理不消费的其余键（`stream_max_retries`、表 `name`、
///   `requires_openai_auth` 这类，只有 Codex CLI 自己读）。
/// - `catalog_consumed`：目录生成器**确实消费**的顶层/独有字段：`model`（没配模型目录时
///   它就是发布的模型名，`codex_published_specs`）、`model_context_window` /
///   `model_auto_compact_token_limit`（`RowWindows::of`）。活跃 Stack 成员的这些进合并
///   目录、随契约当场重算——**前提是目录发布**（默认路由行没带自己的
///   `model_catalog_json`，`codex_direct::stack_catalog` 的跳过条件）。
/// - `top_exclusive_other`：其余顶层/独有字段（`review_model`、`model_verbosity`、推理
///   档位这类）——目录生成器不消费，`config.toml` 里只来自路由那家。
/// - `nested`：嵌在用户表里的模型名（`[agents].default_subagent_model`），同上只来自
///   路由那家。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyFieldChanges {
    pub endpoint_or_key: bool,
    pub route_table_other: bool,
    pub catalog_consumed: bool,
    pub top_exclusive_other: bool,
    pub nested: bool,
}

impl KeyFieldChanges {
    pub(crate) fn any(self) -> bool {
        self.endpoint_or_key
            || self.route_table_other
            || self.catalog_consumed
            || self.top_exclusive_other
            || self.nested
    }
}

/// 代理逐请求读取的路由表键（`proxy/providers/codex.rs`：上游地址、协议选择、凭据）。
const ENDPOINT_TABLE_KEYS: &[&str] = &["base_url", "wire_api", "experimental_bearer_token"];
/// 目录生成器消费的顶层/独有字段（`codex_catalog_from_specs_for_row` / `RowWindows::of` /
/// `codex_published_specs`）。
const CATALOG_CONSUMED_KEYS: &[&str] = &[
    "model",
    "model_context_window",
    "model_auto_compact_token_limit",
];

/// 比较打开编辑器时的投影（`base`）和表单保存的内容（`edited`），得出各类关键字的
/// 改动（issue #7948）。`immediate_exclusive`：这次保存里**立即写进 live** 的独有字段
///（从 live 带进来、被用户删掉的那些——删除是全局改动，当场生效）：不算延迟。
pub(crate) fn key_fields_changed(
    base: &Value,
    edited: &Value,
    immediate_exclusive: &[String],
) -> KeyFieldChanges {
    let (Ok(base_doc), Ok(edited_doc)) = (
        config_text(base).parse::<DocumentMut>(),
        config_text(edited).parse::<DocumentMut>(),
    ) else {
        // 解析不了的按全改过算：宁可多提醒一次，不要静默吞掉。
        return KeyFieldChanges {
            endpoint_or_key: true,
            route_table_other: true,
            catalog_consumed: true,
            top_exclusive_other: true,
            nested: true,
        };
    };
    let top = |doc: &DocumentMut, key: &str| doc.get(key).map(render);
    let mut changes = KeyFieldChanges::default();
    let immediate_delete = |key: &str| {
        edited_doc.get(key).is_none()
            && immediate_exclusive.iter().any(|immediate| immediate == key)
    };
    for key in floor::CODEX_FLOOR_TOP
        .iter()
        .chain(floor::CODEX_EXCLUSIVE_TOP.iter())
    {
        if top(&base_doc, key) == top(&edited_doc, key) || immediate_delete(key) {
            continue;
        }
        // 顶层 `model` 只在没有显式模型目录时才进目录（`codex_published_specs` 的早退）；
        // 配了目录的行改 `model` 目录不变。窗口键不受显式目录影响（`RowWindows::of` 总读）。
        if CATALOG_CONSUMED_KEYS.contains(key)
            && (*key != "model"
                || !(crate::codex_config::codex_has_explicit_catalog(base)
                    || crate::codex_config::codex_has_explicit_catalog(edited)))
        {
            changes.catalog_consumed = true;
        } else {
            changes.top_exclusive_other = true;
        }
    }
    changes.nested |= floor::CODEX_FLOOR_NESTED.iter().any(|segments| {
        let path: Vec<String> = segments.iter().map(|segment| segment.to_string()).collect();
        item_at(&base_doc, &path).map(render) != item_at(&edited_doc, &path).map(render)
    });
    // 选中的路由表按**键**比较：代理消费的键（地址 / 协议 / 凭据）即时，其余键（只有
    // Codex CLI 读）不即时。选了别的表、或哪边选不上表：保守按两类都改了算。
    let selected = selected_table;
    match (selected(&base_doc), selected(&edited_doc)) {
        (Some((base_id, base_table)), Some((edited_id, edited_table))) => {
            let mut keys: Vec<&str> = base_table.iter().map(|(key, _)| key).collect();
            for key in edited_table.iter().map(|(key, _)| key) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            if base_id != edited_id {
                changes.route_table_other = true;
            }
            for key in keys {
                let differs = base_table.get(key).map(render) != edited_table.get(key).map(render);
                if !differs {
                    continue;
                }
                if ENDPOINT_TABLE_KEYS.contains(&key) {
                    changes.endpoint_or_key = true;
                } else {
                    changes.route_table_other = true;
                }
            }
        }
        // 一边有一边没有（删掉整张表 / 新加表）：保守按两类都改了算。
        _ => {
            changes.endpoint_or_key = true;
            changes.route_table_other = true;
        }
    }
    // 两边都没有选中的路由表（原生官方配置常态）：不算路由表变化——selector 的增删由
    // 顶层比较负责，别把官方卡的普通保存误报成关键字段改动（issue #7948 四审 Minor 1）。
    if selected(&base_doc).is_none() && selected(&edited_doc).is_none() {
        changes.endpoint_or_key &= false;
        changes.route_table_other &= false;
    }
    // 行的 Key（API Key 输入框）：在 `auth` 里，不在 TOML 里。
    let row_key = |settings: &Value| {
        settings
            .get("auth")
            .and_then(crate::codex_config::extract_codex_auth_api_key)
    };
    changes.endpoint_or_key |= row_key(base) != row_key(edited);
    // 模型目录列表（`settings.modelCatalog`，不在 TOML 里）：目录条目进合并目录 / 契约，
    // 按目录消费类算——非当前卡片改列表同样延迟生效，要提醒（issue #7948 四审自查）。
    // 比较用消费端解析后的规范化条目：表单保存会规范化学段名，raw JSON 相同不作数
    //（issue #7948 五审 2）。
    if !crate::codex_config::codex_catalog_specs_equal(base, edited) {
        changes.catalog_consumed = true;
    }
    changes
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
/// 保存只存进行、没写进 live 的关键字段改动（代理模式、或直连下编辑的不是当前供应
/// 商）在返回里带的警告码：前端按它提示用户改动何时生效，别当成保存失败（issue #7948）。
pub(crate) const KEYFIELDS_PENDING_SWITCH: &str = "codex_keyfields_pending_switch";

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
    /// 有只存进行、切换 / 重写契约时才进 live 的关键字段改动（issue #7948 的提醒用）。
    pub key_field_changes: KeyFieldChanges,
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
    let immediate_exclusive: Vec<String> = from_live
        .iter()
        .filter(|entry| edited_doc.get(&entry.path[0]).is_none())
        .map(|entry| entry.path[0].clone())
        .collect();
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
        key_field_changes: key_fields_changed(base, edited, &immediate_exclusive),
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

    const VIEW: &str = "approval_policy = \"on-request\"\nmodel_provider = \"custom\"\nmodel = \"gpt-a\"\n\n[model_providers.custom]\nname = \"a\"\nbase_url = \"https://a.example/v1\"\nwire_api = \"responses\"\n";

    fn settings(config: &str, key: &str) -> Value {
        json!({ "auth": { "OPENAI_API_KEY": key }, "config": config })
    }
    #[test]
    fn key_fields_changed_spots_key_edits_and_ignores_global_only_edits() {
        let base = settings(VIEW, "sk-a");
        let changed = |base: &Value, edited: &Value, immediate: &[String]| {
            key_fields_changed(base, edited, immediate)
        };
        assert!(
            changed(
                &base,
                &settings(
                    &VIEW.replace("https://a.example/v1", "https://a2.example/v1"),
                    "sk-a"
                ),
                &[]
            )
            .endpoint_or_key,
            "route table (base_url) edit"
        );
        assert!(
            changed(
                &base,
                &settings(&VIEW.replace("\"gpt-a\"", "\"gpt-b\""), "sk-a"),
                &[]
            )
            .catalog_consumed,
            "top-level model edit (consumed by the catalog generator)"
        );
        assert!(
            changed(
                &base,
                &settings(
                    &VIEW
                        .replace("model_provider = \"custom\"", "model_provider = \"relay\"")
                        .replace("[model_providers.custom]", "[model_providers.relay]"),
                    "sk-a"
                ),
                &[]
            )
            .route_table_other
                && !changed(
                    &base,
                    &settings(
                        &VIEW
                            .replace("model_provider = \"custom\"", "model_provider = \"relay\"")
                            .replace("[model_providers.custom]", "[model_providers.relay]"),
                        "sk-a",
                    ),
                    &[]
                )
                .endpoint_or_key,
            "a pure table rename does not touch the request path"
        );
        assert!(
            changed(&base, &settings(VIEW, "sk-b"), &[]).endpoint_or_key,
            "api key edit"
        );
        assert!(
            !changed(
                &base,
                &settings(&VIEW.replace("on-request", "never"), "sk-a"),
                &[]
            )
            .any(),
            "global-only edit"
        );
        assert!(
            !changed(&base, &settings(VIEW, "sk-a"), &[]).any(),
            "no edit"
        );
    }

    #[test]
    fn key_fields_changed_skips_exclusive_fields_deleted_from_live() {
        // live 带进来的独有字段（编辑器显示里有、行里没有）：用户删掉它是立即生效的
        // 全局改动（removed_from_live），不算延迟生效的关键字段（issue #7948 外审 Minor 4）。
        // 注意 `model_verbosity` 要放在 `[model_providers.custom]` 之前才是顶层键。
        let with_verbosity = VIEW.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
        );
        let base = settings(&with_verbosity, "sk-a");
        let deleted = settings(VIEW, "sk-a");
        assert!(
            !key_fields_changed(&base, &deleted, &["model_verbosity".to_string()]).any(),
            "deleting a live-brought exclusive field applies immediately"
        );
        // 修改（不是删除）仍算关键字段改动：值进供应商行，切换时才生效。
        let modified =
            with_verbosity.replace("model_verbosity = \"high\"", "model_verbosity = \"low\"");
        assert!(
            key_fields_changed(&base, &settings(&modified, "sk-a"), &[]).top_exclusive_other,
            "modifying an exclusive field goes to the row"
        );
    }

    #[test]
    fn key_fields_changed_reports_nested_model_names_as_their_own_category() {
        // 嵌套模型名（[agents].default_subagent_model 这类）单列一类：活跃 Stack 成员
        // 改它仍延迟生效（config.toml 里只来自路由那家的投影），二审 Minor 1+2。
        let with_nested = VIEW.replace(
            "\n[model_providers.custom]",
            "\n[agents]\ndefault_subagent_model = \"gpt-a-mini\"\n\n[model_providers.custom]",
        );
        let base = settings(&with_nested, "sk-a");
        let edited = settings(&with_nested.replace("gpt-a-mini", "gpt-b-mini"), "sk-a");
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(changes.nested, "{changes:?}");
        assert!(
            !changes.endpoint_or_key
                && !changes.route_table_other
                && !changes.catalog_consumed
                && !changes.top_exclusive_other,
            "{changes:?}"
        );
    }

    #[test]
    fn key_fields_changed_splits_endpoint_keys_from_unconsumed_table_keys() {
        // 三审反例 3：`stream_max_retries` 只有 Codex CLI 读，代理不消费——不是即时字段。
        let with_retries = VIEW.replace(
            "wire_api = \"responses\"",
            "wire_api = \"responses\"\nstream_max_retries = 5",
        );
        let base = settings(&with_retries, "sk-a");
        let edited = settings(
            &with_retries.replace("stream_max_retries = 5", "stream_max_retries = 9"),
            "sk-a",
        );
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.route_table_other && !changes.endpoint_or_key,
            "{changes:?}"
        );
        // 改 wire_api（协议选择，代理逐请求消费）是即时字段。
        let edited = settings(
            &with_retries.replace("wire_api = \"responses\"", "wire_api = \"chat\""),
            "sk-a",
        );
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.endpoint_or_key && !changes.route_table_other,
            "{changes:?}"
        );
    }

    #[test]
    fn key_fields_changed_splits_catalog_consumed_fields_from_the_rest() {
        // 三审反例 1：`review_model` / `model_verbosity` 目录生成器不消费——不是即时字段。
        let with_review = VIEW.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nreview_model = \"gpt-a-review\"\n",
        );
        let base = settings(&with_review, "sk-a");
        let edited = settings(&with_review.replace("gpt-a-review", "gpt-b-review"), "sk-a");
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.top_exclusive_other && !changes.catalog_consumed,
            "{changes:?}"
        );
        // 窗口键目录生成器消费（RowWindows::of）。
        let with_window = VIEW.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_context_window = 200000\n",
        );
        let base = settings(&with_window, "sk-a");
        let edited = settings(&with_window.replace("200000", "1000000"), "sk-a");
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.catalog_consumed && !changes.top_exclusive_other,
            "{changes:?}"
        );
    }
    /// 原生官方配置（没有 `model_provider`、没有自定义表）的保存不算路由表变化：
    /// 原样保存、只改全局设置都不该有关键字段改动（issue #7948 四审 Minor 1）。
    #[test]
    fn key_fields_changed_leaves_tableless_native_configs_alone() {
        const NATIVE: &str =
            "approval_policy = \"on-request\"\nmodel = \"gpt-official\"\nmodel_reasoning_effort = \"high\"\n";
        let base = settings(NATIVE, "sk-a");
        assert!(
            !key_fields_changed(&base, &settings(NATIVE, "sk-a"), &[]).any(),
            "no edit"
        );
        assert!(
            !key_fields_changed(
                &base,
                &settings(&NATIVE.replace("on-request", "never"), "sk-a"),
                &[]
            )
            .any(),
            "global-only edit"
        );
        // 官方卡改 Key 仍是关键字段改动（进行、延迟生效）。
        assert!(
            key_fields_changed(&base, &settings(NATIVE, "sk-b"), &[]).endpoint_or_key,
            "api key edit"
        );
    }

    /// 行里配了显式模型目录时，目录生成器不读顶层 `model`（`codex_published_specs` 早退）：
    /// 只改 `model` 目录不变，不算目录消费；窗口键仍被 `RowWindows::of` 消费
    ///（issue #7948 四审 Minor 3）。
    #[test]
    fn key_fields_changed_demotes_model_with_an_explicit_catalog() {
        let explicit = |config: &str| {
            let mut value = settings(config, "sk-a");
            value["modelCatalog"] = json!({ "models": [{ "model": "relay-x" }] });
            value
        };
        let base = explicit(VIEW);
        let edited = explicit(&VIEW.replace("\"gpt-a\"", "\"gpt-b\""));
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.top_exclusive_other && !changes.catalog_consumed,
            "model is not consumed with an explicit catalog: {changes:?}"
        );
        let with_window = VIEW.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_context_window = 200000\n",
        );
        let base = explicit(&with_window);
        let edited = explicit(&with_window.replace("200000", "1000000"));
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.catalog_consumed && !changes.top_exclusive_other,
            "window keys are still consumed: {changes:?}"
        );
    }
    /// 模型目录列表（`modelCatalog.models`，表单字段不在 TOML 里）也算目录消费类：
    /// 非当前卡片改列表延迟生效要提醒；活跃成员改列表契约当场重算（issue #7948 四审自查）。
    #[test]
    fn key_fields_changed_counts_explicit_catalog_list_edits() {
        let base = settings(VIEW, "sk-a");
        let mut edited = settings(VIEW, "sk-a");
        edited["modelCatalog"] = json!({ "models": [{ "model": "relay-x" }] });
        let changes = key_fields_changed(&base, &edited, &[]);
        assert!(
            changes.catalog_consumed && !changes.endpoint_or_key,
            "{changes:?}"
        );
    }
    /// 目录列表比较按消费端解析后的规范化条目：snake_case 与 camelCase 同义不算改动，
    /// 实质不同才算（issue #7948 五审 2）。
    #[test]
    fn key_fields_changed_compares_catalog_lists_by_parsed_specs() {
        let with_catalog = |raw: &str| {
            let mut value = settings(VIEW, "sk-a");
            value["modelCatalog"] = serde_json::from_str::<Value>(raw).expect("catalog json");
            value
        };
        let snake = with_catalog(r#"{"models":[{"model":"relay-x","reasoning_levels":["low"]}]}"#);
        let camel = with_catalog(r#"{"models":[{"model":"relay-x","reasoningLevels":["low"]}]}"#);
        assert!(
            !key_fields_changed(&snake, &camel, &[]).any(),
            "semantically identical catalogs are not a change"
        );
        let other = with_catalog(r#"{"models":[{"model":"relay-y"}]}"#);
        assert!(
            key_fields_changed(&snake, &other, &[]).catalog_consumed,
            "a real list edit is a catalog change"
        );
    }
}
