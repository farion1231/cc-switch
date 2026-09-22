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
fn deeplink_import_claude_desktop_provider_persists_to_correct_app() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let config = BASE64_URL_SAFE_NO_PAD.encode(
        serde_json::json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-config",
            "ANTHROPIC_BASE_URL": "https://api.example.com",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-test"
        }})
        .to_string(),
    );

    for app in ["claude-desktop", "claude_desktop", "claudedesktop"] {
        for params in [
            "endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-url&sonnetModel=sonnet-test"
                .to_string(),
            format!("config={config}&apiKey=sk-url"),
        ] {
            let url = format!(
                "ccswitch://v1/import?resource=provider&app={app}&name=Desktop&enabled=false&{params}"
            );
            let request = parse_deeplink_url(&url).expect("parse desktop deeplink");
            assert_eq!(request.app.as_deref(), Some("claude-desktop"));

            let db = Arc::new(Database::memory().expect("create memory db"));
            let state = AppState::new(db.clone());
            let id =
                import_provider_from_deeplink(&state, request).expect("import desktop provider");
            let providers = db.get_all_providers("claude-desktop").unwrap();
            let provider = providers.get(&id).expect("desktop provider persisted");
            assert_eq!(
                provider.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
                "sk-url"
            );
            assert_eq!(
                provider.settings_config["env"]["ANTHROPIC_BASE_URL"],
                "https://api.example.com"
            );
            assert_eq!(
                provider.settings_config["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
                "sonnet-test"
            );
            assert!(db.get_all_providers("claude").unwrap().is_empty());
        }
    }
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
