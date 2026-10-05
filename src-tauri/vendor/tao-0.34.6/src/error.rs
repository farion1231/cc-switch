// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! The `Error` struct and associated types.
use std::{error, fmt};

use crate::platform_impl;

/// An error whose cause it outside Tao's control.
#[non_exhaustive]
#[derive(Debug)]
pub enum ExternalError {
  /// The operation is not supported by the backend.
  NotSupported(NotSupportedError),
  /// The OS cannot perform the operation.
  Os(OsError),
}

/// The error type for when the requested operation is not supported by the backend.
#[derive(Clone)]
pub struct NotSupportedError {
  _marker: (),
}

/// The error type for when the OS cannot perform the requested operation.
#[derive(Debug)]
pub struct OsError {
  line: u32,
  file: &'static str,
  error: platform_impl::OsError,
}

impl NotSupportedError {
  #[inline]
  #[allow(dead_code)]
  pub(crate) fn new() -> NotSupportedError {
    NotSupportedError { _marker: () }
  }
}

impl OsError {
  #[allow(dead_code)]
  pub(crate) fn new(line: u32, file: &'static str, error: platform_impl::OsError) -> OsError {
    OsError { line, file, error }
  }
}

#[allow(unused_macros)]
macro_rules! os_error {
  ($error:expr) => {{
    crate::error::OsError::new(line!(), file!(), $error)
  }};
}

impl fmt::Display for OsError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    f.pad(&format!(
      "os error at {}:{}: {}",
      self.file, self.line, self.error
    ))
  }
}

impl fmt::Display for ExternalError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    match self {
      ExternalError::NotSupported(e) => e.fmt(f),
      ExternalError::Os(e) => e.fmt(f),
    }
  }
}

impl fmt::Debug for NotSupportedError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    f.debug_struct("NotSupportedError").finish()
  }
}

impl fmt::Display for NotSupportedError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    f.pad("the requested operation is not supported by Tao")
  }
}

impl error::Error for OsError {}
impl error::Error for ExternalError {}
impl error::Error for NotSupportedError {}
