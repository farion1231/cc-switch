#![cfg(unix)]

use cc_switch_lib::{
    migrate_skills_to_ssot, update_settings, AppSettings, AppType, ImportSkillSelection, SkillApps,
    SkillService, SkillStorageLocation, SyncMethod,
};
use std::fs;
use std::path::Path;
#[path = "support.rs"]
mod support;

fn write_original(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).unwrap();
    }
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        "---\nname: Local original\ndescription: shared edits\n---\n",
    )
    .unwrap();
    fs::write(path.join("probe.txt"), "initial").unwrap();
}

#[cfg(unix)]
fn link(source: &Path, destination: &Path) {
    std::os::unix::fs::symlink(source, destination).unwrap();
}

#[cfg(windows)]
fn link(source: &Path, destination: &Path) {
    std::os::windows::fs::symlink_dir(source, destination).unwrap();
}

#[test]
fn linked_workspace_edits_are_shared_without_an_ssot_copy_even_in_copy_mode() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    update_settings(AppSettings {
        skill_sync_method: SyncMethod::Copy,
        ..Default::default()
    })
    .unwrap();
    let home = support::ensure_test_home();
    let source = home.join("workspace/live-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Claude).unwrap();
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, true).unwrap();
    let claude = home.join(".claude/skills/live-probe");
    let codex = home.join(".codex/skills/live-probe");
    assert_eq!(
        fs::read_link(&claude).unwrap(),
        source.canonicalize().unwrap()
    );
    assert_eq!(
        fs::read_link(&codex).unwrap(),
        source.canonicalize().unwrap()
    );
    assert!(!home.join(".cc-switch/skills/live-probe").exists());
    fs::write(source.join("probe.txt"), "source-edited").unwrap();
    assert_eq!(
        fs::read_to_string(codex.join("probe.txt")).unwrap(),
        "source-edited"
    );
    fs::write(codex.join("probe.txt"), "codex-edited").unwrap();
    assert_eq!(
        fs::read_to_string(claude.join("probe.txt")).unwrap(),
        "codex-edited"
    );
    SkillService::sync_to_app(&state.db, &AppType::Claude).unwrap();
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "codex-edited"
    );
    let sql = state.db.export_sql_string_for_sync().unwrap();
    assert!(!sql.contains(&source.to_string_lossy().to_string()));
    assert!(
        !sql.contains("live-probe"),
        "device-local registrations must not create missing-source rows on another device"
    );
    assert!(state
        .db
        .get_installed_skill(&installed.id)
        .unwrap()
        .is_some());
}

#[test]
fn host_original_and_foreign_links_survive_disable_and_uninstall() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/host-probe");
    write_original(&source);
    let foreign = home.join(".gemini/skills/host-probe");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    link(&source, &foreign);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Codex).unwrap();
    assert!(installed.apps.claude && installed.apps.gemini && installed.apps.codex);
    assert!(SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, false).is_err());
    assert!(SkillService::toggle_app(&state.db, &installed.id, &AppType::Gemini, false).is_err());
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, false).unwrap();
    assert!(fs::symlink_metadata(home.join(".codex/skills/host-probe")).is_err());
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, true).unwrap();
    let result = SkillService::uninstall(&state.db, &installed.id).unwrap();
    assert!(result.backup_path.is_none());
    assert!(!fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(&foreign)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "initial"
    );
    assert!(state
        .db
        .get_installed_skill(&installed.id)
        .unwrap()
        .is_none());
}

#[test]
fn offline_source_errors_without_copy_fallback_and_can_be_unlinked() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/offline-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Claude).unwrap();
    let route = home.join(".claude/skills/offline-probe");
    fs::remove_dir_all(&source).unwrap();
    let error =
        SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, true).unwrap_err();
    assert!(format!("{error:#}").contains("unavailable"));
    assert!(fs::symlink_metadata(&route)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!home.join(".codex/skills/offline-probe").exists());
    assert!(
        !state
            .db
            .get_installed_skill(&installed.id)
            .unwrap()
            .unwrap()
            .apps
            .codex
    );
    let outcomes = SkillService::resync_all_apps(&state.db);
    assert!(outcomes
        .iter()
        .any(|outcome| outcome.app == "claude" && !outcome.ok));
    SkillService::uninstall(&state.db, &installed.id).unwrap();
    assert!(fs::symlink_metadata(&route).is_err());
}

#[test]
fn conflicting_destinations_are_preserved_and_failed_registration_is_rolled_back() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/conflict-probe");
    let destination = home.join(".claude/skills/conflict-probe");
    write_original(&source);
    write_original(&destination);
    fs::write(destination.join("probe.txt"), "foreign").unwrap();
    let state = support::create_test_state().unwrap();
    assert!(SkillService::link_source(&state.db, &source, &AppType::Claude).is_err());
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(destination.join("probe.txt")).unwrap(),
        "foreign"
    );
    let installed = SkillService::link_source(&state.db, &source, &AppType::Codex).unwrap();
    assert!(SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, true).is_err());
    SkillService::uninstall(&state.db, &installed.id).unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("probe.txt")).unwrap(),
        "foreign"
    );
}

#[test]
fn nested_source_routes_are_rejected_without_a_self_referencing_symlink() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/nesting-probe");
    write_original(&source);
    update_settings(AppSettings {
        claude_config_dir: Some(source.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .unwrap();
    let state = support::create_test_state().unwrap();
    assert!(SkillService::link_source(&state.db, &source, &AppType::Claude).is_err());
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
    assert!(fs::symlink_metadata(source.join("skills/nesting-probe")).is_err());
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "initial"
    );
}

#[test]
fn link_permission_failure_never_falls_back_to_a_copy_or_leaves_a_registration() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/permission-probe");
    write_original(&source);
    let root = home.join(".claude/skills");
    fs::create_dir_all(&root).unwrap();
    struct RestorePermissions<'a>(&'a Path, fs::Permissions);
    impl Drop for RestorePermissions<'_> {
        fn drop(&mut self) {
            fs::set_permissions(self.0, self.1.clone()).unwrap();
        }
    }
    let _permissions = RestorePermissions(&root, fs::metadata(&root).unwrap().permissions());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    let state = support::create_test_state().unwrap();
    // Backup retention must never be allowed to own an external original.
    let backup_source = home.join(".cc-switch/skill-backups/backup-source-probe");
    write_original(&backup_source);
    assert!(SkillService::link_source(&state.db, &backup_source, &AppType::Codex).is_err());
    assert_eq!(
        fs::read_to_string(backup_source.join("probe.txt")).unwrap(),
        "initial"
    );
    assert!(SkillService::link_source(&state.db, &source, &AppType::Claude).is_err());
    assert!(state.db.get_all_installed_skills().unwrap().is_empty());
    assert!(fs::symlink_metadata(root.join("permission-probe")).is_err());
    assert!(!home.join(".cc-switch/skills/permission-probe").exists());
    let settings: AppSettings =
        serde_json::from_slice(&fs::read(home.join(".cc-switch/settings.json")).unwrap()).unwrap();
    assert!(settings.linked_skill_sources.is_empty());
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "initial"
    );
}

#[test]
fn registration_owns_exact_paths_across_host_directory_changes() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/ownership-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Claude).unwrap();
    let old = home.join(".claude/skills/ownership-probe");
    let mut settings: AppSettings =
        serde_json::from_slice(&fs::read(home.join(".cc-switch/settings.json")).unwrap()).unwrap();
    let override_root = home.join("new-claude");
    let foreign = override_root.join("skills/ownership-probe");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    link(&source, &foreign);
    settings.claude_config_dir = Some(override_root.to_string_lossy().into_owned());
    update_settings(settings).unwrap();
    SkillService::uninstall(&state.db, &installed.id).unwrap();
    assert!(fs::symlink_metadata(old).is_err());
    assert!(fs::symlink_metadata(foreign)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(source.join("SKILL.md").is_file());
}

#[test]
fn storage_migration_leaves_external_originals_and_routes_in_place() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/migration-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Claude).unwrap();
    let result = SkillService::migrate_storage(&state.db, SkillStorageLocation::Unified).unwrap();
    assert!(result.errors.is_empty());
    assert_eq!(result.skipped_count, 1);
    assert!(!home.join(".agents/skills/migration-probe").exists());
    assert_eq!(
        fs::read_link(home.join(".claude/skills/migration-probe")).unwrap(),
        source.canonicalize().unwrap()
    );
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, true).unwrap();
    assert_eq!(
        fs::read_to_string(home.join(".codex/skills/migration-probe/probe.txt")).unwrap(),
        "initial"
    );
}

#[test]
fn legacy_bootstrap_migration_does_not_import_linked_originals() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/bootstrap-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Codex).unwrap();
    assert_eq!(migrate_skills_to_ssot(&state.db).unwrap(), 0);
    assert!(!home.join(".cc-switch/skills/bootstrap-probe").exists());
    let after = state
        .db
        .get_installed_skill(&installed.id)
        .unwrap()
        .unwrap();
    assert_eq!(after.name, installed.name);
    assert!(after.apps.claude && after.apps.codex);
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "initial"
    );
}

#[test]
fn confirmed_conversion_of_an_imported_snapshot_preserves_the_selected_host_original() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/convert-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let previous = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "convert-probe".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap()
    .remove(0);
    let slot = home.join(".cc-switch/skills/convert-probe");
    let codex = home.join(".codex/skills/convert-probe");
    fs::write(source.join("probe.txt"), "chosen-original").unwrap();
    fs::write(slot.join("probe.txt"), "diverged-snapshot").unwrap();
    assert!(SkillService::link_source(&state.db, &source, &AppType::Codex).is_err());
    assert!(slot.is_dir(), "no conversion before explicit confirmation");
    let converted =
        SkillService::link_source_confirmed(&state.db, &source, &AppType::Codex, true).unwrap();
    assert_eq!(converted.id, previous.id);
    assert!(!fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "chosen-original"
    );
    assert_eq!(
        fs::read_to_string(codex.join("probe.txt")).unwrap(),
        "chosen-original"
    );
    assert_eq!(
        fs::read_link(&codex).unwrap(),
        source.canonicalize().unwrap()
    );
    assert!(fs::symlink_metadata(&slot).is_err());
    let backups = SkillService::list_backups().unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(Path::new(&backups[0].backup_path).join("skill/probe.txt")).unwrap(),
        "diverged-snapshot"
    );
    SkillService::uninstall(&state.db, &converted.id).unwrap();
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "chosen-original"
    );
    assert!(fs::symlink_metadata(&codex).is_err());
}

#[test]
fn shared_linked_route_can_disable_either_app_without_harming_other_app() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join("workspace/shared-linked-projection-probe");
    write_original(&source);
    // Both apps resolve to the same physical Skills directory.
    update_settings(AppSettings {
        claude_config_dir: Some(home.join(".codex").to_string_lossy().into_owned()),
        ..Default::default()
    })
    .unwrap();
    let state = support::create_test_state().unwrap();
    let installed = SkillService::link_source(&state.db, &source, &AppType::Codex).unwrap();
    let route = home.join(".codex/skills/shared-linked-projection-probe");
    let original = source.canonicalize().unwrap();
    assert_eq!(fs::read_link(&route).unwrap(), original);

    SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, true).unwrap();
    // Removing one projection must not delete the route needed by the other.
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, false).unwrap();
    assert_eq!(fs::read_link(&route).unwrap(), original);
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, true).unwrap();
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Codex, false).unwrap();
    assert_eq!(fs::read_link(&route).unwrap(), original);

    // The final owner can remove the symlink, but never the workspace original.
    SkillService::toggle_app(&state.db, &installed.id, &AppType::Claude, false).unwrap();
    assert!(fs::symlink_metadata(&route).is_err());
    assert!(source.join("SKILL.md").is_file());
}

#[test]
fn conversion_deduplicates_shared_skills_directory_overrides() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/shared-override-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "shared-override-probe".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap()
    .remove(0);
    let route = home.join(".codex/skills/shared-override-probe");
    let slot = home.join(".cc-switch/skills/shared-override-probe");
    assert_eq!(fs::read_link(&route).unwrap(), slot);

    // Both app types now resolve to the same physical old symlink.
    update_settings(AppSettings {
        claude_config_dir: Some(home.join(".codex").to_string_lossy().into_owned()),
        ..Default::default()
    })
    .unwrap();
    let converted =
        SkillService::link_source_confirmed(&state.db, &source, &AppType::Codex, true).unwrap();
    assert_eq!(converted.id, imported.id);
    assert_eq!(
        fs::read_link(&route).unwrap(),
        source.canonicalize().unwrap()
    );
    assert!(fs::symlink_metadata(&slot).is_err());
    assert!(source.join("SKILL.md").is_file());

    SkillService::uninstall(&state.db, &converted.id).unwrap();
    assert!(fs::symlink_metadata(&route).is_err());
    assert!(source.join("SKILL.md").is_file());
}

#[test]
fn conversion_rejects_independent_app_copies_without_changing_any_content() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/convert-conflict-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let previous = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "convert-conflict-probe".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap()
    .remove(0);
    let slot = home.join(".cc-switch/skills/convert-conflict-probe");
    let codex = home.join(".codex/skills/convert-conflict-probe");
    fs::remove_file(&codex).unwrap();
    write_original(&codex);
    fs::write(codex.join("probe.txt"), "independent-copy").unwrap();
    assert!(
        SkillService::link_source_confirmed(&state.db, &source, &AppType::Codex, true).is_err()
    );
    assert_eq!(
        fs::read_to_string(codex.join("probe.txt")).unwrap(),
        "independent-copy"
    );
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "initial"
    );
    assert_eq!(
        fs::read_to_string(slot.join("probe.txt")).unwrap(),
        "initial"
    );
    assert_eq!(
        state
            .db
            .get_installed_skill(&previous.id)
            .unwrap()
            .unwrap()
            .apps,
        previous.apps
    );
    assert!(SkillService::list_backups().unwrap().is_empty());
    // A foreign pointer to another source is also a conflict, never a route we own.
    fs::remove_dir_all(&codex).unwrap();
    let other = home.join("workspace/foreign-conversion-probe");
    write_original(&other);
    link(&other, &codex);
    assert!(
        SkillService::link_source_confirmed(&state.db, &source, &AppType::Codex, true).is_err()
    );
    assert_eq!(fs::read_link(codex).unwrap(), other);
}

#[test]
fn conversion_failure_rolls_back_snapshot_routes_and_database_without_touching_original() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/convert-rollback-probe");
    write_original(&source);
    let state = support::create_test_state().unwrap();
    let previous = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "convert-rollback-probe".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap()
    .remove(0);
    let slot = home.join(".cc-switch/skills/convert-rollback-probe");
    let codex = home.join(".codex/skills/convert-rollback-probe");
    let old_target = fs::read_link(&codex).unwrap();
    fs::write(source.join("probe.txt"), "chosen-original").unwrap();
    let root = codex.parent().unwrap();
    let permissions = fs::metadata(root).unwrap().permissions();
    fs::set_permissions(root, fs::Permissions::from_mode(0o555)).unwrap();
    let outcome = SkillService::link_source_confirmed(&state.db, &source, &AppType::Codex, true);
    fs::set_permissions(root, permissions).unwrap();
    assert!(outcome.is_err());
    assert_eq!(fs::read_link(codex).unwrap(), old_target);
    assert_eq!(
        fs::read_to_string(slot.join("probe.txt")).unwrap(),
        "initial"
    );
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "chosen-original"
    );
    assert!(!fs::symlink_metadata(source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        state
            .db
            .get_installed_skill(&previous.id)
            .unwrap()
            .unwrap()
            .apps,
        previous.apps
    );
    let settings: AppSettings =
        serde_json::from_slice(&fs::read(home.join(".cc-switch/settings.json")).unwrap()).unwrap();
    assert!(settings.linked_skill_sources.is_empty());
}

#[test]
fn imported_real_app_source_is_a_snapshot_but_new_routes_share_ssot() {
    let _guard = support::test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    support::reset_test_fs();
    let home = support::ensure_test_home();
    let source = home.join(".claude/skills/snapshot-probe");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: snapshot-probe\ndescription: isolated probe\n---\n",
    )
    .unwrap();
    fs::write(source.join("probe.txt"), "initial").unwrap();
    let state = support::create_test_state().unwrap();
    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "snapshot-probe".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert_eq!(imported.len(), 1);
    let route = home.join(".codex/skills/snapshot-probe");
    let managed = home.join(".cc-switch/skills/snapshot-probe");
    assert!(!fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!fs::symlink_metadata(&managed)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(&route)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::write(source.join("probe.txt"), "source-edited").unwrap();
    assert_eq!(
        fs::read_to_string(route.join("probe.txt")).unwrap(),
        "initial"
    );
    fs::write(route.join("probe.txt"), "route-edited").unwrap();
    assert_eq!(
        fs::read_to_string(managed.join("probe.txt")).unwrap(),
        "route-edited"
    );
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "source-edited"
    );
    SkillService::sync_to_app_dir("snapshot-probe", &AppType::Claude).unwrap();
    assert_eq!(
        fs::read_to_string(source.join("probe.txt")).unwrap(),
        "route-edited"
    );
    assert!(!fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
}
