use serde_json::json;

use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, read_json_file, switch_provider_test_hook,
    update_settings, AppSettings, AppType, McpApps, McpServer, MultiAppConfig, Provider,
};

#[path = "support.rs"]
mod support;

use std::collections::HashMap;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn override_toggle_preserves_configured_paths_and_switches_effective_dir() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();

    let override_codex_dir = home.join("wsl").join(".codex");
    let override_codex_dir_str = override_codex_dir.to_string_lossy().to_string();

    let default_auth_path = home.join(".codex").join("auth.json");
    let override_auth_path = override_codex_dir.join("auth.json");

    let mut settings = AppSettings::default();
    settings.codex_config_dir = Some(override_codex_dir_str.clone());
    settings.enable_config_dir_overrides = false;
    settings.sync_provider_switch_to_both_config_dirs = false;
    update_settings(settings).expect("update settings");

    assert_eq!(
        get_codex_auth_path(),
        default_auth_path,
        "override disabled should make default codex dir effective"
    );

    let settings_path = home.join(".cc-switch").join("settings.json");
    let on_disk: serde_json::Value = read_json_file(&settings_path).expect("read settings.json");
    assert_eq!(
        on_disk.get("codexConfigDir").and_then(|v| v.as_str()),
        Some(override_codex_dir_str.as_str()),
        "settings.json should retain codexConfigDir"
    );
    assert_eq!(
        on_disk
            .get("enableConfigDirOverrides")
            .and_then(|v| v.as_bool()),
        Some(false),
        "settings.json should reflect overrides disabled"
    );

    let mut settings = AppSettings::default();
    settings.codex_config_dir = Some(override_codex_dir_str);
    settings.enable_config_dir_overrides = true;
    settings.sync_provider_switch_to_both_config_dirs = false;
    update_settings(settings).expect("re-enable overrides");

    assert_eq!(
        get_codex_auth_path(),
        override_auth_path,
        "override enabled should restore override codex dir effective"
    );
}

#[test]
fn codex_switch_respects_override_enable_and_syncs_to_both_dirs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let default_dir = home.join(".codex");
    let override_dir = home.join("wsl").join(".codex");
    std::fs::create_dir_all(&default_dir).expect("create default codex dir");
    std::fs::create_dir_all(&override_dir).expect("create override codex dir");

    let override_dir_str = override_dir.to_string_lossy().to_string();
    let mut settings = AppSettings::default();
    settings.codex_config_dir = Some(override_dir_str.clone());
    settings.enable_config_dir_overrides = false;
    settings.sync_provider_switch_to_both_config_dirs = true;
    update_settings(settings).expect("update settings");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "stale"},
                    "config": "stale-config"
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Latest".to_string(),
                json!({
                    "auth": {"OPENAI_API_KEY": "fresh-key"},
                    "config": r#"[mcp_servers.latest]
type = "stdio"
command = "say"
"#
                }),
                None,
            ),
        );
    }

    config.mcp.servers = Some(HashMap::new());
    config
        .mcp
        .servers
        .as_mut()
        .expect("mcp servers map")
        .insert(
            "echo-server".into(),
            McpServer {
                id: "echo-server".to_string(),
                name: "Echo Server".to_string(),
                server: json!({
                    "type": "stdio",
                    "command": "echo"
                }),
                apps: McpApps {
                    claude: false,
                    codex: true,
                    gemini: false,
                    opencode: false,
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            },
        );

    let app_state = create_test_state_with_config(&config).expect("create test state");

    switch_provider_test_hook(&app_state, AppType::Codex, "new-provider")
        .expect("switch provider should succeed");

    assert_eq!(
        get_codex_auth_path(),
        default_dir.join("auth.json"),
        "when overrides disabled, codex auth path should use default dir"
    );
    assert_eq!(
        get_codex_config_path(),
        default_dir.join("config.toml"),
        "when overrides disabled, codex config path should use default dir"
    );

    let default_auth: serde_json::Value =
        read_json_file(&default_dir.join("auth.json")).expect("read default auth.json");
    assert_eq!(
        default_auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
        Some("fresh-key"),
        "default auth.json should reflect switched provider"
    );
    let default_config =
        std::fs::read_to_string(default_dir.join("config.toml")).expect("read default config");
    assert!(
        default_config.contains("mcp_servers.latest"),
        "default config should contain provider config"
    );
    assert!(
        default_config.contains("mcp_servers.echo-server"),
        "default config should contain synced MCP server"
    );

    let override_auth: serde_json::Value =
        read_json_file(&override_dir.join("auth.json")).expect("read override auth.json");
    assert_eq!(
        override_auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
        Some("fresh-key"),
        "override auth.json should be synced"
    );
    let override_config =
        std::fs::read_to_string(override_dir.join("config.toml")).expect("read override config");
    assert!(
        override_config.contains("mcp_servers.latest"),
        "override config should contain provider config"
    );
    // MCP sync runs against the effective (primary) directory; secondary sync is best-effort for
    // the provider's live config only.

    let settings_path = home.join(".cc-switch").join("settings.json");
    let persisted: serde_json::Value =
        read_json_file(&settings_path).expect("read persisted settings.json");
    assert_eq!(
        persisted.get("codexConfigDir").and_then(|v| v.as_str()),
        Some(override_dir_str.as_str()),
        "stored override path should be preserved even when disabled"
    );
    assert_eq!(
        persisted
            .get("enableConfigDirOverrides")
            .and_then(|v| v.as_bool()),
        Some(false),
        "override enable flag should persist"
    );
    assert_eq!(
        persisted
            .get("syncProviderSwitchToBothConfigDirs")
            .and_then(|v| v.as_bool()),
        Some(true),
        "sync flag should persist"
    );
}
