// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

use crate::window::BadIcon;

/// An icon used for the window titlebar, taskbar, etc.
#[derive(Debug, Clone)]
pub struct PlatformIcon {
  raw: Vec<u8>,
  width: i32,
  height: i32,
  row_stride: i32,
}

impl From<PlatformIcon> for Pixbuf {
  fn from(icon: PlatformIcon) -> Self {
    Pixbuf::from_mut_slice(
      icon.raw,
      Colorspace::Rgb,
      true,
      8,
      icon.width,
      icon.height,
      icon.row_stride,
    )
  }
}

impl PlatformIcon {
  /// Creates an `Icon` from 32bpp RGBA data.
  ///
  /// The length of `rgba` must be divisible by 4, and `width * height` must equal
  /// `rgba.len() / 4`. Otherwise, this will return a `BadIcon` error.
  pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
    let row_stride =
      Pixbuf::calculate_rowstride(Colorspace::Rgb, true, 8, width as i32, height as i32);
    Ok(Self {
      raw: rgba,
      width: width as i32,
      height: height as i32,
      row_stride,
    })
  }
}
