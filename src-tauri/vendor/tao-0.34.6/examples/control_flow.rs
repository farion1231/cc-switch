// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{thread, time};

use tao::{
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::Key,
  window::WindowBuilder,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
  Wait,
  WaitUntil,
  Poll,
}

const WAIT_TIME: time::Duration = time::Duration::from_millis(100);
const POLL_SLEEP_TIME: time::Duration = time::Duration::from_millis(100);

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();

  println!("Press '1' to switch to Wait mode.");
  println!("Press '2' to switch to WaitUntil mode.");
  println!("Press '3' to switch to Poll mode.");
  println!("Press 'R' to toggle request_redraw() calls.");
  println!("Press 'Esc' to close the window.");

  let event_loop = EventLoop::new();
  let window = WindowBuilder::new()
    .with_title("Press 1, 2, 3 to change control flow mode. Press R to toggle redraw requests.")
    .build(&event_loop)
    .unwrap();

  let mut mode = Mode::Wait;
  let mut request_redraw = false;
  let mut wait_cancelled = false;
  let mut close_requested = false;

  event_loop.run(move |event, _, control_flow| {
    use tao::event::StartCause;
    println!("{event:?}");
    match event {
      Event::NewEvents(start_cause) => {
        wait_cancelled = match start_cause {
          StartCause::WaitCancelled { .. } => mode == Mode::WaitUntil,
          _ => false,
        }
      }
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::CloseRequested => {
          close_requested = true;
        }
        WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              logical_key,
              state: ElementState::Pressed,
              ..
            },
          ..
        } => {
          // WARNING: Consider using `key_without_modifers()` if available on your platform.
          // See the `key_binding` example
          if Key::Character("1") == logical_key {
            mode = Mode::Wait;
            println!("\nmode: {mode:?}\n");
          }
          if Key::Character("2") == logical_key {
            mode = Mode::WaitUntil;
            println!("\nmode: {mode:?}\n");
          }
          if Key::Character("3") == logical_key {
            mode = Mode::Poll;
            println!("\nmode: {mode:?}\n");
          }
          if Key::Character("r") == logical_key {
            request_redraw = !request_redraw;
            println!("\nrequest_redraw: {request_redraw}\n");
          }
          if Key::Escape == logical_key {
            close_requested = true;
          }
        }
        _ => {}
      },
      Event::MainEventsCleared => {
        if request_redraw && !wait_cancelled && !close_requested {
          window.request_redraw();
        }
        if close_requested {
          *control_flow = ControlFlow::Exit;
        }
      }
      Event::RedrawRequested(_window_id) => {}
      Event::RedrawEventsCleared => {
        *control_flow = match mode {
          Mode::Wait => ControlFlow::Wait,
          Mode::WaitUntil => {
            if wait_cancelled {
              *control_flow
            } else {
              ControlFlow::WaitUntil(time::Instant::now() + WAIT_TIME)
            }
          }
          Mode::Poll => {
            thread::sleep(POLL_SLEEP_TIME);
            ControlFlow::Poll
          }
        };
      }
      _ => (),
    }
  });
}
