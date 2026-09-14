use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{import_provider_from_deeplink, parse_deeplink_url, AppState, Database};
use url::Url;

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
fn deeplink_import_grokbuild_provider_preserves_multiple_models() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let source_config = concat!(
        "[models]\ndefault = \"grok-4.6\"\n\n",
        "[model.\"grok-4.6\"]\nmodel = \"grok-4.6\"\n",
        "base_url = \"https://ignored.example/v1\"\nname = \"Grok 4.6\"\n",
        "description = \"Grok 4.6\"\napi_backend = \"responses\"\n",
        "context_window = 500000\n\n",
        "[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\n",
        "base_url = \"https://ignored.example/v1\"\nname = \"Grok 4.5\"\n",
        "description = \"Grok 4.5\"\napi_backend = \"responses\"\n",
        "context_window = 500000\n",
    );
    let encoded_config = BASE64_STANDARD.encode(
        serde_json::json!({ "config": source_config })
            .to_string()
            .as_bytes(),
    );
    let mut url = Url::parse("ccswitch://v1/import").expect("base deeplink URL");
    url.query_pairs_mut()
        .append_pair("resource", "provider")
        .append_pair("app", "grokbuild")
        .append_pair("name", "Gateway Provider Name")
        .append_pair("homepage", "http://127.0.0.1:18765")
        .append_pair("endpoint", "http://127.0.0.1:18765/v1")
        .append_pair("apiKey", "sk-test-grok-key")
        .append_pair("model", "grok-4.6")
        .append_pair("configFormat", "json")
        .append_pair("config", &encoded_config);
    let request = parse_deeplink_url(url.as_str()).expect("parse Grok Build deeplink");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());
    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("import Grok Build provider from deeplink");

    let providers = db.get_all_providers("grokbuild").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("Grok Build provider created via deeplink");
    assert_eq!(provider.name, "Gateway Provider Name");

    let stored = provider
        .settings_config
        .get("config")
        .and_then(|value| value.as_str())
        .expect("stored Grok Build config");
    let parsed = stored.parse::<toml::Value>().expect("stored TOML");
    let models = parsed["model"].as_table().expect("stored models");
    assert_eq!(parsed["models"]["default"].as_str(), Some("grok-4.6"));
    assert_eq!(models.len(), 2);
    for (id, name) in [("grok-4.6", "Grok 4.6"), ("grok-4.5", "Grok 4.5")] {
        let model = models[id].as_table().expect("stored model profile");
        assert_eq!(model["model"].as_str(), Some(id));
        assert_eq!(model["name"].as_str(), Some(name));
        assert_eq!(model["description"].as_str(), Some(name));
        assert_eq!(
            model["base_url"].as_str(),
            Some("http://127.0.0.1:18765/v1")
        );
        assert_eq!(model["api_key"].as_str(), Some("sk-test-grok-key"));
        assert_eq!(model["api_backend"].as_str(), Some("responses"));
    }
    assert!(!stored.contains("Gateway Provider Name"));
    assert!(!stored.contains("ignored.example"));

    let live = std::fs::read_to_string(home.join(".grok/config.toml"))
        .expect("Grok Build live config written");
    assert_eq!(live, stored);
}
