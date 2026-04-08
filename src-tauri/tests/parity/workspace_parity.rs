use serde_json::json;

use cc_switch_lib::bridges::workspace as workspace_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn workspace_parity_daily_memory_round_trip_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    workspace_bridge::legacy_write_daily_memory_file("2026-03-09.md", "hello memory")
        .expect("legacy write daily memory");
    let legacy = json!({
        "files": workspace_bridge::legacy_list_daily_memory_files().expect("legacy list files"),
        "content": workspace_bridge::legacy_read_daily_memory_file("2026-03-09.md").expect("legacy read memory"),
    });

    reset_test_fs();
    let _home = ensure_test_home();
    workspace_bridge::write_daily_memory_file("2026-03-09.md", "hello memory")
        .expect("core write daily memory");
    let core = json!({
        "files": workspace_bridge::list_daily_memory_files().expect("core list files"),
        "content": workspace_bridge::read_daily_memory_file("2026-03-09.md").expect("core read memory"),
    });

    assert_eq!(core, legacy);
}
