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
pub async fn apply_claude_plugin_config(
    official: bool,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| e.to_string())
    } else {
        crate::claude_plugin::write_claude_config_with_db(&state.db)
            .map_err(|e| e.to_string())
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

/// 获取所有插件列表及启用状态
#[tauri::command]
pub async fn list_plugins(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Vec<crate::database::PluginState>, String> {
    state.db.get_all_plugin_states().map_err(|e| e.to_string())
}

/// 设置插件启用/禁用状态，并重写 config.json
#[tauri::command]
pub async fn set_plugin_enabled(
    plugin_id: String,
    enabled: bool,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    let updated = state
        .db
        .set_plugin_enabled(&plugin_id, enabled)
        .map_err(|e| e.to_string())?;

    if !updated {
        return Err(format!("Plugin not found: {plugin_id}"));
    }

    // 检查是否已开启 Claude 插件集成，若是则重写 config.json
    let settings = crate::settings::get_settings();
    if settings.enable_claude_plugin_integration {
        crate::claude_plugin::write_claude_config_with_db(&state.db)
            .map_err(|e| e.to_string())?;
    }

    Ok(true)
}
