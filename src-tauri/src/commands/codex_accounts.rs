use crate::codex_account::{
    CodexAccount, CodexUsageView, ImportResult, LoginSession, RefreshResult,
};
use crate::services::CodexUsageService;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn codex_list_accounts(state: State<'_, AppState>) -> Result<Vec<CodexAccount>, String> {
    state
        .db
        .list_codex_accounts(true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn codex_start_login(provider_id: String) -> Result<LoginSession, String> {
    CodexUsageService::start_login(provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn codex_complete_login(
    state: State<'_, AppState>,
    session_id: String,
    callback_payload: String,
) -> Result<CodexAccount, String> {
    CodexUsageService::complete_login(&state.db, session_id, callback_payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn codex_import_from_switcher_once(state: State<'_, AppState>) -> Result<ImportResult, String> {
    CodexUsageService::import_from_switcher_once(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn codex_get_usage_state(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<CodexUsageView, String> {
    CodexUsageService::get_usage_view_by_provider(&state.db, &provider_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn codex_refresh_usage_now(
    state: State<'_, AppState>,
    provider_id: Option<String>,
) -> Result<RefreshResult, String> {
    CodexUsageService::refresh_usage_now(&state.db, provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn codex_bind_provider_auth(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<CodexAccount, String> {
    CodexUsageService::bind_from_provider_auth(&state.db, &provider_id).map_err(|e| e.to_string())
}
