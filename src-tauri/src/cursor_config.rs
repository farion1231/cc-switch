//! Cursor 配置管理模块
//!
//! Cursor 使用 `~/.cursor/mcp.json` 存放全局 MCP 服务器配置，格式与社区
//! 标准 `.mcp.json` 一致：
//! ```json
//! { "mcpServers": { "name": { "command": ..., "args": [...], "env": {...} } } }
//! ```
//! Cursor 的全局 skills 目录为 `~/.cursor/skills/<skill>/SKILL.md`。

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write, get_home_dir};
use crate::error::AppError;

/// 获取 Cursor 配置目录（`~/.cursor`，可通过 settings 的 cursor_config_dir 覆盖）
pub fn get_cursor_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_cursor_override_dir() {
        return custom;
    }
    get_home_dir().join(".cursor")
}

/// 获取 Cursor 全局 MCP 配置文件路径（`~/.cursor/mcp.json`）
pub fn get_cursor_mcp_path() -> PathBuf {
    get_cursor_dir().join("mcp.json")
}

/// Cursor 是否已初始化（`~/.cursor` 目录存在时才同步，避免为未安装的应用创建目录）
fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

/// 读取 Cursor mcp.json 中的 mcpServers 映射
pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = get_cursor_mcp_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let root = read_json_value(&path)?;
    Ok(root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default())
}

/// 将启用的 MCP 服务器映射写入 Cursor mcp.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let path = get_cursor_mcp_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        // 提取 server 字段（统一结构中部分条目把规范嵌套在 server 下）
        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        // 移除 UI 辅助字段，仅保留 Cursor 识别的 MCP 规范
        // （type/command/args/env/cwd/url/headers 等原样保留）
        obj.remove("enabled");
        obj.remove("source");
        obj.remove("id");
        obj.remove("name");
        obj.remove("description");
        obj.remove("tags");
        obj.remove("homepage");
        obj.remove("docs");

        out.insert(id.clone(), Value::Object(obj));
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.cursor/mcp.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

/// 获取 Cursor 全局 skills 目录（`~/.cursor/skills`）
pub fn get_cursor_skills_dir() -> PathBuf {
    get_cursor_dir().join("skills")
}
