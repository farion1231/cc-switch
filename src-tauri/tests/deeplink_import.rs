use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{import_provider_from_deeplink, parse_deeplink_url, AppState, Database};
use url::form_urlencoded;

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
fn deeplink_import_codex_provider_preserves_model_catalog() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let config = r#"{
        "auth": { "OPENAI_API_KEY": "sk-test-codex-key" },
        "config": "model_provider = \"custom\"\nmodel = \"gpt-5.5\"\nmodel_reasoning_effort = \"high\"\ndisable_response_storage = true\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n\n[model_providers.custom]\nname = \"CliProxy\"\nbase_url = \"https://relay.example.com/group/cs_test/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true",
        "modelCatalog": {
            "models": [
                { "model": "gpt-5.5" },
                { "model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash", "contextWindow": 128000 },
                { "model": "deepseek-v4-pro" }
            ]
        }
    }"#;
    let config_b64 = BASE64_STANDARD.encode(config.as_bytes());
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("resource", "provider")
        .append_pair("app", "codex")
        .append_pair("name", "CliProxy Codex")
        .append_pair("enabled", "true")
        .append_pair("configFormat", "json")
        .append_pair("config", &config_b64)
        .finish();
    let url = format!("ccswitch://v1/import?{query}");
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id =
        import_provider_from_deeplink(&state, request).expect("import provider from deeplink");
    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    let catalog_models = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(|v| v.as_array())
        .expect("modelCatalog.models should be persisted");
    let models: Vec<&str> = catalog_models
        .iter()
        .filter_map(|entry| entry.get("model").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(
        models,
        vec!["gpt-5.5", "deepseek-v4-flash", "deepseek-v4-pro"]
    );
    assert_eq!(
        catalog_models[1]
            .get("displayName")
            .and_then(|v| v.as_str()),
        Some("DeepSeek V4 Flash")
    );
    assert_eq!(
        catalog_models[1]
            .get("contextWindow")
            .and_then(|v| v.as_u64()),
        Some(128000)
    );

    let live_config_path = home.join(".codex").join("config.toml");
    let live_config =
        std::fs::read_to_string(&live_config_path).expect("live Codex config should be written");
    assert!(
        live_config.contains("model_catalog_json = \"cc-switch-model-catalog.json\""),
        "live config should point to generated model catalog, got:\n{live_config}"
    );

    let catalog_path = home.join(".codex").join("cc-switch-model-catalog.json");
    let catalog_text =
        std::fs::read_to_string(&catalog_path).expect("model catalog should be written");
    assert!(
        catalog_text.contains("\"slug\":\"deepseek-v4-flash\"")
            || catalog_text.contains("\"slug\": \"deepseek-v4-flash\""),
        "generated catalog should include DeepSeek request model, got:\n{catalog_text}"
    );
}

#[test]
fn deeplink_import_codex_provider_normalizes_full_catalog_payload() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let config = r#"{
        "auth": { "OPENAI_API_KEY": "sk-test-codex-key" },
        "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example.com/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true",
        "modelCatalog": {
            "models": [
                { "slug": "deepseek-v4-flash", "display_name": "DeepSeek V4 Flash", "context_window": 128000 },
                { "slug": "deepseek-v4-pro", "display_name": "DeepSeek V4 Pro", "max_context_window": 128000 }
            ]
        }
    }"#;
    let config_b64 = BASE64_STANDARD.encode(config.as_bytes());
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("resource", "provider")
        .append_pair("app", "codex")
        .append_pair("name", "CliProxy Codex")
        .append_pair("configFormat", "json")
        .append_pair("config", &config_b64)
        .finish();
    let url = format!("ccswitch://v1/import?{query}");
    let request = parse_deeplink_url(&url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id =
        import_provider_from_deeplink(&state, request).expect("import provider from deeplink");
    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    let catalog_models = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(|v| v.as_array())
        .expect("modelCatalog.models should be persisted");
    assert_eq!(
        catalog_models[0].get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        catalog_models[0]
            .get("displayName")
            .and_then(|v| v.as_str()),
        Some("DeepSeek V4 Flash")
    );
    assert_eq!(
        catalog_models[1].get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-pro")
    );
}
