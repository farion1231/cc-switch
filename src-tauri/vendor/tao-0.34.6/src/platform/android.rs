// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "android")]

pub mod prelude {
  pub use crate::platform_impl::ndk_glue::*;
  pub use tao_macros::{android_fn, generate_package_name};
}
use crate::{
  event_loop::{EventLoop, EventLoopWindowTarget},
  platform_impl::ndk_glue::Rect,
  window::{Window, WindowBuilder},
};
use ndk::configuration::Configuration;

/// Additional methods on `EventLoop` that are specific to Android.
pub trait EventLoopExtAndroid {}

impl<T> EventLoopExtAndroid for EventLoop<T> {}

/// Additional methods on `EventLoopWindowTarget` that are specific to Android.
pub trait EventLoopWindowTargetExtAndroid {}

/// Additional methods on `Window` that are specific to Android.
pub trait WindowExtAndroid {
  fn content_rect(&self) -> Rect;

  fn config(&self) -> Configuration;
}

impl WindowExtAndroid for Window {
  fn content_rect(&self) -> Rect {
    self.window.content_rect()
  }

  fn config(&self) -> Configuration {
    self.window.config()
  }
}

impl<T> EventLoopWindowTargetExtAndroid for EventLoopWindowTarget<T> {}

/// Additional methods on `WindowBuilder` that are specific to Android.
pub trait WindowBuilderExtAndroid {}

impl WindowBuilderExtAndroid for WindowBuilder {}
