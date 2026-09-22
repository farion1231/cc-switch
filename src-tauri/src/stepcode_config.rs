//! StepCode 配置文件读写模块
//!
//! 处理 `~/.stepcode/models.json` 配置文件的读写操作（JSON 格式）。
//! StepCode 使用累加式供应商管理，所有供应商配置共存于 `providers` 节点中，
//! 每个供应商的结构为 `{ baseUrl, api, apiKey, models: [...] }`（camelCase）。
//!
//! 说明：StepCode 另有 `~/.stepcode/config.toml`（存放 defaultProvider /
//! defaultModel 等全局项），但 CC Switch 仅管理 `models.json` 中的供应商，
//! 与 OpenClaw 的处理方式一致，不触碰用户的全局默认设置。

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::effective_backup_retain_count;
use chrono::Local;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 StepCode 配置目录
///
/// 默认路径: `~/.stepcode/`
pub fn get_stepcode_dir() -> PathBuf {
    crate::config::get_home_dir().join(".stepcode")
}

/// 获取 StepCode 供应商配置文件路径
///
/// 返回 `~/.stepcode/models.json`
pub fn get_stepcode_config_path() -> PathBuf {
    get_stepcode_dir().join("models.json")
}

fn default_stepcode_config_value() -> Value {
    json!({ "providers": {} })
}

fn stepcode_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// StepCode 供应商配置（对应 models.json 中 `providers` 的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCodeProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<StepCodeModelEntry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// StepCode 模型条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCodeModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<StepCodeModelCost>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// StepCode 模型成本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepCodeModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 StepCode 供应商配置文件
///
/// 文件不存在时返回默认结构 `{ "providers": {} }`。
pub fn read_stepcode_config() -> Result<Value, AppError> {
    let path = get_stepcode_config_path();
    if !path.exists() {
        return Ok(default_stepcode_config_value());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(default_stepcode_config_value());
    }

    serde_json::from_str::<Value>(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse StepCode models.json as JSON: {e}")))
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value should be object after normalization")
}

/// 将完整配置写回磁盘（保留锁、备份、原子写）。
///
/// 调用方需自行持有 [`stepcode_write_lock`]。
fn write_stepcode_config(config: &Value) -> Result<(), AppError> {
    let path = get_stepcode_config_path();
    let next_source =
        serde_json::to_string_pretty(config).map_err(|e| AppError::JsonSerialize { source: e })?;

    let current_source = if path.exists() {
        Some(fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?)
    } else {
        None
    };

    // 内容未变化时跳过写盘与备份，避免无谓的备份堆积。
    if current_source.as_deref() == Some(next_source.as_str()) {
        return Ok(());
    }

    if let Some(source) = current_source.as_ref() {
        create_stepcode_backup(source)?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(&path, next_source.as_bytes())?;
    log::debug!("StepCode config written to {path:?}");
    Ok(())
}

fn create_stepcode_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("stepcode");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("stepcode_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.json");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.json");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_stepcode_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_stepcode_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
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
                "Failed to remove old StepCode config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
///
/// 从 `providers` 节点读取
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_stepcode_config()?;
    Ok(config
        .get("providers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// 获取单个供应商配置（原始 JSON）
pub fn get_provider(id: &str) -> Result<Option<Value>, AppError> {
    Ok(get_providers()?.get(id).cloned())
}

/// 设置供应商配置（原始 JSON）
///
/// 写入到 `providers` 节点
pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let _guard = stepcode_write_lock().lock()?;
    let mut config = read_stepcode_config()?;
    let root = ensure_object(&mut config);
    let providers = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(providers).insert(id.to_string(), provider_config);
    write_stepcode_config(&config)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let _guard = stepcode_write_lock().lock()?;
    let mut config = read_stepcode_config()?;

    let removed = config
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .map(|providers| providers.remove(id).is_some())
        .unwrap_or(false);

    if !removed {
        return Ok(());
    }

    write_stepcode_config(&config)
}

// ============================================================================
// Provider Functions (Typed)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, StepCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<StepCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse StepCode provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(id: &str, config: &StepCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::OnceLock;

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_paths<T>(test: impl FnOnce() -> T) -> T {
        let _guard = test_guard();
        let temp = tempfile::tempdir().unwrap();
        let stepcode_dir = temp.path().join(".stepcode");
        fs::create_dir_all(&stepcode_dir).unwrap();
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("HOME", temp.path());
        let result = test();
        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        result
    }

    #[test]
    #[serial]
    fn missing_config_returns_empty_providers() {
        with_test_paths(|| {
            let providers = get_providers().unwrap();
            assert!(providers.is_empty());
        });
    }

    #[test]
    #[serial]
    fn set_and_get_typed_provider_roundtrips() {
        with_test_paths(|| {
            let config = StepCodeProviderConfig {
                base_url: Some("https://api.example.com/v1".to_string()),
                api_key: Some("sk-test".to_string()),
                api: Some("openai-responses".to_string()),
                models: vec![StepCodeModelEntry {
                    id: "step-5-preview".to_string(),
                    name: Some("Step 5 Preview".to_string()),
                    reasoning: Some(true),
                    context_window: Some(1_000_000),
                    max_tokens: Some(65_536),
                    cost: None,
                    extra: HashMap::new(),
                }],
                headers: HashMap::new(),
                extra: HashMap::new(),
            };

            set_typed_provider("acme", &config).unwrap();

            let providers = get_typed_providers().unwrap();
            let stored = providers.get("acme").expect("provider should be stored");
            assert_eq!(
                stored.base_url.as_deref(),
                Some("https://api.example.com/v1")
            );
            assert_eq!(stored.api.as_deref(), Some("openai-responses"));
            assert_eq!(stored.models.len(), 1);
            assert_eq!(stored.models[0].id, "step-5-preview");
            assert_eq!(stored.models[0].context_window, Some(1_000_000));

            // camelCase keys must be written to disk (not snake_case).
            let written = fs::read_to_string(get_stepcode_config_path()).unwrap();
            assert!(written.contains("\"baseUrl\""));
            assert!(written.contains("\"apiKey\""));
            assert!(written.contains("\"contextWindow\""));
        });
    }

    #[test]
    #[serial]
    fn set_provider_preserves_other_providers_and_unknown_fields() {
        with_test_paths(|| {
            // Seed a config with an unrelated provider carrying extra fields.
            let seed = json!({
                "providers": {
                    "keep-me": {
                        "baseUrl": "https://keep.example.com",
                        "models": [{ "id": "m1" }],
                        "customVendorField": { "nested": true }
                    }
                }
            });
            fs::write(
                get_stepcode_config_path(),
                serde_json::to_string_pretty(&seed).unwrap(),
            )
            .unwrap();

            set_provider("new-one", json!({ "baseUrl": "https://new.example.com" })).unwrap();

            let providers = get_providers().unwrap();
            assert!(providers.contains_key("keep-me"));
            assert!(providers.contains_key("new-one"));
            // Unknown fields on the untouched provider survive the rewrite.
            assert_eq!(
                providers["keep-me"]["customVendorField"]["nested"],
                Value::Bool(true)
            );
        });
    }

    #[test]
    #[serial]
    fn remove_provider_drops_only_target_and_keeps_others() {
        with_test_paths(|| {
            set_provider("a", json!({ "baseUrl": "https://a.example.com" })).unwrap();
            set_provider("b", json!({ "baseUrl": "https://b.example.com" })).unwrap();

            remove_provider("a").unwrap();

            let providers = get_providers().unwrap();
            assert!(!providers.contains_key("a"));
            assert!(providers.contains_key("b"));

            // Removing a missing provider is a no-op (no error, no rewrite).
            remove_provider("does-not-exist").unwrap();
        });
    }

    fn backup_count(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .map(|entries| entries.count())
            .unwrap_or(0)
    }

    #[test]
    #[serial]
    fn noop_write_skips_backup() {
        with_test_paths(|| {
            let config = StepCodeProviderConfig {
                base_url: Some("https://api.example.com".to_string()),
                api_key: None,
                api: None,
                models: Vec::new(),
                headers: HashMap::new(),
                extra: HashMap::new(),
            };
            let backup_dir = get_app_config_dir().join("backups").join("stepcode");

            // First write creates the file from scratch: nothing to back up.
            set_typed_provider("acme", &config).unwrap();
            assert_eq!(
                backup_count(&backup_dir),
                0,
                "fresh creation should not back up"
            );

            // Writing the identical config again is a no-op: still no backup.
            set_typed_provider("acme", &config).unwrap();
            assert_eq!(
                backup_count(&backup_dir),
                0,
                "no-op write must not create a backup"
            );

            // A real change backs up the previous content exactly once.
            let mut changed = config.clone();
            changed.base_url = Some("https://api.changed.com".to_string());
            set_typed_provider("acme", &changed).unwrap();
            assert_eq!(
                backup_count(&backup_dir),
                1,
                "changed write should back up once"
            );
        });
    }
}
