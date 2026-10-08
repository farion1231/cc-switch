use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::{
  collections::{hash_map::Entry, HashMap, HashSet},
  ffi::OsString,
  os::windows::ffi::OsStringExt,
};

use windows::Win32::{
  System::SystemServices::{LANG_JAPANESE, LANG_KOREAN},
  UI::Input::KeyboardAndMouse::{self as win32km, *},
};

use super::keyboard::ExScancode;
use crate::{
  keyboard::{Key, KeyCode, ModifiersState, NativeKeyCode},
  platform_impl::platform::util,
};

pub(crate) static LAYOUT_CACHE: Lazy<Mutex<LayoutCache>> =
  Lazy::new(|| Mutex::new(LayoutCache::default()));

fn key_pressed(vkey: VIRTUAL_KEY) -> bool {
  unsafe { (GetKeyState(u32::from(vkey.0) as i32) & (1 << 15)) == (1 << 15) }
}

const NUMPAD_VKEYS: [VIRTUAL_KEY; 16] = [
  VK_NUMPAD0,
  VK_NUMPAD1,
  VK_NUMPAD2,
  VK_NUMPAD3,
  VK_NUMPAD4,
  VK_NUMPAD5,
  VK_NUMPAD6,
  VK_NUMPAD7,
  VK_NUMPAD8,
  VK_NUMPAD9,
  VK_MULTIPLY,
  VK_ADD,
  VK_SEPARATOR,
  VK_SUBTRACT,
  VK_DECIMAL,
  VK_DIVIDE,
];

static NUMPAD_KEYCODES: Lazy<HashSet<KeyCode>> = Lazy::new(|| {
  let mut keycodes = HashSet::new();
  keycodes.insert(KeyCode::Numpad0);
  keycodes.insert(KeyCode::Numpad1);
  keycodes.insert(KeyCode::Numpad2);
  keycodes.insert(KeyCode::Numpad3);
  keycodes.insert(KeyCode::Numpad4);
  keycodes.insert(KeyCode::Numpad5);
  keycodes.insert(KeyCode::Numpad6);
  keycodes.insert(KeyCode::Numpad7);
  keycodes.insert(KeyCode::Numpad8);
  keycodes.insert(KeyCode::Numpad9);
  keycodes.insert(KeyCode::NumpadMultiply);
  keycodes.insert(KeyCode::NumpadAdd);
  keycodes.insert(KeyCode::NumpadComma);
  keycodes.insert(KeyCode::NumpadSubtract);
  keycodes.insert(KeyCode::NumpadDecimal);
  keycodes.insert(KeyCode::NumpadDivide);
  keycodes
});

bitflags! {
    #[derive(Clone, Copy, Eq, PartialEq, Hash)]
    pub struct WindowsModifiers : u8 {
        const SHIFT = 1 << 0;
        const CONTROL = 1 << 1;
        const ALT = 1 << 2;
        const CAPS_LOCK = 1 << 3;
        const FLAGS_END = 1 << 4;
    }
}

impl WindowsModifiers {
  pub fn active_modifiers(key_state: &[u8; 256]) -> WindowsModifiers {
    let shift = key_state[usize::from(VK_SHIFT.0)] & 0x80 != 0;
    let lshift = key_state[usize::from(VK_LSHIFT.0)] & 0x80 != 0;
    let rshift = key_state[usize::from(VK_RSHIFT.0)] & 0x80 != 0;

    let control = key_state[usize::from(VK_CONTROL.0)] & 0x80 != 0;
    let lcontrol = key_state[usize::from(VK_LCONTROL.0)] & 0x80 != 0;
    let rcontrol = key_state[usize::from(VK_RCONTROL.0)] & 0x80 != 0;

    let alt = key_state[usize::from(VK_MENU.0)] & 0x80 != 0;
    let lalt = key_state[usize::from(VK_LMENU.0)] & 0x80 != 0;
    let ralt = key_state[usize::from(VK_RMENU.0)] & 0x80 != 0;

    let caps = key_state[usize::from(VK_CAPITAL.0)] & 0x01 != 0;

    let mut result = WindowsModifiers::empty();
    if shift || lshift || rshift {
      result.insert(WindowsModifiers::SHIFT);
    }
    if control || lcontrol || rcontrol {
      result.insert(WindowsModifiers::CONTROL);
    }
    if alt || lalt || ralt {
      result.insert(WindowsModifiers::ALT);
    }
    if caps {
      result.insert(WindowsModifiers::CAPS_LOCK);
    }

    result
  }

  pub fn apply_to_kbd_state(self, key_state: &mut [u8; 256]) {
    if self.intersects(Self::SHIFT) {
      key_state[usize::from(VK_SHIFT.0)] |= 0x80;
    } else {
      key_state[usize::from(VK_SHIFT.0)] &= !0x80;
      key_state[usize::from(VK_LSHIFT.0)] &= !0x80;
      key_state[usize::from(VK_RSHIFT.0)] &= !0x80;
    }
    if self.intersects(Self::CONTROL) {
      key_state[usize::from(VK_CONTROL.0)] |= 0x80;
    } else {
      key_state[usize::from(VK_CONTROL.0)] &= !0x80;
      key_state[usize::from(VK_LCONTROL.0)] &= !0x80;
      key_state[usize::from(VK_RCONTROL.0)] &= !0x80;
    }
    if self.intersects(Self::ALT) {
      key_state[usize::from(VK_MENU.0)] |= 0x80;
    } else {
      key_state[usize::from(VK_MENU.0)] &= !0x80;
      key_state[usize::from(VK_LMENU.0)] &= !0x80;
      key_state[usize::from(VK_RMENU.0)] &= !0x80;
    }
    if self.intersects(Self::CAPS_LOCK) {
      key_state[usize::from(VK_CAPITAL.0)] |= 0x01;
    } else {
      key_state[usize::from(VK_CAPITAL.0)] &= !0x01;
    }
  }

  /// Removes the control modifier if the alt modifier is not present.
  /// This is useful because on Windows: (Control + Alt) == AltGr
  /// but we don't want to interfere with the AltGr state.
  pub fn remove_only_ctrl(mut self) -> WindowsModifiers {
    if !self.contains(WindowsModifiers::ALT) {
      self.remove(WindowsModifiers::CONTROL);
    }
    self
  }
}

pub(crate) struct Layout {
  pub hkl: isize,

  /// Maps numpad keys from Windows virtual key to a `Key`.
  ///
  /// This is useful because some numpad keys generate different charcaters based on the locale.
  /// For example `VK_DECIMAL` is sometimes "." and sometimes ",". Note: numpad-specific virtual
  /// keys are only produced by Windows when the NumLock is active.
  ///
  /// Making this field separate from the `keys` field saves having to add NumLock as a modifier
  /// to `WindowsModifiers`, which would double the number of items in keys.
  pub numlock_on_keys: HashMap<u16, Key<'static>>,
  /// Like `numlock_on_keys` but this will map to the key that would be produced if numlock was
  /// off. The keys of this map are identical to the keys of `numlock_on_keys`.
  pub numlock_off_keys: HashMap<u16, Key<'static>>,

  /// Maps a modifier state to group of key strings
  /// We're not using `ModifiersState` here because that object cannot express caps lock,
  /// but we need to handle caps lock too.
  ///
  /// This map shouldn't need to exist.
  /// However currently this seems to be the only good way
  /// of getting the label for the pressed key. Note that calling `ToUnicode`
  /// just when the key is pressed/released would be enough if `ToUnicode` wouldn't
  /// change the keyboard state (it clears the dead key). There is a flag to prevent
  /// changing the state, but that flag requires Windows 10, version 1607 or newer)
  pub keys: HashMap<WindowsModifiers, HashMap<KeyCode, Key<'static>>>,
  pub has_alt_graph: bool,
}

impl Layout {
  pub fn get_key(
    &self,
    mods: WindowsModifiers,
    num_lock_on: bool,
    vkey: VIRTUAL_KEY,
    scancode: ExScancode,
    keycode: KeyCode,
  ) -> Key<'static> {
    let native_code = NativeKeyCode::Windows(scancode);

    let unknown_alt = vkey == VK_MENU;
    if !unknown_alt {
      // Here we try using the virtual key directly but if the virtual key doesn't distinguish
      // between left and right alt, we can't report AltGr. Therefore, we only do this if the
      // key is not the "unknown alt" key.
      //
      // The reason for using the virtual key directly is that `MapVirtualKeyExW` (used when
      // building the keys map) sometimes maps virtual keys to odd scancodes that don't match
      // the scancode coming from the KEYDOWN message for the same key. For example: `VK_LEFT`
      // is mapped to `0x004B`, but the scancode for the left arrow is `0xE04B`.
      let key_from_vkey =
        vkey_to_non_char_key(vkey, native_code, HKL(self.hkl as _), self.has_alt_graph);

      if !matches!(key_from_vkey, Key::Unidentified(_)) {
        return key_from_vkey;
      }
    }
    if num_lock_on {
      if let Some(key) = self.numlock_on_keys.get(&vkey.0) {
        return key.clone();
      }
    } else if let Some(key) = self.numlock_off_keys.get(&vkey.0) {
      return key.clone();
    }

    if let Some(keys) = self.keys.get(&mods) {
      if let Some(key) = keys.get(&keycode) {
        return key.clone();
      }
    }
    Key::Unidentified(native_code)
  }
}

#[derive(Default)]
pub(crate) struct LayoutCache {
  /// Maps locale identifiers (HKL) to layouts
  pub layouts: HashMap<isize, Layout>,
  pub strings: HashSet<&'static str>,
}

impl LayoutCache {
  /// Checks whether the current layout is already known and
  /// prepares the layout if it isn't known.
  /// The current layout is then returned.
  pub fn get_current_layout<'a>(&'a mut self) -> (HKL, &'a Layout) {
    let locale_id = unsafe { GetKeyboardLayout(0) };
    match self.layouts.entry(locale_id.0 as _) {
      Entry::Occupied(entry) => (locale_id, entry.into_mut()),
      Entry::Vacant(entry) => {
        let layout = Self::prepare_layout(&mut self.strings, locale_id);
        (locale_id, entry.insert(layout))
      }
    }
  }

  pub fn get_agnostic_mods(&mut self) -> ModifiersState {
    let (_, layout) = self.get_current_layout();
    let filter_out_altgr = layout.has_alt_graph && key_pressed(VK_RMENU);
    let mut mods = ModifiersState::empty();
    mods.set(ModifiersState::SHIFT, key_pressed(VK_SHIFT));
    mods.set(
      ModifiersState::CONTROL,
      key_pressed(VK_CONTROL) && !filter_out_altgr,
    );
    mods.set(
      ModifiersState::ALT,
      key_pressed(VK_MENU) && !filter_out_altgr,
    );
    mods.set(
      ModifiersState::SUPER,
      key_pressed(VK_LWIN) || key_pressed(VK_RWIN),
    );
    mods
  }

  fn prepare_layout(strings: &mut HashSet<&'static str>, locale_id: HKL) -> Layout {
    let mut layout = Layout {
      hkl: locale_id.0 as _,
      numlock_on_keys: Default::default(),
      numlock_off_keys: Default::default(),
      keys: Default::default(),
      has_alt_graph: false,
    };

    // We initialize the keyboard state with all zeros to
    // simulate a scenario when no modifier is active.
    let mut key_state = [0u8; 256];

    // `MapVirtualKeyExW` maps (non-numpad-specific) virtual keys to scancodes as if numlock
    // was off. We rely on this behavior to find all virtual keys which are not numpad-specific
    // but map to the numpad.
    //
    // src_vkey: VK  ==>  scancode: u16 (on the numpad)
    //
    // Then we convert the source virtual key into a `Key` and the scancode into a virtual key
    // to get the reverse mapping.
    //
    // src_vkey: VK  ==>  scancode: u16 (on the numpad)
    //    ||                    ||
    //    \/                    \/
    // map_value: Key  <-  map_vkey: VK
    layout.numlock_off_keys.reserve(NUMPAD_KEYCODES.len());
    for vk in 0_u16..256 {
      let scancode =
        unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, Some(locale_id as HKL)) };
      if scancode == 0 {
        continue;
      }
      let vk = VIRTUAL_KEY(vk);
      let keycode = KeyCode::from_scancode(scancode);
      if !is_numpad_specific(vk) && NUMPAD_KEYCODES.contains(&keycode) {
        let native_code = NativeKeyCode::Windows(scancode as u16);
        let map_vkey = keycode_to_vkey(keycode, locale_id);
        if map_vkey == Default::default() {
          continue;
        }
        let map_value = vkey_to_non_char_key(vk, native_code, locale_id, false);
        if matches!(map_value, Key::Unidentified(_)) {
          continue;
        }
        layout.numlock_off_keys.insert(map_vkey.0, map_value);
      }
    }

    layout.numlock_on_keys.reserve(NUMPAD_VKEYS.len());
    for vk in NUMPAD_VKEYS.iter() {
      let scancode =
        unsafe { MapVirtualKeyExW(u32::from(vk.0), MAPVK_VK_TO_VSC_EX, Some(locale_id as HKL)) };
      let unicode = Self::to_unicode_string(&key_state, *vk, scancode, locale_id);
      if let ToUnicodeResult::Str(s) = unicode {
        let static_str = get_or_insert_str(strings, s);
        layout
          .numlock_on_keys
          .insert(vk.0, Key::Character(static_str));
      }
    }

    // Iterate through every combination of modifiers
    let mods_end = WindowsModifiers::FLAGS_END.bits();
    for mod_state in 0..mods_end {
      let mut keys_for_this_mod = HashMap::with_capacity(256);

      let mod_state = WindowsModifiers::from_bits_truncate(mod_state);
      mod_state.apply_to_kbd_state(&mut key_state);

      // Virtual key values are in the domain [0, 255].
      // This is reinforced by the fact that the keyboard state array has 256
      // elements. This array is allowed to be indexed by virtual key values
      // giving the key state for the virtual key used for indexing.
      for vk in 0_u16..256 {
        let scancode =
          unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, Some(locale_id)) };
        if scancode == 0 {
          continue;
        }
        let vk = VIRTUAL_KEY(vk);
        let native_code = NativeKeyCode::Windows(scancode as ExScancode);
        let key_code = KeyCode::from_scancode(scancode);
        // Let's try to get the key from just the scancode and vk
        // We don't necessarily know yet if AltGraph is present on this layout so we'll
        // assume it isn't. Then we'll do a second pass where we set the "AltRight" keys to
        // "AltGr" in case we find out that there's an AltGraph.
        let preliminary_key = vkey_to_non_char_key(vk, native_code, locale_id, false);
        match preliminary_key {
          Key::Unidentified(_) => (),
          _ => {
            keys_for_this_mod.insert(key_code, preliminary_key);
            continue;
          }
        }

        let unicode = Self::to_unicode_string(&key_state, vk, scancode, locale_id);
        let key = match unicode {
          ToUnicodeResult::Str(str) => {
            let static_str = get_or_insert_str(strings, str);
            Key::Character(static_str)
          }
          ToUnicodeResult::Dead(dead_char) => {
            //#[cfg(debug_assertions)] println!("{:?} - {:?} produced dead {:?}", key_code, mod_state, dead_char);
            Key::Dead(dead_char)
          }
          ToUnicodeResult::None => {
            let has_alt = mod_state.contains(WindowsModifiers::ALT);
            let has_ctrl = mod_state.contains(WindowsModifiers::CONTROL);
            // HACK: `ToUnicodeEx` seems to fail getting the string for the numpad
            // divide key, so we handle that explicitly here
            if !has_alt && !has_ctrl && key_code == KeyCode::NumpadDivide {
              Key::Character("/")
            } else {
              // Just use the unidentified key, we got earlier
              preliminary_key
            }
          }
        };

        // Check for alt graph.
        // The logic is that if a key pressed with no modifier produces
        // a different `Character` from when it's pressed with CTRL+ALT then the layout
        // has AltGr.
        let ctrl_alt: WindowsModifiers = WindowsModifiers::CONTROL | WindowsModifiers::ALT;
        let is_in_ctrl_alt = mod_state == ctrl_alt;
        if !layout.has_alt_graph && is_in_ctrl_alt {
          // Unwrapping here because if we are in the ctrl+alt modifier state
          // then the alt modifier state must have come before.
          let simple_keys = layout.keys.get(&WindowsModifiers::empty()).unwrap();
          if let Some(Key::Character(key_no_altgr)) = simple_keys.get(&key_code) {
            if let Key::Character(key) = key.clone() {
              layout.has_alt_graph = key != *key_no_altgr;
            }
          }
        }

        keys_for_this_mod.insert(key_code, key);
      }
      layout.keys.insert(mod_state, keys_for_this_mod);
    }

    // Second pass: replace right alt keys with AltGr if the layout has alt graph
    if layout.has_alt_graph {
      for mod_state in 0..mods_end {
        let mod_state = WindowsModifiers::from_bits_truncate(mod_state);
        if let Some(keys) = layout.keys.get_mut(&mod_state) {
          if let Some(key) = keys.get_mut(&KeyCode::AltRight) {
            *key = Key::AltGraph;
          }
        }
      }
    }

    layout
  }

  fn to_unicode_string(
    key_state: &[u8; 256],
    vkey: VIRTUAL_KEY,
    scancode: u32,
    locale_id: HKL,
  ) -> ToUnicodeResult {
    unsafe {
      let mut label_wide = [0u16; 8];
      let mut wide_len = ToUnicodeEx(
        u32::from(vkey.0),
        scancode,
        key_state,
        &mut label_wide,
        0,
        Some(locale_id),
      );
      if wide_len < 0 {
        // If it's dead, we run `ToUnicode` again to consume the dead-key
        wide_len = ToUnicodeEx(
          u32::from(vkey.0),
          scancode,
          key_state,
          &mut label_wide,
          0,
          Some(locale_id),
        );
        if wide_len > 0 {
          let os_string = OsString::from_wide(&label_wide[0..wide_len as usize]);
          if let Ok(label_str) = os_string.into_string() {
            if let Some(ch) = label_str.chars().next() {
              return ToUnicodeResult::Dead(Some(ch));
            }
          }
        }
        return ToUnicodeResult::Dead(None);
      }
      if wide_len > 0 {
        let os_string = OsString::from_wide(&label_wide[0..wide_len as usize]);
        if let Ok(label_str) = os_string.into_string() {
          return ToUnicodeResult::Str(label_str);
        }
      }
    }
    ToUnicodeResult::None
  }
}

pub fn get_or_insert_str<T>(strings: &mut HashSet<&'static str>, string: T) -> &'static str
where
  T: AsRef<str>,
  String: From<T>,
{
  {
    let str_ref = string.as_ref();
    if let Some(&existing) = strings.get(str_ref) {
      return existing;
    }
  }
  let leaked = Box::leak(Box::from(String::from(string)));
  strings.insert(leaked);
  leaked
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ToUnicodeResult {
  Str(String),
  Dead(Option<char>),
  None,
}

fn is_numpad_specific(vk: VIRTUAL_KEY) -> bool {
  matches!(
    vk,
    win32km::VK_NUMPAD0
      | win32km::VK_NUMPAD1
      | win32km::VK_NUMPAD2
      | win32km::VK_NUMPAD3
      | win32km::VK_NUMPAD4
      | win32km::VK_NUMPAD5
      | win32km::VK_NUMPAD6
      | win32km::VK_NUMPAD7
      | win32km::VK_NUMPAD8
      | win32km::VK_NUMPAD9
      | win32km::VK_ADD
      | win32km::VK_SUBTRACT
      | win32km::VK_DIVIDE
      | win32km::VK_DECIMAL
      | win32km::VK_SEPARATOR
  )
}

fn keycode_to_vkey(keycode: KeyCode, hkl: HKL) -> VIRTUAL_KEY {
  let primary_lang_id = util::PRIMARYLANGID(hkl);
  let is_korean = primary_lang_id == LANG_KOREAN;
  let is_japanese = primary_lang_id == LANG_JAPANESE;

  match keycode {
    KeyCode::Backquote => VIRTUAL_KEY::default(),
    KeyCode::Backslash => VIRTUAL_KEY::default(),
    KeyCode::BracketLeft => VIRTUAL_KEY::default(),
    KeyCode::BracketRight => VIRTUAL_KEY::default(),
    KeyCode::Comma => VIRTUAL_KEY::default(),
    KeyCode::Digit0 => VIRTUAL_KEY::default(),
    KeyCode::Digit1 => VIRTUAL_KEY::default(),
    KeyCode::Digit2 => VIRTUAL_KEY::default(),
    KeyCode::Digit3 => VIRTUAL_KEY::default(),
    KeyCode::Digit4 => VIRTUAL_KEY::default(),
    KeyCode::Digit5 => VIRTUAL_KEY::default(),
    KeyCode::Digit6 => VIRTUAL_KEY::default(),
    KeyCode::Digit7 => VIRTUAL_KEY::default(),
    KeyCode::Digit8 => VIRTUAL_KEY::default(),
    KeyCode::Digit9 => VIRTUAL_KEY::default(),
    KeyCode::Equal => VIRTUAL_KEY::default(),
    KeyCode::IntlBackslash => VIRTUAL_KEY::default(),
    KeyCode::IntlRo => VIRTUAL_KEY::default(),
    KeyCode::IntlYen => VIRTUAL_KEY::default(),
    KeyCode::KeyA => VIRTUAL_KEY::default(),
    KeyCode::KeyB => VIRTUAL_KEY::default(),
    KeyCode::KeyC => VIRTUAL_KEY::default(),
    KeyCode::KeyD => VIRTUAL_KEY::default(),
    KeyCode::KeyE => VIRTUAL_KEY::default(),
    KeyCode::KeyF => VIRTUAL_KEY::default(),
    KeyCode::KeyG => VIRTUAL_KEY::default(),
    KeyCode::KeyH => VIRTUAL_KEY::default(),
    KeyCode::KeyI => VIRTUAL_KEY::default(),
    KeyCode::KeyJ => VIRTUAL_KEY::default(),
    KeyCode::KeyK => VIRTUAL_KEY::default(),
    KeyCode::KeyL => VIRTUAL_KEY::default(),
    KeyCode::KeyM => VIRTUAL_KEY::default(),
    KeyCode::KeyN => VIRTUAL_KEY::default(),
    KeyCode::KeyO => VIRTUAL_KEY::default(),
    KeyCode::KeyP => VIRTUAL_KEY::default(),
    KeyCode::KeyQ => VIRTUAL_KEY::default(),
    KeyCode::KeyR => VIRTUAL_KEY::default(),
    KeyCode::KeyS => VIRTUAL_KEY::default(),
    KeyCode::KeyT => VIRTUAL_KEY::default(),
    KeyCode::KeyU => VIRTUAL_KEY::default(),
    KeyCode::KeyV => VIRTUAL_KEY::default(),
    KeyCode::KeyW => VIRTUAL_KEY::default(),
    KeyCode::KeyX => VIRTUAL_KEY::default(),
    KeyCode::KeyY => VIRTUAL_KEY::default(),
    KeyCode::KeyZ => VIRTUAL_KEY::default(),
    KeyCode::Minus => VIRTUAL_KEY::default(),
    KeyCode::Period => VIRTUAL_KEY::default(),
    KeyCode::Quote => VIRTUAL_KEY::default(),
    KeyCode::Semicolon => VIRTUAL_KEY::default(),
    KeyCode::Slash => VIRTUAL_KEY::default(),
    KeyCode::AltLeft => VK_LMENU,
    KeyCode::AltRight => VK_RMENU,
    KeyCode::Backspace => VK_BACK,
    KeyCode::CapsLock => VK_CAPITAL,
    KeyCode::ContextMenu => VK_APPS,
    KeyCode::ControlLeft => VK_LCONTROL,
    KeyCode::ControlRight => VK_RCONTROL,
    KeyCode::Enter => VK_RETURN,
    KeyCode::SuperLeft => VK_LWIN,
    KeyCode::SuperRight => VK_RWIN,
    KeyCode::ShiftLeft => VK_RSHIFT,
    KeyCode::ShiftRight => VK_LSHIFT,
    KeyCode::Space => VK_SPACE,
    KeyCode::Tab => VK_TAB,
    KeyCode::Convert => VK_CONVERT,
    KeyCode::KanaMode => VK_KANA,
    KeyCode::Lang1 if is_korean => VK_HANGUL,
    KeyCode::Lang1 if is_japanese => VK_KANA,
    KeyCode::Lang2 if is_korean => VK_HANJA,
    KeyCode::Lang2 if is_japanese => VIRTUAL_KEY::default(),
    KeyCode::Lang3 if is_japanese => VK_OEM_FINISH,
    KeyCode::Lang4 if is_japanese => VIRTUAL_KEY::default(),
    KeyCode::Lang5 if is_japanese => VIRTUAL_KEY::default(),
    KeyCode::NonConvert => VK_NONCONVERT,
    KeyCode::Delete => VK_DELETE,
    KeyCode::End => VK_END,
    KeyCode::Help => VK_HELP,
    KeyCode::Home => VK_HOME,
    KeyCode::Insert => VK_INSERT,
    KeyCode::PageDown => VK_NEXT,
    KeyCode::PageUp => VK_PRIOR,
    KeyCode::ArrowDown => VK_DOWN,
    KeyCode::ArrowLeft => VK_LEFT,
    KeyCode::ArrowRight => VK_RIGHT,
    KeyCode::ArrowUp => VK_UP,
    KeyCode::NumLock => VK_NUMLOCK,
    KeyCode::Numpad0 => VK_NUMPAD0,
    KeyCode::Numpad1 => VK_NUMPAD1,
    KeyCode::Numpad2 => VK_NUMPAD2,
    KeyCode::Numpad3 => VK_NUMPAD3,
    KeyCode::Numpad4 => VK_NUMPAD4,
    KeyCode::Numpad5 => VK_NUMPAD5,
    KeyCode::Numpad6 => VK_NUMPAD6,
    KeyCode::Numpad7 => VK_NUMPAD7,
    KeyCode::Numpad8 => VK_NUMPAD8,
    KeyCode::Numpad9 => VK_NUMPAD9,
    KeyCode::NumpadAdd => VK_ADD,
    KeyCode::NumpadBackspace => VK_BACK,
    KeyCode::NumpadClear => VK_CLEAR,
    KeyCode::NumpadClearEntry => VIRTUAL_KEY::default(),
    KeyCode::NumpadComma => VK_SEPARATOR,
    KeyCode::NumpadDecimal => VK_DECIMAL,
    KeyCode::NumpadDivide => VK_DIVIDE,
    KeyCode::NumpadEnter => VK_RETURN,
    KeyCode::NumpadEqual => VIRTUAL_KEY::default(),
    KeyCode::NumpadHash => VIRTUAL_KEY::default(),
    KeyCode::NumpadMemoryAdd => VIRTUAL_KEY::default(),
    KeyCode::NumpadMemoryClear => VIRTUAL_KEY::default(),
    KeyCode::NumpadMemoryRecall => VIRTUAL_KEY::default(),
    KeyCode::NumpadMemoryStore => VIRTUAL_KEY::default(),
    KeyCode::NumpadMemorySubtract => VIRTUAL_KEY::default(),
    KeyCode::NumpadMultiply => VK_MULTIPLY,
    KeyCode::NumpadParenLeft => VIRTUAL_KEY::default(),
    KeyCode::NumpadParenRight => VIRTUAL_KEY::default(),
    KeyCode::NumpadStar => VIRTUAL_KEY::default(),
    KeyCode::NumpadSubtract => VK_SUBTRACT,
    KeyCode::Escape => VK_ESCAPE,
    KeyCode::Fn => VIRTUAL_KEY::default(),
    KeyCode::FnLock => VIRTUAL_KEY::default(),
    KeyCode::PrintScreen => VK_SNAPSHOT,
    KeyCode::ScrollLock => VK_SCROLL,
    KeyCode::Pause => VK_PAUSE,
    KeyCode::BrowserBack => VK_BROWSER_BACK,
    KeyCode::BrowserFavorites => VK_BROWSER_FAVORITES,
    KeyCode::BrowserForward => VK_BROWSER_FORWARD,
    KeyCode::BrowserHome => VK_BROWSER_HOME,
    KeyCode::BrowserRefresh => VK_BROWSER_REFRESH,
    KeyCode::BrowserSearch => VK_BROWSER_SEARCH,
    KeyCode::BrowserStop => VK_BROWSER_STOP,
    KeyCode::Eject => VIRTUAL_KEY::default(),
    KeyCode::LaunchApp1 => VK_LAUNCH_APP1,
    KeyCode::LaunchApp2 => VK_LAUNCH_APP2,
    KeyCode::LaunchMail => VK_LAUNCH_MAIL,
    KeyCode::MediaPlayPause => VK_MEDIA_PLAY_PAUSE,
    KeyCode::MediaSelect => VK_LAUNCH_MEDIA_SELECT,
    KeyCode::MediaStop => VK_MEDIA_STOP,
    KeyCode::MediaTrackNext => VK_MEDIA_NEXT_TRACK,
    KeyCode::MediaTrackPrevious => VK_MEDIA_PREV_TRACK,
    KeyCode::Power => VIRTUAL_KEY::default(),
    KeyCode::Sleep => VIRTUAL_KEY::default(),
    KeyCode::AudioVolumeDown => VK_VOLUME_DOWN,
    KeyCode::AudioVolumeMute => VK_VOLUME_MUTE,
    KeyCode::AudioVolumeUp => VK_VOLUME_UP,
    KeyCode::WakeUp => VIRTUAL_KEY::default(),
    KeyCode::Hyper => VIRTUAL_KEY::default(),
    KeyCode::Turbo => VIRTUAL_KEY::default(),
    KeyCode::Abort => VIRTUAL_KEY::default(),
    KeyCode::Resume => VIRTUAL_KEY::default(),
    KeyCode::Suspend => VIRTUAL_KEY::default(),
    KeyCode::Again => VIRTUAL_KEY::default(),
    KeyCode::Copy => VIRTUAL_KEY::default(),
    KeyCode::Cut => VIRTUAL_KEY::default(),
    KeyCode::Find => VIRTUAL_KEY::default(),
    KeyCode::Open => VIRTUAL_KEY::default(),
    KeyCode::Paste => VIRTUAL_KEY::default(),
    KeyCode::Props => VIRTUAL_KEY::default(),
    KeyCode::Select => VK_SELECT,
    KeyCode::Undo => VIRTUAL_KEY::default(),
    KeyCode::Hiragana => VIRTUAL_KEY::default(),
    KeyCode::Katakana => VIRTUAL_KEY::default(),
    KeyCode::F1 => VK_F1,
    KeyCode::F2 => VK_F2,
    KeyCode::F3 => VK_F3,
    KeyCode::F4 => VK_F4,
    KeyCode::F5 => VK_F5,
    KeyCode::F6 => VK_F6,
    KeyCode::F7 => VK_F7,
    KeyCode::F8 => VK_F8,
    KeyCode::F9 => VK_F9,
    KeyCode::F10 => VK_F10,
    KeyCode::F11 => VK_F11,
    KeyCode::F12 => VK_F12,
    KeyCode::F13 => VK_F13,
    KeyCode::F14 => VK_F14,
    KeyCode::F15 => VK_F15,
    KeyCode::F16 => VK_F16,
    KeyCode::F17 => VK_F17,
    KeyCode::F18 => VK_F18,
    KeyCode::F19 => VK_F19,
    KeyCode::F20 => VK_F20,
    KeyCode::F21 => VK_F21,
    KeyCode::F22 => VK_F22,
    KeyCode::F23 => VK_F23,
    KeyCode::F24 => VK_F24,
    KeyCode::F25 => VIRTUAL_KEY::default(),
    KeyCode::F26 => VIRTUAL_KEY::default(),
    KeyCode::F27 => VIRTUAL_KEY::default(),
    KeyCode::F28 => VIRTUAL_KEY::default(),
    KeyCode::F29 => VIRTUAL_KEY::default(),
    KeyCode::F30 => VIRTUAL_KEY::default(),
    KeyCode::F31 => VIRTUAL_KEY::default(),
    KeyCode::F32 => VIRTUAL_KEY::default(),
    KeyCode::F33 => VIRTUAL_KEY::default(),
    KeyCode::F34 => VIRTUAL_KEY::default(),
    KeyCode::F35 => VIRTUAL_KEY::default(),
    KeyCode::Unidentified(_) => VIRTUAL_KEY::default(),
    _ => VIRTUAL_KEY::default(),
  }
}

/// This converts virtual keys to `Key`s. Only virtual keys which can be unambiguously converted to
/// a `Key`, with only the information passed in as arguments, are converted.
///
/// In other words: this function does not need to "prepare" the current layout in order to do
/// the conversion, but as such it cannot convert certain keys, like language-specific character keys.
///
/// The result includes all non-character keys defined within `Key` plus characters from numpad keys.
/// For example, backspace and tab are included.
fn vkey_to_non_char_key(
  vkey: VIRTUAL_KEY,
  native_code: NativeKeyCode,
  hkl: HKL,
  has_alt_graph: bool,
) -> Key<'static> {
  // List of the Web key names and their corresponding platform-native key names:
  // https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/key/Key_Values

  let primary_lang_id = util::PRIMARYLANGID(hkl);
  let is_korean = primary_lang_id == LANG_KOREAN;
  let is_japanese = primary_lang_id == LANG_JAPANESE;

  match vkey {
    win32km::VK_LBUTTON => Key::Unidentified(NativeKeyCode::Unidentified), // Mouse
    win32km::VK_RBUTTON => Key::Unidentified(NativeKeyCode::Unidentified), // Mouse

    // I don't think this can be represented with a Key
    win32km::VK_CANCEL => Key::Unidentified(native_code),

    win32km::VK_MBUTTON => Key::Unidentified(NativeKeyCode::Unidentified), // Mouse
    win32km::VK_XBUTTON1 => Key::Unidentified(NativeKeyCode::Unidentified), // Mouse
    win32km::VK_XBUTTON2 => Key::Unidentified(NativeKeyCode::Unidentified), // Mouse
    win32km::VK_BACK => Key::Backspace,
    win32km::VK_TAB => Key::Tab,
    win32km::VK_CLEAR => Key::Clear,
    win32km::VK_RETURN => Key::Enter,
    win32km::VK_SHIFT => Key::Shift,
    win32km::VK_CONTROL => Key::Control,
    win32km::VK_MENU => Key::Alt,
    win32km::VK_PAUSE => Key::Pause,
    win32km::VK_CAPITAL => Key::CapsLock,

    //win32km::VK_HANGEUL => Key::HangulMode, // Deprecated in favour of VK_HANGUL

    // VK_HANGUL and VK_KANA are defined as the same constant, therefore
    // we use appropriate conditions to differentate between them
    win32km::VK_HANGUL if is_korean => Key::HangulMode,
    win32km::VK_KANA if is_japanese => Key::KanaMode,

    win32km::VK_JUNJA => Key::JunjaMode,
    win32km::VK_FINAL => Key::FinalMode,

    // VK_HANJA and VK_KANJI are defined as the same constant, therefore
    // we use appropriate conditions to differentate between them
    win32km::VK_HANJA if is_korean => Key::HanjaMode,
    win32km::VK_KANJI if is_japanese => Key::KanjiMode,

    win32km::VK_ESCAPE => Key::Escape,
    win32km::VK_CONVERT => Key::Convert,
    win32km::VK_NONCONVERT => Key::NonConvert,
    win32km::VK_ACCEPT => Key::Accept,
    win32km::VK_MODECHANGE => Key::ModeChange,
    win32km::VK_SPACE => Key::Space,
    win32km::VK_PRIOR => Key::PageUp,
    win32km::VK_NEXT => Key::PageDown,
    win32km::VK_END => Key::End,
    win32km::VK_HOME => Key::Home,
    win32km::VK_LEFT => Key::ArrowLeft,
    win32km::VK_UP => Key::ArrowUp,
    win32km::VK_RIGHT => Key::ArrowRight,
    win32km::VK_DOWN => Key::ArrowDown,
    win32km::VK_SELECT => Key::Select,
    win32km::VK_PRINT => Key::Print,
    win32km::VK_EXECUTE => Key::Execute,
    win32km::VK_SNAPSHOT => Key::PrintScreen,
    win32km::VK_INSERT => Key::Insert,
    win32km::VK_DELETE => Key::Delete,
    win32km::VK_HELP => Key::Help,
    win32km::VK_LWIN => Key::Super,
    win32km::VK_RWIN => Key::Super,
    win32km::VK_APPS => Key::ContextMenu,
    win32km::VK_SLEEP => Key::Standby,

    // Numpad keys produce characters
    win32km::VK_NUMPAD0 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD1 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD2 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD3 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD4 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD5 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD6 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD7 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD8 => Key::Unidentified(native_code),
    win32km::VK_NUMPAD9 => Key::Unidentified(native_code),
    win32km::VK_MULTIPLY => Key::Unidentified(native_code),
    win32km::VK_ADD => Key::Unidentified(native_code),
    win32km::VK_SEPARATOR => Key::Unidentified(native_code),
    win32km::VK_SUBTRACT => Key::Unidentified(native_code),
    win32km::VK_DECIMAL => Key::Unidentified(native_code),
    win32km::VK_DIVIDE => Key::Unidentified(native_code),

    win32km::VK_F1 => Key::F1,
    win32km::VK_F2 => Key::F2,
    win32km::VK_F3 => Key::F3,
    win32km::VK_F4 => Key::F4,
    win32km::VK_F5 => Key::F5,
    win32km::VK_F6 => Key::F6,
    win32km::VK_F7 => Key::F7,
    win32km::VK_F8 => Key::F8,
    win32km::VK_F9 => Key::F9,
    win32km::VK_F10 => Key::F10,
    win32km::VK_F11 => Key::F11,
    win32km::VK_F12 => Key::F12,
    win32km::VK_F13 => Key::F13,
    win32km::VK_F14 => Key::F14,
    win32km::VK_F15 => Key::F15,
    win32km::VK_F16 => Key::F16,
    win32km::VK_F17 => Key::F17,
    win32km::VK_F18 => Key::F18,
    win32km::VK_F19 => Key::F19,
    win32km::VK_F20 => Key::F20,
    win32km::VK_F21 => Key::F21,
    win32km::VK_F22 => Key::F22,
    win32km::VK_F23 => Key::F23,
    win32km::VK_F24 => Key::F24,
    win32km::VK_NAVIGATION_VIEW => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_MENU => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_UP => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_DOWN => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_LEFT => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_RIGHT => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_ACCEPT => Key::Unidentified(native_code),
    win32km::VK_NAVIGATION_CANCEL => Key::Unidentified(native_code),
    win32km::VK_NUMLOCK => Key::NumLock,
    win32km::VK_SCROLL => Key::ScrollLock,
    win32km::VK_OEM_NEC_EQUAL => Key::Unidentified(native_code),
    //win32km::VK_OEM_FJ_JISHO => Key::Unidentified(native_code), // Conflicts with `VK_OEM_NEC_EQUAL`
    win32km::VK_OEM_FJ_MASSHOU => Key::Unidentified(native_code),
    win32km::VK_OEM_FJ_TOUROKU => Key::Unidentified(native_code),
    win32km::VK_OEM_FJ_LOYA => Key::Unidentified(native_code),
    win32km::VK_OEM_FJ_ROYA => Key::Unidentified(native_code),
    win32km::VK_LSHIFT => Key::Shift,
    win32km::VK_RSHIFT => Key::Shift,
    win32km::VK_LCONTROL => Key::Control,
    win32km::VK_RCONTROL => Key::Control,
    win32km::VK_LMENU => Key::Alt,
    win32km::VK_RMENU => {
      if has_alt_graph {
        Key::AltGraph
      } else {
        Key::Alt
      }
    }
    win32km::VK_BROWSER_BACK => Key::BrowserBack,
    win32km::VK_BROWSER_FORWARD => Key::BrowserForward,
    win32km::VK_BROWSER_REFRESH => Key::BrowserRefresh,
    win32km::VK_BROWSER_STOP => Key::BrowserStop,
    win32km::VK_BROWSER_SEARCH => Key::BrowserSearch,
    win32km::VK_BROWSER_FAVORITES => Key::BrowserFavorites,
    win32km::VK_BROWSER_HOME => Key::BrowserHome,
    win32km::VK_VOLUME_MUTE => Key::AudioVolumeMute,
    win32km::VK_VOLUME_DOWN => Key::AudioVolumeDown,
    win32km::VK_VOLUME_UP => Key::AudioVolumeUp,
    win32km::VK_MEDIA_NEXT_TRACK => Key::MediaTrackNext,
    win32km::VK_MEDIA_PREV_TRACK => Key::MediaTrackPrevious,
    win32km::VK_MEDIA_STOP => Key::MediaStop,
    win32km::VK_MEDIA_PLAY_PAUSE => Key::MediaPlayPause,
    win32km::VK_LAUNCH_MAIL => Key::LaunchMail,
    win32km::VK_LAUNCH_MEDIA_SELECT => Key::LaunchMediaPlayer,
    win32km::VK_LAUNCH_APP1 => Key::LaunchApplication1,
    win32km::VK_LAUNCH_APP2 => Key::LaunchApplication2,

    // This function only converts "non-printable"
    win32km::VK_OEM_1 => Key::Unidentified(native_code),
    win32km::VK_OEM_PLUS => Key::Unidentified(native_code),
    win32km::VK_OEM_COMMA => Key::Unidentified(native_code),
    win32km::VK_OEM_MINUS => Key::Unidentified(native_code),
    win32km::VK_OEM_PERIOD => Key::Unidentified(native_code),
    win32km::VK_OEM_2 => Key::Unidentified(native_code),
    win32km::VK_OEM_3 => Key::Unidentified(native_code),

    win32km::VK_GAMEPAD_A => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_B => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_X => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_Y => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_SHOULDER => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_SHOULDER => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_TRIGGER => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_TRIGGER => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_DPAD_UP => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_DPAD_DOWN => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_DPAD_LEFT => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_DPAD_RIGHT => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_MENU => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_VIEW => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_THUMBSTICK_BUTTON => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_THUMBSTICK_BUTTON => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_THUMBSTICK_UP => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_THUMBSTICK_DOWN => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_THUMBSTICK_RIGHT => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_LEFT_THUMBSTICK_LEFT => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_THUMBSTICK_UP => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_THUMBSTICK_DOWN => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_THUMBSTICK_RIGHT => Key::Unidentified(native_code),
    win32km::VK_GAMEPAD_RIGHT_THUMBSTICK_LEFT => Key::Unidentified(native_code),

    // This function only converts "non-printable"
    win32km::VK_OEM_4 => Key::Unidentified(native_code),
    win32km::VK_OEM_5 => Key::Unidentified(native_code),
    win32km::VK_OEM_6 => Key::Unidentified(native_code),
    win32km::VK_OEM_7 => Key::Unidentified(native_code),
    win32km::VK_OEM_8 => Key::Unidentified(native_code),
    win32km::VK_OEM_AX => Key::Unidentified(native_code),
    win32km::VK_OEM_102 => Key::Unidentified(native_code),

    win32km::VK_ICO_HELP => Key::Unidentified(native_code),
    win32km::VK_ICO_00 => Key::Unidentified(native_code),

    win32km::VK_PROCESSKEY => Key::Process,

    win32km::VK_ICO_CLEAR => Key::Unidentified(native_code),
    win32km::VK_PACKET => Key::Unidentified(native_code),
    win32km::VK_OEM_RESET => Key::Unidentified(native_code),
    win32km::VK_OEM_JUMP => Key::Unidentified(native_code),
    win32km::VK_OEM_PA1 => Key::Unidentified(native_code),
    win32km::VK_OEM_PA2 => Key::Unidentified(native_code),
    win32km::VK_OEM_PA3 => Key::Unidentified(native_code),
    win32km::VK_OEM_WSCTRL => Key::Unidentified(native_code),
    win32km::VK_OEM_CUSEL => Key::Unidentified(native_code),

    win32km::VK_OEM_ATTN => Key::Attn,
    win32km::VK_OEM_FINISH => {
      if is_japanese {
        Key::Katakana
      } else {
        // This matches IE and Firefox behaviour according to
        // https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/key/Key_Values
        // At the time of writing, there is no `Key::Finish` variant as
        // Finish is not mentionned at https://w3c.github.io/uievents-key/
        // Also see: https://github.com/pyfisch/keyboard-types/issues/9
        Key::Unidentified(native_code)
      }
    }
    win32km::VK_OEM_COPY => Key::Copy,
    win32km::VK_OEM_AUTO => Key::Hankaku,
    win32km::VK_OEM_ENLW => Key::Zenkaku,
    win32km::VK_OEM_BACKTAB => Key::Romaji,
    win32km::VK_ATTN => Key::KanaMode,
    win32km::VK_CRSEL => Key::CrSel,
    win32km::VK_EXSEL => Key::ExSel,
    win32km::VK_EREOF => Key::EraseEof,
    win32km::VK_PLAY => Key::Play,
    win32km::VK_ZOOM => Key::ZoomToggle,
    win32km::VK_NONAME => Key::Unidentified(native_code),
    win32km::VK_PA1 => Key::Unidentified(native_code),
    win32km::VK_OEM_CLEAR => Key::Clear,
    _ => Key::Unidentified(native_code),
  }
}
