// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! Tao is a cross-platform application window creation and event loop management library.
//!
//! # Building windows
//!
//! Before you can build a [`Window`], you first need to build an [`EventLoop`]. This is done with the
//! [`EventLoop::new()`] function.
//!
//! ```no_run
//! use tao::event_loop::EventLoop;
//! let event_loop = EventLoop::new();
//! ```
//!
//! Once this is done there are two ways to create a [`Window`]:
//!
//!  - Calling [`Window::new(&event_loop)`][window_new].
//!  - Calling [`let builder = WindowBuilder::new()`][window_builder_new] then [`builder.build(&event_loop)`][window_builder_build].
//!
//! The first method is the simplest, and will give you default values for everything. The second
//! method allows you to customize the way your [`Window`] will look and behave by modifying the
//! fields of the [`WindowBuilder`] object before you create the [`Window`].
//!
//! # Event handling
//!
//! Once a [`Window`] has been created, it will generate different *events*. A [`Window`] object can
//! generate [`WindowEvent`]s when certain input events occur, such as a cursor moving over the
//! window or a key getting pressed while the window is focused. Devices can generate
//! [`DeviceEvent`]s, which contain unfiltered event data that isn't specific to a certain window.
//! Some user activity, like mouse movement, can generate both a [`WindowEvent`] *and* a
//! [`DeviceEvent`]. You can also create and handle your own custom [`UserEvent`]s, if desired.
//!
//! You can retrieve events by calling [`EventLoop::run`][event_loop_run]. This function will
//! dispatch events for every [`Window`] that was created with that particular [`EventLoop`], and
//! will run until the `control_flow` argument given to the closure is set to
//! [`ControlFlow`]`::`[`ExitWithCode`] (which [`ControlFlow`]`::`[`Exit`] aliases to), at which
//! point [`Event`]`::`[`LoopDestroyed`] is emitted and the entire program terminates.
//!
//! Tao no longer uses a `EventLoop::poll_events() -> impl Iterator<Event>`-based event loop
//! model, since that can't be implemented properly on some platforms (e.g web, iOS) and works poorly on
//! most other platforms. However, this model can be re-implemented to an extent with
//! [`EventLoopExtRunReturn::run_return`]. See that method's documentation for more reasons about why
//! it's discouraged, beyond compatibility reasons.
//!
//!
//! ```no_run
//! use tao::{
//!     event::{Event, WindowEvent},
//!     event_loop::{ControlFlow, EventLoop},
//!     window::WindowBuilder,
//! };
//!
//! let event_loop = EventLoop::new();
//! let window = WindowBuilder::new().build(&event_loop).unwrap();
//!
//! event_loop.run(move |event, _, control_flow| {
//!     // ControlFlow::Poll continuously runs the event loop, even if the OS hasn't
//!     // dispatched any events. This is ideal for games and similar applications.
//!     *control_flow = ControlFlow::Poll;
//!
//!     // ControlFlow::Wait pauses the event loop if no events are available to process.
//!     // This is ideal for non-game applications that only update in response to user
//!     // input, and uses significantly less power/CPU time than ControlFlow::Poll.
//!     *control_flow = ControlFlow::Wait;
//!
//!     match event {
//!         Event::WindowEvent {
//!             event: WindowEvent::CloseRequested,
//!             ..
//!         } => {
//!             println!("The close button was pressed; stopping");
//!             *control_flow = ControlFlow::Exit
//!         },
//!         Event::MainEventsCleared => {
//!             // Application update code.
//!
//!             // Queue a RedrawRequested event.
//!             //
//!             // You only need to call this if you've determined that you need to redraw, in
//!             // applications which do not always need to. Applications that redraw continuously
//!             // can just render here instead.
//!             window.request_redraw();
//!         },
//!         Event::RedrawRequested(_) => {
//!             // Redraw the application.
//!             //
//!             // It's preferable for applications that do not render continuously to render in
//!             // this event rather than in MainEventsCleared, since rendering in here allows
//!             // the program to gracefully handle redraws requested by the OS.
//!         },
//!         _ => ()
//!     }
//! });
//! ```
//!
//! [`Event`]`::`[`WindowEvent`] has a [`WindowId`] member. In multi-window environments, it should be
//! compared to the value returned by [`Window::id()`][window_id_fn] to determine which [`Window`]
//! dispatched the event.
//!
//! # Drawing on the window
//!
//! Tao doesn't directly provide any methods for drawing on a [`Window`]. However it allows you to
//! retrieve the raw handle of the window and display (see the [`platform`] module and/or the
//! [`raw_window_handle`] and [`raw_display_handle`] methods), which in turn allows you to create an
//! OpenGL/Vulkan/DirectX/Metal/etc. context that can be used to render graphics.
//!
//! Note that many platforms will display garbage data in the window's client area if the
//! application doesn't render anything to the window by the time the desktop compositor is ready to
//! display the window to the user. If you notice this happening, you should create the window with
//! [`visible` set to `false`](crate::window::WindowBuilder::with_visible) and explicitly make the
//! window visible only once you're ready to render into it.
//!
//! [`EventLoop`]: event_loop::EventLoop
//! [`EventLoopExtRunReturn::run_return`]: ./platform/run_return/trait.EventLoopExtRunReturn.html#tymethod.run_return
//! [`EventLoop::new()`]: event_loop::EventLoop::new
//! [event_loop_run]: event_loop::EventLoop::run
//! [`ControlFlow`]: event_loop::ControlFlow
//! [`Exit`]: event_loop::ControlFlow::Exit
//! [`ExitWithCode`]: event_loop::ControlFlow::ExitWithCode
//! [`Window`]: window::Window
//! [`WindowId`]: window::WindowId
//! [`WindowBuilder`]: window::WindowBuilder
//! [window_new]: window::Window::new
//! [window_builder_new]: window::WindowBuilder::new
//! [window_builder_build]: window::WindowBuilder::build
//! [window_id_fn]: window::Window::id
//! [`Event`]: event::Event
//! [`WindowEvent`]: event::WindowEvent
//! [`DeviceEvent`]: event::DeviceEvent
//! [`UserEvent`]: event::Event::UserEvent
//! [`LoopDestroyed`]: event::Event::LoopDestroyed
//! [`platform`]: platform
//! [`raw_window_handle`]: ./window/struct.Window.html#method.raw_window_handle
//! [`raw_display_handle`]: ./window/struct.Window.html#method.raw_display_handle
#![allow(
  clippy::match_str_case_mismatch,
  clippy::upper_case_acronyms,
  clippy::from_over_into,
  clippy::option_map_unit_fn,
  clippy::needless_lifetimes,
  clippy::type_complexity,
  clippy::identity_op,
  clippy::wrong_self_convention,
  clippy::non_send_fields_in_send_ty
)]
#![deny(rustdoc::broken_intra_doc_links)]

use dpi::PixelUnit;
#[cfg(feature = "rwh_04")]
pub use rwh_04;
#[cfg(feature = "rwh_05")]
pub use rwh_05;
#[cfg(feature = "rwh_06")]
pub use rwh_06;

#[allow(unused_imports)]
#[macro_use]
extern crate log;
#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;
#[macro_use]
extern crate bitflags;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[macro_use(class, msg_send, sel)]
extern crate objc2;

pub use dpi;

#[macro_use]
pub mod error;
pub mod event;
pub mod event_loop;
mod icon;
pub mod keyboard;
pub mod monitor;
mod platform_impl;

pub mod window;

pub mod platform;

pub(crate) fn extract_width_height(size: dpi::Size) -> (PixelUnit, PixelUnit) {
  match size {
    dpi::Size::Physical(size) => (
      PixelUnit::Physical(size.width.into()),
      PixelUnit::Physical(size.height.into()),
    ),
    dpi::Size::Logical(size) => (
      PixelUnit::Logical(size.width.into()),
      PixelUnit::Logical(size.height.into()),
    ),
  }
}
