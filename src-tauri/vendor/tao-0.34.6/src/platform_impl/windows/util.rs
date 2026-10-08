// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  io,
  iter::once,
  mem,
  ops::BitAnd,
  os::windows::prelude::OsStrExt,
  slice,
  sync::atomic::{AtomicBool, Ordering},
};

use crate::{
  dpi::{PhysicalPosition, PhysicalSize},
  window::CursorIcon,
};

use once_cell::sync::Lazy;
use windows::{
  core::{BOOL, HRESULT, PCSTR, PCWSTR},
  Win32::{
    Foundation::{COLORREF, FARPROC, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Globalization::lstrlenW,
    Graphics::Gdi::{ClientToScreen, InvalidateRgn, HMONITOR},
    System::LibraryLoader::*,
    UI::{
      HiDpi::*,
      Input::KeyboardAndMouse::*,
      WindowsAndMessaging::{self as win32wm, *},
    },
  },
};

pub fn has_flag<T>(bitset: T, flag: T) -> bool
where
  T: Copy + PartialEq + BitAnd<T, Output = T>,
{
  bitset & flag == flag
}

pub fn wchar_to_string(wchar: &[u16]) -> String {
  String::from_utf16_lossy(wchar)
}

pub fn wchar_ptr_to_string(wchar: PCWSTR) -> String {
  let len = unsafe { lstrlenW(wchar) } as usize;
  let wchar_slice = unsafe { slice::from_raw_parts(wchar.0, len) };
  wchar_to_string(wchar_slice)
}

pub fn encode_wide(string: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
  string.as_ref().encode_wide().chain(once(0)).collect()
}

fn win_to_err<F: FnOnce() -> BOOL>(f: F) -> Result<(), io::Error> {
  if f().as_bool() {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

pub unsafe fn get_window_rect(hwnd: HWND) -> Option<RECT> {
  let mut rect = std::mem::zeroed();
  GetWindowRect(hwnd, &mut rect).ok().map(|_| rect)
}

pub fn get_client_rect(hwnd: HWND) -> Result<RECT, io::Error> {
  let mut rect = RECT::default();
  let mut top_left = POINT::default();

  unsafe {
    win_to_err(|| ClientToScreen(hwnd, &mut top_left))?;
    GetClientRect(hwnd, &mut rect)?;
  }

  rect.left += top_left.x;
  rect.top += top_left.y;
  rect.right += top_left.x;
  rect.bottom += top_left.y;

  Ok(rect)
}

pub fn adjust_size(hwnd: HWND, size: PhysicalSize<u32>, is_decorated: bool) -> PhysicalSize<u32> {
  let (width, height): (u32, u32) = size.into();
  let rect = RECT {
    left: 0,
    right: width as i32,
    top: 0,
    bottom: height as i32,
  };
  let rect = adjust_window_rect(hwnd, rect, is_decorated).unwrap_or(rect);
  PhysicalSize::new((rect.right - rect.left) as _, (rect.bottom - rect.top) as _)
}

pub(crate) fn set_inner_size_physical(window: HWND, x: i32, y: i32, is_decorated: bool) {
  unsafe {
    let rect = adjust_window_rect(
      window,
      RECT {
        top: 0,
        left: 0,
        bottom: y,
        right: x,
      },
      is_decorated,
    )
    .expect("adjust_window_rect failed");

    let outer_x = (rect.right - rect.left).abs();
    let outer_y = (rect.top - rect.bottom).abs();
    let _ = SetWindowPos(
      window,
      None,
      0,
      0,
      outer_x,
      outer_y,
      SWP_ASYNCWINDOWPOS | SWP_NOZORDER | SWP_NOREPOSITION | SWP_NOMOVE | SWP_NOACTIVATE,
    );
    let _ = InvalidateRgn(window, None, false);
  }
}

pub fn adjust_window_rect(hwnd: HWND, rect: RECT, is_decorated: bool) -> Option<RECT> {
  unsafe {
    let mut style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
    // if the window isn't decorated, remove `WS_SIZEBOX` and `WS_CAPTION` so
    // `AdjustWindowRect*` functions doesn't account for the hidden caption and borders and
    // calculates a correct size for the client area.
    if !is_decorated {
      style &= !WS_CAPTION;
      style &= !WS_SIZEBOX;
    }
    let style_ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
    adjust_window_rect_with_styles(hwnd, style, style_ex, rect)
  }
}

pub fn adjust_window_rect_with_styles(
  hwnd: HWND,
  style: WINDOW_STYLE,
  style_ex: WINDOW_EX_STYLE,
  mut rect: RECT,
) -> Option<RECT> {
  let b_menu = !unsafe { GetMenu(hwnd) }.is_invalid();

  if let (Some(get_dpi_for_window), Some(adjust_window_rect_ex_for_dpi)) =
    (*GET_DPI_FOR_WINDOW, *ADJUST_WINDOW_RECT_EX_FOR_DPI)
  {
    let dpi = unsafe { get_dpi_for_window(hwnd) };
    if unsafe { adjust_window_rect_ex_for_dpi(&mut rect, style, b_menu.into(), style_ex, dpi) }
      .as_bool()
    {
      Some(rect)
    } else {
      None
    }
  } else {
    unsafe { AdjustWindowRectEx(&mut rect, style, b_menu, style_ex) }
      .ok()
      .map(|_| rect)
  }
}

pub fn set_cursor_hidden(hidden: bool) {
  static HIDDEN: AtomicBool = AtomicBool::new(false);
  let changed = HIDDEN.swap(hidden, Ordering::SeqCst) ^ hidden;
  if changed {
    unsafe { ShowCursor(!hidden) };
  }
}

pub fn get_cursor_clip() -> windows::core::Result<RECT> {
  unsafe {
    let mut rect = RECT::default();
    GetClipCursor(&mut rect).map(|_| rect)
  }
}

/// Sets the cursor's clip rect.
///
/// Note that calling this will automatically dispatch a `WM_MOUSEMOVE` event.
pub fn set_cursor_clip(rect: Option<RECT>) -> windows::core::Result<()> {
  unsafe {
    let rect_ptr = rect.as_ref().map(|r| r as *const RECT);
    ClipCursor(rect_ptr)
  }
}

pub fn get_desktop_rect() -> RECT {
  unsafe {
    let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
    RECT {
      left,
      top,
      right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
      bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
    }
  }
}

pub fn is_focused(window: HWND) -> bool {
  window == unsafe { GetActiveWindow() }
}

pub fn is_visible(window: HWND) -> bool {
  unsafe { IsWindowVisible(window).as_bool() }
}

pub fn is_maximized(window: HWND) -> windows::core::Result<bool> {
  let mut placement = WINDOWPLACEMENT {
    length: mem::size_of::<WINDOWPLACEMENT>() as u32,
    ..WINDOWPLACEMENT::default()
  };
  unsafe { GetWindowPlacement(window, &mut placement)? };
  Ok(placement.showCmd == SW_MAXIMIZE.0 as u32)
}

pub fn cursor_position() -> windows::core::Result<PhysicalPosition<f64>> {
  let mut pt = POINT { x: 0, y: 0 };
  unsafe { GetCursorPos(&mut pt)? };
  Ok((pt.x, pt.y).into())
}

impl CursorIcon {
  pub(crate) fn to_windows_cursor(self) -> PCWSTR {
    match self {
      CursorIcon::Arrow | CursorIcon::Default => IDC_ARROW,
      CursorIcon::Hand => IDC_HAND,
      CursorIcon::Crosshair => IDC_CROSS,
      CursorIcon::Text | CursorIcon::VerticalText => IDC_IBEAM,
      CursorIcon::NotAllowed | CursorIcon::NoDrop => IDC_NO,
      CursorIcon::Grab | CursorIcon::Grabbing | CursorIcon::Move | CursorIcon::AllScroll => {
        IDC_SIZEALL
      }
      CursorIcon::EResize | CursorIcon::WResize | CursorIcon::EwResize | CursorIcon::ColResize => {
        IDC_SIZEWE
      }
      CursorIcon::NResize | CursorIcon::SResize | CursorIcon::NsResize | CursorIcon::RowResize => {
        IDC_SIZENS
      }
      CursorIcon::NeResize | CursorIcon::SwResize | CursorIcon::NeswResize => IDC_SIZENESW,
      CursorIcon::NwResize | CursorIcon::SeResize | CursorIcon::NwseResize => IDC_SIZENWSE,
      CursorIcon::Wait => IDC_WAIT,
      CursorIcon::Progress => IDC_APPSTARTING,
      CursorIcon::Help => IDC_HELP,
      _ => IDC_ARROW, // use arrow for the missing cases.
    }
  }
}

// Helper function to dynamically load function pointer.
// `library` and `function` must be zero-terminated.
pub(super) fn get_function_impl(library: &str, function: &str) -> FARPROC {
  let library = encode_wide(library);
  assert_eq!(function.chars().last(), Some('\0'));

  // Library names we will use are ASCII so we can use the A version to avoid string conversion.
  let module = unsafe { LoadLibraryW(PCWSTR::from_raw(library.as_ptr())) }.unwrap_or_default();
  if module.is_invalid() {
    return None;
  }

  unsafe { GetProcAddress(module, PCSTR::from_raw(function.as_ptr())) }
}

macro_rules! get_function {
  ($lib:expr, $func:ident) => {
    crate::platform_impl::platform::util::get_function_impl($lib, concat!(stringify!($func), '\0'))
      .map(|f| unsafe { std::mem::transmute::<_, $func>(f) })
  };
}

pub type SetProcessDPIAware = unsafe extern "system" fn() -> BOOL;
pub type SetProcessDpiAwareness =
  unsafe extern "system" fn(value: PROCESS_DPI_AWARENESS) -> HRESULT;
pub type SetProcessDpiAwarenessContext =
  unsafe extern "system" fn(value: DPI_AWARENESS_CONTEXT) -> BOOL;
pub type GetDpiForWindow = unsafe extern "system" fn(hwnd: HWND) -> u32;
pub type GetDpiForMonitor = unsafe extern "system" fn(
  hmonitor: HMONITOR,
  dpi_type: MONITOR_DPI_TYPE,
  dpi_x: *mut u32,
  dpi_y: *mut u32,
) -> HRESULT;
type GetSystemMetricsForDpi =
  unsafe extern "system" fn(nindex: SYSTEM_METRICS_INDEX, dpi: u32) -> i32;
pub type EnableNonClientDpiScaling = unsafe extern "system" fn(hwnd: HWND) -> BOOL;
#[allow(non_snake_case)]
pub type AdjustWindowRectExForDpi = unsafe extern "system" fn(
  rect: *mut RECT,
  dwStyle: WINDOW_STYLE,
  bMenu: BOOL,
  dwExStyle: WINDOW_EX_STYLE,
  dpi: u32,
) -> BOOL;

pub static GET_DPI_FOR_WINDOW: Lazy<Option<GetDpiForWindow>> =
  Lazy::new(|| get_function!("user32.dll", GetDpiForWindow));
pub static ADJUST_WINDOW_RECT_EX_FOR_DPI: Lazy<Option<AdjustWindowRectExForDpi>> =
  Lazy::new(|| get_function!("user32.dll", AdjustWindowRectExForDpi));
pub static GET_DPI_FOR_MONITOR: Lazy<Option<GetDpiForMonitor>> =
  Lazy::new(|| get_function!("shcore.dll", GetDpiForMonitor));
pub static GET_SYSTEM_METRICS_FOR_DPI: Lazy<Option<GetSystemMetricsForDpi>> =
  Lazy::new(|| get_function!("user32.dll", GetSystemMetricsForDpi));
pub static ENABLE_NON_CLIENT_DPI_SCALING: Lazy<Option<EnableNonClientDpiScaling>> =
  Lazy::new(|| get_function!("user32.dll", EnableNonClientDpiScaling));
pub static SET_PROCESS_DPI_AWARENESS_CONTEXT: Lazy<Option<SetProcessDpiAwarenessContext>> =
  Lazy::new(|| get_function!("user32.dll", SetProcessDpiAwarenessContext));
pub static SET_PROCESS_DPI_AWARENESS: Lazy<Option<SetProcessDpiAwareness>> =
  Lazy::new(|| get_function!("shcore.dll", SetProcessDpiAwareness));
pub static SET_PROCESS_DPI_AWARE: Lazy<Option<SetProcessDPIAware>> =
  Lazy::new(|| get_function!("user32.dll", SetProcessDPIAware));

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "32")]
pub fn SetWindowLongPtrW(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
  unsafe { win32wm::SetWindowLongW(window, index, value as _) as _ }
}

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "64")]
pub fn SetWindowLongPtrW(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
  unsafe { win32wm::SetWindowLongPtrW(window, index, value) }
}

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "32")]
pub fn GetWindowLongPtrW(window: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
  unsafe { win32wm::GetWindowLongW(window, index) as _ }
}

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "64")]
pub fn GetWindowLongPtrW(window: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
  unsafe { win32wm::GetWindowLongPtrW(window, index) }
}

/// Implementation of the `LOWORD` macro.
#[allow(non_snake_case)]
#[inline]
pub fn LOWORD(dword: u32) -> u16 {
  (dword & 0xFFFF) as u16
}

/// Implementation of the `HIWORD` macro.
#[allow(non_snake_case)]
#[inline]
pub fn HIWORD(dword: u32) -> u16 {
  ((dword & 0xFFFF_0000) >> 16) as u16
}

/// Implementation of the `GET_X_LPARAM` macro.
#[allow(non_snake_case)]
#[inline]
pub fn GET_X_LPARAM(lparam: LPARAM) -> i16 {
  ((lparam.0 as usize) & 0xFFFF) as u16 as i16
}

/// Implementation of the `GET_Y_LPARAM` macro.
#[allow(non_snake_case)]
#[inline]
pub fn GET_Y_LPARAM(lparam: LPARAM) -> i16 {
  (((lparam.0 as usize) & 0xFFFF_0000) >> 16) as u16 as i16
}

/// Implementation of the `MAKELPARAM` macro.
/// Inverse of [GET_X_LPARAM] and [GET_Y_LPARAM] to put the (`x`, `y`) signed
/// coordinates/values back into an [LPARAM].
#[allow(non_snake_case)]
#[inline]
pub fn MAKELPARAM(x: i16, y: i16) -> LPARAM {
  LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as usize as _)
}

/// Implementation of the `GET_WHEEL_DELTA_WPARAM` macro.
#[allow(non_snake_case)]
#[inline]
pub fn GET_WHEEL_DELTA_WPARAM(wparam: WPARAM) -> i16 {
  ((wparam.0 & 0xFFFF_0000) >> 16) as u16 as i16
}

/// Implementation of the `GET_XBUTTON_WPARAM` macro.
#[allow(non_snake_case)]
#[inline]
pub fn GET_XBUTTON_WPARAM(wparam: WPARAM) -> u16 {
  ((wparam.0 & 0xFFFF_0000) >> 16) as u16
}

/// Implementation of the `PRIMARYLANGID` macro.
#[allow(non_snake_case)]
#[inline]
pub fn PRIMARYLANGID(hkl: HKL) -> u32 {
  ((hkl.0 as usize) & 0x3FF) as u32
}

/// Implementation of the `RGB` macro.
#[allow(non_snake_case)]
#[inline]
pub fn RGB<T: Into<u32>>(r: T, g: T, b: T) -> COLORREF {
  COLORREF(r.into() | (g.into() << 8) | (b.into() << 16))
}

pub unsafe extern "system" fn call_default_window_proc(
  hwnd: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
) -> LRESULT {
  DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn get_instance_handle() -> windows::Win32::Foundation::HMODULE {
  // Gets the instance handle by taking the address of the
  // pseudo-variable created by the microsoft linker:
  // https://devblogs.microsoft.com/oldnewthing/20041025-00/?p=37483

  // This is preferred over GetModuleHandle(NULL) because it also works in DLLs:
  // https://stackoverflow.com/questions/21718027/getmodulehandlenull-vs-hinstance

  extern "C" {
    static __ImageBase: windows::Win32::System::SystemServices::IMAGE_DOS_HEADER;
  }

  windows::Win32::Foundation::HMODULE(unsafe { &__ImageBase as *const _ as _ })
}

pub static WIN_VERSION: Lazy<windows_version::OsVersion> =
  Lazy::new(windows_version::OsVersion::current);

pub fn get_frame_thickness(dpi: u32) -> i32 {
  let resize_frame_thickness = unsafe { get_system_metrics_for_dpi(SM_CXSIZEFRAME, dpi) };
  let padding_thickness = unsafe { get_system_metrics_for_dpi(SM_CXPADDEDBORDER, dpi) };
  resize_frame_thickness + padding_thickness
}

pub fn calculate_insets_for_dpi(dpi: u32) -> RECT {
  // - On Windows 10
  // The top inset must be zero, since if there is any nonclient area, Windows will draw
  // a full native titlebar outside the client area. (This doesn't occur in the maximized
  // case.)
  //
  // - On Windows 11
  // The top inset is calculated using an empirical formula that I derived through various
  // tests. Without this, the top 1-2 rows of pixels in our window would be obscured.

  let frame_thickness = get_frame_thickness(dpi);

  let top_inset = match WIN_VERSION.build {
    v if v >= 22000 => (dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32).round() as i32,
    _ => 0,
  };

  RECT {
    left: frame_thickness,
    top: top_inset,
    right: frame_thickness,
    bottom: frame_thickness,
  }
}

/// Calcuclate window insets, used in WM_NCCALCSIZE
///
/// Derived of GPUI implementation
/// see <https://github.com/zed-industries/zed/blob/7bddb390cabefb177d9996dc580749d64e6ca3b6/crates/gpui/src/platform/windows/events.rs#L1418-L1454>
pub fn calculate_window_insets(window: HWND) -> RECT {
  let dpi = unsafe { super::dpi::hwnd_dpi(window) };
  calculate_insets_for_dpi(dpi)
}

pub fn window_rect(hwnd: HWND) -> RECT {
  unsafe {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
      panic!(
        "Unexpected GetWindowRect failure: please report this error to \
               tauri-apps/tao"
      )
    }
    rect
  }
}

pub fn client_rect(hwnd: HWND) -> RECT {
  unsafe {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
      panic!(
        "Unexpected GetClientRect failure: please report this error to \
               tauri-apps/tao"
      )
    }
    rect
  }
}

pub unsafe fn get_system_metrics_for_dpi(nindex: SYSTEM_METRICS_INDEX, dpi: u32) -> i32 {
  #[allow(non_snake_case)]
  if let Some(GetSystemMetricsForDpi) = *GET_SYSTEM_METRICS_FOR_DPI {
    GetSystemMetricsForDpi(nindex, dpi)
  } else {
    GetSystemMetrics(nindex)
  }
}
