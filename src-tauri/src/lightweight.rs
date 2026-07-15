use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, WebviewWindow, WebviewWindowBuilder};

static LIGHTWEIGHT_MODE: AtomicBool = AtomicBool::new(false);

pub(crate) fn create_main_window(app: &tauri::AppHandle) -> tauri::Result<WebviewWindow> {
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| {
            tauri::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "main window configuration not found",
            ))
        })?;
    let builder = WebviewWindowBuilder::from_config(app, window_config)?;

    #[cfg(target_os = "windows")]
    let builder = match crate::portable::paths() {
        Some(paths) => builder.data_directory(paths.webview2_dir().to_path_buf()),
        None => builder,
    };

    builder.build()
}

pub fn enter_lightweight_mode(app: &tauri::AppHandle) -> Result<(), String> {
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

    create_main_window(app).map_err(|e| format!("创建主窗口失败: {e}"))?;

    if crate::portable::is_portable() {
        crate::portable_window_state::restore(app)
            .map_err(|e| format!("恢复便携窗口状态失败: {e}"))?;
    }

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
