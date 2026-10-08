// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  collections::VecDeque,
  ops::{Deref, DerefMut},
};

use objc2::runtime::{AnyClass, AnyObject};

use crate::{
  dpi::{self, LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position, Size},
  error::{ExternalError, NotSupportedError, OsError as RootOsError},
  event::{Event, WindowEvent},
  icon::Icon,
  monitor::MonitorHandle as RootMonitorHandle,
  platform::ios::{MonitorHandleExtIOS, ScreenEdge, ValidOrientations},
  platform_impl::platform::{
    app_state,
    event_loop::{self, EventProxy, EventWrapper},
    ffi::{
      id, CGFloat, CGPoint, CGRect, CGSize, UIEdgeInsets, UIInterfaceOrientationMask, UIRectEdge,
      UIScreenOverscanCompensation, NO, YES,
    },
    monitor, set_badge_count, view, EventLoopWindowTarget, MonitorHandle,
  },
  window::{
    CursorIcon, Fullscreen, ResizeDirection, Theme, UserAttentionType, WindowAttributes,
    WindowId as RootWindowId, WindowSizeConstraints,
  },
};

pub struct Inner {
  pub window: id,
  pub view_controller: id,
  pub view: id,
  gl_or_metal_backed: bool,
}

impl Drop for Inner {
  fn drop(&mut self) {
    unsafe {
      let () = msg_send![self.view, release];
      let () = msg_send![self.view_controller, release];
      let () = msg_send![self.window, release];
    }
  }
}

impl Inner {
  pub fn set_title(&self, _title: &str) {
    debug!("`Window::set_title` is ignored on iOS")
  }

  pub fn title(&self) -> String {
    String::new()
  }
  pub fn set_visible(&self, visible: bool) {
    match visible {
      true => unsafe {
        let () = msg_send![self.window, setHidden: NO];
      },
      false => unsafe {
        let () = msg_send![self.window, setHidden: YES];
      },
    }
  }

  pub fn set_focus(&self) {
    //FIXME: implementation goes here
    warn!("set_focus not yet implemented on iOS");
  }

  pub fn set_focusable(&self, _focusable: bool) {
    warn!("set_focusable not yet implemented on iOS");
  }

  pub fn is_focused(&self) -> bool {
    warn!("`Window::is_focused` is ignored on iOS");
    false
  }

  pub fn is_always_on_top(&self) -> bool {
    log::warn!("`Window::is_always_on_top` is ignored on iOS");
    false
  }

  pub fn request_redraw(&self) {
    unsafe {
      if self.gl_or_metal_backed {
        // `setNeedsDisplay` does nothing on UIViews which are directly backed by CAEAGLLayer or CAMetalLayer.
        // Ordinarily the OS sets up a bunch of UIKit state before calling drawRect: on a UIView, but when using
        // raw or gl/metal for drawing this work is completely avoided.
        //
        // The docs for `setNeedsDisplay` don't mention `CAMetalLayer`; however, this has been confirmed via
        // testing.
        //
        // https://developer.apple.com/documentation/uikit/uiview/1622437-setneedsdisplay?language=objc
        app_state::queue_gl_or_metal_redraw(self.window);
      } else {
        let () = msg_send![self.view, setNeedsDisplay];
      }
    }
  }

  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    unsafe {
      let safe_area = self.safe_area_screen_space();
      let position = LogicalPosition {
        x: safe_area.origin.x as f64,
        y: safe_area.origin.y as f64,
      };
      let scale_factor = self.scale_factor();
      Ok(position.to_physical(scale_factor))
    }
  }

  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    unsafe {
      let screen_frame = self.screen_frame();
      let position = LogicalPosition {
        x: screen_frame.origin.x as f64,
        y: screen_frame.origin.y as f64,
      };
      let scale_factor = self.scale_factor();
      Ok(position.to_physical(scale_factor))
    }
  }

  pub fn set_outer_position(&self, physical_position: Position) {
    unsafe {
      let scale_factor = self.scale_factor();
      let position = physical_position.to_logical::<f64>(scale_factor);
      let screen_frame = self.screen_frame();
      let new_screen_frame = CGRect {
        origin: CGPoint {
          x: position.x as _,
          y: position.y as _,
        },
        size: screen_frame.size,
      };
      let bounds = self.from_screen_space(new_screen_frame);
      let () = msg_send![self.window, setBounds: bounds];
    }
  }

  pub fn inner_size(&self) -> PhysicalSize<u32> {
    unsafe {
      let scale_factor = self.scale_factor();
      let safe_area = self.safe_area_screen_space();
      let size = LogicalSize {
        width: safe_area.size.width as f64,
        height: safe_area.size.height as f64,
      };
      size.to_physical(scale_factor)
    }
  }

  pub fn outer_size(&self) -> PhysicalSize<u32> {
    unsafe {
      let scale_factor = self.scale_factor();
      let screen_frame = self.screen_frame();
      let size = LogicalSize {
        width: screen_frame.size.width as f64,
        height: screen_frame.size.height as f64,
      };
      size.to_physical(scale_factor)
    }
  }

  pub fn set_inner_size(&self, _size: Size) {
    warn!("not clear what `Window::set_inner_size` means on iOS");
  }

  pub fn set_min_inner_size(&self, _: Option<Size>) {
    warn!("`Window::set_min_inner_size` is ignored on iOS")
  }
  pub fn set_max_inner_size(&self, _: Option<Size>) {
    warn!("`Window::set_max_inner_size` is ignored on iOS")
  }
  pub fn set_inner_size_constraints(&self, _: WindowSizeConstraints) {
    warn!("`Window::set_inner_size_constraints` is ignored on iOS")
  }

  pub fn set_resizable(&self, _resizable: bool) {
    warn!("`Window::set_resizable` is ignored on iOS")
  }

  pub fn set_minimizable(&self, _minimizable: bool) {
    warn!("`Window::set_minimizable` is ignored on iOS")
  }

  pub fn set_maximizable(&self, _maximizable: bool) {
    warn!("`Window::set_maximizable` is ignored on iOS")
  }

  pub fn set_closable(&self, _closable: bool) {
    warn!("`Window::set_closable` is ignored on iOS")
  }

  pub fn scale_factor(&self) -> f64 {
    unsafe {
      let hidpi: CGFloat = msg_send![self.view, contentScaleFactor];
      hidpi as _
    }
  }

  pub fn set_cursor_icon(&self, _cursor: CursorIcon) {
    debug!("`Window::set_cursor_icon` ignored on iOS")
  }

  pub fn set_cursor_position(&self, _position: Position) -> Result<(), ExternalError> {
    Err(ExternalError::NotSupported(NotSupportedError::new()))
  }

  pub fn set_cursor_grab(&self, _grab: bool) -> Result<(), ExternalError> {
    Err(ExternalError::NotSupported(NotSupportedError::new()))
  }

  pub fn set_cursor_visible(&self, _visible: bool) {
    debug!("`Window::set_cursor_visible` is ignored on iOS")
  }

  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    debug!("`Window::cursor_position` is ignored on iOS");
    Ok((0, 0).into())
  }

  pub fn drag_window(&self) -> Result<(), ExternalError> {
    Err(ExternalError::NotSupported(NotSupportedError::new()))
  }

  pub fn drag_resize_window(&self, _direction: ResizeDirection) -> Result<(), ExternalError> {
    Err(ExternalError::NotSupported(NotSupportedError::new()))
  }

  pub fn set_ignore_cursor_events(&self, _ignore: bool) -> Result<(), ExternalError> {
    Err(ExternalError::NotSupported(NotSupportedError::new()))
  }

  pub fn set_minimized(&self, _minimized: bool) {
    warn!("`Window::set_minimized` is ignored on iOS")
  }

  pub fn set_maximized(&self, _maximized: bool) {
    warn!("`Window::set_maximized` is ignored on iOS")
  }

  pub fn is_maximized(&self) -> bool {
    warn!("`Window::is_maximized` is ignored on iOS");
    false
  }

  pub fn is_minimized(&self) -> bool {
    warn!("`Window::is_minimized` is ignored on iOS");
    false
  }

  pub fn is_visible(&self) -> bool {
    log::warn!("`Window::is_visible` is ignored on iOS");
    false
  }

  pub fn is_resizable(&self) -> bool {
    warn!("`Window::is_resizable` is ignored on iOS");
    false
  }

  pub fn is_minimizable(&self) -> bool {
    warn!("`Window::is_minimizable` is ignored on iOS");
    false
  }

  pub fn is_maximizable(&self) -> bool {
    warn!("`Window::is_maximizable` is ignored on iOS");
    false
  }

  pub fn is_closable(&self) -> bool {
    warn!("`Window::is_closable` is ignored on iOS");
    false
  }

  pub fn is_decorated(&self) -> bool {
    warn!("`Window::is_decorated` is ignored on iOS");
    false
  }

  pub fn set_fullscreen(&self, monitor: Option<Fullscreen>) {
    unsafe {
      let uiscreen = match monitor {
        Some(Fullscreen::Exclusive(video_mode)) => {
          let uiscreen = video_mode.video_mode.monitor.ui_screen() as id;
          let () = msg_send![uiscreen, setCurrentMode: video_mode.video_mode.screen_mode.0];
          uiscreen
        }
        Some(Fullscreen::Borderless(monitor)) => monitor
          .unwrap_or_else(|| self.current_monitor_inner())
          .ui_screen() as id,
        None => {
          warn!("`Window::set_fullscreen(None)` ignored on iOS");
          return;
        }
      };

      // this is pretty slow on iOS, so avoid doing it if we can
      let current: id = msg_send![self.window, screen];
      if uiscreen != current {
        let () = msg_send![self.window, setScreen: uiscreen];
      }

      let bounds: CGRect = msg_send![uiscreen, bounds];
      let () = msg_send![self.window, setFrame: bounds];

      // For external displays, we must disable overscan compensation or
      // the displayed image will have giant black bars surrounding it on
      // each side
      let () = msg_send![
        uiscreen,
        setOverscanCompensation: UIScreenOverscanCompensation::None
      ];
    }
  }

  pub fn fullscreen(&self) -> Option<Fullscreen> {
    unsafe {
      let monitor = self.current_monitor_inner();
      let uiscreen = monitor.inner.ui_screen();
      let screen_space_bounds = self.screen_frame();
      let screen_bounds: CGRect = msg_send![uiscreen, bounds];

      // TODO: track fullscreen instead of relying on brittle float comparisons
      if screen_space_bounds.origin.x == screen_bounds.origin.x
        && screen_space_bounds.origin.y == screen_bounds.origin.y
        && screen_space_bounds.size.width == screen_bounds.size.width
        && screen_space_bounds.size.height == screen_bounds.size.height
      {
        Some(Fullscreen::Borderless(Some(monitor)))
      } else {
        None
      }
    }
  }

  pub fn set_decorations(&self, _decorations: bool) {
    warn!("`Window::set_decorations` is ignored on iOS")
  }

  pub fn set_always_on_bottom(&self, _always_on_bottom: bool) {
    warn!("`Window::set_always_on_bottom` is ignored on iOS")
  }

  pub fn set_always_on_top(&self, _always_on_top: bool) {
    warn!("`Window::set_always_on_top` is ignored on iOS")
  }

  pub fn set_window_icon(&self, _icon: Option<Icon>) {
    warn!("`Window::set_window_icon` is ignored on iOS")
  }

  pub fn set_ime_position(&self, _position: Position) {
    warn!("`Window::set_ime_position` is ignored on iOS")
  }

  pub fn request_user_attention(&self, _request_type: Option<UserAttentionType>) {
    warn!("`Window::request_user_attention` is ignored on iOS")
  }

  pub fn set_background_color(&self, _color: Option<crate::window::RGBA>) {}

  // Allow directly accessing the current monitor internally without unwrapping.
  fn current_monitor_inner(&self) -> RootMonitorHandle {
    unsafe {
      let uiscreen: id = msg_send![self.window, screen];
      RootMonitorHandle {
        inner: MonitorHandle::retained_new(uiscreen),
      }
    }
  }

  pub fn current_monitor(&self) -> Option<RootMonitorHandle> {
    Some(self.current_monitor_inner())
  }

  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    unsafe { monitor::uiscreens() }
  }

  #[inline]
  pub fn monitor_from_point(&self, _x: f64, _y: f64) -> Option<RootMonitorHandle> {
    warn!("`Window::monitor_from_point` is ignored on iOS");
    None
  }

  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let monitor = unsafe { monitor::main_uiscreen() };
    Some(RootMonitorHandle { inner: monitor })
  }

  pub fn id(&self) -> WindowId {
    self.window.into()
  }

  #[cfg(feature = "rwh_04")]
  pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
    let mut window_handle = rwh_04::UiKitHandle::empty();
    window_handle.ui_window = self.window as _;
    window_handle.ui_view = self.view as _;
    window_handle.ui_view_controller = self.view_controller as _;
    rwh_04::RawWindowHandle::UiKit(window_handle)
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
    let mut window_handle = rwh_05::UiKitWindowHandle::empty();
    window_handle.ui_window = self.window as _;
    window_handle.ui_view = self.view as _;
    window_handle.ui_view_controller = self.view_controller as _;
    rwh_05::RawWindowHandle::UiKit(window_handle)
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::UiKit(rwh_05::UiKitDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
    let mut window_handle = rwh_06::UiKitWindowHandle::new({
      std::ptr::NonNull::new(self.view as _).expect("self.view should never be null")
    });
    window_handle.ui_view_controller = std::ptr::NonNull::new(self.view_controller as _);
    Ok(rwh_06::RawWindowHandle::UiKit(window_handle))
  }

  #[cfg(feature = "rwh_06")]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::UiKit(
      rwh_06::UiKitDisplayHandle::new(),
    ))
  }

  pub fn theme(&self) -> Theme {
    Theme::Light
  }

  /// Sets badge count on iOS launcher. 0 hides the count
  pub fn set_badge_count(&self, count: i32) {
    set_badge_count(count);
  }
}

pub struct Window {
  pub inner: Inner,
}

impl Drop for Window {
  fn drop(&mut self) {
    unsafe {
      assert_main_thread!("`Window::drop` can only be run on the main thread on iOS");
    }
  }
}

unsafe impl Send for Window {}
unsafe impl Sync for Window {}

impl Deref for Window {
  type Target = Inner;

  fn deref(&self) -> &Inner {
    unsafe {
      assert_main_thread!("`Window` methods can only be run on the main thread on iOS");
    }
    &self.inner
  }
}

impl DerefMut for Window {
  fn deref_mut(&mut self) -> &mut Inner {
    unsafe {
      assert_main_thread!("`Window` methods can only be run on the main thread on iOS");
    }
    &mut self.inner
  }
}

impl Window {
  pub fn new<T>(
    _event_loop: &EventLoopWindowTarget<T>,
    window_attributes: WindowAttributes,
    platform_attributes: PlatformSpecificWindowBuilderAttributes,
  ) -> Result<Window, RootOsError> {
    if window_attributes.always_on_top {
      warn!("`WindowAttributes::always_on_top` is unsupported on iOS");
    }
    // TODO: transparency, visible

    unsafe {
      let screen = match window_attributes.fullscreen {
        Some(Fullscreen::Exclusive(ref video_mode)) => {
          video_mode.video_mode.monitor.ui_screen() as id
        }
        Some(Fullscreen::Borderless(Some(ref monitor))) => monitor.inner.ui_screen(),
        Some(Fullscreen::Borderless(None)) | None => monitor::main_uiscreen().ui_screen() as id,
      };

      let screen_bounds: CGRect = msg_send![screen, bounds];

      let frame = match window_attributes.inner_size {
        Some(dim) => {
          let scale_factor = msg_send![screen, scale];
          let size = dim.to_logical::<f64>(scale_factor);
          CGRect {
            origin: screen_bounds.origin,
            size: CGSize {
              width: size.width as _,
              height: size.height as _,
            },
          }
        }
        None => screen_bounds,
      };

      let view = view::create_view(&window_attributes, &platform_attributes, frame);

      let gl_or_metal_backed = {
        let view_class: *const AnyClass = msg_send![view, class];
        let layer_class: *const AnyClass = msg_send![view_class, layerClass];
        let is_metal: bool = msg_send![layer_class, isSubclassOfClass: class!(CAMetalLayer)];
        let is_gl: bool = msg_send![layer_class, isSubclassOfClass: class!(CAEAGLLayer)];
        is_metal || is_gl
      };

      let view_controller =
        view::create_view_controller(&window_attributes, &platform_attributes, view);
      let window = view::create_window(
        &window_attributes,
        &platform_attributes,
        frame,
        view_controller,
      );

      let result = Window {
        inner: Inner {
          window,
          view_controller,
          view,
          gl_or_metal_backed,
        },
      };
      app_state::set_key_window(window);

      // Like the Windows and macOS backends, we send a `ScaleFactorChanged` and `Resized`
      // event on window creation if the DPI factor != 1.0
      let scale_factor: CGFloat = msg_send![view, contentScaleFactor];
      let scale_factor: f64 = scale_factor.into();
      if scale_factor != 1.0 {
        let bounds: CGRect = msg_send![view, bounds];
        let screen: id = msg_send![window, screen];
        let screen_space: id = msg_send![screen, coordinateSpace];
        let screen_frame: CGRect =
          msg_send![view, convertRect:bounds, toCoordinateSpace:screen_space];
        let size = crate::dpi::LogicalSize {
          width: screen_frame.size.width as _,
          height: screen_frame.size.height as _,
        };
        app_state::handle_nonuser_events(
          std::iter::once(EventWrapper::EventProxy(EventProxy::DpiChangedProxy {
            window_id: window,
            scale_factor,
            suggested_size: size,
          }))
          .chain(std::iter::once(EventWrapper::StaticEvent(
            Event::WindowEvent {
              window_id: RootWindowId(window.into()),
              event: WindowEvent::Resized(size.to_physical(scale_factor)),
            },
          ))),
        );
      }

      Ok(result)
    }
  }
}

// WindowExtIOS
impl Inner {
  pub fn ui_window(&self) -> id {
    self.window
  }
  pub fn ui_view_controller(&self) -> id {
    self.view_controller
  }
  pub fn ui_view(&self) -> id {
    self.view
  }

  pub fn set_scale_factor(&self, scale_factor: f64) {
    unsafe {
      assert!(
        dpi::validate_scale_factor(scale_factor),
        "`WindowExtIOS::set_scale_factor` received an invalid hidpi factor"
      );
      let scale_factor = scale_factor as CGFloat;
      let () = msg_send![self.view, setContentScaleFactor: scale_factor];
    }
  }

  pub fn set_valid_orientations(&self, valid_orientations: ValidOrientations) {
    unsafe {
      let idiom = event_loop::get_idiom();
      let supported_orientations =
        UIInterfaceOrientationMask::from_valid_orientations_idiom(valid_orientations, idiom);
      msg_send![
        self.view_controller,
        setSupportedInterfaceOrientations: supported_orientations
      ]
    }
  }

  pub fn set_prefers_home_indicator_hidden(&self, hidden: bool) {
    unsafe {
      let prefers_home_indicator_hidden = if hidden { YES } else { NO };
      let () = msg_send![
        self.view_controller,
        setPrefersHomeIndicatorAutoHidden: prefers_home_indicator_hidden
      ];
    }
  }

  pub fn set_preferred_screen_edges_deferring_system_gestures(&self, edges: ScreenEdge) {
    let edges: UIRectEdge = edges.into();
    unsafe {
      let () = msg_send![
        self.view_controller,
        setPreferredScreenEdgesDeferringSystemGestures: edges
      ];
    }
  }

  pub fn set_prefers_status_bar_hidden(&self, hidden: bool) {
    unsafe {
      let status_bar_hidden = if hidden { YES } else { NO };
      let () = msg_send![
        self.view_controller,
        setPrefersStatusBarHidden: status_bar_hidden
      ];
    }
  }
}

impl Inner {
  // requires main thread
  unsafe fn screen_frame(&self) -> CGRect {
    self.to_screen_space(msg_send![self.window, bounds])
  }

  // requires main thread
  unsafe fn to_screen_space(&self, rect: CGRect) -> CGRect {
    let screen: id = msg_send![self.window, screen];
    if !screen.is_null() {
      let screen_space: id = msg_send![screen, coordinateSpace];
      msg_send![self.window, convertRect:rect, toCoordinateSpace:screen_space]
    } else {
      rect
    }
  }

  // requires main thread
  unsafe fn from_screen_space(&self, rect: CGRect) -> CGRect {
    let screen: id = msg_send![self.window, screen];
    if !screen.is_null() {
      let screen_space: id = msg_send![screen, coordinateSpace];
      msg_send![self.window, convertRect:rect, fromCoordinateSpace:screen_space]
    } else {
      rect
    }
  }

  // requires main thread
  unsafe fn safe_area_screen_space(&self) -> CGRect {
    let bounds: CGRect = msg_send![self.window, bounds];
    if app_state::os_capabilities().safe_area {
      let safe_area: UIEdgeInsets = msg_send![self.window, safeAreaInsets];
      let safe_bounds = CGRect {
        origin: CGPoint {
          x: bounds.origin.x + safe_area.left,
          y: bounds.origin.y + safe_area.top,
        },
        size: CGSize {
          width: bounds.size.width - safe_area.left - safe_area.right,
          height: bounds.size.height - safe_area.top - safe_area.bottom,
        },
      };
      self.to_screen_space(safe_bounds)
    } else {
      let screen_frame = self.to_screen_space(bounds);
      let status_bar_frame: CGRect = {
        let app: id = msg_send![class!(UIApplication), sharedApplication];
        assert!(
          !app.is_null(),
          "`Window::get_inner_position` cannot be called before `EventLoop::run` on iOS"
        );
        msg_send![app, statusBarFrame]
      };
      let (y, height) = if screen_frame.origin.y > status_bar_frame.size.height {
        (screen_frame.origin.y, screen_frame.size.height)
      } else {
        let y = status_bar_frame.size.height;
        let height =
          screen_frame.size.height - (status_bar_frame.size.height - screen_frame.origin.y);
        (y, height)
      };
      CGRect {
        origin: CGPoint {
          x: screen_frame.origin.x,
          y,
        },
        size: CGSize {
          width: screen_frame.size.width,
          height,
        },
      }
    }
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId {
  window: id,
}

impl WindowId {
  pub unsafe fn dummy() -> Self {
    WindowId {
      window: std::ptr::null_mut(),
    }
  }
}

unsafe impl Send for WindowId {}
unsafe impl Sync for WindowId {}

impl From<&AnyObject> for WindowId {
  fn from(window: &AnyObject) -> WindowId {
    WindowId {
      window: window as *const _ as _,
    }
  }
}

impl From<&mut AnyObject> for WindowId {
  fn from(window: &mut AnyObject) -> WindowId {
    WindowId {
      window: window as _,
    }
  }
}

impl From<id> for WindowId {
  fn from(window: id) -> WindowId {
    WindowId { window }
  }
}

#[derive(Clone)]
pub struct PlatformSpecificWindowBuilderAttributes {
  pub root_view_class: &'static AnyClass,
  pub scale_factor: Option<f64>,
  pub valid_orientations: ValidOrientations,
  pub prefers_home_indicator_hidden: bool,
  pub prefers_status_bar_hidden: bool,
  pub preferred_screen_edges_deferring_system_gestures: ScreenEdge,
}

impl Default for PlatformSpecificWindowBuilderAttributes {
  fn default() -> PlatformSpecificWindowBuilderAttributes {
    PlatformSpecificWindowBuilderAttributes {
      root_view_class: class!(UIView),
      scale_factor: None,
      valid_orientations: Default::default(),
      prefers_home_indicator_hidden: false,
      prefers_status_bar_hidden: false,
      preferred_screen_edges_deferring_system_gestures: Default::default(),
    }
  }
}
