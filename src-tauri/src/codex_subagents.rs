use std::collections::BTreeSet;
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

use crate::config::{
    atomic_write, delete_file, get_app_config_dir, read_json_file, write_json_file,
};
use crate::error::AppError;

const SETTINGS_FILE_NAME: &str = "codex-subagent-settings.json";
const MAX_MODEL_LENGTH: usize = 256;

pub const CODEX_REASONING_EFFORTS: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "ultra"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentSettings {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentSettingsView {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub available_models: Vec<String>,
    pub config_path: String,
}

fn settings_path() -> std::path::PathBuf {
    get_app_config_dir().join(SETTINGS_FILE_NAME)
}

fn normalize_settings(
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<CodexSubagentSettings, AppError> {
    let model = model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if model
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_MODEL_LENGTH)
    {
        return Err(AppError::InvalidInput(format!(
            "Codex subagent model must be at most {MAX_MODEL_LENGTH} characters"
        )));
    }

    let reasoning_effort = reasoning_effort
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(effort) = reasoning_effort.as_deref() {
        if !CODEX_REASONING_EFFORTS.contains(&effort) {
            return Err(AppError::InvalidInput(format!(
                "Unsupported Codex subagent reasoning effort: {effort}"
            )));
        }
    }

    Ok(CodexSubagentSettings {
        model,
        reasoning_effort,
    })
}

fn read_persisted_settings() -> Result<Option<CodexSubagentSettings>, AppError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(None);
    }

    let settings: CodexSubagentSettings = read_json_file(&path)?;
    normalize_settings(settings.model, settings.reasoning_effort).map(Some)
}

fn read_live_defaults(config_text: &str) -> CodexSubagentSettings {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return CodexSubagentSettings::default();
    };

    let Some(agents) = doc.get("agents").and_then(|item| item.as_table_like()) else {
        return CodexSubagentSettings::default();
    };

    let model = agents
        .get("default_subagent_model")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let reasoning_effort = agents
        .get("default_subagent_reasoning_effort")
        .and_then(|item| item.as_str())
        .map(str::to_string);

    normalize_settings(model, reasoning_effort).unwrap_or_default()
}

fn available_models(config_text: &str) -> Vec<String> {
    let mut models = BTreeSet::new();

    if let Ok(doc) = config_text.parse::<DocumentMut>() {
        if let Some(model) = doc.get("model").and_then(|item| item.as_str()) {
            let model = model.trim();
            if !model.is_empty() {
                models.insert(model.to_string());
            }
        }
    }

    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    if let Ok(catalog_text) = fs::read_to_string(catalog_path) {
        if let Ok(catalog) = serde_json::from_str::<Value>(&catalog_text) {
            if let Some(entries) = catalog.get("models").and_then(Value::as_array) {
                for model in entries.iter().filter_map(|entry| {
                    entry
                        .get("slug")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                }) {
                    models.insert(model.to_string());
                }
            }
        }
    }

    models.into_iter().collect()
}

pub fn get_settings_view() -> Result<CodexSubagentSettingsView, AppError> {
    let config_text = crate::codex_config::read_codex_config_text()?;
    let settings = read_persisted_settings()?.unwrap_or_else(|| read_live_defaults(&config_text));

    Ok(CodexSubagentSettingsView {
        model: settings.model,
        reasoning_effort: settings.reasoning_effort,
        available_models: available_models(&config_text),
        config_path: crate::codex_config::get_codex_config_path()
            .to_string_lossy()
            .to_string(),
    })
}

pub fn apply_settings_to_config_text(
    config_text: &str,
    settings: &CodexSubagentSettings,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Message(format!("Invalid Codex config.toml: {error}")))?;

    let has_settings = settings.model.is_some() || settings.reasoning_effort.is_some();
    if !has_settings {
        if let Some(agents) = doc.get_mut("agents") {
            let table = agents.as_table_like_mut().ok_or_else(|| {
                AppError::Message("Codex config [agents] must be a TOML table".to_string())
            })?;
            table.remove("default_subagent_model");
            table.remove("default_subagent_reasoning_effort");
        }
        return Ok(doc.to_string());
    }

    if doc.get("agents").is_none() {
        doc["agents"] = toml_edit::table();
    }

    let agents = doc
        .get_mut("agents")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::Message("Codex config [agents] must be a TOML table".to_string())
        })?;

    match settings.model.as_deref() {
        Some(model) => {
            agents.insert("default_subagent_model", toml_edit::value(model));
        }
        None => {
            agents.remove("default_subagent_model");
        }
    }
    match settings.reasoning_effort.as_deref() {
        Some(effort) => {
            agents.insert(
                "default_subagent_reasoning_effort",
                toml_edit::value(effort),
            );
        }
        None => {
            agents.remove("default_subagent_reasoning_effort");
        }
    }

    Ok(doc.to_string())
}

pub fn apply_persisted_settings_to_config_text(config_text: &str) -> Result<String, AppError> {
    let Some(settings) = read_persisted_settings()? else {
        return Ok(config_text.to_string());
    };
    apply_settings_to_config_text(config_text, &settings)
}

pub fn save_settings(
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<CodexSubagentSettingsView, AppError> {
    let settings = normalize_settings(model, reasoning_effort)?;
    let sidecar_path = settings_path();
    let previous_sidecar = if sidecar_path.exists() {
        Some(fs::read(&sidecar_path).map_err(|error| AppError::io(&sidecar_path, error))?)
    } else {
        None
    };

    write_json_file(&sidecar_path, &settings)?;

    let update_result = (|| {
        let config_text = crate::codex_config::read_codex_config_text()?;
        let updated = apply_settings_to_config_text(&config_text, &settings)?;
        crate::codex_config::write_codex_live_config_atomic(Some(&updated))
    })();

    if let Err(error) = update_result {
        let rollback_result = match previous_sidecar {
            Some(contents) => atomic_write(&sidecar_path, &contents),
            None => delete_file(&sidecar_path),
        };

        if let Err(rollback_error) = rollback_result {
            return Err(AppError::Config(format!(
                "Failed to update Codex subagent settings: {error}; failed to restore the previous settings sidecar: {rollback_error}"
            )));
        }

        return Err(error);
    }

    get_settings_view()
}

#[cfg(test)]
mod tests {
    use super::{
        apply_settings_to_config_text, normalize_settings, read_persisted_settings, save_settings,
        settings_path, CodexSubagentSettings,
    };
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn writes_subagent_defaults_without_touching_other_agents_fields() {
        let input = "[agents]\nmax_threads = 4\n";
        let settings = CodexSubagentSettings {
            model: Some("gpt-5.6-sol".to_string()),
            reasoning_effort: Some("ultra".to_string()),
        };

        let output = apply_settings_to_config_text(input, &settings).unwrap();

        assert!(output.contains("max_threads = 4"));
        assert!(output.contains("default_subagent_model = \"gpt-5.6-sol\""));
        assert!(output.contains("default_subagent_reasoning_effort = \"ultra\""));
    }

    #[test]
    fn removes_only_subagent_defaults_when_cleared() {
        let input = "[agents]\nmax_threads = 4\ndefault_subagent_model = \"gpt-5.6-sol\"\ndefault_subagent_reasoning_effort = \"high\"\n";
        let settings = CodexSubagentSettings::default();

        let output = apply_settings_to_config_text(input, &settings).unwrap();

        assert!(output.contains("max_threads = 4"));
        assert!(!output.contains("default_subagent_model"));
        assert!(!output.contains("default_subagent_reasoning_effort"));
    }

    #[test]
    fn preserves_reasoning_effort_when_model_is_unset() {
        let input = "model = \"gpt-5.6\"\n";
        let settings = normalize_settings(None, Some("high".to_string())).unwrap();

        let output = apply_settings_to_config_text(input, &settings).unwrap();

        assert!(output.contains("default_subagent_reasoning_effort = \"high\""));
        assert!(!output.contains("default_subagent_model"));
    }

    #[test]
    fn creates_agents_table_when_config_is_empty() {
        let settings = CodexSubagentSettings {
            model: Some("gpt-5.6-terra".to_string()),
            reasoning_effort: None,
        };

        let output = apply_settings_to_config_text("", &settings).unwrap();

        assert!(output.contains("[agents]"));
        assert!(output.contains("default_subagent_model = \"gpt-5.6-terra\""));
    }

    #[test]
    #[serial]
    fn save_settings_restores_previous_sidecar_when_config_update_fails() {
        let _home = TempHome::new();
        let previous = CodexSubagentSettings {
            model: Some("gpt-5.6-sol".to_string()),
            reasoning_effort: Some("high".to_string()),
        };
        crate::config::write_json_file(&settings_path(), &previous)
            .expect("seed existing settings sidecar");
        crate::config::write_text_file(
            &crate::codex_config::get_codex_config_path(),
            "[agents\ndefault_subagent_model = \"broken\"\n",
        )
        .expect("seed malformed config");

        let error = save_settings(Some("gpt-5.6-terra".to_string()), Some("ultra".to_string()))
            .expect_err("malformed config should reject the update");

        assert!(error.to_string().contains("TOML"));
        assert_eq!(read_persisted_settings().unwrap(), Some(previous));
    }
}
