// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{
  event::{DeviceEvent, ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::{Key, ModifiersState},
  window::WindowBuilder,
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new()
    .with_title("Super Cursor Grab'n'Hide Simulator 9000")
    .build(&event_loop)
    .unwrap();

  let mut modifiers = ModifiersState::default();

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
        WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              logical_key: key,
              state: ElementState::Released,
              ..
            },
          ..
        } => {
          // WARNING: Consider using `key_without_modifers()` if available on your platform.
          // See the `key_binding` example
          match key {
            Key::Escape => *control_flow = ControlFlow::Exit,
            Key::Character(ch) => match ch.to_lowercase().as_str() {
              "g" => window.set_cursor_grab(!modifiers.shift_key()).unwrap(),
              "h" => window.set_cursor_visible(modifiers.shift_key()),
              _ => (),
            },
            _ => (),
          }
        }
        WindowEvent::ModifiersChanged(m) => modifiers = m,
        _ => (),
      },
      Event::DeviceEvent { event, .. } => match event {
        DeviceEvent::MouseMotion { delta, .. } => println!("mouse moved: {delta:?}"),
        DeviceEvent::Button { button, state, .. } => match state {
          ElementState::Pressed => println!("mouse button {button} pressed"),
          ElementState::Released => println!("mouse button {button} released"),
          _ => (),
        },
        _ => (),
      },
      _ => (),
    }
  });
}
