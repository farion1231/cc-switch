//! Gemini MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{AppType, McpConfig, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, normalize_server_spec};
use super::{apply_parsed_import, build_imported_server, invalid_issue, ParsedImport};

fn should_sync_gemini_mcp() -> bool {
    // Gemini 未安装/未初始化时：~/.gemini 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::gemini_config::get_gemini_dir().exists()
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

/// 将 config.json 中 Gemini 的 enabled==true 项写入 Gemini MCP 配置
pub fn sync_enabled_to_gemini(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.gemini);
    crate::gemini_mcp::set_mcp_servers_map(&enabled)
}

/// 从 Gemini MCP 配置导入到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Gemini 应用，不覆盖其他字段和应用状态
pub fn import_from_gemini(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let parsed = parse_import_from_gemini()?;
    apply_parsed_import(config, parsed, AppType::Gemini)
}

pub(crate) fn parse_import_from_gemini() -> Result<ParsedImport, AppError> {
    let map = crate::gemini_mcp::read_mcp_servers_map()?;
    if map.is_empty() {
        return Ok(ParsedImport::default());
    }

    let mut parsed = ParsedImport::default();

    for (id, spec) in map.iter() {
        match normalize_server_spec(spec) {
            Ok(spec) => parsed
                .servers
                .push(build_imported_server(id.clone(), AppType::Gemini, spec)),
            Err(e) => {
                log::warn!("跳过无效 MCP 服务器 '{id}': {e}");
                parsed
                    .issues
                    .push(invalid_issue(id.clone(), AppType::Gemini, e.to_string()));
            }
        }
    }

    Ok(parsed)
}

/// 将单个 MCP 服务器同步到 Gemini live 配置
pub fn sync_single_server_to_gemini(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::gemini_mcp::read_mcp_servers_map()?;

    // 添加/更新当前服务器
    current.insert(id.to_string(), server_spec.clone());

    // 写回
    crate::gemini_mcp::set_mcp_servers_map(&current)
}

/// 从 Gemini live 配置中移除单个 MCP 服务器
pub fn remove_server_from_gemini(id: &str) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::gemini_mcp::read_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    crate::gemini_mcp::set_mcp_servers_map(&current)
}
