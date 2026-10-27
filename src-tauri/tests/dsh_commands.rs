use cc_switch_lib::{AppType, McpApps, McpServer, McpService, Provider, ProviderService};
use serde_json::json;
use std::{ffi::OsString, fs, path::PathBuf};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct DshHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl DshHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        fs::create_dir_all(directory.path().join("profiles/web")).unwrap();
        Self {
            directory,
            previous,
        }
    }
    fn config(&self) -> PathBuf {
        self.directory.path().join("profiles/web/cordis.patch.yml")
    }
}
impl Drop for DshHome {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
    }
}
fn provider(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({"api":"openai-completions","options":{"baseURL":"https://example.com/v1","apiKey":"test-value"},"models":{"model":{"limit":{"context":100000}}}}),
        None,
    )
}

#[test]
fn dsh_provider_lifecycle_preserves_native_plugins_and_reusable_profiles() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = DshHome::new();
    let state = create_test_state().unwrap();
    let native = "# native\n- id: llm-pi-ai\n  config:\n    providers:\n      native: {api: openai-completions, baseURL: 'https://native.example/v1', apiKeyEnv: NATIVE_KEY, models: [{id: native-model}]}\n- id: other\n  config: {value: !!js process.env.USER_VALUE}\n";
    fs::write(home.config(), native).unwrap();
    let imported = ProviderService::list(&state, AppType::Dsh).unwrap();
    assert_eq!(
        imported["native"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(true)
    );
    ProviderService::add(&state, AppType::Dsh, provider("draft"), false).unwrap();
    assert_eq!(fs::read_to_string(home.config()).unwrap(), native);
    ProviderService::switch(&state, AppType::Dsh, "draft").unwrap();
    let entries = ProviderService::list(&state, AppType::Dsh).unwrap();
    assert_eq!(
        entries["draft"].meta.as_ref().unwrap().live_config_managed,
        Some(true)
    );
    let mut edited = entries["draft"].clone();
    edited.settings_config["options"]["baseURL"] = json!("https://updated.example/v1");
    ProviderService::update(&state, AppType::Dsh, None, edited).unwrap();
    assert!(fs::read_to_string(home.config())
        .unwrap()
        .contains("https://updated.example/v1"));
    assert!(ProviderService::add(&state, AppType::Dsh, provider("native"), true).is_err());
    ProviderService::remove_from_live_config(&state, AppType::Dsh, "draft").unwrap();
    let entries = ProviderService::list(&state, AppType::Dsh).unwrap();
    assert_eq!(
        entries["draft"].meta.as_ref().unwrap().live_config_managed,
        Some(false)
    );
    assert!(fs::read_to_string(home.config())
        .unwrap()
        .contains("value: !!js process.env.USER_VALUE"));
    assert!(entries.contains_key("native"));
    ProviderService::delete(&state, AppType::Dsh, "draft").unwrap();
    assert!(state
        .db
        .get_provider_by_id("draft", "dsh")
        .unwrap()
        .is_none());
}

#[test]
fn dsh_mcp_failure_leaves_native_file_and_managed_flags_unchanged() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = DshHome::new();
    let state = create_test_state().unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type":"stdio","command":"node","args":["server.js"]}),
        apps: McpApps {
            dsh: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server.clone()).unwrap();
    let original = fs::read(home.config()).unwrap();
    let patches: serde_yaml::Value = serde_yaml::from_slice(&original).unwrap();
    assert_eq!(patches[0]["insert"][0]["config"]["serverName"], "test");
    let mut invalid = server.clone();
    invalid.server = json!({"type":"sse","url":"https://example.com/sse"});
    assert!(McpService::upsert_server(&state, invalid).is_err());
    assert_eq!(fs::read(home.config()).unwrap(), original);
    assert_eq!(
        state.db.get_all_mcp_servers().unwrap()["test"].server,
        server.server
    );
    McpService::toggle_app(&state, "test", AppType::Dsh, false).unwrap();
    assert!(!state.db.get_all_mcp_servers().unwrap()["test"].apps.dsh);
    assert!(!fs::read_to_string(home.config())
        .unwrap()
        .contains("server.js"));
    let empty: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(home.config()).unwrap()).unwrap();
    assert_eq!(empty, serde_yaml::Value::Sequence(vec![]));
}

#[test]
fn dsh_and_omp_mcp_writes_roll_back_together() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = DshHome::new();
    let omp_dir = home.directory.path().join("omp");
    fs::create_dir_all(&omp_dir).unwrap();
    let omp_path = omp_dir.join("mcp.json");
    cc_switch_lib::update_settings(cc_switch_lib::AppSettings {
        omp_config_dir: Some(omp_dir.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .unwrap();
    let state = create_test_state().unwrap();
    let server: McpServer = serde_json::from_value(json!({
        "id": "shared", "name": "Shared MCP",
        "server": {"type": "stdio", "command": "node", "args": ["server.js"]},
        "apps": {"dsh": true, "omp": true}
    }))
    .unwrap();

    McpService::upsert_server(&state, server.clone()).unwrap();
    let saved = state.db.get_all_mcp_servers().unwrap()["shared"].clone();
    assert!(saved.apps.dsh && saved.apps.omp);
    let dsh_before = fs::read(home.config()).unwrap();
    let omp_before = fs::read(&omp_path).unwrap();

    fs::write(&omp_path, "invalid json").unwrap();
    let mut edited = server.clone();
    edited.server["args"] = json!(["updated.js"]);
    assert!(McpService::upsert_server(&state, edited).is_err());
    assert_eq!(fs::read(home.config()).unwrap(), dsh_before);
    assert_eq!(fs::read_to_string(&omp_path).unwrap(), "invalid json");
    assert_eq!(
        state.db.get_all_mcp_servers().unwrap()["shared"].server,
        server.server
    );
    assert!(McpService::delete_server(&state, "shared").is_err());
    assert_eq!(fs::read(home.config()).unwrap(), dsh_before);
    assert!(state
        .db
        .get_all_mcp_servers()
        .unwrap()
        .contains_key("shared"));

    fs::write(&omp_path, &omp_before).unwrap();
    fs::write(home.config(), "[").unwrap();
    assert!(McpService::delete_server(&state, "shared").is_err());
    assert_eq!(fs::read(&omp_path).unwrap(), omp_before);
    assert!(state
        .db
        .get_all_mcp_servers()
        .unwrap()
        .contains_key("shared"));

    fs::write(home.config(), &dsh_before).unwrap();
    assert!(McpService::delete_server(&state, "shared").unwrap());
    assert!(!state
        .db
        .get_all_mcp_servers()
        .unwrap()
        .contains_key("shared"));
    assert!(!fs::read_to_string(home.config())
        .unwrap()
        .contains("server.js"));
    let omp: serde_json::Value = serde_json::from_slice(&fs::read(&omp_path).unwrap()).unwrap();
    assert!(omp["mcpServers"].get("shared").is_none());
}
