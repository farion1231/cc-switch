// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![deny(unused_results)]

use std::{
  cell::{RefCell, RefMut},
  collections::HashSet,
  mem,
  os::raw::c_void,
  ptr,
  time::Instant,
};

use objc2::runtime::AnyObject;
use once_cell::sync::Lazy;

use crate::{
  dpi::LogicalSize,
  event::{Event, StartCause, WindowEvent},
  event_loop::ControlFlow,
  platform_impl::platform::{
    event_loop::{EventHandler, EventProxy, EventWrapper, Never},
    ffi::{
      id, kCFRunLoopCommonModes, CFAbsoluteTimeGetCurrent, CFRelease, CFRunLoopAddTimer,
      CFRunLoopGetMain, CFRunLoopRef, CFRunLoopTimerCreate, CFRunLoopTimerInvalidate,
      CFRunLoopTimerRef, CFRunLoopTimerSetNextFireDate, CGRect, CGSize, NSInteger,
      NSOperatingSystemVersion, NSUInteger,
    },
  },
  window::WindowId as RootWindowId,
};

macro_rules! bug {
    ($($msg:tt)*) => {
        panic!("tao iOS bug, file an issue: {}", format!($($msg)*))
    };
}

macro_rules! bug_assert {
    ($test:expr, $($msg:tt)*) => {
        assert!($test, "tao iOS bug, file an issue: {}", format!($($msg)*))
    };
}

enum UserCallbackTransitionResult<'a> {
  Success {
    event_handler: Box<dyn EventHandler>,
    active_control_flow: ControlFlow,
    processing_redraws: bool,
  },
  ReentrancyPrevented {
    queued_events: &'a mut Vec<EventWrapper>,
  },
}

impl Event<'static, Never> {
  fn is_redraw(&self) -> bool {
    matches!(self, Event::RedrawRequested(_))
  }
}

// this is the state machine for the app lifecycle
#[derive(Debug)]
#[must_use = "dropping `AppStateImpl` without inspecting it is probably a bug"]
enum AppStateImpl {
  NotLaunched {
    queued_windows: Vec<id>,
    queued_events: Vec<EventWrapper>,
    queued_gpu_redraws: HashSet<id>,
  },
  Launching {
    queued_windows: Vec<id>,
    queued_events: Vec<EventWrapper>,
    queued_event_handler: Box<dyn EventHandler>,
    queued_gpu_redraws: HashSet<id>,
  },
  ProcessingEvents {
    event_handler: Box<dyn EventHandler>,
    queued_gpu_redraws: HashSet<id>,
    active_control_flow: ControlFlow,
  },
  // special state to deal with reentrancy and prevent mutable aliasing.
  InUserCallback {
    queued_events: Vec<EventWrapper>,
    queued_gpu_redraws: HashSet<id>,
  },
  ProcessingRedraws {
    event_handler: Box<dyn EventHandler>,
    active_control_flow: ControlFlow,
  },
  Waiting {
    waiting_event_handler: Box<dyn EventHandler>,
    start: Instant,
  },
  PollFinished {
    waiting_event_handler: Box<dyn EventHandler>,
  },
  Terminated,
}

struct AppState {
  // This should never be `None`, except for briefly during a state transition.
  app_state: Option<AppStateImpl>,
  control_flow: ControlFlow,
  waker: EventLoopWaker,
}

impl Drop for AppState {
  fn drop(&mut self) {
    match self.state_mut() {
      &mut AppStateImpl::NotLaunched {
        ref mut queued_windows,
        ..
      }
      | &mut AppStateImpl::Launching {
        ref mut queued_windows,
        ..
      } => {
        for &mut window in queued_windows {
          unsafe {
            let () = msg_send![window, release];
          }
        }
      }
      _ => {}
    }
  }
}

impl AppState {
  // requires main thread
  unsafe fn get_mut() -> RefMut<'static, AppState> {
    // basically everything in UIKit requires the main thread, so it's pointless to use the
    // std::sync APIs.
    // must be mut because plain `static` requires `Sync`
    static mut APP_STATE: RefCell<Option<AppState>> = RefCell::new(None);

    if cfg!(debug_assertions) {
      assert_main_thread!(
        "bug in tao: `AppState::get_mut()` can only be called on the main thread"
      );
    }
    let mut guard = APP_STATE.borrow_mut();
    if guard.is_none() {
      #[inline(never)]
      #[cold]
      unsafe fn init_guard(guard: &mut RefMut<'static, Option<AppState>>) {
        let waker = EventLoopWaker::new(CFRunLoopGetMain());
        **guard = Some(AppState {
          app_state: Some(AppStateImpl::NotLaunched {
            queued_windows: Vec::new(),
            queued_events: Vec::new(),
            queued_gpu_redraws: HashSet::new(),
          }),
          control_flow: ControlFlow::default(),
          waker,
        });
      }
      init_guard(&mut guard)
    }
    RefMut::map(guard, |state| state.as_mut().unwrap())
  }

  fn state(&self) -> &AppStateImpl {
    match &self.app_state {
      Some(ref state) => state,
      None => bug!("`AppState` previously failed a state transition"),
    }
  }

  fn state_mut(&mut self) -> &mut AppStateImpl {
    match &mut self.app_state {
      Some(ref mut state) => state,
      None => bug!("`AppState` previously failed a state transition"),
    }
  }

  fn take_state(&mut self) -> AppStateImpl {
    match self.app_state.take() {
      Some(state) => state,
      None => bug!("`AppState` previously failed a state transition"),
    }
  }

  fn set_state(&mut self, new_state: AppStateImpl) {
    bug_assert!(
      self.app_state.is_none(),
      "attempted to set an `AppState` without calling `take_state` first {:?}",
      self.app_state
    );
    self.app_state = Some(new_state)
  }

  fn replace_state(&mut self, new_state: AppStateImpl) -> AppStateImpl {
    match &mut self.app_state {
      Some(ref mut state) => mem::replace(state, new_state),
      None => bug!("`AppState` previously failed a state transition"),
    }
  }

  fn has_launched(&self) -> bool {
    !matches!(
      self.state(),
      &AppStateImpl::NotLaunched { .. } | &AppStateImpl::Launching { .. }
    )
  }

  fn will_launch_transition(&mut self, queued_event_handler: Box<dyn EventHandler>) {
    let (queued_windows, queued_events, queued_gpu_redraws) = match self.take_state() {
      AppStateImpl::NotLaunched {
        queued_windows,
        queued_events,
        queued_gpu_redraws,
      } => (queued_windows, queued_events, queued_gpu_redraws),
      s => bug!("unexpected state {:?}", s),
    };
    self.set_state(AppStateImpl::Launching {
      queued_windows,
      queued_events,
      queued_event_handler,
      queued_gpu_redraws,
    });
  }

  fn did_finish_launching_transition(&mut self) -> (Vec<id>, Vec<EventWrapper>) {
    let (windows, events, event_handler, queued_gpu_redraws) = match self.take_state() {
      AppStateImpl::Launching {
        queued_windows,
        queued_events,
        queued_event_handler,
        queued_gpu_redraws,
      } => (
        queued_windows,
        queued_events,
        queued_event_handler,
        queued_gpu_redraws,
      ),
      s => bug!("unexpected state {:?}", s),
    };
    self.set_state(AppStateImpl::ProcessingEvents {
      event_handler,
      active_control_flow: ControlFlow::Poll,
      queued_gpu_redraws,
    });
    (windows, events)
  }

  fn wakeup_transition(&mut self) -> Option<EventWrapper> {
    // before `AppState::did_finish_launching` is called, pretend there is no running
    // event loop.
    if !self.has_launched() {
      return None;
    }

    let (event_handler, event) = match (self.control_flow, self.take_state()) {
      (
        ControlFlow::Poll,
        AppStateImpl::PollFinished {
          waiting_event_handler,
        },
      ) => (
        waiting_event_handler,
        EventWrapper::StaticEvent(Event::NewEvents(StartCause::Poll)),
      ),
      (
        ControlFlow::Wait,
        AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        },
      ) => (
        waiting_event_handler,
        EventWrapper::StaticEvent(Event::NewEvents(StartCause::WaitCancelled {
          start,
          requested_resume: None,
        })),
      ),
      (
        ControlFlow::WaitUntil(requested_resume),
        AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        },
      ) => {
        let event = if Instant::now() >= requested_resume {
          EventWrapper::StaticEvent(Event::NewEvents(StartCause::ResumeTimeReached {
            start,
            requested_resume,
          }))
        } else {
          EventWrapper::StaticEvent(Event::NewEvents(StartCause::WaitCancelled {
            start,
            requested_resume: Some(requested_resume),
          }))
        };
        (waiting_event_handler, event)
      }
      (ControlFlow::ExitWithCode(_), _) => bug!("unexpected `ControlFlow` `Exit`"),
      s => bug!("`EventHandler` unexpectedly woke up {:?}", s),
    };

    self.set_state(AppStateImpl::ProcessingEvents {
      event_handler,
      queued_gpu_redraws: Default::default(),
      active_control_flow: self.control_flow,
    });
    Some(event)
  }

  fn try_user_callback_transition(&mut self) -> UserCallbackTransitionResult<'_> {
    // If we're not able to process an event due to recursion or `Init` not having been sent out
    // yet, then queue the events up.
    match self.state_mut() {
      &mut AppStateImpl::Launching {
        ref mut queued_events,
        ..
      }
      | &mut AppStateImpl::NotLaunched {
        ref mut queued_events,
        ..
      }
      | &mut AppStateImpl::InUserCallback {
        ref mut queued_events,
        ..
      } => {
        // A lifetime cast: early returns are not currently handled well with NLL, but
        // polonius handles them well. This transmute is a safe workaround.
        return unsafe {
          mem::transmute::<UserCallbackTransitionResult<'_>, UserCallbackTransitionResult<'_>>(
            UserCallbackTransitionResult::ReentrancyPrevented { queued_events },
          )
        };
      }

      &mut AppStateImpl::ProcessingEvents { .. } | &mut AppStateImpl::ProcessingRedraws { .. } => {}

      s @ &mut AppStateImpl::PollFinished { .. }
      | s @ &mut AppStateImpl::Waiting { .. }
      | s @ &mut AppStateImpl::Terminated => {
        bug!("unexpected attempted to process an event {:?}", s)
      }
    }

    let (event_handler, queued_gpu_redraws, active_control_flow, processing_redraws) =
      match self.take_state() {
        AppStateImpl::Launching { .. }
        | AppStateImpl::NotLaunched { .. }
        | AppStateImpl::InUserCallback { .. } => unreachable!(),
        AppStateImpl::ProcessingEvents {
          event_handler,
          queued_gpu_redraws,
          active_control_flow,
        } => (
          event_handler,
          queued_gpu_redraws,
          active_control_flow,
          false,
        ),
        AppStateImpl::ProcessingRedraws {
          event_handler,
          active_control_flow,
        } => (event_handler, Default::default(), active_control_flow, true),
        AppStateImpl::PollFinished { .. }
        | AppStateImpl::Waiting { .. }
        | AppStateImpl::Terminated => unreachable!(),
      };
    self.set_state(AppStateImpl::InUserCallback {
      queued_events: Vec::new(),
      queued_gpu_redraws,
    });
    UserCallbackTransitionResult::Success {
      event_handler,
      active_control_flow,
      processing_redraws,
    }
  }

  fn main_events_cleared_transition(&mut self) -> HashSet<id> {
    let (event_handler, queued_gpu_redraws, active_control_flow) = match self.take_state() {
      AppStateImpl::ProcessingEvents {
        event_handler,
        queued_gpu_redraws,
        active_control_flow,
      } => (event_handler, queued_gpu_redraws, active_control_flow),
      s => bug!("unexpected state {:?}", s),
    };
    self.set_state(AppStateImpl::ProcessingRedraws {
      event_handler,
      active_control_flow,
    });
    queued_gpu_redraws
  }

  fn events_cleared_transition(&mut self) {
    if !self.has_launched() {
      return;
    }
    let (waiting_event_handler, old) = match self.take_state() {
      AppStateImpl::ProcessingRedraws {
        event_handler,
        active_control_flow,
      } => (event_handler, active_control_flow),
      s => bug!("unexpected state {:?}", s),
    };

    let new = self.control_flow;
    match (old, new) {
      (ControlFlow::Poll, ControlFlow::Poll) => self.set_state(AppStateImpl::PollFinished {
        waiting_event_handler,
      }),
      (ControlFlow::Wait, ControlFlow::Wait) => {
        let start = Instant::now();
        self.set_state(AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        });
      }
      (ControlFlow::WaitUntil(old_instant), ControlFlow::WaitUntil(new_instant))
        if old_instant == new_instant =>
      {
        let start = Instant::now();
        self.set_state(AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        });
      }
      (_, ControlFlow::Wait) => {
        let start = Instant::now();
        self.set_state(AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        });
        self.waker.stop()
      }
      (_, ControlFlow::WaitUntil(new_instant)) => {
        let start = Instant::now();
        self.set_state(AppStateImpl::Waiting {
          waiting_event_handler,
          start,
        });
        self.waker.start_at(new_instant)
      }
      (_, ControlFlow::Poll) => {
        self.set_state(AppStateImpl::PollFinished {
          waiting_event_handler,
        });
        self.waker.start()
      }
      (_, ControlFlow::ExitWithCode(_)) => {
        // https://developer.apple.com/library/archive/qa/qa1561/_index.html
        // it is not possible to quit an iOS app gracefully and programatically
        warn!("`ControlFlow::Exit` ignored on iOS");
        self.control_flow = old
      }
    }
  }

  fn terminated_transition(&mut self) -> Box<dyn EventHandler> {
    match self.replace_state(AppStateImpl::Terminated) {
      AppStateImpl::ProcessingEvents { event_handler, .. } => event_handler,
      s => bug!(
        "`LoopDestroyed` happened while not processing events {:?}",
        s
      ),
    }
  }
}

// requires main thread and window is a UIWindow
// retains window
pub unsafe fn set_key_window(window: id) {
  bug_assert!(
    msg_send![window, isKindOfClass: class!(UIWindow)],
    "set_key_window called with an incorrect type"
  );
  let mut this = AppState::get_mut();
  match this.state_mut() {
    &mut AppStateImpl::NotLaunched {
      ref mut queued_windows,
      ..
    } => return queued_windows.push(msg_send![window, retain]),
    &mut AppStateImpl::ProcessingEvents { .. }
    | &mut AppStateImpl::InUserCallback { .. }
    | &mut AppStateImpl::ProcessingRedraws { .. } => {}
    s @ &mut AppStateImpl::Launching { .. }
    | s @ &mut AppStateImpl::Waiting { .. }
    | s @ &mut AppStateImpl::PollFinished { .. } => bug!("unexpected state {:?}", s),
    &mut AppStateImpl::Terminated => {
      panic!("Attempt to create a `Window` after the app has terminated")
    }
  }
  drop(this);
  msg_send![window, makeKeyAndVisible]
}

// requires main thread and window is a UIWindow
// retains window
pub unsafe fn queue_gl_or_metal_redraw(window: id) {
  bug_assert!(
    msg_send![window, isKindOfClass: class!(UIWindow)],
    "set_key_window called with an incorrect type"
  );
  let mut this = AppState::get_mut();
  match this.state_mut() {
    &mut AppStateImpl::NotLaunched {
      ref mut queued_gpu_redraws,
      ..
    }
    | &mut AppStateImpl::Launching {
      ref mut queued_gpu_redraws,
      ..
    }
    | &mut AppStateImpl::ProcessingEvents {
      ref mut queued_gpu_redraws,
      ..
    }
    | &mut AppStateImpl::InUserCallback {
      ref mut queued_gpu_redraws,
      ..
    } => drop(queued_gpu_redraws.insert(window)),
    s @ &mut AppStateImpl::ProcessingRedraws { .. }
    | s @ &mut AppStateImpl::Waiting { .. }
    | s @ &mut AppStateImpl::PollFinished { .. } => bug!("unexpected state {:?}", s),
    &mut AppStateImpl::Terminated => {
      panic!("Attempt to create a `Window` after the app has terminated")
    }
  }
  drop(this);
}

// requires main thread
pub unsafe fn will_launch(queued_event_handler: Box<dyn EventHandler>) {
  AppState::get_mut().will_launch_transition(queued_event_handler)
}

// requires main thread
pub unsafe fn did_finish_launching() {
  let mut this = AppState::get_mut();
  let windows = match this.state_mut() {
    AppStateImpl::Launching { queued_windows, .. } => mem::take(queued_windows),
    s => bug!("unexpected state {:?}", s),
  };

  // start waking up the event loop now!
  bug_assert!(
    this.control_flow == ControlFlow::Poll,
    "unexpectedly not setup to `Poll` on launch!"
  );
  this.waker.start();

  // have to drop RefMut because the window setup code below can trigger new events
  drop(this);

  for window in windows {
    let count: NSUInteger = msg_send![window, retainCount];
    // make sure the window is still referenced
    if count > 1 {
      // Do a little screen dance here to account for windows being created before
      // `UIApplicationMain` is called. This fixes visual issues such as being
      // offcenter and sized incorrectly. Additionally, to fix orientation issues, we
      // gotta reset the `rootViewController`.
      //
      // relevant iOS log:
      // ```
      // [ApplicationLifecycle] Windows were created before application initialzation
      // completed. This may result in incorrect visual appearance.
      // ```
      let screen: id = msg_send![window, screen];
      let _: id = msg_send![screen, retain];
      let () = msg_send![window, setScreen:0 as id];
      let () = msg_send![window, setScreen: screen];
      let () = msg_send![screen, release];
      let controller: id = msg_send![window, rootViewController];
      let () = msg_send![window, setRootViewController: ptr::null::<AnyObject>()];
      let () = msg_send![window, setRootViewController: controller];
      let () = msg_send![window, makeKeyAndVisible];
    }
    let () = msg_send![window, release];
  }

  let (windows, events) = AppState::get_mut().did_finish_launching_transition();

  let events = std::iter::once(EventWrapper::StaticEvent(Event::NewEvents(
    StartCause::Init,
  )))
  .chain(events);
  handle_nonuser_events(events);

  // the above window dance hack, could possibly trigger new windows to be created.
  // we can just set those windows up normally, as they were created after didFinishLaunching
  for window in windows {
    let count: NSUInteger = msg_send![window, retainCount];
    // make sure the window is still referenced
    if count > 1 {
      let () = msg_send![window, makeKeyAndVisible];
    }
    let () = msg_send![window, release];
  }
}

// requires main thread
// AppState::did_finish_launching handles the special transition `Init`
pub unsafe fn handle_wakeup_transition() {
  let mut this = AppState::get_mut();
  let wakeup_event = match this.wakeup_transition() {
    None => return,
    Some(wakeup_event) => wakeup_event,
  };
  drop(this);

  handle_nonuser_event(wakeup_event)
}

// requires main thread
pub unsafe fn handle_nonuser_event(event: EventWrapper) {
  handle_nonuser_events(std::iter::once(event))
}

// requires main thread
pub unsafe fn handle_nonuser_events<I: IntoIterator<Item = EventWrapper>>(events: I) {
  let mut this = AppState::get_mut();
  let (mut event_handler, active_control_flow, processing_redraws) =
    match this.try_user_callback_transition() {
      UserCallbackTransitionResult::ReentrancyPrevented { queued_events } => {
        queued_events.extend(events);
        return;
      }
      UserCallbackTransitionResult::Success {
        event_handler,
        active_control_flow,
        processing_redraws,
      } => (event_handler, active_control_flow, processing_redraws),
    };
  let mut control_flow = this.control_flow;
  drop(this);

  for wrapper in events {
    match wrapper {
      EventWrapper::StaticEvent(event) => {
        if !processing_redraws && event.is_redraw() {
          log::info!("processing `RedrawRequested` during the main event loop");
        } else if processing_redraws && !event.is_redraw() {
          log::warn!(
            "processing non `RedrawRequested` event after the main event loop: {:#?}",
            event
          );
        }
        event_handler.handle_nonuser_event(event, &mut control_flow)
      }
      EventWrapper::EventProxy(proxy) => {
        handle_event_proxy(&mut event_handler, control_flow, proxy)
      }
    }
  }

  loop {
    let mut this = AppState::get_mut();
    let queued_events = match this.state_mut() {
      &mut AppStateImpl::InUserCallback {
        ref mut queued_events,
        queued_gpu_redraws: _,
      } => mem::take(queued_events),
      s => bug!("unexpected state {:?}", s),
    };
    if queued_events.is_empty() {
      let queued_gpu_redraws = match this.take_state() {
        AppStateImpl::InUserCallback {
          queued_events: _,
          queued_gpu_redraws,
        } => queued_gpu_redraws,
        _ => unreachable!(),
      };
      this.app_state = Some(if processing_redraws {
        bug_assert!(
          queued_gpu_redraws.is_empty(),
          "redraw queued while processing redraws"
        );
        AppStateImpl::ProcessingRedraws {
          event_handler,
          active_control_flow,
        }
      } else {
        AppStateImpl::ProcessingEvents {
          event_handler,
          queued_gpu_redraws,
          active_control_flow,
        }
      });
      this.control_flow = control_flow;
      break;
    }
    drop(this);

    for wrapper in queued_events {
      match wrapper {
        EventWrapper::StaticEvent(event) => {
          if !processing_redraws && event.is_redraw() {
            log::info!("processing `RedrawRequested` during the main event loop");
          } else if processing_redraws && !event.is_redraw() {
            log::warn!(
              "processing non-`RedrawRequested` event after the main event loop: {:#?}",
              event
            );
          }
          event_handler.handle_nonuser_event(event, &mut control_flow)
        }
        EventWrapper::EventProxy(proxy) => {
          handle_event_proxy(&mut event_handler, control_flow, proxy)
        }
      }
    }
  }
}

// requires main thread
unsafe fn handle_user_events() {
  let mut this = AppState::get_mut();
  let mut control_flow = this.control_flow;
  let (mut event_handler, active_control_flow, processing_redraws) =
    match this.try_user_callback_transition() {
      UserCallbackTransitionResult::ReentrancyPrevented { .. } => {
        bug!("unexpected attempted to process an event")
      }
      UserCallbackTransitionResult::Success {
        event_handler,
        active_control_flow,
        processing_redraws,
      } => (event_handler, active_control_flow, processing_redraws),
    };
  if processing_redraws {
    bug!("user events attempted to be sent out while `ProcessingRedraws`");
  }
  drop(this);

  event_handler.handle_user_events(&mut control_flow);

  loop {
    let mut this = AppState::get_mut();
    let queued_events = match this.state_mut() {
      &mut AppStateImpl::InUserCallback {
        ref mut queued_events,
        queued_gpu_redraws: _,
      } => mem::take(queued_events),
      s => bug!("unexpected state {:?}", s),
    };
    if queued_events.is_empty() {
      let queued_gpu_redraws = match this.take_state() {
        AppStateImpl::InUserCallback {
          queued_events: _,
          queued_gpu_redraws,
        } => queued_gpu_redraws,
        _ => unreachable!(),
      };
      this.app_state = Some(AppStateImpl::ProcessingEvents {
        event_handler,
        queued_gpu_redraws,
        active_control_flow,
      });
      this.control_flow = control_flow;
      break;
    }
    drop(this);

    for wrapper in queued_events {
      match wrapper {
        EventWrapper::StaticEvent(event) => {
          event_handler.handle_nonuser_event(event, &mut control_flow)
        }
        EventWrapper::EventProxy(proxy) => {
          handle_event_proxy(&mut event_handler, control_flow, proxy)
        }
      }
    }
    event_handler.handle_user_events(&mut control_flow);
  }
}

// requires main thread
pub unsafe fn handle_main_events_cleared() {
  let mut this = AppState::get_mut();
  if !this.has_launched() {
    return;
  }
  match this.state_mut() {
    &mut AppStateImpl::ProcessingEvents { .. } => {}
    _ => bug!("`ProcessingRedraws` happened unexpectedly"),
  };
  drop(this);

  // User events are always sent out at the end of the "MainEventLoop"
  handle_user_events();
  handle_nonuser_event(EventWrapper::StaticEvent(Event::MainEventsCleared));

  let mut this = AppState::get_mut();
  let mut redraw_events: Vec<EventWrapper> = this
    .main_events_cleared_transition()
    .into_iter()
    .map(|window| EventWrapper::StaticEvent(Event::RedrawRequested(RootWindowId(window.into()))))
    .collect();

  if !redraw_events.is_empty() {
    redraw_events.push(EventWrapper::StaticEvent(Event::RedrawEventsCleared));
  }
  drop(this);

  handle_nonuser_events(redraw_events);
}

// requires main thread
pub unsafe fn handle_events_cleared() {
  AppState::get_mut().events_cleared_transition();
}

// requires main thread
pub unsafe fn terminated() {
  let mut this = AppState::get_mut();
  let mut event_handler = this.terminated_transition();
  let mut control_flow = this.control_flow;
  drop(this);

  event_handler.handle_nonuser_event(Event::LoopDestroyed, &mut control_flow)
}

fn handle_event_proxy(
  event_handler: &mut Box<dyn EventHandler>,
  control_flow: ControlFlow,
  proxy: EventProxy,
) {
  match proxy {
    EventProxy::DpiChangedProxy {
      suggested_size,
      scale_factor,
      window_id,
    } => handle_hidpi_proxy(
      event_handler,
      control_flow,
      suggested_size,
      scale_factor,
      window_id,
    ),
  }
}

fn handle_hidpi_proxy(
  event_handler: &mut Box<dyn EventHandler>,
  mut control_flow: ControlFlow,
  suggested_size: LogicalSize<f64>,
  scale_factor: f64,
  window_id: id,
) {
  let mut size = suggested_size.to_physical(scale_factor);
  let new_inner_size = &mut size;
  let event = Event::WindowEvent {
    window_id: RootWindowId(window_id.into()),
    event: WindowEvent::ScaleFactorChanged {
      scale_factor,
      new_inner_size,
    },
  };
  event_handler.handle_nonuser_event(event, &mut control_flow);
  let (view, screen_frame) = get_view_and_screen_frame(window_id);
  let physical_size = *new_inner_size;
  let logical_size = physical_size.to_logical(scale_factor);
  let size = CGSize::new(logical_size);
  let new_frame: CGRect = CGRect::new(screen_frame.origin, size);
  unsafe {
    let () = msg_send![view, setFrame: new_frame];
  }
}

fn get_view_and_screen_frame(window_id: id) -> (id, CGRect) {
  unsafe {
    let view_controller: id = msg_send![window_id, rootViewController];
    let view: id = msg_send![view_controller, view];
    let bounds: CGRect = msg_send![window_id, bounds];
    let screen: id = msg_send![window_id, screen];
    let screen_space: id = msg_send![screen, coordinateSpace];
    let screen_frame: CGRect =
      msg_send![window_id, convertRect:bounds, toCoordinateSpace:screen_space];
    (view, screen_frame)
  }
}

struct EventLoopWaker {
  timer: CFRunLoopTimerRef,
}

impl Drop for EventLoopWaker {
  fn drop(&mut self) {
    unsafe {
      CFRunLoopTimerInvalidate(self.timer);
      CFRelease(self.timer as _);
    }
  }
}

impl EventLoopWaker {
  fn new(rl: CFRunLoopRef) -> EventLoopWaker {
    extern "C" fn wakeup_main_loop(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}
    unsafe {
      // Create a timer with a 0.1µs interval (1ns does not work) to mimic polling.
      // It is initially setup with a first fire time really far into the
      // future, but that gets changed to fire immediately in did_finish_launching
      let timer = CFRunLoopTimerCreate(
        ptr::null_mut(),
        f64::MAX,
        0.000_000_1,
        0,
        0,
        wakeup_main_loop,
        ptr::null_mut(),
      );
      CFRunLoopAddTimer(rl, timer, kCFRunLoopCommonModes);

      EventLoopWaker { timer }
    }
  }

  fn stop(&mut self) {
    unsafe { CFRunLoopTimerSetNextFireDate(self.timer, f64::MAX) }
  }

  fn start(&mut self) {
    unsafe { CFRunLoopTimerSetNextFireDate(self.timer, f64::MIN) }
  }

  fn start_at(&mut self, instant: Instant) {
    let now = Instant::now();
    if now >= instant {
      self.start();
    } else {
      unsafe {
        let current = CFAbsoluteTimeGetCurrent();
        let duration = instant - now;
        let fsecs = duration.subsec_nanos() as f64 / 1_000_000_000.0 + duration.as_secs() as f64;
        CFRunLoopTimerSetNextFireDate(self.timer, current + fsecs)
      }
    }
  }
}

macro_rules! os_capabilities {
    (
        $(
            $(#[$attr:meta])*
            $error_name:ident: $objc_call:literal,
            $name:ident: $major:literal-$minor:literal
        ),*
        $(,)*
    ) => {
        #[derive(Clone, Debug)]
        pub struct OSCapabilities {
            $(
                pub $name: bool,
            )*

            os_version: NSOperatingSystemVersion,
        }

        impl From<NSOperatingSystemVersion> for OSCapabilities {
            fn from(os_version: NSOperatingSystemVersion) -> OSCapabilities {
                $(let $name = os_version.meets_requirements($major, $minor);)*
                OSCapabilities { $($name,)* os_version, }
            }
        }

        impl OSCapabilities {$(
            $(#[$attr])*
            pub fn $error_name(&self, extra_msg: &str) {
                log::warn!(
                    concat!("`", $objc_call, "` requires iOS {}.{}+. This device is running iOS {}.{}.{}. {}"),
                    $major, $minor, self.os_version.major, self.os_version.minor, self.os_version.patch,
                    extra_msg
                )
            }
        )*}
    };
}

os_capabilities! {
    /// https://developer.apple.com/documentation/uikit/uiview/2891103-safeareainsets?language=objc
    #[allow(unused)] // error message unused
    safe_area_err_msg: "-[UIView safeAreaInsets]",
    safe_area: 11-0,
    /// https://developer.apple.com/documentation/uikit/uiviewcontroller/2887509-setneedsupdateofhomeindicatoraut?language=objc
    home_indicator_hidden_err_msg: "-[UIViewController setNeedsUpdateOfHomeIndicatorAutoHidden]",
    home_indicator_hidden: 11-0,
    /// https://developer.apple.com/documentation/uikit/uiviewcontroller/2887507-setneedsupdateofscreenedgesdefer?language=objc
    defer_system_gestures_err_msg: "-[UIViewController setNeedsUpdateOfScreenEdgesDeferringSystem]",
    defer_system_gestures: 11-0,
    /// https://developer.apple.com/documentation/uikit/uiscreen/2806814-maximumframespersecond?language=objc
    maximum_frames_per_second_err_msg: "-[UIScreen maximumFramesPerSecond]",
    maximum_frames_per_second: 10-3,
    /// https://developer.apple.com/documentation/uikit/uitouch/1618110-force?language=objc
    #[allow(unused)] // error message unused
    force_touch_err_msg: "-[UITouch force]",
    force_touch: 9-0,
}

impl NSOperatingSystemVersion {
  fn meets_requirements(&self, required_major: NSInteger, required_minor: NSInteger) -> bool {
    (self.major, self.minor) >= (required_major, required_minor)
  }
}

pub fn os_capabilities() -> OSCapabilities {
  static OS_CAPABILITIES: Lazy<OSCapabilities> = Lazy::new(|| {
    let version: NSOperatingSystemVersion = unsafe {
      let process_info: id = msg_send![class!(NSProcessInfo), processInfo];
      let atleast_ios_8: bool = msg_send![
          process_info,
          respondsToSelector: sel!(operatingSystemVersion)
      ];
      // tao requires atleast iOS 8 because no one has put the time into supporting earlier os versions.
      // Older iOS versions are increasingly difficult to test. For example, Xcode 11 does not support
      // debugging on devices with an iOS version of less than 8. Another example, in order to use an iOS
      // simulator older than iOS 8, you must download an older version of Xcode (<9), and at least Xcode 7
      // has been tested to not even run on macOS 10.15 - Xcode 8 might?
      //
      // The minimum required iOS version is likely to grow in the future.
      assert!(atleast_ios_8, "`tao` requires iOS version 8 or greater");
      msg_send![process_info, operatingSystemVersion]
    };
    version.into()
  });
  OS_CAPABILITIES.clone()
}
