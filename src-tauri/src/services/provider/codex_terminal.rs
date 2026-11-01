//! 「打开终端」用某张 Codex 卡启动 `codex`：只对这一个进程生效，不写 `config.toml`。
//!
//! 办法是 `codex -c key=value`，但它和 `config.toml` 是逐层合并的（0.164 实测）：给
//! `model_providers.custom` 传一整张表，原表里没被覆盖的键（请求头、查询参数、bearer）照样
//! 留着；顶层的键也只能改值、不能删。所以切到这张卡时会变的键这里都显式写一遍：
//!
//! - 路由表：Key 改用 `env_key` 经环境变量传，不出现在命令行上，也压得住原表留下的
//!   `experimental_bearer_token`（实测 `env_key` 优先）。原表里还有覆盖不掉的键时不打开，
//!   免得把当前那家的凭据或参数发给这一家。
//! - 顶层关键字段、独有字段：卡上有的照写；卡上没有、live 里却有的是上一家留下的，尽量
//!   写成和不设置一样的值（模型目录里这个模型的默认值，没有目录时见 [`CODEX_DEFAULT_EFFORT`]）；
//!   没有对应写法的不打开。
//!
//! 卡就是当前直连的那家时直接跑 `codex`：live 本来就是它，用户在 Codex 里选的模型和档位
//! 也照用。官方卡和没有路由的卡跟着 Codex 自己的登录走，只对一个进程换不过去，要先切换；
//! 路由模式下它们是当前路由时也直接跑 `codex`（请求经本地路由到它）。
//! 每种启动都把 `CODEX_HOME` 定到 CC Switch 管的目录，Codex 读的就是这里检查过的那份。

use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, TableLike, Value as TomlValue};

use crate::app_config::AppType;
use crate::codex_config::{
    codex_single_model_catalog, get_codex_config_dir, plan_codex_model_catalog,
};
use crate::database::Database;
use crate::error::AppError;
use crate::live::floor::{CODEX_EXCLUSIVE_TOP, CODEX_FLOOR_NESTED, CODEX_FLOOR_TOP};
use crate::live::project::codex::{
    requires_openai_auth, row_catalog_pointer, Route, RouteAuth, ROUTE_ID,
};
use crate::mode::current::{self, Purpose};
use crate::provider::Provider;

use super::codex_direct::{self, PROXY_KEPT_CLIENT_KEYS};

/// 卡上的 Key 经这个环境变量交给 Codex（路由表的 `env_key`）。
pub(crate) const API_KEY_ENV: &str = "CC_SWITCH_CODEX_API_KEY";

/// 上一家留下了推理强度、这张卡又没有模型目录时写的值。Codex 不设置时对目录外的模型
/// 不发 `reasoning.effort`（0.164 实测），`-c` 写不出「不发」，退而用各家都认的 medium，
/// 总比带着上一家的 xhigh 强。有目录时用目录里这个模型的默认档位，和不设置一样。
const CODEX_DEFAULT_EFFORT: &str = "medium";
/// 上一家留下了输出详略时写的值：Codex 只对支持它的模型发，其余模型忽略。
const CODEX_DEFAULT_VERBOSITY: &str = "medium";

/// 一次终端启动要加的东西。`overrides` 为空就是直接跑 `codex`。
#[derive(Debug, Default)]
pub(crate) struct CodexTerminalLaunch {
    /// `-c` 的值，每项是 `键路径=TOML 值`。
    pub overrides: Vec<String>,
    /// 只给这个进程的环境变量。
    pub env: Vec<(&'static str, Option<String>)>,
    /// 只给这次启动的模型目录：启动方写进临时文件，再用 [`catalog_override`] 指过去。
    pub catalog: Option<Vec<u8>>,
}

pub(crate) fn terminal_launch(
    db: &Database,
    provider: &Provider,
) -> Result<CodexTerminalLaunch, AppError> {
    if crate::tray::provider_needs_routing(&AppType::Codex, provider) {
        return Err(AppError::localized(
            "codex.terminal.needs_routing",
            format!("「{}」要经本地路由才能用，终端不能直连它", provider.name),
            format!(
                "\"{}\" only works through the local router, so a terminal can't connect to it directly",
                provider.name
            ),
        ));
    }
    let proxy = current::is_proxy(&AppType::Codex);
    let purpose = if proxy {
        Purpose::InUse
    } else {
        Purpose::Direct
    };
    let current = current::provider_for(db, &AppType::Codex, purpose)?.as_deref()
        == Some(provider.id.as_str());
    // 直连下当前那家就是 live；路由下跟着登录走的卡也只能经路由用，直接跑 `codex`。
    let plain = current
        && (!proxy
            || matches!(
                codex_direct::project(provider)?.route,
                Route::Official | Route::Default
            ));
    let mut launch = if plain {
        CodexTerminalLaunch::default()
    } else {
        build(provider, &codex_direct::read_config_text())?
    };
    // 检查的是 CC Switch 管的目录（设置里可以改），Codex 也得读这个目录，而不是默认的或
    // shell 里 CODEX_HOME 指的。
    launch.env.push((
        CODEX_HOME_ENV,
        Some(get_codex_config_dir().to_string_lossy().into_owned()),
    ));
    Ok(launch)
}

const CODEX_HOME_ENV: &str = "CODEX_HOME";

/// 指向临时模型目录的 `-c` 值。
pub(crate) fn catalog_override(path: &str) -> String {
    format!("model_catalog_json={}", toml_string(path))
}

fn build(provider: &Provider, live_text: &str) -> Result<CodexTerminalLaunch, AppError> {
    let live = live_text.parse::<DocumentMut>().map_err(|err| {
        AppError::localized(
            "codex.terminal.live_unparsable",
            format!("Codex 的 config.toml 无法解析：{err}"),
            format!("Codex's config.toml cannot be parsed: {err}"),
        )
    })?;
    if live.get("profile").and_then(Item::as_str).is_some() {
        return Err(AppError::localized(
            "codex.terminal.profile_selected",
            "Codex 的 config.toml 选了 profile，它的设置会盖过终端的覆盖。请先切换到这张卡，再打开终端",
            "Codex's config.toml selects a profile, whose settings would override the terminal's. Switch to this provider first, then open the terminal",
        ));
    }
    let projection = codex_direct::project(provider)?;
    let mut launch = CodexTerminalLaunch::default();
    let mut uncovered = Vec::new();

    match &projection.route {
        Route::Custom { table, auth } => {
            let mut table = table.clone();
            if *auth == RouteAuth::Bearer {
                let key = table
                    .remove("experimental_bearer_token")
                    .and_then(|item| item.as_str().map(str::to_string));
                table.insert("env_key", toml_edit::value(API_KEY_ENV));
                launch.env.push((API_KEY_ENV, key));
            }
            let base = provider_table(&live, ROUTE_ID);
            // 盘上有没有登录，live 的表在写入时已经按它算好了。
            let login = base
                .and_then(|base| base.get("requires_openai_auth"))
                .and_then(Item::as_bool)
                .unwrap_or(false);
            table.insert(
                "requires_openai_auth",
                toml_edit::value(requires_openai_auth(*auth, login)),
            );
            if !table.contains_key("wire_api") {
                table.insert("wire_api", toml_edit::value("responses"));
            }
            if !table.contains_key("supports_websockets") {
                table.insert("supports_websockets", toml_edit::value(false));
            }
            if let Some(base) = base {
                uncovered_keys(
                    base,
                    &table,
                    &format!("model_providers.{ROUTE_ID}"),
                    true,
                    &mut uncovered,
                );
            }
            set(&mut launch, "model_provider", toml_string(ROUTE_ID));
            set(
                &mut launch,
                &format!("model_providers.{ROUTE_ID}"),
                render_table(&table),
            );
        }
        Route::BuiltIn { id, table } => {
            set(&mut launch, "model_provider", toml_string(id));
            if let Some(table) = table {
                if let Some(base) = provider_table(&live, id) {
                    uncovered_keys(
                        base,
                        table,
                        &format!("model_providers.{id}"),
                        true,
                        &mut uncovered,
                    );
                }
                set(
                    &mut launch,
                    &format!("model_providers.{id}"),
                    render_table(table),
                );
            }
        }
        Route::Official | Route::Default => {
            return Err(AppError::localized(
                "codex.terminal.follows_login",
                format!(
                    "「{}」跟着 Codex 自己的登录走，终端没法只对这一次换成它。请先切换到这张卡，再打开终端",
                    provider.name
                ),
                format!(
                    "\"{}\" uses Codex's own sign-in, which can't be swapped for a single terminal. Switch to this provider first, then open the terminal",
                    provider.name
                ),
            ));
        }
    }

    let mut top = projection.top.clone();
    top.extend(codex_direct::exclusive_of(provider, &projection));
    for (key, value) in &mut top {
        if key == "model_catalog_json" {
            *value = absolute_catalog_pointer(value);
        }
    }
    let model = top
        .iter()
        .find(|(key, _)| key == "model")
        .and_then(|(_, value)| value.as_str())
        .map(str::to_string);

    if row_catalog_pointer(&projection.top).is_none() {
        let config_text = projection.catalog_input_text();
        let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);
        let mut catalog =
            plan_codex_model_catalog(&provider.settings_config, &config_text, profile)?.catalog;
        // 切换时没有目录的卡会删掉 `model_catalog_json`，`-c` 删不掉：上一家留着指针就给这张卡
        // 的模型生成一份目录盖过去，否则 Codex 只认上一家的模型。
        if catalog.is_none() && live.get("model_catalog_json").is_some() {
            if let Some(model) = model.as_deref() {
                catalog = Some(codex_single_model_catalog(model, &config_text, profile)?);
            }
        }
        launch.catalog = catalog
            .map(|catalog| serde_json::to_vec(&catalog))
            .transpose()
            .map_err(|err| AppError::Message(format!("序列化 Codex 模型目录失败: {err}")))?;
    }
    let entry = launch
        .catalog
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|catalog| catalog_entry(&catalog, model.as_deref()?).cloned());
    let neutral = Neutral {
        model: model.as_deref(),
        entry: entry.as_ref(),
        catalog: launch.catalog.is_some(),
    };

    for (key, value) in &top {
        set(&mut launch, key, render_value(value));
    }
    for key in CODEX_FLOOR_TOP.iter().chain(CODEX_EXCLUSIVE_TOP) {
        if live.get(key).is_none() || top.iter().any(|(set_key, _)| set_key == key) {
            continue;
        }
        match neutral.top(key) {
            Leftover::Ignore => {}
            Leftover::Set(value) => set(&mut launch, key, value),
            Leftover::Uncovered => uncovered.push((*key).to_string()),
        }
    }

    for (path, value) in &projection.nested {
        set(&mut launch, &path.join("."), render_value(value));
    }
    for segments in CODEX_FLOOR_NESTED {
        let targeted = projection
            .nested
            .iter()
            .any(|(path, _)| path.iter().map(String::as_str).eq(segments.iter().copied()));
        if targeted || value_at(live.as_table(), segments).is_none() {
            continue;
        }
        let path = segments.join(".");
        match neutral.nested(segments) {
            Leftover::Ignore => {}
            Leftover::Set(value) => set(&mut launch, &path, value),
            Leftover::Uncovered => uncovered.push(path),
        }
    }

    if !uncovered.is_empty() {
        let keys = uncovered.join(", ");
        return Err(AppError::localized(
            "codex.terminal.uncovered",
            format!(
                "Codex 的 config.toml 里有当前供应商留下的 {keys}，终端覆盖不掉，可能把它的设置带给「{}」。请先切换到这张卡，再打开终端",
                provider.name
            ),
            format!(
                "Codex's config.toml has {keys} left by the current provider. The terminal can't override them, so they could carry over to \"{}\". Switch to this provider first, then open the terminal",
                provider.name
            ),
        ));
    }
    Ok(launch)
}

fn set(launch: &mut CodexTerminalLaunch, path: &str, value: String) {
    launch.overrides.push(format!("{path}={value}"));
}

/// 上一家留在 live 里、这张卡没有的键怎么处理。
enum Leftover {
    /// 不影响这一家（选路已经覆盖、Codex 不认的旧写法、留着无害）。
    Ignore,
    /// 写成这个值，效果等于 Codex 没设置它。
    Set(String),
    /// 没有等价写法。
    Uncovered,
}

struct Neutral<'a> {
    model: Option<&'a str>,
    /// 这次生成的模型目录里这个模型的那一行。
    entry: Option<&'a Value>,
    /// 这次生成了模型目录（启动方会用 `model_catalog_json` 指过去）。
    catalog: bool,
}

impl Neutral<'_> {
    fn entry_str(&self, field: &str) -> Option<&str> {
        self.entry?.get(field)?.as_str()
    }

    fn model(&self) -> Leftover {
        self.model.map_or(Leftover::Uncovered, |model| {
            Leftover::Set(toml_string(model))
        })
    }

    fn effort(&self) -> Leftover {
        let effort = self
            .entry_str("default_reasoning_level")
            .unwrap_or(CODEX_DEFAULT_EFFORT);
        Leftover::Set(toml_string(effort))
    }

    fn top(&self, key: &str) -> Leftover {
        match key {
            // 选路已经写了；`experimental_bearer_token`、`base_url`、`wire_api` 是旧版顶层
            // 写法，Codex 不认；`web_search` 是工具开关，留着只是多一个或少一个工具。
            "model_provider"
            | "openai_base_url"
            | "experimental_bearer_token"
            | "base_url"
            | "wire_api"
            | "web_search" => Leftover::Ignore,
            // 生成了目录，启动方会指过去；没生成说明卡上没有模型名，盖不掉。
            "model_catalog_json" if self.catalog => Leftover::Ignore,
            "model_catalog_json" => Leftover::Uncovered,
            "model" => Leftover::Uncovered,
            "review_model" => self.model(),
            "model_reasoning_effort" | "plan_mode_reasoning_effort" => self.effort(),
            "disable_response_storage" => Leftover::Set("false".to_string()),
            "model_supports_reasoning_summaries" => Leftover::Set(
                self.entry
                    .and_then(|entry| entry.get("supports_reasoning_summaries"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    .to_string(),
            ),
            "model_verbosity" => Leftover::Set(toml_string(
                self.entry_str("default_verbosity")
                    .unwrap_or(CODEX_DEFAULT_VERBOSITY),
            )),
            "model_context_window" => self
                .entry
                .and_then(|entry| entry.get("context_window"))
                .and_then(Value::as_u64)
                .map_or(Leftover::Uncovered, |window| {
                    Leftover::Set(window.to_string())
                }),
            _ => Leftover::Uncovered,
        }
    }

    fn nested(&self, segments: &[&str]) -> Leftover {
        match segments.last().copied() {
            Some("default_subagent_reasoning_effort") => self.effort(),
            // 子代理、记忆用的模型没设置时就是主模型。
            Some(_) => self.model(),
            None => Leftover::Uncovered,
        }
    }
}

fn provider_table<'a>(doc: &'a DocumentMut, id: &str) -> Option<&'a dyn TableLike> {
    doc.get("model_providers")?
        .as_table_like()?
        .get(id)?
        .as_table_like()
}

/// live 原表里有、终端的表压不住的键。`root` 是路由表本身那一层。
fn uncovered_keys(
    base: &dyn TableLike,
    ours: &dyn TableLike,
    prefix: &str,
    root: bool,
    out: &mut Vec<String>,
) {
    for (key, item) in base.iter() {
        let path = format!("{prefix}.{key}");
        match ours.get(key) {
            Some(mine) => {
                if let (Some(base), Some(mine)) = (item.as_table_like(), mine.as_table_like()) {
                    uncovered_keys(base, mine, &path, false, out);
                }
            }
            None => {
                let neutralized = root
                    && ((key == "experimental_bearer_token" && ours.contains_key("env_key"))
                        || PROXY_KEPT_CLIENT_KEYS.contains(&key));
                if !neutralized {
                    out.push(path);
                }
            }
        }
    }
}

fn value_at<'a>(root: &'a Table, segments: &[&str]) -> Option<&'a TomlValue> {
    let (last, parents) = segments.split_last()?;
    let mut current: &dyn TableLike = root;
    for segment in parents {
        current = current.get(segment)?.as_table_like()?;
    }
    current.get(last)?.as_value()
}

fn catalog_entry<'a>(catalog: &'a Value, model: &str) -> Option<&'a Value> {
    catalog
        .get("models")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("slug").and_then(Value::as_str) == Some(model))
}

/// `-c` 里的相对路径按当前目录解析，`config.toml` 里的按 Codex 目录：换成绝对路径。
fn absolute_catalog_pointer(value: &TomlValue) -> TomlValue {
    match value.as_str() {
        Some(path) if std::path::Path::new(path).is_relative() => TomlValue::from(
            get_codex_config_dir()
                .join(path)
                .to_string_lossy()
                .into_owned(),
        ),
        _ => value.clone(),
    }
}

/// TOML 字符串：能用单引号字面量就用（Windows 的 .bat 里传不了双引号），否则用基本字符串。
fn toml_string(text: &str) -> String {
    let literal_ok = !text.contains('\'') && !text.chars().any(|c| c.is_control() && c != '\t');
    if literal_ok {
        format!("'{text}'")
    } else {
        let mut value = TomlValue::from(text);
        value.decor_mut().clear();
        value.to_string()
    }
}

fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_string()
    } else {
        toml_string(key)
    }
}

fn render_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => toml_string(text.value()),
        TomlValue::Array(array) => {
            let items: Vec<String> = array.iter().map(render_value).collect();
            format!("[{}]", items.join(","))
        }
        TomlValue::InlineTable(table) => render_table(table),
        other => {
            let mut other = other.clone();
            other.decor_mut().clear();
            other.to_string()
        }
    }
}

fn render_table(table: &dyn TableLike) -> String {
    let entries: Vec<String> = table
        .iter()
        .filter_map(|(key, item)| {
            let value = match item {
                Item::Value(value) => render_value(value),
                Item::Table(table) => render_table(table),
                _ => return None,
            };
            Some(format!("{}={value}", toml_key(key)))
        })
        .collect();
    format!("{{{}}}", entries.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(config: &str, key: &str) -> Provider {
        Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({ "auth": { "OPENAI_API_KEY": key }, "config": config }),
            None,
        )
    }

    const KIMI: &str = r#"model = "kimi-k2"
model_provider = "kimi"

[model_providers.kimi]
name = "Kimi"
base_url = "https://api.moonshot.cn/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

    #[test]
    fn third_party_card_overrides_route_and_passes_key_by_env() {
        let live = r#"model = "gpt-6-astra"
model_reasoning_effort = "xhigh"

[model_providers.custom]
name = "Old"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#;
        let launch = build(&provider(KIMI, "sk-kimi"), live).unwrap();
        assert_eq!(launch.env, vec![(API_KEY_ENV, Some("sk-kimi".to_string()))]);
        assert!(launch
            .overrides
            .contains(&"model_provider='custom'".to_string()));
        let table = launch
            .overrides
            .iter()
            .find(|o| o.starts_with("model_providers.custom="))
            .expect("route table");
        assert!(table.contains("base_url='https://api.moonshot.cn/v1'"));
        assert!(table.contains(&format!("env_key='{API_KEY_ENV}'")));
        assert!(
            !table.contains("sk-kimi"),
            "the key must not be on the command line"
        );
        // 上一家的模型和推理强度不能带过来。
        assert!(launch.overrides.contains(&"model='kimi-k2'".to_string()));
        assert!(launch
            .overrides
            .contains(&"model_reasoning_effort='medium'".to_string()));
        assert!(launch.overrides.iter().all(|o| !o.contains('"')));
    }

    #[test]
    fn leftover_headers_in_the_live_table_refuse_to_open() {
        let live = r#"model_provider = "custom"

[model_providers.custom]
name = "Other"
base_url = "https://other.example/v1"
http_headers = { "X-Api-Key" = "other-secret" }
"#;
        let err = build(&provider(KIMI, "sk-kimi"), live).unwrap_err();
        assert!(err
            .to_string()
            .contains("model_providers.custom.http_headers"));
    }

    #[test]
    fn keyless_card_cannot_mask_a_leftover_bearer() {
        let config = r#"model = "llama"
model_provider = "local"

[model_providers.local]
name = "Local"
base_url = "http://localhost:8080/v1"
"#;
        let live = r#"[model_providers.custom]
name = "Paid"
base_url = "https://paid.example/v1"
experimental_bearer_token = "sk-paid"
"#;
        let err = build(&provider(config, ""), live).unwrap_err();
        assert!(err
            .to_string()
            .contains("model_providers.custom.experimental_bearer_token"));
    }

    #[test]
    fn leftover_window_without_a_catalog_refuses_but_subagent_model_follows() {
        let live = r#"model = "other"
model_context_window = 1000000

[agents]
default_subagent_model = "other-mini"
"#;
        let err = build(&provider(KIMI, "sk-kimi"), live).unwrap_err();
        assert!(err.to_string().contains("model_context_window"));

        let live = "[agents]\ndefault_subagent_model = \"other-mini\"\n";
        let launch = build(&provider(KIMI, "sk-kimi"), live).unwrap();
        assert!(launch
            .overrides
            .contains(&"agents.default_subagent_model='kimi-k2'".to_string()));
    }

    // 切换时没有目录的卡会删掉指针；终端删不掉，就给这张卡的模型生成目录盖过去。
    #[test]
    fn leftover_catalog_pointer_is_replaced_by_the_cards_own_model() {
        let live = "model_catalog_json = \"/old/catalog.json\"\n";
        let launch = build(&provider(KIMI, "sk-kimi"), live).unwrap();
        let catalog: Value = serde_json::from_slice(&launch.catalog.expect("catalog")).unwrap();
        let slugs: Vec<&str> = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["slug"].as_str())
            .collect();
        assert_eq!(slugs, vec!["kimi-k2"]);
        assert!(launch
            .overrides
            .iter()
            .all(|o| !o.starts_with("model_catalog_json=")));
    }

    #[test]
    fn official_card_needs_a_switch_first() {
        let mut official = provider("", "");
        official.category = Some("official".to_string());
        let err = build(&official, "").unwrap_err();
        assert!(err.to_string().contains("Codex"));
    }

    #[test]
    fn strings_with_single_quotes_fall_back_to_basic_strings() {
        assert_eq!(toml_string("plain"), "'plain'");
        assert_eq!(toml_string("it's"), "\"it's\"");
        assert_eq!(toml_key("api-version"), "api-version");
        assert_eq!(toml_key("X.Y"), "'X.Y'");
    }
}
