use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{
    import_provider_from_deeplink, parse_deeplink_url, update_settings, AppSettings, AppState,
    Database, DeepLinkImportRequest,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn configure_deepseek_harness_test_home(home: &std::path::Path) {
    update_settings(AppSettings {
        deepseek_harness_config_dir: Some(home.join(".dsh").to_string_lossy().into_owned()),
        ..Default::default()
    })
    .expect("configure isolated DeepSeek Harness home");
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
fn deeplink_import_deepseek_harness_allows_native_connection_defaults() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    configure_deepseek_harness_test_home(home);

    let url = "ccswitch://v1/import?resource=provider&app=dsh&name=DeepSeek";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request)
        .expect("import credential-free DeepSeek Harness provider");

    let providers = db
        .get_all_providers("deepseek-harness")
        .expect("get DeepSeek Harness providers");
    let provider = providers
        .get(&provider_id)
        .expect("DeepSeek Harness provider created via deeplink");

    assert_eq!(provider.name, "DeepSeek");
    assert_eq!(
        provider.settings_config["defaultModel"],
        "deepseek-v4-flash"
    );
    assert!(provider.settings_config.get("apiKey").is_none());
    assert!(provider.settings_config.get("baseURL").is_none());
    assert!(provider.website_url.is_none());

    let settings = std::fs::read_to_string(home.join(".dsh/settings.yaml"))
        .expect("read projected Harness settings");
    assert!(settings.contains("provider: deepseek-official"));
    assert!(settings.contains("model: deepseek-v4-flash"));
    assert!(!home.join(".dsh/.credentials.yaml").exists());
}

#[test]
fn deeplink_import_deepseek_harness_rejects_implicit_key_for_custom_endpoint() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    configure_deepseek_harness_test_home(home);

    let url = "ccswitch://v1/import?resource=provider&app=dsh&name=Unsafe&endpoint=https%3A%2F%2Fattacker.example";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let error = import_provider_from_deeplink(&state, request)
        .expect_err("custom endpoint must not reuse an implicit Harness credential");

    assert!(error.to_string().contains("API key is required"));
    assert!(db
        .get_all_providers("deepseek-harness")
        .expect("get DeepSeek Harness providers")
        .is_empty());
    assert!(!home.join(".dsh/settings.yaml").exists());
}

#[test]
fn deeplink_import_deepseek_harness_checks_endpoint_after_inline_config_merge() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    configure_deepseek_harness_test_home(home);

    let config = serde_json::json!({ "baseURL": "https://attacker.example" }).to_string();
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("deepseek-harness".to_string()),
        name: Some("Unsafe Config".to_string()),
        config: Some(BASE64_STANDARD.encode(config)),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let error = import_provider_from_deeplink(&state, request)
        .expect_err("merged custom endpoint must require an explicit credential");

    assert!(error.to_string().contains("API key is required"));
    assert!(db
        .get_all_providers("deepseek-harness")
        .expect("get DeepSeek Harness providers")
        .is_empty());
    assert!(!home.join(".dsh/settings.yaml").exists());
}
