//! Claude MCP 同步和导入模块

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_claude_mcp() -> bool {
    // Claude 未安装/未初始化时：通常 ~/.claude 目录与 ~/.claude.json 都不存在。
    // 按用户偏好：此时跳过写入/删除，不创建任何文件或目录。
    let configured_dir = crate::settings::get_claude_configured_override_dir()
        .unwrap_or_else(|| crate::config::get_home_dir().join(".claude"));
    should_sync_claude_mcp_at(
        &configured_dir,
        &crate::config::get_claude_configured_mcp_path(),
    )
}

fn should_sync_claude_mcp_at(config_dir: &Path, mcp_path: &Path) -> bool {
    config_dir.exists() || mcp_path.exists()
}

fn active_claude_mcp_target() -> (PathBuf, PathBuf) {
    (
        crate::config::get_claude_config_dir(),
        crate::config::get_claude_mcp_path(),
    )
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
    if !should_sync_claude_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map(&enabled)
}

pub fn sync_enabled_to_active_claude(config: &MultiAppConfig) -> Result<(), AppError> {
    let (config_dir, mcp_path) = active_claude_mcp_target();
    if !should_sync_claude_mcp_at(&config_dir, &mcp_path) {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map_at_path(&mcp_path, &enabled)
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

/// 将单个 MCP 服务器同步到 Claude live 配置
pub fn sync_single_server_to_claude(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let current = crate::claude_mcp::read_mcp_servers_map()?;

    // 创建新的 HashMap，包含现有的所有服务器 + 当前要同步的服务器
    let mut updated = current;
    updated.insert(id.to_string(), server_spec.clone());

    // 写回
    crate::claude_mcp::set_mcp_servers_map(&updated)
}

pub fn sync_single_server_to_active_claude(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    let (config_dir, mcp_path) = active_claude_mcp_target();
    if !should_sync_claude_mcp_at(&config_dir, &mcp_path) {
        return Ok(());
    }
    let current = crate::claude_mcp::read_mcp_servers_map_from_path(&mcp_path)?;

    let mut updated = current;
    updated.insert(id.to_string(), server_spec.clone());

    crate::claude_mcp::set_mcp_servers_map_at_path(&mcp_path, &updated)
}

/// 从 Claude live 配置中移除单个 MCP 服务器
pub fn remove_server_from_claude(id: &str) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::claude_mcp::read_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    crate::claude_mcp::set_mcp_servers_map(&current)
}

pub fn remove_server_from_active_claude(id: &str) -> Result<(), AppError> {
    let (config_dir, mcp_path) = active_claude_mcp_target();
    if !should_sync_claude_mcp_at(&config_dir, &mcp_path) {
        return Ok(());
    }
    let mut current = crate::claude_mcp::read_mcp_servers_map_from_path(&mcp_path)?;
    current.remove(id);

    crate::claude_mcp::set_mcp_servers_map_at_path(&mcp_path, &current)
}

pub fn remove_server_from_active_and_configured_claude(id: &str) -> Result<(), AppError> {
    remove_server_from_active_claude(id)?;
    remove_server_from_claude(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_test_home,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(crate::settings::AppSettings::default());
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[test]
    #[serial]
    fn direct_claude_mcp_sync_ignores_provider_only_profile_override() {
        let home = TempHome::new();
        let profile_dir = home.path().join("external-profile").join(".claude");
        let profile_mcp_path = profile_dir.join(".claude.json");
        let default_mcp_path = crate::config::get_default_claude_mcp_path();
        std::fs::create_dir_all(&profile_dir).expect("create profile dir");
        std::fs::write(
            &profile_mcp_path,
            r#"{"mcpServers":{"external":{"type":"stdio","command":"external-tool"}}}"#,
        )
        .expect("seed profile mcp config");
        crate::settings::update_settings(crate::settings::AppSettings {
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set profile override");

        sync_single_server_to_claude(
            &MultiAppConfig::default(),
            "managed-mcp",
            &json!({
                "type": "stdio",
                "command": "python",
                "args": ["-m", "managed_mcp"]
            }),
        )
        .expect("sync should be skipped");

        assert!(
            !default_mcp_path.exists(),
            "provider-only profile must not make direct MCP edits create the configured/default MCP file"
        );
        let profile_mcp = std::fs::read_to_string(&profile_mcp_path).expect("read profile mcp");
        assert!(
            profile_mcp.contains("external-tool") && !profile_mcp.contains("managed_mcp"),
            "provider-only profile MCP file must not be modified by direct MCP edits"
        );
    }
}
