#![allow(non_snake_case)]

use crate::config::ConfigStatus;

/// Claude 插件：获取 ~/.claude/config.json 状态
#[tauri::command]
pub async fn get_claude_plugin_status() -> Result<ConfigStatus, String> {
    crate::claude_plugin::claude_config_status()
        .map(|(exists, path)| ConfigStatus {
            exists,
            path: path.to_string_lossy().to_string(),
        })
        .map_err(|e| e.to_string())
}

/// Claude 插件：读取配置内容（若不存在返回 Ok(None)）
#[tauri::command]
pub async fn read_claude_plugin_config() -> Result<Option<String>, String> {
    crate::claude_plugin::read_claude_config().map_err(|e| e.to_string())
}

/// Claude 插件：写入/清除固定配置
#[tauri::command]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, String> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| e.to_string())
    } else {
        crate::claude_plugin::write_claude_config().map_err(|e| e.to_string())
    }
}

/// Claude 插件：检测是否已写入目标配置
#[tauri::command]
pub async fn is_claude_plugin_applied() -> Result<bool, String> {
    crate::claude_plugin::is_claude_config_applied().map_err(|e| e.to_string())
}

/// Claude Code：跳过初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding=true）
#[tauri::command]
pub async fn apply_claude_onboarding_skip() -> Result<bool, String> {
    crate::claude_mcp::set_has_completed_onboarding().map_err(|e| e.to_string())
}

/// Claude Code：恢复初次安装确认（删除 ~/.claude.json 的 hasCompletedOnboarding 字段）
#[tauri::command]
pub async fn clear_claude_onboarding_skip() -> Result<bool, String> {
    crate::claude_mcp::clear_has_completed_onboarding().map_err(|e| e.to_string())
}

/// Claude Code：启用对话记录保护（写入 ~/.claude/settings.json 的 cleanupPeriodDays=99999）
#[tauri::command]
pub async fn apply_transcript_protection() -> Result<bool, String> {
    crate::claude_mcp::set_cleanup_period_days().map_err(|e| e.to_string())
}

/// Claude Code：关闭对话记录保护（删除 ~/.claude/settings.json 的 cleanupPeriodDays 字段）
#[tauri::command]
pub async fn clear_transcript_protection() -> Result<bool, String> {
    crate::claude_mcp::clear_cleanup_period_days().map_err(|e| e.to_string())
}

/// Claude Code：获取对话记录保护状态（cleanupPeriodDays > 365 视为已启用）
#[tauri::command]
pub async fn get_transcript_protection() -> Result<bool, String> {
    crate::claude_mcp::get_cleanup_period_days()
        .map(|v| v.map_or(false, |days| days > 365))
        .map_err(|e| e.to_string())
}
