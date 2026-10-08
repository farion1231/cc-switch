// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{event::KeyEvent, keyboard::KeyCode};

fn main() {
  use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Theme, WindowBuilder},
  };

  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new()
    .with_title("A fantastic window!")
    // .with_theme(Some(tao::window::Theme::Light))
    .build(&event_loop)
    .unwrap();

  println!("Initial theme: {:?}", window.theme());
  println!("Press D for Dark Mode");
  println!("Press L for Light Mode");
  println!("Press A for Auto Mode");

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    if let Event::WindowEvent { event, .. } = event {
      match event {
        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
        WindowEvent::KeyboardInput {
          event: KeyEvent { physical_key, .. },
          ..
        } => match physical_key {
          KeyCode::KeyD => window.set_theme(Some(Theme::Dark)),
          KeyCode::KeyL => window.set_theme(Some(Theme::Light)),
          KeyCode::KeyA => window.set_theme(None),
          _ => {}
        },
        WindowEvent::ThemeChanged(theme) => {
          println!("Theme is changed: {theme:?}")
        }
        _ => (),
      }
    }
  });
}
