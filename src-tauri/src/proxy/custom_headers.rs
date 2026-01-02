//! Provider 自定义请求头（custom_headers）处理
//!
//! 目标：
//! - 从 Provider.settings_config.custom_headers 读取自定义请求头（对象结构）
//! - 注入到上游请求 headers 中
//! - 自定义请求头优先级最高（覆盖同名头）
//! - 对少量 HTTP 协议级保留头强制忽略，避免协议层异常

use crate::provider::Provider;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const PROTOCOL_RESERVED_HEADERS: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "content-length",
    "host",
];

pub fn apply_custom_headers_from_provider(provider: &Provider, headers: &mut HeaderMap) -> usize {
    let Some(obj) = provider
        .settings_config
        .get("custom_headers")
        .and_then(|v| v.as_object())
    else {
        return 0;
    };

    let mut applied = 0usize;

    for (key, value) in obj {
        let key_trimmed = key.trim();
        if key_trimmed.is_empty() {
            continue;
        }

        let key_lower = key_trimmed.to_ascii_lowercase();
        if PROTOCOL_RESERVED_HEADERS.contains(&key_lower.as_str()) {
            continue;
        }

        let Some(value_str) = value.as_str() else {
            continue;
        };

        let Ok(header_name) = HeaderName::from_bytes(key_trimmed.as_bytes()) else {
            continue;
        };

        let Ok(header_value) = HeaderValue::from_str(value_str) else {
            continue;
        };

        headers.insert(header_name, header_value);
        applied += 1;
    }

    applied
}

pub fn apply_custom_headers_to_request(provider: &Provider, request: &mut reqwest::Request) -> usize {
    apply_custom_headers_from_provider(provider, request.headers_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_provider(settings_config: serde_json::Value) -> Provider {
        Provider::with_id("p1".to_string(), "P1".to_string(), settings_config, None)
    }

    #[test]
    fn apply_custom_headers_overrides_and_filters_protocol_reserved() {
        let provider = make_provider(json!({
            "custom_headers": {
                "X-Tenant-Id": "abc",
                "Authorization": "Bearer provider",
                "Content-Length": "123"
            }
        }));

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer system"),
        );

        let applied = apply_custom_headers_from_provider(&provider, &mut headers);

        assert_eq!(applied, 2);
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer provider"
        );
        assert_eq!(headers.get("x-tenant-id").unwrap().to_str().unwrap(), "abc");
        assert!(headers.get("content-length").is_none());
    }
}

