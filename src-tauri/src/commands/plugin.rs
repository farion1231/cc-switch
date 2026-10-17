#![allow(non_snake_case)]

use cc_command_api::command_api;

use crate::config::ConfigStatus;
use crate::error::AppError;

/// Claude 插件：获取 ~/.claude/config.json 状态
#[command_api]
pub async fn get_claude_plugin_status() -> Result<ConfigStatus, AppError> {
    crate::claude_plugin::claude_config_status()
        .map(|(exists, path)| ConfigStatus {
            exists,
            path: path.to_string_lossy().to_string(),
        })
        .map_err(|e| AppError::Config(e.to_string()))
}

/// Claude 插件：读取配置内容（若不存在返回 Ok(None)）
#[command_api]
pub async fn read_claude_plugin_config() -> Result<Option<String>, AppError> {
    crate::claude_plugin::read_claude_config().map_err(|e| AppError::Config(e.to_string()))
}

/// Claude 插件：写入/清除固定配置
#[command_api]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, AppError> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| AppError::Config(e.to_string()))
    } else {
        crate::claude_plugin::write_claude_config().map_err(|e| AppError::Config(e.to_string()))
    }
}

/// Claude 插件：检测是否已写入目标配置
#[command_api]
pub async fn is_claude_plugin_applied() -> Result<bool, AppError> {
    crate::claude_plugin::is_claude_config_applied().map_err(|e| AppError::Config(e.to_string()))
}

/// Claude Code：跳过初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding=true）
#[command_api]
pub async fn apply_claude_onboarding_skip() -> Result<bool, AppError> {
    crate::claude_mcp::set_has_completed_onboarding().map_err(|e| AppError::Config(e.to_string()))
}

/// Claude Code：恢复初次安装确认（删除 ~/.claude.json 的 hasCompletedOnboarding 字段）
#[command_api]
pub async fn clear_claude_onboarding_skip() -> Result<bool, AppError> {
    crate::claude_mcp::clear_has_completed_onboarding().map_err(|e| AppError::Config(e.to_string()))
}
