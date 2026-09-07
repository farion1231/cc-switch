mod support;

use cc_switch_lib::{AppType, Prompt, PromptService};
use std::fs;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn prompt_list_refreshes_external_edits_without_changing_inactive_templates() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().unwrap();

    for (app, relative_path) in [
        (AppType::Claude, ".claude/CLAUDE.md"),
        (AppType::Codex, ".codex/AGENTS.md"),
    ] {
        let active = Prompt {
            id: "active".into(),
            name: "My instructions".into(),
            content: "original".into(),
            description: Some("Keep my metadata".into()),
            enabled: true,
            created_at: Some(1),
            updated_at: Some(1),
        };
        let inactive = Prompt {
            id: "inactive".into(),
            enabled: false,
            ..active.clone()
        };
        state.db.save_prompt(app.as_str(), &inactive).unwrap();
        PromptService::upsert_prompt(&state, app.clone(), &active.id, active.clone()).unwrap();
        let path = home.join(relative_path);

        for content in ["externally edited\n", ""] {
            fs::write(&path, content).unwrap();
            let prompts = PromptService::get_prompts(&state, app.clone()).unwrap();
            assert_eq!(prompts["active"].content, content);
            assert_eq!(prompts["active"].name, active.name);
            assert_eq!(prompts["active"].description, active.description);
            assert_eq!(prompts["active"].created_at, active.created_at);
            assert!(prompts["active"].enabled);
            assert_eq!(prompts["inactive"].content, "original");
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
            assert_eq!(
                state.db.get_prompts(app.as_str()).unwrap()["active"].content,
                content
            );
        }

        let mut saved = state.db.get_prompts(app.as_str()).unwrap()["active"].clone();
        saved.updated_at = Some(42);
        state.db.save_prompt(app.as_str(), &saved).unwrap();
        assert_eq!(
            PromptService::get_prompts(&state, app.clone()).unwrap()["active"].updated_at,
            Some(42)
        );

        fs::remove_file(&path).unwrap();
        assert_eq!(
            PromptService::get_prompts(&state, app.clone()).unwrap()["active"].updated_at,
            Some(42)
        );
        assert!(!path.exists());

        saved.enabled = false;
        state.db.save_prompt(app.as_str(), &saved).unwrap();
        fs::write(&path, "unmanaged instructions").unwrap();
        assert_eq!(
            PromptService::get_prompts(&state, app).unwrap()["active"].content,
            ""
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "unmanaged instructions");
    }
}
