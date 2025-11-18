use std::{collections::HashMap, fs, sync::RwLock};

use serde_json::json;

use cc_switch_lib::{
    get_claude_mcp_path, get_claude_settings_path, import_default_config_test_hook, AppError,
    AppState, AppType, McpApps, McpServer, McpService, MultiAppConfig,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn import_default_config_claude_persists_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let settings_path = get_claude_settings_path();
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).expect("create claude settings dir");
    }
    let settings = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "test-key",
            "ANTHROPIC_BASE_URL": "https://api.test"
        }
    });
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).expect("serialize settings"),
    )
    .expect("seed claude settings.json");

    let mut config = MultiAppConfig::default();
    config.ensure_app(&AppType::Claude);
    let state = AppState {
        config: RwLock::new(config),
    };

    import_default_config_test_hook(&state, AppType::Claude)
        .expect("import default config succeeds");

    // 验证内存状态
    let guard = state.config.read().expect("lock config");
    let manager = guard
        .get_manager(&AppType::Claude)
        .expect("claude manager present");
    assert_eq!(manager.current, "default");
    let default_provider = manager.providers.get("default").expect("default provider");
    assert_eq!(
        default_provider.settings_config, settings,
        "default provider should capture live settings"
    );
    drop(guard);

    // 验证配置已持久化
    let config_path = home.join(".cc-switch").join("config.json");
    assert!(
        config_path.exists(),
        "importing default config should persist config.json"
    );
}

#[test]
fn import_default_config_without_live_file_returns_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let state = AppState {
        config: RwLock::new(MultiAppConfig::default()),
    };

    let err = import_default_config_test_hook(&state, AppType::Claude)
        .expect_err("missing live file should error");
    match err {
        AppError::Localized { zh, .. } => assert!(
            zh.contains("Claude Code 配置文件不存在"),
            "unexpected error message: {zh}"
        ),
        AppError::Message(msg) => assert!(
            msg.contains("Claude Code 配置文件不存在"),
            "unexpected error message: {msg}"
        ),
        other => panic!("unexpected error variant: {other:?}"),
    }

    let config_path = home.join(".cc-switch").join("config.json");
    assert!(
        !config_path.exists(),
        "failed import should not create config.json"
    );
}

#[test]
fn import_mcp_from_claude_creates_config_and_enables_servers() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mcp_path = get_claude_mcp_path();
    let claude_json = json!({
        "mcpServers": {
            "echo": {
                "type": "stdio",
                "command": "echo"
            }
        }
    });
    fs::write(
        &mcp_path,
        serde_json::to_string_pretty(&claude_json).expect("serialize claude mcp"),
    )
    .expect("seed ~/.claude.json");

    let state = AppState {
        config: RwLock::new(MultiAppConfig::default()),
    };

    let changed = McpService::import_from_claude(&state).expect("import mcp from claude succeeds");
    assert!(
        changed > 0,
        "import should report inserted or normalized entries"
    );

    let guard = state.config.read().expect("lock config");
    // v3.7.0: 检查统一结构
    let servers = guard
        .mcp
        .servers
        .as_ref()
        .expect("unified servers should exist");
    let entry = servers
        .get("echo")
        .expect("server imported into unified structure");
    assert!(
        entry.apps.claude,
        "imported server should have Claude app enabled"
    );
    drop(guard);

    let config_path = home.join(".cc-switch").join("config.json");
    assert!(
        config_path.exists(),
        "state.save should persist config.json when changes detected"
    );
}

#[test]
fn import_mcp_from_claude_invalid_json_preserves_state() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mcp_path = get_claude_mcp_path();
    fs::write(&mcp_path, "{\"mcpServers\":") // 不完整 JSON
        .expect("seed invalid ~/.claude.json");

    let state = AppState {
        config: RwLock::new(MultiAppConfig::default()),
    };

    let err =
        McpService::import_from_claude(&state).expect_err("invalid json should bubble up error");
    match err {
        AppError::McpValidation(msg) => assert!(
            msg.contains("解析 ~/.claude.json 失败"),
            "unexpected error message: {msg}"
        ),
        other => panic!("unexpected error variant: {other:?}"),
    }

    let config_path = home.join(".cc-switch").join("config.json");
    assert!(
        !config_path.exists(),
        "failed import should not persist config.json"
    );
}

#[test]
fn set_mcp_enabled_for_codex_writes_live_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 创建 Codex 配置目录和文件
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).expect("create codex dir");
    fs::write(
        codex_dir.join("auth.json"),
        r#"{"OPENAI_API_KEY":"test-key"}"#,
    )
    .expect("create auth.json");
    fs::write(codex_dir.join("config.toml"), "").expect("create empty config.toml");

    let mut config = MultiAppConfig::default();
    config.ensure_app(&AppType::Codex);

    // v3.7.0: 使用统一结构
    config.mcp.servers = Some(HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
        "codex-server".into(),
        McpServer {
            id: "codex-server".to_string(),
            name: "Codex Server".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
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

    let state = AppState {
        config: RwLock::new(config),
    };

    // v3.7.0: 使用 toggle_app 替代 set_enabled
    McpService::toggle_app(&state, "codex-server", AppType::Codex, true)
        .expect("toggle_app should succeed");

    let guard = state.config.read().expect("lock config");
    let entry = guard
        .mcp
        .servers
        .as_ref()
        .unwrap()
        .get("codex-server")
        .expect("codex server exists");
    assert!(
        entry.apps.codex,
        "server should have Codex app enabled after toggle"
    );
    drop(guard);

    let toml_path = cc_switch_lib::get_codex_config_path();
    assert!(
        toml_path.exists(),
        "enabling server should trigger sync to ~/.codex/config.toml"
    );
    let toml_text = fs::read_to_string(&toml_path).expect("read codex config");
    assert!(
        toml_text.contains("codex-server"),
        "codex config should include the enabled server definition"
    );
}
