// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#[allow(clippy::single_match)]
#[allow(clippy::iter_nth)]
fn main() {
  use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

  use tao::{
    dpi::{PhysicalPosition, PhysicalSize, Position, Size},
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState},
    window::{CursorIcon, Fullscreen, WindowBuilder},
  };

  const WINDOW_COUNT: usize = 3;
  const WINDOW_SIZE: PhysicalSize<u32> = PhysicalSize::new(600, 400);

  env_logger::init();
  let event_loop = EventLoop::new();
  let mut window_senders = HashMap::with_capacity(WINDOW_COUNT);
  for _ in 0..WINDOW_COUNT {
    let window = WindowBuilder::new()
      .with_inner_size(WINDOW_SIZE)
      .build(&event_loop)
      .unwrap();

    let mut video_modes: Vec<_> = window.current_monitor().unwrap().video_modes().collect();
    let mut video_mode_id = 0usize;

    let (tx, rx) = mpsc::channel();
    window_senders.insert(window.id(), tx);
    let mut modifiers = ModifiersState::default();
    thread::spawn(move || {
      while let Ok(event) = rx.recv() {
        match event {
          WindowEvent::Moved { .. } => {
            // We need to update our chosen video mode if the window
            // was moved to an another monitor, so that the window
            // appears on this monitor instead when we go fullscreen
            let previous_video_mode = video_modes.iter().nth(video_mode_id).cloned();
            video_modes = window.current_monitor().unwrap().video_modes().collect();
            video_mode_id = video_mode_id.min(video_modes.len());
            let video_mode = video_modes.iter().nth(video_mode_id);

            // Different monitors may support different video modes,
            // and the index we chose previously may now point to a
            // completely different video mode, so notify the user
            if video_mode != previous_video_mode.as_ref() {
              println!(
                "Window moved to another monitor, picked video mode: {}",
                video_modes.iter().nth(video_mode_id).unwrap()
              );
            }
          }
          WindowEvent::ModifiersChanged(mod_state) => {
            modifiers = mod_state;
          }
          WindowEvent::KeyboardInput {
            event:
              KeyEvent {
                state: ElementState::Released,
                logical_key: key,
                ..
              },
            ..
          } => {
            use Key::{ArrowLeft, ArrowRight, Character};
            window.set_title(&format!("{key:?}"));
            let state = !modifiers.shift_key();
            match &key {
              // WARNING: Consider using `key_without_modifers()` if available on your platform.
              // See the `key_binding` example
              Character(string) => match string.to_lowercase().as_str() {
                "a" => window.set_always_on_top(state),
                "c" => window.set_cursor_icon(match state {
                  true => CursorIcon::Progress,
                  false => CursorIcon::Default,
                }),
                "d" => window.set_decorations(!state),
                "f" => window.set_fullscreen(match (state, modifiers.alt_key()) {
                  (true, false) => Some(Fullscreen::Borderless(None)),
                  (true, true) => Some(Fullscreen::Exclusive(
                    video_modes.iter().nth(video_mode_id).unwrap().clone(),
                  )),
                  (false, _) => None,
                }),
                "g" => window.set_cursor_grab(state).unwrap(),
                "h" => window.set_cursor_visible(!state),
                "i" => {
                  println!("Info:");
                  println!("-> outer_position : {:?}", window.outer_position());
                  println!("-> inner_position : {:?}", window.inner_position());
                  println!("-> outer_size     : {:?}", window.outer_size());
                  println!("-> inner_size     : {:?}", window.inner_size());
                  println!("-> fullscreen     : {:?}", window.fullscreen());
                }
                "l" => window.set_min_inner_size(match state {
                  true => Some(WINDOW_SIZE),
                  false => None,
                }),
                "m" => window.set_maximized(state),
                "p" => window.set_outer_position({
                  let mut position = window.outer_position().unwrap();
                  let sign = if state { 1 } else { -1 };
                  position.x += 10 * sign;
                  position.y += 10 * sign;
                  position
                }),
                "q" => window.request_redraw(),
                "r" => window.set_resizable(state),
                "s" => window.set_inner_size(match state {
                  true => PhysicalSize::new(WINDOW_SIZE.width + 100, WINDOW_SIZE.height + 100),
                  false => WINDOW_SIZE,
                }),
                "w" => {
                  if let Size::Physical(size) = WINDOW_SIZE.into() {
                    window
                      .set_cursor_position(Position::Physical(PhysicalPosition::new(
                        size.width as i32 / 2,
                        size.height as i32 / 2,
                      )))
                      .unwrap()
                  }
                }
                "z" => {
                  window.set_visible(false);
                  thread::sleep(Duration::from_secs(1));
                  window.set_visible(true);
                }
                _ => (),
              },
              ArrowRight | ArrowLeft => {
                video_mode_id = match &key {
                  ArrowLeft => video_mode_id.saturating_sub(1),
                  ArrowRight => (video_modes.len() - 1).min(video_mode_id + 1),
                  _ => unreachable!(),
                };
                println!(
                  "Picking video mode: {}",
                  video_modes.iter().nth(video_mode_id).unwrap()
                );
              }
              _ => (),
            }
          }
          _ => (),
        }
      }
    });
  }
  event_loop.run(move |event, _event_loop, control_flow| {
    *control_flow = match !window_senders.is_empty() {
      true => ControlFlow::Wait,
      false => ControlFlow::Exit,
    };
    match event {
      Event::WindowEvent {
        event, window_id, ..
      } => match event {
        WindowEvent::CloseRequested
        | WindowEvent::Destroyed
        | WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              state: ElementState::Released,
              logical_key: Key::Escape,
              ..
            },
          ..
        } => {
          window_senders.remove(&window_id);
        }
        _ => {
          if let Some(tx) = window_senders.get(&window_id) {
            if let Some(event) = event.to_static() {
              tx.send(event).unwrap();
            }
          }
        }
      },
      _ => (),
    }
  })
}
