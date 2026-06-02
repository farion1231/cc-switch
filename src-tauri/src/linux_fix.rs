//! Linux-specific window startup and size handling.
//!
//! GNOME/Wayland is sensitive to how a hidden Tauri/WebKitGTK window is first
//! shown. The startup path keeps Tauri's normal `show()` call, then refreshes
//! the GTK top-level presentation so native titlebar hit testing is initialized.
//! The old +/-1px resize nudge is intentionally not used because it inflated
//! window size and polluted persisted state on GNOME.

use std::{
    fs,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, Manager, PhysicalSize, Size, WebviewWindow, WindowEvent};

const REALIZE_WAIT: Duration = Duration::from_millis(200);
const STARTUP_RESIZE_IGNORE: Duration = Duration::from_secs(2);

const DEFAULT_WIDTH: u32 = 1000;
const DEFAULT_HEIGHT: u32 = 650;
const MIN_WIDTH: u32 = 900;
const MIN_HEIGHT: u32 = 600;
const MAX_SAVED_WIDTH: u32 = 10_000;
const MAX_SAVED_HEIGHT: u32 = 10_000;
const MONITOR_MARGIN: u32 = 80;
const WINDOW_SIZE_STATE_FILE: &str = "linux-window-size.json";

static STARTED_AT: OnceLock<Instant> = OnceLock::new();

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct SavedWindowSize {
    width: u32,
    height: u32,
}

pub(crate) fn start_window_size_tracking() {
    let _ = STARTED_AT.set(Instant::now());
}

pub(crate) fn restore_saved_window_size(window: &WebviewWindow) {
    let Some(size) = read_saved_window_size() else {
        return;
    };

    let size = clamp_to_current_monitor(window, size);
    if size.width == DEFAULT_WIDTH && size.height == DEFAULT_HEIGHT {
        return;
    }

    match window.set_size(Size::Logical(LogicalSize::new(
        size.width as f64,
        size.height as f64,
    ))) {
        Ok(()) => log::info!(
            "Linux: restored saved window size {}x{}",
            size.width,
            size.height
        ),
        Err(err) => log::warn!("Linux: failed to restore saved window size: {err}"),
    }
}

/// Refresh native GTK/GNOME presentation after Tauri has shown the WebView.
///
/// Calling GTK `present()` instead of Tauri `show()` can leave the WebView
/// blank. Calling it after Tauri `show()` preserves native decorations and fixes
/// the GNOME/Wayland titlebar button hit-test initialization.
pub(crate) fn present_after_tauri_show(window: &WebviewWindow) {
    let presentation_window = window.clone();
    if let Err(err) = window.run_on_main_thread(move || {
        refresh_gtk_presentation(&presentation_window);
    }) {
        log::warn!("Linux: failed to schedule GTK presentation refresh: {err}");
    }
}

pub(crate) fn nudge_main_window(window: WebviewWindow) {
    let _ = window.set_focus();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REALIZE_WAIT).await;
        present_after_tauri_show(&window);
        let _ = window.set_focus();
        log::info!("Linux: retried main window focus");
    });
}

pub(crate) fn handle_window_event(event: &WindowEvent, window: &WebviewWindow) {
    if let WindowEvent::Resized(_) = event {
        save_window_size_after_resize(window);
    }
}

pub(crate) fn save_current_window_size_now(app_handle: &tauri::AppHandle) {
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    if let Err(err) = save_window_size_for_window(&window) {
        log::warn!("Linux: failed to save window size: {err}");
    }
}

fn save_window_size_after_resize(window: &WebviewWindow) {
    if STARTED_AT
        .get()
        .map(|started| started.elapsed() < STARTUP_RESIZE_IGNORE)
        .unwrap_or(true)
    {
        return;
    }

    if let Err(err) = save_window_size_for_window(window) {
        log::warn!("Linux: failed to save window size: {err}");
    }
}

fn save_window_size_for_window(window: &WebviewWindow) -> Result<(), String> {
    if !is_normal_visible_window(window) {
        return Ok(());
    }

    let size = gtk_content_size(window)?;
    if !is_valid_saved_size(size) {
        return Ok(());
    }

    write_saved_window_size(size)
}

fn refresh_gtk_presentation(window: &WebviewWindow) {
    match window.gtk_window() {
        Ok(gtk_window) => {
            use gtk::prelude::{GtkWindowExt, WidgetExt};
            gtk_window.show_all();
            gtk_window.present();
            log::info!("Linux: refreshed GTK presentation after Tauri show");
        }
        Err(err) => {
            log::warn!("Linux: failed to get GTK window for presentation refresh: {err}");
        }
    }
}

fn gtk_content_size(window: &WebviewWindow) -> Result<PhysicalSize<u32>, String> {
    let gtk_window = window
        .gtk_window()
        .map_err(|err| format!("failed to get GTK window: {err}"))?;

    use gtk::prelude::{BinExt, WidgetExt};

    let child = gtk_window
        .child()
        .ok_or_else(|| "GTK window has no content child".to_string())?;
    let allocation = child.allocation();
    let width = u32::try_from(allocation.width()).unwrap_or(0);
    let height = u32::try_from(allocation.height()).unwrap_or(0);

    Ok(PhysicalSize::new(width, height))
}

fn is_normal_visible_window(window: &WebviewWindow) -> bool {
    let visible = window.is_visible().unwrap_or(false);
    let maximized = window.is_maximized().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    visible && !maximized && !minimized
}

fn is_valid_saved_size(size: PhysicalSize<u32>) -> bool {
    (MIN_WIDTH..=MAX_SAVED_WIDTH).contains(&size.width)
        && (MIN_HEIGHT..=MAX_SAVED_HEIGHT).contains(&size.height)
}

fn read_saved_window_size() -> Option<PhysicalSize<u32>> {
    let path = window_size_state_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            log::warn!(
                "Linux: failed to read saved window size {}: {err}",
                path.display()
            );
            return None;
        }
    };

    let saved = match serde_json::from_str::<SavedWindowSize>(&raw) {
        Ok(saved) => saved,
        Err(err) => {
            log::warn!(
                "Linux: failed to parse saved window size {}: {err}",
                path.display()
            );
            return None;
        }
    };

    let size = PhysicalSize::new(saved.width, saved.height);
    is_valid_saved_size(size).then_some(size)
}

fn write_saved_window_size(size: PhysicalSize<u32>) -> Result<(), String> {
    let path = window_size_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let saved = SavedWindowSize {
        width: size.width,
        height: size.height,
    };
    let raw = serde_json::to_string_pretty(&saved)
        .map_err(|err| format!("failed to encode saved window size: {err}"))?;
    fs::write(&path, format!("{raw}\n"))
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;

    Ok(())
}

fn clamp_to_current_monitor(window: &WebviewWindow, size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    let monitor_size = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .map(|monitor| monitor.work_area().size);

    let Some(monitor_size) = monitor_size else {
        return size;
    };

    let max_width = monitor_size
        .width
        .saturating_sub(MONITOR_MARGIN)
        .max(MIN_WIDTH);
    let max_height = monitor_size
        .height
        .saturating_sub(MONITOR_MARGIN)
        .max(MIN_HEIGHT);

    PhysicalSize::new(
        size.width.clamp(MIN_WIDTH, max_width),
        size.height.clamp(MIN_HEIGHT, max_height),
    )
}

fn window_size_state_path() -> PathBuf {
    crate::config::get_app_config_dir().join(WINDOW_SIZE_STATE_FILE)
}
