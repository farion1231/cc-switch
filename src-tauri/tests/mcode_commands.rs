use cc_switch_lib::{AppType, Prompt, PromptService, Provider, ProviderService};
use serde_json::json;
use std::fs;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn failed_mcode_prompt_writes_preserve_state_and_can_be_retried() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".minimax/AGENTS.md");
    let active: Prompt = serde_json::from_value(json!({
        "id":"active", "name":"Active", "content":"original", "enabled":true
    }))
    .unwrap();
    state.db.save_prompt("mcode", &active).unwrap();
    // A directory at the file path deterministically rejects the atomic rename.
    fs::create_dir_all(&path).unwrap();
    let mut edited = active.clone();
    edited.content = "updated".into();
    assert!(
        PromptService::upsert_prompt(&state, AppType::Mcode, &active.id, edited.clone()).is_err()
    );
    let mut disabled = active.clone();
    disabled.enabled = false;
    assert!(
        PromptService::upsert_prompt(&state, AppType::Mcode, &active.id, disabled.clone()).is_err()
    );
    let stored = &state.db.get_prompts("mcode").unwrap()[&active.id];
    assert!(stored.enabled);
    assert_eq!(stored.content, "original");
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "original").unwrap();
    PromptService::upsert_prompt(&state, AppType::Mcode, &active.id, disabled).unwrap();
    assert!(!state.db.get_prompts("mcode").unwrap()[&active.id].enabled);
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
    PromptService::upsert_prompt(&state, AppType::Mcode, &active.id, edited).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "updated");
    assert!(state.db.get_prompts("mcode").unwrap()[&active.id].enabled);
}

#[test]
fn adding_an_mcode_provider_rejects_a_key_already_in_use() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let path = ensure_test_home().join(".minimax/config.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let native = "custom_provider:\n  handwritten:\n    name: Mine\n    models: {}\n  minimax:\n    kind: minimax\n    name: Account\n";
    fs::write(&path, native).unwrap();
    let provider = |id: &str| {
        Provider::with_id(
            id.into(),
            "Test".into(),
            json!({"name":"Test", "kind":"custom", "enabled":true, "api":"openai-completions",
                "options":{"baseURL":"https://api.example.com/v1","apiKey":"test-key"},
                "models":{"model":{"name":"Model"}}}),
            None,
        )
    };

    // Live nodes, custom or owned by MCode, are never overwritten.
    for id in ["handwritten", "minimax"] {
        assert!(ProviderService::add(&state, AppType::Mcode, provider(id), true).is_err());
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), native);
    assert!(state.db.get_all_providers("mcode").unwrap().is_empty());

    // Neither is a row that only lives in the database.
    ProviderService::add(&state, AppType::Mcode, provider("fresh"), false).unwrap();
    let mut renamed = provider("fresh");
    renamed.name = "Other".into();
    assert!(ProviderService::add(&state, AppType::Mcode, renamed, true).is_err());
    assert_eq!(
        state
            .db
            .get_provider_by_id("fresh", "mcode")
            .unwrap()
            .unwrap()
            .name,
        "Test"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), native);
}

#[test]
fn concurrent_mcode_adds_with_one_key_leave_exactly_one_winner() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let state = create_test_state().unwrap();
    let successes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|index| {
                let state = &state;
                scope.spawn(move || {
                    let provider = Provider::with_id(
                        "same".into(),
                        format!("Racer {index}"),
                        json!({"name":"Racer", "kind":"custom", "enabled":true,
                            "api":"openai-completions",
                            "options":{"baseURL":"https://api.example.com/v1","apiKey":"test-key"},
                            "models":{"model":{"name":"Model"}}}),
                        None,
                    );
                    ProviderService::add(state, AppType::Mcode, provider, false)
                        .is_ok()
                        .then_some(index)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(successes.len(), 1);
    assert_eq!(
        state
            .db
            .get_provider_by_id("same", "mcode")
            .unwrap()
            .unwrap()
            .name,
        format!("Racer {}", successes[0])
    );
}
