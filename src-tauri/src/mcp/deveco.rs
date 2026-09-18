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

/// 从 DevEco Code 配置导入 MCP 服务器到数据库。
///
/// 与 OpenCode 那条「覆盖式」导入路径不同，这里采纳 MCode 的冲突语义：原生条目与
/// 库中已有同名服务器传输配置不一致时**跳过并上报**，而不是静默覆盖用户已有的
/// 定义。导入只打开 DevEco Code 应用位，其余应用位保持不变。
pub fn import(state: &crate::store::AppState) -> Result<usize, AppError> {
    let mcp_map = crate::deveco_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();

    for (id, native) in mcp_map {
        let mut spec = match convert_from_opencode_format(&native) {
            Ok(spec) => spec,
            Err(e) => {
                skipped.push(format!("'{id}': {e}"));
                continue;
            }
        };
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }

        // `enabled` 是 DevEco 原生的开关字段，不属于统一传输定义。
        let enabled = native
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if let Some(object) = spec.as_object_mut() {
            object.remove("enabled");
        }

        let server = if let Some(mut server) = existing.shift_remove(&id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.deveco = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    deveco: enabled,
                    ..Default::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            }
        };
        state.db.save_mcp_server(&server)?;
    }

    if skipped.is_empty() {
        Ok(count)
    } else {
        Err(AppError::InvalidInput(format!(
            "Imported {count} DevEco Code MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

/// 只保留传输相关字段，用于判断同名服务器是否真的冲突。
fn transport_spec(spec: &Value) -> Value {
    const TRANSPORT_FIELDS: [&str; 6] = ["type", "command", "args", "env", "url", "headers"];

    let mut spec = spec.clone();
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    }
    spec
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
