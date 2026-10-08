// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  collections::{BTreeSet, VecDeque},
  fmt,
  ops::{Deref, DerefMut},
};

use crate::{
  dpi::{PhysicalPosition, PhysicalSize},
  monitor::{MonitorHandle as RootMonitorHandle, VideoMode as RootVideoMode},
  platform_impl::platform::{
    app_state,
    ffi::{id, nil, CGFloat, CGRect, CGSize, NSInteger, NSUInteger},
  },
};

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct VideoMode {
  pub(crate) size: (u32, u32),
  pub(crate) bit_depth: u16,
  pub(crate) refresh_rate: u16,
  pub(crate) screen_mode: NativeDisplayMode,
  pub(crate) monitor: MonitorHandle,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct NativeDisplayMode(pub id);

unsafe impl Send for NativeDisplayMode {}

impl Drop for NativeDisplayMode {
  fn drop(&mut self) {
    unsafe {
      let () = msg_send![self.0, release];
    }
  }
}

impl Clone for NativeDisplayMode {
  fn clone(&self) -> Self {
    unsafe {
      let _: id = msg_send![self.0, retain];
    }
    NativeDisplayMode(self.0)
  }
}

impl Clone for VideoMode {
  fn clone(&self) -> VideoMode {
    VideoMode {
      size: self.size,
      bit_depth: self.bit_depth,
      refresh_rate: self.refresh_rate,
      screen_mode: self.screen_mode.clone(),
      monitor: self.monitor.clone(),
    }
  }
}

impl VideoMode {
  unsafe fn retained_new(uiscreen: id, screen_mode: id) -> VideoMode {
    assert_main_thread!("`VideoMode` can only be created on the main thread on iOS");
    let os_capabilities = app_state::os_capabilities();
    let refresh_rate: NSInteger = if os_capabilities.maximum_frames_per_second {
      msg_send![uiscreen, maximumFramesPerSecond]
    } else {
      // https://developer.apple.com/library/archive/technotes/tn2460/_index.html
      // https://en.wikipedia.org/wiki/IPad_Pro#Model_comparison
      //
      // All iOS devices support 60 fps, and on devices where `maximumFramesPerSecond` is not
      // supported, they are all guaranteed to have 60hz refresh rates. This does not
      // correctly handle external displays. ProMotion displays support 120fps, but they were
      // introduced at the same time as the `maximumFramesPerSecond` API.
      //
      // FIXME: earlier OSs could calculate the refresh rate using
      // `-[CADisplayLink duration]`.
      os_capabilities.maximum_frames_per_second_err_msg("defaulting to 60 fps");
      60
    };
    let size: CGSize = msg_send![screen_mode, size];
    let screen_mode: id = msg_send![screen_mode, retain];
    let screen_mode = NativeDisplayMode(screen_mode);
    VideoMode {
      size: (size.width as u32, size.height as u32),
      bit_depth: 32,
      refresh_rate: refresh_rate as u16,
      screen_mode,
      monitor: MonitorHandle::retained_new(uiscreen),
    }
  }

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

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Inner {
  uiscreen: id,
}

impl Drop for Inner {
  fn drop(&mut self) {
    unsafe {
      let () = msg_send![self.uiscreen, release];
    }
  }
}

#[derive(PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorHandle {
  inner: Inner,
}

impl Deref for MonitorHandle {
  type Target = Inner;

  fn deref(&self) -> &Inner {
    unsafe {
      assert_main_thread!("`MonitorHandle` methods can only be run on the main thread on iOS");
    }
    &self.inner
  }
}

impl DerefMut for MonitorHandle {
  fn deref_mut(&mut self) -> &mut Inner {
    unsafe {
      assert_main_thread!("`MonitorHandle` methods can only be run on the main thread on iOS");
    }
    &mut self.inner
  }
}

unsafe impl Send for MonitorHandle {}
unsafe impl Sync for MonitorHandle {}

impl Clone for MonitorHandle {
  fn clone(&self) -> MonitorHandle {
    MonitorHandle::retained_new(self.uiscreen)
  }
}

impl Drop for MonitorHandle {
  fn drop(&mut self) {
    unsafe {
      assert_main_thread!("`MonitorHandle` can only be dropped on the main thread on iOS");
    }
  }
}

impl fmt::Debug for MonitorHandle {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    #[derive(Debug)]
    #[allow(unused)]
    struct MonitorHandle {
      name: Option<String>,
      size: PhysicalSize<u32>,
      position: PhysicalPosition<i32>,
      scale_factor: f64,
    }

    let monitor_id_proxy = MonitorHandle {
      name: self.name(),
      size: self.size(),
      position: self.position(),
      scale_factor: self.scale_factor(),
    };

    monitor_id_proxy.fmt(f)
  }
}

impl MonitorHandle {
  pub fn retained_new(uiscreen: id) -> MonitorHandle {
    unsafe {
      assert_main_thread!("`MonitorHandle` can only be cloned on the main thread on iOS");
      let _: id = msg_send![uiscreen, retain];
    }
    MonitorHandle {
      inner: Inner { uiscreen },
    }
  }
}

impl Inner {
  pub fn name(&self) -> Option<String> {
    unsafe {
      let main = main_uiscreen();
      if self.uiscreen == main.uiscreen {
        Some("Primary".to_string())
      } else if self.uiscreen == mirrored_uiscreen(&main).uiscreen {
        Some("Mirrored".to_string())
      } else {
        uiscreens()
          .iter()
          .position(|rhs| rhs.uiscreen == self.uiscreen)
          .map(|idx| idx.to_string())
      }
    }
  }

  pub fn size(&self) -> PhysicalSize<u32> {
    unsafe {
      let bounds: CGRect = msg_send![self.ui_screen(), nativeBounds];
      PhysicalSize::new(bounds.size.width as u32, bounds.size.height as u32)
    }
  }

  pub fn position(&self) -> PhysicalPosition<i32> {
    unsafe {
      let bounds: CGRect = msg_send![self.ui_screen(), nativeBounds];
      (bounds.origin.x as f64, bounds.origin.y as f64).into()
    }
  }

  pub fn scale_factor(&self) -> f64 {
    unsafe {
      let scale: CGFloat = msg_send![self.ui_screen(), nativeScale];
      scale as f64
    }
  }

  pub fn video_modes(&self) -> impl Iterator<Item = RootVideoMode> {
    let mut modes = BTreeSet::new();
    unsafe {
      let available_modes: id = msg_send![self.uiscreen, availableModes];
      let available_mode_count: NSUInteger = msg_send![available_modes, count];

      for i in 0..available_mode_count {
        let mode: id = msg_send![available_modes, objectAtIndex: i];
        modes.insert(RootVideoMode {
          video_mode: VideoMode::retained_new(self.uiscreen, mode),
        });
      }
    }

    modes.into_iter()
  }
}

// MonitorHandleExtIOS
impl Inner {
  pub fn ui_screen(&self) -> id {
    self.uiscreen
  }

  pub fn preferred_video_mode(&self) -> RootVideoMode {
    unsafe {
      let mode: id = msg_send![self.uiscreen, preferredMode];
      RootVideoMode {
        video_mode: VideoMode::retained_new(self.uiscreen, mode),
      }
    }
  }
}

// requires being run on main thread
pub unsafe fn main_uiscreen() -> MonitorHandle {
  let uiscreen: id = msg_send![class!(UIScreen), mainScreen];
  MonitorHandle::retained_new(uiscreen)
}

// requires being run on main thread
unsafe fn mirrored_uiscreen(monitor: &MonitorHandle) -> MonitorHandle {
  let uiscreen: id = msg_send![monitor.uiscreen, mirroredScreen];
  MonitorHandle::retained_new(uiscreen)
}

// requires being run on main thread
pub unsafe fn uiscreens() -> VecDeque<MonitorHandle> {
  let screens: id = msg_send![class!(UIScreen), screens];
  let count: NSUInteger = msg_send![screens, count];
  let mut result = VecDeque::with_capacity(count as _);
  let screens_enum: id = msg_send![screens, objectEnumerator];
  loop {
    let screen: id = msg_send![screens_enum, nextObject];
    if screen == nil {
      break result;
    }
    result.push_back(MonitorHandle::retained_new(screen));
  }
}
