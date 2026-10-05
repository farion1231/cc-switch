// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use gtk::gdk::{self, prelude::MonitorExt, Display};

use crate::{
  dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize},
  monitor::{MonitorHandle as RootMonitorHandle, VideoMode as RootVideoMode},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonitorHandle {
  pub(crate) monitor: gdk::Monitor,
}

impl MonitorHandle {
  pub fn new(display: &gdk::Display, number: i32) -> Self {
    let monitor = display.monitor(number).unwrap();
    Self { monitor }
  }

  #[inline]
  pub fn name(&self) -> Option<String> {
    self.monitor.model().map(|s| s.as_str().to_string())
  }

  #[inline]
  pub fn size(&self) -> PhysicalSize<u32> {
    let rect = self.monitor.geometry();
    LogicalSize {
      width: rect.width() as u32,
      height: rect.height() as u32,
    }
    .to_physical(self.scale_factor())
  }

  #[inline]
  pub fn position(&self) -> PhysicalPosition<i32> {
    let rect = self.monitor.geometry();
    LogicalPosition {
      x: rect.x(),
      y: rect.y(),
    }
    .to_physical(self.scale_factor())
  }

  #[inline]
  pub fn scale_factor(&self) -> f64 {
    self.monitor.scale_factor() as f64
  }

  #[inline]
  pub fn video_modes(&self) -> Box<dyn Iterator<Item = RootVideoMode>> {
    Box::new(Vec::new().into_iter())
  }
}

unsafe impl Send for MonitorHandle {}
unsafe impl Sync for MonitorHandle {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VideoMode;

impl VideoMode {
  #[inline]
  pub fn size(&self) -> PhysicalSize<u32> {
    panic!("VideoMode is unsupported on Linux.")
  }

  #[inline]
  pub fn bit_depth(&self) -> u16 {
    panic!("VideoMode is unsupported on Linux.")
  }

  #[inline]
  pub fn refresh_rate(&self) -> u16 {
    panic!("VideoMode is unsupported on Linux.")
  }

  #[inline]
  pub fn monitor(&self) -> RootMonitorHandle {
    panic!("VideoMode is unsupported on Linux.")
  }
}

pub fn from_point(display: &Display, x: f64, y: f64) -> Option<MonitorHandle> {
  if let Some(monitor) = display.monitor_at_point(x as i32, y as i32) {
    (0..display.n_monitors())
      .map(|i| (i, display.monitor(i).unwrap()))
      .find(|cur| cur.1.geometry() == monitor.geometry())
      .map(|x| MonitorHandle::new(display, x.0))
  } else {
    None
  }
}
