// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{
  char,
  os::raw::c_int,
  ptr,
  sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

use crate::event::{ModifiersState, ScanCode, VirtualKeyCode};

use windows::Win32::{
  Foundation::{HWND, LPARAM, WPARAM},
  UI::{
    Input::KeyboardAndMouse::*,
    TextServices::HKL,
    WindowsAndMessaging::{self as win32wm, *},
  },
};

fn key_pressed(vkey: c_int) -> bool {
  unsafe { (GetKeyState(vkey) & (1 << 15)) == (1 << 15) }
}

pub fn get_key_mods() -> ModifiersState {
  let filter_out_altgr = layout_uses_altgr() && key_pressed(VK_RMENU);

  let mut mods = ModifiersState::empty();
  mods.set(ModifiersState::SHIFT, key_pressed(VK_SHIFT));
  mods.set(
    ModifiersState::CTRL,
    key_pressed(VK_CONTROL) && !filter_out_altgr,
  );
  mods.set(
    ModifiersState::ALT,
    key_pressed(VK_MENU) && !filter_out_altgr,
  );
  mods.set(
    ModifiersState::LOGO,
    key_pressed(VK_LWIN) || key_pressed(VK_RWIN),
  );
  mods
}

bitflags! {
    #[derive(Default)]
    pub struct ModifiersStateSide: u32 {
        const LSHIFT = 0b010 << 0;
        const RSHIFT = 0b001 << 0;

        const LCTRL = 0b010 << 3;
        const RCTRL = 0b001 << 3;

        const LALT = 0b010 << 6;
        const RALT = 0b001 << 6;

        const LLOGO = 0b010 << 9;
        const RLOGO = 0b001 << 9;
    }
}

impl ModifiersStateSide {
  pub fn filter_out_altgr(&self) -> ModifiersStateSide {
    match layout_uses_altgr() && self.contains(Self::RALT) {
      false => *self,
      true => *self & !(Self::LCTRL | Self::RCTRL | Self::LALT | Self::RALT),
    }
  }
}

impl From<ModifiersStateSide> for ModifiersState {
  fn from(side: ModifiersStateSide) -> Self {
    let mut state = ModifiersState::default();
    state.set(
      Self::SHIFT,
      side.intersects(ModifiersStateSide::LSHIFT | ModifiersStateSide::RSHIFT),
    );
    state.set(
      Self::CTRL,
      side.intersects(ModifiersStateSide::LCTRL | ModifiersStateSide::RCTRL),
    );
    state.set(
      Self::ALT,
      side.intersects(ModifiersStateSide::LALT | ModifiersStateSide::RALT),
    );
    state.set(
      Self::LOGO,
      side.intersects(ModifiersStateSide::LLOGO | ModifiersStateSide::RLOGO),
    );
    state
  }
}

pub fn get_pressed_keys() -> impl Iterator<Item = c_int> {
  let mut keyboard_state = vec![0u8; 256];
  unsafe { GetKeyboardState(keyboard_state.as_mut_ptr()) };
  keyboard_state
    .into_iter()
    .enumerate()
    .filter(|(_, p)| (*p & (1 << 7)) != 0) // whether or not a key is pressed is communicated via the high-order bit
    .map(|(i, _)| i as c_int)
}

unsafe fn get_char(keyboard_state: &[u8; 256], v_key: u32, hkl: HKL) -> Option<char> {
  let mut unicode_bytes = [0u16; 5];
  let len = ToUnicodeEx(
    v_key,
    0,
    keyboard_state.as_ptr(),
    unicode_bytes.as_mut_ptr(),
    unicode_bytes.len() as _,
    0,
    hkl,
  );
  if len >= 1 {
    char::decode_utf16(unicode_bytes.iter().cloned())
      .next()
      .and_then(|c| c.ok())
  } else {
    None
  }
}

/// Figures out if the keyboard layout has an AltGr key instead of an Alt key.
///
/// Unfortunately, the Windows API doesn't give a way for us to conveniently figure that out. So,
/// we use a technique blatantly stolen from [the Firefox source code][source]: iterate over every
/// possible virtual key and compare the `char` output when AltGr is pressed vs when it isn't. If
/// pressing AltGr outputs characters that are different from the standard characters, the layout
/// uses AltGr. Otherwise, it doesn't.
///
/// [source]: https://github.com/mozilla/gecko-dev/blob/265e6721798a455604328ed5262f430cfcc37c2f/widget/windows/KeyboardLayout.cpp#L4356-L4416
fn layout_uses_altgr() -> bool {
  unsafe {
    static ACTIVE_LAYOUT: AtomicPtr<HKL__> = AtomicPtr::new(ptr::null_mut());
    static USES_ALTGR: AtomicBool = AtomicBool::new(false);

    let hkl = GetKeyboardLayout(0);
    let old_hkl = ACTIVE_LAYOUT.swap(hkl, Ordering::SeqCst);

    if hkl == old_hkl {
      return USES_ALTGR.load(Ordering::SeqCst);
    }

    let mut keyboard_state_altgr = [0u8; 256];
    // AltGr is an alias for Ctrl+Alt for... some reason. Whatever it is, those are the keypresses
    // we have to emulate to do an AltGr test.
    keyboard_state_altgr[VK_MENU as usize] = 0x80;
    keyboard_state_altgr[VK_CONTROL as usize] = 0x80;

    let keyboard_state_empty = [0u8; 256];

    for v_key in 0..=255 {
      let key_noaltgr = get_char(&keyboard_state_empty, v_key, hkl);
      let key_altgr = get_char(&keyboard_state_altgr, v_key, hkl);
      if let (Some(noaltgr), Some(altgr)) = (key_noaltgr, key_altgr) {
        if noaltgr != altgr {
          USES_ALTGR.store(true, Ordering::SeqCst);
          return true;
        }
      }
    }

    USES_ALTGR.store(false, Ordering::SeqCst);
    false
  }
}

pub fn vkey_to_tao_vkey(vkey: u32) -> Option<VirtualKeyCode> {
  // VK_* codes are documented here https://msdn.microsoft.com/en-us/library/windows/desktop/dd375731(v=vs.85).aspx
  match vkey {
    //win32wm::VK_LBUTTON => Some(VirtualKeyCode::Lbutton),
    //win32wm::VK_RBUTTON => Some(VirtualKeyCode::Rbutton),
    //win32wm::VK_CANCEL => Some(VirtualKeyCode::Cancel),
    //win32wm::VK_MBUTTON => Some(VirtualKeyCode::Mbutton),
    //win32wm::VK_XBUTTON1 => Some(VirtualKeyCode::Xbutton1),
    //win32wm::VK_XBUTTON2 => Some(VirtualKeyCode::Xbutton2),
    win32wm::VK_BACK => Some(VirtualKeyCode::Back),
    win32wm::VK_TAB => Some(VirtualKeyCode::Tab),
    //win32wm::VK_CLEAR => Some(VirtualKeyCode::Clear),
    win32wm::VK_RETURN => Some(VirtualKeyCode::Return),
    win32wm::VK_LSHIFT => Some(VirtualKeyCode::LShift),
    win32wm::VK_RSHIFT => Some(VirtualKeyCode::RShift),
    win32wm::VK_LCONTROL => Some(VirtualKeyCode::LControl),
    win32wm::VK_RCONTROL => Some(VirtualKeyCode::RControl),
    win32wm::VK_LMENU => Some(VirtualKeyCode::LAlt),
    win32wm::VK_RMENU => Some(VirtualKeyCode::RAlt),
    win32wm::VK_PAUSE => Some(VirtualKeyCode::Pause),
    win32wm::VK_CAPITAL => Some(VirtualKeyCode::Capital),
    win32wm::VK_KANA => Some(VirtualKeyCode::Kana),
    //win32wm::VK_HANGUEL => Some(VirtualKeyCode::Hanguel),
    //win32wm::VK_HANGUL => Some(VirtualKeyCode::Hangul),
    //win32wm::VK_JUNJA => Some(VirtualKeyCode::Junja),
    //win32wm::VK_FINAL => Some(VirtualKeyCode::Final),
    //win32wm::VK_HANJA => Some(VirtualKeyCode::Hanja),
    win32wm::VK_KANJI => Some(VirtualKeyCode::Kanji),
    win32wm::VK_ESCAPE => Some(VirtualKeyCode::Escape),
    win32wm::VK_CONVERT => Some(VirtualKeyCode::Convert),
    win32wm::VK_NONCONVERT => Some(VirtualKeyCode::NoConvert),
    //win32wm::VK_ACCEPT => Some(VirtualKeyCode::Accept),
    //win32wm::VK_MODECHANGE => Some(VirtualKeyCode::Modechange),
    win32wm::VK_SPACE => Some(VirtualKeyCode::Space),
    win32wm::VK_PRIOR => Some(VirtualKeyCode::PageUp),
    win32wm::VK_NEXT => Some(VirtualKeyCode::PageDown),
    win32wm::VK_END => Some(VirtualKeyCode::End),
    win32wm::VK_HOME => Some(VirtualKeyCode::Home),
    win32wm::VK_LEFT => Some(VirtualKeyCode::Left),
    win32wm::VK_UP => Some(VirtualKeyCode::Up),
    win32wm::VK_RIGHT => Some(VirtualKeyCode::Right),
    win32wm::VK_DOWN => Some(VirtualKeyCode::Down),
    //win32wm::VK_SELECT => Some(VirtualKeyCode::Select),
    //win32wm::VK_PRINT => Some(VirtualKeyCode::Print),
    //win32wm::VK_EXECUTE => Some(VirtualKeyCode::Execute),
    win32wm::VK_SNAPSHOT => Some(VirtualKeyCode::Snapshot),
    win32wm::VK_INSERT => Some(VirtualKeyCode::Insert),
    win32wm::VK_DELETE => Some(VirtualKeyCode::Delete),
    //win32wm::VK_HELP => Some(VirtualKeyCode::Help),
    0x30 => Some(VirtualKeyCode::Key0),
    0x31 => Some(VirtualKeyCode::Key1),
    0x32 => Some(VirtualKeyCode::Key2),
    0x33 => Some(VirtualKeyCode::Key3),
    0x34 => Some(VirtualKeyCode::Key4),
    0x35 => Some(VirtualKeyCode::Key5),
    0x36 => Some(VirtualKeyCode::Key6),
    0x37 => Some(VirtualKeyCode::Key7),
    0x38 => Some(VirtualKeyCode::Key8),
    0x39 => Some(VirtualKeyCode::Key9),
    0x41 => Some(VirtualKeyCode::A),
    0x42 => Some(VirtualKeyCode::B),
    0x43 => Some(VirtualKeyCode::C),
    0x44 => Some(VirtualKeyCode::D),
    0x45 => Some(VirtualKeyCode::E),
    0x46 => Some(VirtualKeyCode::F),
    0x47 => Some(VirtualKeyCode::G),
    0x48 => Some(VirtualKeyCode::H),
    0x49 => Some(VirtualKeyCode::I),
    0x4A => Some(VirtualKeyCode::J),
    0x4B => Some(VirtualKeyCode::K),
    0x4C => Some(VirtualKeyCode::L),
    0x4D => Some(VirtualKeyCode::M),
    0x4E => Some(VirtualKeyCode::N),
    0x4F => Some(VirtualKeyCode::O),
    0x50 => Some(VirtualKeyCode::P),
    0x51 => Some(VirtualKeyCode::Q),
    0x52 => Some(VirtualKeyCode::R),
    0x53 => Some(VirtualKeyCode::S),
    0x54 => Some(VirtualKeyCode::T),
    0x55 => Some(VirtualKeyCode::U),
    0x56 => Some(VirtualKeyCode::V),
    0x57 => Some(VirtualKeyCode::W),
    0x58 => Some(VirtualKeyCode::X),
    0x59 => Some(VirtualKeyCode::Y),
    0x5A => Some(VirtualKeyCode::Z),
    win32wm::VK_LWIN => Some(VirtualKeyCode::LWin),
    win32wm::VK_RWIN => Some(VirtualKeyCode::RWin),
    win32wm::VK_APPS => Some(VirtualKeyCode::Apps),
    win32wm::VK_SLEEP => Some(VirtualKeyCode::Sleep),
    win32wm::VK_NUMPAD0 => Some(VirtualKeyCode::Numpad0),
    win32wm::VK_NUMPAD1 => Some(VirtualKeyCode::Numpad1),
    win32wm::VK_NUMPAD2 => Some(VirtualKeyCode::Numpad2),
    win32wm::VK_NUMPAD3 => Some(VirtualKeyCode::Numpad3),
    win32wm::VK_NUMPAD4 => Some(VirtualKeyCode::Numpad4),
    win32wm::VK_NUMPAD5 => Some(VirtualKeyCode::Numpad5),
    win32wm::VK_NUMPAD6 => Some(VirtualKeyCode::Numpad6),
    win32wm::VK_NUMPAD7 => Some(VirtualKeyCode::Numpad7),
    win32wm::VK_NUMPAD8 => Some(VirtualKeyCode::Numpad8),
    win32wm::VK_NUMPAD9 => Some(VirtualKeyCode::Numpad9),
    win32wm::VK_MULTIPLY => Some(VirtualKeyCode::NumpadMultiply),
    win32wm::VK_ADD => Some(VirtualKeyCode::NumpadAdd),
    //win32wm::VK_SEPARATOR => Some(VirtualKeyCode::Separator),
    win32wm::VK_SUBTRACT => Some(VirtualKeyCode::NumpadSubtract),
    win32wm::VK_DECIMAL => Some(VirtualKeyCode::NumpadDecimal),
    win32wm::VK_DIVIDE => Some(VirtualKeyCode::NumpadDivide),
    win32wm::VK_F1 => Some(VirtualKeyCode::F1),
    win32wm::VK_F2 => Some(VirtualKeyCode::F2),
    win32wm::VK_F3 => Some(VirtualKeyCode::F3),
    win32wm::VK_F4 => Some(VirtualKeyCode::F4),
    win32wm::VK_F5 => Some(VirtualKeyCode::F5),
    win32wm::VK_F6 => Some(VirtualKeyCode::F6),
    win32wm::VK_F7 => Some(VirtualKeyCode::F7),
    win32wm::VK_F8 => Some(VirtualKeyCode::F8),
    win32wm::VK_F9 => Some(VirtualKeyCode::F9),
    win32wm::VK_F10 => Some(VirtualKeyCode::F10),
    win32wm::VK_F11 => Some(VirtualKeyCode::F11),
    win32wm::VK_F12 => Some(VirtualKeyCode::F12),
    win32wm::VK_F13 => Some(VirtualKeyCode::F13),
    win32wm::VK_F14 => Some(VirtualKeyCode::F14),
    win32wm::VK_F15 => Some(VirtualKeyCode::F15),
    win32wm::VK_F16 => Some(VirtualKeyCode::F16),
    win32wm::VK_F17 => Some(VirtualKeyCode::F17),
    win32wm::VK_F18 => Some(VirtualKeyCode::F18),
    win32wm::VK_F19 => Some(VirtualKeyCode::F19),
    win32wm::VK_F20 => Some(VirtualKeyCode::F20),
    win32wm::VK_F21 => Some(VirtualKeyCode::F21),
    win32wm::VK_F22 => Some(VirtualKeyCode::F22),
    win32wm::VK_F23 => Some(VirtualKeyCode::F23),
    win32wm::VK_F24 => Some(VirtualKeyCode::F24),
    win32wm::VK_NUMLOCK => Some(VirtualKeyCode::Numlock),
    win32wm::VK_SCROLL => Some(VirtualKeyCode::Scroll),
    win32wm::VK_BROWSER_BACK => Some(VirtualKeyCode::NavigateBackward),
    win32wm::VK_BROWSER_FORWARD => Some(VirtualKeyCode::NavigateForward),
    win32wm::VK_BROWSER_REFRESH => Some(VirtualKeyCode::WebRefresh),
    win32wm::VK_BROWSER_STOP => Some(VirtualKeyCode::WebStop),
    win32wm::VK_BROWSER_SEARCH => Some(VirtualKeyCode::WebSearch),
    win32wm::VK_BROWSER_FAVORITES => Some(VirtualKeyCode::WebFavorites),
    win32wm::VK_BROWSER_HOME => Some(VirtualKeyCode::WebHome),
    win32wm::VK_VOLUME_MUTE => Some(VirtualKeyCode::Mute),
    win32wm::VK_VOLUME_DOWN => Some(VirtualKeyCode::VolumeDown),
    win32wm::VK_VOLUME_UP => Some(VirtualKeyCode::VolumeUp),
    win32wm::VK_MEDIA_NEXT_TRACK => Some(VirtualKeyCode::NextTrack),
    win32wm::VK_MEDIA_PREV_TRACK => Some(VirtualKeyCode::PrevTrack),
    win32wm::VK_MEDIA_STOP => Some(VirtualKeyCode::MediaStop),
    win32wm::VK_MEDIA_PLAY_PAUSE => Some(VirtualKeyCode::PlayPause),
    win32wm::VK_LAUNCH_MAIL => Some(VirtualKeyCode::Mail),
    win32wm::VK_LAUNCH_MEDIA_SELECT => Some(VirtualKeyCode::MediaSelect),
    /*win32wm::VK_LAUNCH_APP1 => Some(VirtualKeyCode::Launch_app1),
    win32wm::VK_LAUNCH_APP2 => Some(VirtualKeyCode::Launch_app2),*/
    win32wm::VK_OEM_PLUS => Some(VirtualKeyCode::Equals),
    win32wm::VK_OEM_COMMA => Some(VirtualKeyCode::Comma),
    win32wm::VK_OEM_MINUS => Some(VirtualKeyCode::Minus),
    win32wm::VK_OEM_PERIOD => Some(VirtualKeyCode::Period),
    win32wm::VK_OEM_1 => map_text_keys(vkey),
    win32wm::VK_OEM_2 => map_text_keys(vkey),
    win32wm::VK_OEM_3 => map_text_keys(vkey),
    win32wm::VK_OEM_4 => map_text_keys(vkey),
    win32wm::VK_OEM_5 => map_text_keys(vkey),
    win32wm::VK_OEM_6 => map_text_keys(vkey),
    win32wm::VK_OEM_7 => map_text_keys(vkey),
    /* win32wm::VK_OEM_8 => Some(VirtualKeyCode::Oem_8), */
    win32wm::VK_OEM_102 => Some(VirtualKeyCode::OEM102),
    /*win32wm::VK_PROCESSKEY => Some(VirtualKeyCode::Processkey),
    win32wm::VK_PACKET => Some(VirtualKeyCode::Packet),
    win32wm::VK_ATTN => Some(VirtualKeyCode::Attn),
    win32wm::VK_CRSEL => Some(VirtualKeyCode::Crsel),
    win32wm::VK_EXSEL => Some(VirtualKeyCode::Exsel),
    win32wm::VK_EREOF => Some(VirtualKeyCode::Ereof),
    win32wm::VK_PLAY => Some(VirtualKeyCode::Play),
    win32wm::VK_ZOOM => Some(VirtualKeyCode::Zoom),
    win32wm::VK_NONAME => Some(VirtualKeyCode::Noname),
    win32wm::VK_PA1 => Some(VirtualKeyCode::Pa1),
    win32wm::VK_OEM_CLEAR => Some(VirtualKeyCode::Oem_clear),*/
    _ => None,
  }
}

pub fn handle_extended_keys(
  vkey: u32,
  mut scancode: UINT,
  extended: bool,
) -> Option<(c_int, UINT)> {
  // Welcome to hell https://blog.molecular-matters.com/2011/09/05/properly-handling-keyboard-input/
  scancode = if extended { 0xE000 } else { 0x0000 } | scancode;
  let vkey = match vkey {
    win32wm::VK_SHIFT => unsafe { MapVirtualKeyA(scancode, MAPVK_VSC_TO_VK_EX) as _ },
    win32wm::VK_CONTROL => {
      if extended {
        VK_RCONTROL
      } else {
        VK_LCONTROL
      }
    }
    win32wm::VK_MENU => {
      if extended {
        VK_RMENU
      } else {
        VK_LMENU
      }
    }
    _ => {
      match scancode {
        // When VK_PAUSE is pressed it emits a LeftControl + NumLock scancode event sequence, but reports VK_PAUSE
        // as the virtual key on both events, or VK_PAUSE on the first event or 0xFF when using raw input.
        // Don't emit anything for the LeftControl event in the pair...
        0xE01D if vkey == VK_PAUSE => return None,
        // ...and emit the Pause event for the second event in the pair.
        0x45 if vkey == VK_PAUSE || vkey == 0xFF as _ => {
          scancode = 0xE059;
          VK_PAUSE
        }
        // VK_PAUSE has an incorrect vkey value when used with modifiers. VK_PAUSE also reports a different
        // scancode when used with modifiers than when used without
        0xE046 => {
          scancode = 0xE059;
          VK_PAUSE
        }
        // VK_SCROLL has an incorrect vkey value when used with modifiers.
        0x46 => VK_SCROLL,
        _ => vkey,
      }
    }
  };
  Some((vkey, scancode))
}

pub fn process_key_params(
  wparam: WPARAM,
  lparam: LPARAM,
) -> Option<(ScanCode, Option<VirtualKeyCode>)> {
  let scancode = ((lparam >> 16) & 0xff) as UINT;
  let extended = (lparam & 0x01000000) != 0;
  handle_extended_keys(wparam as _, scancode, extended)
    .map(|(vkey, scancode)| (scancode, vkey_to_tao_vkey(vkey)))
}

// This is needed as windows doesn't properly distinguish
// some virtual key codes for different keyboard layouts
fn map_text_keys(win_virtual_key: i32) -> Option<VirtualKeyCode> {
  let char_key = unsafe { MapVirtualKeyA(win_virtual_key as u32, MAPVK_VK_TO_CHAR) } & 0x7FFF;
  match char::from_u32(char_key) {
    Some(';') => Some(VirtualKeyCode::Semicolon),
    Some('/') => Some(VirtualKeyCode::Slash),
    Some('`') => Some(VirtualKeyCode::Grave),
    Some('[') => Some(VirtualKeyCode::LBracket),
    Some(']') => Some(VirtualKeyCode::RBracket),
    Some('\'') => Some(VirtualKeyCode::Apostrophe),
    Some('\\') => Some(VirtualKeyCode::Backslash),
    _ => None,
  }
}
