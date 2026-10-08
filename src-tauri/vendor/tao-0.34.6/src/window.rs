// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! The `Window` struct and associated types.
use std::fmt;

use crate::{
  dpi::{LogicalSize, PhysicalPosition, PhysicalSize, Pixel, PixelUnit, Position, Size},
  error::{ExternalError, NotSupportedError, OsError},
  event_loop::EventLoopWindowTarget,
  monitor::{MonitorHandle, VideoMode},
  platform_impl,
};

pub use crate::icon::{BadIcon, Icon};

/// Progress State
#[derive(Debug, Clone, Copy)]
pub enum ProgressState {
  None,
  Normal,
  /// **Treated as Normal in linux and macOS**
  Indeterminate,
  /// **Treated as Normal in linux**
  Paused,
  /// **Treated as Normal in linux**
  Error,
}

pub struct ProgressBarState {
  /// The progress bar state.
  pub state: Option<ProgressState>,
  /// The progress bar progress. This can be a value ranging from `0` to `100`
  pub progress: Option<u64>,
  /// The `.desktop` filename with the Unity desktop window manager, for example `myapp.desktop` **Linux Only**
  pub desktop_filename: Option<String>,
}

/// Represents a window.
///
/// # Example
///
/// ```no_run
/// use tao::{
///     event::{Event, WindowEvent},
///     event_loop::{ControlFlow, EventLoop},
///     window::Window,
/// };
///
/// let mut event_loop = EventLoop::new();
/// let window = Window::new(&event_loop).unwrap();
///
/// event_loop.run(move |event, _, control_flow| {
///     *control_flow = ControlFlow::Wait;
///
///     match event {
///         Event::WindowEvent {
///             event: WindowEvent::CloseRequested,
///             ..
///         } => *control_flow = ControlFlow::Exit,
///         _ => (),
///     }
/// });
/// ```
pub struct Window {
  pub(crate) window: platform_impl::Window,
}

impl fmt::Debug for Window {
  fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmtr.pad("Window { .. }")
  }
}

impl Drop for Window {
  fn drop(&mut self) {
    // If the window is in exclusive fullscreen, we must restore the desktop
    // video mode (generally this would be done on application exit, but
    // closing the window doesn't necessarily always mean application exit,
    // such as when there are multiple windows)
    if let Some(Fullscreen::Exclusive(_)) = self.fullscreen() {
      self.set_fullscreen(None);
    }
  }
}

/// Type alias for a color in the RGBA format.
///
/// Each value can be 0..255 inclusive.
pub type RGBA = (u8, u8, u8, u8);

/// Identifier of a window. Unique for each window.
///
/// Can be obtained with `window.id()`.
///
/// Whenever you receive an event specific to a window, this event contains a `WindowId` which you
/// can then compare to the ids of your windows.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub(crate) platform_impl::WindowId);

impl WindowId {
  /// # Safety
  /// Returns a dummy `WindowId`, useful for unit testing. The only guarantee made about the return
  /// value of this function is that it will always be equal to itself and to future values returned
  /// by this function.  No other guarantees are made. This may be equal to a real `WindowId`.
  ///
  /// **Passing this into a tao function will result in undefined behavior.**
  pub unsafe fn dummy() -> Self {
    WindowId(platform_impl::WindowId::dummy())
  }
}

/// Object that allows you to build windows.
#[derive(Clone, Default)]
pub struct WindowBuilder {
  /// The attributes to use to create the window.
  pub window: WindowAttributes,

  // Platform-specific configuration.
  pub(crate) platform_specific: platform_impl::PlatformSpecificWindowBuilderAttributes,
}

impl fmt::Debug for WindowBuilder {
  fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmtr
      .debug_struct("WindowBuilder")
      .field("window", &self.window)
      .finish()
  }
}

/// Attributes to use when creating a window.
#[derive(Debug, Clone)]
pub struct WindowAttributes {
  /// The dimensions of the window. If this is `None`, some platform-specific dimensions will be
  /// used.
  ///
  /// The default is `None`.
  pub inner_size: Option<Size>,

  /// The window size constraints
  pub inner_size_constraints: WindowSizeConstraints,

  /// The desired position of the window. If this is `None`, some platform-specific position
  /// will be chosen.
  ///
  /// The default is `None`.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS**: The top left corner position of the window content, the window's "inner"
  ///   position. The window title bar will be placed above it.
  ///   The window will be positioned such that it fits on screen, maintaining
  ///   set `inner_size` if any.
  ///   If you need to precisely position the top left corner of the whole window you have to
  ///   use [`Window::set_outer_position`] after creating the window.
  /// - **Windows**: The top left corner position of the window title bar, the window's "outer"
  ///   position.
  ///   There may be a small gap between this position and the window due to the specifics of the
  ///   Window Manager.
  /// - **Linux**: The top left corner of the window, the window's "outer" position.
  /// - **Linux(Wayland)**: Unsupported.
  /// - **Others**: Ignored.
  ///
  /// See [`Window::set_outer_position`].
  ///
  /// [`Window::set_outer_position`]: crate::window::Window::set_outer_position
  pub position: Option<Position>,

  /// Whether the window is resizable or not.
  ///
  /// The default is `true`.
  pub resizable: bool,

  /// Whether the window is minimizable or not.
  ///
  /// The default is `true`.
  ///
  /// See [`Window::set_minimizable`] for details.
  pub minimizable: bool,

  /// Whether the window is maximizable or not.
  ///
  /// The default is `true`.
  ///
  /// See [`Window::set_maximizable`] for details.
  pub maximizable: bool,

  /// Whether the window is closable or not.
  ///
  /// The default is `true`.
  ///
  /// See [`Window::set_closable`] for details.
  pub closable: bool,

  /// Whether the window should be set as fullscreen upon creation.
  ///
  /// The default is `None`.
  pub fullscreen: Option<Fullscreen>,

  /// The title of the window in the title bar.
  ///
  /// The default is `"tao window"`.
  pub title: String,

  /// Whether the window should be maximized upon creation.
  ///
  /// The default is `false`.
  pub maximized: bool,

  /// Whether the window should be immediately visible upon creation.
  ///
  /// The default is `true`.
  pub visible: bool,

  /// Whether the the window should be transparent. If this is true, writing colors
  /// with alpha values different than `1.0` will produce a transparent window.
  ///
  /// The default is `false`.
  pub transparent: bool,

  /// Whether the window should have borders and bars.
  ///
  /// The default is `true`.
  pub decorations: bool,

  /// Whether the window should always be on top of other windows.
  ///
  /// The default is `false`.
  ///
  /// ## Platform-specific:
  ///
  /// - **Linux(x11):** Result depends on the system's window manager. Consider this setting a suggestion.
  /// - **Linux(Wayland):** Unsupported.
  // TODO: Unsupported in gtk4
  pub always_on_top: bool,

  /// Whether the window should always be on bottom of other windows.
  ///
  /// The default is `false`.
  ///
  /// ## Platform-specific:
  ///
  /// - **Linux(x11):** Result depends on the system's window manager. Consider this setting a suggestion.
  /// - **Linux(Wayland):** Unsupported.
  // TODO: Unsupported in gtk4
  pub always_on_bottom: bool,

  /// The window icon.
  ///
  /// The default is `None`.
  pub window_icon: Option<Icon>,

  pub preferred_theme: Option<Theme>,

  /// Whether the window should be initially focused or not.
  ///
  /// ## Platform-specific:
  ///
  /// **Android / iOS:** Unsupported.
  pub focused: bool,

  /// Whether the window should be focusable or not.
  ///
  /// ## Platform-specific:
  ///
  /// **Android / iOS:** Unsupported.
  pub focusable: bool,

  /// Prevents the window contents from being captured by other apps.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Linux:** Unsupported.
  pub content_protection: bool,

  /// Sets whether the window should be visible on all workspaces.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Windows:** Unsupported.
  pub visible_on_all_workspaces: bool,

  /// Sets the window background color.
  ///
  /// ## Platform-specific:
  ///
  /// - **Windows:** alpha channel is ignored. Instead manually draw the window, for example using `softbuffer` crate, see <https://github.com/tauri-apps/tao/blob/dev/examples/transparent.rs>
  /// - **iOS / Android:** Unsupported.
  pub background_color: Option<RGBA>,
}

impl Default for WindowAttributes {
  #[inline]
  fn default() -> WindowAttributes {
    WindowAttributes {
      inner_size: None,
      inner_size_constraints: Default::default(),
      position: None,
      resizable: true,
      minimizable: true,
      maximizable: true,
      closable: true,
      title: "tao window".to_owned(),
      maximized: false,
      fullscreen: None,
      visible: true,
      transparent: false,
      decorations: true,
      always_on_top: false,
      always_on_bottom: false,
      window_icon: None,
      preferred_theme: None,
      focused: true,
      focusable: true,
      content_protection: false,
      visible_on_all_workspaces: false,
      background_color: None,
    }
  }
}

impl WindowBuilder {
  /// Initializes a new `WindowBuilder` with default values.
  #[inline]
  pub fn new() -> Self {
    Default::default()
  }

  /// Requests the window to be of specific dimensions.
  ///
  /// See [`Window::set_inner_size`] for details.
  ///
  /// [`Window::set_inner_size`]: crate::window::Window::set_inner_size
  #[inline]
  pub fn with_inner_size<S: Into<Size>>(mut self, size: S) -> Self {
    self.window.inner_size = Some(size.into());
    self
  }

  /// Sets a minimum dimension size for the window.
  ///
  /// See [`Window::set_min_inner_size`] for details.
  ///
  /// [`Window::set_min_inner_size`]: crate::window::Window::set_min_inner_size
  #[inline]
  pub fn with_min_inner_size<S: Into<Size>>(mut self, min_size: S) -> Self {
    let size: Size = min_size.into();
    let (width, height) = crate::extract_width_height(size);
    self.window.inner_size_constraints.min_width = Some(width);
    self.window.inner_size_constraints.min_height = Some(height);
    self
  }

  /// Sets a maximum dimension size for the window.
  ///
  /// See [`Window::set_max_inner_size`] for details.
  ///
  /// [`Window::set_max_inner_size`]: crate::window::Window::set_max_inner_size
  #[inline]
  pub fn with_max_inner_size<S: Into<Size>>(mut self, max_size: S) -> Self {
    let size: Size = max_size.into();
    let (width, height) = crate::extract_width_height(size);
    self.window.inner_size_constraints.max_width = Some(width);
    self.window.inner_size_constraints.max_height = Some(height);
    self
  }

  /// Sets inner size constraints for the window.
  ///
  /// See [`Window::set_inner_size_constraints`] for details.
  ///
  /// [`Window::set_inner_size_constraints`]: crate::window::Window::set_inner_size_constraints
  #[inline]
  pub fn with_inner_size_constraints(mut self, constraints: WindowSizeConstraints) -> Self {
    self.window.inner_size_constraints = constraints;
    self
  }

  /// Sets a desired initial position for the window.
  ///
  /// See [`WindowAttributes::position`] for details.
  ///
  /// [`WindowAttributes::position`]: crate::window::WindowAttributes::position
  #[inline]
  pub fn with_position<P: Into<Position>>(mut self, position: P) -> Self {
    self.window.position = Some(position.into());
    self
  }

  /// Sets whether the window is resizable or not.
  ///
  /// See [`Window::set_resizable`] for details.
  ///
  /// [`Window::set_resizable`]: crate::window::Window::set_resizable
  #[inline]
  pub fn with_resizable(mut self, resizable: bool) -> Self {
    self.window.resizable = resizable;
    self
  }

  /// Sets whether the window is minimizable or not.
  ///
  /// See [`Window::set_minimizable`] for details.
  ///
  /// [`Window::set_minimizable`]: crate::window::Window::set_minimizable
  #[inline]
  pub fn with_minimizable(mut self, minimizable: bool) -> Self {
    self.window.minimizable = minimizable;
    self
  }

  /// Sets whether the window is maximizable or not.
  ///
  /// See [`Window::set_maximizable`] for details.
  ///
  /// [`Window::set_maximizable`]: crate::window::Window::set_maximizable
  #[inline]
  pub fn with_maximizable(mut self, maximizable: bool) -> Self {
    self.window.maximizable = maximizable;
    self
  }

  /// Sets whether the window is closable or not.
  ///
  /// See [`Window::set_closable`] for details.
  ///
  /// [`Window::set_closable`]: crate::window::Window::set_closable
  #[inline]
  pub fn with_closable(mut self, closable: bool) -> Self {
    self.window.closable = closable;
    self
  }

  /// Requests a specific title for the window.
  ///
  /// See [`Window::set_title`] for details.
  ///
  /// [`Window::set_title`]: crate::window::Window::set_title
  #[inline]
  pub fn with_title<T: Into<String>>(mut self, title: T) -> Self {
    self.window.title = title.into();
    self
  }

  /// Sets the window fullscreen state.
  ///
  /// See [`Window::set_fullscreen`] for details.
  ///
  /// [`Window::set_fullscreen`]: crate::window::Window::set_fullscreen
  #[inline]
  pub fn with_fullscreen(mut self, fullscreen: Option<Fullscreen>) -> Self {
    self.window.fullscreen = fullscreen;
    self
  }

  /// Requests maximized mode.
  ///
  /// See [`Window::set_maximized`] for details.
  ///
  /// [`Window::set_maximized`]: crate::window::Window::set_maximized
  #[inline]
  pub fn with_maximized(mut self, maximized: bool) -> Self {
    self.window.maximized = maximized;
    self
  }

  /// Sets whether the window will be initially hidden or visible.
  ///
  /// See [`Window::set_visible`] for details.
  ///
  /// [`Window::set_visible`]: crate::window::Window::set_visible
  #[inline]
  pub fn with_visible(mut self, visible: bool) -> Self {
    self.window.visible = visible;
    self
  }

  /// Sets whether the background of the window should be transparent.
  #[inline]
  pub fn with_transparent(mut self, transparent: bool) -> Self {
    self.window.transparent = transparent;
    self
  }

  /// Sets whether the window should have a border, a title bar, etc.
  ///
  /// See [`Window::set_decorations`] for details.
  ///
  /// [`Window::set_decorations`]: crate::window::Window::set_decorations
  #[inline]
  pub fn with_decorations(mut self, decorations: bool) -> Self {
    self.window.decorations = decorations;
    self
  }

  /// Sets whether or not the window will always be below other windows.
  ///
  /// See [`Window::set_always_on_bottom`] for details.
  ///
  /// [`Window::set_always_on_bottom`]: crate::window::Window::set_always_on_bottom
  #[inline]
  pub fn with_always_on_bottom(mut self, always_on_bottom: bool) -> Self {
    self.window.always_on_top = false;
    self.window.always_on_bottom = always_on_bottom;
    self
  }

  /// Sets whether or not the window will always be on top of other windows.
  ///
  /// See [`Window::set_always_on_top`] for details.
  ///
  /// [`Window::set_always_on_top`]: crate::window::Window::set_always_on_top
  #[inline]
  pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
    self.window.always_on_bottom = false;
    self.window.always_on_top = always_on_top;
    self
  }

  /// Sets the window icon.
  ///
  /// See [`Window::set_window_icon`] for details.
  ///
  /// [`Window::set_window_icon`]: crate::window::Window::set_window_icon
  #[inline]
  pub fn with_window_icon(mut self, window_icon: Option<Icon>) -> Self {
    self.window.window_icon = window_icon;
    self
  }

  /// Forces a theme or uses the system settings if `None` was provided.
  ///
  /// ## Platform-specific:
  ///
  /// - **Windows**: It is recommended to always use the same theme used
  ///   in [`EventLoopBuilderExtWindows::with_theme`] for this method also
  ///   or use `None` so it automatically uses the theme used in [`EventLoopBuilderExtWindows::with_theme`]
  ///   or falls back to the system preference, because [`EventLoopBuilderExtWindows::with_theme`] changes
  ///   the theme for some controls like context menus which is app-wide and can't be changed by this method.
  ///
  /// [`EventLoopBuilderExtWindows::with_theme`]: crate::platform::windows::EventLoopBuilderExtWindows::with_theme
  #[allow(rustdoc::broken_intra_doc_links)]
  #[inline]
  pub fn with_theme(mut self, theme: Option<Theme>) -> WindowBuilder {
    self.window.preferred_theme = theme;
    self
  }

  /// Whether the window will be initially focused or not.
  ///
  /// ## Platform-specific:
  ///
  /// **Android / iOS:** Unsupported.
  #[inline]
  pub fn with_focused(mut self, focused: bool) -> WindowBuilder {
    self.window.focused = focused;
    self
  }

  /// Whether the window will be focusable or not.
  ///
  /// ## Platform-specific:
  /// **Android / iOS:** Unsupported.
  #[inline]
  pub fn with_focusable(mut self, focusable: bool) -> WindowBuilder {
    self.window.focusable = focusable;
    self
  }

  /// Prevents the window contents from being captured by other apps.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Linux:** Unsupported.
  #[inline]
  pub fn with_content_protection(mut self, protected: bool) -> WindowBuilder {
    self.window.content_protection = protected;
    self
  }

  /// Sets whether the window should be visible on all workspaces.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Windows:** Unsupported.
  #[inline]
  pub fn with_visible_on_all_workspaces(mut self, visible: bool) -> WindowBuilder {
    self.window.visible_on_all_workspaces = visible;
    self
  }

  /// Sets the window background color.
  ///
  /// ## Platform-specific:
  ///
  /// - **Windows:** alpha channel is ignored. Instead manually draw the window, for example using `softbuffer` crate, see <https://github.com/tauri-apps/tao/blob/dev/examples/transparent.rs>
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn with_background_color(mut self, color: RGBA) -> WindowBuilder {
    self.window.background_color = Some(color);
    self
  }

  /// Builds the window.
  ///
  /// Possible causes of error include denied permission, incompatible system, and lack of memory.
  #[inline]
  pub fn build<T: 'static>(
    self,
    window_target: &EventLoopWindowTarget<T>,
  ) -> Result<Window, OsError> {
    platform_impl::Window::new(&window_target.p, self.window, self.platform_specific).map(
      |window| {
        window.request_redraw();
        Window { window }
      },
    )
  }
}

/// Base Window functions.
impl Window {
  /// Creates a new Window for platforms where this is appropriate.
  ///
  /// This function is equivalent to [`WindowBuilder::new().build(event_loop)`].
  ///
  /// Error should be very rare and only occur in case of permission denied, incompatible system,
  ///  out of memory, etc.
  ///
  /// [`WindowBuilder::new().build(event_loop)`]: crate::window::WindowBuilder::build
  #[inline]
  pub fn new<T: 'static>(event_loop: &EventLoopWindowTarget<T>) -> Result<Window, OsError> {
    let builder = WindowBuilder::new();
    builder.build(event_loop)
  }

  /// Returns an identifier unique to the window.
  #[inline]
  pub fn id(&self) -> WindowId {
    WindowId(self.window.id())
  }

  /// Returns the scale factor that can be used to map logical pixels to physical pixels, and vice versa.
  ///
  /// See the [`dpi`](crate::dpi) module for more information.
  ///
  /// Note that this value can change depending on user action (for example if the window is
  /// moved to another screen); as such, tracking `WindowEvent::ScaleFactorChanged` events is
  /// the most robust way to track the DPI you need to use to draw.
  ///
  /// ## Platform-specific
  ///
  /// - **Android:** Always returns 1.0.
  /// - **iOS:** Can only be called on the main thread. Returns the underlying `UIView`'s
  ///   [`contentScaleFactor`].
  ///
  /// [`contentScaleFactor`]: https://developer.apple.com/documentation/uikit/uiview/1622657-contentscalefactor?language=objc
  #[inline]
  pub fn scale_factor(&self) -> f64 {
    self.window.scale_factor()
  }

  /// Emits a `WindowEvent::RedrawRequested` event in the associated event loop after all OS
  /// events have been processed by the event loop.
  ///
  /// This is the **strongly encouraged** method of redrawing windows, as it can integrate with
  /// OS-requested redraws (e.g. when a window gets resized).
  ///
  /// This function can cause `RedrawRequested` events to be emitted after `Event::MainEventsCleared`
  /// but before `Event::NewEvents` if called in the following circumstances:
  /// * While processing `MainEventsCleared`.
  /// * While processing a `RedrawRequested` event that was sent during `MainEventsCleared` or any
  ///   directly subsequent `RedrawRequested` event.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread.
  /// - **Android:** Unsupported.
  #[inline]
  pub fn request_redraw(&self) {
    self.window.request_redraw()
  }
}

/// Position and size functions.
impl Window {
  /// Returns the position of the top-left hand corner of the window's client area relative to the
  /// top-left hand corner of the desktop.
  ///
  /// The same conditions that apply to `outer_position` apply to this method.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread. Returns the top left coordinates of the
  ///   window's [safe area] in the screen space coordinate system.
  /// - **Android:** Always returns [`NotSupportedError`].
  ///
  /// [safe area]: https://developer.apple.com/documentation/uikit/uiview/2891103-safeareainsets?language=objc
  #[inline]
  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    self.window.inner_position()
  }

  /// Returns the position of the top-left hand corner of the window relative to the
  ///  top-left hand corner of the desktop.
  ///
  /// Note that the top-left hand corner of the desktop is not necessarily the same as
  ///  the screen. If the user uses a desktop with multiple monitors, the top-left hand corner
  ///  of the desktop is the top-left hand corner of the monitor at the top-left of the desktop.
  ///
  /// The coordinates can be negative if the top-left hand corner of the window is outside
  ///  of the visible screen region.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread. Returns the top left coordinates of the
  ///   window in the screen space coordinate system.
  /// - **Android:** Always returns [`NotSupportedError`].
  /// - **Linux(Wayland)**: Has no effect, since Wayland doesn't support a global cordinate system
  #[inline]
  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    self.window.outer_position()
  }

  /// Modifies the position of the window.
  ///
  /// See `outer_position` for more information about the coordinates. This automatically un-maximizes the
  /// window if it's maximized.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread. Sets the top left coordinates of the
  ///   window in the screen space coordinate system.
  /// - **Android / Linux(Wayland):** Unsupported.
  #[inline]
  pub fn set_outer_position<P: Into<Position>>(&self, position: P) {
    self.window.set_outer_position(position.into())
  }

  /// Returns the physical size of the window's client area.
  ///
  /// The client area is the content of the window, excluding the title bar and borders.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread. Returns the `PhysicalSize` of the window's
  ///   [safe area] in screen space coordinates.
  ///
  /// [safe area]: https://developer.apple.com/documentation/uikit/uiview/2891103-safeareainsets?language=objc
  #[inline]
  pub fn inner_size(&self) -> PhysicalSize<u32> {
    self.window.inner_size()
  }

  /// Modifies the inner size of the window.
  ///
  /// See `inner_size` for more information about the values. This automatically un-maximizes the
  /// window if it's maximized.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_inner_size<S: Into<Size>>(&self, size: S) {
    self.window.set_inner_size(size.into())
  }

  /// Returns the physical size of the entire window.
  ///
  /// These dimensions include the title bar and borders. If you don't want that (and you usually don't),
  /// use `inner_size` instead.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread. Returns the `PhysicalSize` of the window in
  ///   screen space coordinates.
  #[inline]
  pub fn outer_size(&self) -> PhysicalSize<u32> {
    self.window.outer_size()
  }

  /// Sets a minimum dimension size for the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_min_inner_size<S: Into<Size>>(&self, min_size: Option<S>) {
    self.window.set_min_inner_size(min_size.map(|s| s.into()))
  }

  /// Sets a maximum dimension size for the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_max_inner_size<S: Into<Size>>(&self, max_size: Option<S>) {
    self.window.set_max_inner_size(max_size.map(|s| s.into()))
  }

  /// Sets inner size constraints for the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_inner_size_constraints(&self, constraints: WindowSizeConstraints) {
    self.window.set_inner_size_constraints(constraints)
  }
}

/// Misc. attribute functions.
impl Window {
  /// Modifies the title of the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_title(&self, title: &str) {
    self.window.set_title(title)
  }

  /// Gets the current title of the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported. Returns ane empty string.
  #[inline]
  pub fn title(&self) -> String {
    self.window.title()
  }

  /// Modifies the window's visibility.
  ///
  /// If `false`, this will hide the window. If `true`, this will show the window.
  /// ## Platform-specific
  ///
  /// - **Android:** Unsupported.
  /// - **iOS:** Can only be called on the main thread.
  #[inline]
  pub fn set_visible(&self, visible: bool) {
    self.window.set_visible(visible)
  }

  /// Bring the window to front and focus.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_focus(&self) {
    self.window.set_focus()
  }

  /// Sets whether the window is focusable or not.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS**: If the window is already focused, it is not possible to unfocus it after calling `set_focusable(false)`.
  ///   In this case, you might consider calling [`Window::set_focus`] but it will move the window to the back i.e. at the bottom in terms of z-order.
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_focusable(&self, focusable: bool) {
    self.window.set_focusable(focusable)
  }

  /// Is window active and focused?
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_focused(&self) -> bool {
    self.window.is_focused()
  }

  /// Indicates whether the window is always on top of other windows.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_always_on_top(&self) -> bool {
    self.window.is_always_on_top()
  }

  /// Sets whether the window is resizable or not.
  ///
  /// Note that making the window unresizable doesn't exempt you from handling `Resized`, as that event can still be
  /// triggered by DPI scaling, entering fullscreen mode, etc.
  ///
  /// ## Platform-specific
  ///
  /// This only has an effect on desktop platforms.
  ///
  /// Due to a bug in XFCE, this has no effect on Xfwm.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux:** Most size methods like maximized are async and do not work well with calling
  ///   sequentailly. For setting inner or outer size, you don't need to set resizable to true before
  ///   it. It can resize no matter what. But if you insist to do so, it has a `100, 100` minimum
  ///   limitation somehow. For maximizing, it requires resizable is true. If you really want to set
  ///   resizable to false after it. You might need a mechanism to check the window is really
  ///   maximized.
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_resizable(&self, resizable: bool) {
    self.window.set_resizable(resizable)
  }

  /// Sets whether the window is minimizable or not.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / iOS / Android:** Unsupported.
  #[inline]
  pub fn set_minimizable(&self, minimizable: bool) {
    self.window.set_minimizable(minimizable)
  }

  /// Sets whether the window is maximizable or not.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS:** Disables the "zoom" button in the window titlebar, which is also used to enter fullscreen mode.
  /// - **Linux / iOS / Android:** Unsupported.
  #[inline]
  pub fn set_maximizable(&self, maximizable: bool) {
    self.window.set_maximizable(maximizable)
  }

  /// Sets whether the window is closable or not.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux:** "GTK+ will do its best to convince the window manager not to show a close button.
  ///   Depending on the system, this function may not have any effect when called on a window that is already visible"
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_closable(&self, closable: bool) {
    self.window.set_closable(closable)
  }

  /// Sets the window to minimized or back
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_minimized(&self, minimized: bool) {
    self.window.set_minimized(minimized);
  }

  /// Sets the window to maximized or back.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_maximized(&self, maximized: bool) {
    self.window.set_maximized(maximized)
  }

  /// Gets the window's current maximized state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_maximized(&self) -> bool {
    self.window.is_maximized()
  }

  /// Gets the window's current minimized state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_minimized(&self) -> bool {
    self.window.is_minimized()
  }

  /// Gets the window's current visibility state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_visible(&self) -> bool {
    self.window.is_visible()
  }

  /// Gets the window's current resizable state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_resizable(&self) -> bool {
    self.window.is_resizable()
  }

  /// Gets the window's current minimizable state.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / iOS / Android:** Unsupported.
  #[inline]
  pub fn is_minimizable(&self) -> bool {
    self.window.is_minimizable()
  }

  /// Gets the window's current maximizable state.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / iOS / Android:** Unsupported.
  #[inline]
  pub fn is_maximizable(&self) -> bool {
    self.window.is_maximizable()
  }

  /// Gets the window's current closable state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn is_closable(&self) -> bool {
    self.window.is_closable()
  }

  /// Gets the window's current decoration state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  pub fn is_decorated(&self) -> bool {
    self.window.is_decorated()
  }

  /// Sets the window to fullscreen or back.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS:** `Fullscreen::Exclusive` provides true exclusive mode with a
  ///   video mode change. *Caveat!* macOS doesn't provide task switching (or
  ///   spaces!) while in exclusive fullscreen mode. This mode should be used
  ///   when a video mode change is desired, but for a better user experience,
  ///   borderless fullscreen might be preferred.
  ///
  ///   `Fullscreen::Borderless` provides a borderless fullscreen window on a
  ///   separate space. This is the idiomatic way for fullscreen games to work
  ///   on macOS. See `WindowExtMacOs::set_simple_fullscreen` if
  ///   separate spaces are not preferred.
  ///
  ///   The dock and the menu bar are always disabled in fullscreen mode.
  /// - **iOS:** Can only be called on the main thread.
  /// - **Windows:** Screen saver is disabled in fullscreen mode.
  /// - **Linux:** The window will only fullscreen to current monitor no matter which enum variant.
  /// - **Android:** Unsupported.
  #[inline]
  pub fn set_fullscreen(&self, fullscreen: Option<Fullscreen>) {
    self.window.set_fullscreen(fullscreen)
  }

  /// Gets the window's current fullscreen state.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS:** Can only be called on the main thread.
  /// - **Android:** Will always return `None`.
  #[inline]
  pub fn fullscreen(&self) -> Option<Fullscreen> {
    self.window.fullscreen()
  }

  /// Turn window decorations on or off.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  ///
  /// [`setPrefersStatusBarHidden`]: https://developer.apple.com/documentation/uikit/uiviewcontroller/1621440-prefersstatusbarhidden?language=objc
  #[inline]
  pub fn set_decorations(&self, decorations: bool) {
    self.window.set_decorations(decorations)
  }

  /// Change whether or not the window will always be below other windows.
  ///
  /// ## Platform-specific
  ///
  /// - **Windows**: There is no guarantee that the window will be the bottom most but it will try to be.
  /// - **Linux(x11):** Result depends on the system's window manager. Consider this setting a suggestion.
  /// - **Linux(Wayland) / iOS / Android:** Unsupported.
  // TODO: Unsupported in gtk4
  #[inline]
  pub fn set_always_on_bottom(&self, always_on_bottom: bool) {
    self.window.set_always_on_bottom(always_on_bottom)
  }

  /// Change whether or not the window will always be on top of other windows.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux(x11):** Result depends on the system's window manager. Consider this setting a suggestion.
  /// - **Linux(Wayland) / iOS / Android:** Unsupported.
  // TODO: Unsupported in gtk4
  #[inline]
  pub fn set_always_on_top(&self, always_on_top: bool) {
    self.window.set_always_on_top(always_on_top)
  }

  /// Sets the window icon. On Windows and Linux, this is typically the small icon in the top-left
  /// corner of the title bar.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / macOS:** Unsupported.
  ///
  /// On Windows, this sets `ICON_SMALL`. The base size for a window icon is 16x16, but it's
  /// recommended to account for screen scaling and pick a multiple of that, i.e. 32x32.
  #[inline]
  pub fn set_window_icon(&self, window_icon: Option<Icon>) {
    self.window.set_window_icon(window_icon)
  }

  /// Sets location of IME candidate box in client area coordinates relative to the top left.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_ime_position<P: Into<Position>>(&self, position: P) {
    self.window.set_ime_position(position.into())
  }

  /// Sets the taskbar progress state.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / macOS**: Unlike windows, progress bar is app-wide and not specific to this window. Only supported desktop environments with `libunity` (e.g. GNOME).
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_progress_bar(&self, _progress: ProgressBarState) {
    #[cfg(any(
      windows,
      target_os = "linux",
      target_os = "dragonfly",
      target_os = "freebsd",
      target_os = "netbsd",
      target_os = "openbsd",
      target_os = "macos",
    ))]
    self.window.set_progress_bar(_progress)
  }

  /// Requests user attention to the window, this has no effect if the application
  /// is already focused. How requesting for user attention manifests is platform dependent,
  /// see `UserAttentionType` for details.
  ///
  /// Providing `None` will unset the request for user attention. Unsetting the request for
  /// user attention might not be done automatically by the WM when the window receives input.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  /// - **macOS:** `None` has no effect.
  /// - **Linux:** Urgency levels have the same effect.
  #[inline]
  pub fn request_user_attention(&self, request_type: Option<UserAttentionType>) {
    self.window.request_user_attention(request_type)
  }

  /// Returns the current window theme.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn theme(&self) -> Theme {
    self.window.theme()
  }

  /// Sets the theme for this window.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / macOS**: Theme is app-wide and not specific to this window.
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_theme(&self, #[allow(unused)] theme: Option<Theme>) {
    #[cfg(any(
      windows,
      target_os = "linux",
      target_os = "dragonfly",
      target_os = "freebsd",
      target_os = "netbsd",
      target_os = "openbsd",
      target_os = "macos",
    ))]
    self.window.set_theme(theme)
  }

  /// Prevents the window contents from being captured by other apps.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Linux:** Unsupported.
  pub fn set_content_protection(&self, #[allow(unused)] enabled: bool) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    self.window.set_content_protection(enabled);
  }

  /// Sets whether the window should be visible on all workspaces.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Windows:** Unsupported.
  pub fn set_visible_on_all_workspaces(&self, #[allow(unused)] visible: bool) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    self.window.set_visible_on_all_workspaces(visible)
  }

  /// Sets the window background color.
  ///
  /// ## Platform-specific:
  ///
  /// - **Windows:** alpha channel is ignored. Instead manually draw the window, for example using `softbuffer` crate, see <https://github.com/tauri-apps/tao/blob/dev/examples/transparent.rs>
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_background_color(&self, color: Option<RGBA>) {
    self.window.set_background_color(color)
  }
}

/// Cursor functions.
impl Window {
  /// Modifies the cursor icon of the window.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_cursor_icon(&self, cursor: CursorIcon) {
    self.window.set_cursor_icon(cursor);
  }

  /// Changes the position of the cursor in window coordinates.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Always returns an [`ExternalError::NotSupported`].
  #[inline]
  pub fn set_cursor_position<P: Into<Position>>(&self, position: P) -> Result<(), ExternalError> {
    self.window.set_cursor_position(position.into())
  }

  /// Grabs the cursor, preventing it from leaving the window.
  ///
  /// There's no guarantee that the cursor will be hidden. You should
  /// hide it by yourself if you want so.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS:** This locks the cursor in a fixed location, which looks visually awkward.
  /// - **iOS / Android:** Always returns an [`ExternalError::NotSupported`].
  #[inline]
  pub fn set_cursor_grab(&self, grab: bool) -> Result<(), ExternalError> {
    self.window.set_cursor_grab(grab)
  }

  /// Modifies the cursor's visibility.
  ///
  /// If `false`, this will hide the cursor. If `true`, this will show the cursor.
  ///
  /// ## Platform-specific
  ///
  /// - **Windows:** The cursor is only hidden within the confines of the window.
  /// - **macOS:** The cursor is hidden as long as the window has input focus, even if the cursor is
  ///   outside of the window.
  /// - **iOS / Android:** Unsupported.
  #[inline]
  pub fn set_cursor_visible(&self, visible: bool) {
    self.window.set_cursor_visible(visible)
  }

  /// Moves the window with the left mouse button until the button is released.
  ///
  /// There's no guarantee that this will work unless the left mouse button was pressed
  /// immediately before this function is called.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS:** May prevent the button release event to be triggered.
  /// - **iOS / Android:** Always returns an [`ExternalError::NotSupported`].
  #[inline]
  pub fn drag_window(&self) -> Result<(), ExternalError> {
    self.window.drag_window()
  }

  /// Resizes the window with the left mouse button until the button is released.
  ///
  /// There's no guarantee that this will work unless the left mouse button was pressed
  /// immediately before this function is called.
  ///
  /// ## Platform-specific
  ///
  /// - **macOS / iOS / Android:** Always returns an [`ExternalError::NotSupported`].
  #[inline]
  pub fn drag_resize_window(&self, direction: ResizeDirection) -> Result<(), ExternalError> {
    self.window.drag_resize_window(direction)
  }

  /// Modifies whether the window catches cursor events.
  ///
  /// If `true`, the events are passed through the window such that any other window behind it receives them.
  /// If `false` the window will catch the cursor events. By default cursor events are not ignored.
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android:** Always returns an [`ExternalError::NotSupported`]
  #[inline]
  pub fn set_ignore_cursor_events(&self, ignore: bool) -> Result<(), ExternalError> {
    self.window.set_ignore_cursor_events(ignore)
  }

  /// Returns the current cursor position
  ///
  /// ## Platform-specific
  ///
  /// - **iOS / Android / Linux(Wayland)**: Unsupported, returns `0,0`.
  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    self.window.cursor_position()
  }
}

/// Monitor info functions.
impl Window {
  /// Returns the monitor on which the window currently resides.
  ///
  /// Returns `None` if current monitor can't be detected.
  ///
  /// ## Platform-specific
  ///
  /// **iOS:** Can only be called on the main thread.
  #[inline]
  pub fn current_monitor(&self) -> Option<MonitorHandle> {
    self.window.current_monitor()
  }

  #[inline]
  /// Returns the monitor that contains the given point.
  ///
  /// ## Platform-specific:
  ///
  /// - **Android / iOS:** Unsupported.
  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<MonitorHandle> {
    self.window.monitor_from_point(x, y)
  }

  /// Returns the list of all the monitors available on the system.
  ///
  /// This is the same as `EventLoopWindowTarget::available_monitors`, and is provided for convenience.
  ///
  /// ## Platform-specific
  ///
  /// **iOS:** Can only be called on the main thread.
  #[inline]
  pub fn available_monitors(&self) -> impl Iterator<Item = MonitorHandle> {
    self
      .window
      .available_monitors()
      .into_iter()
      .map(|inner| MonitorHandle { inner })
  }

  /// Returns the primary monitor of the system.
  ///
  /// Returns `None` if it can't identify any monitor as a primary one.
  ///
  /// This is the same as `EventLoopWindowTarget::primary_monitor`, and is provided for convenience.
  ///
  /// ## Platform-specific
  ///
  /// **iOS:** Can only be called on the main thread.
  #[inline]
  pub fn primary_monitor(&self) -> Option<MonitorHandle> {
    self.window.primary_monitor()
  }
}

#[cfg(feature = "rwh_04")]
unsafe impl rwh_04::HasRawWindowHandle for Window {
  fn raw_window_handle(&self) -> rwh_04::RawWindowHandle {
    self.window.raw_window_handle_rwh_04()
  }
}

#[cfg(feature = "rwh_05")]
unsafe impl rwh_05::HasRawWindowHandle for Window {
  fn raw_window_handle(&self) -> rwh_05::RawWindowHandle {
    self.window.raw_window_handle_rwh_05()
  }
}

#[cfg(feature = "rwh_05")]
unsafe impl rwh_05::HasRawDisplayHandle for Window {
  fn raw_display_handle(&self) -> rwh_05::RawDisplayHandle {
    self.window.raw_display_handle_rwh_05()
  }
}

#[cfg(feature = "rwh_06")]
impl rwh_06::HasWindowHandle for Window {
  fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
    let raw = self.window.raw_window_handle_rwh_06()?;
    // SAFETY: The window handle will never be deallocated while the window is alive.
    Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
  }
}

#[cfg(feature = "rwh_06")]
impl rwh_06::HasDisplayHandle for Window {
  fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
    let raw = self.window.raw_display_handle_rwh_06()?;
    // SAFETY: The window handle will never be deallocated while the window is alive.
    Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
  }
}

/// Describes the appearance of the mouse cursor.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Default)]
pub enum CursorIcon {
  /// The platform-dependent default cursor.
  #[default]
  Default,
  /// A simple crosshair.
  Crosshair,
  /// A hand (often used to indicate links in web browsers).
  Hand,
  /// Self explanatory.
  Arrow,
  /// Indicates something is to be moved.
  Move,
  /// Indicates text that may be selected or edited.
  Text,
  /// Program busy indicator.
  Wait,
  /// Help indicator (often rendered as a "?")
  Help,
  /// Progress indicator. Shows that processing is being done. But in contrast
  /// with "Wait" the user may still interact with the program. Often rendered
  /// as a spinning beach ball, or an arrow with a watch or hourglass.
  Progress,

  /// Cursor showing that something cannot be done.
  NotAllowed,
  ContextMenu,
  Cell,
  VerticalText,
  Alias,
  Copy,
  NoDrop,
  /// Indicates something can be grabbed.
  Grab,
  /// Indicates something is grabbed.
  Grabbing,
  AllScroll,
  ZoomIn,
  ZoomOut,

  /// Indicate that some edge is to be moved. For example, the 'SeResize' cursor
  /// is used when the movement starts from the south-east corner of the box.
  EResize,
  NResize,
  NeResize,
  NwResize,
  SResize,
  SeResize,
  SwResize,
  WResize,
  EwResize,
  NsResize,
  NeswResize,
  NwseResize,
  ColResize,
  RowResize,
}

/// Fullscreen modes.
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Fullscreen {
  Exclusive(VideoMode),

  /// Providing `None` to `Borderless` will fullscreen on the current monitor.
  Borderless(Option<MonitorHandle>),
}

#[non_exhaustive]
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub enum Theme {
  #[default]
  Light,
  Dark,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UserAttentionType {
  /// ## Platform-specific
  /// - **macOS:** Bounces the dock icon until the application is in focus.
  /// - **Windows:** Flashes both the window and the taskbar button until the application is in focus.
  Critical,
  /// ## Platform-specific
  /// - **macOS:** Bounces the dock icon once.
  /// - **Windows:** Flashes the taskbar button until the application is in focus.
  #[default]
  Informational,
}

/// Window size constraints
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct WindowSizeConstraints {
  /// The minimum width a window can be, If this is `None`, the window will have no minimum width (aside from reserved).
  ///
  /// The default is `None`.
  pub min_width: Option<PixelUnit>,
  /// The minimum height a window can be, If this is `None`, the window will have no minimum height (aside from reserved).
  ///
  /// The default is `None`.
  pub min_height: Option<PixelUnit>,
  /// The maximum width a window can be, If this is `None`, the window will have no maximum width (aside from reserved).
  ///
  /// The default is `None`.
  pub max_width: Option<PixelUnit>,
  /// The maximum height a window can be, If this is `None`, the window will have no maximum height (aside from reserved).
  ///
  /// The default is `None`.
  pub max_height: Option<PixelUnit>,
}

impl WindowSizeConstraints {
  pub fn new(
    min_width: Option<PixelUnit>,
    min_height: Option<PixelUnit>,
    max_width: Option<PixelUnit>,
    max_height: Option<PixelUnit>,
  ) -> Self {
    Self {
      min_width,
      min_height,
      max_width,
      max_height,
    }
  }

  /// Returns true if `min_width` or `min_height` is set.
  pub fn has_min(&self) -> bool {
    self.min_width.is_some() || self.min_height.is_some()
  }
  /// Returns true if `max_width` or `max_height` is set.
  pub fn has_max(&self) -> bool {
    self.max_width.is_some() || self.max_height.is_some()
  }

  /// Returns a physical size that represents the minimum constraints set and fallbacks to [`PixelUnit::MIN`] for unset values
  pub fn min_size_physical<T: Pixel>(&self, scale_factor: f64) -> PhysicalSize<T> {
    PhysicalSize::new(
      self
        .min_width
        .unwrap_or(PixelUnit::MIN)
        .to_physical(scale_factor)
        .0,
      self
        .min_height
        .unwrap_or(PixelUnit::MIN)
        .to_physical(scale_factor)
        .0,
    )
  }

  /// Returns a logical size that represents the minimum constraints set and fallbacks to [`PixelUnit::MIN`] for unset values
  pub fn min_size_logical<T: Pixel>(&self, scale_factor: f64) -> LogicalSize<T> {
    LogicalSize::new(
      self
        .min_width
        .unwrap_or(PixelUnit::MIN)
        .to_logical(scale_factor)
        .0,
      self
        .min_height
        .unwrap_or(PixelUnit::MIN)
        .to_logical(scale_factor)
        .0,
    )
  }

  /// Returns a physical size that represents the maximum constraints set and fallbacks to [`PixelUnit::MAX`] for unset values
  pub fn max_size_physical<T: Pixel>(&self, scale_factor: f64) -> PhysicalSize<T> {
    PhysicalSize::new(
      self
        .max_width
        .unwrap_or(PixelUnit::MAX)
        .to_physical(scale_factor)
        .0,
      self
        .max_height
        .unwrap_or(PixelUnit::MAX)
        .to_physical(scale_factor)
        .0,
    )
  }

  /// Returns a logical size that represents the maximum constraints set and fallbacks to [`PixelUnit::MAX`] for unset values
  pub fn max_size_logical<T: Pixel>(&self, scale_factor: f64) -> LogicalSize<T> {
    LogicalSize::new(
      self
        .max_width
        .unwrap_or(PixelUnit::MAX)
        .to_logical(scale_factor)
        .0,
      self
        .max_height
        .unwrap_or(PixelUnit::MAX)
        .to_logical(scale_factor)
        .0,
    )
  }

  /// Clamps the desired size based on the constraints set
  pub fn clamp(&self, desired_size: Size, scale_factor: f64) -> Size {
    let min_size: PhysicalSize<f64> = self.min_size_physical(scale_factor);
    let max_size: PhysicalSize<f64> = self.max_size_physical(scale_factor);
    Size::clamp(desired_size, min_size.into(), max_size.into(), scale_factor)
  }
}

/// Defines the orientation that a window resize will be performed.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ResizeDirection {
  East,
  North,
  NorthEast,
  NorthWest,
  South,
  SouthEast,
  SouthWest,
  West,
}

pub(crate) fn hit_test(
  (left, top, right, bottom): (i32, i32, i32, i32),
  cx: i32,
  cy: i32,
  border_x: i32,
  border_y: i32,
) -> Option<ResizeDirection> {
  const LEFT: isize = 0b0001;
  const RIGHT: isize = 0b0010;
  const TOP: isize = 0b0100;
  const BOTTOM: isize = 0b1000;
  const TOPLEFT: isize = TOP | LEFT;
  const TOPRIGHT: isize = TOP | RIGHT;
  const BOTTOMLEFT: isize = BOTTOM | LEFT;
  const BOTTOMRIGHT: isize = BOTTOM | RIGHT;

  #[rustfmt::skip]
  let result = (LEFT * (cx < left + border_x) as isize)
             | (RIGHT * (cx >= right - border_x) as isize)
             | (TOP * (cy < top + border_y) as isize)
             | (BOTTOM * (cy >= bottom - border_y) as isize);

  match result {
    LEFT => Some(ResizeDirection::West),
    RIGHT => Some(ResizeDirection::East),
    TOP => Some(ResizeDirection::North),
    BOTTOM => Some(ResizeDirection::South),
    TOPLEFT => Some(ResizeDirection::NorthWest),
    TOPRIGHT => Some(ResizeDirection::NorthEast),
    BOTTOMLEFT => Some(ResizeDirection::SouthWest),
    BOTTOMRIGHT => Some(ResizeDirection::SouthEast),
    _ => None,
  }
}
