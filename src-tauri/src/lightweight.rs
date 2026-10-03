use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

/// 运行期是否处于轻量状态（主窗口已销毁）。仅内存，进程级。
static LIGHTWEIGHT_RUNTIME: AtomicBool = AtomicBool::new(false);

// ===== 运行期：仅操作窗口/dock，不修改用户偏好 =====

/// 进入轻量运行期：销毁主窗口、隐藏 dock。
///
/// 不会写偏好；调用方负责决定是否同时持久化偏好。
pub fn enter_lightweight_runtime(app: &tauri::AppHandle) -> Result<(), String> {
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
    // else: 窗口已不存在，仅同步标志位

    LIGHTWEIGHT_RUNTIME.store(true, Ordering::Release);
    log::info!("进入轻量运行期");
    Ok(())
}

/// 退出轻量运行期：恢复主窗口与 dock 图标。
///
/// 不会写偏好；调用方负责决定是否同时持久化偏好。
pub fn exit_lightweight_runtime(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::WebviewWindowBuilder;

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
        LIGHTWEIGHT_RUNTIME.store(false, Ordering::Release);
        log::info!("退出轻量运行期");
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

    LIGHTWEIGHT_RUNTIME.store(false, Ordering::Release);
    log::info!("退出轻量运行期");
    Ok(())
}

pub fn is_lightweight_runtime() -> bool {
    LIGHTWEIGHT_RUNTIME.load(Ordering::Acquire)
}

// ===== 偏好：持久化在 settings.json 中 =====

/// 用户是否偏好轻量模式（持久化）。
pub fn is_lightweight_preferred() -> bool {
    crate::settings::get_lightweight_mode_persisted()
}

/// 写入轻量模式偏好，并把运行期状态同步到偏好。
///
/// - 偏好开启且当前不在运行期：进入运行期。
/// - 偏好关闭且当前在运行期：退出运行期。
/// - 写盘失败时仍尝试同步运行期，但返回错误以便调用方告警。
pub fn set_lightweight_preference(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let persist_result = crate::settings::set_lightweight_mode_persisted(enabled)
        .map_err(|e| format!("持久化轻量模式偏好失败: {e}"));

    if enabled && !is_lightweight_runtime() {
        enter_lightweight_runtime(app)?;
    } else if !enabled && is_lightweight_runtime() {
        exit_lightweight_runtime(app)?;
    }

    crate::tray::refresh_tray_menu(app);
    persist_result
}
