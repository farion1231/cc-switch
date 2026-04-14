//! Live configuration operations
//!
//! Handles reading and writing live configuration files for Claude, Codex, and Gemini.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, TableLike};

use crate::app_config::AppType;
use crate::codex_config::get_codex_auth_path;
use crate::config::{
    delete_file, get_claude_settings_path, get_claude_settings_paths, read_json_file,
    write_json_file,
};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

use super::gemini_auth::{detect_gemini_auth_type, GeminiAuthType};
use super::normalize_claude_models_in_value;

pub(crate) fn sanitize_claude_settings_for_live(settings: &Value) -> Value {
    let mut v = settings.clone();
    if let Some(obj) = v.as_object_mut() {
        // Internal-only fields - never write to Claude Code settings.json
        obj.remove("api_format");
        obj.remove("apiFormat");
        obj.remove("openrouter_compat_mode");
        obj.remove("openrouterCompatMode");
    }
    v
}

pub(crate) fn provider_exists_in_live_config(
    app_type: &AppType,
    provider_id: &str,
) -> Result<bool, AppError> {
    match app_type {
        AppType::OpenCode => crate::opencode_config::get_providers()
            .map(|providers| providers.contains_key(provider_id)),
        AppType::OpenClaw => crate::openclaw_config::get_providers()
            .map(|providers| providers.contains_key(provider_id)),
        _ => Ok(false),
    }
}

fn for_each_claude_settings_path<F>(mut op: F) -> Result<(), AppError>
where
    F: FnMut(usize, &Path) -> Result<(), AppError>,
{
    let mut paths = get_claude_settings_paths();
    log::debug!("Claude settings paths: {:?}", paths);
    if paths.is_empty() {
        paths.push(get_claude_settings_path());
    }

    for (idx, path) in paths.iter().enumerate() {
        if let Err(err) = op(idx, path) {
            if idx == 0 {
                return Err(err);
            }
            log::warn!(
                "Claude multi-path write skipped for secondary path {}: {}",
                path.display(),
                err
            );
        }
    }
    Ok(())
}

fn for_each_codex_live_path<F>(mut op: F) -> Result<(), AppError>
where
    F: FnMut(usize, &Path, &Path) -> Result<(), AppError>,
{
    let auth_primary = get_codex_auth_path();
    let primary_dir = auth_primary
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::codex_config::get_codex_config_dir);

    let dirs = crate::utils::wsl::expand_wsl_dirs(&primary_dir, &[".codex"]);
    log::debug!("Codex live config dirs: {:?}", dirs);
    for (idx, dir) in dirs.iter().enumerate() {
        let auth_path = dir.join("auth.json");
        let config_path = dir.join("config.toml");
        if let Err(err) = op(idx, &auth_path, &config_path) {
            if idx == 0 {
                return Err(err);
            }
            log::warn!(
                "Codex multi-path write skipped for secondary path {}: {}",
                dir.display(),
                err
            );
        }
    }
    Ok(())
}

fn for_each_gemini_live_path<F>(mut op: F) -> Result<(), AppError>
where
    F: FnMut(usize, &Path, &Path) -> Result<(), AppError>,
{
    let env_primary = crate::gemini_config::get_gemini_env_path();
    let primary_dir = env_primary
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::gemini_config::get_gemini_dir);

    let dirs = crate::utils::wsl::expand_wsl_dirs(&primary_dir, &[".gemini"]);

    for (idx, dir) in dirs.iter().enumerate() {
        let env_path = dir.join(".env");
        let settings_path = dir.join("settings.json");
        if let Err(err) = op(idx, &env_path, &settings_path) {
            if idx == 0 {
                return Err(err);
            }
            log::warn!(
                "Gemini multi-path write skipped for secondary path {}: {}",
                dir.display(),
                err
            );
        }
    }
    Ok(())
}

fn for_each_opencode_config_path<F>(mut op: F) -> Result<(), AppError>
where
    F: FnMut(usize, &Path) -> Result<(), AppError>,
{
    let primary_path = crate::opencode_config::get_opencode_config_path();
    let primary_dir = primary_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::opencode_config::get_opencode_dir);

    let dirs = crate::utils::wsl::expand_wsl_dirs(&primary_dir, &[".config", "opencode"]);

    for (idx, dir) in dirs.iter().enumerate() {
        let path = dir.join("opencode.json");
        if let Err(err) = op(idx, &path) {
            if idx == 0 {
                return Err(err);
            }
            log::warn!(
                "OpenCode multi-path write skipped for secondary path {}: {}",
                path.display(),
                err
            );
        }
    }
    Ok(())
}

fn for_each_openclaw_config_path<F>(mut op: F) -> Result<(), AppError>
where
    F: FnMut(usize, &Path) -> Result<(), AppError>,
{
    let primary_path = crate::openclaw_config::get_openclaw_config_path();
    let primary_dir = primary_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::openclaw_config::get_openclaw_dir);

    let dirs = crate::utils::wsl::expand_wsl_dirs(&primary_dir, &[".openclaw"]);

    for (idx, dir) in dirs.iter().enumerate() {
        let path = dir.join("openclaw.json");
        if let Err(err) = op(idx, &path) {
            if idx == 0 {
                return Err(err);
            }
            log::warn!(
                "OpenClaw multi-path write skipped for secondary path {}: {}",
                path.display(),
                err
            );
        }
    }
    Ok(())
}

fn write_gemini_env_at(env_path: &Path, env_map: &HashMap<String, String>) -> Result<(), AppError> {
    if let Some(parent) = env_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let content = crate::gemini_config::serialize_env_file(env_map);
    crate::config::write_text_file(env_path, &content)
}

fn set_gemini_selected_type_at(
    settings_path: &Path,
    auth_type: &GeminiAuthType,
) -> Result<(), AppError> {
    let selected_type = match auth_type {
        GeminiAuthType::GoogleOfficial => "oauth-personal",
        GeminiAuthType::Packycode | GeminiAuthType::Generic => "gemini-api-key",
    };

    let mut settings_content = if settings_path.exists() {
        read_json_file::<Value>(settings_path).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if let Some(obj) = settings_content.as_object_mut() {
        let security = obj
            .entry("security")
            .or_insert_with(|| serde_json::json!({}));

        if let Some(security_obj) = security.as_object_mut() {
            let auth = security_obj
                .entry("auth")
                .or_insert_with(|| serde_json::json!({}));

            if let Some(auth_obj) = auth.as_object_mut() {
                auth_obj.insert(
                    "selectedType".to_string(),
                    Value::String(selected_type.to_string()),
                );
            }
        }
    }

    write_json_file(settings_path, &settings_content)
}

fn upsert_opencode_provider_at(
    path: &Path,
    provider_id: &str,
    config: Value,
) -> Result<(), AppError> {
    let mut full_config = if path.exists() {
        read_json_file::<Value>(path)
            .unwrap_or_else(|_| json!({ "$schema": "https://opencode.ai/config.json" }))
    } else {
        json!({ "$schema": "https://opencode.ai/config.json" })
    };

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(provider_id.to_string(), config);
    }

    write_json_file(path, &full_config)
}

fn remove_opencode_provider_at(path: &Path, provider_id: &str) -> Result<(), AppError> {
    let mut config = if path.exists() {
        read_json_file::<Value>(path)
            .unwrap_or_else(|_| json!({ "$schema": "https://opencode.ai/config.json" }))
    } else {
        return Ok(());
    };

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(provider_id);
    }

    write_json_file(path, &config)
}

fn upsert_openclaw_provider_at(
    path: &Path,
    provider_id: &str,
    provider_config: Value,
) -> Result<(), AppError> {
    let mut full_config = if path.exists() {
        read_json_file::<Value>(path).unwrap_or_else(|_| {
            json!({
                "models": {
                    "mode": "merge",
                    "providers": {}
                }
            })
        })
    } else {
        json!({
            "models": {
                "mode": "merge",
                "providers": {}
            }
        })
    };

    if !full_config
        .get("models")
        .is_some_and(|value| value.is_object())
    {
        full_config["models"] = json!({
            "mode": "merge",
            "providers": {}
        });
    }

    if !full_config["models"]
        .get("providers")
        .is_some_and(|value| value.is_object())
    {
        full_config["models"]["providers"] = json!({});
    }

    if let Some(providers) = full_config["models"]
        .get_mut("providers")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(provider_id.to_string(), provider_config);
    }

    write_json_file(path, &full_config)
}

fn remove_openclaw_provider_at(path: &Path, provider_id: &str) -> Result<(), AppError> {
    let mut config = if path.exists() {
        read_json_file::<Value>(path).unwrap_or_else(|_| {
            json!({
                "models": {
                    "mode": "merge",
                    "providers": {}
                }
            })
        })
    } else {
        return Ok(());
    };

    if let Some(providers) = config
        .get_mut("models")
        .and_then(|m| m.get_mut("providers"))
        .and_then(|v| v.as_object_mut())
    {
        providers.remove(provider_id);
    }

    write_json_file(path, &config)
}

fn json_is_subset(target: &Value, source: &Value) -> bool {
    match source {
        Value::Object(source_map) => {
            let Some(target_map) = target.as_object() else {
                return false;
            };
            source_map.iter().all(|(key, source_value)| {
                target_map
                    .get(key)
                    .is_some_and(|target_value| json_is_subset(target_value, source_value))
            })
        }
        Value::Array(source_arr) => {
            let Some(target_arr) = target.as_array() else {
                return false;
            };
            json_array_contains_subset(target_arr, source_arr)
        }
        _ => target == source,
    }
}

fn json_array_contains_subset(target_arr: &[Value], source_arr: &[Value]) -> bool {
    let mut matched = vec![false; target_arr.len()];

    source_arr.iter().all(|source_item| {
        if let Some((index, _)) = target_arr.iter().enumerate().find(|(index, target_item)| {
            !matched[*index] && json_is_subset(target_item, source_item)
        }) {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn json_remove_array_items(target_arr: &mut Vec<Value>, source_arr: &[Value]) {
    for source_item in source_arr {
        if let Some(index) = target_arr
            .iter()
            .position(|target_item| json_is_subset(target_item, source_item))
        {
            target_arr.remove(index);
        }
    }
}

fn json_deep_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(target_value) => json_deep_merge(target_value, source_value),
                    None => {
                        target_map.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        (target_value, source_value) => {
            *target_value = source_value.clone();
        }
    }
}

fn json_deep_remove(target: &mut Value, source: &Value) {
    let (Some(target_map), Some(source_map)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };

    for (key, source_value) in source_map {
        let mut remove_key = false;

        if let Some(target_value) = target_map.get_mut(key) {
            if source_value.is_object() && target_value.is_object() {
                json_deep_remove(target_value, source_value);
                remove_key = target_value.as_object().is_some_and(|obj| obj.is_empty());
            } else if let (Some(target_arr), Some(source_arr)) =
                (target_value.as_array_mut(), source_value.as_array())
            {
                json_remove_array_items(target_arr, source_arr);
                remove_key = target_arr.is_empty();
            } else if json_is_subset(target_value, source_value) {
                remove_key = true;
            }
        }

        if remove_key {
            target_map.remove(key);
        }
    }
}

fn toml_value_is_subset(target: &toml_edit::Value, source: &toml_edit::Value) -> bool {
    match (target, source) {
        (toml_edit::Value::String(target), toml_edit::Value::String(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Integer(target), toml_edit::Value::Integer(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Float(target), toml_edit::Value::Float(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Boolean(target), toml_edit::Value::Boolean(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Datetime(target), toml_edit::Value::Datetime(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Array(target), toml_edit::Value::Array(source)) => {
            toml_array_contains_subset(target, source)
        }
        (toml_edit::Value::InlineTable(target), toml_edit::Value::InlineTable(source)) => {
            source.iter().all(|(key, source_item)| {
                target
                    .get(key)
                    .is_some_and(|target_item| toml_value_is_subset(target_item, source_item))
            })
        }
        _ => false,
    }
}

fn toml_array_contains_subset(target: &toml_edit::Array, source: &toml_edit::Array) -> bool {
    let mut matched = vec![false; target.len()];
    let target_items: Vec<&toml_edit::Value> = target.iter().collect();

    source.iter().all(|source_item| {
        if let Some((index, _)) = target_items
            .iter()
            .enumerate()
            .find(|(index, target_item)| {
                !matched[*index] && toml_value_is_subset(target_item, source_item)
            })
        {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn toml_remove_array_items(target: &mut toml_edit::Array, source: &toml_edit::Array) {
    for source_item in source.iter() {
        let index = {
            let target_items: Vec<&toml_edit::Value> = target.iter().collect();
            target_items
                .iter()
                .enumerate()
                .find(|(_, target_item)| toml_value_is_subset(target_item, source_item))
                .map(|(index, _)| index)
        };

        if let Some(index) = index {
            target.remove(index);
        }
    }
}

fn toml_item_is_subset(target: &Item, source: &Item) -> bool {
    if let Some(source_table) = source.as_table_like() {
        let Some(target_table) = target.as_table_like() else {
            return false;
        };
        return source_table.iter().all(|(key, source_item)| {
            target_table
                .get(key)
                .is_some_and(|target_item| toml_item_is_subset(target_item, source_item))
        });
    }

    match (target.as_value(), source.as_value()) {
        (Some(target_value), Some(source_value)) => {
            toml_value_is_subset(target_value, source_value)
        }
        _ => false,
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_toml_table_like(target_table, source_table);
            return;
        }
    }

    *target = source.clone();
}

fn merge_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn remove_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            remove_toml_table_like(target_table, source_table);
            if target_table.is_empty() {
                *target = Item::None;
            }
            return;
        }
    }

    if let Some(source_value) = source.as_value() {
        let mut remove_item = false;

        if let Some(target_value) = target.as_value_mut() {
            match (target_value, source_value) {
                (toml_edit::Value::Array(target_arr), toml_edit::Value::Array(source_arr)) => {
                    toml_remove_array_items(target_arr, source_arr);
                    remove_item = target_arr.is_empty();
                }
                (target_value, source_value)
                    if toml_value_is_subset(target_value, source_value) =>
                {
                    remove_item = true;
                }
                _ => {}
            }
        }

        if remove_item {
            *target = Item::None;
        }
    }
}

fn remove_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_string()).collect();

    for key in keys {
        let mut remove_key = false;
        if let (Some(target_item), Some(source_item)) = (target.get_mut(&key), source.get(&key)) {
            remove_toml_item(target_item, source_item);
            remove_key = target_item.is_none()
                || target_item
                    .as_table_like()
                    .is_some_and(|table_like| table_like.is_empty());
        }

        if remove_key {
            target.remove(&key);
        }
    }
}

fn settings_contain_common_config(app_type: &AppType, settings: &Value, snippet: &str) -> bool {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return false;
    }

    match app_type {
        AppType::Claude => match serde_json::from_str::<Value>(trimmed) {
            Ok(source) if source.is_object() => json_is_subset(settings, &source),
            _ => false,
        },
        AppType::Codex => {
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            if config_toml.trim().is_empty() {
                return false;
            }

            let target_doc = match config_toml.parse::<DocumentMut>() {
                Ok(doc) => doc,
                Err(_) => return false,
            };
            let source_doc = match trimmed.parse::<DocumentMut>() {
                Ok(doc) => doc,
                Err(_) => return false,
            };

            toml_item_is_subset(target_doc.as_item(), source_doc.as_item())
        }
        AppType::Gemini => match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(source_map)) => {
                let Some(target_map) = settings.get("env").and_then(Value::as_object) else {
                    return false;
                };
                source_map.iter().all(|(key, source_value)| {
                    target_map
                        .get(key)
                        .is_some_and(|target_value| json_is_subset(target_value, source_value))
                })
            }
            _ => false,
        },
        AppType::OpenCode | AppType::OpenClaw => false,
    }
}

pub(crate) fn provider_uses_common_config(
    app_type: &AppType,
    provider: &Provider,
    snippet: Option<&str>,
) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
    {
        Some(explicit) => explicit && snippet.is_some_and(|value| !value.trim().is_empty()),
        None => snippet.is_some_and(|value| {
            settings_contain_common_config(app_type, &provider.settings_config, value)
        }),
    }
}

pub(crate) fn remove_common_config_from_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match app_type {
        AppType::Claude => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Claude common config: {e}")))?;
            let mut result = settings.clone();
            json_deep_remove(&mut result, &source);
            Ok(result)
        }
        AppType::Codex => {
            let mut result = settings.clone();
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            let mut target_doc = if config_toml.trim().is_empty() {
                DocumentMut::new()
            } else {
                config_toml.parse::<DocumentMut>().map_err(|e| {
                    AppError::Message(format!(
                        "Invalid Codex config.toml while removing common config: {e}"
                    ))
                })?
            };
            let source_doc = trimmed.parse::<DocumentMut>().map_err(|e| {
                AppError::Message(format!("Invalid Codex common config snippet: {e}"))
            })?;

            remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            if let Some(obj) = result.as_object_mut() {
                obj.insert("config".to_string(), Value::String(target_doc.to_string()));
            }
            Ok(result)
        }
        AppType::Gemini => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
            let mut result = settings.clone();
            if let Some(env) = result.get_mut("env") {
                json_deep_remove(env, &source);
            }
            Ok(result)
        }
        AppType::OpenCode | AppType::OpenClaw => Ok(settings.clone()),
    }
}

fn apply_common_config_to_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match app_type {
        AppType::Claude => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Claude common config: {e}")))?;
            let mut result = settings.clone();
            json_deep_merge(&mut result, &source);
            Ok(result)
        }
        AppType::Codex => {
            let mut result = settings.clone();
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            let mut target_doc = if config_toml.trim().is_empty() {
                DocumentMut::new()
            } else {
                config_toml.parse::<DocumentMut>().map_err(|e| {
                    AppError::Message(format!(
                        "Invalid Codex config.toml while applying common config: {e}"
                    ))
                })?
            };
            let source_doc = trimmed.parse::<DocumentMut>().map_err(|e| {
                AppError::Message(format!("Invalid Codex common config snippet: {e}"))
            })?;

            merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            if let Some(obj) = result.as_object_mut() {
                obj.insert("config".to_string(), Value::String(target_doc.to_string()));
            }
            Ok(result)
        }
        AppType::Gemini => {
            let source = serde_json::from_str::<Value>(trimmed)
                .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
            let mut result = settings.clone();
            if let Some(env) = result.get_mut("env") {
                json_deep_merge(env, &source);
            } else if let Some(obj) = result.as_object_mut() {
                obj.insert("env".to_string(), source);
            }
            Ok(result)
        }
        AppType::OpenCode | AppType::OpenClaw => Ok(settings.clone()),
    }
}

pub(crate) fn build_effective_settings_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<Value, AppError> {
    let snippet = db.get_config_snippet(app_type.as_str())?;
    let mut effective_settings = provider.settings_config.clone();

    if provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        if let Some(snippet_text) = snippet.as_deref() {
            match apply_common_config_to_settings(app_type, &effective_settings, snippet_text) {
                Ok(settings) => effective_settings = settings,
                Err(err) => {
                    log::warn!(
                        "Failed to apply common config for {} provider '{}': {err}",
                        app_type.as_str(),
                        provider.id
                    );
                }
            }
        }
    }

    Ok(effective_settings)
}

pub(crate) fn write_live_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    let mut effective_provider = provider.clone();
    effective_provider.settings_config =
        build_effective_settings_with_common_config(db, app_type, provider)?;

    write_live_snapshot(app_type, &effective_provider)
}

pub(crate) fn strip_common_config_from_live_settings(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    let snippet = match db.get_config_snippet(app_type.as_str()) {
        Ok(snippet) => snippet,
        Err(err) => {
            log::warn!(
                "Failed to load common config for {} while backfilling '{}': {err}",
                app_type.as_str(),
                provider.id
            );
            return live_settings;
        }
    };

    if !provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        return live_settings;
    }

    let Some(snippet_text) = snippet.as_deref() else {
        return live_settings;
    };

    match remove_common_config_from_settings(app_type, &live_settings, snippet_text) {
        Ok(settings) => settings,
        Err(err) => {
            log::warn!(
                "Failed to strip common config for {} provider '{}': {err}",
                app_type.as_str(),
                provider.id
            );
            live_settings
        }
    }
}

pub(crate) fn normalize_provider_common_config_for_storage(
    db: &Database,
    app_type: &AppType,
    provider: &mut Provider,
) -> Result<(), AppError> {
    let uses_common_config = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
        .unwrap_or(false);

    if !uses_common_config {
        return Ok(());
    }

    let Some(snippet) = db.get_config_snippet(app_type.as_str())? else {
        return Ok(());
    };

    if snippet.trim().is_empty() {
        return Ok(());
    }

    match remove_common_config_from_settings(app_type, &provider.settings_config, &snippet) {
        Ok(settings) => provider.settings_config = settings,
        Err(err) => {
            log::warn!(
                "Failed to normalize common config before saving {} provider '{}': {err}",
                app_type.as_str(),
                provider.id
            );
        }
    }

    Ok(())
}

/// Live configuration snapshot for backup/restore
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum LiveSnapshot {
    Claude {
        settings: Option<Value>,
    },
    Codex {
        auth: Option<Value>,
        config: Option<String>,
    },
    Gemini {
        env: Option<HashMap<String, String>>,
        config: Option<Value>,
    },
}

impl LiveSnapshot {
    #[allow(dead_code)]
    pub(crate) fn restore(&self) -> Result<(), AppError> {
        match self {
            LiveSnapshot::Claude { settings } => {
                for_each_claude_settings_path(|_, path| {
                    if let Some(value) = settings {
                        write_json_file(path, value)?;
                    } else if path.exists() {
                        delete_file(path)?;
                    }
                    Ok(())
                })?;
            }
            LiveSnapshot::Codex { auth, config } => {
                for_each_codex_live_path(|_, auth_path, config_path| {
                    if let Some(value) = auth {
                        write_json_file(auth_path, value)?;
                    } else if auth_path.exists() {
                        delete_file(auth_path)?;
                    }

                    if let Some(text) = config {
                        crate::config::write_text_file(config_path, text)?;
                    } else if config_path.exists() {
                        delete_file(config_path)?;
                    }
                    Ok(())
                })?;
            }
            LiveSnapshot::Gemini { env, .. } => {
                for_each_gemini_live_path(|_, env_path, settings_path| {
                    if let Some(env_map) = env {
                        write_gemini_env_at(env_path, env_map)?;
                    } else if env_path.exists() {
                        delete_file(env_path)?;
                    }

                    match self {
                        LiveSnapshot::Gemini {
                            config: Some(cfg), ..
                        } => {
                            write_json_file(settings_path, cfg)?;
                        }
                        LiveSnapshot::Gemini { config: None, .. } if settings_path.exists() => {
                            delete_file(settings_path)?;
                        }
                        _ => {}
                    }
                    Ok(())
                })?;
            }
        }
        Ok(())
    }
}

/// Write live configuration snapshot for a provider
pub(crate) fn write_live_snapshot(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => {
            let settings = sanitize_claude_settings_for_live(&provider.settings_config);
            for_each_claude_settings_path(|_, path| write_json_file(path, &settings))?;
        }
        AppType::Codex => {
            let obj = provider
                .settings_config
                .as_object()
                .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
            let auth = obj
                .get("auth")
                .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
            let config_str = obj.get("config").and_then(|v| v.as_str()).ok_or_else(|| {
                AppError::Config("Codex 供应商配置缺少 'config' 字段或不是字符串".to_string())
            })?;

            for_each_codex_live_path(|_, auth_path, config_path| {
                write_json_file(auth_path, auth)?;
                std::fs::write(config_path, config_str).map_err(|e| AppError::io(config_path, e))
            })?;
        }
        AppType::Gemini => {
            // Delegate to write_gemini_live which handles env file writing correctly
            write_gemini_live(provider)?;
        }
        AppType::OpenCode => {
            use crate::provider::OpenCodeProviderConfig;

            // Defensive check: if settings_config is a full config structure, extract provider fragment
            let config_to_write = if let Some(obj) = provider.settings_config.as_object() {
                // Detect full config structure (has $schema or top-level provider field)
                if obj.contains_key("$schema") || obj.contains_key("provider") {
                    log::warn!(
                        "OpenCode provider '{}' has full config structure in settings_config, attempting to extract fragment",
                        provider.id
                    );
                    // Try to extract from provider.{id}
                    obj.get("provider")
                        .and_then(|p| p.get(&provider.id))
                        .cloned()
                        .unwrap_or_else(|| provider.settings_config.clone())
                } else {
                    provider.settings_config.clone()
                }
            } else {
                provider.settings_config.clone()
            };

            // Convert settings_config to OpenCodeProviderConfig
            let opencode_config_result =
                serde_json::from_value::<OpenCodeProviderConfig>(config_to_write.clone());

            match opencode_config_result {
                Ok(config) => {
                    let value = serde_json::to_value(&config)
                        .map_err(|e| AppError::JsonSerialize { source: e })?;
                    for_each_opencode_config_path(|_, path| {
                        upsert_opencode_provider_at(path, &provider.id, value.clone())
                    })?;
                    log::info!("OpenCode provider '{}' written to live config", provider.id);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse OpenCode provider config for '{}': {}",
                        provider.id,
                        e
                    );
                    // Only write if config looks like a valid provider fragment
                    if config_to_write.get("npm").is_some()
                        || config_to_write.get("options").is_some()
                    {
                        for_each_opencode_config_path(|_, path| {
                            upsert_opencode_provider_at(path, &provider.id, config_to_write.clone())
                        })?;
                        log::info!(
                            "OpenCode provider '{}' written as raw JSON to live config",
                            provider.id
                        );
                    } else {
                        return Err(AppError::Message(format!(
                            "OpenCode provider '{}' has invalid config structure for live config (must contain 'npm' or 'options')",
                            provider.id
                        )));
                    }
                }
            }
        }
        AppType::OpenClaw => {
            use crate::openclaw_config::OpenClawProviderConfig;

            // Convert settings_config to OpenClawProviderConfig
            let openclaw_config_result =
                serde_json::from_value::<OpenClawProviderConfig>(provider.settings_config.clone());

            match openclaw_config_result {
                Ok(config) => {
                    let value = serde_json::to_value(&config)
                        .map_err(|e| AppError::JsonSerialize { source: e })?;
                    for_each_openclaw_config_path(|_, path| {
                        upsert_openclaw_provider_at(path, &provider.id, value.clone())
                    })?;
                    log::info!("OpenClaw provider '{}' written to live config", provider.id);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse OpenClaw provider config for '{}': {}",
                        provider.id,
                        e
                    );
                    // Try to write as raw JSON if it looks valid
                    if provider.settings_config.get("baseUrl").is_some()
                        || provider.settings_config.get("api").is_some()
                        || provider.settings_config.get("models").is_some()
                    {
                        for_each_openclaw_config_path(|_, path| {
                            upsert_openclaw_provider_at(
                                path,
                                &provider.id,
                                provider.settings_config.clone(),
                            )
                        })?;
                        log::info!(
                            "OpenClaw provider '{}' written as raw JSON to live config",
                            provider.id
                        );
                    } else {
                        return Err(AppError::Message(format!(
                            "OpenClaw provider '{}' has invalid config structure for live config (must contain 'baseUrl', 'api', or 'models')",
                            provider.id
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// Key fields definitions for partial merge
// ============================================================================

/// Claude env-level key fields that belong to the provider.
/// When adding a new field here, also update backfill_claude_key_fields().
#[allow(dead_code)]
const CLAUDE_KEY_ENV_FIELDS: &[&str] = &[
    // --- API auth & endpoint ---
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    // --- Model selection ---
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    // --- AWS Bedrock ---
    "CLAUDE_CODE_USE_BEDROCK",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_REGION",
    "AWS_PROFILE",
    "ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION",
    // --- Google Vertex AI ---
    "CLAUDE_CODE_USE_VERTEX",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "CLOUD_ML_REGION",
    // --- Microsoft Foundry ---
    "CLAUDE_CODE_USE_FOUNDRY",
    // --- Provider behavior ---
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "API_TIMEOUT_MS",
    "DISABLE_PROMPT_CACHING",
];

/// Claude top-level key fields (legacy + modern format).
/// When adding a new field here, also update backfill_claude_key_fields().
#[allow(dead_code)]
const CLAUDE_KEY_TOP_LEVEL: &[&str] = &[
    "apiBaseUrl",     // legacy
    "primaryModel",   // legacy
    "smallFastModel", // legacy
    "model",          // modern
    "apiKey",         // Bedrock API Key auth
];

/// Codex TOML key fields.
/// When adding a new field here, also update backfill_codex_key_fields().
#[allow(dead_code)]
const CODEX_KEY_TOP_LEVEL: &[&str] = &[
    "model_provider",
    "model",
    "model_reasoning_effort",
    "review_model",
    "plan_mode_reasoning_effort",
];

/// Gemini env-level key fields.
/// When adding a new field here, also update backfill_gemini_key_fields().
#[allow(dead_code)]
const GEMINI_KEY_ENV_FIELDS: &[&str] = &[
    "GOOGLE_GEMINI_BASE_URL",
    "GEMINI_API_KEY",
    "GEMINI_MODEL",
    "GOOGLE_API_KEY",
];

// ============================================================================
// Partial merge: write only key fields to live config
// ============================================================================

/// Write only provider-specific key fields to live configuration,
/// preserving all other user settings in the live file.
///
/// Used for switch-mode apps (Claude, Codex, Gemini) during:
/// - `switch_normal()` — switching providers
/// - `sync_current_to_live()` — startup sync
/// - `add()` / `update()` when the provider is current
#[allow(dead_code)]
pub(crate) fn write_live_partial(app_type: &AppType, provider: &Provider) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => write_claude_live_partial(provider),
        AppType::Codex => write_codex_live_partial(provider),
        AppType::Gemini => write_gemini_live_partial(provider),
        // Additive mode apps still use full snapshot
        AppType::OpenCode | AppType::OpenClaw => write_live_snapshot(app_type, provider),
    }
}

/// Apply a JSON merge patch (RFC 7396) directly to Claude live settings.json.
/// Used for user-level preferences (attribution, thinking, etc.) that are
/// independent of the active provider.
#[allow(dead_code)]
pub fn patch_claude_live(patch: Value) -> Result<(), AppError> {
    for_each_claude_settings_path(|_, path| {
        let mut live = if path.exists() {
            read_json_file(path).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };
        json_merge_patch(&mut live, &patch);
        let settings = sanitize_claude_settings_for_live(&live);
        write_json_file(path, &settings)
    })?;
    Ok(())
}

/// RFC 7396 JSON Merge Patch: null deletes, objects merge recursively, rest overwrites.
#[allow(dead_code)]
fn json_merge_patch(target: &mut Value, patch: &Value) {
    if let Some(patch_obj) = patch.as_object() {
        if !target.is_object() {
            *target = json!({});
        }
        let target_obj = target.as_object_mut().unwrap();
        for (key, value) in patch_obj {
            if value.is_null() {
                target_obj.remove(key);
            } else if value.is_object() {
                let entry = target_obj.entry(key.clone()).or_insert(json!({}));
                json_merge_patch(entry, value);
                // Clean up empty container objects
                if entry.as_object().is_some_and(|o| o.is_empty()) {
                    target_obj.remove(key);
                }
            } else {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Claude: merge only key env and top-level fields into live settings.json
#[allow(dead_code)]
fn write_claude_live_partial(provider: &Provider) -> Result<(), AppError> {
    for_each_claude_settings_path(|_, path| {
        // 1. Read existing live config (start from empty if file doesn't exist)
        let mut live = if path.exists() {
            read_json_file(path).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        // 2. Ensure live.env exists as an object
        if !live.get("env").is_some_and(|v| v.is_object()) {
            live.as_object_mut()
                .unwrap()
                .insert("env".into(), json!({}));
        }

        // 3. Clear key env fields from live, then write from provider
        let live_env = live.get_mut("env").unwrap().as_object_mut().unwrap();
        for key in CLAUDE_KEY_ENV_FIELDS {
            live_env.remove(*key);
        }

        if let Some(provider_env) = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
        {
            for key in CLAUDE_KEY_ENV_FIELDS {
                if let Some(value) = provider_env.get(*key) {
                    live_env.insert(key.to_string(), value.clone());
                }
            }
        }

        // 4. Handle top-level legacy key fields
        let live_obj = live.as_object_mut().unwrap();
        for key in CLAUDE_KEY_TOP_LEVEL {
            live_obj.remove(*key);
        }
        if let Some(provider_obj) = provider.settings_config.as_object() {
            for key in CLAUDE_KEY_TOP_LEVEL {
                if let Some(value) = provider_obj.get(*key) {
                    live_obj.insert(key.to_string(), value.clone());
                }
            }
        }

        // 5. Sanitize and write
        let settings = sanitize_claude_settings_for_live(&live);
        write_json_file(path, &settings)
    })?;
    Ok(())
}

/// Codex: replace auth.json entirely, partially merge config.toml key fields
#[allow(dead_code)]
fn write_codex_live_partial(provider: &Provider) -> Result<(), AppError> {
    let obj = provider
        .settings_config
        .as_object()
        .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;

    // auth.json is entirely provider-specific, replace it wholesale
    let auth = obj
        .get("auth")
        .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;

    let provider_config_str = obj.get("config").and_then(|v| v.as_str()).unwrap_or("");

    for_each_codex_live_path(|_, auth_path, config_path| {
        let existing_toml = if config_path.exists() {
            std::fs::read_to_string(config_path).unwrap_or_default()
        } else {
            String::new()
        };

        let mut live_doc = existing_toml
            .parse::<toml_edit::DocumentMut>()
            .unwrap_or_else(|_| toml_edit::DocumentMut::new());

        let live_root = live_doc.as_table_mut();
        for key in CODEX_KEY_TOP_LEVEL {
            live_root.remove(key);
        }
        live_root.remove("model_providers");

        if !provider_config_str.is_empty() {
            if let Ok(provider_doc) = provider_config_str.parse::<toml_edit::DocumentMut>() {
                let provider_root = provider_doc.as_table();

                for key in CODEX_KEY_TOP_LEVEL {
                    if let Some(item) = provider_root.get(key) {
                        live_root.insert(key, item.clone());
                    }
                }

                if let Some(mp) = provider_root.get("model_providers") {
                    live_root.insert("model_providers", mp.clone());
                }
            }
        }

        write_json_file(auth_path, auth)?;
        crate::config::write_text_file(config_path, &live_doc.to_string())
    })?;
    Ok(())
}

/// Gemini: merge only key env fields, preserve settings.json (MCP etc.)
#[allow(dead_code)]
fn write_gemini_live_partial(provider: &Provider) -> Result<(), AppError> {
    let auth_type = detect_gemini_auth_type(provider);

    for_each_gemini_live_path(|_, env_path, settings_path| {
        let mut env_map = if env_path.exists() {
            std::fs::read_to_string(env_path)
                .ok()
                .map(|content| crate::gemini_config::parse_env_file(&content))
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        for key in GEMINI_KEY_ENV_FIELDS {
            env_map.remove(*key);
        }

        if let Some(provider_env) = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
        {
            for key in GEMINI_KEY_ENV_FIELDS {
                if let Some(value) = provider_env.get(*key).and_then(|v| v.as_str()) {
                    if !value.is_empty() {
                        env_map.insert(key.to_string(), value.to_string());
                    }
                }
            }
        }

        match auth_type {
            GeminiAuthType::GoogleOfficial => {
                env_map.clear();
                write_gemini_env_at(env_path, &env_map)?;
            }
            GeminiAuthType::Packycode | GeminiAuthType::Generic => {
                crate::gemini_config::validate_gemini_settings_strict(&provider.settings_config)?;
                write_gemini_env_at(env_path, &env_map)?;
            }
        }

        if let Some(config_value) = provider.settings_config.get("config") {
            if config_value.is_object() {
                let mut merged = if settings_path.exists() {
                    read_json_file::<Value>(settings_path).unwrap_or_else(|_| json!({}))
                } else {
                    json!({})
                };
                if let (Some(merged_obj), Some(config_obj)) =
                    (merged.as_object_mut(), config_value.as_object())
                {
                    for (k, v) in config_obj {
                        merged_obj.insert(k.clone(), v.clone());
                    }
                }
                write_json_file(settings_path, &merged)?;
            } else if !config_value.is_null() {
                return Err(AppError::localized(
                    "gemini.validation.invalid_config",
                    "Gemini 配置格式错误: config 必须是对象或 null",
                    "Gemini config invalid: config must be an object or null",
                ));
            }
        }

        set_gemini_selected_type_at(settings_path, &auth_type)
    })?;

    Ok(())
}

// ============================================================================
// Backfill: extract only key fields from live config
// ============================================================================

/// Extract only provider-specific key fields from a live config value.
///
/// Used during backfill to ensure the provider's `settings_config` converges
/// to containing only key fields over time.
#[allow(dead_code)]
pub(crate) fn backfill_key_fields(app_type: &AppType, live_config: &Value) -> Value {
    match app_type {
        AppType::Claude => backfill_claude_key_fields(live_config),
        AppType::Codex => backfill_codex_key_fields(live_config),
        AppType::Gemini => backfill_gemini_key_fields(live_config),
        // Additive mode: return full config (no backfill needed)
        _ => live_config.clone(),
    }
}

#[allow(dead_code)]
fn backfill_claude_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // Extract key env fields
    if let Some(live_env) = live.get("env").and_then(|v| v.as_object()) {
        let mut env_obj = serde_json::Map::new();
        for key in CLAUDE_KEY_ENV_FIELDS {
            if let Some(value) = live_env.get(*key) {
                env_obj.insert(key.to_string(), value.clone());
            }
        }
        if !env_obj.is_empty() {
            result_obj.insert("env".to_string(), Value::Object(env_obj));
        }
    }

    // Extract key top-level fields
    if let Some(live_obj) = live.as_object() {
        for key in CLAUDE_KEY_TOP_LEVEL {
            if let Some(value) = live_obj.get(*key) {
                result_obj.insert(key.to_string(), value.clone());
            }
        }
    }

    result
}

#[allow(dead_code)]
fn backfill_codex_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // auth is entirely provider-specific — keep it as-is
    if let Some(auth) = live.get("auth") {
        result_obj.insert("auth".to_string(), auth.clone());
    }

    // Extract key TOML fields from config string
    if let Some(config_str) = live.get("config").and_then(|v| v.as_str()) {
        if let Ok(doc) = config_str.parse::<toml_edit::DocumentMut>() {
            let mut new_doc = toml_edit::DocumentMut::new();
            let new_root = new_doc.as_table_mut();

            // Copy key top-level fields
            for key in CODEX_KEY_TOP_LEVEL {
                if let Some(item) = doc.as_table().get(key) {
                    new_root.insert(key, item.clone());
                }
            }

            // Copy model_providers table
            if let Some(mp) = doc.as_table().get("model_providers") {
                new_root.insert("model_providers", mp.clone());
            }

            let toml_str = new_doc.to_string();
            if !toml_str.trim().is_empty() {
                result_obj.insert("config".to_string(), Value::String(toml_str));
            }
        }
    }

    result
}

#[allow(dead_code)]
fn backfill_gemini_key_fields(live: &Value) -> Value {
    let mut result = json!({});
    let result_obj = result.as_object_mut().unwrap();

    // Extract key env fields
    if let Some(live_env) = live.get("env").and_then(|v| v.as_object()) {
        let mut env_obj = serde_json::Map::new();
        for key in GEMINI_KEY_ENV_FIELDS {
            if let Some(value) = live_env.get(*key) {
                env_obj.insert(key.to_string(), value.clone());
            }
        }
        if !env_obj.is_empty() {
            result_obj.insert("env".to_string(), Value::Object(env_obj));
        }
    }

    result
}

/// Sync all providers to live configuration (for additive mode apps)
///
/// Writes all providers from the database to the live configuration file.
/// Used for OpenCode and other additive mode applications.
fn sync_all_providers_to_live(state: &AppState, app_type: &AppType) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let mut synced_count = 0usize;

    for provider in providers.values() {
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed)
            == Some(false)
        {
            continue;
        }

        if let Err(e) = write_live_with_common_config(state.db.as_ref(), app_type, provider) {
            log::warn!(
                "Failed to sync {:?} provider '{}' to live: {e}",
                app_type,
                provider.id
            );
            continue;
        }
        synced_count += 1;
    }

    log::info!("Synced {synced_count} {app_type:?} providers to live config");
    Ok(())
}

pub(crate) fn sync_current_provider_for_app_to_live(
    state: &AppState,
    app_type: &AppType,
) -> Result<(), AppError> {
    if app_type.is_additive_mode() {
        sync_all_providers_to_live(state, app_type)?;
    } else {
        let current_id = match crate::settings::get_effective_current_provider(&state.db, app_type)?
        {
            Some(id) => id,
            None => return Ok(()),
        };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get(&current_id) {
            write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
        }
    }

    McpService::sync_all_enabled(state)?;

    Ok(())
}

/// Sync current provider to live configuration
///
/// 使用有效的当前供应商 ID（验证过存在性）。
/// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
/// 这确保了配置导入后无效 ID 会自动 fallback 到数据库。
///
/// For additive mode apps (OpenCode), all providers are synced instead of just the current one.
pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    // Sync providers based on mode
    for app_type in AppType::all() {
        if app_type.is_additive_mode() {
            // Additive mode: sync ALL providers
            sync_all_providers_to_live(state, &app_type)?;
        } else {
            // Switch mode: sync only current provider
            let current_id =
                match crate::settings::get_effective_current_provider(&state.db, &app_type)? {
                    Some(id) => id,
                    None => continue,
                };

            let providers = state.db.get_all_providers(app_type.as_str())?;
            if let Some(provider) = providers.get(&current_id) {
                write_live_with_common_config(state.db.as_ref(), &app_type, provider)?;
            }
            // Note: get_effective_current_provider already validates existence,
            // so providers.get() should always succeed here
        }
    }

    // MCP sync
    McpService::sync_all_enabled(state)?;

    // Skill sync
    for app_type in AppType::all() {
        if let Err(e) = crate::services::skill::SkillService::sync_to_app(&state.db, &app_type) {
            log::warn!("同步 Skill 到 {app_type:?} 失败: {e}");
            // Continue syncing other apps, don't abort
        }
    }

    Ok(())
}

/// Read current live settings for an app type
pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
    match app_type {
        AppType::Codex => {
            let auth_path = get_codex_auth_path();
            if !auth_path.exists() {
                return Err(AppError::localized(
                    "codex.auth.missing",
                    "Codex 配置文件不存在：缺少 auth.json",
                    "Codex configuration missing: auth.json not found",
                ));
            }
            let auth: Value = read_json_file(&auth_path)?;
            let cfg_text = crate::codex_config::read_and_validate_codex_config_text()?;
            Ok(json!({ "auth": auth, "config": cfg_text }))
        }
        AppType::Claude => {
            let path = get_claude_settings_path();
            if !path.exists() {
                return Err(AppError::localized(
                    "claude.live.missing",
                    "Claude Code 配置文件不存在",
                    "Claude settings file is missing",
                ));
            }
            read_json_file(&path)
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

            // Read .env file (environment variables)
            let env_path = get_gemini_env_path();
            if !env_path.exists() {
                return Err(AppError::localized(
                    "gemini.env.missing",
                    "Gemini .env 文件不存在",
                    "Gemini .env file not found",
                ));
            }

            let env_map = read_gemini_env()?;
            let env_json = env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            Ok(json!({
                "env": env_obj,
                "config": config_obj
            }))
        }
        AppType::OpenCode => {
            use crate::opencode_config::{get_opencode_config_path, read_opencode_config};

            let config_path = get_opencode_config_path();
            if !config_path.exists() {
                return Err(AppError::localized(
                    "opencode.config.missing",
                    "OpenCode 配置文件不存在",
                    "OpenCode configuration file not found",
                ));
            }

            let config = read_opencode_config()?;
            Ok(config)
        }
        AppType::OpenClaw => {
            use crate::openclaw_config::{get_openclaw_config_path, read_openclaw_config};

            let config_path = get_openclaw_config_path();
            if !config_path.exists() {
                return Err(AppError::localized(
                    "openclaw.config.missing",
                    "OpenClaw 配置文件不存在",
                    "OpenClaw configuration file not found",
                ));
            }

            let config = read_openclaw_config()?;
            Ok(config)
        }
    }
}

/// Import default configuration from live files
///
/// Returns `Ok(true)` if a provider was actually imported,
/// `Ok(false)` if skipped (providers already exist for this app).
pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    // Additive mode apps (OpenCode, OpenClaw) should use their dedicated
    // import_xxx_providers_from_live functions, not this generic default config import
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    // 允许 "只有官方 seed 预设" 的情况下继续导入 live：
    // - 启动编排顺序是先 import 后 seed，新用户启动时 providers 为空，导入照常
    // - 老用户已有非 seed provider，跳过导入（正确）
    // - 用户手动点 ProviderEmptyState 的导入按钮时，与官方 seed 共存而不被阻塞
    if state.db.has_non_official_seed_provider(app_type.as_str())? {
        return Ok(false);
    }

    let settings_config = match app_type {
        AppType::Codex => {
            let auth_path = get_codex_auth_path();
            if !auth_path.exists() {
                return Err(AppError::localized(
                    "codex.live.missing",
                    "Codex 配置文件不存在",
                    "Codex configuration file is missing",
                ));
            }
            let auth: Value = read_json_file(&auth_path)?;
            let config_str = crate::codex_config::read_and_validate_codex_config_text()?;
            json!({ "auth": auth, "config": config_str })
        }
        AppType::Claude => {
            let settings_path = get_claude_settings_path();
            if !settings_path.exists() {
                return Err(AppError::localized(
                    "claude.live.missing",
                    "Claude Code 配置文件不存在",
                    "Claude settings file is missing",
                ));
            }
            let mut v = read_json_file::<Value>(&settings_path)?;
            let _ = normalize_claude_models_in_value(&mut v);
            v
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
            };

            // Read .env file (environment variables)
            let env_path = get_gemini_env_path();
            if !env_path.exists() {
                return Err(AppError::localized(
                    "gemini.live.missing",
                    "Gemini 配置文件不存在",
                    "Gemini configuration file is missing",
                ));
            }

            let env_map = read_gemini_env()?;
            let env_json = env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

            // Read settings.json file (MCP config etc.)
            let settings_path = get_gemini_settings_path();
            let config_obj = if settings_path.exists() {
                read_json_file(&settings_path)?
            } else {
                json!({})
            };

            // Return complete structure: { "env": {...}, "config": {...} }
            json!({
                "env": env_obj,
                "config": config_obj
            })
        }
        // OpenCode and OpenClaw use additive mode and are handled by early return above
        AppType::OpenCode | AppType::OpenClaw => {
            unreachable!("additive mode apps are handled by early return")
        }
    };

    let mut provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        settings_config,
        None,
    );
    provider.category = Some("custom".to_string());

    state.db.save_provider(app_type.as_str(), &provider)?;
    state
        .db
        .set_current_provider(app_type.as_str(), &provider.id)?;

    Ok(true) // 真正导入了
}

/// Write Gemini live configuration with authentication handling
pub(crate) fn write_gemini_live(provider: &Provider) -> Result<(), AppError> {
    use crate::gemini_config::{json_to_env, validate_gemini_settings_strict};

    // One-time auth type detection to avoid repeated detection
    let auth_type = detect_gemini_auth_type(provider);

    let env_map = json_to_env(&provider.settings_config)?;

    for_each_gemini_live_path(|_, env_path, settings_path| {
        let mut local_env_map = env_map.clone();

        let mut config_to_write: Option<Value> = None;

        if let Some(config_value) = provider.settings_config.get("config") {
            if config_value.is_object() {
                let mut merged = if settings_path.exists() {
                    read_json_file::<Value>(settings_path).unwrap_or_else(|_| json!({}))
                } else {
                    json!({})
                };

                if let (Some(merged_obj), Some(config_obj)) =
                    (merged.as_object_mut(), config_value.as_object())
                {
                    for (k, v) in config_obj {
                        merged_obj.insert(k.clone(), v.clone());
                    }
                }
                config_to_write = Some(merged);
            } else if !config_value.is_null() {
                return Err(AppError::localized(
                    "gemini.validation.invalid_config",
                    "Gemini 配置格式错误: config 必须是对象或 null",
                    "Gemini config invalid: config must be an object or null",
                ));
            }
        }

        if config_to_write.is_none() && settings_path.exists() {
            config_to_write = Some(read_json_file(settings_path)?);
        }

        match auth_type {
            GeminiAuthType::GoogleOfficial => {
                local_env_map.clear();
                write_gemini_env_at(env_path, &local_env_map)?;
            }
            GeminiAuthType::Packycode | GeminiAuthType::Generic => {
                validate_gemini_settings_strict(&provider.settings_config)?;
                write_gemini_env_at(env_path, &local_env_map)?;
            }
        }

        if let Some(config_value) = config_to_write {
            write_json_file(settings_path, &config_value)?;
        }

        set_gemini_selected_type_at(settings_path, &auth_type)
    })?;

    Ok(())
}

/// Remove an OpenCode provider from the live configuration
///
/// This is specific to OpenCode's additive mode - removing a provider
/// from the opencode.json file.
pub(crate) fn remove_opencode_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    for_each_opencode_config_path(|_, path| remove_opencode_provider_at(path, provider_id))?;
    log::info!("OpenCode provider '{provider_id}' removed from live config");

    Ok(())
}

/// Import all providers from OpenCode live config to database
///
/// This imports existing providers from ~/.config/opencode/opencode.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::opencode_config;

    let providers = opencode_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let existing_ids = state.db.get_provider_ids("opencode")?;

    for (id, config) in providers {
        // Skip if already exists in database
        if existing_ids.contains(&id) {
            log::debug!("OpenCode provider '{id}' already exists in database, skipping");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenCode provider '{id}': {e}");
                continue;
            }
        };

        // Create provider
        let mut provider = Provider::with_id(
            id.clone(),
            config.name.clone().unwrap_or_else(|| id.clone()),
            settings_config,
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("opencode", &provider) {
            log::warn!("Failed to import OpenCode provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenCode provider '{id}' from live config");
    }

    Ok(imported)
}

/// Import all providers from OpenClaw live config to database
///
/// This imports existing providers from ~/.openclaw/openclaw.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::openclaw_config;

    let providers = openclaw_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let existing_ids = state.db.get_provider_ids("openclaw")?;

    for (id, config) in providers {
        // Validate: skip entries with empty id or no models
        if id.trim().is_empty() {
            log::warn!("Skipping OpenClaw provider with empty id");
            continue;
        }
        if config.models.is_empty() {
            log::warn!("Skipping OpenClaw provider '{id}': no models defined");
            continue;
        }

        // Skip if already exists in database
        if existing_ids.contains(&id) {
            log::debug!("OpenClaw provider '{id}' already exists in database, skipping");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenClaw provider '{id}': {e}");
                continue;
            }
        };

        // Determine display name: use first model name if available, otherwise use id
        let display_name = config
            .models
            .first()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| id.clone());

        // Create provider
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("openclaw", &provider) {
            log::warn!("Failed to import OpenClaw provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenClaw provider '{id}' from live config");
    }

    Ok(imported)
}

/// Remove an OpenClaw provider from live config
///
/// This removes a specific provider from ~/.openclaw/openclaw.json
/// without affecting other providers in the file.
pub fn remove_openclaw_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    for_each_openclaw_config_path(|_, path| remove_openclaw_provider_at(path, provider_id))?;
    log::info!("OpenClaw provider '{provider_id}' removed from live config");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn claude_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "env": {
                "ANTHROPIC_API_KEY": "sk-test"
            }
        });
        let snippet = r#"{
  "includeCoAuthoredBy": false,
  "env": {
    "CLAUDE_CODE_USE_BEDROCK": "1"
  }
}"#;

        let applied =
            apply_common_config_to_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(applied["includeCoAuthoredBy"], json!(false));
        assert_eq!(applied["env"]["CLAUDE_CODE_USE_BEDROCK"], json!("1"));

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn codex_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            },
            "config": "model_provider = \"openai\"\n[general]\nmodel = \"gpt-5\"\n"
        });
        let snippet = "[shared]\nreasoning = \"medium\"\n";

        let applied = apply_common_config_to_settings(&AppType::Codex, &settings, snippet).unwrap();
        let applied_config = applied["config"].as_str().unwrap_or_default();
        assert!(applied_config.contains("[shared]"));
        assert!(applied_config.contains("reasoning = \"medium\""));

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn explicit_common_config_flag_overrides_legacy_subset_detection() {
        let mut provider = Provider::with_id(
            "claude-test".to_string(),
            "Claude Test".to_string(),
            json!({
                "includeCoAuthoredBy": false
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            common_config_enabled: Some(false),
            ..Default::default()
        });

        assert!(
            !provider_uses_common_config(
                &AppType::Claude,
                &provider,
                Some(r#"{ "includeCoAuthoredBy": false }"#),
            ),
            "explicit false should win over legacy subset detection"
        );
    }

    #[test]
    fn claude_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "allowedTools": ["tool1", "tool2"]
        });
        let snippet = r#"{
  "allowedTools": ["tool1"]
}"#;

        assert!(
            settings_contain_common_config(&AppType::Claude, &settings, snippet),
            "array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(
            stripped,
            json!({
                "allowedTools": ["tool2"]
            })
        );
    }

    #[test]
    fn codex_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "auth": {},
            "config": "allowed_tools = [\"tool1\", \"tool2\"]\n"
        });
        let snippet = "allowed_tools = [\"tool1\"]\n";

        assert!(
            settings_contain_common_config(&AppType::Codex, &settings, snippet),
            "TOML array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &settings, snippet).unwrap();
        assert_eq!(stripped["auth"], json!({}));
        let stripped_config = stripped["config"].as_str().unwrap_or_default();
        let parsed = stripped_config
            .parse::<DocumentMut>()
            .expect("stripped codex config should remain valid TOML");
        let allowed_tools = parsed["allowed_tools"]
            .as_array()
            .expect("allowed_tools should remain an array");
        let values: Vec<&str> = allowed_tools
            .iter()
            .map(|value| value.as_str().expect("tool id should be string"))
            .collect();
        assert_eq!(values, vec!["tool2"]);
    }

    #[test]
    fn upsert_openclaw_provider_recovers_when_models_is_not_an_object() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("cc-switch-openclaw-live-{unique}.json"));

        std::fs::write(&path, r#"{ "models": "broken" }"#)
            .expect("should write malformed openclaw config");

        upsert_openclaw_provider_at(
            &path,
            "test-provider",
            json!({
                "api": {
                    "baseUrl": "https://example.com"
                }
            }),
        )
        .expect("upsert should recover from malformed models value");

        let written = read_json_file::<Value>(&path).expect("should read repaired config");
        assert_eq!(written["models"]["mode"], json!("merge"));
        assert_eq!(
            written["models"]["providers"]["test-provider"]["api"]["baseUrl"],
            json!("https://example.com")
        );

        let _ = std::fs::remove_file(&path);
    }
}
