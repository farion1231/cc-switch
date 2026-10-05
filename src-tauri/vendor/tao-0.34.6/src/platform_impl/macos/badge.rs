use super::ffi::id;
use objc2_app_kit::NSApp;
use objc2_foundation::{MainThreadMarker, NSString};

pub fn set_badge_label(label: Option<String>) {
  // SAFETY: TODO
  let mtm = unsafe { MainThreadMarker::new_unchecked() };
  unsafe {
    let label = label.map(|label| NSString::from_str(&label));
    let dock_tile: id = msg_send![&NSApp(mtm), dockTile];
    let _: () = msg_send![dock_tile, setBadgeLabel: label.as_deref()];
  }
}
