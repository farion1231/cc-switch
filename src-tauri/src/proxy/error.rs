use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("上游响应体超过大小上限: {0} 字节")]
    ResponseBodyTooLarge(usize),

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
                    ProxyError::ResponseBodyTooLarge(_) => {
                        (StatusCode::BAD_GATEWAY, self.to_string())
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

/// 上游 400/422 明确表示目标模型不可用时，失败在供应商侧（映射目标已下线/不存在），
/// 换一家 provider 可能成功。不要把这类错误当成客户端请求格式问题（issue #6821）。
pub(crate) fn is_upstream_model_unavailable(error: &ProxyError) -> bool {
    let ProxyError::UpstreamError { status, body } = error else {
        return false;
    };
    if !matches!(*status, 400 | 422) {
        return false;
    }
    let Some(body) = body.as_deref() else {
        return false;
    };

    extract_upstream_error_fields(body)
        .into_iter()
        .any(|field| has_model_unavailable_phrase(&field.to_ascii_lowercase()))
}

/// JSON 只看公认的错误字段，避免把回显的请求体/诊断上下文当成模型下线信号。
/// 非 JSON 时整段 body 就是错误文本。
fn extract_upstream_error_fields(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return vec![body.to_string()];
    };

    const POINTERS: &[&str] = &[
        "/error/message",
        "/error/code",
        "/error/type",
        "/message",
        "/detail",
        "/error",
    ];
    POINTERS
        .iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn has_model_unavailable_phrase(text: &str) -> bool {
    const PHRASES: &[&str] = &[
        "model is unavailable",
        "model unavailable",
        "model_not_found",
        "model not found",
        "no such model",
        "model does not exist",
        "model doesn't exist",
        "模型不可用",
        "模型不存在",
        "模型已下线",
    ];
    PHRASES.iter().any(|phrase| text.contains(phrase))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_error(status: u16, body: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: Some(body.to_string()),
        }
    }

    #[test]
    fn issue_body_model_is_unavailable_is_detected() {
        let body = r#"{"error":{"type":"server_error","message":"Error from provider (Console): Upstream request failed: Model is unavailable."}}"#;
        assert!(is_upstream_model_unavailable(&unavailable_error(400, body)));
    }

    #[test]
    fn openai_model_not_found_code_is_detected() {
        let body = r#"{"error":{"message":"The model `gpt-5` does not exist","type":"invalid_request_error","code":"model_not_found"}}"#;
        assert!(is_upstream_model_unavailable(&unavailable_error(400, body)));
    }

    #[test]
    fn chinese_model_offline_message_is_detected() {
        assert!(is_upstream_model_unavailable(&unavailable_error(
            422,
            r#"{"error":{"message":"模型已下线"}}"#
        )));
    }

    #[test]
    fn generic_client_400_is_not_treated_as_model_unavailable() {
        let body = r#"{"error":{"message":"invalid request: missing required field"}}"#;
        assert!(!is_upstream_model_unavailable(&unavailable_error(
            400, body
        )));
        assert!(!is_upstream_model_unavailable(&unavailable_error(
            400,
            r#"{"error":{"message":"field does not exist"}}"#
        )));
        assert!(!is_upstream_model_unavailable(&ProxyError::UpstreamError {
            status: 400,
            body: None,
        }));
        assert!(!is_upstream_model_unavailable(&unavailable_error(
            401,
            r#"{"error":{"message":"Model is unavailable."}}"#
        )));
    }

    #[test]
    fn echoed_request_body_is_not_treated_as_model_unavailable() {
        let body = r#"{"error":{"message":"invalid json schema"},"input":"please retry if model not found"}"#;
        assert!(!is_upstream_model_unavailable(&unavailable_error(
            400, body
        )));
    }

    #[test]
    fn plain_text_body_is_scanned() {
        assert!(is_upstream_model_unavailable(&unavailable_error(
            400,
            "Model is unavailable."
        )));
    }
}
