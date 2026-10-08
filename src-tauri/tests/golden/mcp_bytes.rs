//! ① MCP 投影的字节。
//!
//! MCP 和供应商配置住在同一个文件里（Codex 的 config.toml、Gemini 的 settings.json），
//! Claude Code 的在 `~/.claude.json`。重构只改供应商那部分的写法，MCP 这一段的字节
//! 必须不变：投影本身的输出、以及切换供应商前后这一段都锁住。

use serde_json::json;

use cc_switch_lib::{AppType, McpService, ProviderService};

use crate::support::{create_test_state, reset_test_fs, test_mutex};
use crate::util::{
    assert_golden, mcp_server, provider, read_home_file, read_home_json, seed_providers,
    sort_objects, stable_json_file, toml_section, write_home_file,
};

const CLAUDE_JSON: &str = ".claude.json";
const CODEX_CONFIG: &str = ".codex/config.toml";
const GEMINI_SETTINGS: &str = ".gemini/settings.json";

fn seed_mcp_servers(state: &cc_switch_lib::AppState) {
    let all = [AppType::Claude, AppType::Codex, AppType::Gemini];
    for server in [
        mcp_server(
            "stdio-server",
            json!({
                "type": "stdio",
                "command": "example-mcp-server",
                "args": ["--stdio"],
                "env": { "EXAMPLE_TOKEN": "mcp-token" }
            }),
            &all,
        ),
        mcp_server(
            "http-server",
            json!({
                "type": "http",
                "url": "https://mcp.example.com/mcp",
                "headers": { "Authorization": "Bearer mcp-header" }
            }),
            &all,
        ),
        mcp_server(
            "sse-server",
            json!({ "type": "sse", "url": "https://mcp.example.com/sse" }),
            &[AppType::Claude, AppType::Gemini],
        ),
        mcp_server(
            "disabled-server",
            json!({ "type": "stdio", "command": "disabled" }),
            &[],
        ),
    ] {
        state.db.save_mcp_server(&server).expect("save mcp server");
    }
}

#[test]
fn claude_json_mcp_projection_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        CLAUDE_JSON,
        r#"{
  "numStartups": 12,
  "projects": {
    "/Users/me/repo": {
      "allowedTools": [],
      "hasTrustDialogAccepted": true
    }
  },
  "mcpServers": {
    "external-only": {
      "type": "stdio",
      "command": "external"
    }
  },
  "userID": "user-1"
}"#,
    );
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);

    McpService::sync_all_enabled(&state).expect("sync mcp");

    assert_golden(
        "mcp/claude.json",
        &stable_json_file(CLAUDE_JSON, &["/mcpServers"]),
    );
}

#[test]
fn codex_config_mcp_projection_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        CODEX_CONFIG,
        r#"# personal settings
model_provider = "custom"
model = "gpt-5"
approval_policy = "on-request"

[model_providers.custom]
name = "custom"
base_url = "https://api.example.com/v1"
wire_api = "responses"

[projects."/Users/me/repo"]
trust_level = "trusted"

[mcp_servers.external-only]
command = "external"
"#,
    );
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);

    McpService::sync_all_enabled(&state).expect("sync mcp");

    assert_golden("mcp/codex-config.toml", &read_home_file(CODEX_CONFIG));
}

#[test]
fn gemini_settings_mcp_projection_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        GEMINI_SETTINGS,
        r#"{
  "security": {
    "auth": {
      "selectedType": "gemini-api-key"
    }
  },
  "model": {
    "name": "gemini-2.5-pro",
    "compressionThreshold": 0.5
  },
  "ui": {
    "theme": "GitHub"
  },
  "mcpServers": {
    "external-only": {
      "command": "external"
    }
  }
}"#,
    );
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);

    McpService::sync_all_enabled(&state).expect("sync mcp");

    assert_golden(
        "mcp/gemini-settings.json",
        &stable_json_file(GEMINI_SETTINGS, &["/mcpServers"]),
    );
}

fn codex_third_party(id: &str, base_url: &str, key: &str) -> cc_switch_lib::Provider {
    provider(
        id,
        json!({
            "auth": { "OPENAI_API_KEY": key },
            "config": format!(
                "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
            )
        }),
        None,
    )
}

/// 切换供应商前后，config.toml 里 `[mcp_servers.*]` 这一段逐字节不变。
#[test]
fn codex_switch_keeps_mcp_section_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let a = codex_third_party("a", "https://a.example/v1", "sk-a");
    write_home_file(
        CODEX_CONFIG,
        a.settings_config["config"].as_str().expect("config text"),
    );
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);
    seed_providers(
        &state,
        &AppType::Codex,
        &[a, codex_third_party("b", "https://b.example/v1", "sk-b")],
        "a",
    );
    McpService::sync_all_enabled(&state).expect("sync mcp");
    let before = toml_section(&read_home_file(CODEX_CONFIG), "[mcp_servers");
    assert_golden("mcp/codex-section.toml", &format!("{before}\n"));

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    let on_b = toml_section(&read_home_file(CODEX_CONFIG), "[mcp_servers");
    ProviderService::switch(&state, AppType::Codex, "a").expect("switch back to a");
    let on_a = toml_section(&read_home_file(CODEX_CONFIG), "[mcp_servers");
    assert_eq!(before, on_b, "switching must not change the MCP section");
    assert_eq!(
        before, on_a,
        "switching back must not change the MCP section"
    );
}

/// 切换供应商前后，Gemini settings.json 的 `mcpServers` 不变（条目顺序除外，见 `sort_objects`）。
#[test]
fn gemini_switch_keeps_mcp_servers() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        ".gemini/.env",
        "GEMINI_API_KEY=g-a\nGOOGLE_GEMINI_BASE_URL=https://a.example\n",
    );
    write_home_file(GEMINI_SETTINGS, "{}");
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);
    let gemini = |id: &str, url: &str, key: &str| {
        provider(
            id,
            json!({
                "env": { "GEMINI_API_KEY": key, "GOOGLE_GEMINI_BASE_URL": url },
                "config": {}
            }),
            None,
        )
    };
    seed_providers(
        &state,
        &AppType::Gemini,
        &[
            gemini("a", "https://a.example", "g-a"),
            gemini("b", "https://b.example", "g-b"),
        ],
        "a",
    );
    McpService::sync_all_enabled(&state).expect("sync mcp");
    let mcp_servers = || {
        let mut settings = read_home_json(GEMINI_SETTINGS);
        sort_objects(&mut settings, &["/mcpServers"]);
        serde_json::to_string_pretty(&settings["mcpServers"]).expect("serialize mcpServers")
    };
    let before = mcp_servers();
    assert_ne!(before, "null", "MCP servers should have been projected");

    ProviderService::switch(&state, AppType::Gemini, "b").expect("switch to b");
    let on_b = mcp_servers();
    ProviderService::switch(&state, AppType::Gemini, "a").expect("switch back to a");
    let on_a = mcp_servers();
    assert_eq!(before, on_b, "switching must not change mcpServers");
    assert_eq!(before, on_a, "switching back must not change mcpServers");
}

/// Claude Code 切换供应商不改 `~/.claude.json`（`mcpServers` 的顺序除外，见 `sort_objects`）。
#[test]
fn claude_switch_keeps_claude_json_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(CLAUDE_JSON, r#"{ "numStartups": 1 }"#);
    let claude = |id: &str, url: &str, key: &str| {
        provider(
            id,
            json!({ "env": { "ANTHROPIC_BASE_URL": url, "ANTHROPIC_AUTH_TOKEN": key } }),
            None,
        )
    };
    let a = claude("a", "https://a.example", "sk-a");
    write_home_file(".claude/settings.json", &a.settings_config.to_string());
    let state = create_test_state().expect("create test state");
    seed_mcp_servers(&state);
    seed_providers(
        &state,
        &AppType::Claude,
        &[a, claude("b", "https://b.example", "sk-b")],
        "a",
    );
    McpService::sync_all_enabled(&state).expect("sync mcp");
    let claude_json = || stable_json_file(CLAUDE_JSON, &["/mcpServers"]);
    let before = claude_json();

    ProviderService::switch(&state, AppType::Claude, "b").expect("switch to b");
    let on_b = claude_json();
    ProviderService::switch(&state, AppType::Claude, "a").expect("switch back to a");
    assert_eq!(before, on_b, "switching must not change ~/.claude.json");
    assert_eq!(before, claude_json());
}
