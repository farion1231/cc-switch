// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![allow(non_snake_case, unused_unsafe)]

use std::sync::Once;

use windows::Win32::{
  Foundation::HWND,
  Graphics::Gdi::*,
  UI::{HiDpi::*, WindowsAndMessaging::*},
};

use crate::platform_impl::platform::util::{
  ENABLE_NON_CLIENT_DPI_SCALING, GET_DPI_FOR_MONITOR, GET_DPI_FOR_WINDOW, SET_PROCESS_DPI_AWARE,
  SET_PROCESS_DPI_AWARENESS, SET_PROCESS_DPI_AWARENESS_CONTEXT,
};

pub fn become_dpi_aware() {
  static ENABLE_DPI_AWARENESS: Once = Once::new();
  ENABLE_DPI_AWARENESS.call_once(|| {
    unsafe {
      if let Some(SetProcessDpiAwarenessContext) = *SET_PROCESS_DPI_AWARENESS_CONTEXT {
        // We are on Windows 10 Anniversary Update (1607) or later.
        if !SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).as_bool() {
          // V2 only works with Windows 10 Creators Update (1703). Try using the older
          // V1 if we can't set V2.
          let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
      } else if let Some(SetProcessDpiAwareness) = *SET_PROCESS_DPI_AWARENESS {
        // We are on Windows 8.1 or later.
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
      } else if let Some(SetProcessDPIAware) = *SET_PROCESS_DPI_AWARE {
        // We are on Vista or later.
        let _ = SetProcessDPIAware();
      }
    }
  });
}

pub fn enable_non_client_dpi_scaling(hwnd: HWND) {
  unsafe {
    if let Some(EnableNonClientDpiScaling) = *ENABLE_NON_CLIENT_DPI_SCALING {
      let _ = EnableNonClientDpiScaling(hwnd);
    }
  }
}

pub fn get_monitor_dpi(hmonitor: HMONITOR) -> Option<u32> {
  unsafe {
    if let Some(GetDpiForMonitor) = *GET_DPI_FOR_MONITOR {
      // We are on Windows 8.1 or later.
      let mut dpi_x = 0;
      let mut dpi_y = 0;
      if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
        // MSDN says that "the values of *dpiX and *dpiY are identical. You only need to
        // record one of the values to determine the DPI and respond appropriately".
        // https://msdn.microsoft.com/en-us/library/windows/desktop/dn280510(v=vs.85).aspx
        return Some(dpi_x);
      }
    }
  }
  None
}

pub fn dpi_to_scale_factor(dpi: u32) -> f64 {
  dpi as f64 / USER_DEFAULT_SCREEN_DPI as f64
}

pub unsafe fn hwnd_dpi(hwnd: HWND) -> u32 {
  let hdc = GetDC(Some(hwnd));
  if hdc.is_invalid() {
    panic!("[tao] `GetDC` returned null!");
  }
  if let Some(GetDpiForWindow) = *GET_DPI_FOR_WINDOW {
    // We are on Windows 10 Anniversary Update (1607) or later.
    match GetDpiForWindow(hwnd) {
      0 => USER_DEFAULT_SCREEN_DPI, // 0 is returned if hwnd is invalid
      dpi => dpi,
    }
  } else if let Some(GetDpiForMonitor) = *GET_DPI_FOR_MONITOR {
    // We are on Windows 8.1 or later.
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if monitor.is_invalid() {
      return USER_DEFAULT_SCREEN_DPI;
    }

    let mut dpi_x = 0;
    let mut dpi_y = 0;
    if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
      dpi_x
    } else {
      USER_DEFAULT_SCREEN_DPI
    }
  } else {
    // We are on Vista or later.
    if IsProcessDPIAware().as_bool() {
      // If the process is DPI aware, then scaling must be handled by the application using
      // this DPI value.
      GetDeviceCaps(Some(hdc), LOGPIXELSX) as u32
    } else {
      // If the process is DPI unaware, then scaling is performed by the OS; we thus return
      // 96 (scale factor 1.0) to prevent the window from being re-scaled by both the
      // application and the WM.
      USER_DEFAULT_SCREEN_DPI
    }
  }
}
