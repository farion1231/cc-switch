use super::{import_opencode_providers_from_live, ProviderService};
use crate::app_config::AppType;
use crate::database::Database;
use crate::opencode_config;
use crate::provider::Provider;
use crate::settings::{get_settings, update_settings, AppSettings};
use crate::store::AppState;
use serde_json::json;
use serial_test::serial;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

struct Fixture {
    previous_settings: AppSettings,
    previous_env: Vec<(&'static str, Option<OsString>)>,
    config_path: PathBuf,
    state: AppState,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let previous_env = ["CC_SWITCH_TEST_HOME", "OPENCODE_DB"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        std::env::set_var("OPENCODE_DB", dir.path().join("opencode.db"));
        let previous_settings = get_settings();
        let config_dir = dir.path().join("opencode");
        fs::create_dir_all(&config_dir).unwrap();
        update_settings(AppSettings {
            opencode_config_dir: Some(config_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
        Self {
            previous_settings,
            previous_env,
            config_path: config_dir.join("opencode.json"),
            state: AppState::new(Arc::new(Database::memory().unwrap())),
            _dir: dir,
        }
    }

    fn imported(&self) -> Provider {
        self.state
            .db
            .get_provider_by_id("opencode-go", "opencode")
            .unwrap()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Restore the cached settings while writes still target the temporary home.
        update_settings(self.previous_settings.clone()).unwrap();
        for (key, value) in &self.previous_env {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

const KEY_ONLY_CONFIG: &str = r#"{
  // Keep this comment and the user's formatting during import.
  "provider": {"opencode-go": {"options": {"apiKey": "{env:OPENCODE_API_KEY}"}}}
}"#;

#[test]
#[serial]
fn opencode_builtin_import_is_read_only_and_existing_override_is_editable() {
    let fixture = Fixture::new();
    fs::write(&fixture.config_path, KEY_ONLY_CONFIG).unwrap();
    let original_permissions = fs::metadata(&fixture.config_path).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&fixture.config_path, read_only).unwrap();

    let imported = import_opencode_providers_from_live(&fixture.state);
    fs::set_permissions(&fixture.config_path, original_permissions).unwrap();
    assert_eq!(imported.unwrap(), 1);
    assert_eq!(
        import_opencode_providers_from_live(&fixture.state).unwrap(),
        0
    );
    assert_eq!(
        fs::read_to_string(&fixture.config_path).unwrap(),
        KEY_ONLY_CONFIG
    );

    let mut provider = fixture.imported();
    assert!(provider.settings_config.get("npm").is_none());
    assert!(provider.settings_config["options"].get("baseURL").is_none());
    provider.settings_config["options"]["apiKey"] = json!("edited-test-key");
    ProviderService::update(&fixture.state, AppType::OpenCode, None, provider).unwrap();
    let saved = opencode_config::get_providers().unwrap();
    assert_eq!(saved["opencode-go"]["options"]["apiKey"], "edited-test-key");
    assert!(saved["opencode-go"].get("npm").is_none());
    assert!(saved["opencode-go"]["options"].get("baseURL").is_none());
}

#[test]
#[serial]
fn opencode_builtin_partial_model_override_can_omit_display_name() {
    let fixture = Fixture::new();
    opencode_config::set_provider(
        "opencode-go",
        json!({"models": {"glm-5": {"limit": {"context": 100000, "output": 10000}}}}),
    )
    .unwrap();
    assert_eq!(
        import_opencode_providers_from_live(&fixture.state).unwrap(),
        1
    );
    let mut provider = fixture.imported();
    assert!(provider.settings_config["models"]["glm-5"]
        .get("name")
        .is_none());
    provider.settings_config["options"]["apiKey"] = json!("edited-test-key");
    ProviderService::update(&fixture.state, AppType::OpenCode, None, provider).unwrap();
    let saved = opencode_config::get_providers().unwrap();
    assert!(saved["opencode-go"]["models"]["glm-5"]
        .get("name")
        .is_none());
    assert_eq!(
        saved["opencode-go"]["models"]["glm-5"]["limit"]["context"],
        100000
    );
}

#[test]
#[serial]
fn opencode_builtin_incomplete_copies_cannot_be_added_to_live() {
    let fixture = Fixture::new();
    fs::write(&fixture.config_path, KEY_ONLY_CONFIG).unwrap();
    for (index, config) in [
        json!({"options": {"apiKey": "test-key"}}),
        json!({"npm": "@ai-sdk/openai-compatible", "models": {}}),
        json!({"models": {"glm-5": {"name": "GLM 5"}}}),
        json!({"npm": "  ", "models": {"glm-5": {"name": "GLM 5"}}}),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("opencode-go-copy-{index}");
        let provider = Provider::with_id(id.clone(), id.clone(), config, None);
        ProviderService::add(&fixture.state, AppType::OpenCode, provider, false).unwrap();
        let error = ProviderService::switch(&fixture.state, AppType::OpenCode, &id).unwrap_err();
        assert!(error.to_string().contains("npm"));
        assert_eq!(
            fs::read_to_string(&fixture.config_path).unwrap(),
            KEY_ONLY_CONFIG
        );
        let saved = fixture
            .state
            .db
            .get_provider_by_id(&id, "opencode")
            .unwrap()
            .unwrap();
        assert_eq!(saved.meta.unwrap().live_config_managed, Some(false));
    }

    let complete = Provider::with_id(
        "custom-copy".into(),
        "Complete copy".into(),
        json!({"npm": "@ai-sdk/openai-compatible", "models": {"glm-5": {"name": "GLM 5"}}}),
        None,
    );
    ProviderService::add(&fixture.state, AppType::OpenCode, complete, false).unwrap();
    ProviderService::switch(&fixture.state, AppType::OpenCode, "custom-copy").unwrap();
    assert!(opencode_config::get_providers()
        .unwrap()
        .contains_key("custom-copy"));
}

#[test]
#[serial]
fn opencode_builtin_removed_override_cannot_reuse_live_membership() {
    let fixture = Fixture::new();
    fs::write(&fixture.config_path, KEY_ONLY_CONFIG).unwrap();
    import_opencode_providers_from_live(&fixture.state).unwrap();
    opencode_config::remove_provider("opencode-go").unwrap();
    let previous = fs::read(&fixture.config_path).unwrap();
    let error =
        ProviderService::switch(&fixture.state, AppType::OpenCode, "opencode-go").unwrap_err();
    assert!(error.to_string().contains("npm"));
    assert_eq!(fs::read(&fixture.config_path).unwrap(), previous);
}

#[test]
#[serial]
fn opencode_builtin_credential_only_import_does_not_create_config() {
    let fixture = Fixture::new();
    let db_path = opencode_config::get_opencode_db_path();
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch(
        "CREATE TABLE credential (id TEXT, integration_id TEXT, label TEXT, value TEXT,
            connector_id TEXT, active INTEGER, time_created INTEGER);
         INSERT INTO credential VALUES ('go', 'opencode-go', 'default',
            '{\"type\":\"key\",\"key\":\"credential-test-key\"}', NULL, 1, 1);",
    )
    .unwrap();
    drop(db);
    let original_db = fs::read(&db_path).unwrap();
    assert_eq!(
        import_opencode_providers_from_live(&fixture.state).unwrap(),
        0
    );
    assert!(!fixture.config_path.exists());
    assert!(fixture
        .state
        .db
        .get_all_providers("opencode")
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(&db_path).unwrap(), original_db);
}

#[cfg(unix)]
#[test]
#[serial]
fn opencode_builtin_import_preserves_symlink() {
    let fixture = Fixture::new();
    let target = fixture.config_path.with_file_name("dotfiles.json");
    fs::write(&target, KEY_ONLY_CONFIG).unwrap();
    std::os::unix::fs::symlink(&target, &fixture.config_path).unwrap();
    assert_eq!(
        import_opencode_providers_from_live(&fixture.state).unwrap(),
        1
    );
    assert!(fs::symlink_metadata(&fixture.config_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), KEY_ONLY_CONFIG);
}

#[test]
#[serial]
fn opencode_jsonc_provider_and_mcp_workflows_preserve_comments() {
    let fixture = Fixture::new();
    let path = fixture.config_path.with_extension("jsonc");
    let other = r#"{"provider":{"ignored":{"options":{"apiKey":"json-only"}}},"mcp":{"ignored":{"type":"local","command":["ignored"]}}}"#;
    fs::write(&fixture.config_path, other).unwrap();
    let source = r#"{
  /* 用户配置 */
  "model": "keep-this", // 默认模型
  "provider": {"opencode-go": {"options": {"apiKey": "old"}}},
  "mcp": {"existing": {"type": "local", "command": ["npx", "old"], "enabled": true}},
}"#;
    fs::write(&path, source).unwrap();
    assert_eq!(
        import_opencode_providers_from_live(&fixture.state).unwrap(),
        1
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    let mut provider = fixture.imported();
    provider.settings_config["options"]["apiKey"] = json!("updated");
    ProviderService::update(&fixture.state, AppType::OpenCode, None, provider).unwrap();
    let added = Provider::with_id(
        "custom".into(),
        "Custom".into(),
        json!({
            "npm":"@ai-sdk/openai-compatible", "models":{"test":{"name":"Test"}}
        }),
        None,
    );
    ProviderService::add(&fixture.state, AppType::OpenCode, added, false).unwrap();
    ProviderService::switch(&fixture.state, AppType::OpenCode, "custom").unwrap();
    assert!(opencode_config::get_providers()
        .unwrap()
        .contains_key("custom"));
    assert_eq!(
        opencode_config::get_providers().unwrap()["opencode-go"]["options"]["apiKey"],
        "updated"
    );
    ProviderService::remove_from_live_config(&fixture.state, AppType::OpenCode, "custom").unwrap();
    ProviderService::delete(&fixture.state, AppType::OpenCode, "opencode-go").unwrap();
    assert!(opencode_config::get_providers().unwrap().is_empty());

    let mut config = crate::app_config::MultiAppConfig::default();
    let before_import = fs::read(&path).unwrap();
    assert_eq!(crate::mcp::import_from_opencode(&mut config).unwrap(), 1);
    assert_eq!(fs::read(&path).unwrap(), before_import);
    assert_eq!(
        config.mcp.servers.as_ref().unwrap()["existing"].server["command"],
        "npx"
    );
    for (id, spec) in [
        ("existing", json!({"command":"npx","args":["updated"]})),
        (
            "added",
            json!({"type":"http","url":"https://example.com/mcp"}),
        ),
    ] {
        crate::mcp::sync_single_server_to_opencode(&config, id, &spec).unwrap();
    }
    let servers = opencode_config::get_mcp_servers().unwrap();
    assert_eq!(servers["existing"]["command"], json!(["npx", "updated"]));
    assert_eq!(servers["added"]["type"], "remote");
    for id in ["existing", "added"] {
        crate::mcp::remove_server_from_opencode(id).unwrap();
    }
    assert!(opencode_config::get_mcp_servers().unwrap().is_empty());
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("/* 用户配置 */"));
    assert!(saved.contains("\"model\": \"keep-this\", // 默认模型"));
    assert_eq!(fs::read_to_string(&fixture.config_path).unwrap(), other);
}

#[test]
#[serial]
fn opencode_jsonc_omo_registration_removal_and_failure_rollback() {
    use crate::services::omo::{OmoService, STANDARD};
    let fixture = Fixture::new();
    let path = fixture.config_path.with_extension("jsonc");
    let other = "{\"plugin\":[\"ignored\"]}";
    fs::write(&fixture.config_path, other).unwrap();
    let source = "{/* OpenCode 注释 */\"model\":\"keep\",\"plugin\":[\"unrelated\"]}";
    fs::write(&path, source).unwrap();
    let unified = fixture._dir.path().join(".omo/omo.jsonc");
    fs::create_dir_all(unified.parent().unwrap()).unwrap();
    let original_omo = "{/* OMO 注释 */\"[codex]\":{\"agents\":{}},\"_migrations\":[\"keep\"]}";
    fs::write(&unified, original_omo).unwrap();
    OmoService::write_config_to_file(&fixture.state, &STANDARD).unwrap();
    assert_eq!(
        opencode_config::read_opencode_config().unwrap()["plugin"],
        json!(["unrelated", "oh-my-openagent@latest"])
    );
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("/* OpenCode 注释 */"));
    assert!(fs::read_to_string(&unified)
        .unwrap()
        .contains("/* OMO 注释 */"));
    OmoService::delete_config_file(&STANDARD).unwrap();
    assert_eq!(
        opencode_config::read_opencode_config().unwrap()["plugin"],
        json!(["unrelated"])
    );
    let omo: serde_json::Value = json5::from_str(&fs::read_to_string(&unified).unwrap()).unwrap();
    assert!(omo.get("[opencode]").is_none());
    assert_eq!(omo["_migrations"], json!(["keep"]));

    // Plugin sync failure must restore the exact OMO bytes for both enable and disable.
    for original in [
        original_omo,
        "{/*keep*/\"[opencode]\":{\"agents\":{\"old\":{}}},\"[codex]\":{}}",
    ] {
        fs::write(&unified, original).unwrap();
        fs::write(&path, "{broken").unwrap();
        assert!(OmoService::write_config_to_file(&fixture.state, &STANDARD).is_err());
        assert_eq!(fs::read_to_string(&unified).unwrap(), original);
        assert!(OmoService::delete_config_file(&STANDARD).is_err());
        assert_eq!(fs::read_to_string(&unified).unwrap(), original);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{broken");
    }
    assert_eq!(fs::read_to_string(&fixture.config_path).unwrap(), other);
}
