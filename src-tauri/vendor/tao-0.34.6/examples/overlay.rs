// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::{Key, ModifiersState},
  window::WindowBuilder,
};

#[cfg(any(
  target_os = "linux",
  target_os = "dragonfly",
  target_os = "freebsd",
  target_os = "netbsd",
  target_os = "openbsd"
))]
use tao::platform::unix::WindowExtUnix;

#[cfg(target_os = "macos")]
use tao::platform::macos::WindowExtMacOS;

#[cfg(target_os = "ios")]
use tao::platform::ios::WindowExtIOS;

#[cfg(windows)]
use tao::{
  dpi::PhysicalSize, platform::windows::IconExtWindows, platform::windows::WindowExtWindows,
  window::Icon,
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new().build(&event_loop).unwrap();

  let mut modifiers = ModifiersState::default();

  eprintln!("Key mappings:");
  #[cfg(windows)]
  eprintln!("  [any key]: Show the Overlay Icon");
  #[cfg(not(windows))]
  eprintln!("  [1-5]: Show a Badge count");
  eprintln!("  Ctrl+1: Clear");

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
          let _count = match key_str {
            "1" => 1,
            "2" => 2,
            "3" => 3,
            "4" => 4,
            "5" => 5,
            _ => 20,
          };

          if modifiers.is_empty() {
            #[cfg(windows)]
            {
              let mut path = std::env::current_dir().unwrap();
              path.push("./examples/icon.ico");
              let icon = Icon::from_path(path, Some(PhysicalSize::new(32, 32))).unwrap();

              window.set_overlay_icon(Some(&icon));
            }

            #[cfg(any(
              target_os = "linux",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "netbsd",
              target_os = "openbsd"
            ))]
            window.set_badge_count(Some(_count), None);

            #[cfg(target_os = "macos")]
            window.set_badge_label(_count.to_string().into());

            #[cfg(target_os = "ios")]
            window.set_badge_count(_count);
          } else if modifiers.control_key() && key_str == "1" {
            #[cfg(windows)]
            window.set_overlay_icon(None);

            #[cfg(any(
              target_os = "linux",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "netbsd",
              target_os = "openbsd"
            ))]
            window.set_badge_count(None, None);

            #[cfg(target_os = "macos")]
            window.set_badge_label(None);

            #[cfg(target_os = "ios")]
            window.set_badge_count(0);
          }
        }
        _ => {}
      },
      _ => {}
    }
  });
}
