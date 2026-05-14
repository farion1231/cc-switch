use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{import_provider_from_deeplink, parse_deeplink_url, AppState, Database};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn deeplink_import_claude_provider_persists_to_db() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
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
    let _guard = test_mutex().lock().expect("acquire test mutex");
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

#[test]
fn deeplink_import_gemini_provider_persists_extra_env_to_db() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let extra_env_b64 = BASE64_STANDARD
        .encode(r#"{"GEMINI_MODEL":"gemini-2.5-pro","TRACE_SAMPLE_RATE":0.5,"feature.flag":"1"}"#);
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=gemini&name=DeepLink%20Gemini&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fgenerativelanguage.googleapis.com%2Fv1beta&apiKey=gemini-test-key&extraEnv={extra_env_b64}"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("import gemini provider from deeplink");

    let providers = db.get_all_providers("gemini").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("gemini provider created via deeplink");

    assert_eq!(provider.name, "DeepLink Gemini");
    let env = provider
        .settings_config
        .get("env")
        .and_then(|v| v.as_object())
        .expect("gemini env object");
    assert_eq!(
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()),
        Some("gemini-test-key")
    );
    assert_eq!(
        env.get("GOOGLE_GEMINI_BASE_URL").and_then(|v| v.as_str()),
        Some("https://generativelanguage.googleapis.com/v1beta"),
    );
    assert_eq!(
        env.get("GEMINI_MODEL").and_then(|v| v.as_str()),
        Some("gemini-2.5-pro")
    );
    assert_eq!(
        env.get("TRACE_SAMPLE_RATE").and_then(|v| v.as_str()),
        Some("0.5")
    );
    assert!(env.get("feature.flag").is_none());
}

#[test]
fn deeplink_import_enabled_gemini_switches_and_writes_live_env() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let extra_env_b64 =
        BASE64_STANDARD.encode(r#"{"GEMINI_MODEL":"gemini-2.5-pro","TRACE_SAMPLE_RATE":0.5}"#);
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=gemini&name=Enabled%20Gemini&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fgenerativelanguage.googleapis.com%2Fv1beta&apiKey=gemini-live-key&enabled=true&extraEnv={extra_env_b64}"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("import enabled gemini provider from deeplink");

    assert_eq!(
        db.get_current_provider("gemini")
            .expect("get current provider"),
        Some(provider_id),
    );

    let env_path = home.join(".gemini").join(".env");
    let env_content = std::fs::read_to_string(&env_path).expect("read gemini .env");
    assert!(env_content.contains("GEMINI_API_KEY=gemini-live-key"));
    assert!(env_content.contains("GEMINI_MODEL=gemini-2.5-pro"));
    assert!(env_content.contains("TRACE_SAMPLE_RATE=0.5"));

    let settings_path = home.join(".gemini").join("settings.json");
    let settings_raw = std::fs::read_to_string(&settings_path).expect("read gemini settings");
    let settings_value: serde_json::Value =
        serde_json::from_str(&settings_raw).expect("parse gemini settings");
    assert_eq!(
        settings_value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
    );
}

#[test]
fn deeplink_import_rejects_extra_env_for_unsupported_provider_apps() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let extra_env_b64 = BASE64_STANDARD.encode(r#"{"OPENAI_API_KEY":"override-key"}"#);
    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);
    let cases = [
        ("codex", "DeepLink%20Codex", "sk-test-codex-key"),
        ("opencode", "DeepLink%20OpenCode", "sk-test-opencode-key"),
        ("openclaw", "DeepLink%20OpenClaw", "sk-test-openclaw-key"),
        ("hermes", "DeepLink%20Hermes", "sk-test-hermes-key"),
    ];

    for (app, name, api_key) in cases {
        let url = format!(
            "ccswitch://v1/import?resource=provider&app={app}&name={name}&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey={api_key}&extraEnv={extra_env_b64}"
        );
        let request = parse_deeplink_url(&url).expect("parse deeplink url");

        let err = import_provider_from_deeplink(&state, request)
            .expect_err("unsupported provider should reject extraEnv");
        assert!(err
            .to_string()
            .contains("extraEnv is currently only supported for Claude, ClaudeDesktop and Gemini"));
    }
}
