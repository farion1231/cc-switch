use cc_command_api::command_api;

#[tauri::command]
pub fn enter_lightweight_mode(app: tauri::AppHandle) -> Result<(), String> {
    crate::lightweight::enter_lightweight_mode(&app)
}

#[tauri::command]
pub fn exit_lightweight_mode(app: tauri::AppHandle) -> Result<(), String> {
    crate::lightweight::exit_lightweight_mode(&app)
}

#[command_api(rest = "GET /control/v1/lightweight-mode")]
pub fn is_lightweight_mode() -> bool {
    crate::lightweight::is_lightweight_mode()
}
