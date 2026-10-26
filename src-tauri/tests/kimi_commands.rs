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

fn live_config(home: &KimiHome) -> serde_json::Value {
    let text = fs::read_to_string(home.config()).unwrap();
    serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap()
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
fn kimi_official_accounts_edit_models_leave_kimi_and_are_deleted_only_after_leaving() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let mut config = json!({"default_model":"custom/model", "providers":{"custom":{"type":"openai","api_key":"test"}},"models":{"custom/model":{"provider":"custom","model":"test","max_context_size":32000}}});
    let credentials = home.directory.path().join("credentials");
    fs::create_dir_all(&credentials).unwrap();
    for id in ["managed:kimi-code", "kimi-code-ccs-second"] {
        let slot = id.replace(':', "-");
        config["providers"][id] = json!({"type":"kimi","base_url":"https://api.kimi.com/coding/v1","oauth":{"storage":"file","key":format!("oauth/{slot}")}});
        config["models"][format!("{id}/k3")] =
            json!({"provider":id,"model":"k3","max_context_size":1048576});
        fs::write(
            credentials.join(format!("{slot}.json")),
            r#"{"access_token":"a","refresh_token":"r","expires_at":4102444800}"#,
        )
        .unwrap();
    }
    config["services"]["moonshot_search"] = json!({"base_url":"https://api.kimi.com/coding/v1/search","api_key":"","oauth":{"storage":"file","key":"oauth/kimi-code-ccs-second"}});
    fs::write(home.config(), toml::to_string(&config).unwrap()).unwrap();
    let entries = ProviderService::list(&state, AppType::Kimi).unwrap();
    for id in ["managed:kimi-code", "kimi-code-ccs-second"] {
        assert_eq!(entries[id].category.as_deref(), Some("official"));
        assert!(entries[id].name.starts_with("Kimi Official"));
        // Still in Kimi: deleting goes through the Authorization Center.
        let original = fs::read(home.config()).unwrap();
        assert!(ProviderService::delete(&state, AppType::Kimi, id).is_err());
        assert_eq!(fs::read(home.config()).unwrap(), original);

        // Models are editable; the signed-in connection is not.
        let mut edited = entries[id].clone();
        edited.settings_config["provider"] = json!({"type":"openai","api_key":"replacement"});
        edited.settings_config["models"] =
            json!({format!("{id}/k3.1"):{"provider":id,"model":"k3.1","max_context_size":1048576}});
        ProviderService::update(&state, AppType::Kimi, None, edited).unwrap();
        let live = live_config(&home);
        assert_eq!(live["providers"][id], config["providers"][id]);
        assert!(live["models"].get(format!("{id}/k3")).is_none());
        assert_eq!(live["models"][format!("{id}/k3.1")]["model"], "k3.1");
    }

    // No edit turns another card into an official account.
    let mut forged = entries["custom"].clone();
    forged.settings_config["provider"] = config["providers"]["managed:kimi-code"].clone();
    assert!(ProviderService::update(&state, AppType::Kimi, None, forged).is_err());

    // Removing keeps the card and login; services move to Kimi's own login.
    ProviderService::remove_from_live_config(&state, AppType::Kimi, "kimi-code-ccs-second")
        .unwrap();
    let live = live_config(&home);
    assert!(live["providers"].get("kimi-code-ccs-second").is_none());
    assert!(live["models"].get("kimi-code-ccs-second/k3.1").is_none());
    assert_eq!(
        live["services"]["moonshot_search"]["oauth"]["key"],
        "oauth/managed-kimi-code"
    );
    assert!(credentials.join("kimi-code-ccs-second.json").exists());
    let saved = state
        .db
        .get_provider_by_id("kimi-code-ccs-second", "kimi")
        .unwrap()
        .unwrap();
    assert_eq!(saved.meta.unwrap().live_config_managed, Some(false));
    ProviderService::add_to_live_config(&state, AppType::Kimi, "kimi-code-ccs-second").unwrap();
    let live = live_config(&home);
    assert_eq!(
        live["providers"]["kimi-code-ccs-second"],
        config["providers"]["kimi-code-ccs-second"]
    );
    assert_eq!(live["models"]["kimi-code-ccs-second/k3.1"]["model"], "k3.1");

    fs::write(home.config(), "").unwrap();
    let entries = ProviderService::list(&state, AppType::Kimi).unwrap();
    for id in ["managed:kimi-code", "kimi-code-ccs-second"] {
        assert_eq!(
            entries[id].meta.as_ref().unwrap().live_config_managed,
            Some(false)
        );
        ProviderService::remove_from_live_config(&state, AppType::Kimi, id).unwrap();
        ProviderService::delete(&state, AppType::Kimi, id).unwrap();
        assert!(state.db.get_provider_by_id(id, "kimi").unwrap().is_none());
    }
    assert_eq!(fs::read_to_string(home.config()).unwrap(), "");
}

#[test]
fn kimi_official_cards_need_a_local_login_and_take_their_own_credential_with_them() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let custom = json!({"type":"openai","api_key":"test"});
    let mut config = json!({"default_model":"custom/model", "providers":{"custom":custom},"models":{"custom/model":{"provider":"custom","model":"test","max_context_size":32000}}});
    for (id, key) in [
        ("managed:kimi-code", "kimi-code"),
        ("kimi-code-ccs-second", "kimi-code-ccs-second"),
    ] {
        config["providers"][id] = json!({"type":"kimi","base_url":"https://api.kimi.com/coding/v1","oauth":{"storage":"file","key":format!("oauth/{key}")}});
        config["models"][format!("{id}/k3")] =
            json!({"provider":id,"model":"k3","max_context_size":1048576});
    }
    fs::write(home.config(), toml::to_string(&config).unwrap()).unwrap();
    ProviderService::list(&state, AppType::Kimi).unwrap();

    // A card whose login is not on this machine (e.g. synced from another
    // device) must not point Kimi at a missing credential.
    let original = fs::read(home.config()).unwrap();
    let error = ProviderService::switch(&state, AppType::Kimi, "kimi-code-ccs-second").unwrap_err();
    assert!(
        error.to_string().contains("Authorization Center")
            || error.to_string().contains("授权中心")
    );
    assert_eq!(fs::read(home.config()).unwrap(), original);

    let credentials = home.directory.path().join("credentials");
    fs::create_dir_all(&credentials).unwrap();
    let valid = json!({"access_token":"a","refresh_token":"r","expires_at":4_102_444_800i64});
    for name in ["kimi-code", "kimi-code-ccs-second"] {
        fs::write(
            credentials.join(format!("{name}.json")),
            serde_json::to_vec(&valid).unwrap(),
        )
        .unwrap();
    }
    ProviderService::switch(&state, AppType::Kimi, "kimi-code-ccs-second").unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Kimi).unwrap(),
        "kimi-code-ccs-second"
    );
    ProviderService::switch(&state, AppType::Kimi, "custom").unwrap();

    // Kimi dropped both nodes: deleting the cards also deletes CC Switch's own
    // credential slot, but never Kimi's default login.
    let only_custom = json!({"default_model":"custom/model", "providers":{"custom":custom},"models":{"custom/model":{"provider":"custom","model":"test","max_context_size":32000}}});
    fs::write(home.config(), toml::to_string(&only_custom).unwrap()).unwrap();
    ProviderService::list(&state, AppType::Kimi).unwrap();
    for id in ["managed:kimi-code", "kimi-code-ccs-second"] {
        ProviderService::delete(&state, AppType::Kimi, id).unwrap();
    }
    assert!(credentials.join("kimi-code.json").exists());
    assert!(!credentials.join("kimi-code-ccs-second.json").exists());
}

#[test]
fn kimi_official_import_repairs_existing_display_identity_without_rewriting_native_config() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let native = "# native OAuth\ndefault_model = 'kimi-code/k3'\n[providers.'managed:kimi-code']\ntype = 'kimi'\nbase_url = 'https://api.kimi.com/coding/v1'\napi_key = ''\n[providers.'managed:kimi-code'.oauth]\nkey = 'oauth/kimi-code'\nstorage = 'file'\n[models.'kimi-code/k3']\nprovider = 'managed:kimi-code'\nmodel = 'k3'\nmax_context_size = 1048576\n";
    fs::write(home.config(), native).unwrap();
    let id = "managed:kimi-code";
    let mut legacy = Provider::with_id(id.into(), id.into(), json!({}), None);
    legacy.notes = Some("my note".into());
    state.db.save_provider("kimi", &legacy).unwrap();
    let providers = ProviderService::list(&state, AppType::Kimi).unwrap();
    let official = &providers[id];
    assert_eq!(official.name, "Kimi Official");
    assert_eq!(official.icon.as_deref(), Some("kimi"));
    assert_eq!(official.category.as_deref(), Some("official"));
    assert_eq!(
        official.website_url.as_deref(),
        Some("https://www.kimi.com")
    );
    assert_eq!(official.notes.as_deref(), Some("my note"));
    assert_eq!(
        official.settings_config["models"]["kimi-code/k3"]["provider"],
        id
    );
    assert_eq!(ProviderService::current(&state, AppType::Kimi).unwrap(), id);
    assert_eq!(fs::read_to_string(home.config()).unwrap(), native);
    let persisted = state.db.get_provider_by_id(id, "kimi").unwrap().unwrap();
    assert_eq!(persisted.name, "Kimi Official");
    assert_eq!(persisted.category.as_deref(), Some("official"));
    assert_eq!(persisted.icon.as_deref(), Some("kimi"));
}

#[test]
fn kimi_provider_lifecycle_imports_native_config_and_tracks_external_removal() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    let native = "# user settings\ndefault_model = 'official'\n[providers.official]\ntype = 'kimi'\nbase_url = 'https://api.kimi.com/coding/v1'\napi_key = ''\n[providers.official.oauth]\nkey = 'kimi-code'\nstorage = 'file'\n[models.official]\nprovider = 'official'\nmodel = 'kimi-for-coding'\nmax_context_size = 262144\n[thinking]\neffort = 'high'\n";
    fs::write(home.config(), native).unwrap();
    // Switching back to an official account needs its login on this machine.
    fs::create_dir_all(home.directory.path().join("credentials")).unwrap();
    fs::write(
        home.directory.path().join("credentials/kimi-code.json"),
        r#"{"access_token":"a","refresh_token":"r","expires_at":4102444800}"#,
    )
    .unwrap();
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
    assert_eq!(
        ProviderService::current(&state, AppType::Kimi).unwrap(),
        "custom"
    );
    // Select the original native provider before removing the now-current one.
    ProviderService::switch(&state, AppType::Kimi, "official").unwrap();
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
fn kimi_switch_selects_native_model_and_current_tracks_external_changes() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let home = KimiHome::new();
    let state = create_test_state().unwrap();
    ProviderService::add(
        &state,
        AppType::Kimi,
        provider("first", "alias-without-provider-prefix"),
        true,
    )
    .unwrap();
    ProviderService::add(
        &state,
        AppType::Kimi,
        provider("second", "another-alias"),
        false,
    )
    .unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Kimi).unwrap(),
        "first"
    );
    ProviderService::switch(&state, AppType::Kimi, "second").unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Kimi).unwrap(),
        "second"
    );
    assert!(ProviderService::remove_from_live_config(&state, AppType::Kimi, "second").is_err());
    let mut native: toml::Value =
        toml::from_str(&fs::read_to_string(home.config()).unwrap()).unwrap();
    native["default_model"] = toml::Value::String("alias-without-provider-prefix".into());
    native["providers"]["first"]["base_url"] =
        toml::Value::String("https://edited-in-kimi.example/v1".into());
    fs::write(home.config(), toml::to_string(&native).unwrap()).unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Kimi).unwrap(),
        "first"
    );
    ProviderService::switch(&state, AppType::Kimi, "first").unwrap();
    let saved = state
        .db
        .get_provider_by_id("first", "kimi")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.settings_config["provider"]["base_url"],
        "https://edited-in-kimi.example/v1"
    );
    native["default_model"] = toml::Value::String("missing-alias".into());
    fs::write(home.config(), toml::to_string(&native).unwrap()).unwrap();
    assert!(ProviderService::current(&state, AppType::Kimi)
        .unwrap()
        .is_empty());
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
