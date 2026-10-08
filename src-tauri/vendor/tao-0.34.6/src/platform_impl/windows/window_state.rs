// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use crate::{
  dpi::PhysicalPosition,
  icon::Icon,
  keyboard::ModifiersState,
  platform_impl::platform::{event_loop, minimal_ime::MinimalIme, util},
  window::{CursorIcon, Fullscreen, Theme, WindowAttributes, WindowSizeConstraints, RGBA},
};
use parking_lot::MutexGuard;
use std::io;
use windows::Win32::{
  Foundation::{HWND, LPARAM, RECT, WPARAM},
  Graphics::Gdi::InvalidateRgn,
  UI::WindowsAndMessaging::*,
};

/// Contains information about states and the window that the callback is going to use.
pub struct WindowState {
  pub mouse: MouseProperties,

  /// Used by `WM_GETMINMAXINFO`.
  pub size_constraints: WindowSizeConstraints,

  pub window_icon: Option<Icon>,
  pub taskbar_icon: Option<Icon>,

  pub saved_window: Option<SavedWindow>,
  pub scale_factor: f64,

  pub dragging: bool,

  pub skip_taskbar: bool,

  pub modifiers_state: ModifiersState,
  pub fullscreen: Option<Fullscreen>,
  pub current_theme: Theme,
  pub preferred_theme: Option<Theme>,

  pub ime_handler: MinimalIme,

  pub window_flags: WindowFlags,

  // Used by WM_NCACTIVATE, WM_SETFOCUS and WM_KILLFOCUS
  pub is_active: bool,
  pub is_focused: bool,

  pub background_color: Option<RGBA>,
}

unsafe impl Send for WindowState {}
unsafe impl Sync for WindowState {}

#[derive(Clone)]
pub struct SavedWindow {
  pub placement: WINDOWPLACEMENT,
}

#[derive(Clone)]
pub struct MouseProperties {
  pub cursor: CursorIcon,
  pub capture_count: u32,
  cursor_flags: CursorFlags,
  pub last_position: Option<PhysicalPosition<f64>>,
}

bitflags! {
    #[derive(Clone, Copy)]
    pub struct CursorFlags: u8 {
        const GRABBED   = 1 << 0;
        const HIDDEN    = 1 << 1;
        const IN_WINDOW = 1 << 2;
    }
}
bitflags! {
  #[derive(Clone, Copy, PartialEq)]
    pub struct WindowFlags: u32 {
        const RESIZABLE        = 1 << 0;
        const VISIBLE          = 1 << 1;
        const ON_TASKBAR       = 1 << 2;
        const ALWAYS_ON_TOP    = 1 << 3;
        const NO_BACK_BUFFER   = 1 << 4;
        const TRANSPARENT      = 1 << 5;
        const CHILD            = 1 << 6;
        const MAXIMIZED        = 1 << 7;
        const POPUP            = 1 << 8;
        const ALWAYS_ON_BOTTOM = 1 << 9;
        const MINIMIZABLE      = 1 << 10;
        const MAXIMIZABLE      = 1 << 11;
        const CLOSABLE         = 1 << 12;
        const MINIMIZED        = 1 << 13;

        const IGNORE_CURSOR_EVENT = 1 << 14;

        /// Marker flag for fullscreen. Should always match `WindowState::fullscreen`, but is
        /// included here to make masking easier.
        const MARKER_EXCLUSIVE_FULLSCREEN = 1 << 15;
        const MARKER_BORDERLESS_FULLSCREEN = 1 << 16;

        /// The `WM_SIZE` event contains some parameters that can effect the state of `WindowFlags`.
        /// In most cases, it's okay to let those parameters change the state. However, when we're
        /// running the `WindowFlags::apply_diff` function, we *don't* want those parameters to
        /// effect our stored state, because the purpose of `apply_diff` is to update the actual
        /// window's state to match our stored state. This controls whether to accept those changes.
        const MARKER_RETAIN_STATE_ON_SIZE = 1 << 17;

        const MARKER_IN_SIZE_MOVE = 1 << 18;

        const MARKER_DONT_FOCUS = 1 << 19;

        /// Fully decorated window (incl. caption, border and drop shadow).
        const MARKER_DECORATIONS = 1 << 20;
        /// Drop shadow for undecorated windows.
        const MARKER_UNDECORATED_SHADOW = 1 << 21;

        const RIGHT_TO_LEFT_LAYOUT = 1 << 22;

        const FOCUSABLE = 1 << 23;

        const EXCLUSIVE_FULLSCREEN_OR_MASK = WindowFlags::ALWAYS_ON_TOP.bits();
    }
}

impl WindowState {
  pub fn new(
    attributes: &WindowAttributes,
    taskbar_icon: Option<Icon>,
    scale_factor: f64,
    current_theme: Theme,
    preferred_theme: Option<Theme>,
    background_color: Option<RGBA>,
  ) -> WindowState {
    WindowState {
      mouse: MouseProperties {
        cursor: CursorIcon::default(),
        capture_count: 0,
        cursor_flags: CursorFlags::empty(),
        last_position: None,
      },

      size_constraints: attributes.inner_size_constraints,

      window_icon: attributes.window_icon.clone(),
      taskbar_icon,

      saved_window: None,
      scale_factor,

      dragging: false,

      skip_taskbar: false,

      modifiers_state: ModifiersState::default(),
      fullscreen: None,
      current_theme,
      preferred_theme,
      ime_handler: MinimalIme::default(),
      window_flags: WindowFlags::empty(),
      is_active: false,
      is_focused: false,

      background_color,
    }
  }

  pub fn window_flags(&self) -> WindowFlags {
    self.window_flags
  }

  pub fn set_window_flags<F>(mut this: MutexGuard<'_, Self>, window: HWND, f: F)
  where
    F: FnOnce(&mut WindowFlags),
  {
    let old_flags = this.window_flags;
    f(&mut this.window_flags);
    let new_flags = this.window_flags;

    drop(this);
    old_flags.apply_diff(window, new_flags);
  }

  pub fn set_window_flags_in_place<F>(&mut self, f: F)
  where
    F: FnOnce(&mut WindowFlags),
  {
    f(&mut self.window_flags);
  }

  pub fn has_active_focus(&self) -> bool {
    self.is_active && self.is_focused
  }

  // Updates is_active and returns whether active-focus state has changed
  pub fn set_active(&mut self, is_active: bool) -> bool {
    let old = self.has_active_focus();
    self.is_active = is_active;
    old != self.has_active_focus()
  }

  // Updates is_focused and returns whether active-focus state has changed
  pub fn set_focused(&mut self, is_focused: bool) -> bool {
    let old = self.has_active_focus();
    self.is_focused = is_focused;
    old != self.has_active_focus()
  }
}

impl MouseProperties {
  pub fn cursor_flags(&self) -> CursorFlags {
    self.cursor_flags
  }

  pub fn set_cursor_flags<F>(&mut self, window: HWND, f: F) -> Result<(), io::Error>
  where
    F: FnOnce(&mut CursorFlags),
  {
    let old_flags = self.cursor_flags;
    f(&mut self.cursor_flags);
    match self.cursor_flags.refresh_os_cursor(window) {
      Ok(()) => (),
      Err(e) => {
        self.cursor_flags = old_flags;
        return Err(e);
      }
    }

    Ok(())
  }
}

impl WindowFlags {
  fn mask(mut self) -> WindowFlags {
    if self.contains(WindowFlags::MARKER_EXCLUSIVE_FULLSCREEN) {
      self |= WindowFlags::EXCLUSIVE_FULLSCREEN_OR_MASK;
    }

    self
  }

  pub fn to_window_styles(self) -> (WINDOW_STYLE, WINDOW_EX_STYLE) {
    let (mut style, mut style_ex) = (Default::default(), Default::default());
    style |= WS_CAPTION | WS_CLIPSIBLINGS | WS_SYSMENU;
    style_ex |= WS_EX_WINDOWEDGE | WS_EX_ACCEPTFILES;
    if self.contains(WindowFlags::RESIZABLE) {
      style |= WS_SIZEBOX;
    }
    if self.contains(WindowFlags::MAXIMIZABLE) {
      style |= WS_MAXIMIZEBOX;
    }
    if self.contains(WindowFlags::MINIMIZABLE) {
      style |= WS_MINIMIZEBOX;
    }
    if self.contains(WindowFlags::VISIBLE) {
      style |= WS_VISIBLE;
    }
    if self.contains(WindowFlags::ON_TASKBAR) {
      style_ex |= WS_EX_APPWINDOW;
    }
    if self.contains(WindowFlags::ALWAYS_ON_TOP) {
      style_ex |= WS_EX_TOPMOST;
    }
    if self.contains(WindowFlags::NO_BACK_BUFFER) {
      style_ex |= WS_EX_NOREDIRECTIONBITMAP;
    }
    if self.contains(WindowFlags::CHILD) {
      style |= WS_CHILD; // This is incompatible with WS_POPUP if that gets added eventually.

      // Remove decorations window styles for child
      if !self.contains(WindowFlags::MARKER_DECORATIONS) {
        style &= !WS_CAPTION;
        style_ex &= !WS_EX_WINDOWEDGE;
      }
    }
    if self.contains(WindowFlags::POPUP) {
      style |= WS_POPUP;
    }
    if self.contains(WindowFlags::MINIMIZED) {
      style |= WS_MINIMIZE;
    }
    if self.contains(WindowFlags::MAXIMIZED) {
      style |= WS_MAXIMIZE;
    }
    if self.contains(WindowFlags::IGNORE_CURSOR_EVENT) {
      style_ex |= WS_EX_TRANSPARENT | WS_EX_LAYERED;
    }
    if self.intersects(
      WindowFlags::MARKER_EXCLUSIVE_FULLSCREEN | WindowFlags::MARKER_BORDERLESS_FULLSCREEN,
    ) {
      style &= !WS_OVERLAPPEDWINDOW;
    }
    if self.contains(WindowFlags::RIGHT_TO_LEFT_LAYOUT) {
      style_ex |= WS_EX_LAYOUTRTL | WS_EX_RTLREADING | WS_EX_RIGHT;
    }
    if !self.contains(WindowFlags::FOCUSABLE) {
      style_ex |= WS_EX_NOACTIVATE;
    }

    (style, style_ex)
  }

  /// Returns the appropriate window styles for `AdjustWindowRectEx`
  pub fn to_adjusted_window_styles(self) -> (WINDOW_STYLE, WINDOW_EX_STYLE) {
    let (mut style, style_ex) = self.to_window_styles();

    if !self.contains(WindowFlags::MARKER_DECORATIONS) {
      style &= !(WS_CAPTION | WS_THICKFRAME)
    }

    (style, style_ex)
  }

  /// Adjust the window client rectangle to the return value, if present.
  fn apply_diff(mut self, window: HWND, mut new: WindowFlags) {
    self = self.mask();
    new = new.mask();

    let mut diff = self ^ new;

    if diff == WindowFlags::empty() {
      return;
    }

    if new.contains(WindowFlags::VISIBLE) {
      unsafe {
        let _ = ShowWindow(
          window,
          if self.contains(WindowFlags::MARKER_DONT_FOCUS) {
            self.set(WindowFlags::MARKER_DONT_FOCUS, false);
            SW_SHOWNOACTIVATE
          } else {
            SW_SHOW
          },
        );
      }
    }

    if diff.contains(WindowFlags::ALWAYS_ON_TOP) {
      unsafe {
        let _ = SetWindowPos(
          window,
          Some(if new.contains(WindowFlags::ALWAYS_ON_TOP) {
            HWND_TOPMOST
          } else {
            HWND_NOTOPMOST
          }),
          0,
          0,
          0,
          0,
          SWP_ASYNCWINDOWPOS | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = InvalidateRgn(window, None, false);
      }
    }

    if diff.contains(WindowFlags::ALWAYS_ON_BOTTOM) {
      unsafe {
        let _ = SetWindowPos(
          window,
          Some(if new.contains(WindowFlags::ALWAYS_ON_BOTTOM) {
            HWND_BOTTOM
          } else {
            HWND_NOTOPMOST
          }),
          0,
          0,
          0,
          0,
          SWP_ASYNCWINDOWPOS | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = InvalidateRgn(window, None, false);
      }
    }

    if diff.contains(WindowFlags::MAXIMIZED) || new.contains(WindowFlags::MAXIMIZED) {
      unsafe {
        let _ = ShowWindow(
          window,
          match new.contains(WindowFlags::MAXIMIZED) {
            true => SW_MAXIMIZE,
            false => SW_RESTORE,
          },
        );
      }
    }

    // Minimize operations should execute after maximize for proper window animations
    if diff.contains(WindowFlags::MINIMIZED) {
      unsafe {
        let _ = ShowWindow(
          window,
          match new.contains(WindowFlags::MINIMIZED) {
            true => SW_MINIMIZE,
            false => SW_RESTORE,
          },
        );
      }

      diff.remove(WindowFlags::MINIMIZED);
    }

    if diff.contains(WindowFlags::CLOSABLE) || new.contains(WindowFlags::CLOSABLE) {
      unsafe {
        let system_menu = GetSystemMenu(window, false);
        let _ = EnableMenuItem(
          system_menu,
          SC_CLOSE,
          MF_BYCOMMAND
            | if new.contains(WindowFlags::CLOSABLE) {
              MF_ENABLED
            } else {
              MF_GRAYED
            },
        );
      }
    }

    if !new.contains(WindowFlags::VISIBLE) {
      unsafe {
        let _ = ShowWindow(window, SW_HIDE);
      }
    }

    if diff != WindowFlags::empty() {
      let (style, style_ex) = new.to_window_styles();

      unsafe {
        SendMessageW(
          window,
          *event_loop::SET_RETAIN_STATE_ON_SIZE_MSG_ID,
          Some(WPARAM(1)),
          Some(LPARAM(0)),
        );

        // This condition is necessary to avoid having an unrestorable window
        if !new.contains(WindowFlags::MINIMIZED) {
          SetWindowLongW(window, GWL_STYLE, style.0 as i32);
          SetWindowLongW(window, GWL_EXSTYLE, style_ex.0 as i32);
        }

        let mut flags = SWP_NOZORDER | SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED;

        // We generally don't want style changes here to affect window
        // focus, but for fullscreen windows they must be activated
        // (i.e. focused) so that they appear on top of the taskbar
        if !new.contains(WindowFlags::MARKER_EXCLUSIVE_FULLSCREEN)
          && !new.contains(WindowFlags::MARKER_BORDERLESS_FULLSCREEN)
        {
          flags |= SWP_NOACTIVATE;
        }

        // Refresh the window frame
        let _ = SetWindowPos(window, None, 0, 0, 0, 0, flags);
        SendMessageW(
          window,
          *event_loop::SET_RETAIN_STATE_ON_SIZE_MSG_ID,
          Some(WPARAM(0)),
          Some(LPARAM(0)),
        );
      }
    }
  }

  pub fn undecorated_with_shadows(&self) -> bool {
    self.contains(WindowFlags::MARKER_UNDECORATED_SHADOW)
      && !self.contains(WindowFlags::MARKER_DECORATIONS)
  }

  pub fn contains_shadow(&self) -> bool {
    self.contains(WindowFlags::MARKER_UNDECORATED_SHADOW)
      || self.contains(WindowFlags::MARKER_DECORATIONS)
  }
}

impl CursorFlags {
  fn refresh_os_cursor(self, window: HWND) -> Result<(), io::Error> {
    let client_rect = util::get_client_rect(window)?;

    if util::is_focused(window) {
      let cursor_clip = match self.contains(CursorFlags::GRABBED) {
        true => Some(client_rect),
        false => None,
      };

      let rect_to_tuple = |rect: RECT| (rect.left, rect.top, rect.right, rect.bottom);
      let active_cursor_clip = rect_to_tuple(util::get_cursor_clip()?);
      let desktop_rect = rect_to_tuple(util::get_desktop_rect());

      let active_cursor_clip = match desktop_rect == active_cursor_clip {
        true => None,
        false => Some(active_cursor_clip),
      };

      // We do this check because calling `set_cursor_clip` incessantly will flood the event
      // loop with `WM_MOUSEMOVE` events, and `refresh_os_cursor` is called by `set_cursor_flags`
      // which at times gets called once every iteration of the eventloop.
      if active_cursor_clip != cursor_clip.map(rect_to_tuple) {
        util::set_cursor_clip(cursor_clip)?;
      }
    }

    let cursor_in_client = self.contains(CursorFlags::IN_WINDOW);
    if cursor_in_client {
      util::set_cursor_hidden(self.contains(CursorFlags::HIDDEN));
    } else {
      util::set_cursor_hidden(false);
    }

    Ok(())
  }
}
