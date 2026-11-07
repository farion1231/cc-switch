use crate::cline_config::{self, ClineHubStatus};

/// Whether the shared `cline-hub` needs a restart to load custom providers.
#[tauri::command]
pub fn get_cline_hub_status() -> Result<ClineHubStatus, String> {
    Ok(cline_config::hub_status())
}
