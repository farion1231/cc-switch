use cc_switch_lib::bridges::settings as settings_bridge;
use cc_switch_lib::AppSettings;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn settings_baseline_legacy_save_and_get_snapshot_is_stable() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let _home = ensure_test_home();

    settings_bridge::legacy_save_settings(AppSettings {
        language: Some("zh".to_string()),
        show_in_tray: false,
        ..AppSettings::default()
    })
    .expect("legacy save settings");

    let settings = settings_bridge::legacy_get_settings().expect("legacy get settings");
    assert_eq!(settings.language.as_deref(), Some("zh"));
    assert!(!settings.show_in_tray);
}
