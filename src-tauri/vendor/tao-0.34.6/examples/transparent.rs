// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#[cfg(windows)]
use std::{num::NonZeroU32, rc::Rc};

use tao::{
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  window::WindowBuilder,
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new()
    .with_decorations(false)
    .with_transparent(true)
    .build(&event_loop)
    .unwrap();

  #[cfg(windows)]
  let (window, _context, mut surface) = {
    let window = Rc::new(window);
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
    (window, context, surface)
  };

  window.set_title("A fantastic window!");

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;
    println!("{event:?}");

    match event {
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        ..
      } => *control_flow = ControlFlow::Exit,

      #[cfg(windows)]
      Event::RedrawRequested(_) => {
        let (width, height) = {
          let size = window.inner_size();
          (size.width, size.height)
        };
        surface
          .resize(
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
          )
          .unwrap();

        let mut buffer = surface.buffer_mut().unwrap();
        buffer.fill(0);
        buffer.present().unwrap();
      }

      _ => (),
    }
  });
}
