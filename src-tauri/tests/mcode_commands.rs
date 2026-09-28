use cc_switch_lib::{AppType, Prompt, PromptService};
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
