use serde_json::json;

use cc_switch_lib::bridges::plugin as plugin_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn plugin_snapshot() -> serde_json::Value {
    let home = ensure_test_home().to_path_buf();
    let status = plugin_bridge::get_status().expect("plugin status");
    let config = plugin_bridge::read_config().expect("read plugin config");
    let applied = plugin_bridge::is_applied().expect("is applied");
    let onboarding = std::fs::read_to_string(home.join(".claude.json")).ok();

    json!({
        "status": status,
        "config": config,
        "applied": applied,
        "onboarding": onboarding,
    })
}

fn legacy_plugin_snapshot() -> serde_json::Value {
    let home = ensure_test_home().to_path_buf();
    let status = plugin_bridge::legacy_get_status().expect("legacy plugin status");
    let config = plugin_bridge::legacy_read_config().expect("legacy read plugin config");
    let applied = plugin_bridge::legacy_is_applied().expect("legacy is applied");
    let onboarding = std::fs::read_to_string(home.join(".claude.json")).ok();

    json!({
        "status": status,
        "config": config,
        "applied": applied,
        "onboarding": onboarding,
    })
}

#[test]
fn plugin_parity_apply_and_onboarding_match_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    plugin_bridge::legacy_apply_config(false).expect("legacy apply config");
    plugin_bridge::legacy_apply_onboarding_skip().expect("legacy onboarding");
    let legacy = legacy_plugin_snapshot();

    reset_test_fs();
    plugin_bridge::apply_config(false).expect("core apply config");
    plugin_bridge::apply_onboarding_skip().expect("core onboarding");
    let core = plugin_snapshot();

    assert_eq!(core, legacy);
}
