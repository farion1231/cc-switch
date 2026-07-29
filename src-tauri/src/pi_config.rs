//! Pi Agent live configuration helpers.
//!
//! Pi keeps model providers in `models.json` and the active provider/model in
//! `settings.json` under `~/.pi/agent` by default.

use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{get_home_dir, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;

pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_pi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_DIR") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    get_home_dir().join(".pi").join("agent")
}

pub fn get_pi_models_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

pub fn get_pi_settings_path() -> PathBuf {
    get_pi_dir().join("settings.json")
}

fn read_json_or_empty_object(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if content.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }

    serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
}

fn object_mut<'a>(
    value: &'a mut Value,
    path: &Path,
    description: &str,
) -> Result<&'a mut Map<String, Value>, AppError> {
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "{description} must be a JSON object: {}",
            path.display()
        ))
    })
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn model_ids_from_config(config: &Value) -> Vec<String> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => Some(id.as_str()),
                    Value::Object(obj) => obj.get("id").and_then(Value::as_str),
                    _ => None,
                })
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn normalized_models_from_config(config: &Value) -> Option<Vec<Value>> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => {
                        let id = id.trim();
                        (!id.is_empty()).then(|| Value::String(id.to_string()))
                    }
                    Value::Object(object) => {
                        let id = object.get("id").and_then(Value::as_str)?.trim();
                        if id.is_empty() {
                            return None;
                        }

                        let mut normalized = object.clone();
                        normalized.insert("id".to_string(), Value::String(id.to_string()));
                        Some(Value::Object(normalized))
                    }
                    _ => None,
                })
                .collect()
        })
}

fn normalize_provider_config(config: &Value) -> Result<Value, AppError> {
    let Some(source) = config.as_object() else {
        return Err(AppError::Config(
            "Pi provider settings must be a JSON object".to_string(),
        ));
    };

    let mut output = source.clone();

    if let Some(base_url) = first_string(source, &["baseUrl", "baseURL"]) {
        output.insert("baseUrl".to_string(), Value::String(base_url.to_string()));
        output.remove("baseURL");
    }

    if let Some(models) = normalized_models_from_config(config) {
        output.insert("models".to_string(), Value::Array(models));
    }

    // Pi stores the selected model in settings.json, not per provider.
    output.remove("defaultModel");

    Ok(Value::Object(output))
}

fn default_model_for_provider(config: &Value) -> Option<String> {
    config
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| model_ids_from_config(config).into_iter().next())
}

fn providers_mut<'a>(
    models_root: &'a mut Value,
    models_path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = object_mut(models_root, models_path, "Pi models.json root")?;
    let providers = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    object_mut(providers, models_path, "Pi models.json providers")
}

fn insert_provider(
    models_root: &mut Value,
    models_path: &Path,
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    let providers = providers_mut(models_root, models_path)?;
    if providers.contains_key(&provider.id) && !overwrite_existing {
        return Err(AppError::Config(format!(
            "Pi provider '{}' already exists in models.json and is not managed by CC Switch",
            provider.id
        )));
    }

    providers.insert(
        provider.id.clone(),
        normalize_provider_config(&provider.settings_config)?,
    );
    Ok(())
}

fn select_provider(settings_root: &mut Value, provider: &Provider) -> Result<(), AppError> {
    let settings_path = get_pi_settings_path();
    let settings = object_mut(settings_root, &settings_path, "Pi settings.json root")?;
    settings.insert(
        "defaultProvider".to_string(),
        Value::String(provider.id.clone()),
    );
    if let Some(default_model) = default_model_for_provider(&provider.settings_config) {
        settings.insert("defaultModel".to_string(), Value::String(default_model));
    } else {
        settings.remove("defaultModel");
    }
    Ok(())
}

/// Add or update a provider in Pi's additive registry without selecting it.
pub fn upsert_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    let models_path = get_pi_models_path();
    let mut models_root = read_json_or_empty_object(&models_path)?;
    insert_provider(&mut models_root, &models_path, provider, overwrite_existing)?;
    write_json_file(&models_path, &models_root)?;
    Ok(())
}

/// Add or update a provider and select it as Pi's default provider/model.
pub fn activate_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    let models_path = get_pi_models_path();
    let settings_path = get_pi_settings_path();
    let mut models_root = read_json_or_empty_object(&models_path)?;
    insert_provider(&mut models_root, &models_path, provider, overwrite_existing)?;
    let mut settings_root = read_json_or_empty_object(&settings_path)?;
    select_provider(&mut settings_root, provider)?;

    write_json_file(&models_path, &models_root)?;
    write_json_file(&settings_path, &settings_root)?;
    Ok(())
}

/// Compatibility wrapper retained for callers that mean "activate".
pub fn write_pi_live_provider(provider: &Provider) -> Result<(), AppError> {
    activate_pi_live_provider(provider, true)
}

/// Synchronize all CC Switch-managed providers and the selected default.
pub fn sync_pi_live_providers(
    providers: &[&Provider],
    active_provider: Option<&Provider>,
) -> Result<(), AppError> {
    let models_path = get_pi_models_path();
    let mut models_root = read_json_or_empty_object(&models_path)?;
    for provider in providers {
        insert_provider(&mut models_root, &models_path, provider, true)?;
    }
    write_json_file(&models_path, &models_root)?;

    if let Some(active_provider) = active_provider {
        let settings_path = get_pi_settings_path();
        let mut settings_root = read_json_or_empty_object(&settings_path)?;
        select_provider(&mut settings_root, active_provider)?;
        write_json_file(&settings_path, &settings_root)?;
    }
    Ok(())
}

pub fn pi_provider_exists(provider_id: &str) -> Result<bool, AppError> {
    let models_path = get_pi_models_path();
    let models_root = read_json_or_empty_object(&models_path)?;
    let root = models_root.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models.json root must be a JSON object: {}",
            models_path.display()
        ))
    })?;
    Ok(root
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.contains_key(provider_id)))
}

/// Remove a managed provider, but never leave Pi's default pointing at a
/// provider that no longer exists.
pub fn remove_pi_live_provider(provider_id: &str) -> Result<(), AppError> {
    let settings_path = get_pi_settings_path();
    let settings_root = read_json_or_empty_object(&settings_path)?;
    if settings_root.get("defaultProvider").and_then(Value::as_str) == Some(provider_id) {
        return Err(AppError::Config(format!(
            "Cannot remove active Pi provider '{provider_id}'. Switch first."
        )));
    }

    let models_path = get_pi_models_path();
    let mut models_root = read_json_or_empty_object(&models_path)?;
    providers_mut(&mut models_root, &models_path)?.remove(provider_id);
    write_json_file(&models_path, &models_root)
}

fn provider_models_for_form(provider_config: &Value) -> Value {
    let models = provider_config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => Some(json!({ "id": id })),
                    Value::Object(obj) => Some(Value::Object(obj.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Value::Array(models)
}

pub fn read_pi_live_settings() -> Result<Value, AppError> {
    let models_path = get_pi_models_path();
    let settings_path = get_pi_settings_path();
    let models_root = read_json_or_empty_object(&models_path)?;
    let settings_root = read_json_or_empty_object(&settings_path)?;

    let default_provider = settings_root
        .get("defaultProvider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if default_provider.is_empty() {
        return Err(AppError::Config(
            "Pi settings.json does not define defaultProvider".to_string(),
        ));
    }

    let provider_config = models_root
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(default_provider))
        .ok_or_else(|| {
            AppError::Config(format!(
                "Pi models.json does not define provider '{default_provider}'"
            ))
        })?;

    let mut form_config = provider_config.clone();
    if let Some(obj) = form_config.as_object_mut() {
        if let Some(base_url) = obj.get("baseURL").cloned() {
            obj.insert("baseUrl".to_string(), base_url);
        }
        obj.insert(
            "models".to_string(),
            provider_models_for_form(provider_config),
        );
    }

    Ok(json!({
        "defaultProvider": default_provider,
        "defaultModel": settings_root.get("defaultModel").cloned().unwrap_or(Value::Null),
        "providerConfig": form_config
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_provider_config_writes_pi_base_url_key() {
        let config = json!({
            "baseURL": "https://api.example.com/v1",
            "apiKey": "sk-test",
            "api": "openai-completions",
            "models": []
        });

        let normalized = normalize_provider_config(&config).unwrap();

        assert_eq!(
            normalized.get("baseUrl"),
            Some(&json!("https://api.example.com/v1"))
        );
        assert!(
            normalized.get("baseURL").is_none(),
            "Pi models.json must use baseUrl, not baseURL"
        );
    }
}
