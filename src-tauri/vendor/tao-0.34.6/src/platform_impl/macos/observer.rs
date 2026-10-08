// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  self,
  os::raw::*,
  panic::{AssertUnwindSafe, UnwindSafe},
  ptr,
  rc::Weak,
  time::Instant,
};

use crate::platform_impl::platform::{
  app_state::AppState,
  event_loop::{stop_app_on_panic, PanicInfo},
  ffi,
};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
  pub static kCFRunLoopCommonModes: CFRunLoopMode;

  pub fn CFRunLoopGetMain() -> CFRunLoopRef;
  pub fn CFRunLoopWakeUp(rl: CFRunLoopRef);

  pub fn CFRunLoopObserverCreate(
    allocator: CFAllocatorRef,
    activities: CFOptionFlags,
    repeats: ffi::Boolean,
    order: CFIndex,
    callout: CFRunLoopObserverCallBack,
    context: *mut CFRunLoopObserverContext,
  ) -> CFRunLoopObserverRef;
  pub fn CFRunLoopAddObserver(
    rl: CFRunLoopRef,
    observer: CFRunLoopObserverRef,
    mode: CFRunLoopMode,
  );

  pub fn CFRunLoopTimerCreate(
    allocator: CFAllocatorRef,
    fireDate: CFAbsoluteTime,
    interval: CFTimeInterval,
    flags: CFOptionFlags,
    order: CFIndex,
    callout: CFRunLoopTimerCallBack,
    context: *mut CFRunLoopTimerContext,
  ) -> CFRunLoopTimerRef;
  pub fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFRunLoopMode);
  pub fn CFRunLoopTimerSetNextFireDate(timer: CFRunLoopTimerRef, fireDate: CFAbsoluteTime);
  pub fn CFRunLoopTimerInvalidate(time: CFRunLoopTimerRef);

  pub fn CFRunLoopSourceCreate(
    allocator: CFAllocatorRef,
    order: CFIndex,
    context: *mut CFRunLoopSourceContext,
  ) -> CFRunLoopSourceRef;
  pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
  #[allow(dead_code)]
  pub fn CFRunLoopSourceInvalidate(source: CFRunLoopSourceRef);
  pub fn CFRunLoopSourceSignal(source: CFRunLoopSourceRef);

  pub fn CFAbsoluteTimeGetCurrent() -> CFAbsoluteTime;
  pub fn CFRelease(cftype: *const c_void);
}

pub enum CFAllocator {}
pub type CFAllocatorRef = *mut CFAllocator;
pub enum CFRunLoop {}
pub type CFRunLoopRef = *mut CFRunLoop;
pub type CFRunLoopMode = CFStringRef;
pub enum CFRunLoopObserver {}
pub type CFRunLoopObserverRef = *mut CFRunLoopObserver;
pub enum CFRunLoopTimer {}
pub type CFRunLoopTimerRef = *mut CFRunLoopTimer;
pub enum CFRunLoopSource {}
pub type CFRunLoopSourceRef = *mut CFRunLoopSource;
pub enum CFString {}
pub type CFStringRef = *const CFString;

pub type CFHashCode = c_ulong;
pub type CFIndex = c_long;
pub type CFOptionFlags = c_ulong;
pub type CFRunLoopActivity = CFOptionFlags;

pub type CFAbsoluteTime = CFTimeInterval;
pub type CFTimeInterval = f64;

#[allow(non_upper_case_globals)]
pub const kCFRunLoopEntry: CFRunLoopActivity = 0;
#[allow(non_upper_case_globals)]
pub const kCFRunLoopBeforeWaiting: CFRunLoopActivity = 1 << 5;
#[allow(non_upper_case_globals)]
pub const kCFRunLoopAfterWaiting: CFRunLoopActivity = 1 << 6;
#[allow(non_upper_case_globals)]
pub const kCFRunLoopExit: CFRunLoopActivity = 1 << 7;

pub type CFRunLoopObserverCallBack =
  extern "C" fn(observer: CFRunLoopObserverRef, activity: CFRunLoopActivity, info: *mut c_void);
pub type CFRunLoopTimerCallBack = extern "C" fn(timer: CFRunLoopTimerRef, info: *mut c_void);

pub enum CFRunLoopTimerContext {}

/// This mirrors the struct with the same name from Core Foundation.
/// https://developer.apple.com/documentation/corefoundation/cfrunloopobservercontext?language=objc
#[allow(non_snake_case)]
#[repr(C)]
pub struct CFRunLoopObserverContext {
  pub version: CFIndex,
  pub info: *mut c_void,
  pub retain: Option<extern "C" fn(info: *const c_void) -> *const c_void>,
  pub release: Option<extern "C" fn(info: *const c_void)>,
  pub copyDescription: Option<extern "C" fn(info: *const c_void) -> CFStringRef>,
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct CFRunLoopSourceContext {
  pub version: CFIndex,
  pub info: *mut c_void,
  pub retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
  pub release: Option<extern "C" fn(*const c_void)>,
  pub copyDescription: Option<extern "C" fn(*const c_void) -> CFStringRef>,
  pub equal: Option<extern "C" fn(*const c_void, *const c_void) -> ffi::Boolean>,
  pub hash: Option<extern "C" fn(*const c_void) -> CFHashCode>,
  pub schedule: Option<extern "C" fn(*mut c_void, CFRunLoopRef, CFRunLoopMode)>,
  pub cancel: Option<extern "C" fn(*mut c_void, CFRunLoopRef, CFRunLoopMode)>,
  pub perform: Option<extern "C" fn(*mut c_void)>,
}

unsafe fn control_flow_handler<F>(panic_info: *mut c_void, f: F)
where
  F: FnOnce(Weak<PanicInfo>) + UnwindSafe,
{
  let info_from_raw = Weak::from_raw(panic_info as *mut PanicInfo);
  // Asserting unwind safety on this type should be fine because `PanicInfo` is
  // `RefUnwindSafe` and `Rc<T>` is `UnwindSafe` if `T` is `RefUnwindSafe`.
  let panic_info = AssertUnwindSafe(Weak::clone(&info_from_raw));
  // `from_raw` takes ownership of the data behind the pointer.
  // But if this scope takes ownership of the weak pointer, then
  // the weak pointer will get free'd at the end of the scope.
  // However we want to keep that weak reference around after the function.
  std::mem::forget(info_from_raw);

  stop_app_on_panic(Weak::clone(&panic_info), move || {
    let _ = &panic_info;
    f(panic_info.0)
  });
}

// begin is queued with the highest priority to ensure it is processed before other observers
extern "C" fn control_flow_begin_handler(
  _: CFRunLoopObserverRef,
  activity: CFRunLoopActivity,
  panic_info: *mut c_void,
) {
  unsafe {
    control_flow_handler(panic_info, |panic_info| {
      #[allow(non_upper_case_globals)]
      match activity {
        kCFRunLoopAfterWaiting => {
          //trace!("Triggered `CFRunLoopAfterWaiting`");
          AppState::wakeup(panic_info);
          //trace!("Completed `CFRunLoopAfterWaiting`");
        }
        kCFRunLoopEntry => unimplemented!(), // not expected to ever happen
        _ => unreachable!(),
      }
    });
  }
}

// end is queued with the lowest priority to ensure it is processed after other observers
// without that, LoopDestroyed would  get sent after MainEventsCleared
extern "C" fn control_flow_end_handler(
  _: CFRunLoopObserverRef,
  activity: CFRunLoopActivity,
  panic_info: *mut c_void,
) {
  unsafe {
    control_flow_handler(panic_info, |panic_info| {
      #[allow(non_upper_case_globals)]
      match activity {
        kCFRunLoopBeforeWaiting => {
          //trace!("Triggered `CFRunLoopBeforeWaiting`");
          AppState::cleared(panic_info);
          //trace!("Completed `CFRunLoopBeforeWaiting`");
        }
        kCFRunLoopExit => (), //unimplemented!(), // not expected to ever happen
        _ => unreachable!(),
      }
    });
  }
}

struct RunLoop(CFRunLoopRef);

impl RunLoop {
  unsafe fn get() -> Self {
    RunLoop(CFRunLoopGetMain())
  }

  unsafe fn add_observer(
    &self,
    flags: CFOptionFlags,
    priority: CFIndex,
    handler: CFRunLoopObserverCallBack,
    context: *mut CFRunLoopObserverContext,
  ) {
    let observer = CFRunLoopObserverCreate(
      ptr::null_mut(),
      flags,
      ffi::TRUE, // Indicates we want this to run repeatedly
      priority,  // The lower the value, the sooner this will run
      handler,
      context,
    );
    CFRunLoopAddObserver(self.0, observer, kCFRunLoopCommonModes);
  }
}

pub fn setup_control_flow_observers(panic_info: Weak<PanicInfo>) {
  unsafe {
    let mut context = CFRunLoopObserverContext {
      info: Weak::into_raw(panic_info) as *mut _,
      version: 0,
      retain: None,
      release: None,
      copyDescription: None,
    };
    let run_loop = RunLoop::get();
    run_loop.add_observer(
      kCFRunLoopEntry | kCFRunLoopAfterWaiting,
      CFIndex::MIN,
      control_flow_begin_handler,
      &mut context as *mut _,
    );
    run_loop.add_observer(
      kCFRunLoopExit | kCFRunLoopBeforeWaiting,
      CFIndex::MAX,
      control_flow_end_handler,
      &mut context as *mut _,
    );
  }
}

pub struct EventLoopWaker {
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

impl Default for EventLoopWaker {
  fn default() -> EventLoopWaker {
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
      CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
      EventLoopWaker { timer }
    }
  }
}

impl EventLoopWaker {
  pub fn stop(&mut self) {
    unsafe { CFRunLoopTimerSetNextFireDate(self.timer, f64::MAX) }
  }

  pub fn start(&mut self) {
    unsafe { CFRunLoopTimerSetNextFireDate(self.timer, f64::MIN) }
  }

  pub fn start_at(&mut self, instant: Instant) {
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
