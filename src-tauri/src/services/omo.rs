use crate::config::write_json_file;
use crate::database::OmoGlobalConfig;
use crate::error::AppError;
use crate::opencode_config::get_opencode_dir;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmoLocalFileData {
    pub agents: Option<Value>,
    pub categories: Option<Value>,
    pub other_fields: Option<Value>,
    pub global: OmoGlobalConfig,
    pub file_path: String,
    pub last_modified: Option<String>,
}

type OmoProfileData = (Option<Value>, Option<Value>, Option<Value>, bool);

pub struct OmoService;

impl OmoService {
    fn config_path() -> PathBuf {
        get_opencode_dir().join("oh-my-opencode.jsonc")
    }

    fn resolve_local_config_path() -> Result<PathBuf, AppError> {
        let config_path = Self::config_path();
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

    fn extract_other_fields(obj: &Map<String, Value>) -> Map<String, Value> {
        const KNOWN_KEYS: [&str; 13] = [
            "$schema",
            "agents",
            "categories",
            "sisyphus_agent",
            "disabled_agents",
            "disabled_mcps",
            "disabled_hooks",
            "disabled_skills",
            "lsp",
            "experimental",
            "background_task",
            "browser_automation_engine",
            "claude_code",
        ];

        let mut other = Map::new();
        for (k, v) in obj {
            if !KNOWN_KEYS.contains(&k.as_str()) {
                other.insert(k.clone(), v.clone());
            }
        }
        other
    }

    fn extract_string_array(val: &Value) -> Vec<String> {
        val.as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn merge_global_from_obj(obj: &Map<String, Value>, global: &mut OmoGlobalConfig) {
        if let Some(v) = obj.get("$schema") {
            global.schema_url = v.as_str().map(|s| s.to_string());
        }
        for (key, target) in [
            ("disabled_agents", &mut global.disabled_agents),
            ("disabled_mcps", &mut global.disabled_mcps),
            ("disabled_hooks", &mut global.disabled_hooks),
            ("disabled_skills", &mut global.disabled_skills),
        ] {
            if let Some(v) = obj.get(key) {
                *target = Self::extract_string_array(v);
            }
        }
        for (key, target) in [
            ("sisyphus_agent", &mut global.sisyphus_agent),
            ("lsp", &mut global.lsp),
            ("experimental", &mut global.experimental),
            ("background_task", &mut global.background_task),
            (
                "browser_automation_engine",
                &mut global.browser_automation_engine,
            ),
            ("claude_code", &mut global.claude_code),
        ] {
            if let Some(v) = obj.get(key) {
                *target = Some(v.clone());
            }
        }
    }

    fn insert_opt_value(result: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
        if let Some(v) = value {
            result.insert(key.to_string(), v.clone());
        }
    }

    fn insert_string_array(result: &mut Map<String, Value>, key: &str, values: &[String]) {
        if !values.is_empty() {
            result.insert(
                key.to_string(),
                serde_json::to_value(values).unwrap_or(Value::Array(vec![])),
            );
        }
    }

    fn insert_object_entries(result: &mut Map<String, Value>, value: Option<&Value>) {
        if let Some(Value::Object(map)) = value {
            for (k, v) in map {
                result.insert(k.clone(), v.clone());
            }
        }
    }

    pub fn delete_config_file() -> Result<(), AppError> {
        let config_path = Self::config_path();
        if config_path.exists() {
            std::fs::remove_file(&config_path).map_err(|e| AppError::io(&config_path, e))?;
            log::info!("OMO config file deleted: {config_path:?}");
        }
        crate::opencode_config::remove_plugin_by_prefix("oh-my-opencode")?;
        Ok(())
    }

    pub fn write_config_to_file(state: &AppState) -> Result<(), AppError> {
        let global = state.db.get_omo_global_config()?;
        let current_omo = state.db.get_current_omo_provider("opencode")?;

        let profile_data = current_omo.as_ref().map(|p| {
            let agents = p.settings_config.get("agents").cloned();
            let categories = p.settings_config.get("categories").cloned();
            let other_fields = p.settings_config.get("otherFields").cloned();
            let use_common_config = p
                .settings_config
                .get("useCommonConfig")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            (agents, categories, other_fields, use_common_config)
        });

        let merged = Self::merge_config(&global, profile_data.as_ref());
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        write_json_file(&config_path, &merged)?;

        crate::opencode_config::add_plugin("oh-my-opencode@latest")?;

        log::info!("OMO config written to {config_path:?}");
        Ok(())
    }

    fn merge_config(global: &OmoGlobalConfig, profile_data: Option<&OmoProfileData>) -> Value {
        let mut result = Map::new();
        let use_common_config = profile_data.map(|(_, _, _, v)| *v).unwrap_or(true);

        if use_common_config {
            if let Some(url) = &global.schema_url {
                result.insert("$schema".to_string(), Value::String(url.clone()));
            }

            Self::insert_opt_value(&mut result, "sisyphus_agent", &global.sisyphus_agent);
            Self::insert_string_array(&mut result, "disabled_agents", &global.disabled_agents);
            Self::insert_string_array(&mut result, "disabled_mcps", &global.disabled_mcps);
            Self::insert_string_array(&mut result, "disabled_hooks", &global.disabled_hooks);
            Self::insert_string_array(&mut result, "disabled_skills", &global.disabled_skills);
            Self::insert_opt_value(&mut result, "lsp", &global.lsp);
            Self::insert_opt_value(&mut result, "experimental", &global.experimental);
            Self::insert_opt_value(&mut result, "background_task", &global.background_task);
            Self::insert_opt_value(
                &mut result,
                "browser_automation_engine",
                &global.browser_automation_engine,
            );
            Self::insert_opt_value(&mut result, "claude_code", &global.claude_code);

            Self::insert_object_entries(&mut result, global.other_fields.as_ref());
        }

        if let Some((agents, categories, other_fields, _)) = profile_data {
            Self::insert_opt_value(&mut result, "agents", agents);
            Self::insert_opt_value(&mut result, "categories", categories);
            Self::insert_object_entries(&mut result, other_fields.as_ref());
        }

        Value::Object(result)
    }

    pub fn import_from_local(state: &AppState) -> Result<crate::provider::Provider, AppError> {
        let actual_path = Self::resolve_local_config_path()?;
        Self::import_from_path(state, &actual_path)
    }

    fn import_from_path(
        state: &AppState,
        path: &std::path::Path,
    ) -> Result<crate::provider::Provider, AppError> {
        let obj = Self::read_jsonc_object(path)?;

        let mut settings = Map::new();
        if let Some(agents) = obj.get("agents") {
            settings.insert("agents".to_string(), agents.clone());
        }
        if let Some(categories) = obj.get("categories") {
            settings.insert("categories".to_string(), categories.clone());
        }
        settings.insert("useCommonConfig".to_string(), Value::Bool(true));

        let other = Self::extract_other_fields(&obj);
        if !other.is_empty() {
            settings.insert("otherFields".to_string(), Value::Object(other));
        }

        let mut global = state.db.get_omo_global_config()?;
        Self::merge_global_from_obj(&obj, &mut global);
        global.updated_at = chrono::Utc::now().to_rfc3339();
        state.db.save_omo_global_config(&global)?;

        let provider_id = format!("omo-{}", uuid::Uuid::new_v4());
        let name = format!("Imported {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
        let settings_config =
            serde_json::to_value(&settings).unwrap_or_else(|_| serde_json::json!({}));

        let provider = crate::provider::Provider {
            id: provider_id,
            name,
            settings_config,
            website_url: None,
            category: Some("omo".to_string()),
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
            .set_omo_provider_current("opencode", &provider.id)?;
        Self::write_config_to_file(state)?;
        Ok(provider)
    }

    pub fn read_local_file() -> Result<OmoLocalFileData, AppError> {
        let actual_path = Self::resolve_local_config_path()?;
        let metadata = std::fs::metadata(&actual_path).ok();
        let last_modified = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

        let obj = Self::read_jsonc_object(&actual_path)?;

        Ok(Self::build_local_file_data_from_obj(
            &obj,
            actual_path.to_string_lossy().to_string(),
            last_modified,
        ))
    }

    fn build_local_file_data_from_obj(
        obj: &Map<String, Value>,
        file_path: String,
        last_modified: Option<String>,
    ) -> OmoLocalFileData {
        let agents = obj.get("agents").cloned();
        let categories = obj.get("categories").cloned();

        let other = Self::extract_other_fields(obj);
        let other_fields = if other.is_empty() {
            None
        } else {
            Some(Value::Object(other))
        };

        let mut global = OmoGlobalConfig::default();
        Self::merge_global_from_obj(obj, &mut global);
        global.other_fields = other_fields.clone();

        OmoLocalFileData {
            agents,
            categories,
            other_fields,
            global,
            file_path,
            last_modified,
        }
    }

    fn strip_jsonc_comments(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        let mut in_string = false;
        let mut escape = false;

        while let Some(&c) = chars.peek() {
            if in_string {
                result.push(c);
                chars.next();
                if escape {
                    escape = false;
                } else if c == '\\' {
                    escape = true;
                } else if c == '"' {
                    in_string = false;
                }
            } else if c == '"' {
                in_string = true;
                result.push(c);
                chars.next();
            } else if c == '/' {
                chars.next();
                match chars.peek() {
                    Some('/') => {
                        chars.next();
                        while let Some(&nc) = chars.peek() {
                            if nc == '\n' {
                                break;
                            }
                            chars.next();
                        }
                    }
                    Some('*') => {
                        chars.next();
                        while let Some(nc) = chars.next() {
                            if nc == '*' {
                                if let Some(&'/') = chars.peek() {
                                    chars.next();
                                    break;
                                }
                            }
                        }
                    }
                    _ => {
                        result.push('/');
                    }
                }
            } else {
                result.push(c);
                chars.next();
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_jsonc_comments() {
        let input = r#"{
  // This is a comment
  "key": "value", // inline comment
  /* multi
     line */
  "key2": "val//ue"
}"#;
        let result = OmoService::strip_jsonc_comments(input);
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["key2"], "val//ue");
    }

    #[test]
    fn test_merge_config_empty() {
        let global = OmoGlobalConfig::default();
        let merged = OmoService::merge_config(&global, None);
        assert!(merged.is_object());
    }

    #[test]
    fn test_merge_config_with_profile() {
        let global = OmoGlobalConfig {
            schema_url: Some("https://example.com/schema.json".to_string()),
            disabled_agents: vec!["explore".to_string()],
            ..Default::default()
        };
        let agents = Some(serde_json::json!({
            "sisyphus": { "model": "claude-opus-4-5" }
        }));
        let categories = None;
        let other_fields = None;
        let profile_data = (agents, categories, other_fields, true);
        let merged = OmoService::merge_config(&global, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert_eq!(obj["$schema"], "https://example.com/schema.json");
        assert_eq!(obj["disabled_agents"], serde_json::json!(["explore"]));
        assert!(obj.contains_key("agents"));
        assert_eq!(obj["agents"]["sisyphus"]["model"], "claude-opus-4-5");
    }

    #[test]
    fn test_merge_config_without_common_config() {
        let global = OmoGlobalConfig {
            schema_url: Some("https://example.com/schema.json".to_string()),
            disabled_agents: vec!["explore".to_string()],
            ..Default::default()
        };
        let agents = Some(serde_json::json!({
            "sisyphus": { "model": "claude-opus-4-5" }
        }));
        let categories = None;
        let other_fields = None;
        let profile_data = (agents, categories, other_fields, false);
        let merged = OmoService::merge_config(&global, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert!(!obj.contains_key("$schema"));
        assert!(!obj.contains_key("disabled_agents"));
        assert!(obj.contains_key("agents"));
    }

    #[test]
    fn test_build_local_file_data_keeps_unknown_top_level_fields_in_global() {
        let obj = serde_json::json!({
            "$schema": "https://example.com/schema.json",
            "disabled_agents": ["oracle"],
            "agents": {
                "sisyphus": { "model": "claude-opus-4-6" }
            },
            "categories": {
                "code": { "model": "gpt-5.3" }
            },
            "custom_top_level": {
                "enabled": true
            }
        });
        let obj_map = obj.as_object().unwrap().clone();

        let data = OmoService::build_local_file_data_from_obj(
            &obj_map,
            "/tmp/oh-my-opencode.jsonc".to_string(),
            None,
        );

        assert_eq!(
            data.global.schema_url.as_deref(),
            Some("https://example.com/schema.json")
        );
        assert_eq!(data.global.disabled_agents, vec!["oracle".to_string()]);

        assert_eq!(
            data.other_fields,
            Some(serde_json::json!({
                "custom_top_level": { "enabled": true }
            }))
        );
        assert_eq!(data.global.other_fields, data.other_fields);
    }

    #[test]
    fn test_merge_config_ignores_non_object_other_fields() {
        let global = OmoGlobalConfig {
            other_fields: Some(serde_json::json!(["global_non_object"])),
            ..Default::default()
        };
        let agents = None;
        let categories = None;
        let other_fields = Some(serde_json::json!("profile_non_object"));
        let profile_data = (agents, categories, other_fields, true);

        let merged = OmoService::merge_config(&global, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert!(!obj.contains_key("0"));
        assert!(!obj.contains_key("global_non_object"));
        assert!(!obj.contains_key("profile_non_object"));
    }
}
