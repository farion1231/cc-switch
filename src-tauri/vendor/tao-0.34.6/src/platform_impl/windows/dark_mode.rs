// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
#![allow(non_snake_case)]

use once_cell::sync::Lazy;
/// This is a simple implementation of support for Windows Dark Mode,
/// which is inspired by the solution in https://github.com/ysc3839/win32-darkmode
use windows::{
  core::{s, w, BOOL, PCSTR, PSTR},
  Win32::{
    Foundation::{ERROR_SUCCESS, HANDLE, HMODULE, HWND, LPARAM, WPARAM},
    Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE},
    System::{
      LibraryLoader::*,
      Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
    },
    UI::{Accessibility::*, Input::KeyboardAndMouse::GetActiveWindow, WindowsAndMessaging::*},
  },
};

use std::ffi::c_void;

use crate::window::Theme;

use super::util;

static HUXTHEME: Lazy<isize> =
  Lazy::new(|| unsafe { LoadLibraryA(s!("uxtheme.dll")).unwrap_or_default().0 as _ });

static DARK_MODE_SUPPORTED: Lazy<bool> = Lazy::new(|| {
  // We won't try to do anything for windows versions < 17763
  // (Windows 10 October 2018 update)
  let v = *util::WIN_VERSION;
  v.major == 10 && v.minor == 0 && v.build >= 17763
});

pub fn allow_dark_mode_for_app(is_dark_mode: bool) {
  if *DARK_MODE_SUPPORTED {
    const UXTHEME_ALLOWDARKMODEFORAPP_ORDINAL: u16 = 135;
    type AllowDarkModeForApp = unsafe extern "system" fn(bool) -> bool;
    static ALLOW_DARK_MODE_FOR_APP: Lazy<Option<AllowDarkModeForApp>> = Lazy::new(|| unsafe {
      if HMODULE(*HUXTHEME as _).is_invalid() {
        return None;
      }

      GetProcAddress(
        HMODULE(*HUXTHEME as _),
        PCSTR::from_raw(UXTHEME_ALLOWDARKMODEFORAPP_ORDINAL as usize as *mut _),
      )
      .map(|handle| std::mem::transmute(handle))
    });

    #[repr(C)]
    enum PreferredAppMode {
      Default,
      AllowDark,
      // ForceDark,
      // ForceLight,
      // Max,
    }
    const UXTHEME_SETPREFERREDAPPMODE_ORDINAL: u16 = 135;
    type SetPreferredAppMode = unsafe extern "system" fn(PreferredAppMode) -> PreferredAppMode;
    static SET_PREFERRED_APP_MODE: Lazy<Option<SetPreferredAppMode>> = Lazy::new(|| unsafe {
      if HMODULE(*HUXTHEME as _).is_invalid() {
        return None;
      }

      GetProcAddress(
        HMODULE(*HUXTHEME as _),
        PCSTR::from_raw(UXTHEME_SETPREFERREDAPPMODE_ORDINAL as usize as *mut _),
      )
      .map(|handle| std::mem::transmute(handle))
    });

    if util::WIN_VERSION.build < 18362 {
      if let Some(_allow_dark_mode_for_app) = *ALLOW_DARK_MODE_FOR_APP {
        unsafe { _allow_dark_mode_for_app(is_dark_mode) };
      }
    } else if let Some(_set_preferred_app_mode) = *SET_PREFERRED_APP_MODE {
      let mode = if is_dark_mode {
        PreferredAppMode::AllowDark
      } else {
        PreferredAppMode::Default
      };
      unsafe { _set_preferred_app_mode(mode) };
    }

    refresh_immersive_color_policy_state();
  }
}

fn refresh_immersive_color_policy_state() {
  const UXTHEME_REFRESHIMMERSIVECOLORPOLICYSTATE_ORDINAL: u16 = 104;
  type RefreshImmersiveColorPolicyState = unsafe extern "system" fn();
  static REFRESH_IMMERSIVE_COLOR_POLICY_STATE: Lazy<Option<RefreshImmersiveColorPolicyState>> =
    Lazy::new(|| unsafe {
      if HMODULE(*HUXTHEME as _).is_invalid() {
        return None;
      }

      GetProcAddress(
        HMODULE(*HUXTHEME as _),
        PCSTR::from_raw(UXTHEME_REFRESHIMMERSIVECOLORPOLICYSTATE_ORDINAL as usize as *mut _),
      )
      .map(|handle| std::mem::transmute(handle))
    });

  if let Some(_refresh_immersive_color_policy_state) = *REFRESH_IMMERSIVE_COLOR_POLICY_STATE {
    unsafe { _refresh_immersive_color_policy_state() }
  }
}

/// Attempt to set a theme on a window, if necessary.
/// Returns the theme that was picked
pub fn try_window_theme(
  hwnd: HWND,
  preferred_theme: Option<Theme>,
  redraw_title_bar: bool,
) -> Theme {
  if *DARK_MODE_SUPPORTED {
    let is_dark_mode = match preferred_theme {
      Some(theme) => theme == Theme::Dark,
      None => should_use_dark_mode(),
    };

    let theme = match is_dark_mode {
      true => Theme::Dark,
      false => Theme::Light,
    };

    refresh_titlebar_theme_color(hwnd, is_dark_mode, redraw_title_bar);

    theme
  } else {
    Theme::Light
  }
}

pub fn allow_dark_mode_for_window(hwnd: HWND, is_dark_mode: bool) {
  const UXTHEME_ALLOWDARKMODEFORWINDOW_ORDINAL: u16 = 133;
  type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, bool) -> bool;
  static ALLOW_DARK_MODE_FOR_WINDOW: Lazy<Option<AllowDarkModeForWindow>> = Lazy::new(|| unsafe {
    if HMODULE(*HUXTHEME as _).is_invalid() {
      return None;
    }

    GetProcAddress(
      HMODULE(*HUXTHEME as _),
      PCSTR::from_raw(UXTHEME_ALLOWDARKMODEFORWINDOW_ORDINAL as usize as *mut _),
    )
    .map(|handle| std::mem::transmute(handle))
  });

  if *DARK_MODE_SUPPORTED {
    if let Some(_allow_dark_mode_for_window) = *ALLOW_DARK_MODE_FOR_WINDOW {
      unsafe { _allow_dark_mode_for_window(hwnd, is_dark_mode) };
    }
  }
}

fn refresh_titlebar_theme_color(hwnd: HWND, is_dark_mode: bool, redraw_title_bar: bool) {
  if util::WIN_VERSION.build < 17763 {
    let mut is_dark_mode_bigbool: i32 = is_dark_mode.into();
    unsafe {
      let _ = SetPropW(
        hwnd,
        w!("UseImmersiveDarkModeColors"),
        Some(HANDLE(&mut is_dark_mode_bigbool as *mut _ as _)),
      );
    }
  } else {
    // https://github.com/MicrosoftDocs/sdk-api/pull/966/files
    let dwmwa_use_immersive_dark_mode = if util::WIN_VERSION.build > 18985 {
      DWMWINDOWATTRIBUTE(20)
    } else {
      DWMWINDOWATTRIBUTE(19)
    };
    let dark_mode = BOOL::from(is_dark_mode);
    unsafe {
      let _ = DwmSetWindowAttribute(
        hwnd,
        dwmwa_use_immersive_dark_mode,
        &dark_mode as *const BOOL as *const c_void,
        std::mem::size_of::<BOOL>() as u32,
      );
      if redraw_title_bar {
        if GetActiveWindow() == hwnd {
          DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM::default(), LPARAM::default());
          DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(true.into()), LPARAM::default());
        } else {
          DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(true.into()), LPARAM::default());
          DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM::default(), LPARAM::default());
        }
      }
    }
  }
}

fn should_use_dark_mode() -> bool {
  should_apps_use_dark_mode() && !is_high_contrast()
}

fn should_apps_use_dark_mode() -> bool {
  if let Some(apps_use_light_theme) = read_apps_use_light_theme() {
    return !apps_use_light_theme;
  }
  // This undocumented method `ShouldAppsUseDarkMode` may return
  // incorrect values on Windows 11.
  // See https://github.com/tauri-apps/tao/pull/1165
  const UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL: u16 = 132;
  type ShouldAppsUseDarkMode = unsafe extern "system" fn() -> bool;
  static SHOULD_APPS_USE_DARK_MODE: Lazy<Option<ShouldAppsUseDarkMode>> = Lazy::new(|| unsafe {
    if HMODULE(*HUXTHEME as _).is_invalid() {
      return None;
    }

    GetProcAddress(
      HMODULE(*HUXTHEME as _),
      PCSTR::from_raw(UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL as usize as *mut _),
    )
    .map(|handle| std::mem::transmute(handle))
  });

  SHOULD_APPS_USE_DARK_MODE
    .map(|should_apps_use_dark_mode| unsafe { (should_apps_use_dark_mode)() })
    .unwrap_or(false)
}

fn read_apps_use_light_theme() -> Option<bool> {
  let mut data: u32 = 0;
  let mut data_size = std::mem::size_of::<u32>() as u32;
  let status = unsafe {
    RegGetValueW(
      HKEY_CURRENT_USER,
      w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
      w!("AppsUseLightTheme"),
      RRF_RT_REG_DWORD,
      None,
      Some(&mut data as *mut _ as _),
      Some(&mut data_size),
    )
  };
  if status == ERROR_SUCCESS {
    Some(data != 0)
  } else {
    None
  }
}

fn is_high_contrast() -> bool {
  const HCF_HIGHCONTRASTON: u32 = 1;

  let mut hc = HIGHCONTRASTA {
    cbSize: 0,
    dwFlags: Default::default(),
    lpszDefaultScheme: PSTR::null(),
  };

  let ok = unsafe {
    SystemParametersInfoA(
      SPI_GETHIGHCONTRAST,
      std::mem::size_of_val(&hc) as _,
      Some(&mut hc as *mut _ as _),
      Default::default(),
    )
  };

  ok.is_ok() && (HCF_HIGHCONTRASTON & hc.dwFlags.0) != 0
}
