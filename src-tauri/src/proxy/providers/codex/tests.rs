use super::*;
use crate::provider::ProviderMeta;
use serde_json::json;

fn create_provider(config: serde_json::Value) -> Provider {
    Provider {
        id: "test".to_string(),
        name: "Test Codex".to_string(),
        settings_config: config,
        website_url: None,
        category: Some("codex".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn create_copilot_provider() -> Provider {
    let mut provider = create_provider(json!({}));
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..ProviderMeta::default()
    });
    provider
}

#[test]
fn codex_adapter_uses_managed_copilot_auth_and_endpoint() {
    let adapter = CodexAdapter::new();
    let provider = create_copilot_provider();

    assert_eq!(
        adapter.extract_base_url(&provider).unwrap(),
        "https://api.githubcopilot.com"
    );
    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.strategy, AuthStrategy::GitHubCopilot);
    let headers = adapter.get_auth_headers(&auth).unwrap();
    assert!(headers
        .iter()
        .any(|(name, _)| name.as_str() == "copilot-integration-id"));
}

#[test]
fn stack_web_search_rejection_includes_managed_copilot() {
    let copilot = create_copilot_provider();
    assert!(codex_stack_upstream_rejects_web_search(&copilot, None));

    let ordinary = create_provider(json!({
        "config": r#"
model_provider = "custom"
[model_providers.custom]
base_url = "https://relay.example.com/v1"
wire_api = "responses"
"#
    }));
    assert!(!codex_stack_upstream_rejects_web_search(&ordinary, None));
    assert!(codex_stack_upstream_rejects_web_search(
        &ordinary,
        Some("MiniMaxAI/MiniMax-M3")
    ));
}

#[test]
fn copilot_catalog_profile_survives_live_projection_and_format_overrides() {
    use crate::codex_config::{codex_disables_web_search, CodexCatalogToolProfile};
    use crate::live::project::codex::{CodexProjection, RowInput};

    for base_url in ["https://api.githubcopilot.com", "http://localhost:15721/v1"] {
        let mut provider = create_copilot_provider();
        provider.settings_config = json!({
            "auth": {},
            "config": format!(
                "model = \"claude-sonnet-5\"\nmodel_provider = \"copilot\"\n\n[model_providers.copilot]\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
            )
        });
        let projection = CodexProjection::of(&RowInput {
            settings: &provider.settings_config,
            official: is_codex_official_provider(&provider),
            proxy_injected_oauth: provider.uses_proxy_injected_oauth(),
        })
        .unwrap();
        let config = projection.catalog_input_text();
        let parsed: TomlValue = toml::from_str(&config).unwrap();
        assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
        let route = &parsed["model_providers"]["custom"];
        assert_eq!(route["wire_api"].as_str(), Some("responses"));
        assert!(route.get("requires_openai_auth").is_none());

        for api_format in [
            None,
            Some("openai_chat"),
            Some("openai_responses"),
            Some("anthropic"),
        ] {
            for transport in [
                None,
                Some("auto"),
                Some("openai_responses"),
                Some("openai_chat"),
            ] {
                let meta = provider.meta.as_mut().unwrap();
                meta.api_format = api_format.map(str::to_string);
                meta.codex_copilot_api_format = transport.map(str::to_string);
                let profile = resolve_codex_catalog_tool_profile(&provider);
                assert_eq!(profile, CodexCatalogToolProfile::Copilot);
                assert!(codex_disables_web_search(
                    &provider.settings_config,
                    &config,
                    profile
                ));
            }
        }
    }
}

#[test]
fn codex_responses_endpoint_detection_ignores_query() {
    assert!(is_codex_responses_endpoint("/responses"));
    assert!(is_codex_responses_endpoint(
        "/v1/responses/compact?client_version=0.149"
    ));
    assert!(!is_codex_responses_endpoint("/alpha/search"));
}

#[test]
fn grok_build_toml_exposes_upstream_credentials_and_model() {
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "config": r#"
[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "upstream-grok-model"
base_url = "https://relay.example.com/v1/"
name = "Example Relay"
api_key = "grok-secret"
api_backend = "responses"
context_window = 500000
"#
    }));

    assert_eq!(
        adapter.extract_base_url(&provider).unwrap(),
        "https://relay.example.com/v1"
    );
    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "grok-secret");
    assert_eq!(auth.strategy, AuthStrategy::Bearer);
    assert_eq!(
        codex_provider_upstream_model(&provider).as_deref(),
        Some("upstream-grok-model")
    );
}

#[test]
fn explicit_codex_official_cards_use_chatgpt_backend() {
    let mut provider = create_provider(json!({
        "auth": {
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": { "refresh_token": "legacy-live-only-token" }
        },
        "config": ""
    }));
    provider.id = "unbound-official-account".to_string();
    provider.category = Some("official".to_string());
    assert!(is_codex_official_provider(&provider));

    let mut native = provider.clone();
    native.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
    assert!(is_codex_official_provider(&native));

    provider.id = "managed-official-account".to_string();
    provider.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        auth_binding: Some(crate::provider::AuthBinding {
            source: crate::provider::AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".to_string()),
            account_id: Some("acct-managed".to_string()),
        }),
        ..Default::default()
    });
    let adapter = CodexAdapter::new();

    assert!(is_codex_official_provider(&provider));
    assert_eq!(
        adapter
            .extract_base_url(&provider)
            .expect("official base url"),
        "https://chatgpt.com/backend-api/codex"
    );
    assert!(adapter.extract_auth(&provider).is_none());
    assert_eq!(
        adapter.build_url(
            "https://chatgpt.com/backend-api/codex",
            "/responses/compact"
        ),
        "https://chatgpt.com/backend-api/codex/responses/compact"
    );

    let mut official_api_key = create_provider(json!({
        "auth": { "OPENAI_API_KEY": "sk-official" },
        "config": ""
    }));
    official_api_key.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&official_api_key));

    let mut stored_bearer = create_provider(json!({
        "auth": {},
        "config": "experimental_bearer_token = \"sk-legacy\""
    }));
    stored_bearer.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&stored_bearer));

    let mut unmarked_custom = create_provider(json!({
        "auth": {},
        "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://example.com/v1\""
    }));
    unmarked_custom.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&unmarked_custom));

    let mut category_less_fixed_custom = unmarked_custom.clone();
    category_less_fixed_custom.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
    category_less_fixed_custom.category = None;
    assert!(!is_codex_official_provider(&category_less_fixed_custom));

    let mut category_less_fixed_api_key = official_api_key.clone();
    category_less_fixed_api_key.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
    category_less_fixed_api_key.category = None;
    assert!(!is_codex_official_provider(&category_less_fixed_api_key));

    let mut explicit_openai = create_provider(json!({
        "auth": { "OPENAI_API_KEY": "sk-official" },
        "config": "model_provider = \"openai\""
    }));
    explicit_openai.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&explicit_openai));

    let mut managed_with_null_config = provider.clone();
    managed_with_null_config.category = None;
    managed_with_null_config.settings_config["config"] = JsonValue::Null;
    assert!(is_codex_official_provider(&managed_with_null_config));

    let mut unified_session = create_provider(json!({
        "auth": {},
        // 旧版「统一会话历史」注入进 live、又被回填进行里的形态。
        "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"OpenAI\"\nrequires_openai_auth = true\nsupports_websockets = true\nwire_api = \"responses\"\n"
    }));
    unified_session.category = Some("official".to_string());
    assert!(is_codex_official_provider(&unified_session));

    let mut implicit_custom = create_provider(json!({
        "auth": {},
        "config": "model_provider = \"ollama\""
    }));
    implicit_custom.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&implicit_custom));

    let mut grok_official = create_provider(json!({ "config": "" }));
    grok_official.id = crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID.to_string();
    grok_official.category = Some("official".to_string());
    assert!(!is_codex_official_provider(&grok_official));
}

#[test]
fn prompt_cache_routing_auto_enables_known_upstreams_only() {
    let kimi = create_provider(json!({
        "config": r#"
model_provider = "custom"
[model_providers.custom]
base_url = "https://api.kimi.com/coding/v1"
wire_api = "responses"
"#
    }));
    let openai = create_provider(json!({
        "base_url": "https://api.openai.com/v1"
    }));
    let unknown = create_provider(json!({
        "base_url": "https://strict.example.com/v1"
    }));

    assert!(should_send_codex_chat_prompt_cache_key(&kimi));
    assert!(should_send_codex_chat_prompt_cache_key(&openai));
    assert!(!should_send_codex_chat_prompt_cache_key(&unknown));
}

#[test]
fn prompt_cache_routing_user_override_wins_over_auto_detection() {
    let mut kimi = create_provider(json!({
        "base_url": "https://api.kimi.com/coding/v1"
    }));
    kimi.meta = Some(crate::provider::ProviderMeta {
        prompt_cache_routing: Some("disabled".to_string()),
        ..Default::default()
    });
    assert!(!should_send_codex_chat_prompt_cache_key(&kimi));

    let mut unknown = create_provider(json!({
        "base_url": "https://strict.example.com/v1"
    }));
    unknown.meta = Some(crate::provider::ProviderMeta {
        prompt_cache_routing: Some("enabled".to_string()),
        ..Default::default()
    });
    assert!(should_send_codex_chat_prompt_cache_key(&unknown));
}

#[test]
fn prompt_cache_key_prefers_explicit_key_then_real_session() {
    let provider = create_provider(json!({
        "base_url": "https://api.kimi.com/coding/v1"
    }));
    let mut explicit_body = json!({ "model": "kimi-for-coding" });
    assert!(inject_codex_chat_prompt_cache_key(
        &provider,
        &mut explicit_body,
        Some("request-key"),
        Some("session-key"),
    ));
    assert_eq!(explicit_body["prompt_cache_key"], "request-key");

    let mut session_body = json!({ "model": "kimi-for-coding" });
    assert!(inject_codex_chat_prompt_cache_key(
        &provider,
        &mut session_body,
        None,
        Some("session-key"),
    ));
    assert_eq!(session_body["prompt_cache_key"], "session-key");
}

#[test]
fn prompt_cache_key_is_not_injected_without_real_session_or_support() {
    let kimi = create_provider(json!({
        "base_url": "https://api.kimi.com/coding/v1"
    }));
    let mut no_session_body = json!({ "model": "kimi-for-coding" });
    assert!(!inject_codex_chat_prompt_cache_key(
        &kimi,
        &mut no_session_body,
        None,
        None,
    ));
    assert!(no_session_body.get("prompt_cache_key").is_none());

    let unknown = create_provider(json!({
        "base_url": "https://strict.example.com/v1"
    }));
    let mut unsupported_body = json!({ "model": "other" });
    assert!(!inject_codex_chat_prompt_cache_key(
        &unknown,
        &mut unsupported_body,
        Some("request-key"),
        Some("session-key"),
    ));
    assert!(unsupported_body.get("prompt_cache_key").is_none());
}

#[test]
fn test_extract_base_url_direct() {
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "base_url": "https://api.openai.com/v1"
    }));

    let url = adapter.extract_base_url(&provider).unwrap();
    assert_eq!(url, "https://api.openai.com/v1");
}

#[test]
fn test_extract_base_url_from_selected_config_provider() {
    let adapter = CodexAdapter::new();
    let configs = [
        // Another provider table comes first.
        r#"model_provider = "relay"

[model_providers.azure]
name = "Azure"
base_url = "https://azure.example.com/v1"
env_key = "AZURE_OPENAI_API_KEY"

[model_providers.relay]
name = "Relay"
base_url = "https://relay.example.com/v1"
"#,
        // The previous URL is commented out above the current one.
        r#"model_provider = "relay"

[model_providers.relay]
name = "Relay"
# base_url = "https://old-relay.example.com/v1"
base_url = "https://relay.example.com/v1"
"#,
        // No spaces around `=`.
        r#"model_provider = "relay"

[model_providers.relay]
name = "Relay"
base_url="https://relay.example.com/v1/"
"#,
    ];

    for config in configs {
        let provider = create_provider(json!({ "config": config }));
        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            "https://relay.example.com/v1",
            "config:\n{config}"
        );
    }
}

#[test]
fn test_extract_auth_from_auth_field() {
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "auth": {
            "OPENAI_API_KEY": "sk-test-key-12345678"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-test-key-12345678");
    assert_eq!(auth.strategy, AuthStrategy::Bearer);
}

#[test]
fn test_extract_auth_falls_back_to_config_bearer_when_auth_key_empty() {
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "auth": {
            "OPENAI_API_KEY": ""
        },
        "config": r#"model_provider = "custom"

[model_providers.custom]
experimental_bearer_token = "sk-config-key"
"#
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-config-key");
    assert_eq!(auth.strategy, AuthStrategy::Bearer);
}

#[test]
fn test_extract_auth_from_env() {
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "env": {
            "OPENAI_API_KEY": "sk-env-key-12345678"
        }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.api_key, "sk-env-key-12345678");
}

#[test]
fn test_build_url_cases() {
    for (case, base_url, endpoint, expected) in [
        (
            "test_build_url",
            "https://api.openai.com/v1",
            "/responses",
            "https://api.openai.com/v1/responses",
        ),
        (
            "test_build_url_origin_adds_v1",
            "https://api.openai.com",
            "/responses",
            "https://api.openai.com/v1/responses",
        ),
        (
            "test_build_url_custom_prefix_no_v1",
            "https://example.com/openai",
            "/responses",
            "https://example.com/openai/responses",
        ),
        // base_url 已包含 /v1，endpoint 也包含 /v1
        (
            "test_build_url_dedup_v1",
            "https://www.packyapi.com/v1",
            "/v1/responses",
            "https://www.packyapi.com/v1/responses",
        ),
    ] {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url(base_url, endpoint);
        assert_eq!(url, expected, "{case}");
    }
}

// ==================== anthropic upstream detection ====================

#[test]
fn test_uses_anthropic_from_settings_api_format() {
    let provider = create_provider(json!({ "apiFormat": "anthropic" }));
    assert!(codex_provider_uses_anthropic(&provider));

    let provider = create_provider(json!({ "api_format": "anthropic_messages" }));
    assert!(codex_provider_uses_anthropic(&provider));
}

#[test]
fn test_uses_anthropic_from_meta_api_format() {
    let mut provider = create_provider(json!({}));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("anthropic".to_string()),
        ..Default::default()
    });
    assert!(codex_provider_uses_anthropic(&provider));
}

#[test]
fn test_uses_anthropic_from_toml_wire_api() {
    let provider = create_provider(json!({
        "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
    }));
    assert!(codex_provider_uses_anthropic(&provider));
}

#[test]
fn test_anthropic_false_for_chat_and_responses() {
    let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
    assert!(!codex_provider_uses_anthropic(&chat));
    let responses = create_provider(json!({ "apiFormat": "openai_responses" }));
    assert!(!codex_provider_uses_anthropic(&responses));
}

#[test]
fn test_anthropic_and_chat_are_mutually_exclusive() {
    let anth = create_provider(json!({ "apiFormat": "anthropic" }));
    assert!(codex_provider_uses_anthropic(&anth));
    assert!(!codex_provider_uses_chat_completions(&anth));

    let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
    assert!(codex_provider_uses_chat_completions(&chat));
    assert!(!codex_provider_uses_anthropic(&chat));
}

#[test]
fn test_should_convert_responses_to_anthropic_path_guard() {
    let provider = create_provider(json!({ "apiFormat": "anthropic" }));
    assert!(should_convert_codex_responses_to_anthropic(
        &provider,
        "/responses"
    ));
    assert!(should_convert_codex_responses_to_anthropic(
        &provider,
        "/v1/responses/compact"
    ));
    assert!(should_convert_codex_responses_to_anthropic(
        &provider,
        "/responses?x=1"
    ));
    assert!(!should_convert_codex_responses_to_anthropic(
        &provider,
        "/chat/completions"
    ));
}

#[test]
fn test_resolve_catalog_profile_matches_router() {
    use crate::codex_config::CodexCatalogToolProfile;

    // Anthropic declared only via TOML wire_api (no meta.api_format) must still
    // resolve to the Anthropic catalog profile — this is the routing/catalog
    // divergence that let apply_patch leak through.
    let toml_anthropic = create_provider(json!({
        "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
    }));
    assert_eq!(
        resolve_codex_catalog_tool_profile(&toml_anthropic),
        CodexCatalogToolProfile::Anthropic
    );

    // Anthropic via settings apiFormat.
    let settings_anthropic = create_provider(json!({ "apiFormat": "anthropic" }));
    assert_eq!(
        resolve_codex_catalog_tool_profile(&settings_anthropic),
        CodexCatalogToolProfile::Anthropic
    );

    // Native openai_responses (meta) → NativeResponses; chat → ProxyChat.
    let mut native = create_provider(json!({}));
    native.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    assert_eq!(
        resolve_codex_catalog_tool_profile(&native),
        CodexCatalogToolProfile::NativeResponses
    );

    let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
    assert_eq!(
        resolve_codex_catalog_tool_profile(&chat),
        CodexCatalogToolProfile::ProxyChat
    );

    let copilot = create_copilot_provider();
    assert_eq!(
        resolve_codex_catalog_tool_profile(&copilot),
        CodexCatalogToolProfile::Copilot
    );

    // Host fallback (#6944): a DB row saved while the Zhipu preset was still
    // `openai_chat` but whose base_url is the vendor's native Responses
    // gateway (`/api/v1`) resolves to NativeResponses without a re-save —
    // whether the URL lives in the TOML or in settings `baseURL`.
    let legacy_chat_toml = |base_url: &str| {
        create_provider(json!({
            "apiFormat": "openai_chat",
            "config": format!(
                "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"zhipu_glm\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
            )
        }))
    };
    for base_url in [
        "https://open.bigmodel.cn/api/v1",
        "https://api.z.ai/api/v1",
        "https://Open.BigModel.cn/api/v1/",
    ] {
        assert_eq!(
            resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
            CodexCatalogToolProfile::NativeResponses,
            "{base_url}"
        );
    }
    let legacy_chat_settings = create_provider(json!({
        "apiFormat": "openai_chat",
        "baseURL": "https://api.z.ai/api/v1"
    }));
    assert_eq!(
        resolve_codex_catalog_tool_profile(&legacy_chat_settings),
        CodexCatalogToolProfile::NativeResponses
    );

    // The vendor's Chat Completions endpoints are NOT its Responses gateway
    // (Zhipu: `/api/coding/paas/v4` Coding Plan, `/api/paas/v4` pay-as-you-go)
    // — a stale row there keeps ProxyChat so it is never steered onto the
    // endpoint Zhipu documents as unable to use Coding Plan quota.
    for base_url in [
        "https://open.bigmodel.cn/api/coding/paas/v4",
        "https://api.z.ai/api/coding/paas/v4",
        "https://open.bigmodel.cn/api/paas/v4",
        "https://open.bigmodel.cn/api/paas/v4/chat/completions",
    ] {
        assert_eq!(
            resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
            CodexCatalogToolProfile::ProxyChat,
            "{base_url}"
        );
    }

    // Host matching is on DNS labels: `xyz.ai` must not be captured by the
    // 4-char `z.ai` entry (it would lose apply_patch and web_search).
    for base_url in [
        "https://api.xyz.ai/v1",
        "https://viz.ai/v1",
        "https://z.ai.example.com/v1",
    ] {
        assert_eq!(
            resolve_codex_catalog_tool_profile(&legacy_chat_toml(base_url)),
            CodexCatalogToolProfile::ProxyChat,
            "{base_url}"
        );
    }

    // An explicit `openai_responses` on an unlisted host is untouched by the
    // fallback (it only ever widens toward NativeResponses for listed hosts).
    let explicit_native = create_provider(json!({
        "apiFormat": "openai_responses",
        "baseURL": "https://api.xyz.ai/v1"
    }));
    assert_eq!(
        resolve_codex_catalog_tool_profile(&explicit_native),
        CodexCatalogToolProfile::NativeResponses
    );
}

#[test]
fn native_responses_url_guard_matches_gateway_not_chat_paths() {
    for url in [
        "https://open.bigmodel.cn/api/v1",
        "https://api.z.ai/api/v1",
        "https://api.xiaomimimo.com/v1",
        "https://token-plan-cn.xiaomimimo.com/v1",
        "https://api.minimaxi.com/v1",
        "https://api.minimax.cn/v1",
        "https://api.minimax.io/v1",
        "https://api.longcat.chat/openai/v1",
    ] {
        assert!(is_codex_native_responses_url(url), "{url}");
    }
    for url in [
        "https://open.bigmodel.cn/api/coding/paas/v4",
        "https://api.z.ai/api/coding/paas/v4",
        "https://open.bigmodel.cn/api/paas/v4",
        "https://api.minimaxi.com/v1/chat/completions",
        "https://api.minimax.cn/v1/chat/completions",
        "https://api.minimax.cn.example.com/v1",
        "https://api.xyz.ai/v1",
        "https://api.deepseek.com",
        "",
    ] {
        assert!(!is_codex_native_responses_url(url), "{url}");
    }
}

#[test]
fn new_native_presets_respect_explicit_format_without_reclassifying_chat() {
    use crate::codex_config::CodexCatalogToolProfile;

    for base_url in [
        "https://api.stepfun.com/v1",
        "https://api.stepfun.ai/v1",
        "https://api.stepfun.com/step_plan/v1",
        "https://qianfan.baidubce.com/v2",
        "https://maas-coding-api.cn-huabei-1.xf-yun.com/v1",
        "https://tokenhub.tencentmaas.com/plan/v3",
    ] {
        for (api_format, expected) in [
            ("openai_responses", CodexCatalogToolProfile::NativeResponses),
            ("openai_chat", CodexCatalogToolProfile::ProxyChat),
        ] {
            let provider = create_provider(json!({
                "apiFormat": api_format,
                "baseURL": base_url,
            }));
            assert_eq!(
                resolve_codex_catalog_tool_profile(&provider),
                expected,
                "{api_format} @ {base_url}",
            );
        }
    }
}

#[test]
fn test_apply_codex_upstream_model_preserves_one_m_catalog_model() {
    // Regression for the [1m] path: a request model carrying the [1m] marker must
    // match its catalog entry and be preserved (not overridden by the provider
    // default) so the transform can later strip [1m] and emit the context-1m beta.
    // This only works because the forwarder no longer strips [1m] before this call
    // on the Anthropic path.
    let provider = create_provider(json!({
        "config": r#"model_provider = "custom"
model = "claude-opus-4-1"

[model_providers.custom]
wire_api = "anthropic"
"#,
        "modelCatalog": {
            "models": [
                { "model": "claude-opus-4-1[1m]" }
            ]
        }
    }));
    let mut body = json!({ "model": "claude-opus-4-1[1m]", "input": "hi" });
    let result = apply_codex_upstream_model(&provider, &mut body);
    assert_eq!(result.as_deref(), Some("claude-opus-4-1[1m]"));
    assert_eq!(
        body.get("model").and_then(|v| v.as_str()),
        Some("claude-opus-4-1[1m]")
    );
}

#[test]
fn test_anthropic_auth_defaults_to_bearer() {
    // No meta.apiKeyField (defaults to ANTHROPIC_AUTH_TOKEN) → Authorization: Bearer only
    let adapter = CodexAdapter::new();
    let provider = create_provider(json!({
        "apiFormat": "anthropic",
        "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
    }));

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.strategy, AuthStrategy::Bearer);

    let headers = adapter.get_auth_headers(&auth).unwrap();
    let names: Vec<String> = headers
        .iter()
        .map(|(name, _)| name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["authorization".to_string()]);
}

#[test]
fn test_anthropic_auth_x_api_key_when_selected() {
    // meta.apiKeyField = ANTHROPIC_API_KEY → x-api-key only
    let adapter = CodexAdapter::new();
    let mut provider = create_provider(json!({
        "apiFormat": "anthropic",
        "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("anthropic".to_string()),
        api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
        ..Default::default()
    });

    let auth = adapter.extract_auth(&provider).unwrap();
    assert_eq!(auth.strategy, AuthStrategy::Anthropic);

    let headers = adapter.get_auth_headers(&auth).unwrap();
    let names: Vec<String> = headers
        .iter()
        .map(|(name, _)| name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["x-api-key".to_string()]);
}

#[test]
fn test_codex_provider_uses_chat_completions_from_active_wire_api() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "chat_only"
model = "gpt-5"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#
    }));

    assert!(codex_provider_uses_chat_completions(&provider));
    assert!(should_convert_codex_responses_to_chat(
        &provider,
        "/responses?stream=true"
    ));
    assert!(!should_convert_codex_responses_to_chat(
        &provider,
        "/chat/completions"
    ));
}

#[test]
fn test_codex_provider_uses_chat_completions_from_full_chat_url() {
    let provider = create_provider(json!({
        "base_url": "https://example.com/v1/chat/completions"
    }));

    assert!(codex_provider_uses_chat_completions(&provider));
    assert!(should_convert_codex_responses_to_chat(
        &provider,
        "/v1/responses/compact"
    ));
}

#[test]
fn test_codex_provider_uses_chat_completions_from_meta_api_format_for_compact() {
    let mut provider = create_provider(json!({
        "base_url": "https://example.com/v1"
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });

    assert!(codex_provider_uses_chat_completions(&provider));
    assert!(should_convert_codex_responses_to_chat(
        &provider,
        "/responses/compact?stream=true"
    ));
}

#[test]
fn test_codex_provider_uses_chat_completions_from_meta_api_format_for_responses() {
    let mut provider = create_provider(json!({
        "base_url": "https://api.deepseek.com/v1"
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });

    assert!(should_convert_codex_responses_to_chat(
        &provider,
        "/v1/responses"
    ));
}

#[test]
fn test_apply_codex_chat_upstream_model_uses_provider_config_model() {
    let mut provider = create_provider(json!({
        "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });
    let mut body = json!({
        "model": "placeholder-client-model",
        "input": "ping"
    });

    let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

    assert_eq!(upstream_model.as_deref(), Some("deepseek-v4-flash"));
    assert_eq!(
        body.get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash")
    );
}

#[test]
fn test_apply_codex_chat_upstream_model_preserves_catalog_model_selection() {
    let mut provider = create_provider(json!({
        "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#,
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-flash" },
                { "model": "kimi-k2" }
            ]
        }
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });
    let mut body = json!({
        "model": "kimi-k2",
        "input": "ping"
    });

    let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

    assert_eq!(upstream_model.as_deref(), Some("kimi-k2"));
    assert_eq!(body.get("model").and_then(|v| v.as_str()), Some("kimi-k2"));
}

#[test]
fn test_resolve_codex_chat_reasoning_infers_deepseek_effort_support() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
    }));

    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
            .unwrap();

    assert_eq!(config.supports_thinking, Some(true));
    assert_eq!(config.supports_effort, Some(true));
    assert_eq!(config.effort_value_mode.as_deref(), Some("deepseek"));
}

#[test]
fn test_resolve_codex_chat_reasoning_explicit_meta_overrides_inference() {
    let mut provider = create_provider(json!({
        "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        codex_chat_reasoning: Some(CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(false),
            thinking_param: Some("none".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("auto".to_string()),
            effort_levels: None,
        }),
        ..Default::default()
    });

    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
            .unwrap();

    assert_eq!(config.supports_thinking, Some(false));
    assert_eq!(config.supports_effort, Some(false));
    assert_eq!(config.thinking_param.as_deref(), Some("none"));
    assert_eq!(config.effort_param.as_deref(), Some("none"));
}

#[test]
fn test_resolve_codex_chat_reasoning_repairs_enabled_none_effort_param() {
    let mut provider = create_provider(json!({
        "config": r#"
model_provider = "copilot"
model = "claude-opus-5.5"

[model_providers.copilot]
name = "GitHub Copilot"
base_url = "https://api.githubcopilot.com"
wire_api = "chat"
"#
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        codex_chat_reasoning: Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: Some("passthrough".to_string()),
            output_format: Some("auto".to_string()),
            effort_levels: None,
        }),
        ..Default::default()
    });
    let request = json!({
        "model": "claude-opus-5.5",
        "input": "hello",
        "reasoning": { "effort": "max" }
    });

    let config = resolve_codex_chat_reasoning_config(&provider, &request).unwrap();
    assert_eq!(config.effort_param.as_deref(), Some("reasoning_effort"));

    let transformed = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
        request,
        Some(&config),
        true,
    )
    .unwrap();
    assert_eq!(transformed["reasoning_effort"], "max");
}

#[test]
fn test_resolve_codex_chat_reasoning_openrouter_platform_overrides_model() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "openrouter"
model = "deepseek/deepseek-chat-v3.1"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
wire_api = "chat"
"#
    }));

    // 模型名含 "deepseek"，但平台是 OpenRouter —— 平台规则必须覆盖模型规则。
    let config = resolve_codex_chat_reasoning_config(
        &provider,
        &json!({ "model": "deepseek/deepseek-chat-v3.1" }),
    )
    .unwrap();

    assert_eq!(config.thinking_param.as_deref(), Some("none"));
    assert_eq!(config.effort_param.as_deref(), Some("reasoning.effort"));
    assert_eq!(config.effort_value_mode.as_deref(), Some("openrouter"));
    assert_eq!(config.supports_effort, Some(true));
}

#[test]
fn test_resolve_codex_chat_reasoning_siliconflow_platform_overrides_minimax() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "siliconflow"
model = "MiniMaxAI/MiniMax-M2.5"

[model_providers.siliconflow]
name = "SiliconFlow"
base_url = "https://api.siliconflow.cn/v1"
wire_api = "chat"
"#
    }));

    // 模型是 MiniMax（官方用 reasoning_split），但平台是 SiliconFlow —— 应走平台的 enable_thinking。
    let config = resolve_codex_chat_reasoning_config(
        &provider,
        &json!({ "model": "MiniMaxAI/MiniMax-M2.5" }),
    )
    .unwrap();

    assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
    assert_eq!(config.supports_effort, Some(false));
    assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
}

#[test]
fn test_resolve_codex_chat_reasoning_modelscope_platform_overrides_glm() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "modelscope"
model = "ZhipuAI/GLM-5.2"

[model_providers.modelscope]
name = "ModelScope"
base_url = "https://api-inference.modelscope.cn/v1"
wire_api = "chat"
"#
    }));

    // 模型是 GLM（智谱自家用 thinking:{type}），但平台是 ModelScope ——
    // 应走平台级 enable_thinking，而不是被 glm 模型规则注入智谱方言。
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "ZhipuAI/GLM-5.2" }))
            .unwrap();

    assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
    assert_eq!(config.supports_effort, Some(false));
    assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
}

#[test]
fn test_resolve_codex_chat_reasoning_opencode_zen_platform_overrides_model_vendor() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "opencode"
model = "glm-5.2"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#
    }));

    // 模型是 GLM（智谱自家用 thinking:{type} 方言），但平台是 OpenCode Zen ——
    // 必须走平台配置：顶层 reasoning_effort + reasoning_content，
    // 不得注入智谱 thinking 形状。
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "glm-5.2" })).unwrap();

    assert_eq!(config.supports_thinking, Some(true));
    assert_eq!(config.supports_effort, Some(true));
    assert_eq!(config.thinking_param.as_deref(), Some("none"));
    assert_eq!(config.effort_param.as_deref(), Some("reasoning_effort"));
    assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
    assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
}

#[test]
fn test_resolve_codex_chat_reasoning_mistral_api_sends_no_reasoning_fields() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "mistral"
model = "deepseek-v4-1-flash-1017"

[model_providers.mistral]
name = "Mistral AI"
base_url = "https://api.mistral.ai/v1"
wire_api = "chat"
"#
    }));

    // Mistral 托管的 deepseek 名模型不得套 DeepSeek 规则注入 thinking（#9507）。
    let request = json!({
        "model": "deepseek-v4-1-flash-1017",
        "input": "hi",
        "reasoning": {"effort": "high"}
    });
    let config = resolve_codex_chat_reasoning_config(&provider, &request).unwrap();
    let chat = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
        request,
        Some(&config),
        // 与 forwarder 对 api.mistral.ai 的取值一致。
        false,
    )
    .unwrap();

    assert!(chat.get("thinking").is_none());
    assert!(chat.get("reasoning_effort").is_none());
    assert!(chat.get("reasoning").is_none());
}

#[test]
fn test_resolve_codex_chat_reasoning_mistral_rule_needs_exact_host() {
    for base_url in [
        "https://api.mistral.ai.example/v1",
        "https://relay.example/api.mistral.ai/v1",
    ] {
        let provider = create_provider(json!({
            "config": format!(
                "model_provider = \"relay\"\nmodel = \"deepseek-v4-pro\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"{base_url}\"\nwire_api = \"chat\"\n"
            )
        }));

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
                .unwrap();

        assert_eq!(
            config.thinking_param.as_deref(),
            Some("thinking"),
            "{base_url}"
        );
        assert_eq!(
            config.effort_value_mode.as_deref(),
            Some("deepseek"),
            "{base_url}"
        );
    }
}

#[test]
fn test_resolve_codex_chat_reasoning_zen_attaches_per_model_effort_levels() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "opencode"
model = "glm-5.2"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#,
        "modelCatalog": {
            "models": [
                { "model": "glm-5.2", "reasoningLevels": ["high", "max"] },
                { "model": "deepseek-v4-flash", "reasoningLevels": ["low", "high", "max"] },
                { "model": "glm-5.1" }
            ]
        }
    }));

    // 逐模型查表：声明了 reasoningLevels 的模型附上各自档位（模型名大小写不敏感）。
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "GLM-5.2" })).unwrap();
    assert_eq!(
        config.effort_levels,
        Some(vec!["high".to_string(), "max".to_string()])
    );

    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-flash" }))
            .unwrap();
    assert_eq!(
        config.effort_levels,
        Some(vec![
            "low".to_string(),
            "high".to_string(),
            "max".to_string()
        ])
    );

    // toggle 型模型（目录条目未声明 reasoningLevels）→ None：转换层不发 effort 字段。
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "glm-5.1" })).unwrap();
    assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
    assert!(config.effort_levels.is_none());

    // 目录未收录的模型 → 同样 None。
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "kimi-k3" })).unwrap();
    assert!(config.effort_levels.is_none());
}

#[test]
fn test_resolve_codex_chat_reasoning_zen_levels_attach_on_explicit_meta_too() {
    let mut provider = create_provider(json!({
        "config": r#"
model_provider = "opencode"
model = "deepseek-v4-flash"

[model_providers.opencode]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"
"#,
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-flash", "reasoning_levels": ["low", "high", "max"] }
            ]
        }
    }));
    // 显式 meta（表单手选 zen 模式）同样要在 resolve 末端按请求模型附表；
    // 且手写/旧数据可能是 snake_case 的 reasoning_levels（加载侧双格式兼容）。
    provider.meta = Some(crate::provider::ProviderMeta {
        codex_chat_reasoning: Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("zen".to_string()),
            output_format: Some("reasoning_content".to_string()),
            effort_levels: None,
        }),
        ..Default::default()
    });

    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-flash" }))
            .unwrap();

    assert_eq!(config.effort_value_mode.as_deref(), Some("zen"));
    assert_eq!(
        config.effort_levels,
        Some(vec![
            "low".to_string(),
            "high".to_string(),
            "max".to_string()
        ])
    );
}

#[test]
fn test_infer_codex_chat_reasoning_stepfun_per_model_effort() {
    let provider = create_provider(json!({
        "config": r#"
model_provider = "stepfun"
model = "step-3.7-flash"

[model_providers.stepfun]
name = "StepFun"
base_url = "https://api.stepfun.com/step_plan/v1"
wire_api = "chat"
"#
    }));

    // step-3.7-flash：官方三档 low/medium/high —— 必须 passthrough，
    // 套 low_high 会把 medium 塌成 high（假差异档）
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "step-3.7-flash" }))
            .unwrap();
    assert_eq!(config.supports_effort, Some(true));
    assert_eq!(config.effort_value_mode.as_deref(), Some("passthrough"));

    // step-3.5-flash-2603：官方两档 low/high，沿用收敛映射
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "step-3.5-flash-2603" }))
            .unwrap();
    assert_eq!(config.supports_effort, Some(true));
    assert_eq!(config.effort_value_mode.as_deref(), Some("low_high"));

    // 无后缀 step-3.5-flash：官方未暴露 effort，不下发
    let config =
        resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "step-3.5-flash" }))
            .unwrap();
    assert_eq!(config.supports_effort, Some(false));
    assert_eq!(config.thinking_param.as_deref(), Some("none"));
}

#[test]
fn xai_oauth_invariants_ignore_editable_base_url_and_auth() {
    let adapter = CodexAdapter::new();
    let mut provider = create_provider(json!({
        "auth": { "OPENAI_API_KEY": "user-edited" },
        "config": r#"
model = "grok-4.5"

[model_providers.custom]
name = "xai"
base_url = "https://attacker.example/v1"
wire_api = "responses"
"#
    }));
    provider.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("xai_oauth".to_string()),
        ..Default::default()
    });

    // 可编辑字段（base_url / auth key）不得影响托管路由：
    // 端点硬定向 api.x.ai，凭据是占位符（真 token 由 forwarder 注入）。
    assert_eq!(
        adapter.extract_base_url(&provider).unwrap(),
        super::super::XAI_API_BASE_URL
    );
    let auth = adapter
        .extract_auth(&provider)
        .expect("managed auth placeholder");
    assert_eq!(auth.api_key, "xai_oauth_placeholder");
    assert_eq!(auth.strategy, AuthStrategy::XaiOAuth);
}

#[test]
fn xai_oauth_pins_native_responses_catalog_profile() {
    let mut provider = create_provider(json!({ "auth": {}, "config": "" }));
    provider.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("xai_oauth".to_string()),
        // 即使 api_format 被改成 anthropic，catalog 画像也必须钉死原生 Responses
        api_format: Some("anthropic".to_string()),
        ..Default::default()
    });

    assert!(matches!(
        resolve_codex_catalog_tool_profile(&provider),
        crate::codex_config::CodexCatalogToolProfile::NativeResponses
    ));
}

#[test]
fn namespace_flatten_gate_fires_for_xai_oauth_and_api_xai_responses() {
    let mut xai = create_provider(json!({ "auth": {}, "config": "" }));
    xai.meta = Some(crate::provider::ProviderMeta {
        provider_type: Some("xai_oauth".to_string()),
        ..Default::default()
    });
    assert!(provider_needs_responses_namespace_flatten(&xai, None));

    // API-key Grok cards (no xai_oauth meta) still talk to api.x.ai Responses.
    let grok_key = create_provider(json!({
        "auth": { "OPENAI_API_KEY": "sk-x" },
        "config": r#"
model_provider = "custom"
model = "grok-4.6"

[model_providers.custom]
name = "xai"
base_url = "https://api.x.ai/v1"
wire_api = "responses"
"#
    }));
    assert!(provider_needs_responses_namespace_flatten(&grok_key, None));

    // A non-xAI Responses provider must not be flattened.
    let other = create_provider(json!({
        "auth": { "OPENAI_API_KEY": "sk-x" },
        "config": r#"
[model_providers.custom]
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#
    }));
    assert!(!provider_needs_responses_namespace_flatten(&other, None));
    assert!(!provider_needs_responses_namespace_flatten(
        &other,
        Some("grok-4.7")
    ));
}

#[test]
fn copilot_namespace_compat_uses_only_the_outbound_grok_model() {
    let mut copilot = create_copilot_provider();
    copilot.settings_config = json!({
        "config": "model = \"grok-4.7\"",
        "modelCatalog": {"models": [{"model": "grok-4.7"}, {"model": "gpt-6.1-sol"}]}
    });
    for (model, expected) in [
        (Some("grok-4.7"), true),
        (Some("grok-4.5"), true),
        (Some(" GROK-4.7-fast "), true),
        (Some("gpt-6.1-sol"), false),
        (Some("claude-sonnet-4.6"), false),
        (Some("grokker"), false),
        (Some("模型"), false),
        (Some(""), false),
        (None, false),
    ] {
        assert_eq!(
            provider_needs_responses_namespace_flatten(&copilot, model),
            expected,
            "{model:?}"
        );
    }

    copilot.meta = None;
    assert!(!provider_needs_responses_namespace_flatten(
        &copilot,
        Some("grok-4.7")
    ));
}

#[test]
fn strip_private_include_markers_leaves_everything_else() {
    for (case, include, stripped, expected) in [
        (
            "strip_private_include_markers_keeps_other_markers",
            json!(["reasoning.encrypted_content", "output.encrypted_content"]),
            true,
            json!(["reasoning.encrypted_content"]),
        ),
        (
            "strip_private_include_markers_leaves_unrelated_markers_alone",
            json!(["web_search_call.action.sources"]),
            false,
            json!(["web_search_call.action.sources"]),
        ),
        // `include` should only ever hold strings; if something else shows up, the
        // private marker still goes but the unknown entry is left where it was.
        (
            "strip_private_include_markers_keeps_non_string_entries",
            json!(["output.encrypted_content", 42]),
            true,
            json!([42]),
        ),
    ] {
        let mut body = json!({ "include": include });

        assert_eq!(
            strip_codex_private_include_markers(&mut body),
            stripped,
            "{case}"
        );
        assert_eq!(body["include"], expected, "{case}");
    }
}

#[test]
fn strip_private_include_markers_removes_empty_include() {
    let mut body = json!({ "include": ["output.encrypted_content"] });

    assert!(strip_codex_private_include_markers(&mut body));
    assert!(body.get("include").is_none());
}

#[test]
fn strip_private_include_markers_ignores_shapes_it_does_not_own() {
    // Missing key.
    let mut body = json!({ "model": "m" });
    assert!(!strip_codex_private_include_markers(&mut body));

    // Not an array.
    let mut body = json!({ "include": "output.encrypted_content" });
    assert!(!strip_codex_private_include_markers(&mut body));
    assert_eq!(body["include"], json!("output.encrypted_content"));

    // Non-object body.
    let mut body = json!(["output.encrypted_content"]);
    assert!(!strip_codex_private_include_markers(&mut body));
}

#[test]
fn strip_private_include_markers_is_idempotent() {
    let mut body = json!({ "include": ["reasoning.encrypted_content"] });

    assert!(!strip_codex_private_include_markers(&mut body));
    assert!(!strip_codex_private_include_markers(&mut body));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
}

#[test]
fn strip_openai_only_fields_leaves_custom_table_requests_byte_identical() {
    // 和 Codex 对自定义表发的请求一样：没有任何只发给 OpenAI 的字段，什么都不动。
    let mut body = json!({
        "model": "m",
        "include": ["reasoning.encrypted_content"],
        "input": [
            { "type": "message", "role": "user", "content": [] },
            { "type": "reasoning", "summary": [], "encrypted_content": "r" },
            { "type": "compaction", "encrypted_content": "c" },
            { "type": "context_compaction", "encrypted_content": "cc" }
        ]
    });
    let before = body.clone();

    assert!(!strip_codex_openai_only_request_fields(&mut body));
    assert_eq!(body, before);
}

#[test]
fn strip_openai_only_fields_keeps_other_stream_options() {
    let mut body = json!({
        "stream_options": { "reasoning_summary_delivery": "sequential_cutoff", "include_usage": true }
    });

    assert!(strip_codex_openai_only_request_fields(&mut body));
    assert_eq!(body["stream_options"], json!({ "include_usage": true }));
}

#[test]
fn strip_openai_only_fields_keeps_compaction_ciphertext() {
    // 压缩条目的密文 Codex 对非 OpenAI 也照发，第三方压缩靠 `codex_compaction` 处理。
    let mut body = json!({
        "input": [
            {
                "type": "compaction_summary",
                "encrypted_content": "c",
                "internal_chat_message_metadata_passthrough": { "turn_id": "t" }
            },
            { "type": "web_search_call", "status": "completed", "encrypted_content": "w" }
        ]
    });

    assert!(strip_codex_openai_only_request_fields(&mut body));
    assert_eq!(
        body["input"],
        json!([
            { "type": "compaction_summary", "encrypted_content": "c" },
            { "type": "web_search_call", "status": "completed" }
        ])
    );
}

#[test]
fn strip_openai_only_fields_ignores_shapes_it_does_not_own() {
    let mut body = json!(["output.encrypted_content"]);
    assert!(!strip_codex_openai_only_request_fields(&mut body));

    // input 不是数组、条目不是对象：不动。
    let mut body = json!({ "input": "hi", "stream_options": "x" });
    assert!(!strip_codex_openai_only_request_fields(&mut body));
    let mut body = json!({ "input": ["hi", 1] });
    assert!(!strip_codex_openai_only_request_fields(&mut body));
    assert_eq!(body["input"], json!(["hi", 1]));
}

#[test]
fn strip_openai_only_fields_is_idempotent() {
    let mut body = json!({
        "include": ["output.encrypted_content"],
        "access_programs": { "cyber": "standard" },
        "input": [{ "type": "configuration_update", "reasoning": { "effort": "high" } }]
    });

    assert!(strip_codex_openai_only_request_fields(&mut body));
    let once = body.clone();
    assert!(!strip_codex_openai_only_request_fields(&mut body));
    assert_eq!(body, once);
    assert_eq!(body, json!({ "input": [] }));
}

/// API-key Grok 卡按 base_url 的 host 认 xAI；中转站地址里带着 api.x.ai 不算。
#[test]
fn xai_namespace_flatten_follows_the_base_url_host() {
    let responses_card = |base_url: &str| {
        create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-x" },
            "config": format!(
                "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
            )
        }))
    };
    for url in [
        "https://api.x.ai/v1",
        "https://API.X.AI:443/v1",
        "https://api.x.ai./v1",
    ] {
        assert!(
            provider_needs_responses_namespace_flatten(&responses_card(url), None),
            "{url}"
        );
    }
    for url in [
        "https://relay.example.com/api.x.ai/v1",
        "https://relay.example.com/v1?upstream=https://api.x.ai/v1",
        "https://api.x.ai.relay.example.com/v1",
    ] {
        assert!(
            !provider_needs_responses_namespace_flatten(&responses_card(url), None),
            "{url}"
        );
    }
}
