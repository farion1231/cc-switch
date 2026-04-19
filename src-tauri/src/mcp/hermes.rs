//! Hermes MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::hermes_config::{read_hermes_config, write_hermes_config_atomic};

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_hermes_mcp() -> bool {
    // Hermes 未安装/未初始化时：~/.hermes 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::hermes_config::get_hermes_dir().exists()
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

/// 从 Hermes config.yaml 中读取 mcp_servers 映射
///
/// Hermes 使用 YAML 格式，mcp_servers 是一个 mapping。
/// 返回的 Value 是 serde_json 格式，便于与其他模块统一处理。
fn read_hermes_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let yaml = read_hermes_config()?;

    let mcp_servers = yaml.get("mcp_servers").and_then(|v| v.as_mapping());

    if let Some(servers_map) = mcp_servers {
        let mut result = HashMap::new();
        for (key, val) in servers_map.iter() {
            if let Some(key_str) = key.as_str() {
                // 将 serde_yaml::Value 转换为 serde_json::Value
                let json_val = serde_json::to_value(val).map_err(|e| {
                    AppError::localized(
                        "hermes.mcp.convert_error",
                        format!("Hermes MCP 配置转换失败: {e}"),
                        format!("Failed to convert Hermes MCP config: {e}"),
                    )
                })?;
                result.insert(key_str.to_string(), json_val);
            }
        }
        return Ok(result);
    }

    Ok(HashMap::new())
}

/// 将 MCP 服务器映射写入 Hermes config.yaml 的 mcp_servers 字段
fn set_hermes_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let mut yaml = read_hermes_config()?;

    // 确保 yaml 是一个 mapping
    if !yaml.is_mapping() {
        yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let mapping = yaml.as_mapping_mut().expect("yaml is a mapping");
    let mcp_key = serde_yaml::Value::String("mcp_servers".to_string());

    // 构建 mcp_servers mapping
    let mut mcp_map = serde_yaml::Mapping::new();
    for (id, spec) in servers.iter() {
        // 将 serde_json::Value 转换为 serde_yaml::Value
        let yaml_val = serde_yaml::to_value(spec).map_err(|e| {
            AppError::localized(
                "hermes.mcp.convert_error",
                format!("Hermes MCP 配置转换失败: {e}"),
                format!("Failed to convert Hermes MCP config: {e}"),
            )
        })?;
        mcp_map.insert(serde_yaml::Value::String(id.clone()), yaml_val);
    }

    mapping.insert(mcp_key, serde_yaml::Value::Mapping(mcp_map));

    write_hermes_config_atomic(&yaml)
}

/// 将 config.json 中 Hermes 的 enabled==true 项写入 Hermes MCP 配置
pub fn sync_enabled_to_hermes(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_hermes_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.hermes);
    set_hermes_mcp_servers_map(&enabled)
}

/// 从 Hermes MCP 配置导入到统一结构
/// 已存在的服务器将启用 Hermes 应用，不覆盖其他字段和应用状态
pub fn import_from_hermes(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let map = read_hermes_mcp_servers_map()?;
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
            // 已存在：仅启用 Hermes 应用
            if !existing.apps.hermes {
                existing.apps.hermes = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Hermes 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Hermes
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
                        hermes: true,
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

/// 将单个 MCP 服务器同步到 Hermes live 配置
pub fn sync_single_server_to_hermes(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_hermes_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = read_hermes_mcp_servers_map()?;

    // 添加/更新当前服务器
    current.insert(id.to_string(), server_spec.clone());

    // 写回
    set_hermes_mcp_servers_map(&current)
}

/// 从 Hermes live 配置中移除单个 MCP 服务器
pub fn remove_server_from_hermes(id: &str) -> Result<(), AppError> {
    if !should_sync_hermes_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = read_hermes_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    set_hermes_mcp_servers_map(&current)
}
