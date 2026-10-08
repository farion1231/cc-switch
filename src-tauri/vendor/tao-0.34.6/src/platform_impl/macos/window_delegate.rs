// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  f64,
  ffi::CStr,
  os::raw::c_void,
  sync::{Arc, Weak},
};

use objc2::{
  msg_send,
  rc::Retained,
  runtime::{AnyClass as Class, AnyObject as Object, ClassBuilder as ClassDecl, Sel},
};
use objc2_app_kit::{
  self as appkit, NSApplicationPresentationOptions, NSPasteboard, NSView, NSWindow,
};
use objc2_foundation::{ns_string, NSArray, NSAutoreleasePool, NSString, NSUInteger};
use once_cell::sync::Lazy;

use crate::{
  dpi::{LogicalPosition, LogicalSize},
  event::{Event, WindowEvent},
  keyboard::ModifiersState,
  platform_impl::platform::{
    app_state::AppState,
    event::{EventProxy, EventWrapper},
    ffi::{id, nil, BOOL, NO, YES},
    util::{self, IdRef},
    view::ViewState,
    window::{get_ns_theme, get_window_id, UnownedWindow},
  },
  window::{Fullscreen, WindowId},
};

pub struct WindowDelegateState {
  ns_window: Retained<NSWindow>, // never changes
  // We keep this ns_view because we still need its view state for some extern function
  // like didResignKey
  ns_view: Retained<NSView>, // never changes

  window: Weak<UnownedWindow>,

  // TODO: It's possible for delegate methods to be called asynchronously,
  // causing data races / `RefCell` panics.

  // This is set when WindowBuilder::with_fullscreen was set,
  // see comments of `window_did_fail_to_enter_fullscreen`
  initial_fullscreen: bool,

  // During `windowDidResize`, we use this to only send Moved if the position changed.
  previous_position: Option<(f64, f64)>,

  // Used to prevent redundant events.
  previous_scale_factor: f64,

  // Used to prevent resized events from being fired
  // when we are using our workaround in the `is_zoomed` function.
  is_checking_zoomed_in: bool,
}

impl WindowDelegateState {
  pub fn new(window: &Arc<UnownedWindow>, initial_fullscreen: bool) -> Self {
    let scale_factor = window.scale_factor();
    let mut delegate_state = WindowDelegateState {
      ns_window: window.ns_window.clone(),
      ns_view: window.ns_view.clone(),
      window: Arc::downgrade(window),
      initial_fullscreen,
      previous_position: None,
      previous_scale_factor: scale_factor,
      is_checking_zoomed_in: false,
    };
    if (scale_factor - 1.0).abs() > f64::EPSILON {
      delegate_state.emit_static_scale_factor_changed_event();
    }

    delegate_state
  }

  fn ns_view(&self) -> Retained<NSView> {
    self.ns_window.contentView().unwrap()
  }

  fn with_window<F, T>(&mut self, callback: F) -> Option<T>
  where
    F: FnOnce(&UnownedWindow) -> T,
  {
    self.window.upgrade().map(|ref window| callback(window))
  }

  pub fn emit_event(&mut self, event: WindowEvent<'static>) {
    let event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&self.ns_window)),
      event,
    };
    AppState::queue_event(EventWrapper::StaticEvent(event));
  }

  pub fn emit_static_scale_factor_changed_event(&mut self) {
    let scale_factor = self.get_scale_factor();
    if (scale_factor - self.previous_scale_factor).abs() < f64::EPSILON {
      return;
    };

    self.previous_scale_factor = scale_factor;
    let wrapper = EventWrapper::EventProxy(EventProxy::DpiChangedProxy {
      ns_window: self.ns_window.clone(),
      suggested_size: self.view_size(),
      scale_factor,
    });
    AppState::queue_event(wrapper);
  }

  pub fn emit_resize_event(&mut self) {
    let rect = NSView::frame(&self.ns_view());
    let scale_factor = self.get_scale_factor();
    let logical_size = LogicalSize::new(rect.size.width as f64, rect.size.height as f64);
    let size = logical_size.to_physical(scale_factor);
    self.emit_event(WindowEvent::Resized(size));
  }

  fn emit_move_event(&mut self) {
    let rect = NSWindow::frame(&self.ns_window);
    let x = rect.origin.x as f64;
    let y = util::bottom_left_to_top_left(rect);
    let moved = self.previous_position != Some((x, y));
    if moved {
      self.previous_position = Some((x, y));
      let scale_factor = self.get_scale_factor();
      let physical_pos = LogicalPosition::<f64>::from((x, y)).to_physical(scale_factor);
      self.emit_event(WindowEvent::Moved(physical_pos));
    }
  }

  fn get_scale_factor(&self) -> f64 {
    NSWindow::backingScaleFactor(&self.ns_window) as f64
  }

  fn view_size(&self) -> LogicalSize<f64> {
    let ns_size = NSView::frame(&self.ns_view()).size;
    LogicalSize::new(ns_size.width as f64, ns_size.height as f64)
  }
}

pub fn new_delegate(window: &Arc<UnownedWindow>, initial_fullscreen: bool) -> IdRef {
  let state = WindowDelegateState::new(window, initial_fullscreen);
  unsafe {
    // This is free'd in `dealloc`
    let state_ptr = Box::into_raw(Box::new(state)) as *mut c_void;
    let delegate: id = msg_send![WINDOW_DELEGATE_CLASS.0, alloc];
    IdRef::new(msg_send![delegate, initWithTao: state_ptr])
  }
}

struct WindowDelegateClass(*const Class);
unsafe impl Send for WindowDelegateClass {}
unsafe impl Sync for WindowDelegateClass {}

static WINDOW_DELEGATE_CLASS: Lazy<WindowDelegateClass> = Lazy::new(|| unsafe {
  let superclass = class!(NSResponder);
  let mut decl = ClassDecl::new(
    CStr::from_bytes_with_nul(b"TaoWindowDelegate\0").unwrap(),
    superclass,
  )
  .unwrap();

  decl.add_method(sel!(dealloc), dealloc as extern "C" fn(_, _));
  decl.add_method(
    sel!(initWithTao:),
    init_with_tao as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(markIsCheckingZoomedIn),
    mark_is_checking_zoomed_in as extern "C" fn(_, _),
  );
  decl.add_method(
    sel!(clearIsCheckingZoomedIn),
    clear_is_checking_zoomed_in as extern "C" fn(_, _),
  );

  decl.add_method(
    sel!(windowShouldClose:),
    window_should_close as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(windowWillClose:),
    window_will_close as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidResize:),
    window_did_resize as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidMove:),
    window_did_move as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidChangeBackingProperties:),
    window_did_change_backing_properties as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidBecomeKey:),
    window_did_become_key as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidResignKey:),
    window_did_resign_key as extern "C" fn(_, _, _),
  );

  decl.add_method(
    sel!(draggingEntered:),
    dragging_entered as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(prepareForDragOperation:),
    prepare_for_drag_operation as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(performDragOperation:),
    perform_drag_operation as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(concludeDragOperation:),
    conclude_drag_operation as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(draggingExited:),
    dragging_exited as extern "C" fn(_, _, _),
  );

  decl.add_method(
    sel!(window:willUseFullScreenPresentationOptions:),
    window_will_use_fullscreen_presentation_options as extern "C" fn(_, _, _, _) -> _,
  );
  decl.add_method(
    sel!(windowDidEnterFullScreen:),
    window_did_enter_fullscreen as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowWillEnterFullScreen:),
    window_will_enter_fullscreen as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidExitFullScreen:),
    window_did_exit_fullscreen as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowWillExitFullScreen:),
    window_will_exit_fullscreen as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(windowDidFailToEnterFullScreen:),
    window_did_fail_to_enter_fullscreen as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(effectiveAppearanceDidChange:),
    effective_appearance_did_change as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(effectiveAppearanceDidChangedOnMainThread:),
    effective_appearance_did_changed_on_main_thread as extern "C" fn(_, _, _),
  );

  decl.add_ivar::<*mut c_void>(CStr::from_bytes_with_nul(b"taoState\0").unwrap());
  WindowDelegateClass(decl.register())
});

// This function is definitely unsafe, but labeling that would increase
// boilerplate and wouldn't really clarify anything...
fn with_state<F: FnOnce(&mut WindowDelegateState) -> T, T>(this: &Object, callback: F) {
  #[allow(deprecated)] // TODO: Use define_class!
  let state_ptr = unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    &mut *(state_ptr as *mut WindowDelegateState)
  };
  callback(state_ptr);
}

extern "C" fn dealloc(this: &Object, _sel: Sel) {
  with_state(this, |state| unsafe {
    drop(Box::from_raw(state as *mut WindowDelegateState));
  });
}

extern "C" fn init_with_tao(this: &Object, _sel: Sel, state: *mut c_void) -> id {
  #[allow(deprecated)] // TODO: Use define_class!
  unsafe {
    let this: id = msg_send![this, init];
    if this != nil {
      *(*this).get_mut_ivar("taoState") = state;
      with_state(&*this, |state| {
        let () = msg_send![&state.ns_window, setDelegate: this];
      });
    }

    let notification_center: &Object =
      msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
    let notification_name = ns_string!("AppleInterfaceThemeChangedNotification");
    let _: () = msg_send![
        notification_center,
        addObserver: this
        selector: sel!(effectiveAppearanceDidChange:)
        name: &*notification_name
        object: nil
    ];

    this
  }
}

extern "C" fn mark_is_checking_zoomed_in(this: &Object, _sel: Sel) {
  with_state(&*this, |state| {
    state.is_checking_zoomed_in = true;
  });
}

extern "C" fn clear_is_checking_zoomed_in(this: &Object, _sel: Sel) {
  with_state(&*this, |state| {
    state.is_checking_zoomed_in = false;
  });
}

extern "C" fn window_should_close(this: &Object, _: Sel, _: id) -> BOOL {
  trace!("Triggered `windowShouldClose:`");
  with_state(this, |state| state.emit_event(WindowEvent::CloseRequested));
  trace!("Completed `windowShouldClose:`");
  NO
}

extern "C" fn window_will_close(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowWillClose:`");
  with_state(this, |state| unsafe {
    // `setDelegate:` retains the previous value and then autoreleases it
    let _pool = NSAutoreleasePool::new();
    // Since El Capitan, we need to be careful that delegate methods can't
    // be called after the window closes.
    let () = msg_send![&state.ns_window, setDelegate: nil];
    state.emit_event(WindowEvent::Destroyed);
  });
  trace!("Completed `windowWillClose:`");
}

extern "C" fn window_did_resize(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidResize:`");
  with_state(this, |state| {
    if !state.is_checking_zoomed_in {
      state.emit_resize_event();
      state.emit_move_event();
    }
  });
  trace!("Completed `windowDidResize:`");
}

// This won't be triggered if the move was part of a resize.
extern "C" fn window_did_move(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidMove:`");
  with_state(this, |state| {
    state.emit_move_event();
  });
  trace!("Completed `windowDidMove:`");
}

extern "C" fn window_did_change_backing_properties(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidChangeBackingProperties:`");
  with_state(this, |state| {
    state.emit_static_scale_factor_changed_event();
  });
  trace!("Completed `windowDidChangeBackingProperties:`");
}

extern "C" fn window_did_become_key(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidBecomeKey:`");
  with_state(this, |state| {
    // TODO: center the cursor if the window had mouse grab when it
    // lost focus
    state.emit_event(WindowEvent::Focused(true));
  });
  trace!("Completed `windowDidBecomeKey:`");
}

extern "C" fn window_did_resign_key(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidResignKey:`");
  with_state(this, |state| {
    // It happens rather often, e.g. when the user is Cmd+Tabbing, that the
    // NSWindowDelegate will receive a didResignKey event despite no event
    // being received when the modifiers are released.  This is because
    // flagsChanged events are received by the NSView instead of the
    // NSWindowDelegate, and as a result a tracked modifiers state can quite
    // easily fall out of synchrony with reality.  This requires us to emit
    // a synthetic ModifiersChanged event when we lose focus.
    //
    // Here we (very unsafely) acquire the taoState (a ViewState) from the
    // Object referenced by state.ns_view (an IdRef, which is dereferenced
    // to an id)
    #[allow(deprecated)] // TODO: Use define_class!
    let view_state: &mut ViewState = unsafe {
      let ns_view: &Object = &state.ns_view;
      let state_ptr: *mut c_void = *ns_view.get_ivar("taoState");
      &mut *(state_ptr as *mut ViewState)
    };

    // Both update the state and emit a ModifiersChanged event.
    if !view_state.modifiers.is_empty() {
      view_state.modifiers = ModifiersState::empty();
      state.emit_event(WindowEvent::ModifiersChanged(view_state.modifiers));
    }

    state.emit_event(WindowEvent::Focused(false));
  });
  trace!("Completed `windowDidResignKey:`");
}

/// Invoked when the dragged image enters destination bounds or frame
extern "C" fn dragging_entered(this: &Object, _: Sel, sender: id) -> BOOL {
  trace!("Triggered `draggingEntered:`");

  use std::path::PathBuf;

  let pb: Retained<NSPasteboard> = unsafe { msg_send![sender, draggingPasteboard] };
  let filenames =
    unsafe { NSPasteboard::propertyListForType(&pb, appkit::NSFilenamesPboardType) }.unwrap();

  for file in unsafe { Retained::cast_unchecked::<NSArray>(filenames) } {
    let file = unsafe { Retained::cast_unchecked::<NSString>(file) };

    unsafe {
      let f = NSString::UTF8String(&file);
      let path = CStr::from_ptr(f).to_string_lossy().into_owned();

      with_state(this, |state| {
        state.emit_event(WindowEvent::HoveredFile(PathBuf::from(path)));
      });
    }
  }

  trace!("Completed `draggingEntered:`");
  YES
}

/// Invoked when the image is released
extern "C" fn prepare_for_drag_operation(_: &Object, _: Sel, _: id) -> BOOL {
  trace!("Triggered `prepareForDragOperation:`");
  trace!("Completed `prepareForDragOperation:`");
  YES
}

/// Invoked after the released image has been removed from the screen
extern "C" fn perform_drag_operation(this: &Object, _: Sel, sender: id) -> BOOL {
  trace!("Triggered `performDragOperation:`");

  use std::path::PathBuf;

  let pb: Retained<NSPasteboard> = unsafe { msg_send![sender, draggingPasteboard] };
  let filenames =
    unsafe { NSPasteboard::propertyListForType(&pb, appkit::NSFilenamesPboardType) }.unwrap();

  for file in unsafe { Retained::cast_unchecked::<NSArray>(filenames) } {
    let file = unsafe { Retained::cast_unchecked::<NSString>(file) };

    unsafe {
      let f = NSString::UTF8String(&file);
      let path = CStr::from_ptr(f).to_string_lossy().into_owned();

      with_state(this, |state| {
        state.emit_event(WindowEvent::DroppedFile(PathBuf::from(path)));
      });
    }
  }

  trace!("Completed `performDragOperation:`");
  YES
}

/// Invoked when the dragging operation is complete
extern "C" fn conclude_drag_operation(_: &Object, _: Sel, _: id) {
  trace!("Triggered `concludeDragOperation:`");
  trace!("Completed `concludeDragOperation:`");
}

/// Invoked when the dragging operation is cancelled
extern "C" fn dragging_exited(this: &Object, _: Sel, _: id) {
  trace!("Triggered `draggingExited:`");
  with_state(this, |state| {
    state.emit_event(WindowEvent::HoveredFileCancelled)
  });
  trace!("Completed `draggingExited:`");
}

/// Invoked when before enter fullscreen
extern "C" fn window_will_enter_fullscreen(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowWillEnterFullscreen:`");

  with_state(this, |state| {
    state.with_window(|window| {
      trace!("Locked shared state in `window_will_enter_fullscreen`");
      let mut shared_state = window.shared_state.lock().unwrap();
      shared_state.maximized = window.is_zoomed();
      match shared_state.fullscreen {
        // Exclusive mode sets the state in `set_fullscreen` as the user
        // can't enter exclusive mode by other means (like the
        // fullscreen button on the window decorations)
        Some(Fullscreen::Exclusive(_)) => (),
        // `window_will_enter_fullscreen` was triggered and we're already
        // in fullscreen, so we must've reached here by `set_fullscreen`
        // as it updates the state
        Some(Fullscreen::Borderless(_)) => (),
        // Otherwise, we must've reached fullscreen by the user clicking
        // on the green fullscreen button. Update state!
        None => {
          let current_monitor = window.current_monitor_inner();
          shared_state.fullscreen = Some(Fullscreen::Borderless(current_monitor))
        }
      }
      shared_state.in_fullscreen_transition = true;
      trace!("Unlocked shared state in `window_will_enter_fullscreen`");
    })
  });
  trace!("Completed `windowWillEnterFullscreen:`");
}

/// Invoked when before exit fullscreen
extern "C" fn window_will_exit_fullscreen(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowWillExitFullScreen:`");

  with_state(this, |state| {
    state.with_window(|window| {
      trace!("Locked shared state in `window_will_exit_fullscreen`");
      let mut shared_state = window.shared_state.lock().unwrap();
      shared_state.in_fullscreen_transition = true;
      trace!("Unlocked shared state in `window_will_exit_fullscreen`");
    });
  });
  trace!("Completed `windowWillExitFullScreen:`");
}

extern "C" fn window_will_use_fullscreen_presentation_options(
  this: &Object,
  _: Sel,
  _: id,
  proposed_options: NSUInteger,
) -> NSUInteger {
  // Generally, games will want to disable the menu bar and the dock. Ideally,
  // this would be configurable by the user. Unfortunately because of our
  // `CGShieldingWindowLevel() + 1` hack (see `set_fullscreen`), our window is
  // placed on top of the menu bar in exclusive fullscreen mode. This looks
  // broken so we always disable the menu bar in exclusive fullscreen. We may
  // still want to make this configurable for borderless fullscreen. Right now
  // we don't, for consistency. If we do, it should be documented that the
  // user-provided options are ignored in exclusive fullscreen.
  let mut options: NSUInteger = proposed_options;
  with_state(this, |state| {
    state.with_window(|window| {
      trace!("Locked shared state in `window_will_use_fullscreen_presentation_options`");
      let shared_state = window.shared_state.lock().unwrap();
      if let Some(Fullscreen::Exclusive(_)) = shared_state.fullscreen {
        options = (NSApplicationPresentationOptions::FullScreen
          | NSApplicationPresentationOptions::HideDock
          | NSApplicationPresentationOptions::HideMenuBar)
          .bits();
      }
      trace!("Unlocked shared state in `window_will_use_fullscreen_presentation_options`");
    })
  });

  options
}

/// Invoked when entered fullscreen
extern "C" fn window_did_enter_fullscreen(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidEnterFullscreen:`");
  with_state(this, |state| {
    state.initial_fullscreen = false;
    state.with_window(|window| {
      trace!("Locked shared state in `window_did_enter_fullscreen`");
      let mut shared_state = window.shared_state.lock().unwrap();
      shared_state.in_fullscreen_transition = false;
      let target_fullscreen = shared_state.target_fullscreen.take();
      trace!("Unlocked shared state in `window_did_enter_fullscreen`");
      drop(shared_state);
      if let Some(target_fullscreen) = target_fullscreen {
        window.set_fullscreen(target_fullscreen);
      }
    });
    state.emit_resize_event();
    state.emit_move_event();
  });
  trace!("Completed `windowDidEnterFullscreen:`");
}

/// Invoked when exited fullscreen
extern "C" fn window_did_exit_fullscreen(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidExitFullscreen:`");
  with_state(this, |state| {
    state.with_window(|window| {
      window.restore_state_from_fullscreen();
      trace!("Locked shared state in `window_did_exit_fullscreen`");
      let mut shared_state = window.shared_state.lock().unwrap();
      shared_state.in_fullscreen_transition = false;
      let target_fullscreen = shared_state.target_fullscreen.take();
      trace!("Unlocked shared state in `window_did_exit_fullscreen`");
      drop(shared_state);
      if let Some(target_fullscreen) = target_fullscreen {
        window.set_fullscreen(target_fullscreen);
      }
    });
    state.emit_resize_event();
    state.emit_move_event();
  });
  trace!("Completed `windowDidExitFullscreen:`");
}

/// Invoked when fail to enter fullscreen
///
/// When this window launch from a fullscreen app (e.g. launch from VS Code
/// terminal), it creates a new virtual destkop and a transition animation.
/// This animation takes one second and cannot be disable without
/// elevated privileges. In this animation time, all toggleFullscreen events
/// will be failed. In this implementation, we will try again by using
/// performSelector:withObject:afterDelay: until window_did_enter_fullscreen.
/// It should be fine as we only do this at initialzation (i.e with_fullscreen
/// was set).
///
/// From Apple doc:
/// In some cases, the transition to enter full-screen mode can fail,
/// due to being in the midst of handling some other animation or user gesture.
/// This method indicates that there was an error, and you should clean up any
/// work you may have done to prepare to enter full-screen mode.
extern "C" fn window_did_fail_to_enter_fullscreen(this: &Object, _: Sel, _: id) {
  trace!("Triggered `windowDidFailToEnterFullscreen:`");
  with_state(this, |state| {
    state.with_window(|window| {
      trace!("Locked shared state in `window_did_fail_to_enter_fullscreen`");
      let mut shared_state = window.shared_state.lock().unwrap();
      shared_state.in_fullscreen_transition = false;
      shared_state.target_fullscreen = None;
      trace!("Unlocked shared state in `window_did_fail_to_enter_fullscreen`");
    });
    if state.initial_fullscreen {
      let _: () = unsafe {
        msg_send![
            &state.ns_window,
            performSelector:sel!(toggleFullScreen:),
            withObject:nil,
            afterDelay: 0.5,
        ]
      };
    } else {
      state.with_window(|window| window.restore_state_from_fullscreen());
    }
  });
  trace!("Completed `windowDidFailToEnterFullscreen:`");
}

// Observe theme change
extern "C" fn effective_appearance_did_change(this: &Object, _: Sel, _: id) {
  trace!("Triggered `effectiveAppearDidChange:`");
  unsafe {
    let _: () = msg_send![this, performSelectorOnMainThread: sel!(effectiveAppearanceDidChangedOnMainThread:), withObject:nil, waitUntilDone:false];
  }
}
extern "C" fn effective_appearance_did_changed_on_main_thread(this: &Object, _: Sel, _: id) {
  with_state(this, |state| {
    let theme = get_ns_theme();
    let current_theme = state.window.upgrade().map(|w| {
      let mut state = w.shared_state.lock().unwrap();
      let current_theme = state.current_theme;
      state.current_theme = theme;
      current_theme
    });
    if current_theme != Some(theme) {
      state.emit_event(WindowEvent::ThemeChanged(theme));
    }
  });
  trace!("Completed `effectiveAppearDidChange:`");
}
