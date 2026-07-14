use codex_plus_core::assets::{force_chinese_locale_config, injection_script_with_settings};
use codex_plus_core::settings::BackendSettings;

#[test]
fn force_chinese_locale_defaults_to_true() {
    let settings = BackendSettings::default();
    assert!(settings.codex_app_force_chinese_locale);
    assert!(!settings.codex_app_fast_startup);

    let json = serde_json::to_value(&settings).expect("serialize default settings");
    assert_eq!(
        json.get("codexAppForceChineseLocale")
            .and_then(|v| v.as_bool()),
        Some(true),
        "default BackendSettings JSON should include codexAppForceChineseLocale = true"
    );
    assert_eq!(
        json.get("codexAppFastStartup").and_then(|v| v.as_bool()),
        Some(false),
        "default BackendSettings JSON should include codexAppFastStartup = false"
    );
}

#[test]
fn force_chinese_locale_missing_from_old_json_defaults_to_true() {
    let json = serde_json::json!({
        "codexAppPath": "",
        "enhancementsEnabled": true,
    });

    let parsed: BackendSettings = serde_json::from_value(json)
        .expect("old settings JSON without codexAppForceChineseLocale should still load");
    assert!(parsed.codex_app_force_chinese_locale);
    assert!(!parsed.codex_app_fast_startup);
}

#[test]
fn force_chinese_locale_false_round_trips_through_json() {
    let mut settings = BackendSettings::default();
    settings.codex_app_force_chinese_locale = false;

    let json = serde_json::to_value(&settings).expect("serialize");
    assert_eq!(
        json.get("codexAppForceChineseLocale")
            .and_then(|v| v.as_bool()),
        Some(false)
    );

    let parsed: BackendSettings =
        serde_json::from_value(json).expect("deserialize codexAppForceChineseLocale");
    assert!(!parsed.codex_app_force_chinese_locale);
}

#[test]
fn force_chinese_locale_config_reflects_setting() {
    let mut settings = BackendSettings::default();
    assert_eq!(
        force_chinese_locale_config(&settings),
        serde_json::json!({ "enabled": true, "locale": "zh-CN" })
    );

    settings.codex_app_force_chinese_locale = false;
    assert_eq!(
        force_chinese_locale_config(&settings),
        serde_json::json!({ "enabled": false, "locale": "zh-CN" })
    );
}

#[test]
fn injection_script_includes_force_chinese_locale_global_and_patch() {
    let mut settings = BackendSettings::default();
    settings.codex_app_force_chinese_locale = true;
    settings.codex_app_fast_startup = true;
    let script = injection_script_with_settings(0, &settings);
    assert!(script.contains(
        "window.__CODEX_PLUS_FORCE_CHINESE_LOCALE__ = {\"enabled\":true,\"locale\":\"zh-CN\"};"
    ));
    assert!(script.contains(
        "window.__CODEX_PLUS_FAST_STARTUP__ = {\"enabled\":true,\"statsigTimeoutMs\":800};"
    ));
    assert!(script.contains("__codexPlusForceChineseLocaleInstalled"));
    assert!(script.contains("__codexPlusFastStartupInstalled"));
    assert!(script.contains("72216192"));
    assert!(script.contains("enable_i18n"));
    assert!(script.contains("locale_source"));
    assert!(!script.contains("setItem(\"localeOverride\""));

    settings.codex_app_force_chinese_locale = false;
    let script = injection_script_with_settings(0, &settings);
    assert!(script.contains(
        "window.__CODEX_PLUS_FORCE_CHINESE_LOCALE__ = {\"enabled\":false,\"locale\":\"zh-CN\"};"
    ));
}
