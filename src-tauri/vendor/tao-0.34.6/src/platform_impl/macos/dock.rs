#![allow(non_snake_case, non_upper_case_globals)]

use std::{
  sync::Mutex,
  time::{Duration, Instant},
};

use objc2::{runtime::AnyObject, MainThreadMarker};
use objc2_app_kit::NSApplication;

use super::get_aux_state_mut;

const DOCK_SHOW_TIMEOUT: Duration = Duration::from_secs(1);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
  fn TransformProcessType(psn: *const ProcessSerialNumber, transformState: i32) -> i32;
}

#[repr(C)]
struct ProcessSerialNumber {
  highLongOfPSN: u32,
  lowLongOfPSN: u32,
}

/// https://developer.apple.com/documentation/applicationservices/1501096-anonymous/kcurrentprocess?language=objc
pub const kCurrentProcess: u32 = 2;
/// https://developer.apple.com/documentation/applicationservices/1501117-anonymous/kprocesstransformtouielementapplication?language=objc
pub const kProcessTransformToUIElementApplication: i32 = 4;
/// https://developer.apple.com/documentation/applicationservices/1501117-anonymous/kprocesstransformtoforegroundapplication?language=objc
pub const kProcessTransformToForegroundApplication: i32 = 1;

pub fn set_dock_visibility(app_delegate: &AnyObject, visible: bool) {
  let last_dock_show = unsafe { &get_aux_state_mut(app_delegate).last_dock_show };
  if visible {
    set_dock_show(last_dock_show);
  } else {
    set_dock_hide(last_dock_show);
  }
}

fn set_dock_hide(last_dock_show: &Mutex<Option<Instant>>) {
  // Transforming application state from UIElement to Foreground is an
  // asynchronous operation, and unfortunately there is currently no way to know
  // when it is finished.
  // So if we call DockHide => DockShow => DockHide => DockShow in a very short
  // time, we would trigger a bug of macOS that, there would be multiple dock
  // icons of the app left in system.
  // To work around this, we make sure DockHide does nothing if it is called
  // immediately after DockShow. After some experiments, 1 second seems to be
  // a proper interval.
  let now = Instant::now();
  let last_dock_show = last_dock_show.lock().unwrap();
  if let Some(last_dock_show_time) = *last_dock_show {
    if now.duration_since(last_dock_show_time) < DOCK_SHOW_TIMEOUT {
      return;
    }
  }

  unsafe {
    // TODO: Safety.
    let mtm = MainThreadMarker::new_unchecked();
    let app = NSApplication::sharedApplication(mtm);
    let windows = app.windows();

    for window in windows {
      window.setCanHide(false);
    }

    let psn = ProcessSerialNumber {
      highLongOfPSN: 0,
      lowLongOfPSN: kCurrentProcess,
    };
    TransformProcessType(&psn, kProcessTransformToUIElementApplication);
  }
}

fn set_dock_show(last_dock_show: &Mutex<Option<Instant>>) {
  let now = Instant::now();
  let mut last_dock_show = last_dock_show.lock().unwrap();
  *last_dock_show = Some(now);

  unsafe {
    let psn = ProcessSerialNumber {
      highLongOfPSN: 0,
      lowLongOfPSN: kCurrentProcess,
    };

    TransformProcessType(&psn, kProcessTransformToForegroundApplication);
  }
}
