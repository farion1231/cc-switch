// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{fs::File, io::BufWriter, path::Path};

use crate::icon::BadIcon;

#[derive(Debug, Clone)]
pub struct PlatformIcon {
    rgba: Vec<u8>,
    width: i32,
    height: i32,
}

impl PlatformIcon {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
        Ok(Self {
            rgba,
            width: width as i32,
            height: height as i32,
        })
    }

    pub fn write_to_png(&self, path: impl AsRef<Path>) -> crate::Result<()> {
        let png = File::create(path)?;
        let w = &mut BufWriter::new(png);

        let mut encoder = png::Encoder::new(w, self.width as _, self.height as _);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;

        Ok(())
    }
}
