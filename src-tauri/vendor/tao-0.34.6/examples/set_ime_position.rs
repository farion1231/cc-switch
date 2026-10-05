// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{
  dpi::PhysicalPosition,
  event::{ElementState, Event, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  window::WindowBuilder,
};

fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new().build(&event_loop).unwrap();
  window.set_title("A fantastic window!");

  println!("Ime position will system default");
  println!("Click to set ime position to cursor's");

  let mut cursor_position = PhysicalPosition::new(0.0, 0.0);
  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent {
        event: WindowEvent::CursorMoved { position, .. },
        ..
      } => {
        cursor_position = position;
      }
      Event::WindowEvent {
        event:
          WindowEvent::MouseInput {
            state: ElementState::Released,
            ..
          },
        ..
      } => {
        println!(
          "Setting ime position to {}, {}",
          cursor_position.x, cursor_position.y
        );
        window.set_ime_position(cursor_position);
      }
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        ..
      } => {
        *control_flow = ControlFlow::Exit;
      }
      _ => (),
    }
  });
}
