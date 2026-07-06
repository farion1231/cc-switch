#![allow(dead_code)]

use std::fs;

#[path = "../src/pi_config.rs"]
mod pi_config;
#[path = "../src/services/pi_provider/mod.rs"]
mod pi_provider;

#[test]
fn missing_models_json_reads_as_empty_providers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("models.json");

    let loaded = pi_config::read_models_json_at(&path).expect("read missing models");

    assert_eq!(loaded.value["providers"], serde_json::json!({}));
    assert!(loaded.file_hash.is_empty());
}

#[test]
fn valid_models_json_is_loaded_with_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("models.json");
    fs::write(
        &path,
        r#"{"providers":{"ollama":{"baseUrl":"http://localhost:11434/v1"}},"x":true}"#,
    )
    .expect("write fixture");

    let loaded = pi_config::read_models_json_at(&path).expect("read valid models");

    assert_eq!(loaded.value["x"], serde_json::json!(true));
    assert!(!loaded.file_hash.is_empty());
}

#[test]
fn invalid_models_json_returns_parse_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("models.json");
    fs::write(&path, r#"{"providers":"#).expect("write invalid fixture");

    let err = pi_config::read_models_json_at(&path).expect_err("invalid json should fail");

    assert!(err.to_string().contains("Failed to parse Pi models.json"));
}

#[test]
fn backup_and_rollback_restores_models_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let models_path = dir.path().join("models.json");
    let backups_dir = dir.path().join("backups");
    fs::write(
        &models_path,
        r#"{"providers":{"a":{"baseUrl":"https://a"}}}"#,
    )
    .expect("write original");

    let backup = pi_config::create_backup_at(&models_path, &backups_dir).expect("backup");
    fs::write(
        &models_path,
        r#"{"providers":{"b":{"baseUrl":"https://b"}}}"#,
    )
    .expect("overwrite");

    pi_config::rollback_backup_at(&models_path, &backup.path).expect("rollback");

    let restored = fs::read_to_string(&models_path).expect("read restored");
    assert!(restored.contains(r#""a""#));
    assert!(!restored.contains(r#""b""#));
}

#[test]
fn expected_hash_mismatch_blocks_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let models_path = dir.path().join("models.json");
    fs::write(&models_path, r#"{"providers":{}}"#).expect("write original");

    let result = pi_config::write_models_json_with_expected_hash_at(
        &models_path,
        &serde_json::json!({"providers":{"x":{}}}),
        "wrong-hash",
    );

    assert!(result
        .expect_err("must fail")
        .to_string()
        .contains("changed on disk"));
}

#[test]
fn custom_provider_draft_maps_to_pi_models_json() {
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::Custom,
        provider_id: "my-openai".to_string(),
        template: pi_provider::PiProviderTemplate::OpenAiCompatible,
        base_url: Some("https://api.example.com/v1".to_string()),
        api: "openai-completions".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::Env,
            value: "MY_OPENAI_KEY".to_string(),
        },
        headers: vec![pi_provider::PiHeaderDraft {
            key: "x-extra".to_string(),
            value: "$EXTRA".to_string(),
        }],
        models: vec![pi_provider::PiModelDraft {
            id: "model-a".to_string(),
            name: None,
            name_touched: false,
            reasoning: Some(true),
            input: Some(vec!["text".to_string(), "image".to_string()]),
            context_window: Some(200000),
            max_tokens: Some(32000),
            cost: None,
        }],
        compat: None,
        advanced_json: None,
    };

    let provider = pi_provider::draft_to_provider_value(&draft).expect("map draft");

    assert_eq!(provider["baseUrl"], "https://api.example.com/v1");
    assert_eq!(provider["api"], "openai-completions");
    assert_eq!(provider["apiKey"], "$MY_OPENAI_KEY");
    assert_eq!(provider["headers"]["x-extra"], "$EXTRA");
    assert_eq!(provider["models"][0]["id"], "model-a");
    assert_eq!(provider["models"][0]["reasoning"], true);
    assert_eq!(provider["models"][0]["input"][0], "text");
    assert_eq!(provider["models"][0]["input"][1], "image");
    assert_eq!(provider["models"][0]["contextWindow"], 200000);
    assert_eq!(provider["models"][0]["maxTokens"], 32000);
}

#[test]
fn compat_flags_are_serialized_correctly() {
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::Custom,
        provider_id: "local-llm".to_string(),
        template: pi_provider::PiProviderTemplate::LocalOpenAiCompatible,
        base_url: Some("http://localhost:11434/v1".to_string()),
        api: "openai-completions".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::Literal,
            value: "ollama".to_string(),
        },
        headers: vec![],
        models: vec![pi_provider::PiModelDraft {
            id: "llama3.1:8b".to_string(),
            name: Some("Llama 3.1 8B".to_string()),
            name_touched: true,
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: None,
        }],
        compat: Some(pi_provider::PiProviderCompat {
            supports_developer_role: Some(false),
            supports_reasoning_effort: Some(false),
            supports_usage_in_streaming: None,
            max_tokens_field: Some("max_tokens".to_string()),
            thinking_format: None,
            supports_eager_tool_input_streaming: None,
            supports_long_cache_retention: None,
            force_adaptive_thinking: None,
            allow_empty_signature: None,
        }),
        advanced_json: None,
    };

    let provider = pi_provider::draft_to_provider_value(&draft).expect("map compat draft");

    assert_eq!(provider["compat"]["supportsDeveloperRole"], false);
    assert_eq!(provider["compat"]["supportsReasoningEffort"], false);
    assert_eq!(provider["compat"]["maxTokensField"], "max_tokens");
    assert!(provider["compat"].get("supportsUsageInStreaming").is_none());
}

#[test]
fn advanced_json_cannot_override_managed_fields() {
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::Custom,
        provider_id: "bad".to_string(),
        template: pi_provider::PiProviderTemplate::Custom,
        base_url: Some("https://api.example.com".to_string()),
        api: "openai-completions".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::None,
            value: String::new(),
        },
        headers: vec![],
        models: vec![pi_provider::PiModelDraft {
            id: "m".to_string(),
            name: None,
            name_touched: false,
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: None,
        }],
        compat: None,
        advanced_json: Some(serde_json::json!({"apiKey":"sk-override"})),
    };

    let err = pi_provider::draft_to_provider_value(&draft).expect_err("managed override rejected");

    assert!(err
        .to_string()
        .contains("advancedJson cannot override apiKey"));
}

#[test]
fn apply_draft_preserves_unknown_root_fields() {
    let current = serde_json::json!({
        "providers": {
            "existing": { "baseUrl": "https://existing" }
        },
        "unknown": true
    });
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::Custom,
        provider_id: "new-provider".to_string(),
        template: pi_provider::PiProviderTemplate::OpenAiCompatible,
        base_url: Some("https://new.example/v1".to_string()),
        api: "openai-completions".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::None,
            value: String::new(),
        },
        headers: vec![],
        models: vec![pi_provider::PiModelDraft {
            id: "m".to_string(),
            name: None,
            name_touched: false,
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: None,
        }],
        compat: None,
        advanced_json: None,
    };

    let next = pi_provider::upsert_provider_value(current, &draft).expect("upsert");

    assert_eq!(next["unknown"], true);
    assert_eq!(next["providers"]["existing"]["baseUrl"], "https://existing");
    assert_eq!(
        next["providers"]["new-provider"]["baseUrl"],
        "https://new.example/v1"
    );
}

#[test]
fn delete_provider_removes_only_target_provider() {
    let current = serde_json::json!({
        "providers": {
            "a": { "baseUrl": "https://a" },
            "b": { "baseUrl": "https://b" }
        }
    });

    let next = pi_provider::delete_provider_value(current, "a").expect("delete");

    assert!(next["providers"].get("a").is_none());
    assert_eq!(next["providers"]["b"]["baseUrl"], "https://b");
}

#[test]
fn delete_provider_value_keeps_unknown_top_level_fields() {
    let current = serde_json::json!({
        "providers": {
            "target": {},
            "keep": {}
        },
        "root": "kept"
    });

    let next = pi_provider::delete_provider_value(current, "target").expect("delete");

    assert_eq!(next["root"], "kept");
    assert!(next["providers"].get("target").is_none());
    assert!(next["providers"].get("keep").is_some());
}

#[test]
fn multi_model_draft_with_cost_is_rendered() {
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::Custom,
        provider_id: "proxy".to_string(),
        template: pi_provider::PiProviderTemplate::AnthropicCompatible,
        base_url: Some("https://proxy.example.com".to_string()),
        api: "anthropic-messages".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::Env,
            value: "PROXY_KEY".to_string(),
        },
        headers: vec![],
        models: vec![
            pi_provider::PiModelDraft {
                id: "claude-opus-4-8".to_string(),
                name: Some("Claude Opus 4.8".to_string()),
                name_touched: true,
                reasoning: Some(true),
                input: Some(vec!["text".to_string(), "image".to_string()]),
                context_window: Some(200000),
                max_tokens: Some(32000),
                cost: Some(pi_provider::PiModelCost {
                    input: 15.0,
                    output: 75.0,
                    cache_read: Some(1.5),
                    cache_write: Some(18.75),
                }),
            },
            pi_provider::PiModelDraft {
                id: "claude-sonnet-4-5".to_string(),
                name: Some("Claude Sonnet 4.5".to_string()),
                name_touched: true,
                reasoning: Some(true),
                input: None,
                context_window: Some(200000),
                max_tokens: None,
                cost: Some(pi_provider::PiModelCost {
                    input: 3.0,
                    output: 15.0,
                    cache_read: None,
                    cache_write: None,
                }),
            },
        ],
        compat: Some(pi_provider::PiProviderCompat {
            supports_developer_role: None,
            supports_reasoning_effort: None,
            supports_usage_in_streaming: None,
            max_tokens_field: None,
            thinking_format: None,
            supports_eager_tool_input_streaming: Some(false),
            supports_long_cache_retention: None,
            force_adaptive_thinking: Some(true),
            allow_empty_signature: None,
        }),
        advanced_json: None,
    };

    let provider = pi_provider::draft_to_provider_value(&draft).expect("multi-model");

    assert_eq!(provider["api"], "anthropic-messages");
    assert_eq!(provider["models"][0]["id"], "claude-opus-4-8");
    assert_eq!(provider["models"][0]["cost"]["input"], 15.0);
    assert_eq!(provider["models"][0]["cost"]["cacheRead"], 1.5);
    assert_eq!(provider["models"][1]["id"], "claude-sonnet-4-5");
    assert_eq!(provider["models"][1]["cost"]["input"], 3.0);
    assert_eq!(provider["compat"]["supportsEagerToolInputStreaming"], false);
    assert_eq!(provider["compat"]["forceAdaptiveThinking"], true);
}

#[test]
fn builtin_override_mode_allows_empty_models() {
    let draft = pi_provider::PiProviderDraft {
        mode: pi_provider::PiProviderMode::BuiltinOverride,
        provider_id: "anthropic".to_string(),
        template: pi_provider::PiProviderTemplate::AnthropicCompatible,
        base_url: Some("https://my-proxy.example.com/v1".to_string()),
        api: "".to_string(),
        api_key: pi_provider::PiApiKeyDraft {
            mode: pi_provider::PiApiKeyMode::None,
            value: String::new(),
        },
        headers: vec![],
        models: vec![],
        compat: None,
        advanced_json: None,
    };

    let provider = pi_provider::draft_to_provider_value(&draft).expect("builtin override");

    assert_eq!(provider["baseUrl"], "https://my-proxy.example.com/v1");
    // No "models" key since it's a builtin override with empty models
    assert!(provider.get("models").is_none());
}
