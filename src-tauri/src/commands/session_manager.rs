#![allow(non_snake_case)]

use crate::app_config::AppType;
use crate::session_manager;
use crate::store::AppState;
use serde_json::Value;
use tauri::State;

fn shell_single_quote(value: &str) -> String {
    // POSIX-safe single quote escaping: ' -> '"'"'
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn is_codex_command(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed == "codex" || trimmed.starts_with("codex ")
}

fn extract_openai_key_from_auth_value(auth: &Value) -> Option<String> {
    let auth = auth.as_object()?;
    auth
        .get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn extract_codex_openai_key(state: &AppState) -> Result<Option<String>, String> {
    let provider_id = crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)
        .map_err(|e| format!("获取 Codex 当前供应商失败: {e}"))?;
    if let Some(provider_id) = provider_id {
        let provider = state
            .db
            .get_provider_by_id(&provider_id, "codex")
            .map_err(|e| format!("读取 Codex 供应商失败: {e}"))?;
        if let Some(provider) = provider {
            if let Some(auth) = provider.settings_config.get("auth") {
                if let Some(openai_key) = extract_openai_key_from_auth_value(auth) {
                    return Ok(Some(openai_key));
                }
            }
        }
    }

    // 回退：从 live auth.json 读取（避免 DB 与 live 短暂不同步导致恢复会话缺失环境变量）
    let auth_path = crate::codex_config::get_codex_auth_path();
    if auth_path.exists() {
        let text = std::fs::read_to_string(&auth_path)
            .map_err(|e| format!("读取 Codex auth.json 失败: {e}"))?;
        let auth_value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("解析 Codex auth.json 失败: {e}"))?;
        if let Some(openai_key) = extract_openai_key_from_auth_value(&auth_value) {
            return Ok(Some(openai_key));
        }
    }

    Ok(None)
}

fn prepend_codex_env(command: &str, openai_key: &str) -> String {
    format!(
        "export OPENAI_API_KEY={}; {}",
        shell_single_quote(openai_key),
        command
    )
}

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    let provider_id = providerId.clone();
    let source_path = sourcePath.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&provider_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

#[tauri::command]
pub async fn launch_session_terminal(
    state: State<'_, AppState>,
    command: String,
    cwd: Option<String>,
    custom_config: Option<String>,
) -> Result<bool, String> {
    let command = if is_codex_command(&command) {
        match extract_codex_openai_key(state.inner())? {
            Some(openai_key) => prepend_codex_env(&command, &openai_key),
            None => command.clone(),
        }
    } else {
        command.clone()
    };
    let cwd = cwd.clone();
    let custom_config = custom_config.clone();

    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        None => "terminal".to_string(), // Default to Terminal.app on macOS
    };

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::terminal::launch_terminal(
            &target,
            &command,
            cwd.as_deref(),
            custom_config.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}
