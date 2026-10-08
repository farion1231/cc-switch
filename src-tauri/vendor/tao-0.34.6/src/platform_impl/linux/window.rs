// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  cell::RefCell,
  collections::VecDeque,
  rc::Rc,
  sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use gtk::{
  gdk::WindowState,
  glib::{self, translate::ToGlibPtr},
  prelude::*,
  CssProvider, Settings,
};

use crate::{
  dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position, Size},
  error::{ExternalError, NotSupportedError, OsError as RootOsError},
  icon::Icon,
  monitor::MonitorHandle as RootMonitorHandle,
  platform_impl::wayland::header::WlHeader,
  window::{
    CursorIcon, Fullscreen, ProgressBarState, ResizeDirection, Theme, UserAttentionType,
    WindowAttributes, WindowSizeConstraints, RGBA,
  },
};

use super::{
  event_loop::EventLoopWindowTarget,
  monitor::{self, MonitorHandle},
  util, Parent, PlatformSpecificWindowBuilderAttributes,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub(crate) u32);

impl WindowId {
  pub fn dummy() -> Self {
    WindowId(u32::MAX)
  }
}

// Currently GTK doesn't provide feature for detect theme, so we need to check theme manually.
// ref: https://github.com/WebKit/WebKit/blob/e44ffaa0d999a9807f76f1805943eea204cfdfbc/Source/WebKit/UIProcess/API/gtk/PageClientImpl.cpp#L587
const GTK_THEME_SUFFIX_LIST: [&str; 3] = ["-dark", "-Dark", "-Darker"];

pub struct Window {
  /// Window id.
  pub(crate) window_id: WindowId,
  /// Gtk application window.
  pub(crate) window: gtk::ApplicationWindow,
  pub(crate) default_vbox: Option<gtk::Box>,
  /// Window requests sender
  pub(crate) window_requests_tx: glib::Sender<(WindowId, WindowRequest)>,
  scale_factor: Rc<AtomicI32>,
  inner_position: Rc<(AtomicI32, AtomicI32)>,
  outer_position: Rc<(AtomicI32, AtomicI32)>,
  outer_size: Rc<(AtomicI32, AtomicI32)>,
  inner_size: Rc<(AtomicI32, AtomicI32)>,
  maximized: Rc<AtomicBool>,
  is_always_on_top: Rc<AtomicBool>,
  minimized: Rc<AtomicBool>,
  fullscreen: RefCell<Option<Fullscreen>>,
  inner_size_constraints: RefCell<WindowSizeConstraints>,
  /// Draw event Sender
  draw_tx: crossbeam_channel::Sender<WindowId>,
  preferred_theme: RefCell<Option<Theme>>,
  css_provider: CssProvider,
}

impl Window {
  pub(crate) fn new<T>(
    event_loop_window_target: &EventLoopWindowTarget<T>,
    attributes: WindowAttributes,
    pl_attribs: PlatformSpecificWindowBuilderAttributes,
  ) -> Result<Self, RootOsError> {
    let app = &event_loop_window_target.app;
    let window_requests_tx = event_loop_window_target.window_requests_tx.clone();
    let draw_tx = event_loop_window_target.draw_tx.clone();
    let is_wayland = event_loop_window_target.is_wayland();

    let mut window_builder = gtk::ApplicationWindow::builder()
      .application(app)
      .accept_focus(attributes.focusable && attributes.focused);
    if let Parent::ChildOf(parent) = pl_attribs.parent {
      window_builder = window_builder.transient_for(&parent);
    }

    let window = window_builder.build();

    if is_wayland {
      WlHeader::setup(&window, &attributes.title);
    }

    let window_id = WindowId(window.id());
    event_loop_window_target
      .windows
      .borrow_mut()
      .insert(window_id);

    // Set Width/Height & Resizable
    let win_scale_factor = window.scale_factor();
    let (width, height) = attributes
      .inner_size
      .map(|size| size.to_logical::<f64>(win_scale_factor as f64).into())
      .unwrap_or((800, 600));
    window.set_default_size(1, 1);
    window.resize(width, height);

    if attributes.maximized {
      let maximize_process = util::WindowMaximizeProcess::new(window.clone(), attributes.resizable);
      glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
        let mut maximize_process = maximize_process.borrow_mut();
        maximize_process.next_step()
      });
    } else {
      window.set_resizable(attributes.resizable);
    }

    window.set_deletable(attributes.closable);

    // Set Min/Max Size
    util::set_size_constraints(&window, attributes.inner_size_constraints);

    // Set Position
    if let Some(position) = attributes.position {
      let (x, y): (i32, i32) = position.to_logical::<i32>(win_scale_factor as f64).into();
      window.move_(x, y);
    }

    // Set GDK Visual
    if pl_attribs.rgba_visual || attributes.transparent {
      if let Some(screen) = GtkWindowExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
          window.set_visual(Some(&visual));
        }
      }
    }

    if pl_attribs.app_paintable || attributes.transparent {
      // Set a few attributes to make the window can be painted.
      // See Gtk drawing model for more info:
      // https://docs.gtk.org/gtk3/drawing-model.html
      window.set_app_paintable(true);
    }

    if !pl_attribs.double_buffered {
      let widget = window.upcast_ref::<gtk::Widget>();
      if !event_loop_window_target.is_wayland() {
        unsafe {
          gtk::ffi::gtk_widget_set_double_buffered(widget.to_glib_none().0, 0);
        }
      }
    }

    let default_vbox = if pl_attribs.default_vbox {
      let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
      window.add(&box_);
      Some(box_)
    } else {
      None
    };

    // Rest attributes
    window.set_title(&attributes.title);
    if let Some(Fullscreen::Borderless(m)) = &attributes.fullscreen {
      if let Some(monitor) = m {
        let display = window.display();
        let monitor = &monitor.inner;
        let monitors = display.n_monitors();
        for i in 0..monitors {
          let m = display.monitor(i).unwrap();
          if m == monitor.monitor {
            let screen = display.default_screen();
            window.fullscreen_on_monitor(&screen, i);
          }
        }
      } else {
        window.fullscreen();
      }
    }
    window.set_visible(attributes.visible);
    window.set_decorated(attributes.decorations);

    if attributes.always_on_bottom {
      window.set_keep_below(attributes.always_on_bottom);
    }

    if attributes.always_on_top {
      window.set_keep_above(attributes.always_on_top);
    }

    if attributes.visible_on_all_workspaces {
      window.stick();
    }

    let preferred_theme = if let Some(settings) = Settings::default() {
      if let Some(preferred_theme) = attributes.preferred_theme {
        match preferred_theme {
          Theme::Dark => settings.set_gtk_application_prefer_dark_theme(true),
          Theme::Light => {
            if let Some(theme) = settings.gtk_theme_name() {
              let theme = theme.as_str();
              // Remove dark variant.
              if let Some(theme) = GTK_THEME_SUFFIX_LIST
                .iter()
                .find(|t| theme.ends_with(*t))
                .map(|v| theme.strip_suffix(v))
              {
                settings.set_gtk_theme_name(theme);
              }
            }
          }
        }
      }
      attributes.preferred_theme
    } else {
      None
    };

    if attributes.visible {
      window.show_all();
    } else {
      window.hide();
    }

    // restore accept-focus after the window has been drawn
    // if the window was initially created without focus and is supposed to be focusable
    if attributes.focusable && !attributes.focused {
      let signal_id = Rc::new(RefCell::new(None));
      let signal_id_ = signal_id.clone();
      let id = window.connect_draw(move |window, _| {
        if let Some(id) = signal_id_.take() {
          window.set_accept_focus(true);
          window.disconnect(id);
        }
        glib::Propagation::Proceed
      });
      signal_id.borrow_mut().replace(id);
    }

    // Check if we should paint the transparent background ourselves.
    let mut transparent = false;
    if attributes.transparent && pl_attribs.auto_transparent {
      transparent = true;
    }
    let cursor_moved = pl_attribs.cursor_moved;
    if let Err(e) = window_requests_tx.send((
      window_id,
      WindowRequest::WireUpEvents {
        transparent,
        fullscreen: attributes.fullscreen.is_some(),
        cursor_moved,
      },
    )) {
      log::warn!("Fail to send wire up events request: {}", e);
    }

    let (
      scale_factor,
      outer_position,
      inner_position,
      outer_size,
      inner_size,
      maximized,
      minimized,
      is_always_on_top,
    ) = Self::setup_signals(&window, Some(&attributes));

    if let Some(icon) = attributes.window_icon {
      window.set_icon(Some(&icon.inner.into()));
    }

    let win = Self {
      window_id,
      window,
      default_vbox,
      window_requests_tx,
      draw_tx,
      scale_factor,
      outer_position,
      inner_position,
      outer_size,
      inner_size,
      maximized,
      minimized,
      is_always_on_top,
      fullscreen: RefCell::new(attributes.fullscreen),
      inner_size_constraints: RefCell::new(attributes.inner_size_constraints),
      preferred_theme: RefCell::new(preferred_theme),
      css_provider: CssProvider::new(),
    };

    let _ = win.set_skip_taskbar(pl_attribs.skip_taskbar);
    win.set_background_color(attributes.background_color);

    Ok(win)
  }

  fn setup_signals(
    window: &gtk::ApplicationWindow,
    attributes: Option<&WindowAttributes>,
  ) -> (
    Rc<AtomicI32>,
    Rc<(AtomicI32, AtomicI32)>,
    Rc<(AtomicI32, AtomicI32)>,
    Rc<(AtomicI32, AtomicI32)>,
    Rc<(AtomicI32, AtomicI32)>,
    Rc<AtomicBool>,
    Rc<AtomicBool>,
    Rc<AtomicBool>,
  ) {
    let win_scale_factor = window.scale_factor();

    let w_pos = window.position();
    let inner_position: Rc<(AtomicI32, AtomicI32)> = Rc::new((w_pos.0.into(), w_pos.1.into()));
    let inner_position_clone = inner_position.clone();

    let o_pos = window.window().map(|w| w.root_origin()).unwrap_or(w_pos);
    let outer_position: Rc<(AtomicI32, AtomicI32)> = Rc::new((o_pos.0.into(), o_pos.1.into()));
    let outer_position_clone = outer_position.clone();

    let w_size = window.size();
    let inner_size: Rc<(AtomicI32, AtomicI32)> = Rc::new((w_size.0.into(), w_size.1.into()));
    let inner_size_clone = inner_size.clone();

    let o_size = window.window().map(|w| w.root_origin()).unwrap_or(w_pos);
    let outer_size: Rc<(AtomicI32, AtomicI32)> = Rc::new((o_size.0.into(), o_size.1.into()));
    let outer_size_clone = outer_size.clone();

    window.connect_configure_event(move |window, event| {
      let (x, y) = event.position();
      inner_position_clone.0.store(x, Ordering::Release);
      inner_position_clone.1.store(y, Ordering::Release);

      let (w, h) = event.size();
      inner_size_clone.0.store(w as i32, Ordering::Release);
      inner_size_clone.1.store(h as i32, Ordering::Release);

      let (x, y, w, h) = window
        .window()
        .map(|w| {
          let rect = w.frame_extents();
          (rect.x(), rect.y(), rect.width(), rect.height())
        })
        .unwrap_or((x, y, w as i32, h as i32));

      outer_position_clone.0.store(x, Ordering::Release);
      outer_position_clone.1.store(y, Ordering::Release);

      outer_size_clone.0.store(w, Ordering::Release);
      outer_size_clone.1.store(h, Ordering::Release);

      false
    });

    let w_max = window.is_maximized();
    let maximized: Rc<AtomicBool> = Rc::new(w_max.into());
    let max_clone = maximized.clone();
    let minimized = Rc::new(AtomicBool::new(false));
    let minimized_clone = minimized.clone();
    let is_always_on_top = Rc::new(AtomicBool::new(
      attributes.map(|a| a.always_on_top).unwrap_or(false),
    ));
    let is_always_on_top_clone = is_always_on_top.clone();

    window.connect_window_state_event(move |_, event| {
      let state = event.new_window_state();
      max_clone.store(state.contains(WindowState::MAXIMIZED), Ordering::Release);
      minimized_clone.store(state.contains(WindowState::ICONIFIED), Ordering::Release);
      is_always_on_top_clone.store(state.contains(WindowState::ABOVE), Ordering::Release);
      glib::Propagation::Proceed
    });

    let scale_factor: Rc<AtomicI32> = Rc::new(win_scale_factor.into());
    let scale_factor_clone = scale_factor.clone();
    window.connect_scale_factor_notify(move |window| {
      scale_factor_clone.store(window.scale_factor(), Ordering::Release);
    });

    (
      scale_factor,
      outer_position,
      inner_position,
      outer_size,
      inner_size,
      maximized,
      minimized,
      is_always_on_top,
    )
  }

  pub(crate) fn new_from_gtk_window<T>(
    event_loop_window_target: &EventLoopWindowTarget<T>,
    window: gtk::ApplicationWindow,
  ) -> Result<Self, RootOsError> {
    let window_requests_tx = event_loop_window_target.window_requests_tx.clone();
    let draw_tx = event_loop_window_target.draw_tx.clone();

    let window_id = WindowId(window.id());
    event_loop_window_target
      .windows
      .borrow_mut()
      .insert(window_id);

    let (
      scale_factor,
      outer_position,
      inner_position,
      outer_size,
      inner_size,
      maximized,
      minimized,
      is_always_on_top,
    ) = Self::setup_signals(&window, None);

    let win = Self {
      window_id,
      window,
      default_vbox: None,
      window_requests_tx,
      draw_tx,
      scale_factor,
      outer_position,
      inner_position,
      outer_size,
      inner_size,
      maximized,
      minimized,
      is_always_on_top,
      fullscreen: RefCell::new(None),
      inner_size_constraints: RefCell::new(WindowSizeConstraints::default()),
      preferred_theme: RefCell::new(None),
      css_provider: CssProvider::new(),
    };

    Ok(win)
  }

  pub fn id(&self) -> WindowId {
    self.window_id
  }

  pub fn scale_factor(&self) -> f64 {
    self.scale_factor.load(Ordering::Acquire) as f64
  }

  pub fn request_redraw(&self) {
    if let Err(e) = self.draw_tx.send(self.window_id) {
      log::warn!("Failed to send redraw event to event channel: {}", e);
    }
  }

  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    let (x, y) = &*self.inner_position;
    Ok(
      LogicalPosition::new(x.load(Ordering::Acquire), y.load(Ordering::Acquire))
        .to_physical(self.scale_factor.load(Ordering::Acquire) as f64),
    )
  }

  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    let (x, y) = &*self.outer_position;
    Ok(
      LogicalPosition::new(x.load(Ordering::Acquire), y.load(Ordering::Acquire))
        .to_physical(self.scale_factor.load(Ordering::Acquire) as f64),
    )
  }

  pub fn set_outer_position<P: Into<Position>>(&self, position: P) {
    let (x, y): (i32, i32) = position
      .into()
      .to_logical::<i32>(self.scale_factor())
      .into();

    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Position((x, y))))
    {
      log::warn!("Fail to send position request: {}", e);
    }
  }

  pub fn set_background_color(&self, color: Option<RGBA>) {
    if let Err(e) = self.window_requests_tx.send((
      self.window_id,
      WindowRequest::BackgroundColor(self.css_provider.clone(), color),
    )) {
      log::warn!("Fail to send size request: {}", e);
    }
  }

  pub fn inner_size(&self) -> PhysicalSize<u32> {
    let (width, height) = &*self.inner_size;

    LogicalSize::new(
      width.load(Ordering::Acquire) as u32,
      height.load(Ordering::Acquire) as u32,
    )
    .to_physical(self.scale_factor.load(Ordering::Acquire) as f64)
  }

  pub fn set_inner_size<S: Into<Size>>(&self, size: S) {
    let (width, height) = size.into().to_logical::<i32>(self.scale_factor()).into();

    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Size((width, height))))
    {
      log::warn!("Fail to send size request: {}", e);
    }
  }

  pub fn outer_size(&self) -> PhysicalSize<u32> {
    let (width, height) = &*self.outer_size;

    LogicalSize::new(
      width.load(Ordering::Acquire) as u32,
      height.load(Ordering::Acquire) as u32,
    )
    .to_physical(self.scale_factor.load(Ordering::Acquire) as f64)
  }

  fn set_size_constraints(&self, constraints: WindowSizeConstraints) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::SizeConstraints(constraints)))
    {
      log::warn!("Fail to send size constraint request: {}", e);
    }
  }

  pub fn set_min_inner_size(&self, size: Option<Size>) {
    let (width, height) = size.map(crate::extract_width_height).unzip();
    let mut size_constraints = self.inner_size_constraints.borrow_mut();
    size_constraints.min_width = width;
    size_constraints.min_height = height;
    self.set_size_constraints(*size_constraints)
  }

  pub fn set_max_inner_size(&self, size: Option<Size>) {
    let (width, height) = size.map(crate::extract_width_height).unzip();
    let mut size_constraints = self.inner_size_constraints.borrow_mut();
    size_constraints.max_width = width;
    size_constraints.max_height = height;
    self.set_size_constraints(*size_constraints)
  }

  pub fn set_inner_size_constraints(&self, constraints: WindowSizeConstraints) {
    *self.inner_size_constraints.borrow_mut() = constraints;
    self.set_size_constraints(constraints)
  }

  pub fn set_title(&self, title: &str) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Title(title.to_string())))
    {
      log::warn!("Fail to send title request: {}", e);
    }
  }

  pub fn title(&self) -> String {
    self
      .window
      .title()
      .map(|t| t.as_str().to_string())
      .unwrap_or_default()
  }

  pub fn set_visible(&self, visible: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Visible(visible)))
    {
      log::warn!("Fail to send visible request: {}", e);
    }
  }

  pub fn set_focus(&self) {
    if !self.minimized.load(Ordering::Acquire) && self.window.get_visible() {
      if let Err(e) = self
        .window_requests_tx
        .send((self.window_id, WindowRequest::Focus))
      {
        log::warn!("Fail to send visible request: {}", e);
      }
    }
  }

  pub fn set_focusable(&self, focusable: bool) {
    self.window.set_accept_focus(focusable);
  }

  pub fn is_focused(&self) -> bool {
    self.window.is_active()
  }

  pub fn set_resizable(&self, resizable: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Resizable(resizable)))
    {
      log::warn!("Fail to send resizable request: {}", e);
    }
  }

  pub fn set_minimizable(&self, _minimizable: bool) {}

  pub fn set_maximizable(&self, _maximizable: bool) {}

  pub fn set_closable(&self, closable: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Closable(closable)))
    {
      log::warn!("Fail to send closable request: {}", e);
    }
  }

  pub fn set_minimized(&self, minimized: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Minimized(minimized)))
    {
      log::warn!("Fail to send minimized request: {}", e);
    }
  }

  pub fn set_maximized(&self, maximized: bool) {
    let resizable = self.is_resizable();

    if let Err(e) = self.window_requests_tx.send((
      self.window_id,
      WindowRequest::Maximized(maximized, resizable),
    )) {
      log::warn!("Fail to send maximized request: {}", e);
    }
  }

  pub fn is_always_on_top(&self) -> bool {
    self.is_always_on_top.load(Ordering::Acquire)
  }

  pub fn is_maximized(&self) -> bool {
    self.maximized.load(Ordering::Acquire)
  }

  pub fn is_minimized(&self) -> bool {
    self.minimized.load(Ordering::Acquire)
  }

  pub fn is_resizable(&self) -> bool {
    self.window.is_resizable()
  }

  pub fn is_minimizable(&self) -> bool {
    true
  }

  pub fn is_maximizable(&self) -> bool {
    true
  }
  pub fn is_closable(&self) -> bool {
    self.window.is_deletable()
  }

  pub fn is_decorated(&self) -> bool {
    self.window.is_decorated()
  }

  #[inline]
  pub fn is_visible(&self) -> bool {
    self.window.is_visible()
  }

  pub fn drag_window(&self) -> Result<(), ExternalError> {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::DragWindow))
    {
      log::warn!("Fail to send drag window request: {}", e);
    }
    Ok(())
  }

  pub fn drag_resize_window(&self, direction: ResizeDirection) -> Result<(), ExternalError> {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::DragResizeWindow(direction)))
    {
      log::warn!("Fail to send drag window request: {}", e);
    }
    Ok(())
  }

  pub fn set_fullscreen(&self, fullscreen: Option<Fullscreen>) {
    self.fullscreen.replace(fullscreen.clone());
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Fullscreen(fullscreen)))
    {
      log::warn!("Fail to send fullscreen request: {}", e);
    }
  }

  pub fn fullscreen(&self) -> Option<Fullscreen> {
    self.fullscreen.borrow().clone()
  }

  pub fn set_decorations(&self, decorations: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::Decorations(decorations)))
    {
      log::warn!("Fail to send decorations request: {}", e);
    }
  }

  pub fn set_always_on_bottom(&self, always_on_bottom: bool) {
    if let Err(e) = self.window_requests_tx.send((
      self.window_id,
      WindowRequest::AlwaysOnBottom(always_on_bottom),
    )) {
      log::warn!("Fail to send always on bottom request: {}", e);
    }
  }

  pub fn set_always_on_top(&self, always_on_top: bool) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::AlwaysOnTop(always_on_top)))
    {
      log::warn!("Fail to send always on top request: {}", e);
    }
  }

  pub fn set_window_icon(&self, window_icon: Option<Icon>) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::WindowIcon(window_icon)))
    {
      log::warn!("Fail to send window icon request: {}", e);
    }
  }

  pub fn set_ime_position<P: Into<Position>>(&self, _position: P) {
    //TODO
  }

  pub fn request_user_attention(&self, request_type: Option<UserAttentionType>) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::UserAttention(request_type)))
    {
      log::warn!("Fail to send user attention request: {}", e);
    }
  }

  pub fn set_visible_on_all_workspaces(&self, visible: bool) {
    if let Err(e) = self.window_requests_tx.send((
      self.window_id,
      WindowRequest::SetVisibleOnAllWorkspaces(visible),
    )) {
      log::warn!("Fail to send visible on all workspaces request: {}", e);
    }
  }
  pub fn set_cursor_icon(&self, cursor: CursorIcon) {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::CursorIcon(Some(cursor))))
    {
      log::warn!("Fail to send cursor icon request: {}", e);
    }
  }

  pub fn set_cursor_position<P: Into<Position>>(&self, position: P) -> Result<(), ExternalError> {
    let inner_pos = self.inner_position().unwrap_or_default();
    let (x, y): (i32, i32) = position
      .into()
      .to_logical::<i32>(self.scale_factor())
      .into();

    if let Err(e) = self.window_requests_tx.send((
      self.window_id,
      WindowRequest::CursorPosition((x + inner_pos.x, y + inner_pos.y)),
    )) {
      log::warn!("Fail to send cursor position request: {}", e);
    }

    Ok(())
  }

  pub fn set_cursor_grab(&self, _grab: bool) -> Result<(), ExternalError> {
    Ok(())
  }

  pub fn set_ignore_cursor_events(&self, ignore: bool) -> Result<(), ExternalError> {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::CursorIgnoreEvents(ignore)))
    {
      log::warn!("Fail to send cursor position request: {}", e);
    }

    Ok(())
  }

  pub fn set_cursor_visible(&self, visible: bool) {
    let cursor = if visible {
      Some(CursorIcon::Default)
    } else {
      None
    };
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::CursorIcon(cursor)))
    {
      log::warn!("Fail to send cursor visibility request: {}", e);
    }
  }

  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    util::cursor_position(self.is_wayland())
  }

  pub fn current_monitor(&self) -> Option<RootMonitorHandle> {
    let display = self.window.display();
    // `.window()` returns `None` if the window is invisible;
    // we fallback to the primary monitor
    let monitor = self
      .window
      .window()
      .and_then(|window| display.monitor_at_window(&window))
      .or_else(|| display.primary_monitor());

    monitor.map(|monitor| RootMonitorHandle {
      inner: MonitorHandle { monitor },
    })
  }

  #[inline]
  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    let mut handles = VecDeque::new();
    let display = self.window.display();
    let numbers = display.n_monitors();

    for i in 0..numbers {
      let monitor = MonitorHandle::new(&display, i);
      handles.push_back(monitor);
    }

    handles
  }

  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let display = self.window.display();
    display.primary_monitor().map(|monitor| {
      let handle = MonitorHandle { monitor };
      RootMonitorHandle { inner: handle }
    })
  }

  #[inline]
  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<RootMonitorHandle> {
    let display = &self.window.display();
    monitor::from_point(display, x, y).map(|inner| RootMonitorHandle { inner })
  }

  fn is_wayland(&self) -> bool {
    self.window.display().backend().is_wayland()
  }

  #[cfg(feature = "rwh_04")]
  #[inline]
  pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
    if self.is_wayland() {
      let mut window_handle = rwh_04::WaylandHandle::empty();
      if let Some(window) = self.window.window() {
        window_handle.surface =
          unsafe { gdk_wayland_sys::gdk_wayland_window_get_wl_surface(window.as_ptr() as *mut _) };
      }

      rwh_04::RawWindowHandle::Wayland(window_handle)
    } else {
      let mut window_handle = rwh_04::XlibHandle::empty();
      unsafe {
        if let Some(window) = self.window.window() {
          window_handle.window = gdk_x11_sys::gdk_x11_window_get_xid(window.as_ptr() as *mut _);
        }
      }
      rwh_04::RawWindowHandle::Xlib(window_handle)
    }
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
    if self.is_wayland() {
      let mut window_handle = rwh_05::WaylandWindowHandle::empty();
      if let Some(window) = self.window.window() {
        window_handle.surface =
          unsafe { gdk_wayland_sys::gdk_wayland_window_get_wl_surface(window.as_ptr() as *mut _) };
      }

      rwh_05::RawWindowHandle::Wayland(window_handle)
    } else {
      let mut window_handle = rwh_05::XlibWindowHandle::empty();
      unsafe {
        if let Some(window) = self.window.window() {
          window_handle.window = gdk_x11_sys::gdk_x11_window_get_xid(window.as_ptr() as *mut _);
        }
      }
      rwh_05::RawWindowHandle::Xlib(window_handle)
    }
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    if self.is_wayland() {
      let mut display_handle = rwh_05::WaylandDisplayHandle::empty();
      display_handle.display = unsafe {
        gdk_wayland_sys::gdk_wayland_display_get_wl_display(self.window.display().as_ptr() as *mut _)
      };
      rwh_05::RawDisplayHandle::Wayland(display_handle)
    } else {
      let mut display_handle = rwh_05::XlibDisplayHandle::empty();
      unsafe {
        if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
          let display = (xlib.XOpenDisplay)(std::ptr::null());
          display_handle.display = display as _;
          display_handle.screen = (xlib.XDefaultScreen)(display) as _;
        }
      }

      rwh_05::RawDisplayHandle::Xlib(display_handle)
    }
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
    if let Some(window) = self.window.window() {
      if self.is_wayland() {
        let surface =
          unsafe { gdk_wayland_sys::gdk_wayland_window_get_wl_surface(window.as_ptr() as *mut _) };
        let surface = unsafe { std::ptr::NonNull::new_unchecked(surface) };
        let window_handle = rwh_06::WaylandWindowHandle::new(surface);
        Ok(rwh_06::RawWindowHandle::Wayland(window_handle))
      } else {
        #[cfg(feature = "x11")]
        {
          let xid = unsafe { gdk_x11_sys::gdk_x11_window_get_xid(window.as_ptr() as *mut _) };
          let window_handle = rwh_06::XlibWindowHandle::new(xid);
          Ok(rwh_06::RawWindowHandle::Xlib(window_handle))
        }
        #[cfg(not(feature = "x11"))]
        Err(rwh_06::HandleError::Unavailable)
      }
    } else {
      Err(rwh_06::HandleError::Unavailable)
    }
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    if self.is_wayland() {
      let display = unsafe {
        gdk_wayland_sys::gdk_wayland_display_get_wl_display(self.window.display().as_ptr() as *mut _)
      };
      let display = unsafe { std::ptr::NonNull::new_unchecked(display) };
      let display_handle = rwh_06::WaylandDisplayHandle::new(display);
      Ok(rwh_06::RawDisplayHandle::Wayland(display_handle))
    } else {
      #[cfg(feature = "x11")]
      if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
        unsafe {
          let display = (xlib.XOpenDisplay)(std::ptr::null());
          let screen = (xlib.XDefaultScreen)(display) as _;
          let display = std::ptr::NonNull::new_unchecked(display as _);
          let display_handle = rwh_06::XlibDisplayHandle::new(Some(display), screen);
          Ok(rwh_06::RawDisplayHandle::Xlib(display_handle))
        }
      } else {
        Err(rwh_06::HandleError::Unavailable)
      }
      #[cfg(not(feature = "x11"))]
      Err(rwh_06::HandleError::Unavailable)
    }
  }

  pub fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError> {
    if let Err(e) = self
      .window_requests_tx
      .send((self.window_id, WindowRequest::SetSkipTaskbar(skip)))
    {
      log::warn!("Fail to send skip taskbar request: {}", e);
    }

    Ok(())
  }

  pub fn set_progress_bar(&self, progress: ProgressBarState) {
    if let Err(e) = self
      .window_requests_tx
      .send((WindowId::dummy(), WindowRequest::ProgressBarState(progress)))
    {
      log::warn!("Fail to send update progress bar request: {}", e);
    }
  }

  pub fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>) {
    if let Err(e) = self.window_requests_tx.send((
      WindowId::dummy(),
      WindowRequest::BadgeCount(count, desktop_filename),
    )) {
      log::warn!("Fail to send update badge count request: {}", e);
    }
  }

  pub fn theme(&self) -> Theme {
    if let Some(theme) = *self.preferred_theme.borrow() {
      return theme;
    }

    if let Some(theme) = Settings::default().and_then(|s| s.gtk_theme_name()) {
      let theme = theme.as_str();
      if GTK_THEME_SUFFIX_LIST.iter().any(|t| theme.ends_with(t)) {
        return Theme::Dark;
      }
    }

    Theme::Light
  }

  pub fn set_theme(&self, theme: Option<Theme>) {
    *self.preferred_theme.borrow_mut() = theme;
    if let Err(e) = self
      .window_requests_tx
      .send((WindowId::dummy(), WindowRequest::SetTheme(theme)))
    {
      log::warn!("Fail to send set theme request: {e}");
    }
  }
}

// We need GtkWindow to initialize WebView, so we have to keep it in the field.
// It is called on any method.
unsafe impl Send for Window {}
unsafe impl Sync for Window {}

#[non_exhaustive]
pub enum WindowRequest {
  Title(String),
  Position((i32, i32)),
  Size((i32, i32)),
  SizeConstraints(WindowSizeConstraints),
  Visible(bool),
  Focus,
  Resizable(bool),
  Closable(bool),
  Minimized(bool),
  Maximized(bool, bool),
  DragWindow,
  DragResizeWindow(ResizeDirection),
  Fullscreen(Option<Fullscreen>),
  Decorations(bool),
  AlwaysOnBottom(bool),
  AlwaysOnTop(bool),
  WindowIcon(Option<Icon>),
  UserAttention(Option<UserAttentionType>),
  SetSkipTaskbar(bool),
  CursorIcon(Option<CursorIcon>),
  CursorPosition((i32, i32)),
  CursorIgnoreEvents(bool),
  WireUpEvents {
    transparent: bool,
    fullscreen: bool,
    cursor_moved: bool,
  },
  SetVisibleOnAllWorkspaces(bool),
  ProgressBarState(ProgressBarState),
  BadgeCount(Option<i64>, Option<String>),
  SetTheme(Option<Theme>),
  BackgroundColor(CssProvider, Option<RGBA>),
}

impl Drop for Window {
  fn drop(&mut self) {
    unsafe {
      self.window.destroy();
    }
  }
}
