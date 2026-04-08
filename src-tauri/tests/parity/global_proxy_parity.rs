use serde_json::json;

use cc_switch_lib::bridges::global_proxy as global_proxy_bridge;

use super::support::{create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn global_proxy_parity_set_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let legacy = {
        let legacy_state = create_empty_legacy_state();
        global_proxy_bridge::legacy_set_proxy_url(&legacy_state, "http://127.0.0.1:7890")
            .expect("legacy set global proxy");
        json!({
            "url": global_proxy_bridge::legacy_get_proxy_url(&legacy_state).expect("legacy get proxy url"),
            "status": global_proxy_bridge::legacy_get_status(),
        })
    };

    reset_test_fs();
    let _home = ensure_test_home();
    global_proxy_bridge::set_proxy_url("http://127.0.0.1:7890").expect("core set global proxy");
    let core = json!({
        "url": global_proxy_bridge::get_proxy_url().expect("core get proxy url"),
        "status": global_proxy_bridge::get_status().expect("core get proxy status"),
    });

    assert_eq!(core, legacy);
}
