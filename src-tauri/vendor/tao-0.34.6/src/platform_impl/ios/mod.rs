// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! iOS support
//!
//! # Building app
//! To build ios app you will need rustc built for this targets:
//!
//!  - armv7-apple-ios
//!  - armv7s-apple-ios
//!  - i386-apple-ios
//!  - aarch64-apple-ios
//!  - x86_64-apple-ios
//!
//! Then
//!
//! ```
//! cargo build --target=...
//! ```
//! The simplest way to integrate your app into xcode environment is to build it
//! as a static library. Wrap your main function and export it.
//!
//! ```rust, ignore
//! #[no_mangle]
//! pub extern fn start_tao_app() {
//!     start_inner()
//! }
//!
//! fn start_inner() {
//!    ...
//! }
//! ```
//!
//! Compile project and then drag resulting .a into Xcode project. Add tao.h to xcode.
//!
//! ```ignore
//! void start_tao_app();
//! ```
//!
//! Use start_tao_app inside your xcode's main function.
//!
//!
//! # App lifecycle and events
//!
//! iOS environment is very different from other platforms and you must be very
//! careful with it's events. Familiarize yourself with
//! [app lifecycle](https://developer.apple.com/library/ios/documentation/UIKit/Reference/UIApplicationDelegate_Protocol/).
//!
//!
//! This is how those event are represented in tao:
//!
//!  - applicationDidBecomeActive is Resumed
//!  - applicationWillResignActive is Suspended
//!  - applicationWillTerminate is LoopDestroyed
//!
//! Keep in mind that after LoopDestroyed event is received every attempt to draw with
//! opengl will result in segfault.
//!
//! Also note that app may not receive the LoopDestroyed event if suspended; it might be SIGKILL'ed.

// TODO: (mtak-) UIKit requires main thread for virtually all function/method calls. This could be
// worked around in the future by using GCD (grand central dispatch) and/or caching of values like
// window size/position.
macro_rules! assert_main_thread {
    ($($t:tt)*) => {
        let is_main_thread: bool = msg_send![class!(NSThread), isMainThread];
        if !is_main_thread {
            panic!($($t)*);
        }
    };
}

mod app_state;
mod badge;
mod event_loop;
mod ffi;
mod keycode;
mod monitor;
mod view;
mod window;

use std::fmt;

pub(crate) use self::event_loop::PlatformSpecificEventLoopAttributes;
pub use self::{
  event_loop::{EventLoop, EventLoopProxy, EventLoopWindowTarget},
  keycode::{keycode_from_scancode, keycode_to_scancode},
  monitor::{MonitorHandle, VideoMode},
  window::{PlatformSpecificWindowBuilderAttributes, Window, WindowId},
};
pub(crate) use crate::icon::NoIcon as PlatformIcon;
pub(crate) use badge::set_badge_count;

// todo: implement iOS keyboard event
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId {
  uiscreen: ffi::id,
}

impl DeviceId {
  pub unsafe fn dummy() -> Self {
    DeviceId {
      uiscreen: std::ptr::null_mut(),
    }
  }
}

unsafe impl Send for DeviceId {}
unsafe impl Sync for DeviceId {}

#[non_exhaustive]
#[derive(Debug)]
pub enum OsError {}

impl fmt::Display for OsError {
  fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
    unreachable!()
  }
}
