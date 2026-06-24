//! Claude MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use std::path::PathBuf;

use crate::app_config::{AppType, McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

/// 解析指定 Claude 系应用（Claude / Claude Xcode）的配置目录与 MCP 文件路径
fn claude_dir_and_mcp_path(app: &AppType) -> (PathBuf, PathBuf) {
    match app {
        AppType::ClaudeXcode => (
            crate::config::get_claude_xcode_config_dir(),
            crate::config::get_claude_xcode_mcp_path(),
        ),
        _ => (
            crate::config::get_claude_config_dir(),
            crate::config::get_claude_mcp_path(),
        ),
    }
}

fn should_sync_claude_mcp(app: &AppType) -> bool {
    // Claude 未安装/未初始化时：通常配置目录与 MCP 文件都不存在。
    // 按用户偏好：此时跳过写入/删除，不创建任何文件或目录。
    let (dir, mcp_path) = claude_dir_and_mcp_path(app);
    dir.exists() || mcp_path.exists()
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

/// 将 config.json 中 enabled==true 的项投影写入 ~/.claude.json
pub fn sync_enabled_to_claude(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_claude_mcp(&AppType::Claude) {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map(&crate::config::get_claude_mcp_path(), &enabled)
}

/// 从 ~/.claude.json 导入 mcpServers 到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Claude 应用，不覆盖其他字段和应用状态
pub fn import_from_claude(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let text_opt = crate::claude_mcp::read_mcp_json(&crate::config::get_claude_mcp_path())?;
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
                        claude_xcode: false,
                        codex: false,
                        gemini: false,
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

/// 将单个 MCP 服务器同步到 Claude（含 Claude Xcode）live 配置
pub fn sync_single_server_to_claude(
    app: &AppType,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_claude_mcp(app) {
        return Ok(());
    }
    let (_, mcp_path) = claude_dir_and_mcp_path(app);

    // 读取现有的 MCP 配置
    let current = crate::claude_mcp::read_mcp_servers_map(&mcp_path)?;

    // 创建新的 HashMap，包含现有的所有服务器 + 当前要同步的服务器
    let mut updated = current;
    updated.insert(id.to_string(), server_spec.clone());

    // 写回
    crate::claude_mcp::set_mcp_servers_map(&mcp_path, &updated)
}

/// 从 Claude（含 Claude Xcode）live 配置中移除单个 MCP 服务器
pub fn remove_server_from_claude(app: &AppType, id: &str) -> Result<(), AppError> {
    if !should_sync_claude_mcp(app) {
        return Ok(());
    }
    let (_, mcp_path) = claude_dir_and_mcp_path(app);

    // 读取现有的 MCP 配置
    let mut current = crate::claude_mcp::read_mcp_servers_map(&mcp_path)?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    crate::claude_mcp::set_mcp_servers_map(&mcp_path, &current)
}
