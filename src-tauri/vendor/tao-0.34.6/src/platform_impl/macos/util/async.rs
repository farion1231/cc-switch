// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  ops::Deref,
  sync::{Mutex, Weak},
};

use core_graphics::base::CGFloat;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::{rc::autoreleasepool, Message};
use objc2_app_kit::{NSScreen, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSPoint, NSSize, NSString};

use crate::{
  dpi::LogicalSize,
  platform_impl::platform::{
    ffi::{self, id, NO, YES},
    window::SharedState,
  },
};

pub fn is_main_thread() -> bool {
  unsafe { msg_send!(class!(NSThread), isMainThread) }
}

// Unsafe wrapper type that allows us to dispatch things that aren't Send.
// This should *only* be used to dispatch to the main queue.
// While it is indeed not guaranteed that these types can safely be sent to
// other threads, we know that they're safe to use on the main thread.
struct MainThreadSafe<T>(T);

unsafe impl<T> Send for MainThreadSafe<T> {}

impl<T> Deref for MainThreadSafe<T> {
  type Target = T;
  fn deref(&self) -> &T {
    &self.0
  }
}

fn run_on_main<F: Send + FnOnce()>(f: F) {
  if is_main_thread() {
    f();
  } else {
    DispatchQueue::main().exec_sync(f);
  }
}

unsafe fn set_style_mask(ns_window: &NSWindow, ns_view: &NSView, mask: NSWindowStyleMask) {
  ns_window.setStyleMask(mask);
  // If we don't do this, key handling will break
  // (at least until the window is clicked again/etc.)
  ns_window.makeFirstResponder(Some(ns_view));
}

// Always use this function instead of trying to modify `styleMask` directly!
// `setStyleMask:` isn't thread-safe, so we have to use Grand Central Dispatch.
// Otherwise, this would vomit out errors about not being on the main thread
// and fail to do anything.
pub unsafe fn set_style_mask_async(
  ns_window: &NSWindow,
  ns_view: &NSView,
  mask: NSWindowStyleMask,
) {
  let ns_window = MainThreadSafe(ns_window.retain());
  let ns_view = MainThreadSafe(ns_view.retain());
  DispatchQueue::main().exec_async(move || {
    set_style_mask(&ns_window, &ns_view, mask);
  });
}
pub unsafe fn set_style_mask_sync(ns_window: &NSWindow, ns_view: &NSView, mask: NSWindowStyleMask) {
  if is_main_thread() {
    set_style_mask(ns_window, ns_view, mask);
  } else {
    let ns_window = MainThreadSafe(ns_window.retain());
    let ns_view = MainThreadSafe(ns_view.retain());
    DispatchQueue::main().exec_sync(move || {
      set_style_mask(&ns_window, &ns_view, mask);
    })
  }
}

// `setContentSize:` isn't thread-safe either, though it doesn't log any errors
// and just fails silently. Anyway, GCD to the rescue!
pub unsafe fn set_content_size_async(ns_window: &NSWindow, size: LogicalSize<f64>) {
  let ns_window = MainThreadSafe(ns_window.retain());
  DispatchQueue::main().exec_async(move || {
    ns_window.setContentSize(NSSize::new(size.width as CGFloat, size.height as CGFloat));
  });
}

// `setFrameTopLeftPoint:` isn't thread-safe, but fortunately has the courtesy
// to log errors.
pub unsafe fn set_frame_top_left_point_async(ns_window: &NSWindow, point: NSPoint) {
  let ns_window = MainThreadSafe(ns_window.retain());
  DispatchQueue::main().exec_async(move || {
    ns_window.setFrameTopLeftPoint(point);
  });
}

// `setFrameTopLeftPoint:` isn't thread-safe, and fails silently.
pub unsafe fn set_level_async(ns_window: &NSWindow, level: ffi::NSWindowLevel) {
  let ns_window = MainThreadSafe(ns_window.retain());
  DispatchQueue::main().exec_async(move || {
    ns_window.setLevel(level as _);
  });
}

// `toggleFullScreen` is thread-safe, but our additional logic to account for
// window styles isn't.
pub unsafe fn toggle_full_screen_async(
  ns_window: &NSWindow,
  ns_view: &NSView,
  not_fullscreen: bool,
  shared_state: Weak<Mutex<SharedState>>,
) {
  let ns_window = MainThreadSafe(ns_window.retain());
  let ns_view = MainThreadSafe(ns_view.retain());
  let shared_state = MainThreadSafe(shared_state);
  DispatchQueue::main().exec_async(move || {
    // `toggleFullScreen` doesn't work if the `StyleMask` is none, so we
    // set a normal style temporarily. The previous state will be
    // restored in `WindowDelegate::window_did_exit_fullscreen`.
    if not_fullscreen {
      let curr_mask = ns_window.styleMask();
      let required = NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable;
      if !curr_mask.contains(required) {
        set_style_mask(&ns_window, &ns_view, required);
        if let Some(shared_state) = shared_state.upgrade() {
          trace!("Locked shared state in `toggle_full_screen_callback`");
          let mut shared_state_lock = shared_state.lock().unwrap();
          shared_state_lock.saved_style = Some(curr_mask);
          trace!("Unlocked shared state in `toggle_full_screen_callback`");
        }
      }
    }
    // Window level must be restored from `CGShieldingWindowLevel()
    // + 1` back to normal in order for `toggleFullScreen` to do
    // anything
    ns_window.setLevel(0);
    ns_window.toggleFullScreen(None);
  });
}

pub unsafe fn restore_display_mode_async(ns_screen: u32) {
  DispatchQueue::main().exec_async(move || {
    ffi::CGRestorePermanentDisplayConfiguration();
    assert_eq!(ffi::CGDisplayRelease(ns_screen), ffi::kCGErrorSuccess);
  });
}

// `setMaximized` is not thread-safe
pub unsafe fn set_maximized_async(
  ns_window: &NSWindow,
  is_zoomed: bool,
  maximized: bool,
  shared_state: Weak<Mutex<SharedState>>,
) {
  let ns_window = MainThreadSafe(ns_window.retain());
  let shared_state = MainThreadSafe(shared_state);
  DispatchQueue::main().exec_async(move || {
    if let Some(shared_state) = shared_state.upgrade() {
      trace!("Locked shared state in `set_maximized`");
      let mut shared_state_lock = shared_state.lock().unwrap();

      // Save the standard frame sized if it is not zoomed
      if !is_zoomed {
        shared_state_lock.standard_frame = Some(NSWindow::frame(&ns_window));
      }

      shared_state_lock.maximized = maximized;

      let curr_mask = ns_window.styleMask();
      if shared_state_lock.fullscreen.is_some() {
        // Handle it in window_did_exit_fullscreen
        return;
      } else if curr_mask.contains(NSWindowStyleMask::Resizable)
        && curr_mask.contains(NSWindowStyleMask::Titled)
      {
        // Just use the native zoom if resizable
        ns_window.zoom(None);
      } else {
        // if it's not resizable, we set the frame directly
        let new_rect = if maximized {
          let mtm = MainThreadMarker::new_unchecked();
          let screen = NSScreen::mainScreen(mtm).unwrap();
          NSScreen::visibleFrame(&screen)
        } else {
          shared_state_lock.saved_standard_frame()
        };
        let _: () = msg_send![&*ns_window, setFrame:new_rect, display:NO, animate: YES];
      }

      trace!("Unlocked shared state in `set_maximized`");
    }
  });
}

// `orderOut:` isn't thread-safe. Calling it from another thread actually works,
// but with an odd delay.
pub unsafe fn order_out_sync(ns_window: &NSWindow) {
  let ns_window = MainThreadSafe(ns_window.retain());
  run_on_main(move || {
    ns_window.orderOut(None);
  });
}

// `makeKeyAndOrderFront:` isn't thread-safe. Calling it from another thread
// actually works, but with an odd delay.
pub unsafe fn make_key_and_order_front_sync(ns_window: &NSWindow) {
  let ns_window = MainThreadSafe(ns_window.retain());
  run_on_main(move || {
    ns_window.makeKeyAndOrderFront(None);
  });
}

// `setTitle:` isn't thread-safe. Calling it from another thread invalidates the
// window drag regions, which throws an exception when not done in the main
// thread
pub unsafe fn set_title_async(ns_window: &NSWindow, title: String) {
  let ns_window = MainThreadSafe(ns_window.retain());
  DispatchQueue::main().exec_async(move || {
    let title = NSString::from_str(&title);
    ns_window.setTitle(&title);
  });
}

// `setFocus:` isn't thread-safe.
pub unsafe fn set_focus(ns_window: &NSWindow) {
  let ns_window = MainThreadSafe(ns_window.retain());
  run_on_main(move || {
    ns_window.makeKeyAndOrderFront(None);
    let app: id = msg_send![class!(NSApplication), sharedApplication];
    let () = msg_send![app, activateIgnoringOtherApps: YES];
  });
}

// `close:` is thread-safe, but we want the event to be triggered from the main
// thread. Though, it's a good idea to look into that more...
pub unsafe fn close_async(ns_window: &NSWindow) {
  let ns_window = MainThreadSafe(ns_window.retain());
  run_on_main(move || {
    autoreleasepool(move |_| {
      ns_window.close();
    });
  });
}

// `setIgnoresMouseEvents_:` isn't thread-safe, and fails silently.
pub unsafe fn set_ignore_mouse_events(ns_window: &NSWindow, ignore: bool) {
  let ns_window = MainThreadSafe(ns_window.retain());
  DispatchQueue::main().exec_async(move || {
    ns_window.setIgnoresMouseEvents(ignore);
  });
}
