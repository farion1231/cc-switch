use std::sync::Arc;

use cc_switch_lib::{
    import_provider_from_deeplink, parse_deeplink_url, AppState, AppType, Database, Provider,
    ProviderService,
};
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
fn deeplink_import_codex_provider_builds_auth_config_and_catalog() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-5.6-sol&icon=openai&enabled=true";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let existing_provider = Provider::with_id(
        "existing-codex".to_string(),
        "Existing Codex".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "sk-existing-codex-key"},
            "config": r#"model_provider = "custom"
model = "gpt-5.5"

[model_providers.custom]
name = "Existing Codex"
base_url = "https://existing.openai.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );
    ProviderService::add(&state, AppType::Codex, existing_provider, true)
        .expect("add existing Codex provider");
    ProviderService::switch(&state, AppType::Codex, "existing-codex")
        .expect("activate existing Codex provider");
    assert_eq!(
        db.get_current_provider(AppType::Codex.as_str())
            .expect("read initial current Codex provider")
            .as_deref(),
        Some("existing-codex")
    );

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    assert_eq!(
        db.get_current_provider(AppType::Codex.as_str())
            .expect("read current Codex provider after deeplink import")
            .as_deref(),
        Some(provider_id.as_str()),
        "enabled=true should switch from the existing provider to the imported provider"
    );

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    assert!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.api_format.as_deref())
            .is_none(),
        "deeplink model catalog generation must not change api_format"
    );
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
        config_text.contains("model = \"gpt-5.6-sol\""),
        "config.toml content should contain model setting"
    );

    assert_eq!(
        provider
            .settings_config
            .pointer("/modelCatalog/models/0/model")
            .and_then(|value| value.as_str()),
        Some("gpt-5.6-sol"),
        "the explicit deeplink model should be persisted as a catalog entry"
    );

    let live_config = std::fs::read_to_string(ensure_test_home().join(".codex/config.toml"))
        .expect("read generated Codex config");
    assert!(
        live_config.contains("model_catalog_json = \"cc-switch-model-catalog.json\""),
        "switching to the imported provider should point Codex at the generated catalog"
    );
    assert!(
        live_config.contains("model = \"gpt-5.6-sol\""),
        "live config should come from the imported provider"
    );

    let catalog_text = std::fs::read_to_string(
        ensure_test_home().join(".codex/cc-switch-model-catalog.json"),
    )
    .expect("read generated Codex model catalog");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog_text).expect("parse generated Codex model catalog");
    let generated_models = catalog["models"]
        .as_array()
        .expect("generated catalog models array");
    assert_eq!(generated_models.len(), 1);
    let generated_model = &generated_models[0];
    assert_eq!(generated_model["slug"], "gpt-5.6-sol");
    assert_eq!(generated_model["display_name"], "gpt-5.6-sol");
}
