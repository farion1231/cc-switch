//! Cursor IDE 配置文件读写模块
//!
//! Cursor 是一个基于 VS Code 的 AI 编辑器，由 CC Switch 作为**受管应用**接入：
//! 管理 MCP 服务器（`~/.cursor/mcp.json`）、Skills（`~/.cursor/skills`）、
//! 用户规则（`~/.cursor/rules/*.mdc`）与会话浏览。供应商侧只有官方卡，仅展示，
//! 不写 Cursor 的供应商设置（没有本地路由 / 公网隧道 / BYOK 接入）。
//!
//! 本模块不读写 Cursor 的供应商设置：那部分归 Cursor 自己管理，
//! CC Switch 只保留官方供应商卡。

use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_override_dir;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 内置官方供应商 seed 的稳定 ID。
pub const CURSOR_OFFICIAL_PROVIDER_ID: &str = "cursor-official";

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Cursor 配置目录
///
/// 解析顺序:
/// 1. CCS 设置 `cursor_config_dir`（显式覆盖）
/// 2. 平台默认（`~/.cursor/`）
pub fn get_cursor_dir() -> PathBuf {
    if let Some(override_dir) = get_override_dir(&crate::app_config::AppType::Cursor) {
        return override_dir;
    }

    crate::config::get_home_dir().join(".cursor")
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
}
