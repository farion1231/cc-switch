//! Pi CLI 配置文件读写模块
//!
//! 处理 `~/.pi/agent/models.json` 和 `~/.pi/agent/settings.json` 的读写操作。
//! Pi 使用累加式供应商管理（additive mode），所有 CC Switch 管理的供应商
//! 使用 `cc-switch-` 前缀命名空间写入 models.json，与其他来源的供应商共存。

use crate::config::{atomic_write, get_home_dir};
use crate::error::AppError;
use crate::settings::get_pi_override_dir;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

const CC_SWITCH_PROVIDER_PREFIX: &str = "cc-switch-";

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Pi 配置目录
///
/// 默认路径: `~/.pi/agent/`
/// 可通过 settings.pi_config_dir 覆盖
pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = get_pi_override_dir() {
        return override_dir;
    }

    get_home_dir().join(".pi").join("agent")
}

/// 获取 Pi models.json 配置文件路径
pub fn get_models_json_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

/// 获取 Pi settings.json 配置文件路径
pub fn get_settings_json_path() -> PathBuf {
    get_pi_dir().join("settings.json")
}

/// 获取 Pi AGENTS.md 上下文文件路径
pub fn get_pi_agents_md_path() -> PathBuf {
    get_pi_dir().join("AGENTS.md")
}

/// 获取 Pi SYSTEM.md 系统提示词路径
pub fn get_pi_system_md_path() -> PathBuf {
    get_pi_dir().join("SYSTEM.md")
}

/// 获取 Pi skills 目录路径
pub fn get_pi_skills_dir() -> PathBuf {
    get_pi_dir().join("skills")
}

// ============================================================================
// Type Definitions
// ============================================================================

/// Pi 提供商配置（对应 models.json 中的一个 provider 条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiProviderConfig {
    pub base_url: String,
    pub api: String,
    pub api_key: String,
    #[serde(default = "default_true")]
    pub auth_header: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<PiCompatConfig>,
    pub models: Vec<PiModelConfig>,
}

fn default_true() -> bool {
    true
}

/// Pi API 兼容性配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCompatConfig {
    #[serde(default)]
    pub supports_developer_role: bool,
    #[serde(default)]
    pub supports_reasoning_effort: bool,
}

/// Pi 模型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default = "default_input")]
    pub input: Vec<String>,
    #[serde(rename = "contextWindow")]
    pub context_window: u32,
    pub max_tokens: u32,
    pub cost: PiModelCost,
}

fn default_input() -> Vec<String> {
    vec!["text".to_string()]
}

/// Pi 模型定价（USD / 百万 tokens）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiModelCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Pi 全局设置表示
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PiSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_thinking_block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_startup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<PiCompactionSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<PiRetrySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCompactionSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiRetrySettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

// ============================================================================
// models.json: 读取 & 写入
// ============================================================================

fn models_json_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 读取 models.json 并返回完整 JSON 值
pub fn read_models_json() -> Result<Value, AppError> {
    let path = get_models_json_path();

    if !path.exists() {
        return Ok(json!({
            "providers": {}
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Pi models.json: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 models.json（原子写入，合并策略）
///
/// - 保留所有非 `cc-switch-` 前缀的 provider 条目
/// - 替换/添加 CC Switch 管理的 provider 条目
pub fn write_models_json(cc_switch_providers: &Map<String, Value>) -> Result<(), AppError> {
    let _lock = models_json_lock().lock().unwrap_or_else(|e| e.into_inner());

    let path = get_models_json_path();
    let mut config = read_models_json()?;

    // 确保 providers 对象存在
    if config.get("providers").is_none() || !config["providers"].is_object() {
        config["providers"] = json!({});
    }

    if let Some(providers) = config.get_mut("providers").and_then(|v| v.as_object_mut()) {
        // 移除旧的 CC Switch 管理的 provider
        providers.retain(|key, _| !key.starts_with(CC_SWITCH_PROVIDER_PREFIX));

        // 添加/更新 CC Switch 管理的 provider
        for (key, value) in cc_switch_providers {
            providers.insert(key.clone(), value.clone());
        }
    }

    // 确保父目录存在
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(&path, e))?;
    }

    let content =
        serde_json::to_string_pretty(&config).map_err(|e| AppError::JsonSerialize { source: e })?;

    atomic_write(&path, content.as_bytes())?;
    log::debug!("Pi models.json written to {:?}", path);
    Ok(())
}

/// 获取 CC Switch 管理的 Pi provider 列表
pub fn get_pi_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_models_json()?;

    let all_providers = config
        .get("providers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let cc_providers: Map<String, Value> = all_providers
        .into_iter()
        .filter(|(key, _)| key.starts_with(CC_SWITCH_PROVIDER_PREFIX))
        .collect();

    Ok(cc_providers)
}

/// 设置（添加/更新）单个 Pi provider
pub fn set_pi_provider(id: &str, config: &Value) -> Result<(), AppError> {
    let full_id = if id.starts_with(CC_SWITCH_PROVIDER_PREFIX) {
        id.to_string()
    } else {
        format!("{CC_SWITCH_PROVIDER_PREFIX}{id}")
    };

    let _current_providers = get_pi_providers()?;
    let mut new_providers = Map::new();
    new_providers.insert(full_id, config.clone());
    write_models_json(&new_providers)
}

/// 删除单个 Pi provider
pub fn remove_pi_provider(id: &str) -> Result<(), AppError> {
    let full_id = if id.starts_with(CC_SWITCH_PROVIDER_PREFIX) {
        id.to_string()
    } else {
        format!("{CC_SWITCH_PROVIDER_PREFIX}{id}")
    };

    let mut current_providers = get_pi_providers()?;
    current_providers.remove(&full_id);

    // 重新写入（只保留非 cc-switch- 的条目 + 当前剩余 CC Switch 条目）
    let mut config = read_models_json()?;
    if let Some(providers) = config.get_mut("providers").and_then(|v| v.as_object_mut()) {
        providers.remove(&full_id);
    }

    // 如果删除的是当前激活的 provider，清除 settings.json 中的引用
    if let Ok(settings) = read_settings_json() {
        if let Some(active) = settings.get("defaultProvider").and_then(|v| v.as_str()) {
            if active == full_id {
                unset_active_pi_provider()?;
            }
        }
    }

    let content =
        serde_json::to_string_pretty(&config).map_err(|e| AppError::JsonSerialize { source: e })?;
    let path = get_models_json_path();
    atomic_write(&path, content.as_bytes())?;
    log::debug!("Pi provider '{full_id}' removed from models.json");
    Ok(())
}

// ============================================================================
// settings.json: 读取 & 写入
// ============================================================================

fn settings_json_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// CC Switch 管理的 settings.json 字段名列表
const MANAGED_SETTINGS_FIELDS: &[&str] = &[
    "defaultProvider",
    "defaultModel",
    "defaultThinkingLevel",
    "hideThinkingBlock",
    "theme",
    "quietStartup",
    "compaction",
    "retry",
];

/// 读取 settings.json 并返回完整 JSON 值
pub fn read_settings_json() -> Result<Value, AppError> {
    let path = get_settings_json_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Pi settings.json: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 settings.json（原子写入，托管字段合并策略）
///
/// - 仅修改 CC Switch 管理的字段
/// - 保留所有非托管字段不变
/// - 传入 `None` 的字段从托管字段中移除
pub fn write_settings_json(managed_fields: &Map<String, Value>) -> Result<(), AppError> {
    let _lock = settings_json_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let path = get_settings_json_path();
    let mut current = read_settings_json()?;

    if let Some(obj) = current.as_object_mut() {
        // 仅写入托管字段
        for field in MANAGED_SETTINGS_FIELDS {
            if let Some(value) = managed_fields.get(*field) {
                obj.insert(field.to_string(), value.clone());
            }
        }
    } else {
        // settings.json 不是对象，创建新的
        let mut obj = serde_json::Map::new();
        for field in MANAGED_SETTINGS_FIELDS {
            if let Some(value) = managed_fields.get(*field) {
                obj.insert(field.to_string(), value.clone());
            }
        }
        current = Value::Object(obj);
    }

    // 确保父目录存在
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(&path, e))?;
    }

    let content = serde_json::to_string_pretty(&current)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    atomic_write(&path, content.as_bytes())?;
    log::debug!("Pi settings.json written to {:?}", path);
    Ok(())
}

/// 获取 Pi 完整设置结构（仅托管字段）
pub fn get_pi_settings() -> Result<PiSettings, AppError> {
    let raw = read_settings_json()?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

/// 更新 Pi 设置（部分字段更新）
pub fn update_pi_settings(fields: &Map<String, Value>) -> Result<(), AppError> {
    write_settings_json(fields)
}

// ============================================================================
// 当前提供商切换
// ============================================================================

/// 设置当前激活的 Pi 提供商
///
/// 写入 settings.json: defaultProvider + defaultModel
pub fn set_active_pi_provider(provider_id: &str, model_id: Option<&str>) -> Result<(), AppError> {
    let mut fields = Map::new();

    let full_id = if provider_id.starts_with(CC_SWITCH_PROVIDER_PREFIX) {
        provider_id.to_string()
    } else {
        format!("{CC_SWITCH_PROVIDER_PREFIX}{provider_id}")
    };

    fields.insert(
        "defaultProvider".to_string(),
        Value::String(full_id.clone()),
    );

    if let Some(mid) = model_id {
        if !mid.is_empty() {
            fields.insert("defaultModel".to_string(), Value::String(mid.to_string()));
        }
    }

    write_settings_json(&fields)
}

/// 清除当前激活的 Pi 提供商
pub fn unset_active_pi_provider() -> Result<(), AppError> {
    let mut fields = Map::new();
    fields.insert("defaultProvider".to_string(), Value::Null);
    fields.insert("defaultModel".to_string(), Value::Null);
    write_settings_json(&fields)
}
