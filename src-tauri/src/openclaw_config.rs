//! OpenClaw 配置文件读写模块
//!
//! 处理 `~/.openclaw/openclaw.json` 配置文件的读写操作（JSON5 格式）。
//! OpenClaw 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置文件格式
//!
//! ```json5
//! {
//!   // 模型供应商配置（映射为 CC Switch 的"供应商"）
//!   models: {
//!     mode: "merge",
//!     providers: {
//!       "custom-provider": {
//!         baseUrl: "https://api.example.com/v1",
//!         apiKey: "${API_KEY}",
//!         api: "openai-completions",
//!         models: [{ id: "model-id", name: "Model Name" }]
//!       }
//!     }
//!   },
//!   // 环境变量配置
//!   env: {
//!     ANTHROPIC_API_KEY: "sk-...",
//!     vars: { ... }
//!   },
//!   // Agent 默认模型配置
//!   agents: {
//!     defaults: {
//!       model: {
//!         primary: "provider/model",
//!         fallbacks: ["provider2/model2"]
//!       }
//!     }
//!   }
//! }
//! ```

use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_openclaw_override_dir;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 OpenClaw 配置目录
///
/// 默认路径: `~/.openclaw/`
/// 可通过 settings.openclaw_config_dir 覆盖
pub fn get_openclaw_dir() -> PathBuf {
    if let Some(override_dir) = get_openclaw_override_dir() {
        return override_dir;
    }

    // 所有平台统一使用 ~/.openclaw
    dirs::home_dir()
        .map(|h| h.join(".openclaw"))
        .unwrap_or_else(|| PathBuf::from(".openclaw"))
}

/// 获取 OpenClaw 配置文件路径
///
/// 返回 `~/.openclaw/openclaw.json`
pub fn get_openclaw_config_path() -> PathBuf {
    get_openclaw_dir().join("openclaw.json")
}

// ============================================================================
// Type Definitions
// ============================================================================

/// OpenClaw 供应商配置（对应 models.providers 中的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawProviderConfig {
    /// API 基础 URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API Key（支持环境变量引用 ${VAR_NAME}）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// API 类型（如 "openai-completions", "anthropic" 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    /// 支持的模型列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<OpenClawModelEntry>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawModelEntry {
    /// 模型 ID
    pub id: String,

    /// 模型显示名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 模型别名（用于快捷引用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,

    /// 模型成本（输入/输出价格）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<OpenClawModelCost>,

    /// 上下文窗口大小
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型成本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCost {
    /// 输入价格（每百万 token）
    pub input: f64,

    /// 输出价格（每百万 token）
    pub output: f64,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 默认模型配置（agents.defaults.model）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawDefaultModel {
    /// 主模型 ID（格式：provider/model）
    pub primary: String,

    /// 回退模型列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw 模型目录条目（agents.defaults.models 中的值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelCatalogEntry {
    /// 模型别名（用于 UI 显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw agents.defaults 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawAgentsDefaults {
    /// 默认模型配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenClawDefaultModel>,

    /// 模型目录/允许列表（键为 provider/model 格式）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<HashMap<String, OpenClawModelCatalogEntry>>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenClaw agents 顶层配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct OpenClawAgents {
    /// 默认配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<OpenClawAgentsDefaults>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 OpenClaw 配置文件
///
/// 支持 JSON5 格式，返回完整的配置 JSON 对象
pub fn read_openclaw_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();

    if !path.exists() {
        // Return empty config structure
        return Ok(json!({
            "models": {
                "mode": "merge",
                "providers": {}
            }
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    // 尝试 JSON5 解析（支持注释和尾随逗号）
    json5::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse OpenClaw config as JSON5: {}", e)))
}

/// 写入 OpenClaw 配置文件（原子写入）
///
/// 使用标准 JSON 格式写入（JSON5 是 JSON 的超集）
pub fn write_openclaw_config(config: &Value) -> Result<(), AppError> {
    let path = get_openclaw_config_path();

    // 确保目录存在
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 复用统一的原子写入逻辑
    write_json_file(&path, config)?;

    log::debug!("OpenClaw config written to {path:?}");
    Ok(())
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
///
/// 从 `models.providers` 读取
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_openclaw_config()?;
    Ok(config
        .get("models")
        .and_then(|m| m.get("providers"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置供应商配置（原始 JSON）
///
/// 写入到 `models.providers`
pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let mut full_config = read_openclaw_config()?;

    // 确保 models 结构存在
    if full_config.get("models").is_none() {
        full_config["models"] = json!({
            "mode": "merge",
            "providers": {}
        });
    }

    // 确保 providers 对象存在
    if full_config["models"].get("providers").is_none() {
        full_config["models"]["providers"] = json!({});
    }

    // 设置供应商
    if let Some(providers) = full_config["models"]
        .get_mut("providers")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), provider_config);
    }

    write_openclaw_config(&full_config)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    if let Some(providers) = config
        .get_mut("models")
        .and_then(|m| m.get_mut("providers"))
        .and_then(|v| v.as_object_mut())
    {
        providers.remove(id);
    }

    write_openclaw_config(&config)
}

// ============================================================================
// Provider Functions (Typed)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, OpenClawProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenClawProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse OpenClaw provider '{id}': {e}");
                // Skip invalid providers but continue
            }
        }
    }

    Ok(result)
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(id: &str, config: &OpenClawProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

// ============================================================================
// Agents Configuration Functions
// ============================================================================

/// 读取默认模型配置（agents.defaults.model）
pub fn get_default_model() -> Result<Option<OpenClawDefaultModel>, AppError> {
    let config = read_openclaw_config()?;

    let Some(model_value) = config
        .get("agents")
        .and_then(|a| a.get("defaults"))
        .and_then(|d| d.get("model"))
    else {
        return Ok(None);
    };

    let model = serde_json::from_value(model_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.model: {e}")))?;

    Ok(Some(model))
}

/// 设置默认模型配置（agents.defaults.model）
pub fn set_default_model(model: &OpenClawDefaultModel) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    // Ensure agents.defaults path exists, preserving unknown fields
    ensure_agents_defaults_path(&mut config);

    let model_value =
        serde_json::to_value(model).map_err(|e| AppError::JsonSerialize { source: e })?;

    config["agents"]["defaults"]["model"] = model_value;

    write_openclaw_config(&config)
}

/// 读取模型目录/允许列表（agents.defaults.models）
pub fn get_model_catalog() -> Result<Option<HashMap<String, OpenClawModelCatalogEntry>>, AppError> {
    let config = read_openclaw_config()?;

    let Some(models_value) = config
        .get("agents")
        .and_then(|a| a.get("defaults"))
        .and_then(|d| d.get("models"))
    else {
        return Ok(None);
    };

    let catalog = serde_json::from_value(models_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults.models: {e}")))?;

    Ok(Some(catalog))
}

/// 设置模型目录/允许列表（agents.defaults.models）
pub fn set_model_catalog(
    catalog: &HashMap<String, OpenClawModelCatalogEntry>,
) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    // Ensure agents.defaults path exists, preserving unknown fields
    ensure_agents_defaults_path(&mut config);

    let catalog_value =
        serde_json::to_value(catalog).map_err(|e| AppError::JsonSerialize { source: e })?;

    config["agents"]["defaults"]["models"] = catalog_value;

    write_openclaw_config(&config)
}

/// Ensure the `agents.defaults` path exists in the config,
/// preserving any existing unknown fields.
fn ensure_agents_defaults_path(config: &mut Value) {
    if config.get("agents").is_none() {
        config["agents"] = json!({});
    }
    if config["agents"].get("defaults").is_none() {
        config["agents"]["defaults"] = json!({});
    }
}

// ============================================================================
// Full Agents Defaults Functions
// ============================================================================

/// Read the full agents.defaults config
pub fn get_agents_defaults() -> Result<Option<OpenClawAgentsDefaults>, AppError> {
    let config = read_openclaw_config()?;

    let Some(defaults_value) = config.get("agents").and_then(|a| a.get("defaults")) else {
        return Ok(None);
    };

    let defaults = serde_json::from_value(defaults_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse agents.defaults: {e}")))?;

    Ok(Some(defaults))
}

/// Write the full agents.defaults config
pub fn set_agents_defaults(defaults: &OpenClawAgentsDefaults) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    if config.get("agents").is_none() {
        config["agents"] = json!({});
    }

    let value =
        serde_json::to_value(defaults).map_err(|e| AppError::JsonSerialize { source: e })?;

    config["agents"]["defaults"] = value;

    write_openclaw_config(&config)
}

// ============================================================================
// Env Configuration
// ============================================================================

/// OpenClaw env configuration (env section of openclaw.json)
///
/// Stores environment variables like API keys and custom vars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawEnvConfig {
    /// All environment variable key-value pairs
    #[serde(flatten)]
    pub vars: HashMap<String, Value>,
}

/// Read the env config section
pub fn get_env_config() -> Result<OpenClawEnvConfig, AppError> {
    let config = read_openclaw_config()?;

    let Some(env_value) = config.get("env") else {
        return Ok(OpenClawEnvConfig {
            vars: HashMap::new(),
        });
    };

    serde_json::from_value(env_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse env config: {e}")))
}

/// Write the env config section
pub fn set_env_config(env: &OpenClawEnvConfig) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    let value = serde_json::to_value(env).map_err(|e| AppError::JsonSerialize { source: e })?;

    config["env"] = value;

    write_openclaw_config(&config)
}

// ============================================================================
// Tools Configuration
// ============================================================================

/// OpenClaw tools configuration (tools section of openclaw.json)
///
/// Controls tool permissions with profile-based allow/deny lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawToolsConfig {
    /// Active permission profile (e.g. "default", "strict", "permissive")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// Allowed tool patterns
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,

    /// Denied tool patterns
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,

    /// Other custom fields (preserve unknown fields)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Read the tools config section
pub fn get_tools_config() -> Result<OpenClawToolsConfig, AppError> {
    let config = read_openclaw_config()?;

    let Some(tools_value) = config.get("tools") else {
        return Ok(OpenClawToolsConfig {
            profile: None,
            allow: Vec::new(),
            deny: Vec::new(),
            extra: HashMap::new(),
        });
    };

    serde_json::from_value(tools_value.clone())
        .map_err(|e| AppError::Config(format!("Failed to parse tools config: {e}")))
}

/// Write the tools config section
pub fn set_tools_config(tools: &OpenClawToolsConfig) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    let value = serde_json::to_value(tools).map_err(|e| AppError::JsonSerialize { source: e })?;

    config["tools"] = value;

    write_openclaw_config(&config)
}
