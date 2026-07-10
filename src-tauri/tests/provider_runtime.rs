use serde_json::json;

use cc_switch_lib::{
    AppType, PiApiKeyDraft, PiApiKeyMode, PiModelDraft, PiProviderDraft, PiProviderMode,
    PiProviderTemplate, ProviderRuntimeApp, ProviderRuntimeProviders, ProviderRuntimeService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn sample_pi_draft() -> PiProviderDraft {
    PiProviderDraft {
        mode: PiProviderMode::Custom,
        provider_id: "longcat".to_string(),
        template: PiProviderTemplate::OpenAiCompatible,
        base_url: Some("https://api.example.com/v1".to_string()),
        api: "openai-compatible".to_string(),
        api_key: PiApiKeyDraft {
            mode: PiApiKeyMode::Literal,
            value: "sk-test".to_string(),
        },
        headers: Vec::new(),
        models: vec![PiModelDraft {
            id: "LongCat-2.0".to_string(),
            name: None,
            name_touched: false,
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: None,
        }],
        compat: None,
        advanced_json: None,
    }
}

#[test]
fn db_backed_runtime_current_returns_empty_for_opencode_family() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let state = create_test_state().expect("create test state");

    for app in [AppType::OpenCode, AppType::OpenClaw, AppType::Hermes] {
        let current = ProviderRuntimeService::current(Some(&state), app.into())
            .expect("query current provider");

        assert_eq!(current, "");
    }
}

#[test]
fn pi_runtime_lists_providers_from_models_json() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = home.join(".pi").join("agent").join("models.json");
    std::fs::create_dir_all(path.parent().expect("pi dir")).expect("create pi dir");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "providers": {
                "longcat": {
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-compatible"
                }
            }
        }))
        .expect("serialize models json"),
    )
    .expect("write models json");

    let providers = match ProviderRuntimeService::list(None, ProviderRuntimeApp::Pi)
        .expect("list pi providers")
    {
        ProviderRuntimeProviders::Pi(providers) => providers,
        ProviderRuntimeProviders::Db(_) => panic!("Pi runtime must not return DB providers"),
    };

    assert!(providers.contains_key("longcat"));
}

#[test]
fn pi_runtime_apply_respects_expected_file_hash() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = home.join(".pi").join("agent").join("models.json");
    std::fs::create_dir_all(path.parent().expect("pi dir")).expect("create pi dir");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({ "providers": {} }))
            .expect("serialize empty models json"),
    )
    .expect("write models json");

    let draft = sample_pi_draft();
    let preview =
        ProviderRuntimeService::preview_pi_provider_patch(&draft).expect("build pi preview");

    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "providers": {
                "other": {
                    "baseUrl": "https://elsewhere.example/v1",
                    "api": "openai-compatible"
                }
            }
        }))
        .expect("serialize changed models json"),
    )
    .expect("overwrite models json");

    let err = ProviderRuntimeService::apply_pi_provider_patch(&draft, &preview.current_file_hash)
        .expect_err("apply should fail after file hash changes");

    let msg = err.to_string();
    assert!(
        msg.contains("expected hash"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn pi_runtime_delete_updates_only_models_json_providers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let path = home.join(".pi").join("agent").join("models.json");
    std::fs::create_dir_all(path.parent().expect("pi dir")).expect("create pi dir");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "unknownRoot": { "preserve": true },
            "providers": {
                "remove-me": { "api": "openai-compatible" },
                "keep-me": { "api": "anthropic-messages" }
            }
        }))
        .expect("serialize models json"),
    )
    .expect("write models json");

    let file_hash = ProviderRuntimeService::read_pi_models_meta().expect("read Pi file hash");
    let result = ProviderRuntimeService::delete_pi_provider("remove-me", &file_hash)
        .expect("delete Pi provider");

    assert!(result.models_json["providers"].get("remove-me").is_none());
    assert!(result.models_json["providers"].get("keep-me").is_some());
    assert_eq!(result.models_json["unknownRoot"]["preserve"], true);
    assert!(!result.backup_path.is_empty());
}

#[test]
fn provider_runtime_app_parser_accepts_pi() {
    let parsed: ProviderRuntimeApp = "pi".parse().expect("parse pi runtime app");
    assert_eq!(parsed, ProviderRuntimeApp::Pi);
}

#[test]
fn pi_runtime_current_is_empty_without_database_state() {
    let current = ProviderRuntimeService::current(None, ProviderRuntimeApp::Pi)
        .expect("Pi has no current provider");

    assert_eq!(current, "");
}
