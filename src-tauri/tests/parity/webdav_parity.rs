use cc_switch_lib::bridges::webdav as webdav_bridge;
use cc_switch_core::WebDavSyncSettings;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn webdav_parity_save_settings_matches_legacy() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    let settings = WebDavSyncSettings {
        enabled: true,
        base_url: "https://dav.example.com".to_string(),
        username: "alice".to_string(),
        password: "secret".to_string(),
        remote_root: "cc-switch/device".to_string(),
        ..WebDavSyncSettings::default()
    };

    reset_test_fs();
    let _home = ensure_test_home();
    webdav_bridge::legacy_save_settings_from_core(settings.clone()).expect("legacy save webdav");
    let legacy = webdav_bridge::legacy_get_settings_as_core()
        .expect("legacy settings")
        .expect("legacy settings should exist");

    reset_test_fs();
    let _home = ensure_test_home();
    webdav_bridge::save_settings_from_core(settings).expect("core save webdav");
    let core = webdav_bridge::get_settings_as_core()
        .expect("core settings")
        .expect("core settings should exist");

    assert_eq!(
        serde_json::to_value(core).expect("core json"),
        serde_json::to_value(legacy).expect("legacy json")
    );
}
