use crate::config::atomic_write;
use crate::error::AppError;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub fn get_pi_dir() -> PathBuf {
    crate::settings::get_pi_override_dir()
        .unwrap_or_else(|| crate::config::get_home_dir().join(".pi").join("agent"))
}

pub fn get_pi_sessions_dir() -> PathBuf {
    get_pi_dir().join("sessions")
}

pub fn get_pi_models_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

fn pi_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn default_pi_models_config() -> Value {
    json!({
        "providers": {},
        "defaultProvider": null,
        "defaultModel": null
    })
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value should be object after normalization")
}

fn ensure_providers(config: &mut Value) -> &mut Map<String, Value> {
    let root = ensure_object(config);
    let providers = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !providers.is_object() {
        *providers = Value::Object(Map::new());
    }
    providers
        .as_object_mut()
        .expect("providers should be object after normalization")
}

fn first_model_id(provider_config: &Value) -> Option<String> {
    let models = provider_config.get("models")?.as_array()?;
    let first = models.first()?;
    match first {
        Value::String(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(obj) => obj
            .get("id")
            .or_else(|| obj.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        _ => None,
    }
}

fn normalize_provider_config_for_write(mut provider_config: Value) -> Value {
    if let Some(obj) = provider_config.as_object_mut() {
        if matches!(obj.get("authHeader"), Some(Value::String(_))) {
            obj.insert("authHeader".to_string(), Value::Bool(true));
        }
    }
    provider_config
}

pub fn read_pi_models_config() -> Result<Value, AppError> {
    let path = get_pi_models_path();
    if !path.exists() {
        return Ok(default_pi_models_config());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(default_pi_models_config());
    }

    serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse Pi Agent models.json: {e}")))
}

fn write_pi_models_config(config: &Value) -> Result<(), AppError> {
    let path = get_pi_models_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|e| AppError::Config(format!("Failed to serialize Pi Agent models.json: {e}")))?;
    atomic_write(&path, &bytes)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_pi_models_config()?;
    Ok(config
        .get("providers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

pub fn get_provider(id: &str) -> Result<Option<Value>, AppError> {
    Ok(get_providers()?.get(id).cloned())
}

pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let trimmed_id = id.trim();
    if trimmed_id.is_empty() {
        return Err(AppError::Config(
            "Pi Agent provider id must not be empty".to_string(),
        ));
    }

    let provider_config = normalize_provider_config_for_write(provider_config);
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_models_config()?;
    ensure_providers(&mut config).insert(trimmed_id.to_string(), provider_config);
    write_pi_models_config(&config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_models_config()?;
    let providers = ensure_providers(&mut config);
    providers.remove(id);

    if config
        .get("defaultProvider")
        .and_then(Value::as_str)
        .is_some_and(|value| value == id)
    {
        let root = ensure_object(&mut config);
        root.insert("defaultProvider".to_string(), Value::Null);
        root.insert("defaultModel".to_string(), Value::Null);
    }

    write_pi_models_config(&config)
}

pub fn apply_switch_defaults(provider_id: &str, provider_config: &Value) -> Result<(), AppError> {
    let trimmed_id = provider_id.trim();
    if trimmed_id.is_empty() {
        return Ok(());
    }

    let provider_config = normalize_provider_config_for_write(provider_config.clone());
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_models_config()?;
    ensure_providers(&mut config).insert(trimmed_id.to_string(), provider_config.clone());

    let root = ensure_object(&mut config);
    root.insert(
        "defaultProvider".to_string(),
        Value::String(trimmed_id.to_string()),
    );
    root.insert(
        "defaultModel".to_string(),
        first_model_id(&provider_config)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );

    write_pi_models_config(&config)
}

pub fn get_default_provider() -> Result<Option<String>, AppError> {
    Ok(read_pi_models_config()?
        .get("defaultProvider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string))
}
