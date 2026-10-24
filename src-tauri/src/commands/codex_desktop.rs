use crate::app_config::AppType;
use crate::mode::current::{self, Purpose};
use crate::services::codex_desktop::{self, DesktopRestartOutcome, DesktopStatus};
use crate::store::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

static RESTART_PENDING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn get_codex_desktop_status() -> Result<DesktopStatus, String> {
    tauri::async_runtime::spawn_blocking(codex_desktop::status)
        .await
        .map_err(|e| e.to_string())?
}

struct RestartGuard;
impl Drop for RestartGuard {
    fn drop(&mut self) {
        RESTART_PENDING.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn restart_codex_desktop(
    app_state: State<'_, AppState>,
    expected_provider_id: Option<String>,
) -> Result<DesktopRestartOutcome, String> {
    RESTART_PENDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Codex desktop is already restarting".to_string())?;
    let _pending = RestartGuard;
    let _switch = crate::mode::controller::lock_settled(&app_state, &AppType::Codex)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(expected) = expected_provider_id {
        let selected = current::provider_for(&app_state.db, &AppType::Codex, Purpose::Direct)
            .map_err(|e| e.to_string())?;
        codex_desktop::validate_switch_target(
            &expected,
            selected.as_deref(),
            current::is_proxy(&AppType::Codex),
        )?;
    }
    tauri::async_runtime::spawn_blocking(codex_desktop::restart)
        .await
        .map_err(|e| e.to_string())?
}
