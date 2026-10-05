// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

extern crate image;
use std::path::Path;

use tao::{
  event::Event,
  event_loop::{ControlFlow, EventLoop},
  window::{Icon, WindowBuilder},
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();

  // You'll have to choose an icon size at your own discretion. On Linux, the icon should be
  // provided in whatever size it was naturally drawn; that is, don’t scale the image before passing
  // it to Tao. But on Windows, you will have to account for screen scaling. Here we use 32px,
  // since it seems to work well enough in most cases. Be careful about going too high, or
  // you'll be bitten by the low-quality downscaling built into the WM.
  let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/icon.png");

  let icon = load_icon(Path::new(path));

  let event_loop = EventLoop::new();

  let window = WindowBuilder::new()
    .with_title("An iconic window!")
    // At present, this only does anything on Windows and Linux, so if you want to save load
    // time, you can put icon loading behind a function that returns `None` on other platforms.
    .with_window_icon(Some(icon))
    .build(&event_loop)
    .unwrap();

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    if let Event::WindowEvent { event, .. } = event {
      use tao::event::WindowEvent::*;
      match event {
        CloseRequested => *control_flow = ControlFlow::Exit,
        DroppedFile(path) => {
          window.set_window_icon(Some(load_icon(&path)));
        }
        _ => (),
      }
    }
  });
}

fn load_icon(path: &Path) -> Icon {
  let (icon_rgba, icon_width, icon_height) = {
    // alternatively, you can embed the icon in the binary through `include_bytes!` macro and use `image::load_from_memory`
    let image = image::open(path)
      .expect("Failed to open icon path")
      .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    (rgba, width, height)
  };
  Icon::from_rgba(icon_rgba, icon_width, icon_height).expect("Failed to open icon")
}
