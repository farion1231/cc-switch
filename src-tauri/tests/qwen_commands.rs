use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use std::{ffi::OsString, fs};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct QwenHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl QwenHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("QWEN_HOME");
        std::env::set_var("QWEN_HOME", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn settings(&self) -> std::path::PathBuf {
        self.directory.path().join("settings.json")
    }
    fn read(&self) -> Value {
        json5::from_str(&fs::read_to_string(self.settings()).unwrap()).unwrap()
    }
}
impl Drop for QwenHome {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("QWEN_HOME", value);
        } else {
            std::env::remove_var("QWEN_HOME");
        }
    }
}

fn provider(id: &str, base_url: &str, model: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "authType": "openai",
            "baseUrl": base_url,
            "apiKey": format!("sk-{id}"),
            "models": [{"id": model, "name": model, "generationConfig": {"contextWindowSize": 131072}}],
        }),
        None,
    )
}

const NATIVE: &str = r#"{
  // user settings
  "$version": 4,
  "general": { "vimMode": true },
  "security": { "auth": { "selectedType": "openai" } },
  "model": { "name": "gpt-x" },
  "modelProviders": {
    "openai": [
      { "id": "gpt-x", "name": "GPT X", "baseUrl": "https://api.openai.com/v1", "envKey": "OPENAI_API_KEY" }
    ]
  }
}
"#;

#[test]
fn qwen_provider_lifecycle_imports_native_entries_and_keeps_user_settings() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = QwenHome::new();
    let state = create_test_state().unwrap();
    fs::write(home.settings(), NATIVE).unwrap();

    let listed = ProviderService::list(&state, AppType::Qwen).unwrap();
    let native = &listed["openai-api-key"];
    assert_eq!(native.settings_config["envKey"], "OPENAI_API_KEY");
    assert_eq!(
        native.settings_config["baseUrl"],
        "https://api.openai.com/v1"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Qwen).unwrap(),
        "openai-api-key"
    );

    ProviderService::add(
        &state,
        AppType::Qwen,
        provider(
            "bailian",
            "https://dashscope.example/v1",
            "qwen3-coder-plus",
        ),
        false,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(home.settings()).unwrap(), NATIVE);

    ProviderService::switch(&state, AppType::Qwen, "bailian").unwrap();
    let value = home.read();
    assert_eq!(value["env"]["CC_SWITCH_QWEN_BAILIAN"], "sk-bailian");
    assert_eq!(value["model"]["name"], "qwen3-coder-plus");
    assert_eq!(value["model"]["baseUrl"], "https://dashscope.example/v1");
    assert_eq!(value["general"]["vimMode"], true);
    assert!(fs::read_to_string(home.settings())
        .unwrap()
        .contains("// user settings"));
    assert_eq!(
        ProviderService::current(&state, AppType::Qwen).unwrap(),
        "bailian"
    );

    let mut saved = ProviderService::list(&state, AppType::Qwen).unwrap()["bailian"].clone();
    assert_eq!(saved.meta.as_ref().unwrap().live_config_managed, Some(true));
    saved.settings_config["baseUrl"] = json!("https://updated.example/v1");
    ProviderService::update(&state, AppType::Qwen, None, saved).unwrap();
    let value = home.read();
    assert_eq!(
        value["modelProviders"]["openai"][1]["baseUrl"],
        "https://updated.example/v1"
    );
    // Editing the provider in use keeps the selection pointing at it.
    assert_eq!(value["model"]["baseUrl"], "https://updated.example/v1");

    // The provider in use cannot be removed; switch back first.
    assert!(ProviderService::remove_from_live_config(&state, AppType::Qwen, "bailian").is_err());
    ProviderService::switch(&state, AppType::Qwen, "openai-api-key").unwrap();
    ProviderService::remove_from_live_config(&state, AppType::Qwen, "bailian").unwrap();
    assert_eq!(
        ProviderService::list(&state, AppType::Qwen).unwrap()["bailian"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(false)
    );
    ProviderService::delete(&state, AppType::Qwen, "bailian").unwrap();
    assert!(state
        .db
        .get_provider_by_id("bailian", "qwen")
        .unwrap()
        .is_none());

    let value = home.read();
    let original: Value = json5::from_str(NATIVE).unwrap();
    assert_eq!(value["modelProviders"], original["modelProviders"]);
    assert!(value.get("env").is_none());
    assert_eq!(value["model"]["name"], "gpt-x");

    // Native removal keeps the reusable CCS profile, now unmanaged.
    fs::write(home.settings(), "{}").unwrap();
    let listed = ProviderService::list(&state, AppType::Qwen).unwrap();
    assert_eq!(
        listed["openai-api-key"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(false)
    );
}

#[test]
fn qwen_add_rejects_collisions_and_first_provider_becomes_current() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = QwenHome::new();
    let state = create_test_state().unwrap();
    ProviderService::add(
        &state,
        AppType::Qwen,
        provider("first", "https://a.example/v1", "m"),
        true,
    )
    .unwrap();
    assert_eq!(home.read()["$version"], 4);
    assert_eq!(
        ProviderService::current(&state, AppType::Qwen).unwrap(),
        "first"
    );
    let bytes = fs::read(home.settings()).unwrap();
    assert!(ProviderService::add(
        &state,
        AppType::Qwen,
        provider("first", "https://b.example/v1", "n"),
        true
    )
    .is_err());
    // Same model at the same endpoint under another key would be ambiguous.
    assert!(ProviderService::add(
        &state,
        AppType::Qwen,
        provider("second", "https://a.example/v1", "m"),
        true
    )
    .is_err());
    assert!(state
        .db
        .get_provider_by_id("second", "qwen")
        .unwrap()
        .is_none());
    assert_eq!(fs::read(home.settings()).unwrap(), bytes);

    // A second provider with the same model id at another endpoint is fine and
    // does not steal the current selection.
    ProviderService::add(
        &state,
        AppType::Qwen,
        provider("third", "https://c.example/v1", "m"),
        true,
    )
    .unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Qwen).unwrap(),
        "first"
    );
    ProviderService::switch(&state, AppType::Qwen, "third").unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Qwen).unwrap(),
        "third"
    );
}

#[test]
fn qwen_mcp_and_prompt_use_native_locations() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = QwenHome::new();
    let state = create_test_state().unwrap();
    fs::write(home.settings(), NATIVE).unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type":"http","url":"https://example.com/mcp"}),
        apps: McpApps {
            qwen: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    let value = home.read();
    assert_eq!(
        value["mcpServers"]["test"]["httpUrl"],
        "https://example.com/mcp"
    );
    assert!(fs::read_to_string(home.settings())
        .unwrap()
        .contains("// user settings"));
    McpService::toggle_app(&state, "test", AppType::Qwen, false).unwrap();
    assert!(home.read()["mcpServers"].get("test").is_none());

    let prompt: Prompt = serde_json::from_value(
        json!({"id":"global","name":"Global","content":"Use the project conventions.","enabled":false}),
    )
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::Qwen, "global", prompt).unwrap();
    PromptService::enable_prompt(&state, AppType::Qwen, "global").unwrap();
    assert_eq!(
        fs::read_to_string(home.directory.path().join("QWEN.md")).unwrap(),
        "Use the project conventions."
    );
}
