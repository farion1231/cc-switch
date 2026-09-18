//! DevEco Code MCP 同步和导入模块
//!
//! DevEco Code 是 OpenCode 的二次开发分支，MCP 配置格式与 OpenCode **完全一致**
//! （`local`/`remote` + command 数组 + `environment`），因此格式转换直接复用
//! [`super::opencode`] 的转换器，本模块只负责目标文件的差异：
//!
//! | 维度 | OpenCode | DevEco Code |
//! |---|---|---|
//! | 配置文件 | `opencode.json` | `deveco.jsonc`（探测 `.jsonc` → `.json` → `config.json`） |
//! | MCP 段 | `mcp` | `mcp`（同构） |

use serde_json::Value;

use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;

use super::opencode::{convert_from_opencode_format, convert_to_opencode_format};

/// 检查 DevEco Code MCP 同步是否应继续（配置目录存在才同步）
fn should_sync_deveco_mcp() -> bool {
    crate::deveco_config::get_deveco_dir().exists()
}

/// 同步单个 MCP 服务器到 DevEco Code 配置
pub fn sync_single_server_to_deveco(
    _config: &crate::app_config::MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_deveco_mcp() {
        return Ok(());
    }

    let spec = convert_to_opencode_format(server_spec)?;
    crate::deveco_config::set_mcp_server(id, spec)
}

/// 从 DevEco Code 配置移除单个 MCP 服务器
pub fn remove_server_from_deveco(id: &str) -> Result<(), AppError> {
    if !should_sync_deveco_mcp() {
        return Ok(());
    }

    crate::deveco_config::remove_mcp_server(id)
}

/// 从 DevEco Code 配置导入 MCP 服务器到统一结构
///
/// 已存在的服务器只把 DevEco Code 应用位打开，不覆盖其它字段。
pub fn import_from_deveco(
    config: &mut crate::app_config::MultiAppConfig,
) -> Result<usize, AppError> {
    let mcp_map = crate::deveco_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    let servers = config
        .mcp
        .servers
        .get_or_insert_with(std::collections::HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        let unified_spec = match convert_from_opencode_format(&spec) {
            Ok(spec) => spec,
            Err(e) => {
                log::warn!("Skip invalid DevEco Code MCP server '{id}': {e}");
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };

        if let Err(e) = super::validation::validate_server_spec(&unified_spec) {
            log::warn!("Skip invalid MCP server '{id}' after conversion: {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.deveco {
                existing.apps.deveco = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for DevEco Code");
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: unified_spec,
                    apps: McpApps {
                        deveco: true,
                        ..Default::default()
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from DevEco Code");
        }
    }

    if !errors.is_empty() {
        log::warn!(
            "DevEco Code import completed with {} failures: {:?}",
            errors.len(),
            errors
        );
    }

    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mcp_format_conversion_is_shared_with_opencode() {
        // DevEco Code 与 OpenCode 的 MCP 格式同构，转换器必须产出一致的形状
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": { "HOME": "/Users/test" }
        });

        let converted = convert_to_opencode_format(&spec).expect("convert");
        assert_eq!(converted["type"], "local");
        assert_eq!(converted["command"][0], "npx");
        assert_eq!(converted["environment"]["HOME"], "/Users/test");

        let back = convert_from_opencode_format(&converted).expect("convert back");
        assert_eq!(back["type"], "stdio");
        assert_eq!(back["command"], "npx");
        assert_eq!(back["env"]["HOME"], "/Users/test");
    }
}
