#[tauri::command]
pub fn enter_lightweight_mode(app: tauri::AppHandle) -> Result<(), String> {
    crate::lightweight::set_lightweight_preference(&app, true)
}

#[tauri::command]
pub fn exit_lightweight_mode(app: tauri::AppHandle) -> Result<(), String> {
    crate::lightweight::set_lightweight_preference(&app, false)
}

#[tauri::command]
pub fn is_lightweight_mode() -> bool {
    crate::lightweight::is_lightweight_preferred()
}
