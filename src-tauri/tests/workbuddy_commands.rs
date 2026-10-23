use cc_switch_lib::{AppType, Prompt, PromptService, Provider, ProviderService};
use serde_json::{json, Value};
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn provider(id: &str, model: &str) -> Provider {
    Provider::with_id(
        id.into(),
        "DeepSeek".into(),
        json!({
            "vendor": "DeepSeek",
            "url": "https://api.deepseek.com/v1/chat/completions",
            "apiKey": "test-key",
            "models": [{ "id": model, "name": "Chat", "supportsToolCall": true }]
        }),
        None,
    )
}

fn read_models() -> Value {
    let path = ensure_test_home().join(".workbuddy/models.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn workbuddy_providers_are_written_to_models_json_and_removed_cleanly() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let dir = ensure_test_home().join(".workbuddy");
    fs::create_dir_all(&dir).unwrap();
    // WorkBuddy's own entries (no CC Switch marker) and the allow-list must survive.
    fs::write(
        dir.join("models.json"),
        r#"{"models":[{"id":"mine","url":"https://x.test/v1/chat/completions"}],"availableModels":["mine"]}"#,
    )
    .unwrap();

    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-chat"),
        true,
    )
    .unwrap();
    let doc = read_models();
    let models = doc["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "mine");
    assert_eq!(models[1]["id"], "deepseek-chat");
    assert_eq!(
        models[1]["url"],
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(models[1]["apiKey"], "test-key");
    assert_eq!(models[1]["ccSwitchProviderId"], "ds");
    assert_eq!(doc["availableModels"], json!(["mine", "deepseek-chat"]));
    let saved = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.meta.and_then(|meta| meta.live_config_managed),
        Some(true)
    );

    // Editing rewrites the provider's entries in place.
    let mut edited = provider("ds", "deepseek-reasoner");
    edited.meta = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap()
        .meta;
    ProviderService::update(&state, AppType::WorkBuddy, None, edited).unwrap();
    let doc = read_models();
    let ids: Vec<_> = doc["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["mine", "deepseek-reasoner"]);
    assert_eq!(doc["availableModels"], json!(["mine", "deepseek-reasoner"]));

    ProviderService::remove_from_live_config(&state, AppType::WorkBuddy, "ds").unwrap();
    let doc = read_models();
    assert_eq!(doc["models"].as_array().unwrap().len(), 1);
    assert_eq!(doc["availableModels"], json!(["mine"]));
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_some());

    ProviderService::switch(&state, AppType::WorkBuddy, "ds").unwrap();
    assert_eq!(read_models()["models"].as_array().unwrap().len(), 2);
    ProviderService::delete(&state, AppType::WorkBuddy, "ds").unwrap();
    assert_eq!(
        read_models()["models"],
        json!([{"id":"mine","url":"https://x.test/v1/chat/completions"}])
    );
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_none());
}

#[test]
fn workbuddy_refuses_to_take_over_a_model_id_it_does_not_own() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".workbuddy/models.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let native = r#"{"models":[{"id":"deepseek-chat","apiKey":"user-key"}]}"#;
    fs::write(&path, native).unwrap();

    assert!(ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-chat"),
        true
    )
    .is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), native);
    assert!(state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .is_none());

    // An edit that would collide is refused without changing the saved row either.
    ProviderService::add(
        &state,
        AppType::WorkBuddy,
        provider("ds", "deepseek-v4"),
        true,
    )
    .unwrap();
    let mut colliding = provider("ds", "deepseek-chat");
    colliding.meta = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap()
        .meta;
    assert!(ProviderService::update(&state, AppType::WorkBuddy, None, colliding).is_err());
    let saved = state
        .db
        .get_provider_by_id("ds", "workbuddy")
        .unwrap()
        .unwrap();
    assert_eq!(saved.settings_config["models"][0]["id"], "deepseek-v4");
}

#[test]
fn workbuddy_prompts_project_to_codebuddy_md() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let prompt: Prompt = serde_json::from_value(json!({
        "id": "p", "name": "Rules", "content": "be brief", "enabled": true
    }))
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::WorkBuddy, "p", prompt).unwrap();
    let path = ensure_test_home().join(".workbuddy/CODEBUDDY.md");
    assert_eq!(fs::read_to_string(path).unwrap(), "be brief");
}
