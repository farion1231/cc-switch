// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use tao::{event_loop::EventLoop, window::WindowBuilder};

fn main() {
  env_logger::init();
  let event_loop = EventLoop::new();
  let window = WindowBuilder::new().build(&event_loop).unwrap();

  dbg!(window.available_monitors().collect::<Vec<_>>());
  dbg!(window.primary_monitor());
}
