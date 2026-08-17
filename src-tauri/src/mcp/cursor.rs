//! Cursor MCP 同步和导入模块
//!
//! Cursor 使用 `~/.cursor/mcp.json` 存放全局 MCP 服务器，格式与社区标准一致：
//! `{ "mcpServers": { "name": { "command": ..., "args": [...], "env": {...} } } }`。

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_cursor_mcp() -> bool {
    // Cursor 未安装/未初始化时：~/.cursor 目录不存在。
    // 与 Gemini 策略一致：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::cursor_config::get_cursor_dir().exists()
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

/// 将 config.json 中 Cursor 的 enabled==true 项写入 ~/.cursor/mcp.json
pub fn sync_enabled_to_cursor(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_cursor_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.cursor);
    crate::cursor_config::set_mcp_servers_map(&enabled)
}

/// 从 ~/.cursor/mcp.json 导入 mcpServers 到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Cursor 应用，不覆盖其他字段和应用状态
pub fn import_from_cursor(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let map = crate::cursor_config::read_mcp_servers_map()?;
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
            // 已存在：仅启用 Cursor 应用
            if !existing.apps.cursor {
                existing.apps.cursor = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Cursor 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Cursor
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec.clone(),
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        cursor: true,
                        gemini: false,
                        grokbuild: false,
                        opencode: false,
                        hermes: false,
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

/// 将单个 MCP 服务器同步到 Cursor live 配置
pub fn sync_single_server_to_cursor(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_cursor_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::cursor_config::read_mcp_servers_map()?;

    // 添加/更新当前服务器
    current.insert(id.to_string(), server_spec.clone());

    // 写回
    crate::cursor_config::set_mcp_servers_map(&current)
}

/// 从 Cursor live 配置中移除单个 MCP 服务器
pub fn remove_server_from_cursor(id: &str) -> Result<(), AppError> {
    if !should_sync_cursor_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::cursor_config::read_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    crate::cursor_config::set_mcp_servers_map(&current)
}