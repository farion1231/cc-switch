// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::event_loop::EventLoop;

#[allow(clippy::single_match)]
fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();
  let monitor = match event_loop.primary_monitor() {
    Some(monitor) => monitor,
    None => {
      println!("No primary monitor detected.");
      return;
    }
  };

  println!("Listing available video modes:");

  for mode in monitor.video_modes() {
    println!("{mode}");
  }
}
