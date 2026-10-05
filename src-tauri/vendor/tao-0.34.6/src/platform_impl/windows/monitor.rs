// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use windows::{
  core::{BOOL, PCWSTR},
  Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::USER_DEFAULT_SCREEN_DPI,
  },
};

use std::{
  collections::{BTreeSet, VecDeque},
  io, mem,
};

use super::util;
use crate::{
  dpi::{PhysicalPosition, PhysicalSize},
  monitor::{MonitorHandle as RootMonitorHandle, VideoMode as RootVideoMode},
  platform_impl::platform::{
    dpi::{dpi_to_scale_factor, get_monitor_dpi},
    window::Window,
  },
};

#[derive(Clone)]
pub struct VideoMode {
  pub(crate) size: (u32, u32),
  pub(crate) bit_depth: u16,
  pub(crate) refresh_rate: u16,
  pub(crate) monitor: MonitorHandle,
  pub(crate) native_video_mode: DEVMODEW,
}

impl PartialEq for VideoMode {
  fn eq(&self, other: &Self) -> bool {
    self.size == other.size
      && self.bit_depth == other.bit_depth
      && self.refresh_rate == other.refresh_rate
      && self.monitor == other.monitor
  }
}

impl Eq for VideoMode {}

impl std::hash::Hash for VideoMode {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.size.hash(state);
    self.bit_depth.hash(state);
    self.refresh_rate.hash(state);
    self.monitor.hash(state);
  }
}

impl std::fmt::Debug for VideoMode {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("VideoMode")
      .field("size", &self.size)
      .field("bit_depth", &self.bit_depth)
      .field("refresh_rate", &self.refresh_rate)
      .field("monitor", &self.monitor)
      .finish()
  }
}

impl VideoMode {
  pub fn size(&self) -> PhysicalSize<u32> {
    self.size.into()
  }

  pub fn bit_depth(&self) -> u16 {
    self.bit_depth
  }

  pub fn refresh_rate(&self) -> u16 {
    self.refresh_rate
  }

  pub fn monitor(&self) -> RootMonitorHandle {
    RootMonitorHandle {
      inner: self.monitor.clone(),
    }
  }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MonitorHandle(isize);

unsafe extern "system" fn monitor_enum_proc(
  hmonitor: HMONITOR,
  _hdc: HDC,
  _place: *mut RECT,
  data: LPARAM,
) -> BOOL {
  let monitors = data.0 as *mut VecDeque<MonitorHandle>;
  (*monitors).push_back(MonitorHandle::new(hmonitor));
  true.into() // continue enumeration
}

pub fn available_monitors() -> VecDeque<MonitorHandle> {
  let mut monitors: VecDeque<MonitorHandle> = VecDeque::new();
  unsafe {
    let _ = EnumDisplayMonitors(
      None,
      None,
      Some(monitor_enum_proc),
      LPARAM(&mut monitors as *mut _ as _),
    );
  }
  monitors
}

pub fn primary_monitor() -> MonitorHandle {
  const ORIGIN: POINT = POINT { x: 0, y: 0 };
  let hmonitor = unsafe { MonitorFromPoint(ORIGIN, MONITOR_DEFAULTTOPRIMARY) };
  MonitorHandle::new(hmonitor)
}

pub fn current_monitor(hwnd: HWND) -> MonitorHandle {
  let hmonitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
  MonitorHandle::new(hmonitor)
}

pub fn from_point(x: f64, y: f64) -> Option<MonitorHandle> {
  let hmonitor = unsafe {
    MonitorFromPoint(
      POINT {
        x: x as i32,
        y: y as i32,
      },
      MONITOR_DEFAULTTONULL,
    )
  };
  if !hmonitor.is_invalid() {
    Some(MonitorHandle::new(hmonitor))
  } else {
    None
  }
}

impl Window {
  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    available_monitors()
  }

  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let monitor = primary_monitor();
    Some(RootMonitorHandle { inner: monitor })
  }

  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<RootMonitorHandle> {
    from_point(x, y).map(|inner| RootMonitorHandle { inner })
  }
}

pub(crate) fn get_monitor_info(hmonitor: HMONITOR) -> Result<MONITORINFOEXW, io::Error> {
  let mut monitor_info = MONITORINFOEXW::default();
  monitor_info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
  let status = unsafe {
    GetMonitorInfoW(
      hmonitor,
      &mut monitor_info as *mut MONITORINFOEXW as *mut MONITORINFO,
    )
  };
  if !status.as_bool() {
    Err(io::Error::last_os_error())
  } else {
    Ok(monitor_info)
  }
}

impl MonitorHandle {
  pub(crate) fn new(hmonitor: HMONITOR) -> Self {
    MonitorHandle(hmonitor.0 as _)
  }

  #[inline]
  pub fn name(&self) -> Option<String> {
    let monitor_info = get_monitor_info(self.hmonitor()).ok()?;
    Some(util::wchar_ptr_to_string(PCWSTR::from_raw(
      monitor_info.szDevice.as_ptr(),
    )))
  }

  #[inline]
  pub fn native_identifier(&self) -> String {
    self.name().unwrap()
  }

  #[inline]
  pub fn hmonitor(&self) -> HMONITOR {
    HMONITOR(self.0 as _)
  }

  #[inline]
  pub fn size(&self) -> PhysicalSize<u32> {
    let Ok(monitor_info) = get_monitor_info(self.hmonitor()) else {
      return PhysicalSize {
        width: 0,
        height: 0,
      };
    };
    PhysicalSize {
      width: (monitor_info.monitorInfo.rcMonitor.right - monitor_info.monitorInfo.rcMonitor.left)
        as u32,
      height: (monitor_info.monitorInfo.rcMonitor.bottom - monitor_info.monitorInfo.rcMonitor.top)
        as u32,
    }
  }

  #[inline]
  pub fn position(&self) -> PhysicalPosition<i32> {
    let Ok(monitor_info) = get_monitor_info(self.hmonitor()) else {
      return PhysicalPosition { x: 0, y: 0 };
    };
    PhysicalPosition {
      x: monitor_info.monitorInfo.rcMonitor.left,
      y: monitor_info.monitorInfo.rcMonitor.top,
    }
  }

  #[inline]
  pub fn scale_factor(&self) -> f64 {
    dpi_to_scale_factor(self.dpi())
  }

  pub fn dpi(&self) -> u32 {
    get_monitor_dpi(self.hmonitor()).unwrap_or(USER_DEFAULT_SCREEN_DPI)
  }

  #[inline]
  pub fn video_modes(&self) -> impl Iterator<Item = RootVideoMode> {
    // EnumDisplaySettingsExW can return duplicate values (or some of the
    // fields are probably changing, but we aren't looking at those fields
    // anyway), so we're using a BTreeSet deduplicate
    let mut modes = BTreeSet::new();

    let Ok(monitor_info) = get_monitor_info(self.hmonitor()) else {
      return modes.into_iter();
    };

    let device_name = PCWSTR::from_raw(monitor_info.szDevice.as_ptr());
    let mut i = 0;
    loop {
      unsafe {
        let mut mode: DEVMODEW = mem::zeroed();
        mode.dmSize = mem::size_of_val(&mode) as u16;
        if !EnumDisplaySettingsExW(
          device_name,
          ENUM_DISPLAY_SETTINGS_MODE(i),
          &mut mode,
          ENUM_DISPLAY_SETTINGS_FLAGS(0),
        )
        .as_bool()
        {
          break;
        }
        i += 1;

        let required_fields = DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        assert!(mode.dmFields & required_fields == required_fields);

        modes.insert(RootVideoMode {
          video_mode: VideoMode {
            size: (mode.dmPelsWidth, mode.dmPelsHeight),
            bit_depth: mode.dmBitsPerPel as u16,
            refresh_rate: mode.dmDisplayFrequency as u16,
            monitor: self.clone(),
            native_video_mode: mode,
          },
        });
      }
    }

    modes.into_iter()
  }
}
