use cc_switch_lib::{AppType, McpApps, McpServer, McpService, Provider, ProviderService};
use serde_json::{json, Value};
use std::{ffi::OsString, fs};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct TraeHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl TraeHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("TRAE_HOME");
        std::env::set_var("TRAE_HOME", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn config(&self) -> std::path::PathBuf {
        self.directory.path().join("traecli.toml")
    }
    fn read(&self) -> Value {
        toml::from_str(&fs::read_to_string(self.config()).unwrap()).unwrap()
    }
}
impl Drop for TraeHome {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("TRAE_HOME", value);
        } else {
            std::env::remove_var("TRAE_HOME");
        }
    }
}

fn provider(id: &str, model: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "provider": {
                "name": id,
                "base_url": "https://example.com/v1",
                "wire_api": "responses",
                "experimental_bearer_token": "sk-test"
            },
            "model": model
        }),
        None,
    )
}

#[test]
fn trae_providers_add_switch_and_return_to_account_login() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = TraeHome::new();
    fs::write(
        home.config(),
        "# user comment\napproval_policy = \"on-request\"\n[model_providers.native]\nname = \"Native\"\nbase_url = \"https://native.example/v1\"\nenv_key = \"NATIVE_KEY\"\n",
    )
    .unwrap();
    let state = create_test_state().unwrap();

    // Native providers are imported; the default selection is the account login.
    let listed = ProviderService::list(&state, AppType::Trae).unwrap();
    assert_eq!(listed["native"].name, "Native");
    assert_eq!(
        ProviderService::current(&state, AppType::Trae).unwrap(),
        "trae-official"
    );

    ProviderService::add(&state, AppType::Trae, provider("custom", "gpt-5"), true).unwrap();
    assert!(
        ProviderService::add(&state, AppType::Trae, provider("custom", "gpt-5"), true).is_err()
    );
    assert!(home.read()["model_providers"].get("custom").is_some());
    assert!(home.read().get("model_provider").is_none());

    ProviderService::switch(&state, AppType::Trae, "custom").unwrap();
    let config = home.read();
    assert_eq!(config["model_provider"], "custom");
    assert_eq!(config["model"], "gpt-5");
    assert_eq!(config["approval_policy"], "on-request");
    assert_eq!(
        ProviderService::current(&state, AppType::Trae).unwrap(),
        "custom"
    );
    // The active provider cannot be removed or deleted.
    assert!(ProviderService::remove_from_live_config(&state, AppType::Trae, "custom").is_err());
    assert!(ProviderService::delete(&state, AppType::Trae, "custom").is_err());

    // Editing the active provider updates the selected model too.
    ProviderService::update(&state, AppType::Trae, None, provider("custom", "gpt-5.1")).unwrap();
    assert_eq!(home.read()["model"], "gpt-5.1");

    let official = Provider::with_id(
        "trae-official".into(),
        "TRAE".into(),
        json!({"official": true}),
        None,
    );
    ProviderService::add(&state, AppType::Trae, official, true).unwrap();
    ProviderService::switch(&state, AppType::Trae, "trae-official").unwrap();
    let config = home.read();
    assert!(config.get("model_provider").is_none());
    assert!(config.get("model").is_none());
    assert!(fs::read_to_string(home.config())
        .unwrap()
        .contains("# user comment"));

    ProviderService::remove_from_live_config(&state, AppType::Trae, "custom").unwrap();
    assert!(home.read()["model_providers"].get("custom").is_none());
    // The CCS profile keeps the model so the provider can be switched back later.
    let listed = ProviderService::list(&state, AppType::Trae).unwrap();
    assert_eq!(listed["custom"].settings_config["model"], "gpt-5.1");
    ProviderService::switch(&state, AppType::Trae, "custom").unwrap();
    assert_eq!(home.read()["model"], "gpt-5.1");
}

#[test]
fn trae_mcp_servers_sync_into_traecli_toml() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = TraeHome::new();
    fs::write(home.config(), "model_provider = \"custom\"\n").unwrap();
    let state = create_test_state().unwrap();
    let server = McpServer {
        id: "docs".into(),
        name: "docs".into(),
        server: json!({"type": "stdio", "command": "node", "args": ["server.js"]}),
        apps: McpApps {
            trae: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    let config = home.read();
    assert_eq!(config["mcp_servers"]["docs"]["command"], "node");
    assert_eq!(config["model_provider"], "custom");
    McpService::toggle_app(&state, "docs", AppType::Trae, false).unwrap();
    assert!(home
        .read()
        .get("mcp_servers")
        .is_none_or(|servers| servers.get("docs").is_none()));
}
