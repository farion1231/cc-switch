#![allow(non_snake_case)]

/// Claude 插件：写入/清除固定配置
#[tauri::command]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, String> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| e.to_string())
    } else {
        crate::claude_plugin::write_claude_config().map_err(|e| e.to_string())
    }
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
