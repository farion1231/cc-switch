// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn main() {
  use std::collections::HashMap;
  #[cfg(target_os = "macos")]
  use tao::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
  #[cfg(target_os = "linux")]
  use tao::platform::unix::{WindowBuilderExtUnix, WindowExtUnix};
  #[cfg(target_os = "windows")]
  use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
  use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
  };
  env_logger::init();
  let event_loop = EventLoop::new();
  let mut windows = HashMap::new();
  let main_window = WindowBuilder::new().build(&event_loop).unwrap();

  #[cfg(target_os = "macos")]
  let parent_window = main_window.ns_window();
  #[cfg(target_os = "windows")]
  let parent_window = main_window.hwnd();
  #[cfg(target_os = "linux")]
  let parent_window = main_window.gtk_window();

  let child_window_builder = WindowBuilder::new().with_inner_size(LogicalSize::new(200, 200));

  #[cfg(any(target_os = "windows", target_os = "macos"))]
  let child_window_builder = child_window_builder.with_parent_window(parent_window);

  #[cfg(target_os = "linux")]
  let child_window_builder = child_window_builder.with_transient_for(parent_window);

  let child_window = child_window_builder.build(&event_loop).unwrap();

  windows.insert(child_window.id(), child_window);
  windows.insert(main_window.id(), main_window);

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::NewEvents(StartCause::Init) => println!("TAO application started!"),
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        window_id,
        ..
      } => {
        println!("Window {window_id:?} has received the signal to close");
        // This drop the window, causing it to close.
        windows.remove(&window_id);
        if windows.is_empty() {
          *control_flow = ControlFlow::Exit;
        }
      }
      _ => (),
    };
  })
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn main() {
  println!("This platform doesn't have the parent window support.");
}
