use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cc_switch_lib::{McpServer, McpService};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const SOURCES: [&str; 8] = [
    ".claude.json",
    ".codex/config.toml",
    ".gemini/settings.json",
    ".grok/config.toml",
    ".config/opencode/opencode.json",
    ".hermes/config.yaml",
    ".minimax/mcp.json",
    ".pi/agent/mcp.json",
];

fn reset_sources() {
    reset_test_fs();
    for source in SOURCES {
        let path = ensure_test_home().join(source);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}

fn write_source(source: &str, contents: &str) {
    let path = ensure_test_home().join(source);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn snapshots(home: &Path) -> BTreeMap<&'static str, Option<Vec<u8>>> {
    SOURCES
        .into_iter()
        .map(|source| (source, fs::read(home.join(source)).ok()))
        .collect()
}

#[test]
fn discovery_deduplicates_eight_apps_is_read_only_and_import_keeps_flat_ids() {
    let _guard = test_mutex().lock().unwrap();
    reset_sources();
    let state = create_test_state().unwrap();
    let managed: McpServer = serde_json::from_value(json!({
        "id":"managed", "name":"Keep my name", "server":{"command":"original"},
        "apps":{"codex":true}
    }))
    .unwrap();
    state.db.save_mcp_server(&managed).unwrap();
    write_source(
        SOURCES[0],
        r#"{"mcpServers":{"managed":{"command":"node"},"shared":{"command":"node"}}}"#,
    );
    write_source(SOURCES[1], "[mcp_servers.shared]\ncommand = 'node'\n");
    write_source(
        SOURCES[2],
        r#"{"mcpServers":{"shared":{"command":"node"}}}"#,
    );
    write_source(SOURCES[3], "[mcp_servers.shared]\ncommand = 'node'\n");
    write_source(
        SOURCES[4],
        r#"{"mcp":{"shared":{"type":"local","command":["node"]}}}"#,
    );
    write_source(SOURCES[5], "mcp_servers:\n  shared:\n    command: node\n");
    write_source(
        SOURCES[6],
        r#"{"mcpServers":{"shared":{"command":"node"},"mini-disabled":{"command":"node","enabled":false}}}"#,
    );
    write_source(
        SOURCES[7],
        r#"{"mcpServers":{"shared":{"command":"node"},"pi-disabled":{"command":"node","enabled":false}}}"#,
    );
    let before = snapshots(ensure_test_home());
    let rows_before = serde_json::to_value(state.db.get_all_mcp_servers().unwrap()).unwrap();

    for _ in 0..2 {
        let discovery = McpService::scan_unmanaged(&state).unwrap();
        assert_eq!(discovery.unmanaged_count, 3);
        assert!(
            discovery.failed_apps.is_empty(),
            "{:?}",
            discovery.failed_apps
        );
        assert_eq!(
            serde_json::to_value(state.db.get_all_mcp_servers().unwrap()).unwrap(),
            rows_before
        );
        assert_eq!(snapshots(ensure_test_home()), before);
    }

    assert_eq!(McpService::import_from_all_apps(&state).unwrap(), 3);
    let servers = state.db.get_all_mcp_servers().unwrap();
    assert_eq!(servers.len(), 4);
    assert_eq!(servers["managed"].name, "Keep my name");
    assert_eq!(servers["managed"].server, managed.server);
    assert!(!servers["pi-disabled"].apps.pi);
    assert!(!servers["mini-disabled"].apps.mcode);
    assert!(servers["shared"].apps.claude && servers["shared"].apps.codex);
    assert!(servers["shared"].apps.gemini && servers["shared"].apps.grokbuild);
    assert!(servers["shared"].apps.opencode && servers["shared"].apps.hermes);
    assert!(servers["shared"].apps.pi && servers["shared"].apps.mcode);
    assert_eq!(
        McpService::scan_unmanaged(&state).unwrap().unmanaged_count,
        0
    );
    assert_eq!(snapshots(ensure_test_home()), before);
}

#[test]
fn discovery_reports_a_broken_source_without_hiding_others_or_returning_secrets() {
    let _guard = test_mutex().lock().unwrap();
    reset_sources();
    let state = create_test_state().unwrap();
    write_source(SOURCES[1], "[broken\nsecret = 'DO_NOT_RETURN_SECRET'\n");
    write_source(
        SOURCES[2],
        r#"{"mcpServers":{"valid":{"command":"node","env":{"API_KEY":"DO_NOT_RETURN_SECRET"}}}}"#,
    );
    let before = snapshots(ensure_test_home());
    let discovery = McpService::scan_unmanaged(&state).unwrap();
    assert_eq!(discovery.unmanaged_count, 1);
    assert_eq!(discovery.failed_apps, vec!["codex"]);
    let response = serde_json::to_value(&discovery).unwrap();
    assert_eq!(response, json!({"unmanagedCount":1,"failedApps":["codex"]}));
    assert!(!response.to_string().contains("DO_NOT_RETURN_SECRET"));
    assert!(state.db.get_all_mcp_servers().unwrap().is_empty());
    assert_eq!(snapshots(ensure_test_home()), before);
}

#[test]
fn discovery_treats_missing_sources_as_empty_and_ignores_invalid_entries() {
    let _guard = test_mutex().lock().unwrap();
    reset_sources();
    let state = create_test_state().unwrap();
    let before = snapshots(ensure_test_home());
    let result = McpService::scan_unmanaged(&state).unwrap();
    assert_eq!(result.unmanaged_count, 0);
    assert!(result.failed_apps.is_empty());
    assert_eq!(snapshots(ensure_test_home()), before);
    write_source(
        SOURCES[6],
        r#"{"mcpServers":{"invalid":{"env":{"API_KEY":"unused"}}}}"#,
    );
    write_source(SOURCES[7], r#"{"mcpServers":{"invalid":{"command":17}}}"#);
    let result = McpService::scan_unmanaged(&state).unwrap();
    assert_eq!(result.unmanaged_count, 0);
    assert!(result.failed_apps.is_empty());
}
