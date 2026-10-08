// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! The `EventLoop` struct and assorted supporting types, including `ControlFlow`.
//!
//! If you want to send custom events to the event loop, use [`EventLoop::create_proxy()`][create_proxy]
//! to acquire an [`EventLoopProxy`][event_loop_proxy] and call its [`send_event`][send_event] method.
//!
//! See the root-level documentation for information on how to create and use an event loop to
//! handle events.
//!
//! [create_proxy]: crate::event_loop::EventLoop::create_proxy
//! [event_loop_proxy]: crate::event_loop::EventLoopProxy
//! [send_event]: crate::event_loop::EventLoopProxy::send_event
use std::{error, fmt, marker::PhantomData, ops::Deref, time::Instant};

use crate::{
  dpi::PhysicalPosition,
  error::ExternalError,
  event::Event,
  monitor::MonitorHandle,
  platform_impl,
  window::{ProgressBarState, Theme},
};

/// Provides a way to retrieve events from the system and from the windows that were registered to
/// the events loop.
///
/// An `EventLoop` can be seen more or less as a "context". Calling `EventLoop::new()`
/// initializes everything that will be required to create windows.
///
/// To wake up an `EventLoop` from a another thread, see the `EventLoopProxy` docs.
///
/// Note that the `EventLoop` cannot be shared across threads (due to platform-dependant logic
/// forbidding it), as such it is neither `Send` nor `Sync`. If you need cross-thread access, the
/// `Window` created from this `EventLoop` _can_ be sent to an other thread, and the
/// `EventLoopProxy` allows you to wake up an `EventLoop` from another thread.
///
pub struct EventLoop<T: 'static> {
  pub(crate) event_loop: platform_impl::EventLoop<T>,
  pub(crate) _marker: ::std::marker::PhantomData<*mut ()>, // Not Send nor Sync
}

/// Target that associates windows with an `EventLoop`.
///
/// This type exists to allow you to create new windows while Tao executes
/// your callback. `EventLoop` will coerce into this type (`impl<T> Deref for
/// EventLoop<T>`), so functions that take this as a parameter can also take
/// `&EventLoop`.
#[derive(Clone)]
pub struct EventLoopWindowTarget<T: 'static> {
  pub(crate) p: platform_impl::EventLoopWindowTarget<T>,
  pub(crate) _marker: ::std::marker::PhantomData<*mut ()>, // Not Send nor Sync
}

impl<T> fmt::Debug for EventLoop<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad("EventLoop { .. }")
  }
}

impl<T> fmt::Debug for EventLoopWindowTarget<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad("EventLoopWindowTarget { .. }")
  }
}

/// Object that allows building the event loop.
///
/// This is used to make specifying options that affect the whole application
/// easier. But note that constructing multiple event loops is not supported.
#[derive(Default)]
pub struct EventLoopBuilder<T: 'static> {
  pub(crate) platform_specific: platform_impl::PlatformSpecificEventLoopAttributes,
  _p: PhantomData<T>,
}
impl EventLoopBuilder<()> {
  /// Start building a new event loop.
  #[inline]
  pub fn new() -> Self {
    Self::with_user_event()
  }
}
impl<T> EventLoopBuilder<T> {
  /// Start building a new event loop, with the given type as the user event
  /// type.
  #[inline]
  pub fn with_user_event() -> Self {
    Self {
      platform_specific: Default::default(),
      _p: PhantomData,
    }
  }
  /// Builds a new event loop.
  ///
  /// ***For cross-platform compatibility, the `EventLoop` must be created on the main thread.***
  /// Attempting to create the event loop on a different thread will panic. This restriction isn't
  /// strictly necessary on all platforms, but is imposed to eliminate any nasty surprises when
  /// porting to platforms that require it. `EventLoopBuilderExt::any_thread` functions are exposed
  /// in the relevant `platform` module if the target platform supports creating an event loop on
  /// any thread.
  ///
  /// Usage will result in display backend initialisation, this can be controlled on linux
  /// using an environment variable `WINIT_UNIX_BACKEND`. Legal values are `x11` and `wayland`.
  /// If it is not set, winit will try to connect to a wayland connection, and if it fails will
  /// fallback on x11. If this variable is set with any other value, winit will panic.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread.
  #[inline]
  pub fn build(&mut self) -> EventLoop<T> {
    EventLoop {
      #[cfg_attr(
        any(
          target_os = "linux",
          target_os = "dragonfly",
          target_os = "freebsd",
          target_os = "netbsd",
          target_os = "openbsd"
        ),
        allow(clippy::unnecessary_mut_passed)
      )]
      event_loop: platform_impl::EventLoop::new(&mut self.platform_specific),
      _marker: PhantomData,
    }
  }
}

/// Set by the user callback given to the `EventLoop::run` method.
///
/// Indicates the desired behavior of the event loop after [`Event::RedrawEventsCleared`][events_cleared]
/// is emitted. Defaults to `Poll`.
///
/// ## Persistency
/// Almost every change is persistent between multiple calls to the event loop closure within a
/// given run loop. The only exception to this is `ExitWithCode` which, once set, cannot be unset.
/// Changes are **not** persistent between multiple calls to `run_return` - issuing a new call will
/// reset the control flow to `Poll`.
///
/// [events_cleared]: crate::event::Event::RedrawEventsCleared
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ControlFlow {
  /// When the current loop iteration finishes, immediately begin a new iteration regardless of
  /// whether or not new events are available to process.
  Poll,
  /// When the current loop iteration finishes, suspend the thread until another event arrives.
  Wait,
  /// When the current loop iteration finishes, suspend the thread until either another event
  /// arrives or the given time is reached.
  WaitUntil(Instant),
  /// Send a `LoopDestroyed` event and stop the event loop. This variant is *sticky* - once set,
  /// `control_flow` cannot be changed from `ExitWithCode`, and any future attempts to do so will
  /// result in the `control_flow` parameter being reset to `ExitWithCode`.
  ///
  /// The contained number will be used as exit code. The [`Exit`] constant is a shortcut for this
  /// with exit code 0.
  ///
  /// ## Platform-specific
  ///
  /// - **Android / iOS / WASM**: The supplied exit code is unused.
  /// - **Unix**: On most Unix-like platforms, only the 8 least significant bits will be used,
  ///   which can cause surprises with negative exit values (`-42` would end up as `214`). See
  ///   [`std::process::exit`].
  ///
  /// [`Exit`]: ControlFlow::Exit
  ExitWithCode(i32),
}

impl ControlFlow {
  /// Alias for [`ExitWithCode`]`(0)`.
  ///
  /// [`ExitWithCode`]: ControlFlow::ExitWithCode
  #[allow(non_upper_case_globals)]
  pub const Exit: Self = Self::ExitWithCode(0);
}

impl Default for ControlFlow {
  #[inline(always)]
  fn default() -> ControlFlow {
    ControlFlow::Poll
  }
}

impl EventLoop<()> {
  /// Alias for [`EventLoopBuilder::new().build()`].
  ///
  /// [`EventLoopBuilder::new().build()`]: EventLoopBuilder::build
  #[inline]
  pub fn new() -> EventLoop<()> {
    EventLoopBuilder::new().build()
  }
}

impl Default for EventLoop<()> {
  fn default() -> Self {
    Self::new()
  }
}

impl<T> EventLoop<T> {
  /// Hijacks the calling thread and initializes the tao event loop with the provided
  /// closure. Since the closure is `'static`, it must be a `move` closure if it needs to
  /// access any data from the calling context.
  ///
  /// See the [`ControlFlow`] docs for information on how changes to `&mut ControlFlow` impact the
  /// event loop's behavior.
  ///
  /// Any values not passed to this function will *not* be dropped.
  ///
  /// ## Platform-specific
  ///
  /// - **Unix**: The program terminates with exit code 1 if the display server
  ///   disconnects.
  ///
  /// [`ControlFlow`]: crate::event_loop::ControlFlow
  #[inline]
  pub fn run<F>(self, event_handler: F) -> !
  where
    F: 'static + FnMut(Event<'_, T>, &EventLoopWindowTarget<T>, &mut ControlFlow),
  {
    self.event_loop.run(event_handler)
  }

  /// Creates an `EventLoopProxy` that can be used to dispatch user events to the main event loop.
  pub fn create_proxy(&self) -> EventLoopProxy<T> {
    EventLoopProxy {
      event_loop_proxy: self.event_loop.create_proxy(),
    }
  }
}

impl<T> Deref for EventLoop<T> {
  type Target = EventLoopWindowTarget<T>;
  fn deref(&self) -> &EventLoopWindowTarget<T> {
    self.event_loop.window_target()
  }
}

impl<T> EventLoopWindowTarget<T> {
  /// Returns the list of all the monitors available on the system.
  #[inline]
  pub fn available_monitors(&self) -> impl Iterator<Item = MonitorHandle> {
    self
      .p
      .available_monitors()
      .into_iter()
      .map(|inner| MonitorHandle { inner })
  }

  /// Returns the primary monitor of the system.
  ///
  /// Returns `None` if it can't identify any monitor as a primary one.
  #[inline]
  pub fn primary_monitor(&self) -> Option<MonitorHandle> {
    self.p.primary_monitor()
  }

  /// Returns the monitor that contains the given point.
  ///
  /// ## Platform-specific:
  ///
  /// - **Android / iOS:** Unsupported.
  #[inline]
  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<MonitorHandle> {
    self
      .p
      .monitor_from_point(x, y)
      .map(|inner| MonitorHandle { inner })
  }

  /// Change [`DeviceEvent`] filter mode.
  ///
  /// Since the [`DeviceEvent`] capture can lead to high CPU usage for unfocused windows, tao
  /// will ignore them by default for unfocused windows. This method allows changing
  /// this filter at runtime to explicitly capture them again.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / macOS / iOS / Android:** Unsupported.
  ///
  /// [`DeviceEvent`]: crate::event::DeviceEvent
  pub fn set_device_event_filter(&self, _filter: DeviceEventFilter) {
    #[cfg(target_os = "windows")]
    self.p.set_device_event_filter(_filter);
  }

  /// Returns the current cursor position
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Linux(Wayland)**: Unsupported, returns `0,0`.
  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    self.p.cursor_position()
  }

  /// Sets the progress bar state
  ///
  /// ## Platform-specific
  ///
  /// - **Windows:** Unsupported. Use the Progress Bar Function Available in Window (Windows can have different progress bars for different window)
  /// - **Linux:** Only supported desktop environments with `libunity` (e.g. GNOME).
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_progress_bar(&self, _progress: ProgressBarState) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    self.p.set_progress_bar(_progress)
  }

  /// Sets the theme for the application.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_theme(&self, _theme: Option<Theme>) {
    #[cfg(any(
      windows,
      target_os = "linux",
      target_os = "dragonfly",
      target_os = "freebsd",
      target_os = "netbsd",
      target_os = "openbsd",
      target_os = "macos",
    ))]
    self.p.set_theme(_theme)
  }
}

#[cfg(feature = "rwh_05")]
unsafe impl<T> rwh_05::HasRawDisplayHandle for EventLoop<T> {
  fn raw_display_handle(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::HasRawDisplayHandle::raw_display_handle(&**self)
  }
}

#[cfg(feature = "rwh_06")]
impl<T> rwh_06::HasDisplayHandle for EventLoop<T> {
  fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
    rwh_06::HasDisplayHandle::display_handle(&**self)
  }
}

#[cfg(feature = "rwh_05")]
unsafe impl<T> rwh_05::HasRawDisplayHandle for EventLoopWindowTarget<T> {
  fn raw_display_handle(&self) -> rwh_05::RawDisplayHandle {
    self.p.raw_display_handle_rwh_05()
  }
}

#[cfg(feature = "rwh_06")]
impl<T> rwh_06::HasDisplayHandle for EventLoopWindowTarget<T> {
  fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
    let raw = self.p.raw_display_handle_rwh_06()?;
    // SAFETY: The display will never be deallocated while the event loop is alive.
    Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
  }
}

/// Used to send custom events to `EventLoop`.
pub struct EventLoopProxy<T: 'static> {
  event_loop_proxy: platform_impl::EventLoopProxy<T>,
}

impl<T: 'static> Clone for EventLoopProxy<T> {
  fn clone(&self) -> Self {
    Self {
      event_loop_proxy: self.event_loop_proxy.clone(),
    }
  }
}

impl<T: 'static> EventLoopProxy<T> {
  /// Send an event to the `EventLoop` from which this proxy was created. This emits a
  /// `UserEvent(event)` event in the event loop, where `event` is the value passed to this
  /// function.
  ///
  /// Returns an `Err` if the associated `EventLoop` no longer exists.
  pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed<T>> {
    self.event_loop_proxy.send_event(event)
  }
}

impl<T: 'static> fmt::Debug for EventLoopProxy<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad("EventLoopProxy { .. }")
  }
}

/// The error that is returned when an `EventLoopProxy` attempts to wake up an `EventLoop` that
/// no longer exists. Contains the original event given to `send_event`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventLoopClosed<T>(pub T);

impl<T> fmt::Display for EventLoopClosed<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Tried to wake up a closed `EventLoop`")
  }
}

impl<T: fmt::Debug> error::Error for EventLoopClosed<T> {}

/// Fiter controlling the propagation of device events.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DeviceEventFilter {
  /// Always filter out device events.
  Always,
  /// Filter out device events while the window is not focused.
  Unfocused,
  /// Report all device events regardless of window focus.
  Never,
}

impl Default for DeviceEventFilter {
  fn default() -> Self {
    Self::Unfocused
  }
}
