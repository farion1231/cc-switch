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

    // 智能路由的子代理检测始终开启，不受 Copilot 优化器 subagent_detection 配置影响。
    // Copilot 优化器开关控制的是是否注入 x-initiator 计费头，与路由决策是独立的关注点。
    let classification = classify_request(
        body,
        has_anthropic_beta,
        copilot_config.compact_detection,
        true, // 智能路由始终检测子代理
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
    fn test_subagent_detection_works_regardless_of_config_flag() {
        // 即使 copilot_config.subagent_detection = false，
        // 智能路由也始终启用子代理检测（路由决策独立于 Copilot 计费优化）
        let body = json!({
            "messages": [{"role": "user", "content": "__SUBAGENT_MARKER__\nDo something"}]
        });
        let mut config = default_config();
        config.subagent_detection = false; // Copilot 优化器关闭了子代理检测
        let result = classify_request_type(&body, &HeaderMap::new(), &config);
        // 智能路由始终检测子代理，不受 Copilot 配置影响
        assert_eq!(result, RequestType::Others);
    }

    #[test]
    fn test_non_subagent_when_config_disabled_still_main() {
        // 普通用户消息：即使 config flag 怎么设，都走 Main
        let body = json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let mut config = default_config();
        config.subagent_detection = false;
        let result = classify_request_type(&body, &HeaderMap::new(), &config);
        assert_eq!(result, RequestType::Main);
    }
}