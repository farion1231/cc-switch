// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, error::Error, fmt, os::raw::c_int, ptr};

use libc;
use parking_lot::Mutex;

use crate::window::CursorIcon;

use super::ffi;

/// A connection to an X server.
pub struct XConnection {
  pub xlib: ffi::Xlib,
  /// Exposes XRandR functions from version < 1.5
  pub xrandr: ffi::Xrandr_2_2_0,
  /// Exposes XRandR functions from version = 1.5
  pub xrandr_1_5: Option<ffi::Xrandr>,
  pub xcursor: ffi::Xcursor,
  pub xinput2: ffi::XInput2,
  pub xlib_xcb: ffi::Xlib_xcb,
  pub xrender: ffi::Xrender,
  pub display: *mut ffi::Display,
  pub x11_fd: c_int,
  pub latest_error: Mutex<Option<XError>>,
  pub cursor_cache: Mutex<HashMap<Option<CursorIcon>, ffi::Cursor>>,
}

unsafe impl Send for XConnection {}
unsafe impl Sync for XConnection {}

pub type XErrorHandler =
  Option<unsafe extern "C" fn(*mut ffi::Display, *mut ffi::XErrorEvent) -> libc::c_int>;

impl XConnection {
  pub fn new(error_handler: XErrorHandler) -> Result<XConnection, XNotSupported> {
    // opening the libraries
    let xlib = ffi::Xlib::open()?;
    let xcursor = ffi::Xcursor::open()?;
    let xrandr = ffi::Xrandr_2_2_0::open()?;
    let xrandr_1_5 = ffi::Xrandr::open().ok();
    let xinput2 = ffi::XInput2::open()?;
    let xlib_xcb = ffi::Xlib_xcb::open()?;
    let xrender = ffi::Xrender::open()?;

    unsafe { (xlib.XInitThreads)() };
    unsafe { (xlib.XSetErrorHandler)(error_handler) };

    // calling XOpenDisplay
    let display = unsafe {
      let display = (xlib.XOpenDisplay)(ptr::null());
      if display.is_null() {
        return Err(XNotSupported::XOpenDisplayFailed);
      }
      display
    };

    // Get X11 socket file descriptor
    let fd = unsafe { (xlib.XConnectionNumber)(display) };

    Ok(XConnection {
      xlib,
      xrandr,
      xrandr_1_5,
      xcursor,
      xinput2,
      xlib_xcb,
      xrender,
      display,
      x11_fd: fd,
      latest_error: Mutex::new(None),
      cursor_cache: Default::default(),
    })
  }

  /// Checks whether an error has been triggered by the previous function calls.
  #[inline]
  pub fn check_errors(&self) -> Result<(), XError> {
    let error = self.latest_error.lock().take();
    if let Some(error) = error {
      Err(error)
    } else {
      Ok(())
    }
  }

  /// Ignores any previous error.
  #[inline]
  pub fn ignore_error(&self) {
    *self.latest_error.lock() = None;
  }
}

impl fmt::Debug for XConnection {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.display.fmt(f)
  }
}

impl Drop for XConnection {
  #[inline]
  fn drop(&mut self) {
    unsafe { (self.xlib.XCloseDisplay)(self.display) };
  }
}

/// Error triggered by xlib.
#[derive(Debug, Clone)]
pub struct XError {
  pub description: String,
  pub error_code: u8,
  pub request_code: u8,
  pub minor_code: u8,
}

impl Error for XError {}

impl fmt::Display for XError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    write!(
      formatter,
      "X error: {} (code: {}, request code: {}, minor code: {})",
      self.description, self.error_code, self.request_code, self.minor_code
    )
  }
}

/// Error returned if this system doesn't have XLib or can't create an X connection.
#[derive(Clone, Debug)]
pub enum XNotSupported {
  /// Failed to load one or several shared libraries.
  LibraryOpenError(ffi::OpenError),
  /// Connecting to the X server with `XOpenDisplay` failed.
  XOpenDisplayFailed, // TODO: add better message
}

impl From<ffi::OpenError> for XNotSupported {
  #[inline]
  fn from(err: ffi::OpenError) -> XNotSupported {
    XNotSupported::LibraryOpenError(err)
  }
}

impl XNotSupported {
  fn description(&self) -> &'static str {
    match self {
      XNotSupported::LibraryOpenError(_) => "Failed to load one of xlib's shared libraries",
      XNotSupported::XOpenDisplayFailed => "Failed to open connection to X server",
    }
  }
}

impl Error for XNotSupported {
  #[inline]
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    match *self {
      XNotSupported::LibraryOpenError(ref err) => Some(err),
      _ => None,
    }
  }
}

impl fmt::Display for XNotSupported {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    formatter.write_str(self.description())
  }
}
