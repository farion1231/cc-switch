// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

mod app;
mod app_delegate;
mod app_state;
mod badge;
mod dock;
mod event;
mod event_loop;
mod ffi;
mod icon;
mod keycode;
mod monitor;
mod observer;
mod progress_bar;
mod util;
mod view;
mod window;
mod window_delegate;

use std::{fmt, ops::Deref, sync::Arc};

pub(crate) use self::event_loop::PlatformSpecificEventLoopAttributes;
pub use self::{
  app_delegate::get_aux_state_mut,
  event::KeyEventExtra,
  event_loop::{EventLoop, EventLoopWindowTarget, Proxy as EventLoopProxy},
  keycode::{keycode_from_scancode, keycode_to_scancode},
  monitor::{MonitorHandle, VideoMode},
  progress_bar::set_progress_indicator,
  window::{Id as WindowId, Parent, PlatformSpecificWindowBuilderAttributes, UnownedWindow},
};
use crate::{
  error::OsError as RootOsError, event::DeviceId as RootDeviceId, window::WindowAttributes,
};
pub(crate) use badge::set_badge_label;
pub(crate) use dock::set_dock_visibility;
pub(crate) use icon::PlatformIcon;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

impl DeviceId {
  pub unsafe fn dummy() -> Self {
    DeviceId
  }
}

// Constant device ID; to be removed when if backend is updated to report real device IDs.
pub(crate) const DEVICE_ID: RootDeviceId = RootDeviceId(DeviceId);

pub struct Window {
  window: Arc<UnownedWindow>,
  // We keep this around so that it doesn't get dropped until the window does.
  #[allow(dead_code)]
  delegate: util::IdRef,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum OsError {
  CGError(core_graphics::base::CGError),
  CreationError(&'static str),
}

unsafe impl Send for Window {}
unsafe impl Sync for Window {}

impl Deref for Window {
  type Target = UnownedWindow;
  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.window
  }
}

impl Window {
  pub fn new<T: 'static>(
    _window_target: &EventLoopWindowTarget<T>,
    attributes: WindowAttributes,
    pl_attribs: PlatformSpecificWindowBuilderAttributes,
  ) -> Result<Self, RootOsError> {
    let (window, delegate) = UnownedWindow::new(attributes, pl_attribs)?;
    Ok(Window { window, delegate })
  }
}

impl fmt::Display for OsError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      OsError::CGError(e) => f.pad(&format!("CGError {}", e)),
      OsError::CreationError(e) => f.pad(e),
    }
  }
}
