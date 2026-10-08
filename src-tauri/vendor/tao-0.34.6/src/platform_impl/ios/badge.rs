use std::ffi::CStr;

use objc2::{
  msg_send,
  runtime::{AnyClass, AnyObject},
};

pub fn set_badge_count(count: i32) {
  unsafe {
    let ui_application = AnyClass::get(CStr::from_bytes_with_nul(b"UIApplication\0").unwrap())
      .expect("Failed to get UIApplication class");
    let app: *mut AnyObject = msg_send![ui_application, sharedApplication];
    let _: () = msg_send![app, setApplicationIconBadgeNumber:count];
  }
}
