use cc_switch_lib::bridges::env as env_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_shell_conflict() -> std::path::PathBuf {
    let home = ensure_test_home().to_path_buf();
    let path = home.join(".zshrc");
    std::fs::write(&path, "export ANTHROPIC_API_KEY=legacy-key\n").expect("seed zshrc");
    path
}

#[test]
fn env_baseline_legacy_file_conflict_delete_restore_is_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let path = seed_shell_conflict();
    let conflicts = env_bridge::legacy_check_env_conflicts("claude").expect("legacy env scan");
    let file_conflicts: Vec<_> = conflicts
        .into_iter()
        .filter(|item| item.source_type == "file" && item.source_path.starts_with(path.to_string_lossy().as_ref()))
        .collect();
    assert_eq!(file_conflicts.len(), 1);

    let backup = env_bridge::legacy_delete_env_vars(file_conflicts).expect("legacy delete env");
    let deleted = std::fs::read_to_string(&path).expect("read deleted file");
    assert!(!deleted.contains("ANTHROPIC_API_KEY"));

    env_bridge::legacy_restore_env_backup(backup.backup_path).expect("legacy restore env");
    let restored = std::fs::read_to_string(&path).expect("read restored file");
    assert!(restored.contains("ANTHROPIC_API_KEY=legacy-key"));
}
