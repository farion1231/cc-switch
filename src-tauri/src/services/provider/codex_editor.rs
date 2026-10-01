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

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, Value as TomlValue};

use crate::app_config::AppType;
use crate::codex_config::get_codex_config_path;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{read_current, sha256_hex, LiveFile};
use crate::live::floor;
use crate::live::patch::toml::parse;
use crate::live::project::codex::{
    profile_references, CodexProjection, Route, RowInput, OFFICIAL_PROXY_ROUTE_ID, ROUTE_ID,
};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, ModeState, PendingTarget};
use crate::provider::Provider;
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

/// `[model_providers.cc-switch-official]`：旧官方代理路由定义，原样。
fn legacy_route_table(doc: &DocumentMut) -> Option<Item> {
    doc.get("model_providers")
        .and_then(Item::as_table_like)
        .and_then(|providers| providers.get(OFFICIAL_PROXY_ROUTE_ID))
        .cloned()
}

/// 规范指纹：忽略空白和注释的渲染取哈希（同一张表内容相同才相同）。删除基准只带这个，
/// 不带表的实际内容。
fn fingerprint(item: &Item) -> String {
    sha256_hex(render(item).as_bytes())
}

fn render_option(item: Option<&Item>) -> Option<String> {
    item.map(render)
}

/// 打开 Codex 编辑器时旧官方代理路由定义的状态，保存时原样带回：用户在编辑器里删掉
/// 这张表时，删除基准用它，而不是预览里那张规范镜像——预览会把用户打开前的改名、追加
/// 键、过期地址都规范化掉，直接拿它比较会把真实存在的旧表误判成被外部改过。
///
/// 只装判断删除需要的东西：选择器、存在性、规范指纹和 profile 引用状态。指纹是哈希，
/// 表的内容（可能含凭据字段）不出现在快照里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEditorSnapshot {
    /// 打开时顶层 `model_provider` 选的路由。
    #[serde(default)]
    pub selector: Option<String>,
    /// 打开时旧表是否存在。
    #[serde(default)]
    pub legacy_route: bool,
    /// 打开时旧表内容的规范指纹；表不存在时没有。
    #[serde(default)]
    pub legacy_fingerprint: Option<String>,
    /// 打开时有没有 profile 引用旧表。
    #[serde(default)]
    pub profile_referenced: bool,
}

impl CodexEditorSnapshot {
    /// 读一份 live 配置里的旧表状态（编辑器显示的是投影后的样子，快照要的是盘上原样）。
    pub(crate) fn of(doc: &DocumentMut) -> Self {
        let legacy = legacy_route_table(doc);
        Self {
            selector: selected_route(doc).map(str::to_string),
            legacy_route: legacy.is_some(),
            legacy_fingerprint: legacy.as_ref().map(fingerprint),
            profile_referenced: profile_references(doc.as_table(), OFFICIAL_PROXY_ROUTE_ID),
        }
    }

    /// 能当删除基准吗：快照自己一致，而且打开时这张表是休眠的（顶层没选它、也没有
    /// profile 引用它）。打开时就活着的表，删除按当前真实状态拒绝，不拿旧快照当基准。
    fn is_baseline(&self) -> bool {
        self.legacy_route == self.legacy_fingerprint.is_some()
            && self.selector.as_deref() != Some(OFFICIAL_PROXY_ROUTE_ID)
            && !self.profile_referenced
    }

    /// 打开后这张表被改过（或被删掉、被新建）的证据：删除前的值用指纹占位，盘上现在的
    /// 渲染一定不等于它，三方比较因此报冲突。
    fn conflict_baseline(&self) -> Item {
        Item::Value(TomlValue::from(
            self.legacy_fingerprint.as_deref().unwrap_or_default(),
        ))
    }
}

/// 保存时从真实 live 读到的路由状态：旧表现在是不是还活着，只认它（编辑器预览里的
/// `model_provider` 不算数）。调用方持着切换锁、补完 pending 之后读，读到的就是这次
/// 写入要改的那份文件。
pub(crate) struct CodexLiveRouteContext {
    /// 旧表当前的内容（原样），不存在时没有。
    legacy: Option<Item>,
    /// 旧表当前的规范指纹。
    fingerprint: Option<String>,
    /// 当前顶层 `model_provider` 选的路由。
    selector: Option<String>,
    /// 当前有没有 profile 引用旧表。
    profile_referenced: bool,
    /// 代理契约接管 live，而且契约选的就是旧路由。
    contract_uses_legacy_route: bool,
}

impl CodexLiveRouteContext {
    pub(crate) fn read(mode: &ModeState) -> Result<Self, AppError> {
        let path = get_codex_config_path();
        let pre = read_current(&path)?;
        let doc = parse(&path, pre.as_deref())?;
        Ok(Self::of(&doc, mode))
    }

    fn of(doc: &DocumentMut, mode: &ModeState) -> Self {
        let legacy = legacy_route_table(doc);
        let selector = selected_route(doc).map(str::to_string);
        Self {
            fingerprint: legacy.as_ref().map(fingerprint),
            legacy,
            contract_uses_legacy_route: mode.attached
                && mode.contract.is_some()
                && selector.as_deref() == Some(OFFICIAL_PROXY_ROUTE_ID),
            selector,
            profile_referenced: profile_references(doc.as_table(), OFFICIAL_PROXY_ROUTE_ID),
        }
    }

    /// 旧表现在被真实状态占用吗：代理契约正用它、选择器指着它、profile 引用它（包括
    /// 这次要写进去的 profile）。被占用的表不允许从编辑器删除或改写——那会留下指向
    /// 缺失定义的选路。返回原因，给报错文案用。
    fn in_use(&self, edited: &DocumentMut) -> Option<(&'static str, &'static str)> {
        if self.contract_uses_legacy_route {
            return Some(("代理契约正在使用它", "the proxy contract uses it"));
        }
        if self.selector.as_deref() == Some(OFFICIAL_PROXY_ROUTE_ID) {
            return Some(("当前选择器指向它", "the current selector points at it"));
        }
        if self.profile_referenced || profile_references(edited.as_table(), OFFICIAL_PROXY_ROUTE_ID)
        {
            return Some(("profile 引用了它", "a profile references it"));
        }
        None
    }
}

/// 保存时对旧官方代理路由的判定依据。
pub(crate) struct LegacyRouteSave<'a> {
    /// 打开编辑器时带回的快照；旧调用方没有（安全降级，见 [`plan_save`]）。
    pub snapshot: Option<&'a CodexEditorSnapshot>,
    /// 真实 live 的当前状态。
    pub live: &'a CodexLiveRouteContext,
}

/// 活动路由保护：拒绝删除或改写旧表，保持文件、数据库和 pending 不变。
fn legacy_route_in_use(reason_zh: &str, reason_en: &str) -> AppError {
    AppError::localized(
        "provider.codex.editor.official_route_in_use",
        format!(
            "旧官方代理路由 model_providers.{OFFICIAL_PROXY_ROUTE_ID} 正在被使用（{reason_zh}），不能从编辑器删除或改写，否则选路会指向不存在的定义。请先切换路由、退出代理模式或改掉引用它的 profile 再试；本次没有保存任何内容"
        ),
        format!(
            "The legacy official proxy route model_providers.{OFFICIAL_PROXY_ROUTE_ID} is in use ({reason_en}), so it cannot be deleted or rewritten from the editor: the selection would point at a definition that no longer exists. Switch routes, leave proxy mode, or remove the profile that references it and try again. Nothing was saved"
        ),
    )
}

/// 编辑器显示的内容。`settings_config` 是这个供应商的行（新增时是空对象）。
pub fn view(
    state: &AppState,
    settings_config: &Value,
    category: Option<&str>,
) -> Result<EditorView, AppError> {
    let path = get_codex_config_path();
    let pre = read_current(&path)?;
    let mut doc = parse(&path, pre.as_deref())?;
    // 快照说的是盘上真实的旧表，不是下面投影后的显示：删除基准要按真实内容算。
    let snapshot = CodexEditorSnapshot::of(&doc);

    let mut provider =
        Provider::with_id(String::new(), String::new(), settings_config.clone(), None);
    provider.category = category.map(str::to_string);
    let live_owner = LiveOwner::read(state)?;
    let planned = codex_direct::plan(
        &state.db,
        &live_owner.owner(),
        &Target::Direct(Some(&provider)),
        &Prepared::default(),
    )?;
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
        if injected.is_some() && injected == row_key {
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
        codex: Some(snapshot),
    })
}

/// 行里保存着、但不随切换生效的全局设置。
fn inactive_fields(row_text: &str, display: &DocumentMut) -> Vec<InactiveField> {
    let Ok(row) = row_text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    editor_toml::inactive_fields(entries(&row, selected_route(&row).as_slice()), display)
}

/// 一次编辑器保存：存进行的内容，和要写进 live 的全局改动。
#[derive(Debug)]
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
/// 和 `base` 比，得出用户改过的全局设置。行有问题（会把官方登录发给第三方等）在这里报错。
///
/// 从 live 带进来的独有字段不归这个供应商：用户没动就不收进行、也不写（否则切走时会把
/// 用户自己的设置删掉，打开编辑器之后客户端改的值也会被盖回去），用户删了就从 live 删。
/// 哪些是从 live 带进来的见 [`Origin`]。
///
/// 旧官方代理路由定义单独判：删除或改写它要求真实 live 确认它休眠、没被 profile 引用，
/// 而且打开之后没被别人改过（见 [`LegacyRouteSave`]）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_save(
    stored_row: Option<&Value>,
    edited: &Value,
    base: &Value,
    origin: &Origin,
    official: bool,
    proxy_injected_oauth: bool,
    on_conflict: ConflictPolicy,
    legacy: LegacyRouteSave<'_>,
) -> Result<CodexEditorPlan, AppError> {
    let edited_doc = parse_text(config_text(edited), "edited")?;
    let base_doc = parse_text(config_text(base), "base")?;
    let mut projection = CodexProjection::of(&RowInput {
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
    let mut edited_entries = entries(&edited_doc, &routes);
    // 旧官方代理路由表：不是本行的路由表时，也不当普通全局设置比。用户明确删除或改写
    // 它要按真实 live 和打开时的快照判定；预览里那张规范镜像不能当比较基准——用户打开
    // 前的改名、追加键、过期地址都会被它规范化掉，拿它比较会把删除误判成外部修改。
    if !routes.contains(&OFFICIAL_PROXY_ROUTE_ID) {
        let path = vec![
            "model_providers".to_string(),
            OFFICIAL_PROXY_ROUTE_ID.to_string(),
        ];
        let base_item = legacy_route_table(&base_doc);
        let edited_item = legacy_route_table(&edited_doc);
        if render_option(base_item.as_ref()) != render_option(edited_item.as_ref()) {
            // 活动路由优先于任何删除策略：真实选择器、profile 引用或代理契约还占着这张
            // 表就什么都不写，也不能用 KeepMine 绕过。
            if let Some((reason_zh, reason_en)) = legacy.live.in_use(&edited_doc) {
                return Err(legacy_route_in_use(reason_zh, reason_en));
            }
            match legacy.snapshot.filter(|snapshot| snapshot.is_baseline()) {
                // 打开之后这张表没被改过：盘上的内容就是打开时那份（可能和预览里的规范
                // 镜像不同），拿它当删除/改写前的值才比得中。
                Some(snapshot) if snapshot.legacy_fingerprint == legacy.live.fingerprint => {
                    if let Some(item) = legacy.live.legacy.clone() {
                        base_entries.push(Entry {
                            path: path.clone(),
                            item,
                        });
                    }
                    if let Some(item) = edited_item {
                        edited_entries.push(Entry { path, item });
                    }
                }
                // 打开之后被别的程序改过：照常按三方比较报冲突。删除前的值用快照的指纹
                // 占位——盘上现在的值一定不等于它：Refuse 报冲突、KeepMine 照删照写、
                // KeepTheirs 保留外部内容。
                Some(snapshot) => {
                    base_entries.push(Entry {
                        path: path.clone(),
                        item: snapshot.conflict_baseline(),
                    });
                    if let Some(item) = edited_item {
                        edited_entries.push(Entry { path, item });
                    }
                }
                // 没有可用的快照（旧调用方，或打开时这张表本来就不是休眠的）：安全降级，
                // 不动这张表，其余编辑照常保存。
                None => {}
            }
        }
    }
    Ok(CodexEditorPlan {
        row_settings: store_into_row(stored_row, edited, &projection)?,
        edits: TomlEdits::between(&base_entries, &edited_entries, on_conflict),
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
    use crate::mode::state::{Contract, Mode};
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

    /// 直连（没有代理契约）。
    fn direct() -> ModeState {
        ModeState::default()
    }

    /// 代理模式已经接管 live（契约在盘上生效）。
    fn attached() -> ModeState {
        ModeState {
            mode: Some(Mode::Proxy),
            attached: true,
            proxy_route: Some("official".to_string()),
            contract: Some(Contract {
                version: 1,
                key: "contract".to_string(),
                exclusive: Map::new(),
            }),
        }
    }

    fn context(live: &str, mode: &ModeState) -> CodexLiveRouteContext {
        CodexLiveRouteContext::of(&doc(live), mode)
    }

    /// 盘上的旧表：用户打开编辑器前改过名（`OpenAI-legacy`）、加过键（`model_verbosity`
    /// 之外的一个自定义键）、地址也过期了。编辑器预览里那张规范镜像和它不一样。
    const LIVE_WITH_LEGACY: &str = r#"model_provider = "custom"
model = "gpt-a"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"

[model_providers.cc-switch-official]
name = "OpenAI-legacy"
base_url = "http://127.0.0.1:9999/v1"
wire_api = "responses"
requires_openai_auth = true
http_headers = { "x-legacy" = "1" }
"#;

    /// 编辑器预览里那张规范镜像（投影把旧表规范化后的样子），选择器是共享槽。
    const PREVIEW_WITH_LEGACY: &str = r#"model_provider = "custom"
model = "gpt-a"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"

[model_providers.cc-switch-official]
name = "OpenAI"
requires_openai_auth = true
supports_websockets = false
wire_api = "responses"
base_url = "http://127.0.0.1:15721/v1"
"#;

    fn row(config: &str) -> Value {
        json!({ "auth": {}, "config": config })
    }

    /// 编辑结果删掉了旧表、留着本行的 custom 表（投影要求选了 custom 就得有这张表）。
    const EDITED_WITHOUT_LEGACY: &str = "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n";

    const EDITED_WITHOUT_LEGACY_WITH_GLOBAL_EDIT: &str = "approval_policy = \"never\"\nmodel_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n";

    /// 选路还在旧表形态上的行要带一把钥匙，投影才不报官方登录回退。
    fn row_with_key(config: &str) -> Value {
        json!({ "auth": { "OPENAI_API_KEY": "sk-row" }, "config": config })
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
        let live = context("", &direct());
        let plan = plan_save(
            Some(&stored),
            &edited,
            &edited,
            &Origin::row(&stored).unwrap(),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: None,
                live: &live,
            },
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

    /// 用户在编辑器里删掉休眠的旧表：删除基准是打开时的真实快照，不是预览里的规范镜像，
    /// 所以用户打开前的改名、追加键、过期地址不会让删除报假冲突。
    #[test]
    fn dormant_route_delete_uses_open_snapshot() {
        let live = context(LIVE_WITH_LEGACY, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        // 编辑器显示的是规范镜像，删掉表的编辑结果里没有旧表。
        let base = row(PREVIEW_WITH_LEGACY);
        let edited = row(
            "model_provider = \"custom\"\nmodel = \"gpt-a\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n",
        );
        let plan = plan_save(
            None,
            &edited,
            &base,
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();

        let mut disk = doc(LIVE_WITH_LEGACY);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .expect("the drifted table is not an external change");
        assert!(disk["model_providers"]
            .as_table()
            .unwrap()
            .get(OFFICIAL_PROXY_ROUTE_ID)
            .is_none());
        // 用户自己的东西原样留着。
        assert_eq!(
            disk["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://relay.example/v1")
        );
    }

    /// 打开编辑器之后旧表被别的程序改过：默认策略报真实冲突，盘上保持外部内容；
    /// KeepMine 照原语义删除，KeepTheirs 保留外部内容。
    #[test]
    fn external_change_after_editor_open_still_conflicts() {
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let externally_changed = LIVE_WITH_LEGACY.replace("x-legacy", "x-changed");
        let live = context(&externally_changed, &direct());
        let base = row(PREVIEW_WITH_LEGACY);
        let edited = row(
            "model_provider = \"custom\"\nmodel = \"gpt-a\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n",
        );
        let save = |policy: ConflictPolicy| {
            plan_save(
                None,
                &edited,
                &base,
                &Origin::Live(Vec::new()),
                false,
                false,
                policy,
                LegacyRouteSave {
                    snapshot: Some(&snapshot),
                    live: &live,
                },
            )
        };

        let plan = save(ConflictPolicy::Refuse).unwrap();
        let mut disk = doc(&externally_changed);
        let err = plan
            .edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .expect_err("external change conflicts");
        match err {
            LiveWriteError::EditConflict { keys, .. } => assert_eq!(
                keys,
                vec![format!("model_providers.{OFFICIAL_PROXY_ROUTE_ID}")]
            ),
            other => panic!("unexpected: {other}"),
        }
        assert_eq!(disk.to_string(), externally_changed);

        let plan = save(ConflictPolicy::KeepMine).unwrap();
        let mut disk = doc(&externally_changed);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert!(disk["model_providers"]
            .as_table()
            .unwrap()
            .get(OFFICIAL_PROXY_ROUTE_ID)
            .is_none());

        let plan = save(ConflictPolicy::KeepTheirs).unwrap();
        let mut disk = doc(&externally_changed);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert_eq!(disk.to_string(), externally_changed);
    }

    /// 当前选择器还指着旧表（打开之后被改回旧路由也一样）：删除被拒绝，两个策略都盖不过
    /// 活动路由保护，磁盘保持原样。
    #[test]
    fn active_official_route_cannot_be_deleted_from_editor() {
        let active = LIVE_WITH_LEGACY.replace(
            "model_provider = \"custom\"",
            &format!("model_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\""),
        );
        let live = context(&active, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let base = row(PREVIEW_WITH_LEGACY);
        let edited = row(EDITED_WITHOUT_LEGACY);
        for policy in [
            ConflictPolicy::Refuse,
            ConflictPolicy::KeepMine,
            ConflictPolicy::KeepTheirs,
        ] {
            let err = plan_save(
                None,
                &edited,
                &base,
                &Origin::Live(Vec::new()),
                false,
                false,
                policy,
                LegacyRouteSave {
                    snapshot: Some(&snapshot),
                    live: &live,
                },
            )
            .expect_err("active route is protected");
            assert!(
                matches!(
                    err,
                    AppError::Localized {
                        key: "provider.codex.editor.official_route_in_use",
                        ..
                    }
                ),
                "{err}"
            );
        }
        assert_eq!(doc(&active).to_string(), active);
    }

    /// 代理契约接管 live 且契约选的就是旧路由（统一历史关）：同样受保护。
    #[test]
    fn route_used_by_the_proxy_contract_cannot_be_deleted() {
        // 契约在盘上写的选择器就是旧路由。
        let attached_live = LIVE_WITH_LEGACY.replace(
            "model_provider = \"custom\"",
            &format!("model_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\""),
        );
        let live = context(&attached_live, &attached());
        assert!(live.contract_uses_legacy_route);
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let err = plan_save(
            None,
            &row(EDITED_WITHOUT_LEGACY),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .expect_err("contract holds the route");
        assert!(
            matches!(
                err,
                AppError::Localized {
                    key: "provider.codex.editor.official_route_in_use",
                    ..
                }
            ),
            "{err}"
        );
    }

    /// 有 profile 引用旧表（这次要写的 profile 也算）：删除被拒绝，profile 配置不动。
    #[test]
    fn profile_referenced_route_cannot_be_deleted() {
        let referenced = format!(
            "{LIVE_WITH_LEGACY}\n[profiles.work]\nmodel_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\"\n"
        );
        let live = context(&referenced, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let err = plan_save(
            None,
            &row(EDITED_WITHOUT_LEGACY),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .expect_err("a profile holds the route");
        assert!(
            matches!(
                err,
                AppError::Localized {
                    key: "provider.codex.editor.official_route_in_use",
                    ..
                }
            ),
            "{err}"
        );

        // 这次要写进去的 profile 引用了它（删表的同时把引用也写进来），同样拒绝。
        let live = context(LIVE_WITH_LEGACY, &direct());
        let profile = format!(
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n\n[profiles.work]\nmodel_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\"\n"
        );
        let err = plan_save(
            None,
            &row(&profile),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .expect_err("the edited profile holds the route");
        assert!(
            matches!(
                err,
                AppError::Localized {
                    key: "provider.codex.editor.official_route_in_use",
                    ..
                }
            ),
            "{err}"
        );
    }

    /// 休眠旧表被 profile 引用时本来就不该删：这里断言「不删」即可（编辑其它字段照常）。
    #[test]
    fn a_save_that_leaves_the_legacy_official_proxy_route_alone_emits_no_edit_for_it() {
        // 休眠的旧路由定义不是本行的路由：编辑器不收进行、也不当全局设置改写，
        // 用户没动它就不该出现在三方比较里。
        let live = context(LIVE_WITH_LEGACY, &direct());
        let twin = LIVE_WITH_LEGACY
            .split_once("[model_providers.cc-switch-official]")
            .map(|(_, table)| format!("\n[model_providers.cc-switch-official]{table}"))
            .unwrap();
        let base = row(&format!(
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Relay\"\n{twin}"
        ));
        let edited = row(&format!(
            "approval_policy = \"never\"\nmodel_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Relay\"\n{twin}"
        ));
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let plan = plan_save(
            None,
            &edited,
            &base,
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();

        assert!(plan
            .edits
            .paths()
            .iter()
            .all(|path| !path.contains(OFFICIAL_PROXY_ROUTE_ID)));
        assert_eq!(plan.edits.paths(), vec!["approval_policy".to_string()]);
    }

    /// 没有快照的旧调用方：安全降级，不删旧表，其余编辑照常。
    #[test]
    fn dormant_route_delete_is_dropped_without_a_snapshot() {
        let live = context(LIVE_WITH_LEGACY, &direct());
        let plan = plan_save(
            None,
            &row(EDITED_WITHOUT_LEGACY_WITH_GLOBAL_EDIT),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: None,
                live: &live,
            },
        )
        .unwrap();
        assert_eq!(plan.edits.paths(), vec!["approval_policy".to_string()]);

        let mut disk = doc(LIVE_WITH_LEGACY);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert!(disk["model_providers"]
            .as_table()
            .unwrap()
            .contains_key(OFFICIAL_PROXY_ROUTE_ID));
        assert_eq!(disk["approval_policy"].as_str(), Some("never"));
    }

    /// 旧表在本行的选择器上（编辑器显示的就是它）：通用差异不碰它，删除意图落不到
    /// 这张表上，也就不会写出悬空选择器。
    #[test]
    fn a_route_selected_in_the_payload_is_not_deleted_by_the_generic_diff() {
        let selected = LIVE_WITH_LEGACY.replace(
            "model_provider = \"custom\"",
            &format!("model_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\""),
        );
        let live = context(&selected, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(&selected));
        let base = row_with_key(&selected);
        let edited = row_with_key(EDITED_WITHOUT_LEGACY);
        let plan = plan_save(
            None,
            &edited,
            &base,
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        assert!(plan
            .edits
            .paths()
            .iter()
            .all(|path| !path.contains(OFFICIAL_PROXY_ROUTE_ID)));
    }

    /// 内联表形态：容器是 `model_providers = { … }` 时删除照样成立。
    #[test]
    fn dormant_inline_route_delete_uses_open_snapshot() {
        let live_text = "model_provider = \"custom\"\nmodel = \"gpt-a\"\n\nmodel_providers = { custom = { name = \"Relay\", base_url = \"https://relay.example/v1\", wire_api = \"responses\" }, cc-switch-official = { name = \"OpenAI-legacy\", base_url = \"http://127.0.0.1:9999/v1\", wire_api = \"responses\", requires_openai_auth = true } }\n";
        let preview = "model_provider = \"custom\"\nmodel = \"gpt-a\"\n\nmodel_providers = { custom = { name = \"Relay\", base_url = \"https://relay.example/v1\", wire_api = \"responses\" }, cc-switch-official = { name = \"OpenAI\", requires_openai_auth = true, supports_websockets = false, wire_api = \"responses\", base_url = \"http://127.0.0.1:15721/v1\" } }\n";
        let live = context(live_text, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(live_text));
        assert!(snapshot.legacy_route);
        let plan = plan_save(
            None,
            &row("model_provider = \"custom\"\nmodel = \"gpt-a\"\n\nmodel_providers = { custom = { name = \"Relay\", base_url = \"https://relay.example/v1\", wire_api = \"responses\" } }\n"),
            &row(preview),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        let mut disk = doc(live_text);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        let providers = disk["model_providers"].as_inline_table().unwrap();
        assert!(!providers.contains_key(OFFICIAL_PROXY_ROUTE_ID));
        assert!(providers.contains_key("custom"));
    }

    /// 旧表不存在：没什么可删，普通编辑照常；编辑器新写一张同名表也只是普通写入（选择器
    /// 没指它就不是活动路由）。
    #[test]
    fn a_missing_legacy_route_is_left_alone() {
        let live_text = "model_provider = \"custom\"\nmodel = \"gpt-a\"\n\n[model_providers.custom]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\n";
        let live = context(live_text, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(live_text));
        assert!(!snapshot.legacy_route && snapshot.legacy_fingerprint.is_none());
        let base = row(live_text);
        let edited = row(&format!(
            "{live_text}\n[model_providers.{OFFICIAL_PROXY_ROUTE_ID}]\nname = \"OpenAI\"\nwire_api = \"responses\"\n"
        ));
        let plan = plan_save(
            None,
            &edited,
            &base,
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        let mut disk = doc(live_text);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert_eq!(
            disk["model_providers"][OFFICIAL_PROXY_ROUTE_ID]["name"].as_str(),
            Some("OpenAI")
        );
    }

    /// 打开时这张表就活着（快照不算基准）：删除按安全降级处理，不拿旧快照当删除依据。
    #[test]
    fn a_snapshot_of_an_active_route_is_no_delete_baseline() {
        let active = LIVE_WITH_LEGACY.replace(
            "model_provider = \"custom\"",
            &format!("model_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\""),
        );
        // 选择器在打开之后被改回别的路由，这张表现在是休眠的；但打开时它活着，快照
        // 不能证明「当时是休眠的」，删除降级为不动。
        let live = context(LIVE_WITH_LEGACY, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(&active));
        assert!(!snapshot.is_baseline());
        let plan = plan_save(
            None,
            &row(EDITED_WITHOUT_LEGACY),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        assert!(plan.edits.is_empty());
    }

    /// 快照只带判断删除需要的东西：表格内容（可能含凭据字段）不进快照。
    #[test]
    fn the_snapshot_carries_no_route_contents() {
        let secret = "sk-secret-token";
        let text = LIVE_WITH_LEGACY.replace(
            "http_headers = { \"x-legacy\" = \"1\" }",
            &format!("experimental_bearer_token = \"{secret}\""),
        );
        let snapshot = CodexEditorSnapshot::of(&doc(&text));
        assert!(snapshot.legacy_route);
        assert_eq!(
            snapshot.legacy_fingerprint.as_deref().map(str::len),
            Some(64),
            "指纹是哈希"
        );
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains(secret), "{serialized}");
        assert!(!serialized.contains("127.0.0.1:9999"), "{serialized}");
        // 同一张表（忽略空白和注释）指纹相同，内容变了指纹就变。
        assert_eq!(
            snapshot,
            CodexEditorSnapshot::of(&doc(&format!(
                "# 注释\n{}\n",
                LIVE_WITH_LEGACY.replace(
                    "http_headers = { \"x-legacy\" = \"1\" }",
                    &format!("experimental_bearer_token = \"{secret}\"")
                )
            )))
        );
        assert_ne!(
            snapshot.legacy_fingerprint,
            CodexEditorSnapshot::of(&doc(&text.replace("127.0.0.1:9999", "127.0.0.1:8888")))
                .legacy_fingerprint
        );
    }

    /// 编辑普通字段、而旧表在选择器上时，保存不受影响。
    #[test]
    fn ordinary_edits_still_save_while_the_route_is_active() {
        let active = LIVE_WITH_LEGACY.replace(
            "model_provider = \"custom\"",
            &format!("model_provider = \"{OFFICIAL_PROXY_ROUTE_ID}\""),
        );
        let live = context(&active, &direct());
        let snapshot = CodexEditorSnapshot::of(&doc(&active));
        let base = row_with_key(&active);
        let edited = row_with_key(
            &active
                .replace("model = \"gpt-a\"", "model = \"gpt-b\"")
                .replace(
                    "model_provider =",
                    "approval_policy = \"never\"\nmodel_provider =",
                ),
        );
        let plan = plan_save(
            None,
            &edited,
            &base,
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        let mut disk = doc(&active);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert_eq!(disk["approval_policy"].as_str(), Some("never"));
        assert!(disk["model_providers"]
            .as_table()
            .unwrap()
            .contains_key(OFFICIAL_PROXY_ROUTE_ID));
    }

    /// 代理接管但契约用的是共享槽（统一历史开）：旧表是休眠的，明确删除可以成功。
    #[test]
    fn unified_proxy_contract_leaves_the_route_deletable() {
        let live = context(LIVE_WITH_LEGACY, &attached());
        assert!(!live.contract_uses_legacy_route);
        let snapshot = CodexEditorSnapshot::of(&doc(LIVE_WITH_LEGACY));
        let plan = plan_save(
            None,
            &row(EDITED_WITHOUT_LEGACY),
            &row(PREVIEW_WITH_LEGACY),
            &Origin::Live(Vec::new()),
            false,
            false,
            ConflictPolicy::Refuse,
            LegacyRouteSave {
                snapshot: Some(&snapshot),
                live: &live,
            },
        )
        .unwrap();
        let mut disk = doc(LIVE_WITH_LEGACY);
        plan.edits
            .apply_to(Path::new("config.toml"), &mut disk)
            .unwrap();
        assert!(disk["model_providers"]
            .as_table()
            .unwrap()
            .get(OFFICIAL_PROXY_ROUTE_ID)
            .is_none());
    }
}
