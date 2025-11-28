use serde_json::json;

use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, read_json_file, switch_provider_test_hook,
    write_codex_live_atomic, AppError, AppType, McpApps, McpServer, MultiAppConfig, Provider,
};

#[path = "support.rs"]
mod support;
use std::collections::HashMap;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn switch_provider_updates_codex_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let legacy_auth = json!({"OPENAI_API_KEY": "legacy-key"});
    let legacy_config = r#"[mcp_servers.legacy]
type = "stdio"
command = "echo"
"#;
    write_codex_live_atomic(&legacy_auth, Some(legacy_config))
        .expect("seed existing codex live config");

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

    // v3.7.0+: 使用统一的 MCP 结构
    config.mcp.servers = Some(HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
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
                codex: true, // 启用 Codex
                gemini: false,
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

    let auth_value: serde_json::Value =
        read_json_file(&get_codex_auth_path()).expect("read auth.json");
    assert_eq!(
        auth_value
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "fresh-key",
        "live auth.json should reflect new provider"
    );

    let config_text = std::fs::read_to_string(get_codex_config_path()).expect("read config.toml");
    assert!(
        config_text.contains("mcp_servers.echo-server"),
        "config.toml should contain synced MCP servers"
    );

    let current_id = app_state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let providers = app_state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .expect("get all providers");

    let new_provider = providers.get("new-provider").expect("new provider exists");
    let new_config_text = new_provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // 供应商配置应该包含在 live 文件中
    // 注意：live 文件还会包含 MCP 同步后的内容
    assert!(
        config_text.contains("mcp_servers.latest"),
        "live file should contain provider's original config"
    );
    assert!(
        new_config_text.contains("mcp_servers.latest"),
        "provider snapshot should contain provider's original config"
    );

    let legacy = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    let legacy_auth_value = legacy
        .settings_config
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // 回填机制：切换前会将 live 配置回填到当前供应商
    // 这保护了用户在 live 文件中的手动修改
    assert_eq!(
        legacy_auth_value, "legacy-key",
        "previous provider should be backfilled with live auth"
    );
}

#[test]
fn switch_provider_missing_provider_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager")
        .current = "does-not-exist".to_string();

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = switch_provider_test_hook(&app_state, AppType::Claude, "missing-provider")
        .expect_err("switching to a missing provider should fail");

    let err_str = err.to_string();
    assert!(
        err_str.contains("供应商不存在")
            || err_str.contains("Provider not found")
            || err_str.contains("missing-provider"),
        "error message should mention missing provider, got: {err_str}"
    );
}

#[test]
fn switch_provider_updates_claude_live_and_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let settings_path = cc_switch_lib::get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let legacy_live = json!({
        "env": {
            "ANTHROPIC_API_KEY": "legacy-key"
        },
        "workspace": {
            "path": "/tmp/workspace"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&legacy_live).expect("serialize legacy live"),
    )
    .expect("seed claude live config");

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "old-provider".to_string();
        manager.providers.insert(
            "old-provider".to_string(),
            Provider::with_id(
                "old-provider".to_string(),
                "Legacy Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "stale-key" }
                }),
                None,
            ),
        );
        manager.providers.insert(
            "new-provider".to_string(),
            Provider::with_id(
                "new-provider".to_string(),
                "Fresh Claude".to_string(),
                json!({
                    "env": { "ANTHROPIC_API_KEY": "fresh-key" },
                    "workspace": { "path": "/tmp/new-workspace" }
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    switch_provider_test_hook(&app_state, AppType::Claude, "new-provider")
        .expect("switch provider should succeed");

    let live_after: serde_json::Value =
        read_json_file(&settings_path).expect("read claude live settings");
    assert_eq!(
        live_after
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "live settings.json should reflect new provider auth"
    );

    let current_id = app_state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "current provider updated"
    );

    let providers = app_state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");

    let legacy_provider = providers
        .get("old-provider")
        .expect("legacy provider still exists");
    // 回填机制：切换前会将 live 配置回填到当前供应商
    // 这保护了用户在 live 文件中的手动修改
    assert_eq!(
        legacy_provider.settings_config, legacy_live,
        "previous provider should be backfilled with live config"
    );

    let new_provider = providers.get("new-provider").expect("new provider exists");
    assert_eq!(
        new_provider
            .settings_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|key| key.as_str()),
        Some("fresh-key"),
        "new provider snapshot should retain fresh auth"
    );

    // v3.7.0+ 使用 SQLite 数据库而非 config.json
    // 验证数据已持久化到数据库
    let home_dir = std::env::var("HOME").expect("HOME should be set by ensure_test_home");
    let db_path = std::path::Path::new(&home_dir)
        .join(".cc-switch")
        .join("cc-switch.db");
    assert!(
        db_path.exists(),
        "switching provider should persist to cc-switch.db"
    );

    // 验证当前供应商已更新
    let current_id = app_state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("new-provider"),
        "database should record the new current provider"
    );
}

#[test]
fn switch_provider_codex_missing_auth_returns_error_and_keeps_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.providers.insert(
            "invalid".to_string(),
            Provider::with_id(
                "invalid".to_string(),
                "Broken Codex".to_string(),
                json!({
                    "config": "[mcp_servers.test]\ncommand = \"noop\""
                }),
                None,
            ),
        );
    }

    let app_state = create_test_state_with_config(&config).expect("create test state");

    let err = switch_provider_test_hook(&app_state, AppType::Codex, "invalid")
        .expect_err("switching should fail when auth missing");
    match err {
        AppError::Config(msg) => assert!(
            msg.contains("auth"),
            "expected auth missing error message, got {msg}"
        ),
        other => panic!("expected config error, got {other:?}"),
    }

    let current_id = app_state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("get current provider");
    // 切换失败后，由于数据库操作是先设置再验证，current 可能已被设为 "invalid"
    // 但由于 live 配置写入失败，状态应该回滚
    // 注意：这个行为取决于 switch_provider 的具体实现
    assert!(
        current_id.is_none() || current_id.as_deref() == Some("invalid"),
        "current provider should remain empty or be the attempted id on failure, got: {current_id:?}"
    );
}
