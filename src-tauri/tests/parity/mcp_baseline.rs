use std::collections::BTreeMap;

use serde_json::json;

use cc_switch_lib::{bridges::mcp as mcp_bridge, McpApps, McpServer};

use super::support::{create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex};

fn demo_server() -> McpServer {
    McpServer {
        id: "demo".to_string(),
        name: "Demo".to_string(),
        server: json!({
            "type": "stdio",
            "command": "echo",
            "args": ["hello"]
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
        tags: vec![],
    }
}

fn snapshot(home: &std::path::Path, servers: serde_json::Value) -> serde_json::Value {
    let mut files = BTreeMap::new();
    for (key, path) in [
        ("codex/config.toml", home.join(".codex").join("config.toml")),
        ("claude/mcp.json", home.join(".claude.json")),
    ] {
        if let Ok(content) = std::fs::read_to_string(path) {
            files.insert(key.to_string(), content);
        }
    }

    json!({
        "servers": servers,
        "files": files,
    })
}

#[test]
fn mcp_baseline_legacy_upsert_snapshot_is_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    std::fs::create_dir_all(home.join(".codex")).expect("create codex dir");
    std::fs::write(home.join(".codex").join("auth.json"), r#"{"OPENAI_API_KEY":"seed-key"}"#)
        .expect("seed auth");
    std::fs::write(home.join(".codex").join("config.toml"), "").expect("seed config");

    let state = create_empty_legacy_state();
    mcp_bridge::legacy_upsert_mcp_server(&state, demo_server()).expect("legacy upsert mcp");

    let servers = mcp_bridge::legacy_get_all_mcp_servers(&state).expect("get mcp servers");
    let snapshot = snapshot(
        &home,
        serde_json::to_value(servers).expect("servers json"),
    );

    assert!(
        snapshot["files"]["codex/config.toml"]
            .as_str()
            .expect("codex config")
            .contains("mcp_servers.demo")
    );
}
