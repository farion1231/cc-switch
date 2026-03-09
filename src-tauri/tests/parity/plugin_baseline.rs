use cc_switch_lib::bridges::plugin as plugin_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn plugin_baseline_legacy_apply_and_onboarding_are_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    reset_test_fs();

    let home = ensure_test_home().to_path_buf();

    let initial = plugin_bridge::legacy_get_status().expect("legacy plugin status");
    assert!(!initial.exists);

    assert!(
        plugin_bridge::legacy_apply_config(false).expect("legacy apply plugin"),
        "first managed apply should modify config"
    );
    assert!(plugin_bridge::legacy_is_applied().expect("legacy is applied"));

    let config = plugin_bridge::legacy_read_config()
        .expect("legacy read config")
        .expect("plugin config should exist");
    assert!(config.contains("\"primaryApiKey\": \"any\""));

    assert!(
        plugin_bridge::legacy_apply_onboarding_skip().expect("legacy onboarding apply"),
        "onboarding flag should be written"
    );
    let onboarding = std::fs::read_to_string(home.join(".claude.json")).expect("read onboarding");
    assert!(onboarding.contains("\"hasCompletedOnboarding\": true"));
}
