use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use cc_switch_lib::{bridges::prompt as prompt_bridge, AppType, MultiAppConfig, Prompt};

use super::support::{
    create_legacy_state_with_config, ensure_test_home, reset_test_fs, test_mutex,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PromptSnapshot {
    prompts: serde_json::Value,
    current_file: Option<String>,
    files: BTreeMap<String, String>,
}

fn prompt_fixture() -> Prompt {
    Prompt {
        id: "prompt-1".into(),
        name: "Prompt One".into(),
        content: "Always answer with a short status line.".into(),
        description: Some("baseline".into()),
        enabled: true,
        created_at: Some(1),
        updated_at: Some(1),
    }
}

#[test]
fn prompt_baseline_legacy_upsert_enabled_prompt_writes_live_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let home = ensure_test_home();

    let state = create_legacy_state_with_config(&MultiAppConfig::default());
    prompt_bridge::legacy_upsert_prompt(&state, AppType::Claude, "prompt-1", prompt_fixture())
        .expect("legacy prompt upsert");

    let prompts =
        prompt_bridge::legacy_get_prompts(&state, AppType::Claude).expect("legacy prompts");
    let current_file = prompt_bridge::legacy_get_current_prompt_file_content(AppType::Claude)
        .expect("current file");

    let mut files = BTreeMap::new();
    let prompt_path = home.join(".claude").join("CLAUDE.md");
    if let Ok(text) = std::fs::read_to_string(&prompt_path) {
        files.insert("claude/CLAUDE.md".into(), text);
    }

    let snapshot = PromptSnapshot {
        prompts: serde_json::to_value(prompts).expect("serialize prompts"),
        current_file,
        files,
    };

    assert!(snapshot
        .files
        .get("claude/CLAUDE.md")
        .is_some_and(|text| text.contains("Always answer")));
    assert_eq!(
        snapshot.current_file.as_deref(),
        Some("Always answer with a short status line.")
    );
}
