use crate::services::codex_desktop::{self, DesktopRestartOutcome};
use crate::store::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

static RESTART_PENDING: AtomicBool = AtomicBool::new(false);
struct RestartGuard;
impl Drop for RestartGuard {
    fn drop(&mut self) {
        RESTART_PENDING.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn restart_codex_desktop(
    app_state: State<'_, AppState>,
) -> Result<DesktopRestartOutcome, String> {
    RESTART_PENDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Codex desktop is already restarting".to_string())?;
    let _pending = RestartGuard;
    let _switch = app_state.proxy_service.lock_switch_for_app("codex").await;
    tauri::async_runtime::spawn_blocking(codex_desktop::restart)
        .await
        .map_err(|e| e.to_string())?
}
