//! Qwen MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::validate_server_spec;

fn should_sync_qwen_mcp() -> bool {
    crate::qwen_config::get_qwen_dir().exists()
}

pub fn import_from_qwen(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let map = crate::qwen_mcp::read_mcp_servers_map()?;
    if map.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in map.iter() {
        if let Err(e) = validate_server_spec(spec) {
            log::warn!("跳过无效 MCP 服务器 '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            if !existing.apps.qwen {
                existing.apps.qwen = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Qwen 应用");
            }
        } else {
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
                        opencode: false,
                        qwen: true,
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

pub fn sync_single_server_to_qwen(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_qwen_mcp() {
        return Ok(());
    }
    let mut current = crate::qwen_mcp::read_mcp_servers_map()?;
    current.insert(id.to_string(), server_spec.clone());
    crate::qwen_mcp::set_mcp_servers_map(&current)
}

pub fn remove_server_from_qwen(id: &str) -> Result<(), AppError> {
    if !should_sync_qwen_mcp() {
        return Ok(());
    }
    let mut current = crate::qwen_mcp::read_mcp_servers_map()?;
    current.remove(id);
    crate::qwen_mcp::set_mcp_servers_map(&current)
}
