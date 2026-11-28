use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use cc_switch_lib::{hermes_config, update_settings, AppSettings, McpServer, McpService};
use serde_json::json;

use crate::support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const SOURCES: [&str; 16] = [
    ".claude.json",
    ".codex/config.toml",
    ".gemini/settings.json",
    ".grok/config.toml",
    ".config/opencode/opencode.json",
    ".hermes-mcp-discovery/config.yaml",
    ".minimax/mcp.json",
    ".pi/agent/mcp.json",
    ".kimi-code/mcp.json",
    ".gemini/config/mcp_config.json",
    ".omp/agent/mcp.json",
    ".dsh/profiles/web/cordis.patch.yml",
    ".copilot/mcp-config.json",
    ".workbuddy/mcp.json",
    ".codebuddy/.mcp.json",
    ".zcode/cli/config.json",
];

struct SourceFixture;

impl Drop for SourceFixture {
    fn drop(&mut self) {
        for source in SOURCES {
            let _ = fs::remove_file(ensure_test_home().join(source));
        }
    }
}

fn reset_sources() -> SourceFixture {
    reset_test_fs();
    // Do not let inherited tool overrides redirect fixtures into real configs.
    for var in [
        "CODEBUDDY_CONFIG_DIR",
        "DSH_HOME",
        "KIMI_CODE_HOME",
        "MINIMAX_DATA_DIR",
        "MAVIS_DATA_DIR",
        "PI_CODING_AGENT_DIR",
        "OMP_PROFILE",
        "PI_PROFILE",
    ] {
        std::env::remove_var(var);
    }
    std::env::set_var("COPILOT_HOME", ensure_test_home().join(".copilot"));
    std::env::set_var(
        "WORKBUDDY_CONFIG_DIR",
        ensure_test_home().join(".workbuddy"),
    );
    // Pin the reader to the fixture directory on every OS, independently of
    // LOCALAPPDATA, HERMES_HOME and the platform's default Hermes location.
    let hermes_config = ensure_test_home().join(SOURCES[5]);
    update_settings(AppSettings {
        hermes_config_dir: Some(
            hermes_config
                .parent()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
        ..AppSettings::default()
    })
    .expect("set Hermes discovery fixture directory");
    assert_eq!(hermes_config::get_hermes_config_path(), hermes_config);
    for source in SOURCES {
        let path = ensure_test_home().join(source);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    SourceFixture
}

/// 发现结果里的名字，按页面收到的 JSON 读
fn ids(result: &impl serde::Serialize) -> Vec<String> {
    serde_json::to_value(result).unwrap()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|server| server["id"].as_str().unwrap().to_owned())
        .collect()
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
fn discovery_deduplicates_sixteen_apps_is_read_only_and_import_keeps_flat_ids() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
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
    write_source(
        SOURCES[8],
        r#"{"mcpServers":{"kimi-only":{"command":"node"}}}"#,
    );
    write_source(
        SOURCES[9],
        r#"{"mcpServers":{"agy-only":{"command":"node"}}}"#,
    );
    write_source(
        SOURCES[10],
        r#"{"mcpServers":{"omp-only":{"command":"node"}}}"#,
    );
    write_source(SOURCES[11], "- id: dsh-only\n  name: '@deepseek-ai/dsh-mcp-client'\n  config: {serverName: dsh-only, transport: stdio, command: node}\n");
    write_source(
        SOURCES[12],
        r#"{"mcpServers":{"copilot-only":{"type":"local","command":"node","tools":["*"]}}}"#,
    );
    write_source(
        SOURCES[13],
        r#"{"mcpServers":{"workbuddy-only":{"command":"node","disabled":true},"connector":{"command":"node","managedBy":"connector"}}}"#,
    );
    write_source(
        SOURCES[14],
        r#"{"mcpServers":{"codebuddy-only":{"command":"node"}}}"#,
    );
    write_source(
        SOURCES[15],
        r#"{"mcp":{"servers":{"zcode-only":{"type":"stdio","command":"node"}}}}"#,
    );
    let before = snapshots(ensure_test_home());
    let rows_before = serde_json::to_value(state.db.get_all_mcp_servers().unwrap()).unwrap();

    for _ in 0..2 {
        let discovery = McpService::scan_unmanaged(&state).unwrap();
        assert_eq!(
            ids(&discovery),
            [
                "agy-only",
                "codebuddy-only",
                "copilot-only",
                "dsh-only",
                "kimi-only",
                "mini-disabled",
                "omp-only",
                "pi-disabled",
                "shared",
                "workbuddy-only",
                "zcode-only",
            ]
        );
        let shared = discovery.servers.iter().find(|s| s.id == "shared").unwrap();
        assert_eq!(
            shared.apps,
            [
                "claude",
                "codex",
                "gemini",
                "grokbuild",
                "opencode",
                "hermes",
                "mcode",
                "pi"
            ]
        );
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

    assert_eq!(McpService::import_from_all_apps(&state).unwrap(), 11);
    let servers = state.db.get_all_mcp_servers().unwrap();
    assert_eq!(servers.len(), 12);
    assert!(servers["codebuddy-only"].apps.codebuddy);
    assert!(servers["zcode-only"].apps.zcode);
    assert_eq!(servers["managed"].name, "Keep my name");
    assert_eq!(servers["managed"].server, managed.server);
    assert!(servers["copilot-only"].apps.copilot);
    assert!(!servers["workbuddy-only"].apps.workbuddy);
    assert!(!servers.contains_key("connector"));
    assert!(!servers["pi-disabled"].apps.pi);
    assert!(!servers["mini-disabled"].apps.mcode);
    assert!(servers["shared"].apps.claude && servers["shared"].apps.codex);
    assert!(servers["shared"].apps.gemini && servers["shared"].apps.grokbuild);
    assert!(servers["shared"].apps.opencode && servers["shared"].apps.hermes);
    assert!(servers["shared"].apps.pi && servers["shared"].apps.mcode);
    assert!(McpService::scan_unmanaged(&state)
        .unwrap()
        .servers
        .is_empty());
    assert_eq!(snapshots(ensure_test_home()), before);
}

#[test]
fn discovery_reports_a_broken_source_without_hiding_others_or_returning_secrets() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
    let state = create_test_state().unwrap();
    write_source(SOURCES[1], "[broken\nsecret = 'DO_NOT_RETURN_SECRET'\n");
    write_source(
        SOURCES[2],
        r#"{"mcpServers":{"valid":{"command":"node","env":{"API_KEY":"DO_NOT_RETURN_SECRET"}}}}"#,
    );
    let before = snapshots(ensure_test_home());
    let discovery = McpService::scan_unmanaged(&state).unwrap();
    assert_eq!(discovery.failed_apps, vec!["codex"]);
    let response = serde_json::to_value(&discovery).unwrap();
    assert_eq!(
        response,
        json!({"servers":[{"id":"valid","apps":["gemini"]}],"failedApps":["codex"]})
    );
    assert!(!response.to_string().contains("DO_NOT_RETURN_SECRET"));
    assert!(state.db.get_all_mcp_servers().unwrap().is_empty());
    assert_eq!(snapshots(ensure_test_home()), before);
}

#[test]
fn discovery_treats_missing_sources_as_empty_and_ignores_invalid_entries() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
    let state = create_test_state().unwrap();
    let before = snapshots(ensure_test_home());
    let result = McpService::scan_unmanaged(&state).unwrap();
    assert!(result.servers.is_empty());
    assert!(result.failed_apps.is_empty());
    assert_eq!(snapshots(ensure_test_home()), before);
    write_source(
        SOURCES[6],
        r#"{"mcpServers":{"invalid":{"env":{"API_KEY":"unused"}}}}"#,
    );
    write_source(SOURCES[7], r#"{"mcpServers":{"invalid":{"command":17}}}"#);
    let result = McpService::scan_unmanaged(&state).unwrap();
    assert!(result.servers.is_empty());
    assert!(result.failed_apps.is_empty());
}

#[test]
fn discovery_skips_unreadable_managed_ids_without_hiding_new_native_entries() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
    let state = create_test_state().unwrap();
    let broken: McpServer = serde_json::from_value(json!({
        "id":"broken", "name":"Broken", "server":{"command":"node"}, "apps":{}
    }))
    .unwrap();
    state.db.save_mcp_server(&broken).unwrap();
    let conn =
        rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db")).unwrap();
    let invalid = r#"{"command":"#;
    conn.execute(
        "UPDATE mcp_servers SET server_config = ?1 WHERE id = 'broken'",
        [invalid],
    )
    .unwrap();
    write_source(
        SOURCES[0],
        r#"{"mcpServers":{"broken":{"command":"node"},"new":{"command":"node"}}}"#,
    );
    let before = snapshots(ensure_test_home());
    let result = McpService::scan_unmanaged(&state).unwrap();
    assert_eq!(ids(&result), ["new"]);
    assert!(result.failed_apps.is_empty());
    assert_eq!(McpService::import_from_all_apps(&state).unwrap(), 1);
    assert!(McpService::scan_unmanaged(&state)
        .unwrap()
        .servers
        .is_empty());
    let rows = state.db.get_mcp_server_rows().unwrap();
    assert!(rows.readable.contains_key("new"));
    assert_eq!(rows.unreadable.len(), 1);
    assert_eq!(rows.unreadable[0].id, "broken");
    let raw: String = conn
        .query_row(
            "SELECT server_config FROM mcp_servers WHERE id = 'broken'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, invalid);
    assert_eq!(snapshots(ensure_test_home()), before);
}

/// Pi 不收的条目（SSE、非法名、只差 - / _ 的重名）不提示，免得导入跳过后提示永远消不掉
#[test]
fn discovery_applies_pi_import_rules() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
    let state = create_test_state().unwrap();
    write_source(
        SOURCES[7],
        r#"{"mcpServers":{
            "sse":{"type":"sse","url":"https://example.com/sse"},
            "bad name":{"command":"node"},
            "same-name":{"command":"node"},
            "same_name":{"command":"node"},
            "pi-ok":{"command":"node"}
        }}"#,
    );
    assert_eq!(ids(&McpService::scan_unmanaged(&state).unwrap()), ["pi-ok"]);
    // Pi 会跳过并报告其余条目，但能导入的那条入库后提示归零
    assert!(McpService::import_from_all_apps(&state).is_err());
    assert!(state.db.get_all_mcp_servers().unwrap()["pi-ok"].apps.pi);
    assert!(McpService::scan_unmanaged(&state)
        .unwrap()
        .servers
        .is_empty());
}

/// 勾选导入只动勾上的名字：没勾的留在原生文件里继续提示，已管理的同名也不顺带勾上应用
#[test]
fn selected_import_only_touches_chosen_ids() {
    let _guard = test_mutex().lock().unwrap();
    let _sources = reset_sources();
    let state = create_test_state().unwrap();
    let managed: McpServer = serde_json::from_value(json!({
        "id":"managed", "name":"managed", "server":{"command":"node"}, "apps":{"codex":true}
    }))
    .unwrap();
    state.db.save_mcp_server(&managed).unwrap();
    write_source(
        SOURCES[0],
        r#"{"mcpServers":{"managed":{"command":"node"},"keep":{"command":"node"},"skip":{"command":"node"}}}"#,
    );
    write_source(SOURCES[1], "[mcp_servers.keep]\ncommand = 'node'\n");
    write_source(SOURCES[7], r#"{"mcpServers":{"skip":{"command":"node"}}}"#);
    let before = snapshots(ensure_test_home());

    let selection: HashSet<String> = ["keep".to_string()].into();
    assert_eq!(
        McpService::import_selected(&state, Some(&selection)).unwrap(),
        1
    );
    let servers = state.db.get_all_mcp_servers().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers["keep"].apps.claude && servers["keep"].apps.codex);
    assert!(!servers.contains_key("skip"));
    assert!(!servers["managed"].apps.claude);
    assert_eq!(ids(&McpService::scan_unmanaged(&state).unwrap()), ["skip"]);
    assert_eq!(snapshots(ensure_test_home()), before);
}
