// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use tao::{
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  window::Window,
};

fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let mut windows = HashMap::new();
  for _ in 0..3 {
    let window = Window::new(&event_loop).unwrap();
    windows.insert(window.id(), window);
  }

  event_loop.run(move |event, event_loop, control_flow| {
    *control_flow = ControlFlow::Wait;

    if let Event::WindowEvent {
      event, window_id, ..
    } = event
    {
      match event {
        WindowEvent::CloseRequested => {
          println!("Window {window_id:?} has received the signal to close");

          // This drops the window, causing it to close.
          windows.remove(&window_id);

          if windows.is_empty() {
            *control_flow = ControlFlow::Exit;
          }
        }
        WindowEvent::KeyboardInput {
          event: KeyEvent {
            state: ElementState::Pressed,
            ..
          },
          ..
        } => {
          let window = Window::new(event_loop).unwrap();
          windows.insert(window.id(), window);
        }
        _ => (),
      }
    }
  })
}
