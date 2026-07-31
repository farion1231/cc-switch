//! Qoder CLI configuration support.
//!
//! Qoder BYOK is catalog-backed. Providers and plan `type` values must match
//! Qoder's catalog, while the CLI also permits another model ID within a
//! supported provider/type group. Arbitrary OpenAI-compatible base URLs are
//! intentionally not supported here.

use crate::config::{get_home_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::settings::get_qodercli_override_dir;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

type CatalogEntry = (&'static str, &'static str);

const BAILIAN_MODELS: &[CatalogEntry] = &[
    ("qwen3.8-max-tp", "tp"),
    ("qwen3.7-max-tp", "tp"),
    ("qwen3.7-plus-tp", "tp"),
    ("qwen3.6-plus-tp", "tp"),
    ("qwen3.6-flash-tp", "tp"),
    ("glm5.2-tp", "tp"),
    ("glm5.1-tp", "tp"),
    ("glm5-tp", "tp"),
    ("kimi-k2.7-code-tp", "tp"),
    ("kimi-k2.6-tp", "tp"),
    ("kimi-k2.5-tp", "tp"),
    ("deepseek-v4-flash-tp", "tp"),
    ("deepseek-v4-pro-tp", "tp"),
    ("minimax-m2.5-tp", "tp"),
    ("qwen3.7-plus-cp", "cp"),
    ("qwen3.6-plus-cp", "cp"),
    ("glm5-cp", "cp"),
    ("kimi-k2.5-cp", "cp"),
    ("minimax-m2.5-cp", "cp"),
    ("qwen3.7-max-pg", "pg"),
    ("qwen3.7-plus-pg", "pg"),
    ("qwen3.6-max-pg", "pg"),
    ("qwen3.6-plus-pg", "pg"),
    ("glm5.2-pg", "pg"),
    ("deepseek-v4-pro-pg", "pg"),
];

const BAILIAN_AMERICA_MODELS: &[CatalogEntry] = &[
    ("qwen3.7-max-pg", "pg"),
    ("qwen3.7-plus-pg", "pg"),
    ("qwen3.6-max-pg", "pg"),
    ("qwen3.6-plus-pg", "pg"),
    ("glm5.2-pg", "pg"),
    ("deepseek-v4-pro-pg", "pg"),
];

const ZHIPU_MODELS: &[CatalogEntry] = &[
    ("glm5.2-cp", "cp"),
    ("glm5.1-cp", "cp"),
    ("glm-5v-turbo-cp", "cp"),
    ("glm5-cp", "cp"),
    ("glm4.7-cp", "cp"),
    ("glm4.6-cp", "cp"),
    ("glm5.2-pg", "pg"),
    ("glm5.1-pg", "pg"),
    ("glm-5v-turbo-pg", "pg"),
    ("glm5-pg", "pg"),
];

const ZHIPU_INTL_MODELS: &[CatalogEntry] = &[
    ("glm5.2-cp", "cp"),
    ("glm5.1-cp", "cp"),
    ("glm5-cp", "cp"),
    ("glm4.7-cp", "cp"),
    ("glm4.6-cp", "cp"),
    ("glm5.2-pg", "pg"),
    ("glm5.1-pg", "pg"),
    ("glm-5v-turbo-pg", "pg"),
    ("glm5-pg", "pg"),
];

const KIMI_MODELS: &[CatalogEntry] = &[
    ("kimi-k3-pg", "pg"),
    ("kimi-k2.7-code-pg", "pg"),
    ("kimi-k2.7-code-highspeed-pg", "pg"),
    ("kimi-k2.6-pg", "pg"),
    ("kimi-k3-cp", "cp"),
    ("kimi-k2.7-code-cp", "cp"),
    ("kimi-k2.7-code-highspeed-cp", "cp"),
    ("kimi-k2.6-cp", "cp"),
    ("kimi-for-coding-cp", "cp"),
];

const MINIMAX_MODELS: &[CatalogEntry] = &[
    ("minimax-m3-cp", "cp"),
    ("minimax-m2.7-cp", "cp"),
    ("minimax-m2.7-highspeed-cp", "cp"),
    ("minimax-m2.5-cp", "cp"),
];

const DEEPSEEK_MODELS: &[CatalogEntry] =
    &[("deepseek-v4-pro-pg", "pg"), ("deepseek-v4-flash-pg", "pg")];

const XIAOMI_MODELS: &[CatalogEntry] = &[
    ("mimo-v2.5-pro-tp", "tp"),
    ("mimo-v2.5-tp", "tp"),
    ("mimo-v2.5-pro-pg", "pg"),
    ("mimo-v2.5-pg", "pg"),
];

fn provider_catalog(provider: &str) -> Option<&'static [CatalogEntry]> {
    match provider {
        "bailian" | "bailian-intl" => Some(BAILIAN_MODELS),
        "bailian-america" => Some(BAILIAN_AMERICA_MODELS),
        "zhipu" => Some(ZHIPU_MODELS),
        "zhipu-intl" => Some(ZHIPU_INTL_MODELS),
        "kimi" => Some(KIMI_MODELS),
        "minimax" | "minimax-intl" => Some(MINIMAX_MODELS),
        "deepseek" => Some(DEEPSEEK_MODELS),
        "xiaomi-china" => Some(XIAOMI_MODELS),
        _ => None,
    }
}

fn validate_provider(provider: &str) -> Result<(), AppError> {
    if provider_catalog(provider).is_some() {
        return Ok(());
    }
    Err(AppError::localized(
        "provider.qodercli.provider.unsupported",
        format!("Qoder 不支持供应商 '{provider}'，请从官方 BYOK 目录中选择"),
        format!(
            "Qoder provider '{provider}' is not supported; select one from the official BYOK catalog"
        ),
    ))
}

fn validate_model(provider: &str, model: &QoderCliModel) -> Result<(), AppError> {
    let model_id = model.id.trim();
    let plan_type = model.plan_type.trim();
    let format = model.format.trim();
    let is_supported_type = provider_catalog(provider)
        .is_some_and(|catalog| catalog.iter().any(|(_, kind)| *kind == plan_type));
    if !model_id.is_empty() && is_supported_type && format == "openai" {
        return Ok(());
    }

    Err(AppError::localized(
        "provider.qodercli.model.unsupported",
        format!(
            "模型 '{model_id}' 的套餐或格式（{plan_type}/{format}）不受 Qoder 供应商 '{provider}' 支持"
        ),
        format!(
            "Model '{model_id}' uses a plan or format ({plan_type}/{format}) not supported by Qoder provider '{provider}'"
        ),
    ))
}

fn validate_api_key(api_key: &str) -> Result<(), AppError> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(AppError::localized(
            "provider.qodercli.apikey.empty",
            "API Key 不能为空",
            "API key must not be empty",
        ));
    }
    if trimmed.contains("${") {
        return Err(AppError::localized(
            "provider.qodercli.apikey.placeholder",
            "API Key 不能包含未解析的 ${...} 占位符",
            "API key must not contain unresolved ${...} placeholders",
        ));
    }
    Ok(())
}

pub fn get_qodercli_dir() -> PathBuf {
    get_qodercli_override_dir().unwrap_or_else(|| get_home_dir().join(".qoder"))
}

pub fn get_qodercli_settings_path() -> PathBuf {
    get_qodercli_dir().join("settings.json")
}

pub fn read_qodercli_settings() -> Result<Value, AppError> {
    let path = get_qodercli_settings_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    read_json_file::<Value>(&path)
}

pub fn write_qodercli_settings(settings: &Value) -> Result<(), AppError> {
    let path = get_qodercli_settings_path();
    write_json_file(&path, settings)?;
    log::debug!("qodercli settings written to {path:?}");
    Ok(())
}

fn ensure_custom_models_mut(settings: &mut Value) -> &mut Vec<Value> {
    if !settings.is_object() {
        *settings = json!({});
    }
    let root = settings
        .as_object_mut()
        .expect("settings was normalized to an object");
    let model_configs = root
        .entry("modelConfigs".to_string())
        .or_insert_with(|| json!({}));
    if !model_configs.is_object() {
        *model_configs = json!({});
    }
    let model_configs = model_configs
        .as_object_mut()
        .expect("modelConfigs was normalized to an object");
    let custom_models = model_configs
        .entry("customModels".to_string())
        .or_insert_with(|| json!([]));
    if !custom_models.is_array() {
        *custom_models = json!([]);
    }
    custom_models
        .as_array_mut()
        .expect("customModels was normalized to an array")
}

fn get_custom_models() -> Result<Vec<Value>, AppError> {
    let settings = read_qodercli_settings()?;
    Ok(settings
        .get("modelConfigs")
        .and_then(|value| value.get("customModels"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn cleanup_model_configs_shell(settings: &mut Value) {
    if let Some(root) = settings.as_object_mut() {
        if let Some(model_configs) = root.get_mut("modelConfigs").and_then(Value::as_object_mut) {
            if model_configs
                .get("customModels")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                model_configs.remove("customModels");
            }
            if model_configs
                .get("providers")
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
            {
                model_configs.remove("providers");
            }
            if model_configs.is_empty() {
                root.remove("modelConfigs");
            }
        }
    }
}

fn default_format() -> String {
    "openai".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct QoderCliProviderConfig {
    #[serde(default)]
    pub provider: String,

    pub apiKey: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<QoderCliModel>,

    /// Read-only migration field for configurations created before catalog mode.
    /// It is never written to Qoder's live custom model entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseURL: Option<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct QoderCliModel {
    #[serde(rename = "model")]
    pub id: String,

    #[serde(rename = "type", default)]
    pub plan_type: String,

    #[serde(default = "default_format")]
    pub format: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub displayName: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxInputTokens: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub isVl: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub isReasoning: Option<bool>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl QoderCliProviderConfig {
    fn default_model_key(&self, fallback_provider: &str) -> Option<String> {
        let provider = if self.provider.trim().is_empty() {
            fallback_provider
        } else {
            self.provider.trim()
        };
        self.models
            .first()
            .map(|model| format!("{provider}/{}", model.id.trim()))
            .filter(|key| !key.ends_with('/'))
    }
}

pub fn model_record_id(provider: &str, model: &str) -> String {
    format!("{}/{}", provider.trim(), model.trim())
}

fn split_model_record_id(record_id: &str) -> Option<(&str, &str)> {
    let (provider, model) = record_id.split_once('/')?;
    if provider.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    Some((provider, model))
}

/// Preserve old data so editing can show a migration path, but do not infer an
/// arbitrary endpoint as a supported Qoder provider.
pub fn migrate_legacy(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut migrated = object.clone();
    if !migrated.contains_key("baseURL") {
        if let Some(base_url) = migrated.remove("baseUrl") {
            migrated.insert("baseURL".to_string(), base_url);
        }
    }
    if !migrated.contains_key("models") {
        if let Some(model_id) = migrated.get("model").and_then(Value::as_str) {
            let mut model = Map::new();
            model.insert("model".to_string(), json!(model_id));
            if let Some(tokens) = migrated.get("contextWindow").and_then(Value::as_u64) {
                model.insert("maxInputTokens".to_string(), json!(tokens));
            }
            migrated.insert("models".to_string(), json!([model]));
        }
    }
    migrated.remove("model");
    migrated.remove("contextWindow");
    migrated.remove("maxOutputTokens");
    Value::Object(migrated)
}

pub fn parse_provider_config(value: &Value) -> Result<QoderCliProviderConfig, serde_json::Error> {
    serde_json::from_value(migrate_legacy(value))
}

fn expand_to_custom_models(config: &QoderCliProviderConfig) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for model in &config.models {
        let model_id = model.id.trim();
        if model_id.is_empty() || !seen.insert((model_id, model.plan_type.trim())) {
            continue;
        }

        let mut entry = Map::new();
        entry.insert("provider".to_string(), json!(config.provider.trim()));
        entry.insert("apiKey".to_string(), json!(config.apiKey.trim()));
        entry.insert("model".to_string(), json!(model_id));
        entry.insert("type".to_string(), json!(model.plan_type.trim()));
        entry.insert("format".to_string(), json!(model.format.trim()));
        if let Some(name) = model
            .displayName
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry.insert("displayName".to_string(), json!(name));
        }
        if let Some(tokens) = model.maxInputTokens {
            entry.insert("maxInputTokens".to_string(), json!(tokens));
        }
        if let Some(is_vl) = model.isVl {
            entry.insert("isVl".to_string(), json!(is_vl));
        }
        if let Some(is_reasoning) = model.isReasoning {
            entry.insert("isReasoning".to_string(), json!(is_reasoning));
        }
        for (key, value) in &model.extra {
            entry.entry(key.clone()).or_insert_with(|| value.clone());
        }
        entries.push(Value::Object(entry));
    }
    entries
}

fn custom_model_entry_to_model(entry: &Map<String, Value>) -> Option<QoderCliModel> {
    let id = entry.get("model").and_then(Value::as_str)?.to_string();
    let known = [
        "provider",
        "apiKey",
        "model",
        "baseURL",
        "type",
        "format",
        "displayName",
        "maxInputTokens",
        "isVl",
        "isReasoning",
    ];
    let extra = entry
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Some(QoderCliModel {
        id,
        plan_type: entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        format: entry
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("openai")
            .to_string(),
        displayName: entry
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::to_string),
        maxInputTokens: entry.get("maxInputTokens").and_then(Value::as_u64),
        isVl: entry.get("isVl").and_then(Value::as_bool),
        isReasoning: entry.get("isReasoning").and_then(Value::as_bool),
        extra,
    })
}

pub fn get_typed_providers() -> Result<IndexMap<String, QoderCliProviderConfig>, AppError> {
    let mut result: IndexMap<String, QoderCliProviderConfig> = IndexMap::new();
    for entry in get_custom_models()? {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(provider) = object.get("provider").and_then(Value::as_str) else {
            continue;
        };
        if provider_catalog(provider).is_none() {
            log::debug!("Ignoring unsupported Qoder custom model provider '{provider}'");
            continue;
        }
        let Some(model) = custom_model_entry_to_model(object) else {
            continue;
        };
        if validate_model(provider, &model).is_err() {
            log::debug!(
                "Ignoring model '{}' because its plan or format is not valid for Qoder provider '{provider}'",
                model.id
            );
            continue;
        }
        let api_key = object
            .get("apiKey")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let legacy_base_url = object
            .get("baseURL")
            .and_then(Value::as_str)
            .map(str::to_string);

        result
            .entry(provider.to_string())
            .and_modify(|group| group.models.push(model.clone()))
            .or_insert_with(|| QoderCliProviderConfig {
                provider: provider.to_string(),
                apiKey: api_key.to_string(),
                models: vec![model],
                baseURL: legacy_base_url,
                extra: Map::new(),
            });
    }
    Ok(result)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let mut result = Map::new();
    for (provider, config) in get_typed_providers()? {
        for model in &config.models {
            let mut single_model_config = config.clone();
            single_model_config.models = vec![model.clone()];
            result.insert(
                model_record_id(&provider, &model.id),
                serde_json::to_value(single_model_config)
                    .map_err(|source| AppError::JsonSerialize { source })?,
            );
        }
    }
    Ok(result)
}

pub fn set_typed_provider(
    database_id: &str,
    config: &QoderCliProviderConfig,
) -> Result<(), AppError> {
    let provider = config.provider.trim();
    validate_provider(provider)?;
    validate_api_key(&config.apiKey)?;
    if config.models.is_empty() {
        return Err(AppError::localized(
            "provider.qodercli.models.empty",
            "请选择一个 Qoder 官方预设模型，或添加该供应商支持的其他模型",
            "Select an official Qoder model preset or add another model supported by the provider",
        ));
    }
    for model in &config.models {
        validate_model(provider, model)?;
    }

    let new_entries = expand_to_custom_models(config);
    let new_record_ids = config
        .models
        .iter()
        .map(|model| model_record_id(provider, &model.id))
        .collect::<std::collections::HashSet<_>>();
    let database_record_id = split_model_record_id(database_id);
    let mut settings = read_qodercli_settings()?;
    {
        let custom_models = ensure_custom_models_mut(&mut settings);
        custom_models.retain(|entry| {
            let entry_provider = entry.get("provider").and_then(Value::as_str);
            let entry_model = entry.get("model").and_then(Value::as_str);

            // Legacy UUID records stored the database ID directly as Qoder's
            // provider. Remove those while migrating to catalog-backed IDs.
            if entry_provider == Some(database_id) {
                return false;
            }

            let Some(entry_provider) = entry_provider else {
                return true;
            };
            let Some(entry_model) = entry_model else {
                return true;
            };
            let entry_record_id = model_record_id(entry_provider, entry_model);

            // A new write replaces only the same model, not every model from
            // the same supplier.
            if new_record_ids.contains(&entry_record_id) {
                return false;
            }

            if let Some((database_provider, database_model)) = database_record_id {
                return entry_provider != database_provider || entry_model != database_model;
            }

            // Old CC Switch builds used the bare official supplier key as the
            // database ID and represented all its models in one row.
            if database_id == provider {
                return entry_provider != provider;
            }

            true
        });
        custom_models.extend(new_entries);
    }

    if let Some(model_configs) = settings
        .get_mut("modelConfigs")
        .and_then(Value::as_object_mut)
    {
        if let Some(providers) = model_configs
            .get_mut("providers")
            .and_then(Value::as_object_mut)
        {
            providers.remove(database_id);
            providers.remove(provider);
        }
    }
    cleanup_model_configs_shell(&mut settings);
    write_qodercli_settings(&settings)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut settings = read_qodercli_settings()?;
    let mut removed = false;
    let record_id = split_model_record_id(id);
    if let Some(custom_models) = settings
        .get_mut("modelConfigs")
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("customModels"))
        .and_then(Value::as_array_mut)
    {
        let before = custom_models.len();
        custom_models.retain(|entry| {
            let entry_provider = entry.get("provider").and_then(Value::as_str);
            let entry_model = entry.get("model").and_then(Value::as_str);
            match record_id {
                Some((provider, model)) => {
                    entry_provider != Some(provider) || entry_model != Some(model)
                }
                None => entry_provider != Some(id),
            }
        });
        removed = custom_models.len() < before;
    }
    if let Some(providers) = settings
        .get_mut("modelConfigs")
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        if record_id.is_none() {
            removed |= providers.remove(id).is_some();
        }
    }
    cleanup_model_configs_shell(&mut settings);
    if removed {
        write_qodercli_settings(&settings)?;
        log::info!("qodercli provider '{id}' removed from live config");
    }
    Ok(())
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let settings = read_qodercli_settings()?;
    Ok(settings
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, spec: Value) -> Result<(), AppError> {
    let mut settings = read_qodercli_settings()?;
    if !settings.is_object() {
        settings = json!({});
    }
    let root = settings
        .as_object_mut()
        .expect("settings was normalized to an object");
    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    servers
        .as_object_mut()
        .expect("mcpServers was normalized to an object")
        .insert(id.to_string(), spec);
    write_qodercli_settings(&settings)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut settings = read_qodercli_settings()?;
    let removed = settings
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .is_some_and(|servers| servers.remove(id).is_some());
    if settings
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        settings
            .as_object_mut()
            .expect("settings containing mcpServers is an object")
            .remove("mcpServers");
    }
    if removed {
        write_qodercli_settings(&settings)?;
    }
    Ok(())
}

pub fn apply_switch_defaults(provider_id: &str, settings_config: &Value) -> Result<(), AppError> {
    let model_key = parse_provider_config(settings_config)
        .ok()
        .and_then(|config| config.default_model_key(provider_id))
        .filter(|key| !key.ends_with('/'));
    let Some(model_key) = model_key else {
        log::warn!("qodercli provider '{provider_id}' has no active model");
        return Ok(());
    };

    let mut settings = read_qodercli_settings()?;
    if !settings.is_object() {
        settings = json!({});
    }
    let root = settings
        .as_object_mut()
        .expect("settings was normalized to an object");
    let model = root.entry("model".to_string()).or_insert_with(|| json!({}));
    if !model.is_object() {
        *model = json!({});
    }
    model
        .as_object_mut()
        .expect("model was normalized to an object")
        .insert("name".to_string(), Value::String(model_key.clone()));
    write_qodercli_settings(&settings)?;
    log::info!("qodercli active model set to '{model_key}'");
    Ok(())
}

#[allow(dead_code)]
pub fn get_active_model_name() -> Result<Option<String>, AppError> {
    let settings = read_qodercli_settings()?;
    Ok(settings
        .get("model")
        .and_then(|model| model.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn isolate_home() -> (TempDir, Option<std::ffi::OsString>) {
        let temp = TempDir::new().expect("temp dir");
        let original = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        (temp, original)
    }

    fn restore_home(original: Option<std::ffi::OsString>) {
        match original {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    fn qoder_model(id: &str, plan_type: &str) -> QoderCliModel {
        QoderCliModel {
            id: id.to_string(),
            plan_type: plan_type.to_string(),
            format: "openai".to_string(),
            displayName: Some(id.to_string()),
            maxInputTokens: Some(1_000_000),
            isVl: None,
            isReasoning: Some(true),
            extra: Map::new(),
        }
    }

    fn sample_config() -> QoderCliProviderConfig {
        QoderCliProviderConfig {
            provider: "deepseek".to_string(),
            apiKey: "test-api-key".to_string(),
            models: vec![
                qoder_model("deepseek-v4-pro-pg", "pg"),
                qoder_model("deepseek-v4-flash-pg", "pg"),
            ],
            baseURL: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn catalog_restricts_provider_type_and_format_but_allows_other_model_ids() {
        assert!(validate_provider("deepseek").is_ok());
        assert!(validate_provider("my-openai").is_err());
        assert!(validate_model("deepseek", &qoder_model("deepseek-v4-pro-pg", "pg")).is_ok());
        assert!(validate_model("deepseek", &qoder_model("deepseek-v4-pro-pg", "tp")).is_err());
        assert!(validate_model("deepseek", &qoder_model("deepseek-chat", "pg")).is_ok());
        assert!(validate_model("deepseek", &qoder_model("", "pg")).is_err());
        let mut wrong_format = qoder_model("deepseek-chat", "pg");
        wrong_format.format = "anthropic".to_string();
        assert!(validate_model("deepseek", &wrong_format).is_err());
    }

    #[test]
    fn migration_keeps_legacy_endpoint_only_as_database_metadata() {
        let migrated = migrate_legacy(&json!({
            "baseUrl": "https://api.example.com/v1",
            "apiKey": "old",
            "model": "gpt-4o",
            "contextWindow": 128000
        }));
        assert_eq!(
            migrated.get("baseURL").and_then(Value::as_str),
            Some("https://api.example.com/v1")
        );
        assert_eq!(
            migrated
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| models.first())
                .and_then(|model| model.get("model"))
                .and_then(Value::as_str),
            Some("gpt-4o")
        );
    }

    #[test]
    #[serial]
    fn roundtrip_writes_only_official_qoder_fields_and_preserves_other_keys() {
        let (_temp, original) = isolate_home();
        write_qodercli_settings(&json!({
            "permissions": { "trustDirectories": ["C:/work"] },
            "modelConfigs": {
                "customModels": [
                    {
                        "provider": "other",
                        "apiKey": "other-key",
                        "model": "other-model",
                        "baseURL": "https://other.example"
                    }
                ]
            }
        }))
        .expect("seed");

        set_typed_provider("legacy-uuid", &sample_config()).expect("set");
        let settings = read_qodercli_settings().expect("read");
        let entries = settings
            .get("modelConfigs")
            .and_then(|value| value.get("customModels"))
            .and_then(Value::as_array)
            .expect("customModels");
        assert_eq!(entries.len(), 3);
        let mine: Vec<&Value> = entries
            .iter()
            .filter(|entry| entry.get("provider").and_then(Value::as_str) == Some("deepseek"))
            .collect();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].get("type").and_then(Value::as_str), Some("pg"));
        assert_eq!(
            mine[0].get("format").and_then(Value::as_str),
            Some("openai")
        );
        assert!(mine[0].get("baseURL").is_none());
        assert!(settings.get("permissions").is_some());

        let typed = get_typed_providers().expect("typed");
        assert_eq!(typed.len(), 1);
        assert_eq!(typed["deepseek"].models.len(), 2);

        restore_home(original);
    }

    #[test]
    #[serial]
    fn set_replaces_legacy_uuid_and_same_official_provider() {
        let (_temp, original) = isolate_home();
        write_qodercli_settings(&json!({
            "modelConfigs": {
                "customModels": [
                    {"provider": "old-uuid", "apiKey": "old", "model": "bad"},
                    {
                        "provider": "deepseek",
                        "apiKey": "old",
                        "model": "deepseek-v4-pro-pg",
                        "type": "pg",
                        "format": "openai"
                    }
                ]
            }
        }))
        .expect("seed");

        set_typed_provider("old-uuid", &sample_config()).expect("set");
        let entries = get_custom_models().expect("entries");
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .all(|entry| { entry.get("provider").and_then(Value::as_str) == Some("deepseek") }));

        restore_home(original);
    }

    #[test]
    #[serial]
    fn separate_model_records_from_same_provider_coexist_and_switch() {
        let (_temp, original) = isolate_home();
        let mut pro = sample_config();
        pro.models = vec![qoder_model("deepseek-v4-pro-pg", "pg")];
        let mut flash = sample_config();
        flash.models = vec![qoder_model("deepseek-v4-flash-pg", "pg")];

        set_typed_provider("deepseek/deepseek-v4-pro-pg", &pro).expect("add pro");
        set_typed_provider("deepseek/deepseek-v4-flash-pg", &flash).expect("add flash");

        let entries = get_custom_models().expect("entries");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| {
            entry.get("model").and_then(Value::as_str) == Some("deepseek-v4-pro-pg")
        }));
        assert!(entries.iter().any(|entry| {
            entry.get("model").and_then(Value::as_str) == Some("deepseek-v4-flash-pg")
        }));

        let providers = get_providers().expect("providers");
        assert!(providers.contains_key("deepseek/deepseek-v4-pro-pg"));
        assert!(providers.contains_key("deepseek/deepseek-v4-flash-pg"));

        apply_switch_defaults(
            "deepseek/deepseek-v4-pro-pg",
            &serde_json::to_value(&pro).unwrap(),
        )
        .expect("switch pro");
        assert_eq!(
            get_active_model_name().expect("active").as_deref(),
            Some("deepseek/deepseek-v4-pro-pg")
        );
        apply_switch_defaults(
            "deepseek/deepseek-v4-flash-pg",
            &serde_json::to_value(&flash).unwrap(),
        )
        .expect("switch flash");
        assert_eq!(
            get_active_model_name().expect("active").as_deref(),
            Some("deepseek/deepseek-v4-flash-pg")
        );

        remove_provider("deepseek/deepseek-v4-pro-pg").expect("remove pro");
        let entries = get_custom_models().expect("entries after remove");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("model").and_then(Value::as_str),
            Some("deepseek-v4-flash-pg")
        );
        restore_home(original);
    }

    #[test]
    #[serial]
    fn manually_entered_model_roundtrips_for_a_supported_provider_and_plan() {
        let (_temp, original) = isolate_home();
        let config = QoderCliProviderConfig {
            provider: "kimi".to_string(),
            apiKey: "test-api-key".to_string(),
            models: vec![qoder_model("moonshot-v1-custom", "cp")],
            baseURL: None,
            extra: Map::new(),
        };

        set_typed_provider("kimi/moonshot-v1-custom", &config).expect("set custom model");
        let entries = get_custom_models().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("provider").and_then(Value::as_str),
            Some("kimi")
        );
        assert_eq!(
            entries[0].get("model").and_then(Value::as_str),
            Some("moonshot-v1-custom")
        );
        assert_eq!(entries[0].get("type").and_then(Value::as_str), Some("cp"));

        let providers = get_providers().expect("providers");
        assert!(providers.contains_key("kimi/moonshot-v1-custom"));
        apply_switch_defaults(
            "kimi/moonshot-v1-custom",
            &serde_json::to_value(&config).unwrap(),
        )
        .expect("switch custom model");
        assert_eq!(
            get_active_model_name().expect("active").as_deref(),
            Some("kimi/moonshot-v1-custom")
        );
        restore_home(original);
    }

    #[test]
    #[serial]
    fn remove_provider_cleans_only_its_entries() {
        let (_temp, original) = isolate_home();
        set_typed_provider("deepseek", &sample_config()).expect("set");
        remove_provider("deepseek").expect("remove");
        let settings = read_qodercli_settings().expect("read");
        assert!(settings.get("modelConfigs").is_none());
        restore_home(original);
    }

    #[test]
    #[serial]
    fn switch_sets_official_provider_model_key_and_preserves_context() {
        let (_temp, original) = isolate_home();
        write_qodercli_settings(&json!({
            "model": {"name": "old/model", "contextWindow": 200000}
        }))
        .expect("seed");
        let config = serde_json::to_value(sample_config()).expect("serialize");
        apply_switch_defaults("legacy-uuid", &config).expect("switch");
        let settings = read_qodercli_settings().expect("read");
        assert_eq!(
            settings
                .get("model")
                .and_then(|model| model.get("name"))
                .and_then(Value::as_str),
            Some("deepseek/deepseek-v4-pro-pg")
        );
        assert_eq!(
            settings
                .get("model")
                .and_then(|model| model.get("contextWindow"))
                .and_then(Value::as_u64),
            Some(200000)
        );
        restore_home(original);
    }

    #[test]
    #[serial]
    fn set_rejects_unknown_provider_invalid_plan_or_key_without_writing() {
        let (_temp, original) = isolate_home();
        let mut bad_provider = sample_config();
        bad_provider.provider = "openai".to_string();
        assert!(set_typed_provider("openai", &bad_provider).is_err());

        let mut bad_plan = sample_config();
        bad_plan.models = vec![qoder_model("deepseek-chat", "tp")];
        assert!(set_typed_provider("deepseek", &bad_plan).is_err());

        let mut bad_key = sample_config();
        bad_key.apiKey = "${API_KEY}".to_string();
        assert!(set_typed_provider("deepseek", &bad_key).is_err());
        assert!(get_custom_models().expect("models").is_empty());
        restore_home(original);
    }
}
