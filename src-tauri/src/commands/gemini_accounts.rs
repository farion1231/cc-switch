use crate::codex_account::RefreshResult;
use crate::gemini_account::{
    GeminiAccount, GeminiLoginSession, GeminiLoginStatus, GeminiPoolStatus, GeminiUsageView,
};
use crate::services::GeminiUsageService;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn gemini_list_accounts(state: State<'_, AppState>) -> Result<Vec<GeminiAccount>, String> {
    GeminiUsageService::list_accounts(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_start_cli_login(provider_id: String) -> Result<GeminiLoginSession, String> {
    GeminiUsageService::start_cli_login(provider_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_get_cli_login_status(session_id: String) -> Result<GeminiLoginStatus, String> {
    GeminiUsageService::get_cli_login_status(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_cancel_cli_login(session_id: String) -> Result<bool, String> {
    GeminiUsageService::cancel_cli_login(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_finalize_cli_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<GeminiAccount, String> {
    GeminiUsageService::finalize_cli_login(&state.db, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_get_usage_state(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<GeminiUsageView, String> {
    GeminiUsageService::get_usage_view_by_provider(&state.db, &provider_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_refresh_usage_now(
    state: State<'_, AppState>,
    provider_id: Option<String>,
) -> Result<RefreshResult, String> {
    GeminiUsageService::refresh_usage_now(&state.db, provider_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gemini_pool_status(state: State<'_, AppState>) -> Result<GeminiPoolStatus, String> {
    GeminiUsageService::pool_status(&state.db).map_err(|e| e.to_string())
}
