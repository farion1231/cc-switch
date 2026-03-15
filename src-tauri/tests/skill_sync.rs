use std::fs;

use cc_switch_lib::{
    migrate_skills_to_ssot, AppType, ImportSkillSelection, InstalledSkill, SkillApps, SkillService,
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

#[cfg(unix)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) {
    std::os::unix::fs::symlink(src, dest).expect("create symlink");
}

#[cfg(windows)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) {
    std::os::windows::fs::symlink_dir(src, dest).expect("create symlink");
}

#[test]
fn import_from_apps_respects_explicit_app_selection() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("shared-skill"),
        "Shared",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("shared-skill"),
        "Shared",
    );

    let state = create_test_state().expect("create test state");

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "shared-skill".to_string(),
            apps: SkillApps {
                claude: false,
                codex: false,
                gemini: false,
                opencode: true,
            },
        }],
    )
    .expect("import skills");

    assert_eq!(imported.len(), 1, "expected exactly one imported skill");
    let skill = imported.first().expect("imported skill");
    assert!(
        skill.apps.opencode,
        "explicitly selected OpenCode app should remain enabled"
    );
    assert!(
        !skill.apps.claude && !skill.apps.codex && !skill.apps.gemini,
        "import should no longer infer apps from every matching source path"
    );
}

#[test]
fn sync_to_app_removes_disabled_and_orphaned_ssot_symlinks() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_dir = home.join(".cc-switch").join("skills");
    let disabled_skill = ssot_dir.join("disabled-skill");
    let orphan_skill = ssot_dir.join("orphan-skill");
    write_skill(&disabled_skill, "Disabled");
    write_skill(&orphan_skill, "Orphan");

    let opencode_skills_dir = home.join(".config").join("opencode").join("skills");
    fs::create_dir_all(&opencode_skills_dir).expect("create opencode skills dir");
    symlink_dir(&disabled_skill, &opencode_skills_dir.join("disabled-skill"));
    symlink_dir(&orphan_skill, &opencode_skills_dir.join("orphan-skill"));

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:disabled-skill".to_string(),
            name: "Disabled".to_string(),
            description: None,
            directory: "disabled-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: false,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 0,
        })
        .expect("save disabled skill");

    SkillService::sync_to_app(&state.db, &AppType::OpenCode).expect("reconcile skills");

    assert!(
        !opencode_skills_dir.join("disabled-skill").exists(),
        "DB-known disabled skill should be removed from OpenCode live dir"
    );
    assert!(
        !opencode_skills_dir.join("orphan-skill").exists(),
        "orphaned symlink into SSOT should be cleaned up"
    );
}

#[test]
fn migration_snapshot_overrides_multi_source_directory_inference() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("demo-skill"),
        "Demo",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("demo-skill"),
        "Demo",
    );

    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"demo-skill","app_type":"claude"}]"#,
        )
        .expect("seed migration snapshot");

    let count = migrate_skills_to_ssot(&state.db).expect("migrate skills to ssot");
    assert_eq!(count, 1, "expected one migrated skill");

    let skills = state.db.get_all_installed_skills().expect("get skills");
    let migrated = skills
        .values()
        .find(|skill| skill.directory == "demo-skill")
        .expect("migrated demo-skill");

    assert!(
        migrated.apps.claude,
        "legacy snapshot should preserve Claude enablement"
    );
    assert!(
        !migrated.apps.opencode,
        "migration should no longer infer OpenCode enablement from a duplicate directory alone"
    );
}
