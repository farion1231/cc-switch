use codex_plus_core::assets::paste_fix_enabled_config;
use codex_plus_core::settings::BackendSettings;

#[test]
fn paste_fix_defaults_to_false() {
    let settings = BackendSettings::default();
    assert!(!settings.codex_app_paste_fix);

    let json = serde_json::to_value(&settings).expect("serialize default settings");
    assert_eq!(
        json.get("codexAppPasteFix").and_then(|v| v.as_bool()),
        Some(false),
        "default BackendSettings JSON should include codexAppPasteFix = false"
    );
}

#[test]
fn paste_fix_round_trips_through_json() {
    let mut settings = BackendSettings::default();
    settings.codex_app_paste_fix = true;

    let json = serde_json::to_value(&settings).expect("serialize");
    assert_eq!(
        json.get("codexAppPasteFix").and_then(|v| v.as_bool()),
        Some(true)
    );

    let parsed: BackendSettings =
        serde_json::from_value(json).expect("deserialize codexAppPasteFix");
    assert!(parsed.codex_app_paste_fix);
}

#[test]
fn paste_fix_missing_from_old_json_defaults_to_false() {
    let json = serde_json::json!({
        "codexAppPath": "",
        "enhancementsEnabled": true,
    });

    let parsed: BackendSettings = serde_json::from_value(json)
        .expect("old settings JSON without codexAppPasteFix should still load");
    assert!(!parsed.codex_app_paste_fix);
}

#[test]
fn paste_fix_config_reflects_setting() {
    let mut settings = BackendSettings::default();
    assert_eq!(
        paste_fix_enabled_config(&settings),
        serde_json::json!({ "enabled": false })
    );

    settings.codex_app_paste_fix = true;
    assert_eq!(
        paste_fix_enabled_config(&settings),
        serde_json::json!({ "enabled": true })
    );
}

#[test]
fn injection_script_includes_paste_fix_global() {
    use codex_plus_core::assets::injection_script_with_settings;

    let mut settings = BackendSettings::default();
    settings.codex_app_paste_fix = true;
    let script = injection_script_with_settings(0, &settings);
    assert!(
        script.contains("window.__CODEX_PLUS_PASTE_FIX__ = {\"enabled\":true};"),
        "script should declare __CODEX_PLUS_PASTE_FIX__ as enabled, got: {}",
        &script[..script.find("window.__CODEX_PLUS_PASTE_FIX__").unwrap_or(0) + 80]
    );

    settings.codex_app_paste_fix = false;
    let script = injection_script_with_settings(0, &settings);
    assert!(
        script.contains("window.__CODEX_PLUS_PASTE_FIX__ = {\"enabled\":false};"),
        "script should declare __CODEX_PLUS_PASTE_FIX__ as disabled"
    );
}
