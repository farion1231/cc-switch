use crate::liveagent_config::{self, LiveAgentStatus, NativeSettings};

#[tauri::command]
pub fn get_liveagent_status() -> Result<LiveAgentStatus, String> {
    liveagent_config::status().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_liveagent_settings() -> Result<NativeSettings, String> {
    liveagent_config::native_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_liveagent_default_model(provider: String, model: String) -> Result<(), String> {
    liveagent_config::set_default(&provider, &model).map_err(|e| e.to_string())
}
