// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::io::{stdin, stdout, Write};

use tao::{
  event::{ElementState, Event, KeyEvent, WindowEvent},
  event_loop::{ControlFlow, EventLoop},
  keyboard::Key,
  monitor::{MonitorHandle, VideoMode},
  window::{Fullscreen, WindowBuilder},
};
#[allow(clippy::single_match)]
#[allow(clippy::ok_expect)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();

  print!("Please choose the fullscreen mode: (1) exclusive, (2) borderless, (3) borderless on current monitor: ");
  stdout().flush().unwrap();

  let mut num = String::new();
  stdin().read_line(&mut num).unwrap();
  let num = num.trim().parse().ok().expect("Please enter a number");

  let fullscreen = Some(match num {
    1 => Fullscreen::Exclusive(prompt_for_video_mode(&prompt_for_monitor(&event_loop))),
    2 => Fullscreen::Borderless(Some(prompt_for_monitor(&event_loop))),
    3 => Fullscreen::Borderless(None),
    _ => panic!("Please enter a valid number"),
  });

  let mut decorations = true;

  let window = WindowBuilder::new()
    .with_title("Hello world!")
    .with_fullscreen(fullscreen.clone())
    .build(&event_loop)
    .unwrap();

  event_loop.run(move |event, _, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
        WindowEvent::KeyboardInput {
          event:
            KeyEvent {
              logical_key,
              state: ElementState::Pressed,
              ..
            },
          ..
        } => {
          if Key::Escape == logical_key {
            *control_flow = ControlFlow::Exit
          }

          if Key::Character("f") == logical_key {
            if window.fullscreen().is_some() {
              window.set_fullscreen(None);
            } else {
              window.set_fullscreen(fullscreen.clone());
            }
          }

          if Key::Character("s") == logical_key {
            println!("window.fullscreen {:?}", window.fullscreen());
          }
          if Key::Character("m") == logical_key {
            let is_maximized = window.is_maximized();
            window.set_maximized(!is_maximized);
          }
          if Key::Character("d") == logical_key {
            decorations = !decorations;
            window.set_decorations(decorations);
          }
        }
        _ => (),
      },
      _ => {}
    }
  });
}

// Enumerate monitors and prompt user to choose one
fn prompt_for_monitor(event_loop: &EventLoop<()>) -> MonitorHandle {
  for (num, monitor) in event_loop.available_monitors().enumerate() {
    println!("Monitor #{}: {:?}", num, monitor.name());
  }

  print!("Please write the number of the monitor to use: ");
  stdout().flush().unwrap();

  let mut num = String::new();
  stdin().read_line(&mut num).unwrap();
  let num = num.trim().parse().expect("Please enter a number");
  let monitor = event_loop
    .available_monitors()
    .nth(num)
    .expect("Please enter a valid ID");

  println!("Using {:?}", monitor.name());

  monitor
}

fn prompt_for_video_mode(monitor: &MonitorHandle) -> VideoMode {
  for (i, video_mode) in monitor.video_modes().enumerate() {
    println!("Video mode #{i}: {video_mode}");
  }

  print!("Please write the number of the video mode to use: ");
  stdout().flush().unwrap();

  let mut num = String::new();
  stdin().read_line(&mut num).unwrap();
  let num = num.trim().parse().expect("Please enter a number");
  let video_mode = monitor
    .video_modes()
    .nth(num)
    .expect("Please enter a valid ID");

  println!("Using {video_mode}");

  video_mode
}
