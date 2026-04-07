use cc_switch_lib::bridges::workspace as workspace_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn workspace_baseline_legacy_daily_memory_round_trip_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    workspace_bridge::legacy_write_daily_memory_file("2026-03-09.md", "hello memory")
        .expect("legacy write daily memory");
    let files = workspace_bridge::legacy_list_daily_memory_files().expect("legacy list files");
    let content = workspace_bridge::legacy_read_daily_memory_file("2026-03-09.md")
        .expect("legacy read daily memory");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, "2026-03-09.md");
    assert_eq!(content.as_deref(), Some("hello memory"));
}
