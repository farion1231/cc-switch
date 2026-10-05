// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "macos")]

use std::os::raw::c_void;

use objc2_foundation::NSObject;

use crate::{
  dpi::{LogicalSize, Position},
  event_loop::{EventLoop, EventLoopWindowTarget},
  monitor::MonitorHandle,
  platform_impl::{get_aux_state_mut, set_badge_label, set_dock_visibility, Parent},
  window::{Window, WindowBuilder},
};

/// Additional methods on `Window` that are specific to MacOS.
pub trait WindowExtMacOS {
  /// Returns a pointer to the cocoa `NSWindow` that is used by this window.
  ///
  /// The pointer will become invalid when the `Window` is destroyed.
  fn ns_window(&self) -> *mut c_void;

  /// Returns a pointer to the cocoa `NSView` that is used by this window.
  ///
  /// The pointer will become invalid when the `Window` is destroyed.
  fn ns_view(&self) -> *mut c_void;

  /// Returns whether or not the window is in simple fullscreen mode.
  fn simple_fullscreen(&self) -> bool;

  /// Toggles a fullscreen mode that doesn't require a new macOS space.
  /// Returns a boolean indicating whether the transition was successful (this
  /// won't work if the window was already in the native fullscreen).
  ///
  /// This is how fullscreen used to work on macOS in versions before Lion.
  /// And allows the user to have a fullscreen window without using another
  /// space or taking control over the entire monitor.
  fn set_simple_fullscreen(&self, fullscreen: bool) -> bool;

  /// Returns whether or not the window has shadow.
  fn has_shadow(&self) -> bool;

  /// Sets whether or not the window has shadow.
  fn set_has_shadow(&self, has_shadow: bool);

  /// Set the window traffic light position relative to the upper left corner
  fn set_traffic_light_inset<P: Into<Position>>(&self, position: P);
  /// Put the window in a state which indicates a file save is required.
  ///
  /// <https://developer.apple.com/documentation/appkit/nswindow/1419311-isdocumentedited>
  fn set_is_document_edited(&self, edited: bool);

  /// Get the window's edit state
  fn is_document_edited(&self) -> bool;

  /// Sets whether the system can automatically organize windows into tabs.
  ///
  /// <https://developer.apple.com/documentation/appkit/nswindow/1646657-allowsautomaticwindowtabbing>
  fn set_allows_automatic_window_tabbing(&self, enabled: bool);

  /// Returns whether the system can automatically organize windows into tabs.
  fn allows_automatic_window_tabbing(&self) -> bool;

  /// Group windows together by using the same tabbing identifier.
  ///
  /// <https://developer.apple.com/documentation/appkit/nswindow/1644704-tabbingidentifier>
  fn set_tabbing_identifier(&self, identifier: &str);

  /// Returns the window's tabbing identifier.
  fn tabbing_identifier(&self) -> String;

  /// The content view consumes the full size of the window.
  ///
  /// <https://developer.apple.com/documentation/appkit/nsfullsizecontentviewwindowmask>
  fn set_fullsize_content_view(&self, fullsize: bool);

  /// A Boolean value that indicates whether the title bar draws its background.
  ///
  /// <https://developer.apple.com/documentation/appkit/nswindow/1419167-titlebarappearstransparent>
  fn set_titlebar_transparent(&self, transparent: bool);

  /// Sets the badge label on the taskbar
  fn set_badge_label(&self, label: Option<String>);
}

impl WindowExtMacOS for Window {
  #[inline]
  fn ns_window(&self) -> *mut c_void {
    self.window.ns_window()
  }

  #[inline]
  fn ns_view(&self) -> *mut c_void {
    self.window.ns_view()
  }

  #[inline]
  fn simple_fullscreen(&self) -> bool {
    self.window.simple_fullscreen()
  }

  #[inline]
  fn set_simple_fullscreen(&self, fullscreen: bool) -> bool {
    self.window.set_simple_fullscreen(fullscreen)
  }

  #[inline]
  fn has_shadow(&self) -> bool {
    self.window.has_shadow()
  }

  #[inline]
  fn set_has_shadow(&self, has_shadow: bool) {
    self.window.set_has_shadow(has_shadow)
  }

  #[inline]
  fn set_traffic_light_inset<P: Into<Position>>(&self, position: P) {
    self.window.set_traffic_light_inset(position)
  }

  #[inline]
  fn set_is_document_edited(&self, edited: bool) {
    self.window.set_is_document_edited(edited)
  }

  #[inline]
  fn is_document_edited(&self) -> bool {
    self.window.is_document_edited()
  }

  #[inline]
  fn set_allows_automatic_window_tabbing(&self, enabled: bool) {
    self.window.set_allows_automatic_window_tabbing(enabled)
  }

  #[inline]
  fn allows_automatic_window_tabbing(&self) -> bool {
    self.window.allows_automatic_window_tabbing()
  }

  #[inline]
  fn set_tabbing_identifier(&self, identifier: &str) {
    self.window.set_tabbing_identifier(identifier)
  }

  #[inline]
  fn tabbing_identifier(&self) -> String {
    self.window.tabbing_identifier()
  }

  #[inline]
  fn set_fullsize_content_view(&self, fullsize: bool) {
    self.window.set_fullsize_content_view(fullsize);
  }

  #[inline]
  fn set_titlebar_transparent(&self, transparent: bool) {
    self.window.set_titlebar_transparent(transparent);
  }

  #[inline]
  fn set_badge_label(&self, label: Option<String>) {
    self.window.set_badge_label(label);
  }
}

/// Corresponds to `NSApplicationActivationPolicy`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ActivationPolicy {
  /// Corresponds to `NSApplicationActivationPolicyRegular`.
  #[default]
  Regular,
  /// Corresponds to `NSApplicationActivationPolicyAccessory`.
  Accessory,
  /// Corresponds to `NSApplicationActivationPolicyProhibited`.
  Prohibited,
}

/// Additional methods on `WindowBuilder` that are specific to MacOS.
///
/// **Note:** Properties dealing with the titlebar will be overwritten by the `with_decorations` method
/// on the base `WindowBuilder`:
///
///  - `with_titlebar_transparent`
///  - `with_title_hidden`
///  - `with_titlebar_hidden`
///  - `with_titlebar_buttons_hidden`
///  - `with_fullsize_content_view`
pub trait WindowBuilderExtMacOS {
  /// Sets a parent to the window to be created.
  fn with_parent_window(self, parent: *mut c_void) -> WindowBuilder;
  /// Enables click-and-drag behavior for the entire window, not just the titlebar.
  fn with_movable_by_window_background(self, movable_by_window_background: bool) -> WindowBuilder;
  /// Makes the titlebar transparent and allows the content to appear behind it.
  fn with_titlebar_transparent(self, titlebar_transparent: bool) -> WindowBuilder;
  /// Hides the window title.
  fn with_title_hidden(self, title_hidden: bool) -> WindowBuilder;
  /// Hides the window titlebar.
  fn with_titlebar_hidden(self, titlebar_hidden: bool) -> WindowBuilder;
  /// Hides the window titlebar buttons.
  fn with_titlebar_buttons_hidden(self, titlebar_buttons_hidden: bool) -> WindowBuilder;
  /// Makes the window content appear behind the titlebar.
  fn with_fullsize_content_view(self, fullsize_content_view: bool) -> WindowBuilder;
  /// Build window with `resizeIncrements` property. Values must not be 0.
  fn with_resize_increments(self, increments: LogicalSize<f64>) -> WindowBuilder;
  fn with_disallow_hidpi(self, disallow_hidpi: bool) -> WindowBuilder;
  /// Sets whether or not the window has shadow.
  fn with_has_shadow(self, has_shadow: bool) -> WindowBuilder;
  /// Sets the traffic light position to (x, y) relative to the upper left corner
  fn with_traffic_light_inset<P: Into<Position>>(self, inset: P) -> WindowBuilder;
  /// Sets whether the system can automatically organize windows into tabs.
  fn with_automatic_window_tabbing(self, automatic_tabbing: bool) -> WindowBuilder;
  /// Defines the window [tabbing identifier].
  ///
  /// [tabbing identifier]: <https://developer.apple.com/documentation/appkit/nswindow/1644704-tabbingidentifier>
  fn with_tabbing_identifier(self, identifier: &str) -> WindowBuilder;
}

impl WindowBuilderExtMacOS for WindowBuilder {
  #[inline]
  fn with_parent_window(mut self, parent: *mut c_void) -> WindowBuilder {
    self.platform_specific.parent = Parent::ChildOf(parent);
    self
  }

  #[inline]
  fn with_movable_by_window_background(
    mut self,
    movable_by_window_background: bool,
  ) -> WindowBuilder {
    self.platform_specific.movable_by_window_background = movable_by_window_background;
    self
  }

  #[inline]
  fn with_titlebar_transparent(mut self, titlebar_transparent: bool) -> WindowBuilder {
    self.platform_specific.titlebar_transparent = titlebar_transparent;
    self
  }

  #[inline]
  fn with_titlebar_hidden(mut self, titlebar_hidden: bool) -> WindowBuilder {
    self.platform_specific.titlebar_hidden = titlebar_hidden;
    self
  }

  #[inline]
  fn with_titlebar_buttons_hidden(mut self, titlebar_buttons_hidden: bool) -> WindowBuilder {
    self.platform_specific.titlebar_buttons_hidden = titlebar_buttons_hidden;
    self
  }

  #[inline]
  fn with_title_hidden(mut self, title_hidden: bool) -> WindowBuilder {
    self.platform_specific.title_hidden = title_hidden;
    self
  }

  #[inline]
  fn with_fullsize_content_view(mut self, fullsize_content_view: bool) -> WindowBuilder {
    self.platform_specific.fullsize_content_view = fullsize_content_view;
    self
  }

  #[inline]
  fn with_resize_increments(mut self, increments: LogicalSize<f64>) -> WindowBuilder {
    self.platform_specific.resize_increments = Some(increments);
    self
  }

  #[inline]
  fn with_disallow_hidpi(mut self, disallow_hidpi: bool) -> WindowBuilder {
    self.platform_specific.disallow_hidpi = disallow_hidpi;
    self
  }

  #[inline]
  fn with_has_shadow(mut self, has_shadow: bool) -> WindowBuilder {
    self.platform_specific.has_shadow = has_shadow;
    self
  }

  #[inline]
  fn with_traffic_light_inset<P: Into<Position>>(mut self, inset: P) -> WindowBuilder {
    self.platform_specific.traffic_light_inset = Some(inset.into());
    self
  }

  #[inline]
  fn with_automatic_window_tabbing(mut self, automatic_tabbing: bool) -> WindowBuilder {
    self.platform_specific.automatic_tabbing = automatic_tabbing;
    self
  }

  #[inline]
  fn with_tabbing_identifier(mut self, tabbing_identifier: &str) -> WindowBuilder {
    self
      .platform_specific
      .tabbing_identifier
      .replace(tabbing_identifier.into());
    self
  }
}

pub trait EventLoopExtMacOS {
  /// Sets the activation policy for the application. It is set to
  /// `NSApplicationActivationPolicyRegular` by default.
  ///
  /// This function only takes effect if it's called before calling
  /// [`run`](crate::event_loop::EventLoop::run) or
  /// [`run_return`](crate::platform::run_return::EventLoopExtRunReturn::run_return).
  /// To set the activation policy after that, use
  /// [`EventLoopWindowTargetExtMacOS::set_activation_policy_at_runtime`](crate::platform::macos::EventLoopWindowTargetExtMacOS::set_activation_policy_at_runtime).
  fn set_activation_policy(&mut self, activation_policy: ActivationPolicy);

  /// Sets the visibility of the application in the dock.
  ///
  /// This function only takes effect if it's called before calling
  /// [`run`](crate::event_loop::EventLoop::run) or
  /// [`run_return`](crate::platform::run_return::EventLoopExtRunReturn::run_return).
  fn set_dock_visibility(&mut self, visible: bool);

  /// Used to prevent the application from automatically activating when launched if
  /// another application is already active
  ///
  /// The default behavior is to ignore other applications and activate when launched.
  ///
  /// This function only takes effect if it's called before calling
  /// [`run`](crate::event_loop::EventLoop::run) or
  /// [`run_return`](crate::platform::run_return::EventLoopExtRunReturn::run_return)
  fn set_activate_ignoring_other_apps(&mut self, ignore: bool);
}

impl<T> EventLoopExtMacOS for EventLoop<T> {
  #[inline]
  fn set_activation_policy(&mut self, activation_policy: ActivationPolicy) {
    unsafe {
      get_aux_state_mut(&**self.event_loop.delegate).activation_policy = activation_policy;
    }
  }

  #[inline]
  fn set_dock_visibility(&mut self, visible: bool) {
    unsafe {
      get_aux_state_mut(&**self.event_loop.delegate).dock_visibility = visible;
    }
  }

  #[inline]
  fn set_activate_ignoring_other_apps(&mut self, ignore: bool) {
    unsafe {
      get_aux_state_mut(&**self.event_loop.delegate).activate_ignoring_other_apps = ignore;
    }
  }
}

/// Additional methods on `MonitorHandle` that are specific to MacOS.
pub trait MonitorHandleExtMacOS {
  /// Returns the identifier of the monitor for Cocoa.
  fn native_id(&self) -> u32;
  /// Returns a pointer to the NSScreen representing this monitor.
  fn ns_screen(&self) -> Option<*mut c_void>;
}

impl MonitorHandleExtMacOS for MonitorHandle {
  #[inline]
  fn native_id(&self) -> u32 {
    self.inner.native_identifier()
  }

  fn ns_screen(&self) -> Option<*mut c_void> {
    self
      .inner
      .ns_screen()
      .map(|s| objc2::rc::Retained::into_raw(s) as *mut c_void)
  }
}

/// Additional methods on `EventLoopWindowTarget` that are specific to macOS.
pub trait EventLoopWindowTargetExtMacOS {
  /// Hide the entire application. In most applications this is typically triggered with Command-H.
  fn hide_application(&self);
  /// Show the entire application.
  fn show_application(&self);
  /// Hide the other applications. In most applications this is typically triggered with Command+Option-H.
  fn hide_other_applications(&self);
  /// Sets the activation policy for the application. It is set to
  /// `NSApplicationActivationPolicyRegular` by default.
  ///
  /// To set the activation policy before the app starts running, see
  /// [`EventLoopExtMacOS::set_activation_policy`](crate::platform::macos::EventLoopExtMacOS::set_activation_policy).
  fn set_activation_policy_at_runtime(&self, activation_policy: ActivationPolicy);

  /// Sets the visibility of the application in the dock.
  ///
  /// To set the dock visibility before the app starts running, see
  /// [`EventLoopExtMacOS::set_dock_visibility`](crate::platform::macos::EventLoopExtMacOS::set_dock_visibility).
  fn set_dock_visibility(&self, visible: bool);

  /// Sets the badge label on macos dock
  fn set_badge_label(&self, label: Option<String>);
}

impl<T> EventLoopWindowTargetExtMacOS for EventLoopWindowTarget<T> {
  fn hide_application(&self) {
    // TODO: Safety.
    let mtm = unsafe { objc2_foundation::MainThreadMarker::new_unchecked() };
    objc2_app_kit::NSApplication::sharedApplication(mtm).hide(None)
  }

  fn show_application(&self) {
    // TODO: Safety.
    let mtm = unsafe { objc2_foundation::MainThreadMarker::new_unchecked() };
    unsafe { objc2_app_kit::NSApplication::sharedApplication(mtm).unhide(None) }
  }

  fn hide_other_applications(&self) {
    // TODO: Safety.
    let mtm = unsafe { objc2_foundation::MainThreadMarker::new_unchecked() };
    objc2_app_kit::NSApplication::sharedApplication(mtm).hideOtherApplications(None)
  }

  fn set_activation_policy_at_runtime(&self, activation_policy: ActivationPolicy) {
    use objc2_app_kit::NSApplicationActivationPolicy;

    let ns_activation_policy = match activation_policy {
      ActivationPolicy::Regular => NSApplicationActivationPolicy::Regular,
      ActivationPolicy::Accessory => NSApplicationActivationPolicy::Accessory,
      ActivationPolicy::Prohibited => NSApplicationActivationPolicy::Prohibited,
    };

    // TODO: Safety.
    let mtm = unsafe { objc2_foundation::MainThreadMarker::new_unchecked() };
    objc2_app_kit::NSApplication::sharedApplication(mtm).setActivationPolicy(ns_activation_policy);
  }

  fn set_dock_visibility(&self, visible: bool) {
    let Some(Ok(delegate)) = (unsafe {
      // TODO: Safety.
      let mtm = objc2_foundation::MainThreadMarker::new_unchecked();
      objc2_app_kit::NSApplication::sharedApplication(mtm)
        .delegate()
        .map(|delegate| delegate.downcast::<NSObject>())
    }) else {
      return;
    };
    set_dock_visibility(&delegate, visible);
  }

  fn set_badge_label(&self, label: Option<String>) {
    set_badge_label(label);
  }
}
