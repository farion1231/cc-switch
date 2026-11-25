use super::*;
use crate::provider::ProviderMeta;
use serde_json::json;

fn create_provider(config: serde_json::Value) -> Provider {
    Provider {
        id: "test".to_string(),
        name: "Test Claude".to_string(),
        settings_config: config,
        website_url: None,
        category: Some("claude".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn create_provider_with_meta(config: serde_json::Value, meta: ProviderMeta) -> Provider {
    Provider {
        id: "test".to_string(),
        name: "Test Claude".to_string(),
        settings_config: config,
        website_url: None,
        category: Some("claude".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        meta: Some(meta),
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

#[test]
fn test_extract_base_url_from_env() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
        }
    }));

    let url = adapter.extract_base_url(&provider).unwrap();
    assert_eq!(url, "https://api.anthropic.com");
}

#[test]
fn test_extract_auth_anthropic_auth_token_uses_claude_auth_strategy() {
    // ANTHROPIC_AUTH_TOKEN 在 Anthropic SDK 里语义就是 Authorization: Bearer，
    // 因此走 ClaudeAuth strategy 而不是 Anthropic（x-api-key）。
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-ant-test-key"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-ant-test-key");
    assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
}

#[test]
fn test_extract_auth_anthropic_api_key() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
            "ANTHROPIC_API_KEY": "sk-ant-test-key"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-ant-test-key");
    assert_eq!(auth.strategy, AuthStrategy::Anthropic);
}

#[test]
fn test_extract_auth_openai_formats_always_use_bearer() {
    // 认证字段选了 ANTHROPIC_API_KEY、上游却是 OpenAI 协议：x-api-key 会被拒，必须发 Bearer。
    let adapter = ClaudeAdapter::new();
    for format in ["openai_chat", "openai_responses"] {
        for env in [
            json!({
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_API_KEY": "sk-test"
            }),
            json!({
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            }),
        ] {
            let provider = create_provider_with_meta(
                json!({ "env": env }),
                ProviderMeta {
                    api_format: Some(format.to_string()),
                    ..Default::default()
                },
            );
            let auth = adapter.extract_auth(&provider).unwrap();
            assert_eq!(auth.strategy, AuthStrategy::Bearer, "{format}");
            let headers = adapter.get_auth_headers(&auth).unwrap();
            assert_eq!(headers.len(), 1);
            assert_eq!(headers[0].0.as_str(), "authorization");
            assert_eq!(headers[0].1.to_str().unwrap(), "Bearer sk-test");
        }
    }
}

#[test]
fn test_extract_auth_anthropic_format_keeps_env_var_semantics() {
    // Anthropic Messages 上游照旧按变量名区分（bdc4c1e8b 修的严格端点不能回退）。
    let adapter = ClaudeAdapter::new();
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_API_KEY": "sk-test"
            }
        }),
        ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        },
    );
    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.strategy, AuthStrategy::Anthropic);
}

#[test]
fn test_extract_auth_both_env_vars_prefer_auth_token() {
    // 两个变量都填时，extract_key 选 AUTH_TOKEN，strategy 推断也必须保持一致。
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-from-auth-token",
            "ANTHROPIC_API_KEY": "sk-from-api-key"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-from-auth-token");
    assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
}

#[test]
fn test_extract_auth_apikey_field_fallback_uses_anthropic_strategy() {
    // 当用户没填任一 ANTHROPIC_* env，而是直接使用 apiKey 字段时，
    // 视为没有显式语义偏好，默认走 Anthropic 官方协议（x-api-key）。
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "apiKey": "sk-direct",
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-direct");
    assert_eq!(auth.strategy, AuthStrategy::Anthropic);
}

#[test]
fn test_extract_auth_reads_bedrock_bearer_token_env() {
    // 新版 Bedrock API Key 预设把 Key 放在 env.AWS_BEARER_TOKEN_BEDROCK；
    // 鉴权策略与旧版顶层 apiKey 保持一致，不在这次改动里变。
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "apiKey": "stale-top-level",
        "env": {
            "ANTHROPIC_BASE_URL": "https://bedrock-runtime.us-west-2.amazonaws.com",
            "AWS_BEARER_TOKEN_BEDROCK": "bedrock-key",
            "CLAUDE_CODE_USE_BEDROCK": "1"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "bedrock-key");
    assert_eq!(auth.strategy, AuthStrategy::Anthropic);
}

#[test]
fn codex_oauth_generation_uses_current_client_identity() {
    let headers: http::HeaderMap = ClaudeAdapter::new()
        .get_auth_headers(&AuthInfo::new(
            "test-token".into(),
            AuthStrategy::CodexOAuth,
        ))
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(headers["authorization"], "Bearer test-token");
    assert_eq!(headers["originator"], "codex_cli_rs");
    let version: Vec<u32> = headers["version"]
        .to_str()
        .unwrap()
        .split('.')
        .map(|part| part.parse().unwrap())
        .collect();
    assert!(
        version.as_slice() >= [0, 169, 0].as_slice(),
        "ChatGPT OAuth requests should advertise Codex >= 0.169.0; sent {version:?}"
    );
}

#[test]
fn test_get_auth_headers_anthropic_emits_x_api_key() {
    let adapter = ClaudeAdapter::new();
    let auth = AuthInfo::new("sk-ant-test".to_string(), AuthStrategy::Anthropic);

    let headers = adapter.get_auth_headers(&auth).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0.as_str(), "x-api-key");
    assert_eq!(headers[0].1.to_str().unwrap(), "sk-ant-test");
}

#[test]
fn test_get_auth_headers_claude_auth_emits_authorization_bearer() {
    let adapter = ClaudeAdapter::new();
    let auth = AuthInfo::new("sk-relay-test".to_string(), AuthStrategy::ClaudeAuth);

    let headers = adapter.get_auth_headers(&auth).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0.as_str(), "authorization");
    assert_eq!(headers[0].1.to_str().unwrap(), "Bearer sk-relay-test");
}

#[test]
fn test_get_auth_headers_bearer_emits_authorization_bearer() {
    let adapter = ClaudeAdapter::new();
    let auth = AuthInfo::new("sk-or-test".to_string(), AuthStrategy::Bearer);

    let headers = adapter.get_auth_headers(&auth).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0.as_str(), "authorization");
    assert_eq!(headers[0].1.to_str().unwrap(), "Bearer sk-or-test");
}

#[test]
fn test_get_auth_headers_rejects_illegal_header_chars() {
    // 用户粘贴含 \r\n 的"脏"key 不能让进程 panic
    let adapter = ClaudeAdapter::new();
    let auth = AuthInfo::new(
        "sk-ant-bad\r\nX-Inject: 1".to_string(),
        AuthStrategy::Anthropic,
    );

    let result = adapter.get_auth_headers(&auth);
    assert!(result.is_err(), "expected AuthError, got Ok");
    assert!(matches!(result, Err(ProxyError::AuthError(_))));
}

#[test]
fn test_extract_auth_openrouter() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
            "OPENROUTER_API_KEY": "sk-or-test-key"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-or-test-key");
    assert_eq!(auth.strategy, AuthStrategy::Bearer);
}

#[test]
fn test_extract_auth_gemini_api_key() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com/v1beta",
                "GEMINI_API_KEY": "gemini-test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "gemini-test-key");
    assert_eq!(auth.strategy, AuthStrategy::Google);
}

#[test]
fn test_extract_auth_claude_auth_mode() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://some-proxy.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key"
        },
        "auth_mode": "bearer_only"
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-proxy-key");
    assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
}

#[test]
fn test_extract_auth_claude_auth_env_mode() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://some-proxy.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key",
            "AUTH_MODE": "bearer_only"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-proxy-key");
    assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
}

/// Regression: a Gemini OAuth credential JSON that carries only a
/// refresh_token (no active access_token) must not be surfaced as an
/// `AuthInfo` whose bearer would be empty. Without the guard, downstream
/// header injection produces `Authorization: Bearer ` and a deterministic
/// 401 from upstream.
#[test]
fn test_extract_auth_gemini_cli_refresh_only_json_does_not_expose_empty_bearer() {
    let adapter = ClaudeAdapter::new();
    let refresh_only_json = r#"{"refresh_token":"rt-abc","client_id":"cid","client_secret":"cs"}"#;
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": refresh_only_json
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    let auth = adapter.extract_auth(&provider).unwrap();
    // access_token must not be surfaced as `Some("")` — the OAuth header
    // builder uses `access_token.as_ref().unwrap_or(&api_key)`, so a
    // `Some("")` would win over the raw key and emit `Bearer `.
    assert!(
        auth.access_token.as_deref().is_none_or(|t| !t.is_empty()),
        "empty access_token leaked into AuthInfo"
    );
    assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
}

/// Companion case: a JSON credential with an empty-string `access_token`
/// field (the shape an expired credential can take after partial writes)
/// must degrade the same way.
#[test]
fn test_extract_auth_gemini_cli_empty_access_token_degrades_to_raw_key() {
    let adapter = ClaudeAdapter::new();
    let expired_json =
        r#"{"access_token":"","refresh_token":"rt-abc","client_id":"cid","client_secret":"cs"}"#;
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": expired_json
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    let auth = adapter.extract_auth(&provider).unwrap();
    assert!(
        auth.access_token.as_deref().is_none_or(|t| !t.is_empty()),
        "empty access_token leaked into AuthInfo"
    );
    assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
}

/// Counter-case: a well-formed JSON credential with a non-empty
/// access_token must still flow through the OAuth path unchanged.
#[test]
fn test_extract_auth_gemini_cli_valid_json_keeps_access_token() {
    let adapter = ClaudeAdapter::new();
    let valid_json = r#"{"access_token":"ya29.valid","refresh_token":"rt"}"#;
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": valid_json
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.access_token.as_deref(), Some("ya29.valid"));
    assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
}

/// 回归:从 oauth_creds.json 复制时常带前导换行/空格。未 trim 时
/// `starts_with('{')` 会落空,导致误分类为 `ProviderType::Gemini`,再
/// 以 raw JSON 当 `x-goog-api-key` 发出去触发 401。trim 应在 provider
/// 类型判定和 OAuth 解析前统一生效。
#[test]
fn test_extract_auth_gemini_cli_json_with_leading_whitespace_classifies_correctly() {
    let adapter = ClaudeAdapter::new();
    let valid_json = r#"{"access_token":"ya29.valid","refresh_token":"rt"}"#;
    let key_with_whitespace = format!("\n  {valid_json}\n");
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": key_with_whitespace
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(adapter.provider_type(&provider), ProviderType::GeminiCli);

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.access_token.as_deref(), Some("ya29.valid"));
    assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
}

/// 回归:裸 `ya29.` access_token 若带前导换行,也应被 trim 后识别为
/// Gemini CLI OAuth,避免前导空白把 `starts_with("ya29.")` 检查顶穿。
#[test]
fn test_extract_auth_gemini_cli_access_token_with_leading_newline_classifies_correctly() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": "\nya29.raw-token-value\n"
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(adapter.provider_type(&provider), ProviderType::GeminiCli);

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.access_token.as_deref(), Some("ya29.raw-token-value"));
    assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
}

#[test]
fn test_provider_type_detection() {
    let adapter = ClaudeAdapter::new();

    // Anthropic 官方
    let anthropic = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-ant-test"
        }
    }));
    assert_eq!(adapter.provider_type(&anthropic), ProviderType::Claude);

    // OpenRouter
    let openrouter = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
            "OPENROUTER_API_KEY": "sk-or-test"
        }
    }));
    assert_eq!(adapter.provider_type(&openrouter), ProviderType::OpenRouter);

    // ClaudeAuth
    let claude_auth = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://some-proxy.com",
            "ANTHROPIC_AUTH_TOKEN": "sk-test"
        },
        "auth_mode": "bearer_only"
    }));
    assert_eq!(
        adapter.provider_type(&claude_auth),
        ProviderType::ClaudeAuth
    );
}

#[test]
fn test_build_url_cases() {
    for (case, base_url, endpoint, expected) in [
        (
            "test_build_url_anthropic",
            "https://api.anthropic.com",
            "/v1/messages",
            "https://api.anthropic.com/v1/messages",
        ),
        (
            "test_build_url_openrouter",
            "https://openrouter.ai/api",
            "/v1/messages",
            "https://openrouter.ai/api/v1/messages",
        ),
        (
            "test_build_url_no_beta_for_other_endpoints",
            "https://api.anthropic.com",
            "/v1/complete",
            "https://api.anthropic.com/v1/complete",
        ),
        (
            "test_build_url_preserve_existing_query",
            "https://api.anthropic.com",
            "/v1/messages?foo=bar",
            "https://api.anthropic.com/v1/messages?foo=bar",
        ),
        (
            "test_build_url_no_beta_for_github_copilot",
            "https://api.githubcopilot.com",
            "/v1/messages",
            "https://api.githubcopilot.com/v1/messages",
        ),
        (
            "test_build_url_no_beta_for_openai_chat_completions",
            "https://integrate.api.nvidia.com",
            "/v1/chat/completions",
            "https://integrate.api.nvidia.com/v1/chat/completions",
        ),
    ] {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url(base_url, endpoint);
        assert_eq!(url, expected, "{case}");
    }
}

#[test]
fn xai_oauth_invariants_ignore_editable_format_and_base_url() {
    let adapter = ClaudeAdapter::new();
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://attacker.example/anthropic",
                "ANTHROPIC_API_KEY": "user-edited"
            }
        }),
        ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            api_format: Some("anthropic".to_string()),
            is_full_url: Some(true),
            ..Default::default()
        },
    );

    assert_eq!(get_claude_api_format(&provider), "openai_responses");
    assert_eq!(adapter.provider_type(&provider), ProviderType::XaiOAuth);
    assert_eq!(
        adapter.extract_base_url(&provider).unwrap(),
        super::super::XAI_API_BASE_URL
    );
    assert!(adapter.needs_transform(&provider));
    assert_eq!(
        adapter
            .extract_auth(&provider)
            .expect("managed auth placeholder")
            .strategy,
        AuthStrategy::XaiOAuth
    );
    assert_eq!(
        adapter.build_url(super::super::XAI_API_BASE_URL, "/v1/messages?beta=1"),
        "https://api.x.ai/v1/responses?beta=1"
    );

    let transformed = transform_claude_request_for_api_format(
        json!({
            "model": "grok-4.5",
            "max_tokens": 2048,
            "thinking": { "type": "enabled", "budget_tokens": 20000 },
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        &provider,
        "openai_responses",
        None,
        None,
    )
    .unwrap();
    assert_eq!(transformed["reasoning"]["effort"], json!("high"));
    assert_eq!(
        transformed["include"],
        json!(["reasoning.encrypted_content"])
    );
    assert!(transformed.get("store").is_none());
}

#[test]
fn xai_oauth_forced_web_search_uses_string_tool_choice() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.x.ai/v1",
                "ANTHROPIC_API_KEY": "placeholder"
            }
        }),
        ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        },
    );
    let transformed = transform_claude_request_for_api_format(
        json!({
            "model": "grok-4.5",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "search"}],
            "tools": [
                {"name": "Bash", "input_schema": {"type": "object"}},
                {"type": "web_search_20250305", "name": "web_search"}
            ],
            "tool_choice": {"type": "tool", "name": "web_search"}
        }),
        &provider,
        "openai_responses",
        None,
        None,
    )
    .unwrap();

    assert_eq!(transformed["tool_choice"], json!("required"));
    assert_eq!(transformed["tools"].as_array().unwrap().len(), 1);
    assert_eq!(transformed["tools"][0]["type"], json!("web_search"));
    assert!(transformed["include"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("reasoning.encrypted_content")));
}

#[test]
fn openai_responses_keeps_hosted_web_search_selector() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.openai.com/v1",
                "ANTHROPIC_API_KEY": "sk-test"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        },
    );
    let transformed = transform_claude_request_for_api_format(
        json!({
            "model": "gpt-5",
            "max_tokens": 256,
            "messages": [{"role": "user", "content": "search"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}],
            "tool_choice": {"type": "tool", "name": "web_search"}
        }),
        &provider,
        "openai_responses",
        None,
        None,
    )
    .unwrap();

    assert_eq!(transformed["tool_choice"], json!({"type": "web_search"}));
}

#[test]
fn test_needs_transform() {
    let adapter = ClaudeAdapter::new();

    // Default: no transform (anthropic format) - no meta
    let anthropic_provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
        }
    }));
    assert!(!adapter.needs_transform(&anthropic_provider));

    // Explicit anthropic format in meta: no transform
    let explicit_anthropic = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        },
    );
    assert!(!adapter.needs_transform(&explicit_anthropic));

    // Legacy settings_config.api_format: openai_chat should enable transform
    let legacy_settings_api_format = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com"
        },
        "api_format": "openai_chat"
    }));
    assert!(adapter.needs_transform(&legacy_settings_api_format));

    // Legacy openrouter_compat_mode: bool/number/string should enable transform
    let legacy_openrouter_bool = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com"
        },
        "openrouter_compat_mode": true
    }));
    assert!(adapter.needs_transform(&legacy_openrouter_bool));

    let legacy_openrouter_num = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com"
        },
        "openrouter_compat_mode": 1
    }));
    assert!(adapter.needs_transform(&legacy_openrouter_num));

    let legacy_openrouter_str = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com"
        },
        "openrouter_compat_mode": "true"
    }));
    assert!(adapter.needs_transform(&legacy_openrouter_str));

    // OpenAI Chat format in meta: needs transform
    let openai_chat_provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    assert!(adapter.needs_transform(&openai_chat_provider));

    // OpenAI Responses format in meta: needs transform
    let openai_responses_provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        },
    );
    assert!(adapter.needs_transform(&openai_responses_provider));

    let gemini_native_provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );
    assert!(adapter.needs_transform(&gemini_native_provider));
    assert_eq!(
        adapter.provider_type(&gemini_native_provider),
        ProviderType::Gemini
    );

    // meta takes precedence over legacy settings_config fields
    let meta_precedence_over_settings = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "api_format": "openai_chat",
            "openrouter_compat_mode": true
        }),
        ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        },
    );
    assert!(!adapter.needs_transform(&meta_precedence_over_settings));

    // Unknown format in meta: default to anthropic (no transform)
    let unknown_format = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("unknown".to_string()),
            ..Default::default()
        },
    );
    assert!(!adapter.needs_transform(&unknown_format));
}

#[test]
fn test_github_copilot_detection_by_url() {
    let adapter = ClaudeAdapter::new();

    // GitHub Copilot by base_url
    let copilot = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
        }
    }));
    assert_eq!(adapter.provider_type(&copilot), ProviderType::GitHubCopilot);
}

#[test]
fn test_github_copilot_detection_by_meta() {
    let adapter = ClaudeAdapter::new();

    // GitHub Copilot by meta.provider_type
    let copilot_meta = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        },
    );
    assert_eq!(
        adapter.provider_type(&copilot_meta),
        ProviderType::GitHubCopilot
    );
}

#[test]
fn copilot_legacy_url_in_any_base_url_field() {
    let adapter = ClaudeAdapter::new();
    for field in ["base_url", "baseURL", "apiEndpoint"] {
        let legacy = create_provider(json!({ field: "https://api.githubcopilot.com" }));
        assert!(!legacy.is_github_copilot(), "{field}");
        assert_eq!(
            adapter.provider_type(&legacy),
            ProviderType::GitHubCopilot,
            "{field}"
        );
    }
}

/// A declared OAuth kind beats a Copilot URL here, while
/// `Provider::is_github_copilot` lets the URL win. Kept as is.
#[test]
fn declared_oauth_kind_beats_copilot_url() {
    let adapter = ClaudeAdapter::new();
    let codex = create_provider_with_meta(
        json!({ "env": { "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com" } }),
        ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        },
    );
    assert_eq!(adapter.provider_type(&codex), ProviderType::CodexOAuth);
    assert!(codex.is_github_copilot());
}

#[test]
fn test_github_copilot_auth() {
    let adapter = ClaudeAdapter::new();

    let copilot = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
        }
    }));

    let auth = adapter.extract_auth(&copilot).unwrap();
    assert_eq!(auth.strategy, AuthStrategy::GitHubCopilot);
}

#[test]
fn test_github_copilot_needs_transform() {
    let adapter = ClaudeAdapter::new();

    let copilot = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
        }
    }));

    // GitHub Copilot always needs transform
    assert!(adapter.needs_transform(&copilot));
}

#[test]
fn test_transform_claude_request_for_api_format_responses() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
        }
    }));
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_responses", None, None)
            .unwrap();

    assert_eq!(transformed["model"], "gpt-5.4");
    assert!(transformed.get("input").is_some());
    assert!(transformed.get("max_output_tokens").is_some());
}

#[test]
fn test_responses_sends_effort_for_unlisted_deepseek_model() {
    // #9062：Desktop 把 claude-fable-5 映射到 deepseek-flash，经自建 Responses 网关转发。
    let provider = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://gateway.example.com/v1" }
    }));
    let body = json!({
        "model": "deepseek-flash",
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": "high" },
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_responses", None, None)
            .unwrap();

    assert_eq!(transformed["reasoning"], json!({ "effort": "high" }));
}

#[test]
fn test_chat_folds_effort_to_standard_tiers_for_unlisted_model() {
    let provider = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://api.groq.com/openai/v1" }
    }));
    let body = json!({
        "model": "openai/gpt-oss-120b",
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": "max" },
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert_eq!(transformed["reasoning_effort"], "high");
}

#[test]
fn test_transform_claude_request_openai_chat_streaming_injects_include_usage() {
    let provider = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1" }
    }));
    // 流式请求必须注入 stream_options.include_usage，否则 OpenAI 兼容上游不在
    // SSE 末尾吐 usage → 转换出的 Anthropic message_delta 全 0 → 整笔 usage 漏记。
    let body = json!({
        "model": "moonshotai/kimi-k2",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128,
        "stream": true
    });
    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();
    assert_eq!(transformed["stream"], true);
    assert_eq!(transformed["stream_options"]["include_usage"], true);
}

#[test]
fn test_transform_claude_request_openai_chat_non_streaming_omits_stream_options() {
    let provider = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1" }
    }));
    // 非流式请求不应注入 stream_options（usage 在非流式响应体里恒有）。
    let body = json!({
        "model": "moonshotai/kimi-k2",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });
    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();
    assert!(transformed.get("stream_options").is_none());
}

#[test]
fn test_transform_claude_request_for_codex_oauth_uses_session_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            provider_type: Some("codex_oauth".to_string()),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed = transform_claude_request_for_api_format(
        body,
        &provider,
        "openai_responses",
        Some("session-123"),
        None,
    )
    .unwrap();

    assert_eq!(transformed["prompt_cache_key"], "session-123");
}

#[test]
fn test_codex_oauth_cache_key_follows_claude_json_conversation() {
    use crate::proxy::session::extract_session_id;
    use axum::http::HeaderMap;

    let provider = create_provider_with_meta(
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
        }}),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            provider_type: Some("codex_oauth".to_string()),
            ..ProviderMeta::default()
        },
    );
    for client_format in ["claude", "claude-desktop"] {
        for session_id in ["conversation-123", "conversation-456"] {
            for turn in 0..3 {
                let body = json!({
                    "model": "gpt-5.4",
                    "max_tokens": 128,
                    "messages": [{"role": "user", "content": format!("turn {turn}")}],
                    "metadata": {"user_id": json!({
                        "device_id": "same-device",
                        "account_uuid": "same-account",
                        "session_id": session_id
                    }).to_string()}
                });
                let session = extract_session_id(&HeaderMap::new(), &body, client_format);
                // Match the forwarder's rule: generated IDs must not become cache keys.
                let cache_session = session
                    .client_provided
                    .then_some(session.session_id.as_str());
                let transformed = transform_claude_request_for_api_format(
                    body,
                    &provider,
                    "openai_responses",
                    cache_session,
                    None,
                )
                .unwrap();
                assert_eq!(transformed["prompt_cache_key"], session_id);
                assert_eq!(transformed["store"], false);
            }
        }
    }
}

#[test]
fn test_transform_claude_request_for_codex_oauth_without_session_omits_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            provider_type: Some("codex_oauth".to_string()),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_responses", None, None)
            .unwrap();

    assert!(transformed.get("prompt_cache_key").is_none());
}

#[test]
fn test_transform_claude_request_for_responses_uses_session_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.openai.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed = transform_claude_request_for_api_format(
        body,
        &provider,
        "openai_responses",
        Some("claude-session-123"),
        None,
    )
    .unwrap();

    assert_eq!(transformed["prompt_cache_key"], "claude-session-123");
}

#[test]
fn test_transform_claude_request_for_responses_without_session_omits_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.openai.example.com"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_responses", None, None)
            .unwrap();

    assert!(transformed.get("prompt_cache_key").is_none());
}

#[test]
fn test_transform_claude_request_for_codex_oauth_keeps_explicit_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            provider_type: Some("codex_oauth".to_string()),
            prompt_cache_key: Some("explicit-cache-key".to_string()),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed = transform_claude_request_for_api_format(
        body,
        &provider,
        "openai_responses",
        Some("session-123"),
        None,
    )
    .unwrap();

    assert_eq!(transformed["prompt_cache_key"], "explicit-cache-key");
}

#[test]
fn test_transform_claude_request_for_api_format_codex_oauth_fast_mode_off() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            codex_fast_mode: Some(false),
            ..ProviderMeta::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 128
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_responses", None, None)
            .unwrap();

    assert_eq!(transformed["store"], json!(false));
    assert!(transformed.get("service_tier").is_none());
    assert_eq!(
        transformed["include"],
        json!(["reasoning.encrypted_content"])
    );
}

#[test]
fn test_transform_claude_request_for_api_format_gemini_native() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("gemini_native".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "gemini-2.5-pro",
        "system": "You are helpful.",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "gemini_native", None, None)
            .unwrap();

    assert!(transformed.get("contents").is_some());
    assert_eq!(
        transformed["systemInstruction"]["parts"][0]["text"],
        "You are helpful."
    );
    assert_eq!(transformed["generationConfig"]["maxOutputTokens"], 64);
}

#[test]
fn test_transform_claude_request_openai_chat_strips_stop_for_copilot() {
    // Issue #5175: GitHub Copilot 的 OpenAI Chat 端点不支持 `stop` 参数，
    // 携带该字段会返回 400，导致 Claude Code auto mode 的 classifier
    // 请求（携带 stop_sequences）始终失败。修复后 Copilot 路径应移除转换出的 stop。
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64,
        "stop_sequences": ["\n\nHuman:"]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert!(transformed.get("stop").is_none());
}

#[test]
fn copilot_gemini_chat_sends_mid_conversation_system_as_user() {
    // #9613: Copilot hoists every system message into the Gemini instruction,
    // so only Gemini models on Copilot get later system messages as user ones.
    let copilot = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com" }
    }));
    let generic = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1" }
    }));
    let body = |model: &str| {
        json!({
            "model": model,
            "system": "You are Claude Code.",
            "max_tokens": 64,
            "messages": [
                { "role": "user", "content": "hello" },
                { "role": "assistant", "content": "hi" },
                { "role": "system", "content": "<total_tokens>9 tokens left</total_tokens>" },
                { "role": "user", "content": "go on" }
            ]
        })
    };
    let roles = |transformed: &Value| -> Vec<String> {
        transformed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap().to_string())
            .collect()
    };

    let transformed = transform_claude_request_for_api_format(
        body("gemini-4-carbon"),
        &copilot,
        "openai_chat",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        roles(&transformed),
        ["system", "user", "assistant", "user", "user"]
    );
    assert_eq!(
        transformed["messages"][3]["content"],
        "<total_tokens>9 tokens left</total_tokens>"
    );

    for (model, provider) in [("gpt-4.1", &copilot), ("gemini-4-carbon", &generic)] {
        let transformed = transform_claude_request_for_api_format(
            body(model),
            provider,
            "openai_chat",
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            roles(&transformed),
            ["system", "user", "assistant", "system", "user"],
            "{model}"
        );
    }
}

#[test]
fn test_transform_claude_request_openai_chat_strips_stop_for_copilot_env_url() {
    // 兼容旧/手动 Copilot provider：即使没有 meta.provider_type，
    // 只要 ANTHROPIC_BASE_URL 指向 Copilot，也应移除 stop。
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64,
        "stop_sequences": ["\n\nHuman:"]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert!(transformed.get("stop").is_none());
}

#[test]
fn test_transform_claude_request_openai_chat_keeps_stop_for_generic_provider() {
    // 非 Copilot 的 OpenAI 兼容上游（如 OpenRouter）应继续保留 stop 语义，
    // 确认修复未影响其它 provider。
    let provider = create_provider(json!({
        "env": { "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1" }
    }));
    let body = json!({
        "model": "moonshotai/kimi-k2",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64,
        "stop_sequences": ["\n\nHuman:"]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert_eq!(transformed["stop"], json!(["\n\nHuman:"]));
}

#[test]
fn test_transform_claude_request_for_api_format_openai_chat_skips_prompt_cache_key_by_default() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert!(transformed.get("prompt_cache_key").is_none());
}

#[test]
fn test_transform_claude_request_for_api_format_openai_chat_keeps_explicit_prompt_cache_key() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            prompt_cache_key: Some("claude-cache-route".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "messages": [{ "role": "user", "content": "hello" }],
        "max_tokens": 64
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    assert_eq!(transformed["prompt_cache_key"], "claude-cache-route");
}

#[test]
fn test_transform_openai_chat_skips_reasoning_content_for_generic_provider() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "gpt-5.4",
        "max_tokens": 64,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "I should call the tool."},
                {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
            ]
        }]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    let msg = &transformed["messages"][0];
    assert!(msg.get("tool_calls").is_some());
    assert!(msg.get("reasoning_content").is_none());
}

#[test]
fn test_transform_openai_chat_skips_reasoning_content_for_kimi_provider() {
    // Kimi 2026-08 反馈：不再需要 reasoning_content 回放，注入反而扰乱思维链。
    // Kimi/Moonshot 已从 REASONING_VENDOR_HINTS 撤出，应与通用 provider 同行为。
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.moonshot.cn/v1",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "kimi-k2.6",
        "max_tokens": 64,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "I should call the tool."},
                {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
            ]
        }]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    let msg = &transformed["messages"][0];
    assert!(msg.get("tool_calls").is_some());
    assert!(msg.get("reasoning_content").is_none());
}

#[test]
fn test_transform_openai_chat_mistral_deepseek_tool_history() {
    for settings in [
        json!({"env": {"ANTHROPIC_BASE_URL": "https://api.mistral.ai"}}),
        json!({"base_url": "https://api.mistral.ai/v1/"}),
        json!({"baseURL": "https://API.MISTRAL.AI/v1"}),
        json!({"apiEndpoint": "https://api.mistral.ai/v1/chat/completions"}),
    ] {
        let provider = create_provider_with_meta(
            settings,
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
        // Cover real thinking and the missing-thinking placeholder that
        // previously broke the follow-up after a tool ran.
        for with_thinking in [false, true] {
            let mut content = vec![json!({
                "type": "tool_use", "id": "call_123",
                "name": "get_weather", "input": {"location": "Tokyo"}
            })];
            if with_thinking {
                content.insert(
                    0,
                    json!({"type": "thinking", "thinking": "I should call the tool."}),
                );
            }
            let body = json!({
                "model": "deepseek-v4-1-flash-1017",
                "max_tokens": 64,
                "messages": [
                    {"role": "user", "content": "What is the weather in Tokyo?"},
                    {"role": "assistant", "content": content},
                    {"role": "user", "content": [{
                        "type": "tool_result", "tool_use_id": "call_123",
                        "content": "Sunny"
                    }]}
                ]
            });
            let transformed =
                transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
                    .unwrap();

            let messages = transformed["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 3);
            assert!(messages
                .iter()
                .all(|msg| msg.get("reasoning_content").is_none()));
            let call = &messages[1]["tool_calls"][0];
            assert_eq!(call["id"], "call_123");
            assert_eq!(call["function"]["name"], "get_weather");
            assert_eq!(call["function"]["arguments"], r#"{"location":"Tokyo"}"#);
            assert_eq!(messages[2]["role"], "tool");
            assert_eq!(messages[2]["tool_call_id"], "call_123");
            assert_eq!(messages[2]["content"], "Sunny");
        }
    }
}

#[test]
fn test_reasoning_content_mistral_exception_does_not_match_other_gateways() {
    for base_url in [
        "https://relay.example/v1",
        "https://api.mistral.ai.example/v1",
        "https://relay.example/api.mistral.ai",
    ] {
        let provider = create_provider(json!({"env": {"ANTHROPIC_BASE_URL": base_url}}));
        assert!(should_preserve_reasoning_content_for_openai_chat(
            &provider,
            &json!({"model": "deepseek-v4-1-flash-1017"}),
        ));
    }
}

#[test]
fn test_transform_openai_chat_preserves_reasoning_content_for_deepseek_provider() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/v1",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "deepseek-v4-flash",
        "max_tokens": 64,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "I should call the tool."},
                {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
            ]
        }]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    let msg = &transformed["messages"][0];
    assert_eq!(msg["reasoning_content"], "I should call the tool.");
    assert!(msg.get("tool_calls").is_some());
}

#[test]
fn test_transform_openai_chat_preserves_reasoning_content_for_mimo_provider() {
    let provider = create_provider_with_meta(
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.xiaomimimo.com/v1",
                "ANTHROPIC_API_KEY": "test-key"
            }
        }),
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let body = json!({
        "model": "mimo-v2.5-pro",
        "max_tokens": 64,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "I should call the tool."},
                {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
            ]
        }]
    });

    let transformed =
        transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
            .unwrap();

    let msg = &transformed["messages"][0];
    assert_eq!(msg["reasoning_content"], "I should call the tool.");
    assert!(msg.get("tool_calls").is_some());
}

#[test]
fn test_deepseek_anthropic_tool_history_injects_missing_thinking() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "text", "text": "I will inspect the repo."},
                {"type": "tool_use", "id": "call_123", "name": "read_file", "input": {"path": "README.md"}}
            ]
        }]
    });

    let changed =
        normalize_anthropic_tool_thinking_history_for_provider(&mut body, &provider, "anthropic");

    assert!(changed);
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(content[0]["thinking"], ANTHROPIC_THINKING_PLACEHOLDER);
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[2]["type"], "tool_use");
}

#[test]
fn test_anthropic_messages_no_longer_hoists_system_role_messages() {
    // After reverting #3775, role=system messages are left in `messages[]`
    // (DeepSeek's endpoint accepts them natively) and the top-level `system`
    // field is untouched, preserving the request prefix.
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "system": "Existing top-level system.",
        "model": "deepseek-v4-pro",
        "messages": [
            { "role": "system", "content": "Message system one." },
            { "role": "user", "content": "hello" },
            {
                "role": "system",
                "content": [{ "type": "text", "text": "Message system two." }]
            }
        ]
    });

    let changed = normalize_anthropic_messages_for_provider(&mut body, &provider, "anthropic");

    assert!(!changed);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[2]["role"], "system");
    assert_eq!(body["system"], "Existing top-level system.");
}

#[test]
fn test_anthropic_system_role_messages_skip_non_anthropic_format() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/v1",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "messages": [
            { "role": "system", "content": "Keep in messages." },
            { "role": "user", "content": "hello" }
        ]
    });

    let changed = normalize_anthropic_messages_for_provider(&mut body, &provider, "openai_chat");

    assert!(!changed);
    assert!(body.get("system").is_none());
    assert_eq!(body["messages"][0]["role"], "system");
}

#[test]
fn test_kimi_anthropic_tool_history_not_modified() {
    // Kimi 2026-08 反馈：Anthropic 兼容端点不再要求 tool_use 轮回放 thinking，
    // 注入占位符反而扰乱思维链。Kimi 应走通用透传，请求体一字不动。
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "kimi-for-coding",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "call_123", "name": "read_file", "input": {"path": "README.md"}}
            ]
        }]
    });
    let original = body.clone();

    let changed =
        normalize_anthropic_tool_thinking_history_for_provider(&mut body, &provider, "anthropic");

    assert!(!changed);
    assert_eq!(body, original);
}

#[test]
fn test_deepseek_anthropic_tool_history_rewrites_redacted_thinking() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "tool_use", "id": "call_123", "name": "read_file", "input": {"path": "README.md"}}
            ]
        }]
    });

    let changed =
        normalize_anthropic_tool_thinking_history_for_provider(&mut body, &provider, "anthropic");

    assert!(changed);
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(
        content[0]["thinking"],
        ANTHROPIC_REDACTED_THINKING_PLACEHOLDER
    );
    assert!(content[0].get("data").is_none());
}

#[test]
fn test_deepseek_anthropic_tool_history_keeps_thinking_text_but_drops_signature() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Need to inspect the file.", "signature": "anthropic-signature"},
                {"type": "tool_use", "id": "call_123", "name": "read_file", "input": {"path": "README.md"}}
            ]
        }]
    });

    let changed =
        normalize_anthropic_tool_thinking_history_for_provider(&mut body, &provider, "anthropic");

    assert!(changed);
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(content[0]["thinking"], "Need to inspect the file.");
    assert!(content[0].get("signature").is_none());
}

#[test]
fn test_generic_anthropic_tool_history_is_not_modified() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "claude-sonnet-4.6",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "call_123", "name": "read_file", "input": {"path": "README.md"}}
            ]
        }]
    });
    let original = body.clone();

    let changed =
        normalize_anthropic_tool_thinking_history_for_provider(&mut body, &provider, "anthropic");

    assert!(!changed);
    assert_eq!(body, original);
}

// ==================== normalize_deepseek_thinking_disabled_strip_effort 测试 ====================

fn deepseek_official_provider() -> Provider {
    create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }))
}

#[test]
fn test_deepseek_official_strips_output_config_effort() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "output_config": { "effort": "max" },
        "max_tokens": 100000
    });

    let changed =
        normalize_deepseek_thinking_disabled_strip_effort(&mut body, &deepseek_official_provider());

    assert!(changed);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("output_config").is_none());
}

#[test]
fn test_deepseek_official_strips_reasoning_effort() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "reasoning_effort": "high",
        "max_tokens": 100000
    });

    let changed =
        normalize_deepseek_thinking_disabled_strip_effort(&mut body, &deepseek_official_provider());

    assert!(changed);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_deepseek_official_strips_both_effort_fields() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "output_config": { "effort": "max" },
        "reasoning_effort": "high",
        "max_tokens": 100000
    });

    let changed =
        normalize_deepseek_thinking_disabled_strip_effort(&mut body, &deepseek_official_provider());

    assert!(changed);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("output_config").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_deepseek_official_no_effort_no_change() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "max_tokens": 100000
    });
    let original = body.clone();

    let changed =
        normalize_deepseek_thinking_disabled_strip_effort(&mut body, &deepseek_official_provider());

    assert!(!changed);
    assert_eq!(body, original);
}

#[test]
fn test_deepseek_official_preserves_output_config_other_fields() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "output_config": { "effort": "max", "temperature": 0.5 },
        "max_tokens": 100000
    });

    let changed =
        normalize_deepseek_thinking_disabled_strip_effort(&mut body, &deepseek_official_provider());

    assert!(changed);
    assert_eq!(body["output_config"]["temperature"], 0.5);
    assert!(body["output_config"].get("effort").is_none());
}

#[test]
fn test_deepseek_official_non_disabled_not_modified() {
    let cases = vec![
        (
            "enabled",
            json!({ "type": "enabled", "budget_tokens": 16000 }),
        ),
        ("adaptive", json!({ "type": "adaptive" })),
    ];

    for (label, thinking_value) in cases {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "thinking": thinking_value,
            "output_config": { "effort": "max" },
            "max_tokens": 100000
        });
        let original = body.clone();

        let changed = normalize_deepseek_thinking_disabled_strip_effort(
            &mut body,
            &deepseek_official_provider(),
        );

        assert!(!changed, "should not modify thinking.type={label}");
        assert_eq!(body, original);
    }

    // missing thinking field entirely
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "output_config": { "effort": "max" },
        "max_tokens": 100000
    });
    let original = body.clone();
    assert!(!normalize_deepseek_thinking_disabled_strip_effort(
        &mut body,
        &deepseek_official_provider()
    ));
    assert_eq!(body, original);
}

#[test]
fn test_deepseek_official_url_with_trailing_slash() {
    let provider = create_provider(json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic/",
            "ANTHROPIC_API_KEY": "test-key"
        }
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "output_config": { "effort": "max" },
        "max_tokens": 100000
    });

    let changed = normalize_deepseek_thinking_disabled_strip_effort(&mut body, &provider);

    assert!(changed);
    assert!(body.get("output_config").is_none());
}

#[test]
fn test_deepseek_official_detected_via_base_url_fallback() {
    let provider = create_provider(json!({
        "base_url": "https://api.deepseek.com/anthropic",
        "ANTHROPIC_API_KEY": "test-key"
    }));
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "reasoning_effort": "high",
        "max_tokens": 100000
    });

    let changed = normalize_deepseek_thinking_disabled_strip_effort(&mut body, &provider);

    assert!(changed);
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_non_deepseek_endpoint_not_modified() {
    let providers = vec![
        create_provider(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://other-api.com/anthropic", "ANTHROPIC_API_KEY": "test-key" }
        })),
        create_provider(json!({
            "env": { "ANTHROPIC_BASE_URL": "https://api.anthropic.com", "ANTHROPIC_API_KEY": "test-key" }
        })),
    ];

    for provider in providers {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "thinking": { "type": "disabled" },
            "output_config": { "effort": "max" },
            "max_tokens": 100000
        });
        let original = body.clone();

        let changed = normalize_deepseek_thinking_disabled_strip_effort(&mut body, &provider);

        assert!(
            !changed,
            "should not modify for {}",
            provider.settings_config["env"]["ANTHROPIC_BASE_URL"]
        );
        assert_eq!(body, original);
    }
}

#[test]
fn test_normalize_messages_pipeline_strips_effort_for_deepseek() {
    let mut body = json!({
        "model": "deepseek-v4-pro",
        "thinking": { "type": "disabled" },
        "output_config": { "effort": "max" },
        "max_tokens": 100000,
        "messages": [{ "role": "user", "content": "hello" }]
    });

    let changed = normalize_anthropic_messages_for_provider(
        &mut body,
        &deepseek_official_provider(),
        "anthropic",
    );

    assert!(changed);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("output_config").is_none());
}

/// 只有发往 xAI 的 Responses 请求改写托管搜索的 tool_choice；中转站地址里带着
/// api.x.ai 不算发往 xAI。
#[test]
fn xai_web_search_rewrite_follows_the_base_url_host() {
    let forced_search = |base_url: &str| {
        let provider = create_provider_with_meta(
            json!({"env": {"ANTHROPIC_BASE_URL": base_url, "ANTHROPIC_API_KEY": "sk"}}),
            ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                ..Default::default()
            },
        );
        transform_claude_request_for_api_format(
            json!({
                "model": "grok-4.5",
                "max_tokens": 256,
                "messages": [{"role": "user", "content": "search"}],
                "tools": [{"type": "web_search_20250305", "name": "web_search"}],
                "tool_choice": {"type": "tool", "name": "web_search"}
            }),
            &provider,
            "openai_responses",
            None,
            None,
        )
        .unwrap()["tool_choice"]
            .clone()
    };
    for url in [
        "https://api.x.ai/v1",
        "https://API.X.AI/v1",
        "https://eu.api.x.ai:8443/v1",
        "https://api.x.ai./v1",
    ] {
        assert_eq!(forced_search(url), json!("required"), "{url}");
    }
    for url in [
        "https://relay.example.com/api.x.ai/v1",
        "https://relay.example.com/v1?upstream=https://api.x.ai/v1",
        "https://api.x.ai.relay.example.com/v1",
    ] {
        assert_ne!(forced_search(url), json!("required"), "{url}");
    }
}
