use std::path::Path;
use std::sync::PoisonError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误: {0}")]
    Config(String),
    #[error("无效输入: {0}")]
    InvalidInput(String),
    /// Native files changed after CC Switch last read them.
    #[error("并发冲突: {0}")]
    Conflict(String),
    #[error("IO 错误: {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    IoContext {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON 解析错误: {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("JSON 序列化失败: {source}")]
    JsonSerialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("TOML 解析错误: {path}: {source}")]
    Toml {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("锁获取失败: {0}")]
    Lock(String),
    #[error("MCP 校验失败: {0}")]
    McpValidation(String),
    #[error("{0}")]
    Message(String),
    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("{zh} ({en})")]
    Localized {
        key: &'static str,
        zh: String,
        en: String,
    },
    #[error("数据库错误: {0}")]
    Database(String),
    #[error("OMO 配置文件不存在")]
    OmoConfigNotFound,
    #[error("所有供应商已熔断，无可用渠道")]
    AllProvidersCircuitOpen,
    #[error("未配置供应商")]
    NoProvidersConfigured,
}

impl AppError {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn json(path: impl AsRef<Path>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn toml(path: impl AsRef<Path>, source: toml::de::Error) -> Self {
        Self::Toml {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn localized(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> Self {
        Self::Localized {
            key,
            zh: zh.into(),
            en: en.into(),
        }
    }
}

impl<T> From<PoisonError<T>> for AppError {
    fn from(err: PoisonError<T>) -> Self {
        Self::Lock(err.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Native UI surfaces need readable text while command errors retain their code.
pub(crate) fn display_error_message(message: &str) -> String {
    let Some(start) = message.find('{') else {
        return message.to_string();
    };
    let Ok(error) = serde_json::from_str::<serde_json::Value>(&message[start..]) else {
        return message.to_string();
    };
    if error.get("code").and_then(serde_json::Value::as_str)
        == Some("codex.official_login_unavailable")
    {
        if let Some(detail) = error.get("message").and_then(serde_json::Value::as_str) {
            return format!("{}{detail}", &message[..start]);
        }
    }
    message.to_string()
}

/// 格式化为 JSON 错误字符串，前端可解析为结构化错误
pub fn format_skill_error(
    code: &str,
    context: &[(&str, &str)],
    suggestion: Option<&str>,
) -> String {
    use serde_json::json;

    let mut ctx_map = serde_json::Map::new();
    for (key, value) in context {
        ctx_map.insert(key.to_string(), json!(value));
    }

    let error_obj = json!({
        "code": code,
        "context": ctx_map,
        "suggestion": suggestion,
    });

    serde_json::to_string(&error_obj).unwrap_or_else(|_| {
        // 如果 JSON 序列化失败，返回简单格式
        format!("ERROR:{code}")
    })
}

#[cfg(test)]
mod tests {
    use super::display_error_message;

    #[test]
    fn codex_login_display_keeps_context_without_showing_json() {
        let error = serde_json::json!({
            "code": "codex.official_login_unavailable",
            "message": "Add the account in Accounts."
        })
        .to_string();
        for prefix in ["", "切换路由失败: "] {
            assert_eq!(
                display_error_message(&format!("{prefix}{error}")),
                format!("{prefix}Add the account in Accounts.")
            );
        }
    }

    #[test]
    fn codex_login_display_preserves_unrecognized_errors() {
        for message in [
            "an ordinary error",
            "wrapped: {invalid json}",
            r#"{"code":"another.error","message":"keep the payload"}"#,
            r#"{"code":"codex.official_login_unavailable"}"#,
        ] {
            assert_eq!(display_error_message(message), message);
        }
    }
}
