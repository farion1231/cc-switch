use std::collections::HashMap;
use std::fs;

use serde_json::json;

use cc_switch_lib::{
    get_claude_mcp_path, get_claude_settings_path, import_default_config_test_hook, AppError,
    AppType, McpApps, McpServer, McpService, MultiAppConfig,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex};

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
    let state = create_test_state_with_config(&config).expect("create test state");

    import_default_config_test_hook(&state, AppType::Claude)
        .expect("import default config succeeds");

    // 验证内存状态
    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    let current_id = state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(current_id.as_deref(), Some("default"));
    let default_provider = providers.get("default").expect("default provider");
    assert_eq!(
        default_provider.settings_config, settings,
        "default provider should capture live settings"
    );

    // 验证数据已持久化到数据库（v3.7.0+ 使用 SQLite 而非 config.json）
    let db_path = home.join(".cc-switch").join("cc-switch.db");
    assert!(
        db_path.exists(),
        "importing default config should persist to cc-switch.db"
    );
}

#[test]
fn import_default_config_without_live_file_returns_error() {
    use support::create_test_state;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

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

    // 使用数据库架构，不再检查 config.json
    // 失败的导入不应该向数据库写入任何供应商
    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("get all providers");
    assert!(
        providers.is_empty(),
        "failed import should not create any providers in database"
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

    let config = MultiAppConfig::default();
    let state = create_test_state_with_config(&config).expect("create test state");

    let changed = McpService::import_from_claude(&state).expect("import mcp from claude succeeds");
    assert!(
        changed > 0,
        "import should report inserted or normalized entries"
    );

    let servers = state.db.get_all_mcp_servers().expect("get all mcp servers");
    let entry = servers
        .get("echo")
        .expect("server imported into unified structure");
    assert!(
        entry.apps.claude,
        "imported server should have Claude app enabled"
    );

    // 验证数据已持久化到数据库
    let db_path = home.join(".cc-switch").join("cc-switch.db");
    assert!(
        db_path.exists(),
        "state.save should persist to cc-switch.db when changes detected"
    );
}

#[test]
fn import_mcp_from_claude_invalid_json_preserves_state() {
    use support::create_test_state;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let mcp_path = get_claude_mcp_path();
    fs::write(&mcp_path, "{\"mcpServers\":") // 不完整 JSON
        .expect("seed invalid ~/.claude.json");

    let state = create_test_state().expect("create test state");

    let err =
        McpService::import_from_claude(&state).expect_err("invalid json should bubble up error");
    match err {
        AppError::McpValidation(msg) => assert!(
            msg.contains("解析 ~/.claude.json 失败"),
            "unexpected error message: {msg}"
        ),
        other => panic!("unexpected error variant: {other:?}"),
    }

    // 使用数据库架构，检查 MCP 服务器未被写入
    let servers = state.db.get_all_mcp_servers().expect("get all mcp servers");
    assert!(
        servers.is_empty(),
        "failed import should not persist any MCP servers to database"
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

    let state = create_test_state_with_config(&config).expect("create test state");

    // v3.7.0: 使用 toggle_app 替代 set_enabled
    McpService::toggle_app(&state, "codex-server", AppType::Codex, true)
        .expect("toggle_app should succeed");

    let servers = state.db.get_all_mcp_servers().expect("get all mcp servers");
    let entry = servers.get("codex-server").expect("codex server exists");
    assert!(
        entry.apps.codex,
        "server should have Codex app enabled after toggle"
    );

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

#[test]
fn enabling_codex_mcp_skips_when_codex_dir_missing() {
    use support::create_test_state;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 确认 Codex 配置目录不存在（模拟“未安装/未运行过 Codex CLI”）
    assert!(
        !home.join(".codex").exists(),
        "~/.codex should not exist in fresh test environment"
    );

    let state = create_test_state().expect("create test state");

    // 先插入一个未启用 Codex 的 MCP 服务器（避免 upsert 触发同步）
    McpService::upsert_server(
        &state,
        McpServer {
            id: "codex-server".to_string(),
            name: "Codex Server".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )
    .expect("insert server without syncing");

    // 启用 Codex：目录缺失时应跳过写入（不创建 ~/.codex/config.toml）
    McpService::toggle_app(&state, "codex-server", AppType::Codex, true)
        .expect("toggle codex should succeed even when ~/.codex is missing");

    assert!(
        !home.join(".codex").exists(),
        "~/.codex should still not exist after skipped sync"
    );
}

#[test]
fn upsert_mcp_server_disabling_app_removes_from_claude_live_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 模拟 Claude 已安装/已初始化：存在 ~/.claude 目录
    fs::create_dir_all(home.join(".claude")).expect("create ~/.claude dir");

    // 先创建一个启用 Claude 的 MCP 服务器
    let state = support::create_test_state().expect("create test state");
    McpService::upsert_server(
        &state,
        McpServer {
            id: "echo".to_string(),
            name: "echo".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: true,
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )
    .expect("upsert should sync to Claude live config");

    // 确认已写入 ~/.claude.json
    let mcp_path = get_claude_mcp_path();
    let text = fs::read_to_string(&mcp_path).expect("read ~/.claude.json");
    let v: serde_json::Value = serde_json::from_str(&text).expect("parse ~/.claude.json");
    assert!(
        v.pointer("/mcpServers/echo").is_some(),
        "echo should exist in Claude live config after enabling"
    );

    // 再次 upsert：取消勾选 Claude（apps.claude=false），应从 Claude live 配置中移除
    McpService::upsert_server(
        &state,
        McpServer {
            id: "echo".to_string(),
            name: "echo".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )
    .expect("upsert disabling app should remove from Claude live config");

    let text = fs::read_to_string(&mcp_path).expect("read ~/.claude.json after disable");
    let v: serde_json::Value = serde_json::from_str(&text).expect("parse ~/.claude.json");
    assert!(
        v.pointer("/mcpServers/echo").is_none(),
        "echo should be removed from Claude live config after disabling"
    );
}

#[test]
fn import_mcp_from_multiple_apps_merges_enabled_flags() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 1) Claude: ~/.claude.json
    let mcp_path = get_claude_mcp_path();
    let claude_json = json!({
        "mcpServers": {
            "shared": {
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

    // 2) Codex: ~/.codex/config.toml
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).expect("create codex dir");
    fs::write(
        codex_dir.join("config.toml"),
        r#"[mcp_servers.shared]
type = "stdio"
command = "echo"
"#,
    )
    .expect("seed ~/.codex/config.toml");

    let state = support::create_test_state().expect("create test state");

    McpService::import_from_claude(&state).expect("import from claude");
    McpService::import_from_codex(&state).expect("import from codex");

    let servers = state.db.get_all_mcp_servers().expect("get all mcp servers");
    let entry = servers.get("shared").expect("shared server exists");
    assert!(entry.apps.claude, "shared should enable Claude");
    assert!(entry.apps.codex, "shared should enable Codex");
}

#[test]
fn import_mcp_from_gemini_sse_url_only_is_valid() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // Gemini MCP 位于 ~/.gemini/settings.json
    let gemini_dir = home.join(".gemini");
    fs::create_dir_all(&gemini_dir).expect("create gemini dir");
    let settings_path = gemini_dir.join("settings.json");

    // Gemini SSE：只包含 url（Gemini 不使用 type 字段）
    let gemini_settings = json!({
        "mcpServers": {
            "sse-server": {
                "url": "https://example.com/sse"
            }
        }
    });
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&gemini_settings).expect("serialize gemini settings"),
    )
    .expect("seed ~/.gemini/settings.json");

    let state = support::create_test_state().expect("create test state");
    let changed = McpService::import_from_gemini(&state).expect("import from gemini");
    assert!(changed > 0, "should import at least 1 server");

    let servers = state.db.get_all_mcp_servers().expect("get all mcp servers");
    let entry = servers.get("sse-server").expect("sse-server exists");
    assert!(entry.apps.gemini, "imported server should enable Gemini");
    assert_eq!(
        entry.server.get("type").and_then(|v| v.as_str()),
        Some("sse"),
        "Gemini url-only server should be normalized to type=sse in unified structure"
    );
}

#[test]
fn enabling_gemini_mcp_skips_when_gemini_dir_missing() {
    use support::create_test_state;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 确认 Gemini 配置目录不存在（模拟“未安装/未运行过 Gemini CLI”）
    assert!(
        !home.join(".gemini").exists(),
        "~/.gemini should not exist in fresh test environment"
    );

    let state = create_test_state().expect("create test state");

    // 先插入一个未启用 Gemini 的 MCP 服务器（避免 upsert 触发同步）
    McpService::upsert_server(
        &state,
        McpServer {
            id: "gemini-server".to_string(),
            name: "Gemini Server".to_string(),
            server: json!({
                "type": "sse",
                "url": "https://example.com/sse"
            }),
            apps: McpApps {
                claude: false,
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )
    .expect("insert server without syncing");

    // 启用 Gemini：目录缺失时应跳过写入（不创建 ~/.gemini/settings.json）
    McpService::toggle_app(&state, "gemini-server", AppType::Gemini, true)
        .expect("toggle gemini should succeed even when ~/.gemini is missing");

    assert!(
        !home.join(".gemini").exists(),
        "~/.gemini should still not exist after skipped sync"
    );
}

#[test]
fn enabling_claude_mcp_skips_when_claude_config_absent() {
    use support::create_test_state;

    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 确认 Claude 相关目录/文件都不存在（模拟“未安装/未运行过 Claude”）
    assert!(
        !home.join(".claude").exists(),
        "~/.claude should not exist in fresh test environment"
    );
    assert!(
        !home.join(".claude.json").exists(),
        "~/.claude.json should not exist in fresh test environment"
    );

    let state = create_test_state().expect("create test state");

    // 先插入一个未启用 Claude 的 MCP 服务器（避免 upsert 触发同步）
    McpService::upsert_server(
        &state,
        McpServer {
            id: "claude-server".to_string(),
            name: "Claude Server".to_string(),
            server: json!({
                "type": "stdio",
                "command": "echo"
            }),
            apps: McpApps {
                claude: false,
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    )
    .expect("insert server without syncing");

    // 启用 Claude：配置缺失时应跳过写入（不创建 ~/.claude.json）
    McpService::toggle_app(&state, "claude-server", AppType::Claude, true)
        .expect("toggle claude should succeed even when ~/.claude is missing");

    assert!(
        !home.join(".claude.json").exists(),
        "~/.claude.json should still not exist after skipped sync"
    );
}
