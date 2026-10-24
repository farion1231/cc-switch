//! Optional macOS branding. The bundle identity and product name stay unchanged.

#[cfg(target_os = "macos")]
pub fn apply(app: &tauri::AppHandle) -> Result<(), String> {
    let neutral = crate::settings::get_settings().neutral_app_icon;
    use objc2::{class, msg_send, runtime::AnyObject};
    // Called on the main thread (AppKit requirement). NSData copies the embedded
    // PNG; NSApplication retains the image after setApplicationIconImage:.
    unsafe {
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let image: *mut AnyObject = if neutral {
            let bytes = include_bytes!("../icons/neutral.png");
            let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>(), length: bytes.len()];
            let allocated: *mut AnyObject = msg_send![class!(NSImage), alloc];
            let image: *mut AnyObject = msg_send![allocated, initWithData: data];
            if image.is_null() {
                return Err("Unable to load neutral app icon".into());
            }
            image
        } else {
            // nil restores the bundle's original icon.
            std::ptr::null_mut()
        };
        let _: () = msg_send![application, setApplicationIconImage: image];
        if !image.is_null() {
            let _: () = msg_send![image, release];
        }
    }
    crate::tray::apply_app_icon(app)
}
