//! OpenCode 配置文件读写模块
//!
//! 处理 `~/.config/opencode/opencode.json` 配置文件的读写操作。
//! OpenCode 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置文件格式
//!
//! ```json
//! {
//!   "$schema": "https://opencode.ai/config.json",
//!   "provider": {
//!     "my-provider": {
//!       "npm": "@ai-sdk/openai-compatible",
//!       "options": { "baseURL": "...", "apiKey": "{env:API_KEY}" },
//!       "models": { "gpt-4o": { "name": "GPT-4o" } }
//!     }
//!   },
//!   "mcp": {
//!     "my-server": { "type": "local", "command": ["..."] }
//!   }
//! }
//! ```

use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 OpenCode 配置目录
///
/// 默认路径: `~/.config/opencode/`
/// 可通过 settings.opencode_config_dir 覆盖
pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: %APPDATA%\opencode
        dirs::data_dir()
            .map(|d| d.join("opencode"))
            .unwrap_or_else(|| PathBuf::from(".config").join("opencode"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix: ~/.config/opencode
        dirs::home_dir()
            .map(|h| h.join(".config").join("opencode"))
            .unwrap_or_else(|| PathBuf::from(".config").join("opencode"))
    }
}

/// 获取 OpenCode 配置文件路径
///
/// 返回 `~/.config/opencode/opencode.json`
pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

/// 获取 OpenCode 环境变量文件路径（如果存在）
///
/// 返回 `~/.config/opencode/.env`
#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 OpenCode 配置文件
///
/// 返回完整的配置 JSON 对象
pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        // Return empty config with schema
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

/// 写入 OpenCode 配置文件（原子写入）
///
/// 使用临时文件 + 重命名确保原子性
pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    // 复用统一的原子写入逻辑（兼容 Windows 上目标文件已存在的情况）
    write_json_file(&path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(())
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置供应商配置（原始 JSON）
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_opencode_config(&config)
}

// ============================================================================
// Provider Functions (Typed - using OpenCodeProviderConfig)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
                // Skip invalid providers but continue
            }
        }
    }

    Ok(result)
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

// ============================================================================
// MCP Functions
// ============================================================================

/// 获取所有 MCP 服务器配置
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置 MCP 服务器配置
pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

/// 删除 MCP 服务器配置
pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}
