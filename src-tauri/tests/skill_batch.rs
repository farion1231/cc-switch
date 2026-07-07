use std::fs;
use std::sync::Arc;

use cc_switch_lib::{
    AppType, BatchSkillRequest, BatchSkillResult, InstalledSkill, SkillApps, SkillService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path, name: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

/// Helper: insert a skill into the database and create its SSOT directory.
fn insert_test_skill(
    db: &Arc<cc_switch_lib::Database>,
    dir_name: &str,
    display_name: &str,
    apps: SkillApps,
) -> String {
    let id = format!("local:{dir_name}");

    // Create SSOT directory so that uninstall can remove it
    let ssot = SkillService::get_ssot_dir().expect("get ssot dir");
    write_skill(&ssot.join(dir_name), display_name);

    db.save_skill(&InstalledSkill {
        id: id.clone(),
        name: display_name.to_string(),
        description: Some("Test".to_string()),
        directory: dir_name.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps,
        installed_at: 1000,
        content_hash: None,
        updated_at: 1000,
    })
    .expect("save skill");

    id
}

#[test]
fn batch_uninstall_all_succeed() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = insert_test_skill(
        &state.db,
        "batch-test-a",
        "Batch A",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );
    let id2 = insert_test_skill(
        &state.db,
        "batch-test-b",
        "Batch B",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );

    let results = SkillService::batch_uninstall(
        &state.db,
        &[BatchSkillRequest { id: id1 }, BatchSkillRequest { id: id2 }],
    );

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.success, "skill should be uninstalled: {r:?}");
    }

    // Verify both are removed from DB
    let installed = SkillService::get_all_installed(&state.db).expect("get installed");
    assert!(installed.is_empty(), "all skills should be removed");
}

#[test]
fn batch_uninstall_partial_failure() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = insert_test_skill(
        &state.db,
        "batch-partial-a",
        "Partial A",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );

    let results = SkillService::batch_uninstall(
        &state.db,
        &[
            BatchSkillRequest { id: id1 },
            BatchSkillRequest {
                id: "nonexistent-id".to_string(),
            },
        ],
    );

    assert_eq!(results.len(), 2);
    assert!(results[0].success, "first skill should be uninstalled");
    assert!(!results[1].success, "nonexistent skill should fail");
    assert!(
        results[1].error.is_some(),
        "failure should include an error message"
    );
}

#[test]
fn batch_toggle_app_all_succeed() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = insert_test_skill(
        &state.db,
        "batch-toggle-a",
        "Toggle A",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );
    let id2 = insert_test_skill(
        &state.db,
        "batch-toggle-b",
        "Toggle B",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );

    // Disable claude for both
    let results = SkillService::batch_toggle_app(
        &state.db,
        &[BatchSkillRequest { id: id1 }, BatchSkillRequest { id: id2 }],
        &AppType::Claude,
        false,
    );

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.success, "toggle should succeed: {r:?}");
    }

    // Verify claude is disabled for both
    let installed = SkillService::get_all_installed(&state.db).expect("get installed");
    assert_eq!(installed.len(), 2);
    for skill in &installed {
        assert!(
            !skill.apps.claude,
            "claude should be disabled for {}",
            skill.name
        );
    }
}

/// Verify that batch_uninstall correctly maps errors from the underlying
/// collect_batch_results helper. Since collect_batch_results is private,
/// we test it indirectly through batch_uninstall with a mix of valid and
/// invalid IDs.
#[test]
fn batch_uninstall_maps_errors_correctly() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    // Only one valid skill
    let id1 = insert_test_skill(
        &state.db,
        "batch-error-map",
        "Error Map",
        SkillApps {
            claude: true,
            ..Default::default()
        },
    );

    let results = SkillService::batch_uninstall(
        &state.db,
        &[
            BatchSkillRequest { id: id1 },
            BatchSkillRequest {
                id: "bad-id-1".to_string(),
            },
            BatchSkillRequest {
                id: "bad-id-2".to_string(),
            },
        ],
    );

    assert_eq!(results.len(), 3);
    // First succeeds
    assert!(results[0].success);
    assert!(results[0].error.is_none());
    // Second and third fail with error messages
    assert!(!results[1].success);
    assert!(results[1].error.is_some());
    assert!(!results[2].success);
    assert!(results[2].error.is_some());
}
