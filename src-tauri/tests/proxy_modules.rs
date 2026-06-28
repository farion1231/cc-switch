//! Integration tests for proxy module components:
//!   - types.rs: ProxyConfig, ProxyStatus, RectifierConfig, LogConfig, etc.
//!   - model_mapper.rs: ModelMapping, apply_model_mapping, has_thinking_enabled
//!   - thinking_rectifier.rs: should_rectify_thinking_signature, rectify_anthropic_request
//!   - thinking_budget_rectifier.rs: should_rectify_thinking_budget, rectify_thinking_budget

use cc_switch_lib::proxy::types::{
    AppProxyConfig, GlobalProxyConfig, LogConfig, ProxyConfig, ProxyStatus, ProxyTakeoverStatus,
    RectifierConfig,
};
use cc_switch_lib::proxy::{
    model_mapper::{apply_model_mapping, ModelMapping},
    thinking_budget_rectifier::{rectify_thinking_budget, should_rectify_thinking_budget},
    thinking_rectifier::{rectify_anthropic_request, should_rectify_thinking_signature},
};
use cc_switch_lib::Provider;
use serde_json::json;

// ============================================================
// Helper factories
// ============================================================

fn make_provider_full_mapping() -> Provider {
    Provider {
        id: "full".to_string(),
        name: "Full Mapping".to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped",
                "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-mapped"
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

fn make_provider_no_mapping() -> Provider {
    Provider {
        id: "empty".to_string(),
        name: "No Mapping".to_string(),
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

fn make_provider_default_only() -> Provider {
    Provider {
        id: "default-only".to_string(),
        name: "Default Only".to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_MODEL": "default-model"
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

fn make_provider_empty_string_mapping() -> Provider {
    Provider {
        id: "empty-str".to_string(),
        name: "Empty String Mapping".to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "",
                "ANTHROPIC_MODEL": "fallback-model"
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

fn enabled_config() -> RectifierConfig {
    RectifierConfig {
        enabled: true,
        request_thinking_signature: true,
        request_thinking_budget: true,
        request_media_fallback: true,
        request_media_heuristic: true,
    }
}

fn disabled_config() -> RectifierConfig {
    RectifierConfig {
        enabled: false,
        request_thinking_signature: true,
        request_thinking_budget: true,
        request_media_fallback: true,
        request_media_heuristic: true,
    }
}

// ============================================================
// types.rs tests
// ============================================================

/// Test 1: ProxyConfig default values are sensible
#[test]
fn test_proxy_config_default_values() {
    let config = ProxyConfig::default();
    assert_eq!(config.listen_address, "127.0.0.1");
    assert_eq!(config.listen_port, 15721);
    assert_eq!(config.max_retries, 3);
    assert!(config.enable_logging);
    assert!(!config.live_takeover_active);
    assert_eq!(config.streaming_first_byte_timeout, 60);
    assert_eq!(config.streaming_idle_timeout, 120);
    assert_eq!(config.non_streaming_timeout, 600);
}

/// Test 2: ProxyConfig serialization round-trip via JSON preserves all fields
#[test]
fn test_proxy_config_serde_roundtrip() {
    let config = ProxyConfig {
        listen_address: "0.0.0.0".to_string(),
        listen_port: 8080,
        max_retries: 5,
        request_timeout: 300,
        enable_logging: false,
        live_takeover_active: true,
        streaming_first_byte_timeout: 30,
        streaming_idle_timeout: 60,
        non_streaming_timeout: 120,
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: ProxyConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.listen_address, "0.0.0.0");
    assert_eq!(parsed.listen_port, 8080);
    assert_eq!(parsed.max_retries, 5);
    assert!(!parsed.enable_logging);
    assert!(parsed.live_takeover_active);
    assert_eq!(parsed.streaming_first_byte_timeout, 30);
}

/// Test 3: ProxyConfig missing optional fields use serde defaults
#[test]
fn test_proxy_config_serde_defaults_for_new_fields() {
    // Omit new-generation timeout fields
    let json = r#"{
        "listen_address": "127.0.0.1",
        "listen_port": 15721,
        "max_retries": 3,
        "request_timeout": 600,
        "enable_logging": true
    }"#;
    let config: ProxyConfig = serde_json::from_str(json).expect("deserialize");
    assert_eq!(config.streaming_first_byte_timeout, 60);
    assert_eq!(config.streaming_idle_timeout, 120);
    assert_eq!(config.non_streaming_timeout, 600);
    assert!(!config.live_takeover_active);
}

/// Test 4: ProxyStatus default is zeroed/empty
#[test]
fn test_proxy_status_default() {
    let status = ProxyStatus::default();
    assert!(!status.running);
    assert_eq!(status.total_requests, 0);
    assert_eq!(status.success_requests, 0);
    assert_eq!(status.failed_requests, 0);
    assert!(status.current_provider.is_none());
    assert!(status.last_error.is_none());
    assert!(status.active_targets.is_empty());
}

/// Test 5: RectifierConfig default is all-on; serde defaults work for missing fields
#[test]
fn test_rectifier_config_serde_defaults_and_default_impl() {
    let default_config = RectifierConfig::default();
    assert!(default_config.enabled);
    assert!(default_config.request_thinking_signature);
    assert!(default_config.request_thinking_budget);

    // Only 'enabled' provided; other fields should default to true
    let partial_json = r#"{"enabled": false}"#;
    let parsed: RectifierConfig = serde_json::from_str(partial_json).expect("deserialize");
    assert!(!parsed.enabled);
    assert!(parsed.request_thinking_signature);
    assert!(parsed.request_thinking_budget);
}

/// Test 6: LogConfig to_level_filter maps all levels and respects disabled flag
#[test]
fn test_log_config_to_level_filter_all_levels() {
    let levels = [
        ("error", log::LevelFilter::Error),
        ("warn", log::LevelFilter::Warn),
        ("info", log::LevelFilter::Info),
        ("debug", log::LevelFilter::Debug),
        ("trace", log::LevelFilter::Trace),
        // Case-insensitive
        ("ERROR", log::LevelFilter::Error),
        ("WARN", log::LevelFilter::Warn),
        // Unknown falls back to Info
        ("unknown_level", log::LevelFilter::Info),
    ];

    for (level_str, expected) in &levels {
        let config = LogConfig {
            enabled: true,
            level: level_str.to_string(),
        };
        assert_eq!(
            config.to_level_filter(),
            *expected,
            "level '{}' should map to {:?}",
            level_str,
            expected
        );
    }

    // Disabled overrides any level to Off
    let disabled = LogConfig {
        enabled: false,
        level: "trace".to_string(),
    };
    assert_eq!(disabled.to_level_filter(), log::LevelFilter::Off);
}

/// Test 7: GlobalProxyConfig uses camelCase serde rename
#[test]
fn test_global_proxy_config_camel_case_serde() {
    let json = r#"{"proxyEnabled":true,"listenAddress":"127.0.0.1","listenPort":9090,"enableLogging":false}"#;
    let config: GlobalProxyConfig = serde_json::from_str(json).expect("deserialize");
    assert!(config.proxy_enabled);
    assert_eq!(config.listen_address, "127.0.0.1");
    assert_eq!(config.listen_port, 9090);
    assert!(!config.enable_logging);

    // Round-trip
    let re_json = serde_json::to_string(&config).expect("serialize");
    assert!(re_json.contains("proxyEnabled"));
    assert!(re_json.contains("listenAddress"));
}

/// Test 8: ProxyTakeoverStatus default is all false
#[test]
fn test_proxy_takeover_status_default_all_false() {
    let status = ProxyTakeoverStatus::default();
    assert!(!status.claude);
    assert!(!status.codex);
    assert!(!status.gemini);
    assert!(!status.opencode);
    assert!(!status.openclaw);
}

// ============================================================
// model_mapper.rs tests
// ============================================================

/// Test 9: ModelMapping::from_provider extracts all fields; empty strings become None
#[test]
fn test_model_mapping_from_provider_full() {
    let provider = make_provider_full_mapping();
    let mapping = ModelMapping::from_provider(&provider);
    assert!(mapping.has_mapping());
    assert_eq!(mapping.haiku_model.as_deref(), Some("haiku-mapped"));
    assert_eq!(mapping.sonnet_model.as_deref(), Some("sonnet-mapped"));
    assert_eq!(mapping.opus_model.as_deref(), Some("opus-mapped"));
    assert_eq!(mapping.fable_model.as_deref(), Some("fable-mapped"));
    assert_eq!(mapping.default_model.as_deref(), Some("default-model"));
}

/// Test 10: Empty-string env values are treated as None (not mapped)
#[test]
fn test_model_mapping_empty_string_treated_as_none() {
    let provider = make_provider_empty_string_mapping();
    let mapping = ModelMapping::from_provider(&provider);
    // sonnet_model is "" so it should be None
    assert!(mapping.sonnet_model.is_none());
    // default_model is "fallback-model" so it should exist
    assert_eq!(mapping.default_model.as_deref(), Some("fallback-model"));
}

/// Test 11: apply_model_mapping maps sonnet model
#[test]
fn test_apply_model_mapping_sonnet() {
    let provider = make_provider_full_mapping();
    let body = json!({"model": "claude-sonnet-4-5"});
    let (result, original, mapped) = apply_model_mapping(body, &provider);
    assert_eq!(result["model"], "sonnet-mapped");
    assert_eq!(original.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(mapped.as_deref(), Some("sonnet-mapped"));
}

/// Test 12: apply_model_mapping maps fable model and preserves other fields
#[test]
fn test_apply_model_mapping_fable_preserves_other_fields() {
    let provider = make_provider_full_mapping();
    let body = json!({"model": "claude-fable-5", "max_tokens": 1024});
    let (result, original, mapped) = apply_model_mapping(body, &provider);
    assert_eq!(result["model"], "fable-mapped");
    assert_eq!(original.as_deref(), Some("claude-fable-5"));
    assert_eq!(mapped.as_deref(), Some("fable-mapped"));
    assert_eq!(result["max_tokens"], 1024);
}

/// Test 13: apply_model_mapping – no mapping configured, body unchanged
#[test]
fn test_apply_model_mapping_no_mapping_passthrough() {
    let provider = make_provider_no_mapping();
    let body = json!({"model": "claude-haiku-3", "max_tokens": 1024});
    let (result, original, mapped) = apply_model_mapping(body, &provider);
    assert_eq!(result["model"], "claude-haiku-3");
    assert_eq!(original.as_deref(), Some("claude-haiku-3"));
    assert!(mapped.is_none(), "mapped should be None when no config");
    // Other fields preserved
    assert_eq!(result["max_tokens"], 1024);
}

/// Test 14: apply_model_mapping – only default model configured maps unknown models
#[test]
fn test_apply_model_mapping_default_only_maps_unknown() {
    let provider = make_provider_default_only();
    let body = json!({"model": "claude-sonnet-4-5"});
    let (result, original, mapped) = apply_model_mapping(body, &provider);
    assert_eq!(result["model"], "default-model");
    assert_eq!(original.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(mapped.as_deref(), Some("default-model"));
}

/// Test 15: apply_model_mapping – case-insensitive model type detection
#[test]
fn test_apply_model_mapping_case_insensitive() {
    let provider = make_provider_full_mapping();

    // All-caps HAIKU
    let (result, _, mapped) = apply_model_mapping(json!({"model": "CLAUDE-HAIKU-3"}), &provider);
    assert_eq!(result["model"], "haiku-mapped");
    assert_eq!(mapped.as_deref(), Some("haiku-mapped"));

    // Mixed-case Opus
    let (result2, _, mapped2) = apply_model_mapping(json!({"model": "Claude-Opus-4-5"}), &provider);
    assert_eq!(result2["model"], "opus-mapped");
    assert_eq!(mapped2.as_deref(), Some("opus-mapped"));
}

// ============================================================
// thinking_rectifier.rs tests
// ============================================================

/// Test 16: should_rectify_thinking_signature detects all documented error scenarios
#[test]
fn test_should_rectify_thinking_signature_all_scenarios() {
    let cfg = enabled_config();

    // Scenario 1 – invalid signature in thinking block
    assert!(should_rectify_thinking_signature(
        Some("Invalid `signature` in `thinking` block"),
        &cfg
    ));

    // Scenario 2 – must start with thinking block
    assert!(should_rectify_thinking_signature(
        Some("must start with a thinking block"),
        &cfg
    ));

    // Scenario 3 – expected thinking found tool_use
    assert!(should_rectify_thinking_signature(
        Some("Expected `thinking` or `redacted_thinking`, but found `tool_use`"),
        &cfg
    ));

    // Scenario 4 – signature field required
    assert!(should_rectify_thinking_signature(
        Some("signature: Field required"),
        &cfg
    ));

    // Scenario 5 – extra inputs not permitted
    assert!(should_rectify_thinking_signature(
        Some("xxx.signature: Extra inputs are not permitted"),
        &cfg
    ));

    // Scenario 6 – blocks cannot be modified
    assert!(should_rectify_thinking_signature(
        Some("thinking or redacted_thinking blocks cannot be modified"),
        &cfg
    ));

    // Scenario 7 – invalid request (Chinese/English/generic)
    assert!(should_rectify_thinking_signature(Some("非法请求"), &cfg));
    assert!(should_rectify_thinking_signature(
        Some("illegal request"),
        &cfg
    ));
    assert!(should_rectify_thinking_signature(
        Some("invalid request"),
        &cfg
    ));
}

/// Test 17: should_rectify_thinking_signature respects config switches
#[test]
fn test_should_rectify_thinking_signature_respects_config_switches() {
    let error = Some("Invalid `signature` in `thinking` block");

    // Master switch off
    assert!(!should_rectify_thinking_signature(
        error,
        &disabled_config()
    ));

    // Sub-switch off
    let sub_off = RectifierConfig {
        enabled: true,
        request_thinking_signature: false,
        request_thinking_budget: true,
        request_media_fallback: true,
        request_media_heuristic: true,
    };
    assert!(!should_rectify_thinking_signature(error, &sub_off));

    // None error message
    assert!(!should_rectify_thinking_signature(None, &enabled_config()));

    // Unrelated error
    assert!(!should_rectify_thinking_signature(
        Some("network timeout"),
        &enabled_config()
    ));

    // Scenario 3 without tool_use should NOT trigger
    assert!(!should_rectify_thinking_signature(
        Some("Expected `thinking` or `redacted_thinking`, but found `text`"),
        &enabled_config()
    ));
}

/// Test 18: rectify_anthropic_request removes thinking/redacted_thinking blocks and stray signatures
#[test]
fn test_rectify_anthropic_request_removes_thinking_blocks_and_signatures() {
    let mut body = json!({
        "model": "claude-test",
        "messages": [{
            "role": "assistant",
            "content": [
                { "type": "thinking", "thinking": "internal", "signature": "sig1" },
                { "type": "redacted_thinking", "data": "r", "signature": "sig_r" },
                { "type": "text", "text": "hello", "signature": "sig_text" },
                { "type": "tool_use", "id": "tu1", "name": "Search", "input": {} }
            ]
        }]
    });

    let result = rectify_anthropic_request(&mut body);

    assert!(result.applied);
    assert_eq!(result.removed_thinking_blocks, 1);
    assert_eq!(result.removed_redacted_thinking_blocks, 1);
    assert_eq!(result.removed_signature_fields, 1); // only sig_text (tool_use has none)

    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert!(
        content[0].get("signature").is_none(),
        "signature should be removed from text block"
    );
    assert_eq!(content[1]["type"], "tool_use");
}

/// Test 19: rectify_anthropic_request removes top-level thinking when tool_use has no thinking prefix
#[test]
fn test_rectify_anthropic_request_removes_top_level_thinking_for_tool_use_without_thinking_prefix()
{
    let mut body = json!({
        "model": "claude-test",
        "thinking": { "type": "enabled", "budget_tokens": 1024 },
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "tu1", "name": "Search", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "tu1", "content": "result" }]
            }
        ]
    });

    let result = rectify_anthropic_request(&mut body);

    assert!(result.applied);
    assert!(
        body.get("thinking").is_none(),
        "top-level thinking should be removed"
    );
}

/// Test 20: rectify_anthropic_request is a no-op on clean messages
#[test]
fn test_rectify_anthropic_request_noop_on_clean_messages() {
    let mut body = json!({
        "model": "claude-test",
        "messages": [{
            "role": "user",
            "content": [{ "type": "text", "text": "hello" }]
        }]
    });

    let result = rectify_anthropic_request(&mut body);

    assert!(!result.applied);
    assert_eq!(result.removed_thinking_blocks, 0);
    assert_eq!(result.removed_redacted_thinking_blocks, 0);
    assert_eq!(result.removed_signature_fields, 0);
}

// ============================================================
// thinking_budget_rectifier.rs tests
// ============================================================

/// Test 21: should_rectify_thinking_budget triggers only with correct triple condition
#[test]
fn test_should_rectify_thinking_budget_trigger_conditions() {
    let cfg = enabled_config();

    // Triggers when all three conditions present
    assert!(should_rectify_thinking_budget(
        Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
        &cfg
    ));
    assert!(should_rectify_thinking_budget(
        Some("thinking budget_tokens must be >= 1024"),
        &cfg
    ));

    // Does NOT trigger without all three conditions
    assert!(!should_rectify_thinking_budget(
        Some("budget_tokens must be less than max_tokens"),
        &cfg
    ));
    assert!(!should_rectify_thinking_budget(
        Some("Request timeout"),
        &cfg
    ));
    assert!(!should_rectify_thinking_budget(None, &cfg));
}

/// Test 22: should_rectify_thinking_budget respects config switches
#[test]
fn test_should_rectify_thinking_budget_config_switches() {
    let error = Some("thinking.budget_tokens: Input should be greater than or equal to 1024");

    // Master switch off
    assert!(!should_rectify_thinking_budget(error, &disabled_config()));

    // Sub-switch off
    let sub_off = RectifierConfig {
        enabled: true,
        request_thinking_signature: true,
        request_thinking_budget: false,
        request_media_fallback: true,
        request_media_heuristic: true,
    };
    assert!(!should_rectify_thinking_budget(error, &sub_off));
}

/// Test 23: rectify_thinking_budget upgrades budget and max_tokens, skips adaptive
#[test]
fn test_rectify_thinking_budget_upgrades_values_and_skips_adaptive() {
    // Normal case: upgrades budget and max_tokens
    let mut body = json!({
        "model": "claude-test",
        "thinking": { "type": "enabled", "budget_tokens": 512 },
        "max_tokens": 1024
    });
    let result = rectify_thinking_budget(&mut body);
    assert!(result.applied);
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 32000u64);
    assert_eq!(body["max_tokens"], 64000u64);

    // Adaptive: must not be modified
    let mut adaptive_body = json!({
        "model": "claude-test",
        "thinking": { "type": "adaptive", "budget_tokens": 512 },
        "max_tokens": 1024
    });
    let adaptive_result = rectify_thinking_budget(&mut adaptive_body);
    assert!(!adaptive_result.applied);
    assert_eq!(adaptive_body["thinking"]["type"], "adaptive");
    assert_eq!(adaptive_body["thinking"]["budget_tokens"], 512);
    assert_eq!(adaptive_body["max_tokens"], 1024);
}

/// Test 24: rectify_thinking_budget preserves large max_tokens; creates thinking object if missing
#[test]
fn test_rectify_thinking_budget_preserves_large_max_tokens_and_creates_missing_object() {
    // Preserves large max_tokens (>= 32001)
    let mut body_large = json!({
        "model": "claude-test",
        "thinking": { "type": "enabled", "budget_tokens": 100 },
        "max_tokens": 100_000u64
    });
    let r = rectify_thinking_budget(&mut body_large);
    assert!(r.applied);
    assert_eq!(
        body_large["max_tokens"], 100_000u64,
        "large max_tokens must not be reduced"
    );

    // Missing thinking object: created and set to enabled
    let mut body_missing = json!({"model": "claude-test", "max_tokens": 512u64});
    let r2 = rectify_thinking_budget(&mut body_missing);
    assert!(r2.applied);
    assert_eq!(body_missing["thinking"]["type"], "enabled");
    assert_eq!(body_missing["thinking"]["budget_tokens"], 32000u64);
    assert_eq!(body_missing["max_tokens"], 64000u64);
}

/// Test 25: AppProxyConfig serde uses camelCase field names
#[test]
fn test_app_proxy_config_camel_case_serde() {
    let json = r#"{
        "appType": "claude",
        "enabled": true,
        "autoFailoverEnabled": false,
        "maxRetries": 2,
        "streamingFirstByteTimeout": 30,
        "streamingIdleTimeout": 60,
        "nonStreamingTimeout": 300,
        "circuitFailureThreshold": 5,
        "circuitSuccessThreshold": 2,
        "circuitTimeoutSeconds": 30,
        "circuitErrorRateThreshold": 0.5,
        "circuitMinRequests": 10
    }"#;
    let config: AppProxyConfig = serde_json::from_str(json).expect("deserialize AppProxyConfig");
    assert_eq!(config.app_type, "claude");
    assert!(config.enabled);
    assert!(!config.auto_failover_enabled);
    assert_eq!(config.max_retries, 2);
    assert_eq!(config.streaming_first_byte_timeout, 30);

    // Round-trip
    let re_json = serde_json::to_string(&config).expect("serialize");
    assert!(
        re_json.contains("appType"),
        "should use camelCase: {re_json}"
    );
    assert!(
        re_json.contains("autoFailoverEnabled"),
        "should use camelCase: {re_json}"
    );
}
