use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("服务器已在运行")]
    AlreadyRunning,

    #[error("服务器未运行")]
    NotRunning,

    #[error("地址绑定失败: {0}")]
    BindFailed(String),

    #[error("停止超时")]
    StopTimeout,

    #[error("停止失败: {0}")]
    StopFailed(String),

    #[error("请求转发失败: {0}")]
    ForwardFailed(String),

    #[error("无可用的Provider")]
    NoAvailableProvider,

    #[error("所有供应商已熔断，无可用渠道")]
    AllProvidersCircuitOpen,

    #[error("未配置供应商")]
    NoProvidersConfigured,

    #[allow(dead_code)]
    #[error("Provider不健康: {0}")]
    ProviderUnhealthy(String),

    #[error("上游错误 (状态码 {status}): {body:?}")]
    UpstreamError { status: u16, body: Option<String> },

    #[error("超过最大重试次数")]
    MaxRetriesExceeded,

    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[allow(dead_code)]
    #[error("格式转换错误: {0}")]
    TransformError(String),

    #[allow(dead_code)]
    #[error("无效的请求: {0}")]
    InvalidRequest(String),

    #[error("超时: {0}")]
    Timeout(String),

    /// 流式响应空闲超时
    #[allow(dead_code)]
    #[error("流式响应空闲超时: {0}秒无数据")]
    StreamIdleTimeout(u64),

    /// 认证错误
    #[error("认证失败: {0}")]
    AuthError(String),

    #[allow(dead_code)]
    #[error("内部错误: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ProxyError::UpstreamError {
                status: upstream_status,
                body: upstream_body,
            } => {
                let http_status =
                    StatusCode::from_u16(*upstream_status).unwrap_or(StatusCode::BAD_GATEWAY);

                // 尝试解析上游响应体为 JSON，如果失败则包装为字符串
                let error_body = if let Some(body_str) = upstream_body {
                    if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(body_str) {
                        // 上游返回的是 JSON，直接透传
                        json_body
                    } else {
                        // 上游返回的不是 JSON，包装为错误消息
                        json!({
                            "error": {
                                "message": body_str,
                                "type": "upstream_error",
                            }
                        })
                    }
                } else {
                    json!({
                        "error": {
                            "message": format!("Upstream error (status {})", upstream_status),
                            "type": "upstream_error",
                        }
                    })
                };

                (http_status, error_body)
            }
            _ => {
                let (http_status, message) = match &self {
                    ProxyError::AlreadyRunning => (StatusCode::CONFLICT, self.to_string()),
                    ProxyError::NotRunning => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
                    ProxyError::BindFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopTimeout => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ForwardFailed(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
                    ProxyError::NoAvailableProvider => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::AllProvidersCircuitOpen => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::NoProvidersConfigured => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::ProviderUnhealthy(_) => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::MaxRetriesExceeded => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::DatabaseError(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ConfigError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::TransformError(_) => {
                        (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
                    }
                    ProxyError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),
                    ProxyError::StreamIdleTimeout(_) => {
                        (StatusCode::GATEWAY_TIMEOUT, self.to_string())
                    }
                    ProxyError::AuthError(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
                    ProxyError::Internal(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::UpstreamError { .. } => unreachable!(),
                };

                let error_body = json!({
                    "error": {
                        "message": message,
                        "type": "proxy_error",
                    }
                });

                (http_status, error_body)
            }
        };

        (status, Json(body)).into_response()
    }
}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 可重试错误（网络问题、5xx）
    Retryable, // 网络超时、5xx 错误
    /// 不可重试错误（4xx、认证失败）
    NonRetryable, // 认证失败、参数错误、4xx 错误
    #[allow(dead_code)]
    ClientAbort, // 客户端主动中断
}

/// 判断错误是否可重试
#[allow(dead_code)]
pub fn categorize_error(error: &reqwest::Error) -> ErrorCategory {
    if error.is_timeout() || error.is_connect() {
        return ErrorCategory::Retryable;
    }

    if let Some(status) = error.status() {
        if status.is_server_error() {
            ErrorCategory::Retryable
        } else if status.is_client_error() {
            ErrorCategory::NonRetryable
        } else {
            ErrorCategory::Retryable
        }
    } else {
        ErrorCategory::Retryable
    }
}

/// Convert a ProxyError into the Anthropic error format when the response
/// is destined for a Claude Code client that went through a format-transform
/// path (openai_chat / openai_responses / gemini_native).
///
/// Claude Code expects errors in Anthropic shape:
///
/// ```json
/// {"type": "error", "error": {"type": "<error_type>", "message": "..."}}
/// ```
///
/// Without this mapping, upstream OpenAI/LiteLLM error bodies like:
///
/// ```json
/// {"error": {"message": "...", "type": "...", "code": "500"}}
/// ```
///
/// are passed through verbatim, which Claude Code cannot parse correctly.
/// This can cause Claude Code to hang or retry indefinitely because it never
/// receives a properly structured error response.
pub fn map_proxy_error_to_anthropic(error: ProxyError) -> ProxyError {
    let (status, error_type, message) = match &error {
        ProxyError::UpstreamError {
            status,
            body: Some(body_str),
        } => {
            let error_type = if *status >= 500 {
                "api_error"
            } else {
                "invalid_request_error"
            };
            let message = extract_openai_error_message(body_str).unwrap_or_else(|| body_str.clone());
            (*status, error_type.to_string(), message)
        }
        ProxyError::UpstreamError { status, body: None } => {
            let error_type = if *status >= 500 {
                "api_error"
            } else {
                "invalid_request_error"
            };
            (
                *status,
                error_type.to_string(),
                format!("Upstream error (status {})", status),
            )
        }
        ProxyError::Timeout(msg) => (504, "timeout_error".to_string(), msg.clone()),
        ProxyError::StreamIdleTimeout(secs) => (
            504,
            "timeout_error".to_string(),
            format!("Stream idle timeout: {}s without data", secs),
        ),
        ProxyError::ForwardFailed(msg) => (502, "api_error".to_string(), msg.clone()),
        ProxyError::NoAvailableProvider
        | ProxyError::AllProvidersCircuitOpen
        | ProxyError::NoProvidersConfigured => (
            503,
            "api_error".to_string(),
            "No available provider".to_string(),
        ),
        ProxyError::MaxRetriesExceeded => (
            503,
            "api_error".to_string(),
            "Max retries exceeded".to_string(),
        ),
        ProxyError::TransformError(msg) => (
            500,
            "api_error".to_string(),
            format!("Transform error: {}", msg),
        ),
        ProxyError::AuthError(msg) => (401, "authentication_error".to_string(), msg.clone()),
        ProxyError::ConfigError(msg) | ProxyError::InvalidRequest(msg) => {
            (400, "invalid_request_error".to_string(), msg.clone())
        }
        _ => (500, "api_error".to_string(), error.to_string()),
    };

    let anthropic_body = json!({
        "type": "error",
        "error": {
            "type": error_type,
            "message": message
        }
    });
    ProxyError::UpstreamError {
        status,
        body: Some(serde_json::to_string(&anthropic_body).unwrap_or_else(|_| message)),
    }
}

/// Try to extract a human-readable message from an OpenAI-format error body.
///
/// OpenAI/LiteLLM errors look like:
/// ```json
/// {"error": {"message": "...", "type": "...", "code": "500"}}
/// ```
fn extract_openai_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = v.get("error")?.get("message")?.as_str()?;
    Some(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_500_maps_to_anthropic_api_error() {
        let openai_body = r#"{"error":{"message":"Internal Server Error","type":"server_error","code":"500"}}"#;
        let error = ProxyError::UpstreamError {
            status: 500,
            body: Some(openai_body.to_string()),
        };
        let result = map_proxy_error_to_anthropic(error);
        match result {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 500);
                let v: serde_json::Value = serde_json::from_str(body.unwrap().as_str()).unwrap();
                assert_eq!(v["type"], "error");
                assert_eq!(v["error"]["type"], "api_error");
                assert_eq!(v["error"]["message"], "Internal Server Error");
            }
            _ => panic!("Expected UpstreamError"),
        }
    }

    #[test]
    fn test_upstream_400_maps_to_anthropic_invalid_request() {
        let openai_body = r#"{"error":{"message":"Invalid model name","type":"invalid_request_error","code":"400"}}"#;
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(openai_body.to_string()),
        };
        let result = map_proxy_error_to_anthropic(error);
        match result {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 400);
                let v: serde_json::Value = serde_json::from_str(body.unwrap().as_str()).unwrap();
                assert_eq!(v["type"], "error");
                assert_eq!(v["error"]["type"], "invalid_request_error");
                assert_eq!(v["error"]["message"], "Invalid model name");
            }
            _ => panic!("Expected UpstreamError"),
        }
    }

    #[test]
    fn test_timeout_maps_to_anthropic_timeout_error() {
        let error = ProxyError::Timeout("request timed out".to_string());
        let result = map_proxy_error_to_anthropic(error);
        match result {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 504);
                let v: serde_json::Value = serde_json::from_str(body.unwrap().as_str()).unwrap();
                assert_eq!(v["type"], "error");
                assert_eq!(v["error"]["type"], "timeout_error");
                assert_eq!(v["error"]["message"], "request timed out");
            }
            _ => panic!("Expected UpstreamError"),
        }
    }

    #[test]
    fn test_extract_openai_error_message() {
        let body = r#"{"error":{"message":"Invalid tool_choice","type":"invalid_request_error","code":"400"}}"#;
        assert_eq!(
            extract_openai_error_message(body),
            Some("Invalid tool_choice".to_string())
        );
    }

    #[test]
    fn test_extract_openai_error_message_non_json() {
        assert_eq!(extract_openai_error_message("not json"), None);
    }

    #[test]
    fn test_upstream_no_body_uses_default_message() {
        let error = ProxyError::UpstreamError {
            status: 502,
            body: None,
        };
        let result = map_proxy_error_to_anthropic(error);
        match result {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 502);
                let v: serde_json::Value = serde_json::from_str(body.unwrap().as_str()).unwrap();
                assert_eq!(v["type"], "error");
                assert_eq!(v["error"]["type"], "api_error");
                assert!(v["error"]["message"].as_str().unwrap().contains("502"));
            }
            _ => panic!("Expected UpstreamError"),
        }
    }

    #[test]
    fn test_litellm_tool_choice_error_maps_correctly() {
        let litellm_body = r#"{"error":{"message":"litellm.APIConnectionError: Invalid tool_choice, tool_choice={'type': 'tool', 'name': 'web_search'}","type":null,"param":null,"code":"500"}}"#;
        let error = ProxyError::UpstreamError {
            status: 500,
            body: Some(litellm_body.to_string()),
        };
        let result = map_proxy_error_to_anthropic(error);
        match result {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 500);
                let v: serde_json::Value = serde_json::from_str(body.unwrap().as_str()).unwrap();
                assert_eq!(v["type"], "error");
                assert_eq!(v["error"]["type"], "api_error");
                assert!(v["error"]["message"].as_str().unwrap().contains("Invalid tool_choice"));
            }
            _ => panic!("Expected UpstreamError"),
        }
    }
}
