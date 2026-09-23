//! MCode TUI and desktop share this file. MCode owns model selection.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) fn data_dir() -> PathBuf {
    explicit_data_dir(
        std::env::var("MINIMAX_DATA_DIR").ok().as_deref(),
        std::env::var("MAVIS_DATA_DIR").ok().as_deref(),
    )
    .unwrap_or_else(|| get_home_dir().join(".minimax"))
}

fn explicit_data_dir(minimax: Option<&str>, mavis: Option<&str>) -> Option<PathBuf> {
    [minimax, mavis]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|path| !path.is_empty())
        .map(PathBuf::from)
}

// Callers hold their feature lock across the native write and database commit.
pub(crate) fn write_and_commit<T>(
    path: &Path,
    write: impl FnOnce() -> Result<(), AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let previous = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io(path, error)),
    };
    write()?;
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            let rollback = match previous {
                Some(bytes) => atomic_write_private(path, &bytes),
                None => fs::remove_file(path).map_err(|error| AppError::io(path, error)),
            };
            if let Err(rollback_error) = rollback {
                return Err(AppError::Message(format!(
                    "MiniMax Code update failed ({error}); restoring {} also failed: {rollback_error}",
                    path.display()
                )));
            }
            Err(error)
        }
    }
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("config.yaml")
}

fn read(path: &Path) -> Result<serde_yaml::Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(AppError::Message(format!(
                "Cannot read MCode configuration: {e}"
            )))
        }
    };
    let value: serde_yaml::Value = serde_yaml::from_str(&text)
        .map_err(|_| AppError::Config("Invalid MCode YAML configuration".into()))?;
    if value.is_null() {
        return Ok(serde_yaml::Value::Mapping(Default::default()));
    }
    if !value.is_mapping() {
        return Err(AppError::Config(
            "MCode configuration must be a mapping".into(),
        ));
    }
    Ok(value)
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read(&config_path())?;
    match document.get("custom_provider") {
        None | Some(serde_yaml::Value::Null) => Ok(IndexMap::new()),
        Some(value) => {
            let mut providers: IndexMap<String, Value> = serde_yaml::from_value(value.clone())
                .map_err(|_| {
                    AppError::Config("Invalid MCode custom_provider configuration".into())
                })?;
            providers.retain(|_, provider| {
                provider
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "custom")
            });
            Ok(providers)
        }
    }
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.is_empty()
        || id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
    {
        return Err(AppError::InvalidInput(
            "MCode provider key must contain letters, digits, '-' or '_'".into(),
        ));
    }
    if !matches!(
        config
            .get("api")
            .map_or(Some("anthropic-messages"), Value::as_str),
        Some("anthropic-messages" | "openai-completions" | "openai-responses")
    ) {
        return Err(AppError::InvalidInput(
            "Select a supported MCode API format".into(),
        ));
    }
    let base_url = config
        .pointer("/options/baseURL")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !url::Url::parse(base_url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid MCode endpoint URL".into(),
        ));
    }
    if config
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .is_none_or(|key| key.trim().is_empty())
    {
        return Err(AppError::InvalidInput("Enter an API key".into()));
    }
    if config
        .get("models")
        .and_then(Value::as_object)
        .is_none_or(|models| models.is_empty() || models.keys().any(|id| id.trim().is_empty()))
    {
        return Err(AppError::InvalidInput(
            "Add at least one MCode model".into(),
        ));
    }
    Ok(())
}

// MCode's proper-lockfile uses the same atomic directory lock.
struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn update(path: &Path, id: &str, provider: Option<Value>) -> Result<(), AppError> {
    fs::create_dir_all(path.parent().expect("MCode configuration directory"))
        .map_err(|e| AppError::Message(format!("Cannot create MCode directory: {e}")))?;
    let path = if path.exists() {
        fs::canonicalize(path).map_err(|e| AppError::Message(e.to_string()))?
    } else {
        path.to_path_buf()
    };
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    if let Err(error) = fs::create_dir(&lock_path) {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(AppError::io(&lock_path, error));
        }
        let stale = fs::metadata(&lock_path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(10));
        if !stale {
            return Err(AppError::Conflict(
                "MCode configuration is busy; retry after MCode finishes saving".into(),
            ));
        }
        fs::remove_dir(&lock_path).map_err(|e| AppError::io(&lock_path, e))?;
        fs::create_dir(&lock_path).map_err(|e| AppError::io(&lock_path, e))?;
    }
    let _lock = ConfigLock(lock_path);
    let mut document = read(&path)?;
    let prefix = format!("custom_provider:{id}/");
    if ["defaultModel", "defaultLightModel"].iter().any(|field| {
        let selected = document
            .get(*field)
            .and_then(serde_yaml::Value::as_str)
            .and_then(|model| model.strip_prefix(&prefix));
        selected.is_some_and(|model| {
            !provider.as_ref().is_some_and(|provider| {
                provider.get("enabled") != Some(&Value::Bool(false))
                    && provider
                        .get("models")
                        .and_then(|models| models.get(model))
                        .is_some_and(|model| {
                            model.is_object() && model.get("enabled") != Some(&Value::Bool(false))
                        })
            })
        })
    }) {
        return Err(AppError::InvalidInput(
            "Select another default model in MCode before removing this model or provider".into(),
        ));
    }
    let root = document.as_mapping_mut().expect("Validated mapping");
    let entry = root
        .entry("custom_provider".into())
        .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
    if entry.is_null() {
        *entry = serde_yaml::Value::Mapping(Default::default());
    }
    let providers = entry
        .as_mapping_mut()
        .ok_or_else(|| AppError::Config("Invalid MCode custom_provider configuration".into()))?;
    if providers
        .get(serde_yaml::Value::from(id))
        .and_then(|value| value.get("kind"))
        .and_then(serde_yaml::Value::as_str)
        .is_some_and(|kind| kind != "custom")
    {
        return Err(AppError::InvalidInput(
            "MCode owns this account provider".into(),
        ));
    }
    if let Some(provider) = provider {
        providers.insert(
            id.into(),
            serde_yaml::to_value(provider)
                .map_err(|_| AppError::Config("Invalid MCode provider".into()))?,
        );
    } else {
        providers.remove(serde_yaml::Value::from(id));
    }
    let yaml = serde_yaml::to_string(&document)
        .map_err(|_| AppError::Config("Cannot serialize MCode configuration".into()))?;
    atomic_write_private(&path, yaml.as_bytes())
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    update(&config_path(), id, Some(config))
}
pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    if !config_path().exists() {
        return Ok(());
    }
    update(&config_path(), id, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_data_directory_precedence_and_blank_values() {
        assert_eq!(
            explicit_data_dir(Some(" /primary "), Some("/legacy")),
            Some("/primary".into())
        );
        assert_eq!(
            explicit_data_dir(Some(" \t"), Some(" /legacy ")),
            Some("/legacy".into())
        );
        assert_eq!(explicit_data_dir(None, Some("")), None);
        assert_eq!(explicit_data_dir(None, None), None);
    }

    #[test]
    #[cfg(unix)]
    fn reclaims_a_stale_native_lock_but_preserves_an_active_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let lock = path.with_extension("yaml.lock");
        fs::create_dir(&lock).unwrap();
        assert!(update(&path, "test", Some(json!({}))).is_err());
        assert!(lock.exists());
        fs::File::open(&lock)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(11))
            .unwrap();
        update(&path, "test", Some(json!({"name":"Recovered"}))).unwrap();
        assert!(!lock.exists());
        assert_eq!(
            read(&path).unwrap()["custom_provider"]["test"]["name"],
            "Recovered"
        );
    }

    #[test]
    fn native_provider_can_omit_api_format() {
        let mut provider = json!({"options":{"baseURL":"https://example.com","apiKey":"test-key"},"models":{"model":{}}});
        validate_provider("native", &provider).unwrap();
        for api in [json!(null), json!(true), json!("unsupported")] {
            provider["api"] = api;
            assert!(validate_provider("native", &provider).is_err());
        }
    }
    #[test]
    fn additive_changes_preserve_other_providers_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, "defaultModel: minimax/MiniMax-M3\ncustom_provider:\n  existing:\n    name: Keep\nminimax_api:\n  apiKey: keep-secret\nunknown: [a, b]\n").unwrap();
        let before = read(&path).unwrap();
        let provider = json!({"api":"anthropic-messages","options":{"baseURL":"https://api.minimaxi.com/anthropic","apiKey":"test-key"},"models":{"MiniMax-M3":{}}});
        validate_provider("cc-switch-minimax", &provider).unwrap();
        update(&path, "cc-switch-minimax", Some(provider)).unwrap();
        let after = read(&path).unwrap();
        for field in ["defaultModel", "minimax_api", "unknown"] {
            assert_eq!(before[field], after[field]);
        }
        assert_eq!(
            before["custom_provider"]["existing"],
            after["custom_provider"]["existing"]
        );
        update(&path, "cc-switch-minimax", None).unwrap();
        assert_eq!(before, read(&path).unwrap());
    }
    #[test]
    fn respects_native_lock_and_selected_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let text = "defaultModel: custom_provider:chosen/model\ncustom_provider:\n  chosen: {}\n";
        fs::write(&path, text).unwrap();
        fs::create_dir(path.with_extension("yaml.lock")).unwrap();
        assert!(update(&path, "other", Some(json!({}))).is_err());
        fs::remove_dir(path.with_extension("yaml.lock")).unwrap();
        assert!(update(&path, "chosen", None).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), text);
    }
    #[test]
    fn preserves_default_and_light_model_availability() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        for field in ["defaultModel", "defaultLightModel"] {
            let text = format!("{field}: custom_provider:chosen/model\ncustom_provider:\n  chosen:\n    models:\n      model: {{}}\n");
            fs::write(&path, &text).unwrap();
            for replacement in [
                None,
                Some(json!({"models":{"other":{}}})),
                Some(json!({"enabled":false,"models":{"model":{}}})),
                Some(json!({"models":{"model":{"enabled":false}}})),
            ] {
                assert!(update(&path, "chosen", replacement).is_err());
                assert_eq!(fs::read_to_string(&path).unwrap(), text);
            }
            update(&path, "chosen", Some(json!({"models":{"model":{}}}))).unwrap();
            assert_eq!(read(&path).unwrap()[field], "custom_provider:chosen/model");
        }
    }
    /// Run alone with an isolated CC_SWITCH_TEST_HOME and MCODE_TEST_API_KEY.
    #[test]
    #[ignore = "requires an isolated home and a real API key"]
    fn mcode_provider_lifecycle() {
        use crate::{
            app_config::AppType, database::Database, provider::Provider,
            services::provider::ProviderService, store::AppState,
        };
        use std::sync::Arc;
        let home = std::env::var("CC_SWITCH_TEST_HOME").expect("Set an isolated home");
        assert!(Some(PathBuf::from(&home)) != dirs::home_dir());
        let key = std::env::var("MCODE_TEST_API_KEY").expect("Set a real API key");
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let config = json!({"name":"MCode validation", "kind":"custom", "enabled":true, "api":"openai-completions", "options":{"baseURL":"https://api.minimaxi.com/v1","apiKey":key},"models":{"MiniMax-M3":{"name":"MiniMax-M3"}}});
        let mut provider = Provider::with_id(
            "cc-switch-validation".into(),
            "MCode validation".into(),
            config,
            None,
        );
        provider.category = Some("custom".into());
        fs::create_dir_all(config_path().parent().unwrap()).unwrap();
        fs::write(config_path(), "custom_provider:\n  keep:\n    kind: custom\n    enabled: false\n    name: Keep\n    models: {}\nunknown: [a, b]\n").unwrap();
        let before = read(&config_path()).unwrap();
        let lock_path = config_path().with_extension("yaml.lock");
        fs::create_dir(&lock_path).unwrap();
        assert!(ProviderService::add(&state, AppType::Mcode, provider.clone(), true).is_err());
        assert!(state.db.get_all_providers("mcode").unwrap().is_empty());
        fs::remove_dir(lock_path).unwrap();
        ProviderService::add(&state, AppType::Mcode, provider.clone(), false)
            .expect("Save catalog entry");
        assert!(!get_providers().unwrap().contains_key(&provider.id));
        ProviderService::switch(&state, AppType::Mcode, &provider.id)
            .expect("Add to live configuration");
        assert!(get_providers().unwrap().contains_key(&provider.id));
        assert!(ProviderService::current(&state, AppType::Mcode)
            .unwrap()
            .is_empty());
        provider.name = "Renamed validation".into();
        provider.settings_config["name"] = json!(provider.name);
        ProviderService::update(&state, AppType::Mcode, None, provider.clone())
            .expect("Edit live provider");
        assert!(get_providers().unwrap()[&provider.id]["name"] == provider.name);
        let selected = format!("custom_provider:{}/MiniMax-M3", provider.id);
        let mut document = read(&config_path()).unwrap();
        document["defaultModel"] = selected.into();
        fs::write(config_path(), serde_yaml::to_string(&document).unwrap()).unwrap();
        let previous = state
            .db
            .get_provider_by_id(&provider.id, "mcode")
            .unwrap()
            .unwrap();
        let mut rejected = provider.clone();
        rejected.settings_config["models"] = json!({"other":{}});
        assert!(ProviderService::update(&state, AppType::Mcode, None, rejected).is_err());
        let restored = state
            .db
            .get_provider_by_id(&provider.id, "mcode")
            .unwrap()
            .unwrap();
        assert!(restored.settings_config == previous.settings_config);
        assert!(read(&config_path()).unwrap() == document);
        document
            .as_mapping_mut()
            .unwrap()
            .remove(serde_yaml::Value::from("defaultModel"));
        fs::write(config_path(), serde_yaml::to_string(&document).unwrap()).unwrap();
        ProviderService::remove_from_live_config(&state, AppType::Mcode, &provider.id)
            .expect("Remove from live");
        assert!(!get_providers().unwrap().contains_key(&provider.id));
        ProviderService::switch(&state, AppType::Mcode, &provider.id).expect("Re-add to live");
        ProviderService::delete(&state, AppType::Mcode, &provider.id).expect("Delete provider");
        assert!(!get_providers().unwrap().contains_key(&provider.id));
        assert!(read(&config_path()).unwrap() == before);
        ProviderService::add(&state, AppType::Mcode, provider.clone(), true)
            .expect("Prepare real MCode run");
        let listed = ProviderService::list(&state, AppType::Mcode)
            .expect("Import and refresh native providers");
        assert!(listed.contains_key("keep"));
        assert!(
            listed[&provider.id]
                .meta
                .as_ref()
                .unwrap()
                .live_config_managed
                == Some(true)
        );
        provider.settings_config["name"] = json!("Renamed in MCode");
        set_provider(&provider.id, provider.settings_config.clone()).unwrap();
        let mut refreshed = ProviderService::list(&state, AppType::Mcode).unwrap();
        let mut edited = refreshed.swap_remove(&provider.id).unwrap();
        assert_eq!(edited.name, "Renamed in MCode");
        edited.notes = Some("Edited in CC Switch".into());
        ProviderService::update(&state, AppType::Mcode, None, edited).unwrap();
        assert_eq!(
            get_providers().unwrap()[&provider.id]["name"],
            "Renamed in MCode"
        );
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use crate::{
        app_config::{AppType, McpApps, McpServer},
        database::Database,
        services::{prompt::PromptService, skill::SkillService, McpService},
        store::AppState,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    #[ignore = "runs alone in an isolated CC_SWITCH_TEST_HOME; prepares real MCode validation"]
    fn mcode_capability_lifecycle() {
        let home = std::env::var("CC_SWITCH_TEST_HOME").expect("isolated home");
        assert_ne!(Some(PathBuf::from(&home)), dirs::home_dir());
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        fs::create_dir_all(data_dir()).unwrap();
        let mcp_path = data_dir().join("mcp.json");
        let original = json!({"extension":42,"mcpServers":{"keep":{"type":"sse","url":"https://example.com/sse","enabled":false}}});
        fs::write(&mcp_path, original.to_string()).unwrap();
        let server = McpServer {
            id: "cc-switch-validation".into(),
            name: "Validation".into(),
            server: json!({"type":"stdio","command":std::env::var("MCODE_TEST_NODE").unwrap(),"args":[std::env::var("MCODE_TEST_MCP_SERVER").unwrap()]}),
            apps: McpApps {
                mcode: true,
                ..Default::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        };
        McpService::upsert_server(&state, server.clone()).unwrap();
        assert!(db.get_all_mcp_servers().unwrap()[&server.id].apps.mcode);
        McpService::toggle_app(&state, &server.id, AppType::Mcode, false).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&mcp_path).unwrap()).unwrap(),
            original
        );
        McpService::toggle_app(&state, &server.id, AppType::Mcode, true).unwrap();
        McpService::delete_server(&state, &server.id).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&mcp_path).unwrap()).unwrap(),
            original
        );
        McpService::upsert_server(&state, server).unwrap();
        crate::mcp::mcode::import(&state).unwrap();
        assert!(!db.get_all_mcp_servers().unwrap()["keep"].apps.mcode);
        let path = crate::prompt_files::prompt_file_path(&AppType::Mcode).unwrap();
        fs::write(
            &path,
            "For validation, write GLOBAL-INSTRUCTION-OK to global-proof.txt in the project.\n",
        )
        .unwrap();
        let original_instructions = fs::read_to_string(&path).unwrap();
        let draft = serde_json::from_value(
            json!({"id":"draft","name":"Draft","content":"Draft instructions","enabled":false}),
        )
        .unwrap();
        PromptService::upsert_prompt(&state, AppType::Mcode, "draft", draft).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original_instructions);
        PromptService::delete_prompt(&state, AppType::Mcode, "draft").unwrap();
        assert_eq!(
            PromptService::import_from_file_on_first_launch(&state, AppType::Mcode).unwrap(),
            1
        );
        let prompts = PromptService::get_prompts(&state, AppType::Mcode).unwrap();
        let (id, prompt) = prompts.first().unwrap();
        PromptService::enable_prompt(&state, AppType::Mcode, id).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), prompt.content);
        let mut oversized = prompt.clone();
        oversized.id = "oversized".into();
        oversized.content = "中".repeat(10923);
        assert!(PromptService::upsert_prompt(
            &state,
            AppType::Mcode,
            "oversized",
            oversized.clone()
        )
        .is_err());
        assert_eq!(db.get_prompts("mcode").unwrap().len(), 1);
        oversized.enabled = false;
        db.save_prompt("mcode", &oversized).unwrap();
        assert!(PromptService::enable_prompt(&state, AppType::Mcode, "oversized").is_err());
        assert!(db.get_prompts("mcode").unwrap()[id].enabled);
        assert!(!db.get_prompts("mcode").unwrap()["oversized"].enabled);
        db.delete_prompt("mcode", id).unwrap();
        oversized.enabled = true;
        db.save_prompt("mcode", &oversized).unwrap();
        assert!(PromptService::sync_to_live(&state, AppType::Mcode).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), prompt.content);
        db.delete_prompt("mcode", "oversized").unwrap();
        db.save_prompt("mcode", prompt).unwrap();
        let mut disabled = prompt.clone();
        disabled.enabled = false;
        PromptService::upsert_prompt(&state, AppType::Mcode, id, disabled).unwrap();
        assert!(fs::read_to_string(&path).unwrap().is_empty());
        PromptService::enable_prompt(&state, AppType::Mcode, id).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original_instructions);
        let skill_dir = SkillService::get_ssot_dir()
            .unwrap()
            .join("cc-switch-validation");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"),"---\nname: cc-switch-validation\ndescription: Use when asked to validate the CC Switch MCode integration.\n---\nCall the cc-switch-validation MCP tool validation_proof. Write its exact result into mcp-proof.txt. Write SKILL-INSTRUCTION-OK into skill-proof.txt. Fix the project bug and run its test.\n").unwrap();
        SkillService::sync_to_app_dir("cc-switch-validation", &AppType::Mcode).unwrap();
        assert!(data_dir()
            .join("skills/cc-switch-validation/SKILL.md")
            .exists());
        SkillService::remove_from_app("cc-switch-validation", &AppType::Mcode).unwrap();
        assert!(!data_dir().join("skills/cc-switch-validation").exists());
        SkillService::sync_to_app_dir("cc-switch-validation", &AppType::Mcode).unwrap();
    }
}
