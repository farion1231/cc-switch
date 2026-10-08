// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::{Key, ModifiersState},
  window::{ProgressBarState, ProgressState, WindowBuilder},
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new().build(&event_loop).unwrap();

  let mut modifiers = ModifiersState::default();

  eprintln!("Key mappings:");
  eprintln!("  [1-5]: Set progress to [0%, 25%, 50%, 75%, 100%]");
  eprintln!("  Ctrl+1: Set state to None");
  eprintln!("  Ctrl+2: Set state to Normal");
  eprintln!("  Ctrl+3: Set state to Indeterminate");
  eprintln!("  Ctrl+4: Set state to Paused");
  eprintln!("  Ctrl+5: Set state to Error");

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        ..
      } => *control_flow = ControlFlow::Exit,
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::ModifiersChanged(new_state) => {
          modifiers = new_state;
        }
        WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              logical_key: Key::Character(key_str),
              state: ElementState::Released,
              ..
            },
          ..
        } => {
          if modifiers.is_empty() {
            let mut progress: u64 = 0;
            match key_str {
              "1" => progress = 0,
              "2" => progress = 25,
              "3" => progress = 50,
              "4" => progress = 75,
              "5" => progress = 100,
              _ => {}
            }

            window.set_progress_bar(ProgressBarState {
              progress: Some(progress),
              state: Some(ProgressState::Normal),
              desktop_filename: None,
            });
          } else if modifiers.control_key() {
            let mut state = ProgressState::None;
            match key_str {
              "1" => state = ProgressState::None,
              "2" => state = ProgressState::Normal,
              "3" => state = ProgressState::Indeterminate,
              "4" => state = ProgressState::Paused,
              "5" => state = ProgressState::Error,
              _ => {}
            }

            window.set_progress_bar(ProgressBarState {
              progress: None,
              state: Some(state),
              desktop_filename: None,
            });
          }
        }
        _ => {}
      },
      _ => {}
    }
  });
}
