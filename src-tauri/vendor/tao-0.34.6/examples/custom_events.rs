// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
  };

  #[derive(Debug, Clone, Copy)]
  enum CustomEvent {
    Timer,
  }

  let event_loop = EventLoopBuilder::<CustomEvent>::with_user_event().build();

  let _window = WindowBuilder::new()
    .with_title("A fantastic window!")
    .build(&event_loop)
    .unwrap();

  // `EventLoopProxy` allows you to dispatch custom events to the main Tao event
  // loop from any thread.
  let event_loop_proxy = event_loop.create_proxy();

  std::thread::spawn(move || {
    // Wake up the `event_loop` once every second and dispatch a custom event
    // from a different thread.
    loop {
      std::thread::sleep(std::time::Duration::from_secs(1));
      event_loop_proxy.send_event(CustomEvent::Timer).ok();
    }
  });

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::UserEvent(event) => println!("user event: {event:?}"),
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        ..
      } => *control_flow = ControlFlow::Exit,
      _ => (),
    }
  });
}
