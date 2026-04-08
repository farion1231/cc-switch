use std::collections::BTreeMap;

use serde_json::json;

use cc_switch_lib::{bridges::mcp as mcp_bridge, McpApps, McpServer};

use super::support::{
    create_empty_core_state, create_empty_legacy_state, ensure_test_home, reset_test_fs, test_mutex,
};

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
fn mcp_parity_upsert_matches_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    std::fs::create_dir_all(home.join(".codex")).expect("create codex dir");
    std::fs::write(
        home.join(".codex").join("auth.json"),
        r#"{"OPENAI_API_KEY":"seed-key"}"#,
    )
    .expect("seed auth");
    std::fs::write(home.join(".codex").join("config.toml"), "").expect("seed config");
    let state = create_empty_legacy_state();
    mcp_bridge::legacy_upsert_mcp_server(&state, demo_server()).expect("legacy upsert mcp");
    let legacy_servers = mcp_bridge::legacy_get_all_mcp_servers(&state).expect("legacy servers");
    let legacy = snapshot(
        &home,
        serde_json::to_value(legacy_servers).expect("legacy json"),
    );

    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    std::fs::create_dir_all(home.join(".codex")).expect("create codex dir");
    std::fs::write(
        home.join(".codex").join("auth.json"),
        r#"{"OPENAI_API_KEY":"seed-key"}"#,
    )
    .expect("seed auth");
    std::fs::write(home.join(".codex").join("config.toml"), "").expect("seed config");
    let _state = create_empty_core_state();
    mcp_bridge::upsert_mcp_server(demo_server()).expect("core upsert mcp");
    let core_servers = mcp_bridge::get_all_mcp_servers().expect("core servers");
    let core = snapshot(
        &home,
        serde_json::to_value(core_servers).expect("core json"),
    );

    assert_eq!(core, legacy);
}
