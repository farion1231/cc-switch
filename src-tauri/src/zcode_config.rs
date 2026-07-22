//! ZCode 配置文件读写
//!
//! ZCode (dev.zcode.app) 的配置分散在两个文件：
//!
//! - Provider 配置：`~/.zcode/v2/config.json`，顶层有 `provider` 字段
//! - MCP 配置：`~/.zcode/cli/config.json`，顶层有 `mcp.servers` 字段
//!
//! 两者都是 additive 模式（增删 provider 条目而非切换整个文件）。
//! ZCode 的 MCP 使用标准 `type: "stdio"` 格式，与 Claude Code 一致，
//! 因此 MCP 配置直接透传，不做任何格式转换。

use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::ZCodeProviderConfig;
use crate::settings::get_zcode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// 获取 ZCode 配置根目录（`~/.zcode`），支持 settings 覆盖
pub fn get_zcode_dir() -> PathBuf {
    if let Some(override_dir) = get_zcode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".zcode")
}

/// Provider 配置路径：`~/.zcode/v2/config.json`
pub fn get_zcode_config_path() -> PathBuf {
    get_zcode_dir().join("v2").join("config.json")
}

/// MCP 配置路径：`~/.zcode/cli/config.json`（与 provider 是不同文件！）
pub fn get_zcode_cli_config_path() -> PathBuf {
    get_zcode_dir().join("cli").join("config.json")
}

/// Usage 数据库路径：`~/.zcode/cli/db/db.sqlite`
pub fn get_zcode_usage_db_path() -> PathBuf {
    get_zcode_dir().join("cli").join("db").join("db.sqlite")
}

// ============================================================================
// Provider 配置（v2/config.json）
// ============================================================================

/// 读取 ZCode provider 配置文件
///
/// 文件不存在时返回空对象 `Ok(json!({}))`，不抛错。
pub fn read_zcode_config() -> Result<Value, AppError> {
    let path = get_zcode_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse ZCode config: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 ZCode provider 配置文件
pub fn write_zcode_config(config: &Value) -> Result<(), AppError> {
    let path = get_zcode_config_path();
    write_json_file(&path, config)?;

    log::debug!("ZCode config written to {path:?}");
    Ok(())
}

/// 获取所有 provider 条目（config.json 的 `provider` 字段）
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_zcode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置（新增/更新）一个 provider 条目
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_zcode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_zcode_config(&full_config)
}

/// 移除一个 provider 条目
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_zcode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_zcode_config(&config)
}

/// 以类型化结构读取所有 provider
pub fn get_typed_providers() -> Result<IndexMap<String, ZCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<ZCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse ZCode provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

/// 以类型化结构写入一个 provider
pub fn set_typed_provider(id: &str, config: &ZCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

// ============================================================================
// MCP 配置（cli/config.json，独立文件！）
// ============================================================================

/// 读取 ZCode CLI 配置（含 MCP / skills）
///
/// 文件不存在时返回空对象 `Ok(json!({}))`，不抛错。
pub fn read_zcode_cli_config() -> Result<Value, AppError> {
    let path = get_zcode_cli_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse ZCode CLI config: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 ZCode CLI 配置
pub fn write_zcode_cli_config(config: &Value) -> Result<(), AppError> {
    let path = get_zcode_cli_config_path();
    write_json_file(&path, config)?;

    log::debug!("ZCode CLI config written to {path:?}");
    Ok(())
}

/// 获取所有 MCP server 条目（cli/config.json 的 `mcp.servers` 字段）
///
/// ZCode 的 MCP 使用标准 `type: "stdio"` 格式，直接透传，无需转换。
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_zcode_cli_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置（新增/更新）一个 MCP server 条目（透传，不转换格式）
pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_zcode_cli_config()?;

    let mcp = full_config
        .as_object_mut()
        .ok_or_else(|| AppError::Config("ZCode CLI config root is not a JSON object".into()))?
        .entry("mcp")
        .or_insert_with(|| json!({}));

    let mcp_obj = mcp
        .as_object_mut()
        .ok_or_else(|| AppError::Config("ZCode CLI config 'mcp' is not a JSON object".into()))?;

    let servers = mcp_obj.entry("servers").or_insert_with(|| json!({}));

    if let Some(servers) = servers.as_object_mut() {
        servers.insert(id.to_string(), config);
    }

    write_zcode_cli_config(&full_config)
}

/// 移除一个 MCP server 条目
pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_zcode_cli_config()?;

    if let Some(servers) = config
        .get_mut("mcp")
        .and_then(|v| v.get_mut("servers"))
        .and_then(|v| v.as_object_mut())
    {
        servers.remove(id);
    }

    write_zcode_cli_config(&config)
}
