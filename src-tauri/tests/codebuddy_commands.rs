use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use std::{ffi::OsString, fs};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

/// Points CodeBuddy Code at a private directory through `CODEBUDDY_CONFIG_DIR`.
struct CodebuddyHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl CodebuddyHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CODEBUDDY_CONFIG_DIR");
        std::env::set_var("CODEBUDDY_CONFIG_DIR", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn models(&self) -> std::path::PathBuf {
        self.directory.path().join("models.json")
    }
    fn read_models(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.models()).unwrap()).unwrap()
    }
    fn ids(&self) -> Vec<String> {
        self.read_models()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap().to_string())
            .collect()
    }
}
impl Drop for CodebuddyHome {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("CODEBUDDY_CONFIG_DIR", value);
        } else {
            std::env::remove_var("CODEBUDDY_CONFIG_DIR");
        }
    }
}

fn provider(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "name": id,
            "vendor": "Example",
            "url": "https://api.example.com/v1/chat/completions",
            "apiKey": "test-key",
            "supportsToolCall": true
        }),
        None,
    )
}

#[test]
fn codebuddy_provider_lifecycle_preserves_native_models() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CodebuddyHome::new();
    let state = create_test_state().unwrap();
    fs::write(
        home.models(),
        r#"{"models":[{"id":"native","name":"Native","url":"https://native.example/v1/chat/completions"}],"availableModels":["native"]}"#,
    )
    .unwrap();

    // Native entries are imported into the list and marked as present in live.
    let listed = ProviderService::list(&state, AppType::Codebuddy).unwrap();
    assert_eq!(listed["native"].name, "Native");
    assert_eq!(
        listed["native"].meta.as_ref().unwrap().live_config_managed,
        Some(true)
    );
    assert!(ProviderService::current(&state, AppType::Codebuddy)
        .unwrap()
        .is_empty());

    // Saved without touching models.json, then added to live.
    ProviderService::add(&state, AppType::Codebuddy, provider("cc-model"), false).unwrap();
    assert_eq!(home.ids(), ["native"]);
    ProviderService::switch(&state, AppType::Codebuddy, "cc-model").unwrap();
    assert_eq!(home.ids(), ["native", "cc-model"]);
    let models = home.read_models();
    assert_eq!(models["availableModels"], json!(["native", "cc-model"]));
    assert_eq!(models["models"][1]["apiKey"], "test-key");

    // Duplicate keys are rejected, whether in the DB or only in models.json.
    assert!(ProviderService::add(&state, AppType::Codebuddy, provider("cc-model"), true).is_err());

    // Editing rewrites only this entry.
    let mut edited = provider("cc-model");
    edited.settings_config["name"] = json!("Renamed");
    ProviderService::update(&state, AppType::Codebuddy, None, edited).unwrap();
    let models = home.read_models();
    assert_eq!(models["models"][1]["name"], "Renamed");
    assert_eq!(models["models"][0]["name"], "Native");

    // An invalid endpoint never reaches the file.
    let mut invalid = provider("cc-model");
    invalid.settings_config["url"] = json!("not a url");
    assert!(ProviderService::update(&state, AppType::Codebuddy, None, invalid).is_err());
    assert_eq!(home.read_models()["models"][1]["name"], "Renamed");

    // The default model in settings.json is protected from removal.
    fs::write(
        home.directory.path().join("settings.json"),
        r#"{"model":"cc-model"}"#,
    )
    .unwrap();
    assert!(
        ProviderService::remove_from_live_config(&state, AppType::Codebuddy, "cc-model").is_err()
    );
    fs::write(home.directory.path().join("settings.json"), "{}").unwrap();

    ProviderService::remove_from_live_config(&state, AppType::Codebuddy, "cc-model").unwrap();
    assert_eq!(home.ids(), ["native"]);
    assert_eq!(
        state
            .db
            .get_provider_by_id("cc-model", "codebuddy")
            .unwrap()
            .unwrap()
            .meta
            .unwrap()
            .live_config_managed,
        Some(false)
    );

    ProviderService::switch(&state, AppType::Codebuddy, "cc-model").unwrap();
    ProviderService::delete(&state, AppType::Codebuddy, "cc-model").unwrap();
    assert_eq!(home.ids(), ["native"]);
    assert!(state
        .db
        .get_provider_by_id("cc-model", "codebuddy")
        .unwrap()
        .is_none());

    // Adding straight to live creates the entry in one step.
    ProviderService::add(&state, AppType::Codebuddy, provider("direct"), true).unwrap();
    assert_eq!(home.ids(), ["native", "direct"]);
}

#[test]
fn codebuddy_add_creates_models_json_when_missing() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CodebuddyHome::new();
    let state = create_test_state().unwrap();
    assert!(!home.models().exists());
    ProviderService::add(&state, AppType::Codebuddy, provider("fresh"), true).unwrap();
    assert_eq!(home.ids(), ["fresh"]);
}

#[test]
fn codebuddy_prompts_write_global_codebuddy_md() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CodebuddyHome::new();
    let state = create_test_state().unwrap();
    let path = home.directory.path().join("CODEBUDDY.md");
    fs::write(&path, "existing instructions\n").unwrap();
    assert_eq!(
        PromptService::import_from_file_on_first_launch(&state, AppType::Codebuddy).unwrap(),
        1
    );

    let prompt: Prompt = serde_json::from_value(
        json!({"id":"rules","name":"Rules","content":"Always answer in Chinese.","enabled":false}),
    )
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::Codebuddy, "rules", prompt).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "existing instructions\n"
    );
    PromptService::enable_prompt(&state, AppType::Codebuddy, "rules").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Always answer in Chinese."
    );
}

#[test]
fn codebuddy_mcp_writes_the_file_codebuddy_reads_and_round_trips() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CodebuddyHome::new();
    let state = create_test_state().unwrap();
    // Only the deprecated file exists: CodeBuddy reads it, so CC Switch must write it
    // instead of creating a .mcp.json that would shadow the user's servers.
    let legacy = home.directory.path().join("mcp.json");
    fs::write(
        &legacy,
        json!({"mcpServers": {"mine": {"type": "stdio", "command": "node"}}}).to_string(),
    )
    .unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type":"http","url":"https://example.com/mcp"}),
        apps: McpApps {
            codebuddy: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    assert!(!home.directory.path().join(".mcp.json").exists());
    let value: Value = serde_json::from_slice(&fs::read(&legacy).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["test"]["type"], "http");
    assert_eq!(value["mcpServers"]["mine"]["command"], "node");

    McpService::toggle_app(&state, "test", AppType::Codebuddy, false).unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&legacy).unwrap()).unwrap();
    assert!(value["mcpServers"].get("test").is_none());
    assert!(
        !state.db.get_all_mcp_servers().unwrap()["test"]
            .apps
            .codebuddy
    );

    // Import picks up the user's own server.
    McpService::import_from_all_apps(&state).unwrap();
    let servers = state.db.get_all_mcp_servers().unwrap();
    assert!(servers["mine"].apps.codebuddy);
    assert_eq!(servers["mine"].server["type"], "stdio");
}
