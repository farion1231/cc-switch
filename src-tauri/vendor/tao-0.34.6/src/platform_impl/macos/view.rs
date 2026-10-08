// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
#![allow(unused_unsafe)]
#![allow(deprecated)] // TODO: Use define_class!

use std::{
  boxed::Box,
  collections::{HashSet, VecDeque},
  ffi::CStr,
  os::raw::*,
  ptr,
  sync::{Arc, Mutex, Weak},
};

use objc2::{
  msg_send,
  rc::Retained,
  runtime::{
    AnyClass as Class, AnyObject as Object, AnyProtocol as Protocol, ClassBuilder as ClassDecl, Sel,
  },
  AllocAnyThread,
};
use objc2_app_kit::{
  NSApp, NSEvent, NSEventModifierFlags, NSEventPhase, NSView, NSWindow, NSWindowButton,
};
use objc2_foundation::{
  ns_string, MainThreadMarker, NSAttributedString, NSInteger, NSMutableAttributedString, NSPoint,
  NSRange, NSRect, NSSize, NSString, NSUInteger,
};
use once_cell::sync::Lazy;

use crate::{
  dpi::LogicalPosition,
  event::{
    DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
  },
  keyboard::{KeyCode, ModifiersState},
  platform_impl::platform::{
    app_state::AppState,
    event::{code_to_key, create_key_event, event_mods, get_scancode, EventWrapper},
    ffi::*,
    util::{self},
    window::get_window_id,
    DEVICE_ID,
  },
  window::WindowId,
};

pub struct CursorState {
  pub visible: bool,
  pub cursor: util::Cursor,
}

impl Default for CursorState {
  fn default() -> Self {
    Self {
      visible: true,
      cursor: Default::default(),
    }
  }
}

pub(super) struct ViewState {
  ns_window: objc2::rc::Weak<NSWindow>,
  pub cursor_state: Arc<Mutex<CursorState>>,
  ime_spot: Option<(f64, f64)>,

  /// This is true when we are currently modifying a marked text
  /// using ime. When the text gets commited, this is set to false.
  in_ime_preedit: bool,

  /// This is used to detect if a key-press causes an ime event.
  /// If a key-press does not cause an ime event, that means
  /// that the key-press cancelled the ime session. (Except arrow keys)
  key_triggered_ime: bool,
  // Not Needed Anymore
  //raw_characters: Option<String>,
  is_key_down: bool,
  pub(super) modifiers: ModifiersState,
  phys_modifiers: HashSet<KeyCode>,
  tracking_rect: Option<NSInteger>,
  pub(super) traffic_light_inset: Option<LogicalPosition<f64>>,
}

impl ViewState {
  fn get_scale_factor(&self) -> f64 {
    (unsafe { NSWindow::backingScaleFactor(&self.ns_window.load().unwrap()) }) as f64
  }
}

pub fn new_view(ns_window: &NSWindow) -> (Option<Retained<NSView>>, Weak<Mutex<CursorState>>) {
  let cursor_state = Default::default();
  let cursor_access = Arc::downgrade(&cursor_state);
  let state = ViewState {
    ns_window: objc2::rc::Weak::from(ns_window),
    cursor_state,
    ime_spot: None,
    in_ime_preedit: false,
    key_triggered_ime: false,
    is_key_down: false,
    modifiers: Default::default(),
    phys_modifiers: Default::default(),
    tracking_rect: None,
    traffic_light_inset: None,
  };
  unsafe {
    // This is free'd in `dealloc`
    let state_ptr = Box::into_raw(Box::new(state)) as *mut c_void;
    let ns_view = msg_send![VIEW_CLASS.0, alloc];
    (msg_send![ns_view, initWithTao: state_ptr], cursor_access)
  }
}

pub unsafe fn set_ime_position(ns_view: &NSView, input_context: id, x: f64, y: f64) {
  let state_ptr: *mut c_void = *ns_view.get_ivar("taoState");
  let state = &mut *(state_ptr as *mut ViewState);
  let content_rect = NSWindow::contentRectForFrameRect(
    &state.ns_window.load().unwrap(),
    NSWindow::frame(&state.ns_window.load().unwrap()),
  );
  let base_x = content_rect.origin.x as f64;
  let base_y = (content_rect.origin.y + content_rect.size.height) as f64;
  state.ime_spot = Some((base_x + x, base_y - y));
  let _: () = msg_send![input_context, invalidateCharacterCoordinates];
}

fn is_arrow_key(keycode: KeyCode) -> bool {
  matches!(
    keycode,
    KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight
  )
}

struct ViewClass(&'static Class);
unsafe impl Send for ViewClass {}
unsafe impl Sync for ViewClass {}

static VIEW_CLASS: Lazy<ViewClass> = Lazy::new(|| unsafe {
  let superclass = class!(NSView);
  let mut decl =
    ClassDecl::new(CStr::from_bytes_with_nul(b"TaoView\0").unwrap(), superclass).unwrap();
  decl.add_method(sel!(dealloc), dealloc as extern "C" fn(_, _));
  decl.add_method(
    sel!(initWithTao:),
    init_with_tao as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(viewDidMoveToWindow),
    view_did_move_to_window as extern "C" fn(_, _),
  );
  decl.add_method(sel!(drawRect:), draw_rect as extern "C" fn(_, _, _));
  decl.add_method(
    sel!(acceptsFirstResponder),
    accepts_first_responder as extern "C" fn(_, _) -> _,
  );
  decl.add_method(sel!(touchBar), touch_bar as extern "C" fn(_, _) -> _);
  decl.add_method(
    sel!(resetCursorRects),
    reset_cursor_rects as extern "C" fn(_, _),
  );
  decl.add_method(
    sel!(hasMarkedText),
    has_marked_text as extern "C" fn(_, _) -> _,
  );
  decl.add_method(sel!(markedRange), marked_range as extern "C" fn(_, _) -> _);
  decl.add_method(
    sel!(selectedRange),
    selected_range as extern "C" fn(_, _) -> _,
  );
  decl.add_method(
    sel!(setMarkedText:selectedRange:replacementRange:),
    set_marked_text as extern "C" fn(_, _, _, _, _),
  );
  decl.add_method(sel!(unmarkText), unmark_text as extern "C" fn(_, _));
  decl.add_method(
    sel!(validAttributesForMarkedText),
    valid_attributes_for_marked_text as extern "C" fn(_, _) -> _,
  );
  decl.add_method(
    sel!(attributedSubstringForProposedRange:actualRange:),
    attributed_substring_for_proposed_range as extern "C" fn(_, _, _, _) -> _,
  );
  decl.add_method(
    sel!(insertText:replacementRange:),
    insert_text as extern "C" fn(_, _, _, _),
  );
  decl.add_method(
    sel!(characterIndexForPoint:),
    character_index_for_point as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(firstRectForCharacterRange:actualRange:),
    first_rect_for_character_range as extern "C" fn(_, _, _, _) -> _,
  );
  decl.add_method(
    sel!(doCommandBySelector:),
    do_command_by_selector as extern "C" fn(_, _, _),
  );
  decl.add_method(sel!(keyDown:), key_down as extern "C" fn(_, _, _));
  decl.add_method(sel!(keyUp:), key_up as extern "C" fn(_, _, _));
  decl.add_method(sel!(flagsChanged:), flags_changed as extern "C" fn(_, _, _));
  decl.add_method(sel!(insertTab:), insert_tab as extern "C" fn(_, _, _));
  decl.add_method(
    sel!(insertBackTab:),
    insert_back_tab as extern "C" fn(_, _, _),
  );
  decl.add_method(sel!(mouseDown:), mouse_down as extern "C" fn(_, _, _));
  decl.add_method(sel!(mouseUp:), mouse_up as extern "C" fn(_, _, _));
  decl.add_method(
    sel!(rightMouseDown:),
    right_mouse_down as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(rightMouseUp:),
    right_mouse_up as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(otherMouseDown:),
    other_mouse_down as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(otherMouseUp:),
    other_mouse_up as extern "C" fn(_, _, _),
  );
  decl.add_method(sel!(mouseMoved:), mouse_moved as extern "C" fn(_, _, _));
  decl.add_method(sel!(mouseDragged:), mouse_dragged as extern "C" fn(_, _, _));
  decl.add_method(
    sel!(rightMouseDragged:),
    right_mouse_dragged as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(otherMouseDragged:),
    other_mouse_dragged as extern "C" fn(_, _, _),
  );
  decl.add_method(sel!(mouseEntered:), mouse_entered as extern "C" fn(_, _, _));
  decl.add_method(sel!(mouseExited:), mouse_exited as extern "C" fn(_, _, _));
  decl.add_method(sel!(scrollWheel:), scroll_wheel as extern "C" fn(_, _, _));
  decl.add_method(
    sel!(pressureChangeWithEvent:),
    pressure_change_with_event as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(_wantsKeyDownForEvent:),
    wants_key_down_for_event as extern "C" fn(_, _, _) -> _,
  );
  decl.add_method(
    sel!(cancelOperation:),
    cancel_operation as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(frameDidChange:),
    frame_did_change as extern "C" fn(_, _, _),
  );
  decl.add_method(
    sel!(acceptsFirstMouse:),
    accepts_first_mouse as extern "C" fn(_, _, _) -> _,
  );
  decl.add_ivar::<*mut c_void>(CStr::from_bytes_with_nul(b"taoState\0").unwrap());
  decl.add_ivar::<id>(CStr::from_bytes_with_nul(b"markedText\0").unwrap());
  let protocol = Protocol::get(CStr::from_bytes_with_nul(b"NSTextInputClient\0").unwrap()).unwrap();
  decl.add_protocol(protocol);
  ViewClass(decl.register())
});

extern "C" fn dealloc(this: &Object, _sel: Sel) {
  unsafe {
    let state: *mut c_void = *this.get_ivar("taoState");
    let marked_text: *mut NSMutableAttributedString = *this.get_ivar("markedText");
    let _: () = msg_send![marked_text, release];
    drop(Box::from_raw(state as *mut ViewState));
  }
}

extern "C" fn init_with_tao(this: &Object, _sel: Sel, state: *mut c_void) -> id {
  unsafe {
    let this: id = msg_send![this, init];
    if this != nil {
      *(*this).get_mut_ivar("taoState") = state;
      let marked_text = Retained::into_raw(NSMutableAttributedString::new());
      *(*this).get_mut_ivar("markedText") = marked_text;
      let _: () = msg_send![this, setPostsFrameChangedNotifications: YES];

      let notification_center: &Object = msg_send![class!(NSNotificationCenter), defaultCenter];
      let notification_name = ns_string!("NSViewFrameDidChangeNotification");
      let _: () = msg_send![
          notification_center,
          addObserver: this
          selector: sel!(frameDidChange:)
          name: &*notification_name
          object: this
      ];
    }
    this
  }
}

extern "C" fn view_did_move_to_window(this: &Object, _sel: Sel) {
  trace!("Triggered `viewDidMoveToWindow`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    if let Some(tracking_rect) = state.tracking_rect.take() {
      let _: () = msg_send![this, removeTrackingRect: tracking_rect];
    }

    let rect: NSRect = msg_send![this, visibleRect];
    let tracking_rect: NSInteger = msg_send![this,
        addTrackingRect:rect
        owner:this
        userData:ptr::null_mut::<c_void>()
        assumeInside:NO
    ];
    state.tracking_rect = Some(tracking_rect);
  }
  trace!("Completed `viewDidMoveToWindow`");
}

extern "C" fn frame_did_change(this: &Object, _sel: Sel, _event: id) {
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    if let Some(tracking_rect) = state.tracking_rect.take() {
      let _: () = msg_send![this, removeTrackingRect: tracking_rect];
    }

    let rect: NSRect = msg_send![this, visibleRect];
    let tracking_rect: NSInteger = msg_send![this,
        addTrackingRect:rect
        owner:this
        userData:ptr::null_mut::<c_void>()
        assumeInside:NO
    ];

    state.tracking_rect = Some(tracking_rect);
  }
}

extern "C" fn draw_rect(this: &Object, _sel: Sel, rect: NSRect) {
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    if let Some(position) = state.traffic_light_inset {
      let window = state.ns_window.load().unwrap();
      inset_traffic_lights(&window, position);
    }

    AppState::handle_redraw(WindowId(get_window_id(&state.ns_window.load().unwrap())));

    let superclass = util::superclass(this);
    let () = msg_send![super(this, superclass), drawRect: rect];
  }
}

extern "C" fn accepts_first_responder(_this: &Object, _sel: Sel) -> BOOL {
  YES
}

// This is necessary to prevent a beefy terminal error on MacBook Pros:
// IMKInputSession [0x7fc573576ff0 presentFunctionRowItemTextInputViewWithEndpoint:completionHandler:] : [self textInputContext]=0x7fc573558e10 *NO* NSRemoteViewController to client, NSError=Error Domain=NSCocoaErrorDomain Code=4099 "The connection from pid 0 was invalidated from this process." UserInfo={NSDebugDescription=The connection from pid 0 was invalidated from this process.}, com.apple.inputmethod.EmojiFunctionRowItem
// TODO: Add an API extension for using `NSTouchBar`
extern "C" fn touch_bar(_this: &Object, _sel: Sel) -> id {
  nil
}

extern "C" fn reset_cursor_rects(this: &Object, _sel: Sel) {
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let bounds: NSRect = msg_send![this, bounds];
    let cursor_state = state.cursor_state.lock().unwrap();
    let cursor = if cursor_state.visible {
      cursor_state.cursor.load()
    } else {
      util::invisible_cursor()
    };

    if !cursor.is_null() {
      let _: () = msg_send![this,
          addCursorRect:bounds
          cursor:cursor
      ];
    }
  }
}

extern "C" fn has_marked_text(this: &Object, _sel: Sel) -> BOOL {
  unsafe {
    trace!("Triggered `hasMarkedText`");
    let marked_text: &NSMutableAttributedString = *this.get_ivar("markedText");
    trace!("Completed `hasMarkedText`");
    (marked_text.length() > 0).into()
  }
}

extern "C" fn marked_range(this: &Object, _sel: Sel) -> NSRange {
  unsafe {
    trace!("Triggered `markedRange`");
    let marked_text: &NSMutableAttributedString = *this.get_ivar("markedText");
    let length = marked_text.length();
    trace!("Completed `markedRange`");
    if length > 0 {
      NSRange::new(0, length - 1)
    } else {
      util::EMPTY_RANGE
    }
  }
}

extern "C" fn selected_range(_this: &Object, _sel: Sel) -> NSRange {
  trace!("Triggered `selectedRange`");
  trace!("Completed `selectedRange`");
  util::EMPTY_RANGE
}

/// An IME pre-edit operation happened, changing the text that's
/// currently being pre-edited. This more or less corresponds to
/// the `compositionupdate` event on the web.
extern "C" fn set_marked_text(
  this: &mut Object,
  _sel: Sel,
  string: id,
  _selected_range: NSRange,
  _replacement_range: NSRange,
) {
  trace!("Triggered `setMarkedText`");
  unsafe {
    let has_attr: bool = msg_send![string, isKindOfClass: class!(NSAttributedString)];
    let marked_text = if has_attr {
      NSMutableAttributedString::initWithAttributedString(
        NSMutableAttributedString::alloc(),
        &*(string as *const NSAttributedString),
      )
    } else {
      NSMutableAttributedString::initWithString(
        NSMutableAttributedString::alloc(),
        &*(string as *const NSString),
      )
    };
    let marked_text_ref: &mut *mut NSMutableAttributedString = this.get_mut_ivar("markedText");
    let () = msg_send![(*marked_text_ref), release];
    *marked_text_ref = Retained::into_raw(marked_text);

    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);
    state.in_ime_preedit = true;
    state.key_triggered_ime = true;
  }
  trace!("Completed `setMarkedText`");
}

extern "C" fn unmark_text(this: &mut Object, _sel: Sel) {
  trace!("Triggered `unmarkText`");
  unsafe {
    let marked_text_ref: &mut *mut NSMutableAttributedString = this.get_mut_ivar("markedText");
    let () = msg_send![(*marked_text_ref), release];
    *marked_text_ref = Retained::into_raw(NSMutableAttributedString::new());
    let input_context: id = msg_send![this, inputContext];
    let _: () = msg_send![input_context, discardMarkedText];
  }
  trace!("Completed `unmarkText`");
}

extern "C" fn valid_attributes_for_marked_text(_this: &Object, _sel: Sel) -> id {
  trace!("Triggered `validAttributesForMarkedText`");
  trace!("Completed `validAttributesForMarkedText`");
  unsafe { msg_send![class!(NSArray), array] }
}

extern "C" fn attributed_substring_for_proposed_range(
  _this: &Object,
  _sel: Sel,
  _range: NSRange,
  _actual_range: *mut c_void, // *mut NSRange
) -> id {
  trace!("Triggered `attributedSubstringForProposedRange`");
  trace!("Completed `attributedSubstringForProposedRange`");
  nil
}

extern "C" fn character_index_for_point(_this: &Object, _sel: Sel, _point: NSPoint) -> NSUInteger {
  trace!("Triggered `characterIndexForPoint`");
  trace!("Completed `characterIndexForPoint`");
  0
}

extern "C" fn first_rect_for_character_range(
  this: &Object,
  _sel: Sel,
  _range: NSRange,
  _actual_range: *mut c_void, // *mut NSRange
) -> NSRect {
  unsafe {
    trace!("Triggered `firstRectForCharacterRange`");
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);
    let (x, y) = state.ime_spot.unwrap_or_else(|| {
      let content_rect = NSWindow::contentRectForFrameRect(
        &state.ns_window.load().unwrap(),
        NSWindow::frame(&state.ns_window.load().unwrap()),
      );
      let x = content_rect.origin.x;
      let y = util::bottom_left_to_top_left(content_rect);
      (x, y)
    });
    trace!("Completed `firstRectForCharacterRange`");
    NSRect::new(NSPoint::new(x as _, y as _), NSSize::new(0.0, 0.0))
  }
}

extern "C" fn insert_text(
  this: &Object,
  _sel: Sel,
  string: &NSString,
  _replacement_range: NSRange,
) {
  trace!("Triggered `insertText`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let has_attr: bool = msg_send![string, isKindOfClass: class!(NSAttributedString)];
    let characters = if has_attr {
      // This is a *mut NSAttributedString
      msg_send![string, string]
    } else {
      // This is already a *mut NSString
      string
    };

    let string: String = characters
      .to_string()
      .chars()
      .filter(|c| !is_corporate_character(*c))
      .collect();
    state.is_key_down = true;

    // We don't need this now, but it's here if that changes.
    //let event: id = msg_send![NSApp(), currentEvent];

    AppState::queue_event(EventWrapper::StaticEvent(Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::ReceivedImeText(string),
    }));
    if state.in_ime_preedit {
      state.in_ime_preedit = false;
      state.key_triggered_ime = true;
    }
  }
  trace!("Completed `insertText`");
}

extern "C" fn do_command_by_selector(_this: &Object, _sel: Sel, _command: Sel) {
  trace!("Triggered `doCommandBySelector`");
  // TODO: (Artur) all these inputs seem to trigger a key event with the correct text
  // content so this is not needed anymore, it seems.

  // Basically, we're sent this message whenever a keyboard event that doesn't generate a "human readable" character
  // happens, i.e. newlines, tabs, and Ctrl+C.

  // unsafe {
  //     let state_ptr: *mut c_void = *this.get_ivar("taoState");
  //     let state = &mut *(state_ptr as *mut ViewState);

  //     let mut events = VecDeque::with_capacity(1);
  //     if command == sel!(insertNewline:) {
  //         // The `else` condition would emit the same character, but I'm keeping this here both...
  //         // 1) as a reminder for how `doCommandBySelector` works
  //         // 2) to make our use of carriage return explicit
  //         events.push_back(EventWrapper::StaticEvent(Event::WindowEvent {
  //             window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
  //             event: WindowEvent::ReceivedCharacter('\r'),
  //         }));
  //     } else {
  //         let raw_characters = state.raw_characters.take();
  //         if let Some(raw_characters) = raw_characters {
  //             for character in raw_characters
  //                 .chars()
  //                 .filter(|c| !is_corporate_character(*c))
  //             {
  //                 events.push_back(EventWrapper::StaticEvent(Event::WindowEvent {
  //                     window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
  //                     event: WindowEvent::ReceivedCharacter(character),
  //                 }));
  //             }
  //         }
  //     };
  //     AppState::queue_events(events);
  // }

  trace!("Completed `doCommandBySelector`");
}

// Get characters
// fn get_characters(event: id, ignore_modifiers: bool) -> String {
//   unsafe {
//     let characters: id = if ignore_modifiers {
//       msg_send![event, charactersIgnoringModifiers]
//     } else {
//       msg_send![event, characters]
//     };

//     assert_ne!(characters, nil);
//     let slice = slice::from_raw_parts(characters.UTF8String() as *const c_uchar, characters.len());

//     let string = str::from_utf8_unchecked(slice);
//     string.to_owned()
//   }
// }

// As defined in: https://www.unicode.org/Public/MAPPINGS/VENDORS/APPLE/CORPCHAR.TXT
fn is_corporate_character(c: char) -> bool {
  #[allow(clippy::match_like_matches_macro)]
  match c {
    '\u{F700}'..='\u{F747}'
    | '\u{F802}'..='\u{F84F}'
    | '\u{F850}'
    | '\u{F85C}'
    | '\u{F85D}'
    | '\u{F85F}'
    | '\u{F860}'..='\u{F86B}'
    | '\u{F870}'..='\u{F8FF}' => true,
    _ => false,
  }
}

// // Retrieves a layout-independent keycode given an event.
// fn retrieve_keycode(event: id) -> Option<VirtualKeyCode> {
//     #[inline]
//     fn get_code(ev: id, raw: bool) -> Option<VirtualKeyCode> {
//         let characters = get_characters(ev, raw);
//         characters.chars().next().and_then(|c| char_to_keycode(c))
//     }

//     // Cmd switches Roman letters for Dvorak-QWERTY layout, so we try modified characters first.
//     // If we don't get a match, then we fall back to unmodified characters.
//     let code = get_code(event, false).or_else(|| get_code(event, true));

//     // We've checked all layout related keys, so fall through to scancode.
//     // Reaching this code means that the key is layout-independent (e.g. Backspace, Return).
//     //
//     // We're additionally checking here for F21-F24 keys, since their keycode
//     // can vary, but we know that they are encoded
//     // in characters property.
//     code.or_else(|| {
//         let scancode = get_scancode(event);
//         scancode_to_keycode(scancode).or_else(|| check_function_keys(&get_characters(event, true)))
//     })
// }

// Update `state.modifiers` if `event` has something different
fn update_potentially_stale_modifiers(state: &mut ViewState, event: &NSEvent) {
  let event_modifiers = event_mods(event);
  if state.modifiers != event_modifiers {
    state.modifiers = event_modifiers;

    AppState::queue_event(EventWrapper::StaticEvent(Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::ModifiersChanged(state.modifiers),
    }));
  }
}

extern "C" fn key_down(this: &mut Object, _sel: Sel, event: &NSEvent) {
  trace!("Triggered `keyDown`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);
    let window_id = WindowId(get_window_id(&state.ns_window.load().unwrap()));

    // keyboard refactor: don't seems to be needed anymore
    // let characters = get_characters(event, false);
    //state.raw_characters = Some(characters);

    let is_repeat: bool = msg_send![event, isARepeat];

    update_potentially_stale_modifiers(state, event);

    let pass_along = !is_repeat || !state.is_key_down;
    if pass_along {
      // See below for why we do this.
      let marked_text_ref: &mut *mut NSMutableAttributedString = this.get_mut_ivar("markedText");
      let () = msg_send![(*marked_text_ref), release];
      *marked_text_ref = Retained::into_raw(NSMutableAttributedString::new());
      state.key_triggered_ime = false;

      // Some keys (and only *some*, with no known reason) don't trigger `insertText`, while others do...
      // So, we don't give repeats the opportunity to trigger that, since otherwise our hack will cause some
      // keys to generate twice as many characters.
      let array: id = msg_send![class!(NSArray), arrayWithObject: event];
      let () = msg_send![&*this, interpretKeyEvents: array];
    }
    // The `interpretKeyEvents` above, may invoke `set_marked_text` or `insert_text`,
    // if the event corresponds to an IME event.
    let in_ime = state.key_triggered_ime;
    let key_event = create_key_event(event, true, is_repeat, in_ime, None);
    let is_arrow_key = is_arrow_key(key_event.physical_key);
    if pass_along {
      // The `interpretKeyEvents` above, may invoke `set_marked_text` or `insert_text`,
      // if the event corresponds to an IME event.
      // If `set_marked_text` or `insert_text` were not invoked, then the IME was deactivated,
      // and we should cancel the IME session.
      // When using arrow keys in an IME window, the input context won't invoke the
      // IME related methods, so in that case we shouldn't cancel the IME session.
      let is_preediting: bool = state.in_ime_preedit;
      if is_preediting && !state.key_triggered_ime && !is_arrow_key {
        // In this case we should cancel the IME session.
        let () = msg_send![this, unmarkText];
        state.in_ime_preedit = false;
      }
    }
    let window_event = Event::WindowEvent {
      window_id,
      event: WindowEvent::KeyboardInput {
        device_id: DEVICE_ID,
        event: key_event,
        is_synthetic: false,
      },
    };
    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `keyDown`");
}

extern "C" fn key_up(this: &Object, _sel: Sel, event: &NSEvent) {
  trace!("Triggered `keyUp`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    state.is_key_down = false;

    update_potentially_stale_modifiers(state, event);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::KeyboardInput {
        device_id: DEVICE_ID,
        event: create_key_event(event, false, false, false, None),
        is_synthetic: false,
      },
    };
    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `keyUp`");
}

extern "C" fn flags_changed(this: &Object, _sel: Sel, ns_event: &NSEvent) {
  use KeyCode::{
    AltLeft, AltRight, ControlLeft, ControlRight, ShiftLeft, ShiftRight, SuperLeft, SuperRight,
  };

  trace!("Triggered `flagsChanged`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let scancode = get_scancode(ns_event);

    // We'll correct the `is_press` and the `key_override` below.
    let event = create_key_event(ns_event, false, false, false, Some(KeyCode::SuperLeft));
    let mut events = VecDeque::with_capacity(4);

    macro_rules! process_event {
      ($tao_flag:expr, $ns_flag:expr, $target_key:expr) => {
        let ns_event_contains_keymask = NSEvent::modifierFlags(ns_event).contains($ns_flag);
        let mut actual_key = KeyCode::from_scancode(scancode as u32);
        let was_pressed = state.phys_modifiers.contains(&actual_key);
        let mut is_pressed = ns_event_contains_keymask;
        let matching_keys;
        if actual_key == KeyCode::KeyA {
          // When switching keyboard layout using Ctrl+Space, the Ctrl release event
          // has `KeyA` as its keycode which would produce an incorrect key event.
          // To avoid this, we detect this scenario and override the key with one
          // that should be reasonable
          actual_key = $target_key;
          matching_keys = true;
        } else {
          matching_keys = actual_key == $target_key;
          if matching_keys && is_pressed && was_pressed {
            // If we received a modifier flags changed event AND the key that triggered
            // it was already pressed then this must mean that this event indicates the
            // release of that key.
            is_pressed = false;
          }
        }
        if matching_keys {
          if is_pressed != was_pressed {
            let mut event = event.clone();
            event.state = if is_pressed {
              ElementState::Pressed
            } else {
              ElementState::Released
            };
            event.physical_key = actual_key;
            event.logical_key = code_to_key(event.physical_key, scancode);
            events.push_back(WindowEvent::KeyboardInput {
              device_id: DEVICE_ID,
              event,
              is_synthetic: false,
            });
            if is_pressed {
              state.phys_modifiers.insert($target_key);
            } else {
              state.phys_modifiers.remove(&$target_key);
            }
            if ns_event_contains_keymask {
              state.modifiers.insert($tao_flag);
            } else {
              state.modifiers.remove($tao_flag);
            }
          }
        }
      };
    }
    process_event!(
      ModifiersState::SHIFT,
      NSEventModifierFlags::Shift,
      ShiftLeft
    );
    process_event!(
      ModifiersState::SHIFT,
      NSEventModifierFlags::Shift,
      ShiftRight
    );
    process_event!(
      ModifiersState::CONTROL,
      NSEventModifierFlags::Control,
      ControlLeft
    );
    process_event!(
      ModifiersState::CONTROL,
      NSEventModifierFlags::Control,
      ControlRight
    );
    process_event!(ModifiersState::ALT, NSEventModifierFlags::Option, AltLeft);
    process_event!(ModifiersState::ALT, NSEventModifierFlags::Option, AltRight);
    process_event!(
      ModifiersState::SUPER,
      NSEventModifierFlags::Command,
      SuperLeft
    );
    process_event!(
      ModifiersState::SUPER,
      NSEventModifierFlags::Command,
      SuperRight
    );

    let window_id = WindowId(get_window_id(&state.ns_window.load().unwrap()));

    for event in events {
      AppState::queue_event(EventWrapper::StaticEvent(Event::WindowEvent {
        window_id,
        event,
      }));
    }

    AppState::queue_event(EventWrapper::StaticEvent(Event::WindowEvent {
      window_id,
      event: WindowEvent::ModifiersChanged(state.modifiers),
    }));
  }
  trace!("Completed `flagsChanged`");
}

extern "C" fn insert_tab(this: &Object, _sel: Sel, _sender: id) {
  unsafe {
    let window: id = msg_send![this, window];
    let first_responder: id = msg_send![window, firstResponder];
    let this_ptr = this as *const _ as *mut _;
    if first_responder == this_ptr {
      let (): _ = msg_send![window, selectNextKeyView: this];
    }
  }
}

extern "C" fn insert_back_tab(this: &Object, _sel: Sel, _sender: id) {
  unsafe {
    let window: id = msg_send![this, window];
    let first_responder: id = msg_send![window, firstResponder];
    let this_ptr = this as *const _ as *mut _;
    if first_responder == this_ptr {
      let (): _ = msg_send![window, selectPreviousKeyView: this];
    }
  }
}

// Allows us to receive Cmd-. (the shortcut for closing a dialog)
// https://bugs.eclipse.org/bugs/show_bug.cgi?id=300620#c6
extern "C" fn cancel_operation(this: &Object, _sel: Sel, _sender: id) {
  trace!("Triggered `cancelOperation`");
  unsafe {
    let mtm = MainThreadMarker::new_unchecked();
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let event: Retained<NSEvent> = msg_send![&NSApp(mtm), currentEvent];
    update_potentially_stale_modifiers(state, &event);

    let scancode = 0x2f;
    let key = KeyCode::from_scancode(scancode);
    debug_assert_eq!(key, KeyCode::Period);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::KeyboardInput {
        device_id: DEVICE_ID,
        event: create_key_event(&event, true, false, false, Some(key)),
        is_synthetic: false,
      },
    };
    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `cancelOperation`");
}

fn mouse_click(this: &Object, event: &NSEvent, button: MouseButton, button_state: ElementState) {
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    update_potentially_stale_modifiers(state, event);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::MouseInput {
        device_id: DEVICE_ID,
        state: button_state,
        button,
        modifiers: event_mods(event),
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
}

extern "C" fn mouse_down(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Left, ElementState::Pressed);
}

extern "C" fn mouse_up(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Left, ElementState::Released);
}

extern "C" fn right_mouse_down(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Right, ElementState::Pressed);
}

extern "C" fn right_mouse_up(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Right, ElementState::Released);
}

extern "C" fn other_mouse_down(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Middle, ElementState::Pressed);
}

extern "C" fn other_mouse_up(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
  mouse_click(this, event, MouseButton::Middle, ElementState::Released);
}

fn mouse_motion(this: &NSView, event: &NSEvent) {
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let window_point = event.locationInWindow();
    let view_point = this.convertPoint_fromView(window_point, None);
    let view_rect = NSView::frame(this);

    if view_point.x.is_sign_negative()
      || view_point.y.is_sign_negative()
      || view_point.x > view_rect.size.width
      || view_point.y > view_rect.size.height
    {
      let mouse_buttons_down: NSUInteger = msg_send![class!(NSEvent), pressedMouseButtons];
      if mouse_buttons_down == 0 {
        // Point is outside of the client area (view) and no buttons are pressed
        return;
      }
    }

    let x = view_point.x as f64;
    let y = view_rect.size.height as f64 - view_point.y as f64;
    let logical_position = LogicalPosition::new(x, y);

    update_potentially_stale_modifiers(state, event);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::CursorMoved {
        device_id: DEVICE_ID,
        position: logical_position.to_physical(state.get_scale_factor()),
        modifiers: event_mods(event),
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
}

extern "C" fn mouse_moved(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
}

extern "C" fn mouse_dragged(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
}

extern "C" fn right_mouse_dragged(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
}

extern "C" fn other_mouse_dragged(this: &NSView, _sel: Sel, event: &NSEvent) {
  mouse_motion(this, event);
}

extern "C" fn mouse_entered(this: &Object, _sel: Sel, _event: id) {
  trace!("Triggered `mouseEntered`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let enter_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::CursorEntered {
        device_id: DEVICE_ID,
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(enter_event));
  }
  trace!("Completed `mouseEntered`");
}

extern "C" fn mouse_exited(this: &Object, _sel: Sel, _event: id) {
  trace!("Triggered `mouseExited`");
  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::CursorLeft {
        device_id: DEVICE_ID,
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `mouseExited`");
}

extern "C" fn scroll_wheel(this: &NSView, _sel: Sel, event: &NSEvent) {
  trace!("Triggered `scrollWheel`");

  mouse_motion(this, event);

  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let delta = {
      // macOS horizontal sign convention is the inverse of tao.
      let (x, y) = (event.scrollingDeltaX() * -1.0, event.scrollingDeltaY());
      if event.hasPreciseScrollingDeltas() {
        let delta = LogicalPosition::new(x, y).to_physical(state.get_scale_factor());
        MouseScrollDelta::PixelDelta(delta)
      } else {
        MouseScrollDelta::LineDelta(x as f32, y as f32)
      }
    };
    let phase = match event.phase() {
      NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
      NSEventPhase::Ended => TouchPhase::Ended,
      _ => TouchPhase::Moved,
    };

    let device_event = Event::DeviceEvent {
      device_id: DEVICE_ID,
      event: DeviceEvent::MouseWheel { delta },
    };

    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    update_potentially_stale_modifiers(state, event);

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::MouseWheel {
        device_id: DEVICE_ID,
        delta,
        phase,
        modifiers: event_mods(event),
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(device_event));
    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `scrollWheel`");
}

extern "C" fn pressure_change_with_event(this: &NSView, _sel: Sel, event: &NSEvent) {
  trace!("Triggered `pressureChangeWithEvent`");

  mouse_motion(this, event);

  unsafe {
    let state_ptr: *mut c_void = *this.get_ivar("taoState");
    let state = &mut *(state_ptr as *mut ViewState);

    let pressure = event.pressure();
    let stage = event.stage();

    let window_event = Event::WindowEvent {
      window_id: WindowId(get_window_id(&state.ns_window.load().unwrap())),
      event: WindowEvent::TouchpadPressure {
        device_id: DEVICE_ID,
        pressure,
        stage: stage as i64,
      },
    };

    AppState::queue_event(EventWrapper::StaticEvent(window_event));
  }
  trace!("Completed `pressureChangeWithEvent`");
}

// Allows us to receive Ctrl-Tab and Ctrl-Esc.
// Note that this *doesn't* help with any missing Cmd inputs.
// https://github.com/chromium/chromium/blob/a86a8a6bcfa438fa3ac2eba6f02b3ad1f8e0756f/ui/views/cocoa/bridged_content_view.mm#L816
extern "C" fn wants_key_down_for_event(_this: &Object, _sel: Sel, _event: id) -> BOOL {
  YES
}

extern "C" fn accepts_first_mouse(_this: &Object, _sel: Sel, _event: id) -> BOOL {
  YES
}

pub unsafe fn inset_traffic_lights(window: &NSWindow, position: LogicalPosition<f64>) {
  let (x, y) = (position.x, position.y);

  let close = window
    .standardWindowButton(NSWindowButton::CloseButton)
    .unwrap();
  let miniaturize = window
    .standardWindowButton(NSWindowButton::MiniaturizeButton)
    .unwrap();
  let zoom = window
    .standardWindowButton(NSWindowButton::ZoomButton)
    .unwrap();

  let title_bar_container_view = close.superview().unwrap().superview().unwrap();

  let close_rect = NSView::frame(&close);
  let title_bar_frame_height = close_rect.size.height + y;
  let mut title_bar_rect = NSView::frame(&title_bar_container_view);
  title_bar_rect.size.height = title_bar_frame_height;
  title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
  let _: () = msg_send![&title_bar_container_view, setFrame: title_bar_rect];

  let window_buttons = vec![close, miniaturize.clone(), zoom];
  let space_between = NSView::frame(&miniaturize).origin.x - close_rect.origin.x;

  for (i, button) in window_buttons.into_iter().enumerate() {
    let mut rect = NSView::frame(&button);
    rect.origin.x = x + (i as f64 * space_between);
    button.setFrameOrigin(rect.origin);
  }
}
