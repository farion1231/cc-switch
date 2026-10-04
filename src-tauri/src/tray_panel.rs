//! 托盘左键弹出的用量面板（macOS）：菜单栏图标下方的一块毛玻璃窗口，
//! 写 token 用量和各应用在用那家的额度。右键仍是原生菜单（切换供应商）。
//!
//! 窗口懒创建、之后只隐藏不销毁；失焦即隐藏，像系统的菜单栏弹出面板。

use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::store::AppState;
use crate::tray::TrayPanelApp;

#[cfg(target_os = "macos")]
pub use popup::toggle_panel;

pub const PANEL_LABEL: &str = "tray-panel";

const PANEL_WIDTH: f64 = 340.0;
const PANEL_MIN_HEIGHT: f64 = 160.0;
const PANEL_MAX_HEIGHT: f64 = 720.0;

fn panel_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL_LABEL)
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(window) = panel_window(app) {
        let _ = window.hide();
    }
}

/// 只有 macOS 托盘左键会弹出面板（Windows 左键开主界面，Linux 不派发点击事件）。
#[cfg(target_os = "macos")]
mod popup {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::window::{Effect, EffectState, EffectsBuilder};
    use tauri::{
        AppHandle, Emitter, PhysicalPosition, Rect, WebviewUrl, WebviewWindow,
        WebviewWindowBuilder, WindowEvent,
    };

    use super::{panel_window, PANEL_LABEL, PANEL_WIDTH};

    /// 前端每次弹出时据此重新拉数据。
    const PANEL_SHOWN_EVENT: &str = "tray-panel-shown";
    /// 面板顶边离菜单栏图标的距离（逻辑像素）。
    const PANEL_GAP: f64 = 6.0;
    /// 面板离屏幕左右边缘至少留这么多（逻辑像素）。
    const SCREEN_MARGIN: f64 = 8.0;

    /// 点托盘图标关面板时，失焦隐藏会先于点击事件到达；这段时间内的点击当作「关」，
    /// 否则面板会被刚隐藏又立刻弹出。
    const BLUR_CLICK_GRACE: Duration = Duration::from_millis(300);
    static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

    fn create_panel(app: &AppHandle) -> tauri::Result<WebviewWindow> {
        let window =
            WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("tray-panel.html".into()))
                .title("CC Switch")
                .inner_size(PANEL_WIDTH, 480.0)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible_on_all_workspaces(true)
                .visible(false)
                .focused(false)
                .shadow(true)
                .transparent(true)
                .effects(
                    EffectsBuilder::new()
                        .effect(Effect::Popover)
                        // 失焦也保持毛玻璃（虽然失焦就会隐藏，但避免弹出瞬间闪灰）
                        .state(EffectState::Active)
                        .radius(14.0)
                        .build(),
                )
                .build()?;

        let handle = window.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::Focused(false) if handle.is_visible().unwrap_or(false) => {
                *LAST_BLUR_HIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                let _ = handle.hide();
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = handle.hide();
            }
            _ => {}
        });
        Ok(window)
    }

    /// 面板放在图标正下方、水平居中，左右不出屏。`icon` 是托盘点击事件带的图标矩形。
    fn position_panel(app: &AppHandle, window: &WebviewWindow, icon: &Rect) {
        let scale = window.scale_factor().unwrap_or(1.0);
        let icon_pos = icon.position.to_physical::<f64>(scale);
        let icon_size = icon.size.to_physical::<f64>(scale);
        let center_x = icon_pos.x + icon_size.width / 2.0;

        let monitor = app
            .monitor_from_point(center_x, icon_pos.y + icon_size.height / 2.0)
            .ok()
            .flatten();
        let scale = monitor.as_ref().map_or(scale, |m| m.scale_factor());
        let width = PANEL_WIDTH * scale;
        let mut x = center_x - width / 2.0;
        if let Some(m) = &monitor {
            let left = m.position().x as f64 + SCREEN_MARGIN * scale;
            let right = (m.position().x + m.size().width as i32) as f64 - SCREEN_MARGIN * scale;
            x = x.min(right - width).max(left);
        }
        let y = icon_pos.y + icon_size.height + PANEL_GAP * scale;
        let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }

    /// 托盘左键：开着就关，关着就在图标下方弹出。
    pub fn toggle_panel(app: &AppHandle, icon: &Rect) {
        let window = match panel_window(app) {
            Some(w) => w,
            None => match create_panel(app) {
                Ok(w) => w,
                Err(e) => {
                    log::error!("[TrayPanel] 创建面板窗口失败: {e}");
                    return;
                }
            },
        };

        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }
        let just_blurred = LAST_BLUR_HIDE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .is_some_and(|at| at.elapsed() < BLUR_CLICK_GRACE);
        if just_blurred {
            return;
        }

        position_panel(app, &window, icon);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit_to(PANEL_LABEL, PANEL_SHOWN_EVENT, ());
    }
}

// ─── 命令 ──────────────────────────────────────────────────────────────────────

/// 读缓存里的额度快照，不发请求。
#[tauri::command]
pub fn get_tray_panel_apps(state: State<'_, AppState>) -> Vec<TrayPanelApp> {
    crate::tray::collect_panel_apps(&state)
}

/// 刷新各应用在用那家的额度（与托盘悬停共用 10 秒节流）后返回快照。
#[tauri::command]
pub async fn refresh_tray_panel_apps(app: AppHandle) -> Vec<TrayPanelApp> {
    crate::tray::refresh_all_usage_in_tray(&app).await;
    crate::tray::collect_panel_apps(&app.state::<AppState>())
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    hide_panel(&app);
}

/// 前端按内容高度回报，面板跟着内容伸缩（顶边不动）。
#[tauri::command]
pub fn tray_panel_set_height(app: AppHandle, height: f64) {
    if let Some(window) = panel_window(&app) {
        let height = height.clamp(PANEL_MIN_HEIGHT, PANEL_MAX_HEIGHT);
        let _ = window.set_size(tauri::LogicalSize::new(PANEL_WIDTH, height));
    }
}

#[tauri::command]
pub fn tray_panel_open_main(app: AppHandle) {
    hide_panel(&app);
    crate::tray::show_main_window(&app);
}
