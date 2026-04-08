use cc_switch_lib::bridges::env as env_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_shell_conflict() -> std::path::PathBuf {
    let home = ensure_test_home().to_path_buf();
    let path = home.join(".zshrc");
    std::fs::write(&path, "export ANTHROPIC_API_KEY=legacy-key\n").expect("seed zshrc");
    path
}

#[test]
fn env_parity_file_conflict_delete_restore_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let path = seed_shell_conflict();
    let legacy_conflicts: Vec<_> = env_bridge::legacy_check_env_conflicts("claude")
        .expect("legacy scan")
        .into_iter()
        .filter(|item| {
            item.source_type == "file"
                && item
                    .source_path
                    .starts_with(path.to_string_lossy().as_ref())
        })
        .collect();
    let legacy_backup =
        env_bridge::legacy_delete_env_vars(legacy_conflicts.clone()).expect("legacy delete");
    let legacy_deleted = std::fs::read_to_string(&path).expect("legacy deleted file");
    env_bridge::legacy_restore_env_backup(legacy_backup.backup_path).expect("legacy restore");
    let legacy_restored = std::fs::read_to_string(&path).expect("legacy restored file");

    reset_test_fs();
    let path = seed_shell_conflict();
    let core_conflicts: Vec<_> = env_bridge::check_env_conflicts("claude")
        .expect("core scan")
        .into_iter()
        .filter(|item| {
            item.source_type == "file"
                && item
                    .source_path
                    .starts_with(path.to_string_lossy().as_ref())
        })
        .collect();
    let core_backup = env_bridge::delete_env_vars(core_conflicts.clone()).expect("core delete");
    let core_deleted = std::fs::read_to_string(&path).expect("core deleted file");
    env_bridge::restore_env_backup(core_backup.backup_path).expect("core restore");
    let core_restored = std::fs::read_to_string(&path).expect("core restored file");

    assert_eq!(
        serde_json::to_value(core_conflicts).expect("core conflicts"),
        serde_json::to_value(legacy_conflicts).expect("legacy conflicts")
    );
    assert_eq!(core_deleted, legacy_deleted);
    assert_eq!(core_restored, legacy_restored);
}
