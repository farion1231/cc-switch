//! 妯″瀷鏄犲皠妯″潡
//!
//! 鍦ㄨ姹傝浆鍙戝墠锛屾牴鎹?Provider 閰嶇疆鏇挎崲璇锋眰涓殑妯″瀷鍚嶇О

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use crate::provider::Provider;
use serde_json::Value;

/// 妯″瀷鏄犲皠閰嶇疆
pub struct ModelMapping {
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub default_model: Option<String>,
}

impl ModelMapping {
    /// 浠?Provider 閰嶇疆涓彁鍙栨ā鍨嬫槧灏?    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");

        Self {
            haiku_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 妫€鏌ユ槸鍚﹂厤缃簡浠讳綍妯″瀷鏄犲皠
    pub fn has_mapping(&self) -> bool {
        self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.default_model.is_some()
    }

    /// 鏍规嵁鍘熷妯″瀷鍚嶇О鑾峰彇鏄犲皠鍚庣殑妯″瀷
    pub fn map_model(&self, original_model: &str) -> String {
        let model_lower = original_model.to_lowercase();

        // 1. 鎸夋ā鍨嬬被鍨嬪尮閰?        if model_lower.contains("haiku") {
            if let Some(ref m) = self.haiku_model {
                return m.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref m) = self.sonnet_model {
                return m.clone();
            }
        }

        // 2. 榛樿妯″瀷
        if let Some(ref m) = self.default_model {
            return m.clone();
        }

        // 3. 鏃犳槧灏勶紝淇濇寔鍘熸牱
        original_model.to_string()
    }
}

/// 瀵硅姹備綋搴旂敤妯″瀷鏄犲皠
///
/// 杩斿洖 (鏄犲皠鍚庣殑璇锋眰浣? 鍘熷妯″瀷鍚? 鏄犲皠鍚庢ā鍨嬪悕)
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 濡傛灉娌℃湁閰嶇疆鏄犲皠锛岀洿鎺ヨ繑鍥?    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 鎻愬彇鍘熷妯″瀷鍚?    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    if let Some(ref original) = original_model {
        let mapped = mapping.map_model(original);

        if mapped != *original {
            log::debug!("[ModelMapper] 妯″瀷鏄犲皠: {original} 鈫?{mapped}");
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.clone()), Some(mapped));
        }
    }

    (body, original_model, None)
}

/// Claude Code 閫氳繃 `[1M]` 鍚庣紑澹版槑 100 涓囦笂涓嬫枃鑳藉姏锛涗笂娓?API
/// 閫氬父涓嶆帴鍙楄繖涓湰鍦拌兘鍔涙爣璁帮紝杞彂鍓嶉渶瑕佸墺绂汇€?pub fn strip_one_m_suffix_for_upstream(model: &str) -> &str {
    let trimmed = model.trim_end();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    model
}

pub fn strip_one_m_suffix_for_upstream_from_body(mut body: Value) -> Value {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return body;
    };

    let stripped = strip_one_m_suffix_for_upstream(model);
    if stripped != model {
        log::debug!("[ModelMapper] 鍘婚櫎鏈湴 1M 鏍囪: {model} 鈫?{stripped}");
        body["model"] = serde_json::json!(stripped);
    }
    body
}

/// 妫€娴嬭姹備綋涓槸鍚﹀寘鍚浘鐗囧唴瀹?///
/// 鏀寔涓ょ API 鏍煎紡锛?/// - Anthropic Messages API: `messages[].content[].type == "image"`
/// - OpenAI Responses API: `input[].content[].type == "input_image"`
///   鎴?`input[].type == "message"` 鍚浘鐗?content
pub fn request_contains_images(body: &Value) -> bool {
    // Anthropic Messages API 鏍煎紡
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("image") {
                        // 验证 source 字段是否存在且有效
                        if let Some(source) = block.get("source") {
                            // 检查 source.type 是否为 base64 或 url
                            if let Some(source_type) = source.get("type").and_then(|t| t.as_str()) {
                                if source_type == "base64" && source.get("data").is_some() {
                                    return true;
                                }
                                if source_type == "url" && source.get("url").is_some() {
                                    return true;
                                }
                            }
                            // 兼容旧格式：直接检查 data 或 url
                            if source.get("data").is_some() || source.get("url").is_some() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    // OpenAI Responses API 鏍煎紡
    if let Some(input) = body.get("input").and_then(|i| i.as_array()) {
        for item in input {
            // input item 鐩存帴鏄?content block
            if item.get("type").and_then(|t| t.as_str()) == Some("input_image") {
                return true;
            }
            // input item 鏄?message锛屽唴鍚?content 鏁扮粍
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("input_image") {
                        return true;
                    }
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider_with_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_without_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_sonnet_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-sonnet-4-5-20250929"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(original, Some("claude-sonnet-4-5-20250929".to_string()));
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_haiku_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-haiku-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "haiku-mapped");
        assert_eq!(mapped, Some("haiku-mapped".to_string()));
    }

    #[test]
    fn test_opus_mapping() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "claude-opus-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "opus-mapped");
        assert_eq!(mapped, Some("opus-mapped".to_string()));
    }

    #[test]
    fn test_thinking_does_not_affect_model_mapping() {
        // Issue #2081: thinking 鍙傛暟涓嶅簲褰卞搷妯″瀷鏄犲皠
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_adaptive_does_not_affect_model_mapping() {
        // Issue #2081: adaptive thinking 涔熶笉搴斿奖鍝嶆ā鍨嬫槧灏?        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "adaptive"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_thinking_disabled() {
        let provider = create_provider_with_mapping();
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "disabled"}
        });
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn test_unknown_model_uses_default() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "some-unknown-model"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "default-model");
        assert_eq!(mapped, Some("default-model".to_string()));
    }

    #[test]
    fn test_no_mapping_configured() {
        let provider = create_provider_without_mapping();
        let body = json!({"model": "claude-sonnet-4-5"});
        let (result, original, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "claude-sonnet-4-5");
        assert_eq!(original, Some("claude-sonnet-4-5".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let provider = create_provider_with_mapping();
        let body = json!({"model": "Claude-SONNET-4-5"});
        let (result, _, mapped) = apply_model_mapping(body, &provider);
        assert_eq!(result["model"], "sonnet-mapped");
        assert_eq!(mapped, Some("sonnet-mapped".to_string()));
    }

    #[test]
    fn strips_one_m_suffix_before_upstream() {
        let body = json!({"model": "deepseek-v4-pro[1M]"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn strips_one_m_suffix_after_mapping() {
        let mut provider = create_provider_with_mapping();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro [1M]"
            }
        });

        let body = json!({"model": "claude-sonnet-4-6"});
        let (mapped, _, _) = apply_model_mapping(body, &provider);
        let result = strip_one_m_suffix_for_upstream_from_body(mapped);

        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    #[test]
    fn keeps_model_without_one_m_suffix() {
        let body = json!({"model": "deepseek-v4-pro"});
        let result = strip_one_m_suffix_for_upstream_from_body(body);
        assert_eq!(result["model"], "deepseek-v4-pro");
    }

    // ==================== request_contains_images 娴嬭瘯 ====================

    #[test]
    fn test_anthropic_format_with_image() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "describe this"},
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
                    ]
                }
            ]
        });
        assert!(request_contains_images(&body));
    }

    #[test]
    fn test_responses_format_with_input_image() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "describe this"},
                    {"type": "input_image", "image_url": "data:image/png;base64,abc123"}
                ]}
            ]
        });
        assert!(request_contains_images(&body));
    }

    #[test]
    fn test_responses_format_with_direct_input_image() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "input": [
                {"type": "input_image", "image_url": "data:image/png;base64,abc123"}
            ]
        });
        assert!(request_contains_images(&body));
    }

    #[test]
    fn test_text_only_request_returns_false() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "messages": [
                {"role": "user", "content": "hello world"}
            ]
        });
        assert!(!request_contains_images(&body));
    }

    #[test]
    fn test_text_content_array_returns_false() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "hello"}
                    ]
                }
            ]
        });
        assert!(!request_contains_images(&body));
    }

    #[test]
    fn test_empty_body_returns_false() {
        let body = json!({"model": "mimo-v2.5-pro"});
        assert!(!request_contains_images(&body));
    }

    #[test]
    fn test_empty_messages_returns_false() {
        let body = json!({"model": "mimo-v2.5-pro", "messages": []});
        assert!(!request_contains_images(&body));
    }
}
