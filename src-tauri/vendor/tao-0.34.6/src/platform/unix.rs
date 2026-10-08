// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(any(
  target_os = "linux",
  target_os = "dragonfly",
  target_os = "freebsd",
  target_os = "netbsd",
  target_os = "openbsd"
))]

#[cfg(feature = "x11")]
use std::{os::raw::c_int, sync::Arc};

// XConnection utilities
#[doc(hidden)]
#[cfg(feature = "x11")]
pub use crate::platform_impl::x11;

#[cfg(feature = "x11")]
use crate::platform_impl::x11::xdisplay::XError;
pub use crate::platform_impl::EventLoop as UnixEventLoop;
use crate::{
  error::{ExternalError, OsError},
  event_loop::{EventLoopBuilder, EventLoopWindowTarget},
  monitor::MonitorHandle,
  platform_impl::{Parent, Window as UnixWindow},
  window::{Window, WindowBuilder},
};

#[cfg(feature = "x11")]
use self::x11::xdisplay::XConnection;

/// Additional methods on `EventLoop` that are specific to Unix.
pub trait EventLoopBuilderExtUnix {
  /// Whether to allow the event loop to be created off of the main thread.
  ///
  /// By default, the window is only allowed to be created on the main
  /// thread, to make platform compatibility easier.
  ///
  /// # `Window` caveats
  ///
  /// Note that any `Window` created on the new thread will be destroyed when the thread
  /// terminates. Attempting to use a `Window` after its parent thread terminates has
  /// unspecified, although explicitly not undefined, behavior.
  fn with_any_thread(&mut self, any_thread: bool) -> &mut Self;

  /// Set the gtk application id.
  ///
  /// If no application ID is given then some features (most notably application uniqueness) will be disabled.
  fn with_app_id<S: Into<String>>(&mut self, id: S) -> &mut Self;
}

impl<T> EventLoopBuilderExtUnix for EventLoopBuilder<T> {
  #[inline]
  fn with_any_thread(&mut self, any_thread: bool) -> &mut Self {
    self.platform_specific.any_thread = any_thread;
    self
  }

  fn with_app_id<S: Into<String>>(&mut self, id: S) -> &mut Self {
    self.platform_specific.app_id = Some(id.into());
    self
  }
}

/// Additional methods on `Window` that are specific to Unix.
pub trait WindowExtUnix {
  /// Create a new Tao window from an existing GTK window. Generally you should use
  /// the non-Linux `WindowBuilder`, this is for those who need lower level window access
  /// and know what they're doing.
  fn new_from_gtk_window<T: 'static>(
    event_loop_window_target: &EventLoopWindowTarget<T>,
    window: gtk::ApplicationWindow,
  ) -> Result<Window, OsError>;

  /// Returns the `gtk::ApplicatonWindow` from gtk crate that is used by this window.
  fn gtk_window(&self) -> &gtk::ApplicationWindow;

  /// Returns the vertical `gtk::Box` that is added by default as the sole child of this window.
  /// Returns `None` if the default vertical `gtk::Box` creation was disabled by [`WindowBuilderExtUnix::with_default_vbox`].
  fn default_vbox(&self) -> Option<&gtk::Box>;

  /// Whether to show the window icon in the taskbar or not.
  fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError>;

  fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>);
}

impl WindowExtUnix for Window {
  fn gtk_window(&self) -> &gtk::ApplicationWindow {
    &self.window.window
  }

  fn default_vbox(&self) -> Option<&gtk::Box> {
    self.window.default_vbox.as_ref()
  }

  fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError> {
    self.window.set_skip_taskbar(skip)
  }

  fn new_from_gtk_window<T: 'static>(
    event_loop_window_target: &EventLoopWindowTarget<T>,
    window: gtk::ApplicationWindow,
  ) -> Result<Window, OsError> {
    let window = UnixWindow::new_from_gtk_window(&event_loop_window_target.p, window)?;
    Ok(Window { window })
  }

  fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>) {
    self.window.set_badge_count(count, desktop_filename);
  }
}

pub trait WindowBuilderExtUnix {
  /// Whether to create the window icon with the taskbar icon or not.
  fn with_skip_taskbar(self, skip: bool) -> WindowBuilder;
  /// Set this window as a transient dialog for `parent`
  /// <https://gtk-rs.org/gtk3-rs/stable/latest/docs/gdk/struct.Window.html#method.set_transient_for>
  fn with_transient_for(self, parent: &impl gtk::glib::IsA<gtk::Window>) -> WindowBuilder;

  /// Whether to enable or disable the internal draw for transparent window.
  ///
  /// When tranparent attribute is enabled, we will call `connect_draw` and draw a transparent background.
  /// For anyone who wants to draw the background themselves, set this to `false`.
  /// Default is `true`.
  fn with_transparent_draw(self, draw: bool) -> WindowBuilder;

  /// Whether to enable or disable the double buffered rendering of the window.
  ///
  /// Default is `true`.
  fn with_double_buffered(self, double_buffered: bool) -> WindowBuilder;

  /// Whether to enable the rgba visual for the window.
  ///
  /// Default is `false` but is always `true` if [`WindowAttributes::transparent`](crate::window::WindowAttributes::transparent) is `true`
  fn with_rgba_visual(self, rgba_visual: bool) -> WindowBuilder;

  /// Wether to set this window as app paintable
  ///
  /// <https://docs.gtk.org/gtk3/method.Widget.set_app_paintable.html>
  ///
  /// Default is `false` but is always `true` if [`WindowAttributes::transparent`](crate::window::WindowAttributes::transparent) is `true`
  fn with_app_paintable(self, app_paintable: bool) -> WindowBuilder;

  /// Whether to set cursor moved event. Cursor event is suited for native GUI frameworks and
  /// games. But it can block gtk's own pipeline occasionally. Turn this off can help Gtk looks
  /// smoother.
  ///
  /// Default is `true`.
  fn with_cursor_moved_event(self, cursor_moved: bool) -> WindowBuilder;

  /// Whether to create a vertical `gtk::Box` and add it as the sole child of this window.
  /// Created by default.
  fn with_default_vbox(self, add: bool) -> WindowBuilder;
}

impl WindowBuilderExtUnix for WindowBuilder {
  fn with_skip_taskbar(mut self, skip: bool) -> WindowBuilder {
    self.platform_specific.skip_taskbar = skip;
    self
  }

  fn with_transient_for(mut self, parent: &impl gtk::glib::IsA<gtk::Window>) -> WindowBuilder {
    use gtk::glib::Cast;
    self.platform_specific.parent = Parent::ChildOf(parent.clone().upcast());
    self
  }

  fn with_transparent_draw(mut self, draw: bool) -> WindowBuilder {
    self.platform_specific.auto_transparent = draw;
    self
  }

  fn with_double_buffered(mut self, double_buffered: bool) -> WindowBuilder {
    self.platform_specific.double_buffered = double_buffered;
    self
  }

  fn with_rgba_visual(mut self, rgba_visual: bool) -> WindowBuilder {
    self.platform_specific.rgba_visual = rgba_visual;
    self
  }

  fn with_app_paintable(mut self, app_paintable: bool) -> WindowBuilder {
    self.platform_specific.app_paintable = app_paintable;
    self
  }

  fn with_cursor_moved_event(mut self, cursor_moved: bool) -> WindowBuilder {
    self.platform_specific.cursor_moved = cursor_moved;
    self
  }

  fn with_default_vbox(mut self, add: bool) -> WindowBuilder {
    self.platform_specific.default_vbox = add;
    self
  }
}

/// Additional methods on `EventLoopWindowTarget` that are specific to Unix.
pub trait EventLoopWindowTargetExtUnix {
  /// True if the `EventLoopWindowTarget` uses Wayland.
  fn is_wayland(&self) -> bool;

  /// True if the `EventLoopWindowTarget` uses X11.
  #[cfg(feature = "x11")]
  fn is_x11(&self) -> bool;

  #[cfg(feature = "x11")]
  fn xlib_xconnection(&self) -> Option<Arc<XConnection>>;

  // /// Returns a pointer to the `wl_display` object of wayland that is used by this
  // /// `EventLoopWindowTarget`.
  // ///
  // /// Returns `None` if the `EventLoop` doesn't use wayland (if it uses xlib for example).
  // ///
  // /// The pointer will become invalid when the winit `EventLoop` is destroyed.
  // fn wayland_display(&self) -> Option<*mut raw::c_void>;

  /// Returns the gtk application for this event loop.
  fn gtk_app(&self) -> &gtk::Application;

  /// Sets the badge count on the taskbar
  fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>);
}

impl<T> EventLoopWindowTargetExtUnix for EventLoopWindowTarget<T> {
  #[inline]
  fn is_wayland(&self) -> bool {
    self.p.is_wayland()
  }

  #[cfg(feature = "x11")]
  #[inline]
  fn is_x11(&self) -> bool {
    !self.p.is_wayland()
  }

  #[cfg(feature = "x11")]
  #[inline]
  fn xlib_xconnection(&self) -> Option<Arc<XConnection>> {
    if self.is_x11() {
      if let Ok(xconn) = XConnection::new(Some(x_error_callback)) {
        Some(Arc::new(xconn))
      } else {
        None
      }
    } else {
      None
    }
  }

  // #[inline]
  // fn wayland_display(&self) -> Option<*mut raw::c_void> {
  //     match self.p {
  //         LinuxEventLoopWindowTarget::Wayland(ref p) => {
  //             Some(p.display().get_display_ptr() as *mut _)
  //         }
  //         #[cfg(feature = "x11")]
  //         _ => None,
  //     }
  // }

  #[inline]
  fn gtk_app(&self) -> &gtk::Application {
    &self.p.app
  }

  #[inline]
  fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>) {
    self.p.set_badge_count(count, desktop_filename);
  }
}

#[cfg(feature = "x11")]
unsafe extern "C" fn x_error_callback(
  _display: *mut x11::ffi::Display,
  event: *mut x11::ffi::XErrorEvent,
) -> c_int {
  let error = XError {
    // TODO get the error text as description
    description: String::new(),
    error_code: (*event).error_code,
    request_code: (*event).request_code,
    minor_code: (*event).minor_code,
  };

  error!("X11 error: {:#?}", error);

  // Fun fact: this return value is completely ignored.
  0
}

/// Additional methods on `MonitorHandle` that are specific to Unix.
pub trait MonitorHandleExtUnix {
  /// Returns the gdk handle of the monitor.
  fn gdk_monitor(&self) -> &gtk::gdk::Monitor;
}

impl MonitorHandleExtUnix for MonitorHandle {
  #[inline]
  fn gdk_monitor(&self) -> &gtk::gdk::Monitor {
    &self.inner.monitor
  }
}
