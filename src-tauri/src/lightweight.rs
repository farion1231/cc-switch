use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::Manager;

static LIGHTWEIGHT_MODE: AtomicBool = AtomicBool::new(false);
static AUTO_LIGHTWEIGHT_TIMER: AtomicU64 = AtomicU64::new(0);
static AUTO_LIGHTWEIGHT_PENDING: AtomicBool = AtomicBool::new(false);

const DEFAULT_AUTO_LIGHTWEIGHT_DELAY_MINUTES: u32 = 20;
const MAX_AUTO_LIGHTWEIGHT_DELAY_MINUTES: u32 = 1440;

pub(crate) fn normalize_auto_lightweight_delay_minutes(minutes: u32) -> u32 {
    if minutes == 0 {
        DEFAULT_AUTO_LIGHTWEIGHT_DELAY_MINUTES
    } else {
        minutes.min(MAX_AUTO_LIGHTWEIGHT_DELAY_MINUTES)
    }
}

pub fn cancel_auto_lightweight_timer() -> bool {
    AUTO_LIGHTWEIGHT_TIMER.fetch_add(1, Ordering::AcqRel);
    AUTO_LIGHTWEIGHT_PENDING.swap(false, Ordering::AcqRel)
}

pub fn schedule_auto_lightweight_after_close(app: tauri::AppHandle, delay_minutes: u32) {
    let delay_minutes = normalize_auto_lightweight_delay_minutes(delay_minutes);
    let ticket = AUTO_LIGHTWEIGHT_TIMER.fetch_add(1, Ordering::AcqRel) + 1;
    AUTO_LIGHTWEIGHT_PENDING.store(true, Ordering::Release);
    crate::tray::refresh_tray_menu(&app);

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(delay_minutes as u64 * 60)).await;

        if AUTO_LIGHTWEIGHT_TIMER.load(Ordering::Acquire) != ticket || is_lightweight_mode() {
            return;
        }

        let Some(window) = app.get_webview_window("main") else {
            if AUTO_LIGHTWEIGHT_PENDING.swap(false, Ordering::AcqRel) {
                crate::tray::refresh_tray_menu(&app);
            }
            return;
        };

        if window.is_visible().unwrap_or(false) {
            if AUTO_LIGHTWEIGHT_PENDING.swap(false, Ordering::AcqRel) {
                crate::tray::refresh_tray_menu(&app);
            }
            return;
        }

        if let Err(e) = enter_lightweight_mode(&app) {
            log::error!("进入轻量模式失败: {e}");
            crate::tray::refresh_tray_menu(&app);
        }
    });
}

pub fn enter_lightweight_mode(app: &tauri::AppHandle) -> Result<(), String> {
    cancel_auto_lightweight_timer();

    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_skip_taskbar(true);
        }
    }
    #[cfg(target_os = "macos")]
    {
        crate::tray::apply_tray_policy(app, false);
    }

    if let Some(window) = app.get_webview_window("main") {
        crate::save_window_state_before_exit(app);
        window
            .destroy()
            .map_err(|e| format!("销毁主窗口失败: {e}"))?;
    }
    // else: already in lightweight mode or window not found, just set the flag

    LIGHTWEIGHT_MODE.store(true, Ordering::Release);
    crate::tray::refresh_tray_menu(app);
    log::info!("进入轻量模式");
    Ok(())
}

pub fn exit_lightweight_mode(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::WebviewWindowBuilder;

    cancel_auto_lightweight_timer();

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "linux")]
        {
            crate::linux_fix::nudge_main_window(window.clone());
        }
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_skip_taskbar(false);
        }
        #[cfg(target_os = "macos")]
        {
            crate::tray::apply_tray_policy(app, true);
        }
        LIGHTWEIGHT_MODE.store(false, Ordering::Release);
        crate::tray::refresh_tray_menu(app);
        log::info!("退出轻量模式");
        return Ok(());
    }

    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or("主窗口配置未找到")?;

    WebviewWindowBuilder::from_config(app, window_config)
        .map_err(|e| format!("加载主窗口配置失败: {e}"))?
        .build()
        .map_err(|e| format!("创建主窗口失败: {e}"))?;

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "linux")]
        {
            crate::linux_fix::nudge_main_window(window.clone());
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_skip_taskbar(false);
        }
    }
    #[cfg(target_os = "macos")]
    {
        crate::tray::apply_tray_policy(app, true);
    }

    LIGHTWEIGHT_MODE.store(false, Ordering::Release);
    crate::tray::refresh_tray_menu(app);
    log::info!("退出轻量模式");
    Ok(())
}

pub fn is_lightweight_mode() -> bool {
    LIGHTWEIGHT_MODE.load(Ordering::Acquire)
}

pub fn is_auto_lightweight_pending() -> bool {
    AUTO_LIGHTWEIGHT_PENDING.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    fn reset_auto_lightweight_pending_for_test() {
        super::cancel_auto_lightweight_timer();
    }

    fn mark_auto_lightweight_pending_for_test() {
        super::AUTO_LIGHTWEIGHT_PENDING.store(true, std::sync::atomic::Ordering::Release);
    }

    #[test]
    fn auto_lightweight_delay_defaults_when_zero() {
        assert_eq!(super::normalize_auto_lightweight_delay_minutes(0), 20);
    }

    #[test]
    fn auto_lightweight_delay_keeps_custom_value() {
        assert_eq!(super::normalize_auto_lightweight_delay_minutes(10), 10);
    }

    #[test]
    fn auto_lightweight_delay_caps_large_value() {
        assert_eq!(super::normalize_auto_lightweight_delay_minutes(1441), 1440);
    }

    #[test]
    fn auto_lightweight_pending_state_can_be_cancelled() {
        reset_auto_lightweight_pending_for_test();
        mark_auto_lightweight_pending_for_test();
        assert!(super::is_auto_lightweight_pending());

        super::cancel_auto_lightweight_timer();
        assert!(!super::is_auto_lightweight_pending());
    }
}
