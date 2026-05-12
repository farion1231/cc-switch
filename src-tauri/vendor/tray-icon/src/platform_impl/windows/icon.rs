// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// taken from https://github.com/rust-windowing/winit/blob/92fdf5ba85f920262a61cee4590f4a11ad5738d1/src/platform_impl/windows/icon.rs

use std::{fmt, io, mem, path::Path, sync::Arc};

use windows_sys::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{
        CreateIcon, DestroyIcon, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE,
    },
};

use crate::icon::*;

use super::util;

impl Pixel {
    fn convert_to_bgra(&mut self) {
        mem::swap(&mut self.r, &mut self.b);
    }
}

impl RgbaIcon {
    fn into_windows_icon(self) -> Result<WinIcon, BadIcon> {
        let rgba = self.rgba;
        let pixel_count = rgba.len() / PIXEL_SIZE;
        let mut and_mask = Vec::with_capacity(pixel_count);
        let pixels =
            unsafe { std::slice::from_raw_parts_mut(rgba.as_ptr() as *mut Pixel, pixel_count) };
        for pixel in pixels {
            and_mask.push(pixel.a.wrapping_sub(u8::MAX)); // invert alpha channel
            pixel.convert_to_bgra();
        }
        assert_eq!(and_mask.len(), pixel_count);
        let handle = unsafe {
            CreateIcon(
                std::ptr::null_mut(),
                self.width as i32,
                self.height as i32,
                1,
                (PIXEL_SIZE * 8) as u8,
                and_mask.as_ptr(),
                rgba.as_ptr(),
            )
        };
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }
}

#[derive(Debug)]
struct RaiiIcon {
    handle: HICON,
}

#[derive(Clone)]
pub(crate) struct WinIcon {
    inner: Arc<RaiiIcon>,
}

unsafe impl Send for WinIcon {}

impl WinIcon {
    pub fn as_raw_handle(&self) -> HICON {
        self.inner.handle
    }

    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
        let rgba_icon = RgbaIcon::from_rgba(rgba, width, height)?;
        rgba_icon.into_windows_icon()
    }

    pub(crate) fn from_handle(handle: HICON) -> Self {
        Self {
            #[allow(clippy::arc_with_non_send_sync)]
            inner: Arc::new(RaiiIcon { handle }),
        }
    }

    pub(crate) fn from_path<P: AsRef<Path>>(
        path: P,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        // width / height of 0 along with LR_DEFAULTSIZE tells windows to load the default icon size
        let (width, height) = size.unwrap_or((0, 0));

        let wide_path = util::encode_wide(path.as_ref());

        let handle = unsafe {
            LoadImageW(
                std::ptr::null_mut(),
                wide_path.as_ptr(),
                IMAGE_ICON,
                width as i32,
                height as i32,
                LR_DEFAULTSIZE | LR_LOADFROMFILE,
            )
        };
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle as HICON))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }

    fn from_resource_inner_name(name: PCWSTR, size: Option<(u32, u32)>) -> Result<Self, BadIcon> {
        // width / height of 0 along with LR_DEFAULTSIZE tells windows to load the default icon size
        let (width, height) = size.unwrap_or((0, 0));
        let handle = unsafe {
            LoadImageW(
                util::get_instance_handle(),
                name,
                IMAGE_ICON,
                width as i32,
                height as i32,
                LR_DEFAULTSIZE,
            )
        };
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle as HICON))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }

    pub(crate) fn from_resource(
        resource_id: u16,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        Self::from_resource_inner_name(resource_id as PCWSTR, size)
    }

    pub(crate) fn from_resource_name(
        resource_name: &str,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        let wide_name = util::encode_wide(resource_name);
        Self::from_resource_inner_name(wide_name.as_ptr(), size)
    }
}

impl Drop for RaiiIcon {
    fn drop(&mut self) {
        unsafe { DestroyIcon(self.handle) };
    }
}

impl fmt::Debug for WinIcon {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        (*self.inner).fmt(formatter)
    }
}
