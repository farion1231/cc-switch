use serde_json::json;

use cc_switch_lib::bridges::settings as settings_bridge;
use cc_switch_lib::AppSettings;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn demo_settings() -> AppSettings {
    AppSettings {
        language: Some("zh".to_string()),
        show_in_tray: false,
        ..AppSettings::default()
    }
}

#[test]
fn settings_parity_save_and_get_matches_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    settings_bridge::legacy_save_settings(demo_settings()).expect("legacy save settings");
    let legacy = settings_bridge::legacy_get_settings().expect("legacy get settings");

    reset_test_fs();
    let _home = ensure_test_home();
    settings_bridge::save_settings(demo_settings()).expect("core save settings");
    let core = settings_bridge::get_settings().expect("core get settings");

    assert_eq!(json!(core), json!(legacy));
}
