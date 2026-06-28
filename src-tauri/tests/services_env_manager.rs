use cc_switch_lib::{delete_env_vars, restore_env_backup, BackupInfo, EnvConflict};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn make_conflict(
    var_name: &str,
    var_value: &str,
    source_type: &str,
    source_path: &str,
) -> EnvConflict {
    EnvConflict {
        var_name: var_name.to_string(),
        var_value: var_value.to_string(),
        source_type: source_type.to_string(),
        source_path: source_path.to_string(),
    }
}

#[test]
fn delete_env_vars_empty_conflicts_returns_backup_info() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let result = delete_env_vars(vec![]).expect("empty conflicts should succeed");
    assert!(result.conflicts.is_empty(), "no conflicts in backup");
    assert!(!result.backup_path.is_empty(), "backup_path must be set");
    assert!(!result.timestamp.is_empty(), "timestamp must be set");
}

#[test]
fn delete_env_vars_creates_backup_file_on_disk() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let result = delete_env_vars(vec![]).expect("should succeed");
    let backup_file = std::path::PathBuf::from(&result.backup_path);
    assert!(
        backup_file.exists(),
        "backup file must exist at {}",
        result.backup_path
    );
}

#[test]
fn delete_env_vars_backup_path_contains_cc_switch_backups() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let result = delete_env_vars(vec![]).expect("should succeed");
    assert!(
        result.backup_path.contains(".cc-switch"),
        "backup path should be inside .cc-switch dir, got: {}",
        result.backup_path
    );
    assert!(
        result.backup_path.contains("backups"),
        "backup path should be inside backups subdir, got: {}",
        result.backup_path
    );
}

#[test]
fn delete_env_vars_backup_file_contains_serialized_conflicts() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    #[cfg(not(target_os = "windows"))]
    {
        let home = ensure_test_home();
        let shell_file = home.join(".bashrc");
        std::fs::write(&shell_file, "export MY_VAR=hello\n").expect("write shell file");
        let source_path = format!("{}:1", shell_file.to_string_lossy());

        let conflict = make_conflict("MY_VAR", "hello", "file", &source_path);
        let result = delete_env_vars(vec![conflict]).expect("should succeed");

        let content = std::fs::read_to_string(&result.backup_path).expect("read backup file");
        let parsed: BackupInfo = serde_json::from_str(&content).expect("parse backup JSON");
        assert_eq!(parsed.conflicts.len(), 1, "one conflict in backup");
        assert_eq!(parsed.conflicts[0].var_name, "MY_VAR");
    }

    #[cfg(target_os = "windows")]
    {
        let result = delete_env_vars(vec![]).expect("should succeed");
        let content = std::fs::read_to_string(&result.backup_path).expect("read backup file");
        assert!(
            content.contains("conflicts"),
            "backup JSON has conflicts field"
        );
    }
}

#[test]
fn backup_info_serializes_with_camel_case_fields() {
    let info = BackupInfo {
        backup_path: "/tmp/test.json".to_string(),
        timestamp: "20260101_120000".to_string(),
        conflicts: vec![],
    };

    let json_str = serde_json::to_string(&info).expect("serialize BackupInfo");
    assert!(
        json_str.contains("backupPath"),
        "should serialize as camelCase backupPath"
    );
    assert!(
        json_str.contains("timestamp"),
        "should contain timestamp field"
    );
    assert!(
        json_str.contains("conflicts"),
        "should contain conflicts field"
    );
    assert!(
        !json_str.contains("backup_path"),
        "should NOT use snake_case"
    );
}

#[test]
fn restore_from_backup_errors_when_file_missing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let result = restore_env_backup("/nonexistent/path/backup.json".to_string());
    assert!(result.is_err(), "should error for nonexistent file");
    let err = result.unwrap_err();
    assert!(
        err.contains("读取备份文件失败") || err.contains("backup") || err.len() > 0,
        "error message should be informative: {err}"
    );
}

#[test]
fn restore_from_backup_errors_when_file_not_valid_json() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let bad_file = home.join("bad-backup.json");
    std::fs::write(&bad_file, "this is not json at all").expect("write bad file");

    let result = restore_env_backup(bad_file.to_string_lossy().to_string());
    assert!(result.is_err(), "should error for invalid JSON");
    let err = result.unwrap_err();
    assert!(!err.is_empty(), "error message must not be empty");
}

#[test]
fn restore_from_backup_errors_when_json_schema_wrong() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let bad_file = home.join("wrong-schema.json");
    std::fs::write(&bad_file, r#"{"someOtherField": 42}"#).expect("write wrong-schema file");

    let result = restore_env_backup(bad_file.to_string_lossy().to_string());
    assert!(result.is_err(), "should error for wrong schema JSON");
}

#[test]
fn restore_from_backup_succeeds_with_empty_conflicts() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let valid_json = r#"{"backupPath":"/tmp/x.json","timestamp":"20260101_120000","conflicts":[]}"#;
    let backup_file = home.join("empty-conflicts-backup.json");
    std::fs::write(&backup_file, valid_json).expect("write valid backup");

    let result = restore_env_backup(backup_file.to_string_lossy().to_string());
    assert!(
        result.is_ok(),
        "empty conflicts backup should restore without error: {result:?}"
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn delete_env_vars_unix_file_type_removes_export_line() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let shell_file = home.join(".profile_test_delete");
    std::fs::write(
        &shell_file,
        "export OTHER_VAR=keep\nexport REMOVE_ME=value\n",
    )
    .expect("write");
    let source_path = format!("{}:2", shell_file.to_string_lossy());

    let conflict = make_conflict("REMOVE_ME", "value", "file", &source_path);
    let result = delete_env_vars(vec![conflict]).expect("delete should succeed");
    assert!(!result.backup_path.is_empty());

    let remaining = std::fs::read_to_string(&shell_file).expect("read shell file after delete");
    assert!(!remaining.contains("REMOVE_ME"), "REMOVE_ME should be gone");
    assert!(remaining.contains("OTHER_VAR"), "OTHER_VAR should remain");
}

#[cfg(not(target_os = "windows"))]
#[test]
fn delete_env_vars_unix_file_type_handles_plain_var_eq_val_pattern() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let shell_file = home.join(".env_test_plain");
    std::fs::write(&shell_file, "KEEP=yes\nDELETE_THIS=no\n").expect("write");
    let source_path = format!("{}:2", shell_file.to_string_lossy());

    let conflict = make_conflict("DELETE_THIS", "no", "file", &source_path);
    let result = delete_env_vars(vec![conflict]).expect("delete should succeed");
    assert!(!result.backup_path.is_empty());

    let remaining = std::fs::read_to_string(&shell_file).expect("read after delete");
    assert!(
        !remaining.contains("DELETE_THIS"),
        "DELETE_THIS should be removed"
    );
    assert!(remaining.contains("KEEP"), "KEEP should remain");
}

#[cfg(not(target_os = "windows"))]
#[test]
fn delete_env_vars_unix_system_type_is_noop() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let conflict = make_conflict("SOME_VAR", "value", "system", "");
    let result = delete_env_vars(vec![conflict]);
    assert!(
        result.is_ok(),
        "system type on Unix should be no-op, not error: {result:?}"
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn delete_env_vars_unix_file_type_errors_when_file_missing() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    ensure_test_home();

    let conflict = make_conflict(
        "MISSING_FILE_VAR",
        "val",
        "file",
        "/nonexistent/shell/file.sh:1",
    );
    let result = delete_env_vars(vec![conflict]);
    assert!(result.is_err(), "should error when file does not exist");
}

#[cfg(not(target_os = "windows"))]
#[test]
fn restore_from_backup_unix_file_type_appends_export_line() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let shell_file = home.join(".restore_target");
    std::fs::write(&shell_file, "export EXISTING=value").expect("write shell file");
    let source_path = format!("{}:1", shell_file.to_string_lossy());

    let conflict = make_conflict("RESTORED_VAR", "restored_value", "file", &source_path);
    let backup_info = BackupInfo {
        backup_path: String::new(),
        timestamp: "20260101_120000".to_string(),
        conflicts: vec![conflict],
    };
    let json_str = serde_json::to_string(&backup_info).expect("serialize");
    let backup_file = home.join("restore-test-backup.json");
    std::fs::write(&backup_file, json_str).expect("write backup");

    let result = restore_env_backup(backup_file.to_string_lossy().to_string());
    assert!(result.is_ok(), "restore should succeed: {result:?}");

    let content = std::fs::read_to_string(&shell_file).expect("read shell file after restore");
    assert!(
        content.contains("export RESTORED_VAR=restored_value"),
        "restored var should be in file: {content}"
    );
}
