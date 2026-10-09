use std::sync::Arc;

use cc_switch_lib::{import_provider_from_deeplink, parse_deeplink_url, AppState, Database};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

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

#[test]
fn deeplink_import_codex_remote_catalog_survives_db_and_live_switch() {
    use base64::prelude::*;

    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let config = serde_json::json!({
        "config": "model_provider = \"relay\"\nmodel = \"custom-model\"\n\n[model_providers.relay]\nbase_url = \"https://api.example.com/v1\"\nmodel_catalog_url = \"https://api.example.com/v1/models?format=codex\"\n\n[model_providers.aaa_inactive]\nbase_url = \"https://inactive.example.com/v1\"\n"
    });
    let mut url = url::Url::parse("ccswitch://v1/import").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("resource", "provider"),
        ("app", "codex"),
        ("name", "Catalog Relay"),
        ("apiKey", "sk-test-catalog"),
        ("enabled", "true"),
        ("configFormat", "json"),
        (
            "config",
            BASE64_STANDARD.encode(config.to_string()).as_str(),
        ),
    ]);
    let request = parse_deeplink_url(url.as_str()).expect("parse catalog deeplink");
    let db = Arc::new(Database::memory().expect("memory database"));
    let state = AppState::new(db.clone());
    let id =
        import_provider_from_deeplink(&state, request).expect("import and enable catalog provider");
    let providers = db
        .get_all_providers("codex")
        .expect("read imported providers");
    let stored = providers[&id].settings_config["config"].as_str().unwrap();
    let live = std::fs::read_to_string(home.join(".codex/config.toml")).expect("live Codex config");
    for text in [stored, live.as_str()] {
        let config: toml::Value = toml::from_str(text).expect("valid persisted TOML");
        assert_eq!(
            config["model_providers"]["custom"]["model_catalog_url"].as_str(),
            Some("https://api.example.com/v1/models?format=codex")
        );
        assert_eq!(
            config["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(config["model"].as_str(), Some("custom-model"));
    }
    assert_eq!(
        providers[&id].settings_config["auth"]["OPENAI_API_KEY"],
        "sk-test-catalog"
    );
}
