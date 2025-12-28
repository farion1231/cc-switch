//! Droid MCP 同步和导入模块
//!
//! Droid 的 MCP 配置文件位于 ~/.factory/mcp.json
//! 格式与 Claude 类似，使用 mcpServers 字段存储服务器配置

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::config::atomic_write;
use crate::droid_config::get_droid_config_dir;
use crate::error::AppError;

use super::validation::validate_server_spec;

/// 获取 Droid MCP 配置文件路径 (~/.factory/mcp.json)
fn get_droid_mcp_path() -> PathBuf {
    get_droid_config_dir().join("mcp.json")
}

fn should_sync_droid_mcp() -> bool {
    // Droid 未安装/未初始化时：~/.factory 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    get_droid_config_dir().exists()
}

fn read_json_value(path: &std::path::Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &std::path::Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

/// 读取 Droid mcp.json 中的 mcpServers 映射
pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = get_droid_mcp_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let root = read_json_value(&path)?;
    let servers: HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    Ok(servers)
}

/// 获取 Droid mcp.json 中已启用的服务器 ID 列表
/// 如果 mcp.json 中存在该 key，就认为是启用的
pub fn get_enabled_server_ids() -> HashSet<String> {
    read_mcp_servers_map()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// 将给定的启用 MCP 服务器映射写入到 Droid mcp.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let path = get_droid_mcp_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 构建 mcpServers 对象：移除 UI 辅助字段（enabled/source），仅保留实际 MCP 规范
    let mut out: serde_json::Map<String, Value> = serde_json::Map::new();
    for (id, spec) in servers.iter() {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        // 提取 server 字段（如果存在）
        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        // 移除 UI 辅助字段
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
            .ok_or_else(|| AppError::Config("~/.factory/mcp.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

/// 从 Droid MCP 配置导入到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Droid 应用，不覆盖其他字段和应用状态
#[allow(dead_code)]
pub fn import_from_droid(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let map = read_mcp_servers_map()?;
    if map.is_empty() {
        return Ok(0);
    }

    // 确保新结构存在
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in map.iter() {
        // 校验：单项失败不中止，收集错误继续处理
        if let Err(e) = validate_server_spec(spec) {
            log::warn!("跳过无效 MCP 服务器 '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            // 已存在：仅启用 Droid 应用
            if !existing.apps.droid {
                existing.apps.droid = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Droid 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Droid
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec.clone(),
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        gemini: false,
                        droid: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("导入新 MCP 服务器 '{id}'");
        }
    }

    if !errors.is_empty() {
        log::warn!("导入完成，但有 {} 项失败: {:?}", errors.len(), errors);
    }

    Ok(changed)
}

/// 将单个 MCP 服务器同步到 Droid live 配置
pub fn sync_single_server_to_droid(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_droid_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = read_mcp_servers_map()?;

    // 添加/更新当前服务器
    current.insert(id.to_string(), server_spec.clone());

    // 写回
    set_mcp_servers_map(&current)
}

/// 从 Droid live 配置中移除单个 MCP 服务器
pub fn remove_server_from_droid(id: &str) -> Result<(), AppError> {
    if !should_sync_droid_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = read_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    set_mcp_servers_map(&current)
}
