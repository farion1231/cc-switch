//! Codex 的投影：供应商行 → `config.toml` 的关键字段和独有字段。
//!
//! 行的形状是 `{auth, config}`（`config` 是 TOML 文本）。投影只取关键字段，第三方路由
//! 一律写成 `[model_providers.custom]`：行里用别的 id（`deepseek`）、旧形态的顶层
//! `openai_base_url`、旧版留下的保留 id 表（`[model_providers.openai]`），都在这里归一。
//! 行里其余内容（旧版回填进来的 MCP、projects、插件）不投影，归用户和 Codex。
//!
//! Key 写成路由表的 `experimental_bearer_token`：Codex 0.149 起自定义 provider 不再读
//! `auth.json` 里的 Key，`auth.json` 只留给官方登录。

use std::path::Path;

use serde_json::Value;
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value as TomlValue};

use crate::error::AppError;
use crate::live::floor;
use crate::live::patch::toml::{same_value, shape_error, TomlDocPatch};
use crate::live::patch::LiveWriteError;

/// CC Switch 写入的路由表 id。
pub const ROUTE_ID: &str = "custom";
/// 代理模式下官方路由的表 id（旧版也认这个 id，兼容期保留）。
pub const OFFICIAL_PROXY_ROUTE_ID: &str = "cc-switch-official";
/// CC Switch 生成的模型目录文件名。
pub const CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
pub use super::claude::PROXY_TOKEN_PLACEHOLDER;
/// `web_search` 的禁用值。
pub const WEB_SEARCH_DISABLED: &str = "disabled";
const MODEL_CATALOG_JSON: &str = "model_catalog_json";

/// Codex 内置的 provider id（大小写敏感，和上游一致：`OpenAI` 是合法的自定义 id）。
const BUILT_IN_IDS: &[&str] = &[
    "amazon-bedrock",
    "amazon-bedrock-runtime",
    "openai",
    "ollama",
    "lmstudio",
];
/// 这几个 id 的表会让 Codex 0.148 起整份拒绝加载（bedrock 两个允许覆盖）。
const RESERVED_TABLE_IDS: &[&str] = &["openai", "ollama", "lmstudio"];
/// 不写 `model_provider` 时 Codex 用的内置 provider。
const DEFAULT_PROVIDER_ID: &str = "openai";
const BEDROCK_IDS: &[&str] = &["amazon-bedrock", "amazon-bedrock-runtime"];
/// 旧版把顶层 `openai_base_url` 归一成的表 id（`cc-switch`、`cc-switch-2`…）。
const LEGACY_REROUTE_ID: &str = "cc-switch";

/// 顶层的关键字段里，直接取行里值的那几个（选路、凭据、模型目录指针另算）。
const ROW_TOP_FIELDS: &[&str] = &[
    "model",
    "review_model",
    "model_reasoning_effort",
    "plan_mode_reasoning_effort",
    "disable_response_storage",
];

fn is_built_in_id(id: &str) -> bool {
    BUILT_IN_IDS.contains(&id)
}

/// 路由表的凭据从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAuth {
    /// 表里声明了 `env_key`。
    EnvKey,
    /// CC Switch 把 Key 写成表的 `experimental_bearer_token`。
    Bearer,
    /// 表自己带鉴权（`auth` 命令、`aws`、`Authorization` 头、查询参数），不注入 Key。
    Headers,
    /// 没有凭据（本地服务等）。
    None,
}

/// 第三方路由表的 `requires_openai_auth`。
///
/// Codex 0.149 上这个值不决定请求用什么凭据（`env_key`、`experimental_bearer_token`
/// 优先），但决定登录界面：为 `true` 而 `auth.json` 里没有登录，Codex 会卡在登录页；为
/// `false` 而旁边留着 ChatGPT 登录，Codex 当成已登出（账号信息不显示、token 不刷新）。
/// 所以只有凭据走自己的通道（Bearer、EnvKey）时才跟着「写完后盘上有没有登录」走；
/// Headers、None 恒为 `false`，否则请求会回退去读 `auth.json` 里的官方登录，把它发给
/// 第三方地址。官方镜像形态（统一会话历史、代理的官方路由）恒为 `true`，不经过这里。
pub fn requires_openai_auth(auth: RouteAuth, login_on_disk: bool) -> bool {
    matches!(auth, RouteAuth::EnvKey | RouteAuth::Bearer) && login_on_disk
}

/// 行要求 Codex 走哪条路由。
#[derive(Debug, Clone)]
pub enum Route {
    /// 官方：不写选路，走 Codex 内置的 openai（统一会话历史时另写官方镜像表）。
    Official,
    /// 第三方：写成 `[model_providers.custom]`。`table` 已带上 Key，`requires_openai_auth`
    /// 在写入时按 [`requires_openai_auth`] 现算。
    Custom { table: Table, auth: RouteAuth },
    /// 选中 Codex 内置的其他 provider（ollama、lmstudio、bedrock），bedrock 可以带覆盖表。
    BuiltIn { id: String, table: Option<Table> },
    /// 第三方行里没有任何路由（只有 model、MCP 之类）：不写选路。
    Default,
}

/// 一个供应商在 `config.toml` 里拥有的键。
#[derive(Debug, Clone)]
pub struct CodexProjection {
    pub route: Route,
    /// 顶层关键字段（模型名、推理档位、`disable_response_storage`）。
    pub top: Vec<(String, TomlValue)>,
    /// 嵌在用户表里的模型名（`agents.default_subagent_model` 等）。
    pub nested: Vec<(Vec<String>, TomlValue)>,
    /// 独有字段（窗口、推理摘要能力、verbosity、行里自己写的 `web_search`）。上游拒收
    /// web_search 时写入方再把它改成 `"disabled"`。
    pub exclusive: Vec<(String, TomlValue)>,
}

/// 投影的输入。
pub struct RowInput<'a> {
    /// 供应商行的 `settings_config`（`{auth, config}`）。
    pub settings: &'a Value,
    /// 官方卡（`category == "official"`，或按 `is_codex_official_provider` 认出来的）。
    pub official: bool,
    /// 代理注入凭据的 OAuth 卡（xai_oauth、github_copilot…）：本来就没有 Key，行里的
    /// `requires_openai_auth = true` 是旧模板留下的。
    pub proxy_injected_oauth: bool,
}

fn config_error(zh: impl Into<String>, en: impl Into<String>) -> AppError {
    AppError::localized("provider.codex.config.invalid", zh.into(), en.into())
}

impl CodexProjection {
    pub fn of(input: &RowInput<'_>) -> Result<Self, AppError> {
        let settings = input
            .settings
            .as_object()
            .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
        if !settings.contains_key("auth") {
            return Err(AppError::Config(
                "Codex 供应商配置缺少 'auth' 字段".to_string(),
            ));
        }
        let config_text = settings.get("config").and_then(Value::as_str).unwrap_or("");
        let doc = config_text.parse::<DocumentMut>().map_err(|err| {
            config_error(
                format!("供应商的 Codex config.toml 无法解析：{err}"),
                format!("The provider's Codex config.toml cannot be parsed: {err}"),
            )
        })?;

        let mut top: Vec<(String, TomlValue)> = ROW_TOP_FIELDS
            .iter()
            .filter_map(|key| {
                let value = doc.get(key)?.as_value()?.clone();
                Some(((*key).to_string(), undecorated(value)))
            })
            .collect();
        // 行里自己指定的模型目录（用户管理的文件）照写；指向 CC Switch 自己目录的不算，
        // 那个指针由写入方按有没有生成目录决定。
        if let Some(pointer) =
            doc.get(MODEL_CATALOG_JSON)
                .and_then(Item::as_value)
                .filter(|value| {
                    value
                        .as_str()
                        .is_some_and(|path| !is_cc_switch_catalog(path))
                })
        {
            top.push((MODEL_CATALOG_JSON.to_string(), undecorated(pointer.clone())));
        }
        let nested = floor::CODEX_FLOOR_NESTED
            .iter()
            .filter_map(|segments| {
                let value = value_at(doc.as_table(), segments)?.clone();
                Some((
                    segments.iter().map(|s| (*s).to_string()).collect(),
                    undecorated(value),
                ))
            })
            .collect();
        let exclusive = floor::CODEX_EXCLUSIVE_TOP
            .iter()
            .filter_map(|key| {
                let value = doc.get(key)?.as_value()?.clone();
                Some(((*key).to_string(), undecorated(value)))
            })
            .collect();

        let route = if input.official {
            Route::Official
        } else {
            third_party_route(&doc, input)?
        };
        Ok(Self {
            route,
            top,
            nested,
            exclusive,
        })
    }

    /// 给模型目录和 `web_search` 判定用的归一化配置：选路、路由表地址、模型名、窗口。
    /// 这两个判定按「Codex 实际会用的地址和模型」算，和行里原来的写法无关。
    pub fn catalog_input_text(&self) -> String {
        let mut doc = DocumentMut::new();
        for (key, value) in self.top.iter().chain(&self.exclusive) {
            doc[key.as_str()] = Item::Value(value.clone());
        }
        if let Route::Custom { table, .. } = &self.route {
            doc["model_provider"] = toml_edit::value(ROUTE_ID);
            let mut providers = Table::new();
            providers.set_implicit(true);
            providers.insert(ROUTE_ID, Item::Table(table.clone()));
            doc["model_providers"] = Item::Table(providers);
        }
        doc.to_string()
    }
}

fn undecorated(mut value: TomlValue) -> TomlValue {
    value.decor_mut().clear();
    value
}

fn value_at<'a>(root: &'a Table, segments: &[&str]) -> Option<&'a TomlValue> {
    let (last, parents) = segments.split_last()?;
    let mut current: &dyn TableLike = root;
    for segment in parents {
        current = current.get(segment)?.as_table_like()?;
    }
    current.get(last)?.as_value()
}

fn non_empty_str(item: Option<&Item>) -> Option<String> {
    item.and_then(Item::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// 行里的表（标准表或内联表）转成标准表，去掉原来的空白和注释。
fn to_table(item: &Item) -> Option<Table> {
    let mut table = match item {
        Item::Table(table) => table.clone(),
        Item::Value(TomlValue::InlineTable(inline)) => inline.clone().into_table(),
        _ => return None,
    };
    table.decor_mut().clear();
    table.set_implicit(false);
    table.set_dotted(false);
    for (_, value) in table.iter_mut() {
        if let Some(value) = value.as_value_mut() {
            value.decor_mut().clear();
        }
    }
    table.sort_values_by(|_, _, _, _| std::cmp::Ordering::Equal);
    Some(table)
}

fn table_bool(table: &dyn TableLike, key: &str) -> bool {
    table.get(key).and_then(Item::as_bool).unwrap_or(false)
}

fn declares_authorization_header(item: Option<&Item>) -> bool {
    item.and_then(Item::as_table_like).is_some_and(|table| {
        table
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("authorization"))
    })
}

/// 表自己带的凭据来源。`requires_openai_auth = true` 时 `Authorization` 头不算：官方
/// 登录会在头之后覆盖它（Codex 0.149），只有注入的 Key 能挡住回退。
fn declared_auth(table: &dyn TableLike) -> Option<RouteAuth> {
    if table.get("env_key").is_some() {
        return Some(RouteAuth::EnvKey);
    }
    if table.get("auth").is_some() || table.get("aws").is_some() {
        return Some(RouteAuth::Headers);
    }
    let requires = table_bool(table, "requires_openai_auth");
    if !requires
        && (declares_authorization_header(table.get("http_headers"))
            || declares_authorization_header(table.get("env_http_headers")))
    {
        return Some(RouteAuth::Headers);
    }
    None
}

fn third_party_route(doc: &DocumentMut, input: &RowInput<'_>) -> Result<Route, AppError> {
    // 配置整个是空的却带着 Key：没有地方放 Key，也不知道该发到哪。
    if doc.as_table().is_empty() && row_key(doc, None, input).is_some() {
        return Err(AppError::localized(
            "provider.codex.config.missing",
            "Codex 第三方供应商缺少 config.toml 配置，无法写入 bearer token",
            "Codex third-party provider is missing config.toml, cannot write bearer token",
        ));
    }
    let providers = doc.get("model_providers").and_then(Item::as_table_like);
    let selector = non_empty_str(doc.get("model_provider"));
    let row_table = |id: &str| providers.and_then(|p| p.get(id)).and_then(to_table);

    let (table, fallback_name) = match selector.as_deref() {
        Some(id) if !is_built_in_id(id) => match row_table(id) {
            Some(table) => (table, id.to_string()),
            None => {
                return Err(AppError::localized(
                    "provider.codex.config.no_custom_provider",
                    format!("Codex 配置选择了 model_provider = \"{id}\"，但没有对应的 [model_providers.{id}] 表"),
                    format!("The Codex config selects model_provider = \"{id}\" but has no [model_providers.{id}] table"),
                ))
            }
        },
        // 旧版留下的保留 id 表：Codex 整份拒绝加载，按内容改写成 custom。
        Some(id) if RESERVED_TABLE_IDS.contains(&id) && row_table(id).is_some() => {
            let mut table = row_table(id).expect("checked above");
            table.insert("wire_api", toml_edit::value("responses"));
            (table, "Custom".to_string())
        }
        // 旧形态：内置 openai 加顶层 openai_base_url 改道。
        None | Some("openai") => match non_empty_str(doc.get("openai_base_url")) {
            Some(base_url) => {
                let mut table = Table::new();
                table.insert("name", toml_edit::value("Custom"));
                table.insert("base_url", toml_edit::value(base_url));
                table.insert("wire_api", toml_edit::value("responses"));
                (table, "Custom".to_string())
            }
            None if selector.is_none() => return default_route(doc, input),
            None => return built_in_route("openai", providers, doc, input),
        },
        Some(id) => return built_in_route(id, providers, doc, input),
    };
    custom_route(table, &fallback_name, doc, input)
}

/// 行的 Key：`auth.OPENAI_API_KEY`，或者直接写在配置里的 `experimental_bearer_token`
/// （路由表里的优先，其次顶层）。
fn row_key(doc: &DocumentMut, table: Option<&Table>, input: &RowInput<'_>) -> Option<String> {
    input
        .settings
        .get("auth")
        .and_then(|auth| auth.get("OPENAI_API_KEY"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .or_else(|| table.and_then(|table| non_empty_str(table.get("experimental_bearer_token"))))
        .or_else(|| non_empty_str(doc.get("experimental_bearer_token")))
}

fn keyless_fallback_error() -> AppError {
    AppError::localized(
        "provider.codex.config.official_auth_fallback",
        "该 Codex 配置没有可用的 API 密钥，而 requires_openai_auth = true（或顶层 openai_base_url）会让 Codex 回退使用 auth.json 里的登录凭据访问第三方地址。请为供应商填写 API 密钥，或移除该回退指令",
        "This Codex config has no usable API key, and requires_openai_auth = true (or a top-level openai_base_url) would make Codex fall back to whatever login auth.json holds for a third-party route. Add an API key to the provider or remove the fallback directive",
    )
}

fn custom_route(
    mut table: Table,
    fallback_name: &str,
    doc: &DocumentMut,
    input: &RowInput<'_>,
) -> Result<Route, AppError> {
    if non_empty_str(table.get("name")).is_none() {
        table.insert("name", toml_edit::value(fallback_name));
    }
    let key = row_key(doc, Some(&table), input);
    let auth = match declared_auth(&table) {
        Some(declared) => declared,
        None => match key {
            Some(key) => {
                table.insert("experimental_bearer_token", toml_edit::value(key));
                RouteAuth::Bearer
            }
            None => {
                let requires = table_bool(&table, "requires_openai_auth");
                let rerouted =
                    doc.get("openai_base_url").is_some() && doc.get("model_providers").is_none();
                if (requires || rerouted) && !input.proxy_injected_oauth {
                    return Err(keyless_fallback_error());
                }
                if table.get("query_params").is_some()
                    || declares_authorization_header(table.get("http_headers"))
                    || declares_authorization_header(table.get("env_http_headers"))
                {
                    RouteAuth::Headers
                } else {
                    RouteAuth::None
                }
            }
        },
    };
    if matches!(auth, RouteAuth::Headers | RouteAuth::None) {
        table.remove("requires_openai_auth");
    }
    Ok(Route::Custom { table, auth })
}

fn built_in_route(
    id: &str,
    providers: Option<&dyn TableLike>,
    doc: &DocumentMut,
    input: &RowInput<'_>,
) -> Result<Route, AppError> {
    // 内置 openai 没有改道，就是官方地址：带 Key 没地方放，不带 Key 会用官方登录。
    if id == "openai" {
        return match row_key(doc, None, input) {
            Some(_) => Err(no_token_slot_error()),
            None => Err(keyless_fallback_error()),
        };
    }
    let table = BEDROCK_IDS
        .contains(&id)
        .then(|| providers.and_then(|p| p.get(id)).and_then(to_table))
        .flatten();
    Ok(Route::BuiltIn {
        id: id.to_string(),
        table,
    })
}

fn no_token_slot_error() -> AppError {
    AppError::localized(
        "provider.codex.config.no_custom_provider",
        "Codex 第三方配置必须包含自定义 model_providers 条目以承载 API 密钥（Codex 不识别顶层 experimental_bearer_token）",
        "A Codex third-party config must define a custom model_providers entry to carry the API key (Codex ignores a top-level experimental_bearer_token)",
    )
}

/// 没有任何路由的第三方行（只改模型、MCP 的卡）：有意放行，Codex 继续用内置 openai
/// 和它自己的登录。行里即使带着 Key 也用不上（没有第三方地址可发），不写进 live。
fn default_route(_doc: &DocumentMut, _input: &RowInput<'_>) -> Result<Route, AppError> {
    Ok(Route::Default)
}

/// 写进 `config.toml` 的路由。
#[derive(Debug, Clone)]
pub enum RouteWrite {
    /// 官方直连：不写选路。live 里已有 custom 表时改写成休眠形态（本地代理地址加占位
    /// Key）：Codex 按 provider id 给会话分桶，表一删，第三方的旧会话就 resume 不了。
    Official { dormant_base_url: String },
    /// 官方直连且开了「统一会话历史」：选路写 custom，表是官方镜像（认证走官方登录）。
    OfficialMirror,
    /// 第三方（直连或代理契约）：选路写 custom。
    Custom(Table),
    /// Codex 内置的其他 provider。
    BuiltIn { id: String, table: Option<Table> },
    /// 第三方行没有路由：不写选路。
    Default,
    /// 代理的官方路由：选路写 `cc-switch-official`，客户端带自己的登录。
    OfficialProxy(Table),
}

impl RouteWrite {
    fn selector(&self) -> Option<&str> {
        match self {
            Self::Official { .. } | Self::Default => None,
            Self::OfficialMirror | Self::Custom(_) => Some(ROUTE_ID),
            Self::BuiltIn { id, .. } => Some(id),
            Self::OfficialProxy(_) => Some(OFFICIAL_PROXY_ROUTE_ID),
        }
    }
}

/// 官方镜像表：`name = "OpenAI"`、认证走官方登录。统一会话历史和代理的官方路由用。
pub fn official_mirror_table(base_url: Option<&str>, supports_websockets: bool) -> Table {
    let mut table = Table::new();
    table.insert("name", toml_edit::value("OpenAI"));
    table.insert("requires_openai_auth", toml_edit::value(true));
    table.insert("supports_websockets", toml_edit::value(supports_websockets));
    table.insert("wire_api", toml_edit::value("responses"));
    if let Some(base_url) = base_url {
        table.insert("base_url", toml_edit::value(base_url.trim_end_matches('/')));
    }
    table
}

/// 指向本地代理的第三方路由表（代理契约和官方直连的休眠表共用）。
pub fn proxy_route_table(name: &str, base_url: &str, requires_openai_auth: bool) -> Table {
    let mut table = Table::new();
    table.insert("name", toml_edit::value(name));
    table.insert("base_url", toml_edit::value(base_url));
    table.insert("wire_api", toml_edit::value("responses"));
    table.insert(
        "experimental_bearer_token",
        toml_edit::value(PROXY_TOKEN_PLACEHOLDER),
    );
    if requires_openai_auth {
        table.insert("requires_openai_auth", toml_edit::value(true));
    }
    table
}

/// 一张能证明是 CC Switch 写进去的 provider 表：id 和地址都对得上某个供应商行的投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownTable {
    pub id: String,
    pub base_url: String,
}

/// Codex `config.toml` 的补丁：只改关键字段和独有字段，其余字节不碰。
#[derive(Debug, Clone)]
pub struct CodexConfigPatch {
    /// 顶层关键字段的目标值。
    pub top: Vec<(String, TomlValue)>,
    pub nested: Vec<(Vec<String>, TomlValue)>,
    /// 独有字段的目标值（含 `web_search`）。
    pub exclusive: Vec<(String, TomlValue)>,
    /// 上一家带进来的独有字段和它行里指定的模型目录指针：live 里的值还相同才删。
    pub outgoing: Vec<(String, TomlValue)>,
    pub route: RouteWrite,
    /// 指向 CC Switch 生成的模型目录（用户自己的指针不认领、不删除）。
    pub catalog: bool,
    /// 旧版按别的 id 写进去的表，能证明是 CC Switch 写的就删掉（里面可能有真实 Key）。
    pub retired: Vec<KnownTable>,
}

impl TomlDocPatch for CodexConfigPatch {
    fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        Self::apply_to(self, path, doc)
    }
}

/// 原位改值：已有就沿用原来的空白和行尾注释，没有就追加。
fn put_value(table: &mut dyn TableLike, key: &str, value: &TomlValue) {
    match table.get_mut(key) {
        Some(Item::Value(slot)) => {
            let decor = slot.decor().clone();
            *slot = value.clone();
            *slot.decor_mut() = decor;
        }
        Some(slot) => *slot = Item::Value(value.clone()),
        None => {
            table.insert(key, Item::Value(value.clone()));
        }
    }
}

fn is_cc_switch_catalog(value: &str) -> bool {
    Path::new(value).file_name().and_then(|name| name.to_str()) == Some(CATALOG_FILENAME)
}

/// 行里自己指定的模型目录指针（投影的 `top` 只收不是 CC Switch 的指针）。它和独有字段
/// 一样跟着这一家走：切走时 live 里的值还相同就删（见 [`CodexConfigPatch::outgoing`]），
/// 否则第 1 步会把它当成用户的指针留下，之后每一家都用它的模型目录。
pub fn row_catalog_pointer(top: &[(String, TomlValue)]) -> Option<&(String, TomlValue)> {
    top.iter().find(|(key, _)| key == MODEL_CATALOG_JSON)
}

impl CodexConfigPatch {
    pub fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let target_top: Vec<&str> = self
            .top
            .iter()
            .chain(&self.exclusive)
            .map(|(key, _)| key.as_str())
            .collect();
        let selector = self.route.selector();
        let root = doc.as_table_mut();

        // 1. 清空顶层关键字段。目标里也有的留给后面原位改值；模型目录指针只认自己的，
        //    上一家行里指定的指针在第 3 步按值删。
        let doomed: Vec<String> = root
            .iter()
            .map(|(key, _)| key.to_string())
            .filter(|key| {
                floor::CODEX_FLOOR_TOP.contains(&key.as_str())
                    && !target_top.contains(&key.as_str())
                    && !(key == "model_provider" && selector.is_some())
            })
            .collect();
        for key in doomed {
            if key == MODEL_CATALOG_JSON {
                let ours = root
                    .get(MODEL_CATALOG_JSON)
                    .and_then(Item::as_str)
                    .is_some_and(is_cc_switch_catalog);
                if !ours || self.catalog {
                    continue;
                }
            }
            root.remove(&key);
        }

        // 2. 嵌在用户表里的模型名：只清这几个键，表里的其他设置不动。
        for segments in floor::CODEX_FLOOR_NESTED {
            let (last, parents) = segments.split_last().expect("nested paths");
            let targeted = self
                .nested
                .iter()
                .any(|(path, _)| path.iter().map(String::as_str).eq(segments.iter().copied()));
            if targeted {
                continue;
            }
            let mut current: Option<&mut dyn TableLike> = Some(root as &mut dyn TableLike);
            for segment in parents {
                current = current
                    .and_then(|table| table.get_mut(segment))
                    .and_then(Item::as_table_like_mut);
            }
            if let Some(table) = current {
                table.remove(last);
            }
        }

        // 3. 上一家带进来的独有字段和模型目录指针：值还相同才删。
        for (key, value) in &self.outgoing {
            if target_top.contains(&key.as_str()) {
                continue;
            }
            let matches = root
                .get(key)
                .and_then(Item::as_value)
                .is_some_and(|current| same_value(current, value));
            if matches {
                root.remove(key);
            }
        }

        // 4. 选路和路由表。
        self.write_route(path, doc)?;
        let root = doc.as_table_mut();

        // 5. 目标值：已有就原位改，没有就追加。
        for (key, value) in self.top.iter().chain(&self.exclusive) {
            put_value(root, key, value);
        }
        for (segments, value) in &self.nested {
            let (last, parents) = segments.split_last().expect("nested paths");
            let mut current: &mut dyn TableLike = root;
            for (depth, segment) in parents.iter().enumerate() {
                if !current.contains_key(segment) {
                    current.insert(segment, Item::Table(Table::new()));
                }
                current = current
                    .get_mut(segment)
                    .and_then(Item::as_table_like_mut)
                    .ok_or_else(|| shape_error(path, &segments[..=depth]))?;
            }
            put_value(current, last, value);
        }
        let row_pointer = self.top.iter().any(|(key, _)| key == MODEL_CATALOG_JSON);
        if self.catalog && !row_pointer {
            let user_pointer = root
                .get(MODEL_CATALOG_JSON)
                .and_then(Item::as_str)
                .is_some_and(|value| !is_cc_switch_catalog(value));
            if !user_pointer {
                put_value(root, MODEL_CATALOG_JSON, &TomlValue::from(CATALOG_FILENAME));
            }
        }

        check_effective_route(doc, selector)
    }

    fn write_route(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let root = doc.as_table_mut();
        match self.route.selector() {
            Some(id) => put_value(root, "model_provider", &TomlValue::from(id)),
            None => {
                root.remove("model_provider");
            }
        }

        let referenced = profile_selectors(root);
        let container_inline = matches!(root.get("model_providers"), Some(Item::Value(_)));
        let Some(container) = root.get_mut("model_providers") else {
            // 没有 model_providers：只有要写表时才建。
            let owned = self.owned_table();
            if let Some((id, table)) = owned {
                let mut providers = Table::new();
                providers.set_implicit(true);
                providers.insert(id, Item::Table(table));
                root.insert("model_providers", Item::Table(providers));
            }
            return Ok(());
        };
        let providers = container
            .as_table_like_mut()
            .ok_or_else(|| shape_error(path, &["model_providers".to_string()]))?;

        // 旧版留下的保留 id 表会让 Codex 整份拒绝加载：能证明是 CC Switch 写的删掉，
        // 其余按原样改名成 cc-switch-N（不知道用户在乎其中哪些键）。
        for id in RESERVED_TABLE_IDS {
            let Some(item) = providers.get(id) else {
                continue;
            };
            if item.as_table_like().is_none() {
                continue;
            }
            let item = providers.remove(id).expect("present");
            if self.is_retired(id, &item) || holds_placeholder(&item) {
                continue;
            }
            let renamed = first_free_id(providers, LEGACY_REROUTE_ID);
            providers.insert(&renamed, item);
        }

        // 旧版按别的 id 写进去的表、残留的代理占位表。被 profile 引用的不动。
        let doomed: Vec<String> = providers
            .iter()
            .filter(|(id, item)| {
                *id != ROUTE_ID
                    && !referenced.iter().any(|name| name == id)
                    && (*id == OFFICIAL_PROXY_ROUTE_ID
                        || holds_placeholder(item)
                        || self.is_retired(id, item))
            })
            .map(|(id, _)| id.to_string())
            .collect();
        for id in doomed {
            providers.remove(&id);
        }

        match &self.route {
            RouteWrite::Official { dormant_base_url } => {
                if providers.contains_key(ROUTE_ID) {
                    let dormant = proxy_route_table(ROUTE_ID, dormant_base_url, false);
                    put_table(providers, ROUTE_ID, dormant, container_inline);
                }
            }
            RouteWrite::Default | RouteWrite::BuiltIn { table: None, .. } => {
                // custom 表是 CC Switch 的，没人选它了也留着（改成不带真实 Key 的休眠形态
                // 由切回官方负责）：这里保持原样，只保证不留真实 Key。
                if let Some(item) = providers.get_mut(ROUTE_ID) {
                    if let Some(table) = item.as_table_like_mut() {
                        table.remove("experimental_bearer_token");
                    }
                }
            }
            RouteWrite::BuiltIn {
                id,
                table: Some(table),
            } => {
                put_table(providers, id, table.clone(), container_inline);
            }
            RouteWrite::OfficialMirror => {
                put_table(
                    providers,
                    ROUTE_ID,
                    official_mirror_table(None, true),
                    container_inline,
                );
            }
            RouteWrite::Custom(table) => {
                put_table(providers, ROUTE_ID, table.clone(), container_inline);
            }
            RouteWrite::OfficialProxy(table) => {
                // 之前第三方路由留下的 custom 表改成休眠形态（同样指向本地代理）。
                if providers.contains_key(ROUTE_ID) {
                    if let Some(base_url) = table.get("base_url").and_then(Item::as_str) {
                        let dormant = proxy_route_table(ROUTE_ID, base_url, false);
                        put_table(providers, ROUTE_ID, dormant, container_inline);
                    }
                }
                put_table(
                    providers,
                    OFFICIAL_PROXY_ROUTE_ID,
                    table.clone(),
                    container_inline,
                );
            }
        }

        if providers.is_empty() {
            root.remove("model_providers");
        }
        Ok(())
    }

    /// 没有 `model_providers` 时要新建的那张表。
    fn owned_table(&self) -> Option<(&str, Table)> {
        match &self.route {
            RouteWrite::Custom(table) => Some((ROUTE_ID, table.clone())),
            RouteWrite::OfficialMirror => Some((ROUTE_ID, official_mirror_table(None, true))),
            RouteWrite::OfficialProxy(table) => Some((OFFICIAL_PROXY_ROUTE_ID, table.clone())),
            RouteWrite::BuiltIn {
                id,
                table: Some(table),
            } => Some((id.as_str(), table.clone())),
            _ => None,
        }
    }

    fn is_retired(&self, id: &str, item: &Item) -> bool {
        let base_url = item
            .as_table_like()
            .and_then(|table| non_empty_str(table.get("base_url")));
        self.retired
            .iter()
            .any(|known| known.id == id && base_url.as_deref() == Some(known.base_url.as_str()))
    }
}

fn holds_placeholder(item: &Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("experimental_bearer_token"))
        .and_then(Item::as_str)
        == Some(PROXY_TOKEN_PLACEHOLDER)
}

fn first_free_id(providers: &dyn TableLike, base: &str) -> String {
    let mut candidate = base.to_string();
    let mut suffix = 2usize;
    while providers.contains_key(&candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

/// 按容器原来的形式写表：内联容器（或原来就是内联表）写内联表，否则写标准表，并留在
/// 原来的位置。
fn put_table(providers: &mut dyn TableLike, id: &str, table: Table, container_inline: bool) {
    let existing_inline = matches!(providers.get(id), Some(Item::Value(_)));
    if container_inline || existing_inline {
        let inline: InlineTable = table.into_inline_table();
        match providers.get_mut(id) {
            Some(slot) => *slot = Item::Value(TomlValue::InlineTable(inline)),
            None => {
                providers.insert(id, Item::Value(TomlValue::InlineTable(inline)));
            }
        }
        return;
    }
    let mut table = table;
    match providers.get_mut(id) {
        Some(slot) => {
            if let Item::Table(old) = slot {
                *table.decor_mut() = old.decor().clone();
                if let Some(position) = old.position() {
                    table.set_position(position);
                }
            }
            *slot = Item::Table(table);
        }
        None => {
            providers.insert(id, Item::Table(table));
        }
    }
}

/// `[profiles.*]` 里 `model_provider` 引用的表 id。
fn profile_selectors(root: &Table) -> Vec<String> {
    root.get("profiles")
        .and_then(Item::as_table_like)
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|(_, profile)| {
                    non_empty_str(profile.as_table_like()?.get("model_provider"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 写入前校验最终有效路由：当前生效的 profile（顶层 `profile`）覆盖了选路或改道时，
/// Codex 实际走的不是目标供应商，拒绝写入并指出是哪个 profile。
fn check_effective_route(doc: &DocumentMut, selector: Option<&str>) -> Result<(), LiveWriteError> {
    let Some(name) = non_empty_str(doc.get("profile")) else {
        return Ok(());
    };
    let Some(profile) = doc
        .get("profiles")
        .and_then(Item::as_table_like)
        .and_then(|profiles| profiles.get(&name))
        .and_then(Item::as_table_like)
    else {
        return Ok(());
    };
    let overridden = [
        "model_provider",
        "openai_base_url",
        "experimental_bearer_token",
    ]
    .into_iter()
    .find(|key| match (*key, non_empty_str(profile.get(key))) {
        (_, None) => false,
        // 顶层不写 model_provider 时 Codex 用内置的 `openai`：profile 显式选它，请求去的
        // 还是官方卡要的地方。
        ("model_provider", Some(id)) => id != selector.unwrap_or(DEFAULT_PROVIDER_ID),
        _ => true,
    });
    match overridden {
        Some(key) => Err(LiveWriteError::Route {
            profile: name,
            key: key.to_string(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(auth: Value, config: &str) -> Value {
        json!({ "auth": auth, "config": config })
    }

    fn project(settings: &Value) -> Result<CodexProjection, AppError> {
        CodexProjection::of(&RowInput {
            settings,
            official: false,
            proxy_injected_oauth: false,
        })
    }

    fn custom(projection: &CodexProjection) -> (&Table, RouteAuth) {
        match &projection.route {
            Route::Custom { table, auth } => (table, *auth),
            other => panic!("expected custom route, got {other:?}"),
        }
    }

    const RELAY: &str = "model_provider = \"relay\"\nmodel = \"gpt-5\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";

    #[test]
    fn requires_openai_auth_follows_the_login_only_for_own_credentials() {
        for (auth, login, expected) in [
            (RouteAuth::Bearer, true, true),
            (RouteAuth::Bearer, false, false),
            (RouteAuth::EnvKey, true, true),
            (RouteAuth::EnvKey, false, false),
            (RouteAuth::Headers, true, false),
            (RouteAuth::Headers, false, false),
            (RouteAuth::None, true, false),
            (RouteAuth::None, false, false),
        ] {
            assert_eq!(
                requires_openai_auth(auth, login),
                expected,
                "{auth:?} {login}"
            );
        }
    }

    #[test]
    fn a_third_party_row_is_normalized_into_the_custom_route_with_its_key() {
        let settings = row(json!({ "OPENAI_API_KEY": "sk-relay" }), RELAY);
        let projection = project(&settings).unwrap();
        let (table, auth) = custom(&projection);
        assert_eq!(auth, RouteAuth::Bearer);
        assert_eq!(
            table
                .get("experimental_bearer_token")
                .and_then(Item::as_str),
            Some("sk-relay")
        );
        assert_eq!(table.get("name").and_then(Item::as_str), Some("Relay"));
        assert_eq!(projection.top[0].0, "model");
    }

    #[test]
    fn a_table_with_its_own_credentials_gets_no_key() {
        for extra in [
            "env_key = \"RELAY_KEY\"",
            "http_headers = { Authorization = \"Bearer x\" }",
        ] {
            let config = format!(
                "model_provider = \"relay\"\n[model_providers.relay]\nname = \"R\"\nbase_url = \"https://r.example\"\n{extra}\n"
            );
            let settings = row(json!({ "OPENAI_API_KEY": "sk" }), &config);
            let projection = project(&settings).unwrap();
            let (table, _) = custom(&projection);
            assert!(table.get("experimental_bearer_token").is_none(), "{extra}");
        }
    }

    #[test]
    fn requires_openai_auth_does_not_stop_the_key_injection() {
        let config = "model_provider = \"relay\"\n[model_providers.relay]\nname = \"R\"\nbase_url = \"https://r.example\"\nrequires_openai_auth = true\nhttp_headers = { Authorization = \"Bearer x\" }\n";
        let settings = row(json!({ "OPENAI_API_KEY": "sk" }), config);
        let projection = project(&settings).unwrap();
        let (table, auth) = custom(&projection);
        assert_eq!(auth, RouteAuth::Bearer);
        assert!(table.get("experimental_bearer_token").is_some());
    }

    #[test]
    fn legacy_shapes_become_the_custom_route() {
        let reroute = row(
            json!({ "OPENAI_API_KEY": "sk" }),
            "openai_base_url = \"https://legacy.example/v1\"\nmodel = \"m\"\n",
        );
        let projection = project(&reroute).unwrap();
        let (table, _) = custom(&projection);
        assert_eq!(
            table.get("base_url").and_then(Item::as_str),
            Some("https://legacy.example/v1")
        );

        let reserved = row(
            json!({ "OPENAI_API_KEY": "sk" }),
            "model_provider = \"openai\"\n[model_providers.openai]\nbase_url = \"https://stale.example\"\nwire_api = \"chat\"\n",
        );
        let projection = project(&reserved).unwrap();
        let (table, _) = custom(&projection);
        assert_eq!(
            table.get("wire_api").and_then(Item::as_str),
            Some("responses")
        );
        assert_eq!(table.get("name").and_then(Item::as_str), Some("Custom"));
    }

    #[test]
    fn rows_that_would_send_the_official_login_to_a_third_party_are_refused() {
        let keyless = row(json!({}), RELAY);
        assert!(project(&keyless).is_err());
        let reroute = row(
            json!({}),
            "openai_base_url = \"https://legacy.example/v1\"\n",
        );
        assert!(project(&reroute).is_err());
        let key_without_slot = row(
            json!({ "OPENAI_API_KEY": "sk" }),
            "model_provider = \"relay\"\nmodel = \"m\"\n",
        );
        assert!(project(&key_without_slot).is_err());

        // 代理注入凭据的 OAuth 卡本来就没有 Key：放行，且不带 requires_openai_auth。
        let oauth = CodexProjection::of(&RowInput {
            settings: &keyless,
            official: false,
            proxy_injected_oauth: true,
        })
        .unwrap();
        let (table, auth) = custom(&oauth);
        assert_eq!(auth, RouteAuth::None);
        assert!(table.get("requires_openai_auth").is_none());

        // 只有 model、没有路由的卡：有意放行（带 Key 也一样，Key 没有第三方地址可发）。
        for auth in [json!({}), json!({ "OPENAI_API_KEY": "sk" })] {
            let model_only = row(auth, "model = \"m\"\n");
            assert!(matches!(
                project(&model_only).unwrap().route,
                Route::Default
            ));
        }
    }
}
