// Copyright 2014-2021 The tao contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "windows")]

use std::path::Path;

use crate::{
  dpi::PhysicalSize,
  error::ExternalError,
  event::DeviceId,
  event_loop::EventLoopBuilder,
  monitor::MonitorHandle,
  platform_impl::{Parent, WinIcon},
  window::{BadIcon, Icon, Theme, Window, WindowBuilder},
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub type HWND = isize;
pub type HMENU = isize;

/// Additional methods on `EventLoop` that are specific to Windows.
pub trait EventLoopBuilderExtWindows {
  /// Whether to allow the event loop to be created off of the main thread.
  ///
  /// By default, the window is only allowed to be created on the main
  /// thread, to make platform compatibility easier.
  ///
  /// # `Window` caveats
  ///
  /// Note that any `Window` created on the new thread will be destroyed when the thread
  /// terminates. Attempting to use a `Window` after its parent thread terminates has
  /// unspecified, although explicitly not undefined, behavior.
  fn with_any_thread(&mut self, any_thread: bool) -> &mut Self;

  /// Whether to enable process-wide DPI awareness.
  ///
  /// By default, `tao` will attempt to enable process-wide DPI awareness. If
  /// that's undesirable, you can disable it with this function.
  ///
  /// # Example
  ///
  /// Disable process-wide DPI awareness.
  ///
  /// ```
  /// use tao::event_loop::EventLoopBuilder;
  /// #[cfg(target_os = "windows")]
  /// use tao::platform::windows::EventLoopBuilderExtWindows;
  ///
  /// let mut builder = EventLoopBuilder::new();
  /// #[cfg(target_os = "windows")]
  /// builder.with_dpi_aware(false);
  /// # if false { // We can't test this part
  /// let event_loop = builder.build();
  /// # }
  /// ```
  fn with_dpi_aware(&mut self, dpi_aware: bool) -> &mut Self;

  /// A callback to be executed before dispatching a win32 message to the window procedure.
  /// Return true to disable tao's internal message dispatching.
  ///
  /// # Example
  ///
  /// ```
  /// # use windows::Win32::UI::WindowsAndMessaging::{ACCEL, CreateAcceleratorTableW, TranslateAcceleratorW, DispatchMessageW, TranslateMessage, MSG};
  /// use tao::event_loop::EventLoopBuilder;
  /// #[cfg(target_os = "windows")]
  /// use tao::platform::windows::EventLoopBuilderExtWindows;
  ///
  /// let mut builder = EventLoopBuilder::new();
  /// #[cfg(target_os = "windows")]
  /// builder.with_msg_hook(|msg|{
  ///     let msg = msg as *const MSG;
  /// #   let accels_: Vec<ACCEL> = Vec::new();
  /// #   let accels = accels_.as_slice();
  ///     let translated = unsafe {
  ///         TranslateAcceleratorW(
  ///             (*msg).hwnd,
  ///             CreateAcceleratorTableW(accels).unwrap(),
  ///             msg,
  ///         ) == 1
  ///     };
  ///     translated
  /// });
  /// ```
  fn with_msg_hook<F>(&mut self, callback: F) -> &mut Self
  where
    F: FnMut(*const std::ffi::c_void) -> bool + 'static;

  /// Forces a theme or uses the system settings if `None` was provided.
  ///
  /// This will only affect some controls like context menus.
  ///
  /// ## Note
  ///
  /// Since this setting is app-wide, using [`WindowBuilder::with_theme`]
  /// will not change the affected controls for that specific window,
  /// so it is recommended to always use the same theme used for this app-wide setting
  /// or use `None` so it automatically uses the theme of this method
  /// or falls back to the system preference.
  fn with_theme(&mut self, theme: Option<Theme>) -> &mut Self;
}

impl<T> EventLoopBuilderExtWindows for EventLoopBuilder<T> {
  #[inline]
  fn with_any_thread(&mut self, any_thread: bool) -> &mut Self {
    self.platform_specific.any_thread = any_thread;
    self
  }

  #[inline]
  fn with_dpi_aware(&mut self, dpi_aware: bool) -> &mut Self {
    self.platform_specific.dpi_aware = dpi_aware;
    self
  }

  #[inline]
  fn with_msg_hook<F>(&mut self, callback: F) -> &mut Self
  where
    F: FnMut(*const std::ffi::c_void) -> bool + 'static,
  {
    self.platform_specific.msg_hook = Some(Box::new(callback));
    self
  }

  #[inline]
  fn with_theme(&mut self, theme: Option<Theme>) -> &mut Self {
    self.platform_specific.preferred_theme = theme;
    self
  }
}

/// Additional methods on `Window` that are specific to Windows.
pub trait WindowExtWindows {
  /// Returns the HINSTANCE of the window
  fn hinstance(&self) -> isize;
  /// Returns the native handle that is used by this window.
  ///
  /// The pointer will become invalid when the native window was destroyed.
  fn hwnd(&self) -> isize;

  /// Enables or disables mouse and keyboard input to the specified window.
  ///
  /// A window must be enabled before it can be activated.
  /// If an application has create a modal dialog box by disabling its owner window
  /// (as described in [`WindowBuilderExtWindows::with_owner_window`]), the application must enable
  /// the owner window before destroying the dialog box.
  /// Otherwise, another window will receive the keyboard focus and be activated.
  ///
  /// If a child window is disabled, it is ignored when the system tries to determine which
  /// window should receive mouse messages.
  ///
  /// For more information, see <https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enablewindow#remarks>
  /// and <https://docs.microsoft.com/en-us/windows/win32/winmsg/window-features#disabled-windows>
  fn set_enable(&self, enabled: bool);

  /// This sets `ICON_BIG`. A good ceiling here is 256x256.
  fn set_taskbar_icon(&self, taskbar_icon: Option<Icon>);

  /// This sets the overlay icon
  fn set_overlay_icon(&self, icon: Option<&Icon>);

  /// Returns the current window theme.
  fn theme(&self) -> Theme;

  /// Reset the dead key state of the keyboard.
  ///
  /// This is useful when a dead key is bound to trigger an action. Then
  /// this function can be called to reset the dead key state so that
  /// follow-up text input won't be affected by the dead key.
  fn reset_dead_keys(&self);

  /// Starts the resizing drag from given edge
  fn begin_resize_drag(&self, edge: isize, button: u32, x: i32, y: i32);

  /// Whether to show the window icon in the taskbar or not.
  fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError>;

  /// Shows or hides the background drop shadow for undecorated windows.
  ///
  /// Enabling the shadow causes a thin 1px line to appear on the top of the window.
  fn set_undecorated_shadow(&self, shadow: bool);

  /// Returns whether this window has shadow for undecorated windows.
  fn has_undecorated_shadow(&self) -> bool;

  /// Sets right-to-left layout.
  ///
  /// Enabling this mainly flips the orientation of menus and title bar buttons
  fn set_rtl(&self, rtl: bool);
}

impl WindowExtWindows for Window {
  #[inline]
  fn hinstance(&self) -> isize {
    self.window.hinstance().0 as _
  }

  #[inline]
  fn hwnd(&self) -> isize {
    self.window.hwnd().0 as _
  }

  #[inline]
  fn set_enable(&self, enabled: bool) {
    unsafe {
      let _ = EnableWindow(self.window.hwnd(), enabled);
    }
  }

  #[inline]
  fn set_taskbar_icon(&self, taskbar_icon: Option<Icon>) {
    self.window.set_taskbar_icon(taskbar_icon)
  }

  #[inline]
  fn theme(&self) -> Theme {
    self.window.theme()
  }

  #[inline]
  fn reset_dead_keys(&self) {
    self.window.reset_dead_keys();
  }

  #[inline]
  fn begin_resize_drag(&self, edge: isize, button: u32, x: i32, y: i32) {
    self.window.begin_resize_drag(edge, button, x, y)
  }

  #[inline]
  fn set_skip_taskbar(&self, skip: bool) -> Result<(), ExternalError> {
    self.window.set_skip_taskbar(skip)
  }

  #[inline]
  fn set_undecorated_shadow(&self, shadow: bool) {
    self.window.set_undecorated_shadow(shadow)
  }

  #[inline]
  fn has_undecorated_shadow(&self) -> bool {
    self.window.has_undecorated_shadow()
  }

  #[inline]
  fn set_rtl(&self, rtl: bool) {
    self.window.set_rtl(rtl)
  }

  #[inline]
  fn set_overlay_icon(&self, icon: Option<&Icon>) {
    self.window.set_overlay_icon(icon);
  }
}

/// Additional methods on `WindowBuilder` that are specific to Windows.
pub trait WindowBuilderExtWindows {
  /// Sets a parent to the window to be created.
  ///
  /// A child window has the WS_CHILD style and is confined to the client area of its parent window.
  ///
  /// For more information, see <https://docs.microsoft.com/en-us/windows/win32/winmsg/window-features#child-windows>
  fn with_parent_window(self, parent: HWND) -> WindowBuilder;

  /// Set an owner to the window to be created. Can be used to create a dialog box, for example.
  /// Can be used in combination with [`WindowExtWindows::set_enable(false)`](WindowExtWindows::set_enable)
  /// on the owner window to create a modal dialog box.
  ///
  /// From MSDN:
  /// - An owned window is always above its owner in the z-order.
  /// - The system automatically destroys an owned window when its owner is destroyed.
  /// - An owned window is hidden when its owner is minimized.
  ///
  /// For more information, see <https://docs.microsoft.com/en-us/windows/win32/winmsg/window-features#owned-windows>
  fn with_owner_window(self, parent: HWND) -> WindowBuilder;

  /// Sets a menu on the window to be created.
  ///
  /// Parent and menu are mutually exclusive; a child window cannot have a menu!
  ///
  /// The menu must have been manually created beforehand with [`windows::Win32::UI::WindowsAndMessaging::CreateMenu`]
  /// or similar.
  ///
  /// Note: Dark mode cannot be supported for win32 menus, it's simply not possible to change how the menus look.
  /// If you use this, it is recommended that you combine it with `with_theme(Some(Theme::Light))` to avoid a jarring effect.
  fn with_menu(self, menu: HMENU) -> WindowBuilder;

  /// This sets `ICON_BIG`. A good ceiling here is 256x256.
  fn with_taskbar_icon(self, taskbar_icon: Option<Icon>) -> WindowBuilder;

  /// This sets `WS_EX_NOREDIRECTIONBITMAP`.
  fn with_no_redirection_bitmap(self, flag: bool) -> WindowBuilder;

  /// Enables or disables drag and drop support (enabled by default). Will interfere with other crates
  /// that use multi-threaded COM API (`CoInitializeEx` with `COINIT_MULTITHREADED` instead of
  /// `COINIT_APARTMENTTHREADED`) on the same thread. Note that tao may still attempt to initialize
  /// COM API regardless of this option. Currently only fullscreen mode does that, but there may be more in the future.
  /// If you need COM API with `COINIT_MULTITHREADED` you must initialize it before calling any tao functions.
  /// See <https://docs.microsoft.com/en-us/windows/win32/api/objbase/nf-objbase-coinitialize#remarks> for more information.
  fn with_drag_and_drop(self, flag: bool) -> WindowBuilder;

  /// Whether to create the window icon with the taskbar icon or not.
  fn with_skip_taskbar(self, skip: bool) -> WindowBuilder;

  /// Customize the window class name.
  fn with_window_classname<S: Into<String>>(self, classname: S) -> WindowBuilder;

  /// Shows or hides the background drop shadow for undecorated windows.
  ///
  /// The shadow is hidden by default.
  /// Enabling the shadow causes a thin 1px line to appear on the top of the window.
  fn with_undecorated_shadow(self, shadow: bool) -> WindowBuilder;

  /// Sets right-to-left layout.
  fn with_rtl(self, rtl: bool) -> WindowBuilder;
}

impl WindowBuilderExtWindows for WindowBuilder {
  #[inline]
  fn with_parent_window(mut self, parent: HWND) -> WindowBuilder {
    self.platform_specific.parent = Parent::ChildOf(windows::Win32::Foundation::HWND(parent as _));
    self
  }

  #[inline]
  fn with_owner_window(mut self, parent: HWND) -> WindowBuilder {
    self.platform_specific.parent = Parent::OwnedBy(windows::Win32::Foundation::HWND(parent as _));
    self
  }

  #[inline]
  fn with_menu(mut self, menu: HMENU) -> WindowBuilder {
    self.platform_specific.menu = Some(windows::Win32::UI::WindowsAndMessaging::HMENU(menu as _));
    self
  }

  #[inline]
  fn with_taskbar_icon(mut self, taskbar_icon: Option<Icon>) -> WindowBuilder {
    self.platform_specific.taskbar_icon = taskbar_icon;
    self
  }

  #[inline]
  fn with_no_redirection_bitmap(mut self, flag: bool) -> WindowBuilder {
    self.platform_specific.no_redirection_bitmap = flag;
    self
  }

  #[inline]
  fn with_drag_and_drop(mut self, flag: bool) -> WindowBuilder {
    self.platform_specific.drag_and_drop = flag;
    self
  }

  #[inline]
  fn with_skip_taskbar(mut self, skip: bool) -> WindowBuilder {
    self.platform_specific.skip_taskbar = skip;
    self
  }

  #[inline]
  fn with_window_classname<S: Into<String>>(mut self, classname: S) -> WindowBuilder {
    self.platform_specific.window_classname = classname.into();
    self
  }

  #[inline]
  fn with_undecorated_shadow(mut self, shadow: bool) -> WindowBuilder {
    self.platform_specific.decoration_shadow = shadow;
    self
  }

  #[inline]
  fn with_rtl(mut self, rtl: bool) -> WindowBuilder {
    self.platform_specific.rtl = rtl;
    self
  }
}

/// Additional methods on `MonitorHandle` that are specific to Windows.
pub trait MonitorHandleExtWindows {
  /// Returns the name of the monitor adapter specific to the Win32 API.
  fn native_id(&self) -> String;

  /// Returns the handle of the monitor - `HMONITOR`.
  fn hmonitor(&self) -> isize;
}

impl MonitorHandleExtWindows for MonitorHandle {
  #[inline]
  fn native_id(&self) -> String {
    self.inner.native_identifier()
  }

  #[inline]
  fn hmonitor(&self) -> isize {
    self.inner.hmonitor().0 as _
  }
}

/// Additional methods on `DeviceId` that are specific to Windows.
pub trait DeviceIdExtWindows {
  /// Returns an identifier that persistently refers to this specific device.
  ///
  /// Will return `None` if the device is no longer available.
  fn persistent_identifier(&self) -> Option<String>;
}

impl DeviceIdExtWindows for DeviceId {
  #[inline]
  fn persistent_identifier(&self) -> Option<String> {
    self.0.persistent_identifier()
  }
}

/// Additional methods on `Icon` that are specific to Windows.
pub trait IconExtWindows: Sized {
  /// Create an icon from a file path.
  ///
  /// Specify `size` to load a specific icon size from the file, or `None` to load the default
  /// icon size from the file.
  ///
  /// In cases where the specified size does not exist in the file, Windows may perform scaling
  /// to get an icon of the desired size.
  fn from_path<P: AsRef<Path>>(path: P, size: Option<PhysicalSize<u32>>) -> Result<Self, BadIcon>;

  /// Create an icon from a resource embedded in this executable or library.
  ///
  /// Specify `size` to load a specific icon size from the file, or `None` to load the default
  /// icon size from the file.
  ///
  /// In cases where the specified size does not exist in the file, Windows may perform scaling
  /// to get an icon of the desired size.
  fn from_resource(ordinal: u16, size: Option<PhysicalSize<u32>>) -> Result<Self, BadIcon>;
}

impl IconExtWindows for Icon {
  fn from_path<P: AsRef<Path>>(path: P, size: Option<PhysicalSize<u32>>) -> Result<Self, BadIcon> {
    let win_icon = WinIcon::from_path(path, size)?;
    Ok(Icon { inner: win_icon })
  }

  fn from_resource(ordinal: u16, size: Option<PhysicalSize<u32>>) -> Result<Self, BadIcon> {
    let win_icon = WinIcon::from_resource(ordinal, size)?;
    Ok(Icon { inner: win_icon })
  }
}
