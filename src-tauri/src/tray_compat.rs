//! Windows：兼容 MyDockFinder 这类接管了系统托盘的第三方宿主。
//!
//! tray-icon 收到托盘回调后要先用 `Shell_NotifyIconGetRect` 取图标矩形，取不到就把这次
//! 消息整个丢掉。第三方宿主转发了点击，却不支持这个查询，于是左右键都没有点击事件，
//! 面板和原生菜单都打不开；有的宿主右键还直接发 `WM_CONTEXTMENU`。
//!
//! 这里给托盘的隐藏窗口加一层子类：取不到矩形时，把光标位置当作图标补发一次点击，
//! 交给 `tray::handle_tray_icon_event`，和正常托盘走同一套面板 / 主界面逻辑；原生菜单模式
//! 下右键再自己弹出当前菜单。Explorer 下矩形取得到，消息原样交给 tray-icon。

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::menu::{ContextMenu, Menu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, PhysicalPosition, PhysicalSize, Rect, Wry};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, S_OK, WPARAM};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, SetWindowSubclass, Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, SetForegroundWindow, TrackPopupMenu, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    WM_CONTEXTMENU, WM_LBUTTONUP, WM_RBUTTONUP,
};

use crate::tray::TRAY_ID;

/// tray-icon 注册图标时用的回调消息（它内部的 `WM_USER_TRAYICON`）。
const TRAY_CALLBACK_MSG: u32 = 6002;
/// 这层子类的 id，和 muda 挂在同一窗口上的菜单子类错开。
const SUBCLASS_ID: usize = 0x4343_5357;
/// 一次右键可能既有回调又有 `WM_CONTEXTMENU`：这段时间内的第二条不再处理。
const RIGHT_CLICK_DEDUP: Duration = Duration::from_millis(300);

static APP: OnceLock<AppHandle> = OnceLock::new();
static LAST_RIGHT_CLICK: Mutex<Option<Instant>> = Mutex::new(None);
/// 当前挂在托盘上的原生菜单（`create_tray_menu` 每次重建都覆盖）。
static NATIVE_MENU: Mutex<Option<Menu<Wry>>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn remember_native_menu(menu: &Menu<Wry>) {
    *lock(&NATIVE_MENU) = Some(menu.clone());
}

/// 托盘建好后调用一次（主线程）。
pub fn install(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let hwnd = match tray.with_inner_tray_icon(|t| t.window_handle() as isize) {
        Ok(hwnd) => hwnd,
        Err(e) => {
            log::warn!("[TrayCompat] 取托盘窗口句柄失败: {e}");
            return;
        }
    };
    let _ = APP.set(app.clone());
    if unsafe { SetWindowSubclass(hwnd as HWND, Some(subclass_proc), SUBCLASS_ID, 0) } == 0 {
        log::warn!(
            "[TrayCompat] 挂托盘窗口子类失败: {}",
            std::io::Error::last_os_error()
        );
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // tray-icon 没调 NIM_SETVERSION，走旧协议：wParam 是图标 id，lParam 是鼠标消息。
        TRAY_CALLBACK_MSG if matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) => {
            let button = if lparam as u32 == WM_LBUTTONUP {
                MouseButton::Left
            } else {
                MouseButton::Right
            };
            if icon_rect_available(hwnd, wparam as u32) {
                if button == MouseButton::Right {
                    claim_right_click();
                }
            } else {
                if button == MouseButton::Left || claim_right_click() {
                    post_click(hwnd, button);
                }
                return 0;
            }
        }
        WM_CONTEXTMENU => {
            if claim_right_click() {
                post_click(hwnd, MouseButton::Right);
            }
            return 0;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn icon_rect_available(hwnd: HWND, id: u32) -> bool {
    let nid = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: hwnd,
        uID: id,
        ..unsafe { std::mem::zeroed() }
    };
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe { Shell_NotifyIconGetRect(&nid, &mut rect) == S_OK }
}

/// 记下这次右键；离上次不到 `RIGHT_CLICK_DEDUP` 就算同一次，返回 false。
fn claim_right_click() -> bool {
    let mut last = lock(&LAST_RIGHT_CLICK);
    let duplicate = last.is_some_and(|at| at.elapsed() < RIGHT_CLICK_DEDUP);
    *last = Some(Instant::now());
    !duplicate
}

/// 先挪出窗口过程，和 tray-icon 的点击事件一样回到事件循环里处理。
fn post_click(hwnd: HWND, button: MouseButton) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let mut cursor = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut cursor) };
    let (x, y) = (cursor.x, cursor.y);
    let hwnd = hwnd as isize;
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || click(&handle, hwnd, button, x, y));
    });
}

fn click(app: &AppHandle, hwnd: isize, button: MouseButton, x: i32, y: i32) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    // 拿不到图标矩形：把光标处当成图标，面板弹在光标旁边。
    let rect = Rect {
        position: PhysicalPosition::new(x, y).into(),
        size: PhysicalSize::new(1u32, 1u32).into(),
    };
    crate::tray::handle_tray_icon_event(
        &tray,
        TrayIconEvent::Click {
            id: tray.id().clone(),
            position: PhysicalPosition::new(x as f64, y as f64),
            rect,
            button,
            button_state: MouseButtonState::Up,
        },
    );
    if button == MouseButton::Right && crate::tray_panel::uses_native_menu() {
        show_native_menu(hwnd, x, y);
    }
}

/// 和 tray-icon 弹托盘菜单一样：托盘窗口上挂着 muda 的子类，菜单项点击照常派发。
fn show_native_menu(hwnd: isize, x: i32, y: i32) {
    let menu = lock(&NATIVE_MENU).clone();
    let Some(hmenu) = menu.and_then(|m| m.hpopupmenu().ok()) else {
        return;
    };
    unsafe {
        SetForegroundWindow(hwnd as HWND);
        TrackPopupMenu(
            hmenu as _,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            x,
            y,
            0,
            hwnd as HWND,
            std::ptr::null(),
        );
    }
}
