// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

mod icon;
mod util;
use std::ptr;

use once_cell::sync::Lazy;
use windows_sys::{
    s,
    Win32::{
        Foundation::{FALSE, HWND, LPARAM, LRESULT, POINT, RECT, S_OK, TRUE, WPARAM},
        UI::{
            Shell::{
                Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP,
                NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
            },
            WindowsAndMessaging::{
                ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
                GetCursorPos, KillTimer, RegisterClassW, RegisterWindowMessageA, SendMessageW,
                SetForegroundWindow, SetTimer, TrackPopupMenu, CREATESTRUCTW, CW_USEDEFAULT,
                GWL_USERDATA, HICON, HMENU, MSGFLT_ALLOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
                WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
                WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_OVERLAPPED,
            },
        },
    },
};

use crate::{
    dpi::PhysicalPosition, icon::Icon, menu, MouseButton, MouseButtonState, Rect,
    TrayIconAttributes, TrayIconEvent, TrayIconId, COUNTER,
};

pub(crate) use self::icon::WinIcon as PlatformIcon;

const WM_USER_TRAYICON: u32 = 6002;
const WM_USER_UPDATE_TRAYMENU: u32 = 6003;
const WM_USER_UPDATE_TRAYICON: u32 = 6004;
const WM_USER_SHOW_TRAYICON: u32 = 6005;
const WM_USER_HIDE_TRAYICON: u32 = 6006;
const WM_USER_UPDATE_TRAYTOOLTIP: u32 = 6007;
const WM_USER_LEAVE_TIMER_ID: u32 = 6008;
const WM_USER_SHOW_MENU_ON_LEFT_CLICK: u32 = 6009;
/// When the taskbar is created, it registers a message with the "TaskbarCreated" string and then broadcasts this message to all top-level windows
/// When the application receives this message, it should assume that any taskbar icons it added have been removed and add them again.
static S_U_TASKBAR_RESTART: Lazy<u32> =
    Lazy::new(|| unsafe { RegisterWindowMessageA(s!("TaskbarCreated")) });

struct TrayUserData {
    internal_id: u32,
    id: TrayIconId,
    hwnd: HWND,
    hpopupmenu: Option<HMENU>,
    icon: Option<Icon>,
    tooltip: Option<String>,
    entered: bool,
    last_position: Option<PhysicalPosition<f64>>,
    menu_on_left_click: bool,
}

pub struct TrayIcon {
    hwnd: HWND,
    menu: Option<Box<dyn menu::ContextMenu>>,
    internal_id: u32,
}

impl TrayIcon {
    pub fn new(id: TrayIconId, attrs: TrayIconAttributes) -> crate::Result<Self> {
        let internal_id = COUNTER.next();

        let class_name = util::encode_wide("tray_icon_app");
        unsafe {
            let hinstance = util::get_instance_handle();

            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(tray_proc),
                lpszClassName: class_name.as_ptr(),
                hInstance: hinstance,
                ..std::mem::zeroed()
            };

            RegisterClassW(&wnd_class);

            let traydata = TrayUserData {
                id,
                internal_id,
                hwnd: std::ptr::null_mut(),
                hpopupmenu: attrs.menu.as_ref().map(|m| m.hpopupmenu() as _),
                icon: attrs.icon.clone(),
                tooltip: attrs.tooltip.clone(),
                entered: false,
                last_position: None,
                menu_on_left_click: attrs.menu_on_left_click,
            };

            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED |
            // WS_EX_TOOLWINDOW prevents this window from ever showing up in the taskbar, which
            // we want to avoid. If you remove this style, this window won't show up in the
            // taskbar *initially*, but it can show up at some later point. This can sometimes
            // happen on its own after several hours have passed, although this has proven
            // difficult to reproduce. Alternatively, it can be manually triggered by killing
            // `explorer.exe` and then starting the process back up.
            // It is unclear why the bug is triggered by waiting for several hours.
            WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                ptr::null(),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                0,
                CW_USEDEFAULT,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                Box::into_raw(Box::new(traydata)) as _,
            );
            if hwnd.is_null() {
                return Err(crate::Error::OsError(std::io::Error::last_os_error()));
            }

            // Allow "TaskbarCreated" through UIPI so elevated apps can re-register on explorer restart.
            ChangeWindowMessageFilterEx(hwnd, *S_U_TASKBAR_RESTART, MSGFLT_ALLOW, ptr::null_mut());

            let hicon = attrs.icon.as_ref().map(|i| i.inner.as_raw_handle());

            if !register_tray_icon(hwnd, internal_id, &hicon, &attrs.tooltip) {
                // Explorer/taskbar may not be ready yet (e.g., app starts before explorer.exe).
                // Keep the window alive and wait for TaskbarCreated to re-register.
            }

            if let Some(menu) = &attrs.menu {
                menu.attach_menu_subclass_for_hwnd(hwnd as _);
            }

            Ok(Self {
                hwnd,
                internal_id,
                menu: attrs.menu,
            })
        }
    }

    pub fn set_icon(&mut self, icon: Option<Icon>) -> crate::Result<()> {
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                uFlags: NIF_ICON,
                hWnd: self.hwnd,
                uID: self.internal_id,
                ..std::mem::zeroed()
            };

            if let Some(hicon) = icon.as_ref().map(|i| i.inner.as_raw_handle()) {
                nid.hIcon = hicon;
            }

            if Shell_NotifyIconW(NIM_MODIFY, &mut nid as _) == 0 {
                return Err(crate::Error::OsError(std::io::Error::last_os_error()));
            }

            // send the new icon to the subclass proc to store it in the tray data
            SendMessageW(
                self.hwnd,
                WM_USER_UPDATE_TRAYICON,
                Box::into_raw(Box::new(icon)) as _,
                0,
            );
        }

        Ok(())
    }

    pub fn set_menu(&mut self, menu: Option<Box<dyn menu::ContextMenu>>) {
        // Safety: self.hwnd is valid as long as as the TrayIcon is
        if let Some(menu) = &self.menu {
            unsafe { menu.detach_menu_subclass_from_hwnd(self.hwnd as _) };
        }
        if let Some(menu) = &menu {
            unsafe { menu.attach_menu_subclass_for_hwnd(self.hwnd as _) };
        }

        unsafe {
            // send the new menu to the subclass proc where we will update there
            SendMessageW(
                self.hwnd,
                WM_USER_UPDATE_TRAYMENU,
                Box::into_raw(Box::new(menu.as_ref().map(|m| m.hpopupmenu()))) as _,
                0,
            );
        }

        self.menu = menu;
    }

    pub fn set_tooltip<S: AsRef<str>>(&mut self, tooltip: Option<S>) -> crate::Result<()> {
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                uFlags: NIF_TIP,
                hWnd: self.hwnd,
                uID: self.internal_id,
                ..std::mem::zeroed()
            };
            if let Some(tooltip) = &tooltip {
                let tip = util::encode_wide(tooltip.as_ref());
                #[allow(clippy::manual_memcpy)]
                for i in 0..tip.len().min(128) {
                    nid.szTip[i] = tip[i];
                }
            }

            if Shell_NotifyIconW(NIM_MODIFY, &mut nid as _) == 0 {
                return Err(crate::Error::OsError(std::io::Error::last_os_error()));
            }

            // send the new tooltip to the subclass proc to store it in the tray data
            SendMessageW(
                self.hwnd,
                WM_USER_UPDATE_TRAYTOOLTIP,
                Box::into_raw(Box::new(tooltip.map(|t| t.as_ref().to_string()))) as _,
                0,
            );
        }

        Ok(())
    }

    pub fn set_show_menu_on_left_click(&mut self, enable: bool) {
        unsafe {
            SendMessageW(
                self.hwnd,
                WM_USER_SHOW_MENU_ON_LEFT_CLICK,
                enable as usize,
                0,
            );
        }
    }

    pub fn set_title<S: AsRef<str>>(&mut self, _title: Option<S>) {}

    pub fn set_visible(&mut self, visible: bool) -> crate::Result<()> {
        unsafe {
            SendMessageW(
                self.hwnd,
                if visible {
                    WM_USER_SHOW_TRAYICON
                } else {
                    WM_USER_HIDE_TRAYICON
                },
                0,
                0,
            );
        }

        Ok(())
    }

    pub fn rect(&self) -> Option<Rect> {
        get_tray_rect(self.internal_id, self.hwnd).map(Into::into)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            remove_tray_icon(self.hwnd, self.internal_id);

            if let Some(menu) = &self.menu {
                menu.detach_menu_subclass_from_hwnd(self.hwnd as _);
            }

            // destroy the hidden window used by the tray
            DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let userdata_ptr = unsafe { util::get_window_long(hwnd, GWL_USERDATA) };
    let userdata_ptr = match (userdata_ptr, msg) {
        (0, WM_NCCREATE) => {
            let createstruct = unsafe { &mut *(lparam as *mut CREATESTRUCTW) };
            let userdata = unsafe { &mut *(createstruct.lpCreateParams as *mut TrayUserData) };
            userdata.hwnd = hwnd;
            util::set_window_long(hwnd, GWL_USERDATA, createstruct.lpCreateParams as _);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        // Getting here should quite frankly be impossible,
        // but we'll make window creation fail here just in case.
        (0, WM_CREATE) => return -1,
        (_, WM_CREATE) => return DefWindowProcW(hwnd, msg, wparam, lparam),
        (0, _) => return DefWindowProcW(hwnd, msg, wparam, lparam),
        _ => userdata_ptr as *mut TrayUserData,
    };

    let userdata = &mut *(userdata_ptr);

    match msg {
        WM_DESTROY => {
            drop(Box::from_raw(userdata_ptr));
            return 0;
        }
        WM_USER_UPDATE_TRAYMENU => {
            let hpopupmenu = Box::from_raw(wparam as *mut Option<isize>);
            userdata.hpopupmenu = (*hpopupmenu).map(|h| h as *mut _);
        }
        WM_USER_UPDATE_TRAYICON => {
            let icon = Box::from_raw(wparam as *mut Option<Icon>);
            userdata.icon = *icon;
        }
        WM_USER_SHOW_TRAYICON => {
            register_tray_icon(
                userdata.hwnd,
                userdata.internal_id,
                &userdata.icon.as_ref().map(|i| i.inner.as_raw_handle()),
                &userdata.tooltip,
            );
        }
        WM_USER_HIDE_TRAYICON => {
            remove_tray_icon(userdata.hwnd, userdata.internal_id);
        }
        WM_USER_UPDATE_TRAYTOOLTIP => {
            let tooltip = Box::from_raw(wparam as *mut Option<String>);
            userdata.tooltip = *tooltip;
        }
        _ if msg == *S_U_TASKBAR_RESTART => {
            register_tray_icon(
                userdata.hwnd,
                userdata.internal_id,
                &userdata.icon.as_ref().map(|i| i.inner.as_raw_handle()),
                &userdata.tooltip,
            );
        }
        WM_USER_SHOW_MENU_ON_LEFT_CLICK => {
            userdata.menu_on_left_click = wparam != 0;
        }

        WM_USER_TRAYICON
            if matches!(
                lparam as u32,
                WM_LBUTTONDOWN
                    | WM_RBUTTONDOWN
                    | WM_MBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_RBUTTONUP
                    | WM_MBUTTONUP
                    | WM_LBUTTONDBLCLK
                    | WM_RBUTTONDBLCLK
                    | WM_MBUTTONDBLCLK
                    | WM_MOUSEMOVE
            ) =>
        {
            let mut cursor = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut cursor as _) == 0 {
                return 0;
            }

            let id = userdata.id.clone();
            let position = PhysicalPosition::new(cursor.x as f64, cursor.y as f64);

            let rect = match get_tray_rect(userdata.internal_id, hwnd) {
                Some(rect) => Rect::from(rect),
                None => return 0,
            };

            let event = match lparam as u32 {
                WM_LBUTTONDOWN => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Down,
                },
                WM_RBUTTONDOWN => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Down,
                },
                WM_MBUTTONDOWN => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Middle,
                    button_state: MouseButtonState::Down,
                },
                WM_LBUTTONUP => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                },
                WM_RBUTTONUP => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                },
                WM_MBUTTONUP => TrayIconEvent::Click {
                    id,
                    rect,
                    position,
                    button: MouseButton::Middle,
                    button_state: MouseButtonState::Up,
                },
                WM_LBUTTONDBLCLK => TrayIconEvent::DoubleClick {
                    id,
                    rect,
                    position,
                    button: MouseButton::Left,
                },
                WM_RBUTTONDBLCLK => TrayIconEvent::DoubleClick {
                    id,
                    rect,
                    position,
                    button: MouseButton::Right,
                },
                WM_MBUTTONDBLCLK => TrayIconEvent::DoubleClick {
                    id,
                    rect,
                    position,
                    button: MouseButton::Middle,
                },
                WM_MOUSEMOVE if !userdata.entered => {
                    userdata.entered = true;
                    TrayIconEvent::Enter { id, rect, position }
                }
                WM_MOUSEMOVE if userdata.entered => {
                    // handle extra WM_MOUSEMOVE events, ignore if position hasn't changed
                    let cursor_moved = userdata.last_position != Some(position);
                    userdata.last_position = Some(position);
                    if cursor_moved {
                        // Set or update existing timer, where we check if cursor left
                        SetTimer(hwnd, WM_USER_LEAVE_TIMER_ID as _, 15, Some(tray_timer_proc));

                        TrayIconEvent::Move { id, rect, position }
                    } else {
                        return 0;
                    }
                }

                _ => unreachable!(),
            };

            TrayIconEvent::send(event);

            if lparam as u32 == WM_RBUTTONDOWN
                || (userdata.menu_on_left_click && lparam as u32 == WM_LBUTTONDOWN)
            {
                if let Some(menu) = userdata.hpopupmenu {
                    show_tray_menu(hwnd, menu, cursor.x, cursor.y);
                }
            }
        }

        WM_TIMER if wparam as u32 == WM_USER_LEAVE_TIMER_ID => {
            if let Some(position) = userdata.last_position.take() {
                let mut cursor = POINT { x: 0, y: 0 };
                if GetCursorPos(&mut cursor as _) == 0 {
                    return 0;
                }

                let rect = match get_tray_rect(userdata.internal_id, hwnd) {
                    Some(r) => r,
                    None => return 0,
                };

                let in_x = (rect.left..rect.right).contains(&cursor.x);
                let in_y = (rect.top..rect.bottom).contains(&cursor.y);

                if !in_x || !in_y {
                    KillTimer(hwnd, WM_USER_LEAVE_TIMER_ID as _);
                    userdata.entered = false;

                    TrayIconEvent::send(TrayIconEvent::Leave {
                        id: userdata.id.clone(),
                        rect: rect.into(),
                        position,
                    });
                }
            }

            return 0;
        }

        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn tray_timer_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: u32) {
    tray_proc(hwnd, msg, wparam, lparam as _);
}

#[inline]
unsafe fn show_tray_menu(hwnd: HWND, menu: HMENU, x: i32, y: i32) {
    // bring the hidden window to the foreground so the pop up menu
    // would automatically hide on click outside
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        menu,
        // align bottom / right, maybe we could expose this later..
        TPM_BOTTOMALIGN | TPM_LEFTALIGN,
        x,
        y,
        0,
        hwnd,
        std::ptr::null_mut(),
    );
}

#[inline]
unsafe fn register_tray_icon(
    hwnd: HWND,
    tray_id: u32,
    hicon: &Option<HICON>,
    tooltip: &Option<String>,
) -> bool {
    let mut h_icon = std::ptr::null_mut();
    let mut flags = NIF_MESSAGE;
    let mut sz_tip: [u16; 128] = [0; 128];

    if let Some(hicon) = hicon {
        flags |= NIF_ICON;
        h_icon = *hicon;
    }

    if let Some(tooltip) = tooltip {
        flags |= NIF_TIP;
        let tip = util::encode_wide(tooltip);
        #[allow(clippy::manual_memcpy)]
        for i in 0..tip.len().min(128) {
            sz_tip[i] = tip[i];
        }
    }

    let mut nid = NOTIFYICONDATAW {
        uFlags: flags,
        hWnd: hwnd,
        uID: tray_id,
        uCallbackMessage: WM_USER_TRAYICON,
        hIcon: h_icon,
        szTip: sz_tip,
        ..std::mem::zeroed()
    };

    Shell_NotifyIconW(NIM_ADD, &mut nid as _) == TRUE
}

#[inline]
unsafe fn remove_tray_icon(hwnd: HWND, id: u32) {
    let mut nid = NOTIFYICONDATAW {
        uFlags: NIF_ICON,
        hWnd: hwnd,
        uID: id,
        ..std::mem::zeroed()
    };

    if Shell_NotifyIconW(NIM_DELETE, &mut nid as _) == FALSE {
        eprintln!("Error removing system tray icon");
    }
}

#[inline]
fn get_tray_rect(id: u32, hwnd: HWND) -> Option<RECT> {
    let nid = NOTIFYICONIDENTIFIER {
        hWnd: hwnd,
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as _,
        uID: id,
        ..unsafe { std::mem::zeroed() }
    };

    let mut rect = RECT {
        left: 0,
        bottom: 0,
        right: 0,
        top: 0,
    };
    if unsafe { Shell_NotifyIconGetRect(&nid, &mut rect) } == S_OK {
        Some(rect)
    } else {
        None
    }
}

impl From<RECT> for Rect {
    fn from(rect: RECT) -> Self {
        Self {
            position: crate::dpi::PhysicalPosition::new(rect.left.into(), rect.top.into()),
            size: crate::dpi::PhysicalSize::new(
                rect.right.saturating_sub(rect.left) as u32,
                rect.bottom.saturating_sub(rect.top) as u32,
            ),
        }
    }
}
