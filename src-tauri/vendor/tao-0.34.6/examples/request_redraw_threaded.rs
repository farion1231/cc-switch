// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{thread, time};

use tao::{
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  window::WindowBuilder,
};

#[allow(clippy::single_match)]
#[allow(clippy::collapsible_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  let window = WindowBuilder::new()
    .with_title("A fantastic window!")
    .build(&event_loop)
    .unwrap();

  thread::spawn(move || loop {
    thread::sleep(time::Duration::from_secs(1));
    window.request_redraw();
  });

  event_loop.run(move |event, _, control_flow| {
    println!("{event:?}");

    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
        _ => (),
      },
      Event::RedrawRequested(_) => {
        println!("\nredrawing!\n");
      }
      _ => (),
    }
  });
}
