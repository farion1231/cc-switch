// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  cell::RefCell,
  collections::{HashSet, VecDeque},
  error::Error,
  process,
  rc::Rc,
  sync::atomic::{AtomicBool, Ordering},
  time::Instant,
};

use cairo::{RectangleInt, Region};
use crossbeam_channel::SendError;
use gdk::{Cursor, CursorType, EventKey, EventMask, ScrollDirection, WindowEdge, WindowState};
use gio::Cancellable;
use glib::{source::Priority, MainContext};
use gtk::{
  cairo, gdk, gio,
  glib::{self},
  prelude::*,
  Settings,
};

#[cfg(feature = "x11")]
use crate::platform_impl::platform::device;
use crate::{
  dpi::{LogicalPosition, LogicalSize, PhysicalPosition},
  error::ExternalError,
  event::{
    ElementState, Event, MouseButton, MouseScrollDelta, StartCause, TouchPhase, WindowEvent,
  },
  event_loop::{ControlFlow, EventLoopClosed, EventLoopWindowTarget as RootELW},
  keyboard::ModifiersState,
  monitor::MonitorHandle as RootMonitorHandle,
  platform_impl::platform::DEVICE_ID,
  window::{
    CursorIcon, Fullscreen, ProgressBarState, ResizeDirection, Theme, WindowId as RootWindowId,
  },
};

use super::{
  keyboard,
  monitor::{self, MonitorHandle},
  taskbar, util,
  window::{WindowId, WindowRequest},
};

use taskbar::TaskbarIndicator;

#[derive(Clone)]
pub struct EventLoopWindowTarget<T> {
  /// Gdk display
  pub(crate) display: gdk::Display,
  /// Gtk application
  pub(crate) app: gtk::Application,
  /// Window Ids of the application
  pub(crate) windows: Rc<RefCell<HashSet<WindowId>>>,
  /// Window requests sender
  pub(crate) window_requests_tx: glib::Sender<(WindowId, WindowRequest)>,
  /// Draw event sender
  pub(crate) draw_tx: crossbeam_channel::Sender<WindowId>,
  _marker: std::marker::PhantomData<T>,
}

impl<T> EventLoopWindowTarget<T> {
  #[inline]
  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<MonitorHandle> {
    monitor::from_point(&self.display, x, y)
  }
  #[inline]
  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    let mut handles = VecDeque::new();
    let display = &self.display;
    let numbers = display.n_monitors();

    for i in 0..numbers {
      let monitor = MonitorHandle::new(display, i);
      handles.push_back(monitor);
    }

    handles
  }

  #[inline]
  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let monitor = self.display.primary_monitor();
    monitor.map(|monitor| RootMonitorHandle {
      inner: MonitorHandle { monitor },
    })
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    if self.is_wayland() {
      let mut display_handle = rwh_05::WaylandDisplayHandle::empty();
      display_handle.display = unsafe {
        gdk_wayland_sys::gdk_wayland_display_get_wl_display(self.display.as_ptr() as *mut _)
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
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    if self.is_wayland() {
      let display = unsafe {
        gdk_wayland_sys::gdk_wayland_display_get_wl_display(self.display.as_ptr() as *mut _)
      };
      let display = unsafe { std::ptr::NonNull::new_unchecked(display) };
      let display_handle = rwh_06::WaylandDisplayHandle::new(display);
      Ok(rwh_06::RawDisplayHandle::Wayland(display_handle))
    } else {
      #[cfg(feature = "x11")]
      unsafe {
        if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
          let display = (xlib.XOpenDisplay)(std::ptr::null());
          let screen = (xlib.XDefaultScreen)(display) as _;
          let display = std::ptr::NonNull::new_unchecked(display as _);
          let display_handle = rwh_06::XlibDisplayHandle::new(Some(display), screen);
          Ok(rwh_06::RawDisplayHandle::Xlib(display_handle))
        } else {
          Err(rwh_06::HandleError::Unavailable)
        }
      }
      #[cfg(not(feature = "x11"))]
      Err(rwh_06::HandleError::Unavailable)
    }
  }

  pub fn is_wayland(&self) -> bool {
    self.display.backend().is_wayland()
  }

  #[cfg(feature = "x11")]
  pub fn is_x11(&self) -> bool {
    self.display.backend().is_x11()
  }

  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    util::cursor_position(self.is_wayland())
  }

  #[inline]
  pub fn set_progress_bar(&self, progress: ProgressBarState) {
    if let Err(e) = self
      .window_requests_tx
      .send((WindowId::dummy(), WindowRequest::ProgressBarState(progress)))
    {
      log::warn!("Fail to send update progress bar request: {e}");
    }
  }

  #[inline]
  pub fn set_badge_count(&self, count: Option<i64>, desktop_filename: Option<String>) {
    if let Err(e) = self.window_requests_tx.send((
      WindowId::dummy(),
      WindowRequest::BadgeCount(count, desktop_filename),
    )) {
      log::warn!("Fail to send update progress bar request: {e}");
    }
  }

  #[inline]
  pub fn set_theme(&self, theme: Option<Theme>) {
    if let Err(e) = self
      .window_requests_tx
      .send((WindowId::dummy(), WindowRequest::SetTheme(theme)))
    {
      log::warn!("Fail to send update theme request: {e}");
    }
  }
}

pub struct EventLoop<T: 'static> {
  /// Window target.
  window_target: RootELW<T>,
  /// User event sender for EventLoopProxy
  pub(crate) user_event_tx: crossbeam_channel::Sender<Event<'static, T>>,
  /// Event queue of EventLoop
  events: crossbeam_channel::Receiver<Event<'static, T>>,
  /// Draw queue of EventLoop
  draws: crossbeam_channel::Receiver<WindowId>,
  /// Boolean to control device event thread
  run_device_thread: Option<Rc<AtomicBool>>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlatformSpecificEventLoopAttributes {
  pub(crate) any_thread: bool,
  pub(crate) app_id: Option<String>,
}

impl<T: 'static> EventLoop<T> {
  pub(crate) fn new(attrs: &PlatformSpecificEventLoopAttributes) -> EventLoop<T> {
    if !attrs.any_thread {
      assert_is_main_thread("new_any_thread");
    }

    let context = MainContext::default();
    context
      .with_thread_default(|| {
        EventLoop::new_gtk(attrs.app_id.as_deref()).expect("Failed to initialize gtk backend!")
      })
      .expect("Failed to initialize gtk backend!")
  }

  fn new_gtk(app_id: Option<&str>) -> Result<EventLoop<T>, Box<dyn Error>> {
    // This should be done by gtk::Application::new, but does not work properly
    gtk::init()?;
    let context = MainContext::default();
    let app = gtk::Application::new(app_id, gio::ApplicationFlags::empty());
    let app_ = app.clone();
    let cancellable: Option<&Cancellable> = None;
    app.register(cancellable)?;

    // Send StartCause::Init event
    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let (draw_tx, draw_rx) = crossbeam_channel::unbounded();
    let event_tx_ = event_tx.clone();
    app.connect_activate(move |_| {
      if let Err(e) = event_tx_.send(Event::NewEvents(StartCause::Init)) {
        log::warn!("Failed to send init event to event channel: {}", e);
      }
    });
    let draw_tx_ = draw_tx.clone();
    let user_event_tx = event_tx.clone();

    // Create event loop window target.
    let (window_requests_tx, window_requests_rx) = glib::MainContext::channel(Priority::default());
    let display = gdk::Display::default()
      .expect("GdkDisplay not found. This usually means `gkt_init` hasn't called yet.");
    let window_target = EventLoopWindowTarget {
      display,
      app,
      windows: Rc::new(RefCell::new(HashSet::new())),
      window_requests_tx,
      draw_tx: draw_tx_,
      _marker: std::marker::PhantomData,
    };

    // Spawn x11 thread to receive Device events.
    #[cfg(feature = "x11")]
    let run_device_thread = if window_target.is_x11() {
      let (device_tx, device_rx) = glib::MainContext::channel(glib::Priority::default());
      let user_event_tx = user_event_tx.clone();
      let run_device_thread = Rc::new(AtomicBool::new(true));
      let run = run_device_thread.clone();
      device::spawn(device_tx);
      device_rx.attach(Some(&context), move |event| {
        if let Err(e) = user_event_tx.send(Event::DeviceEvent {
          device_id: DEVICE_ID,
          event,
        }) {
          log::warn!("Fail to send device event to event channel: {}", e);
        }
        if run.load(Ordering::Relaxed) {
          glib::ControlFlow::Continue
        } else {
          glib::ControlFlow::Break
        }
      });
      Some(run_device_thread)
    } else {
      None
    };
    #[cfg(not(feature = "x11"))]
    let run_device_thread = None;

    let mut taskbar = TaskbarIndicator::new();
    let is_wayland = window_target.is_wayland();

    // Window Request
    window_requests_rx.attach(Some(&context), move |(id, request)| {
      if let Some(window) = app_.window_by_id(id.0) {
        match request {
          WindowRequest::Title(title) => window.set_title(&title),
          WindowRequest::Position((x, y)) => window.move_(x, y),
          WindowRequest::Size((w, h)) => window.resize(w, h),
          WindowRequest::SizeConstraints(constraints) => {
            util::set_size_constraints(&window, constraints);
          }
          WindowRequest::Visible(visible) => {
            if visible {
              window.show_all();
            } else {
              window.hide();
            }
          }
          WindowRequest::Focus => {
            window.present_with_time(gdk::ffi::GDK_CURRENT_TIME as _);
          }
          WindowRequest::Resizable(resizable) => window.set_resizable(resizable),
          WindowRequest::Closable(closable) => window.set_deletable(closable),
          WindowRequest::Minimized(minimized) => {
            if minimized {
              window.iconify();
            } else {
              window.deiconify();
            }
          }
          WindowRequest::Maximized(maximized, resizable) => {
            if maximized {
              let maximize_process = util::WindowMaximizeProcess::new(window.clone(), resizable);
              glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
                let mut maximize_process = maximize_process.borrow_mut();
                maximize_process.next_step()
              });
            } else {
              window.unmaximize();
            }
          }
          WindowRequest::DragWindow => {
            if let Some(cursor) = window
              .display()
              .default_seat()
              .and_then(|seat| seat.pointer())
            {
              let (_, x, y) = cursor.position();
              window.begin_move_drag(1, x, y, 0);
            }
          }
          WindowRequest::DragResizeWindow(direction) => {
            if let Some(cursor) = window
              .display()
              .default_seat()
              .and_then(|seat| seat.pointer())
            {
              let (_, x, y) = cursor.position();
              window.begin_resize_drag(
                direction.to_gtk_edge(),
                1,
                x,
                y,
                gtk::gdk::ffi::GDK_CURRENT_TIME as _,
              );
            }
          }
          WindowRequest::Fullscreen(fullscreen) => match fullscreen {
            Some(f) => {
              if let Fullscreen::Borderless(m) = f {
                if let Some(monitor) = m {
                  let display = window.display();
                  let monitor = monitor.inner;
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
            }
            None => window.unfullscreen(),
          },
          WindowRequest::Decorations(decorations) => window.set_decorated(decorations),
          WindowRequest::AlwaysOnBottom(always_on_bottom) => {
            window.set_keep_below(always_on_bottom)
          }
          WindowRequest::AlwaysOnTop(always_on_top) => window.set_keep_above(always_on_top),
          WindowRequest::WindowIcon(window_icon) => {
            if let Some(icon) = window_icon {
              window.set_icon(Some(&icon.inner.into()));
            }
          }
          WindowRequest::UserAttention(request_type) => {
            window.set_urgency_hint(request_type.is_some())
          }
          WindowRequest::SetSkipTaskbar(skip) => {
            window.set_skip_taskbar_hint(skip);
            window.set_skip_pager_hint(skip)
          }
          WindowRequest::BackgroundColor(css_provider, color) => {
            unsafe { window.set_data("background_color", color) };

            let style_context = window.style_context();
            style_context.remove_provider(&css_provider);

            if let Some(color) = color {
              let theme = format!(
                r#"
                  window {{
                    background-color:  rgba({},{},{},{});
                    }}
                    "#,
                color.0,
                color.1,
                color.2,
                color.3 as f64 / 255.0
              );
              let _ = css_provider.load_from_data(theme.as_bytes());
              style_context.add_provider(&css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            };
          }
          WindowRequest::SetVisibleOnAllWorkspaces(visible) => {
            if visible {
              window.stick();
            } else {
              window.unstick();
            }
          }
          WindowRequest::CursorIcon(cursor) => {
            if let Some(gdk_window) = window.window() {
              let display = window.display();
              match cursor {
                Some(cr) => {
                  gdk_window.set_cursor(Cursor::from_name(&display, cr.to_str()).as_ref())
                }
                None => gdk_window
                  .set_cursor(Cursor::for_display(&display, CursorType::BlankCursor).as_ref()),
              }
            };
          }
          WindowRequest::CursorPosition((x, y)) => {
            if let Some(cursor) = window
              .display()
              .default_seat()
              .and_then(|seat| seat.pointer())
            {
              if let Some(screen) = GtkWindowExt::screen(&window) {
                cursor.warp(&screen, x, y);
              }
            }
          }
          WindowRequest::CursorIgnoreEvents(ignore) => {
            if ignore {
              let empty_region = Region::create_rectangle(&RectangleInt::new(0, 0, 1, 1));
              window
                .window()
                .unwrap()
                .input_shape_combine_region(&empty_region, 0, 0);
            } else {
              window.input_shape_combine_region(None)
            };
          }
          WindowRequest::ProgressBarState(_) => unreachable!(),
          WindowRequest::BadgeCount(_, _) => unreachable!(),
          WindowRequest::SetTheme(_) => unreachable!(),
          WindowRequest::WireUpEvents {
            transparent,
            fullscreen,
            cursor_moved,
          } => {
            window.add_events(
              EventMask::POINTER_MOTION_MASK
                | EventMask::BUTTON1_MOTION_MASK
                | EventMask::BUTTON_PRESS_MASK
                | EventMask::TOUCH_MASK
                | EventMask::STRUCTURE_MASK
                | EventMask::FOCUS_CHANGE_MASK
                | EventMask::SCROLL_MASK,
            );

            let fullscreen = Rc::new(AtomicBool::new(fullscreen));
            let fullscreen_ = fullscreen.clone();
            window.connect_window_state_event(move |_window, event| {
              let state = event.changed_mask();
              if state.contains(WindowState::FULLSCREEN) {
                fullscreen_.store(
                  event.new_window_state().contains(WindowState::FULLSCREEN),
                  Ordering::Relaxed,
                );
              }
              glib::Propagation::Proceed
            });

            // Allow resizing unmaximized non-fullscreen undecorated window
            let fullscreen_ = fullscreen.clone();
            window.connect_motion_notify_event(move |window, event| {
              if !window.is_decorated() && window.is_resizable() && !window.is_maximized() {
                if let Some(window) = window.window() {
                  let (cx, cy) = event.root();
                  let (left, top) = window.position();
                  let (w, h) = (window.width(), window.height());
                  let (right, bottom) = (left + w, top + h);
                  let border = window.scale_factor() * 5;
                  let edge = crate::window::hit_test(
                    (left, top, right, bottom),
                    cx as _,
                    cy as _,
                    border,
                    border,
                  );

                  let edge = match &edge {
                    Some(e) if !fullscreen_.load(Ordering::Relaxed) => e.to_cursor_str(),
                    _ => "default",
                  };
                  window.set_cursor(Cursor::from_name(&window.display(), edge).as_ref());
                }
              }
              glib::Propagation::Proceed
            });
            window.connect_button_press_event(move |window, event| {
              const LMB: u32 = 1;
              if (is_wayland || !window.is_decorated())
                && window.is_resizable()
                && !window.is_maximized()
                && event.button() == LMB
              {
                let (cx, cy) = event.root();
                let (left, top) = window.position();
                let (w, h) = window.size();
                let (right, bottom) = (left + w, top + h);
                let border = window.scale_factor() * 5;
                let edge = crate::window::hit_test(
                  (left, top, right, bottom),
                  cx as _,
                  cy as _,
                  border,
                  border,
                )
                .map(|d| d.to_gtk_edge())
                // we return `WindowEdge::__Unknown` to be ignored later.
                // we must return 8 or bigger, otherwise it will be the same as one of the other 7 variants of `WindowEdge` enum.
                .unwrap_or(WindowEdge::__Unknown(8));
                // Ignore the `__Unknown` variant so the window receives the click correctly if it is not on the edges.
                match edge {
                  WindowEdge::__Unknown(_) => (),
                  _ => {
                    // FIXME: calling `window.begin_resize_drag` uses the default cursor, it should show a resizing cursor instead
                    window.begin_resize_drag(edge, LMB as i32, cx as i32, cy as i32, event.time())
                  }
                }
              }

              glib::Propagation::Proceed
            });
            window.connect_touch_event(move |window, event| {
              if !window.is_decorated() && window.is_resizable() && !window.is_maximized() {
                if let Some(window) = window.window() {
                  if let Some((cx, cy)) = event.root_coords() {
                    if let Some(device) = event.device() {
                      let (left, top) = window.position();
                      let (w, h) = (window.width(), window.height());
                      let (right, bottom) = (left + w, top + h);
                      let border = window.scale_factor() * 5;
                      let edge = crate::window::hit_test(
                        (left, top, right, bottom),
                        cx as _,
                        cy as _,
                        border,
                        border,
                      )
                      .map(|d| d.to_gtk_edge())
                      // we return `WindowEdge::__Unknown` to be ignored later.
                      // we must return 8 or bigger, otherwise it will be the same as one of the other 7 variants of `WindowEdge` enum.
                      .unwrap_or(WindowEdge::__Unknown(8));

                      // Ignore the `__Unknown` variant so the window receives the click correctly if it is not on the edges.
                      match edge {
                        WindowEdge::__Unknown(_) => (),
                        _ => window.begin_resize_drag_for_device(
                          edge,
                          &device,
                          0,
                          cx as i32,
                          cy as i32,
                          event.time(),
                        ),
                      }
                    }
                  }
                }
              }

              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_delete_event(move |_, _| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::CloseRequested,
              }) {
                log::warn!("Failed to send window close event to event channel: {}", e);
              }
              glib::Propagation::Stop
            });

            let tx_clone = event_tx.clone();
            window.connect_configure_event(move |window, event| {
              let scale_factor = window.scale_factor();

              let (x, y) = window
                .window()
                .map(|w| w.root_origin())
                .unwrap_or_else(|| event.position());
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::Moved(
                  LogicalPosition::new(x, y).to_physical(scale_factor as f64),
                ),
              }) {
                log::warn!("Failed to send window moved event to event channel: {}", e);
              }

              let (w, h) = event.size();
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::Resized(
                  LogicalSize::new(w, h).to_physical(scale_factor as f64),
                ),
              }) {
                log::warn!(
                  "Failed to send window resized event to event channel: {}",
                  e
                );
              }
              false
            });

            let tx_clone = event_tx.clone();
            window.connect_focus_in_event(move |_, _| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::Focused(true),
              }) {
                log::warn!(
                  "Failed to send window focus-in event to event channel: {}",
                  e
                );
              }
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_focus_out_event(move |_, _| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::Focused(false),
              }) {
                log::warn!(
                  "Failed to send window focus-out event to event channel: {}",
                  e
                );
              }
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_destroy(move |_| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::Destroyed,
              }) {
                log::warn!(
                  "Failed to send window destroyed event to event channel: {}",
                  e
                );
              }
            });

            let tx_clone = event_tx.clone();
            window.connect_enter_notify_event(move |_, _| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::CursorEntered {
                  device_id: DEVICE_ID,
                },
              }) {
                log::warn!(
                  "Failed to send cursor entered event to event channel: {}",
                  e
                );
              }
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_motion_notify_event(move |window, motion| {
              if cursor_moved {
                if let Some(cursor) = motion.device() {
                  let scale_factor = window.scale_factor();
                  let (_, x, y) = cursor.window_at_position();
                  if let Err(e) = tx_clone.send(Event::WindowEvent {
                    window_id: RootWindowId(id),
                    event: WindowEvent::CursorMoved {
                      position: LogicalPosition::new(x, y).to_physical(scale_factor as f64),
                      device_id: DEVICE_ID,
                      // this field is depracted so it is fine to pass empty state
                      modifiers: ModifiersState::empty(),
                    },
                  }) {
                    log::warn!("Failed to send cursor moved event to event channel: {}", e);
                  }
                }
              }
              glib::Propagation::Stop
            });

            let tx_clone = event_tx.clone();
            window.connect_leave_notify_event(move |_, _| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::CursorLeft {
                  device_id: DEVICE_ID,
                },
              }) {
                log::warn!("Failed to send cursor left event to event channel: {}", e);
              }
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_button_press_event(move |_, event| {
              let button = event.button();
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::MouseInput {
                  button: match button {
                    1 => MouseButton::Left,
                    2 => MouseButton::Middle,
                    3 => MouseButton::Right,
                    _ => MouseButton::Other(button as u16),
                  },
                  state: ElementState::Pressed,
                  device_id: DEVICE_ID,
                  // this field is depracted so it is fine to pass empty state
                  modifiers: ModifiersState::empty(),
                },
              }) {
                log::warn!(
                  "Failed to send mouse input pressed event to event channel: {}",
                  e
                );
              }
              glib::Propagation::Stop
            });

            let tx_clone = event_tx.clone();
            window.connect_button_release_event(move |_, event| {
              let button = event.button();
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::MouseInput {
                  button: match button {
                    1 => MouseButton::Left,
                    2 => MouseButton::Middle,
                    3 => MouseButton::Right,
                    _ => MouseButton::Other(button as u16),
                  },
                  state: ElementState::Released,
                  device_id: DEVICE_ID,
                  // this field is depracted so it is fine to pass empty state
                  modifiers: ModifiersState::empty(),
                },
              }) {
                log::warn!(
                  "Failed to send mouse input released event to event channel: {}",
                  e
                );
              }
              glib::Propagation::Stop
            });

            let tx_clone = event_tx.clone();
            window.connect_scroll_event(move |_, event| {
              let (x, y) = event.delta();
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::MouseWheel {
                  device_id: DEVICE_ID,
                  delta: MouseScrollDelta::LineDelta(-x as f32, -y as f32),
                  phase: match event.direction() {
                    ScrollDirection::Smooth => TouchPhase::Moved,
                    _ => TouchPhase::Ended,
                  },
                  modifiers: ModifiersState::empty(),
                },
              }) {
                log::warn!("Failed to send scroll event to event channel: {}", e);
              }
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            let keyboard_handler = Rc::new(move |event_key: EventKey, element_state| {
              // if we have a modifier lets send it
              let mut mods = keyboard::get_modifiers(event_key.clone());
              if !mods.is_empty() {
                // if we release the modifier tell the world
                if ElementState::Released == element_state {
                  mods = ModifiersState::empty();
                }

                if let Err(e) = tx_clone.send(Event::WindowEvent {
                  window_id: RootWindowId(id),
                  event: WindowEvent::ModifiersChanged(mods),
                }) {
                  log::warn!(
                    "Failed to send modifiers changed event to event channel: {}",
                    e
                  );
                } else {
                  // stop here we don't want to send the key event
                  // as we emit the `ModifiersChanged`
                  return glib::ControlFlow::Continue;
                }
              }

              // todo: implement repeat?
              let event = keyboard::make_key_event(&event_key, false, None, element_state);

              if let Some(event) = event {
                if let Err(e) = tx_clone.send(Event::WindowEvent {
                  window_id: RootWindowId(id),
                  event: WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event,
                    is_synthetic: false,
                  },
                }) {
                  log::warn!("Failed to send keyboard event to event channel: {}", e);
                }
              }
              glib::ControlFlow::Continue
            });

            let tx_clone = event_tx.clone();
            // TODO Add actual IME from system
            let ime = gtk::IMContextSimple::default();
            ime.set_client_window(window.window().as_ref());
            ime.focus_in();
            ime.connect_commit(move |_, s| {
              if let Err(e) = tx_clone.send(Event::WindowEvent {
                window_id: RootWindowId(id),
                event: WindowEvent::ReceivedImeText(s.to_string()),
              }) {
                log::warn!(
                  "Failed to send received IME text event to event channel: {}",
                  e
                );
              }
            });

            let handler = keyboard_handler.clone();
            window.connect_key_press_event(move |_, event_key| {
              handler(event_key.to_owned(), ElementState::Pressed);
              ime.filter_keypress(event_key);

              glib::Propagation::Proceed
            });

            let handler = keyboard_handler.clone();
            window.connect_key_release_event(move |_, event_key| {
              handler(event_key.to_owned(), ElementState::Released);
              glib::Propagation::Proceed
            });

            let tx_clone = event_tx.clone();
            window.connect_window_state_event(move |window, event| {
              let state = event.changed_mask();
              if state.contains(WindowState::ICONIFIED) || state.contains(WindowState::MAXIMIZED) {
                let scale_factor = window.scale_factor();

                let (x, y) = window.position();
                if let Err(e) = tx_clone.send(Event::WindowEvent {
                  window_id: RootWindowId(id),
                  event: WindowEvent::Moved(
                    LogicalPosition::new(x, y).to_physical(scale_factor as f64),
                  ),
                }) {
                  log::warn!("Failed to send window moved event to event channel: {}", e);
                }

                let (w, h) = window.size();
                if let Err(e) = tx_clone.send(Event::WindowEvent {
                  window_id: RootWindowId(id),
                  event: WindowEvent::Resized(
                    LogicalSize::new(w, h).to_physical(scale_factor as f64),
                  ),
                }) {
                  log::warn!(
                    "Failed to send window resized event to event channel: {}",
                    e
                  );
                }
              }
              glib::Propagation::Proceed
            });

            // Receive draw events of the window.
            let draw_clone = draw_tx.clone();
            window.connect_draw(move |window, cr| {
              if let Err(e) = draw_clone.send(id) {
                log::warn!("Failed to send redraw event to event channel: {}", e);
              }

              if transparent {
                let background_color = unsafe {
                  window
                    .data::<Option<crate::window::RGBA>>("background_color")
                    .and_then(|c| *c.as_ref())
                };

                let rgba = background_color
                  .map(|(r, g, b, a)| (r as f64, g as f64, b as f64, a as f64 / 255.0))
                  .unwrap_or((0., 0., 0., 0.));

                let rect = window
                  .child()
                  .map(|c| c.allocation())
                  .unwrap_or_else(|| window.allocation());

                cr.rectangle(
                  rect.x() as _,
                  rect.y() as _,
                  rect.width() as _,
                  rect.height() as _,
                );
                cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
                cr.set_operator(cairo::Operator::Source);
                let _ = cr.fill();
                cr.set_operator(cairo::Operator::Over);
              }

              glib::Propagation::Proceed
            });
          }
        }
      } else if id == WindowId::dummy() {
        match request {
          WindowRequest::ProgressBarState(state) => {
            taskbar.update(state);
          }
          WindowRequest::BadgeCount(count, desktop_filename) => {
            taskbar.update_count(count, desktop_filename);
          }
          WindowRequest::SetTheme(theme) => {
            if let Some(settings) = Settings::default() {
              match theme {
                Some(Theme::Dark) => settings.set_gtk_application_prefer_dark_theme(true),
                Some(Theme::Light) | None => settings.set_gtk_application_prefer_dark_theme(false),
              }
            }
          }
          _ => unreachable!(),
        }
      }
      glib::ControlFlow::Continue
    });

    // Create event loop itself.
    let event_loop = Self {
      window_target: RootELW {
        p: window_target,
        _marker: std::marker::PhantomData,
      },
      user_event_tx,
      events: event_rx,
      draws: draw_rx,
      run_device_thread,
    };

    Ok(event_loop)
  }

  #[inline]
  pub fn run<F>(mut self, callback: F) -> !
  where
    F: FnMut(Event<'_, T>, &RootELW<T>, &mut ControlFlow) + 'static,
  {
    let exit_code = self.run_return(callback);
    process::exit(exit_code)
  }

  /// This is the core event loop logic. It basically loops on `gtk_main_iteration` and processes one
  /// event along with that iteration. Depends on current control flow and what it should do, an
  /// event state is defined. The whole state flow chart runs like following:
  ///
  /// ```ignore
  ///                                   Poll/Wait/WaitUntil
  ///       +-------------------------------------------------------------------------+
  ///       |                                                                         |
  ///       |                   Receiving event from event channel                    |   Receiving event from draw channel
  ///       |                               +-------+                                 |   +---+
  ///       v                               v       |                                 |   v   |
  /// +----------+  Poll/Wait/WaitUntil   +------------+  Poll/Wait/WaitUntil   +-----------+ |
  /// | NewStart | ---------------------> | EventQueue | ---------------------> | DrawQueue | |
  /// +----------+                        +------------+                        +-----------+ |
  ///       |ExitWithCode                        |ExitWithCode            ExitWithCode|   |   |
  ///       +------------------------------------+------------------------------------+   +---+
  ///                                            |
  ///                                            v
  ///                                    +---------------+
  ///                                    | LoopDestroyed |
  ///                                    +---------------+
  /// ```
  ///
  /// There are a dew notibale event will sent to callback when state is transisted:
  /// - On any state moves to `LoopDestroyed`, a `LoopDestroyed` event is sent.
  /// - On `NewStart` to `EventQueue`, a `NewEvents` with corresponding `StartCause` depends on
  ///   current control flow is sent.
  /// - On `EventQueue` to `DrawQueue`, a `MainEventsCleared` event is sent.
  /// - On `DrawQueue` back to `NewStart`, a `RedrawEventsCleared` event is sent.
  pub(crate) fn run_return<F>(&mut self, mut callback: F) -> i32
  where
    F: FnMut(Event<'_, T>, &RootELW<T>, &mut ControlFlow),
  {
    enum EventState {
      NewStart,
      EventQueue,
      DrawQueue,
    }

    let context = MainContext::default();
    let run_device_thread = self.run_device_thread.clone();

    context
      .with_thread_default(|| {
        let mut control_flow = ControlFlow::default();
        let window_target = &self.window_target;
        let events = &self.events;
        let draws = &self.draws;

        window_target.p.app.activate();

        let mut state = EventState::NewStart;
        let exit_code = loop {
          let mut blocking = false;
          match state {
            EventState::NewStart => match control_flow {
              ControlFlow::ExitWithCode(code) => {
                callback(Event::LoopDestroyed, window_target, &mut control_flow);
                break code;
              }
              ControlFlow::Wait => {
                if !events.is_empty() {
                  callback(
                    Event::NewEvents(StartCause::WaitCancelled {
                      start: Instant::now(),
                      requested_resume: None,
                    }),
                    window_target,
                    &mut control_flow,
                  );
                  state = EventState::EventQueue;
                } else {
                  blocking = true;
                }
              }
              ControlFlow::WaitUntil(requested_resume) => {
                let start = Instant::now();
                if start >= requested_resume {
                  callback(
                    Event::NewEvents(StartCause::ResumeTimeReached {
                      start,
                      requested_resume,
                    }),
                    window_target,
                    &mut control_flow,
                  );
                  state = EventState::EventQueue;
                } else if !events.is_empty() {
                  callback(
                    Event::NewEvents(StartCause::WaitCancelled {
                      start,
                      requested_resume: Some(requested_resume),
                    }),
                    window_target,
                    &mut control_flow,
                  );
                  state = EventState::EventQueue;
                } else {
                  blocking = true;
                }
              }
              _ => {
                callback(
                  Event::NewEvents(StartCause::Poll),
                  window_target,
                  &mut control_flow,
                );
                state = EventState::EventQueue;
              }
            },
            EventState::EventQueue => match control_flow {
              ControlFlow::ExitWithCode(code) => {
                callback(Event::LoopDestroyed, window_target, &mut control_flow);
                break code;
              }
              _ => match events.try_recv() {
                Ok(event) => match event {
                  Event::LoopDestroyed => control_flow = ControlFlow::ExitWithCode(1),
                  _ => callback(event, window_target, &mut control_flow),
                },
                Err(_) => {
                  callback(Event::MainEventsCleared, window_target, &mut control_flow);
                  state = EventState::DrawQueue;
                }
              },
            },
            EventState::DrawQueue => match control_flow {
              ControlFlow::ExitWithCode(code) => {
                callback(Event::LoopDestroyed, window_target, &mut control_flow);
                break code;
              }
              _ => {
                if let Ok(id) = draws.try_recv() {
                  callback(
                    Event::RedrawRequested(RootWindowId(id)),
                    window_target,
                    &mut control_flow,
                  );
                }
                callback(Event::RedrawEventsCleared, window_target, &mut control_flow);
                state = EventState::NewStart;
              }
            },
          }
          gtk::main_iteration_do(blocking);
        };
        if let Some(run_device_thread) = run_device_thread {
          run_device_thread.store(false, Ordering::Relaxed);
        }
        exit_code
      })
      .unwrap_or(1)
  }

  #[inline]
  pub fn window_target(&self) -> &RootELW<T> {
    &self.window_target
  }

  /// Creates an `EventLoopProxy` that can be used to dispatch user events to the main event loop.
  pub fn create_proxy(&self) -> EventLoopProxy<T> {
    EventLoopProxy {
      user_event_tx: self.user_event_tx.clone(),
    }
  }
}

/// Used to send custom events to `EventLoop`.
#[derive(Debug)]
pub struct EventLoopProxy<T: 'static> {
  user_event_tx: crossbeam_channel::Sender<Event<'static, T>>,
}

impl<T: 'static> Clone for EventLoopProxy<T> {
  fn clone(&self) -> Self {
    Self {
      user_event_tx: self.user_event_tx.clone(),
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
    self
      .user_event_tx
      .send(Event::UserEvent(event))
      .map_err(|SendError(event)| {
        if let Event::UserEvent(error) = event {
          EventLoopClosed(error)
        } else {
          unreachable!();
        }
      })?;

    let context = MainContext::default();
    context.wakeup();

    Ok(())
  }
}

fn assert_is_main_thread(suggested_method: &str) {
  assert!(
    is_main_thread(),
    "Initializing the event loop outside of the main thread is a significant \
             cross-platform compatibility hazard. If you really, absolutely need to create an \
             EventLoop on a different thread, please use the `EventLoopExtUnix::{suggested_method}` function."
  );
}

#[cfg(target_os = "linux")]
fn is_main_thread() -> bool {
  use libc::{c_long, getpid, syscall, SYS_gettid};

  unsafe { syscall(SYS_gettid) == getpid() as c_long }
}

#[cfg(any(target_os = "dragonfly", target_os = "freebsd", target_os = "openbsd"))]
fn is_main_thread() -> bool {
  use libc::pthread_main_np;

  unsafe { pthread_main_np() == 1 }
}

#[cfg(target_os = "netbsd")]
fn is_main_thread() -> bool {
  std::thread::current().name() == Some("main")
}

impl CursorIcon {
  fn to_str(&self) -> &str {
    match self {
      CursorIcon::Crosshair => "crosshair",
      CursorIcon::Hand => "pointer",
      CursorIcon::Arrow => "arrow",
      CursorIcon::Move => "move",
      CursorIcon::Text => "text",
      CursorIcon::Wait => "wait",
      CursorIcon::Help => "help",
      CursorIcon::Progress => "progress",
      CursorIcon::NotAllowed => "not-allowed",
      CursorIcon::ContextMenu => "context-menu",
      CursorIcon::Cell => "cell",
      CursorIcon::VerticalText => "vertical-text",
      CursorIcon::Alias => "alias",
      CursorIcon::Copy => "copy",
      CursorIcon::NoDrop => "no-drop",
      CursorIcon::Grab => "grab",
      CursorIcon::Grabbing => "grabbing",
      CursorIcon::AllScroll => "all-scroll",
      CursorIcon::ZoomIn => "zoom-in",
      CursorIcon::ZoomOut => "zoom-out",
      CursorIcon::EResize => "e-resize",
      CursorIcon::NResize => "n-resize",
      CursorIcon::NeResize => "ne-resize",
      CursorIcon::NwResize => "nw-resize",
      CursorIcon::SResize => "s-resize",
      CursorIcon::SeResize => "se-resize",
      CursorIcon::SwResize => "sw-resize",
      CursorIcon::WResize => "w-resize",
      CursorIcon::EwResize => "ew-resize",
      CursorIcon::NsResize => "ns-resize",
      CursorIcon::NeswResize => "nesw-resize",
      CursorIcon::NwseResize => "nwse-resize",
      CursorIcon::ColResize => "col-resize",
      CursorIcon::RowResize => "row-resize",
      CursorIcon::Default => "default",
    }
  }
}

impl ResizeDirection {
  fn to_cursor_str(&self) -> &str {
    match self {
      ResizeDirection::East => "e-resize",
      ResizeDirection::North => "n-resize",
      ResizeDirection::NorthEast => "ne-resize",
      ResizeDirection::NorthWest => "nw-resize",
      ResizeDirection::South => "s-resize",
      ResizeDirection::SouthEast => "se-resize",
      ResizeDirection::SouthWest => "sw-resize",
      ResizeDirection::West => "w-resize",
    }
  }

  fn to_gtk_edge(&self) -> WindowEdge {
    match self {
      ResizeDirection::East => WindowEdge::East,
      ResizeDirection::North => WindowEdge::North,
      ResizeDirection::NorthEast => WindowEdge::NorthEast,
      ResizeDirection::NorthWest => WindowEdge::NorthWest,
      ResizeDirection::South => WindowEdge::South,
      ResizeDirection::SouthEast => WindowEdge::SouthEast,
      ResizeDirection::SouthWest => WindowEdge::SouthWest,
      ResizeDirection::West => WindowEdge::West,
    }
  }
}
