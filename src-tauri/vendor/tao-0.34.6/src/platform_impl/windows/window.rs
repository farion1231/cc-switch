// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "windows")]

use mem::MaybeUninit;
use parking_lot::Mutex;
use std::{
  cell::{Cell, RefCell},
  ffi::OsStr,
  io, mem,
  os::windows::ffi::OsStrExt,
  sync::Arc,
};

use crossbeam_channel as channel;
use windows::{
  core::PCWSTR,
  Win32::{
    Foundation::{
      self as win32f, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, POINTS, RECT, WPARAM,
    },
    Graphics::{
      Dwm::{DwmEnableBlurBehindWindow, DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND},
      Gdi::*,
    },
    System::{Com::*, LibraryLoader::*, Ole::*},
    UI::{
      Input::{Ime::*, KeyboardAndMouse::*, Touch::*},
      Shell::{ITaskbarList4 as ITaskbarList, TaskbarList, *},
      WindowsAndMessaging::{self as win32wm, *},
    },
  },
};

use crate::{
  dpi::{PhysicalPosition, PhysicalSize, Position, Size},
  error::{ExternalError, NotSupportedError, OsError as RootOsError},
  icon::Icon,
  monitor::MonitorHandle as RootMonitorHandle,
  platform_impl::platform::{
    dark_mode::try_window_theme,
    dpi::{dpi_to_scale_factor, hwnd_dpi},
    drop_handler::FileDropHandler,
    event_loop::{self, EventLoopWindowTarget, DESTROY_MSG_ID},
    icon::{self, IconType},
    monitor::{self},
    util,
    window_state::{CursorFlags, SavedWindow, WindowFlags, WindowState},
    OsError, Parent, PlatformSpecificWindowBuilderAttributes, WindowId,
  },
  window::{
    CursorIcon, Fullscreen, ProgressBarState, ProgressState, ResizeDirection, Theme,
    UserAttentionType, WindowAttributes, WindowSizeConstraints, RGBA,
  },
};

use super::{
  event_loop::CHANGE_THEME_MSG_ID,
  keyboard::{KeyEventBuilder, KEY_EVENT_BUILDERS},
  util::calculate_insets_for_dpi,
};

/// A simple non-owning wrapper around a window.
#[derive(Clone, Copy)]
pub struct WindowWrapper(pub HWND);

// Send and Sync are not implemented for HWND and HDC, we have to wrap it and implement them manually.
// For more info see:
// https://github.com/retep998/winapi-rs/issues/360
// https://github.com/retep998/winapi-rs/issues/396
unsafe impl Sync for WindowWrapper {}
unsafe impl Send for WindowWrapper {}

/// The Win32 implementation of the main `Window` object.
pub struct Window {
  /// Main handle for the window.
  window: WindowWrapper,

  /// The current window state.
  window_state: Arc<Mutex<WindowState>>,

  // The events loop proxy.
  thread_executor: event_loop::EventLoopThreadExecutor,
}

impl Window {
  pub fn new<T: 'static>(
    event_loop: &EventLoopWindowTarget<T>,
    w_attr: WindowAttributes,
    pl_attr: PlatformSpecificWindowBuilderAttributes,
  ) -> Result<Window, RootOsError> {
    // We dispatch an `init` function because of code style.
    // First person to remove the need for cloning here gets a cookie!
    //
    // done. you owe me -- ossi
    unsafe {
      let drag_and_drop = pl_attr.drag_and_drop;
      init(w_attr, pl_attr, event_loop).map(|win| {
        let file_drop_handler = if drag_and_drop {
          // It is ok if the initialize result is `S_FALSE` because it might happen that
          // multiple windows are created on the same thread.
          if let Err(error) = OleInitialize(None) {
            match error.code() {
              win32f::OLE_E_WRONGCOMPOBJ => {
                panic!("OleInitialize failed! Result was: `OLE_E_WRONGCOMPOBJ`")
              }
              win32f::RPC_E_CHANGED_MODE => panic!(
                "OleInitialize failed! Result was: `RPC_E_CHANGED_MODE`. \
                Make sure other crates are not using multithreaded COM library \
                on the same thread or disable drag and drop support."
              ),
              _ => (),
            };
          }

          let file_drop_runner = event_loop.runner_shared.clone();
          let file_drop_handler: IDropTarget = FileDropHandler::new(
            win.window.0,
            Box::new(move |event| {
              if let Ok(e) = event.map_nonuser_event() {
                file_drop_runner.send_event(e)
              }
            }),
          )
          .into();

          assert!(RegisterDragDrop(win.window.0, &file_drop_handler).is_ok());
          Some(file_drop_handler)
        } else {
          None
        };

        let subclass_input = event_loop::SubclassInput {
          window_state: win.window_state.clone(),
          event_loop_runner: event_loop.runner_shared.clone(),
          _file_drop_handler: file_drop_handler,
          subclass_removed: Cell::new(false),
          recurse_depth: Cell::new(0),
          event_loop_preferred_theme: event_loop.preferred_theme.clone(),
        };

        event_loop::subclass_window(win.window.0, subclass_input);
        win
      })
    }
  }

  pub fn set_title(&self, text: &str) {
    let text = util::encode_wide(text);
    unsafe {
      let _ = SetWindowTextW(self.window.0, PCWSTR::from_raw(text.as_ptr()));
    }
  }

  pub fn title(&self) -> String {
    let len = unsafe { GetWindowTextLengthW(self.window.0) };
    let mut buf = vec![0; (len + 1) as usize];
    unsafe { GetWindowTextW(self.window.0, &mut buf) };
    String::from_utf16_lossy(&buf[..len as _])
  }
  #[inline]
  pub fn set_visible(&self, visible: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::VISIBLE, visible)
      });
    });
  }

  #[inline]
  pub fn set_focus(&self) {
    let window = self.window;
    let window_flags = self.window_state.lock().window_flags();

    let is_visible = window_flags.contains(WindowFlags::VISIBLE);
    let is_minimized = window_flags.contains(WindowFlags::MINIMIZED);
    let is_foreground = window.0 == unsafe { GetForegroundWindow() };

    if is_visible && !is_minimized && !is_foreground {
      unsafe { force_window_active(window.0) };
    }
  }

  #[inline]
  pub fn set_focusable(&self, focusable: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::FOCUSABLE, focusable)
      });
    });
  }

  #[inline]
  pub fn is_focused(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.has_active_focus()
  }

  #[inline]
  pub fn request_redraw(&self) {
    unsafe {
      let _ = RedrawWindow(Some(self.window.0), None, None, RDW_INTERNALPAINT);
    }
  }

  #[inline]
  pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    unsafe { util::get_window_rect(self.window.0) }
      .map(|rect| Ok(PhysicalPosition::new(rect.left, rect.top)))
      .expect("Unexpected GetWindowRect failure")
  }

  #[inline]
  pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
    let mut position = POINT::default();
    if !unsafe { ClientToScreen(self.window.0, &mut position) }.as_bool() {
      panic!("Unexpected ClientToScreen failure")
    }
    Ok(PhysicalPosition::new(position.x, position.y))
  }

  #[inline]
  pub fn set_outer_position(&self, position: Position) {
    let (x, y): (i32, i32) = position.to_physical::<i32>(self.scale_factor()).into();

    let window_state = Arc::clone(&self.window_state);
    let window = self.window.0 .0 as isize;
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MAXIMIZED, false)
      });
    });

    unsafe {
      let _ = SetWindowPos(
        self.window.0,
        None,
        x,
        y,
        0,
        0,
        SWP_ASYNCWINDOWPOS | SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE,
      );
      let _ = InvalidateRgn(self.window.0, None, false);
    }
  }

  #[inline]
  pub fn inner_size(&self) -> PhysicalSize<u32> {
    let client_rect = util::client_rect(self.hwnd());
    PhysicalSize::new(
      (client_rect.right - client_rect.left) as u32,
      (client_rect.bottom - client_rect.top) as u32,
    )
  }

  #[inline]
  pub fn outer_size(&self) -> PhysicalSize<u32> {
    let window_rect = util::window_rect(self.hwnd());
    PhysicalSize::new(
      (window_rect.right - window_rect.left) as u32,
      (window_rect.bottom - window_rect.top) as u32,
    )
  }

  #[inline]
  pub fn set_inner_size(&self, size: Size) {
    let scale_factor = self.scale_factor();

    let (mut desired_width, mut desired_height) = size.to_physical::<i32>(scale_factor).into();

    let window_flags = self.window_state.lock().window_flags;

    // undecorated windows with shadows have hidden offsets
    // we need to calculate them and account for them in new size
    //
    // implementation derived from GPUI
    // see <https://github.com/zed-industries/zed/blob/7bddb390cabefb177d9996dc580749d64e6ca3b6/crates/gpui/src/platform/windows/window.rs#L1167-L1180>
    if window_flags.undecorated_with_shadows() {
      let hwnd = self.hwnd();

      let client_rect = util::client_rect(hwnd);
      let window_rect = util::window_rect(hwnd);

      let width_offset =
        (window_rect.right - window_rect.left) - (client_rect.right - client_rect.left);
      let height_offset =
        (window_rect.bottom - window_rect.top) - (client_rect.bottom - client_rect.top);

      desired_width += width_offset;
      desired_height += height_offset;
    }

    let window_state = Arc::clone(&self.window_state);
    let window = self.window.0 .0 as isize;
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MAXIMIZED, false)
      });
    });

    util::set_inner_size_physical(
      self.window.0,
      desired_width,
      desired_height,
      window_flags.contains(WindowFlags::MARKER_DECORATIONS),
    );
  }

  #[inline]
  pub fn set_min_inner_size(&self, size: Option<Size>) {
    let (width, height) = size.map(crate::extract_width_height).unzip();

    {
      let mut window_state = self.window_state.lock();
      window_state.size_constraints.min_width = width;
      window_state.size_constraints.min_height = height;
    }

    // Make windows re-check the window size bounds.
    let size = self.inner_size();
    self.set_inner_size(size.into());
  }

  #[inline]
  pub fn set_max_inner_size(&self, size: Option<Size>) {
    let (width, height) = size.map(crate::extract_width_height).unzip();

    {
      let mut window_state = self.window_state.lock();
      window_state.size_constraints.max_width = width;
      window_state.size_constraints.max_height = height;
    }

    // Make windows re-check the window size bounds.
    let size = self.inner_size();
    self.set_inner_size(size.into());
  }

  #[inline]
  pub fn set_inner_size_constraints(&self, constraints: WindowSizeConstraints) {
    self.window_state.lock().size_constraints = constraints;
    // Make windows re-check the window size bounds.
    let size = self.inner_size();
    self.set_inner_size(size.into());
  }

  #[inline]
  pub fn set_resizable(&self, resizable: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::RESIZABLE, resizable)
      });
    });
  }

  #[inline]
  pub fn set_minimizable(&self, minimizable: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MINIMIZABLE, minimizable)
      });
    });
  }

  #[inline]
  pub fn set_maximizable(&self, maximizable: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MAXIMIZABLE, maximizable)
      });
    });
  }

  #[inline]
  pub fn set_closable(&self, closable: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::CLOSABLE, closable)
      });
    });
  }

  /// Returns the `hwnd` of this window.
  #[inline]
  pub fn hwnd(&self) -> HWND {
    self.window.0
  }

  #[inline]
  pub fn hinstance(&self) -> HMODULE {
    util::get_instance_handle()
  }

  #[cfg(feature = "rwh_04")]
  #[inline]
  pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
    let mut window_handle = rwh_04::Win32Handle::empty();
    window_handle.hwnd = self.window.0 .0 as *mut _;
    let hinstance = util::GetWindowLongPtrW(self.hwnd(), GWLP_HINSTANCE);
    window_handle.hinstance = hinstance as *mut _;
    rwh_04::RawWindowHandle::Win32(window_handle)
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
    let mut window_handle = rwh_05::Win32WindowHandle::empty();
    window_handle.hwnd = self.window.0 .0 as *mut _;
    let hinstance = util::GetWindowLongPtrW(self.hwnd(), GWLP_HINSTANCE);
    window_handle.hinstance = hinstance as *mut _;
    rwh_05::RawWindowHandle::Win32(window_handle)
  }

  #[cfg(feature = "rwh_05")]
  #[inline]
  pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
    rwh_05::RawDisplayHandle::Windows(rwh_05::WindowsDisplayHandle::empty())
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
    let mut window_handle = rwh_06::Win32WindowHandle::new(unsafe {
      // SAFETY: Handle will never be zero.
      let window = self.window.0 .0;
      std::num::NonZeroIsize::new_unchecked(window as _)
    });
    let hinstance = util::GetWindowLongPtrW(self.hwnd(), GWLP_HINSTANCE);
    window_handle.hinstance = std::num::NonZeroIsize::new(hinstance);
    Ok(rwh_06::RawWindowHandle::Win32(window_handle))
  }

  #[cfg(feature = "rwh_06")]
  #[inline]
  pub fn raw_display_handle_rwh_06(&self) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
    Ok(rwh_06::RawDisplayHandle::Windows(
      rwh_06::WindowsDisplayHandle::new(),
    ))
  }

  #[inline]
  pub fn set_cursor_icon(&self, cursor: CursorIcon) {
    self.window_state.lock().mouse.cursor = cursor;
    self.thread_executor.execute_in_thread(move || unsafe {
      let cursor = LoadCursorW(None, cursor.to_windows_cursor()).ok();
      SetCursor(cursor);
    });
  }

  #[inline]
  pub fn set_cursor_grab(&self, grab: bool) -> Result<(), ExternalError> {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    let (tx, rx) = channel::unbounded();

    self.thread_executor.execute_in_thread(move || {
      let result = window_state
        .lock()
        .mouse
        .set_cursor_flags(HWND(window as _), |f| f.set(CursorFlags::GRABBED, grab))
        .map_err(|e| ExternalError::Os(os_error!(OsError::IoError(e))));
      let _ = tx.send(result);
    });
    rx.recv().unwrap()
  }

  #[inline]
  pub fn set_cursor_visible(&self, visible: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    let (tx, rx) = channel::unbounded();

    self.thread_executor.execute_in_thread(move || {
      let result = window_state
        .lock()
        .mouse
        .set_cursor_flags(HWND(window as _), |f| f.set(CursorFlags::HIDDEN, !visible))
        .map_err(|e| e.to_string());
      let _ = tx.send(result);
    });
    rx.recv().unwrap().ok();
  }

  #[inline]
  pub fn cursor_position(&self) -> Result<PhysicalPosition<f64>, ExternalError> {
    util::cursor_position().map_err(Into::into)
  }

  #[inline]
  pub fn scale_factor(&self) -> f64 {
    self.window_state.lock().scale_factor
  }

  #[inline]
  pub fn set_cursor_position(&self, position: Position) -> Result<(), ExternalError> {
    let scale_factor = self.scale_factor();
    let (x, y) = position.to_physical::<i32>(scale_factor).into();

    let mut point = POINT { x, y };
    unsafe {
      if !ClientToScreen(self.window.0, &mut point).as_bool() {
        return Err(ExternalError::Os(os_error!(OsError::IoError(
          io::Error::last_os_error()
        ))));
      }
      SetCursorPos(point.x, point.y)
        .map_err(|e| ExternalError::Os(os_error!(OsError::IoError(e.into()))))
    }
  }

  fn handle_os_dragging(&self, wparam: WPARAM) -> Result<(), ExternalError> {
    let points = {
      let mut pos = unsafe { mem::zeroed() };
      unsafe { GetCursorPos(&mut pos)? };
      pos
    };
    let points = POINTS {
      x: points.x as i16,
      y: points.y as i16,
    };
    unsafe { ReleaseCapture()? };

    self.window_state.lock().dragging = true;

    unsafe {
      PostMessageW(
        Some(self.hwnd()),
        WM_NCLBUTTONDOWN,
        wparam,
        LPARAM(&points as *const _ as _),
      )?
    };

    Ok(())
  }

  #[inline]
  pub fn drag_window(&self) -> Result<(), ExternalError> {
    self.handle_os_dragging(WPARAM(HTCAPTION as _))
  }

  #[inline]
  pub fn drag_resize_window(&self, direction: ResizeDirection) -> Result<(), ExternalError> {
    self.handle_os_dragging(WPARAM(direction.to_win32() as _))
  }

  #[inline]
  pub fn set_ignore_cursor_events(&self, ignore: bool) -> Result<(), ExternalError> {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);
    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::IGNORE_CURSOR_EVENT, ignore)
      });
    });

    Ok(())
  }

  #[inline]
  pub fn id(&self) -> WindowId {
    WindowId(self.window.0 .0 as _)
  }

  #[inline]
  pub fn set_minimized(&self, minimized: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    let is_minimized = self.is_minimized();

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags_in_place(&mut window_state.lock(), |f| {
        f.set(WindowFlags::MINIMIZED, is_minimized)
      });
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MINIMIZED, minimized)
      });
    });
  }

  #[inline]
  pub fn set_maximized(&self, maximized: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MAXIMIZED, maximized)
      });
    });
  }

  #[inline]
  pub fn is_maximized(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.window_flags.contains(WindowFlags::MAXIMIZED)
  }

  #[inline]
  pub fn is_always_on_top(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state
      .window_flags
      .contains(WindowFlags::ALWAYS_ON_TOP)
  }

  #[inline]
  pub fn is_minimized(&self) -> bool {
    unsafe { IsIconic(self.hwnd()) }.as_bool()
  }

  #[inline]
  pub fn is_resizable(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.window_flags.contains(WindowFlags::RESIZABLE)
  }

  #[inline]
  pub fn is_minimizable(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.window_flags.contains(WindowFlags::MINIMIZABLE)
  }

  #[inline]
  pub fn is_maximizable(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.window_flags.contains(WindowFlags::MAXIMIZABLE)
  }

  #[inline]
  pub fn is_closable(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state.window_flags.contains(WindowFlags::CLOSABLE)
  }

  #[inline]
  pub fn is_decorated(&self) -> bool {
    let window_state = self.window_state.lock();
    window_state
      .window_flags
      .contains(WindowFlags::MARKER_DECORATIONS)
  }

  #[inline]
  pub fn is_visible(&self) -> bool {
    util::is_visible(self.window.0)
  }

  #[inline]
  pub fn fullscreen(&self) -> Option<Fullscreen> {
    let window_state = self.window_state.lock();
    window_state.fullscreen.clone()
  }

  #[inline]
  pub fn set_fullscreen(&self, fullscreen: Option<Fullscreen>) {
    let window = self.window;
    let window_state = Arc::clone(&self.window_state);

    let mut window_state_lock = window_state.lock();
    let old_fullscreen = window_state_lock.fullscreen.clone();

    match (&old_fullscreen, &fullscreen) {
      // Return if we already in the same fullscreen mode
      _ if old_fullscreen == fullscreen => return,
      // Return if saved Borderless(monitor) is the same as current monitor when requested fullscreen is Borderless(None)
      (Some(Fullscreen::Borderless(Some(monitor))), Some(Fullscreen::Borderless(None)))
        if monitor.inner == monitor::current_monitor(window.0) =>
      {
        return
      }
      _ => {}
    }

    window_state_lock.fullscreen = fullscreen.clone();
    drop(window_state_lock);

    let window_isize = window.0 .0 as isize;
    self.thread_executor.execute_in_thread(move || {
      let hwnd = HWND(window_isize as _);
      // Change video mode if we're transitioning to or from exclusive
      // fullscreen
      match (&old_fullscreen, &fullscreen) {
        (&None, &Some(Fullscreen::Exclusive(ref video_mode)))
        | (&Some(Fullscreen::Borderless(_)), &Some(Fullscreen::Exclusive(ref video_mode)))
        | (&Some(Fullscreen::Exclusive(_)), &Some(Fullscreen::Exclusive(ref video_mode))) => {
          let monitor = video_mode.monitor();

          let mut display_name = OsStr::new(&monitor.inner.native_identifier())
            .encode_wide()
            .collect::<Vec<_>>();
          // `encode_wide` does not add a null-terminator but
          // `ChangeDisplaySettingsExW` requires a null-terminated
          // string, so add it
          display_name.push(0);

          let native_video_mode = video_mode.video_mode.native_video_mode;

          let res = unsafe {
            ChangeDisplaySettingsExW(
              PCWSTR::from_raw(display_name.as_ptr()),
              Some(&native_video_mode),
              None,
              CDS_FULLSCREEN,
              None,
            )
          };

          debug_assert!(res != DISP_CHANGE_BADFLAGS);
          debug_assert!(res != DISP_CHANGE_BADMODE);
          debug_assert!(res != DISP_CHANGE_BADPARAM);
          debug_assert!(res != DISP_CHANGE_FAILED);
          assert_eq!(res, DISP_CHANGE_SUCCESSFUL);
        }
        (&Some(Fullscreen::Exclusive(_)), &None)
        | (&Some(Fullscreen::Exclusive(_)), &Some(Fullscreen::Borderless(_))) => {
          let res =
            unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_FULLSCREEN, None) };

          debug_assert!(res != DISP_CHANGE_BADFLAGS);
          debug_assert!(res != DISP_CHANGE_BADMODE);
          debug_assert!(res != DISP_CHANGE_BADPARAM);
          debug_assert!(res != DISP_CHANGE_FAILED);
          assert_eq!(res, DISP_CHANGE_SUCCESSFUL);
        }
        _ => (),
      }

      unsafe {
        // There are some scenarios where calling `ChangeDisplaySettingsExW` takes long
        // enough to execute that the DWM thinks our program has frozen and takes over
        // our program's window. When that happens, the `SetWindowPos` call below gets
        // eaten and the window doesn't get set to the proper fullscreen position.
        //
        // Calling `PeekMessageW` here notifies Windows that our process is still running
        // fine, taking control back from the DWM and ensuring that the `SetWindowPos` call
        // below goes through.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
      }

      // Update window style
      WindowState::set_window_flags(window_state.lock(), HWND(window_isize as _), |f| {
        f.set(
          WindowFlags::MARKER_EXCLUSIVE_FULLSCREEN,
          matches!(fullscreen, Some(Fullscreen::Exclusive(_))),
        );
        f.set(
          WindowFlags::MARKER_BORDERLESS_FULLSCREEN,
          matches!(fullscreen, Some(Fullscreen::Borderless(_))),
        );
      });

      // Update window bounds
      match &fullscreen {
        Some(fullscreen) => {
          // Save window bounds before entering fullscreen
          let placement = unsafe {
            let mut placement = WINDOWPLACEMENT::default();
            let _ = GetWindowPlacement(hwnd, &mut placement);
            placement
          };

          window_state.lock().saved_window = Some(SavedWindow { placement });

          let monitor = match &fullscreen {
            Fullscreen::Exclusive(video_mode) => video_mode.monitor(),
            Fullscreen::Borderless(Some(monitor)) => monitor.clone(),
            Fullscreen::Borderless(None) => RootMonitorHandle {
              inner: monitor::current_monitor(hwnd),
            },
          };

          let position: (i32, i32) = monitor.position().into();
          let size: (u32, u32) = monitor.size().into();

          unsafe {
            let _ = SetWindowPos(
              hwnd,
              None,
              position.0,
              position.1,
              size.0 as i32,
              size.1 as i32,
              SWP_ASYNCWINDOWPOS | SWP_NOZORDER,
            );
            let _ = InvalidateRgn(hwnd, None, false);
          }
        }
        None => {
          let mut window_state_lock = window_state.lock();
          if let Some(SavedWindow { placement }) = window_state_lock.saved_window.take() {
            drop(window_state_lock);
            unsafe {
              let _ = SetWindowPlacement(hwnd, &placement);
              let _ = InvalidateRgn(hwnd, None, false);
            }
          }
        }
      }

      unsafe {
        taskbar_mark_fullscreen(hwnd, fullscreen.is_some());
      }
    });
  }

  #[inline]
  pub fn set_decorations(&self, decorations: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::MARKER_DECORATIONS, decorations)
      });
    });
  }

  #[inline]
  pub fn set_always_on_bottom(&self, always_on_bottom: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::ALWAYS_ON_BOTTOM, always_on_bottom)
      });
    });
  }

  #[inline]
  pub fn set_always_on_top(&self, always_on_top: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::ALWAYS_ON_TOP, always_on_top)
      });
    });
  }

  pub fn set_rtl(&self, rtl: bool) {
    let window = self.window.0 .0 as isize;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      WindowState::set_window_flags(window_state.lock(), HWND(window as _), |f| {
        f.set(WindowFlags::RIGHT_TO_LEFT_LAYOUT, rtl)
      });
    });
  }

  #[inline]
  pub fn current_monitor(&self) -> Option<RootMonitorHandle> {
    Some(RootMonitorHandle {
      inner: monitor::current_monitor(self.window.0),
    })
  }

  #[inline]
  pub fn set_window_icon(&self, window_icon: Option<Icon>) {
    if let Some(ref window_icon) = window_icon {
      window_icon
        .inner
        .set_for_window(self.window.0, IconType::Small);
    } else {
      icon::unset_for_window(self.window.0, IconType::Small);
    }
    self.window_state.lock().window_icon = window_icon;
  }

  #[inline]
  pub fn set_taskbar_icon(&self, taskbar_icon: Option<Icon>) {
    if let Some(ref taskbar_icon) = taskbar_icon {
      taskbar_icon
        .inner
        .set_for_window(self.window.0, IconType::Big);
    } else {
      icon::unset_for_window(self.window.0, IconType::Big);
    }
    self.window_state.lock().taskbar_icon = taskbar_icon;
  }

  pub(crate) fn set_ime_position_physical(&self, x: i32, y: i32) {
    if unsafe { GetSystemMetrics(SM_IMMENABLED) } != 0 {
      let composition_form = COMPOSITIONFORM {
        dwStyle: CFS_POINT,
        ptCurrentPos: POINT { x, y },
        rcArea: RECT::default(),
      };
      unsafe {
        let himc = ImmGetContext(self.window.0);
        let _ = ImmSetCompositionWindow(himc, &composition_form);
        let _ = ImmReleaseContext(self.window.0, himc);
      }
    }
  }

  #[inline]
  pub fn set_ime_position(&self, spot: Position) {
    let (x, y) = spot.to_physical::<i32>(self.scale_factor()).into();
    self.set_ime_position_physical(x, y);
  }

  #[inline]
  pub fn request_user_attention(&self, request_type: Option<UserAttentionType>) {
    let window = self.window;
    let active_window_handle = unsafe { GetActiveWindow() };
    if window.0 == active_window_handle {
      // active window could be minimized, so we skip requesting attention
      // if it is not minimized
      let window_flags = self.window_state.lock().window_flags();
      let is_minimized = window_flags.contains(WindowFlags::MINIMIZED);
      if !is_minimized {
        return;
      }
    }

    let window_isize = window.0 .0 as isize;

    self.thread_executor.execute_in_thread(move || unsafe {
      let (flags, count) = request_type
        .map(|ty| match ty {
          UserAttentionType::Critical => (FLASHW_ALL | FLASHW_TIMERNOFG, u32::MAX),
          UserAttentionType::Informational => (FLASHW_TRAY, 4),
        })
        .unwrap_or((FLASHW_STOP, 0));

      let flash_info = FLASHWINFO {
        cbSize: mem::size_of::<FLASHWINFO>() as u32,
        hwnd: HWND(window_isize as _),
        dwFlags: flags,
        uCount: count,
        dwTimeout: 0,
      };
      let _ = FlashWindowEx(&flash_info);
    });
  }

  #[inline]
  pub fn theme(&self) -> Theme {
    self.window_state.lock().current_theme
  }

  pub fn set_theme(&self, theme: Option<Theme>) {
    {
      let mut window_state = self.window_state.lock();
      if window_state.preferred_theme == theme {
        return;
      }
      window_state.preferred_theme = theme;
    }
    unsafe { SendMessageW(self.hwnd(), *CHANGE_THEME_MSG_ID, None, None) };
  }

  #[inline]
  pub fn reset_dead_keys(&self) {
    // `ToUnicode` consumes the dead-key by default, so we are constructing a fake (but valid)
    // key input which we can call `ToUnicode` with.
    unsafe {
      let vk = u32::from(VK_SPACE.0);
      let scancode = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
      let kbd_state = [0; 256];
      let mut char_buff: [MaybeUninit<u16>; 8] = [MaybeUninit::uninit(); 8];
      ToUnicode(
        vk,
        scancode,
        Some(&kbd_state),
        mem::transmute::<&mut [std::mem::MaybeUninit<u16>], &mut [u16]>(char_buff.as_mut()),
        0,
      );
    }
  }

  #[inline]
  pub fn begin_resize_drag(&self, edge: isize, button: u32, x: i32, y: i32) {
    unsafe {
      let w_param = WPARAM(edge as _);
      let l_param = util::MAKELPARAM(x as i16, y as i16);

      let _ = ReleaseCapture();
      let _ = PostMessageW(Some(self.hwnd()), button, w_param, l_param);
    }
  }

  #[inline]
  pub(crate) fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError> {
    self.window_state.lock().skip_taskbar = skip;
    unsafe { set_skip_taskbar(self.hwnd(), skip) }
  }

  #[inline]
  pub fn set_background_color(&self, color: Option<RGBA>) {
    self.window_state.lock().background_color = color;

    unsafe {
      let _ = InvalidateRect(Some(self.hwnd()), None, true);
      let _ = UpdateWindow(self.hwnd());
    }
  }

  #[inline]
  pub fn set_progress_bar(&self, progress: ProgressBarState) {
    unsafe {
      let taskbar_list: ITaskbarList = CoCreateInstance(&TaskbarList, None, CLSCTX_SERVER).unwrap();
      let handle = self.window.0;

      if let Some(state) = progress.state {
        let taskbar_state = {
          match state {
            ProgressState::None => TBPF_NOPROGRESS,
            ProgressState::Indeterminate => TBPF_INDETERMINATE,
            ProgressState::Normal => TBPF_NORMAL,
            ProgressState::Error => TBPF_ERROR,
            ProgressState::Paused => TBPF_PAUSED,
          }
        };

        taskbar_list
          .SetProgressState(handle, taskbar_state)
          .unwrap_or(());
      }
      if let Some(value) = progress.progress {
        let value = if value > 100 { 100 } else { value };

        taskbar_list
          .SetProgressValue(handle, value, 100)
          .unwrap_or(());
      }
    }
  }

  #[inline]
  pub fn set_overlay_icon(&self, icon: Option<&Icon>) {
    let taskbar: ITaskbarList =
      unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_SERVER).unwrap() };

    let icon = icon.map(|i| i.inner.as_raw_handle()).unwrap_or_default();

    unsafe {
      taskbar
        .SetOverlayIcon(self.window.0, icon, None)
        .unwrap_or(());
    }
  }

  #[inline]
  pub fn set_undecorated_shadow(&self, shadow: bool) {
    let window = self.window;
    let window_state = Arc::clone(&self.window_state);

    self.thread_executor.execute_in_thread(move || {
      let _ = &window;
      WindowState::set_window_flags(window_state.lock(), window.0, |f| {
        f.set(WindowFlags::MARKER_UNDECORATED_SHADOW, shadow)
      });
    });
  }

  #[inline]
  pub fn has_undecorated_shadow(&self) -> bool {
    self
      .window_state
      .lock()
      .window_flags
      .contains(WindowFlags::MARKER_UNDECORATED_SHADOW)
  }

  pub fn set_content_protection(&self, enabled: bool) {
    unsafe {
      let _ = SetWindowDisplayAffinity(
        self.hwnd(),
        if enabled {
          WDA_EXCLUDEFROMCAPTURE
        } else {
          WDA_NONE
        },
      );
    }
  }
}

impl Drop for Window {
  #[inline]
  fn drop(&mut self) {
    KEY_EVENT_BUILDERS.lock().remove(&self.id());
    unsafe {
      // The window must be destroyed from the same thread that created it, so we send a
      // custom message to be handled by our callback to do the actual work.
      let _ = PostMessageW(Some(self.window.0), *DESTROY_MSG_ID, WPARAM(0), LPARAM(0));
    }
  }
}

unsafe fn init<T: 'static>(
  attributes: WindowAttributes,
  pl_attribs: PlatformSpecificWindowBuilderAttributes,
  event_loop: &EventLoopWindowTarget<T>,
) -> Result<Window, RootOsError> {
  // registering the window class
  let class_name = register_window_class(&pl_attribs.window_classname);

  let mut window_flags = WindowFlags::empty();
  window_flags.set(WindowFlags::MARKER_DECORATIONS, attributes.decorations);
  window_flags.set(
    WindowFlags::MARKER_UNDECORATED_SHADOW,
    pl_attribs.decoration_shadow,
  );
  window_flags.set(WindowFlags::ALWAYS_ON_BOTTOM, attributes.always_on_bottom);
  window_flags.set(WindowFlags::ALWAYS_ON_TOP, attributes.always_on_top);
  window_flags.set(
    WindowFlags::NO_BACK_BUFFER,
    pl_attribs.no_redirection_bitmap,
  );
  window_flags.set(WindowFlags::TRANSPARENT, attributes.transparent);
  // WindowFlags::VISIBLE and MAXIMIZED are set down below after the window has been configured.
  window_flags.set(WindowFlags::RESIZABLE, attributes.resizable);
  window_flags.set(WindowFlags::MINIMIZABLE, attributes.minimizable);
  window_flags.set(WindowFlags::MAXIMIZABLE, attributes.maximizable);
  // will be changed later using `window.set_closable`
  // but we need to have a default for the diffing to work
  window_flags.set(WindowFlags::CLOSABLE, true);

  window_flags.set(WindowFlags::FOCUSABLE, attributes.focusable);

  window_flags.set(WindowFlags::MARKER_DONT_FOCUS, !attributes.focused);

  window_flags.set(WindowFlags::RIGHT_TO_LEFT_LAYOUT, pl_attribs.rtl);

  let parent = match pl_attribs.parent {
    Parent::ChildOf(parent) => {
      window_flags.set(WindowFlags::CHILD, true);
      if pl_attribs.menu.is_some() {
        warn!("Setting a menu on a child window is unsupported");
      }
      Some(parent)
    }
    Parent::OwnedBy(parent) => {
      window_flags.set(WindowFlags::POPUP, true);
      Some(parent)
    }
    Parent::None => {
      window_flags.set(WindowFlags::ON_TASKBAR, true);
      None
    }
  };

  // creating the real window this time, by using the functions in `extra_functions`
  let real_window = {
    let (style, ex_style) = window_flags.to_window_styles();
    let title = util::encode_wide(&attributes.title);

    let (target_monitor, position) = attributes
      .position
      .and_then(|p| {
        monitor::available_monitors()
          .into_iter()
          .find_map(|monitor| {
            let dpi = monitor.dpi();
            let scale_factor = dpi_to_scale_factor(dpi);
            let position = p.to_physical::<i32>(scale_factor);
            let (x, y): (i32, i32) = monitor.position().into();
            let (width, height): (i32, i32) = monitor.size().into();

            let frame_thickness = if window_flags.contains_shadow() {
              util::get_frame_thickness(dpi)
            } else {
              0
            };

            // Only the starting position x needs to be accounted
            if x <= position.x + frame_thickness
              && position.x <= x + width
              && y <= position.y
              && position.y <= y + height
            {
              Some((monitor, position.into()))
            } else {
              None
            }
          })
      })
      .unwrap_or_else(|| (monitor::primary_monitor(), (CW_USEDEFAULT, CW_USEDEFAULT)));

    let desired_size = attributes
      .inner_size
      .unwrap_or_else(|| PhysicalSize::new(800, 600).into());
    let clamped_size = attributes
      .inner_size_constraints
      .clamp(desired_size, target_monitor.scale_factor());

    // Best effort: try to create the window with the requested inner size
    let adjusted_size = {
      let (mut w, mut h): (i32, i32) = clamped_size
        .to_physical::<u32>(target_monitor.scale_factor())
        .into();

      if window_flags.contains(WindowFlags::MARKER_DECORATIONS) {
        let mut rect = RECT {
          left: 0,
          top: 0,
          right: w,
          bottom: h,
        };

        unsafe {
          AdjustWindowRectEx(
            &mut rect,
            window_flags.to_adjusted_window_styles().0,
            pl_attribs.menu.is_some(),
            ex_style,
          )?;
        }

        w = rect.right - rect.left;
        h = rect.bottom - rect.top;
      } else if window_flags.undecorated_with_shadows() {
        let dpi = target_monitor.dpi();
        let insets = calculate_insets_for_dpi(dpi);
        w += insets.left + insets.right;
        h += insets.top + insets.bottom;
      }

      (w, h)
    };

    let handle = CreateWindowExW(
      ex_style,
      PCWSTR::from_raw(class_name.as_ptr()),
      PCWSTR::from_raw(title.as_ptr()),
      style,
      position.0,
      position.1,
      adjusted_size.0,
      adjusted_size.1,
      parent,
      pl_attribs.menu,
      GetModuleHandleW(PCWSTR::null()).map(Into::into).ok(),
      Some(Box::into_raw(Box::new(window_flags)) as _),
    )?;

    if !IsWindow(Some(handle)).as_bool() {
      return Err(os_error!(OsError::IoError(io::Error::last_os_error())));
    }

    super::dark_mode::allow_dark_mode_for_window(handle, true);

    WindowWrapper(handle)
  };

  // Register for touch events if applicable
  {
    let digitizer = GetSystemMetrics(SM_DIGITIZER) as u32;
    if digitizer & NID_READY != 0 {
      RegisterTouchWindow(real_window.0, TWF_WANTPALM)?;
    }
  }

  let dpi = hwnd_dpi(real_window.0);
  let scale_factor = dpi_to_scale_factor(dpi);

  // making the window transparent
  if attributes.transparent && !pl_attribs.no_redirection_bitmap {
    // Empty region for the blur effect, so the window is fully transparent
    let region = CreateRectRgn(0, 0, -1, -1);

    let bb = DWM_BLURBEHIND {
      dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
      fEnable: true.into(),
      hRgnBlur: region,
      fTransitionOnMaximized: false.into(),
    };

    let _ = DwmEnableBlurBehindWindow(real_window.0, &bb);
    let _ = DeleteObject(region.into());
  }

  // If the system theme is dark, we need to set the window theme now
  // before we update the window flags (and possibly show the
  // window for the first time).
  let current_theme = try_window_theme(
    real_window.0,
    attributes
      .preferred_theme
      .or(*event_loop.preferred_theme.lock()),
    false,
  );

  let window_state = {
    let window_state = WindowState::new(
      &attributes,
      None,
      scale_factor,
      current_theme,
      attributes.preferred_theme,
      attributes.background_color,
    );
    let window_state = Arc::new(Mutex::new(window_state));
    WindowState::set_window_flags(window_state.lock(), real_window.0, |f| *f = window_flags);
    window_state
  };

  let win = Window {
    window: real_window,
    window_state,
    thread_executor: event_loop.create_thread_executor(),
  };

  KEY_EVENT_BUILDERS
    .lock()
    .insert(win.id(), KeyEventBuilder::default());

  let _ = win.set_skip_taskbar(pl_attribs.skip_taskbar);
  win.set_window_icon(attributes.window_icon);
  win.set_taskbar_icon(pl_attribs.taskbar_icon);

  if attributes.fullscreen.is_some() {
    win.set_fullscreen(attributes.fullscreen);
    force_window_active(win.window.0);
  } else if attributes.maximized {
    win.set_maximized(true);
  }

  if attributes.content_protection {
    win.set_content_protection(true);
  }

  win.set_visible(attributes.visible);
  win.set_closable(attributes.closable);

  Ok(win)
}

unsafe fn register_window_class(window_classname: &str) -> Vec<u16> {
  let class_name = util::encode_wide(window_classname);

  let class = WNDCLASSEXW {
    cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
    style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
    lpfnWndProc: Some(window_proc),
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

  // We ignore errors because registering the same window class twice would trigger
  //  an error, and because errors here are detected during CreateWindowEx anyway.
  // Also since there is no weird element in the struct, there is no reason for this
  //  call to fail.
  RegisterClassExW(&class);

  class_name
}

unsafe extern "system" fn window_proc(
  window: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
) -> LRESULT {
  // This window procedure is only needed until the subclass procedure is attached.
  // we need this because we need to respond to WM_NCCALCSIZE as soon as possible
  // in order to make the window borderless if needed.
  match msg {
    win32wm::WM_NCCALCSIZE => {
      let userdata = util::GetWindowLongPtrW(window, GWL_USERDATA);
      if userdata != 0 {
        let window_flags = WindowFlags::from_bits_truncate(userdata as _);

        if wparam == WPARAM(0) || window_flags.contains(WindowFlags::MARKER_DECORATIONS) {
          return DefWindowProcW(window, msg, wparam, lparam);
        }

        // adjust the maximized borderless window so it doesn't cover the taskbar
        if util::is_maximized(window).unwrap_or(false) {
          let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
          if let Ok(monitor_info) =
            monitor::get_monitor_info(MonitorFromRect(&params.rgrc[0], MONITOR_DEFAULTTONULL))
          {
            params.rgrc[0] = monitor_info.monitorInfo.rcWork;
          }
        } else if window_flags.contains(WindowFlags::MARKER_UNDECORATED_SHADOW) {
          let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);

          let insets = util::calculate_window_insets(window);

          params.rgrc[0].left += insets.left;
          params.rgrc[0].top += insets.top;
          params.rgrc[0].right -= insets.right;
          params.rgrc[0].bottom -= insets.bottom;
        }
        return LRESULT(0); // return 0 here to make the window borderless
      }

      DefWindowProcW(window, msg, wparam, lparam)
    }
    win32wm::WM_NCCREATE => {
      let userdata = util::GetWindowLongPtrW(window, GWL_USERDATA);
      if userdata == 0 {
        let createstruct = &*(lparam.0 as *const CREATESTRUCTW);
        let userdata = createstruct.lpCreateParams;
        let window_flags = Box::from_raw(userdata as *mut WindowFlags);
        util::SetWindowLongPtrW(window, GWL_USERDATA, window_flags.bits() as _);
      }
      DefWindowProcW(window, msg, wparam, lparam)
    }
    _ => DefWindowProcW(window, msg, wparam, lparam),
  }
}

struct ComInitialized(Option<()>);
impl Drop for ComInitialized {
  fn drop(&mut self) {
    if let Some(()) = self.0.take() {
      unsafe { CoUninitialize() };
    }
  }
}

thread_local! {
    static COM_INITIALIZED: ComInitialized = {
        unsafe {
            ComInitialized(match CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() {
              Ok(()) => Some(()),
              Err(_) => None,
            })
        }
    };

    static TASKBAR_LIST: RefCell<Option<ITaskbarList2>> = const { RefCell::new(None) };
}

pub fn com_initialized() {
  COM_INITIALIZED.with(|_| {});
}

// Reference Implementation:
// https://github.com/chromium/chromium/blob/f18e79d901f56154f80eea1e2218544285e62623/ui/views/win/fullscreen_handler.cc
//
// As per MSDN marking the window as fullscreen should ensure that the
// taskbar is moved to the bottom of the Z-order when the fullscreen window
// is activated. If the window is not fullscreen, the Shell falls back to
// heuristics to determine how the window should be treated, which means
// that it could still consider the window as fullscreen. :(
unsafe fn taskbar_mark_fullscreen(handle: HWND, fullscreen: bool) {
  com_initialized();

  TASKBAR_LIST.with(|task_bar_list_ptr| {
    let mut task_bar_list = task_bar_list_ptr.borrow().clone();

    if task_bar_list.is_none() {
      let result: windows::core::Result<ITaskbarList2> =
        CoCreateInstance(&TaskbarList, None, CLSCTX_ALL);
      if let Ok(created) = result {
        if let Ok(()) = created.HrInit() {
          task_bar_list = Some(created);
        }
      }

      if task_bar_list.is_none() {
        return;
      }

      *task_bar_list_ptr.borrow_mut() = task_bar_list.clone();
    }

    let _ = task_bar_list
      .unwrap()
      .MarkFullscreenWindow(handle, fullscreen);
  })
}

unsafe fn force_window_active(handle: HWND) {
  // Try to focus the window without the hack first.
  if SetForegroundWindow(handle).as_bool() {
    return;
  }

  // In some situations, calling SetForegroundWindow could not bring up the window,
  // This is a little hack which can "steal" the foreground window permission.
  // We only call this function in the window creation, so it should be fine.
  // See: https://stackoverflow.com/questions/10740346/setforegroundwindow-only-working-while-visual-studio-is-open
  let alt_sc = MapVirtualKeyW(u32::from(VK_MENU.0), MAPVK_VK_TO_VSC);

  let mut inputs: [INPUT; 2] = mem::zeroed();
  inputs[0].r#type = INPUT_KEYBOARD;
  inputs[0].Anonymous.ki.wVk = VK_LMENU as _;
  inputs[0].Anonymous.ki.wScan = alt_sc as _;
  inputs[0].Anonymous.ki.dwFlags = KEYEVENTF_EXTENDEDKEY;

  inputs[1].r#type = INPUT_KEYBOARD;
  inputs[1].Anonymous.ki.wVk = VK_LMENU as _;
  inputs[1].Anonymous.ki.wScan = alt_sc as _;
  inputs[1].Anonymous.ki.dwFlags = KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP;

  // Simulate a key press and release
  SendInput(&inputs, mem::size_of::<INPUT>() as _);

  let _ = SetForegroundWindow(handle);
}

pub(crate) unsafe fn set_skip_taskbar(hwnd: HWND, skip: bool) -> Result<(), ExternalError> {
  com_initialized();
  let taskbar_list: ITaskbarList = CoCreateInstance(&TaskbarList, None, CLSCTX_SERVER)?;
  if skip {
    taskbar_list.DeleteTab(hwnd)?;
  } else {
    taskbar_list.AddTab(hwnd)?;
  }

  Ok(())
}

impl ResizeDirection {
  pub(crate) fn to_win32(&self) -> u32 {
    match self {
      ResizeDirection::East => HTRIGHT,
      ResizeDirection::North => HTTOP,
      ResizeDirection::NorthEast => HTTOPRIGHT,
      ResizeDirection::NorthWest => HTTOPLEFT,
      ResizeDirection::South => HTBOTTOM,
      ResizeDirection::SouthEast => HTBOTTOMRIGHT,
      ResizeDirection::SouthWest => HTBOTTOMLEFT,
      ResizeDirection::West => HTLEFT,
    }
  }
}
