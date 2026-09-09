//! Claude MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

/// Check if Claude's config locations exist on disk.
/// Used only for removal (remove_server_from_claude) — sync operations
/// should NOT guard on this to avoid silently skipping deployment when
/// Claude Code hasn't been launched yet.
fn should_sync_claude_mcp() -> bool {
    crate::config::get_claude_config_dir().exists() || crate::config::get_claude_mcp_path().exists()
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

/// Project enabled servers from config.json into ~/.claude.json.
pub fn sync_enabled_to_claude(config: &MultiAppConfig) -> Result<(), AppError> {
    // Don't guard with should_sync_claude_mcp() — when ~/.claude/ or
    // ~/.claude.json don't exist yet (fresh Claude Code install that
    // hasn't been launched), the guard would silently skip the write,
    // and MCP configs configured in CC Switch are never deployed.
    // set_mcp_servers_map already handles parent directory creation.
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map(&enabled)
}

/// 从 ~/.claude.json 导入 mcpServers 到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Claude 应用，不覆盖其他字段和应用状态
pub fn import_from_claude(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let text_opt = crate::claude_mcp::read_mcp_json()?;
    let Some(text) = text_opt else { return Ok(0) };

    let v: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.claude.json 失败: {e}")))?;
    let Some(map) = v.get("mcpServers").and_then(|x| x.as_object()) else {
        return Ok(0);
    };

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
            // 已存在：仅启用 Claude 应用
            if !existing.apps.claude {
                existing.apps.claude = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Claude 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Claude
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec.clone(),
                    apps: McpApps {
                        claude: true,
                        codex: false,
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

/// Sync a single MCP server into Claude's live config (~/.claude.json).
pub fn sync_single_server_to_claude(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    // Don't guard with should_sync_claude_mcp() — see sync_enabled_to_claude.
    // Read the existing MCP config
    let current = crate::claude_mcp::read_mcp_servers_map()?;

    // Merge the server we're syncing with existing servers
    let mut updated = current;
    updated.insert(id.to_string(), server_spec.clone());

    // Write back
    crate::claude_mcp::set_mcp_servers_map(&updated)
}

/// Remove a single MCP server from Claude's live config (~/.claude.json).
/// If the config doesn't exist, there's nothing to remove — return Ok early.
pub fn remove_server_from_claude(id: &str) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        log::debug!("Skipping MCP removal from Claude: config does not exist");
        return Ok(());
    }
    // Read the existing MCP config
    let mut current = crate::claude_mcp::read_mcp_servers_map()?;

    // Remove the specified server
    current.remove(id);

    // Write back
    crate::claude_mcp::set_mcp_servers_map(&current)
}
