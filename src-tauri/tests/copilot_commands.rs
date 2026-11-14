use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::json;
use std::{ffi::OsString, fs};

#[path = "support/mod.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct CopilotHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
    previous_providers: Option<OsString>,
}
impl CopilotHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("COPILOT_HOME");
        let previous_providers = std::env::var_os("COPILOT_PROVIDERS_CONFIG");
        std::env::set_var("COPILOT_HOME", directory.path());
        // providers.json follows COPILOT_HOME only while this override is unset.
        std::env::remove_var("COPILOT_PROVIDERS_CONFIG");
        Self {
            directory,
            previous,
            previous_providers,
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
        if let Some(value) = &self.previous_providers {
            std::env::set_var("COPILOT_PROVIDERS_CONFIG", value);
        }
    }
}

fn byok(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "provider": {"type":"openai","baseUrl":"https://example.com/v1","apiKey":"fixture-key"},
            "models": [{"id":"gpt-5.4"}],
            "reasoningEffort": "high"
        }),
        None,
    )
}

#[test]
fn copilot_byok_cards_live_in_providers_json_and_official_cards_never_write() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = CopilotHome::new();
    let state = create_test_state().unwrap();
    let original = "{\n  \"model\": \"claude-sonnet-4.6\",\n  \"theme\": \"dark\"\n}";
    fs::write(home.path("settings.json"), original).unwrap();
    // Copilot's own entries (e.g. Foundry Local) must survive CC Switch writes.
    let foreign = json!({
        "providers": [{"name":"foundry","baseUrl":"http://localhost:5272/v1"}],
        "models": [{"id":"phi","provider":"foundry"}]
    });
    fs::write(home.path("providers.json"), foreign.to_string()).unwrap();

    assert!(ProviderService::add(&state, AppType::Copilot, byok("relay"), true).unwrap());
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(home.path("providers.json")).unwrap()).unwrap();
    assert_eq!(written["providers"][0], foreign["providers"][0]);
    assert_eq!(written["providers"][1]["name"], "relay");
    assert_eq!(
        written["models"][1],
        json!({"id":"gpt-5.4","provider":"relay"})
    );
    assert!(!written.to_string().contains("reasoningEffort"));

    // Saving validates the card before touching the database or the file.
    let mut missing_model = byok("broken");
    missing_model.settings_config["models"] = json!([]);
    assert!(ProviderService::add(&state, AppType::Copilot, missing_model, true).is_err());
    assert!(ProviderService::list(&state, AppType::Copilot)
        .unwrap()
        .get("broken")
        .is_none());

    // Official cards only open terminals: saved, never written, cannot be added.
    let mut official = Provider::with_id(
        "login".into(),
        "GitHub Copilot".into(),
        json!({"env":{"COPILOT_MODEL":"gpt-5-mini"}}),
        None,
    );
    official.category = Some("official".into());
    assert!(ProviderService::add(&state, AppType::Copilot, official, true).unwrap());
    let error = ProviderService::switch(&state, AppType::Copilot, "login").unwrap_err();
    assert!(error.to_string().contains("Open terminal"), "{error}");

    ProviderService::delete(&state, AppType::Copilot, "relay").unwrap();
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(home.path("providers.json")).unwrap()).unwrap();
    assert_eq!(after, foreign);
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
    // 刚装好的 Copilot 还没有 settings.json：没什么可导入，不报错。
    assert!(!ProviderService::import_default_config(&state, AppType::Copilot).unwrap());
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
