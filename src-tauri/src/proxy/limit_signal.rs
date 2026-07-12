//! 限流信号分类器
//!
//! 检测上游响应是否包含"该轮换 key"的信号——这些信号触发 `KeyRing` 轮换
//! 到下一把 key，而不是 fail over 到下一个 provider。
//!
//! 三种触发：
//! - HTTP 429：始终触发（语义明确）
//! - 401 / 403 / 5xx + quota message：上游通过状态码 + body 文字暗示 key 失效
//!   （`insufficient_quota` / `quota exceeded` / `usage limit` / `billing` 等）
//! - 任意 status + 用户 regex 命中：自定义代理 / 私有网关的特殊语种
//!
//! 与 fail over 区别：
//! - fail over：`RequestForwarder` 推进到 provider 列表里的下一个
//! - key 轮换：`KeyRing::next_key` 在同一个 provider 的 key 池里推进
//!
//! 一把 key 命中限流 → 轮换；所有 key 都耗尽 → 落入 fail over 路径（forwarder
//! 的现有 `continue` 兜底）。

use crate::proxy::error::ProxyError;
use http::HeaderMap;

/// 内置"quota / 限流"短语子串。匹配时不区分大小写。
///
/// 覆盖 Anthropic / OpenAI / Google / 主流中转服务的常见错误文案。新供应商
/// 上线时如果发现漏报，加到这里（不引入 i18n 配置——内部错误体都用英文）。
const BUILTIN_QUOTA_PHRASES: &[&str] = &[
    "insufficient_quota",
    "quota exceeded",
    "quota_exceeded",
    "rate limit",
    "rate_limit",
    "rate-limit",
    "usage limit",
    "usage_limit",
    "billing",
    "credit balance",
    "payment required",
];

/// 触发轮换的限流信号。
///
/// 简洁结构——只携带"为什么轮换"和"建议冷却多久"，forwarder 据此决定
/// 走 `KeyRing::mark_rate_limited(signal.retry_after_secs)` 还是让 KeyRing
/// 用默认下限。
#[derive(Debug, Clone, Copy)]
pub struct LimitSignal {
    /// 简短原因代码（用于日志/UI 分类）。
    pub reason: &'static str,
    /// 建议的冷却秒数。`None` 表示让 KeyRing 用默认下限（`MIN_COOLDOWN_SECS`）。
    pub retry_after_secs: Option<u64>,
}

/// 检测 `ProxyError` 是否包含"该轮换 key"的信号。
///
/// 入口签名只接 `&ProxyError`（不接 `HeaderMap`）——上游响应头在 forwarder
/// 构造 `UpstreamError` 时已经预解析到 `retry_after_secs` 字段。这样做的好处：
/// 1. 错误对象自包含，下游 `classify_limit_signal` 调用方不依赖外部状态；
/// 2. 测试只需要构造一个 `ProxyError` 即可驱动分类路径；
/// 3. 避免 `HeaderMap` 跨 await 边界的生命周期问题。
///
/// 返回 `Some(LimitSignal)` 时 forwarder 触发 key 轮换。
pub fn classify_limit_signal(
    err: &ProxyError,
    user_regex: Option<&regex::Regex>,
) -> Option<LimitSignal> {
    let (status, body, retry_after_secs) = match err {
        ProxyError::UpstreamError {
            status,
            body,
            retry_after_secs,
        } => (*status, body.as_deref(), *retry_after_secs),
        _ => return None,
    };

    // 1) HTTP 429 — 始终触发（语义最明确）。
    if status == 429 {
        return Some(LimitSignal {
            reason: "429",
            retry_after_secs,
        });
    }

    // 2) 内置 quota 短语 OR 用户 regex → 触发轮换。
    // 误报代价低——多换一把 key 比「错失轮换 + provider failover」要可控得多。
    if let Some(quota_match) = detect_quota_message(body.unwrap_or(""), user_regex) {
        let reason = match quota_match {
            QuotaMatch::Builtin => "quota_message",
            QuotaMatch::UserRegex => "user_regex",
        };
        return Some(LimitSignal {
            reason,
            retry_after_secs,
        });
    }

    None
}

/// quota 消息识别结果——`detect_quota_message` 返回值。给出用户
/// 实际命中的来源，便于上游 `classify_limit_signal` 写对日志 reason。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaMatch {
    Builtin,
    UserRegex,
}

/// 在响应体里检测 quota / 限流消息（大小写不敏感）。
///
/// 内置短语覆盖绝大多数主流供应商；用户 regex 作为可选补充（用于
/// 自家代理 / 私有网关）。user_regex 优先级高于内置短语——用户
/// 显式声明的 regex 是更精确的信号，应承认为用户自定义而不是通用命中。
pub fn detect_quota_message(
    body: &str,
    user_regex: Option<&regex::Regex>,
) -> Option<QuotaMatch> {
    if body.is_empty() {
        return user_regex
            .and_then(|re| re.is_match(body).then_some(QuotaMatch::UserRegex));
    }
    if let Some(re) = user_regex {
        if re.is_match(body) {
            return Some(QuotaMatch::UserRegex);
        }
    }
    let lower = body.to_ascii_lowercase();
    if BUILTIN_QUOTA_PHRASES.iter().any(|p| lower.contains(p)) {
        return Some(QuotaMatch::Builtin);
    }
    None
}

/// 从 `Retry-After` 头解析秒数。支持两种格式：
/// - `<seconds>`：纯整数
/// - HTTP-date：`Wed, 21 Oct 2015 07:28:00 GMT`
///
/// 解析失败返回 `None`（不 panic）。HTTP-date 路径用 `chrono` 解析（项目已用）。
pub fn parse_retry_after(headers: &HeaderMap) -> Option<u64> {
    let v = headers.get("retry-after")?.to_str().ok()?.trim();

    // 1) 纯整数秒数（最常见）
    if let Ok(secs) = v.parse::<u64>() {
        return Some(secs);
    }

    // 2) HTTP-date（RFC 7231）
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(v) {
        let now = chrono::Utc::now();
        let diff = (dt.with_timezone(&chrono::Utc) - now).num_seconds();
        // 已经过期 → 返回 0（让 KeyRing 用默认下限）
        return Some(diff.max(0) as u64);
    }

    None
}

/// 从 `Retry-After-MS` / `x-ratelimit-reset-ms` 头解析毫秒数，返回秒。
///
/// 两个头名都识别（OpenAI 风格 / 自定义网关风格）。
pub fn parse_retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    for name in &["retry-after-ms", "x-ratelimit-reset-ms"] {
        if let Some(v) = headers.get(*name).and_then(|h| h.to_str().ok()) {
            if let Ok(ms) = v.trim().parse::<u64>() {
                return Some(ms.div_ceil(1000));
            }
        }
    }
    None
}

/// 组合 `parse_retry_after` + `parse_retry_after_ms`，返回最终秒数。
///
/// 优先级：`Retry-After`（标准）> `Retry-After-MS` / `x-ratelimit-reset-ms`（扩展）。
pub fn extract_retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    parse_retry_after(headers).or_else(|| parse_retry_after_ms(headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;
    use regex::Regex;

    fn upstream(status: u16, body: Option<&str>, retry_after: Option<u64>) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: body.map(String::from),
            retry_after_secs: retry_after,
        }
    }

    // ---- classify_limit_signal ----

    #[test]
    fn classify_429_always_triggers() {
        let err = upstream(429, None, None);
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "429");
    }

    #[test]
    fn classify_429_propagates_retry_after() {
        let err = upstream(429, Some("rate limit"), Some(45));
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "429");
        assert_eq!(s.retry_after_secs, Some(45));
    }

    #[test]
    fn classify_401_with_insufficient_quota_triggers() {
        let err = upstream(
            401,
            Some(r#"{"error":{"message":"insufficient_quota"}}"#),
            None,
        );
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "quota_message");
    }

    #[test]
    fn classify_403_with_quota_text_triggers() {
        let err = upstream(403, Some("payment required - billing issue"), None);
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "quota_message");
    }

    #[test]
    fn classify_500_with_quota_text_triggers() {
        let err = upstream(500, Some("quota exceeded for this account"), None);
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "quota_message");
    }

    #[test]
    fn classify_500_without_quota_does_not_trigger() {
        let err = upstream(500, Some(r#"{"error":"internal_server_error"}"#), None);
        assert!(classify_limit_signal(&err, None).is_none());
    }

    #[test]
    fn classify_400_without_quota_does_not_trigger() {
        // 400 without a quota phrase is a real client error, not a quota
        // signal — don't waste a key rotation on it.
        let err = upstream(400, Some("bad request"), None);
        assert!(classify_limit_signal(&err, None).is_none());
    }

    #[test]
    fn classify_400_with_quota_text_triggers() {
        // MiniMax / PackyCode / 中转服务常用：HTTP 400 状态码 + quota 文案。
        // 旧版要求 status ∈ {401,403,5xx} 才接 body 检测，会错失轮换。
        // 新版：quota phrase 在任意 status 下都触发。
        let err = upstream(400, Some("usage limit reached, retry after 5h"), None);
        let s = classify_limit_signal(&err, None).unwrap();
        assert_eq!(s.reason, "quota_message");
    }

    #[test]
    fn classify_user_regex_match_triggers() {
        // body 含 custom 标记但不带内置 quota 短语 —— 走 user_regex 路径。
        // 不能写 "usage limit" 之类，因为新逻辑里 quota phrase 优先级更高。
        let re = Regex::new(r"custom-marker").unwrap();
        let err = upstream(400, Some("response body has custom-marker inside"), None);
        let s = classify_limit_signal(&err, Some(&re)).unwrap();
        assert_eq!(s.reason, "user_regex");
    }

    #[test]
    fn classify_non_upstream_error_returns_none() {
        let err = ProxyError::Timeout("foo".to_string());
        assert!(classify_limit_signal(&err, None).is_none());
    }

    // ---- detect_quota_message ----

    #[test]
    fn detect_quota_builtin_phrases_case_insensitive() {
        for phrase in &[
            "insufficient_quota",
            "Quota Exceeded",
            "rate limit",
            "USAGE LIMIT",
            "billing issue",
            "credit balance low",
            "payment required",
        ] {
            assert_eq!(
                detect_quota_message(phrase, None),
                Some(QuotaMatch::Builtin),
                "should match builtin: {phrase}"
            );
        }
    }

    #[test]
    fn detect_quota_negative() {
        assert_eq!(detect_quota_message("OK", None), None);
        assert_eq!(
            detect_quota_message(r#"{"error":"not_found"}"#, None),
            None
        );
    }

    #[test]
    fn detect_quota_user_regex_overrides_builtin_label() {
        // 用户 regex 命中时返回 UserRegex，即使同时匹配 builtin 短语——
        // user 显式声明更精确。
        let re = Regex::new(r"专属限流提示").unwrap();
        let body = "专属限流提示 / usage limit";
        assert_eq!(
            detect_quota_message(body, Some(&re)),
            Some(QuotaMatch::UserRegex),
        );
        // 仅有 builtin 命中
        assert_eq!(
            detect_quota_message(body, None),
            Some(QuotaMatch::Builtin),
        );
    }

    // ---- parse_retry_after ----

    #[test]
    fn parse_retry_after_seconds() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", HeaderValue::from_static("120"));
        assert_eq!(parse_retry_after(&h), Some(120));
    }

    #[test]
    fn parse_retry_after_http_date() {
        let mut h = HeaderMap::new();
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        h.insert(
            "retry-after",
            HeaderValue::from_str(&future.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
                .unwrap(),
        );
        let secs = parse_retry_after(&h).unwrap();
        // 1h ± 5s tolerance
        assert!(
            (3590..=3605).contains(&secs),
            "expected ~3600s, got {secs}"
        );
    }

    #[test]
    fn parse_retry_after_http_date_past_returns_zero() {
        let mut h = HeaderMap::new();
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        h.insert(
            "retry-after",
            HeaderValue::from_str(&past.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
                .unwrap(),
        );
        assert_eq!(parse_retry_after(&h), Some(0));
    }

    #[test]
    fn parse_retry_after_invalid_returns_none() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", HeaderValue::from_static("not-a-number"));
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn parse_retry_after_missing_returns_none() {
        let h = HeaderMap::new();
        assert_eq!(parse_retry_after(&h), None);
    }

    // ---- parse_retry_after_ms ----

    #[test]
    fn parse_retry_after_ms_standard_header() {
        let mut h = HeaderMap::new();
        h.insert("retry-after-ms", HeaderValue::from_static("5000"));
        assert_eq!(parse_retry_after_ms(&h), Some(5));
    }

    #[test]
    fn parse_retry_after_ms_x_ratelimit_header() {
        let mut h = HeaderMap::new();
        h.insert("x-ratelimit-reset-ms", HeaderValue::from_static("30000"));
        assert_eq!(parse_retry_after_ms(&h), Some(30));
    }

    #[test]
    fn parse_retry_after_ms_rounds_up_subsecond() {
        // 1500ms → 2s (向上取整，避免 1.5s 被 floor 成 1s 然后立刻被打回)
        let mut h = HeaderMap::new();
        h.insert("retry-after-ms", HeaderValue::from_static("1500"));
        assert_eq!(parse_retry_after_ms(&h), Some(2));
    }

    // ---- extract_retry_after_secs ----

    #[test]
    fn extract_prefers_standard_retry_after() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", HeaderValue::from_static("10"));
        h.insert("retry-after-ms", HeaderValue::from_static("5000"));
        assert_eq!(extract_retry_after_secs(&h), Some(10));
    }

    #[test]
    fn extract_falls_back_to_ms_header() {
        let mut h = HeaderMap::new();
        h.insert("retry-after-ms", HeaderValue::from_static("5000"));
        assert_eq!(extract_retry_after_secs(&h), Some(5));
    }

    #[test]
    fn extract_returns_none_when_no_headers() {
        let h = HeaderMap::new();
        assert_eq!(extract_retry_after_secs(&h), None);
    }
}
