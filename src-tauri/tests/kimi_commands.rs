use cc_switch_lib::{
    AppType, McpApps, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::json;
use std::{ffi::OsString, fs};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct KimiHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl KimiHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("KIMI_CODE_HOME");
        std::env::set_var("KIMI_CODE_HOME", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn config(&self) -> std::path::PathBuf {
        self.directory.path().join("config.toml")
    }
}
impl Drop for KimiHome {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("KIMI_CODE_HOME", value);
        } else {
            std::env::remove_var("KIMI_CODE_HOME");
        }
    }
}

fn provider(id: &str, model: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({"provider":{"type":"openai","base_url":"https://example.com/v1","api_key":"test"},"models":{model:{"model":"vendor/model","max_context_size":128000}}}),
        None,
    )
}

#[test]
fn kimi_provider_lifecycle_imports_native_config_and_tracks_external_removal() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let native = "# user settings\ndefault_model = 'official'\n[providers.official]\ntype = 'kimi'\nbase_url = 'https://api.kimi.com/coding/v1'\napi_key = ''\n[providers.official.oauth]\nkey = 'kimi-code'\nstorage = 'file'\n[models.official]\nprovider = 'official'\nmodel = 'kimi-for-coding'\nmax_context_size = 262144\n[thinking]\neffort = 'high'\n";
    fs::write(home.config(), native).unwrap();
    assert!(ProviderService::list(&state, AppType::Kimi)
        .unwrap()
        .contains_key("official"));
    ProviderService::add(
        &state,
        AppType::Kimi,
        provider("custom", "custom/model"),
        false,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(home.config()).unwrap(), native);
    assert_eq!(
        ProviderService::list(&state, AppType::Kimi).unwrap()["custom"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(false)
    );
    ProviderService::switch(&state, AppType::Kimi, "custom").unwrap();
    let mut saved = ProviderService::list(&state, AppType::Kimi).unwrap()["custom"].clone();
    assert_eq!(saved.meta.as_ref().unwrap().live_config_managed, Some(true));
    saved.settings_config["provider"]["base_url"] = json!("https://updated.example.com/v1");
    ProviderService::update(&state, AppType::Kimi, None, saved).unwrap();
    let text = fs::read_to_string(home.config()).unwrap();
    assert!(text.contains("updated.example.com"));
    assert!(text.contains("# user settings"));
    ProviderService::remove_from_live_config(&state, AppType::Kimi, "custom").unwrap();
    assert_eq!(
        ProviderService::list(&state, AppType::Kimi).unwrap()["custom"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(false)
    );
    ProviderService::delete(&state, AppType::Kimi, "custom").unwrap();
    assert!(!state
        .db
        .get_all_providers("kimi")
        .unwrap()
        .contains_key("custom"));
    let actual: toml::Value = toml::from_str(&fs::read_to_string(home.config()).unwrap()).unwrap();
    let expected: toml::Value = toml::from_str(native).unwrap();
    assert_eq!(actual, expected);
    // Native removal changes membership, while the reusable CCS profile remains saved.
    fs::write(home.config(), "[thinking]\neffort='low'\n").unwrap();
    let imported = ProviderService::list(&state, AppType::Kimi).unwrap();
    assert_eq!(
        imported["official"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed,
        Some(false)
    );
}

#[test]
fn kimi_provider_add_rejects_collisions_and_protects_the_default_model() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    ProviderService::add(
        &state,
        AppType::Kimi,
        provider("first", "global/model"),
        true,
    )
    .unwrap();
    let bytes = fs::read(home.config()).unwrap();
    assert!(ProviderService::add(
        &state,
        AppType::Kimi,
        provider("first", "another/model"),
        true
    )
    .is_err());
    assert!(ProviderService::add(
        &state,
        AppType::Kimi,
        provider("second", "global/model"),
        true
    )
    .is_err());
    assert!(state
        .db
        .get_provider_by_id("second", "kimi")
        .unwrap()
        .is_none());
    assert!(ProviderService::remove_from_live_config(&state, AppType::Kimi, "first").is_err());
    assert_eq!(fs::read(home.config()).unwrap(), bytes);
}

#[test]
fn kimi_import_and_mutations_preserve_native_capabilities() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let native_provider = |id: &str| {
        let mut entry = provider(id, &format!("{id}/model"));
        entry.settings_config["models"][format!("{id}/model")]["capabilities"] = json!([
            "audio_in",
            "image_in",
            "video_in",
            "image_out",
            "audio_out",
            "future_capability",
            "thinking",
            "tool_use"
        ]);
        entry.settings_config["models"][format!("{id}/model")]["overrides"] = json!({"capabilities":["video_in", "audio_in", "image_in", "future_override"], "default_effort":"max"});
        entry
    };
    let native = native_provider("native");
    let mut model = native.settings_config["models"]["native/model"].clone();
    model["provider"] = json!("native");
    let document = json!({"default_model":"native/model", "providers":{"native":native.settings_config["provider"]},"models":{"native/model":model}});
    fs::write(home.config(), toml::to_string(&document).unwrap()).unwrap();
    let original = fs::read(home.config()).unwrap();
    state
        .db
        .save_provider("kimi", &native_provider("saved"))
        .unwrap();
    let entries = ProviderService::list(&state, AppType::Kimi).unwrap();
    for id in ["native", "saved"] {
        let model = &entries[id].settings_config["models"][format!("{id}/model")];
        assert_eq!(
            model["capabilities"],
            native_provider(id).settings_config["models"][format!("{id}/model")]["capabilities"]
        );
        assert_eq!(
            model["overrides"],
            native_provider(id).settings_config["models"][format!("{id}/model")]["overrides"]
        );
        assert_eq!(
            state
                .db
                .get_provider_by_id(id, "kimi")
                .unwrap()
                .unwrap()
                .settings_config,
            entries[id].settings_config
        );
    }
    assert_eq!(fs::read(home.config()).unwrap(), original);
    ProviderService::switch(&state, AppType::Kimi, "saved").unwrap();
    let mut edited = entries["native"].clone();
    edited.settings_config["provider"]["base_url"] = json!("https://updated.example.com/v1");
    ProviderService::update(&state, AppType::Kimi, None, edited).unwrap();
    ProviderService::add(&state, AppType::Kimi, native_provider("new"), false).unwrap();
    let saved: serde_json::Value =
        toml::from_str(&fs::read_to_string(home.config()).unwrap()).unwrap();
    for id in ["native", "saved"] {
        assert_eq!(
            saved["models"][format!("{id}/model")]["capabilities"],
            native_provider(id).settings_config["models"][format!("{id}/model")]["capabilities"]
        );
        assert_eq!(
            saved["models"][format!("{id}/model")]["overrides"]["capabilities"],
            native_provider(id).settings_config["models"][format!("{id}/model")]["overrides"]
                ["capabilities"]
        );
    }
    assert_eq!(
        state
            .db
            .get_provider_by_id("new", "kimi")
            .unwrap()
            .unwrap()
            .settings_config["models"]["new/model"]["capabilities"],
        native_provider("new").settings_config["models"]["new/model"]["capabilities"]
    );
}

#[test]
fn kimi_mcp_and_global_prompts_follow_the_native_home() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let server = McpServer {
        id: "test".into(),
        name: "Test".into(),
        server: json!({"type":"sse","url":"https://example.com/sse"}),
        apps: McpApps {
            kimi: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    McpService::upsert_server(&state, server).unwrap();
    let path = home.directory.path().join("mcp.json");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["test"]["transport"], "sse");
    McpService::toggle_app(&state, "test", AppType::Kimi, false).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(value["mcpServers"].get("test").is_none());
    let prompt:Prompt=serde_json::from_value(json!({"id":"global","name":"Global","content":"Use the project conventions.","enabled":false})).unwrap();
    PromptService::upsert_prompt(&state, AppType::Kimi, "global", prompt).unwrap();
    PromptService::enable_prompt(&state, AppType::Kimi, "global").unwrap();
    assert_eq!(
        fs::read_to_string(home.directory.path().join("AGENTS.md")).unwrap(),
        "Use the project conventions."
    );
}
