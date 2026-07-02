use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{
    get_codex_auth_path, import_provider_from_deeplink, parse_deeplink_url, read_json_file,
    AppState, Database, Provider,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn assert_error_contains(err: impl std::fmt::Display, zh: &str, en: &str) {
    let msg = err.to_string();
    assert!(msg.contains(zh), "error should contain '{zh}', got: {msg}");
    assert!(msg.contains(en), "error should contain '{en}', got: {msg}");
}

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
fn deeplink_import_codex_openai_official_without_key_or_endpoint() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let config = BASE64_STANDARD.encode(r#"{"auth":{},"config":""}"#);
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=codex&name=OpenAI%20Official&homepage=https%3A%2F%2Fexample.com&configFormat=json&config={config}&icon=anthropic"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import OpenAI Official provider from deeplink");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("official provider created via deeplink");

    assert_eq!(provider.name, "OpenAI Official");
    assert_eq!(provider.category.as_deref(), Some("official"));
    assert_eq!(
        provider.website_url.as_deref(),
        Some("https://chatgpt.com/codex")
    );
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    assert_eq!(provider.icon_color.as_deref(), Some("#00A67E"));
    assert_eq!(
        provider
            .settings_config
            .get("auth")
            .and_then(|value| value.as_object())
            .map(|auth| auth.is_empty()),
        Some(true)
    );
    assert_eq!(
        provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str()),
        Some("")
    );
}

#[test]
fn deeplink_import_codex_openai_official_accepts_auth_material_and_writes_auth() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let expected_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": "sk-official-auth-json",
        "tokens": {
            "access_token": "official-token",
            "id_token": "official-id-token"
        }
    });
    let config_payload = json!({
        "auth": expected_auth.clone(),
        "config": ""
    });
    let config = BASE64_STANDARD.encode(config_payload.to_string());
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=codex&name=OpenAI%20Official&configFormat=json&config={config}"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());
    let existing_provider = Provider::with_id(
        "existing-codex".to_string(),
        "Existing Codex".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "existing-key" },
            "config": ""
        }),
        Some("https://example.com".to_string()),
    );
    db.save_provider("codex", &existing_provider)
        .expect("seed existing codex provider");
    db.set_current_provider("codex", "existing-codex")
        .expect("seed current codex provider");

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("official deeplink should import embedded auth material");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("official provider created via deeplink");
    assert_eq!(provider.category.as_deref(), Some("official"));
    assert_eq!(provider.settings_config.get("auth"), Some(&expected_auth));

    let stored_auth: serde_json::Value =
        read_json_file(&get_codex_auth_path()).expect("read auth.json");
    assert_eq!(stored_auth, expected_auth);
}

#[test]
fn deeplink_import_codex_openai_official_preserves_config_toml() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let config_toml = r#"model = "gpt-5-codex"
approval_policy = "on-request"
"#;
    let config_payload = json!({
        "auth": {},
        "config": config_toml
    });
    let config = BASE64_STANDARD.encode(config_payload.to_string());
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=codex&name=OpenAI%20Official&configFormat=json&config={config}"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("official deeplink should preserve embedded config.toml");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("official provider created via deeplink");

    assert_eq!(provider.category.as_deref(), Some("official"));
    assert_eq!(
        provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str()),
        Some(config_toml)
    );
}

#[test]
fn deeplink_import_codex_openai_official_rejects_model_override() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let config = BASE64_STANDARD.encode(r#"{"auth":{},"config":""}"#);
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=codex&name=OpenAI%20Official&configFormat=json&config={config}&model=gpt-5-codex"
    );
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db);

    let err = import_provider_from_deeplink(&state, request)
        .expect_err("official deeplink must reject model overrides");

    assert_error_contains(err, "模型覆盖", "model override");
}

#[test]
fn deeplink_import_codex_openai_official_name_with_key_and_endpoint_stays_custom() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=OpenAI%20Official&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&icon=openai";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id =
        import_provider_from_deeplink(&state, request).expect("import custom provider");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, "OpenAI Official");
    assert_eq!(provider.category.as_deref(), None);
    assert!(
        provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
            .is_some_and(|config| config.contains("https://api.openai.example/v1")),
        "custom provider must keep the supplied endpoint"
    );
}
