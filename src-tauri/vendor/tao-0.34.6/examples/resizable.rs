// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{
  dpi::LogicalSize,
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::KeyCode,
  window::WindowBuilder,
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let mut resizable = false;

  let window = WindowBuilder::new()
    .with_title("Hit space to toggle resizability.")
    .with_inner_size(LogicalSize::new(400.0, 200.0))
    .with_resizable(resizable)
    .build(&event_loop)
    .unwrap();

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
        WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              physical_key: KeyCode::Space,
              state: ElementState::Released,
              ..
            },
          ..
        } => {
          resizable = !resizable;
          println!("Resizable: {resizable}");
          window.set_resizable(resizable);
        }
        _ => (),
      },
      _ => (),
    };
  });
}
