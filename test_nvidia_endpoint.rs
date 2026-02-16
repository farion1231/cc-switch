//! Test case demonstrating the Nvidia provider endpoint bug
//!
//! The issue: build_url() adds ?beta=true to OpenAI Chat Completions endpoints,
//! which causes Nvidia's API to reject the request.

#[cfg(test)]
mod nvidia_endpoint_tests {
    use crate::proxy::providers::ClaudeAdapter;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn create_nvidia_provider() -> Provider {
        Provider {
            id: "nvidia-test".to_string(),
            name: "Nvidia".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://integrate.api.nvidia.com",
                    "ANTHROPIC_AUTH_TOKEN": "nvapi-test-key"
                }
            }),
            website_url: Some("https://build.nvidia.com".to_string()),
            category: Some("aggregator".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            }),
            icon: Some("nvidia".to_string()),
            icon_color: Some("#000000".to_string()),
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_nvidia_openai_chat_endpoint() {
        let adapter = ClaudeAdapter::new();
        let provider = create_nvidia_provider();

        // Verify needs_transform returns true for Nvidia
        assert!(adapter.needs_transform(&provider),
            "Nvidia provider with api_format='openai_chat' should need transform");

        // The forwarder will remap /v1/messages to /v1/chat/completions
        let openai_endpoint = "/v1/chat/completions";
        let base_url = "https://integrate.api.nvidia.com";

        let url = adapter.build_url(base_url, openai_endpoint);

        // BUG: Current implementation produces:
        // "https://integrate.api.nvidia.com/v1/chat/completions?beta=true"
        //
        // Expected (after fix):
        // "https://integrate.api.nvidia.com/v1/chat/completions"

        println!("Nvidia OpenAI Chat URL: {}", url);

        // This assertion will FAIL with the current bug
        assert_eq!(url, "https://integrate.api.nvidia.com/v1/chat/completions",
            "OpenAI Chat Completions endpoint should NOT have ?beta=true parameter");
    }

    #[test]
    fn test_anthropic_endpoint_still_gets_beta() {
        let adapter = ClaudeAdapter::new();
        let anthropic_provider = Provider {
            id: "anthropic-test".to_string(),
            name: "Anthropic".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-ant-test"
                }
            }),
            website_url: Some("https://www.anthropic.com".to_string()),
            category: Some("official".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                api_format: Some("anthropic".to_string()),
                ..Default::default()
            }),
            icon: Some("anthropic".to_string()),
            icon_color: Some("#D4915D".to_string()),
            in_failover_queue: false,
        };

        // Verify needs_transform returns false for Anthropic
        assert!(!adapter.needs_transform(&anthropic_provider),
            "Anthropic provider with api_format='anthropic' should NOT need transform");

        let anthropic_endpoint = "/v1/messages";
        let base_url = "https://api.anthropic.com";

        let url = adapter.build_url(base_url, anthropic_endpoint);

        // This SHOULD have ?beta=true
        assert_eq!(url, "https://api.anthropic.com/v1/messages?beta=true",
            "Anthropic Messages endpoint SHOULD have ?beta=true parameter");
    }

    #[test]
    fn test_nvidia_forwarder_flow() {
        // Simulates the complete flow from forwarder
        let provider = create_nvidia_provider();
        let adapter = ClaudeAdapter::new();

        // Check transformation is needed
        let needs_transform = adapter.needs_transform(&provider);
        assert!(needs_transform, "Nvidia should need transform");

        // Forwarder remaps endpoint
        let original_endpoint = "/v1/messages";
        let effective_endpoint = if needs_transform && adapter.name() == "Claude" && original_endpoint == "/v1/messages" {
            "/v1/chat/completions"
        } else {
            original_endpoint
        };

        assert_eq!(effective_endpoint, "/v1/chat/completions",
            "Endpoint should be remapped for openai_chat format");

        // Build final URL
        let base_url = adapter.extract_base_url(&provider).unwrap();
        let final_url = adapter.build_url(&base_url, effective_endpoint);

        println!("Nvidia complete flow URL: {}", final_url);

        // The bug: final_url contains ?beta=true which Nvidia rejects
        // Expected: "https://integrate.api.nvidia.com/v1/chat/completions"
        // Actual: "https://integrate.api.nvidia.com/v1/chat/completions?beta=true"

        assert!(!final_url.contains("?beta=true"),
            "OpenAI Chat endpoint should NOT contain ?beta=true parameter");
    }
}
