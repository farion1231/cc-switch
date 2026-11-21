//! Switching to a tray-only app can discard a hidden window's fullscreen Space.
//! Exit the Space before hiding, then restore fullscreen when the window reopens.

use std::{cell::RefCell, ptr::NonNull};

use block2::RcBlock;
use objc2_06::{
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
    MainThreadMarker,
};
use objc2_app_kit_03::{
    NSWindow, NSWindowDidExitFullScreenNotification, NSWindowWillCloseNotification,
};
use objc2_foundation_03::{NSNotification, NSNotificationCenter, NSOperationQueue};
use tauri::{Manager, WebviewWindow, Window};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    ExitThenHide,
    ExitThenReopen,
    Hidden,
}

struct Transition {
    window_id: usize,
    phase: Phase,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl Drop for Transition {
    fn drop(&mut self) {
        // SAFETY: this is the token returned by the default notification center.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self.observer.as_ref()) }
    }
}

// Only the main window can close to tray. Its native identity changes when
// lightweight mode destroys and recreates it. AppKit delivers these events on
// the main thread, so we can keep the non-Send observer token there as well.
thread_local! {
    static TRANSITION: RefCell<Option<Transition>> = const { RefCell::new(None) };
}

fn clear(window_id: usize) {
    // Drop the observer outside the borrow, as unregistering it calls AppKit.
    drop(TRANSITION.with_borrow_mut(|slot| slot.take_if(|current| current.window_id == window_id)));
}

// Once hidden, only a show request consumes the saved fullscreen state.
fn update_phase(window_id: usize, next: Phase) -> Option<Phase> {
    TRANSITION.with_borrow_mut(|slot| {
        let current = slot
            .as_mut()
            .filter(|current| current.window_id == window_id)?;
        let previous = current.phase;
        if previous != Phase::Hidden {
            current.phase = next;
        }
        Some(previous)
    })
}

fn did_exit(window: &Window, window_id: usize) {
    let previous = update_phase(window_id, Phase::Hidden);
    if previous == Some(Phase::ExitThenHide) {
        if !crate::tray::hide_main_to_tray(window) {
            clear(window_id);
        }
    } else if previous == Some(Phase::ExitThenReopen) {
        clear(window_id);
        if let Err(error) = window.set_fullscreen(true) {
            log::warn!("Could not restore fullscreen after reopening: {error}");
        }
    }
}

/// Called from the native close event on the main thread.
pub fn close_to_tray(window: &Window) -> bool {
    let (Some(_mtm), Ok(ptr)) = (MainThreadMarker::new(), window.ns_window()) else {
        return false;
    };
    let window_id = ptr as usize;
    if !matches!(
        update_phase(window_id, Phase::ExitThenHide),
        Some(Phase::ExitThenHide | Phase::ExitThenReopen)
    ) {
        if !window.is_fullscreen().unwrap_or(false) {
            return false;
        }
        // SAFETY: Tauri owns this NSWindow and the close event is on its main thread.
        let native = unsafe { &*ptr.cast::<NSWindow>() };
        let handle = window.clone();
        let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
            // SAFETY: AppKit supplies the notification for the duration of the callback.
            let name = unsafe { notification.as_ref() }.name();
            if *name == *unsafe { NSWindowWillCloseNotification } {
                clear(window_id);
            } else if *name == *unsafe { NSWindowDidExitFullScreenNotification } {
                let handle = handle.clone();
                let finish = RcBlock::new(move || did_exit(&handle, window_id));
                // Let Tao's fullscreen delegate complete before another window operation.
                unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&finish) };
            }
        });
        // SAFETY: filter by this NSWindow only and deliver callbacks on the main thread.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                None,
                Some(native),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        let previous = TRANSITION.with_borrow_mut(|slot| {
            slot.replace(Transition {
                window_id,
                phase: Phase::ExitThenHide,
                observer,
            })
        });
        drop(previous);
    }
    // Repeated close requests may also retry a failed native transition.
    if let Err(error) = window.set_fullscreen(false) {
        clear(window_id);
        log::warn!("Could not exit fullscreen to hide the main window: {error}");
    }
    true
}

/// A second instance or a deep link can also request a show from a worker thread.
pub fn show(window: &WebviewWindow) {
    let handle = window.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        crate::tray::apply_tray_policy(handle.app_handle(), true);
        let _ = handle.unminimize();
        crate::window_fit::fit_to_work_area(&handle);
        if let Err(error) = handle.show() {
            log::warn!("Could not show the main window: {error}");
            return;
        }
        let _ = handle.set_focus();
        let Ok(ptr) = handle.ns_window() else { return };
        let window_id = ptr as usize;
        if update_phase(window_id, Phase::ExitThenReopen) == Some(Phase::Hidden) {
            clear(window_id);
            if let Err(error) = handle.set_fullscreen(true) {
                log::warn!("Could not restore fullscreen after reopening: {error}");
            }
        }
    }) {
        log::warn!("Could not schedule showing the main window: {error}");
    }
}
