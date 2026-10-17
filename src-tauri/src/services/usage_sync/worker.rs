//! Background scheduler for automatic usage sync.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::database::Database;

pub const EVENT_USAGE_SYNC_STATUS: &str = "usage-sync-status-updated";

/// First automatic run waits for startup work (session scan, rollup) to settle.
const STARTUP_DELAY: Duration = Duration::from_secs(20);

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn wake() -> &'static Arc<Notify> {
    static WAKE: OnceLock<Arc<Notify>> = OnceLock::new();
    WAKE.get_or_init(|| Arc::new(Notify::new()))
}

/// Re-evaluate the schedule now (preferences changed).
pub(crate) fn notify_settings_changed() {
    wake().notify_one();
}

pub(crate) fn emit_status_changed() {
    if let Some(app) = APP_HANDLE.get() {
        if let Err(error) = app.emit(EVENT_USAGE_SYNC_STATUS, ()) {
            log::debug!("[UsageSync] failed to emit status event: {error}");
        }
    }
}

pub fn start_worker(db: Arc<Database>, app: AppHandle) {
    let _ = APP_HANDLE.set(app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let settings = crate::settings::get_usage_sync_settings();
            if settings.enabled && settings.auto_sync {
                if let Err(error) = super::run_sync(&db, "auto").await {
                    log::warn!("[UsageSync] automatic sync failed: {error}");
                }
            }
            let interval =
                Duration::from_secs(u64::from(settings.normalized_interval_minutes()) * 60);
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = wake().notified() => {}
            }
        }
    });
}
