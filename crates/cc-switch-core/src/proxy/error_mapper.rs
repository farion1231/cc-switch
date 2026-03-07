//! 错误类型到 HTTP 状态码的映射
//!
//! 将 ProxyError 映射到合适的 HTTP 状态码，用于日志记录

use super::ProxyError;

/// 将 ProxyError 映射到 HTTP 状态码
///
/// 映射规则：
/// - 上游错误：直接使用上游返回的状态码
/// - 超时：504 Gateway Timeout
/// - 连接失败：502 Bad Gateway
/// - 无可用 Provider：503 Service Unavailable
/// - 重试耗尽：503 Service Unavailable
/// - 其他错误：500 Internal Server Error
pub fn map_proxy_error_to_status(error: &ProxyError) -> u16 {
    match error {
        // 上游错误：使用实际状态码
        ProxyError::UpstreamError { status, .. } => *status,

        // 超时错误：504 Gateway Timeout
        ProxyError::Timeout(_) => 504,

        // 转发失败/连接失败：502 Bad Gateway
        ProxyError::ForwardFailed(_) => 502,

        // 无可用 Provider：503 Service Unavailable
        ProxyError::NoAvailableProvider => 503,

        // 所有供应商已熔断：503 Service Unavailable
        ProxyError::AllProvidersCircuitOpen => 503,

        // 未配置供应商：503 Service Unavailable
        ProxyError::NoProvidersConfigured => 503,

        // 重试耗尽：503 Service Unavailable
        ProxyError::MaxRetriesExceeded => 503,

        // Provider 不健康：503 Service Unavailable
        ProxyError::ProviderUnhealthy(_) => 503,

        // 数据库错误：500 Internal Server Error
        ProxyError::DatabaseError(_) => 500,

        // 转换错误：500 Internal Server Error
        ProxyError::TransformError(_) => 500,

        // 其他未知错误：500 Internal Server Error
        _ => 500,
    }
}

/// 将 ProxyError 转换为用户友好的错误消息
pub fn get_error_message(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            if let Some(body) = body {
                format!("上游错误 ({status}): {body}")
            } else {
                format!("上游错误 ({status})")
            }
        }
        ProxyError::Timeout(msg) => format!("请求超时: {msg}"),
        ProxyError::ForwardFailed(msg) => format!("转发失败: {msg}"),
        ProxyError::NoAvailableProvider => "无可用 Provider".to_string(),
        ProxyError::AllProvidersCircuitOpen => "所有供应商已熔断，无可用渠道".to_string(),
        ProxyError::NoProvidersConfigured => "未配置供应商".to_string(),
        ProxyError::MaxRetriesExceeded => "所有 Provider 都失败，重试耗尽".to_string(),
        ProxyError::ProviderUnhealthy(msg) => format!("Provider 不健康: {msg}"),
        ProxyError::DatabaseError(msg) => format!("数据库错误: {msg}"),
        ProxyError::TransformError(msg) => format!("请求/响应转换错误: {msg}"),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_upstream_error() {
        let error = ProxyError::UpstreamError {
            status: 401,
            body: Some("Unauthorized".to_string()),
        };
        assert_eq!(map_proxy_error_to_status(&error), 401);
    }

    #[test]
    fn test_map_timeout_error() {
        let error = ProxyError::Timeout("Request timeout".to_string());
        assert_eq!(map_proxy_error_to_status(&error), 504);
    }

    #[test]
    fn test_map_connection_error() {
        let error = ProxyError::ForwardFailed("Connection refused".to_string());
        assert_eq!(map_proxy_error_to_status(&error), 502);
    }

    #[test]
    fn test_map_no_provider_error() {
        let error = ProxyError::NoAvailableProvider;
        assert_eq!(map_proxy_error_to_status(&error), 503);
    }

    #[test]
    fn test_get_error_message() {
        let error = ProxyError::UpstreamError {
            status: 500,
            body: Some("Internal Server Error".to_string()),
        };
        let msg = get_error_message(&error);
        assert!(msg.contains("上游错误"));
        assert!(msg.contains("500"));
        assert!(msg.contains("Internal Server Error"));
    }
}
