use serde_json::json;
use std::fs;
use std::path::PathBuf;

use cc_switch_lib::{
    get_claude_settings_path, read_json_file, AppError, AppType, ConfigService, MultiAppConfig,
    Provider, ProviderMeta,
};

#[path = "support.rs"]
mod support;
use support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex,
};

#[test]
fn sync_claude_provider_writes_live_settings() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    let provider_config = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "test-key",
            "ANTHROPIC_BASE_URL": "https://api.test"
        },
        "ui": {
            "displayName": "Test Provider"
        }
    });

    let provider = Provider::with_id(
        "prov-1".to_string(),
        "Test Claude".to_string(),
        provider_config.clone(),
        None,
    );

    let manager = config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager");
    manager.providers.insert("prov-1".to_string(), provider);
    manager.current = "prov-1".to_string();

    ConfigService::sync_current_providers_to_live(&mut config).expect("sync live settings");

    let settings_path = get_claude_settings_path();
    assert!(
        settings_path.exists(),
        "live settings should be written to {}",
        settings_path.display()
    );

    let live_value: serde_json::Value = read_json_file(&settings_path).expect("read live file");
    assert_eq!(live_value, provider_config);

    // 确认 SSOT 中的供应商也同步了最新内容
    let updated = config
        .get_manager(&AppType::Claude)
        .and_then(|m| m.providers.get("prov-1"))
        .expect("provider in config");
    assert_eq!(updated.settings_config, provider_config);

    // 额外确认写入位置位于测试 HOME 下
    assert!(
        settings_path.starts_with(home),
        "settings path {settings_path:?} should reside under test HOME {home:?}"
    );
}

#[test]
fn sync_codex_provider_writes_auth_and_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();

    // 注意：v3.7.0 后 MCP 同步由 McpService 独立处理，不再通过 provider 切换触发
    // 此测试仅验证 auth.json 和 config.toml 基础配置的写入

    let provider_config = json!({
        "auth": {
            "OPENAI_API_KEY": "codex-key"
        },
        "config": r#"base_url = "https://codex.test""#
    });

    let provider = Provider::with_id(
        "codex-1".to_string(),
        "Codex Test".to_string(),
        provider_config.clone(),
        None,
    );

    let manager = config
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager");
    manager.providers.insert("codex-1".to_string(), provider);
    manager.current = "codex-1".to_string();

    ConfigService::sync_current_providers_to_live(&mut config).expect("sync codex live");

    let auth_path = cc_switch_lib::get_codex_auth_path();
    let config_path = cc_switch_lib::get_codex_config_path();

    assert!(
        auth_path.exists(),
        "auth.json should exist at {}",
        auth_path.display()
    );
    assert!(
        config_path.exists(),
        "config.toml should exist at {}",
        config_path.display()
    );

    let auth_value: serde_json::Value = read_json_file(&auth_path).expect("read auth");
    assert_eq!(
        auth_value,
        provider_config.get("auth").cloned().expect("auth object")
    );

    let toml_text = fs::read_to_string(&config_path).expect("read config.toml");
    // 验证基础配置正确写入
    assert!(
        toml_text.contains("base_url"),
        "config.toml should contain base_url from provider config"
    );

    // 当前供应商应同步最新 config 文本
    let manager = config.get_manager(&AppType::Codex).expect("codex manager");
    let synced = manager.providers.get("codex-1").expect("codex provider");
    let synced_cfg = synced
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config string");
    assert_eq!(synced_cfg, toml_text);
}

#[test]
fn sync_enabled_to_codex_writes_enabled_servers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    // 模拟 Codex 已安装/已初始化：存在 ~/.codex 目录
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }

    let mut config = MultiAppConfig::default();
    config.mcp.codex.servers.insert(
        "stdio-enabled".into(),
        json!({
            "id": "stdio-enabled",
            "enabled": true,
            "server": {
                "type": "stdio",
                "command": "echo",
                "args": ["ok"],
            }
        }),
    );

    cc_switch_lib::sync_enabled_to_codex(&config).expect("sync codex");

    assert!(path.exists(), "config.toml should be created");
    let text = fs::read_to_string(&path).expect("read config.toml");
    assert!(
        text.contains("mcp_servers") && text.contains("stdio-enabled"),
        "enabled servers should be serialized"
    );
}

#[test]
fn sync_enabled_to_codex_preserves_non_mcp_content_and_style() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    // 预置含有顶层注释与非 MCP 键的 config.toml
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    let seed = r#"# top-comment
title = "keep-me"

[profile]
mode = "dev"
"#;
    fs::write(&path, seed).expect("seed config.toml");

    // 启用一个 MCP 项，触发增量写入
    let mut config = MultiAppConfig::default();
    config.mcp.codex.servers.insert(
        "echo".into(),
        json!({
            "id": "echo",
            "enabled": true,
            "server": { "type": "stdio", "command": "echo" }
        }),
    );

    cc_switch_lib::sync_enabled_to_codex(&config).expect("sync codex");

    let text = fs::read_to_string(&path).expect("read config.toml");
    // 顶层注释与非 MCP 键应保留
    assert!(
        text.contains("# top-comment"),
        "top comment should be preserved"
    );
    assert!(
        text.contains("title = \"keep-me\""),
        "top key should be preserved"
    );
    assert!(
        text.contains("[profile]"),
        "non-MCP table should be preserved"
    );
    assert!(
        text.contains("mcp_servers"),
        "mcp_servers table should be present"
    );
    assert!(
        !text.contains("[mcp.servers]"),
        "invalid [mcp.servers] table should not appear"
    );
    assert!(
        text.contains("echo") && text.contains("command = \"echo\""),
        "echo server should be serialized"
    );
}

#[test]
fn sync_enabled_to_codex_migrates_erroneous_mcp_dot_servers_to_mcp_servers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    // 预置错误的 mcp.servers 风格（应迁移为顶层 mcp_servers）
    let seed = r#"[mcp]
  other = "keep"
  [mcp.servers]
"#;
    fs::write(&path, seed).expect("seed config.toml");

    let mut config = MultiAppConfig::default();
    config.mcp.codex.servers.insert(
        "echo".into(),
        json!({
            "id": "echo",
            "enabled": true,
            "server": { "type": "stdio", "command": "echo" }
        }),
    );

    cc_switch_lib::sync_enabled_to_codex(&config).expect("sync codex");
    let text = fs::read_to_string(&path).expect("read config.toml");
    // 应迁移到顶层 mcp_servers，并移除错误的 mcp.servers 表
    assert!(
        text.contains("mcp_servers"),
        "should migrate to mcp_servers table"
    );
    assert!(
        !text.contains("[mcp.servers]"),
        "invalid [mcp.servers] table should be removed"
    );
}

#[test]
fn sync_enabled_to_codex_removes_servers_when_none_enabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    fs::write(
        &path,
        r#"[mcp_servers]
disabled = { type = "stdio", command = "noop" }
"#,
    )
    .expect("seed config file");

    let config = MultiAppConfig::default(); // 无启用项
    cc_switch_lib::sync_enabled_to_codex(&config).expect("sync codex");

    let text = fs::read_to_string(&path).expect("read config.toml");
    assert!(
        !text.contains("mcp_servers") && !text.contains("servers"),
        "disabled entries should be removed from config.toml"
    );
}

#[test]
fn sync_enabled_to_codex_returns_error_on_invalid_toml() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    fs::write(&path, "invalid = [").expect("write invalid config");

    let mut config = MultiAppConfig::default();
    config.mcp.codex.servers.insert(
        "broken".into(),
        json!({
            "id": "broken",
            "enabled": true,
            "server": {
                "type": "stdio",
                "command": "echo"
            }
        }),
    );

    let err = cc_switch_lib::sync_enabled_to_codex(&config).expect_err("sync should fail");
    match err {
        cc_switch_lib::AppError::Toml { path, .. } => {
            assert!(
                path.ends_with("config.toml"),
                "path should reference config.toml"
            );
        }
        cc_switch_lib::AppError::McpValidation(msg) => {
            assert!(
                msg.contains("config.toml"),
                "error message should mention config.toml"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn sync_codex_provider_missing_auth_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let mut config = MultiAppConfig::default();
    let provider = Provider::with_id(
        "codex-missing-auth".to_string(),
        "No Auth".to_string(),
        json!({
            "config": "model = \"test\""
        }),
        None,
    );
    let manager = config
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager");
    manager.providers.insert(provider.id.clone(), provider);
    manager.current = "codex-missing-auth".to_string();

    let err = ConfigService::sync_current_providers_to_live(&mut config)
        .expect_err("sync should fail when auth missing");
    match err {
        cc_switch_lib::AppError::Config(msg) => {
            assert!(msg.contains("auth"), "error message should mention auth");
        }
        other => panic!("unexpected error variant: {other:?}"),
    }

    // 确认未产生任何 live 配置文件
    assert!(
        !cc_switch_lib::get_codex_auth_path().exists(),
        "auth.json should not be created on failure"
    );
    assert!(
        !cc_switch_lib::get_codex_config_path().exists(),
        "config.toml should not be created on failure"
    );
}

#[test]
fn write_codex_live_atomic_persists_auth_and_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let auth = json!({ "OPENAI_API_KEY": "dev-key" });
    let config_text = r#"
[mcp_servers.echo]
type = "stdio"
command = "echo"
args = ["ok"]
"#;

    cc_switch_lib::write_codex_live_atomic(&auth, Some(config_text))
        .expect("atomic write should succeed");

    let auth_path = cc_switch_lib::get_codex_auth_path();
    let config_path = cc_switch_lib::get_codex_config_path();
    assert!(auth_path.exists(), "auth.json should be created");
    assert!(config_path.exists(), "config.toml should be created");

    let stored_auth: serde_json::Value =
        cc_switch_lib::read_json_file(&auth_path).expect("read auth");
    assert_eq!(stored_auth, auth, "auth.json should match input");

    let stored_config = std::fs::read_to_string(&config_path).expect("read config");
    assert!(
        stored_config.contains("mcp_servers.echo"),
        "config.toml should contain serialized table"
    );
}

#[test]
fn write_codex_live_atomic_rolls_back_auth_when_config_write_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let auth_path = cc_switch_lib::get_codex_auth_path();
    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).expect("create codex dir");
    }
    std::fs::write(&auth_path, r#"{"OPENAI_API_KEY":"legacy"}"#).expect("seed auth");

    let config_path = cc_switch_lib::get_codex_config_path();
    std::fs::create_dir_all(&config_path).expect("create blocking directory");

    let auth = json!({ "OPENAI_API_KEY": "new-key" });
    let config_text = r#"[mcp_servers.sample]
type = "stdio"
command = "noop"
"#;

    let err = cc_switch_lib::write_codex_live_atomic(&auth, Some(config_text))
        .expect_err("config write should fail when target is directory");
    match err {
        cc_switch_lib::AppError::Io { path, .. } => {
            assert!(
                path.ends_with("config.toml"),
                "io error path should point to config.toml"
            );
        }
        cc_switch_lib::AppError::IoContext { context, .. } => {
            assert!(
                context.contains("config.toml"),
                "error context should mention config path"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }

    let stored = std::fs::read_to_string(&auth_path).expect("read existing auth");
    assert!(
        stored.contains("legacy"),
        "auth.json should roll back to legacy content"
    );
    assert!(
        std::fs::metadata(&config_path)
            .expect("config path metadata")
            .is_dir(),
        "config path should remain a directory after failure"
    );
}

#[test]
fn import_from_codex_adds_servers_from_mcp_servers_table() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    fs::write(
        &path,
        r#"[mcp_servers.echo_server]
type = "stdio"
command = "echo"
args = ["hello"]

[mcp_servers.http_server]
type = "http"
url = "https://example.com"
"#,
    )
    .expect("write codex config");

    let mut config = MultiAppConfig::default();
    let changed = cc_switch_lib::import_from_codex(&mut config).expect("import codex");
    assert!(changed >= 2, "should import both servers");

    // v3.7.0: 检查统一结构
    let servers = config
        .mcp
        .servers
        .as_ref()
        .expect("unified servers should exist");

    let echo = servers.get("echo_server").expect("echo server");
    assert!(
        echo.apps.codex,
        "Codex app should be enabled for echo_server"
    );
    let server_spec = echo.server.as_object().expect("server spec");
    assert_eq!(
        server_spec
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "echo"
    );

    let http = servers.get("http_server").expect("http server");
    assert!(
        http.apps.codex,
        "Codex app should be enabled for http_server"
    );
    let http_spec = http.server.as_object().expect("http spec");
    assert_eq!(
        http_spec.get("url").and_then(|v| v.as_str()).unwrap_or(""),
        "https://example.com"
    );
}

#[test]
fn import_from_codex_merges_into_existing_entries() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let path = cc_switch_lib::get_codex_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create codex dir");
    }
    fs::write(
        &path,
        r#"[mcp.servers.existing]
type = "stdio"
command = "echo"
"#,
    )
    .expect("write codex config");

    let mut config = MultiAppConfig::default();
    // v3.7.0: 在统一结构中创建已存在的服务器
    config.mcp.servers = Some(std::collections::HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
        "existing".to_string(),
        cc_switch_lib::McpServer {
            id: "existing".to_string(),
            name: "existing".to_string(),
            server: json!({
                "type": "stdio",
                "command": "prev"
            }),
            apps: cc_switch_lib::McpApps {
                claude: false,
                codex: false, // 初始未启用
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let changed = cc_switch_lib::import_from_codex(&mut config).expect("import codex");
    assert!(changed >= 1, "should mark change for enabled flag");

    // v3.7.0: 检查统一结构
    let entry = config
        .mcp
        .servers
        .as_ref()
        .unwrap()
        .get("existing")
        .expect("existing entry");

    // 验证 Codex 应用已启用
    assert!(entry.apps.codex, "Codex app should be enabled after import");

    // 验证现有配置被保留（server 不应被覆盖）
    let spec = entry.server.as_object().expect("server spec");
    assert_eq!(
        spec.get("command").and_then(|v| v.as_str()),
        Some("prev"),
        "existing server config should be preserved, not overwritten by import"
    );
}

#[test]
fn sync_claude_enabled_mcp_projects_to_user_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 模拟 Claude 已安装/已初始化：存在 ~/.claude 目录
    fs::create_dir_all(home.join(".claude")).expect("create claude dir");

    let mut config = MultiAppConfig::default();

    config.mcp.claude.servers.insert(
        "stdio-enabled".into(),
        json!({
            "id": "stdio-enabled",
            "enabled": true,
            "server": {
                "type": "stdio",
                "command": "echo",
                "args": ["hi"],
            }
        }),
    );
    config.mcp.claude.servers.insert(
        "http-disabled".into(),
        json!({
            "id": "http-disabled",
            "enabled": false,
            "server": {
                "type": "http",
                "url": "https://example.com",
            }
        }),
    );

    cc_switch_lib::sync_enabled_to_claude(&config).expect("sync Claude MCP");

    let claude_path = cc_switch_lib::get_claude_mcp_path();
    assert!(claude_path.exists(), "claude config should exist");
    let text = fs::read_to_string(&claude_path).expect("read .claude.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse claude json");
    let servers = value
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .expect("mcpServers map");
    assert_eq!(servers.len(), 1, "only enabled entries should be written");
    let enabled = servers.get("stdio-enabled").expect("enabled entry");
    assert_eq!(
        enabled
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "echo"
    );
    assert!(servers.get("http-disabled").is_none());
}

#[test]
fn import_from_claude_merges_into_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let claude_path = home.join(".claude.json");

    fs::write(
        &claude_path,
        serde_json::to_string_pretty(&json!({
            "mcpServers": {
                "stdio-enabled": {
                    "type": "stdio",
                    "command": "echo",
                    "args": ["hello"]
                }
            }
        }))
        .unwrap(),
    )
    .expect("write claude json");

    let mut config = MultiAppConfig::default();
    // v3.7.0: 在统一结构中创建已存在的服务器
    config.mcp.servers = Some(std::collections::HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
        "stdio-enabled".to_string(),
        cc_switch_lib::McpServer {
            id: "stdio-enabled".to_string(),
            name: "stdio-enabled".to_string(),
            server: json!({
                "type": "stdio",
                "command": "prev"
            }),
            apps: cc_switch_lib::McpApps {
                claude: false, // 初始未启用
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let changed = cc_switch_lib::import_from_claude(&mut config).expect("import from claude");
    assert!(changed >= 1, "should mark at least one change");

    // v3.7.0: 检查统一结构
    let entry = config
        .mcp
        .servers
        .as_ref()
        .unwrap()
        .get("stdio-enabled")
        .expect("entry exists");

    // 验证 Claude 应用已启用
    assert!(
        entry.apps.claude,
        "Claude app should be enabled after import"
    );

    // 验证现有配置被保留（server 不应被覆盖）
    let server = entry.server.as_object().expect("server obj");
    assert_eq!(
        server.get("command").and_then(|v| v.as_str()).unwrap_or(""),
        "prev",
        "existing server config should be preserved"
    );
}

#[test]
fn create_backup_skips_missing_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_path = home.join(".cc-switch").join("config.json");

    // 未创建文件时应返回空字符串，不报错
    let result = ConfigService::create_backup(&config_path).expect("create backup");
    assert!(
        result.is_empty(),
        "expected empty backup id when config file missing"
    );
}

#[test]
fn create_backup_generates_snapshot_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_dir = home.join(".cc-switch");
    let config_path = config_dir.join("config.json");
    fs::create_dir_all(&config_dir).expect("prepare config dir");
    fs::write(&config_path, r#"{"version":2}"#).expect("write config file");

    let backup_id = ConfigService::create_backup(&config_path).expect("backup success");
    assert!(
        !backup_id.is_empty(),
        "backup id should contain timestamp information"
    );

    let backup_path = config_dir.join("backups").join(format!("{backup_id}.json"));
    assert!(
        backup_path.exists(),
        "expected backup file at {}",
        backup_path.display()
    );

    let backup_content = fs::read_to_string(&backup_path).expect("read backup");
    assert!(
        backup_content.contains(r#""version":2"#),
        "backup content should match original config"
    );
}

#[test]
fn create_backup_retains_only_latest_entries() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_dir = home.join(".cc-switch");
    let config_path = config_dir.join("config.json");
    fs::create_dir_all(&config_dir).expect("prepare config dir");
    fs::write(&config_path, r#"{"version":3}"#).expect("write config file");

    let backups_dir = config_dir.join("backups");
    fs::create_dir_all(&backups_dir).expect("create backups dir");
    for idx in 0..12 {
        let manual = backups_dir.join(format!("manual_{idx:02}.json"));
        fs::write(&manual, format!("{{\"idx\":{idx}}}")).expect("seed manual backup");
    }

    std::thread::sleep(std::time::Duration::from_secs(1));

    let latest_backup_id =
        ConfigService::create_backup(&config_path).expect("create backup with cleanup");
    assert!(
        !latest_backup_id.is_empty(),
        "backup id should not be empty when config exists"
    );

    let entries: Vec<_> = fs::read_dir(&backups_dir)
        .expect("read backups dir")
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(
        entries.len() <= 10,
        "expected backups to be trimmed to at most 10 files, got {}",
        entries.len()
    );

    let latest_path = backups_dir.join(format!("{latest_backup_id}.json"));
    assert!(
        latest_path.exists(),
        "latest backup {} should be preserved",
        latest_path.display()
    );

    // 进一步确认保留的条目包含一些历史文件，说明清理逻辑仅裁剪多余部分
    let manual_kept = entries
        .iter()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with("manual_"));
    assert!(
        manual_kept,
        "cleanup should keep part of the older backups to maintain history"
    );
}

#[test]
fn sync_gemini_packycode_sets_security_selected_type() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "packy-1".to_string();
        manager.providers.insert(
            "packy-1".to_string(),
            Provider::with_id(
                "packy-1".to_string(),
                "PackyCode".to_string(),
                json!({
                    "env": {
                        "GEMINI_API_KEY": "pk-key",
                        "GOOGLE_GEMINI_BASE_URL": "https://api-slb.packyapi.com"
                    }
                }),
                Some("https://www.packyapi.com".to_string()),
            ),
        );
    }

    ConfigService::sync_current_providers_to_live(&mut config)
        .expect("syncing gemini live should succeed");

    // security field is written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let gemini_settings = home.join(".gemini").join("settings.json");
    assert!(
        gemini_settings.exists(),
        "Gemini settings.json should exist at {}",
        gemini_settings.display()
    );

    let raw = std::fs::read_to_string(&gemini_settings).expect("read gemini settings.json");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse gemini settings.json");
    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key"),
        "syncing PackyCode Gemini should enforce security.auth.selectedType in Gemini settings"
    );
}

#[test]
fn sync_gemini_google_official_sets_oauth_security() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Gemini)
            .expect("gemini manager");
        manager.current = "google-official".to_string();
        let mut provider = Provider::with_id(
            "google-official".to_string(),
            "Google".to_string(),
            json!({
                "env": {}
            }),
            Some("https://ai.google.dev".to_string()),
        );
        provider.meta = Some(ProviderMeta {
            partner_promotion_key: Some("google-official".to_string()),
            ..ProviderMeta::default()
        });
        manager
            .providers
            .insert("google-official".to_string(), provider);
    }

    ConfigService::sync_current_providers_to_live(&mut config)
        .expect("syncing google official gemini should succeed");

    // security field is written to ~/.gemini/settings.json, not ~/.cc-switch/settings.json
    let gemini_settings = home.join(".gemini").join("settings.json");
    assert!(
        gemini_settings.exists(),
        "Gemini settings should exist at {}",
        gemini_settings.display()
    );
    let gemini_raw = std::fs::read_to_string(&gemini_settings).expect("read gemini settings");
    let gemini_value: serde_json::Value =
        serde_json::from_str(&gemini_raw).expect("parse gemini settings json");
    assert_eq!(
        gemini_value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("oauth-personal"),
        "Gemini settings should record oauth-personal for Google Official"
    );
}

#[test]
fn export_sql_writes_to_target_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // Create test state with some data
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "test-provider".to_string();
        manager.providers.insert(
            "test-provider".to_string(),
            Provider::with_id(
                "test-provider".to_string(),
                "Test Provider".to_string(),
                json!({"env": {"ANTHROPIC_API_KEY": "test-key"}}),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    // Export to SQL file
    let export_path = home.join("test-export.sql");
    state
        .db
        .export_sql(&export_path)
        .expect("export should succeed");

    // Verify file exists and contains data
    assert!(export_path.exists(), "export file should exist");
    let content = fs::read_to_string(&export_path).expect("read exported file");
    assert!(
        content.contains("INSERT INTO") && content.contains("providers"),
        "exported SQL should contain INSERT statements for providers"
    );
    assert!(
        content.contains("test-provider"),
        "exported SQL should contain test data"
    );
}

#[test]
fn export_sql_returns_error_for_invalid_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    // Try to export to an invalid path (parent directory doesn't exist)
    let invalid_path = PathBuf::from("/nonexistent/directory/export.sql");
    let err = state
        .db
        .export_sql(&invalid_path)
        .expect_err("export to invalid path should fail");

    // The error can be either IoContext or Io depending on where it fails
    match err {
        AppError::IoContext { context, .. } => {
            assert!(
                context.contains("原子写入失败") || context.contains("写入失败"),
                "expected IO error message about atomic write failure, got: {context}"
            );
        }
        AppError::Io { path, .. } => {
            assert!(
                path.starts_with("/nonexistent"),
                "expected error for /nonexistent path, got: {path:?}"
            );
        }
        other => panic!("expected IoContext or Io error, got {other:?}"),
    }
}

#[test]
fn import_sql_rejects_non_cc_switch_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let import_path = home.join("not-cc-switch.sql");
    fs::write(&import_path, "CREATE TABLE x (id INTEGER);").expect("write import sql");

    let err = state
        .db
        .import_sql(&import_path)
        .expect_err("non-cc-switch sql should be rejected");

    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "backup.sql.invalid_format");
        }
        other => panic!("expected Localized error, got {other:?}"),
    }
}

#[test]
fn import_sql_accepts_cc_switch_exported_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // Create a database with some data and export it.
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "test-provider".to_string();
        manager.providers.insert(
            "test-provider".to_string(),
            Provider::with_id(
                "test-provider".to_string(),
                "Test Provider".to_string(),
                json!({"env": {"ANTHROPIC_API_KEY": "test-key"}}),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    let export_path = home.join("cc-switch-export.sql");
    state
        .db
        .export_sql(&export_path)
        .expect("export should succeed");

    // Reset database, then import into a fresh one.
    reset_test_fs();
    let state = create_test_state().expect("create test state");
    state
        .db
        .import_sql(&export_path)
        .expect("import should succeed");

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("load providers");
    assert!(
        providers.contains_key("test-provider"),
        "imported providers should contain test-provider"
    );
}
