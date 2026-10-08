// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "android")]
use crate::{
  dpi::{PhysicalPosition, PhysicalSize, Position, Size},
  error, event,
  event_loop::{self, ControlFlow},
  keyboard::{Key, KeyCode, KeyLocation, NativeKeyCode},
  monitor,
  window::{self, ResizeDirection, Theme, WindowSizeConstraints},
};
use crossbeam_channel::{Receiver, Sender};
use ndk::{
  configuration::Configuration,
  event::{InputEvent, KeyAction, MotionAction},
  looper::{ForeignLooper, Poll, ThreadLooper},
};
use once_cell::sync::Lazy;
use std::{
  collections::VecDeque,
  sync::RwLock,
  time::{Duration, Instant},
};

pub mod ndk_glue;
use ndk_glue::{Event, Rect};

static CONFIG: Lazy<RwLock<Configuration>> = Lazy::new(|| RwLock::new(Configuration::new()));

enum EventSource {
  Callback,
  InputQueue,
  User,
}

fn poll(poll: Poll) -> Option<EventSource> {
  match poll {
    Poll::Event { ident, .. } => match ident {
      ndk_glue::NDK_GLUE_LOOPER_EVENT_PIPE_IDENT => Some(EventSource::Callback),
      ndk_glue::NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT => Some(EventSource::InputQueue),
      _ => unreachable!(),
    },
    Poll::Timeout => None,
    Poll::Wake => Some(EventSource::User),
    Poll::Callback => unreachable!(),
  }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {}

pub struct EventLoop<T: 'static> {
  window_target: event_loop::EventLoopWindowTarget<T>,
  receiver: Receiver<T>,
  sender_to_clone: Sender<T>,
  first_event: Option<EventSource>,
  start_cause: event::StartCause,
  looper: ThreadLooper,
  running: bool,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlatformSpecificEventLoopAttributes {}

macro_rules! call_event_handler {
  ( $event_handler:expr, $window_target:expr, $cf:expr, $event:expr ) => {{
    if let ControlFlow::ExitWithCode(code) = $cf {
      $event_handler($event, $window_target, &mut ControlFlow::ExitWithCode(code));
    } else {
      $event_handler($event, $window_target, &mut $cf);
    }
  }};
}

impl<T: 'static> EventLoop<T> {
  pub(crate) fn new(_: &PlatformSpecificEventLoopAttributes) -> Self {
    let (sender, receiver) = crossbeam_channel::unbounded();

    Self {
      window_target: event_loop::EventLoopWindowTarget {
        p: EventLoopWindowTarget {
          _marker: std::marker::PhantomData,
        },
        _marker: std::marker::PhantomData,
      },
      sender_to_clone: sender,
      receiver,
      first_event: None,
      start_cause: event::StartCause::Init,
      looper: ThreadLooper::for_thread().unwrap(),
      running: false,
    }
  }

  pub fn run<F>(mut self, event_handler: F) -> !
  where
    F:
      'static + FnMut(event::Event<'_, T>, &event_loop::EventLoopWindowTarget<T>, &mut ControlFlow),
  {
    let exit_code = self.run_return(event_handler);
    ::std::process::exit(exit_code);
  }

  pub fn run_return<F>(&mut self, mut event_handler: F) -> i32
  where
    F: FnMut(event::Event<'_, T>, &event_loop::EventLoopWindowTarget<T>, &mut ControlFlow),
  {
    let mut control_flow = ControlFlow::default();

    'event_loop: loop {
      call_event_handler!(
        event_handler,
        self.window_target(),
        control_flow,
        event::Event::NewEvents(self.start_cause)
      );

      let mut redraw = false;
      let mut resized = false;

      match self.first_event.take() {
        Some(EventSource::Callback) => match ndk_glue::poll_events().unwrap() {
          Event::Resume => {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::Resumed
            );
          }
          Event::WindowResized => resized = true,
          Event::WindowRedrawNeeded => redraw = true,
          Event::Pause => {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::Suspended
            );
          }
          Event::Stop => self.running = false,
          Event::Start => self.running = true,
          Event::ConfigChanged => {
            // #[allow(deprecated)] // TODO: use ndk-context instead
            // let am = ndk_glue::native_activity().asset_manager();
            // let config = Configuration::from_asset_manager(&am);
            // let old_scale_factor = MonitorHandle.scale_factor();
            // *CONFIG.write().unwrap() = config;
            // let scale_factor = MonitorHandle.scale_factor();
            // if (scale_factor - old_scale_factor).abs() < f64::EPSILON {
            //   let mut size = MonitorHandle.size();
            //   let event = event::Event::WindowEvent {
            //     window_id: window::WindowId(WindowId),
            //     event: event::WindowEvent::ScaleFactorChanged {
            //       new_inner_size: &mut size,
            //       scale_factor,
            //     },
            //   };
            //   call_event_handler!(event_handler, self.window_target(), control_flow, event);
            // }
          }
          Event::WindowHasFocus => {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::WindowEvent {
                window_id: window::WindowId(WindowId),
                event: event::WindowEvent::Focused(true),
              }
            );
          }
          Event::WindowLostFocus => {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::WindowEvent {
                window_id: window::WindowId(WindowId),
                event: event::WindowEvent::Focused(false),
              }
            );
          }
          Event::Destroy => {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::WindowEvent {
                window_id: window::WindowId(WindowId),
                event: event::WindowEvent::Destroyed,
              }
            );
          }
          _ => {}
        },
        Some(EventSource::InputQueue) => {
          if let Some(input_queue) = ndk_glue::input_queue().as_ref() {
            while let Ok(Some(event)) = input_queue.event() {
              if let Some(event) = input_queue.pre_dispatch(event) {
                let mut handled = true;
                let window_id = window::WindowId(WindowId);
                let device_id = event::DeviceId(DeviceId);
                match &event {
                  InputEvent::MotionEvent(motion_event) => {
                    let phase = match motion_event.action() {
                      MotionAction::Down | MotionAction::PointerDown => {
                        Some(event::TouchPhase::Started)
                      }
                      MotionAction::Up | MotionAction::PointerUp => Some(event::TouchPhase::Ended),
                      MotionAction::Move => Some(event::TouchPhase::Moved),
                      MotionAction::Cancel => Some(event::TouchPhase::Cancelled),
                      _ => {
                        handled = false;
                        None // TODO mouse events
                      }
                    };
                    if let Some(phase) = phase {
                      let pointers: Box<dyn Iterator<Item = ndk::event::Pointer<'_>>> = match phase
                      {
                        event::TouchPhase::Started | event::TouchPhase::Ended => {
                          Box::new(std::iter::once(
                            motion_event.pointer_at_index(motion_event.pointer_index()),
                          ))
                        }
                        event::TouchPhase::Moved | event::TouchPhase::Cancelled => {
                          Box::new(motion_event.pointers())
                        }
                      };

                      for pointer in pointers {
                        let location = PhysicalPosition {
                          x: pointer.x() as _,
                          y: pointer.y() as _,
                        };
                        let event = event::Event::WindowEvent {
                          window_id,
                          event: event::WindowEvent::Touch(event::Touch {
                            device_id,
                            phase,
                            location,
                            id: pointer.pointer_id() as u64,
                            force: None,
                          }),
                        };
                        call_event_handler!(
                          event_handler,
                          self.window_target(),
                          control_flow,
                          event
                        );
                      }
                    }
                  }
                  InputEvent::KeyEvent(key) => {
                    let state = match key.action() {
                      KeyAction::Down => event::ElementState::Pressed,
                      KeyAction::Up => event::ElementState::Released,
                      _ => event::ElementState::Released,
                    };

                    let keycode = key.key_code();
                    let native = NativeKeyCode::Android(keycode.into());
                    let physical_key = KeyCode::Unidentified(native);
                    let logical_key = keycode_to_logical(keycode, native);
                    // TODO: maybe use getUnicodeChar to get the logical key

                    let event = event::Event::WindowEvent {
                      window_id,
                      event: event::WindowEvent::KeyboardInput {
                        device_id,
                        event: event::KeyEvent {
                          state,
                          physical_key,
                          logical_key,
                          location: keycode_to_location(keycode),
                          repeat: key.repeat_count() > 0,
                          text: None,
                          platform_specific: KeyEventExtra {},
                        },
                        is_synthetic: false,
                      },
                    };
                    call_event_handler!(event_handler, self.window_target(), control_flow, event);
                  }
                  _ => {}
                };
                input_queue.finish_event(event, handled);
              }
            }
          }
        }
        Some(EventSource::User) => {
          while let Ok(event) = self.receiver.try_recv() {
            call_event_handler!(
              event_handler,
              self.window_target(),
              control_flow,
              event::Event::UserEvent(event)
            );
          }
        }
        None => {}
      }

      call_event_handler!(
        event_handler,
        self.window_target(),
        control_flow,
        event::Event::MainEventsCleared
      );

      if resized && self.running {
        let size = MonitorHandle.size();
        let event = event::Event::WindowEvent {
          window_id: window::WindowId(WindowId),
          event: event::WindowEvent::Resized(size),
        };
        call_event_handler!(event_handler, self.window_target(), control_flow, event);
      }

      if redraw && self.running {
        let event = event::Event::RedrawRequested(window::WindowId(WindowId));
        call_event_handler!(event_handler, self.window_target(), control_flow, event);
      }

      call_event_handler!(
        event_handler,
        self.window_target(),
        control_flow,
        event::Event::RedrawEventsCleared
      );

      match control_flow {
        ControlFlow::ExitWithCode(code) => {
          self.first_event = poll(
            self
              .looper
              .poll_once_timeout(Duration::from_millis(0))
              .unwrap(),
          );
          self.start_cause = event::StartCause::WaitCancelled {
            start: Instant::now(),
            requested_resume: None,
          };

          call_event_handler!(
            event_handler,
            self.window_target(),
            control_flow,
            event::Event::LoopDestroyed
          );

          break 'event_loop code;
        }
        ControlFlow::Poll => {
          self.first_event = poll(
            self
              .looper
              .poll_all_timeout(Duration::from_millis(0))
              .unwrap(),
          );
          self.start_cause = event::StartCause::Poll;
        }
        ControlFlow::Wait => {
          self.first_event = poll(self.looper.poll_all().unwrap());
          self.start_cause = event::StartCause::WaitCancelled {
            start: Instant::now(),
            requested_resume: None,
          }
        }
        ControlFlow::WaitUntil(instant) => {
          let start = Instant::now();
          let duration = if instant <= start {
            Duration::default()
          } else {
            instant - start
          };
          self.first_event = poll(self.looper.poll_all_timeout(duration).unwrap());
          self.start_cause = if self.first_event.is_some() {
            event::StartCause::WaitCancelled {
              start,
              requested_resume: Some(instant),
            }
          } else {
            event::StartCause::ResumeTimeReached {
              start,
              requested_resume: instant,
            }
          }
        }
      }
    }
  }

  pub fn window_target(&self) -> &event_loop::EventLoopWindowTarget<T> {
    &self.window_target
  }

  pub fn create_proxy(&self) -> EventLoopProxy<T> {
    EventLoopProxy {
      queue: self.sender_to_clone.clone(),
      looper: ForeignLooper::for_thread().expect("called from event loop thread"),
    }
  }
}

pub struct EventLoopProxy<T: 'static> {
  queue: Sender<T>,
  looper: ForeignLooper,
}

impl<T> EventLoopProxy<T> {
  pub fn send_event(&self, event: T) -> Result<(), event_loop::EventLoopClosed<T>> {
    _ = self.queue.try_send(event);
    self.looper.wake();
    Ok(())
  }
}

impl<T> Clone for EventLoopProxy<T> {
  fn clone(&self) -> Self {
    EventLoopProxy {
      queue: self.queue.clone(),
      looper: self.looper.clone(),
    }
  }
}

#[derive(Clone)]
pub struct EventLoopWindowTarget<T: 'static> {
  _marker: std::marker::PhantomData<T>,
}

impl<T: 'static> EventLoopWindowTarget<T> {
  pub fn primary_monitor(&self) -> Option<monitor::MonitorHandle> {
    Some(monitor::MonitorHandle {
      inner: MonitorHandle,
    })
  }

  #[inline]
  pub fn monitor_from_point(&self, _x: f64, _y: f64) -> Option<MonitorHandle> {
    warn!("`Window::monitor_from_point` is ignored on Android");
    return None;
  }

  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    let mut v = VecDeque::with_capacity(1);
    v.push_back(MonitorHandle);
    v
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::Android(rwh_05::AndroidDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::Android(
      rwh_06::AndroidDisplayHandle::new(),
    ))
  }

  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, error::ExternalError> {
    debug!("`EventLoopWindowTarget::cursor_position` is ignored on Android");
    Ok((0, 0).into())
  }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId;

impl WindowId {
  pub fn dummy() -> Self {
    WindowId
  }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId;

impl DeviceId {
  pub fn dummy() -> Self {
    DeviceId
  }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformSpecificWindowBuilderAttributes;

pub struct Window;

impl Window {
  pub fn new<T: 'static>(
    _el: &EventLoopWindowTarget<T>,
    _window_attrs: window::WindowAttributes,
    _: PlatformSpecificWindowBuilderAttributes,
  ) -> Result<Self, error::OsError> {
    // FIXME this ignores requested window attributes
    Ok(Self)
  }

  pub fn id(&self) -> WindowId {
    WindowId
  }

  pub fn primary_monitor(&self) -> Option<monitor::MonitorHandle> {
    Some(monitor::MonitorHandle {
      inner: MonitorHandle,
    })
  }

  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    let mut v = VecDeque::with_capacity(1);
    v.push_back(MonitorHandle);
    v
  }

  #[inline]
  pub fn monitor_from_point(&self, _x: f64, _y: f64) -> Option<monitor::MonitorHandle> {
    warn!("`Window::monitor_from_point` is ignored on Android");
    None
  }

  pub fn current_monitor(&self) -> Option<monitor::MonitorHandle> {
    Some(monitor::MonitorHandle {
      inner: MonitorHandle,
    })
  }

  pub fn scale_factor(&self) -> f64 {
    MonitorHandle.scale_factor()
  }

  pub fn request_redraw(&self) {
    // TODO
  }

  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
    Err(error::NotSupportedError::new())
  }

  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
    Err(error::NotSupportedError::new())
  }

  pub fn set_outer_position(&self, _position: Position) {
    // no effect
  }

  pub fn inner_size(&self) -> PhysicalSize<u32> {
    self.outer_size()
  }

  pub fn set_inner_size(&self, _size: Size) {
    warn!("Cannot set window size on Android");
  }

  pub fn outer_size(&self) -> PhysicalSize<u32> {
    MonitorHandle.size()
  }

  pub fn set_min_inner_size(&self, _: Option<Size>) {}
  pub fn set_max_inner_size(&self, _: Option<Size>) {}
  pub fn set_inner_size_constraints(&self, _: WindowSizeConstraints) {}

  pub fn set_title(&self, _title: &str) {}
  pub fn title(&self) -> String {
    String::new()
  }

  pub fn set_visible(&self, _visibility: bool) {}

  pub fn set_focus(&self) {
    //FIXME: implementation goes here
    warn!("set_focus not yet implemented on Android");
  }

  pub fn set_focusable(&self, _focusable: bool) {
    warn!("set_focusable not yet implemented on Android");
  }

  pub fn is_focused(&self) -> bool {
    log::warn!("`Window::is_focused` is ignored on Android");
    false
  }

  pub fn is_always_on_top(&self) -> bool {
    log::warn!("`Window::is_always_on_top` is ignored on Android");
    false
  }

  pub fn set_resizable(&self, _resizeable: bool) {
    warn!("`Window::set_resizable` is ignored on Android")
  }

  pub fn set_minimizable(&self, _minimizable: bool) {
    warn!("`Window::set_minimizable` is ignored on Android")
  }

  pub fn set_maximizable(&self, _maximizable: bool) {
    warn!("`Window::set_maximizable` is ignored on Android")
  }

  pub fn set_closable(&self, _closable: bool) {
    warn!("`Window::set_closable` is ignored on Android")
  }

  pub fn set_minimized(&self, _minimized: bool) {}

  pub fn set_maximized(&self, _maximized: bool) {}

  pub fn is_maximized(&self) -> bool {
    false
  }

  pub fn is_minimized(&self) -> bool {
    false
  }

  pub fn is_visible(&self) -> bool {
    log::warn!("`Window::is_visible` is ignored on Android");
    false
  }

  pub fn is_resizable(&self) -> bool {
    warn!("`Window::is_resizable` is ignored on Android");
    false
  }

  pub fn is_minimizable(&self) -> bool {
    warn!("`Window::is_minimizable` is ignored on Android");
    false
  }

  pub fn is_maximizable(&self) -> bool {
    warn!("`Window::is_maximizable` is ignored on Android");
    false
  }

  pub fn is_closable(&self) -> bool {
    warn!("`Window::is_closable` is ignored on Android");
    false
  }

  pub fn is_decorated(&self) -> bool {
    warn!("`Window::is_decorated` is ignored on Android");
    false
  }

  pub fn set_fullscreen(&self, _monitor: Option<window::Fullscreen>) {
    warn!("Cannot set fullscreen on Android");
  }

  pub fn fullscreen(&self) -> Option<window::Fullscreen> {
    None
  }

  pub fn set_decorations(&self, _decorations: bool) {}

  pub fn set_always_on_bottom(&self, _always_on_bottom: bool) {}

  pub fn set_always_on_top(&self, _always_on_top: bool) {}

  pub fn set_window_icon(&self, _window_icon: Option<crate::icon::Icon>) {}

  pub fn set_ime_position(&self, _position: Position) {}

  pub fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {}

  pub fn set_cursor_icon(&self, _: window::CursorIcon) {}

  pub fn set_cursor_position(&self, _: Position) -> Result<(), error::ExternalError> {
    Err(error::ExternalError::NotSupported(
      error::NotSupportedError::new(),
    ))
  }

  pub fn set_cursor_grab(&self, _: bool) -> Result<(), error::ExternalError> {
    Err(error::ExternalError::NotSupported(
      error::NotSupportedError::new(),
    ))
  }

  pub fn set_cursor_visible(&self, _: bool) {}

  pub fn drag_window(&self) -> Result<(), error::ExternalError> {
    Err(error::ExternalError::NotSupported(
      error::NotSupportedError::new(),
    ))
  }

  pub fn drag_resize_window(
    &self,
    _direction: ResizeDirection,
  ) -> Result<(), error::ExternalError> {
    Err(error::ExternalError::NotSupported(
      error::NotSupportedError::new(),
    ))
  }

  pub fn set_background_color(&self, _color: Option<crate::window::RGBA>) {}

  pub fn set_ignore_cursor_events(&self, _ignore: bool) -> Result<(), error::ExternalError> {
    Err(error::ExternalError::NotSupported(
      error::NotSupportedError::new(),
    ))
  }

  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, error::ExternalError> {
    debug!("`Window::cursor_position` is ignored on Android");
    Ok((0, 0).into())
  }

  #[cfg(feature = "rwh_04")]
  pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
    // TODO: Use main activity instead?
    let mut handle = rwh_04::AndroidNdkHandle::empty();
    if let Some(w) = ndk_glue::window_manager().as_ref() {
      handle.a_native_window = w.as_obj().as_raw() as *mut _;
    } else {
      panic!("Cannot get the native window, it's null and will always be null before Event::Resumed and after Event::Suspended. Make sure you only call this function between those events.");
    };
    rwh_04::RawWindowHandle::AndroidNdk(handle)
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
    // TODO: Use main activity instead?
    let mut handle = rwh_05::AndroidNdkWindowHandle::empty();
    if let Some(w) = ndk_glue::window_manager().as_ref() {
      handle.a_native_window = w.as_obj().as_raw() as *mut _;
    } else {
      panic!("Cannot get the native window, it's null and will always be null before Event::Resumed and after Event::Suspended. Make sure you only call this function between those events.");
    };
    rwh_05::RawWindowHandle::AndroidNdk(handle)
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::Android(rwh_05::AndroidDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
    // TODO: Use main activity instead?
    if let Some(w) = ndk_glue::window_manager().as_ref() {
      let native_window =
        unsafe { std::ptr::NonNull::new_unchecked(w.as_obj().as_raw() as *mut _) };
      // native_window shuldn't be null
      let handle = rwh_06::AndroidNdkWindowHandle::new(native_window);
      Ok(rwh_06::RawWindowHandle::AndroidNdk(handle))
    } else {
      Err(rwh_06::HandleError::Unavailable)
    }
  }

  #[cfg(feature = "rwh_06")]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::Android(
      rwh_06::AndroidDisplayHandle::new(),
    ))
  }
  pub fn config(&self) -> Configuration {
    CONFIG.read().unwrap().clone()
  }

  pub fn content_rect(&self) -> Rect {
    ndk_glue::content_rect()
  }

  pub fn theme(&self) -> Theme {
    Theme::Light
  }
}

#[derive(Default, Clone, Debug)]
pub struct OsError;

use std::fmt::{self, Display, Formatter};
impl Display for OsError {
  fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
    write!(fmt, "Android OS Error")
  }
}

pub(crate) use crate::icon::NoIcon as PlatformIcon;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonitorHandle;

impl MonitorHandle {
  pub fn name(&self) -> Option<String> {
    Some("Android Device".to_owned())
  }

  pub fn size(&self) -> PhysicalSize<u32> {
    // TODO decide how to get JNIENV
    if let Some(w) = ndk_glue::window_manager().as_ref() {
      let ctx = ndk_context::android_context();
      let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
      let mut env = vm.attach_current_thread().unwrap();
      let window_manager = w.as_obj();
      let metrics = env
        .call_method(
          window_manager,
          "getCurrentWindowMetrics",
          "()Landroid/view/WindowMetrics;",
          &[],
        )
        .unwrap()
        .l()
        .unwrap();
      let rect = env
        .call_method(&metrics, "getBounds", "()Landroid/graphics/Rect;", &[])
        .unwrap()
        .l()
        .unwrap();
      let width = env
        .call_method(&rect, "width", "()I", &[])
        .unwrap()
        .i()
        .unwrap();
      let height = env
        .call_method(&rect, "height", "()I", &[])
        .unwrap()
        .i()
        .unwrap();
      PhysicalSize::new(width as u32, height as u32)
    } else {
      PhysicalSize::new(0, 0)
    }
  }

  pub fn position(&self) -> PhysicalPosition<i32> {
    (0, 0).into()
  }

  pub fn scale_factor(&self) -> f64 {
    let config = CONFIG.read().unwrap();
    config
      .density()
      .map(|dpi| dpi as f64 / 160.0)
      .unwrap_or(1.0)
  }

  pub fn video_modes(&self) -> impl Iterator<Item = monitor::VideoMode> {
    let size = self.size().into();
    let mut v = Vec::new();
    // FIXME this is not the real refresh rate
    // (it is guarunteed to support 32 bit color though)
    v.push(monitor::VideoMode {
      video_mode: VideoMode {
        size,
        bit_depth: 32,
        refresh_rate: 60,
        monitor: self.clone(),
      },
    });
    v.into_iter()
  }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoMode {
  size: (u32, u32),
  bit_depth: u16,
  refresh_rate: u16,
  monitor: MonitorHandle,
}

impl VideoMode {
  pub fn size(&self) -> PhysicalSize<u32> {
    self.size.into()
  }

  pub fn bit_depth(&self) -> u16 {
    self.bit_depth
  }

  pub fn refresh_rate(&self) -> u16 {
    self.refresh_rate
  }

  pub fn monitor(&self) -> monitor::MonitorHandle {
    monitor::MonitorHandle {
      inner: self.monitor.clone(),
    }
  }
}

fn keycode_to_logical(keycode: ndk::event::Keycode, native: NativeKeyCode) -> Key<'static> {
  use ndk::event::Keycode::*;

  // The android `Keycode` is sort-of layout dependent. More specifically
  // if I press the Z key using a US layout, then I get KEYCODE_Z,
  // but if I press the same key after switching to a HUN layout, I get
  // KEYCODE_Y.
  //
  // To prevents us from using this value to determine the `physical_key`
  // (also know as winit's `KeyCode`)
  //
  // Unfortunately the documentation says that the scancode values
  // "are not reliable and vary from device to device". Which seems to mean
  // that there's no way to reliably get the physical_key on android.

  match keycode {
    Unknown => Key::Unidentified(native),

    // Can be added on demand
    SoftLeft => Key::Unidentified(native),
    SoftRight => Key::Unidentified(native),

    // Using `BrowserHome` instead of `GoHome` according to
    // https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/key/Key_Values
    Home => Key::BrowserHome,
    Back => Key::BrowserBack,
    Call => Key::Call,
    Endcall => Key::EndCall,

    //-------------------------------------------------------------------------------
    // Reporting unidentified, because the specific character is layout dependent.
    // (I'm not sure though)
    Keycode0 => Key::Unidentified(native),
    Keycode1 => Key::Unidentified(native),
    Keycode2 => Key::Unidentified(native),
    Keycode3 => Key::Unidentified(native),
    Keycode4 => Key::Unidentified(native),
    Keycode5 => Key::Unidentified(native),
    Keycode6 => Key::Unidentified(native),
    Keycode7 => Key::Unidentified(native),
    Keycode8 => Key::Unidentified(native),
    Keycode9 => Key::Unidentified(native),
    Star => Key::Unidentified(native),
    Pound => Key::Unidentified(native),
    A => Key::Unidentified(native),
    B => Key::Unidentified(native),
    C => Key::Unidentified(native),
    D => Key::Unidentified(native),
    E => Key::Unidentified(native),
    F => Key::Unidentified(native),
    G => Key::Unidentified(native),
    H => Key::Unidentified(native),
    I => Key::Unidentified(native),
    J => Key::Unidentified(native),
    K => Key::Unidentified(native),
    L => Key::Unidentified(native),
    M => Key::Unidentified(native),
    N => Key::Unidentified(native),
    O => Key::Unidentified(native),
    P => Key::Unidentified(native),
    Q => Key::Unidentified(native),
    R => Key::Unidentified(native),
    S => Key::Unidentified(native),
    T => Key::Unidentified(native),
    U => Key::Unidentified(native),
    V => Key::Unidentified(native),
    W => Key::Unidentified(native),
    X => Key::Unidentified(native),
    Y => Key::Unidentified(native),
    Z => Key::Unidentified(native),
    Comma => Key::Unidentified(native),
    Period => Key::Unidentified(native),
    Grave => Key::Unidentified(native),
    Minus => Key::Unidentified(native),
    Equals => Key::Unidentified(native),
    LeftBracket => Key::Unidentified(native),
    RightBracket => Key::Unidentified(native),
    Backslash => Key::Unidentified(native),
    Semicolon => Key::Unidentified(native),
    Apostrophe => Key::Unidentified(native),
    Slash => Key::Unidentified(native),
    At => Key::Unidentified(native),
    Plus => Key::Unidentified(native),
    //-------------------------------------------------------------------------------
    DpadUp => Key::ArrowUp,
    DpadDown => Key::ArrowDown,
    DpadLeft => Key::ArrowLeft,
    DpadRight => Key::ArrowRight,
    DpadCenter => Key::Enter,

    VolumeUp => Key::AudioVolumeUp,
    VolumeDown => Key::AudioVolumeDown,
    Power => Key::Power,
    Camera => Key::Camera,
    Clear => Key::Clear,

    AltLeft => Key::Alt,
    AltRight => Key::Alt,
    ShiftLeft => Key::Shift,
    ShiftRight => Key::Shift,
    Tab => Key::Tab,
    Space => Key::Space,
    Sym => Key::Symbol,
    Explorer => Key::LaunchWebBrowser,
    Envelope => Key::LaunchMail,
    Enter => Key::Enter,
    Del => Key::Backspace,

    // According to https://developer.android.com/reference/android/view/KeyEvent#KEYCODE_NUM
    Num => Key::Alt,

    Headsethook => Key::HeadsetHook,
    Focus => Key::CameraFocus,

    Menu => Key::Unidentified(native),

    Notification => Key::Notification,
    Search => Key::BrowserSearch,
    MediaPlayPause => Key::MediaPlayPause,
    MediaStop => Key::MediaStop,
    MediaNext => Key::MediaTrackNext,
    MediaPrevious => Key::MediaTrackPrevious,
    MediaRewind => Key::MediaRewind,
    MediaFastForward => Key::MediaFastForward,
    Mute => Key::MicrophoneVolumeMute,
    PageUp => Key::PageUp,
    PageDown => Key::PageDown,
    Pictsymbols => Key::Unidentified(native),
    SwitchCharset => Key::Unidentified(native),

    // -----------------------------------------------------------------
    // Gamepad events should be exposed through a separate API, not
    // keyboard events
    ButtonA => Key::Unidentified(native),
    ButtonB => Key::Unidentified(native),
    ButtonC => Key::Unidentified(native),
    ButtonX => Key::Unidentified(native),
    ButtonY => Key::Unidentified(native),
    ButtonZ => Key::Unidentified(native),
    ButtonL1 => Key::Unidentified(native),
    ButtonR1 => Key::Unidentified(native),
    ButtonL2 => Key::Unidentified(native),
    ButtonR2 => Key::Unidentified(native),
    ButtonThumbl => Key::Unidentified(native),
    ButtonThumbr => Key::Unidentified(native),
    ButtonStart => Key::Unidentified(native),
    ButtonSelect => Key::Unidentified(native),
    ButtonMode => Key::Unidentified(native),
    // -----------------------------------------------------------------
    Escape => Key::Escape,
    ForwardDel => Key::Delete,
    CtrlLeft => Key::Control,
    CtrlRight => Key::Control,
    CapsLock => Key::CapsLock,
    ScrollLock => Key::ScrollLock,
    MetaLeft => Key::Super,
    MetaRight => Key::Super,
    Function => Key::Fn,
    Sysrq => Key::PrintScreen,
    Break => Key::Pause,
    MoveHome => Key::Home,
    MoveEnd => Key::End,
    Insert => Key::Insert,
    Forward => Key::BrowserForward,
    MediaPlay => Key::MediaPlay,
    MediaPause => Key::MediaPause,
    MediaClose => Key::MediaClose,
    MediaEject => Key::Eject,
    MediaRecord => Key::MediaRecord,
    F1 => Key::F1,
    F2 => Key::F2,
    F3 => Key::F3,
    F4 => Key::F4,
    F5 => Key::F5,
    F6 => Key::F6,
    F7 => Key::F7,
    F8 => Key::F8,
    F9 => Key::F9,
    F10 => Key::F10,
    F11 => Key::F11,
    F12 => Key::F12,
    NumLock => Key::NumLock,
    Numpad0 => Key::Unidentified(native),
    Numpad1 => Key::Unidentified(native),
    Numpad2 => Key::Unidentified(native),
    Numpad3 => Key::Unidentified(native),
    Numpad4 => Key::Unidentified(native),
    Numpad5 => Key::Unidentified(native),
    Numpad6 => Key::Unidentified(native),
    Numpad7 => Key::Unidentified(native),
    Numpad8 => Key::Unidentified(native),
    Numpad9 => Key::Unidentified(native),
    NumpadDivide => Key::Unidentified(native),
    NumpadMultiply => Key::Unidentified(native),
    NumpadSubtract => Key::Unidentified(native),
    NumpadAdd => Key::Unidentified(native),
    NumpadDot => Key::Unidentified(native),
    NumpadComma => Key::Unidentified(native),
    NumpadEnter => Key::Unidentified(native),
    NumpadEquals => Key::Unidentified(native),
    NumpadLeftParen => Key::Unidentified(native),
    NumpadRightParen => Key::Unidentified(native),

    VolumeMute => Key::AudioVolumeMute,
    Info => Key::Info,
    ChannelUp => Key::ChannelUp,
    ChannelDown => Key::ChannelDown,
    ZoomIn => Key::ZoomIn,
    ZoomOut => Key::ZoomOut,
    Tv => Key::TV,
    Window => Key::Unidentified(native),
    Guide => Key::Guide,
    Dvr => Key::DVR,
    Bookmark => Key::BrowserFavorites,
    Captions => Key::ClosedCaptionToggle,
    Settings => Key::Settings,
    TvPower => Key::TVPower,
    TvInput => Key::TVInput,
    StbPower => Key::STBPower,
    StbInput => Key::STBInput,
    AvrPower => Key::AVRPower,
    AvrInput => Key::AVRInput,
    ProgRed => Key::ColorF0Red,
    ProgGreen => Key::ColorF1Green,
    ProgYellow => Key::ColorF2Yellow,
    ProgBlue => Key::ColorF3Blue,
    AppSwitch => Key::AppSwitch,
    Button1 => Key::Unidentified(native),
    Button2 => Key::Unidentified(native),
    Button3 => Key::Unidentified(native),
    Button4 => Key::Unidentified(native),
    Button5 => Key::Unidentified(native),
    Button6 => Key::Unidentified(native),
    Button7 => Key::Unidentified(native),
    Button8 => Key::Unidentified(native),
    Button9 => Key::Unidentified(native),
    Button10 => Key::Unidentified(native),
    Button11 => Key::Unidentified(native),
    Button12 => Key::Unidentified(native),
    Button13 => Key::Unidentified(native),
    Button14 => Key::Unidentified(native),
    Button15 => Key::Unidentified(native),
    Button16 => Key::Unidentified(native),
    LanguageSwitch => Key::GroupNext,
    MannerMode => Key::MannerMode,
    Keycode3dMode => Key::TV3DMode,
    Contacts => Key::LaunchContacts,
    Calendar => Key::LaunchCalendar,
    Music => Key::LaunchMusicPlayer,
    Calculator => Key::LaunchApplication2,
    ZenkakuHankaku => Key::ZenkakuHankaku,
    Eisu => Key::Eisu,
    Muhenkan => Key::NonConvert,
    Henkan => Key::Convert,
    KatakanaHiragana => Key::HiraganaKatakana,
    Yen => Key::Unidentified(native),
    Ro => Key::Unidentified(native),
    Kana => Key::KanjiMode,
    Assist => Key::Unidentified(native),
    BrightnessDown => Key::BrightnessDown,
    BrightnessUp => Key::BrightnessUp,
    MediaAudioTrack => Key::MediaAudioTrack,
    Sleep => Key::Standby,
    Wakeup => Key::WakeUp,
    Pairing => Key::Pairing,
    MediaTopMenu => Key::MediaTopMenu,
    Keycode11 => Key::Unidentified(native),
    Keycode12 => Key::Unidentified(native),
    LastChannel => Key::MediaLast,
    TvDataService => Key::TVDataService,
    VoiceAssist => Key::VoiceDial,
    TvRadioService => Key::TVRadioService,
    TvTeletext => Key::Teletext,
    TvNumberEntry => Key::TVNumberEntry,
    TvTerrestrialAnalog => Key::TVTerrestrialAnalog,
    TvTerrestrialDigital => Key::TVTerrestrialDigital,
    TvSatellite => Key::TVSatellite,
    TvSatelliteBs => Key::TVSatelliteBS,
    TvSatelliteCs => Key::TVSatelliteCS,
    TvSatelliteService => Key::TVSatelliteToggle,
    TvNetwork => Key::TVNetwork,
    TvAntennaCable => Key::TVAntennaCable,
    TvInputHdmi1 => Key::TVInputHDMI1,
    TvInputHdmi2 => Key::TVInputHDMI2,
    TvInputHdmi3 => Key::TVInputHDMI3,
    TvInputHdmi4 => Key::TVInputHDMI4,
    TvInputComposite1 => Key::TVInputComposite1,
    TvInputComposite2 => Key::TVInputComposite2,
    TvInputComponent1 => Key::TVInputComponent1,
    TvInputComponent2 => Key::TVInputComponent2,
    TvInputVga1 => Key::TVInputVGA1,
    TvAudioDescription => Key::TVAudioDescription,
    TvAudioDescriptionMixUp => Key::TVAudioDescriptionMixUp,
    TvAudioDescriptionMixDown => Key::TVAudioDescriptionMixDown,
    TvZoomMode => Key::ZoomToggle,
    TvContentsMenu => Key::TVContentsMenu,
    TvMediaContextMenu => Key::TVMediaContext,
    TvTimerProgramming => Key::TVTimer,
    Help => Key::Help,
    NavigatePrevious => Key::NavigatePrevious,
    NavigateNext => Key::NavigateNext,
    NavigateIn => Key::NavigateIn,
    NavigateOut => Key::NavigateOut,
    StemPrimary => Key::Unidentified(native),
    Stem1 => Key::Unidentified(native),
    Stem2 => Key::Unidentified(native),
    Stem3 => Key::Unidentified(native),
    DpadUpLeft => Key::Unidentified(native),
    DpadDownLeft => Key::Unidentified(native),
    DpadUpRight => Key::Unidentified(native),
    DpadDownRight => Key::Unidentified(native),
    MediaSkipForward => Key::MediaSkipForward,
    MediaSkipBackward => Key::MediaSkipBackward,
    MediaStepForward => Key::MediaStepForward,
    MediaStepBackward => Key::MediaStepBackward,
    SoftSleep => Key::Unidentified(native),
    Cut => Key::Cut,
    Copy => Key::Copy,
    Paste => Key::Paste,
    SystemNavigationUp => Key::Unidentified(native),
    SystemNavigationDown => Key::Unidentified(native),
    SystemNavigationLeft => Key::Unidentified(native),
    SystemNavigationRight => Key::Unidentified(native),
    AllApps => Key::Unidentified(native),
    Refresh => Key::BrowserRefresh,
    ThumbsUp => Key::Unidentified(native),
    ThumbsDown => Key::Unidentified(native),
    ProfileSwitch => Key::Unidentified(native),
    _ => Key::Unidentified(native),
  }
}

fn keycode_to_location(keycode: ndk::event::Keycode) -> KeyLocation {
  use ndk::event::Keycode::*;

  match keycode {
    AltLeft => KeyLocation::Left,
    AltRight => KeyLocation::Right,
    ShiftLeft => KeyLocation::Left,
    ShiftRight => KeyLocation::Right,

    // According to https://developer.android.com/reference/android/view/KeyEvent#KEYCODE_NUM
    Num => KeyLocation::Left,

    CtrlLeft => KeyLocation::Left,
    CtrlRight => KeyLocation::Right,
    MetaLeft => KeyLocation::Left,
    MetaRight => KeyLocation::Right,

    NumLock => KeyLocation::Numpad,
    Numpad0 => KeyLocation::Numpad,
    Numpad1 => KeyLocation::Numpad,
    Numpad2 => KeyLocation::Numpad,
    Numpad3 => KeyLocation::Numpad,
    Numpad4 => KeyLocation::Numpad,
    Numpad5 => KeyLocation::Numpad,
    Numpad6 => KeyLocation::Numpad,
    Numpad7 => KeyLocation::Numpad,
    Numpad8 => KeyLocation::Numpad,
    Numpad9 => KeyLocation::Numpad,
    NumpadDivide => KeyLocation::Numpad,
    NumpadMultiply => KeyLocation::Numpad,
    NumpadSubtract => KeyLocation::Numpad,
    NumpadAdd => KeyLocation::Numpad,
    NumpadDot => KeyLocation::Numpad,
    NumpadComma => KeyLocation::Numpad,
    NumpadEnter => KeyLocation::Numpad,
    NumpadEquals => KeyLocation::Numpad,
    NumpadLeftParen => KeyLocation::Numpad,
    NumpadRightParen => KeyLocation::Numpad,

    _ => KeyLocation::Standard,
  }
}

// FIXME: Implement android
pub fn keycode_to_scancode(_code: KeyCode) -> Option<u32> {
  None
}

pub fn keycode_from_scancode(_scancode: u32) -> KeyCode {
  KeyCode::Unidentified(NativeKeyCode::Unidentified)
}
