//! GitHub Copilot CLI (`copilot`) 的 live 配置。
//!
//! Copilot CLI 的数据目录是 `~/.copilot`（`COPILOT_HOME` 可改）：`settings.json` 管模型等
//! 用户设置，`mcp-config.json` 管 MCP，`skills/` 和 `copilot-instructions.md` 分别是个人
//! Skills 和全局指令，会话在 `session-state/<id>/`。
//!
//! 卡片分两种：
//!
//! - **官方登录卡**（分类为官方，或没有 `provider` 节点）：启动式，只有「打开终端」，
//!   `copilot --model=<模型>`（没设模型就是 `copilot`），用 Copilot CLI 自己的 GitHub 登录；
//!   绑了授权中心的 GitHub 账号时，经 `COPILOT_GITHUB_TOKEN` 只给这一个进程。绝不写任何文件。
//! - **自带模型卡（BYOK）**：累加式。「添加 / 移除」只改写 Copilot 原生的 `providers.json`
//!   里属于这张卡的条目（`name` == 卡片 id 的供应商、`provider` == 卡片 id 的模型），其余条目、
//!   顶层键和顺序原样保留。Copilot 的 `/model` 里出现 `<卡片 id>/<模型 id>`，和官方模型并列。
//!
//! 卡片的 `settingsConfig`：`{ provider: {...不含 name}, models: [{...不含 provider}], reasoningEffort? }`。
//! `reasoningEffort` 只用于「打开终端」的 `--reasoning-effort`，不写进 `providers.json`。
//!
//! Copilot 对 `providers.json` 校验很严（未知键、名字含 `/`、重名、模型引用不存在的供应商、
//! websockets 配 completions 都会让它整个起不来，官方模型也用不了），所以每次写之前都按同样的
//! 规则校验整份结果文档，不过就不写。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::config::{get_home_dir, stage_write};
use crate::error::AppError;

/// 旧版（PR 期间）自带模型卡用的环境变量。「打开终端」时全部清掉：进程环境里只要有
/// `COPILOT_PROVIDER_BASE_URL`，Copilot 就完全不读 `providers.json`。
pub(crate) const BYOK_ENV_KEYS: &[&str] = &[
    "COPILOT_PROVIDER_BASE_URL",
    "COPILOT_PROVIDER_TYPE",
    "COPILOT_PROVIDER_API_KEY",
    "COPILOT_PROVIDER_BEARER_TOKEN",
    "COPILOT_PROVIDER_WIRE_API",
    "COPILOT_PROVIDER_TRANSPORT",
    "COPILOT_PROVIDER_AZURE_API_VERSION",
    "COPILOT_PROVIDER_MODEL_ID",
    "COPILOT_PROVIDER_WIRE_MODEL",
    "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
    "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
    "COPILOT_PROVIDER_HEADERS",
];

pub(crate) const MODEL_ENV: &str = "COPILOT_MODEL";
pub(crate) const BASE_URL_ENV: &str = "COPILOT_PROVIDER_BASE_URL";
pub(crate) const API_KEY_ENV: &str = "COPILOT_PROVIDER_API_KEY";
pub(crate) const BEARER_TOKEN_ENV: &str = "COPILOT_PROVIDER_BEARER_TOKEN";

/// 非空时 Copilot 改读这个路径的供应商文件。
pub(crate) const PROVIDERS_CONFIG_ENV: &str = "COPILOT_PROVIDERS_CONFIG";

/// 卡片里的推理强度（`settingsConfig.reasoningEffort`），启动时作为 `--reasoning-effort`
/// 传给 copilot：Copilot CLI 没有对应的环境变量或设置项。`none` 时 Copilot 不再在
/// Responses 请求里带 `reasoning.summary`，不认这个字段的中转靠它才能用 Responses。
pub(crate) const REASONING_EFFORT_KEY: &str = "reasoningEffort";
pub(crate) const REASONING_EFFORTS: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Copilot 认的供应商条目键（additionalProperties: false）。
const PROVIDER_KEYS: &[&str] = &[
    "name",
    "type",
    "wireApi",
    "transport",
    "baseUrl",
    "apiKey",
    "bearerToken",
    "azure",
    "headers",
    "modelProvider",
    "hasBearerTokenProvider",
];

/// Copilot 认的模型条目键（additionalProperties: false）。
const MODEL_KEYS: &[&str] = &[
    "id",
    "provider",
    "wireModel",
    "modelId",
    "name",
    "maxPromptTokens",
    "maxContextWindowTokens",
    "maxOutputTokens",
    "capabilities",
    "systemMessage",
    "metadata",
];

/// 卡片 id 的长度上限（也是 `providers.json` 里的供应商名）。
const MAX_CARD_ID_CHARS: usize = 64;

/// 进程内串行化 `providers.json` 的读改写。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// `~/.copilot`；设置了 `COPILOT_HOME` 时跟随它（CC Switch 进程需要继承这个环境变量）。
pub fn copilot_dir() -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".copilot"))
}

pub fn settings_path() -> PathBuf {
    copilot_dir().join("settings.json")
}

pub fn mcp_config_path() -> PathBuf {
    copilot_dir().join("mcp-config.json")
}

pub fn skills_dir() -> PathBuf {
    copilot_dir().join("skills")
}

pub fn session_state_dir() -> PathBuf {
    copilot_dir().join("session-state")
}

/// Copilot 读的供应商文件：`COPILOT_PROVIDERS_CONFIG` 非空时用它，否则 `<copilot_dir>/providers.json`。
pub fn providers_path() -> PathBuf {
    std::env::var_os(PROVIDERS_CONFIG_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| copilot_dir().join("providers.json"))
}

/// 读 `settings.json`：Copilot 写的是 JSON，但用户手改时可能带注释，按 JSON5 兜底。
/// 文件不存在时返回 `None`；内容坏了报错，不当作空文件。
pub(crate) fn read_settings_document() -> Result<Option<Value>, AppError> {
    read_json_document(&settings_path())
}

pub(crate) fn read_json_document(path: &Path) -> Result<Option<Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AppError::io(path, e)),
    };
    if text.trim().is_empty() {
        return Ok(Some(json!({})));
    }
    let value: Value = serde_json::from_str(&text)
        .or_else(|_| json5::from_str(&text))
        .map_err(|e| {
            AppError::Config(format!("Invalid Copilot CLI JSON {}: {e}", path.display()))
        })?;
    if !value.is_object() {
        return Err(AppError::Config(format!(
            "Copilot CLI {} must be a JSON object",
            path.display()
        )));
    }
    Ok(Some(value))
}

/// 从 live 读回官方卡编辑器用的 `{ env }`：只有 `settings.json` 的模型。选中的是
/// `providers.json` 里某家的模型（`<供应商>/<模型>`）时不算官方模型，不读回。
pub fn read_live_settings() -> Result<Value, AppError> {
    let Some(settings) = read_settings_document()? else {
        return Err(AppError::localized(
            "copilot.config.missing",
            "GitHub Copilot CLI 配置文件不存在",
            "GitHub Copilot CLI configuration file is missing",
        ));
    };
    let byok_names = read_providers(&providers_path())
        .map(|document| provider_names(&document))
        .unwrap_or_default();
    let mut env = serde_json::Map::new();
    if let Some(model) = settings
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .filter(|model| selected_provider_name(model).is_none_or(|name| !byok_names.contains(name)))
    {
        env.insert(MODEL_ENV.into(), json!(model));
    }
    Ok(json!({ "env": env }))
}

/// 配置里是否带有值得导入的内容。Copilot 首次启动就会写出只有界面设置的 `settings.json`，
/// 没有模型的不是用户的供应商。
pub(crate) fn has_managed_content(settings: &Value) -> bool {
    CopilotProjection::of(settings).model.is_some()
}

/// 自带模型卡：有 `provider` 节点。
pub(crate) fn is_byok_settings(settings: &Value) -> bool {
    settings.get("provider").is_some_and(Value::is_object)
}

/// 官方卡里 Copilot CLI 认的字段。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CopilotProjection {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl CopilotProjection {
    pub(crate) fn of(settings: &Value) -> Self {
        let env = settings.get("env");
        Self {
            model: env_string(env, MODEL_ENV),
            reasoning_effort: reasoning_effort_of(settings),
        }
    }
}

fn reasoning_effort_of(settings: &Value) -> Option<String> {
    settings
        .get(REASONING_EFFORT_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn env_string(env: Option<&Value>, key: &str) -> Option<String> {
    env?.get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn invalid(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> AppError {
    AppError::localized(key, zh, en)
}

/// 卡片 id 就是 `providers.json` 里的供应商名，也是 `/model` 里 `<id>/<模型>` 的前缀：
/// 非空、不含 `/`、空白和控制字符，不超过 64 个字符。
pub(crate) fn validate_card_id(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.contains('/')
        || id.chars().any(|c| c.is_whitespace() || c.is_control())
        || id.chars().count() > MAX_CARD_ID_CHARS
    {
        return Err(invalid(
            "copilot.validation.invalid_provider_id",
            format!("供应商 ID「{id}」不可用：不能为空，不能含 / 、空白或控制字符，最多 {MAX_CARD_ID_CHARS} 个字符"),
            format!("Provider ID \"{id}\" is invalid: it must be non-empty, without /, whitespace or control characters, and at most {MAX_CARD_ID_CHARS} characters"),
        ));
    }
    Ok(())
}

fn validate_reasoning_effort(settings: &Map<String, Value>) -> Result<(), AppError> {
    if let Some(value) = settings.get(REASONING_EFFORT_KEY) {
        let valid = value.is_null()
            || value
                .as_str()
                .map(str::trim)
                .is_some_and(|text| text.is_empty() || REASONING_EFFORTS.contains(&text));
        if !valid {
            return Err(invalid(
                "copilot.validation.invalid_reasoning_effort",
                format!("推理强度只能是 {}", REASONING_EFFORTS.join(" / ")),
                format!(
                    "Reasoning effort must be one of {}",
                    REASONING_EFFORTS.join(" / ")
                ),
            ));
        }
    }
    Ok(())
}

/// 保存前校验。自带模型卡（有 `provider` 节点）按 Copilot 的条目规则校验；官方卡只有
/// `env.COPILOT_MODEL` 和推理强度。旧版的 `COPILOT_PROVIDER_*` 环境变量形状仍然放行
/// （列表读库时会转成新形状）。
pub(crate) fn validate_provider(id: &str, settings: &Value) -> Result<(), AppError> {
    let obj = settings.as_object().ok_or_else(|| {
        invalid(
            "copilot.validation.not_object",
            "GitHub Copilot CLI 配置必须是 JSON 对象",
            "GitHub Copilot CLI configuration must be a JSON object",
        )
    })?;
    validate_reasoning_effort(obj)?;
    if obj.contains_key("provider") {
        return validate_card(id, settings);
    }
    if let Some(env) = obj.get("env") {
        let env = env.as_object().ok_or_else(|| {
            invalid(
                "copilot.validation.env_not_object",
                "GitHub Copilot CLI 的 env 必须是对象",
                "GitHub Copilot CLI env must be an object",
            )
        })?;
        for (key, value) in env {
            if value.is_null() {
                continue;
            }
            let Some(text) = value.as_str() else {
                return Err(invalid(
                    "copilot.validation.env_not_string",
                    format!("GitHub Copilot CLI 环境变量 {key} 必须是字符串"),
                    format!("GitHub Copilot CLI environment variable {key} must be a string"),
                ));
            };
            let allows_newlines = key == "COPILOT_PROVIDER_HEADERS";
            if text
                .chars()
                .any(|c| c.is_control() && !(allows_newlines && matches!(c, '\n' | '\r')))
            {
                return Err(invalid(
                    "copilot.validation.env_multiline",
                    format!("GitHub Copilot CLI 环境变量 {key} 不能包含控制字符"),
                    format!("GitHub Copilot CLI environment variable {key} must not contain control characters"),
                ));
            }
        }
    }
    Ok(())
}

/// 自带模型卡：卡片 id、地址、至少一个模型，再把要写进文件的条目按 Copilot 的规则校验一遍。
fn validate_card(id: &str, settings: &Value) -> Result<(), AppError> {
    validate_card_id(id)?;
    let provider = settings
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid(
                "copilot.validation.provider_not_object",
                "自带模型的 provider 必须是对象",
                "The bring-your-own-model provider must be an object",
            )
        })?;
    let base_url = provider
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if url::Url::parse(base_url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .is_none()
    {
        return Err(invalid(
            "copilot.validation.invalid_base_url",
            format!("接口地址不是有效的 http(s) 地址: {base_url}"),
            format!("The endpoint is not a valid http(s) URL: {base_url}"),
        ));
    }
    let models = settings.get("models").and_then(Value::as_array);
    if models.is_none_or(|models| models.is_empty()) {
        return Err(invalid(
            "copilot.validation.missing_model",
            "自带模型至少要有一个模型",
            "Bring-your-own-model providers need at least one model",
        ));
    }
    let document = json!({
        "providers": [provider_entry(id, settings)?],
        "models": model_entries(id, settings)?,
    });
    // 卡片里的字符串（含请求头的名和值）不许有控制字符：模型 id 会拼进「打开终端」的启动
    // 命令（Windows 是 .bat，换行就成了另一条命令），Key 和请求头里的换行也只会让请求坏掉。
    if let Some(path) = first_control_char(&document) {
        return Err(invalid(
            "copilot.validation.control_chars",
            format!("{path} 不能包含换行等控制字符"),
            format!("{path} must not contain control characters such as line breaks"),
        ));
    }
    validate_document(&document, Some(id))
}

/// 找第一个含控制字符的字符串（对象的键也算），返回它的位置，如 `models[0].id`。
fn first_control_char(value: &Value) -> Option<String> {
    fn walk(value: &Value, path: &str) -> Option<String> {
        let bad = |text: &str| text.chars().any(char::is_control);
        match value {
            Value::String(text) if bad(text) => Some(path.to_string()),
            Value::Array(items) => items
                .iter()
                .enumerate()
                .find_map(|(index, item)| walk(item, &format!("{path}[{index}]"))),
            Value::Object(map) => map.iter().find_map(|(key, item)| {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if bad(key) {
                    Some(child)
                } else {
                    walk(item, &child)
                }
            }),
            _ => None,
        }
    }
    walk(value, "")
}

/// 卡片的 `provider` 节点加上 `name` = 卡片 id（放在最前面）。
fn provider_entry(id: &str, settings: &Value) -> Result<Value, AppError> {
    let provider = settings
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid(
                "copilot.validation.provider_not_object",
                "自带模型的 provider 必须是对象",
                "The bring-your-own-model provider must be an object",
            )
        })?;
    let mut entry = Map::new();
    entry.insert("name".into(), json!(id));
    for (key, value) in provider {
        if key != "name" {
            entry.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(entry))
}

/// 卡片的模型加上 `provider` = 卡片 id（紧跟在 `id` 后面）。
fn model_entries(id: &str, settings: &Value) -> Result<Vec<Value>, AppError> {
    let Some(models) = settings.get("models") else {
        return Ok(Vec::new());
    };
    let models = models.as_array().ok_or_else(|| {
        invalid(
            "copilot.validation.models_not_array",
            "自带模型的 models 必须是数组",
            "The bring-your-own-model models must be an array",
        )
    })?;
    models
        .iter()
        .map(|model| {
            let model = model.as_object().ok_or_else(|| {
                invalid(
                    "copilot.validation.model_not_object",
                    "每个模型都必须是对象",
                    "Each model must be an object",
                )
            })?;
            let mut entry = Map::new();
            if let Some(model_id) = model.get("id") {
                entry.insert("id".into(), model_id.clone());
            }
            entry.insert("provider".into(), json!(id));
            for (key, value) in model {
                if key != "id" && key != "provider" {
                    entry.insert(key.clone(), value.clone());
                }
            }
            Ok(Value::Object(entry))
        })
        .collect()
}

fn document_error(zh: String, en: String) -> AppError {
    invalid("copilot.providers.invalid", zh, en)
}

fn check_enum(
    entry: &Map<String, Value>,
    where_: &str,
    key: &str,
    allowed: &[&str],
) -> Result<(), AppError> {
    match entry.get(key) {
        None => Ok(()),
        Some(value) if value.as_str().is_some_and(|text| allowed.contains(&text)) => Ok(()),
        Some(_) => Err(document_error(
            format!("{where_} 的 {key} 只能是 {}", allowed.join(" / ")),
            format!("{key} of {where_} must be one of {}", allowed.join(" / ")),
        )),
    }
}

fn check_kind(
    entry: &Map<String, Value>,
    where_: &str,
    key: &str,
    expected: &str,
    ok: impl Fn(&Value) -> bool,
) -> Result<(), AppError> {
    match entry.get(key) {
        Some(value) if !ok(value) => Err(document_error(
            format!("{where_} 的 {key} 必须是{expected}"),
            format!("{key} of {where_} must be {expected}"),
        )),
        _ => Ok(()),
    }
}

fn check_unknown_keys(
    entry: &Map<String, Value>,
    where_: &str,
    allowed: &[&str],
) -> Result<(), AppError> {
    if let Some(key) = entry.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(document_error(
            format!("{where_} 有 Copilot 不认的字段「{key}」（会导致 Copilot 无法启动）"),
            format!("{where_} has a field Copilot does not accept: \"{key}\" (Copilot would fail to start)"),
        ));
    }
    Ok(())
}

fn validate_system_message(message: &Map<String, Value>, model: &str) -> Result<(), AppError> {
    let path = format!("{model}.systemMessage");
    let mode = match message.get("mode") {
        None | Some(Value::Null) => "append",
        Some(value) => value.as_str().unwrap_or(""),
    };
    let allowed: &[&str] = match mode {
        "append" | "replace" => &["mode", "content"],
        "customize" => &["mode", "content", "sections"],
        _ => {
            return Err(document_error(
                format!("{path}.mode 必须是 append、replace 或 customize"),
                format!("{path}.mode must be append, replace, or customize"),
            ))
        }
    };
    check_unknown_keys(message, &path, allowed)?;
    if mode == "replace" && !message.get("content").is_some_and(Value::is_string) {
        return Err(document_error(
            format!("{path}.content 在 replace 模式下必须是字符串"),
            format!("{path}.content must be a string in replace mode"),
        ));
    }
    check_kind(message, &path, "content", "a string", |v| {
        v.is_null() || v.is_string()
    })?;
    let Some(sections) = message.get("sections").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let sections = sections.as_object().ok_or_else(|| {
        document_error(
            format!("{path}.sections 必须是对象"),
            format!("{path}.sections must be an object"),
        )
    })?;
    for (name, section) in sections {
        let path = format!("{path}.sections.{name}");
        let section = section.as_object().ok_or_else(|| {
            document_error(
                format!("{path} 必须是对象"),
                format!("{path} must be an object"),
            )
        })?;
        check_unknown_keys(section, &path, &["action", "content"])?;
        if !section.contains_key("action") {
            return Err(document_error(
                format!("{path} 缺少 action"),
                format!("{path} is missing action"),
            ));
        }
        check_enum(
            section,
            &path,
            "action",
            &["append", "prepend", "replace", "remove"],
        )?;
        check_kind(section, &path, "content", "a string", |v| {
            v.is_null() || v.is_string()
        })?;
    }
    Ok(())
}

/// 按 Copilot 自己的规则校验整份 `providers.json`：供应商名必填、唯一、不含 `/`；模型的
/// `provider` 必须引用存在的供应商，同一供应商下模型 id 唯一；websockets 只能配 responses；
/// token 上限是 0..=u32::MAX 的整数（Copilot 1.0.107 实测：`128000.5` 整个起不来，`0` 和
/// `128000.0` 都认）。这些违反了 Copilot 会整个起不来，所以对整份文档都查。
///
/// 未知字段 Copilot 运行时并不拒绝（实测），别人（Copilot 自己、用户、更新版本的 Copilot）
/// 写的条目里有我们不认识的字段不能挡住写入；只对 `own`（CC Switch 这张卡）的条目查，
/// 免得把拼错的字段写进去。
pub(crate) fn validate_document(document: &Value, own: Option<&str>) -> Result<(), AppError> {
    let obj = document.as_object().ok_or_else(|| {
        document_error(
            "providers.json 必须是 JSON 对象".into(),
            "providers.json must be a JSON object".into(),
        )
    })?;
    let empty = Vec::new();
    let providers = match obj.get("providers") {
        None => &empty,
        Some(value) => value.as_array().ok_or_else(|| {
            document_error(
                "providers.json 的 providers 必须是数组".into(),
                "providers in providers.json must be an array".into(),
            )
        })?,
    };
    let models = match obj.get("models") {
        None => &empty,
        Some(value) => value.as_array().ok_or_else(|| {
            document_error(
                "providers.json 的 models 必须是数组".into(),
                "models in providers.json must be an array".into(),
            )
        })?,
    };

    let mut names = HashSet::new();
    for (index, entry) in providers.iter().enumerate() {
        let where_ = format!("providers[{index}]");
        let entry = entry.as_object().ok_or_else(|| {
            document_error(
                format!("{where_} 必须是对象"),
                format!("{where_} must be an object"),
            )
        })?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                document_error(
                    format!("{where_} 缺少 name"),
                    format!("{where_} is missing name"),
                )
            })?;
        let where_ = format!("供应商「{name}」");
        let where_en = format!("provider \"{name}\"");
        if own == Some(name) {
            check_unknown_keys(entry, &where_, PROVIDER_KEYS)?;
        }
        if name.contains('/') {
            return Err(document_error(
                format!("{where_} 的名字不能含 /"),
                format!("The name of {where_en} must not contain /"),
            ));
        }
        if !names.insert(name.to_string()) {
            return Err(document_error(
                format!("{where_} 重名"),
                format!("Duplicate {where_en}"),
            ));
        }
        check_enum(entry, &where_en, "type", &["openai", "azure", "anthropic"])?;
        check_enum(entry, &where_en, "wireApi", &["completions", "responses"])?;
        check_enum(entry, &where_en, "transport", &["http", "websockets"])?;
        if entry.get("transport").and_then(Value::as_str) == Some("websockets")
            && entry.get("wireApi").and_then(Value::as_str) != Some("responses")
        {
            return Err(document_error(
                format!("{where_} 用 websockets 时 wireApi 必须是 responses"),
                format!("{where_en} uses websockets, which needs wireApi \"responses\""),
            ));
        }
        if entry
            .get("baseUrl")
            .and_then(Value::as_str)
            .is_none_or(|url| url.trim().is_empty())
        {
            return Err(document_error(
                format!("{where_} 缺少 baseUrl"),
                format!("{where_en} is missing baseUrl"),
            ));
        }
        for key in ["apiKey", "bearerToken", "modelProvider"] {
            check_kind(entry, &where_en, key, "a string", Value::is_string)?;
        }
        check_kind(
            entry,
            &where_en,
            "hasBearerTokenProvider",
            "a boolean",
            Value::is_boolean,
        )?;
        check_kind(entry, &where_en, "azure", "an object", Value::is_object)?;
        check_kind(entry, &where_en, "headers", "an object of strings", |v| {
            v.as_object()
                .is_some_and(|headers| headers.values().all(Value::is_string))
        })?;
    }

    let mut model_ids = HashSet::new();
    for (index, entry) in models.iter().enumerate() {
        let where_ = format!("models[{index}]");
        let entry = entry.as_object().ok_or_else(|| {
            document_error(
                format!("{where_} 必须是对象"),
                format!("{where_} must be an object"),
            )
        })?;
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                document_error(
                    format!("{where_} 缺少 id"),
                    format!("{where_} is missing id"),
                )
            })?;
        let provider = entry
            .get("provider")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                document_error(
                    format!("模型「{id}」缺少 provider"),
                    format!("Model \"{id}\" is missing provider"),
                )
            })?;
        if !names.contains(provider) {
            return Err(document_error(
                format!("模型「{id}」引用了不存在的供应商「{provider}」"),
                format!("Model \"{id}\" refers to a provider that does not exist: \"{provider}\""),
            ));
        }
        if !model_ids.insert((provider.to_string(), id.to_string())) {
            return Err(document_error(
                format!("供应商「{provider}」下的模型「{id}」重复"),
                format!("Duplicate model \"{id}\" under provider \"{provider}\""),
            ));
        }
        let where_en = format!("model \"{provider}/{id}\"");
        if own == Some(provider) {
            check_unknown_keys(entry, &where_, MODEL_KEYS)?;
        }
        for key in ["wireModel", "modelId", "name"] {
            check_kind(entry, &where_en, key, "a string", Value::is_string)?;
        }
        for key in [
            "maxPromptTokens",
            "maxContextWindowTokens",
            "maxOutputTokens",
        ] {
            check_kind(
                entry,
                &where_en,
                key,
                "a whole number from 0 to 4294967295",
                |v| {
                    v.as_f64().is_some_and(|n| {
                        n.is_finite() && n.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&n)
                    })
                },
            )?;
        }
        for key in ["capabilities", "systemMessage", "metadata"] {
            check_kind(entry, &where_en, key, "an object", Value::is_object)?;
        }
        if own == Some(provider) {
            if let (Some(context), Some(output)) = (
                entry.get("maxContextWindowTokens").and_then(Value::as_f64),
                entry.get("maxOutputTokens").and_then(Value::as_f64),
            ) {
                if output >= context {
                    return Err(document_error(
                        format!("{where_en} 的最大输出必须小于上下文窗口，才能为输入留出空间"),
                        format!("maxOutputTokens of {where_en} must be smaller than maxContextWindowTokens to leave room for input"),
                    ));
                }
            }
            if let Some(message) = entry.get("systemMessage").and_then(Value::as_object) {
                validate_system_message(message, &where_en)?;
            }
        }
    }
    Ok(())
}

/// 读 `providers.json`：不存在或空文件是空文档；必须是严格 JSON（Copilot 不认注释），
/// 顶层是对象，`providers` / `models` 有的话是数组。
pub(crate) fn read_providers(path: &Path) -> Result<Value, AppError> {
    parse_providers(path, read_raw(path)?.as_deref())
}

/// 文件原始字节；不存在是 `None`。
fn read_raw(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

/// 空文件 Copilot 当作没有供应商（1.0.107 实测），这里一样当空文档。
fn parse_providers(path: &Path, raw: Option<&[u8]>) -> Result<Value, AppError> {
    let Some(raw) = raw else {
        return Ok(json!({}));
    };
    let text = std::str::from_utf8(raw).map_err(|e| {
        invalid(
            "copilot.providers.invalid_json",
            format!("{} 不是有效的 UTF-8：{e}", path.display()),
            format!("{} is not valid UTF-8: {e}", path.display()),
        )
    })?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| {
        invalid(
            "copilot.providers.invalid_json",
            format!("{} 不是有效的 JSON：{e}", path.display()),
            format!("{} is not valid JSON: {e}", path.display()),
        )
    })?;
    let valid_shape = value.is_object()
        && ["providers", "models"]
            .iter()
            .all(|key| value.get(key).is_none_or(Value::is_array));
    if !valid_shape {
        return Err(invalid(
            "copilot.providers.invalid_shape",
            format!(
                "{} 必须是对象，providers 和 models 必须是数组",
                path.display()
            ),
            format!(
                "{} must be an object whose providers and models are arrays",
                path.display()
            ),
        ));
    }
    Ok(value)
}

fn provider_names(document: &Value) -> HashSet<String> {
    document
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// 文件里的供应商，按卡片形状（`provider` 去掉 `name`，`models` 去掉 `provider`）。
fn providers_in(document: &Value) -> IndexMap<String, Value> {
    let models = document
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    document
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?;
            let mut provider = entry.as_object()?.clone();
            provider.shift_remove("name");
            let models: Vec<Value> = models
                .iter()
                .filter(|model| model.get("provider").and_then(Value::as_str) == Some(name))
                .filter_map(|model| {
                    let mut model = model.as_object()?.clone();
                    model.shift_remove("provider");
                    Some(Value::Object(model))
                })
                .collect();
            Some((
                name.to_string(),
                json!({"provider": provider, "models": models}),
            ))
        })
        .collect()
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    Ok(providers_in(&read_providers(&providers_path())?))
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(provider_names(&read_providers(&providers_path())?).contains(id))
}

/// `/model` 选择 id 的供应商部分：`<供应商>/<模型>` 的 `<供应商>`（供应商名不含 `/`）。
pub(crate) fn selected_provider_name(model: &str) -> Option<&str> {
    model
        .split_once('/')
        .map(|(name, _)| name)
        .filter(|name| !name.is_empty())
}

/// Copilot `settings.json` 里选中的模型（`model`）。
fn selected_model() -> Result<Option<String>, AppError> {
    Ok(read_settings_document()?.and_then(|settings| {
        settings
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string)
    }))
}

/// 选中模型属于哪家（只认 `providers.json` 里有的）。
fn current_provider_in(selected: Option<&str>, document: &Value) -> Option<String> {
    let name = selected_provider_name(selected?)?;
    provider_names(document)
        .contains(name)
        .then(|| name.to_string())
}

/// Copilot 当前默认的自带模型供应商：`settings.json` 的 `model` 是 `<id>/...` 且 `<id>`
/// 在 `providers.json` 里，否则空串。
pub(crate) fn current_provider() -> Result<String, AppError> {
    let selected = selected_model()?;
    let document = read_providers(&providers_path())?;
    Ok(current_provider_in(selected.as_deref(), &document).unwrap_or_default())
}

/// 正在被 Copilot 用作默认模型的供应商不能移除，删掉它选中的那个模型也不行：Copilot 会
/// 指向一个不存在的模型。
fn check_default(
    selected: Option<&str>,
    before: &Value,
    id: &str,
    settings: Option<&Value>,
) -> Result<(), AppError> {
    if current_provider_in(selected, before).as_deref() != Some(id) {
        return Ok(());
    }
    let model = selected
        .and_then(|selected| selected.split_once('/'))
        .map(|(_, model)| model)
        .unwrap_or_default();
    let kept = settings
        .and_then(|settings| settings.get("models"))
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
        });
    if kept {
        return Ok(());
    }
    if settings.is_some() {
        return Err(invalid(
            "copilot.provider.default_model_cannot_drop",
            format!("「{id}/{model}」是 GitHub Copilot CLI 当前的默认模型，不能从这张卡里去掉；先在 Copilot 里用 /model 换一个再改"),
            format!("\"{id}/{model}\" is GitHub Copilot CLI's current default model and cannot be dropped from this card. Select another model with /model in Copilot first"),
        ));
    }
    Err(invalid(
        "copilot.provider.default_cannot_remove",
        format!("「{id}」是 GitHub Copilot CLI 当前的默认模型，先在 Copilot 里用 /model 换一个再移除"),
        format!("\"{id}\" is GitHub Copilot CLI's current default model. Select another model with /model in Copilot before removing it"),
    ))
}

/// 只改这一家：供应商条目原位替换（新建追加到末尾），它的模型整体替换、放回原先第一个
/// 模型的位置；`settings` 为 `None` 时删掉这家和它的模型。其余条目、顶层键、顺序不动。
/// 结果按 Copilot 的规则校验整份文档，不过就报错。
fn apply_provider(
    before: &Value,
    id: &str,
    settings: Option<&Value>,
    create: bool,
) -> Result<Value, AppError> {
    let mut document = before.clone();
    let obj = document.as_object_mut().ok_or_else(|| {
        document_error(
            "providers.json 必须是 JSON 对象".into(),
            "providers.json must be a JSON object".into(),
        )
    })?;
    let position = obj
        .get("providers")
        .and_then(Value::as_array)
        .and_then(|providers| {
            providers
                .iter()
                .position(|entry| entry.get("name").and_then(Value::as_str) == Some(id))
        });
    let belongs = |model: &Value| model.get("provider").and_then(Value::as_str) == Some(id);

    let Some(settings) = settings else {
        let Some(position) = position else {
            return Ok(before.clone());
        };
        if let Some(providers) = obj.get_mut("providers").and_then(Value::as_array_mut) {
            providers.remove(position);
        }
        if let Some(models) = obj.get_mut("models").and_then(Value::as_array_mut) {
            models.retain(|model| !belongs(model));
        }
        validate_document(&document, None)?;
        return Ok(document);
    };

    validate_card(id, settings)?;
    if create && position.is_some() {
        return Err(invalid(
            "copilot.provider.exists",
            format!("GitHub Copilot CLI 的 providers.json 里已经有「{id}」"),
            format!("GitHub Copilot CLI providers.json already has \"{id}\""),
        ));
    }
    let entry = provider_entry(id, settings)?;
    let new_models = model_entries(id, settings)?;

    let providers = obj
        .entry("providers")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            document_error(
                "providers.json 的 providers 必须是数组".into(),
                "providers in providers.json must be an array".into(),
            )
        })?;
    match position {
        Some(position) => providers[position] = entry,
        None => providers.push(entry),
    }

    let models = obj
        .entry("models")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            document_error(
                "providers.json 的 models 必须是数组".into(),
                "models in providers.json must be an array".into(),
            )
        })?;
    // 第一个属于这家的模型之前都是别家的，删掉这家的之后下标仍然对得上。
    let first = models.iter().position(belongs).unwrap_or(models.len());
    models.retain(|model| !belongs(model));
    let first = first.min(models.len());
    models.splice(first..first, new_models);

    validate_document(&document, Some(id))?;
    Ok(document)
}

fn external_change(path: &Path) -> AppError {
    invalid(
        "copilot.providers.changed",
        format!(
            "{} 刚被其他程序（可能是 Copilot）改过，请重试",
            path.display()
        ),
        format!(
            "{} was just changed by another program (possibly Copilot). Please try again",
            path.display()
        ),
    )
}

/// 改 `providers.json` 的唯一入口。Copilot 自己也写这个文件，进程间没有锁可用，所以：
/// 1. 读一次原始字节，`compute` 基于它算出新文档（`None` 表示不用改）；
/// 2. 先把新内容写进临时文件（0600），紧挨着替换之前再读一次，字节变了就放弃
///    （不拿旧快照盖掉别人刚写的内容）；
/// 3. 写后重读比对；
/// 4. `commit`（存库）失败时，只有文件仍是我们写的那份才还原，否则保留别人的改动。
///
/// 没有和 Copilot 共用的锁，「最后一次比对 → rename」之间仍有极短的窗口，这里只是尽力检测。
fn transact<T>(
    path: &Path,
    compute: impl FnOnce(&Value) -> Result<Option<Value>, AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let original = read_raw(path)?;
    let before = parse_providers(path, original.as_deref())?;
    let Some(after) = compute(&before)? else {
        return commit();
    };
    let mut text =
        serde_json::to_string_pretty(&after).map_err(|e| AppError::Config(e.to_string()))?;
    text.push('\n');
    let staged = stage_write(path, text.as_bytes(), Some(0o600), false)?;
    // 软链接指向的文件：首次创建失败要回滚时删它，链接本身保留。
    let target = staged.target().to_path_buf();
    if read_raw(path)? != original {
        staged.discard();
        return Err(external_change(path));
    }
    staged.commit()?;
    if read_providers(path)? != after {
        return Err(external_change(path));
    }
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            let restore = match original
                .as_deref()
                .map(|bytes| stage_write(path, bytes, Some(0o600), false))
                .transpose()
            {
                Ok(restore) => restore,
                Err(stage_error) => {
                    return Err(AppError::Message(format!(
                        "Config update failed ({error}); restoring {} also failed: {stage_error}",
                        path.display()
                    )))
                }
            };
            if read_raw(path)?.as_deref() != Some(text.as_bytes()) {
                if let Some(restore) = restore {
                    restore.discard();
                }
                log::warn!(
                    "{} changed after CC Switch wrote it; not rolling back",
                    path.display()
                );
                return Err(error);
            }
            let rollback = match restore {
                Some(restore) => restore.commit(),
                None => fs::remove_file(&target).map_err(|e| AppError::io(&target, e)),
            };
            if let Err(rollback_error) = rollback {
                return Err(AppError::Message(format!(
                    "Config update failed ({error}); restoring {} also failed: {rollback_error}",
                    path.display()
                )));
            }
            Err(error)
        }
    }
}

/// 测试用：不经全局锁和路径解析，直接对 `path` 做一次 [`save_and_commit`] 的写入。
#[cfg(test)]
fn update_at(
    path: &Path,
    selected: Option<&str>,
    id: &str,
    settings: Option<&Value>,
    create: bool,
) -> Result<(), AppError> {
    transact(
        path,
        |before| {
            check_default(selected, before, id, settings)?;
            let after = apply_provider(before, id, settings, create)?;
            Ok((after != *before).then_some(after))
        },
        || Ok(()),
    )
}

/// 写这一家（`settings` 为 `None` 时移除），成功后执行 `commit`（存库）；`commit` 失败时
/// 把文件还原（见 [`transact`]）。
pub(crate) fn save_and_commit<T>(
    id: &str,
    settings: Option<&Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = providers_path();
    transact(
        &path,
        |before| {
            // 只有这家在文件里时才可能是默认：不在时不读 settings.json（它坏了也不挡删除
            // 未添加的卡）。
            let selected = if provider_names(before).contains(id) {
                selected_model()?
            } else {
                None
            };
            check_default(selected.as_deref(), before, id, settings)?;
            let after = apply_provider(before, id, settings, create)?;
            Ok((after != *before).then_some(after))
        },
        commit,
    )
}

/// 「添加」：文件里还没有这家时写进去；已经有了就是 Copilot 或用户的，原样保留（同 Kimi）。
pub(crate) fn add_and_commit<T>(
    id: &str,
    settings: &Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = providers_path();
    transact(
        &path,
        |before| {
            if provider_names(before).contains(id) {
                return Ok(None);
            }
            apply_provider(before, id, Some(settings), true).map(Some)
        },
        commit,
    )
}

/// 旧版（PR 期间）自带模型卡的 `{env: {COPILOT_PROVIDER_*, COPILOT_MODEL}}` 转成新形状；
/// 不是旧形状时返回 `None`。只转形状，不写文件。
pub(crate) fn convert_legacy_settings(settings: &Value) -> Option<Value> {
    if is_byok_settings(settings) {
        return None;
    }
    let env = settings.get("env")?;
    let get = |key: &str| env_string(Some(env), key);
    let base_url = get(BASE_URL_ENV)?;

    let mut provider = Map::new();
    for (env_key, key) in [
        ("COPILOT_PROVIDER_TYPE", "type"),
        ("COPILOT_PROVIDER_WIRE_API", "wireApi"),
        ("COPILOT_PROVIDER_TRANSPORT", "transport"),
    ] {
        if let Some(value) = get(env_key) {
            provider.insert(key.into(), json!(value));
        }
    }
    provider.insert("baseUrl".into(), json!(base_url));
    if let Some(value) = get(API_KEY_ENV) {
        provider.insert("apiKey".into(), json!(value));
    }
    if let Some(value) = get(BEARER_TOKEN_ENV) {
        provider.insert("bearerToken".into(), json!(value));
    }
    if let Some(version) = get("COPILOT_PROVIDER_AZURE_API_VERSION") {
        provider.insert("azure".into(), json!({"apiVersion": version}));
    }
    if let Some(text) = get("COPILOT_PROVIDER_HEADERS") {
        let headers: Map<String, Value> = text
            .replace("\\n", "\n")
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                let name = name.trim();
                (!name.is_empty()).then(|| (name.to_string(), json!(value.trim())))
            })
            .collect();
        if !headers.is_empty() {
            provider.insert("headers".into(), Value::Object(headers));
        }
    }

    let model_id = get(MODEL_ENV);
    let wire_model = get("COPILOT_PROVIDER_WIRE_MODEL");
    let native_id = get("COPILOT_PROVIDER_MODEL_ID");
    let mut models = Vec::new();
    if let Some(id) = model_id
        .clone()
        .or_else(|| native_id.clone())
        .or_else(|| wire_model.clone())
    {
        let mut model = Map::new();
        model.insert("id".into(), json!(id));
        if let Some(wire) = wire_model.filter(|wire| *wire != id) {
            model.insert("wireModel".into(), json!(wire));
        }
        if let Some(native) = native_id.filter(|native| *native != id) {
            model.insert("modelId".into(), json!(native));
        }
        for (env_key, key) in [
            ("COPILOT_PROVIDER_MAX_PROMPT_TOKENS", "maxPromptTokens"),
            ("COPILOT_PROVIDER_MAX_OUTPUT_TOKENS", "maxOutputTokens"),
        ] {
            if let Some(n) = get(env_key).and_then(|text| text.parse::<u64>().ok()) {
                model.insert(key.into(), json!(n));
            }
        }
        models.push(Value::Object(model));
    }

    let mut converted = Map::new();
    converted.insert("provider".into(), Value::Object(provider));
    converted.insert("models".into(), Value::Array(models));
    if let Some(obj) = settings.as_object() {
        for (key, value) in obj {
            if key != "env" {
                converted.insert(key.clone(), value.clone());
            }
        }
    }
    Some(Value::Object(converted))
}

/// 在终端里启动这张卡要用的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopilotLaunch {
    /// `--model`：官方卡是卡上设的模型；自带模型卡是 `<卡片 id>/<第一个模型 id>`。
    pub model: Option<String>,
    /// 只给这一个 copilot 进程的环境变量；`None` 表示要清掉（用户 rc 里可能 export 过）。
    pub env: Vec<(&'static str, Option<String>)>,
    pub byok: bool,
    /// `--reasoning-effort`；没设时用 Copilot 自己的默认值。
    pub reasoning_effort: Option<String>,
}

/// 准备在终端里启动某张卡（自带模型卡要先确认已经添加进 `providers.json`，见
/// `copilot_direct::terminal_launch`）。两种卡都清掉所有 `COPILOT_PROVIDER_*`：rc 里 export
/// 的 BYOK 变量会让官方卡悄悄变成自带模型，也会让 Copilot 完全忽略 `providers.json`。
pub(crate) fn prepare_launch(id: &str, settings: &Value) -> Result<CopilotLaunch, AppError> {
    validate_provider(id, settings)?;
    let mut env: Vec<(&'static str, Option<String>)> =
        BYOK_ENV_KEYS.iter().map(|key| (*key, None)).collect();
    if !is_byok_settings(settings) {
        let projection = CopilotProjection::of(settings);
        return Ok(CopilotLaunch {
            model: projection.model,
            env,
            byok: false,
            reasoning_effort: projection.reasoning_effort,
        });
    }
    let model = settings
        .pointer("/models/0/id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                "copilot.validation.missing_model",
                "自带模型至少要有一个模型",
                "Bring-your-own-model providers need at least one model",
            )
        })?;
    env.push((MODEL_ENV, None));
    Ok(CopilotLaunch {
        model: Some(format!("{id}/{model}")),
        env,
        byok: true,
        reasoning_effort: reasoning_effort_of(settings),
    })
}

/// 会话页的恢复命令。
pub fn resume_command(session_id: &str) -> String {
    format!("copilot --resume={session_id}")
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsString;
    use std::path::Path;

    /// 把 `COPILOT_HOME` 指到临时目录、清掉 `COPILOT_PROVIDERS_CONFIG`，析构时还原。
    /// 和其它改环境变量的测试一样要配合 `#[serial]`。
    pub(crate) struct CopilotHomeGuard {
        home: Option<OsString>,
        providers: Option<OsString>,
    }

    impl CopilotHomeGuard {
        pub(crate) fn new(dir: &Path) -> Self {
            let guard = Self {
                home: std::env::var_os("COPILOT_HOME"),
                providers: std::env::var_os(super::PROVIDERS_CONFIG_ENV),
            };
            std::env::set_var("COPILOT_HOME", dir);
            std::env::remove_var(super::PROVIDERS_CONFIG_ENV);
            guard
        }
    }

    impl Drop for CopilotHomeGuard {
        fn drop(&mut self) {
            match self.home.take() {
                Some(value) => std::env::set_var("COPILOT_HOME", value),
                None => std::env::remove_var("COPILOT_HOME"),
            }
            match self.providers.take() {
                Some(value) => std::env::set_var(super::PROVIDERS_CONFIG_ENV, value),
                None => std::env::remove_var(super::PROVIDERS_CONFIG_ENV),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn card() -> Value {
        json!({
            "provider": {"type":"openai","wireApi":"completions","baseUrl":"https://api.example.test/v1","apiKey":"sk-fixture","headers":{"X-A":"1"}},
            "models": [
                {"id":"m1","name":"Model 1","maxContextWindowTokens":128000,"maxOutputTokens":8192},
                {"id":"m2"}
            ],
            "reasoningEffort": "high"
        })
    }

    fn foreign_document() -> Value {
        json!({
            "providers": [
                {"name":"foundry","type":"openai","baseUrl":"http://localhost:5272/v1"},
                {"name":"other","type":"anthropic","baseUrl":"https://other.test"}
            ],
            "models": [
                {"id":"phi","provider":"foundry"},
                {"id":"o1","provider":"other"}
            ],
            "futureTopLevel": {"keep": true}
        })
    }

    fn write(path: &Path, value: &Value) {
        fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    #[test]
    fn foreign_entries_with_unknown_fields_or_whole_float_limits_do_not_block_writes() {
        // Copilot 1.0.107 实测：未知字段、0、128000.0 都能启动；128000.5 起不来。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        let mut doc = foreign_document();
        doc["providers"][0]["futureField"] = json!({"x": 1});
        doc["models"][0]["futureModelField"] = json!(true);
        doc["models"][0]["maxContextWindowTokens"] = json!(0);
        doc["models"][1]["maxOutputTokens"] = json!(128000.0);
        write(&path, &doc);
        update_at(&path, None, "deepseek", Some(&card()), true).unwrap();
        let after = read_providers(&path).unwrap();
        assert_eq!(after["providers"][0]["futureField"], json!({"x": 1}));

        doc["models"][1]["maxOutputTokens"] = json!(128000.5);
        write(&path, &doc);
        assert!(update_at(&path, None, "deepseek", Some(&card()), true).is_err());
        assert_eq!(read_providers(&path).unwrap(), doc);
    }

    #[test]
    fn reject_unusable_limits_and_system_messages_before_writing_a_card() {
        let mut settings = card();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        let original = foreign_document();
        write(&path, &original);
        for message in [
            json!({"mode":"unexpected"}),
            json!({"mode":"replace"}),
            json!({"mode":"replace","content":5}),
            json!({"mode":"append","sections":{}}),
            json!({"mode":"customize","sections":{"identity":{"action":"transform"}}}),
        ] {
            settings["models"][0]["systemMessage"] = message;
            assert!(update_at(&path, None, "mine", Some(&settings), true).is_err());
            assert_eq!(read_providers(&path).unwrap(), original);
        }
        for message in [
            json!({}),
            json!({"mode":"replace","content":""}),
            json!({"mode":"customize","sections":{"identity":{"action":"prepend","content":"hello"}}}),
        ] {
            settings["models"][0]["systemMessage"] = message;
            validate_provider("mine", &settings).unwrap();
        }
        settings["models"][0]["maxOutputTokens"] = json!(262144);
        settings["models"][0]["maxContextWindowTokens"] = json!(262144);
        assert!(validate_provider("mine", &settings).is_err());
    }

    #[test]
    fn control_characters_in_card_strings_are_rejected() {
        let mut model = card();
        model["models"][0]["id"] = json!("m1\r\ncalc");
        assert!(validate_provider("x", &model).is_err());
        let mut header = card();
        header["provider"]["headers"] = json!({"X-A": "1\nX-B: 2"});
        assert!(validate_provider("x", &header).is_err());
        let mut key = card();
        key["provider"]["apiKey"] = json!("sk\u{0007}");
        assert!(validate_provider("x", &key).is_err());
    }

    #[test]
    fn a_change_made_between_read_and_write_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        write(&path, &foreign_document());
        let mut changed = foreign_document();
        changed["providers"][0]["baseUrl"] = json!("http://localhost:9999/v1");
        let error = transact(
            &path,
            |before| {
                // 模拟 Copilot 在 CC Switch 读完之后写了文件
                write(&path, &changed);
                apply_provider(before, "deepseek", Some(&card()), true).map(Some)
            },
            || Ok(()),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("providers.json")
                || matches!(error, AppError::Localized { .. })
        );
        assert_eq!(read_providers(&path).unwrap(), changed);
    }

    #[test]
    fn a_failed_commit_rolls_back_only_while_the_file_is_still_ours() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        write(&path, &foreign_document());
        let original = fs::read(&path).unwrap();
        let result: Result<(), AppError> = transact(
            &path,
            |before| apply_provider(before, "deepseek", Some(&card()), true).map(Some),
            || Err(AppError::Message("db failed".into())),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);

        let mut changed = foreign_document();
        changed["models"][0]["name"] = json!("Phi (edited by Copilot)");
        let result: Result<(), AppError> = transact(
            &path,
            |before| apply_provider(before, "deepseek", Some(&card()), true).map(Some),
            || {
                write(&path, &changed);
                Err(AppError::Message("db failed".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(read_providers(&path).unwrap(), changed);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_first_write_through_a_dangling_symlink_removes_the_target_and_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        let target = dir.path().join("dotfiles").join("providers.json");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let result: Result<(), AppError> = transact(
            &path,
            |before| apply_provider(before, "deepseek", Some(&card()), true).map(Some),
            || Err(AppError::Message("db failed".into())),
        );
        assert!(result.is_err());
        assert!(!target.exists());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn adding_appends_only_this_card_and_keeps_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        write(&path, &foreign_document());
        update_at(&path, None, "deepseek", Some(&card()), true).unwrap();
        let after = read_providers(&path).unwrap();
        let names: Vec<_> = after["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["foundry", "other", "deepseek"]);
        assert_eq!(after["futureTopLevel"], json!({"keep": true}));
        // reasoningEffort 不进文件；name / provider 由卡片 id 补上。
        assert!(!serde_json::to_string(&after)
            .unwrap()
            .contains("reasoningEffort"));
        assert_eq!(after["providers"][2]["apiKey"], "sk-fixture");
        let models = after["models"].as_array().unwrap();
        assert_eq!(models.len(), 4);
        assert_eq!(
            models[2],
            json!({"id":"m1","provider":"deepseek","name":"Model 1","maxContextWindowTokens":128000,"maxOutputTokens":8192})
        );
        assert_eq!(models[3], json!({"id":"m2","provider":"deepseek"}));
        // 读回是卡片形状
        let back = &providers_in(&after)["deepseek"];
        assert_eq!(back["provider"]["baseUrl"], "https://api.example.test/v1");
        assert!(back["provider"].get("name").is_none());
        assert_eq!(back["models"][1], json!({"id":"m2"}));
        // 新建同名拒绝
        assert!(update_at(&path, None, "deepseek", Some(&card()), true).is_err());
    }

    #[test]
    fn updating_replaces_in_place_and_keeps_model_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        let mut document = foreign_document();
        document["providers"]
            .as_array_mut()
            .unwrap()
            .insert(1, json!({"name":"mine","baseUrl":"https://old.test"}));
        document["models"]
            .as_array_mut()
            .unwrap()
            .insert(1, json!({"id":"old","provider":"mine"}));
        write(&path, &document);
        update_at(&path, None, "mine", Some(&card()), false).unwrap();
        let after = read_providers(&path).unwrap();
        let names: Vec<_> = after["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["foundry", "mine", "other"]);
        let models: Vec<_> = after["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                format!(
                    "{}/{}",
                    m["provider"].as_str().unwrap(),
                    m["id"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(models, ["foundry/phi", "mine/m1", "mine/m2", "other/o1"]);
    }

    #[test]
    fn removing_deletes_only_this_card() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        write(&path, &foreign_document());
        update_at(&path, None, "deepseek", Some(&card()), true).unwrap();
        update_at(&path, None, "deepseek", None, false).unwrap();
        assert_eq!(read_providers(&path).unwrap(), foreign_document());
        // 不在文件里时什么都不写
        update_at(&path, None, "missing", None, false).unwrap();
        assert_eq!(read_providers(&path).unwrap(), foreign_document());
    }

    #[test]
    fn missing_file_is_created_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("providers.json");
        update_at(&path, None, "deepseek", Some(&card()), true).unwrap();
        let after = read_providers(&path).unwrap();
        assert_eq!(after["providers"][0]["name"], "deepseek");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn broken_or_invalid_documents_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        for text in [
            "{ not json",
            "// comment\n{}",
            "[]",
            r#"{"providers":{}}"#,
            r#"{"providers":[{"name":"a/b","baseUrl":"https://x.test"}]}"#,
            r#"{"providers":[{"name":"a","baseUrl":"https://x.test"},{"name":"a","baseUrl":"https://y.test"}]}"#,
            r#"{"models":[{"id":"m","provider":"a","maxOutputTokens":1.5}],"providers":[{"name":"a","baseUrl":"https://x.test"}]}"#,
            r#"{"providers":[{"name":"a","baseUrl":"https://x.test","transport":"websockets","wireApi":"completions"}]}"#,
            r#"{"models":[{"id":"m","provider":"ghost"}]}"#,
        ] {
            fs::write(&path, text).unwrap();
            assert!(
                update_at(&path, None, "deepseek", Some(&card()), true).is_err(),
                "{text}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn cards_that_copilot_would_reject_are_refused() {
        let ok = card();
        assert!(validate_provider("deepseek", &ok).is_ok());
        for id in ["", "a/b", "a b", "a\tb", &"x".repeat(65)] {
            assert!(validate_provider(id, &ok).is_err(), "{id:?}");
        }
        let mut no_models = card();
        no_models["models"] = json!([]);
        assert!(validate_provider("x", &no_models).is_err());
        let mut dup = card();
        dup["models"] = json!([{"id":"m"},{"id":"m"}]);
        assert!(validate_provider("x", &dup).is_err());
        let mut unknown = card();
        unknown["provider"]["region"] = json!("us");
        assert!(validate_provider("x", &unknown).is_err());
        let mut unknown_model = card();
        unknown_model["models"][0]["temperature"] = json!(1);
        assert!(validate_provider("x", &unknown_model).is_err());
        let mut ws = card();
        ws["provider"]["transport"] = json!("websockets");
        assert!(validate_provider("x", &ws).is_err());
        ws["provider"]["wireApi"] = json!("responses");
        assert!(validate_provider("x", &ws).is_ok());
        let mut bad_url = card();
        bad_url["provider"]["baseUrl"] = json!("ftp://x.test");
        assert!(validate_provider("x", &bad_url).is_err());
        let mut bad_effort = card();
        bad_effort["reasoningEffort"] = json!("turbo");
        assert!(validate_provider("x", &bad_effort).is_err());
        let mut bad_limit = card();
        bad_limit["models"][0]["maxOutputTokens"] = json!(8192.5);
        assert!(validate_provider("x", &bad_limit).is_err());
        // 官方卡
        assert!(validate_provider("default", &json!({"env":{}})).is_ok());
        assert!(validate_provider("default", &json!({"env":{"COPILOT_MODEL":"auto"}})).is_ok());
    }

    #[test]
    fn the_default_provider_and_its_selected_model_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.json");
        update_at(&path, None, "deepseek", Some(&card()), true).unwrap();
        let selected = Some("deepseek/m2");
        let error = update_at(&path, selected, "deepseek", None, false).unwrap_err();
        assert!(matches!(
            error,
            AppError::Localized {
                key: "copilot.provider.default_cannot_remove",
                ..
            }
        ));
        let mut fewer = card();
        fewer["models"] = json!([{"id":"m1"}]);
        let error = update_at(&path, selected, "deepseek", Some(&fewer), false).unwrap_err();
        assert!(matches!(
            error,
            AppError::Localized {
                key: "copilot.provider.default_model_cannot_drop",
                ..
            }
        ));
        // 选中的模型还在就可以改；选的是别家时随便移除
        let mut renamed = card();
        renamed["models"] = json!([{"id":"m2","name":"Two"}]);
        update_at(&path, selected, "deepseek", Some(&renamed), false).unwrap();
        update_at(&path, Some("other/x"), "deepseek", None, false).unwrap();
        assert_eq!(
            current_provider_in(Some("deepseek/m2"), &read_providers(&path).unwrap()),
            None
        );
    }

    #[test]
    fn launch_clears_every_byok_variable_and_selects_the_card_model() {
        let launch = prepare_launch("deepseek", &card()).unwrap();
        assert!(launch.byok);
        assert_eq!(launch.model.as_deref(), Some("deepseek/m1"));
        assert_eq!(launch.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(launch.env.len(), BYOK_ENV_KEYS.len() + 1);
        assert!(launch.env.iter().all(|(_, value)| value.is_none()));
        assert!(launch.env.iter().any(|(key, _)| *key == MODEL_ENV));

        let official = prepare_launch(
            "default",
            &json!({"env":{"COPILOT_MODEL":"claude-sonnet-4.6"},"reasoningEffort":"none"}),
        )
        .unwrap();
        assert!(!official.byok);
        assert_eq!(official.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(official.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(official.env.len(), BYOK_ENV_KEYS.len());
        assert!(official.env.iter().all(|(_, value)| value.is_none()));
    }

    #[test]
    fn legacy_environment_cards_convert_to_the_new_shape() {
        let legacy = json!({"env":{
            "COPILOT_PROVIDER_BASE_URL":"https://relay.test/v1",
            "COPILOT_PROVIDER_TYPE":"azure",
            "COPILOT_PROVIDER_WIRE_API":"responses",
            "COPILOT_PROVIDER_TRANSPORT":"websockets",
            "COPILOT_PROVIDER_API_KEY":"sk-a",
            "COPILOT_PROVIDER_BEARER_TOKEN":"tok",
            "COPILOT_PROVIDER_AZURE_API_VERSION":"2024-10-21",
            "COPILOT_PROVIDER_HEADERS":"X-A: 1\nX-B: two: parts",
            "COPILOT_PROVIDER_WIRE_MODEL":"gpt-wire",
            "COPILOT_PROVIDER_MODEL_ID":"gpt-native",
            "COPILOT_PROVIDER_MAX_PROMPT_TOKENS":"1000",
            "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS":"200",
            "COPILOT_MODEL":"gpt-5.4"
        },"reasoningEffort":"low"});
        let converted = convert_legacy_settings(&legacy).unwrap();
        assert_eq!(
            converted,
            json!({
                "provider": {
                    "type":"azure","wireApi":"responses","transport":"websockets",
                    "baseUrl":"https://relay.test/v1","apiKey":"sk-a","bearerToken":"tok",
                    "azure":{"apiVersion":"2024-10-21"},
                    "headers":{"X-A":"1","X-B":"two: parts"}
                },
                "models":[{"id":"gpt-5.4","wireModel":"gpt-wire","modelId":"gpt-native","maxPromptTokens":1000,"maxOutputTokens":200}],
                "reasoningEffort":"low"
            })
        );
        assert!(validate_provider("relay", &converted).is_ok());

        let minimal = json!({"env":{"COPILOT_PROVIDER_BASE_URL":"https://r.test","COPILOT_PROVIDER_WIRE_MODEL":"w"}});
        assert_eq!(
            convert_legacy_settings(&minimal).unwrap()["models"],
            json!([{"id":"w"}])
        );
        assert!(convert_legacy_settings(&json!({"env":{"COPILOT_MODEL":"auto"}})).is_none());
        assert!(convert_legacy_settings(&card()).is_none());
    }

    #[test]
    fn only_settings_with_a_model_are_imported() {
        assert!(!has_managed_content(&json!({"env":{}})));
        assert!(has_managed_content(
            &json!({"env":{"COPILOT_MODEL":"gpt-5.6-terra"}})
        ));
    }

    #[test]
    #[serial]
    fn providers_config_variable_overrides_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let _home = test_support::CopilotHomeGuard::new(dir.path());
        assert_eq!(providers_path(), dir.path().join("providers.json"));
        let custom = dir.path().join("custom.json");
        std::env::set_var(PROVIDERS_CONFIG_ENV, &custom);
        assert_eq!(providers_path(), custom);
        save_and_commit("deepseek", Some(&card()), true, || Ok(())).unwrap();
        assert!(custom.exists());
        assert!(!dir.path().join("providers.json").exists());
        assert!(get_providers().unwrap().contains_key("deepseek"));
    }

    #[test]
    #[serial]
    fn failed_commit_restores_the_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let _home = test_support::CopilotHomeGuard::new(dir.path());
        let path = providers_path();
        write(&path, &foreign_document());
        let before = fs::read(&path).unwrap();
        let result: Result<(), AppError> = save_and_commit("deepseek", Some(&card()), true, || {
            Err(AppError::Message("db".into()))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    #[serial]
    fn current_provider_follows_the_selected_model() {
        let dir = tempfile::tempdir().unwrap();
        let _home = test_support::CopilotHomeGuard::new(dir.path());
        assert_eq!(current_provider().unwrap(), "");
        save_and_commit("deepseek", Some(&card()), true, || Ok(())).unwrap();
        fs::write(settings_path(), r#"{"model":"deepseek/m1"}"#).unwrap();
        assert_eq!(current_provider().unwrap(), "deepseek");
        // 官方模型的编辑器读回不把 BYOK 选择当成官方模型
        assert_eq!(read_live_settings().unwrap(), json!({"env":{}}));
        fs::write(settings_path(), r#"{"model":"ghost/m1"}"#).unwrap();
        assert_eq!(current_provider().unwrap(), "");
        fs::write(settings_path(), r#"{"model":"gpt-5-mini"}"#).unwrap();
        assert_eq!(current_provider().unwrap(), "");
        assert_eq!(
            read_live_settings().unwrap(),
            json!({"env":{"COPILOT_MODEL":"gpt-5-mini"}})
        );
        assert!(save_and_commit("deepseek", None, false, || Ok(())).is_ok());
    }
}
