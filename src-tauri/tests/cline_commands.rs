use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use std::{ffi::OsString, fs, path::PathBuf};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

const ENV_KEYS: [&str; 5] = [
    "CLINE_DIR",
    "CLINE_DATA_DIR",
    "CLINE_PROVIDER_SETTINGS_PATH",
    "CLINE_MCP_SETTINGS_PATH",
    "CLINE_DB_DATA_DIR",
];

/// Points every Cline path at a temporary `CLINE_DIR` and restores the caller's environment.
struct ClineHome {
    directory: tempfile::TempDir,
    previous: Vec<(&'static str, Option<OsString>)>,
}
impl ClineHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
        std::env::set_var("CLINE_DIR", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn settings(&self) -> PathBuf {
        self.directory.path().join("data").join("settings")
    }
    fn providers(&self) -> Value {
        serde_json::from_slice(&fs::read(self.settings().join("providers.json")).unwrap()).unwrap()
    }
    fn models(&self) -> Value {
        serde_json::from_slice(&fs::read(self.settings().join("models.json")).unwrap()).unwrap()
    }
}
impl Drop for ClineHome {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn custom(id: &str, model: &str, base: &str) -> Provider {
    Provider::with_id(
        id.into(),
        format!("Relay {id}"),
        json!({
            "settings": {"apiKey": "sk-test", "model": model},
            "provider": {"name": format!("Relay {id}"), "baseUrl": base, "defaultModelId": model,
                         "protocol": "anthropic", "client": "anthropic"},
            "models": {model: {"id": model, "name": model, "contextWindow": 200000}}
        }),
        None,
    )
}

fn write_native(home: &ClineHome) -> Value {
    fs::create_dir_all(home.settings()).unwrap();
    let providers = json!({
        "version": 1,
        "lastUsedProvider": "cline",
        "modes": {},
        "providers": {
            "cline": {"settings": {"provider": "cline", "model": "anthropic/claude-sonnet-5.5",
                                   "auth": {"accessToken": "token", "refreshToken": "refresh"}},
                      "updatedAt": "2026-01-01T00:00:00.000Z", "tokenSource": "oauth"},
            "user-relay": {"settings": {"provider": "user-relay", "apiKey": "sk-user", "model": "m"},
                           "updatedAt": "2026-01-01T00:00:00.000Z", "tokenSource": "manual"}
        }
    });
    fs::write(
        home.settings().join("providers.json"),
        serde_json::to_vec_pretty(&providers).unwrap(),
    )
    .unwrap();
    fs::write(
        home.settings().join("models.json"),
        json!({"version": 1, "providers": {"user-relay": {
            "provider": {"name": "User Relay", "baseUrl": "https://user.example/v1", "protocol": "openai-chat"},
            "models": {"m": {"id": "m"}}, "discoveredModelIds": ["m"]}}})
        .to_string(),
    )
    .unwrap();
    providers
}

#[test]
fn cline_provider_lifecycle_imports_native_config_and_keeps_cline_owned_entries() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ClineHome::new();
    let state = create_test_state().unwrap();
    let native = write_native(&home);

    let listed = ProviderService::list(&state, AppType::Cline).unwrap();
    assert_eq!(listed["user-relay"].name, "User Relay");
    assert_eq!(listed["cline"].settings_config["tokenSource"], "oauth");
    assert!(listed["cline"].settings_config.get("provider").is_none());
    assert_eq!(
        ProviderService::current(&state, AppType::Cline).unwrap(),
        "cline"
    );

    // Saving without adding to live leaves Cline's files untouched.
    ProviderService::add(
        &state,
        AppType::Cline,
        custom("ccs-relay", "m1", "https://relay.example.com/v1"),
        false,
    )
    .unwrap();
    assert_eq!(home.providers(), native);

    ProviderService::switch(&state, AppType::Cline, "ccs-relay").unwrap();
    let providers = home.providers();
    assert_eq!(providers["lastUsedProvider"], "ccs-relay");
    assert_eq!(
        providers["providers"]["ccs-relay"]["settings"]["provider"],
        "ccs-relay"
    );
    assert_eq!(
        home.models()["providers"]["ccs-relay"]["provider"]["client"],
        "anthropic"
    );
    // The OAuth entry and other user providers are preserved byte-for-byte in content.
    assert_eq!(
        providers["providers"]["cline"],
        native["providers"]["cline"]
    );
    assert_eq!(
        home.models()["providers"]["user-relay"]["discoveredModelIds"][0],
        "m"
    );
    assert!(home
        .settings()
        .join("providers.json.cc-switch.bak")
        .exists());

    let mut saved = ProviderService::list(&state, AppType::Cline).unwrap()["ccs-relay"].clone();
    saved.settings_config["provider"]["baseUrl"] = json!("https://updated.example.com/v1");
    ProviderService::update(&state, AppType::Cline, None, saved).unwrap();
    assert_eq!(
        home.models()["providers"]["ccs-relay"]["provider"]["baseUrl"],
        "https://updated.example.com/v1"
    );

    // The current provider cannot be removed; switching away first allows it.
    assert!(ProviderService::delete(&state, AppType::Cline, "ccs-relay").is_err());
    ProviderService::switch(&state, AppType::Cline, "cline").unwrap();
    ProviderService::delete(&state, AppType::Cline, "ccs-relay").unwrap();
    assert!(home.providers()["providers"].get("ccs-relay").is_none());
    assert!(home.models()["providers"].get("ccs-relay").is_none());
    assert!(!state
        .db
        .get_all_providers("cline")
        .unwrap()
        .contains_key("ccs-relay"));
    assert_eq!(
        home.providers()["providers"]["cline"],
        native["providers"]["cline"]
    );
}

#[test]
fn cline_login_tokens_never_reach_the_database() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ClineHome::new();
    let state = create_test_state().unwrap();
    let native = write_native(&home);
    // A row saved by an earlier build still holds the login tokens.
    let legacy = Provider::with_id(
        "cline".into(),
        "cline".into(),
        json!({"settings": {"model": "m", "auth": {"accessToken": "old", "refreshToken": "old"}},
               "tokenSource": "oauth"}),
        None,
    );
    state.db.save_provider("cline", &legacy).unwrap();

    let listed = ProviderService::list(&state, AppType::Cline).unwrap();
    assert!(listed["cline"].settings_config["settings"]
        .get("auth")
        .is_none());
    for provider in state.db.get_all_providers("cline").unwrap().values() {
        assert!(
            provider.settings_config["settings"].get("auth").is_none(),
            "{} kept credentials",
            provider.id
        );
    }

    // Saving the account card changes only the model; Cline's tokens stay.
    let mut account = listed["cline"].clone();
    account.settings_config["settings"]["model"] = json!("anthropic/claude-opus-5-5");
    ProviderService::update(&state, AppType::Cline, None, account).unwrap();
    let entry = &home.providers()["providers"]["cline"];
    assert_eq!(
        entry["settings"]["auth"],
        native["providers"]["cline"]["settings"]["auth"]
    );
    assert_eq!(entry["settings"]["model"], "anthropic/claude-opus-5-5");
    assert!(state
        .db
        .get_provider_by_id("cline", "cline")
        .unwrap()
        .unwrap()
        .settings_config["settings"]
        .get("auth")
        .is_none());
}

#[test]
fn cline_built_in_provider_writes_only_providers_json() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ClineHome::new();
    let state = create_test_state().unwrap();
    let provider = Provider::with_id(
        "anthropic".into(),
        "Anthropic".into(),
        json!({"settings": {"apiKey": "sk-ant", "model": "claude-sonnet-4-5",
                            "baseUrl": "https://api.anthropic.com/v1"}}),
        None,
    );
    ProviderService::add(&state, AppType::Cline, provider, true).unwrap();
    let providers = home.providers();
    assert_eq!(providers["lastUsedProvider"], "anthropic");
    assert_eq!(
        providers["providers"]["anthropic"]["settings"]["baseUrl"],
        "https://api.anthropic.com/v1"
    );
    assert!(!home.settings().join("models.json").exists());
    assert_eq!(
        ProviderService::current(&state, AppType::Cline).unwrap(),
        "anthropic"
    );
}

#[test]
fn cline_rejects_settings_that_would_reset_its_provider_file() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ClineHome::new();
    let state = create_test_state().unwrap();
    let native = write_native(&home);
    let mut bad = custom("ccs-bad", "m1", "https://relay.example.com/v1");
    bad.settings_config["models"]["m1"]["contextWindow"] = json!("large");
    assert!(ProviderService::add(&state, AppType::Cline, bad, true).is_err());
    let mut bad = custom("ccs-bad", "m1", "https://relay.example.com/v1");
    bad.settings_config["provider"]
        .as_object_mut()
        .unwrap()
        .remove("client");
    assert!(ProviderService::add(&state, AppType::Cline, bad, true).is_err());
    assert_eq!(home.providers(), native);
    assert!(!state
        .db
        .get_all_providers("cline")
        .unwrap()
        .contains_key("ccs-bad"));
}

#[test]
fn cline_mcp_and_global_prompts_follow_cline_dir() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = ClineHome::new();
    let state = create_test_state().unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type": "http", "url": "https://example.com/mcp"}),
        apps: McpApps {
            cline: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    let path = home.settings().join("cline_mcp_settings.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["test"]["type"], "streamableHttp");
    McpService::toggle_app(&state, "test", AppType::Cline, false).unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(value["mcpServers"].get("test").is_none());

    // A server configured in Cline is imported with its enabled state.
    fs::write(
        &path,
        json!({"mcpServers": {"native": {"url": "https://example.com/sse", "disabled": true}}})
            .to_string(),
    )
    .unwrap();
    McpService::import_from_all_apps(&state).ok();
    let servers = McpService::get_all_servers(&state).unwrap();
    assert_eq!(servers["native"].server["type"], "sse");
    assert!(!servers["native"].apps.cline);

    let prompt: Prompt = serde_json::from_value(json!({"id": "global", "name": "Global",
        "content": "Use the project conventions.", "enabled": false}))
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::Cline, "global", prompt).unwrap();
    PromptService::enable_prompt(&state, AppType::Cline, "global").unwrap();
    assert_eq!(
        fs::read_to_string(home.directory.path().join("rules").join("AGENTS.md")).unwrap(),
        "Use the project conventions."
    );
}
