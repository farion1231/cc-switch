// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  any::Any,
  cell::{Cell, RefCell},
  collections::VecDeque,
  marker::PhantomData,
  mem,
  os::raw::c_void,
  panic::{catch_unwind, resume_unwind, RefUnwindSafe, UnwindSafe},
  process, ptr,
  rc::{Rc, Weak},
};

use crossbeam_channel::{self as channel, Receiver, Sender};
use objc2::{msg_send, rc::Retained};
use objc2_app_kit::{NSApp, NSApplication, NSEventModifierFlags, NSEventSubtype, NSEventType};
use objc2_foundation::{MainThreadMarker, NSAutoreleasePool, NSInteger, NSPoint, NSTimeInterval};

use crate::{
  dpi::PhysicalPosition,
  error::ExternalError,
  event::Event,
  event_loop::{ControlFlow, EventLoopClosed, EventLoopWindowTarget as RootWindowTarget},
  monitor::MonitorHandle as RootMonitorHandle,
  platform_impl::{
    platform::{
      app::APP_CLASS,
      app_delegate::APP_DELEGATE_CLASS,
      app_state::AppState,
      ffi::{id, nil, YES},
      monitor::{self, MonitorHandle},
      observer::*,
      util::{self, IdRef},
    },
    set_badge_label, set_progress_indicator,
  },
  window::{ProgressBarState, Theme},
};

use super::window::set_ns_theme;

#[derive(Default)]
pub struct PanicInfo {
  inner: Cell<Option<Box<dyn Any + Send + 'static>>>,
}

// WARNING:
// As long as this struct is used through its `impl`, it is UnwindSafe.
// (If `get_mut` is called on `inner`, unwind safety may get broken.)
impl UnwindSafe for PanicInfo {}
impl RefUnwindSafe for PanicInfo {}
impl PanicInfo {
  pub fn is_panicking(&self) -> bool {
    let inner = self.inner.take();
    let result = inner.is_some();
    self.inner.set(inner);
    result
  }
  /// Overwrites the curret state if the current state is not panicking
  pub fn set_panic(&self, p: Box<dyn Any + Send + 'static>) {
    if !self.is_panicking() {
      self.inner.set(Some(p));
    }
  }
  pub fn take(&self) -> Option<Box<dyn Any + Send + 'static>> {
    self.inner.take()
  }
}

#[derive(Clone)]
pub struct EventLoopWindowTarget<T: 'static> {
  pub sender: Sender<T>, // this is only here to be cloned elsewhere
  pub receiver: Receiver<T>,
}

impl<T> Default for EventLoopWindowTarget<T> {
  fn default() -> Self {
    let (sender, receiver) = channel::unbounded();
    EventLoopWindowTarget { sender, receiver }
  }
}

impl<T: 'static> EventLoopWindowTarget<T> {
  #[inline]
  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    monitor::available_monitors()
  }

  #[inline]
  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<MonitorHandle> {
    monitor::from_point(x, y)
  }

  #[inline]
  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let monitor = monitor::primary_monitor();
    Some(RootMonitorHandle { inner: monitor })
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::AppKit(rwh_05::AppKitDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::AppKit(
      rwh_06::AppKitDisplayHandle::new(),
    ))
  }
  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    util::cursor_position()
  }

  #[inline]
  pub fn set_progress_bar(&self, progress: ProgressBarState) {
    set_progress_indicator(progress);
  }

  #[inline]
  pub fn set_badge_count(&self, count: Option<i64>, _desktop_filename: Option<String>) {
    set_badge_label(count.map(|c| c.to_string()));
  }

  #[inline]
  pub fn set_badge_label(&self, label: Option<String>) {
    set_badge_label(label);
  }

  #[inline]
  pub fn set_theme(&self, theme: Option<Theme>) {
    set_ns_theme(theme)
  }
}

pub struct EventLoop<T: 'static> {
  pub(crate) delegate: IdRef,

  window_target: Rc<RootWindowTarget<T>>,
  panic_info: Rc<PanicInfo>,

  /// We make sure that the callback closure is dropped during a panic
  /// by making the event loop own it.
  ///
  /// Every other reference should be a Weak reference which is only upgraded
  /// into a strong reference in order to call the callback but then the
  /// strong reference should be dropped as soon as possible.
  _callback: Option<Rc<RefCell<dyn FnMut(Event<'_, T>, &RootWindowTarget<T>, &mut ControlFlow)>>>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct PlatformSpecificEventLoopAttributes {}

impl<T> EventLoop<T> {
  pub(crate) fn new(_: &PlatformSpecificEventLoopAttributes) -> Self {
    let panic_info: Rc<PanicInfo> = Default::default();
    setup_control_flow_observers(Rc::downgrade(&panic_info));

    let delegate = unsafe {
      if !util::is_main_thread() {
        panic!("On macOS, `EventLoop` must be created on the main thread!");
      }

      // This must be done before `NSApp()` (equivalent to sending
      // `sharedApplication`) is called anywhere else, or we'll end up
      // with the wrong `NSApplication` class and the wrong thread could
      // be marked as main.
      let app: id = msg_send![APP_CLASS.0, sharedApplication];

      let delegate = IdRef::new(msg_send![APP_DELEGATE_CLASS.0, new]);
      let _pool = NSAutoreleasePool::new();
      let _: () = msg_send![app, setDelegate:*delegate];
      delegate
    };

    EventLoop {
      delegate,
      window_target: Rc::new(RootWindowTarget {
        p: Default::default(),
        _marker: PhantomData,
      }),
      panic_info,
      _callback: None,
    }
  }

  pub fn window_target(&self) -> &RootWindowTarget<T> {
    &self.window_target
  }

  pub fn run<F>(mut self, callback: F) -> !
  where
    F: 'static + FnMut(Event<'_, T>, &RootWindowTarget<T>, &mut ControlFlow),
  {
    let exit_code = self.run_return(callback);
    process::exit(exit_code);
  }

  pub fn run_return<F>(&mut self, callback: F) -> i32
  where
    F: FnMut(Event<'_, T>, &RootWindowTarget<T>, &mut ControlFlow),
  {
    // This transmute is always safe, in case it was reached through `run`, since our
    // lifetime will be already 'static. In other cases caller should ensure that all data
    // they passed to callback will actually outlive it, some apps just can't move
    // everything to event loop, so this is something that they should care about.
    let callback = unsafe {
      mem::transmute::<
        Rc<RefCell<dyn FnMut(Event<'_, T>, &RootWindowTarget<T>, &mut ControlFlow)>>,
        Rc<RefCell<dyn FnMut(Event<'_, T>, &RootWindowTarget<T>, &mut ControlFlow)>>,
      >(Rc::new(RefCell::new(callback)))
    };

    self._callback = Some(Rc::clone(&callback));

    let mtm = MainThreadMarker::new().unwrap();

    let exit_code = unsafe {
      let _pool = NSAutoreleasePool::new();
      let app = NSApp(mtm);

      // A bit of juggling with the callback references to make sure
      // that `self.callback` is the only owner of the callback.
      let weak_cb: Weak<_> = Rc::downgrade(&callback);
      mem::drop(callback);

      AppState::set_callback(weak_cb, Rc::clone(&self.window_target));
      let () = msg_send![&app, run];

      if let Some(panic) = self.panic_info.take() {
        drop(self._callback.take());
        resume_unwind(panic);
      }
      AppState::exit()
    };
    drop(self._callback.take());

    exit_code
  }

  pub fn create_proxy(&self) -> Proxy<T> {
    Proxy::new(self.window_target.p.sender.clone())
  }
}

#[inline]
pub unsafe fn post_dummy_event(target: &NSApplication) {
  let event_class = class!(NSEvent);
  let dummy_event: id = msg_send![
      event_class,
      otherEventWithType: NSEventType::ApplicationDefined,
      location: NSPoint::new(0.0, 0.0),
      modifierFlags: NSEventModifierFlags::empty(),
      timestamp: 0 as NSTimeInterval,
      windowNumber: 0 as NSInteger,
      context: nil,
      subtype: NSEventSubtype::WindowExposed,
      data1: 0 as NSInteger,
      data2: 0 as NSInteger,
  ];
  let () = msg_send![target, postEvent: dummy_event, atStart: YES];
}

/// Catches panics that happen inside `f` and when a panic
/// happens, stops the `sharedApplication`
#[inline]
pub fn stop_app_on_panic<F: FnOnce() -> R + UnwindSafe, R>(
  panic_info: Weak<PanicInfo>,
  f: F,
) -> Option<R> {
  match catch_unwind(f) {
    Ok(r) => Some(r),
    Err(e) => {
      // It's important that we set the panic before requesting a `stop`
      // because some callback are still called during the `stop` message
      // and we need to know in those callbacks if the application is currently
      // panicking
      {
        let panic_info = panic_info.upgrade().unwrap();
        panic_info.set_panic(e);
      }
      unsafe {
        let app_class = class!(NSApplication);
        let app: Retained<NSApplication> = msg_send![app_class, sharedApplication];
        let () = msg_send![&app, stop: nil];

        // Posting a dummy event to get `stop` to take effect immediately.
        // See: https://stackoverflow.com/questions/48041279/stopping-the-nsapplication-main-event-loop/48064752#48064752
        post_dummy_event(&app);
      }
      None
    }
  }
}

pub struct Proxy<T> {
  sender: Sender<T>,
  source: CFRunLoopSourceRef,
}

unsafe impl<T: Send> Send for Proxy<T> {}
unsafe impl<T: Send> Sync for Proxy<T> {}

impl<T> Drop for Proxy<T> {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.source as _);
    }
  }
}

impl<T> Clone for Proxy<T> {
  fn clone(&self) -> Self {
    Proxy::new(self.sender.clone())
  }
}

impl<T> Proxy<T> {
  fn new(sender: Sender<T>) -> Self {
    unsafe {
      // just wake up the eventloop
      extern "C" fn event_loop_proxy_handler(_: *mut c_void) {}

      // adding a Source to the main CFRunLoop lets us wake it up and
      // process user events through the normal OS EventLoop mechanisms.
      let rl = CFRunLoopGetMain();
      let mut context: CFRunLoopSourceContext = mem::zeroed();
      context.perform = Some(event_loop_proxy_handler);
      let source = CFRunLoopSourceCreate(ptr::null_mut(), CFIndex::MAX - 1, &mut context);
      CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
      CFRunLoopWakeUp(rl);

      Proxy { sender, source }
    }
  }

  pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed<T>> {
    self
      .sender
      .send(event)
      .map_err(|channel::SendError(x)| EventLoopClosed(x))?;
    unsafe {
      // let the main thread know there's a new event
      CFRunLoopSourceSignal(self.source);
      let rl = CFRunLoopGetMain();
      CFRunLoopWakeUp(rl);
    }
    Ok(())
  }
}
