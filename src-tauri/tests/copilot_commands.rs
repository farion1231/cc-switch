use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::json;
use std::{ffi::OsString, fs};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct CopilotHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl CopilotHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("COPILOT_HOME");
        std::env::set_var("COPILOT_HOME", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.directory.path().join(name)
    }
}
impl Drop for CopilotHome {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("COPILOT_HOME", value);
        } else {
            std::env::remove_var("COPILOT_HOME");
        }
    }
}

fn byok(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({"env":{
            "COPILOT_PROVIDER_BASE_URL":"https://example.com/v1",
            "COPILOT_PROVIDER_API_KEY":"fixture-key",
            "COPILOT_PROVIDER_TYPE":"openai",
            "COPILOT_MODEL":"gpt-5.4"
        }}),
        None,
    )
}

#[test]
fn copilot_cards_launch_terminals_and_never_write_native_settings() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CopilotHome::new();
    let state = create_test_state().unwrap();
    let original = "{\n  \"model\": \"claude-sonnet-4.6\",\n  \"theme\": \"dark\"\n}";
    fs::write(home.path("settings.json"), original).unwrap();

    assert!(ProviderService::add(&state, AppType::Copilot, byok("relay"), true).unwrap());
    let error = ProviderService::switch(&state, AppType::Copilot, "relay").unwrap_err();
    assert!(error.to_string().contains("Open terminal"), "{error}");

    // Saving validates the native environment contract before touching the database.
    let mut missing_model = byok("broken");
    missing_model.settings_config["env"]
        .as_object_mut()
        .unwrap()
        .remove("COPILOT_MODEL");
    assert!(ProviderService::add(&state, AppType::Copilot, missing_model, true).is_err());

    ProviderService::delete(&state, AppType::Copilot, "relay").unwrap();
    assert_eq!(
        fs::read_to_string(home.path("settings.json")).unwrap(),
        original
    );
    for entry in fs::read_dir(home.directory.path()).unwrap().flatten() {
        let text = fs::read_to_string(entry.path()).unwrap_or_default();
        assert!(!text.contains("fixture-key"), "{}", entry.path().display());
    }
}

#[test]
fn copilot_import_uses_the_signed_in_model_only_when_present() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CopilotHome::new();
    let state = create_test_state().unwrap();
    fs::write(home.path("settings.json"), "{\"theme\":\"dark\"}").unwrap();
    assert!(!ProviderService::import_default_config(&state, AppType::Copilot).unwrap());

    fs::write(
        home.path("settings.json"),
        "// user comment\n{\"model\":\"gpt-5.6-terra\"}",
    )
    .unwrap();
    assert!(ProviderService::import_default_config(&state, AppType::Copilot).unwrap());
    let providers = ProviderService::list(&state, AppType::Copilot).unwrap();
    let imported = &providers["default"];
    assert_eq!(imported.category.as_deref(), Some("official"));
    assert_eq!(imported.name, "GitHub Copilot");
    assert_eq!(imported.icon.as_deref(), Some("githubcopilot"));
    assert_eq!(
        imported.settings_config["env"]["COPILOT_MODEL"],
        "gpt-5.6-terra"
    );
}

#[test]
fn copilot_mcp_skills_and_prompts_follow_the_native_home() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = CopilotHome::new();
    let state = create_test_state().unwrap();
    // An unmanaged native server must survive CC Switch writes.
    fs::write(
        home.path("mcp-config.json"),
        json!({"mcpServers":{"native":{"type":"local","command":"node","args":["x.js"],"tools":["*"]}}})
            .to_string(),
    )
    .unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type":"stdio","command":"npx","args":["-y","pkg"]}),
        apps: McpApps {
            copilot: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    let read = || -> serde_json::Value {
        serde_json::from_slice(&fs::read(home.path("mcp-config.json")).unwrap()).unwrap()
    };
    let value = read();
    assert_eq!(value["mcpServers"]["test"]["type"], "local");
    assert_eq!(value["mcpServers"]["test"]["tools"], json!(["*"]));
    assert_eq!(value["mcpServers"]["native"]["command"], "node");

    McpService::toggle_app(&state, "test", AppType::Copilot, false).unwrap();
    let value = read();
    assert!(value["mcpServers"].get("test").is_none());
    assert!(value["mcpServers"].get("native").is_some());

    // Import marks servers listed in disabledMcpServers as not enabled for Copilot.
    fs::write(
        home.path("settings.json"),
        json!({"disabledMcpServers":["native"]}).to_string(),
    )
    .unwrap();
    McpService::import_from_all_apps(&state).ok();
    let servers = McpService::get_all_servers(&state).unwrap();
    assert!(!servers["native"].apps.copilot);

    let prompt: Prompt = serde_json::from_value(
        json!({"id":"global","name":"Global","content":"Prefer small commits.","enabled":false}),
    )
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::Copilot, "global", prompt).unwrap();
    PromptService::enable_prompt(&state, AppType::Copilot, "global").unwrap();
    assert_eq!(
        fs::read_to_string(home.path("copilot-instructions.md")).unwrap(),
        "Prefer small commits."
    );
}
