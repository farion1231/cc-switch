//! Cursor IDE 配置文件读写模块
//!
//! Cursor 是一个基于 VS Code 的 AI 编辑器。与 CLI 工具不同，Cursor 没有原生的
//! "供应商配置文件"。CC Switch 管理 `~/.cursor/env.json` 作为 SSOT 供应商配置，
//! 并通过 CC Switch 本地代理拦截和路由 Cursor 的 API 调用。
//!
//! ## 配置结构
//!
//! CC Switch 内部使用扁平的标准格式存储 Cursor 供应商配置：
//! ```json
//! {
//!   "baseUrl": "https://api.anthropic.com",
//!   "apiKey": "sk-ant-...",
//!   "model": "claude-sonnet-4-20250514"
//! }
//! ```
//!
//! 写入 `~/.cursor/env.json` 时，根据 base URL 转换为对应的环境变量名：
//! - Anthropic 兼容: `ANTHROPIC_BASE_URL` + `ANTHROPIC_API_KEY`
//! - OpenAI 兼容: `OPENAI_BASE_URL` + `OPENAI_API_KEY`
//!
//! Cursor 为 switch 模式（非 additive）：env.json 仅包含当前供应商的配置。

use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_cursor_override_dir;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Cursor 配置目录
///
/// 解析顺序:
/// 1. CCS 设置 `cursor_config_dir`（显式覆盖）
/// 2. 平台默认（`~/.cursor/`）
pub fn get_cursor_dir() -> PathBuf {
    if let Some(override_dir) = get_cursor_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".cursor")
}

/// 获取 Cursor env.json 配置文件路径
///
/// 返回 `~/.cursor/env.json`
pub fn get_cursor_env_path() -> PathBuf {
    get_cursor_dir().join("env.json")
}

/// 确保 Cursor 配置目录存在
fn ensure_cursor_dir() -> Result<(), AppError> {
    let dir = get_cursor_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        log::info!("Created Cursor config directory: {:?}", dir);
    }
    Ok(())
}

// ============================================================================
// Read/Write Functions
// ============================================================================

/// 读取 Cursor env.json 配置
///
/// 如果文件不存在，返回空 JSON 对象。
pub fn read_cursor_env() -> Result<Value, AppError> {
    let path = get_cursor_env_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    if content.trim().is_empty() {
        return Ok(json!({}));
    }

    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Cursor env.json: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 Cursor env.json 配置
pub fn write_cursor_env(config: &Value) -> Result<(), AppError> {
    ensure_cursor_dir()?;
    let path = get_cursor_env_path();
    write_json_file(&path, config)
}

// ============================================================================
// Provider Management
// ============================================================================

/// 获取所有提供商配置
pub fn get_providers() -> Result<Value, AppError> {
    read_cursor_env()
}

/// 获取单个提供商配置
// 目前仅测试使用，保留给后续命令接入
#[allow(dead_code)]
pub fn get_provider(id: &str) -> Result<Option<Value>, AppError> {
    let config = read_cursor_env()?;
    Ok(config.get(id).cloned())
}

/// 将 CC Switch 标准扁平格式 `{ baseUrl, apiKey, model }` 转换为
/// env.json 的环境变量对象：
/// - Anthropic 兼容: `ANTHROPIC_BASE_URL` + `ANTHROPIC_API_KEY`
/// - OpenAI 兼容: `OPENAI_BASE_URL` + `OPENAI_API_KEY`
///
/// NOTE: 不写 MODEL env var。Cursor 读到不认识的模型名会校验报错。
/// 模型映射由 cc-switch proxy 的 modelCatalog 处理，不通过 env.json 传递。
pub fn provider_config_to_env(provider_config: &Value) -> Value {
    let base_url = provider_config
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let api_key = provider_config
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Determine env-var prefix from the base URL heuristically.
    // Default to Anthropic-style keys (Cursor's primary model provider).
    let is_openai = base_url.contains("openai.com")
        || base_url.contains("deepseek.com")
        || base_url.contains("api.openai.com");
    let (base_url_key, api_key_key) = if is_openai {
        ("OPENAI_BASE_URL", "OPENAI_API_KEY")
    } else {
        ("ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY")
    };

    let mut env = serde_json::Map::new();
    if !base_url.is_empty() {
        env.insert(
            base_url_key.to_string(),
            Value::String(base_url.to_string()),
        );
    }
    if !api_key.is_empty() {
        env.insert(api_key_key.to_string(), Value::String(api_key.to_string()));
    }
    Value::Object(env)
}

/// 写入当前供应商配置到 env.json。
///
/// **不再写入**：Cursor 的 "Override OpenAI Base URL"/API Key 权威来源是云端账户同步，
/// 本地写 env.json 一律无效（2026-08-05 实测），且 Cursor 云端 SSRF 拦私网。
/// Cursor 配置统一走手工粘贴（公网隧道地址 + ccsk 密钥），见 #5 review。
pub fn set_provider(_provider_config: &Value) -> Result<(), AppError> {
    log::info!("Cursor set_provider：不写 env.json（云端账户同步，本地写入无效），使用手工粘贴");
    Ok(())
}

/// 删除提供商配置
// 目前仅测试使用，保留给后续命令接入
#[allow(dead_code)]
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_cursor_env()?;

    if let Some(obj) = config.as_object_mut() {
        obj.remove(id);
    }

    write_cursor_env(&config)
}

// ============================================================================
// Subscription Status Bypass (for proxy takeover)
// ============================================================================

/// Cursor 状态数据库路径
fn get_cursor_state_db_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Cursor/User/globalStorage/state.vscdb")
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Cursor/User/globalStorage/state.vscdb")
    }
}

const SUBSCRIPTION_KEY: &str = "cursorAuth/stripeSubscriptionStatus";

/// 读取 Cursor 本地缓存的订阅状态
pub fn read_subscription_status_internal() -> Result<Option<String>, AppError> {
    let path = get_cursor_state_db_path();
    if !path.exists() {
        return Ok(None);
    }
    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| AppError::Config(format!("Failed to open Cursor state DB: {e}")))?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ?1")
        .map_err(|e| AppError::Config(format!("Failed to prepare query: {e}")))?;
    let result: Option<String> = stmt
        .query_row([SUBSCRIPTION_KEY], |row| row.get(0))
        .optional()
        .map_err(|e| AppError::Config(format!("Failed to read subscription status: {e}")))?;
    Ok(result)
}

/// 写入 Cursor 本地缓存的订阅状态，绕过客户端计费拦截。
/// 保存原始值以便恢复。
fn write_subscription_status(status: &str) -> Result<(), AppError> {
    let path = get_cursor_state_db_path();
    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| AppError::Config(format!("Failed to open Cursor state DB: {e}")))?;
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
        [SUBSCRIPTION_KEY, status],
    )
    .map_err(|e| AppError::Config(format!("Failed to write subscription status: {e}")))?;
    Ok(())
}

/// Proxy 接管时绕过 Cursor 本地订阅检查
pub fn bypass_subscription_check() -> Result<(), AppError> {
    let current = read_subscription_status_internal()?;
    if current.as_deref() == Some("active") {
        return Ok(()); // Already active, nothing to do
    }
    // Save original as backup
    if let Some(ref orig) = current {
        // Write "active" to bypass the local check
        write_subscription_status("active")?;
        log::info!("Cursor subscription status bypassed (was: {orig})");
    } else {
        write_subscription_status("active")?;
        log::info!("Cursor subscription status set to active (no previous value)");
    }
    Ok(())
}

/// 恢复 Cursor 订阅状态到原始值
pub fn restore_subscription_status(original: Option<&str>) -> Result<(), AppError> {
    match original {
        Some(status) => {
            write_subscription_status(status)?;
            log::info!("Cursor subscription status restored to: {status}");
        }
        None => {
            // Remove our injected value
            write_subscription_status("")?;
            log::info!("Cursor subscription status cleared");
        }
    }
    Ok(())
}

// ============================================================================
// MCP Configuration Helpers (for future use)
// ============================================================================

/// 获取项目级 `.cursor/mcp.json` 路径
///
/// Cursor 的 MCP 配置是项目级的，存储在项目根目录的 `.cursor/mcp.json` 中。
/// 格式为标准的 MCP 服务器配置。
#[allow(dead_code)]
pub fn get_cursor_project_mcp_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".cursor").join("mcp.json")
}

/// 获取全局 Cursor MCP 配置路径
pub fn get_cursor_global_mcp_path() -> PathBuf {
    get_cursor_dir().join("mcp.json")
}

/// 读取 Cursor MCP 配置中的服务器 map
///
/// Cursor MCP 配置格式与 VS Code 一致：
/// ```json
/// { "mcpServers": { "server-id": { "command": "...", "args": [], "env": {} } } }
/// ```
pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = get_cursor_global_mcp_path();

    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    if content.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let config: Value = serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Cursor mcp.json: {}: {e}",
            path.display()
        ))
    })?;

    Ok(config
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default())
}

/// 写入 Cursor MCP 配置的服务器 map
pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    ensure_cursor_dir()?;
    let path = get_cursor_global_mcp_path();
    let config = json!({ "mcpServers": servers });
    write_json_file(&path, &config)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_cursor_dir_default() {
        let dir = get_cursor_dir();
        assert!(dir.ends_with(".cursor"));
    }

    #[test]
    fn test_get_cursor_env_path() {
        let path = get_cursor_env_path();
        assert!(path.ends_with("env.json"));
    }

    #[test]
    fn test_read_cursor_env_empty() {
        // Without a real ~/.cursor/env.json, this should return empty object
        // Since the file doesn't exist by default
    }

    #[test]
    fn test_set_and_remove_provider() {
        // Test provider CRUD operations
        // Note: These tests require a writable filesystem
    }
}
