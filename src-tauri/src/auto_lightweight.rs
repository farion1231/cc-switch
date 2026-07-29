use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct AutoLightweightState {
    unfocused_at: Option<Instant>,
}

static STATE: OnceLock<Mutex<AutoLightweightState>> = OnceLock::new();
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);

fn state() -> &'static Mutex<AutoLightweightState> {
    STATE.get_or_init(|| Mutex::new(AutoLightweightState { unfocused_at: None }))
}

pub fn mark_focused() {
    let mut guard = state().lock().unwrap_or_else(|e| {
        log::warn!("Auto-lightweight state lock poisoned, recovering: {e}");
        e.into_inner()
    });
    guard.unfocused_at = None;
}

pub fn mark_unfocused() {
    let mut guard = state().lock().unwrap_or_else(|e| {
        log::warn!("Auto-lightweight state lock poisoned, recovering: {e}");
        e.into_inner()
    });
    if guard.unfocused_at.is_none() {
        guard.unfocused_at = Some(Instant::now());
    }
}

pub fn record_focus_state(focused: bool) {
    if focused {
        mark_focused();
    } else {
        mark_unfocused();
    }
}

fn idle_deadline_reached(
    unfocused_at: Option<Instant>,
    idle_minutes: Option<u32>,
    now: Instant,
) -> bool {
    let Some(minutes) = idle_minutes.filter(|minutes| *minutes > 0) else {
        return false;
    };
    let Some(unfocused_at) = unfocused_at else {
        return false;
    };

    now.saturating_duration_since(unfocused_at) >= Duration::from_secs(u64::from(minutes) * 60)
}

pub fn start_worker(app: tauri::AppHandle) {
    if WORKER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            if crate::lightweight::is_lightweight_mode() {
                continue;
            }

            let should_enter = {
                let guard = state().lock().unwrap_or_else(|e| {
                    log::warn!("Auto-lightweight state lock poisoned, recovering: {e}");
                    e.into_inner()
                });
                idle_deadline_reached(
                    guard.unfocused_at,
                    crate::settings::auto_lightweight_idle_minutes(),
                    Instant::now(),
                )
            };

            if should_enter {
                log::info!("Auto-lightweight idle threshold reached; entering lightweight mode");
                if let Err(err) = crate::lightweight::enter_lightweight_mode(&app) {
                    log::error!("Auto-lightweight mode failed: {err}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_deadline_is_disabled_without_positive_minutes() {
        let start = Instant::now();
        let now = start + Duration::from_secs(60 * 60);

        assert!(!idle_deadline_reached(Some(start), None, now));
        assert!(!idle_deadline_reached(Some(start), Some(0), now));
    }

    #[test]
    fn idle_deadline_requires_unfocused_time() {
        let now = Instant::now();

        assert!(!idle_deadline_reached(None, Some(1), now));
    }

    #[test]
    fn idle_deadline_uses_configured_minutes() {
        let start = Instant::now();

        assert!(!idle_deadline_reached(
            Some(start),
            Some(5),
            start + Duration::from_secs(299)
        ));
        assert!(idle_deadline_reached(
            Some(start),
            Some(5),
            start + Duration::from_secs(300)
        ));
    }
}
