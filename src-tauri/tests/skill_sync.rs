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
fn scan_unmanaged_detects_nested_claude_skills() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming"),
        "Brainstorming",
    );

    let state = create_test_state().expect("create test state");
    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");

    let skill = unmanaged
        .iter()
        .find(|skill| skill.directory == "superpowers/brainstorming")
        .expect("nested Claude skill should be discovered");

    assert_eq!(skill.name, "Brainstorming");
    assert!(
        skill.found_in.iter().any(|source| source == "claude"),
        "nested skill should be reported as coming from Claude"
    );
}

#[test]
fn scan_unmanaged_normalizes_managed_directory_casing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("MySkill");
    write_skill(&ssot_skill_dir, "MySkill");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:MySkill".to_string(),
            name: "MySkill".to_string(),
            description: Some("Managed mixed-case skill".to_string()),
            directory: "MySkill".to_string(),
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
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save mixed-case skill");

    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");
    assert!(
        unmanaged.iter().all(|skill| skill.directory != "MySkill"),
        "managed mixed-case directory should not be reported as unmanaged"
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
            content_hash: None,
            updated_at: 0,
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
fn sync_to_app_removes_disabled_copied_skill_directories() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_dir = home.join(".cc-switch").join("skills");
    let disabled_skill = ssot_dir.join("disabled-copy-skill");
    write_skill(&disabled_skill, "Disabled Copy");

    let opencode_skills_dir = home.join(".config").join("opencode").join("skills");
    let copied_skill_dir = opencode_skills_dir.join("disabled-copy-skill");
    write_skill(&copied_skill_dir, "Disabled Copy");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:disabled-copy-skill".to_string(),
            name: "Disabled Copy".to_string(),
            description: None,
            directory: "disabled-copy-skill".to_string(),
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
            content_hash: None,
            updated_at: 0,
        })
        .expect("save disabled copied skill");

    SkillService::sync_to_app(&state.db, &AppType::OpenCode).expect("reconcile skills");

    assert!(
        !copied_skill_dir.exists(),
        "disabled copied skill directory should be removed from the live app dir"
    );
}

#[test]
fn import_from_apps_accepts_nested_claude_skill_paths() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming"),
        "Brainstorming",
    );

    let state = create_test_state().expect("create test state");
    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "superpowers/brainstorming".to_string(),
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
        }],
    )
    .expect("import nested Claude skill");

    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].directory, "superpowers/brainstorming");
    assert!(
        home.join(".cc-switch")
            .join("skills")
            .join("superpowers")
            .join("brainstorming")
            .join("SKILL.md")
            .exists(),
        "nested Claude skill should be copied into SSOT with its relative path"
    );
    assert!(
        home.join(".claude")
            .join("skills")
            .join("brainstorming")
            .join("SKILL.md")
            .exists(),
        "imported nested Claude skill should be synced to the live leaf directory immediately"
    );
    assert!(
        !home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming")
            .exists(),
        "legacy nested Claude directory should be cleaned up during import reconciliation"
    );
}

#[test]
fn import_from_apps_rejects_conflicting_claude_leaf_names() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming"),
        "Superpowers Brainstorming",
    );
    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("tools")
            .join("brainstorming"),
        "Tools Brainstorming",
    );

    let state = create_test_state().expect("create test state");
    let error = SkillService::import_from_apps(
        &state.db,
        vec![
            ImportSkillSelection {
                directory: "superpowers/brainstorming".to_string(),
                apps: SkillApps {
                    claude: true,
                    codex: false,
                    gemini: false,
                    opencode: false,
                },
            },
            ImportSkillSelection {
                directory: "tools/brainstorming".to_string(),
                apps: SkillApps {
                    claude: true,
                    codex: false,
                    gemini: false,
                    opencode: false,
                },
            },
        ],
    )
    .expect_err("conflicting Claude leaf names should be rejected");

    assert!(
        error
            .to_string()
            .contains("Claude skills 目标路径冲突: brainstorming"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn uninstall_skill_creates_backup_before_removing_ssot() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("backup-skill");
    write_skill(&ssot_skill_dir, "Backup Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "backup me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:backup-skill".to_string(),
            name: "Backup Skill".to_string(),
            description: Some("Back me up before uninstall".to_string()),
            directory: "backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 123,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let result = SkillService::uninstall(&state.db, "local:backup-skill").expect("uninstall skill");
    let backup_path = result.backup_path.expect("backup path should be returned");
    let backup_dir = std::path::PathBuf::from(&backup_path);

    assert!(backup_dir.exists(), "backup directory should exist");
    assert!(
        backup_dir.join("skill").join("SKILL.md").exists(),
        "backup should include SKILL.md"
    );
    assert_eq!(
        fs::read_to_string(backup_dir.join("skill").join("prompt.md"))
            .expect("read backed up prompt"),
        "backup me"
    );

    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(backup_dir.join("meta.json")).expect("read backup metadata"),
    )
    .expect("parse backup metadata");
    assert_eq!(metadata["skill"]["directory"], "backup-skill");
    assert_eq!(metadata["skill"]["name"], "Backup Skill");

    assert!(
        !ssot_skill_dir.exists(),
        "SSOT skill directory should be removed after uninstall"
    );
    assert!(
        state
            .db
            .get_installed_skill("local:backup-skill")
            .expect("query skill")
            .is_none(),
        "database row should be deleted after uninstall"
    );
}

#[test]
fn restore_skill_backup_restores_files_to_ssot_and_current_app() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("restore-skill");
    write_skill(&ssot_skill_dir, "Restore Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "restore me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:restore-skill".to_string(),
            name: "Restore Skill".to_string(),
            description: Some("Bring the files back".to_string()),
            directory: "restore-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 456,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:restore-skill").expect("uninstall skill");
    let backup_id = std::path::Path::new(
        &uninstall
            .backup_path
            .expect("backup path should be returned on uninstall"),
    )
    .file_name()
    .expect("backup dir name")
    .to_string_lossy()
    .to_string();

    let restored = SkillService::restore_from_backup(&state.db, &backup_id, &AppType::Claude)
        .expect("restore from backup");

    assert_eq!(restored.directory, "restore-skill");
    assert!(restored.apps.claude, "restored skill should enable Claude");
    assert!(
        !restored.apps.codex && !restored.apps.gemini && !restored.apps.opencode,
        "restore should only enable the selected app"
    );
    assert!(
        home.join(".cc-switch")
            .join("skills")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored skill should exist in SSOT"
    );
    assert!(
        home.join(".claude")
            .join("skills")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored skill should sync to the selected app"
    );
    assert!(
        state
            .db
            .get_installed_skill("local:restore-skill")
            .expect("query restored skill")
            .is_some(),
        "restored skill should be written back to the database"
    );
}

#[test]
fn sync_to_claude_flattens_nested_skill_paths_to_leaf_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("superpowers")
        .join("brainstorming");
    write_skill(&ssot_skill_dir, "Brainstorming");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:superpowers/brainstorming".to_string(),
            name: "Brainstorming".to_string(),
            description: Some("Nested Claude skill".to_string()),
            directory: "superpowers/brainstorming".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 789,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save nested skill");

    SkillService::sync_to_app(&state.db, &AppType::Claude).expect("sync Claude skills");

    assert!(
        home.join(".claude")
            .join("skills")
            .join("brainstorming")
            .join("SKILL.md")
            .exists(),
        "nested skill should be exposed at Claude top level using its leaf directory"
    );
    assert!(
        !home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .exists(),
        "Claude live dir should not keep the grouping directory after sync"
    );
}

#[test]
fn sync_to_claude_cleans_legacy_nested_dir_and_scan_unmanaged_does_not_repeat() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming"),
        "Brainstorming",
    );

    let state = create_test_state().expect("create test state");
    SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "superpowers/brainstorming".to_string(),
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
        }],
    )
    .expect("import nested Claude skill");

    SkillService::sync_to_app(&state.db, &AppType::Claude).expect("sync Claude skills");

    assert!(
        home.join(".claude")
            .join("skills")
            .join("brainstorming")
            .join("SKILL.md")
            .exists(),
        "synced Claude leaf directory should exist"
    );
    assert!(
        !home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .join("brainstorming")
            .exists(),
        "legacy nested Claude directory should be removed after sync"
    );
    assert!(
        !home
            .join(".claude")
            .join("skills")
            .join("superpowers")
            .exists(),
        "empty Claude grouping directory should be pruned after cleanup"
    );

    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");
    assert!(
        unmanaged.iter().all(|skill| {
            skill.directory != "brainstorming" && skill.directory != "superpowers/brainstorming"
        }),
        "managed nested Claude skill should not reappear as unmanaged after sync"
    );
}

#[test]
fn real_claude_leaf_skill_conflicting_with_managed_nested_skill_still_appears_as_unmanaged() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("brainstorming"),
        "Real Claude Brainstorming",
    );

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("superpowers")
        .join("brainstorming");
    write_skill(&ssot_skill_dir, "Managed Brainstorming");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:superpowers/brainstorming".to_string(),
            name: "Managed Brainstorming".to_string(),
            description: Some("Enabled for Claude".to_string()),
            directory: "superpowers/brainstorming".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save managed nested skill");

    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");
    assert!(
        unmanaged.iter().any(|skill| skill.directory == "brainstorming"),
        "real Claude leaf skill should stay visible as unmanaged even when a managed nested Claude skill maps to the same leaf path"
    );
}

#[test]
fn sync_to_claude_rejects_overwriting_real_leaf_skill_with_managed_nested_skill() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let real_leaf_dir = home.join(".claude").join("skills").join("brainstorming");
    write_skill(&real_leaf_dir, "Real Claude Brainstorming");

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("superpowers")
        .join("brainstorming");
    write_skill(&ssot_skill_dir, "Managed Brainstorming");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:superpowers/brainstorming".to_string(),
            name: "Managed Brainstorming".to_string(),
            description: Some("Enabled for Claude".to_string()),
            directory: "superpowers/brainstorming".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save managed nested skill");

    let error = SkillService::sync_to_app(&state.db, &AppType::Claude)
        .expect_err("sync should refuse to overwrite a real Claude leaf skill");
    assert!(
        error
            .to_string()
            .contains("目标路径已存在且不是由 CC Switch 管理"),
        "unexpected error: {error:#}"
    );
    assert!(
        fs::read_to_string(real_leaf_dir.join("SKILL.md"))
            .expect("read real Claude skill")
            .contains("Real Claude Brainstorming"),
        "real Claude leaf skill should remain untouched after the rejected sync"
    );
}

#[test]
fn codex_only_nested_skill_does_not_block_real_claude_leaf_skill() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("brainstorming"),
        "Claude Brainstorming",
    );

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("superpowers")
        .join("brainstorming");
    write_skill(&ssot_skill_dir, "Codex Brainstorming");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:superpowers/brainstorming".to_string(),
            name: "Codex Brainstorming".to_string(),
            description: Some("Only enabled for Codex".to_string()),
            directory: "superpowers/brainstorming".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: false,
                codex: true,
                gemini: false,
                opencode: false,
            },
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save codex-only skill");

    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");
    assert!(
        unmanaged.iter().any(|skill| skill.directory == "brainstorming"),
        "real Claude leaf skill should still appear as unmanaged when only a Codex skill shares its leaf name"
    );

    SkillService::sync_to_app(&state.db, &AppType::Claude).expect("sync Claude skills");
    assert!(
        home.join(".claude")
            .join("skills")
            .join("brainstorming")
            .join("SKILL.md")
            .exists(),
        "sync_to_app(Claude) should not delete a real Claude skill just because a Codex-only skill shares its leaf name"
    );
}

#[test]
fn external_nested_claude_skill_with_conflicting_leaf_still_appears_as_unmanaged() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("superpowers")
        .join("brainstorming");
    write_skill(&ssot_skill_dir, "Managed Brainstorming");

    write_skill(
        &home
            .join(".claude")
            .join("skills")
            .join("tools")
            .join("brainstorming"),
        "External Brainstorming",
    );

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:superpowers/brainstorming".to_string(),
            name: "Managed Brainstorming".to_string(),
            description: Some("Enabled for Claude".to_string()),
            directory: "superpowers/brainstorming".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save managed claude skill");

    let unmanaged = SkillService::scan_unmanaged(&state.db).expect("scan unmanaged skills");
    assert!(
        unmanaged
            .iter()
            .any(|skill| skill.directory == "tools/brainstorming"),
        "external nested Claude skill should remain visible as unmanaged even if its leaf path conflicts with a managed Claude skill"
    );
}

#[test]
fn delete_skill_backup_removes_backup_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("delete-backup-skill");
    write_skill(&ssot_skill_dir, "Delete Backup Skill");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:delete-backup-skill".to_string(),
            name: "Delete Backup Skill".to_string(),
            description: Some("Remove my backup".to_string()),
            directory: "delete-backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            installed_at: 789,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:delete-backup-skill").expect("uninstall skill");
    let backup_path = uninstall
        .backup_path
        .expect("backup path should be returned on uninstall");
    let backup_id = std::path::Path::new(&backup_path)
        .file_name()
        .expect("backup dir name")
        .to_string_lossy()
        .to_string();

    assert!(
        std::path::Path::new(&backup_path).exists(),
        "backup directory should exist before deletion"
    );

    SkillService::delete_backup(&backup_id).expect("delete backup");

    assert!(
        !std::path::Path::new(&backup_path).exists(),
        "backup directory should be removed"
    );
    assert!(
        SkillService::list_backups()
            .expect("list backups")
            .into_iter()
            .all(|entry| entry.backup_id != backup_id),
        "deleted backup should no longer appear in backup list"
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
