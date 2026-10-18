use std::sync::Arc;

use base64::Engine;
use cc_switch_lib::{
    api_key_vars::{
        self,
        runner::{ApiKeyRefreshConfig, RefreshOutput, ScriptRun},
    },
    import_provider_from_deeplink, parse_deeplink_url, AppState, Database,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn provider_link(app: &str) -> url::Url {
    provider_link_with_key(app, "public-test-key")
}

fn provider_link_with_key(app: &str, api_key: &str) -> url::Url {
    let mut url = url::Url::parse("ccswitch://v1/import").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("resource", "provider"),
        ("app", app),
        ("name", "Deep Link Test"),
        ("homepage", "https://example.com"),
        ("endpoint", "https://api.example.com/v1"),
        ("apiKey", api_key),
        ("model", "example-model"),
    ]);
    url
}

fn assert_link_is_rejected_before_persistence(url: &url::Url, app: &str) {
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());
    seed_available_variables(&state);
    let error = match parse_deeplink_url(url.as_str()) {
        Ok(request) => import_provider_from_deeplink(&state, request)
            .expect_err("untrusted key reference must not be imported"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("cannot import API key"),
        "unexpected error for {app}: {error}"
    );
    assert!(db.get_all_providers(app).unwrap().is_empty());
}

fn seed_available_variables(state: &AppState) {
    let config = ApiKeyRefreshConfig {
        enabled: true,
        script_dir: String::new(),
        timeout_secs: None,
        lead_time_secs: None,
    };
    for name in ["MY_KEY", "MY_USAGE_KEY", "MY_ACCESS_TOKEN"] {
        state
            .db
            .upsert_api_key_var(None, name, &config)
            .expect("create enabled variable");
        api_key_vars::commit_result(
            state,
            name,
            Ok(ScriptRun {
                output: RefreshOutput {
                    api_key: "test-variable-value".to_string(),
                    expires_at: None,
                    refresh_after: None,
                    message: None,
                },
                resolved_file: "index.mjs".to_string(),
            }),
        )
        .expect("seed variable value");
    }
}

#[test]
fn deeplink_rejects_api_key_variable_references_for_every_app() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    for app in [
        "claude",
        "codex",
        "gemini",
        "grokbuild",
        "opencode",
        "openclaw",
        "hermes",
        "pi",
    ] {
        let url = provider_link_with_key(app, "{{MY_KEY}}");
        assert_link_is_rejected_before_persistence(&url, app);
    }
}

#[test]
fn deeplink_rejects_usage_credential_variable_references_for_every_app() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    for app in [
        "claude",
        "codex",
        "gemini",
        "grokbuild",
        "opencode",
        "openclaw",
        "hermes",
        "pi",
    ] {
        for (field, reference) in [
            ("usageApiKey", "{{MY_USAGE_KEY}}"),
            ("usageAccessToken", "{{MY_ACCESS_TOKEN}}"),
        ] {
            let mut url = provider_link(app);
            url.query_pairs_mut().append_pair(field, reference);
            assert_link_is_rejected_before_persistence(&url, app);
        }
    }
}

#[test]
fn deeplink_rejects_merged_config_key_references_and_malformed_references() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let cases = [
        (
            "claude",
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"{{MY_KEY}}","ANTHROPIC_BASE_URL":"https://api.example.com/v1"}}"#,
        ),
        (
            "codex",
            r#"{"auth":{"OPENAI_API_KEY":"{{MY_KEY}}"},"config":"model_provider = \"example\"\n[model_providers.example]\nbase_url = \"https://api.example.com/v1\""}"#,
        ),
        (
            "gemini",
            r#"{"GEMINI_API_KEY":"{{MY_KEY}}","GOOGLE_GEMINI_BASE_URL":"https://api.example.com/v1"}"#,
        ),
        (
            "opencode",
            r#"{"apiKey":"{{MY_KEY}}","baseUrl":"https://api.example.com/v1"}"#,
        ),
    ];
    for (app, config) in cases {
        let encoded = base64::engine::general_purpose::STANDARD.encode(config);
        let mut url = provider_link(app);
        url.query_pairs_mut().clear().extend_pairs([
            ("resource", "provider"),
            ("app", app),
            ("name", "Deep Link Test"),
            ("homepage", "https://example.com"),
            ("endpoint", "https://api.example.com/v1"),
            ("model", "example-model"),
            ("config", &encoded),
            ("configFormat", "json"),
        ]);
        assert_link_is_rejected_before_persistence(&url, app);
    }

    // `apiKey` in the URL overrides the primary config key, but config-only credential fields
    // are preserved in the final Claude settings. The final key-field guard must catch those.
    let secondary_key_config = base64::engine::general_purpose::STANDARD.encode(
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"config-literal-key","ANTHROPIC_API_KEY":"{{MY_KEY}}"}}"#,
    );
    let mut secondary_key_url = provider_link("claude");
    secondary_key_url.query_pairs_mut().extend_pairs([
        ("config", secondary_key_config.as_str()),
        ("configFormat", "json"),
    ]);
    assert_link_is_rejected_before_persistence(&secondary_key_url, "claude");

    let malformed = provider_link_with_key("claude", "{{MY_KEY");
    assert_link_is_rejected_before_persistence(&malformed, "claude");
}

#[test]
fn deeplink_allows_literal_credentials_and_non_credential_variable_text() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    for app in [
        "claude",
        "codex",
        "gemini",
        "grokbuild",
        "opencode",
        "openclaw",
        "hermes",
        "pi",
    ] {
        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        let mut url = provider_link_with_key(app, "sk-literal-{not-a-variable}");
        url.query_pairs_mut().clear().extend_pairs([
            ("resource", "provider"),
            ("app", app),
            ("name", "Provider {{MY_KEY}}"),
            ("homepage", "https://example.com"),
            ("endpoint", "https://api.example.com/v1"),
            ("apiKey", "sk-literal-{not-a-variable}"),
            ("model", "model-{{MY_KEY}}"),
        ]);
        let request = parse_deeplink_url(url.as_str()).expect("parse deeplink url");
        import_provider_from_deeplink(&state, request)
            .expect("ordinary text must remain importable");
        assert_eq!(db.get_all_providers(app).unwrap().len(), 1);
    }
}

#[test]
fn deeplink_import_opencode_generates_editable_ids_and_preserves_display_names() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());
    let key_pattern = regex::Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").unwrap();
    let mut ids = std::collections::HashSet::new();

    for name in ["中文供应商", "公司 AI", "Test_Provider", "---", "🚀"] {
        // Repeated imports must still create distinct records, even in the same millisecond.
        for _ in 0..2 {
            let mut url = url::Url::parse("ccswitch://v1/import").unwrap();
            url.query_pairs_mut().extend_pairs([
                ("resource", "provider"),
                ("app", "opencode"),
                ("name", name),
                ("endpoint", "https://example.com/v1"),
                ("apiKey", "public-test-key"),
                ("model", "example-model"),
            ]);
            let request = parse_deeplink_url(url.as_str()).unwrap();
            let id = import_provider_from_deeplink(&state, request).unwrap();
            assert!(key_pattern.is_match(&id), "uneditable provider ID: {id}");
            assert!(ids.insert(id.clone()), "duplicate provider ID: {id}");
            let providers = db.get_all_providers("opencode").unwrap();
            let provider = providers.get(&id).unwrap();
            assert_eq!(provider.name, name);
            let live: serde_json::Value = serde_json::from_slice(
                &std::fs::read(_home.join(".config/opencode/opencode.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(live["provider"][&id], provider.settings_config);
        }
    }
    assert_eq!(db.get_all_providers("opencode").unwrap().len(), ids.len());
}

#[test]
fn deeplink_import_claude_provider_persists_to_db() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=DeepLink%20Claude&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-claude-key&model=claude-sonnet-4&icon=claude";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    // Verify DB state
    let providers = db.get_all_providers("claude").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("claude"));
    let auth_token = provider
        .settings_config
        .pointer("/env/ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str());
    let base_url = provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str());
    assert_eq!(auth_token, request.api_key.as_deref());
    assert_eq!(base_url, request.endpoint.as_deref());
}

#[test]
fn deeplink_import_codex_provider_builds_auth_and_config() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&icon=openai";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    let auth_value = provider
        .settings_config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(|v| v.as_str());
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(auth_value, request.api_key.as_deref());
    assert!(
        config_text.contains(request.endpoint.as_deref().unwrap()),
        "config.toml content should contain endpoint"
    );
    assert!(
        config_text.contains("model = \"gpt-4o\""),
        "config.toml content should contain model setting"
    );
}
