use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEndpoint {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorModelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "type")]
    pub provider_type: String,
    #[serde(default)]
    pub provider_group: String,
    #[serde(default)]
    pub endpoint_id: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(default)]
    pub pricing_model: String,
    #[serde(default = "default_tooltip")]
    pub tooltip_data: String,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    #[serde(default = "default_openai_endpoint", rename = "openAIEndpoint")]
    pub open_ai_endpoint: String,
    #[serde(default, rename = "openAIExtraParamsEnabled")]
    pub open_ai_extra_params_enabled: bool,
    #[serde(default, rename = "openAIExtraParamsJSON")]
    pub open_ai_extra_params_json: String,
    #[serde(default)]
    pub custom_headers_enabled: bool,
    #[serde(default, rename = "customHeadersJSON")]
    pub custom_headers_json: String,
    #[serde(default)]
    pub anthropic_extra_params_enabled: bool,
    #[serde(default, rename = "anthropicExtraParamsJSON")]
    pub anthropic_extra_params_json: String,
    #[serde(default)]
    pub context_window_tokens: i64,
    #[serde(default)]
    pub max_completion_tokens: i64,
    #[serde(default)]
    pub anthropic_max_tokens: i64,
    #[serde(default = "default_anthropic_effort")]
    pub anthropic_thinking_effort: String,
    #[serde(default)]
    pub thinking_budget_tokens: i64,
}

impl Default for CursorModelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider_type: "openai".to_string(),
            provider_group: String::new(),
            endpoint_id: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model_id: String::new(),
            pricing_model: String::new(),
            tooltip_data: default_tooltip(),
            reasoning_effort: default_reasoning_effort(),
            open_ai_endpoint: default_openai_endpoint(),
            open_ai_extra_params_enabled: false,
            open_ai_extra_params_json: String::new(),
            custom_headers_enabled: false,
            custom_headers_json: String::new(),
            anthropic_extra_params_enabled: false,
            anthropic_extra_params_json: String::new(),
            context_window_tokens: 0,
            max_completion_tokens: 0,
            anthropic_max_tokens: 0,
            anthropic_thinking_effort: default_anthropic_effort(),
            thinking_budget_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarModelAdapter {
    #[serde(default)]
    pub source_provider_id: String,
    #[serde(default)]
    pub source_provider_name: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub tooltip_data: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(default)]
    pub pricing_model: String,
    pub reasoning_effort: String,
    #[serde(rename = "openAIEndpoint")]
    pub open_ai_endpoint: String,
    #[serde(rename = "openAIExtraParamsEnabled")]
    pub open_ai_extra_params_enabled: bool,
    #[serde(rename = "openAIExtraParamsJSON")]
    pub open_ai_extra_params_json: String,
    pub custom_headers_enabled: bool,
    #[serde(rename = "customHeadersJSON")]
    pub custom_headers_json: String,
    pub anthropic_extra_params_enabled: bool,
    #[serde(rename = "anthropicExtraParamsJSON")]
    pub anthropic_extra_params_json: String,
    pub context_window_tokens: i64,
    pub max_completion_tokens: i64,
    pub anthropic_max_tokens: i64,
    #[serde(default)]
    pub anthropic_thinking_effort: String,
    pub thinking_budget_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarConfig {
    pub log: bool,
    pub provider_stream_idle_timeout: i64,
    pub backend_listen_addr: String,
    pub proxy_listen_addr: String,
    pub model_adapters: Vec<SidecarModelAdapter>,
    pub routing: SidecarRoutingConfig,
    pub home_metrics: SidecarHomeMetricsConfig,
    pub last_agent_model_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarRoutingConfig {
    pub mode: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarHomeMetricsConfig {
    pub include_cache_write_in_hit_rate: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SidecarRuntimeState {
    pub backend_listen_addr: String,
    pub backend_running: bool,
    pub proxy_listen_addr: String,
    pub proxy_running: bool,
    pub cursor_settings_applied: bool,
    pub ca_installed: bool,
    #[serde(default)]
    pub ca_fingerprint: String,
    pub last_error: String,
}

impl SidecarRuntimeState {
    pub(crate) fn into_runtime_state(
        self,
        phase: impl Into<String>,
        sidecar_running: bool,
        platform: impl Into<String>,
    ) -> CursorRuntimeState {
        CursorRuntimeState {
            phase: phase.into(),
            sidecar_running,
            backend_listen_addr: self.backend_listen_addr,
            backend_running: self.backend_running,
            proxy_listen_addr: self.proxy_listen_addr,
            proxy_running: self.proxy_running,
            cursor_settings_applied: self.cursor_settings_applied,
            ca_installed: self.ca_installed,
            ca_fingerprint: self.ca_fingerprint,
            platform: platform.into(),
            last_error: self.last_error,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorRuntimeState {
    pub phase: String,
    pub sidecar_running: bool,
    pub backend_listen_addr: String,
    pub backend_running: bool,
    pub proxy_listen_addr: String,
    pub proxy_running: bool,
    pub cursor_settings_applied: bool,
    pub ca_installed: bool,
    #[serde(default)]
    pub ca_fingerprint: String,
    pub platform: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorModelTestResult {
    pub adapter_id: String,
    pub status: String,
    pub tokens_per_second: f64,
    pub first_text_token_ms: i64,
    pub total_duration_ms: i64,
    pub output_tokens: i64,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorUsageEventPage {
    pub events: Vec<CursorUsageEvent>,
    pub next_cursor: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorUsageEvent {
    pub sequence: i64,
    pub event_id: String,
    #[serde(default = "default_cursor_usage_event_kind")]
    pub kind: String,
    pub status: String,
    pub source_provider_id: String,
    pub source_provider_name: String,
    pub provider_type: String,
    pub channel_id: String,
    pub request_model: String,
    pub model: String,
    pub pricing_model: String,
    pub status_code: i64,
    #[serde(default)]
    pub error: String,
    pub latency_ms: i64,
    pub first_token_ms: i64,
    pub duration_ms: i64,
    pub is_streaming: bool,
    pub at: chrono::DateTime<chrono::Utc>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub usage_present: bool,
    #[serde(default)]
    pub usage_status: String,
    #[serde(default)]
    pub cache_usage_observed: bool,
}

fn default_tooltip() -> String {
    "Managed by CC Switch".to_string()
}
fn default_reasoning_effort() -> String {
    "medium".to_string()
}
fn default_openai_endpoint() -> String {
    "/v1/responses".to_string()
}
fn default_anthropic_effort() -> String {
    "xhigh".to_string()
}
fn default_cursor_usage_event_kind() -> String {
    "provider_call".to_string()
}

#[cfg(test)]
mod tests {
    use super::{CursorModelConfig, CursorUsageEvent, SidecarModelAdapter, SidecarRuntimeState};

    #[test]
    fn cursor_usage_event_defaults_legacy_kind_to_provider_call() {
        let event: CursorUsageEvent = serde_json::from_value(serde_json::json!({
            "sequence": 1,
            "eventId": "legacy-event",
            "status": "completed",
            "sourceProviderId": "provider",
            "sourceProviderName": "Provider",
            "providerType": "openai",
            "channelId": "channel",
            "requestModel": "alias",
            "model": "model",
            "pricingModel": "model",
            "statusCode": 200,
            "latencyMs": 10,
            "firstTokenMs": 2,
            "durationMs": 12,
            "isStreaming": true,
            "at": "2026-03-14T00:00:00Z",
            "inputTokens": 10,
            "outputTokens": 2,
            "cacheReadTokens": 0,
            "cacheWriteTokens": 0,
            "usagePresent": true
        }))
        .expect("deserialize legacy usage event");

        assert_eq!(event.kind, "provider_call");
    }

    #[test]
    fn sidecar_adapter_uses_go_control_protocol_acronyms() {
        let adapter = SidecarModelAdapter {
            source_provider_id: "provider-1".to_string(),
            source_provider_name: "Provider 1".to_string(),
            display_name: "Model 1".to_string(),
            provider_type: "openai".to_string(),
            base_url: "https://example.com".to_string(),
            api_key: "secret".to_string(),
            tooltip_data: "Managed by CC Switch".to_string(),
            model_id: "gpt-test".to_string(),
            pricing_model: "gpt-test-priced".to_string(),
            reasoning_effort: "medium".to_string(),
            open_ai_endpoint: "/v1/responses".to_string(),
            open_ai_extra_params_enabled: false,
            open_ai_extra_params_json: String::new(),
            custom_headers_enabled: false,
            custom_headers_json: String::new(),
            anthropic_extra_params_enabled: false,
            anthropic_extra_params_json: String::new(),
            context_window_tokens: 0,
            max_completion_tokens: 0,
            anthropic_max_tokens: 0,
            anthropic_thinking_effort: "xhigh".to_string(),
            thinking_budget_tokens: 0,
        };

        let value = serde_json::to_value(adapter).expect("serialize sidecar adapter");
        assert_eq!(value["baseURL"], "https://example.com");
        assert_eq!(value["apiKey"], "secret");
        assert_eq!(value["modelID"], "gpt-test");
        assert_eq!(value["pricingModel"], "gpt-test-priced");
        assert_eq!(value["openAIEndpoint"], "/v1/responses");
        assert!(value.get("baseUrl").is_none());
        assert!(value.get("modelId").is_none());
    }

    #[test]
    fn sidecar_adapter_accepts_go_omitted_fields() {
        let adapter: SidecarModelAdapter = serde_json::from_value(serde_json::json!({
            "displayName": "OpenAI Model",
            "type": "openai",
            "baseURL": "https://example.com",
            "apiKey": "secret",
            "tooltipData": "Managed by CC Switch",
            "modelID": "gpt-test",
            "reasoningEffort": "medium",
            "openAIEndpoint": "/v1/responses",
            "openAIExtraParamsEnabled": false,
            "openAIExtraParamsJSON": "",
            "customHeadersEnabled": false,
            "customHeadersJSON": "",
            "anthropicExtraParamsEnabled": false,
            "anthropicExtraParamsJSON": "",
            "contextWindowTokens": 0,
            "maxCompletionTokens": 0,
            "anthropicMaxTokens": 0,
            "thinkingBudgetTokens": 0
        }))
        .expect("deserialize normalized Go adapter response");

        assert_eq!(adapter.provider_type, "openai");
        assert!(adapter.source_provider_id.is_empty());
        assert!(adapter.source_provider_name.is_empty());
        assert!(adapter.pricing_model.is_empty());
        assert!(adapter.anthropic_thinking_effort.is_empty());
    }

    #[test]
    fn sidecar_state_does_not_require_cc_switch_managed_fields() {
        let sidecar_state: SidecarRuntimeState = serde_json::from_value(serde_json::json!({
            "backendListenAddr": "127.0.0.1:10001",
            "backendRunning": true,
            "proxyListenAddr": "127.0.0.1:10002",
            "proxyRunning": true,
            "cursorSettingsApplied": true,
            "caInstalled": true,
            "caFingerprint": "AA:BB",
            "lastError": ""
        }))
        .expect("deserialize Go sidecar state");

        let runtime_state = sidecar_state.into_runtime_state("running", true, "macos");
        assert_eq!(runtime_state.phase, "running");
        assert!(runtime_state.sidecar_running);
        assert_eq!(runtime_state.platform, "macos");
        assert!(runtime_state.backend_running);
        assert!(runtime_state.proxy_running);
        assert!(runtime_state.cursor_settings_applied);
        assert!(runtime_state.ca_installed);
    }

    #[test]
    fn provider_config_accepts_frontend_camel_case() {
        let config: CursorModelConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "type": "anthropic",
            "baseURL": "https://api.anthropic.com",
            "apiKey": "secret",
            "modelID": "claude-test",
            "pricingModel": "claude-priced",
            "openAIEndpoint": "/v1/responses",
            "openAIExtraParamsJSON": "{}",
            "customHeadersJSON": "{}",
            "anthropicExtraParamsJSON": "{}"
        }))
        .expect("deserialize provider config");

        assert_eq!(config.base_url, "https://api.anthropic.com");
        assert_eq!(config.model_id, "claude-test");
        assert_eq!(config.pricing_model, "claude-priced");
    }
}
