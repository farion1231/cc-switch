//! OMO / OMO Slim config management for OpenCode.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::write_json_file;
use crate::error::AppError;
use crate::opencode_config::get_opencode_dir;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmoLocalFileData {
    pub agents: Option<Value>,
    pub categories: Option<Value>,
    pub other_fields: Option<Value>,
    pub file_path: String,
    pub last_modified: Option<String>,
}

type OmoProfileData = (Option<Value>, Option<Value>, Option<Value>);

pub struct OmoVariant {
    pub filename: &'static str,
    pub category: &'static str,
    pub provider_prefix: &'static str,
    pub plugin_name: &'static str,
    pub plugin_prefix: &'static str,
    pub has_categories: bool,
    pub label: &'static str,
    pub import_label: &'static str,
}

pub const STANDARD: OmoVariant = OmoVariant {
    filename: "oh-my-opencode.jsonc",
    category: "omo",
    provider_prefix: "omo-",
    plugin_name: "oh-my-opencode@latest",
    plugin_prefix: "oh-my-opencode",
    has_categories: true,
    label: "OMO",
    import_label: "Imported",
};

pub const SLIM: OmoVariant = OmoVariant {
    filename: "oh-my-opencode-slim.jsonc",
    category: "omo-slim",
    provider_prefix: "omo-slim-",
    plugin_name: "oh-my-opencode-slim@latest",
    plugin_prefix: "oh-my-opencode-slim",
    has_categories: false,
    label: "OMO Slim",
    import_label: "Imported Slim",
};

pub struct OmoService;

impl OmoService {
    fn config_path(variant: &OmoVariant) -> PathBuf {
        get_opencode_dir().join(variant.filename)
    }

    fn resolve_local_config_path(variant: &OmoVariant) -> Result<PathBuf, AppError> {
        let config_path = Self::config_path(variant);
        if config_path.exists() {
            return Ok(config_path);
        }

        let json_path = config_path.with_extension("json");
        if json_path.exists() {
            return Ok(json_path);
        }

        Err(AppError::OmoConfigNotFound)
    }

    fn read_jsonc_object(path: &Path) -> Result<Map<String, Value>, AppError> {
        let content = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let cleaned = Self::strip_jsonc_comments(&content);
        let parsed: Value = serde_json::from_str(&cleaned)
            .map_err(|e| AppError::Config(format!("Failed to parse oh-my-opencode config: {e}")))?;
        parsed
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::Config("Expected JSON object".to_string()))
    }

    fn extract_other_fields_with_keys(
        obj: &Map<String, Value>,
        known: &[&str],
    ) -> Map<String, Value> {
        let mut other = Map::new();
        for (key, value) in obj {
            if !known.contains(&key.as_str()) {
                other.insert(key.clone(), value.clone());
            }
        }
        other
    }

    fn insert_opt_value(result: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
        if let Some(value) = value {
            result.insert(key.to_string(), value.clone());
        }
    }

    fn insert_object_entries(result: &mut Map<String, Value>, value: Option<&Value>) {
        if let Some(Value::Object(map)) = value {
            for (key, value) in map {
                result.insert(key.clone(), value.clone());
            }
        }
    }

    pub fn delete_config_file(variant: &OmoVariant) -> Result<(), AppError> {
        let config_path = Self::config_path(variant);
        if config_path.exists() {
            std::fs::remove_file(&config_path).map_err(|e| AppError::io(&config_path, e))?;
        }
        crate::opencode_config::remove_plugin_by_prefix(variant.plugin_prefix)?;
        Ok(())
    }

    pub fn write_config_to_file(state: &AppState, variant: &OmoVariant) -> Result<(), AppError> {
        let current_omo = state
            .db
            .get_current_omo_provider("opencode", variant.category)?;
        let profile_data = current_omo.as_ref().map(|provider| {
            let agents = provider.settings_config.get("agents").cloned();
            let categories = if variant.has_categories {
                provider.settings_config.get("categories").cloned()
            } else {
                None
            };
            let other_fields = provider.settings_config.get("otherFields").cloned();
            (agents, categories, other_fields)
        });

        let merged = Self::build_config(variant, profile_data.as_ref());
        let config_path = Self::config_path(variant);
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        write_json_file(&config_path, &merged)?;
        crate::opencode_config::add_plugin(variant.plugin_name)?;
        Ok(())
    }

    fn build_config(variant: &OmoVariant, profile_data: Option<&OmoProfileData>) -> Value {
        let mut result = Map::new();
        if let Some((agents, categories, other_fields)) = profile_data {
            Self::insert_object_entries(&mut result, other_fields.as_ref());
            Self::insert_opt_value(&mut result, "agents", agents);
            if variant.has_categories {
                Self::insert_opt_value(&mut result, "categories", categories);
            }
        }
        Value::Object(result)
    }

    pub fn import_from_local(
        state: &AppState,
        variant: &OmoVariant,
    ) -> Result<crate::provider::Provider, AppError> {
        let actual_path = Self::resolve_local_config_path(variant)?;
        let obj = Self::read_jsonc_object(&actual_path)?;

        let mut settings = Map::new();
        if let Some(agents) = obj.get("agents") {
            settings.insert("agents".to_string(), agents.clone());
        }
        if variant.has_categories {
            if let Some(categories) = obj.get("categories") {
                settings.insert("categories".to_string(), categories.clone());
            }
        }

        let other = Self::extract_other_fields_with_keys(&obj, &["agents", "categories"]);
        if !other.is_empty() {
            settings.insert("otherFields".to_string(), Value::Object(other));
        }

        let provider_id = format!("{}{}", variant.provider_prefix, uuid::Uuid::new_v4());
        let name = format!(
            "{} {}",
            variant.import_label,
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
        let settings_config =
            serde_json::to_value(&settings).unwrap_or_else(|_| serde_json::json!({}));

        let provider = crate::provider::Provider {
            id: provider_id,
            name,
            settings_config,
            website_url: None,
            category: Some(variant.category.to_string()),
            created_at: Some(chrono::Utc::now().timestamp_millis()),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        state.db.save_provider("opencode", &provider)?;
        state
            .db
            .set_omo_provider_current("opencode", &provider.id, variant.category)?;
        Self::write_config_to_file(state, variant)?;
        Ok(provider)
    }

    pub fn read_local_file(variant: &OmoVariant) -> Result<OmoLocalFileData, AppError> {
        let actual_path = Self::resolve_local_config_path(variant)?;
        let metadata = std::fs::metadata(&actual_path).ok();
        let last_modified = metadata
            .and_then(|meta| meta.modified().ok())
            .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339());

        let obj = Self::read_jsonc_object(&actual_path)?;
        Ok(Self::build_local_file_data(
            variant,
            &obj,
            actual_path.to_string_lossy().to_string(),
            last_modified,
        ))
    }

    fn build_local_file_data(
        variant: &OmoVariant,
        obj: &Map<String, Value>,
        file_path: String,
        last_modified: Option<String>,
    ) -> OmoLocalFileData {
        let agents = obj.get("agents").cloned();
        let categories = if variant.has_categories {
            obj.get("categories").cloned()
        } else {
            None
        };

        let other = Self::extract_other_fields_with_keys(obj, &["agents", "categories"]);
        let other_fields = if other.is_empty() {
            None
        } else {
            Some(Value::Object(other))
        };

        OmoLocalFileData {
            agents,
            categories,
            other_fields,
            file_path,
            last_modified,
        }
    }

    fn strip_jsonc_comments(content: &str) -> String {
        let mut result = String::with_capacity(content.len());
        let mut chars = content.chars().peekable();
        let mut in_string = false;
        let mut escaped = false;

        while let Some(&ch) = chars.peek() {
            if in_string {
                result.push(ch);
                chars.next();
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            if ch == '"' {
                in_string = true;
                result.push(ch);
                chars.next();
                continue;
            }

            if ch == '/' {
                chars.next();
                match chars.peek() {
                    Some('/') => {
                        chars.next();
                        while let Some(&next) = chars.peek() {
                            if next == '\n' {
                                break;
                            }
                            chars.next();
                        }
                    }
                    Some('*') => {
                        chars.next();
                        while let Some(next) = chars.next() {
                            if next == '*' && chars.peek() == Some(&'/') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => {
                        result.push('/');
                    }
                }
                continue;
            }

            result.push(ch);
            chars.next();
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    use crate::database::Database;

    #[test]
    fn strip_jsonc_comments_keeps_string_content() {
        let input = r#"{
  // comment
  "name": "hello // world"
}"#;
        let output = OmoService::strip_jsonc_comments(input);
        assert!(output.contains("\"hello // world\""));
        assert!(!output.contains("// comment"));
    }

    #[test]
    fn strip_jsonc_comments_removes_block_comments() {
        let input = r#"{
  /* multi
     line */
  "name": "value"
}"#;
        let output = OmoService::strip_jsonc_comments(input);
        assert!(output.contains("\"name\": \"value\""));
        assert!(!output.contains("multi"));
    }

    #[test]
    #[serial]
    fn write_standard_omo_file_from_current_provider() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        let state = AppState::new(Database::memory()?);

        let provider = crate::provider::Provider::with_id(
            "omo-demo".into(),
            "OMO Demo".into(),
            serde_json::json!({
                "agents": { "demo": { "prompt": "hi" } },
                "categories": ["tools"],
                "otherFields": { "theme": "default" }
            }),
            None,
        );
        let mut provider = provider;
        provider.category = Some("omo".into());
        state.db.save_provider("opencode", &provider)?;
        state
            .db
            .set_omo_provider_current("opencode", "omo-demo", "omo")?;

        OmoService::write_config_to_file(&state, &STANDARD)?;

        let path = temp.path().join(".config/opencode/oh-my-opencode.jsonc");
        let written: Value = crate::config::read_json_file(&path)?;
        assert!(written.get("agents").is_some());
        assert!(written.get("categories").is_some());
        Ok(())
    }
}
