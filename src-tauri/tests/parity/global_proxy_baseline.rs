use serde_json::json;

use cc_switch_lib::bridges::global_proxy as global_proxy_bridge;

use super::support::{create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn global_proxy_baseline_legacy_set_snapshot_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_empty_legacy_state();
    global_proxy_bridge::legacy_set_proxy_url(&state, "http://127.0.0.1:7890")
        .expect("legacy set global proxy");

    let snapshot = json!({
        "url": global_proxy_bridge::legacy_get_proxy_url(&state).expect("legacy get proxy url"),
        "status": global_proxy_bridge::legacy_get_status(),
    });

    assert_eq!(snapshot["url"], json!("http://127.0.0.1:7890"));
    assert_eq!(snapshot["status"]["enabled"], json!(true));
}
