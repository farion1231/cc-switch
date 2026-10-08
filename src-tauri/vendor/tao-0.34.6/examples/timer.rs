// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use tao::{
  event::{Event, StartCause, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  window::WindowBuilder,
};

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let _window = WindowBuilder::new()
    .with_title("A fantastic window!")
    .build(&event_loop)
    .unwrap();

  let timer_length = Duration::new(1, 0);

  event_loop.run(move |event, _, control_flow| {
    println!("{event:?}");

    match event {
      Event::NewEvents(StartCause::Init) => {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + timer_length)
      }
      Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + timer_length);
        println!("\nTimer\n");
      }
      Event::WindowEvent {
        event: WindowEvent::CloseRequested,
        ..
      } => *control_flow = ControlFlow::Exit,
      _ => (),
    }
  });
}
