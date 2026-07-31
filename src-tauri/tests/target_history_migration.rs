use std::fs;

use cc_switch_lib::{
    AppType, CodexTargetHistoryManager, ConfigLocation, ManagedTarget, ManagementState, TargetKind,
};
use rusqlite::Connection;
use tempfile::tempdir;

fn windows_target(config_dir: &std::path::Path) -> ManagedTarget {
    ManagedTarget {
        id: "codex-windows-test".to_string(),
        app: AppType::Codex,
        name: "Windows Codex".to_string(),
        kind: TargetKind::LocalWindows,
        config_location: ConfigLocation {
            path: config_dir.to_string_lossy().to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Managed,
        provider_overrides: Default::default(),
        last_viewed_at: None,
    }
}

fn wsl_target(config_dir: &std::path::Path) -> ManagedTarget {
    ManagedTarget {
        id: "codex-wsl-test".to_string(),
        app: AppType::Codex,
        name: "Ubuntu · tester".to_string(),
        kind: TargetKind::Wsl {
            distro: "Ubuntu-Test".to_string(),
            user: "tester".to_string(),
        },
        config_location: ConfigLocation {
            path: config_dir.to_string_lossy().to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Managed,
        provider_overrides: Default::default(),
        last_viewed_at: None,
    }
}

#[test]
fn local_target_migration_unifies_every_legacy_bucket_and_keeps_a_backup() {
    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join(".codex");
    let sessions = config_dir.join("sessions/2026/07/22");
    fs::create_dir_all(&sessions).expect("session tree");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"custom\"\n",
    )
    .expect("config");
    fs::write(
        sessions.join("official.jsonl"),
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"official\",\"model_provider\":\"openai\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"text\":\"unchanged\"}}\n"
        ),
    )
    .expect("official session");
    fs::write(
        sessions.join("generated.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"generated\",\"model_provider\":\"cc_switch_pinai_deadbeef\"}}\n",
    )
    .expect("generated session");
    fs::write(
        sessions.join("custom.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"custom\",\"model_provider\":\"custom\"}}\n",
    )
    .expect("custom session");

    let state_path = config_dir.join("state_5.sqlite");
    let state = Connection::open(&state_path).expect("state db");
    state
        .execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads VALUES
               ('official', 'openai'),
               ('generated', 'cc-switch-official'),
               ('custom', 'custom');",
        )
        .expect("state rows");
    drop(state);

    let backup_parent = fixture.path().join("backups");
    let manager = CodexTargetHistoryManager::with_local_backup_parent(backup_parent.clone());
    let result = manager
        .migrate(&windows_target(&config_dir))
        .expect("migrate target");

    assert_eq!(result.changed_jsonl_files, 2);
    assert_eq!(result.changed_state_rows, 2);
    let backup_path = result.backup_path.expect("backup generation");
    assert!(std::path::Path::new(&backup_path).starts_with(&backup_parent));
    assert!(std::path::Path::new(&backup_path)
        .join("manifest.json")
        .is_file());

    let official = fs::read_to_string(sessions.join("official.jsonl")).expect("official result");
    assert!(official.contains("\"model_provider\":\"custom\""));
    assert!(official.contains("\"text\":\"unchanged\""));
    let generated = fs::read_to_string(sessions.join("generated.jsonl")).expect("generated result");
    assert!(generated.contains("\"model_provider\":\"custom\""));
    let custom = fs::read_to_string(sessions.join("custom.jsonl")).expect("custom result");
    assert!(custom.contains("\"model_provider\":\"custom\""));

    let state = Connection::open(&state_path).expect("reopen state db");
    let custom_rows: i64 = state
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE model_provider = 'custom'",
            [],
            |row| row.get(0),
        )
        .expect("custom row count");
    assert_eq!(custom_rows, 3);
    drop(state);

    let second = manager
        .migrate(&windows_target(&config_dir))
        .expect("repeat migration");
    assert_eq!(second.changed_jsonl_files, 0);
    assert_eq!(second.changed_state_rows, 0);
    assert_eq!(second.skipped_reason.as_deref(), Some("already_unified"));
    assert!(second.backup_path.is_none());
}

#[test]
fn target_migration_refuses_to_hide_history_when_live_route_is_not_custom() {
    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join(".codex");
    let sessions = config_dir.join("sessions/2026/07/22");
    fs::create_dir_all(&sessions).expect("session tree");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"openai\"\n",
    )
    .expect("config");
    let session_path = sessions.join("official.jsonl");
    fs::write(
        &session_path,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"official\",\"model_provider\":\"openai\"}}\n",
    )
    .expect("session");

    let manager =
        CodexTargetHistoryManager::with_local_backup_parent(fixture.path().join("backups"));
    let result = manager
        .migrate(&windows_target(&config_dir))
        .expect("guarded migration");

    assert_eq!(result.skipped_reason.as_deref(), Some("live_not_unified"));
    assert!(result.backup_path.is_none());
    assert!(fs::read_to_string(session_path)
        .expect("unchanged session")
        .contains("\"model_provider\":\"openai\""));
}

#[test]
fn local_target_restore_recovers_each_original_bucket_from_the_backup_ledger() {
    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join(".codex");
    let sessions = config_dir.join("sessions/2026/07/22");
    fs::create_dir_all(&sessions).expect("session tree");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"custom\"\n",
    )
    .expect("config");
    fs::write(
        sessions.join("official.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"official\",\"model_provider\":\"openai\"}}\n",
    )
    .expect("official session");
    fs::write(
        sessions.join("generated.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"generated\",\"model_provider\":\"cc_switch_pinai_deadbeef\"}}\n",
    )
    .expect("generated session");

    let state_path = config_dir.join("state_5.sqlite");
    let state = Connection::open(&state_path).expect("state db");
    state
        .execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads VALUES
               ('official', 'openai'),
               ('generated', 'cc-switch-official');",
        )
        .expect("state rows");
    drop(state);

    let manager =
        CodexTargetHistoryManager::with_local_backup_parent(fixture.path().join("backups"));
    manager
        .migrate(&windows_target(&config_dir))
        .expect("migrate target");
    let restored = manager
        .restore(&windows_target(&config_dir))
        .expect("restore target");

    assert_eq!(restored.changed_jsonl_files, 2);
    assert_eq!(restored.changed_state_rows, 2);
    assert!(restored.backup_path.is_some());
    let official = fs::read_to_string(sessions.join("official.jsonl")).expect("official result");
    assert!(official.contains("\"model_provider\":\"openai\""));
    let generated = fs::read_to_string(sessions.join("generated.jsonl")).expect("generated result");
    assert!(generated.contains("\"model_provider\":\"cc_switch_pinai_deadbeef\""));

    let state = Connection::open(&state_path).expect("reopen state db");
    let official_provider: String = state
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'official'",
            [],
            |row| row.get(0),
        )
        .expect("official provider");
    let generated_provider: String = state
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'generated'",
            [],
            |row| row.get(0),
        )
        .expect("generated provider");
    assert_eq!(official_provider, "openai");
    assert_eq!(generated_provider, "cc-switch-official");
}

#[test]
fn local_target_restore_reports_no_backup_ledger_when_none_exists() {
    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join(".codex");
    fs::create_dir_all(&config_dir).expect("codex dir");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"custom\"\n",
    )
    .expect("config");

    let manager =
        CodexTargetHistoryManager::with_local_backup_parent(fixture.path().join("backups"));
    let result = manager
        .restore(&windows_target(&config_dir))
        .expect("empty restore is a skip");
    assert_eq!(result.skipped_reason.as_deref(), Some("no_backup_ledger"));
    assert_eq!(result.changed_jsonl_files, 0);
    assert_eq!(result.changed_state_rows, 0);
    assert!(result.backup_path.is_none());
}

#[cfg(unix)]
#[test]
fn wsl_target_migration_runs_inside_the_distro_and_unifies_jsonl_and_sqlite() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join("wsl-home/.codex");
    let sessions = config_dir.join("sessions/2026/07/22");
    fs::create_dir_all(&sessions).expect("session tree");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"custom\"\n",
    )
    .expect("config");
    fs::write(
        sessions.join("legacy.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"legacy\",\"model_provider\":\"openai\"}}\n",
    )
    .expect("legacy session");
    let state_path = config_dir.join("state_5.sqlite");
    let state = Connection::open(&state_path).expect("state db");
    state
        .execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads VALUES ('legacy', 'cc_switch_pinai_deadbeef');",
        )
        .expect("state row");
    drop(state);

    let fake_wsl = fixture.path().join("wsl.exe");
    fs::write(
        &fake_wsl,
        "#!/bin/sh\nwhile [ \"$1\" != \"--exec\" ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .expect("fake wsl");
    let mut permissions = fs::metadata(&fake_wsl)
        .expect("fake metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_wsl, permissions).expect("fake permissions");

    let backup_parent = fixture.path().join("wsl-backups");
    let manager =
        CodexTargetHistoryManager::with_wsl_executable(fake_wsl, backup_parent.to_string_lossy());
    let result = manager
        .migrate(&wsl_target(&config_dir))
        .expect("migrate WSL target");

    assert_eq!(result.changed_jsonl_files, 1);
    assert_eq!(result.changed_state_rows, 1);
    assert!(result.backup_path.is_some());
    assert!(fs::read_to_string(sessions.join("legacy.jsonl"))
        .expect("rewritten session")
        .contains("\"model_provider\":\"custom\""));
    let state = Connection::open(&state_path).expect("reopen state db");
    let provider: String = state
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("provider");
    assert_eq!(provider, "custom");
}

#[cfg(unix)]
#[test]
fn wsl_target_restore_recovers_original_labels_and_is_idempotent() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join("wsl-home/.codex");
    let sessions = config_dir.join("sessions/2026/07/22");
    fs::create_dir_all(&sessions).expect("session tree");
    fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"custom\"\n",
    )
    .expect("config");
    fs::write(
        sessions.join("legacy.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"legacy\",\"model_provider\":\"openai\"}}\n",
    )
    .expect("legacy session");
    let state_path = config_dir.join("state_5.sqlite");
    let state = Connection::open(&state_path).expect("state db");
    state
        .execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads VALUES ('legacy', 'cc-switch-official');",
        )
        .expect("state row");
    drop(state);

    let fake_wsl = fixture.path().join("wsl.exe");
    fs::write(
        &fake_wsl,
        "#!/bin/sh\nwhile [ \"$1\" != \"--exec\" ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .expect("fake wsl");
    let mut permissions = fs::metadata(&fake_wsl)
        .expect("fake metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_wsl, permissions).expect("fake permissions");

    let manager = CodexTargetHistoryManager::with_wsl_executable(
        fake_wsl,
        fixture.path().join("wsl-backups").to_string_lossy(),
    );
    let target = wsl_target(&config_dir);
    manager.migrate(&target).expect("migrate WSL target");
    let restored = manager.restore(&target).expect("restore WSL target");
    assert_eq!(restored.changed_jsonl_files, 1);
    assert_eq!(restored.changed_state_rows, 1);
    assert!(fs::read_to_string(sessions.join("legacy.jsonl"))
        .expect("restored session")
        .contains("\"model_provider\":\"openai\""));
    let state = Connection::open(&state_path).expect("reopen state db");
    let provider: String = state
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("provider");
    assert_eq!(provider, "cc-switch-official");
    drop(state);

    let second_restore = manager.restore(&target).expect("repeat restore");
    assert_eq!(second_restore.changed_jsonl_files, 0);
    assert_eq!(second_restore.changed_state_rows, 0);
    assert_eq!(
        second_restore.skipped_reason.as_deref(),
        Some("nothing_to_restore")
    );
}
