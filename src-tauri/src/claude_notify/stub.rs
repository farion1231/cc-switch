use super::types::ClaudeNotifyRuntimeStatus;

#[derive(Default)]
pub struct ClaudeNotifyService;

impl ClaudeNotifyService {
    pub fn new() -> Self {
        Self
    }

    pub async fn set_app_handle(&self, _handle: tauri::AppHandle) {}

    pub async fn ensure_started(&self) -> Result<ClaudeNotifyRuntimeStatus, String> {
        Ok(self.get_status().await)
    }

    pub async fn stop(&self) -> Result<(), String> {
        Ok(())
    }

    pub async fn sync_with_settings(&self) -> Result<ClaudeNotifyRuntimeStatus, String> {
        Ok(self.get_status().await)
    }

    pub async fn get_status(&self) -> ClaudeNotifyRuntimeStatus {
        ClaudeNotifyRuntimeStatus {
            listening: false,
            port: None,
        }
    }
}
