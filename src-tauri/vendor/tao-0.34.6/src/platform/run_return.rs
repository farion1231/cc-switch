// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(not(target_os = "ios"))]

use crate::{
  event::Event,
  event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
};

/// Additional methods on `EventLoop` to return control flow to the caller.
pub trait EventLoopExtRunReturn {
  /// A type provided by the user that can be passed through `Event::UserEvent`.
  type UserEvent;

  /// Initializes the `tao` event loop.
  ///
  /// Unlike `run`, this function accepts non-`'static` (i.e. non-`move`) closures and returns
  /// control flow to the caller when `control_flow` is set to `ControlFlow::Exit`.
  ///
  /// # Caveats
  /// Despite its appearance at first glance, this is *not* a perfect replacement for
  /// `poll_events`. For example, this function will not return on Windows or macOS while a
  /// window is getting resized, resulting in all application logic outside of the
  /// `event_handler` closure not running until the resize operation ends. Other OS operations
  /// may also result in such freezes. This behavior is caused by fundamental limitations in the
  /// underlying OS APIs, which cannot be hidden by `tao` without severe stability repercussions.
  ///
  /// You are strongly encouraged to use `run`, unless the use of this is absolutely necessary.
  ///
  /// ## Platform-specific
  ///
  /// - **Unix-alikes** (**X11** or **Wayland**): This function returns `1` upon disconnection from
  ///   the display server.
  fn run_return<F>(&mut self, event_handler: F) -> i32
  where
    F: FnMut(Event<'_, Self::UserEvent>, &EventLoopWindowTarget<Self::UserEvent>, &mut ControlFlow);
}

impl<T> EventLoopExtRunReturn for EventLoop<T> {
  type UserEvent = T;

  fn run_return<F>(&mut self, event_handler: F) -> i32
  where
    F: FnMut(Event<'_, Self::UserEvent>, &EventLoopWindowTarget<Self::UserEvent>, &mut ControlFlow),
  {
    self.event_loop.run_return(event_handler)
  }
}
