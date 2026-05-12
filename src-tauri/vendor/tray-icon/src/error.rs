// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use thiserror::Error;

/// Errors returned by tray-icon.
#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    OsError(#[from] std::io::Error),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[error(transparent)]
    PngEncodingError(#[from] png::EncodingError),
    #[error("not on the main thread")]
    NotMainThread,
}

/// Convenient type alias of Result type for tray-icon.
pub type Result<T> = std::result::Result<T, Error>;
