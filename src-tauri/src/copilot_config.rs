//! GitHub Copilot CLI (`copilot`) 的 live 配置。
//!
//! Copilot CLI 的数据目录是 `~/.copilot`（`COPILOT_HOME` 可改）：`settings.json` 管模型等
//! 用户设置，`mcp-config.json` 管 MCP，`skills/` 和 `copilot-instructions.md` 分别是个人
//! Skills 和全局指令，会话在 `session-state/<id>/`。
//!
//! 自带模型（BYOK）只能经进程环境 `COPILOT_PROVIDER_*` 传入，没有对应的配置文件。CC Switch
//! 管不到用户自己敲的 `copilot`，所以和 Antigravity 一样，卡片没有「启用」，只有「打开终端」：
//!
//! - **官方登录卡**：`copilot --model=<模型>`（没设模型就是 `copilot`），用 Copilot CLI 自己的
//!   GitHub 登录；卡片绑了授权中心的 GitHub 账号时，经 `COPILOT_GITHUB_TOKEN` 只给这一个进程。
//! - **自带模型卡**：Key、地址、协议和模型都只经子进程环境传入，不写进任何文件。
//!
//! `settings.json` 和会话目录始终共用，CC Switch 不改写其中的供应商相关字段。

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::get_home_dir;
use crate::error::AppError;

/// 自带模型时 CC Switch 管理的环境变量，顺序即「打开终端」时的设置顺序。
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
pub(crate) const TYPE_ENV: &str = "COPILOT_PROVIDER_TYPE";
pub(crate) const WIRE_API_ENV: &str = "COPILOT_PROVIDER_WIRE_API";
pub(crate) const HEADERS_ENV: &str = "COPILOT_PROVIDER_HEADERS";

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

/// 读 `settings.json`：Copilot 写的是 JSON，但用户手改时可能带注释，按 JSON5 兜底。
/// 文件不存在时返回 `None`；内容坏了报错，不当作空文件。
pub(crate) fn read_settings_document() -> Result<Option<Value>, AppError> {
    read_json_document(&settings_path())
}

pub(crate) fn read_json_document(path: &std::path::Path) -> Result<Option<Value>, AppError> {
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

/// 从 live 读回编辑器用的 `{ env }`：只有 `settings.json` 的模型，Key 不在任何文件里。
pub fn read_live_settings() -> Result<Value, AppError> {
    let Some(settings) = read_settings_document()? else {
        return Err(AppError::localized(
            "copilot.config.missing",
            "GitHub Copilot CLI 配置文件不存在",
            "GitHub Copilot CLI configuration file is missing",
        ));
    };
    let mut env = serde_json::Map::new();
    if let Some(model) = settings
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
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

/// 卡片里 Copilot CLI 认的字段。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CopilotProjection {
    pub model: Option<String>,
    /// 自带模型的环境变量（不含 `COPILOT_MODEL`），只收非空值。
    pub byok: Vec<(&'static str, String)>,
}

impl CopilotProjection {
    pub(crate) fn of(settings: &Value) -> Self {
        let env = settings.get("env");
        let model = env_string(env, MODEL_ENV);
        let byok = BYOK_ENV_KEYS
            .iter()
            .filter_map(|key| env_string(env, key).map(|value| (*key, value)))
            .collect();
        Self { model, byok }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.byok
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }

    /// 设了地址才是自带模型：Copilot CLI 也只认 `COPILOT_PROVIDER_BASE_URL` 来开 BYOK。
    pub(crate) fn is_byok(&self) -> bool {
        self.get(BASE_URL_ENV).is_some()
    }
}

fn env_string(env: Option<&Value>, key: &str) -> Option<String> {
    env?.get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 保存前校验：值必须是单行字符串（`COPILOT_PROVIDER_HEADERS` 允许换行分隔多个头）；
/// 自带模型要有模型；协议、接口取值必须是 Copilot CLI 认的。
pub(crate) fn validate_settings(settings: &Value) -> Result<(), AppError> {
    let obj = settings.as_object().ok_or_else(|| {
        AppError::localized(
            "copilot.validation.not_object",
            "GitHub Copilot CLI 配置必须是 JSON 对象",
            "GitHub Copilot CLI configuration must be a JSON object",
        )
    })?;
    if let Some(env) = obj.get("env") {
        let env = env.as_object().ok_or_else(|| {
            AppError::localized(
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
                return Err(AppError::localized(
                    "copilot.validation.env_not_string",
                    format!("GitHub Copilot CLI 环境变量 {key} 必须是字符串"),
                    format!("GitHub Copilot CLI environment variable {key} must be a string"),
                ));
            };
            let allows_newlines = key == HEADERS_ENV;
            if text
                .chars()
                .any(|c| c.is_control() && !(allows_newlines && matches!(c, '\n' | '\r')))
            {
                return Err(AppError::localized(
                    "copilot.validation.env_multiline",
                    format!("GitHub Copilot CLI 环境变量 {key} 不能包含控制字符"),
                    format!("GitHub Copilot CLI environment variable {key} must not contain control characters"),
                ));
            }
        }
    }
    let projection = CopilotProjection::of(settings);
    let has_credentials_or_options = projection.byok.iter().any(|(key, _)| *key != BASE_URL_ENV);
    if !projection.is_byok() && has_credentials_or_options {
        return Err(AppError::localized(
            "copilot.validation.missing_base_url",
            "自带模型需要填写 COPILOT_PROVIDER_BASE_URL",
            "Bring-your-own-model providers need COPILOT_PROVIDER_BASE_URL",
        ));
    }
    if projection.is_byok() {
        if projection.model.is_none()
            && projection.get("COPILOT_PROVIDER_MODEL_ID").is_none()
            && projection.get("COPILOT_PROVIDER_WIRE_MODEL").is_none()
        {
            return Err(AppError::localized(
                "copilot.validation.missing_model",
                "自带模型需要填写模型（COPILOT_MODEL）",
                "Bring-your-own-model providers need a model (COPILOT_MODEL)",
            ));
        }
        let base_url = projection.get(BASE_URL_ENV).unwrap_or_default();
        if url::Url::parse(base_url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .is_none()
        {
            return Err(AppError::localized(
                "copilot.validation.invalid_base_url",
                format!("COPILOT_PROVIDER_BASE_URL 不是有效的 http(s) 地址: {base_url}"),
                format!("COPILOT_PROVIDER_BASE_URL is not a valid http(s) URL: {base_url}"),
            ));
        }
    }
    for (key, allowed) in [
        (TYPE_ENV, &["openai", "azure", "anthropic"][..]),
        (WIRE_API_ENV, &["completions", "responses"][..]),
        ("COPILOT_PROVIDER_TRANSPORT", &["http", "websockets"][..]),
    ] {
        if let Some(value) = projection.get(key) {
            if !allowed.contains(&value) {
                return Err(AppError::localized(
                    "copilot.validation.invalid_option",
                    format!("{key} 只能是 {}", allowed.join(" / ")),
                    format!("{key} must be one of {}", allowed.join(" / ")),
                ));
            }
        }
    }
    for key in [
        "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
        "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
    ] {
        if let Some(value) = projection.get(key) {
            if value.parse::<u64>().ok().filter(|n| *n > 0).is_none() {
                return Err(AppError::localized(
                    "copilot.validation.invalid_token_limit",
                    format!("{key} 必须是正整数"),
                    format!("{key} must be a positive integer"),
                ));
            }
        }
    }
    Ok(())
}

/// 在终端里启动这张卡要用的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopilotLaunch {
    /// 官方登录卡的 `--model`；自带模型卡的模型经 `COPILOT_MODEL` 传入，为 `None`。
    pub model: Option<String>,
    /// 只给这一个 copilot 进程的环境变量；`None` 表示要清掉（用户 rc 里可能 export 过）。
    pub env: Vec<(&'static str, Option<String>)>,
    pub byok: bool,
}

/// 准备在终端里启动某张卡。官方卡清掉所有 `COPILOT_PROVIDER_*`，免得 rc 里 export 的
/// BYOK 变量把它悄悄变成自带模型；自带模型卡把没填的同名变量也清掉，避免和 rc 混用。
pub(crate) fn prepare_launch(settings: &Value) -> Result<CopilotLaunch, AppError> {
    validate_settings(settings)?;
    let projection = CopilotProjection::of(settings);
    if !projection.is_byok() {
        return Ok(CopilotLaunch {
            model: projection.model,
            env: BYOK_ENV_KEYS.iter().map(|key| (*key, None)).collect(),
            byok: false,
        });
    }
    let mut env: Vec<(&'static str, Option<String>)> = BYOK_ENV_KEYS
        .iter()
        .map(|key| {
            let value = projection.get(key).map(|value| {
                if *key == HEADERS_ENV {
                    // Windows .bat 装不下换行：Copilot CLI 也认字面量 `\n` 作分隔。
                    value
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\\n")
                } else {
                    value.to_string()
                }
            });
            (*key, value)
        })
        .collect();
    env.push((MODEL_ENV, projection.model.clone()));
    Ok(CopilotLaunch {
        model: None,
        env,
        byok: true,
    })
}

/// 会话页的恢复命令。
pub fn resume_command(session_id: &str) -> String {
    format!("copilot --resume={session_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn byok() -> Value {
        json!({"env":{
            "COPILOT_PROVIDER_BASE_URL":"https://example.test/v1",
            "COPILOT_PROVIDER_API_KEY":"fixture",
            "COPILOT_PROVIDER_TYPE":"openai",
            "COPILOT_PROVIDER_WIRE_API":"responses",
            "COPILOT_MODEL":"gpt-5.4"
        }})
    }

    #[test]
    fn official_launch_clears_byok_variables_and_passes_the_model() {
        let launch = prepare_launch(&json!({"env":{"COPILOT_MODEL":"claude-sonnet-4.6"}})).unwrap();
        assert!(!launch.byok);
        assert_eq!(launch.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(launch.env.len(), BYOK_ENV_KEYS.len());
        assert!(launch.env.iter().all(|(_, value)| value.is_none()));
    }

    #[test]
    fn byok_launch_passes_only_the_card_values_through_the_environment() {
        let launch = prepare_launch(&byok()).unwrap();
        assert!(launch.byok);
        assert_eq!(launch.model, None);
        let get = |key: &str| {
            launch
                .env
                .iter()
                .find(|(name, _)| *name == key)
                .and_then(|(_, value)| value.clone())
        };
        assert_eq!(get(API_KEY_ENV).as_deref(), Some("fixture"));
        assert_eq!(get(MODEL_ENV).as_deref(), Some("gpt-5.4"));
        assert_eq!(get(BEARER_TOKEN_ENV), None);
        // Unset values are cleared rather than inherited from the user's shell rc.
        assert!(launch
            .env
            .iter()
            .any(|(name, value)| *name == BEARER_TOKEN_ENV && value.is_none()));
    }

    #[test]
    fn headers_are_flattened_to_literal_newline_separators() {
        let mut settings = byok();
        settings["env"][HEADERS_ENV] = json!("X-A: 1\r\n\nX-B: 2\n");
        let launch = prepare_launch(&settings).unwrap();
        let headers = launch
            .env
            .iter()
            .find(|(name, _)| *name == HEADERS_ENV)
            .and_then(|(_, value)| value.clone())
            .unwrap();
        assert_eq!(headers, "X-A: 1\\nX-B: 2");
    }

    #[test]
    fn validation_rejects_incomplete_or_invalid_byok_cards() {
        let mut no_url = byok();
        no_url["env"].as_object_mut().unwrap().remove(BASE_URL_ENV);
        assert!(validate_settings(&no_url).is_err());

        let mut no_model = byok();
        no_model["env"].as_object_mut().unwrap().remove(MODEL_ENV);
        assert!(validate_settings(&no_model).is_err());

        let mut bad_type = byok();
        bad_type["env"][TYPE_ENV] = json!("gemini");
        assert!(validate_settings(&bad_type).is_err());

        let mut bad_url = byok();
        bad_url["env"][BASE_URL_ENV] = json!("ftp://example.test");
        assert!(validate_settings(&bad_url).is_err());

        let mut multiline = byok();
        multiline["env"][API_KEY_ENV] = json!("bad\nkey");
        assert!(validate_settings(&multiline).is_err());

        let mut bad_limit = byok();
        bad_limit["env"]["COPILOT_PROVIDER_MAX_OUTPUT_TOKENS"] = json!("0");
        assert!(validate_settings(&bad_limit).is_err());

        assert!(validate_settings(&json!({"env":{}})).is_ok());
        assert!(validate_settings(&json!({"env":{"COPILOT_MODEL":"auto"}})).is_ok());
    }

    #[test]
    fn only_settings_with_a_model_are_imported() {
        assert!(!has_managed_content(&json!({"env":{}})));
        assert!(has_managed_content(
            &json!({"env":{"COPILOT_MODEL":"gpt-5.6-terra"}})
        ));
    }
}
