// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

pub use jni::{
  self,
  objects::{GlobalRef, JClass, JMap, JObject, JString},
  sys::jobject,
  JNIEnv,
};
use log::Level;
pub use ndk;
use ndk::{
  input_queue::InputQueue,
  looper::{FdEvent, ForeignLooper, ThreadLooper},
};
use once_cell::sync::{Lazy, OnceCell};
use std::{
  cell::RefCell,
  ffi::{CStr, CString},
  fs::File,
  io::{BufRead, BufReader},
  os::unix::prelude::*,
  sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard},
  thread,
};

/// Android pacakge name that could be used to reference classes
/// in the android project.
pub static PACKAGE: OnceCell<&str> = OnceCell::new();

/// Generate JNI compilant functions that are necessary for
/// building android apps with tao.
///
/// Arguments in order:
/// 1. android app domain name in reverse snake_case as an ident (for ex: com_example)
/// 2. android package anme (for ex: wryapp)
/// 3. the android activity that has external linking for the following functions and calls them:
///       - `private external fun create(activity: WryActivity)``
///       - `private external fun start()`
///       - `private external fun resume()`
///       - `private external fun pause()`
///       - `private external fun stop()`
///       - `private external fun save()`
///       - `private external fun destroy()`
///       - `private external fun memory()`
///       - `private external fun focus(focus: Boolean)`
/// 4. a one time setup function that will be ran once after tao has created its event loop in the `create` function above.
/// 5. the main entry point of your android application.
#[rustfmt::skip]
#[macro_export]
macro_rules! android_binding {
  ($domain:ident, $package:ident, $activity:ident, $setup:path, $main:ident) => {
    ::tao::android_binding!($domain, $package, $activity, $setup, $main, ::tao)
  };
  ($domain:ident, $package:ident, $activity:ident, $setup:path, $main:ident, $tao:path) => {{
    // NOTE: be careful when changing how this use statement is written
    use $tao::{platform::android::prelude::android_fn, platform::android::prelude::*};
    fn _____tao_store_package_name__() {
      PACKAGE.get_or_init(move || generate_package_name!($domain, $package));
    }

    android_fn!(
      $domain,
      $package,
      $activity,
      create,
      [JObject],
      __VOID__,
      [$setup, $main],
      _____tao_store_package_name__,
    );
    android_fn!($domain, $package, $activity, start, [JObject]);
    android_fn!($domain, $package, $activity, stop, [JObject]);
    android_fn!($domain, $package, $activity, resume, [JObject]);
    android_fn!($domain, $package, $activity, pause, [JObject]);
    android_fn!($domain, $package, $activity, save, [JObject]);
    android_fn!($domain, $package, $activity, destroy, [JObject]);
    android_fn!($domain, $package, $activity, memory, [JObject]);
    android_fn!($domain, $package, $activity, focus, [i32]);
  }};
}

/// `ndk-glue` macros register the reading end of an event pipe with the
/// main [`ThreadLooper`] under this `ident`.
/// When returned from [`ThreadLooper::poll_*`](ThreadLooper::poll_once)
/// an event can be retrieved from [`poll_events()`].
pub const NDK_GLUE_LOOPER_EVENT_PIPE_IDENT: i32 = 0;

/// The [`InputQueue`] received from Android is registered with the main
/// [`ThreadLooper`] under this `ident`.
/// When returned from [`ThreadLooper::poll_*`](ThreadLooper::poll_once)
/// an event can be retrieved from [`input_queue()`].
pub const NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT: i32 = 1;

pub fn android_log(level: Level, tag: &CStr, msg: &CStr) {
  let prio = match level {
    Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
    Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
    Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
    Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
    Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
  };
  unsafe {
    ndk_sys::__android_log_write(prio.0 as _, tag.as_ptr(), msg.as_ptr());
  }
}

pub(crate) struct StaticCell<T>(RefCell<T>);

unsafe impl<T> Send for StaticCell<T> {}
unsafe impl<T> Sync for StaticCell<T> {}

impl<T> std::ops::Deref for StaticCell<T> {
  type Target = RefCell<T>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

static WINDOW_MANAGER: StaticCell<Option<GlobalRef>> = StaticCell(RefCell::new(None));
static INPUT_QUEUE: Lazy<RwLock<Option<InputQueue>>> = Lazy::new(|| Default::default());
static CONTENT_RECT: Lazy<RwLock<Rect>> = Lazy::new(|| Default::default());
static LOOPER: Lazy<Mutex<Option<ForeignLooper>>> = Lazy::new(|| Default::default());

pub fn window_manager() -> std::cell::Ref<'static, Option<GlobalRef>> {
  WINDOW_MANAGER.0.borrow()
}

pub fn input_queue() -> RwLockReadGuard<'static, Option<InputQueue>> {
  INPUT_QUEUE.read().unwrap()
}

pub fn content_rect() -> Rect {
  CONTENT_RECT.read().unwrap().clone()
}

pub static PIPE: Lazy<[OwnedFd; 2]> = Lazy::new(|| {
  let mut pipe: [RawFd; 2] = Default::default();
  unsafe { libc::pipe(pipe.as_mut_ptr()) };
  pipe.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
});

pub fn poll_events() -> Option<Event> {
  unsafe {
    let size = std::mem::size_of::<Event>();
    let mut event = Event::Start;
    if libc::read(PIPE[0].as_raw_fd(), &mut event as *mut _ as *mut _, size)
      == size as libc::ssize_t
    {
      Some(event)
    } else {
      None
    }
  }
}

unsafe fn wake(event: Event) {
  log::trace!("{:?}", event);
  let size = std::mem::size_of::<Event>();
  let res = libc::write(PIPE[1].as_raw_fd(), &event as *const _ as *const _, size);
  assert_eq!(res, size as libc::ssize_t);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
  pub left: u32,
  pub top: u32,
  pub right: u32,
  pub bottom: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Event {
  Start,
  Resume,
  SaveInstanceState,
  Pause,
  Stop,
  Destroy,
  ConfigChanged,
  LowMemory,
  WindowLostFocus,
  WindowHasFocus,
  WindowCreated,
  WindowResized,
  WindowRedrawNeeded,
  WindowDestroyed,
  InputQueueCreated,
  InputQueueDestroyed,
  ContentRectChanged,
}

pub unsafe fn create(
  mut env: JNIEnv,
  _jclass: JClass,
  jobject: JObject,
  setup: unsafe fn(&str, JNIEnv, &ThreadLooper, GlobalRef),
  main: fn(),
) {
  //-> jobjectArray {
  // Initialize global context
  let window_manager = env
    .call_method(
      &jobject,
      "getWindowManager",
      "()Landroid/view/WindowManager;",
      &[],
    )
    .unwrap()
    .l()
    .unwrap();
  let window_manager = env.new_global_ref(window_manager).unwrap();
  WINDOW_MANAGER.replace(Some(window_manager));
  let activity = env.new_global_ref(jobject).unwrap();
  let vm = env.get_java_vm().unwrap();
  let env = vm.attach_current_thread_as_daemon().unwrap();
  ndk_context::initialize_android_context(
    vm.get_java_vm_pointer() as *mut _,
    activity.as_obj().as_raw() as *mut _,
  );

  let looper = ThreadLooper::for_thread().unwrap();
  setup(PACKAGE.get().unwrap(), env, &looper, activity);

  let logpipe = {
    let mut logpipe: [RawFd; 2] = Default::default();
    libc::pipe(logpipe.as_mut_ptr());
    libc::dup2(logpipe[1], libc::STDOUT_FILENO);
    libc::dup2(logpipe[1], libc::STDERR_FILENO);

    logpipe.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
  };
  thread::spawn(move || {
    let tag = CStr::from_bytes_with_nul(b"RustStdoutStderr\0").unwrap();
    let file = File::from_raw_fd(logpipe[0].as_raw_fd());
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    loop {
      buffer.clear();
      if let Ok(len) = reader.read_line(&mut buffer) {
        if len == 0 {
          break;
        } else if let Ok(msg) = CString::new(buffer.clone()) {
          android_log(Level::Info, tag, &msg);
        }
      }
    }
  });

  let looper_ready = Arc::new(Condvar::new());
  let signal_looper_ready = looper_ready.clone();

  thread::spawn(move || {
    let looper = ThreadLooper::prepare();
    let foreign = looper.into_foreign();
    foreign
      .add_fd(
        PIPE[0].as_fd(),
        NDK_GLUE_LOOPER_EVENT_PIPE_IDENT,
        FdEvent::INPUT,
        std::ptr::null_mut(),
      )
      .unwrap();

    {
      let mut locked_looper = LOOPER.lock().unwrap();
      *locked_looper = Some(foreign);
      signal_looper_ready.notify_one();
    }

    main()
  });

  // Don't return from this function (`ANativeActivity_onCreate`) until the thread
  // has created its `ThreadLooper` and assigned it to the static `LOOPER`
  // variable. It will be used from `on_input_queue_created` as soon as this
  // function returns.
  let locked_looper = LOOPER.lock().unwrap();
  let _mutex_guard = looper_ready
    .wait_while(locked_looper, |looper| looper.is_none())
    .unwrap();
}

pub unsafe fn resume(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::Resume);
}

pub unsafe fn pause(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::Pause);
}

pub unsafe fn focus(_: JNIEnv, _: JClass, has_focus: libc::c_int) {
  let event = if has_focus == 0 {
    Event::WindowLostFocus
  } else {
    Event::WindowHasFocus
  };
  wake(event);
}

pub unsafe fn start(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::Start);
}

pub unsafe fn stop(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::Stop);
}

///////////////////////////////////////////////
// Events below are not used by event loop yet.
///////////////////////////////////////////////

pub unsafe fn save(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::SaveInstanceState);
}

pub unsafe fn destroy(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::Destroy);
  ndk_context::release_android_context();
  WINDOW_MANAGER.take();
}

pub unsafe fn memory(_: JNIEnv, _: JClass, _: JObject) {
  wake(Event::LowMemory);
}

/*
unsafe extern "C" fn on_configuration_changed(activity: *mut ANativeActivity) {
  wake(activity, Event::ConfigChanged);
}

unsafe extern "C" fn on_window_resized(
  activity: *mut ANativeActivity,
  _window: *mut ANativeWindow,
) {
  wake(activity, Event::WindowResized);
}

unsafe extern "C" fn on_input_queue_created(
  activity: *mut ANativeActivity,
  queue: *mut AInputQueue,
) {
  let input_queue = InputQueue::from_ptr(NonNull::new(queue).unwrap());
  let locked_looper = LOOPER.lock().unwrap();
  // The looper should always be `Some` after `fn init()` returns, unless
  // future code cleans it up and sets it back to `None` again.
  let looper = locked_looper.as_ref().expect("Looper does not exist");
  input_queue.attach_looper(looper, NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT);
  *INPUT_QUEUE.write().unwrap() = Some(input_queue);
  wake(activity, Event::InputQueueCreated);
}

unsafe extern "C" fn on_input_queue_destroyed(
  activity: *mut ANativeActivity,
  queue: *mut AInputQueue,
) {
  wake(activity, Event::InputQueueDestroyed);
  let mut input_queue_guard = INPUT_QUEUE.write().unwrap();
  assert_eq!(input_queue_guard.as_ref().unwrap().ptr().as_ptr(), queue);
  let input_queue = InputQueue::from_ptr(NonNull::new(queue).unwrap());
  input_queue.detach_looper();
  *input_queue_guard = None;
}

unsafe extern "C" fn on_content_rect_changed(activity: *mut ANativeActivity, rect: *const ARect) {
  let rect = Rect {
    left: (*rect).left as _,
    top: (*rect).top as _,
    right: (*rect).right as _,
    bottom: (*rect).bottom as _,
  };
  *CONTENT_RECT.write().unwrap() = rect;
  wake(activity, Event::ContentRectChanged);
}
*/
