// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, ffi::c_void, os::raw::c_ushort, sync::Mutex};

use objc2::{msg_send, rc::Retained};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSWindow};
use once_cell::sync::Lazy;

use core_foundation::{base::CFRelease, data::CFDataGetBytePtr};
use objc2_foundation::NSString;

use crate::{
  dpi::LogicalSize,
  event::{ElementState, Event, KeyEvent},
  keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NativeKeyCode},
  platform_impl::platform::{ffi, util::Never},
};

static KEY_STRINGS: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn insert_or_get_key_str(string: String) -> &'static str {
  let mut string_set = KEY_STRINGS.lock().unwrap();
  if let Some(contained) = string_set.get(string.as_str()) {
    return contained;
  }
  let static_str = Box::leak(string.into_boxed_str());
  string_set.insert(static_str);
  static_str
}

#[non_exhaustive]
#[derive(Debug)]
pub enum EventWrapper {
  StaticEvent(Event<'static, Never>),
  EventProxy(EventProxy),
}

#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum EventProxy {
  #[non_exhaustive]
  DpiChangedProxy {
    ns_window: Retained<NSWindow>,
    suggested_size: LogicalSize<f64>,
    scale_factor: f64,
  },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEventExtra {
  pub text_with_all_modifiers: Option<&'static str>,
  pub key_without_modifiers: Key<'static>,
}

pub fn get_modifierless_char(scancode: u16) -> Key<'static> {
  let mut string = [0; 16];
  let input_source;
  let layout;
  unsafe {
    input_source = ffi::TISCopyCurrentKeyboardLayoutInputSource();
    if input_source.is_null() {
      log::error!("`TISCopyCurrentKeyboardLayoutInputSource` returned null ptr");
      return Key::Unidentified(NativeKeyCode::MacOS(scancode));
    }
    let layout_data =
      ffi::TISGetInputSourceProperty(input_source, ffi::kTISPropertyUnicodeKeyLayoutData);
    if layout_data.is_null() {
      CFRelease(input_source as *mut c_void);
      log::error!("`TISGetInputSourceProperty` returned null ptr");
      return Key::Unidentified(NativeKeyCode::MacOS(scancode));
    }
    layout = CFDataGetBytePtr(layout_data) as *const ffi::UCKeyboardLayout;
  }
  let keyboard_type = unsafe { ffi::LMGetKbdType() };

  let mut result_len = 0;
  let mut dead_keys = 0;
  let modifiers = 0;
  let translate_result = unsafe {
    ffi::UCKeyTranslate(
      layout,
      scancode,
      ffi::kUCKeyActionDisplay,
      modifiers,
      keyboard_type as u32,
      ffi::kUCKeyTranslateNoDeadKeysMask,
      &mut dead_keys,
      string.len() as ffi::UniCharCount,
      &mut result_len,
      string.as_mut_ptr(),
    )
  };
  unsafe {
    CFRelease(input_source as *mut c_void);
  }
  if translate_result != 0 {
    log::error!(
      "`UCKeyTranslate` returned with the non-zero value: {}",
      translate_result
    );
    return Key::Unidentified(NativeKeyCode::MacOS(scancode));
  }
  if result_len == 0 {
    log::error!("`UCKeyTranslate` was succesful but gave a string of 0 length.");
    return Key::Unidentified(NativeKeyCode::MacOS(scancode));
  }
  let chars = String::from_utf16_lossy(&string[0..result_len as usize]);
  Key::Character(insert_or_get_key_str(chars))
}

fn get_logical_key_char(ns_event: &NSEvent, modifierless_chars: &str) -> Key<'static> {
  let characters: Retained<NSString> = unsafe { msg_send![ns_event, charactersIgnoringModifiers] };
  let string = characters.to_string();
  if string.is_empty() {
    // Probably a dead key
    let first_char = modifierless_chars.chars().next();
    return Key::Dead(first_char);
  }
  Key::Character(insert_or_get_key_str(string))
}

#[allow(clippy::unnecessary_unwrap)]
pub fn create_key_event(
  ns_event: &NSEvent,
  is_press: bool,
  is_repeat: bool,
  in_ime: bool,
  key_override: Option<KeyCode>,
) -> KeyEvent {
  use ElementState::{Pressed, Released};
  let state = if is_press { Pressed } else { Released };

  let scancode = get_scancode(ns_event);
  let mut physical_key = key_override.unwrap_or_else(|| KeyCode::from_scancode(scancode as u32));

  let text_with_all_modifiers: Option<&'static str> = {
    if key_override.is_some() {
      None
    } else {
      let characters: Retained<NSString> = unsafe { msg_send![ns_event, characters] };
      let characters = characters.to_string();
      if characters.is_empty() {
        None
      } else {
        if matches!(physical_key, KeyCode::Unidentified(_)) {
          // The key may be one of the funky function keys
          physical_key = extra_function_key_to_code(scancode, &characters);
        }
        Some(insert_or_get_key_str(characters))
      }
    }
  };
  let key_from_code = code_to_key(physical_key, scancode);
  let logical_key;
  let key_without_modifiers;
  if !matches!(key_from_code, Key::Unidentified(_)) {
    logical_key = key_from_code.clone();
    key_without_modifiers = key_from_code;
  } else {
    //#[cfg(debug_assertions)] println!("Couldn't get key from code: {:?}", physical_key);
    key_without_modifiers = get_modifierless_char(scancode);

    let modifiers = unsafe { NSEvent::modifierFlags(ns_event) };
    let has_alt = modifiers.contains(NSEventModifierFlags::Option);
    let has_ctrl = modifiers.contains(NSEventModifierFlags::Control);
    if has_alt || has_ctrl || text_with_all_modifiers.is_none() || !is_press {
      let modifierless_chars = match key_without_modifiers.clone() {
        Key::Character(ch) => ch,
        _ => "",
      };
      logical_key = get_logical_key_char(ns_event, modifierless_chars);
    } else {
      logical_key = Key::Character(text_with_all_modifiers.unwrap());
    }
  }
  let text = if in_ime || !is_press {
    None
  } else {
    logical_key.to_text()
  };
  KeyEvent {
    location: code_to_location(physical_key),
    logical_key,
    physical_key,
    repeat: is_repeat,
    state,
    text,
    platform_specific: KeyEventExtra {
      text_with_all_modifiers,
      key_without_modifiers,
    },
  }
}

pub fn code_to_key(code: KeyCode, scancode: u16) -> Key<'static> {
  match code {
    KeyCode::Enter => Key::Enter,
    KeyCode::Tab => Key::Tab,
    KeyCode::Space => Key::Space,
    KeyCode::Backspace => Key::Backspace,
    KeyCode::Escape => Key::Escape,
    KeyCode::SuperRight => Key::Super,
    KeyCode::SuperLeft => Key::Super,
    KeyCode::ShiftLeft => Key::Shift,
    KeyCode::AltLeft => Key::Alt,
    KeyCode::ControlLeft => Key::Control,
    KeyCode::ShiftRight => Key::Shift,
    KeyCode::AltRight => Key::Alt,
    KeyCode::ControlRight => Key::Control,

    KeyCode::NumLock => Key::NumLock,
    KeyCode::AudioVolumeUp => Key::AudioVolumeUp,
    KeyCode::AudioVolumeDown => Key::AudioVolumeDown,

    // Other numpad keys all generate text on macOS (if I understand correctly)
    KeyCode::NumpadEnter => Key::Enter,

    KeyCode::F1 => Key::F1,
    KeyCode::F2 => Key::F2,
    KeyCode::F3 => Key::F3,
    KeyCode::F4 => Key::F4,
    KeyCode::F5 => Key::F5,
    KeyCode::F6 => Key::F6,
    KeyCode::F7 => Key::F7,
    KeyCode::F8 => Key::F8,
    KeyCode::F9 => Key::F9,
    KeyCode::F10 => Key::F10,
    KeyCode::F11 => Key::F11,
    KeyCode::F12 => Key::F12,
    KeyCode::F13 => Key::F13,
    KeyCode::F14 => Key::F14,
    KeyCode::F15 => Key::F15,
    KeyCode::F16 => Key::F16,
    KeyCode::F17 => Key::F17,
    KeyCode::F18 => Key::F18,
    KeyCode::F19 => Key::F19,
    KeyCode::F20 => Key::F20,

    KeyCode::Insert => Key::Insert,
    KeyCode::Home => Key::Home,
    KeyCode::PageUp => Key::PageUp,
    KeyCode::Delete => Key::Delete,
    KeyCode::End => Key::End,
    KeyCode::PageDown => Key::PageDown,
    KeyCode::ArrowLeft => Key::ArrowLeft,
    KeyCode::ArrowRight => Key::ArrowRight,
    KeyCode::ArrowDown => Key::ArrowDown,
    KeyCode::ArrowUp => Key::ArrowUp,
    _ => Key::Unidentified(NativeKeyCode::MacOS(scancode)),
  }
}

fn code_to_location(code: KeyCode) -> KeyLocation {
  match code {
    KeyCode::SuperRight => KeyLocation::Right,
    KeyCode::SuperLeft => KeyLocation::Left,
    KeyCode::ShiftLeft => KeyLocation::Left,
    KeyCode::AltLeft => KeyLocation::Left,
    KeyCode::ControlLeft => KeyLocation::Left,
    KeyCode::ShiftRight => KeyLocation::Right,
    KeyCode::AltRight => KeyLocation::Right,
    KeyCode::ControlRight => KeyLocation::Right,

    KeyCode::NumLock => KeyLocation::Numpad,
    KeyCode::NumpadDecimal => KeyLocation::Numpad,
    KeyCode::NumpadMultiply => KeyLocation::Numpad,
    KeyCode::NumpadAdd => KeyLocation::Numpad,
    KeyCode::NumpadDivide => KeyLocation::Numpad,
    KeyCode::NumpadEnter => KeyLocation::Numpad,
    KeyCode::NumpadSubtract => KeyLocation::Numpad,
    KeyCode::NumpadEqual => KeyLocation::Numpad,
    KeyCode::Numpad0 => KeyLocation::Numpad,
    KeyCode::Numpad1 => KeyLocation::Numpad,
    KeyCode::Numpad2 => KeyLocation::Numpad,
    KeyCode::Numpad3 => KeyLocation::Numpad,
    KeyCode::Numpad4 => KeyLocation::Numpad,
    KeyCode::Numpad5 => KeyLocation::Numpad,
    KeyCode::Numpad6 => KeyLocation::Numpad,
    KeyCode::Numpad7 => KeyLocation::Numpad,
    KeyCode::Numpad8 => KeyLocation::Numpad,
    KeyCode::Numpad9 => KeyLocation::Numpad,

    _ => KeyLocation::Standard,
  }
}

// While F1-F20 have scancodes we can match on, we have to check against UTF-16
// constants for the rest.
// https://developer.apple.com/documentation/appkit/1535851-function-key_unicodes?preferredLanguage=occ
pub fn extra_function_key_to_code(scancode: u16, string: &str) -> KeyCode {
  if let Some(ch) = string.encode_utf16().next() {
    match ch {
      0xf718 => KeyCode::F21,
      0xf719 => KeyCode::F22,
      0xf71a => KeyCode::F23,
      0xf71b => KeyCode::F24,
      _ => KeyCode::Unidentified(NativeKeyCode::MacOS(scancode)),
    }
  } else {
    KeyCode::Unidentified(NativeKeyCode::MacOS(scancode))
  }
}

pub fn event_mods(event: &NSEvent) -> ModifiersState {
  let flags = unsafe { NSEvent::modifierFlags(event) };
  let mut m = ModifiersState::empty();
  m.set(
    ModifiersState::SHIFT,
    flags.contains(NSEventModifierFlags::Shift),
  );
  m.set(
    ModifiersState::CONTROL,
    flags.contains(NSEventModifierFlags::Control),
  );
  m.set(
    ModifiersState::ALT,
    flags.contains(NSEventModifierFlags::Option),
  );
  m.set(
    ModifiersState::SUPER,
    flags.contains(NSEventModifierFlags::Command),
  );
  m
}

pub fn get_scancode(event: &NSEvent) -> c_ushort {
  // In AppKit, `keyCode` refers to the position (scancode) of a key rather than its character,
  // and there is no easy way to navtively retrieve the layout-dependent character.
  // In tao, we use keycode to refer to the key's character, and so this function aligns
  // AppKit's terminology with ours.
  unsafe { msg_send![event, keyCode] }
}
