// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "x11")]
mod device;
mod event_loop;
mod icon;
mod keyboard;
mod keycode;
mod monitor;
mod util;
mod window;

pub mod taskbar;
pub mod wayland;
#[cfg(feature = "x11")]
pub mod x11;

pub use self::keycode::{keycode_from_scancode, keycode_to_scancode};
pub(crate) use event_loop::PlatformSpecificEventLoopAttributes;
pub use event_loop::{EventLoop, EventLoopProxy, EventLoopWindowTarget};
pub use icon::PlatformIcon;
pub use monitor::{MonitorHandle, VideoMode};
pub use window::{Window, WindowId};

use crate::{event::DeviceId as RootDeviceId, keyboard::Key};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {
  pub text_with_all_modifiers: Option<&'static str>,
  pub key_without_modifiers: Key<'static>,
}

#[non_exhaustive]
#[derive(Clone, Default)]
pub enum Parent {
  #[default]
  None,
  ChildOf(gtk::Window),
}

#[derive(Clone)]
pub struct PlatformSpecificWindowBuilderAttributes {
  pub parent: Parent,
  pub skip_taskbar: bool,
  pub auto_transparent: bool,
  pub double_buffered: bool,
  pub app_paintable: bool,
  pub rgba_visual: bool,
  pub cursor_moved: bool,
  pub default_vbox: bool,
}

impl Default for PlatformSpecificWindowBuilderAttributes {
  fn default() -> Self {
    Self {
      parent: Default::default(),
      skip_taskbar: Default::default(),
      auto_transparent: true,
      double_buffered: true,
      app_paintable: false,
      rgba_visual: false,
      cursor_moved: true,
      default_vbox: true,
    }
  }
}

unsafe impl Send for PlatformSpecificWindowBuilderAttributes {}
unsafe impl Sync for PlatformSpecificWindowBuilderAttributes {}

#[derive(Debug, Clone)]
pub struct OsError;

impl std::fmt::Display for OsError {
  fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
    Ok(())
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(usize);

impl DeviceId {
  pub unsafe fn dummy() -> Self {
    Self(0)
  }
}

// FIXME: currently we use a dummy device id, find if we can get device id from gtk
pub(crate) const DEVICE_ID: RootDeviceId = RootDeviceId(DeviceId(0));
