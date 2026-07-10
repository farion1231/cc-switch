use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cc_switch_lib::{import_provider_from_deeplink, parse_deeplink_url, AppState, Database};
use serde_json::json;

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
fn deeplink_import_pi_provider_preserves_pi_specific_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let inline_config = json!({
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-test-pi-key",
        "api": "anthropic-messages",
        "models": [{
            "id": "claude-sonnet-4",
            "name": "Claude Sonnet 4",
            "contextWindow": 200000
        }],
        "defaultModel": "claude-sonnet-4",
        "customHeader": "preserve-me"
    });
    let encoded = URL_SAFE_NO_PAD.encode(inline_config.to_string());
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=pi&name=DeepLink%20Pi&config={encoded}&configFormat=json"
    );
    let request = parse_deeplink_url(&url).expect("parse Pi deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());
    let provider_id =
        import_provider_from_deeplink(&state, request).expect("import Pi provider from deeplink");

    let providers = db.get_all_providers("pi").expect("get Pi providers");
    let provider = providers
        .get(&provider_id)
        .expect("Pi provider created via deeplink");

    assert_eq!(provider.settings_config["api"], "anthropic-messages");
    assert_eq!(
        provider.settings_config["models"][0]["name"],
        "Claude Sonnet 4"
    );
    assert_eq!(
        provider.settings_config["models"][0]["contextWindow"],
        200000
    );
    assert_eq!(provider.settings_config["defaultModel"], "claude-sonnet-4");
    assert_eq!(provider.settings_config["customHeader"], "preserve-me");
}
