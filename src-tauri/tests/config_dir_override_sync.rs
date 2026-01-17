use serde_json::json;

use cc_switch_lib::{
    read_json_file, update_settings, write_codex_live_atomic, write_gemini_live, AppSettings,
    Provider,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn write_codex_live_respects_override_toggle_and_can_sync_both_dirs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();

    let override_codex_dir = home.join("wsl").join(".codex");
    let override_codex_dir_str = override_codex_dir.to_string_lossy().to_string();

    let mut settings = AppSettings::default();
    settings.codex_config_dir = Some(override_codex_dir_str);
    settings.enable_config_dir_overrides = false;
    settings.sync_provider_switch_to_both_config_dirs = true;
    update_settings(settings).expect("update settings");

    let default_auth_path = home.join(".codex").join("auth.json");
    let default_config_path = home.join(".codex").join("config.toml");
    let override_auth_path = override_codex_dir.join("auth.json");
    let override_config_path = override_codex_dir.join("config.toml");

    // Override stored but disabled should make default dir effective.
    assert_eq!(
        cc_switch_lib::get_codex_auth_path(),
        default_auth_path,
        "override disabled should make default codex dir effective"
    );

    let auth = json!({ "OPENAI_API_KEY": "fresh-key" });
    let config_text = r#"model_provider = "custom"
model = "gpt-4"
"#;

    write_codex_live_atomic(&auth, Some(config_text)).expect("write codex live");

    let auth_default: serde_json::Value =
        read_json_file(&default_auth_path).expect("read default auth.json");
    assert_eq!(
        auth_default
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "fresh-key",
        "default auth.json should be updated"
    );

    let auth_override: serde_json::Value =
        read_json_file(&override_auth_path).expect("read override auth.json");
    assert_eq!(
        auth_override
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "fresh-key",
        "override auth.json should be updated as well"
    );

    let cfg_default =
        std::fs::read_to_string(&default_config_path).expect("read default config.toml");
    assert!(
        cfg_default.contains("model_provider"),
        "default config.toml should be updated"
    );

    let cfg_override =
        std::fs::read_to_string(&override_config_path).expect("read override config.toml");
    assert!(
        cfg_override.contains("model_provider"),
        "override config.toml should be updated as well"
    );
}

#[test]
fn write_gemini_live_respects_override_toggle_and_can_sync_both_dirs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();

    let override_gemini_dir = home.join("wsl").join(".gemini");
    let override_gemini_dir_str = override_gemini_dir.to_string_lossy().to_string();

    let default_env_path = home.join(".gemini").join(".env");
    let override_env_path = override_gemini_dir.join(".env");

    let default_settings_path = home.join(".gemini").join("settings.json");
    let override_settings_path = override_gemini_dir.join("settings.json");

    std::fs::create_dir_all(default_settings_path.parent().expect("default settings parent"))
        .expect("create default settings dir");
    std::fs::create_dir_all(override_settings_path.parent().expect("override settings parent"))
        .expect("create override settings dir");

    std::fs::write(
        &default_settings_path,
        serde_json::to_string_pretty(&json!({ "marker": "default" }))
            .expect("serialize default settings"),
    )
    .expect("seed default settings.json");
    std::fs::write(
        &override_settings_path,
        serde_json::to_string_pretty(&json!({
            "marker": "override",
            "security": { "auth": { "selectedType": "old-type" } }
        }))
        .expect("serialize override settings"),
    )
    .expect("seed override settings.json");

    std::fs::write(&override_env_path, "GEMINI_API_KEY=old-key").expect("seed override .env");

    // Stage 1: override configured but disabled; no sync -> only default updated.
    let mut settings = AppSettings::default();
    settings.gemini_config_dir = Some(override_gemini_dir_str.clone());
    settings.enable_config_dir_overrides = false;
    settings.sync_provider_switch_to_both_config_dirs = false;
    update_settings(settings).expect("update settings");

    let provider_key_1 = Provider::with_id(
        "p1".to_string(),
        "Custom".to_string(),
        json!({ "env": { "GEMINI_API_KEY": "key-1" } }),
        None,
    );
    write_gemini_live(&provider_key_1).expect("write gemini live");

    let env_default =
        std::fs::read_to_string(&default_env_path).expect("read default .env after stage 1");
    assert!(
        env_default.contains("GEMINI_API_KEY=key-1"),
        "default .env should be updated when override disabled"
    );

    let env_override =
        std::fs::read_to_string(&override_env_path).expect("read override .env after stage 1");
    assert!(
        env_override.contains("GEMINI_API_KEY=old-key"),
        "override .env should not be updated when sync is disabled"
    );

    let settings_default: serde_json::Value =
        read_json_file(&default_settings_path).expect("read default settings.json after stage 1");
    assert_eq!(
        settings_default.get("marker").and_then(|v| v.as_str()),
        Some("default"),
        "default settings.json should preserve existing fields"
    );
    assert_eq!(
        settings_default
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "default selectedType should be updated"
    );

    let settings_override: serde_json::Value =
        read_json_file(&override_settings_path).expect("read override settings.json after stage 1");
    assert_eq!(
        settings_override.get("marker").and_then(|v| v.as_str()),
        Some("override"),
        "override settings.json should preserve existing fields"
    );
    assert_eq!(
        settings_override
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("old-type"),
        "override selectedType should not be updated when sync is disabled"
    );

    // Stage 2: same mode (override disabled) but enable sync -> both dirs updated.
    let mut settings = AppSettings::default();
    settings.gemini_config_dir = Some(override_gemini_dir_str);
    settings.enable_config_dir_overrides = false;
    settings.sync_provider_switch_to_both_config_dirs = true;
    update_settings(settings).expect("update settings stage 2");

    let provider_key_2 = Provider::with_id(
        "p2".to_string(),
        "Custom".to_string(),
        json!({ "env": { "GEMINI_API_KEY": "key-2" } }),
        None,
    );
    write_gemini_live(&provider_key_2).expect("write gemini live stage 2");

    let env_default =
        std::fs::read_to_string(&default_env_path).expect("read default .env after stage 2");
    assert!(
        env_default.contains("GEMINI_API_KEY=key-2"),
        "default .env should be updated in stage 2"
    );

    let env_override =
        std::fs::read_to_string(&override_env_path).expect("read override .env after stage 2");
    assert!(
        env_override.contains("GEMINI_API_KEY=key-2"),
        "override .env should be updated when sync is enabled"
    );

    let settings_override: serde_json::Value =
        read_json_file(&override_settings_path).expect("read override settings.json after stage 2");
    assert_eq!(
        settings_override.get("marker").and_then(|v| v.as_str()),
        Some("override"),
        "override settings.json should preserve existing fields in stage 2"
    );
    assert_eq!(
        settings_override
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "override selectedType should be updated when sync is enabled"
    );
}
