use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use tauri::Manager;

const WINDOW_STATE_FILE: &str = ".window-state.json";
const MAX_WINDOW_DIMENSION: u32 = 32_768;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct WindowState {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalRect {
    x: i64,
    y: i64,
    width: u32,
    height: u32,
}

impl PhysicalRect {
    const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x: x as i64,
            y: y as i64,
            width,
            height,
        }
    }
}

fn state_path() -> Result<PathBuf, String> {
    crate::portable::paths()
        .map(|paths| paths.tauri_dir().join(WINDOW_STATE_FILE))
        .ok_or_else(|| "portable window state requested outside portable mode".to_string())
}

fn is_reasonable_size(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && width <= MAX_WINDOW_DIMENSION && height <= MAX_WINDOW_DIMENSION
}

fn rects_intersect(left: PhysicalRect, right: PhysicalRect) -> bool {
    let left_right = left.x + i64::from(left.width);
    let left_bottom = left.y + i64::from(left.height);
    let right_right = right.x + i64::from(right.width);
    let right_bottom = right.y + i64::from(right.height);

    left.x < right_right && left_right > right.x && left.y < right_bottom && left_bottom > right.y
}

pub fn save(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is not available".to_string())?;
    let size = window
        .inner_size()
        .map_err(|error| format!("read main window size: {error}"))?;
    let position = window
        .outer_position()
        .map_err(|error| format!("read main window position: {error}"))?;
    let maximized = window
        .is_maximized()
        .map_err(|error| format!("read main window maximized state: {error}"))?;
    let state = WindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
    };
    let json = serde_json::to_vec_pretty(&state)
        .map_err(|error| format!("serialize portable window state: {error}"))?;

    crate::config::atomic_write(&state_path()?, &json)
        .map_err(|error| format!("write portable window state: {error}"))
}

pub fn restore(app: &tauri::AppHandle) -> Result<(), String> {
    let path = state_path()?;
    let json = match fs::read(&path) {
        Ok(json) => json,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            log::debug!("Portable window state does not exist: {}", path.display());
            return Ok(());
        }
        Err(error) => {
            return Err(format!(
                "read portable window state {}: {error}",
                path.display()
            ));
        }
    };
    let state: WindowState = match serde_json::from_slice(&json) {
        Ok(state) => state,
        Err(error) => {
            log::warn!(
                "Ignoring invalid portable window state {}: {error}",
                path.display()
            );
            return Ok(());
        }
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is not available".to_string())?;

    if is_reasonable_size(state.width, state.height) {
        window
            .set_size(tauri::PhysicalSize::new(state.width, state.height))
            .map_err(|error| format!("restore main window size: {error}"))?;

        let saved_rect = PhysicalRect::new(state.x, state.y, state.width, state.height);
        let monitors = window
            .available_monitors()
            .map_err(|error| format!("enumerate monitors: {error}"))?;
        let intersects_monitor = monitors.iter().any(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            rects_intersect(
                saved_rect,
                PhysicalRect::new(position.x, position.y, size.width, size.height),
            )
        });

        if intersects_monitor {
            window
                .set_position(tauri::PhysicalPosition::new(state.x, state.y))
                .map_err(|error| format!("restore main window position: {error}"))?;
        }
    } else {
        log::warn!(
            "Ignoring invalid portable window size: {}x{}",
            state.width,
            state.height
        );
    }

    if state.maximized {
        window
            .maximize()
            .map_err(|error| format!("restore maximized window state: {error}"))?;
    } else {
        window
            .unmaximize()
            .map_err(|error| format!("restore unmaximized window state: {error}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_reasonable_size, rects_intersect, PhysicalRect, WindowState};

    #[test]
    fn window_state_json_roundtrip() {
        let state = WindowState {
            width: 1280,
            height: 720,
            x: -320,
            y: 48,
            maximized: true,
        };

        let json = serde_json::to_string(&state).expect("serialize window state");
        let restored: WindowState = serde_json::from_str(&json).expect("deserialize window state");

        assert_eq!(restored, state);
    }

    #[test]
    fn rejects_zero_and_unreasonably_large_sizes() {
        assert!(!is_reasonable_size(0, 720));
        assert!(!is_reasonable_size(1280, 0));
        assert!(!is_reasonable_size(u32::MAX, 720));
        assert!(!is_reasonable_size(1280, u32::MAX));
        assert!(is_reasonable_size(1280, 720));
    }

    #[test]
    fn rectangle_intersection_requires_positive_overlap() {
        let monitor = PhysicalRect::new(0, 0, 1920, 1080);

        assert!(rects_intersect(
            PhysicalRect::new(-200, 100, 400, 300),
            monitor
        ));
        assert!(!rects_intersect(
            PhysicalRect::new(1920, 100, 400, 300),
            monitor
        ));
        assert!(!rects_intersect(
            PhysicalRect::new(100, 1080, 400, 300),
            monitor
        ));
    }
}
