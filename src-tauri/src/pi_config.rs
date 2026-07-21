//! Pi Coding Agent 配置文件读写模块
//!
//! 处理 `~/.pi/agent/models.json` 配置文件的读写操作（JSON 格式）。
//! Pi 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置结构示例
//!
//! ```json
//! {
//!   "providers": {
//!     "anthropic": {
//!       "baseUrl": "https://api.anthropic.com",
//!       "api": "anthropic-messages",
//!       "apiKey": "$ANTHROPIC_API_KEY",
//!       "models": [
//!         { "id": "claude-opus-4-8", "name": "Claude Opus 4.8" }
//!       ]
//!     },
//!     "openai": {
//!       "baseUrl": "https://api.openai.com/v1",
//!       "api": "openai-completions",
//!       "apiKey": "sk-...",
//!       "models": [
//!         { "id": "gpt-4o", "name": "GPT-4o" }
//!       ]
//!     }
//!   }
//! }
//! ```

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::get_pi_override_dir;
use chrono::Local;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// serde_json compatibility helpers
// ============================================================================

/// Get or insert a key in a JSON value's object map, initializing with an empty object.
fn json_value_entry<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut()
        .expect("just ensured object")
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
}

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Pi 配置目录
///
/// 解析顺序:
///   1. CCS 设置 `pi_config_dir`(显式覆盖)
///   2. `PI_HOME` 环境变量(trim 后非空)
///   3. 平台默认 `~/.pi`
pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = get_pi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("PI_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    crate::config::get_home_dir().join(".pi")
}

/// 获取 Pi 配置文件路径
///
/// 返回 `~/.pi/agent/models.json`
pub fn get_pi_config_path() -> PathBuf {
    get_pi_dir().join("agent").join("models.json")
}

/// 获取 Pi settings 文件路径
///
/// 返回 `~/.pi/agent/settings.json`
pub fn get_pi_settings_path() -> PathBuf {
    get_pi_dir().join("agent").join("settings.json")
}

fn pi_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// Pi 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PiWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

/// Pi 供应商配置（对应 providers 中的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<PiModelEntry>,
    /// Preserve unknown fields for forward compatibility (e.g. `headers`,
    /// `authHeader`, `oauth`, `modelOverrides`, `compat`).
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl Default for PiProviderConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            api_key: None,
            api: None,
            models: Vec::new(),
            extra: HashMap::new(),
        }
    }
}

/// Pi model 配置条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    /// Preserve unknown fields for forward compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ============================================================================
// Backup & Cleanup
// ============================================================================

fn create_pi_backup(source: &str) -> Result<PathBuf, AppError> {
    use crate::settings::effective_backup_retain_count;

    let backup_dir = get_app_config_dir().join("backups").join("pi");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("pi_models_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.json");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.json");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_pi_backups(&backup_dir, effective_backup_retain_count())?;
    Ok(backup_path)
}

fn cleanup_pi_backups(dir: &Path, retain: usize) -> Result<(), AppError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old Pi config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// Core Read/Write
// ============================================================================

/// 读取整个 `models.json` 为 `serde_json::Value`。
///
/// 如果文件不存在，返回包含空 `providers` 映射的默认结构。
pub fn read_pi_config() -> Result<Value, AppError> {
    let path = get_pi_config_path();
    if !path.exists() {
        return Ok(default_pi_config_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(default_pi_config_value());
    }

    let mut value: Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse Pi config as JSON: {e}")))?;

    // Heal historical/legacy structures — older Pi versions or manually-edited
    // configs may omit the `providers` key, or carry an array instead of a
    // dict. Normalize to a `providers: {}` object so downstream code can
    // uniformly treat the result.
    normalize_providers_field(&mut value);

    Ok(value)
}

/// Ensure the top-level `providers` field is a JSON object. Insert default
/// if missing; convert arrays (legacy) into a dict by taking any non-empty
/// `name` / `id` field as the key. Non-object types (e.g. strings) are
/// replaced with an empty dict.
fn normalize_providers_field(value: &mut Value) {
    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => {
            *value = default_pi_config_value();
            return;
        }
    };

    // Normalize "providers": ensure it's a dict (not array, not missing).
    match obj.get_mut("providers") {
        None => {
            obj.insert("providers".to_string(), Value::Object(Map::new()));
        }
        Some(Value::Object(_)) => {
            // Already an object — nothing to do.
        }
        Some(Value::Array(arr)) => {
            let mut map = Map::new();
            for entry in arr.drain(..) {
                if let Some(key) = entry
                    .as_object()
                    .and_then(|o| o.get("name").or_else(|| o.get("id")))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    map.insert(key.to_string(), entry);
                }
            }
            *obj.get_mut("providers").unwrap() = Value::Object(map);
        }
        Some(_) => {
            *obj.get_mut("providers").unwrap() = Value::Object(Map::new());
        }
    }
}

fn default_pi_config_value() -> Value {
    serde_json::json!({ "providers": {} })
}

fn default_pi_settings_value() -> Value {
    serde_json::json!({})
}

/// 读取 `settings.json` 为 `serde_json::Value`。
pub fn read_pi_settings() -> Result<Value, AppError> {
    let path = get_pi_settings_path();
    if !path.exists() {
        return Ok(default_pi_settings_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(default_pi_settings_value());
    }

    serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse Pi settings as JSON: {e}")))
}

/// 内部写入:把完整的 `settings.json` 落盘(调用方需持锁)。
fn write_pi_settings_locked(value: &Value) -> Result<PiWriteOutcome, AppError> {
    let path = get_pi_settings_path();
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    let serialized = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    let serialized = format!("{serialized}\n");

    if serialized == raw {
        return Ok(PiWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_pi_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&path, serialized.as_bytes())?;

    log::debug!("Pi settings written to {:?}", path);
    Ok(PiWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// 内部写入:把完整的 `models.json` 落盘(调用方需持锁)。
fn write_pi_config_locked(value: &Value) -> Result<PiWriteOutcome, AppError> {
    let path = get_pi_config_path();
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    // 重新归一化一遍,避免写入意料外的形状(防御性)。
    let mut to_write = value.clone();
    normalize_providers_field(&mut to_write);

    let serialized = serde_json::to_string_pretty(&to_write)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    let serialized = format!("{serialized}\n");

    if serialized == raw {
        return Ok(PiWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_pi_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&path, serialized.as_bytes())?;

    log::debug!("Pi config written to {:?}", path);
    Ok(PiWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

// ============================================================================
// Provider Functions
// ============================================================================

/// 获取所有供应商配置,返回 name -> provider config 的 map。
///
/// 使用 `IndexMap` 保证声明顺序(与磁盘文件一致)。
pub fn get_providers() -> Result<IndexMap<String, PiProviderConfig>, AppError> {
    let config = read_pi_config()?;
    let mut out: IndexMap<String, PiProviderConfig> = IndexMap::new();

    if let Some(map) = config.get("providers").and_then(|v| v.as_object()) {
        for (name, raw) in map {
            // 容错:任何非对象条目都跳过,而不是让整次读取失败。
            match serde_json::from_value::<PiProviderConfig>(raw.clone()) {
                Ok(provider) => {
                    out.insert(name.clone(), provider);
                }
                Err(e) => {
                    log::warn!("Failed to parse Pi provider '{name}': {e}");
                }
            }
        }
    }

    Ok(out)
}

/// 获取单个供应商配置。
pub fn get_provider(name: &str) -> Result<Option<PiProviderConfig>, AppError> {
    Ok(get_providers()?.shift_remove(name))
}

/// Upsert 一个供应商配置。
pub fn set_provider(
    name: &str,
    provider_config: PiProviderConfig,
) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_config()?;
    let providers_value = json_value_entry(&mut config, "providers");

    if !providers_value.is_object() {
        *providers_value = Value::Object(Map::new());
    }

    let providers_map = providers_value
        .as_object_mut()
        .expect("just ensured object");

    let serialized = serde_json::to_value(&provider_config)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    // Forward-compat merge: 保留 Pi 端可能在磁盘上新增的字段(如未来
    // `headers`、`oauth` 字段),CC Switch 没建模的字段不会被 set 覆盖丢失。
    if let Some(existing) = providers_map.get(name) {
        if let (Some(existing_obj), Some(new_obj)) =
            (existing.as_object(), serialized.as_object())
        {
            let mut merged = existing_obj.clone();
            for (k, v) in new_obj {
                merged.insert(k.clone(), v.clone());
            }
            providers_map.insert(name.to_string(), Value::Object(merged));
        } else {
            providers_map.insert(name.to_string(), serialized);
        }
    } else {
        providers_map.insert(name.to_string(), serialized);
    }

    write_pi_config_locked(&config)
}

/// Upsert 一个供应商配置（保留原始 JSON，用于结构不完全匹配时的兜底写入）。
pub fn set_provider_raw(name: &str, config: Value) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut root = read_pi_config()?;
    let providers_value = json_value_entry(&mut root, "providers");

    if !providers_value.is_object() {
        *providers_value = Value::Object(Map::new());
    }

    let providers_map = providers_value
        .as_object_mut()
        .expect("just ensured object");

    if let Some(existing) = providers_map.get(name) {
        if let (Some(existing_obj), Some(new_obj)) = (existing.as_object(), config.as_object()) {
            let mut merged = existing_obj.clone();
            for (k, v) in new_obj {
                merged.insert(k.clone(), v.clone());
            }
            providers_map.insert(name.to_string(), Value::Object(merged));
        } else {
            providers_map.insert(name.to_string(), config);
        }
    } else {
        providers_map.insert(name.to_string(), config);
    }

    write_pi_config_locked(&root)
}

/// 删除一个供应商配置。
pub fn remove_provider(name: &str) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut config = read_pi_config()?;
    let removed = if let Some(providers) = config
        .get_mut("providers")
        .and_then(|v| v.as_object_mut())
    {
        providers.remove(name).is_some()
    } else {
        false
    };

    if !removed {
        return Ok(PiWriteOutcome::default());
    }

    write_pi_config_locked(&config)
}

/// 持久化一个新的"当前激活"供应商选择到磁盘（写入 settings.json 的 `defaultProvider`）。
pub fn set_active_provider(name: &str) -> Result<PiWriteOutcome, AppError> {
    let _guard = pi_write_lock().lock()?;
    let mut settings = read_pi_settings()?;
    settings["defaultProvider"] = Value::String(name.to_string());
    write_pi_settings_locked(&settings)
}

/// 读取当前激活的供应商（从 settings.json 的 `defaultProvider`）。
pub fn get_active_provider() -> Result<Option<String>, AppError> {
    let settings = read_pi_settings()?;
    Ok(settings
        .get("defaultProvider")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

// ============================================================================
// 健康检查
// ============================================================================

/// Pi 配置文件健康警告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PiHealthWarning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 扫描 `models.json` 中的已知配置隐患,例如重复的 provider 名、
/// 缺失的 `baseUrl` / `api` 字段等。
pub fn scan_pi_config_health() -> Result<Vec<PiHealthWarning>, AppError> {
    let config = read_pi_config()?;
    let mut warnings = Vec::new();

    let Some(providers) = config.get("providers").and_then(|v| v.as_object()) else {
        return Ok(warnings);
    };

    let mut seen_names: HashMap<String, ()> = HashMap::new();

    for (name, value) in providers {
        // 重复的 provider 名(在 JSON 里这是不合法的,但 serde_json 可能会保留
        // 最后一个;我们在 PI 端做一次显式扫描以便给用户清晰提示)。
        if seen_names.contains_key(name) {
            warnings.push(PiHealthWarning {
                code: "duplicate_provider".to_string(),
                message: format!("Provider '{name}' is duplicated in models.json"),
                path: Some(get_pi_config_path().display().to_string()),
            });
        }
        seen_names.insert(name.clone(), ());

        if let Some(obj) = value.as_object() {
            if !obj.contains_key("baseUrl") {
                warnings.push(PiHealthWarning {
                    code: "missing_base_url".to_string(),
                    message: format!("Provider '{name}' is missing 'baseUrl'"),
                    path: Some(get_pi_config_path().display().to_string()),
                });
            }
            if !obj.contains_key("api") {
                warnings.push(PiHealthWarning {
                    code: "missing_api".to_string(),
                    message: format!(
                        "Provider '{name}' is missing 'api' (openai-completions / openai-responses / anthropic-messages / google-generative-ai)"
                    ),
                    path: Some(get_pi_config_path().display().to_string()),
                });
            }
        }
    }

    Ok(warnings)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_home<T>(test_fn: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        let old_pi_home = std::env::var_os("PI_HOME");
        std::env::remove_var("PI_HOME");
        let result = test_fn();
        match old_pi_home {
            Some(value) => std::env::set_var("PI_HOME", value),
            None => std::env::remove_var("PI_HOME"),
        }
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        result
    }

    #[test]
    fn normalize_inserts_default_providers() {
        let mut v = serde_json::json!({});
        normalize_providers_field(&mut v);
        assert!(v.get("providers").unwrap().is_object());
        assert_eq!(
            v.get("providers").unwrap().as_object().unwrap().len(),
            0
        );
    }

    #[test]
    fn normalize_converts_array_to_dict() {
        let mut v = serde_json::json!({
            "providers": [
                { "name": "openai", "baseUrl": "https://api.openai.com/v1" },
                { "id": "anthropic", "baseUrl": "https://api.anthropic.com" }
            ]
        });
        normalize_providers_field(&mut v);
        let providers = v.get("providers").unwrap().as_object().unwrap();
        assert!(providers.contains_key("openai"));
        assert!(providers.contains_key("anthropic"));
    }

    #[test]
    fn normalize_replaces_invalid_type() {
        let mut v = serde_json::json!({ "providers": "not-an-object" });
        normalize_providers_field(&mut v);
        assert!(v.get("providers").unwrap().is_object());
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        with_test_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://example.com/v1".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-test".to_string()),
                models: vec![PiModelEntry {
                    id: "model-1".to_string(),
                    name: Some("Model 1".to_string()),
                    reasoning: true,
                    context_window: Some(128000),
                    max_tokens: None,
                    input: vec!["text".to_string()],
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            };
            set_provider("test-provider", provider).unwrap();

            // Now write an unknown field directly to the file and re-read.
            let path = get_pi_config_path();
            let raw = fs::read_to_string(&path).unwrap();
            let mut value: Value = serde_json::from_str(&raw).unwrap();
            value["providers"]["test-provider"]["headers"] =
                serde_json::json!({ "x-custom": "value" });
            fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

            // set_provider again should not clobber the unknown `headers`.
            let provider2 = PiProviderConfig {
                base_url: Some("https://example.com/v2".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: None,
                models: vec![],
                extra: HashMap::new(),
            };
            set_provider("test-provider", provider2).unwrap();

            let raw = fs::read_to_string(&path).unwrap();
            let value: Value = serde_json::from_str(&raw).unwrap();
            let entry = &value["providers"]["test-provider"];
            assert_eq!(entry["baseUrl"], "https://example.com/v2");
            assert_eq!(entry["headers"]["x-custom"], "value");
        });
    }

    #[test]
    fn scan_detects_missing_fields() {
        with_test_home(|| {
            let raw = r#"{
                "providers": {
                    "good": {
                        "baseUrl": "https://api.example.com",
                        "api": "openai-completions"
                    },
                    "bad": { "models": [] }
                }
            }"#;
            let path = get_pi_config_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, raw).unwrap();

            let warnings = scan_pi_config_health().unwrap();
            assert!(warnings
                .iter()
                .any(|w| w.code == "missing_base_url" && w.message.contains("bad")));
            assert!(warnings
                .iter()
                .any(|w| w.code == "missing_api" && w.message.contains("bad")));
            assert!(!warnings.iter().any(|w| w.message.contains("good")));
        });
    }

    #[test]
    fn remove_provider_drops_entry() {
        with_test_home(|| {
            let provider = PiProviderConfig {
                base_url: Some("https://example.com".to_string()),
                api: Some("openai-completions".to_string()),
                api_key: Some("sk-x".to_string()),
                models: vec![],
                extra: HashMap::new(),
            };
            set_provider("to-remove", provider).unwrap();
            assert!(get_provider("to-remove").unwrap().is_some());

            remove_provider("to-remove").unwrap();
            assert!(get_provider("to-remove").unwrap().is_none());
        });
    }

    #[test]
    fn active_provider_round_trips_via_settings_json() {
        with_test_home(|| {
            set_active_provider("anthropic").unwrap();
            assert_eq!(get_active_provider().unwrap().as_deref(), Some("anthropic"));

            let settings_path = get_pi_settings_path();
            assert!(settings_path.exists(), "settings.json should be created");
            let raw = fs::read_to_string(&settings_path).unwrap();
            let value: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(value["defaultProvider"], "anthropic");

            let models_path = get_pi_config_path();
            if models_path.exists() {
                let models_raw = fs::read_to_string(&models_path).unwrap();
                let models_value: Value = serde_json::from_str(&models_raw).unwrap();
                assert!(
                    models_value.get("defaultProvider").is_none(),
                    "defaultProvider must not be written to models.json"
                );
            }
        });
    }
}
