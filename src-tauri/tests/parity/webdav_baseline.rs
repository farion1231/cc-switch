use cc_switch_lib::bridges::webdav as webdav_bridge;
use cc_switch_core::WebDavSyncSettings;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn webdav_baseline_legacy_save_settings_is_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    let settings = WebDavSyncSettings {
        enabled: true,
        base_url: "https://dav.example.com".to_string(),
        username: "alice".to_string(),
        password: "secret".to_string(),
        remote_root: "cc-switch/device".to_string(),
        ..WebDavSyncSettings::default()
    };
    webdav_bridge::legacy_save_settings_from_core(settings.clone()).expect("legacy save webdav");
    let stored = webdav_bridge::legacy_get_settings_as_core()
        .expect("stored settings")
        .expect("settings should exist");
    assert_eq!(stored.base_url, settings.base_url);
    assert_eq!(stored.username, settings.username);
}
