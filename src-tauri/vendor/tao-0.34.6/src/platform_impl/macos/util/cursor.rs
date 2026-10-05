// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use crate::platform_impl::platform::ffi::{id, nil, NO};
use objc2::{
  msg_send,
  rc::Retained,
  runtime::{AnyObject, Sel},
  AllocAnyThread,
};
use objc2_app_kit::NSImage;
use objc2_foundation::{ns_string, NSDictionary, NSPoint, NSString};
use std::{
  cell::RefCell,
  ffi::{c_void, CString},
  ptr::null_mut,
};

use crate::window::CursorIcon;

#[derive(Default)]
pub enum Cursor {
  #[default]
  Default,
  Native(&'static str),
  Undocumented(&'static str),
  WebKit(&'static str),
}

impl From<CursorIcon> for Cursor {
  fn from(cursor: CursorIcon) -> Self {
    // See native cursors at https://developer.apple.com/documentation/appkit/nscursor?language=objc.
    match cursor {
      CursorIcon::Default => Cursor::Default,
      CursorIcon::Arrow => Cursor::Native("arrowCursor"),
      CursorIcon::Hand => Cursor::Native("pointingHandCursor"),
      CursorIcon::Grab => Cursor::Native("openHandCursor"),
      CursorIcon::Grabbing => Cursor::Native("closedHandCursor"),
      CursorIcon::Text => Cursor::Native("IBeamCursor"),
      CursorIcon::VerticalText => Cursor::Native("IBeamCursorForVerticalLayout"),
      CursorIcon::Copy => Cursor::Native("dragCopyCursor"),
      CursorIcon::Alias => Cursor::Native("dragLinkCursor"),
      CursorIcon::NotAllowed | CursorIcon::NoDrop => Cursor::Native("operationNotAllowedCursor"),
      CursorIcon::ContextMenu => Cursor::Native("contextualMenuCursor"),
      CursorIcon::Crosshair => Cursor::Native("crosshairCursor"),
      CursorIcon::EResize => Cursor::Native("resizeRightCursor"),
      CursorIcon::NResize => Cursor::Native("resizeUpCursor"),
      CursorIcon::WResize => Cursor::Native("resizeLeftCursor"),
      CursorIcon::SResize => Cursor::Native("resizeDownCursor"),
      CursorIcon::EwResize | CursorIcon::ColResize => Cursor::Native("resizeLeftRightCursor"),
      CursorIcon::NsResize | CursorIcon::RowResize => Cursor::Native("resizeUpDownCursor"),

      // Undocumented cursors: https://stackoverflow.com/a/46635398/5435443
      CursorIcon::Help => Cursor::Undocumented("_helpCursor"),
      CursorIcon::ZoomIn => Cursor::Undocumented("_zoomInCursor"),
      CursorIcon::ZoomOut => Cursor::Undocumented("_zoomOutCursor"),
      CursorIcon::NeResize => Cursor::Undocumented("_windowResizeNorthEastCursor"),
      CursorIcon::NwResize => Cursor::Undocumented("_windowResizeNorthWestCursor"),
      CursorIcon::SeResize => Cursor::Undocumented("_windowResizeSouthEastCursor"),
      CursorIcon::SwResize => Cursor::Undocumented("_windowResizeSouthWestCursor"),
      CursorIcon::NeswResize => Cursor::Undocumented("_windowResizeNorthEastSouthWestCursor"),
      CursorIcon::NwseResize => Cursor::Undocumented("_windowResizeNorthWestSouthEastCursor"),

      // While these are available, the former just loads a white arrow,
      // and the latter loads an ugly deflated beachball!
      // CursorIcon::Move => Cursor::Undocumented("_moveCursor"),
      // CursorIcon::Wait => Cursor::Undocumented("_waitCursor"),

      // An even more undocumented cursor...
      // https://bugs.eclipse.org/bugs/show_bug.cgi?id=522349
      // This is the wrong semantics for `Wait`, but it's the same as
      // what's used in Safari and Chrome.
      CursorIcon::Wait | CursorIcon::Progress => Cursor::Undocumented("busyButClickableCursor"),

      // For the rest, we can just snatch the cursors from WebKit...
      // They fit the style of the native cursors, and will seem
      // completely standard to macOS users.
      // https://stackoverflow.com/a/21786835/5435443
      CursorIcon::Move | CursorIcon::AllScroll => Cursor::WebKit("move"),
      CursorIcon::Cell => Cursor::WebKit("cell"),
    }
  }
}

impl Cursor {
  pub unsafe fn load(&self) -> id {
    match self {
      Cursor::Default => null_mut(),
      Cursor::Native(cursor_name) => {
        let sel = Sel::register(&CString::new(*cursor_name).unwrap());
        msg_send![class!(NSCursor), performSelector: sel]
      }
      Cursor::Undocumented(cursor_name) => {
        let class = class!(NSCursor);
        let sel = Sel::register(&CString::new(*cursor_name).unwrap());
        let sel = if msg_send![class, respondsToSelector: sel] {
          sel
        } else {
          warn!("Cursor `{}` appears to be invalid", cursor_name);
          sel!(arrowCursor)
        };
        msg_send![class, performSelector: sel]
      }
      Cursor::WebKit(cursor_name) => load_webkit_cursor(cursor_name),
    }
  }
}

// Note that loading `busybutclickable` with this code won't animate the frames;
// instead you'll just get them all in a column.
pub unsafe fn load_webkit_cursor(cursor_name: &str) -> id {
  const CURSOR_ROOT: &str = "/System/Library/Frameworks/ApplicationServices.framework/Versions/A/Frameworks/HIServices.framework/Versions/A/Resources/cursors";
  let cursor_root = ns_string!(CURSOR_ROOT);
  let cursor_name = NSString::from_str(cursor_name);
  let cursor_pdf = ns_string!("cursor.pdf");
  let cursor_plist = ns_string!("info.plist");
  let key_x = ns_string!("hotx");
  let key_y = ns_string!("hoty");

  let cursor_path: Retained<NSString> =
    msg_send![cursor_root, stringByAppendingPathComponent: &*cursor_name];
  let pdf_path: Retained<NSString> =
    msg_send![&cursor_path, stringByAppendingPathComponent: &*cursor_pdf];
  let info_path: Retained<NSString> =
    msg_send![&cursor_path, stringByAppendingPathComponent: &*cursor_plist];

  let image = NSImage::initByReferencingFile(NSImage::alloc(), &pdf_path).unwrap();
  #[allow(deprecated)]
  let info =
    NSDictionary::<AnyObject, AnyObject>::dictionaryWithContentsOfFile(&info_path).unwrap();
  let x = info.objectForKey(&key_x).unwrap();
  let y = info.objectForKey(&key_y).unwrap();
  let point = NSPoint::new(msg_send![&x, doubleValue], msg_send![&y, doubleValue]);
  let cursor: id = msg_send![class!(NSCursor), alloc];
  msg_send![
    cursor,
    initWithImage:&*image,
    hotSpot:point,
  ]
}

pub unsafe fn invisible_cursor() -> id {
  // 16x16 GIF data for invisible cursor
  // You can reproduce this via ImageMagick.
  // $ convert -size 16x16 xc:none cursor.gif
  static CURSOR_BYTES: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x10, 0x00, 0x10, 0x00, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00,
    0x10, 0x00, 0x10, 0x00, 0x00, 0x02, 0x0E, 0x84, 0x8F, 0xA9, 0xCB, 0xED, 0x0F, 0xA3, 0x9C, 0xB4,
    0xDA, 0x8B, 0xB3, 0x3E, 0x05, 0x00, 0x3B,
  ];

  thread_local! {
      // We can't initialize this at startup.
      static CURSOR_OBJECT: RefCell<id> = const { RefCell::new(nil) };
  }

  CURSOR_OBJECT.with(|cursor_obj| {
    if *cursor_obj.borrow() == nil {
      // Create a cursor from `CURSOR_BYTES`
      let cursor_data: id = msg_send![
        class!(NSData),
        dataWithBytesNoCopy:CURSOR_BYTES.as_ptr().cast::<c_void>(),
        length:CURSOR_BYTES.len(),
        freeWhenDone:NO,
      ];

      let ns_image: id = msg_send![class!(NSImage), alloc];
      let _: id = msg_send![ns_image, initWithData: cursor_data];
      let cursor: id = msg_send![class!(NSCursor), alloc];
      *cursor_obj.borrow_mut() =
        msg_send![cursor, initWithImage:ns_image, hotSpot: NSPoint::new(0.0, 0.0)];
    }
    *cursor_obj.borrow()
  })
}
