// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

mod r#async;
mod cursor;

pub use self::{cursor::*, r#async::*};

use std::ops::{BitAnd, Deref};

use core_graphics::display::CGDisplay;
use objc2::{
  class,
  runtime::{AnyClass as Class, AnyObject as Object, Sel},
};
use objc2_app_kit::{NSApp, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSAutoreleasePool, NSPoint, NSRange, NSRect, NSUInteger};

use crate::{
  dpi::{LogicalPosition, PhysicalPosition},
  error::ExternalError,
  platform_impl::platform::ffi::{self, id, nil, BOOL, YES},
};

// Replace with `!` once stable
#[derive(Debug)]
pub enum Never {}

pub fn has_flag<T>(bitset: T, flag: T) -> bool
where
  T: Copy + PartialEq + BitAnd<T, Output = T>,
{
  bitset & flag == flag
}

pub const EMPTY_RANGE: NSRange = NSRange {
  location: ffi::NSNotFound as NSUInteger,
  length: 0,
};

#[derive(Debug, PartialEq)]
pub struct IdRef(id);

impl IdRef {
  pub fn new(inner: id) -> IdRef {
    IdRef(inner)
  }

  #[allow(dead_code)]
  pub fn retain(inner: id) -> IdRef {
    if inner != nil {
      let _: id = unsafe { msg_send![inner, retain] };
    }
    IdRef(inner)
  }
}

impl Drop for IdRef {
  fn drop(&mut self) {
    if self.0 != nil {
      unsafe {
        let _pool = NSAutoreleasePool::new();
        let () = msg_send![self.0, release];
      };
    }
  }
}

impl Deref for IdRef {
  type Target = id;
  #[allow(clippy::needless_lifetimes)]
  fn deref<'a>(&'a self) -> &'a id {
    &self.0
  }
}

impl Clone for IdRef {
  fn clone(&self) -> IdRef {
    IdRef::retain(self.0)
  }
}

// For consistency with other platforms, this will...
// 1. translate the bottom-left window corner into the top-left window corner
// 2. translate the coordinate from a bottom-left origin coordinate system to a top-left one
pub fn bottom_left_to_top_left(rect: NSRect) -> f64 {
  CGDisplay::main().pixels_high() as f64 - (rect.origin.y + rect.size.height)
}

/// Converts from tao screen-coordinates to macOS screen-coordinates.
/// Tao: top-left is (0, 0) and y increasing downwards
/// macOS: bottom-left is (0, 0) and y increasing upwards
pub fn window_position(position: LogicalPosition<f64>) -> NSPoint {
  NSPoint::new(
    position.x,
    CGDisplay::main().pixels_high() as f64 - position.y,
  )
}

pub fn cursor_position() -> Result<PhysicalPosition<f64>, ExternalError> {
  let point: NSPoint = unsafe { msg_send![class!(NSEvent), mouseLocation] };
  let y = CGDisplay::main().pixels_high() as f64 - point.y;
  let point = LogicalPosition::new(point.x, y);
  Ok(point.to_physical(super::monitor::primary_monitor().scale_factor()))
}

pub unsafe fn superclass<'a>(this: &'a Object) -> &'a Class {
  let superclass: *const Class = msg_send![this, superclass];
  &*superclass
}

pub unsafe fn create_input_context(view: &NSView) -> IdRef {
  let input_context: id = msg_send![class!(NSTextInputContext), alloc];
  let input_context: id = msg_send![input_context, initWithClient: view];
  IdRef::new(input_context)
}

#[allow(dead_code)]
pub unsafe fn open_emoji_picker() {
  // SAFETY: TODO
  let mtm = unsafe { MainThreadMarker::new_unchecked() };
  let () = msg_send![&NSApp(mtm), orderFrontCharacterPalette: nil];
}

pub extern "C" fn yes(_: &Object, _: Sel) -> BOOL {
  YES
}

pub unsafe fn toggle_style_mask(
  window: &NSWindow,
  view: &NSView,
  mask: NSWindowStyleMask,
  on: bool,
) {
  let current_style_mask = window.styleMask();
  if on {
    window.setStyleMask(current_style_mask | mask);
  } else {
    window.setStyleMask(current_style_mask & (!mask));
  }

  // If we don't do this, key handling will break. Therefore, never call `setStyleMask` directly!
  window.makeFirstResponder(Some(view));
}
