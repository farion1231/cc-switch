//! 请求分类器模块
//!
//! 根据请求内容将请求分为 Main 或 Others 类型，
//! 用于智能路由将不同类型的请求分配到不同的供应商队列。

use crate::proxy::copilot_optimizer::classify_request;
use crate::proxy::types::RequestType;
use axum::http::HeaderMap;
use serde_json::Value;

/// 分类请求类型
///
/// 只有子代理 (subagent) 请求路由到 Others（子供应商队列），
/// 其它所有请求一律走 Main（主供应商队列）。
///
/// 实测发现 compact/warmup/agent-initiated 等场景过于频繁，
/// 导致主供应商几乎不被使用，因此精简分类策略。
pub fn classify_request_type(
    body: &Value,
    headers: &HeaderMap,
    copilot_config: &crate::proxy::types::CopilotOptimizerConfig,
) -> RequestType {
    let has_anthropic_beta = headers
        .get("anthropic-beta")
        .is_some();

    let classification = classify_request(
        body,
        has_anthropic_beta,
        copilot_config.compact_detection,
        copilot_config.subagent_detection,
    );

    if classification.is_subagent {
        return RequestType::Others;
    }

    RequestType::Main
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::CopilotOptimizerConfig;
    use serde_json::json;

    fn default_config() -> CopilotOptimizerConfig {
        CopilotOptimizerConfig::default()
    }

    #[test]
    fn test_user_initiator_classified_as_main() {
        let body = json!({
            "messages": [{"role": "user", "content": "Hello, how are you?"}]
        });
        let headers = HeaderMap::new();
        let result = classify_request_type(&body, &headers, &default_config());
        assert_eq!(result, RequestType::Main);
    }

    #[test]
    fn test_subagent_classified_as_others() {
        let body = json!({
            "messages": [{"role": "user", "content": "__SUBAGENT_MARKER__\nDo something"}]
        });
        let headers = HeaderMap::new();
        let result = classify_request_type(&body, &headers, &default_config());
        assert_eq!(result, RequestType::Others);
    }

    #[test]
    fn test_compact_classified_as_main() {
        // compact 请求不再分流到 Others，保持主供应商处理
        let body = json!({
            "system": "You are a helpful AI assistant tasked with summarizing conversations.",
            "messages": [{"role": "user", "content": "Summarize the conversation"}]
        });
        let headers = HeaderMap::new();
        let mut config = default_config();
        config.compact_detection = true;
        let result = classify_request_type(&body, &headers, &config);
        assert_eq!(result, RequestType::Main);
    }

    #[test]
    fn test_warmup_classified_as_main() {
        // warmup 请求不再分流到 Others，保持主供应商处理
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1
        });
        let headers = HeaderMap::new();
        let mut config = default_config();
        config.warmup_downgrade = true;
        let result = classify_request_type(&body, &headers, &config);
        assert_eq!(result, RequestType::Main);
    }

    #[test]
    fn test_agent_initiator_classified_as_main() {
        // 工具续写请求不再分流到 Others，保持主供应商处理
        let body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "I will read the file."},
                    {"type": "tool_use", "id": "toolu_001", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_001", "content": "file contents"}
                ]}
            ]
        });
        let result = classify_request_type(&body, &HeaderMap::new(), &default_config());
        assert_eq!(result, RequestType::Main);
    }

    #[test]
    fn test_empty_body_classified_as_main() {
        let body = json!({});
        let headers = HeaderMap::new();
        let result = classify_request_type(&body, &headers, &default_config());
        assert_eq!(result, RequestType::Main);
    }

    #[test]
    fn test_subagent_detection_disabled_all_to_main() {
        // subagent_detection 关闭时，所有请求都走 Main
        let body = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let mut config = default_config();
        config.compact_detection = false;
        config.subagent_detection = false;
        let result = classify_request_type(&body, &HeaderMap::new(), &config);
        assert_eq!(result, RequestType::Main);
    }
}