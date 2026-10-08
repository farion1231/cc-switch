// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![allow(non_snake_case)]

mod runner;

use crossbeam_channel::{self as channel, Receiver, Sender};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::{
  cell::Cell,
  collections::VecDeque,
  ffi::c_void,
  marker::PhantomData,
  mem, panic,
  rc::Rc,
  sync::Arc,
  thread,
  time::{Duration, Instant},
};
use windows::{
  core::{s, BOOL, PCWSTR},
  Win32::{
    Foundation::{
      HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WAIT_TIMEOUT, WPARAM,
    },
    Graphics::Gdi::*,
    System::{
      LibraryLoader::GetModuleHandleW,
      Ole::{IDropTarget, RevokeDragDrop},
      Threading::{GetCurrentThreadId, INFINITE},
    },
    UI::{
      Controls::{self as win32c, HOVER_DEFAULT},
      Input::{KeyboardAndMouse::*, Pointer::*, Touch::*, *},
      Shell::{
        DefSubclassProc, RemoveWindowSubclass, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM,
        ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAR, APPBARDATA,
      },
      WindowsAndMessaging::{self as win32wm, *},
    },
  },
};

use crate::{
  dpi::{PhysicalPosition, PhysicalSize, PixelUnit},
  error::ExternalError,
  event::{DeviceEvent, Event, Force, RawKeyEvent, Touch, TouchPhase, WindowEvent},
  event_loop::{ControlFlow, DeviceEventFilter, EventLoopClosed, EventLoopWindowTarget as RootELW},
  keyboard::{KeyCode, ModifiersState},
  monitor::MonitorHandle as RootMonitorHandle,
  platform_impl::platform::{
    dark_mode::try_window_theme,
    dpi::{become_dpi_aware, dpi_to_scale_factor, enable_non_client_dpi_scaling},
    keyboard::is_msg_keyboard_related,
    keyboard_layout::LAYOUT_CACHE,
    minimal_ime::is_msg_ime_related,
    monitor::{self, MonitorHandle},
    raw_input, util,
    window::set_skip_taskbar,
    window_state::{CursorFlags, WindowFlags, WindowState},
    wrap_device_id, WindowId, DEVICE_ID,
  },
  window::{Fullscreen, Theme, WindowId as RootWindowId},
};
use runner::{EventLoopRunner, EventLoopRunnerShared};

use super::{dpi::hwnd_dpi, util::get_system_metrics_for_dpi};

// This is defined in `winuser.h` as a macro that expands to `UINT_MAX`
const WHEEL_PAGESCROLL: u32 = u32::MAX;
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfoa#:~:text=SPI_GETWHEELSCROLLLINES
const DEFAULT_SCROLL_LINES_PER_WHEEL_DELTA: isize = 3;
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfoa#:~:text=SPI_GETWHEELSCROLLCHARS
const DEFAULT_SCROLL_CHARACTERS_PER_WHEEL_DELTA: isize = 3;

type GetPointerFrameInfoHistory = unsafe extern "system" fn(
  pointerId: u32,
  entriesCount: *mut u32,
  pointerCount: *mut u32,
  pointerInfo: *mut POINTER_INFO,
) -> BOOL;

type SkipPointerFrameMessages = unsafe extern "system" fn(pointerId: u32) -> BOOL;

type GetPointerDeviceRects = unsafe extern "system" fn(
  device: HANDLE,
  pointerDeviceRect: *mut RECT,
  displayRect: *mut RECT,
) -> BOOL;

type GetPointerTouchInfo =
  unsafe extern "system" fn(pointerId: u32, touchInfo: *mut POINTER_TOUCH_INFO) -> BOOL;

type GetPointerPenInfo =
  unsafe extern "system" fn(pointId: u32, penInfo: *mut POINTER_PEN_INFO) -> BOOL;

static GET_POINTER_FRAME_INFO_HISTORY: Lazy<Option<GetPointerFrameInfoHistory>> =
  Lazy::new(|| get_function!("user32.dll", GetPointerFrameInfoHistory));
static SKIP_POINTER_FRAME_MESSAGES: Lazy<Option<SkipPointerFrameMessages>> =
  Lazy::new(|| get_function!("user32.dll", SkipPointerFrameMessages));
static GET_POINTER_DEVICE_RECTS: Lazy<Option<GetPointerDeviceRects>> =
  Lazy::new(|| get_function!("user32.dll", GetPointerDeviceRects));
static GET_POINTER_TOUCH_INFO: Lazy<Option<GetPointerTouchInfo>> =
  Lazy::new(|| get_function!("user32.dll", GetPointerTouchInfo));
static GET_POINTER_PEN_INFO: Lazy<Option<GetPointerPenInfo>> =
  Lazy::new(|| get_function!("user32.dll", GetPointerPenInfo));

pub(crate) struct SubclassInput<T: 'static> {
  pub window_state: Arc<Mutex<WindowState>>,
  pub event_loop_runner: EventLoopRunnerShared<T>,
  pub _file_drop_handler: Option<IDropTarget>,
  pub subclass_removed: Cell<bool>,
  pub recurse_depth: Cell<u32>,
  pub event_loop_preferred_theme: Arc<Mutex<Option<Theme>>>,
}

impl<T> SubclassInput<T> {
  unsafe fn send_event(&self, event: Event<'_, T>) {
    self.event_loop_runner.send_event(event);
  }
}

struct ThreadMsgTargetSubclassInput<T: 'static> {
  event_loop_runner: EventLoopRunnerShared<T>,
  user_event_receiver: Receiver<T>,
}

impl<T> ThreadMsgTargetSubclassInput<T> {
  unsafe fn send_event(&self, event: Event<'_, T>) {
    self.event_loop_runner.send_event(event);
  }
}

/// The result of a subclass procedure (the message handling callback)
pub(crate) enum ProcResult {
  DefSubclassProc, // <- this should be the default value
  DefWindowProc,
  Value(LRESULT),
}

pub struct EventLoop<T: 'static> {
  thread_msg_sender: Sender<T>,
  window_target: RootELW<T>,
  msg_hook: Option<Box<dyn FnMut(*const c_void) -> bool + 'static>>,
}

pub(crate) struct PlatformSpecificEventLoopAttributes {
  pub(crate) any_thread: bool,
  pub(crate) dpi_aware: bool,
  pub(crate) msg_hook: Option<Box<dyn FnMut(*const c_void) -> bool + 'static>>,
  pub(crate) preferred_theme: Option<Theme>,
}

impl Default for PlatformSpecificEventLoopAttributes {
  fn default() -> Self {
    Self {
      any_thread: false,
      dpi_aware: true,
      msg_hook: None,
      preferred_theme: None,
    }
  }
}

#[derive(Clone)]
pub struct EventLoopWindowTarget<T: 'static> {
  thread_id: u32,
  thread_msg_target: HWND,
  pub(crate) preferred_theme: Arc<Mutex<Option<Theme>>>,
  pub(crate) runner_shared: EventLoopRunnerShared<T>,
}

impl<T: 'static> EventLoop<T> {
  pub(crate) fn new(attributes: &mut PlatformSpecificEventLoopAttributes) -> EventLoop<T> {
    let thread_id = unsafe { GetCurrentThreadId() };

    if !attributes.any_thread && thread_id != main_thread_id() {
      panic!(
        "Initializing the event loop outside of the main thread is a significant \
             cross-platform compatibility hazard. If you absolutely need to create an \
             EventLoop on a different thread, you can use the \
             `EventLoopBuilderExtWindows::any_thread` function."
      );
    }

    if attributes.dpi_aware {
      become_dpi_aware();
    }

    let thread_msg_target = create_event_target_window();

    super::dark_mode::allow_dark_mode_for_app(true);

    let send_thread_msg_target = thread_msg_target.0 as isize;
    thread::spawn(move || wait_thread(thread_id, HWND(send_thread_msg_target as _)));
    let wait_thread_id = get_wait_thread_id();

    let runner_shared = Rc::new(EventLoopRunner::new(thread_msg_target, wait_thread_id));

    let thread_msg_sender = subclass_event_target_window(thread_msg_target, runner_shared.clone());
    raw_input::register_all_mice_and_keyboards_for_raw_input(thread_msg_target, Default::default());

    EventLoop {
      thread_msg_sender,
      window_target: RootELW {
        p: EventLoopWindowTarget {
          thread_id,
          thread_msg_target,
          runner_shared,
          preferred_theme: Arc::new(Mutex::new(attributes.preferred_theme)),
        },
        _marker: PhantomData,
      },
      msg_hook: attributes.msg_hook.take(),
    }
  }

  pub fn window_target(&self) -> &RootELW<T> {
    &self.window_target
  }

  pub fn run<F>(mut self, event_handler: F) -> !
  where
    F: 'static + FnMut(Event<'_, T>, &RootELW<T>, &mut ControlFlow),
  {
    let exit_code = self.run_return(event_handler);
    ::std::process::exit(exit_code);
  }

  pub fn run_return<F>(&mut self, mut event_handler: F) -> i32
  where
    F: FnMut(Event<'_, T>, &RootELW<T>, &mut ControlFlow),
  {
    let event_loop_windows_ref = &self.window_target;

    unsafe {
      self
        .window_target
        .p
        .runner_shared
        .set_event_handler(move |event, control_flow| {
          event_handler(event, event_loop_windows_ref, control_flow);
        });
    }

    let runner = &self.window_target.p.runner_shared;

    let exit_code = unsafe {
      let mut msg = MSG::default();

      runner.poll();
      'main: loop {
        if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
          break 'main 0;
        }

        let handled = if let Some(callback) = self.msg_hook.as_deref_mut() {
          callback(&mut msg as *mut _ as *mut _)
        } else {
          false
        };
        if !handled {
          let _ = TranslateMessage(&msg);
          DispatchMessageW(&msg);
        }

        if let Err(payload) = runner.take_panic_error() {
          runner.reset_runner();
          panic::resume_unwind(payload);
        }

        if let ControlFlow::ExitWithCode(code) = runner.control_flow() {
          if !runner.handling_events() {
            break 'main code;
          }
        }
      }
    };

    unsafe {
      runner.loop_destroyed();
    }
    runner.reset_runner();
    exit_code
  }

  pub fn create_proxy(&self) -> EventLoopProxy<T> {
    EventLoopProxy {
      target_window: self.window_target.p.thread_msg_target,
      event_send: self.thread_msg_sender.clone(),
    }
  }
}

impl<T> EventLoopWindowTarget<T> {
  #[inline(always)]
  pub(crate) fn create_thread_executor(&self) -> EventLoopThreadExecutor {
    EventLoopThreadExecutor {
      thread_id: self.thread_id,
      target_window: self.thread_msg_target,
    }
  }

  // TODO: Investigate opportunities for caching
  pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
    monitor::available_monitors()
  }

  pub fn primary_monitor(&self) -> Option<RootMonitorHandle> {
    let monitor = monitor::primary_monitor();
    Some(RootMonitorHandle { inner: monitor })
  }

  pub fn monitor_from_point(&self, x: f64, y: f64) -> Option<MonitorHandle> {
    monitor::from_point(x, y)
  }

  #[cfg(feature = "rwh_05")]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::Windows(rwh_05::WindowsDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::Windows(
      rwh_06::WindowsDisplayHandle::new(),
    ))
  }

  pub fn set_device_event_filter(&self, filter: DeviceEventFilter) {
    raw_input::register_all_mice_and_keyboards_for_raw_input(self.thread_msg_target, filter);
  }

  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    util::cursor_position().map_err(Into::into)
  }

  #[inline]
  pub fn set_theme(&self, theme: Option<Theme>) {
    *self.preferred_theme.lock() = theme;
    self.runner_shared.owned_windows(|window| {
      let _ = unsafe { SendMessageW(window, *CHANGE_THEME_MSG_ID, None, None) };
    });
  }
}

fn main_thread_id() -> u32 {
  static mut MAIN_THREAD_ID: u32 = 0;
  #[used]
  #[allow(non_upper_case_globals)]
  #[link_section = ".CRT$XCU"]
  static INIT_MAIN_THREAD_ID: unsafe extern "C" fn() = {
    unsafe extern "C" fn initer() {
      MAIN_THREAD_ID = GetCurrentThreadId();
    }
    initer
  };

  unsafe { MAIN_THREAD_ID }
}

fn get_wait_thread_id() -> u32 {
  unsafe {
    let mut msg = MSG::default();
    let result = GetMessageW(
      &mut msg,
      None,
      *SEND_WAIT_THREAD_ID_MSG_ID,
      *SEND_WAIT_THREAD_ID_MSG_ID,
    );
    assert_eq!(
      msg.message, *SEND_WAIT_THREAD_ID_MSG_ID,
      "this shouldn't be possible. please open an issue with Tauri. error code: {}",
      result.0
    );
    msg.lParam.0 as u32
  }
}

fn wait_thread(parent_thread_id: u32, msg_window_id: HWND) {
  unsafe {
    let mut msg: MSG;

    let cur_thread_id = GetCurrentThreadId();
    let _ = PostThreadMessageW(
      parent_thread_id,
      *SEND_WAIT_THREAD_ID_MSG_ID,
      WPARAM(0),
      LPARAM(cur_thread_id as _),
    );

    let mut wait_until_opt = None;
    'main: loop {
      // Zeroing out the message ensures that the `WaitUntilInstantBox` doesn't get
      // double-freed if `MsgWaitForMultipleObjectsEx` returns early and there aren't
      // additional messages to process.
      msg = MSG::default();

      if wait_until_opt.is_some() {
        if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
          let _ = TranslateMessage(&msg);
          DispatchMessageW(&msg);
        }
      } else if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
        break 'main;
      } else {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }

      if msg.message == *WAIT_UNTIL_MSG_ID {
        wait_until_opt = Some(*WaitUntilInstantBox::from_raw(msg.lParam.0 as *mut _));
      } else if msg.message == *CANCEL_WAIT_UNTIL_MSG_ID {
        wait_until_opt = None;
      }

      if let Some(wait_until) = wait_until_opt {
        let now = Instant::now();
        if now < wait_until {
          // MsgWaitForMultipleObjects tends to overshoot just a little bit. We subtract
          // 1 millisecond from the requested time and spinlock for the remainder to
          // compensate for that.
          let resume_reason = MsgWaitForMultipleObjectsEx(
            None,
            dur2timeout(wait_until - now).saturating_sub(1),
            QS_ALLEVENTS,
            MWMO_INPUTAVAILABLE,
          );
          if resume_reason == WAIT_TIMEOUT {
            let _ = PostMessageW(
              Some(msg_window_id),
              *PROCESS_NEW_EVENTS_MSG_ID,
              WPARAM(0),
              LPARAM(0),
            );
            wait_until_opt = None;
          }
        } else {
          let _ = PostMessageW(
            Some(msg_window_id),
            *PROCESS_NEW_EVENTS_MSG_ID,
            WPARAM(0),
            LPARAM(0),
          );
          wait_until_opt = None;
        }
      }
    }
  }
}

// Implementation taken from https://github.com/rust-lang/rust/blob/db5476571d9b27c862b95c1e64764b0ac8980e23/src/libstd/sys/windows/mod.rs
fn dur2timeout(dur: Duration) -> u32 {
  // Note that a duration is a (u64, u32) (seconds, nanoseconds) pair, and the
  // timeouts in windows APIs are typically u32 milliseconds. To translate, we
  // have two pieces to take care of:
  //
  // * Nanosecond precision is rounded up
  // * Greater than u32::MAX milliseconds (50 days) is rounded up to INFINITE
  //   (never time out).
  dur
    .as_secs()
    .checked_mul(1000)
    .and_then(|ms| ms.checked_add((dur.subsec_nanos() as u64) / 1_000_000))
    .and_then(|ms| {
      ms.checked_add(if dur.subsec_nanos() % 1_000_000 > 0 {
        1
      } else {
        0
      })
    })
    .map(|ms| {
      if ms > u32::MAX as u64 {
        INFINITE
      } else {
        ms as u32
      }
    })
    .unwrap_or(INFINITE)
}

impl<T> Drop for EventLoop<T> {
  fn drop(&mut self) {
    unsafe {
      let _ = DestroyWindow(self.window_target.p.thread_msg_target);
    }
  }
}

pub(crate) struct EventLoopThreadExecutor {
  thread_id: u32,
  target_window: HWND,
}

unsafe impl Send for EventLoopThreadExecutor {}
unsafe impl Sync for EventLoopThreadExecutor {}

impl EventLoopThreadExecutor {
  /// Check to see if we're in the parent event loop's thread.
  pub(super) fn in_event_loop_thread(&self) -> bool {
    let cur_thread_id = unsafe { GetCurrentThreadId() };
    self.thread_id == cur_thread_id
  }

  /// Executes a function in the event loop thread. If we're already in the event loop thread,
  /// we just call the function directly.
  ///
  /// The `Inserted` can be used to inject a `WindowState` for the callback to use. The state is
  /// removed automatically if the callback receives a `WM_CLOSE` message for the window.
  ///
  /// Note that if you are using this to change some property of a window and updating
  /// `WindowState` then you should call this within the lock of `WindowState`. Otherwise the
  /// events may be sent to the other thread in different order to the one in which you set
  /// `WindowState`, leaving them out of sync.
  ///
  /// Note that we use a FnMut instead of a FnOnce because we're too lazy to create an equivalent
  /// to the unstable FnBox.
  pub(super) fn execute_in_thread<F>(&self, mut function: F)
  where
    F: FnMut() + Send + 'static,
  {
    unsafe {
      if self.in_event_loop_thread() {
        function();
      } else {
        // We double-box because the first box is a fat pointer.
        let boxed = Box::new(function) as Box<dyn FnMut()>;
        let boxed2: ThreadExecFn = Box::new(boxed);

        let raw = Box::into_raw(boxed2);

        let res = PostMessageW(
          Some(self.target_window),
          *EXEC_MSG_ID,
          WPARAM(raw as _),
          LPARAM(0),
        );
        assert!(
          res.is_ok(),
          "PostMessage failed ; is the messages queue full?"
        );
      }
    }
  }
}

type ThreadExecFn = Box<Box<dyn FnMut()>>;

pub struct EventLoopProxy<T: 'static> {
  target_window: HWND,
  event_send: Sender<T>,
}
unsafe impl<T: Send + 'static> Send for EventLoopProxy<T> {}
unsafe impl<T: Send + 'static> Sync for EventLoopProxy<T> {}

impl<T: 'static> Clone for EventLoopProxy<T> {
  fn clone(&self) -> Self {
    Self {
      target_window: self.target_window,
      event_send: self.event_send.clone(),
    }
  }
}

impl<T: 'static> EventLoopProxy<T> {
  pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed<T>> {
    unsafe {
      if PostMessageW(
        Some(self.target_window),
        *USER_EVENT_MSG_ID,
        WPARAM(0),
        LPARAM(0),
      )
      .is_ok()
      {
        self.event_send.send(event).ok();
        Ok(())
      } else {
        Err(EventLoopClosed(event))
      }
    }
  }
}

type WaitUntilInstantBox = Box<Instant>;

/// Message sent by the `EventLoopProxy` when we want to wake up the thread.
/// WPARAM and LPARAM are unused.
static USER_EVENT_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::WakeupMsg")) });
/// Message sent when we want to execute a closure in the thread.
/// WPARAM contains a Box<Box<dyn FnMut()>> that must be retrieved with `Box::from_raw`,
/// and LPARAM is unused.
static EXEC_MSG_ID: Lazy<u32> = Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::ExecMsg")) });
static PROCESS_NEW_EVENTS_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::ProcessNewEvents")) });
/// lparam is the wait thread's message id.
static SEND_WAIT_THREAD_ID_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::SendWaitThreadId")) });
/// lparam points to a `Box<Instant>` signifying the time `PROCESS_NEW_EVENTS_MSG_ID` should
/// be sent.
static WAIT_UNTIL_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::WaitUntil")) });
static CANCEL_WAIT_UNTIL_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::CancelWaitUntil")) });
/// Message sent by a `Window` when it wants to be destroyed by the main thread.
/// WPARAM and LPARAM are unused.
pub static DESTROY_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::DestroyMsg")) });
/// WPARAM is a bool specifying the `WindowFlags::MARKER_RETAIN_STATE_ON_SIZE` flag. See the
/// documentation in the `window_state` module for more information.
pub static SET_RETAIN_STATE_ON_SIZE_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::SetRetainMaximized")) });
/// Message sent by event loop when event loop's prefered theme changed.
/// WPARAM and LPARAM are unused.
pub static CHANGE_THEME_MSG_ID: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("Tao::ChangeTheme")) });
/// When the taskbar is created, it registers a message with the "TaskbarCreated" string and then broadcasts this message to all top-level windows
/// When the application receives this message, it should assume that any taskbar icons it added have been removed and add them again.
pub static S_U_TASKBAR_RESTART: Lazy<u32> =
  Lazy::new(|| unsafe { RegisterWindowMessageA(s!("TaskbarCreated")) });
static THREAD_EVENT_TARGET_WINDOW_CLASS: Lazy<Vec<u16>> = Lazy::new(|| unsafe {
  let class_name = util::encode_wide("Tao Thread Event Target");

  let class = WNDCLASSEXW {
    cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
    style: Default::default(),
    lpfnWndProc: Some(util::call_default_window_proc),
    cbClsExtra: 0,
    cbWndExtra: 0,
    hInstance: HINSTANCE(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().0),
    hIcon: HICON::default(),
    hCursor: HCURSOR::default(), // must be null in order for cursor state to work properly
    hbrBackground: HBRUSH::default(),
    lpszMenuName: PCWSTR::null(),
    lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
    hIconSm: HICON::default(),
  };

  RegisterClassExW(&class);

  class_name
});

fn create_event_target_window() -> HWND {
  let window = unsafe {
    CreateWindowExW(
      WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED |
      // WS_EX_TOOLWINDOW prevents this window from ever showing up in the taskbar, which
      // we want to avoid. If you remove this style, this window won't show up in the
      // taskbar *initially*, but it can show up at some later point. This can sometimes
      // happen on its own after several hours have passed, although this has proven
      // difficult to reproduce. Alternatively, it can be manually triggered by killing
      // `explorer.exe` and then starting the process back up.
      // It is unclear why the bug is triggered by waiting for several hours.
      WS_EX_TOOLWINDOW,
      PCWSTR::from_raw(THREAD_EVENT_TARGET_WINDOW_CLASS.clone().as_ptr()),
      PCWSTR::null(),
      WS_OVERLAPPED,
      0,
      0,
      0,
      0,
      None,
      None,
      GetModuleHandleW(PCWSTR::null()).map(Into::into).ok(),
      None,
    )
  };

  let window = match window {
    Ok(w) => w,
    Err(_) => return HWND::default(),
  };

  util::SetWindowLongPtrW(
    window,
    GWL_STYLE,
    // The window technically has to be visible to receive WM_PAINT messages (which are used
    // for delivering events during resizes), but it isn't displayed to the user because of
    // the LAYERED style.
    (WS_VISIBLE | WS_POPUP).0 as isize,
  );
  window
}

fn subclass_event_target_window<T>(
  window: HWND,
  event_loop_runner: EventLoopRunnerShared<T>,
) -> Sender<T> {
  unsafe {
    let (tx, rx) = channel::unbounded();

    let subclass_input = ThreadMsgTargetSubclassInput {
      event_loop_runner,
      user_event_receiver: rx,
    };
    let input_ptr = Box::into_raw(Box::new(subclass_input));
    let subclass_result = SetWindowSubclass(
      window,
      Some(thread_event_target_callback::<T>),
      THREAD_EVENT_TARGET_SUBCLASS_ID,
      input_ptr as usize,
    );
    assert!(subclass_result.as_bool());

    tx
  }
}

fn remove_event_target_window_subclass<T: 'static>(window: HWND) {
  let removal_result = unsafe {
    RemoveWindowSubclass(
      window,
      Some(thread_event_target_callback::<T>),
      THREAD_EVENT_TARGET_SUBCLASS_ID,
    )
  };
  assert!(removal_result.as_bool());
}

/// Capture mouse input, allowing `window` to receive mouse events when the cursor is outside of
/// the window.
unsafe fn capture_mouse(window: HWND, window_state: &mut WindowState) {
  window_state.mouse.capture_count += 1;
  SetCapture(window);
}

/// Release mouse input, stopping windows on this thread from receiving mouse input when the cursor
/// is outside the window.
unsafe fn release_mouse(mut window_state: parking_lot::MutexGuard<'_, WindowState>) {
  window_state.mouse.capture_count = window_state.mouse.capture_count.saturating_sub(1);
  if window_state.mouse.capture_count == 0 {
    // ReleaseCapture() causes a WM_CAPTURECHANGED where we lock the window_state.
    drop(window_state);
    let _ = ReleaseCapture();
  }
}

const WINDOW_SUBCLASS_ID: usize = 0;
const THREAD_EVENT_TARGET_SUBCLASS_ID: usize = 1;
pub(crate) fn subclass_window<T>(window: HWND, subclass_input: SubclassInput<T>) {
  subclass_input.event_loop_runner.register_window(window);
  let input_ptr = Box::into_raw(Box::new(subclass_input));
  let subclass_result = unsafe {
    SetWindowSubclass(
      window,
      Some(public_window_callback::<T>),
      WINDOW_SUBCLASS_ID,
      input_ptr as usize,
    )
  };
  assert!(subclass_result.as_bool());
}

fn remove_window_subclass<T: 'static>(window: HWND) {
  let removal_result = unsafe {
    RemoveWindowSubclass(
      window,
      Some(public_window_callback::<T>),
      WINDOW_SUBCLASS_ID,
    )
  };
  assert!(removal_result.as_bool());
}

fn normalize_pointer_pressure(pressure: u32) -> Option<Force> {
  match pressure {
    1..=1024 => Some(Force::Normalized(pressure as f64 / 1024.0)),
    _ => None,
  }
}

/// Flush redraw events for Tao's windows.
///
/// Tao's API guarantees that all redraw events will be clustered together and dispatched all at
/// once, but the standard Windows message loop doesn't always exhibit that behavior. If multiple
/// windows have had redraws scheduled, but an input event is pushed to the message queue between
/// the `WM_PAINT` call for the first window and the `WM_PAINT` call for the second window, Windows
/// will dispatch the input event immediately instead of flushing all the redraw events. This
/// function explicitly pulls all of Tao's redraw events out of the event queue so that they
/// always all get processed in one fell swoop.
///
/// Returns `true` if this invocation flushed all the redraw events. If this function is re-entrant,
/// it won't flush the redraw events and will return `false`.
#[must_use]
unsafe fn flush_paint_messages<T: 'static>(
  except: Option<HWND>,
  runner: &EventLoopRunner<T>,
) -> bool {
  if !runner.redrawing() {
    runner.main_events_cleared();
    let mut msg = MSG::default();
    runner.owned_windows(|redraw_window| {
      if Some(redraw_window) == except {
        return;
      }

      if !PeekMessageW(
        &mut msg,
        Some(redraw_window),
        WM_PAINT,
        WM_PAINT,
        PM_REMOVE | PM_QS_PAINT,
      )
      .as_bool()
      {
        return;
      }

      let _ = TranslateMessage(&msg);
      DispatchMessageW(&msg);
    });
    true
  } else {
    false
  }
}

unsafe fn process_control_flow<T: 'static>(runner: &EventLoopRunner<T>) {
  match runner.control_flow() {
    ControlFlow::Poll => {
      let _ = PostMessageW(
        Some(runner.thread_msg_target()),
        *PROCESS_NEW_EVENTS_MSG_ID,
        WPARAM(0),
        LPARAM(0),
      );
    }
    ControlFlow::Wait => (),
    ControlFlow::WaitUntil(until) => {
      let _ = PostThreadMessageW(
        runner.wait_thread_id(),
        *WAIT_UNTIL_MSG_ID,
        WPARAM(0),
        LPARAM(Box::into_raw(WaitUntilInstantBox::new(until)) as _),
      );
    }
    ControlFlow::ExitWithCode(_) => (),
  }
}

/// Emit a `ModifiersChanged` event whenever modifiers have changed.
/// Returns the current modifier state
fn update_modifiers<T>(window: HWND, subclass_input: &SubclassInput<T>) -> ModifiersState {
  use crate::event::WindowEvent::ModifiersChanged;

  let modifiers = LAYOUT_CACHE.lock().get_agnostic_mods();
  let mut window_state = subclass_input.window_state.lock();
  if window_state.modifiers_state != modifiers {
    window_state.modifiers_state = modifiers;

    // Drop lock
    drop(window_state);

    unsafe {
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: ModifiersChanged(modifiers),
      });
    }
  }
  modifiers
}

unsafe fn gain_active_focus<T>(window: HWND, subclass_input: &SubclassInput<T>) {
  use crate::event::WindowEvent::Focused;
  update_modifiers(window, subclass_input);

  subclass_input.send_event(Event::WindowEvent {
    window_id: RootWindowId(WindowId(window.0 as _)),
    event: Focused(true),
  });
}

unsafe fn lose_active_focus<T>(window: HWND, subclass_input: &SubclassInput<T>) {
  use crate::event::WindowEvent::{Focused, ModifiersChanged};

  subclass_input.window_state.lock().modifiers_state = ModifiersState::empty();
  subclass_input.send_event(Event::WindowEvent {
    window_id: RootWindowId(WindowId(window.0 as _)),
    event: ModifiersChanged(ModifiersState::empty()),
  });

  subclass_input.send_event(Event::WindowEvent {
    window_id: RootWindowId(WindowId(window.0 as _)),
    event: Focused(false),
  });
}

/// Any window whose callback is configured to this function will have its events propagated
/// through the events loop of the thread the window was created in.
//
// This is the callback that is called by `DispatchMessage` in the events loop.
//
// Returning 0 tells the Win32 API that the message has been processed.
// FIXME: detect WM_DWMCOMPOSITIONCHANGED and call DwmEnableBlurBehindWindow if necessary
unsafe extern "system" fn public_window_callback<T: 'static>(
  window: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  uidsubclass: usize,
  subclass_input_ptr: usize,
) -> LRESULT {
  let subclass_input_ptr = subclass_input_ptr as *mut SubclassInput<T>;
  let (result, subclass_removed, recurse_depth) = {
    let subclass_input = &*subclass_input_ptr;
    subclass_input
      .recurse_depth
      .set(subclass_input.recurse_depth.get() + 1);

    // Clear userdata
    util::SetWindowLongPtrW(window, GWL_USERDATA, 0);

    let result =
      public_window_callback_inner(window, msg, wparam, lparam, uidsubclass, subclass_input);

    let subclass_removed = subclass_input.subclass_removed.get();
    let recurse_depth = subclass_input.recurse_depth.get() - 1;
    subclass_input.recurse_depth.set(recurse_depth);

    (result, subclass_removed, recurse_depth)
  };

  if subclass_removed && recurse_depth == 0 {
    drop(Box::from_raw(subclass_input_ptr))
  }

  result
}

unsafe fn public_window_callback_inner<T: 'static>(
  window: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  _: usize,
  subclass_input: &SubclassInput<T>,
) -> LRESULT {
  let _ = RedrawWindow(
    Some(subclass_input.event_loop_runner.thread_msg_target()),
    None,
    None,
    RDW_INTERNALPAINT,
  );

  let mut result = ProcResult::DefSubclassProc;

  // Send new modifiers before sending key events.
  let mods_changed_callback = || match msg {
    win32wm::WM_KEYDOWN | win32wm::WM_SYSKEYDOWN | win32wm::WM_KEYUP | win32wm::WM_SYSKEYUP => {
      update_modifiers(window, subclass_input);
      result = ProcResult::Value(LRESULT(0));
    }
    _ => (),
  };
  subclass_input
    .event_loop_runner
    .catch_unwind(mods_changed_callback)
    .unwrap_or_else(|| result = ProcResult::Value(LRESULT(-1)));

  let keyboard_callback = || {
    use crate::event::WindowEvent::KeyboardInput;
    let is_keyboard_related = is_msg_keyboard_related(msg);
    if !is_keyboard_related {
      // We return early to avoid a deadlock from locking the window state
      // when not appropriate.
      return;
    }
    let events = {
      let mut key_event_builders =
        crate::platform_impl::platform::keyboard::KEY_EVENT_BUILDERS.lock();
      if let Some(key_event_builder) = key_event_builders.get_mut(&WindowId(window.0 as _)) {
        key_event_builder.process_message(window, msg, wparam, lparam, &mut result)
      } else {
        Vec::new()
      }
    };
    for event in events {
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: KeyboardInput {
          device_id: DEVICE_ID,
          event: event.event,
          is_synthetic: event.is_synthetic,
        },
      });
    }
  };
  subclass_input
    .event_loop_runner
    .catch_unwind(keyboard_callback)
    .unwrap_or_else(|| result = ProcResult::Value(LRESULT(-1)));

  let ime_callback = || {
    use crate::event::WindowEvent::ReceivedImeText;
    let is_ime_related = is_msg_ime_related(msg);
    if !is_ime_related {
      return;
    }
    let text = {
      let mut window_state = subclass_input.window_state.lock();
      window_state
        .ime_handler
        .process_message(window, msg, wparam, lparam, &mut result)
    };
    if let Some(str) = text {
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: ReceivedImeText(str),
      });
    }
  };
  subclass_input
    .event_loop_runner
    .catch_unwind(ime_callback)
    .unwrap_or_else(|| result = ProcResult::Value(LRESULT(-1)));

  // I decided to bind the closure to `callback` and pass it to catch_unwind rather than passing
  // the closure to catch_unwind directly so that the match body indendation wouldn't change and
  // the git blame and history would be preserved.
  let callback = || match msg {
    win32wm::WM_ENTERSIZEMOVE => {
      subclass_input
        .window_state
        .lock()
        .set_window_flags_in_place(|f| f.insert(WindowFlags::MARKER_IN_SIZE_MOVE));
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_EXITSIZEMOVE => {
      let mut state = subclass_input.window_state.lock();
      if state.dragging {
        state.dragging = false;
        let _ = unsafe { PostMessageW(Some(window), WM_LBUTTONUP, WPARAM::default(), lparam) };
      }
      state.set_window_flags_in_place(|f| f.remove(WindowFlags::MARKER_IN_SIZE_MOVE));
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_NCCREATE => {
      enable_non_client_dpi_scaling(window);
    }
    win32wm::WM_NCLBUTTONDOWN => {
      if wparam.0 == HTCAPTION as _ {
        let _ = PostMessageW(Some(window), WM_MOUSEMOVE, WPARAM(0), lparam);
      }

      use crate::event::WindowEvent::DecorationsClick;
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: DecorationsClick,
      });
    }

    win32wm::WM_CLOSE => {
      use crate::event::WindowEvent::CloseRequested;
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: CloseRequested,
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_DESTROY => {
      use crate::event::WindowEvent::Destroyed;
      let _ = RevokeDragDrop(window);
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: Destroyed,
      });
      subclass_input.event_loop_runner.remove_window(window);
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_NCDESTROY => {
      remove_window_subclass::<T>(window);
      subclass_input.subclass_removed.set(true);
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_PAINT => {
      if subclass_input.event_loop_runner.should_buffer() {
        // this branch can happen in response to `UpdateWindow`, if win32 decides to
        // redraw the window outside the normal flow of the event loop.
        let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      } else {
        let managing_redraw = flush_paint_messages(Some(window), &subclass_input.event_loop_runner);
        subclass_input.send_event(Event::RedrawRequested(RootWindowId(WindowId(
          window.0 as _,
        ))));
        if managing_redraw {
          subclass_input.event_loop_runner.redraw_events_cleared();
          process_control_flow(&subclass_input.event_loop_runner);
        }
      }
    }

    win32wm::WM_ERASEBKGND => {
      let w = subclass_input.window_state.lock();
      if let Some(color) = w.background_color {
        let hdc = HDC(wparam.0 as *mut _);
        let mut rc = RECT::default();
        if GetClientRect(window, &mut rc).is_ok() {
          let brush = CreateSolidBrush(util::RGB(color.0, color.1, color.2));
          FillRect(hdc, &rc, brush);
          let _ = DeleteObject(brush.into());

          result = ProcResult::Value(LRESULT(1));
        } else {
          result = ProcResult::DefSubclassProc;
        }
      } else {
        result = ProcResult::DefSubclassProc;
      }
    }

    win32wm::WM_WINDOWPOSCHANGING => {
      let mut window_state = subclass_input.window_state.lock();

      if let Some(ref mut fullscreen) = window_state.fullscreen {
        let window_pos = &mut *(lparam.0 as *mut WINDOWPOS);
        let new_rect = RECT {
          left: window_pos.x,
          top: window_pos.y,
          right: window_pos.x + window_pos.cx,
          bottom: window_pos.y + window_pos.cy,
        };

        const NOMOVE_OR_NOSIZE: SET_WINDOW_POS_FLAGS =
          SET_WINDOW_POS_FLAGS(SWP_NOMOVE.0 | SWP_NOSIZE.0);

        let new_rect = if (window_pos.flags & NOMOVE_OR_NOSIZE) != SET_WINDOW_POS_FLAGS::default() {
          let cur_rect = util::get_window_rect(window)
            .expect("Unexpected GetWindowRect failure; please report this error to tauri-apps/tao on GitHub");

          match window_pos.flags & NOMOVE_OR_NOSIZE {
            NOMOVE_OR_NOSIZE => None,

            SWP_NOMOVE => Some(RECT {
              left: cur_rect.left,
              top: cur_rect.top,
              right: cur_rect.left + window_pos.cx,
              bottom: cur_rect.top + window_pos.cy,
            }),

            SWP_NOSIZE => Some(RECT {
              left: window_pos.x,
              top: window_pos.y,
              right: window_pos.x - cur_rect.left + cur_rect.right,
              bottom: window_pos.y - cur_rect.top + cur_rect.bottom,
            }),

            _ => unreachable!(),
          }
        } else {
          Some(new_rect)
        };

        if let Some(new_rect) = new_rect {
          let new_monitor = MonitorFromRect(&new_rect, MONITOR_DEFAULTTONULL);
          match fullscreen {
            Fullscreen::Borderless(ref mut fullscreen_monitor) => {
              if !new_monitor.is_invalid()
                && fullscreen_monitor
                  .as_ref()
                  .map(|monitor| new_monitor != monitor.inner.hmonitor())
                  .unwrap_or(true)
              {
                if let Ok(new_monitor_info) = monitor::get_monitor_info(new_monitor) {
                  let new_monitor_rect = new_monitor_info.monitorInfo.rcMonitor;
                  window_pos.x = new_monitor_rect.left;
                  window_pos.y = new_monitor_rect.top;
                  window_pos.cx = new_monitor_rect.right - new_monitor_rect.left;
                  window_pos.cy = new_monitor_rect.bottom - new_monitor_rect.top;
                }
                *fullscreen_monitor = Some(crate::monitor::MonitorHandle {
                  inner: MonitorHandle::new(new_monitor),
                });
              }
            }
            Fullscreen::Exclusive(ref video_mode) => {
              let old_monitor = video_mode.video_mode.monitor.hmonitor();
              if let Ok(old_monitor_info) = monitor::get_monitor_info(old_monitor) {
                let old_monitor_rect = old_monitor_info.monitorInfo.rcMonitor;
                window_pos.x = old_monitor_rect.left;
                window_pos.y = old_monitor_rect.top;
                window_pos.cx = old_monitor_rect.right - old_monitor_rect.left;
                window_pos.cy = old_monitor_rect.bottom - old_monitor_rect.top;
              }
            }
          }
        }
      }

      let window_flags = window_state.window_flags;
      if window_flags.contains(WindowFlags::ALWAYS_ON_BOTTOM) {
        let window_pos = &mut *(lparam.0 as *mut WINDOWPOS);
        window_pos.hwndInsertAfter = HWND_BOTTOM;
      }

      result = ProcResult::Value(LRESULT(0));
    }

    // WM_MOVE supplies client area positions, so we send Moved here instead.
    win32wm::WM_WINDOWPOSCHANGED => {
      use crate::event::WindowEvent::Moved;

      let windowpos = lparam.0 as *const WINDOWPOS;
      if (*windowpos).flags & SWP_NOMOVE != SWP_NOMOVE {
        let physical_position = PhysicalPosition::new((*windowpos).x, (*windowpos).y);
        subclass_input.send_event(Event::WindowEvent {
          window_id: RootWindowId(WindowId(window.0 as _)),
          event: Moved(physical_position),
        });
      }

      // This is necessary for us to still get sent WM_SIZE.
      result = ProcResult::DefSubclassProc;
    }

    win32wm::WM_SIZE => {
      use crate::event::WindowEvent::Resized;
      let w = u32::from(util::LOWORD(lparam.0 as u32));
      let h = u32::from(util::HIWORD(lparam.0 as u32));

      let physical_size = PhysicalSize::new(w, h);
      let event = Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: Resized(physical_size),
      };

      {
        let mut w = subclass_input.window_state.lock();
        // See WindowFlags::MARKER_RETAIN_STATE_ON_SIZE docs for info on why this `if` check exists.
        if !w
          .window_flags()
          .contains(WindowFlags::MARKER_RETAIN_STATE_ON_SIZE)
        {
          let maximized = wparam.0 == win32wm::SIZE_MAXIMIZED as _;
          w.set_window_flags_in_place(|f| f.set(WindowFlags::MAXIMIZED, maximized));
        }
      }

      subclass_input.send_event(event);
      result = ProcResult::Value(LRESULT(0));
    }

    // this is necessary for us to maintain minimize/restore state
    win32wm::WM_SYSCOMMAND => {
      if wparam.0 == SC_RESTORE as _ {
        let mut w = subclass_input.window_state.lock();
        w.set_window_flags_in_place(|f| f.set(WindowFlags::MINIMIZED, false));
      }
      if wparam.0 == SC_MINIMIZE as _ {
        let mut w = subclass_input.window_state.lock();
        w.set_window_flags_in_place(|f| f.set(WindowFlags::MINIMIZED, true));
      }
      // Send `WindowEvent::Minimized` here if we decide to implement one

      if wparam.0 == SC_SCREENSAVE as _ {
        let window_state = subclass_input.window_state.lock();
        if window_state.fullscreen.is_some() {
          result = ProcResult::Value(LRESULT(0));
          return;
        }
      }

      result = ProcResult::DefWindowProc;
    }

    win32wm::WM_MOUSEMOVE => {
      use crate::event::WindowEvent::{CursorEntered, CursorMoved};
      let mouse_was_outside_window = {
        let mut w = subclass_input.window_state.lock();

        let was_outside_window = !w.mouse.cursor_flags().contains(CursorFlags::IN_WINDOW);
        w.mouse
          .set_cursor_flags(window, |f| f.set(CursorFlags::IN_WINDOW, true))
          .ok();
        was_outside_window
      };

      if mouse_was_outside_window {
        subclass_input.send_event(Event::WindowEvent {
          window_id: RootWindowId(WindowId(window.0 as _)),
          event: CursorEntered {
            device_id: DEVICE_ID,
          },
        });

        // Calling TrackMouseEvent in order to receive mouse leave events.
        let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
          cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
          dwFlags: TME_LEAVE,
          hwndTrack: window,
          dwHoverTime: HOVER_DEFAULT,
        });
      }

      let x = f64::from(util::GET_X_LPARAM(lparam));
      let y = f64::from(util::GET_Y_LPARAM(lparam));
      let position = PhysicalPosition::new(x, y);
      let cursor_moved;
      {
        // handle spurious WM_MOUSEMOVE messages
        // see https://devblogs.microsoft.com/oldnewthing/20031001-00/?p=42343
        // and http://debugandconquer.blogspot.com/2015/08/the-cause-of-spurious-mouse-move.html
        let mut w = subclass_input.window_state.lock();
        cursor_moved = w.mouse.last_position != Some(position);
        w.mouse.last_position = Some(position);
      }
      if cursor_moved {
        let modifiers = update_modifiers(window, subclass_input);
        subclass_input.send_event(Event::WindowEvent {
          window_id: RootWindowId(WindowId(window.0 as _)),
          event: CursorMoved {
            device_id: DEVICE_ID,
            position,
            modifiers,
          },
        });
      }

      result = ProcResult::Value(LRESULT(0));
    }

    win32c::WM_MOUSELEAVE => {
      use crate::event::WindowEvent::CursorLeft;
      {
        let mut w = subclass_input.window_state.lock();
        w.mouse
          .set_cursor_flags(window, |f| f.set(CursorFlags::IN_WINDOW, false))
          .ok();
      }

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: CursorLeft {
          device_id: DEVICE_ID,
        },
      });

      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_MOUSEWHEEL => {
      use crate::event::MouseScrollDelta::LineDelta;

      let value = f32::from(util::GET_WHEEL_DELTA_WPARAM(wparam));
      let value = value / WHEEL_DELTA as f32;

      let modifiers = update_modifiers(window, subclass_input);

      let mut scroll_lines = DEFAULT_SCROLL_LINES_PER_WHEEL_DELTA;

      let _ = SystemParametersInfoW(
        SPI_GETWHEELSCROLLLINES,
        0,
        Some(&mut scroll_lines as *mut isize as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
      );

      if scroll_lines as u32 == WHEEL_PAGESCROLL {
        // TODO: figure out how to handle page scrolls
        scroll_lines = DEFAULT_SCROLL_LINES_PER_WHEEL_DELTA;
      }

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: WindowEvent::MouseWheel {
          device_id: DEVICE_ID,
          delta: LineDelta(0.0, value * scroll_lines as f32),
          phase: TouchPhase::Moved,
          modifiers,
        },
      });

      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_MOUSEHWHEEL => {
      use crate::event::MouseScrollDelta::LineDelta;

      let value = f32::from(util::GET_WHEEL_DELTA_WPARAM(wparam));
      let value = value / WHEEL_DELTA as f32;

      let modifiers = update_modifiers(window, subclass_input);

      let mut scroll_characters = DEFAULT_SCROLL_CHARACTERS_PER_WHEEL_DELTA;

      let _ = SystemParametersInfoW(
        SPI_GETWHEELSCROLLCHARS,
        0,
        Some(&mut scroll_characters as *mut isize as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
      );

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: WindowEvent::MouseWheel {
          device_id: DEVICE_ID,
          delta: LineDelta(value * scroll_characters as f32, 0.0),
          phase: TouchPhase::Moved,
          modifiers,
        },
      });

      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_KEYDOWN | win32wm::WM_SYSKEYDOWN => {
      if msg == WM_SYSKEYDOWN && wparam.0 == usize::from(VK_F4.0) {
        result = ProcResult::DefSubclassProc;
      }
    }

    win32wm::WM_LBUTTONDOWN => {
      use crate::event::{ElementState::Pressed, MouseButton::Left, WindowEvent::MouseInput};

      capture_mouse(window, &mut subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Pressed,
          button: Left,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_LBUTTONUP => {
      use crate::event::{ElementState::Released, MouseButton::Left, WindowEvent::MouseInput};

      release_mouse(subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Released,
          button: Left,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_RBUTTONDOWN => {
      use crate::event::{ElementState::Pressed, MouseButton::Right, WindowEvent::MouseInput};

      capture_mouse(window, &mut subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Pressed,
          button: Right,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_RBUTTONUP => {
      use crate::event::{ElementState::Released, MouseButton::Right, WindowEvent::MouseInput};

      release_mouse(subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Released,
          button: Right,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_MBUTTONDOWN => {
      use crate::event::{ElementState::Pressed, MouseButton::Middle, WindowEvent::MouseInput};

      capture_mouse(window, &mut subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Pressed,
          button: Middle,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_MBUTTONUP => {
      use crate::event::{ElementState::Released, MouseButton::Middle, WindowEvent::MouseInput};

      release_mouse(subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Released,
          button: Middle,
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_XBUTTONDOWN => {
      use crate::event::{ElementState::Pressed, MouseButton::Other, WindowEvent::MouseInput};
      let xbutton = util::GET_XBUTTON_WPARAM(wparam);

      capture_mouse(window, &mut subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Pressed,
          button: Other(xbutton),
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_XBUTTONUP => {
      use crate::event::{ElementState::Released, MouseButton::Other, WindowEvent::MouseInput};
      let xbutton = util::GET_XBUTTON_WPARAM(wparam);

      release_mouse(subclass_input.window_state.lock());

      let modifiers = update_modifiers(window, subclass_input);

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: MouseInput {
          device_id: DEVICE_ID,
          state: Released,
          button: Other(xbutton),
          modifiers,
        },
      });
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_CAPTURECHANGED => {
      // lparam here is a handle to the window which is gaining mouse capture.
      // If it is the same as our window, then we're essentially retaining the capture. This
      // can happen if `SetCapture` is called on our window when it already has the mouse
      // capture.
      if lparam.0 != window.0 as _ {
        subclass_input.window_state.lock().mouse.capture_count = 0;
      }
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_TOUCH => {
      let pcount = usize::from(util::LOWORD(wparam.0 as u32));
      let mut inputs: Vec<TOUCHINPUT> = Vec::with_capacity(pcount);
      let uninit_inputs = inputs.spare_capacity_mut();
      let htouch = HTOUCHINPUT(lparam.0 as _);
      if GetTouchInputInfo(
        htouch,
        mem::transmute::<
          &mut [std::mem::MaybeUninit<windows::Win32::UI::Input::Touch::TOUCHINPUT>],
          &mut [windows::Win32::UI::Input::Touch::TOUCHINPUT],
        >(uninit_inputs),
        mem::size_of::<TOUCHINPUT>() as i32,
      )
      .is_ok()
      {
        inputs.set_len(pcount);
        for input in &inputs {
          let mut location = POINT {
            x: input.x / 100,
            y: input.y / 100,
          };

          if !ScreenToClient(window, &mut location as *mut _).as_bool() {
            continue;
          }

          let x = location.x as f64 + (input.x % 100) as f64 / 100f64;
          let y = location.y as f64 + (input.y % 100) as f64 / 100f64;
          let location = PhysicalPosition::new(x, y);
          subclass_input.send_event(Event::WindowEvent {
            window_id: RootWindowId(WindowId(window.0 as _)),
            event: WindowEvent::Touch(Touch {
              phase: if (input.dwFlags & TOUCHEVENTF_DOWN) != Default::default() {
                TouchPhase::Started
              } else if (input.dwFlags & TOUCHEVENTF_UP) != Default::default() {
                TouchPhase::Ended
              } else if (input.dwFlags & TOUCHEVENTF_MOVE) != Default::default() {
                TouchPhase::Moved
              } else {
                continue;
              },
              location,
              force: None, // WM_TOUCH doesn't support pressure information
              id: input.dwID as u64,
              device_id: DEVICE_ID,
            }),
          });
        }
      }
      let _ = CloseTouchInputHandle(htouch);
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_POINTERDOWN | win32wm::WM_POINTERUPDATE | win32wm::WM_POINTERUP => {
      if let (
        Some(GetPointerFrameInfoHistory),
        Some(SkipPointerFrameMessages),
        Some(GetPointerDeviceRects),
      ) = (
        *GET_POINTER_FRAME_INFO_HISTORY,
        *SKIP_POINTER_FRAME_MESSAGES,
        *GET_POINTER_DEVICE_RECTS,
      ) {
        let pointer_id = u32::from(util::LOWORD(wparam.0 as u32));
        let mut entries_count = 0_u32;
        let mut pointers_count = 0_u32;
        if !GetPointerFrameInfoHistory(
          pointer_id,
          &mut entries_count as *mut _,
          &mut pointers_count as *mut _,
          std::ptr::null_mut(),
        )
        .as_bool()
        {
          result = ProcResult::Value(LRESULT(0));
          return;
        }

        let pointer_info_count = (entries_count * pointers_count) as usize;
        let mut pointer_infos: Vec<POINTER_INFO> = Vec::with_capacity(pointer_info_count);
        let uninit_pointer_infos = pointer_infos.spare_capacity_mut();
        if !GetPointerFrameInfoHistory(
          pointer_id,
          &mut entries_count as *mut _,
          &mut pointers_count as *mut _,
          uninit_pointer_infos.as_mut_ptr() as *mut _,
        )
        .as_bool()
        {
          result = ProcResult::Value(LRESULT(0));
          return;
        }
        pointer_infos.set_len(pointer_info_count);

        // https://docs.microsoft.com/en-us/windows/desktop/api/winuser/nf-winuser-getpointerframeinfohistory
        // The information retrieved appears in reverse chronological order, with the most recent entry in the first
        // row of the returned array
        for pointer_info in pointer_infos.iter().rev() {
          let mut device_rect = mem::MaybeUninit::uninit();
          let mut display_rect = mem::MaybeUninit::uninit();

          if !(GetPointerDeviceRects(
            pointer_info.sourceDevice,
            device_rect.as_mut_ptr(),
            display_rect.as_mut_ptr(),
          ))
          .as_bool()
          {
            continue;
          }

          let device_rect = device_rect.assume_init();
          let display_rect = display_rect.assume_init();

          // For the most precise himetric to pixel conversion we calculate the ratio between the resolution
          // of the display device (pixel) and the touch device (himetric).
          let himetric_to_pixel_ratio_x = (display_rect.right - display_rect.left) as f64
            / (device_rect.right - device_rect.left) as f64;
          let himetric_to_pixel_ratio_y = (display_rect.bottom - display_rect.top) as f64
            / (device_rect.bottom - device_rect.top) as f64;

          // ptHimetricLocation's origin is 0,0 even on multi-monitor setups.
          // On multi-monitor setups we need to translate the himetric location to the rect of the
          // display device it's attached to.
          let x = display_rect.left as f64
            + pointer_info.ptHimetricLocation.x as f64 * himetric_to_pixel_ratio_x;
          let y = display_rect.top as f64
            + pointer_info.ptHimetricLocation.y as f64 * himetric_to_pixel_ratio_y;

          let mut location = POINT {
            x: x.floor() as i32,
            y: y.floor() as i32,
          };

          if !ScreenToClient(window, &mut location as *mut _).as_bool() {
            continue;
          }

          let force = match pointer_info.pointerType {
            win32wm::PT_TOUCH => {
              let mut touch_info = mem::MaybeUninit::uninit();
              GET_POINTER_TOUCH_INFO.and_then(|GetPointerTouchInfo| {
                if GetPointerTouchInfo(pointer_info.pointerId, touch_info.as_mut_ptr()).as_bool() {
                  normalize_pointer_pressure(touch_info.assume_init().pressure)
                } else {
                  None
                }
              })
            }
            win32wm::PT_PEN => {
              let mut pen_info = mem::MaybeUninit::uninit();
              GET_POINTER_PEN_INFO.and_then(|GetPointerPenInfo| {
                if GetPointerPenInfo(pointer_info.pointerId, pen_info.as_mut_ptr()).as_bool() {
                  normalize_pointer_pressure(pen_info.assume_init().pressure)
                } else {
                  None
                }
              })
            }
            _ => None,
          };

          let x = location.x as f64 + x.fract();
          let y = location.y as f64 + y.fract();
          let location = PhysicalPosition::new(x, y);
          subclass_input.send_event(Event::WindowEvent {
            window_id: RootWindowId(WindowId(window.0 as _)),
            event: WindowEvent::Touch(Touch {
              phase: if (pointer_info.pointerFlags & POINTER_FLAG_DOWN) != Default::default() {
                TouchPhase::Started
              } else if (pointer_info.pointerFlags & POINTER_FLAG_UP) != Default::default() {
                TouchPhase::Ended
              } else if (pointer_info.pointerFlags & POINTER_FLAG_UPDATE) != Default::default() {
                TouchPhase::Moved
              } else {
                continue;
              },
              location,
              force,
              id: pointer_info.pointerId as u64,
              device_id: DEVICE_ID,
            }),
          });
        }

        let _ = SkipPointerFrameMessages(pointer_id);
      }

      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_NCACTIVATE => {
      let is_active = wparam != WPARAM(0);
      let active_focus_changed = subclass_input.window_state.lock().set_active(is_active);
      if active_focus_changed {
        if is_active {
          gain_active_focus(window, subclass_input);
        } else {
          lose_active_focus(window, subclass_input);
        }
      }
      result = ProcResult::DefWindowProc;
    }

    win32wm::WM_SETFOCUS => {
      let active_focus_changed = subclass_input.window_state.lock().set_focused(true);
      if active_focus_changed {
        gain_active_focus(window, subclass_input);
      }
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_KILLFOCUS => {
      let active_focus_changed = subclass_input.window_state.lock().set_focused(false);
      if active_focus_changed {
        lose_active_focus(window, subclass_input);
      }
      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_SETCURSOR => {
      let set_cursor_to = {
        let window_state = subclass_input.window_state.lock();
        // The return value for the preceding `WM_NCHITTEST` message is conveniently
        // provided through the low-order word of lParam. We use that here since
        // `WM_MOUSEMOVE` seems to come after `WM_SETCURSOR` for a given cursor movement.
        let in_client_area = u32::from(util::LOWORD(lparam.0 as u32)) == HTCLIENT;
        if in_client_area {
          Some(window_state.mouse.cursor)
        } else {
          None
        }
      };

      match set_cursor_to {
        Some(cursor) => {
          if let Ok(cursor) = LoadCursorW(None, cursor.to_windows_cursor()) {
            SetCursor(Some(cursor));
          }
          result = ProcResult::Value(LRESULT(0));
        }
        None => result = ProcResult::DefWindowProc,
      }
    }

    win32wm::WM_GETMINMAXINFO => {
      let mmi = lparam.0 as *mut MINMAXINFO;

      let window_state = subclass_input.window_state.lock();
      let is_decorated = window_state
        .window_flags()
        .contains(WindowFlags::MARKER_DECORATIONS);

      let size_constraints = window_state.size_constraints;

      if size_constraints.has_min() {
        let min_size = PhysicalSize::new(
          size_constraints
            .min_width
            .unwrap_or_else(|| PixelUnit::Physical(GetSystemMetrics(SM_CXMINTRACK).into()))
            .to_physical(window_state.scale_factor)
            .0,
          size_constraints
            .min_height
            .unwrap_or_else(|| PixelUnit::Physical(GetSystemMetrics(SM_CYMINTRACK).into()))
            .to_physical(window_state.scale_factor)
            .0,
        );
        let (width, height): (u32, u32) = util::adjust_size(window, min_size, is_decorated).into();
        (*mmi).ptMinTrackSize = POINT {
          x: width as i32,
          y: height as i32,
        };
      }
      if size_constraints.has_max() {
        let max_size = PhysicalSize::new(
          size_constraints
            .max_width
            .unwrap_or_else(|| PixelUnit::Physical(GetSystemMetrics(SM_CXMAXTRACK).into()))
            .to_physical(window_state.scale_factor)
            .0,
          size_constraints
            .max_height
            .unwrap_or_else(|| PixelUnit::Physical(GetSystemMetrics(SM_CYMAXTRACK).into()))
            .to_physical(window_state.scale_factor)
            .0,
        );
        let (width, height): (u32, u32) = util::adjust_size(window, max_size, is_decorated).into();
        (*mmi).ptMaxTrackSize = POINT {
          x: width as i32,
          y: height as i32,
        };
      }

      result = ProcResult::Value(LRESULT(0));
    }

    // Only sent on Windows 8.1 or newer. On Windows 7 and older user has to log out to change
    // DPI, therefore all applications are closed while DPI is changing.
    win32wm::WM_DPICHANGED => {
      use crate::event::WindowEvent::ScaleFactorChanged;

      // This message actually provides two DPI values - x and y. However MSDN says that
      // "you only need to use either the X-axis or the Y-axis value when scaling your
      // application since they are the same".
      // https://msdn.microsoft.com/en-us/library/windows/desktop/dn312083(v=vs.85).aspx
      let new_dpi_x = u32::from(util::LOWORD(wparam.0 as u32));
      let new_scale_factor = dpi_to_scale_factor(new_dpi_x);
      let old_scale_factor: f64;

      let (allow_resize, is_decorated) = {
        let mut window_state = subclass_input.window_state.lock();
        old_scale_factor = window_state.scale_factor;
        window_state.scale_factor = new_scale_factor;

        if (new_scale_factor - old_scale_factor).abs() < f64::EPSILON {
          result = ProcResult::Value(LRESULT(0));
          return;
        }

        let window_flags = window_state.window_flags();
        (
          window_state.fullscreen.is_none() && !window_flags.contains(WindowFlags::MAXIMIZED),
          window_flags.contains(WindowFlags::MARKER_DECORATIONS),
        )
      };

      let mut style = WINDOW_STYLE(GetWindowLongW(window, GWL_STYLE) as u32);
      // if the window isn't decorated, remove `WS_SIZEBOX` and `WS_CAPTION` so
      // `AdjustWindowRect*` functions doesn't account for the hidden caption and borders and
      // calculates a correct size for the client area.
      if !is_decorated {
        style &= !WS_CAPTION;
        style &= !WS_SIZEBOX;
      }
      let style_ex = WINDOW_EX_STYLE(GetWindowLongW(window, GWL_EXSTYLE) as u32);

      // New size as suggested by Windows.
      let suggested_rect = *(lparam.0 as *const RECT);

      // The window rect provided is the window's outer size, not it's inner size. However,
      // win32 doesn't provide an `UnadjustWindowRectEx` function to get the client rect from
      // the outer rect, so we instead adjust the window rect to get the decoration margins
      // and remove them from the outer size.
      let margin_left: i32;
      let margin_top: i32;
      // let margin_right: i32;
      // let margin_bottom: i32;
      {
        let adjusted_rect =
          util::adjust_window_rect_with_styles(window, style, style_ex, suggested_rect)
            .unwrap_or(suggested_rect);
        margin_left = suggested_rect.left - adjusted_rect.left;
        margin_top = suggested_rect.top - adjusted_rect.top;
        // margin_right = adjusted_rect.right - suggested_rect.right;
        // margin_bottom = adjusted_rect.bottom - suggested_rect.bottom;
      }

      let old_physical_inner_rect = {
        let mut old_physical_inner_rect = RECT::default();
        let _ = GetClientRect(window, &mut old_physical_inner_rect);
        let mut origin = POINT::default();
        let _ = ClientToScreen(window, &mut origin);

        old_physical_inner_rect.left += origin.x;
        old_physical_inner_rect.right += origin.x;
        old_physical_inner_rect.top += origin.y;
        old_physical_inner_rect.bottom += origin.y;

        old_physical_inner_rect
      };
      let old_physical_inner_size = PhysicalSize::new(
        (old_physical_inner_rect.right - old_physical_inner_rect.left) as u32,
        (old_physical_inner_rect.bottom - old_physical_inner_rect.top) as u32,
      );

      // `allow_resize` prevents us from re-applying DPI adjustment to the restored size after
      // exiting fullscreen (the restored size is already DPI adjusted).
      let mut new_physical_inner_size = match allow_resize {
        // We calculate our own size because the default suggested rect doesn't do a great job
        // of preserving the window's logical size.
        true => old_physical_inner_size
          .to_logical::<f64>(old_scale_factor)
          .to_physical::<u32>(new_scale_factor),
        false => old_physical_inner_size,
      };

      // When the "Show window contents while dragging" is turned off, there is no need to adjust the window size.
      if !is_show_window_contents_while_dragging_enabled() {
        new_physical_inner_size = old_physical_inner_size;
      }

      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: ScaleFactorChanged {
          scale_factor: new_scale_factor,
          new_inner_size: &mut new_physical_inner_size,
        },
      });

      let dragging_window: bool;

      {
        let window_state = subclass_input.window_state.lock();
        dragging_window = window_state
          .window_flags()
          .contains(WindowFlags::MARKER_IN_SIZE_MOVE);
        // Unset maximized if we're changing the window's size.
        if new_physical_inner_size != old_physical_inner_size {
          WindowState::set_window_flags(window_state, window, |f| {
            f.set(WindowFlags::MAXIMIZED, false)
          });
        }
      }

      let new_outer_rect: RECT;
      if util::WIN_VERSION.build < 22000 {
        // The window position needs adjustment on Windows 10.
        {
          let suggested_ul = (
            suggested_rect.left + margin_left,
            suggested_rect.top + margin_top,
          );

          let mut conservative_rect = RECT {
            left: suggested_ul.0,
            top: suggested_ul.1,
            right: suggested_ul.0 + new_physical_inner_size.width as i32,
            bottom: suggested_ul.1 + new_physical_inner_size.height as i32,
          };

          conservative_rect =
            util::adjust_window_rect_with_styles(window, style, style_ex, conservative_rect)
              .unwrap_or(conservative_rect);

          // If we're dragging the window, offset the window so that the cursor's
          // relative horizontal position in the title bar is preserved.
          if dragging_window {
            let bias = {
              let cursor_pos = {
                let mut pos = POINT::default();
                let _ = GetCursorPos(&mut pos);
                pos
              };
              let suggested_cursor_horizontal_ratio = (cursor_pos.x - suggested_rect.left) as f64
                / (suggested_rect.right - suggested_rect.left) as f64;

              (cursor_pos.x
                - (suggested_cursor_horizontal_ratio
                  * (conservative_rect.right - conservative_rect.left) as f64)
                  as i32)
                - conservative_rect.left
            };
            conservative_rect.left += bias;
            conservative_rect.right += bias;
          }

          // Check to see if the new window rect is on the monitor with the new DPI factor.
          // If it isn't, offset the window so that it is.
          let new_dpi_monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
          let conservative_rect_monitor =
            MonitorFromRect(&conservative_rect, MONITOR_DEFAULTTONULL);
          new_outer_rect = {
            if conservative_rect_monitor != new_dpi_monitor {
              let get_monitor_rect = |monitor| {
                let mut monitor_info = MONITORINFO {
                  cbSize: mem::size_of::<MONITORINFO>() as _,
                  ..Default::default()
                };
                let _ = GetMonitorInfoW(monitor, &mut monitor_info);
                monitor_info.rcMonitor
              };
              let wrong_monitor = conservative_rect_monitor;
              let wrong_monitor_rect = get_monitor_rect(wrong_monitor);
              let new_monitor_rect = get_monitor_rect(new_dpi_monitor);

              // The direction to nudge the window in to get the window onto the monitor with
              // the new DPI factor. We calculate this by seeing which monitor edges are
              // shared and nudging away from the wrong monitor based on those.
              let delta_nudge_to_dpi_monitor = (
                if wrong_monitor_rect.left == new_monitor_rect.right {
                  -1
                } else if wrong_monitor_rect.right == new_monitor_rect.left {
                  1
                } else {
                  0
                },
                if wrong_monitor_rect.bottom == new_monitor_rect.top {
                  1
                } else if wrong_monitor_rect.top == new_monitor_rect.bottom {
                  -1
                } else {
                  0
                },
              );

              let abort_after_iterations = new_monitor_rect.right - new_monitor_rect.left
                + new_monitor_rect.bottom
                - new_monitor_rect.top;
              for _ in 0..abort_after_iterations {
                conservative_rect.left += delta_nudge_to_dpi_monitor.0;
                conservative_rect.right += delta_nudge_to_dpi_monitor.0;
                conservative_rect.top += delta_nudge_to_dpi_monitor.1;
                conservative_rect.bottom += delta_nudge_to_dpi_monitor.1;

                if MonitorFromRect(&conservative_rect, MONITOR_DEFAULTTONULL) == new_dpi_monitor {
                  break;
                }
              }
            }

            conservative_rect
          };
        }
      } else {
        // The suggested position is fine w/o adjustment on Windows 11.
        new_outer_rect = suggested_rect
      }

      let _ = SetWindowPos(
        window,
        None,
        new_outer_rect.left,
        new_outer_rect.top,
        new_outer_rect.right - new_outer_rect.left,
        new_outer_rect.bottom - new_outer_rect.top,
        SWP_NOZORDER | SWP_NOACTIVATE,
      );

      result = ProcResult::Value(LRESULT(0));
    }

    win32wm::WM_SETTINGCHANGE => {
      update_theme(subclass_input, window, true);
    }

    win32wm::WM_NCCALCSIZE => {
      let window_flags = subclass_input.window_state.lock().window_flags();
      let is_fullscreen = subclass_input.window_state.lock().fullscreen.is_some();

      if wparam == WPARAM(0) || window_flags.contains(WindowFlags::MARKER_DECORATIONS) {
        result = ProcResult::DefSubclassProc;
      } else {
        // adjust the maximized borderless window so it doesn't cover the taskbar
        if util::is_maximized(window).unwrap_or(false) {
          let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
          if let Ok(monitor_info) =
            monitor::get_monitor_info(MonitorFromRect(&params.rgrc[0], MONITOR_DEFAULTTONULL))
          {
            let mut rect = monitor_info.monitorInfo.rcWork;

            fn has_edge(edge: u32) -> bool {
              let mut app_data = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as _,
                uEdge: edge,
                ..Default::default()
              };
              unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAR, &mut app_data) != 0 }
            }

            // keep a 1px for taskbar auto-hide to work
            if has_edge(ABE_BOTTOM) {
              rect.bottom -= 1;
            }
            if has_edge(ABE_LEFT) {
              rect.left += 1;
            }
            if has_edge(ABE_TOP) {
              rect.top += 1;
            }
            if has_edge(ABE_RIGHT) {
              rect.right -= 1;
            }

            params.rgrc[0] = rect;
          }
        } else if window_flags.contains(WindowFlags::MARKER_UNDECORATED_SHADOW) && !is_fullscreen {
          let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);

          let insets = util::calculate_window_insets(window);

          params.rgrc[0].left += insets.left;
          params.rgrc[0].top += insets.top;
          params.rgrc[0].right -= insets.right;
          params.rgrc[0].bottom -= insets.bottom;
        }
        result = ProcResult::Value(LRESULT(0)); // return 0 here to make the window borderless
      }
    }

    win32wm::WM_NCHITTEST => {
      let window_state = subclass_input.window_state.lock();
      let window_flags = window_state.window_flags();

      // Allow resizing unmaximized non-fullscreen undecorated window
      if !window_flags.contains(WindowFlags::MARKER_DECORATIONS)
        && window_flags.contains(WindowFlags::RESIZABLE)
        && window_state.fullscreen.is_none()
        && !util::is_maximized(window).unwrap_or(false)
      {
        // cursor location
        let (cx, cy) = (
          util::GET_X_LPARAM(lparam) as i32,
          util::GET_Y_LPARAM(lparam) as i32,
        );

        let dpi = hwnd_dpi(window);
        let border_y = get_system_metrics_for_dpi(SM_CYFRAME, dpi);

        // if we have undecorated shadows, we only need to handle the top edge
        if window_flags.contains(WindowFlags::MARKER_UNDECORATED_SHADOW) {
          let rect = util::client_rect(window);
          let mut cursor_pt = POINT { x: cx, y: cy };
          if ScreenToClient(window, &mut cursor_pt).as_bool()
            && cursor_pt.y >= 0
            && cursor_pt.y <= border_y
            && cursor_pt.x >= 0
            && cursor_pt.x <= rect.right
          {
            result = ProcResult::Value(LRESULT(HTTOP as _));
          }
        }
        // otherwise do full hit testing
        else {
          let border_x = get_system_metrics_for_dpi(SM_CXFRAME, dpi);
          let rect = util::window_rect(window);
          let hit_result = crate::window::hit_test(
            (rect.left, rect.top, rect.right, rect.bottom),
            cx,
            cy,
            border_x,
            border_y,
          )
          .map(|d| d.to_win32());

          result = hit_result
            .map(|r| ProcResult::Value(LRESULT(r as _)))
            .unwrap_or(ProcResult::DefSubclassProc);
        }
      } else {
        result = ProcResult::DefSubclassProc;
      }
    }

    win32wm::WM_SYSCHAR => {
      // Handle system shortcut e.g. Alt+Space for window menu
      result = ProcResult::DefWindowProc;
    }

    _ => {
      if msg == *DESTROY_MSG_ID {
        let _ = DestroyWindow(window);
        result = ProcResult::Value(LRESULT(0));
      } else if msg == *SET_RETAIN_STATE_ON_SIZE_MSG_ID {
        let mut window_state = subclass_input.window_state.lock();
        window_state.set_window_flags_in_place(|f| {
          f.set(WindowFlags::MARKER_RETAIN_STATE_ON_SIZE, wparam.0 != 0)
        });
        result = ProcResult::Value(LRESULT(0));
      } else if msg == *CHANGE_THEME_MSG_ID {
        update_theme(subclass_input, window, false);
        result = ProcResult::Value(LRESULT(0));
      } else if msg == *S_U_TASKBAR_RESTART {
        // The Explorer COM call pumps messages and can re-enter this handler.
        // Backport tauri-apps/tao#1264: release the non-reentrant mutex first.
        let skip_taskbar = subclass_input.window_state.lock().skip_taskbar;
        let _ = set_skip_taskbar(window, skip_taskbar);
      }
    }
  };

  subclass_input
    .event_loop_runner
    .catch_unwind(callback)
    .unwrap_or_else(|| result = ProcResult::Value(LRESULT(-1)));

  match result {
    ProcResult::DefSubclassProc => DefSubclassProc(window, msg, wparam, lparam),
    ProcResult::DefWindowProc => DefWindowProcW(window, msg, wparam, lparam),
    ProcResult::Value(val) => val,
  }
}

fn update_theme<T>(
  subclass_input: &SubclassInput<T>,
  window: HWND,
  from_settings_change_event: bool,
) {
  let mut window_state = subclass_input.window_state.lock();
  let preferred_theme = window_state
    .preferred_theme
    .or(*subclass_input.event_loop_preferred_theme.lock());
  if from_settings_change_event && preferred_theme.is_some() {
    return;
  }
  let new_theme = try_window_theme(window, preferred_theme, !from_settings_change_event);
  if window_state.current_theme != new_theme {
    window_state.current_theme = new_theme;
    mem::drop(window_state);
    unsafe {
      subclass_input.send_event(Event::WindowEvent {
        window_id: RootWindowId(WindowId(window.0 as _)),
        event: WindowEvent::ThemeChanged(new_theme),
      })
    };
  }
}

fn is_show_window_contents_while_dragging_enabled() -> bool {
  let mut is_enabled: BOOL = BOOL(0);
  let result = unsafe {
    SystemParametersInfoW(
      SPI_GETDRAGFULLWINDOWS,
      0,
      Option::from(&mut is_enabled as *mut _ as *mut std::ffi::c_void),
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
  };
  result.is_ok() && is_enabled.0 != 0
}

unsafe extern "system" fn thread_event_target_callback<T: 'static>(
  window: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  _: usize,
  subclass_input_ptr: usize,
) -> LRESULT {
  let subclass_input = Box::from_raw(subclass_input_ptr as *mut ThreadMsgTargetSubclassInput<T>);

  let mut subclass_removed = false;

  // I decided to bind the closure to `callback` and pass it to catch_unwind rather than passing
  // the closure to catch_unwind directly so that the match body indendation wouldn't change and
  // the git blame and history would be preserved.
  let callback = || match msg {
    win32wm::WM_NCDESTROY => {
      remove_event_target_window_subclass::<T>(window);
      subclass_removed = true;
      let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      LRESULT(0)
    }
    // Because WM_PAINT comes after all other messages, we use it during modal loops to detect
    // when the event queue has been emptied. See `process_event` for more details.
    win32wm::WM_PAINT => {
      let _ = ValidateRect(Some(window), None);
      // If the WM_PAINT handler in `public_window_callback` has already flushed the redraw
      // events, `handling_events` will return false and we won't emit a second
      // `RedrawEventsCleared` event.
      if subclass_input.event_loop_runner.handling_events() {
        if subclass_input.event_loop_runner.should_buffer() {
          // This branch can be triggered when a nested win32 event loop is triggered
          // inside of the `event_handler` callback.
          let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
        } else {
          // This WM_PAINT handler will never be re-entrant because `flush_paint_messages`
          // doesn't call WM_PAINT for the thread event target (i.e. this window).
          assert!(flush_paint_messages(
            None,
            &subclass_input.event_loop_runner
          ));
          subclass_input.event_loop_runner.redraw_events_cleared();
          process_control_flow(&subclass_input.event_loop_runner);
        }
      }

      // Default WM_PAINT behaviour. This makes sure modals and popups are shown immediatly when opening them.
      DefSubclassProc(window, msg, wparam, lparam)
    }

    win32wm::WM_INPUT_DEVICE_CHANGE => {
      let event = match wparam.0 as u32 {
        win32wm::GIDC_ARRIVAL => DeviceEvent::Added,
        win32wm::GIDC_REMOVAL => DeviceEvent::Removed,
        _ => unreachable!(),
      };

      subclass_input.send_event(Event::DeviceEvent {
        device_id: wrap_device_id(lparam.0),
        event,
      });
      let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);

      LRESULT(0)
    }

    win32wm::WM_INPUT => {
      if let Some(data) = raw_input::get_raw_input_data(HRAWINPUT(lparam.0 as _)) {
        handle_raw_input(&subclass_input, data);
        let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      }

      DefSubclassProc(window, msg, wparam, lparam)
    }

    // We don't process `WM_QUERYENDSESSION` yet until we introduce the same mechanism as Tauri's `ExitRequested` event
    // win32wm::WM_QUERYENDSESSION => {}
    win32wm::WM_ENDSESSION => {
      // `wParam` is `FALSE` is for if the shutdown gets canceled,
      // and we don't need to handle that case since we didn't do anything prior in response to `WM_QUERYENDSESSION`
      if wparam.0 == TRUE.0 as usize {
        subclass_input.event_loop_runner.loop_destroyed();
      }
      // Note: after we return 0 here, Windows will shut us down
      LRESULT(0)
    }

    _ if msg == *USER_EVENT_MSG_ID => {
      if let Ok(event) = subclass_input.user_event_receiver.recv() {
        subclass_input.send_event(Event::UserEvent(event));
      }
      let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      LRESULT(0)
    }
    _ if msg == *EXEC_MSG_ID => {
      let mut function: ThreadExecFn = Box::from_raw(wparam.0 as *mut _);
      function();
      let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      LRESULT(0)
    }
    _ if msg == *PROCESS_NEW_EVENTS_MSG_ID => {
      let _ = PostThreadMessageW(
        subclass_input.event_loop_runner.wait_thread_id(),
        *CANCEL_WAIT_UNTIL_MSG_ID,
        WPARAM(0),
        LPARAM(0),
      );

      // if the control_flow is WaitUntil, make sure the given moment has actually passed
      // before emitting NewEvents
      if let ControlFlow::WaitUntil(wait_until) = subclass_input.event_loop_runner.control_flow() {
        let mut msg = MSG::default();
        while Instant::now() < wait_until {
          if PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE).as_bool() {
            // This works around a "feature" in PeekMessageW. If the message PeekMessageW
            // gets is a WM_PAINT message that had RDW_INTERNALPAINT set (i.e. doesn't
            // have an update region), PeekMessageW will remove that window from the
            // redraw queue even though we told it not to remove messages from the
            // queue. We fix it by re-dispatching an internal paint message to that
            // window.
            if msg.message == WM_PAINT {
              let mut rect = RECT::default();
              if !GetUpdateRect(msg.hwnd, Some(&mut rect), false).as_bool() {
                let _ = RedrawWindow(Some(msg.hwnd), None, None, RDW_INTERNALPAINT);
              }
            }

            break;
          }
        }
      }
      subclass_input.event_loop_runner.poll();
      let _ = RedrawWindow(Some(window), None, None, RDW_INTERNALPAINT);
      LRESULT(0)
    }
    _ => DefSubclassProc(window, msg, wparam, lparam),
  };

  let result = subclass_input
    .event_loop_runner
    .catch_unwind(callback)
    .unwrap_or(LRESULT(-1));
  if subclass_removed {
    mem::drop(subclass_input);
  } else {
    // FIXME: this seems to leak intentionally?
    #[allow(unused_must_use)]
    Box::into_raw(subclass_input);
  }
  result
}

unsafe fn handle_raw_input<T: 'static>(
  subclass_input: &ThreadMsgTargetSubclassInput<T>,
  data: RAWINPUT,
) {
  use crate::event::{
    DeviceEvent::{Button, Key, Motion, MouseMotion, MouseWheel},
    ElementState::{Pressed, Released},
    MouseScrollDelta::LineDelta,
  };

  let device_id = wrap_device_id(data.header.hDevice.0 as _);

  if data.header.dwType == RIM_TYPEMOUSE.0 {
    let mouse = data.data.mouse;

    if util::has_flag(mouse.usFlags.0, MOUSE_MOVE_RELATIVE.0) {
      let x = mouse.lLastX as f64;
      let y = mouse.lLastY as f64;

      if x != 0.0 {
        subclass_input.send_event(Event::DeviceEvent {
          device_id,
          event: Motion { axis: 0, value: x },
        });
      }

      if y != 0.0 {
        subclass_input.send_event(Event::DeviceEvent {
          device_id,
          event: Motion { axis: 1, value: y },
        });
      }

      if x != 0.0 || y != 0.0 {
        subclass_input.send_event(Event::DeviceEvent {
          device_id,
          event: MouseMotion { delta: (x, y) },
        });
      }
    }

    if util::has_flag(
      mouse.Anonymous.Anonymous.usButtonFlags,
      RI_MOUSE_WHEEL as u16,
    ) {
      // We must cast to SHORT first, becaues `usButtonData` must be interpreted as signed.
      let delta = mouse.Anonymous.Anonymous.usButtonData as i16 as f32 / WHEEL_DELTA as f32;
      subclass_input.send_event(Event::DeviceEvent {
        device_id,
        event: MouseWheel {
          delta: LineDelta(0.0, delta),
        },
      });
    }

    let button_state =
      raw_input::get_raw_mouse_button_state(mouse.Anonymous.Anonymous.usButtonFlags);
    // Left, middle, and right, respectively.
    for (index, state) in button_state.iter().enumerate() {
      if let Some(state) = *state {
        // This gives us consistency with X11, since there doesn't
        // seem to be anything else reasonable to do for a mouse
        // button ID.
        let button = (index + 1) as _;
        subclass_input.send_event(Event::DeviceEvent {
          device_id,
          event: Button { button, state },
        });
      }
    }
  } else if data.header.dwType == RIM_TYPEKEYBOARD.0 {
    let keyboard = data.data.keyboard;

    let pressed = keyboard.Message == WM_KEYDOWN || keyboard.Message == WM_SYSKEYDOWN;
    let released = keyboard.Message == WM_KEYUP || keyboard.Message == WM_SYSKEYUP;

    if !pressed && !released {
      return;
    }

    let state = if pressed { Pressed } else { Released };
    let extension = {
      if util::has_flag(keyboard.Flags, RI_KEY_E0 as _) {
        0xE000
      } else if util::has_flag(keyboard.Flags, RI_KEY_E1 as _) {
        0xE100
      } else {
        0x0000
      }
    };
    let scancode = if keyboard.MakeCode == 0 {
      // In some cases (often with media keys) the device reports a scancode of 0 but a
      // valid virtual key. In these cases we obtain the scancode from the virtual key.
      MapVirtualKeyW(keyboard.VKey as u32, MAPVK_VK_TO_VSC_EX) as u16
    } else {
      keyboard.MakeCode | extension
    };
    if scancode == 0xE11D || scancode == 0xE02A {
      // At the hardware (or driver?) level, pressing the Pause key is equivalent to pressing
      // Ctrl+NumLock.
      // This equvalence means that if the user presses Pause, the keyboard will emit two
      // subsequent keypresses:
      // 1, 0xE11D - Which is a left Ctrl (0x1D) with an extension flag (0xE100)
      // 2, 0x0045 - Which on its own can be interpreted as Pause
      //
      // There's another combination which isn't quite an equivalence:
      // PrtSc used to be Shift+Asterisk. This means that on some keyboards, presssing
      // PrtSc (print screen) produces the following sequence:
      // 1, 0xE02A - Which is a left shift (0x2A) with an extension flag (0xE000)
      // 2, 0xE037 - Which is a numpad multiply (0x37) with an exteion flag (0xE000). This on
      //             its own it can be interpreted as PrtSc
      //
      // For this reason, if we encounter the first keypress, we simply ignore it, trusting
      // that there's going to be another event coming, from which we can extract the
      // appropriate key.
      // For more on this, read the article by Raymond Chen, titled:
      // "Why does Ctrl+ScrollLock cancel dialogs?"
      // https://devblogs.microsoft.com/oldnewthing/20080211-00/?p=23503
      return;
    }
    let code = if VIRTUAL_KEY(keyboard.VKey) == VK_NUMLOCK {
      // Historically, the NumLock and the Pause key were one and the same physical key.
      // The user could trigger Pause by pressing Ctrl+NumLock.
      // Now these are often physically separate and the two keys can be differentiated by
      // checking the extension flag of the scancode. NumLock is 0xE045, Pause is 0x0045.
      //
      // However in this event, both keys are reported as 0x0045 even on modern hardware.
      // Therefore we use the virtual key instead to determine whether it's a NumLock and
      // set the KeyCode accordingly.
      //
      // For more on this, read the article by Raymond Chen, titled:
      // "Why does Ctrl+ScrollLock cancel dialogs?"
      // https://devblogs.microsoft.com/oldnewthing/20080211-00/?p=23503
      KeyCode::NumLock
    } else {
      KeyCode::from_scancode(scancode as u32)
    };
    if VIRTUAL_KEY(keyboard.VKey) == VK_SHIFT {
      match code {
        KeyCode::NumpadDecimal
        | KeyCode::Numpad0
        | KeyCode::Numpad1
        | KeyCode::Numpad2
        | KeyCode::Numpad3
        | KeyCode::Numpad4
        | KeyCode::Numpad5
        | KeyCode::Numpad6
        | KeyCode::Numpad7
        | KeyCode::Numpad8
        | KeyCode::Numpad9 => {
          // On Windows, holding the Shift key makes numpad keys behave as if NumLock
          // wasn't active. The way this is exposed to applications by the system is that
          // the application receives a fake key release event for the shift key at the
          // moment when the numpad key is pressed, just before receiving the numpad key
          // as well.
          //
          // The issue is that in the raw device event (here), the fake shift release
          // event reports the numpad key as the scancode. Unfortunately, the event doesn't
          // have any information to tell whether it's the left shift or the right shift
          // that needs to get the fake release (or press) event so we don't forward this
          // event to the application at all.
          //
          // For more on this, read the article by Raymond Chen, titled:
          // "The shift key overrides NumLock"
          // https://devblogs.microsoft.com/oldnewthing/20040906-00/?p=37953
          return;
        }
        _ => (),
      }
    }
    subclass_input.send_event(Event::DeviceEvent {
      device_id,
      event: Key(RawKeyEvent {
        physical_key: code,
        state,
      }),
    });
  }
}
