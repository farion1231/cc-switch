//! 多维错误分类模块
//!
//! 使用位掩码标志对 Provider 错误进行分类，
//! 驱动回退链路中的后续决策（是否重试、是否抑制、是否固定）。

use bitflags::bitflags;
use crate::proxy::error::ProxyError;
use std::collections::HashMap;

bitflags! {
    /// 多维错误分类标志。
    /// 多个标志可同时设置，以实现精细化的恢复决策。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ErrorFlags: u32 {
        /// 瞬态故障，重试可能成功（网络抖动、连接重置等）
        const TRANSIENT        = 1 << 0;
        /// 用量/配额耗尽（429 或 usage_limit 错误）
        const USAGE_LIMIT      = 1 << 1;
        /// 认证失败（API Key 无效/过期）— 不可重试
        const AUTH_FAILED      = 1 << 2;
        /// 上下文窗口溢出（需要压缩而非切换）
        const CONTEXT_OVERFLOW = 1 << 3;
        /// 账户级策略拒绝（封禁、欠费等）
        const ACCOUNT_POLICY   = 1 << 4;
        /// 内容安全过滤器拦截
        const CONTENT_BLOCKED  = 1 << 5;
        /// 流式拒绝检测（Guardrail 在流中检测到 LLM 拒绝）
        const CLASSIFIER_REFUSAL = 1 << 6;
        /// 服务端错误（5xx）
        const SERVER_ERROR     = 1 << 7;
        /// 网络层错误（超时、DNS、连接被拒）
        const NETWORK_ERROR    = 1 << 8;
        /// Provider 明确返回过载信号
        const PROVIDER_OVERLOAD = 1 << 9;
        /// 未知/未分类错误
        const UNKNOWN          = 1 << 10;
    }
}

/// 分类后的错误信息，包含标志和元数据。
#[derive(Debug, Clone)]
pub struct ClassifiedError {
    /// 错误分类标志
    pub flags: ErrorFlags,
    /// HTTP 状态码（如果有）
    pub status_code: Option<u16>,
    /// 错误消息
    pub message: String,
    /// Retry-After 头解析出的秒数（如果有）
    pub retry_after_seconds: Option<u64>,
    /// 触发此错误的 Provider Selector 标识
    pub selector_identity: String,
}

impl ClassifiedError {
    /// 该错误是否可重试。
    /// 可重试的条件：包含 TRANSIENT/USAGE_LIMIT/SERVER_ERROR/
    /// PROVIDER_OVERLOAD/NETWORK_ERROR/CLASSIFIER_REFUSAL 中的任一标志，
    /// 且不包含 AUTH_FAILED。
    pub fn is_retryable(&self) -> bool {
        self.flags.intersects(
            ErrorFlags::TRANSIENT
                | ErrorFlags::USAGE_LIMIT
                | ErrorFlags::SERVER_ERROR
                | ErrorFlags::PROVIDER_OVERLOAD
                | ErrorFlags::NETWORK_ERROR
                | ErrorFlags::CLASSIFIER_REFUSAL,
        ) && !self.flags.contains(ErrorFlags::AUTH_FAILED)
    }

    /// 是否应当抑制（冷却）当前 Selector。
    /// Provider 健康类错误（用量耗尽、服务端错误、过载、网络错误）
    /// 和账户策略错误应触发抑制。
    pub fn should_suppress(&self) -> bool {
        self.flags.intersects(
            ErrorFlags::USAGE_LIMIT
                | ErrorFlags::SERVER_ERROR
                | ErrorFlags::PROVIDER_OVERLOAD
                | ErrorFlags::NETWORK_ERROR
                | ErrorFlags::ACCOUNT_POLICY,
        )
    }

    /// 是否应当固定（pin）回退状态，不再自动切回主 Provider。
    /// 内容安全拒绝和分类器拒绝需要固定。
    pub fn should_pin(&self) -> bool {
        self.flags.contains(ErrorFlags::CLASSIFIER_REFUSAL)
            || self.flags.contains(ErrorFlags::CONTENT_BLOCKED)
    }

    /// 是否应当尝试切换凭证而非切换 Provider。
    pub fn should_rotate_credentials(&self) -> bool {
        self.flags.contains(ErrorFlags::USAGE_LIMIT)
    }
}

/// 从 HTTP 状态码和响应体分类错误。
pub fn classify_http_error(
    status_code: u16,
    headers: &HashMap<String, String>,
    body: &str,
    selector_identity: String,
) -> ClassifiedError {
    let mut flags = ErrorFlags::empty();

    match status_code {
        // 429 — 可能是速率限制（TRANSIENT）或用量配额（USAGE_LIMIT）
        429 => {
            let body_lower = body.to_lowercase();
            if body_lower.contains("quota")
                || body_lower.contains("usage_limit")
                || body_lower.contains("insufficient_quota")
                || body_lower.contains("billing")
                || body_lower.contains("payment")
            {
                flags |= ErrorFlags::USAGE_LIMIT;
            } else if body_lower.contains("rate_limit")
                || body_lower.contains("too many requests")
                || body_lower.contains("resource_exhausted")
            {
                flags |= ErrorFlags::TRANSIENT;
            } else {
                // 429 默认视为用量限制
                flags |= ErrorFlags::USAGE_LIMIT;
            }
            // 检查是否同时也是账户策略问题
            if body_lower.contains("account")
                || body_lower.contains("suspended")
                || body_lower.contains("banned")
            {
                flags |= ErrorFlags::ACCOUNT_POLICY;
            }
        }

        // 5xx — 服务端错误
        500..=599 => {
            flags |= ErrorFlags::SERVER_ERROR;
            let body_lower = body.to_lowercase();
            if body_lower.contains("overload")
                || body_lower.contains("capacity")
                || body_lower.contains("unavailable")
            {
                flags |= ErrorFlags::PROVIDER_OVERLOAD;
            }
        }

        // 401 — 认证失败
        401 => {
            flags |= ErrorFlags::AUTH_FAILED;
        }

        // 403 — 可能是并发上限（TRANSIENT）或账户策略
        403 => {
            let body_lower = body.to_lowercase();
            if body_lower.contains("concurrent")
                || body_lower.contains("too many")
                || body_lower.contains("parallel")
            {
                flags |= ErrorFlags::TRANSIENT;
            } else if body_lower.contains("policy")
                || body_lower.contains("content")
                || body_lower.contains("safety")
            {
                flags |= ErrorFlags::CONTENT_BLOCKED;
            } else {
                flags |= ErrorFlags::ACCOUNT_POLICY;
            }
        }

        // 400 — 可能是上下文溢出
        400 => {
            let body_lower = body.to_lowercase();
            if body_lower.contains("context")
                || body_lower.contains("token")
                || body_lower.contains("window")
                || body_lower.contains("too long")
            {
                flags |= ErrorFlags::CONTEXT_OVERFLOW;
            } else if body_lower.contains("refusal")
                || body_lower.contains("safety")
                || body_lower.contains("content filter")
            {
                flags |= ErrorFlags::CONTENT_BLOCKED;
            }
        }

        // 其他状态码
        _ => {
            flags |= ErrorFlags::UNKNOWN;
        }
    }

    // 解析 Retry-After 头
    let retry_after_seconds = parse_retry_after(headers);

    ClassifiedError {
        flags,
        status_code: Some(status_code),
        message: body.to_string(),
        retry_after_seconds,
        selector_identity,
    }
}

/// 从网络/连接错误分类错误。
pub fn classify_network_error(
    error_msg: &str,
    selector_identity: String,
) -> ClassifiedError {
    let msg_lower = error_msg.to_lowercase();

    let flags = if msg_lower.contains("timeout")
        || msg_lower.contains("timed out")
    {
        ErrorFlags::NETWORK_ERROR
    } else if msg_lower.contains("dns")
        || msg_lower.contains("resolve")
        || msg_lower.contains("name")
    {
        ErrorFlags::NETWORK_ERROR | ErrorFlags::TRANSIENT
    } else if msg_lower.contains("refused")
        || msg_lower.contains("reset")
        || msg_lower.contains("broken pipe")
        || msg_lower.contains("eof")
    {
        ErrorFlags::NETWORK_ERROR | ErrorFlags::TRANSIENT
    } else if msg_lower.contains("tls")
        || msg_lower.contains("ssl")
        || msg_lower.contains("certificate")
    {
        ErrorFlags::NETWORK_ERROR
    } else {
        ErrorFlags::NETWORK_ERROR | ErrorFlags::UNKNOWN
    };

    ClassifiedError {
        flags,
        status_code: None,
        message: error_msg.to_string(),
        retry_after_seconds: None,
        selector_identity,
    }
}

/// 检查流式响应中是否包含分类器拒绝信号。
pub fn detect_classifier_refusal(
    chunk: &str,
    has_tool_calls: bool,
) -> Option<ErrorFlags> {
    let lower = chunk.to_lowercase();

    // 强拒绝模式
    let strong_patterns = [
        "i cannot assist",
        "i cannot help with",
        "i can't help with",
        "i'm not able to",
        "i'm unable to help",
        "i'm afraid i can't",
        "i apologize",
        "i'm sorry",
    ];

    for pattern in &strong_patterns {
        if lower.contains(pattern) {
            return Some(ErrorFlags::CLASSIFIER_REFUSAL);
        }
    }

    // 上下文拒绝（需要同时结合内容策略检查）
    let context_patterns = [
        "content policy",
        "safety guidelines",
        "against our policy",
        "violates our terms",
    ];

    for pattern in &context_patterns {
        if lower.contains(pattern) {
            return Some(ErrorFlags::CLASSIFIER_REFUSAL | ErrorFlags::CONTENT_BLOCKED);
        }
    }

    // 弱拒绝（仅在没有工具调用时才认为是拒绝）
    if !has_tool_calls {
        let weak_patterns = [
            "that's not something i can",
            "have you considered",
            "instead of",
        ];

        for pattern in &weak_patterns {
            if lower.contains(pattern) {
                return Some(ErrorFlags::CLASSIFIER_REFUSAL);
            }
        }
    }

    None
}

/// 将 cc-switch 的 `ProxyError` 分类为多维错误标志。
///
/// 这是 fallback chain 与现有转发循环的桥接函数：把重试循环捕获的
/// 上游/网络错误映射为 ErrorFlags，驱动后续的抑制/退避/固定决策。
pub fn classify_proxy_error(error: &crate::proxy::error::ProxyError, selector_identity: String) -> ClassifiedError {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let headers = HashMap::new();
            let body_text = body.clone().unwrap_or_default();
            classify_http_error(*status, &headers, &body_text, selector_identity)
        }
        ProxyError::Timeout(_) => ClassifiedError {
            flags: ErrorFlags::NETWORK_ERROR | ErrorFlags::TRANSIENT,
            status_code: None,
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
        ProxyError::ForwardFailed(_) | ProxyError::ProviderUnhealthy(_) => ClassifiedError {
            flags: ErrorFlags::NETWORK_ERROR,
            status_code: None,
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
        ProxyError::StreamIdleTimeout(_) => ClassifiedError {
            flags: ErrorFlags::TRANSIENT,
            status_code: None,
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
        ProxyError::AuthError(_) => ClassifiedError {
            flags: ErrorFlags::AUTH_FAILED,
            status_code: Some(401),
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
        ProxyError::ConfigError(_) | ProxyError::TransformError(_) => ClassifiedError {
            flags: ErrorFlags::TRANSIENT,
            status_code: None,
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
        _ => ClassifiedError {
            flags: ErrorFlags::UNKNOWN,
            status_code: None,
            message: error.to_string(),
            retry_after_seconds: None,
            selector_identity,
        },
    }
}

/// 从 HTTP 响应头中解析 Retry-After 值。
fn parse_retry_after(headers: &HashMap<String, String>) -> Option<u64> {    // 首先检查 retry-after-ms 自定义头
    for key in &["retry-after-ms", "x-ratelimit-reset-ms"] {
        if let Some(value) = headers.get(&key.to_lowercase()) {
            if let Ok(ms) = value.parse::<u64>() {
                return Some(ms / 1000);
            }
        }
    }

    // 标准 Retry-After 头
    if let Some(value) = headers.get("retry-after") {
        // 尝试作为秒数解析
        if let Ok(seconds) = value.parse::<u64>() {
            return Some(seconds);
        }
        // 尝试作为 HTTP 日期解析
        if let Ok(datetime) = chrono::DateTime::parse_from_rfc2822(value) {
            let datetime_utc = datetime.with_timezone(&chrono::Utc);
            let now_utc = chrono::Utc::now();
            let diff = (datetime_utc - now_utc).num_seconds();
            if diff > 0 {
                return Some(diff as u64);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_transient() {
        let error = ClassifiedError {
            flags: ErrorFlags::TRANSIENT,
            status_code: Some(503),
            message: "Service temporarily unavailable".into(),
            retry_after_seconds: None,
            selector_identity: "test:model".into(),
        };
        assert!(error.is_retryable());
        assert!(!error.should_pin());
        // TRANSIENT 是瞬时抖动（网络、连接重置），不应抑制 Selector，
        // 否则一次网络波动会导致长时间冷却。抑制留给健康类错误（5xx/429/过载等）。
        assert!(!error.should_suppress());
    }

    #[test]
    fn test_auth_failed_not_retryable() {
        let error = ClassifiedError {
            flags: ErrorFlags::AUTH_FAILED,
            status_code: Some(401),
            message: "Invalid API key".into(),
            retry_after_seconds: None,
            selector_identity: "test:model".into(),
        };
        assert!(!error.is_retryable());
        assert!(!error.should_suppress());
    }

    #[test]
    fn test_classifier_refusal_pins_and_retryable() {
        let error = ClassifiedError {
            flags: ErrorFlags::CLASSIFIER_REFUSAL,
            status_code: Some(400),
            message: "I cannot assist with that request".into(),
            retry_after_seconds: None,
            selector_identity: "test:model".into(),
        };
        assert!(error.is_retryable());
        assert!(error.should_pin());
        assert!(!error.should_suppress());
    }

    #[test]
    fn test_content_blocked_pins() {
        let error = ClassifiedError {
            flags: ErrorFlags::CONTENT_BLOCKED | ErrorFlags::CLASSIFIER_REFUSAL,
            status_code: Some(400),
            message: "Content policy violation".into(),
            retry_after_seconds: None,
            selector_identity: "test:model".into(),
        };
        assert!(error.should_pin());
    }

    #[test]
    fn test_usage_limit_should_rotate() {
        let error = ClassifiedError {
            flags: ErrorFlags::USAGE_LIMIT,
            status_code: Some(429),
            message: "Quota exceeded".into(),
            retry_after_seconds: Some(3600),
            selector_identity: "test:model".into(),
        };
        assert!(error.is_retryable());
        assert!(error.should_suppress());
        assert!(error.should_rotate_credentials());
    }

    #[test]
    fn test_classify_http_429_quota() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "3600".to_string());

        let error = classify_http_error(429, &headers, "quota exceeded", "test:claude".into());
        assert!(error.flags.contains(ErrorFlags::USAGE_LIMIT));
        assert_eq!(error.retry_after_seconds, Some(3600));
    }

    #[test]
    fn test_classify_http_503() {
        let error = classify_http_error(
            503,
            &HashMap::new(),
            "Service Unavailable",
            "test:claude".into(),
        );
        assert!(error.flags.contains(ErrorFlags::SERVER_ERROR));
        assert!(error.is_retryable());
    }

    #[test]
    fn test_classify_http_401() {
        let error = classify_http_error(
            401,
            &HashMap::new(),
            "Invalid API key",
            "test:claude".into(),
        );
        assert!(error.flags.contains(ErrorFlags::AUTH_FAILED));
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_classify_http_403_policy() {
        let error = classify_http_error(
            403,
            &HashMap::new(),
            "Content policy violation",
            "test:claude".into(),
        );
        assert!(error.flags.contains(ErrorFlags::CONTENT_BLOCKED));
    }

    #[test]
    fn test_classify_network_timeout() {
        let error = classify_network_error("request timed out", "test:claude".into());
        assert!(error.flags.contains(ErrorFlags::NETWORK_ERROR));
        assert!(error.is_retryable());
    }

    #[test]
    fn test_detect_classifier_refusal_strong() {
        let result = detect_classifier_refusal(
            "I cannot assist with that request, it violates our terms of service.",
            false,
        );
        assert!(result.is_some());
        assert!(result.unwrap().contains(ErrorFlags::CLASSIFIER_REFUSAL));
    }

    #[test]
    fn test_detect_classifier_refusal_content_policy() {
        let result = detect_classifier_refusal(
            "This request goes against our content policy and safety guidelines.",
            false,
        );
        assert!(result.is_some());
        assert!(result.unwrap().contains(ErrorFlags::CONTENT_BLOCKED));
    }

    #[test]
    fn test_detect_classifier_refusal_not_refusal() {
        let result = detect_classifier_refusal(
            "Here is the code you requested: \n```rust\nfn main() {}\n```",
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_classifier_refusal_weak_with_tools() {
        // 弱拒绝 + 有工具调用 → 不是拒绝
        let result = detect_classifier_refusal(
            "Have you considered using a different approach?",
            true,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_classifier_refusal_weak_without_tools() {
        // 弱拒绝 + 无工具调用 → 可能是拒绝
        let result = detect_classifier_refusal(
            "Have you considered using a different approach?",
            false,
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_classify_proxy_error_429_quota() {
        let error = crate::proxy::error::ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"quota exceeded"}}"#.to_string()),
        };
        let classified = classify_proxy_error(&error, "test:model".into());
        assert!(classified.flags.contains(ErrorFlags::USAGE_LIMIT));
        assert!(classified.is_retryable());
        assert!(classified.should_suppress());
        assert!(classified.should_rotate_credentials());
    }

    #[test]
    fn test_classify_proxy_error_503_server() {
        let error = crate::proxy::error::ProxyError::UpstreamError {
            status: 503,
            body: Some("Service Unavailable".to_string()),
        };
        let classified = classify_proxy_error(&error, "test:model".into());
        assert!(classified.flags.contains(ErrorFlags::SERVER_ERROR));
        assert!(classified.is_retryable());
        assert!(classified.should_suppress());
    }

    #[test]
    fn test_classify_proxy_error_auth() {
        let error = crate::proxy::error::ProxyError::AuthError("invalid key".into());
        let classified = classify_proxy_error(&error, "test:model".into());
        assert!(classified.flags.contains(ErrorFlags::AUTH_FAILED));
        assert!(!classified.is_retryable());
        assert!(!classified.should_suppress());
    }

    #[test]
    fn test_classify_proxy_error_timeout() {
        let error = crate::proxy::error::ProxyError::Timeout("request timed out".into());
        let classified = classify_proxy_error(&error, "test:model".into());
        assert!(classified.flags.contains(ErrorFlags::NETWORK_ERROR));
        assert!(classified.is_retryable());
        assert!(classified.should_suppress());
    }
}
