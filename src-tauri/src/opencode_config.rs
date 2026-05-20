use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const STANDARD_OMO_PLUGIN_PREFIXES: [&str; 2] = ["oh-my-openagent", "oh-my-opencode"];
const SLIM_OMO_PLUGIN_PREFIXES: [&str; 1] = ["oh-my-opencode-slim"];

fn matches_plugin_prefix(plugin_name: &str, prefix: &str) -> bool {
    plugin_name == prefix
        || plugin_name
            .strip_prefix(prefix)
            .map(|suffix| suffix.starts_with('@'))
            .unwrap_or(false)
}

fn matches_any_plugin_prefix(plugin_name: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| matches_plugin_prefix(plugin_name, prefix))
}

fn canonicalize_plugin_name(plugin_name: &str) -> String {
    if let Some(suffix) = plugin_name.strip_prefix("oh-my-opencode") {
        if suffix.is_empty() || suffix.starts_with('@') {
            return format!("oh-my-openagent{suffix}");
        }
    }
    plugin_name.to_string()
}

pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("opencode")
}

pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenCode config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    write_json_file(&path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_opencode_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    let normalized_plugin_name = canonicalize_plugin_name(plugin_name);

    let plugins = config.get_mut("plugin").and_then(|v| v.as_array_mut());

    match plugins {
        Some(arr) => {
            // Mutual exclusion: standard OMO and OMO Slim cannot coexist as plugins
            if matches_any_plugin_prefix(&normalized_plugin_name, &STANDARD_OMO_PLUGIN_PREFIXES) {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            } else if matches_any_plugin_prefix(&normalized_plugin_name, &SLIM_OMO_PLUGIN_PREFIXES)
            {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            }

            let already_exists = arr
                .iter()
                .any(|v| v.as_str() == Some(normalized_plugin_name.as_str()));
            if !already_exists {
                arr.push(Value::String(normalized_plugin_name));
            }
        }
        None => {
            config["plugin"] = json!([normalized_plugin_name]);
        }
    }

    write_opencode_config(&config)
}

pub fn remove_plugins_by_prefixes(prefixes: &[&str]) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(arr) = config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        arr.retain(|v| {
            v.as_str()
                .map(|s| !matches_any_plugin_prefix(s, prefixes))
                .unwrap_or(true)
        });

        if arr.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    write_opencode_config(&config)
}

pub fn get_opencode_auth_path() -> PathBuf {
    get_opencode_data_dir().join("auth.json")
}

/// OpenCode data directory for credential storage.
/// Resolves to `~/.local/share/opencode`, respecting CC_SWITCH_TEST_HOME.
pub fn get_opencode_data_dir() -> PathBuf {
    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
}

pub fn read_opencode_auth() -> Result<Map<String, Value>, AppError> {
    let path = get_opencode_auth_path();

    if !path.exists() {
        return Ok(Map::new());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let parsed: Value = serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))?;

    match parsed {
        Value::Object(map) => Ok(map),
        other => Err(AppError::Config(format!(
            "OpenCode auth.json must be a JSON object, got {}",
            json_type_name(&other)
        ))),
    }
}

pub fn write_opencode_auth(auth: &Map<String, Value>) -> Result<(), AppError> {
    let path = get_opencode_auth_path();
    let value = Value::Object(auth.clone());
    write_json_file(&path, &value)?;

    log::debug!("OpenCode auth written to {:?}", path);
    Ok(())
}

pub fn get_opencode_auth_entry(provider_id: &str) -> Result<Option<Value>, AppError> {
    let auth = read_opencode_auth()?;
    Ok(auth.get(provider_id).cloned())
}

pub fn set_opencode_auth_entry(provider_id: &str, entry: Value) -> Result<(), AppError> {
    if !entry.is_object() {
        return Err(AppError::Config(format!(
            "OpenCode auth.json entry for '{provider_id}' must be a JSON object, got {}",
            json_type_name(&entry)
        )));
    }
    let mut auth = read_opencode_auth()?;
    auth.insert(provider_id.to_string(), entry);
    write_opencode_auth(&auth)
}

pub fn remove_opencode_auth_entry(provider_id: &str) -> Result<(), AppError> {
    let mut auth = read_opencode_auth()?;
    if auth.remove(provider_id).is_some() {
        write_opencode_auth(&auth)?;
    }
    Ok(())
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use serial_test::serial;
    use std::fs;

    struct TempHome {
        dir: PathBuf,
        old_var: Option<std::ffi::OsString>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cc-switch-auth-test-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = fs::create_dir_all(&dir);
            let home = dir.join("home");
            let _ = fs::create_dir_all(home.join(".local").join("share").join("opencode"));
            let _ = fs::create_dir_all(home.join(".config").join("opencode"));

            let old_var = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", home.to_str().unwrap());

            Self { dir, old_var }
        }

        fn auth_path(&self) -> PathBuf {
            self.dir
                .join("home")
                .join(".local")
                .join("share")
                .join("opencode")
                .join("auth.json")
        }

        fn opencode_data_dir(&self) -> PathBuf {
            self.dir.join("home").join(".local").join("share").join("opencode")
        }

        fn opencode_config_dir(&self) -> PathBuf {
            self.dir.join("home").join(".config").join("opencode")
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.old_var {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    #[serial]
    fn read_missing_auth_returns_empty_map() {
        let _th = TempHome::new();
        let result = read_opencode_auth().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    #[serial]
    fn read_missing_auth_does_not_create_directory() {
        let th = TempHome::new();
        let data_dir = th.opencode_data_dir();
        let _ = fs::remove_dir_all(&data_dir);
        assert!(!data_dir.exists());

        let result = read_opencode_auth().unwrap();
        assert!(result.is_empty());
        assert!(
            !data_dir.exists(),
            "read_opencode_auth must not create the opencode data directory"
        );
    }

    #[test]
    #[serial]
    fn read_valid_auth_file() {
        let th = TempHome::new();
        fs::write(
            th.auth_path(),
            r#"{"orouter-byok": {"type": "api", "key": "FAKE_KEY"}}"#,
        )
        .unwrap();

        let auth = read_opencode_auth().unwrap();
        assert_eq!(auth.len(), 1);
        assert!(auth.contains_key("orouter-byok"));
    }

    #[test]
    #[serial]
    fn read_invalid_json_returns_error() {
        let th = TempHome::new();
        fs::write(th.auth_path(), "{invalid json}").unwrap();

        let result = read_opencode_auth();
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn read_non_object_json_returns_error() {
        let th = TempHome::new();
        fs::write(th.auth_path(), "[1, 2, 3]").unwrap();

        let result = read_opencode_auth();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("must be a JSON object"));
    }

    #[test]
    #[serial]
    fn set_and_get_auth_entry() {
        let _th = TempHome::new();
        let entry = json!({"type": "api", "key": "FAKE_KEY"});
        set_opencode_auth_entry("test-provider", entry.clone()).unwrap();

        let got = get_opencode_auth_entry("test-provider").unwrap();
        assert_eq!(got, Some(entry));
    }

    #[test]
    #[serial]
    fn set_entry_preserves_other_entries() {
        let th = TempHome::new();
        fs::write(
            th.auth_path(),
            r#"{"existing": {"type": "api", "key": "FAKE_KEY"}}"#,
        )
        .unwrap();

        set_opencode_auth_entry(
            "new-provider",
            json!({"type": "api", "key": "TEST_KEY"}),
        )
        .unwrap();

        let auth = read_opencode_auth().unwrap();
        assert!(auth.contains_key("existing"));
        assert!(auth.contains_key("new-provider"));
    }

    #[test]
    #[serial]
    fn remove_entry_preserves_other_entries() {
        let th = TempHome::new();
        fs::write(
            th.auth_path(),
            r#"{"a": {"type": "api", "key": "FAKE_KEY"}, "b": {"type": "api", "key": "FAKE_KEY"}}"#,
        )
        .unwrap();

        remove_opencode_auth_entry("a").unwrap();

        let auth = read_opencode_auth().unwrap();
        assert!(!auth.contains_key("a"));
        assert!(auth.contains_key("b"));
    }

    #[test]
    #[serial]
    fn remove_nonexistent_entry_is_noop() {
        let th = TempHome::new();
        fs::write(
            th.auth_path(),
            r#"{"a": {"type": "api", "key": "FAKE_KEY"}}"#,
        )
        .unwrap();

        remove_opencode_auth_entry("nonexistent").unwrap();

        let auth = read_opencode_auth().unwrap();
        assert_eq!(auth.len(), 1);
        assert!(auth.contains_key("a"));
    }

    #[test]
    #[serial]
    fn write_creates_parent_dir_if_missing() {
        let th = TempHome::new();
        let data_dir = th.opencode_data_dir();
        let _ = fs::remove_dir_all(&data_dir);

        set_opencode_auth_entry("fresh", json!({"type": "api", "key": "FAKE_KEY"})).unwrap();

        let got = get_opencode_auth_entry("fresh").unwrap();
        assert!(got.is_some());
    }

    #[test]
    #[serial]
    fn unknown_object_fields_are_preserved() {
        let th = TempHome::new();
        fs::write(
            th.auth_path(),
            r#"{"custom": {"unusual": true, "nested": {"deep": 42}}}"#,
        )
        .unwrap();

        let entry = get_opencode_auth_entry("custom").unwrap();
        assert!(entry.is_some());
        let val = entry.unwrap();
        assert_eq!(val["unusual"], json!(true));
        assert_eq!(val["nested"]["deep"], json!(42));
    }

    #[test]
    #[serial]
    fn auth_path_does_not_use_config_dir() {
        let th = TempHome::new();
        set_opencode_auth_entry("regression", json!({"type": "api", "key": "FAKE_KEY"})).unwrap();

        let config_dir_auth = th.opencode_config_dir().join("auth.json");
        assert!(
            !config_dir_auth.exists(),
            "auth.json must not be written under .config/opencode"
        );

        let data_dir_auth = th.auth_path();
        assert!(
            data_dir_auth.exists(),
            "auth.json must be written under .local/share/opencode"
        );
    }

    #[test]
    #[serial]
    fn set_auth_entry_rejects_non_object() {
        let _th = TempHome::new();
        let result = set_opencode_auth_entry("bad-provider", Value::String("FAKE_RAW_AUTH".to_string()));
        assert!(result.is_err(), "non-object auth entry should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("must be a JSON object"),
            "error should mention object requirement, got: {err_msg}"
        );
    }
}
