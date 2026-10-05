// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! **UNSTABLE** -- Types related to the keyboard.

// This file contains a substantial portion of the UI Events Specification by the W3C. In
// particular, the variant names within `Key` and `KeyCode` and their documentation are modified
// versions of contents of the aforementioned specification.
//
// The original documents are:
//
// ### For `Key`
// UI Events KeyboardEvent key Values
// https://www.w3.org/TR/2017/CR-uievents-key-20170601/
// Copyright © 2017 W3C® (MIT, ERCIM, Keio, Beihang).
//
// ### For `KeyCode`
// UI Events KeyboardEvent code Values
// https://www.w3.org/TR/2017/CR-uievents-code-20170601/
// Copyright © 2017 W3C® (MIT, ERCIM, Keio, Beihang).
//
// These documents were used under the terms of the following license. This W3C license as well as
// the W3C short notice apply to the `Key` and `KeyCode` enums and their variants and the
// documentation attached to their variants.

// --------- BEGGINING OF W3C LICENSE --------------------------------------------------------------
//
// License
//
// By obtaining and/or copying this work, you (the licensee) agree that you have read, understood,
// and will comply with the following terms and conditions.
//
// Permission to copy, modify, and distribute this work, with or without modification, for any
// purpose and without fee or royalty is hereby granted, provided that you include the following on
// ALL copies of the work or portions thereof, including modifications:
//
// - The full text of this NOTICE in a location viewable to users of the redistributed or derivative
//   work.
// - Any pre-existing intellectual property disclaimers, notices, or terms and conditions. If none
//   exist, the W3C Software and Document Short Notice should be included.
// - Notice of any changes or modifications, through a copyright statement on the new code or
//   document such as "This software or document includes material copied from or derived from
//   [title and URI of the W3C document]. Copyright © [YEAR] W3C® (MIT, ERCIM, Keio, Beihang)."
//
// Disclaimers
//
// THIS WORK IS PROVIDED "AS IS," AND COPYRIGHT HOLDERS MAKE NO REPRESENTATIONS OR WARRANTIES,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO, WARRANTIES OF MERCHANTABILITY OR FITNESS FOR
// ANY PARTICULAR PURPOSE OR THAT THE USE OF THE SOFTWARE OR DOCUMENT WILL NOT INFRINGE ANY THIRD
// PARTY PATENTS, COPYRIGHTS, TRADEMARKS OR OTHER RIGHTS.
//
// COPYRIGHT HOLDERS WILL NOT BE LIABLE FOR ANY DIRECT, INDIRECT, SPECIAL OR CONSEQUENTIAL DAMAGES
// ARISING OUT OF ANY USE OF THE SOFTWARE OR DOCUMENT.
//
// The name and trademarks of copyright holders may NOT be used in advertising or publicity
// pertaining to the work without specific, written prior permission. Title to copyright in this
// work will at all times remain with copyright holders.
//
// --------- END OF W3C LICENSE --------------------------------------------------------------------

// --------- BEGGINING OF W3C SHORT NOTICE ---------------------------------------------------------
//
// tao: https://github.com/tauri-apps/tao
//
// Copyright © 2021 World Wide Web Consortium, (Massachusetts Institute of Technology, European
// Research Consortium for Informatics and Mathematics, Keio University, Beihang). All Rights
// Reserved. This work is distributed under the W3C® Software License [1] in the hope that it will
// be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE.
//
// [1] http://www.w3.org/Consortium/Legal/copyright-software
//
// --------- END OF W3C SHORT NOTICE ---------------------------------------------------------------

use std::{fmt, str::FromStr};

use crate::{
  error::OsError,
  platform_impl::{
    keycode_from_scancode as platform_keycode_from_scancode,
    keycode_to_scancode as platform_keycode_to_scancode,
  },
};

impl ModifiersState {
  /// Returns `true` if the shift key is pressed.
  pub fn shift_key(&self) -> bool {
    self.intersects(Self::SHIFT)
  }
  /// Returns `true` if the control key is pressed.
  pub fn control_key(&self) -> bool {
    self.intersects(Self::CONTROL)
  }
  /// Returns `true` if the alt key is pressed.
  pub fn alt_key(&self) -> bool {
    self.intersects(Self::ALT)
  }
  /// Returns `true` if the super key is pressed.
  pub fn super_key(&self) -> bool {
    self.intersects(Self::SUPER)
  }
}

bitflags! {
    /// Represents the current state of the keyboard modifiers
    ///
    /// Each flag represents a modifier and is set if this modifier is active.
    #[derive(Clone, Copy, Default, Debug, PartialEq)]
    pub struct ModifiersState: u32 {
        // left and right modifiers are currently commented out, but we should be able to support
        // them in a future release
        /// The "shift" key.
        const SHIFT = 0b100 << 0;
        // const LSHIFT = 0b010 << 0;
        // const RSHIFT = 0b001 << 0;
        /// The "control" key.
        const CONTROL = 0b100 << 3;
        // const LCTRL = 0b010 << 3;
        // const RCTRL = 0b001 << 3;
        /// The "alt" key.
        const ALT = 0b100 << 6;
        // const LALT = 0b010 << 6;
        // const RALT = 0b001 << 6;
        /// This is the "windows" key on PC and "command" key on Mac.
        const SUPER = 0b100 << 9;
        // const LSUPER  = 0b010 << 9;
        // const RSUPER  = 0b001 << 9;
    }
}

#[cfg(feature = "serde")]
mod modifiers_serde {
  use super::ModifiersState;
  use serde::{Deserialize, Deserializer, Serialize, Serializer};

  #[derive(Default, Serialize, Deserialize)]
  #[serde(default)]
  #[serde(rename = "ModifiersState")]
  pub struct ModifiersStateSerialize {
    pub shift_key: bool,
    pub control_key: bool,
    pub alt_key: bool,
    pub super_key: bool,
  }

  impl Serialize for ModifiersState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: Serializer,
    {
      let s = ModifiersStateSerialize {
        shift_key: self.shift_key(),
        control_key: self.control_key(),
        alt_key: self.alt_key(),
        super_key: self.super_key(),
      };
      s.serialize(serializer)
    }
  }

  impl<'de> Deserialize<'de> for ModifiersState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
      D: Deserializer<'de>,
    {
      let ModifiersStateSerialize {
        shift_key,
        control_key,
        alt_key,
        super_key,
      } = ModifiersStateSerialize::deserialize(deserializer)?;
      let mut m = ModifiersState::empty();
      m.set(ModifiersState::SHIFT, shift_key);
      m.set(ModifiersState::CONTROL, control_key);
      m.set(ModifiersState::ALT, alt_key);
      m.set(ModifiersState::SUPER, super_key);
      Ok(m)
    }
  }
}

/// Contains the platform-native physical key identifier (aka scancode)
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NativeKeyCode {
  Unidentified,
  Windows(u16),
  MacOS(u16),
  Gtk(u16),

  /// This is the android "key code" of the event as returned by
  /// `KeyEvent.getKeyCode()`
  Android(i32),
}

/// Represents the code of a physical key.
///
/// This mostly conforms to the UI Events Specification's [`KeyboardEvent.code`] with a few
/// exceptions:
/// - The keys that the specification calls "MetaLeft" and "MetaRight" are named "SuperLeft" and
///   "SuperRight" here.
/// - The key that the specification calls "Super" is reported as `Unidentified` here.
/// - The `Unidentified` variant here, can still identifiy a key through it's `NativeKeyCode`.
///
/// [`KeyboardEvent.code`]: https://w3c.github.io/uievents-code/#code-value-tables
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum KeyCode {
  /// This variant is used when the key cannot be translated to any
  /// other variant.
  ///
  /// The native scancode is provided (if available) in order
  /// to allow the user to specify keybindings for keys which
  /// are not defined by this API.
  Unidentified(NativeKeyCode),
  /// <kbd>`</kbd> on a US keyboard. This is also called a backtick or grave.
  /// This is the <kbd>半角</kbd>/<kbd>全角</kbd>/<kbd>漢字</kbd>
  /// (hankaku/zenkaku/kanji) key on Japanese keyboards
  Backquote,
  /// Used for both the US <kbd>\\</kbd> (on the 101-key layout) and also for the key
  /// located between the <kbd>"</kbd> and <kbd>Enter</kbd> keys on row C of the 102-,
  /// 104- and 106-key layouts.
  /// Labeled <kbd>#</kbd> on a UK (102) keyboard.
  Backslash,
  /// <kbd>[</kbd> on a US keyboard.
  BracketLeft,
  /// <kbd>]</kbd> on a US keyboard.
  BracketRight,
  /// <kbd>,</kbd> on a US keyboard.
  Comma,
  /// <kbd>0</kbd> on a US keyboard.
  Digit0,
  /// <kbd>1</kbd> on a US keyboard.
  Digit1,
  /// <kbd>2</kbd> on a US keyboard.
  Digit2,
  /// <kbd>3</kbd> on a US keyboard.
  Digit3,
  /// <kbd>4</kbd> on a US keyboard.
  Digit4,
  /// <kbd>5</kbd> on a US keyboard.
  Digit5,
  /// <kbd>6</kbd> on a US keyboard.
  Digit6,
  /// <kbd>7</kbd> on a US keyboard.
  Digit7,
  /// <kbd>8</kbd> on a US keyboard.
  Digit8,
  /// <kbd>9</kbd> on a US keyboard.
  Digit9,
  /// <kbd>=</kbd> on a US keyboard.
  Equal,
  /// Located between the left <kbd>Shift</kbd> and <kbd>Z</kbd> keys.
  /// Labeled <kbd>\\</kbd> on a UK keyboard.
  IntlBackslash,
  /// Located between the <kbd>/</kbd> and right <kbd>Shift</kbd> keys.
  /// Labeled <kbd>\\</kbd> (ro) on a Japanese keyboard.
  IntlRo,
  /// Located between the <kbd>=</kbd> and <kbd>Backspace</kbd> keys.
  /// Labeled <kbd>¥</kbd> (yen) on a Japanese keyboard. <kbd>\\</kbd> on a
  /// Russian keyboard.
  IntlYen,
  /// <kbd>a</kbd> on a US keyboard.
  /// Labeled <kbd>q</kbd> on an AZERTY (e.g., French) keyboard.
  KeyA,
  /// <kbd>b</kbd> on a US keyboard.
  KeyB,
  /// <kbd>c</kbd> on a US keyboard.
  KeyC,
  /// <kbd>d</kbd> on a US keyboard.
  KeyD,
  /// <kbd>e</kbd> on a US keyboard.
  KeyE,
  /// <kbd>f</kbd> on a US keyboard.
  KeyF,
  /// <kbd>g</kbd> on a US keyboard.
  KeyG,
  /// <kbd>h</kbd> on a US keyboard.
  KeyH,
  /// <kbd>i</kbd> on a US keyboard.
  KeyI,
  /// <kbd>j</kbd> on a US keyboard.
  KeyJ,
  /// <kbd>k</kbd> on a US keyboard.
  KeyK,
  /// <kbd>l</kbd> on a US keyboard.
  KeyL,
  /// <kbd>m</kbd> on a US keyboard.
  KeyM,
  /// <kbd>n</kbd> on a US keyboard.
  KeyN,
  /// <kbd>o</kbd> on a US keyboard.
  KeyO,
  /// <kbd>p</kbd> on a US keyboard.
  KeyP,
  /// <kbd>q</kbd> on a US keyboard.
  /// Labeled <kbd>a</kbd> on an AZERTY (e.g., French) keyboard.
  KeyQ,
  /// <kbd>r</kbd> on a US keyboard.
  KeyR,
  /// <kbd>s</kbd> on a US keyboard.
  KeyS,
  /// <kbd>t</kbd> on a US keyboard.
  KeyT,
  /// <kbd>u</kbd> on a US keyboard.
  KeyU,
  /// <kbd>v</kbd> on a US keyboard.
  KeyV,
  /// <kbd>w</kbd> on a US keyboard.
  /// Labeled <kbd>z</kbd> on an AZERTY (e.g., French) keyboard.
  KeyW,
  /// <kbd>x</kbd> on a US keyboard.
  KeyX,
  /// <kbd>y</kbd> on a US keyboard.
  /// Labeled <kbd>z</kbd> on a QWERTZ (e.g., German) keyboard.
  KeyY,
  /// <kbd>z</kbd> on a US keyboard.
  /// Labeled <kbd>w</kbd> on an AZERTY (e.g., French) keyboard, and <kbd>y</kbd> on a
  /// QWERTZ (e.g., German) keyboard.
  KeyZ,
  /// <kbd>-</kbd> on a US keyboard.
  Minus,
  /// <kbd>Shift</kbd>+<kbd>=</kbd> on a US keyboard.
  Plus,
  /// <kbd>.</kbd> on a US keyboard.
  Period,
  /// <kbd>'</kbd> on a US keyboard.
  Quote,
  /// <kbd>;</kbd> on a US keyboard.
  Semicolon,
  /// <kbd>/</kbd> on a US keyboard.
  Slash,
  /// <kbd>Alt</kbd>, <kbd>Option</kbd>, or <kbd>⌥</kbd>.
  AltLeft,
  /// <kbd>Alt</kbd>, <kbd>Option</kbd>, or <kbd>⌥</kbd>.
  /// This is labeled <kbd>AltGr</kbd> on many keyboard layouts.
  AltRight,
  /// <kbd>Backspace</kbd> or <kbd>⌫</kbd>.
  /// Labeled <kbd>Delete</kbd> on Apple keyboards.
  Backspace,
  /// <kbd>CapsLock</kbd> or <kbd>⇪</kbd>
  CapsLock,
  /// The application context menu key, which is typically found between the right
  /// <kbd>Super</kbd> key and the right <kbd>Control</kbd> key.
  ContextMenu,
  /// <kbd>Control</kbd> or <kbd>⌃</kbd>
  ControlLeft,
  /// <kbd>Control</kbd> or <kbd>⌃</kbd>
  ControlRight,
  /// <kbd>Enter</kbd> or <kbd>↵</kbd>. Labeled <kbd>Return</kbd> on Apple keyboards.
  Enter,
  /// The Windows, <kbd>⌘</kbd>, <kbd>Command</kbd>, or other OS symbol key.
  SuperLeft,
  /// The Windows, <kbd>⌘</kbd>, <kbd>Command</kbd>, or other OS symbol key.
  SuperRight,
  /// <kbd>Shift</kbd> or <kbd>⇧</kbd>
  ShiftLeft,
  /// <kbd>Shift</kbd> or <kbd>⇧</kbd>
  ShiftRight,
  /// <kbd> </kbd> (space)
  Space,
  /// <kbd>Tab</kbd> or <kbd>⇥</kbd>
  Tab,
  /// Japanese: <kbd>変</kbd> (henkan)
  Convert,
  /// Japanese: <kbd>カタカナ</kbd>/<kbd>ひらがな</kbd>/<kbd>ローマ字</kbd> (katakana/hiragana/romaji)
  KanaMode,
  /// Korean: HangulMode <kbd>한/영</kbd> (han/yeong)
  ///
  /// Japanese (Mac keyboard): <kbd>か</kbd> (kana)
  Lang1,
  /// Korean: Hanja <kbd>한</kbd> (hanja)
  ///
  /// Japanese (Mac keyboard): <kbd>英</kbd> (eisu)
  Lang2,
  /// Japanese (word-processing keyboard): Katakana
  Lang3,
  /// Japanese (word-processing keyboard): Hiragana
  Lang4,
  /// Japanese (word-processing keyboard): Zenkaku/Hankaku
  Lang5,
  /// Japanese: <kbd>無変換</kbd> (muhenkan)
  NonConvert,
  /// <kbd>⌦</kbd>. The forward delete key.
  /// Note that on Apple keyboards, the key labelled <kbd>Delete</kbd> on the main part of
  /// the keyboard is encoded as [`Backspace`].
  ///
  /// [`Backspace`]: Self::Backspace
  Delete,
  /// <kbd>Page Down</kbd>, <kbd>End</kbd>, or <kbd>↘</kbd>
  End,
  /// <kbd>Help</kbd>. Not present on standard PC keyboards.
  Help,
  /// <kbd>Home</kbd> or <kbd>↖</kbd>
  Home,
  /// <kbd>Insert</kbd> or <kbd>Ins</kbd>. Not present on Apple keyboards.
  Insert,
  /// <kbd>Page Down</kbd>, <kbd>PgDn</kbd>, or <kbd>⇟</kbd>
  PageDown,
  /// <kbd>Page Up</kbd>, <kbd>PgUp</kbd>, or <kbd>⇞</kbd>
  PageUp,
  /// <kbd>↓</kbd>
  ArrowDown,
  /// <kbd>←</kbd>
  ArrowLeft,
  /// <kbd>→</kbd>
  ArrowRight,
  /// <kbd>↑</kbd>
  ArrowUp,
  /// On the Mac, this is used for the numpad <kbd>Clear</kbd> key.
  NumLock,
  /// <kbd>0 Ins</kbd> on a keyboard. <kbd>0</kbd> on a phone or remote control
  Numpad0,
  /// <kbd>1 End</kbd> on a keyboard. <kbd>1</kbd> or <kbd>1 QZ</kbd> on a phone or remote control
  Numpad1,
  /// <kbd>2 ↓</kbd> on a keyboard. <kbd>2 ABC</kbd> on a phone or remote control
  Numpad2,
  /// <kbd>3 PgDn</kbd> on a keyboard. <kbd>3 DEF</kbd> on a phone or remote control
  Numpad3,
  /// <kbd>4 ←</kbd> on a keyboard. <kbd>4 GHI</kbd> on a phone or remote control
  Numpad4,
  /// <kbd>5</kbd> on a keyboard. <kbd>5 JKL</kbd> on a phone or remote control
  Numpad5,
  /// <kbd>6 →</kbd> on a keyboard. <kbd>6 MNO</kbd> on a phone or remote control
  Numpad6,
  /// <kbd>7 Home</kbd> on a keyboard. <kbd>7 PQRS</kbd> or <kbd>7 PRS</kbd> on a phone
  /// or remote control
  Numpad7,
  /// <kbd>8 ↑</kbd> on a keyboard. <kbd>8 TUV</kbd> on a phone or remote control
  Numpad8,
  /// <kbd>9 PgUp</kbd> on a keyboard. <kbd>9 WXYZ</kbd> or <kbd>9 WXY</kbd> on a phone
  /// or remote control
  Numpad9,
  /// <kbd>+</kbd>
  NumpadAdd,
  /// Found on the Microsoft Natural Keyboard.
  NumpadBackspace,
  /// <kbd>C</kbd> or <kbd>A</kbd> (All Clear). Also for use with numpads that have a
  /// <kbd>Clear</kbd> key that is separate from the <kbd>NumLock</kbd> key. On the Mac, the
  /// numpad <kbd>Clear</kbd> key is encoded as [`NumLock`].
  ///
  /// [`NumLock`]: Self::NumLock
  NumpadClear,
  /// <kbd>C</kbd> (Clear Entry)
  NumpadClearEntry,
  /// <kbd>,</kbd> (thousands separator). For locales where the thousands separator
  /// is a "." (e.g., Brazil), this key may generate a <kbd>.</kbd>.
  NumpadComma,
  /// <kbd>. Del</kbd>. For locales where the decimal separator is "," (e.g.,
  /// Brazil), this key may generate a <kbd>,</kbd>.
  NumpadDecimal,
  /// <kbd>/</kbd>
  NumpadDivide,
  NumpadEnter,
  /// <kbd>=</kbd>
  NumpadEqual,
  /// <kbd>#</kbd> on a phone or remote control device. This key is typically found
  /// below the <kbd>9</kbd> key and to the right of the <kbd>0</kbd> key.
  NumpadHash,
  /// <kbd>M</kbd> Add current entry to the value stored in memory.
  NumpadMemoryAdd,
  /// <kbd>M</kbd> Clear the value stored in memory.
  NumpadMemoryClear,
  /// <kbd>M</kbd> Replace the current entry with the value stored in memory.
  NumpadMemoryRecall,
  /// <kbd>M</kbd> Replace the value stored in memory with the current entry.
  NumpadMemoryStore,
  /// <kbd>M</kbd> Subtract current entry from the value stored in memory.
  NumpadMemorySubtract,
  /// <kbd>*</kbd> on a keyboard. For use with numpads that provide mathematical
  /// operations (<kbd>+</kbd>, <kbd>-</kbd> <kbd>*</kbd> and <kbd>/</kbd>).
  ///
  /// Use `NumpadStar` for the <kbd>*</kbd> key on phones and remote controls.
  NumpadMultiply,
  /// <kbd>(</kbd> Found on the Microsoft Natural Keyboard.
  NumpadParenLeft,
  /// <kbd>)</kbd> Found on the Microsoft Natural Keyboard.
  NumpadParenRight,
  /// <kbd>*</kbd> on a phone or remote control device.
  ///
  /// This key is typically found below the <kbd>7</kbd> key and to the left of
  /// the <kbd>0</kbd> key.
  ///
  /// Use <kbd>"NumpadMultiply"</kbd> for the <kbd>*</kbd> key on
  /// numeric keypads.
  NumpadStar,
  /// <kbd>-</kbd>
  NumpadSubtract,
  /// <kbd>Esc</kbd> or <kbd>⎋</kbd>
  Escape,
  /// <kbd>Fn</kbd> This is typically a hardware key that does not generate a separate code.
  Fn,
  /// <kbd>FLock</kbd> or <kbd>FnLock</kbd>. Function Lock key. Found on the Microsoft
  /// Natural Keyboard.
  FnLock,
  /// <kbd>PrtScr SysRq</kbd> or <kbd>Print Screen</kbd>
  PrintScreen,
  /// <kbd>Scroll Lock</kbd>
  ScrollLock,
  /// <kbd>Pause Break</kbd>
  Pause,
  /// Some laptops place this key to the left of the <kbd>↑</kbd> key.
  ///
  /// This also the "back" button (triangle) on Android.
  BrowserBack,
  BrowserFavorites,
  /// Some laptops place this key to the right of the <kbd>↑</kbd> key.
  BrowserForward,
  /// The "home" button on Android.
  BrowserHome,
  BrowserRefresh,
  BrowserSearch,
  BrowserStop,
  /// <kbd>Eject</kbd> or <kbd>⏏</kbd>. This key is placed in the function section on some Apple
  /// keyboards.
  Eject,
  /// Sometimes labelled <kbd>My Computer</kbd> on the keyboard
  LaunchApp1,
  /// Sometimes labelled <kbd>Calculator</kbd> on the keyboard
  LaunchApp2,
  LaunchMail,
  MediaPlayPause,
  MediaSelect,
  MediaStop,
  MediaTrackNext,
  MediaTrackPrevious,
  /// This key is placed in the function section on some Apple keyboards, replacing the
  /// <kbd>Eject</kbd> key.
  Power,
  Sleep,
  AudioVolumeDown,
  AudioVolumeMute,
  AudioVolumeUp,
  WakeUp,
  Hyper,
  Turbo,
  Abort,
  Resume,
  Suspend,
  /// Found on Sun’s USB keyboard.
  Again,
  /// Found on Sun’s USB keyboard.
  Copy,
  /// Found on Sun’s USB keyboard.
  Cut,
  /// Found on Sun’s USB keyboard.
  Find,
  /// Found on Sun’s USB keyboard.
  Open,
  /// Found on Sun’s USB keyboard.
  Paste,
  /// Found on Sun’s USB keyboard.
  Props,
  /// Found on Sun’s USB keyboard.
  Select,
  /// Found on Sun’s USB keyboard.
  Undo,
  /// Use for dedicated <kbd>ひらがな</kbd> key found on some Japanese word processing keyboards.
  Hiragana,
  /// Use for dedicated <kbd>カタカナ</kbd> key found on some Japanese word processing keyboards.
  Katakana,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F1,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F2,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F3,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F4,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F5,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F6,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F7,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F8,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F9,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F10,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F11,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F12,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F13,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F14,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F15,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F16,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F17,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F18,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F19,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F20,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F21,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F22,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F23,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F24,
  /// General-purpose function key.
  F25,
  /// General-purpose function key.
  F26,
  /// General-purpose function key.
  F27,
  /// General-purpose function key.
  F28,
  /// General-purpose function key.
  F29,
  /// General-purpose function key.
  F30,
  /// General-purpose function key.
  F31,
  /// General-purpose function key.
  F32,
  /// General-purpose function key.
  F33,
  /// General-purpose function key.
  F34,
  /// General-purpose function key.
  F35,
}

impl KeyCode {
  /// Return platform specific scancode.
  pub fn to_scancode(self) -> Option<u32> {
    platform_keycode_to_scancode(self)
  }
  /// Return `KeyCode` from platform scancode.
  pub fn from_scancode(scancode: u32) -> KeyCode {
    platform_keycode_from_scancode(scancode)
  }
}

impl FromStr for KeyCode {
  type Err = OsError;
  fn from_str(accelerator_string: &str) -> Result<Self, Self::Err> {
    let keycode = match accelerator_string.to_uppercase().as_str() {
      "`" | "BACKQUOTE" => KeyCode::Backquote,
      "BACKSLASH" => KeyCode::Backslash,
      "[" | "BRACKETLEFT" => KeyCode::BracketLeft,
      "]" | "BRACKETRIGHT" => KeyCode::BracketRight,
      "," | "COMMA" => KeyCode::Comma,
      "0" => KeyCode::Digit0,
      "1" => KeyCode::Digit1,
      "2" => KeyCode::Digit2,
      "3" => KeyCode::Digit3,
      "4" => KeyCode::Digit4,
      "5" => KeyCode::Digit5,
      "6" => KeyCode::Digit6,
      "7" => KeyCode::Digit7,
      "8" => KeyCode::Digit8,
      "9" => KeyCode::Digit9,
      "NUM0" | "NUMPAD0" => KeyCode::Numpad0,
      "NUM1" | "NUMPAD1" => KeyCode::Numpad1,
      "NUM2" | "NUMPAD2" => KeyCode::Numpad2,
      "NUM3" | "NUMPAD3" => KeyCode::Numpad3,
      "NUM4" | "NUMPAD4" => KeyCode::Numpad4,
      "NUM5" | "NUMPAD5" => KeyCode::Numpad5,
      "NUM6" | "NUMPAD6" => KeyCode::Numpad6,
      "NUM7" | "NUMPAD7" => KeyCode::Numpad7,
      "NUM8" | "NUMPAD8" => KeyCode::Numpad8,
      "NUM9" | "NUMPAD9" => KeyCode::Numpad9,
      "=" => KeyCode::Equal,
      "-" => KeyCode::Minus,
      "PLUS" => KeyCode::Plus,
      "." | "PERIOD" => KeyCode::Period,
      "'" | "QUOTE" => KeyCode::Quote,
      "\\" => KeyCode::IntlBackslash,
      "A" => KeyCode::KeyA,
      "B" => KeyCode::KeyB,
      "C" => KeyCode::KeyC,
      "D" => KeyCode::KeyD,
      "E" => KeyCode::KeyE,
      "F" => KeyCode::KeyF,
      "G" => KeyCode::KeyG,
      "H" => KeyCode::KeyH,
      "I" => KeyCode::KeyI,
      "J" => KeyCode::KeyJ,
      "K" => KeyCode::KeyK,
      "L" => KeyCode::KeyL,
      "M" => KeyCode::KeyM,
      "N" => KeyCode::KeyN,
      "O" => KeyCode::KeyO,
      "P" => KeyCode::KeyP,
      "Q" => KeyCode::KeyQ,
      "R" => KeyCode::KeyR,
      "S" => KeyCode::KeyS,
      "T" => KeyCode::KeyT,
      "U" => KeyCode::KeyU,
      "V" => KeyCode::KeyV,
      "W" => KeyCode::KeyW,
      "X" => KeyCode::KeyX,
      "Y" => KeyCode::KeyY,
      "Z" => KeyCode::KeyZ,

      ";" | "SEMICOLON" => KeyCode::Semicolon,
      "/" | "SLASH" => KeyCode::Slash,
      "BACKSPACE" => KeyCode::Backspace,
      "CAPSLOCK" => KeyCode::CapsLock,
      "CONTEXTMENU" => KeyCode::ContextMenu,
      "ENTER" => KeyCode::Enter,
      "SPACE" => KeyCode::Space,
      "TAB" => KeyCode::Tab,
      "CONVERT" => KeyCode::Convert,

      "DELETE" => KeyCode::Delete,
      "END" => KeyCode::End,
      "HELP" => KeyCode::Help,
      "HOME" => KeyCode::Home,
      "PAGEDOWN" => KeyCode::PageDown,
      "PAGEUP" => KeyCode::PageUp,

      "DOWN" | "ARROWDOWN" => KeyCode::ArrowDown,
      "UP" | "ARROWUP" => KeyCode::ArrowUp,
      "LEFT" | "ARROWLEFT" => KeyCode::ArrowLeft,
      "RIGHT" | "ARROWRIGHT" => KeyCode::ArrowRight,

      "NUMLOCK" => KeyCode::NumLock,
      "NUMADD" | "NUMPADADD" => KeyCode::NumpadAdd,
      "NUMBACKSPACE" | "NUMPADBACKSPACE" => KeyCode::NumpadBackspace,
      "NUMCLEAR" | "NUMPADCLEAR" => KeyCode::NumpadClear,
      "NUMCOMMA" | "NUMPADCOMMA" => KeyCode::NumpadComma,
      "NUMDIVIDE" | "NUMPADDIVIDE" => KeyCode::NumpadDivide,
      "NUMSUBSTRACT" | "NUMPADSUBSTRACT" => KeyCode::NumpadSubtract,
      "NUMENTER" | "NUMPADENTER" => KeyCode::NumpadEnter,

      "ESC" | "ESCAPE" => KeyCode::Escape,
      "FN" => KeyCode::Fn,
      "FNLOCK" => KeyCode::FnLock,
      "PRINTSCREEN" => KeyCode::PrintScreen,
      "SCROLLLOCK" => KeyCode::ScrollLock,

      "PAUSE" => KeyCode::Pause,

      "VOLUMEMUTE" => KeyCode::AudioVolumeMute,
      "VOLUMEDOWN" => KeyCode::AudioVolumeDown,
      "VOLUMEUP" => KeyCode::AudioVolumeUp,
      "MEDIANEXTTRACK" => KeyCode::MediaTrackNext,
      "MEDIAPREVIOUSTRACK" => KeyCode::MediaTrackPrevious,
      "MEDIAPLAYPAUSE" => KeyCode::MediaPlayPause,
      "LAUNCHMAIL" => KeyCode::LaunchMail,

      "SUSPEND" => KeyCode::Suspend,
      "F1" => KeyCode::F1,
      "F2" => KeyCode::F2,
      "F3" => KeyCode::F3,
      "F4" => KeyCode::F4,
      "F5" => KeyCode::F5,
      "F6" => KeyCode::F6,
      "F7" => KeyCode::F7,
      "F8" => KeyCode::F8,
      "F9" => KeyCode::F9,
      "F10" => KeyCode::F10,
      "F11" => KeyCode::F11,
      "F12" => KeyCode::F12,
      "F13" => KeyCode::F13,
      "F14" => KeyCode::F14,
      "F15" => KeyCode::F15,
      "F16" => KeyCode::F16,
      "F17" => KeyCode::F17,
      "F18" => KeyCode::F18,
      "F19" => KeyCode::F19,
      "F20" => KeyCode::F20,
      "F21" => KeyCode::F21,
      "F22" => KeyCode::F22,
      "F23" => KeyCode::F23,
      "F24" => KeyCode::F24,
      "F25" => KeyCode::F25,
      "F26" => KeyCode::F26,
      "F27" => KeyCode::F27,
      "F28" => KeyCode::F28,
      "F29" => KeyCode::F29,
      "F30" => KeyCode::F30,
      "F31" => KeyCode::F31,
      "F32" => KeyCode::F32,
      "F33" => KeyCode::F33,
      "F34" => KeyCode::F34,
      "F35" => KeyCode::F35,
      _ => KeyCode::Unidentified(NativeKeyCode::Unidentified),
    };

    Ok(keycode)
  }
}

impl fmt::Display for KeyCode {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      &KeyCode::Unidentified(_) => write!(f, "{:?}", "Unidentified"),
      val => write!(f, "{val:?}"),
    }
  }
}

/// Key represents the meaning of a keypress.
///
/// This mostly conforms to the UI Events Specification's [`KeyboardEvent.key`] with a few
/// exceptions:
/// - The `Super` variant here, is named `Meta` in the aforementioned specification. (There's
///   another key which the specification calls `Super`. That does not exist here.)
/// - The `Space` variant here, can be identified by the character it generates in the
///   specificaiton.
/// - The `Unidentified` variant here, can still identifiy a key through it's `NativeKeyCode`.
/// - The `Dead` variant here, can specify the character which is inserted when pressing the
///   dead-key twice.
///
/// [`KeyboardEvent.key`]: https://w3c.github.io/uievents-key/
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Key<'a> {
  /// A key string that corresponds to the character typed by the user, taking into account the
  /// user’s current locale setting, and any system-level keyboard mapping overrides that are in
  /// effect.
  Character(&'a str),

  /// This variant is used when the key cannot be translated to any other variant.
  ///
  /// The native scancode is provided (if available) in order to allow the user to specify
  /// keybindings for keys which are not defined by this API.
  Unidentified(NativeKeyCode),

  /// Contains the text representation of the dead-key when available.
  ///
  /// ## Platform-specific
  /// - **Web:** Always contains `None`
  Dead(Option<char>),

  /// The `Alt` (Alternative) key.
  ///
  /// This key enables the alternate modifier function for interpreting concurrent or subsequent
  /// keyboard input. This key value is also used for the Apple <kbd>Option</kbd> key.
  Alt,
  /// The Alternate Graphics (<kbd>AltGr</kbd> or <kbd>AltGraph</kbd>) key.
  ///
  /// This key is used enable the ISO Level 3 shift modifier (the standard `Shift` key is the
  /// level 2 modifier).
  AltGraph,
  /// The `Caps Lock` (Capital) key.
  ///
  /// Toggle capital character lock function for interpreting subsequent keyboard input event.
  CapsLock,
  /// The `Control` or `Ctrl` key.
  ///
  /// Used to enable control modifier function for interpreting concurrent or subsequent keyboard
  /// input.
  Control,
  /// The Function switch `Fn` key. Activating this key simultaneously with another key changes
  /// that key’s value to an alternate character or function. This key is often handled directly
  /// in the keyboard hardware and does not usually generate key events.
  Fn,
  /// The Function-Lock (`FnLock` or `F-Lock`) key. Activating this key switches the mode of the
  /// keyboard to changes some keys' values to an alternate character or function. This key is
  /// often handled directly in the keyboard hardware and does not usually generate key events.
  FnLock,
  /// The `NumLock` or Number Lock key. Used to toggle numpad mode function for interpreting
  /// subsequent keyboard input.
  NumLock,
  /// Toggle between scrolling and cursor movement modes.
  ScrollLock,
  /// Used to enable shift modifier function for interpreting concurrent or subsequent keyboard
  /// input.
  Shift,
  /// The Symbol modifier key (used on some virtual keyboards).
  Symbol,
  SymbolLock,
  Hyper,
  /// Used to enable "super" modifier function for interpreting concurrent or subsequent keyboard
  /// input. This key value is used for the "Windows Logo" key and the Apple `Command` or `⌘` key.
  ///
  /// Note: In some contexts (e.g. the Web) this is referred to as the "Meta" key.
  Super,
  /// The `Enter` or `↵` key. Used to activate current selection or accept current input. This key
  /// value is also used for the `Return` (Macintosh numpad) key. This key value is also used for
  /// the Android `KEYCODE_DPAD_CENTER`.
  Enter,
  /// The Horizontal Tabulation `Tab` key.
  Tab,
  /// Used in text to insert a space between words. Usually located below the character keys.
  Space,
  /// Navigate or traverse downward. (`KEYCODE_DPAD_DOWN`)
  ArrowDown,
  /// Navigate or traverse leftward. (`KEYCODE_DPAD_LEFT`)
  ArrowLeft,
  /// Navigate or traverse rightward. (`KEYCODE_DPAD_RIGHT`)
  ArrowRight,
  /// Navigate or traverse upward. (`KEYCODE_DPAD_UP`)
  ArrowUp,
  /// The End key, used with keyboard entry to go to the end of content (`KEYCODE_MOVE_END`).
  End,
  /// The Home key, used with keyboard entry, to go to start of content (`KEYCODE_MOVE_HOME`).
  /// For the mobile phone `Home` key (which goes to the phone’s main screen), use [`GoHome`].
  ///
  /// [`GoHome`]: Self::GoHome
  Home,
  /// Scroll down or display next page of content.
  PageDown,
  /// Scroll up or display previous page of content.
  PageUp,
  /// Used to remove the character to the left of the cursor. This key value is also used for
  /// the key labeled `Delete` on MacOS keyboards.
  Backspace,
  /// Remove the currently selected input.
  Clear,
  /// Copy the current selection. (`APPCOMMAND_COPY`)
  Copy,
  /// The Cursor Select key.
  CrSel,
  /// Cut the current selection. (`APPCOMMAND_CUT`)
  Cut,
  /// Used to delete the character to the right of the cursor. This key value is also used for the
  /// key labeled `Delete` on MacOS keyboards when `Fn` is active.
  Delete,
  /// The Erase to End of Field key. This key deletes all characters from the current cursor
  /// position to the end of the current field.
  EraseEof,
  /// The Extend Selection (Exsel) key.
  ExSel,
  /// Toggle between text modes for insertion or overtyping.
  /// (`KEYCODE_INSERT`)
  Insert,
  /// The Paste key. (`APPCOMMAND_PASTE`)
  Paste,
  /// Redo the last action. (`APPCOMMAND_REDO`)
  Redo,
  /// Undo the last action. (`APPCOMMAND_UNDO`)
  Undo,
  /// The Accept (Commit, OK) key. Accept current option or input method sequence conversion.
  Accept,
  /// Redo or repeat an action.
  Again,
  /// The Attention (Attn) key.
  Attn,
  Cancel,
  /// Show the application’s context menu.
  /// This key is commonly found between the right `Super` key and the right `Control` key.
  ContextMenu,
  /// The `Esc` key. This key was originally used to initiate an escape sequence, but is
  /// now more generally used to exit or "escape" the current context, such as closing a dialog
  /// or exiting full screen mode.
  Escape,
  Execute,
  /// Open the Find dialog. (`APPCOMMAND_FIND`)
  Find,
  /// Open a help dialog or toggle display of help information. (`APPCOMMAND_HELP`,
  /// `KEYCODE_HELP`)
  Help,
  /// Pause the current state or application (as appropriate).
  ///
  /// Note: Do not use this value for the `Pause` button on media controllers. Use `"MediaPause"`
  /// instead.
  Pause,
  /// Play or resume the current state or application (as appropriate).
  ///
  /// Note: Do not use this value for the `Play` button on media controllers. Use `"MediaPlay"`
  /// instead.
  Play,
  /// The properties (Props) key.
  Props,
  Select,
  /// The ZoomIn key. (`KEYCODE_ZOOM_IN`)
  ZoomIn,
  /// The ZoomOut key. (`KEYCODE_ZOOM_OUT`)
  ZoomOut,
  /// The Brightness Down key. Typically controls the display brightness.
  /// (`KEYCODE_BRIGHTNESS_DOWN`)
  BrightnessDown,
  /// The Brightness Up key. Typically controls the display brightness. (`KEYCODE_BRIGHTNESS_UP`)
  BrightnessUp,
  /// Toggle removable media to eject (open) and insert (close) state. (`KEYCODE_MEDIA_EJECT`)
  Eject,
  LogOff,
  /// Toggle power state. (`KEYCODE_POWER`)
  /// Note: Note: Some devices might not expose this key to the operating environment.
  Power,
  /// The `PowerOff` key. Sometime called `PowerDown`.
  PowerOff,
  /// Initiate print-screen function.
  PrintScreen,
  /// The Hibernate key. This key saves the current state of the computer to disk so that it can
  /// be restored. The computer will then shutdown.
  Hibernate,
  /// The Standby key. This key turns off the display and places the computer into a low-power
  /// mode without completely shutting down. It is sometimes labelled `Suspend` or `Sleep` key.
  /// (`KEYCODE_SLEEP`)
  Standby,
  /// The WakeUp key. (`KEYCODE_WAKEUP`)
  WakeUp,
  /// Initate the multi-candidate mode.
  AllCandidates,
  Alphanumeric,
  /// Initiate the Code Input mode to allow characters to be entered by
  /// their code points.
  CodeInput,
  /// The Compose key, also known as "Multi_key" on the X Window System. This key acts in a
  /// manner similar to a dead key, triggering a mode where subsequent key presses are combined to
  /// produce a different character.
  Compose,
  /// Convert the current input method sequence.
  Convert,
  /// The Final Mode `Final` key used on some Asian keyboards, to enable the final mode for IMEs.
  FinalMode,
  /// Switch to the first character group. (ISO/IEC 9995)
  GroupFirst,
  /// Switch to the last character group. (ISO/IEC 9995)
  GroupLast,
  /// Switch to the next character group. (ISO/IEC 9995)
  GroupNext,
  /// Switch to the previous character group. (ISO/IEC 9995)
  GroupPrevious,
  /// Toggle between or cycle through input modes of IMEs.
  ModeChange,
  NextCandidate,
  /// Accept current input method sequence without
  /// conversion in IMEs.
  NonConvert,
  PreviousCandidate,
  Process,
  SingleCandidate,
  /// Toggle between Hangul and English modes.
  HangulMode,
  HanjaMode,
  JunjaMode,
  /// The Eisu key. This key may close the IME, but its purpose is defined by the current IME.
  /// (`KEYCODE_EISU`)
  Eisu,
  /// The (Half-Width) Characters key.
  Hankaku,
  /// The Hiragana (Japanese Kana characters) key.
  Hiragana,
  /// The Hiragana/Katakana toggle key. (`KEYCODE_KATAKANA_HIRAGANA`)
  HiraganaKatakana,
  /// The Kana Mode (Kana Lock) key. This key is used to enter hiragana mode (typically from
  /// romaji mode).
  KanaMode,
  /// The Kanji (Japanese name for ideographic characters of Chinese origin) Mode key. This key is
  /// typically used to switch to a hiragana keyboard for the purpose of converting input into
  /// kanji. (`KEYCODE_KANA`)
  KanjiMode,
  /// The Katakana (Japanese Kana characters) key.
  Katakana,
  /// The Roman characters function key.
  Romaji,
  /// The Zenkaku (Full-Width) Characters key.
  Zenkaku,
  /// The Zenkaku/Hankaku (full-width/half-width) toggle key. (`KEYCODE_ZENKAKU_HANKAKU`)
  ZenkakuHankaku,
  /// General purpose virtual function key, as index 1.
  Soft1,
  /// General purpose virtual function key, as index 2.
  Soft2,
  /// General purpose virtual function key, as index 3.
  Soft3,
  /// General purpose virtual function key, as index 4.
  Soft4,
  /// Select next (numerically or logically) lower channel. (`APPCOMMAND_MEDIA_CHANNEL_DOWN`,
  /// `KEYCODE_CHANNEL_DOWN`)
  ChannelDown,
  /// Select next (numerically or logically) higher channel. (`APPCOMMAND_MEDIA_CHANNEL_UP`,
  /// `KEYCODE_CHANNEL_UP`)
  ChannelUp,
  /// Close the current document or message (Note: This doesn’t close the application).
  /// (`APPCOMMAND_CLOSE`)
  Close,
  /// Open an editor to forward the current message. (`APPCOMMAND_FORWARD_MAIL`)
  MailForward,
  /// Open an editor to reply to the current message. (`APPCOMMAND_REPLY_TO_MAIL`)
  MailReply,
  /// Send the current message. (`APPCOMMAND_SEND_MAIL`)
  MailSend,
  /// Close the current media, for example to close a CD or DVD tray. (`KEYCODE_MEDIA_CLOSE`)
  MediaClose,
  /// Initiate or continue forward playback at faster than normal speed, or increase speed if
  /// already fast forwarding. (`APPCOMMAND_MEDIA_FAST_FORWARD`, `KEYCODE_MEDIA_FAST_FORWARD`)
  MediaFastForward,
  /// Pause the currently playing media. (`APPCOMMAND_MEDIA_PAUSE`, `KEYCODE_MEDIA_PAUSE`)
  ///
  /// Note: Media controller devices should use this value rather than `"Pause"` for their pause
  /// keys.
  MediaPause,
  /// Initiate or continue media playback at normal speed, if not currently playing at normal
  /// speed. (`APPCOMMAND_MEDIA_PLAY`, `KEYCODE_MEDIA_PLAY`)
  MediaPlay,
  /// Toggle media between play and pause states. (`APPCOMMAND_MEDIA_PLAY_PAUSE`,
  /// `KEYCODE_MEDIA_PLAY_PAUSE`)
  MediaPlayPause,
  /// Initiate or resume recording of currently selected media. (`APPCOMMAND_MEDIA_RECORD`,
  /// `KEYCODE_MEDIA_RECORD`)
  MediaRecord,
  /// Initiate or continue reverse playback at faster than normal speed, or increase speed if
  /// already rewinding. (`APPCOMMAND_MEDIA_REWIND`, `KEYCODE_MEDIA_REWIND`)
  MediaRewind,
  /// Stop media playing, pausing, forwarding, rewinding, or recording, if not already stopped.
  /// (`APPCOMMAND_MEDIA_STOP`, `KEYCODE_MEDIA_STOP`)
  MediaStop,
  /// Seek to next media or program track. (`APPCOMMAND_MEDIA_NEXTTRACK`, `KEYCODE_MEDIA_NEXT`)
  MediaTrackNext,
  /// Seek to previous media or program track. (`APPCOMMAND_MEDIA_PREVIOUSTRACK`,
  /// `KEYCODE_MEDIA_PREVIOUS`)
  MediaTrackPrevious,
  /// Open a new document or message. (`APPCOMMAND_NEW`)
  New,
  /// Open an existing document or message. (`APPCOMMAND_OPEN`)
  Open,
  /// Print the current document or message. (`APPCOMMAND_PRINT`)
  Print,
  /// Save the current document or message. (`APPCOMMAND_SAVE`)
  Save,
  /// Spellcheck the current document or selection. (`APPCOMMAND_SPELL_CHECK`)
  SpellCheck,
  /// The `11` key found on media numpads that
  /// have buttons from `1` ... `12`.
  Key11,
  /// The `12` key found on media numpads that
  /// have buttons from `1` ... `12`.
  Key12,
  /// Adjust audio balance leftward. (`VK_AUDIO_BALANCE_LEFT`)
  AudioBalanceLeft,
  /// Adjust audio balance rightward. (`VK_AUDIO_BALANCE_RIGHT`)
  AudioBalanceRight,
  /// Decrease audio bass boost or cycle down through bass boost states. (`APPCOMMAND_BASS_DOWN`,
  /// `VK_BASS_BOOST_DOWN`)
  AudioBassBoostDown,
  /// Toggle bass boost on/off. (`APPCOMMAND_BASS_BOOST`)
  AudioBassBoostToggle,
  /// Increase audio bass boost or cycle up through bass boost states. (`APPCOMMAND_BASS_UP`,
  /// `VK_BASS_BOOST_UP`)
  AudioBassBoostUp,
  /// Adjust audio fader towards front. (`VK_FADER_FRONT`)
  AudioFaderFront,
  /// Adjust audio fader towards rear. (`VK_FADER_REAR`)
  AudioFaderRear,
  /// Advance surround audio mode to next available mode. (`VK_SURROUND_MODE_NEXT`)
  AudioSurroundModeNext,
  /// Decrease treble. (`APPCOMMAND_TREBLE_DOWN`)
  AudioTrebleDown,
  /// Increase treble. (`APPCOMMAND_TREBLE_UP`)
  AudioTrebleUp,
  /// Decrease audio volume. (`APPCOMMAND_VOLUME_DOWN`, `KEYCODE_VOLUME_DOWN`)
  AudioVolumeDown,
  /// Increase audio volume. (`APPCOMMAND_VOLUME_UP`, `KEYCODE_VOLUME_UP`)
  AudioVolumeUp,
  /// Toggle between muted state and prior volume level. (`APPCOMMAND_VOLUME_MUTE`,
  /// `KEYCODE_VOLUME_MUTE`)
  AudioVolumeMute,
  /// Toggle the microphone on/off. (`APPCOMMAND_MIC_ON_OFF_TOGGLE`)
  MicrophoneToggle,
  /// Decrease microphone volume. (`APPCOMMAND_MICROPHONE_VOLUME_DOWN`)
  MicrophoneVolumeDown,
  /// Increase microphone volume. (`APPCOMMAND_MICROPHONE_VOLUME_UP`)
  MicrophoneVolumeUp,
  /// Mute the microphone. (`APPCOMMAND_MICROPHONE_VOLUME_MUTE`, `KEYCODE_MUTE`)
  MicrophoneVolumeMute,
  /// Show correction list when a word is incorrectly identified. (`APPCOMMAND_CORRECTION_LIST`)
  SpeechCorrectionList,
  /// Toggle between dictation mode and command/control mode.
  /// (`APPCOMMAND_DICTATE_OR_COMMAND_CONTROL_TOGGLE`)
  SpeechInputToggle,
  /// The first generic "LaunchApplication" key. This is commonly associated with launching "My
  /// Computer", and may have a computer symbol on the key. (`APPCOMMAND_LAUNCH_APP1`)
  LaunchApplication1,
  /// The second generic "LaunchApplication" key. This is commonly associated with launching
  /// "Calculator", and may have a calculator symbol on the key. (`APPCOMMAND_LAUNCH_APP2`,
  /// `KEYCODE_CALCULATOR`)
  LaunchApplication2,
  /// The "Calendar" key. (`KEYCODE_CALENDAR`)
  LaunchCalendar,
  /// The "Contacts" key. (`KEYCODE_CONTACTS`)
  LaunchContacts,
  /// The "Mail" key. (`APPCOMMAND_LAUNCH_MAIL`)
  LaunchMail,
  /// The "Media Player" key. (`APPCOMMAND_LAUNCH_MEDIA_SELECT`)
  LaunchMediaPlayer,
  LaunchMusicPlayer,
  LaunchPhone,
  LaunchScreenSaver,
  LaunchSpreadsheet,
  LaunchWebBrowser,
  LaunchWebCam,
  LaunchWordProcessor,
  /// Navigate to previous content or page in current history. (`APPCOMMAND_BROWSER_BACKWARD`)
  BrowserBack,
  /// Open the list of browser favorites. (`APPCOMMAND_BROWSER_FAVORITES`)
  BrowserFavorites,
  /// Navigate to next content or page in current history. (`APPCOMMAND_BROWSER_FORWARD`)
  BrowserForward,
  /// Go to the user’s preferred home page. (`APPCOMMAND_BROWSER_HOME`)
  BrowserHome,
  /// Refresh the current page or content. (`APPCOMMAND_BROWSER_REFRESH`)
  BrowserRefresh,
  /// Call up the user’s preferred search page. (`APPCOMMAND_BROWSER_SEARCH`)
  BrowserSearch,
  /// Stop loading the current page or content. (`APPCOMMAND_BROWSER_STOP`)
  BrowserStop,
  /// The Application switch key, which provides a list of recent apps to switch between.
  /// (`KEYCODE_APP_SWITCH`)
  AppSwitch,
  /// The Call key. (`KEYCODE_CALL`)
  Call,
  /// The Camera key. (`KEYCODE_CAMERA`)
  Camera,
  /// The Camera focus key. (`KEYCODE_FOCUS`)
  CameraFocus,
  /// The End Call key. (`KEYCODE_ENDCALL`)
  EndCall,
  /// The Back key. (`KEYCODE_BACK`)
  GoBack,
  /// The Home key, which goes to the phone’s main screen. (`KEYCODE_HOME`)
  GoHome,
  /// The Headset Hook key. (`KEYCODE_HEADSETHOOK`)
  HeadsetHook,
  LastNumberRedial,
  /// The Notification key. (`KEYCODE_NOTIFICATION`)
  Notification,
  /// Toggle between manner mode state: silent, vibrate, ring, ... (`KEYCODE_MANNER_MODE`)
  MannerMode,
  VoiceDial,
  /// Switch to viewing TV. (`KEYCODE_TV`)
  TV,
  /// TV 3D Mode. (`KEYCODE_3D_MODE`)
  TV3DMode,
  /// Toggle between antenna and cable input. (`KEYCODE_TV_ANTENNA_CABLE`)
  TVAntennaCable,
  /// Audio description. (`KEYCODE_TV_AUDIO_DESCRIPTION`)
  TVAudioDescription,
  /// Audio description mixing volume down. (`KEYCODE_TV_AUDIO_DESCRIPTION_MIX_DOWN`)
  TVAudioDescriptionMixDown,
  /// Audio description mixing volume up. (`KEYCODE_TV_AUDIO_DESCRIPTION_MIX_UP`)
  TVAudioDescriptionMixUp,
  /// Contents menu. (`KEYCODE_TV_CONTENTS_MENU`)
  TVContentsMenu,
  /// Contents menu. (`KEYCODE_TV_DATA_SERVICE`)
  TVDataService,
  /// Switch the input mode on an external TV. (`KEYCODE_TV_INPUT`)
  TVInput,
  /// Switch to component input #1. (`KEYCODE_TV_INPUT_COMPONENT_1`)
  TVInputComponent1,
  /// Switch to component input #2. (`KEYCODE_TV_INPUT_COMPONENT_2`)
  TVInputComponent2,
  /// Switch to composite input #1. (`KEYCODE_TV_INPUT_COMPOSITE_1`)
  TVInputComposite1,
  /// Switch to composite input #2. (`KEYCODE_TV_INPUT_COMPOSITE_2`)
  TVInputComposite2,
  /// Switch to HDMI input #1. (`KEYCODE_TV_INPUT_HDMI_1`)
  TVInputHDMI1,
  /// Switch to HDMI input #2. (`KEYCODE_TV_INPUT_HDMI_2`)
  TVInputHDMI2,
  /// Switch to HDMI input #3. (`KEYCODE_TV_INPUT_HDMI_3`)
  TVInputHDMI3,
  /// Switch to HDMI input #4. (`KEYCODE_TV_INPUT_HDMI_4`)
  TVInputHDMI4,
  /// Switch to VGA input #1. (`KEYCODE_TV_INPUT_VGA_1`)
  TVInputVGA1,
  /// Media context menu. (`KEYCODE_TV_MEDIA_CONTEXT_MENU`)
  TVMediaContext,
  /// Toggle network. (`KEYCODE_TV_NETWORK`)
  TVNetwork,
  /// Number entry. (`KEYCODE_TV_NUMBER_ENTRY`)
  TVNumberEntry,
  /// Toggle the power on an external TV. (`KEYCODE_TV_POWER`)
  TVPower,
  /// Radio. (`KEYCODE_TV_RADIO_SERVICE`)
  TVRadioService,
  /// Satellite. (`KEYCODE_TV_SATELLITE`)
  TVSatellite,
  /// Broadcast Satellite. (`KEYCODE_TV_SATELLITE_BS`)
  TVSatelliteBS,
  /// Communication Satellite. (`KEYCODE_TV_SATELLITE_CS`)
  TVSatelliteCS,
  /// Toggle between available satellites. (`KEYCODE_TV_SATELLITE_SERVICE`)
  TVSatelliteToggle,
  /// Analog Terrestrial. (`KEYCODE_TV_TERRESTRIAL_ANALOG`)
  TVTerrestrialAnalog,
  /// Digital Terrestrial. (`KEYCODE_TV_TERRESTRIAL_DIGITAL`)
  TVTerrestrialDigital,
  /// Timer programming. (`KEYCODE_TV_TIMER_PROGRAMMING`)
  TVTimer,
  /// Switch the input mode on an external AVR (audio/video receiver). (`KEYCODE_AVR_INPUT`)
  AVRInput,
  /// Toggle the power on an external AVR (audio/video receiver). (`KEYCODE_AVR_POWER`)
  AVRPower,
  /// General purpose color-coded media function key, as index 0 (red). (`VK_COLORED_KEY_0`,
  /// `KEYCODE_PROG_RED`)
  ColorF0Red,
  /// General purpose color-coded media function key, as index 1 (green). (`VK_COLORED_KEY_1`,
  /// `KEYCODE_PROG_GREEN`)
  ColorF1Green,
  /// General purpose color-coded media function key, as index 2 (yellow). (`VK_COLORED_KEY_2`,
  /// `KEYCODE_PROG_YELLOW`)
  ColorF2Yellow,
  /// General purpose color-coded media function key, as index 3 (blue). (`VK_COLORED_KEY_3`,
  /// `KEYCODE_PROG_BLUE`)
  ColorF3Blue,
  /// General purpose color-coded media function key, as index 4 (grey). (`VK_COLORED_KEY_4`)
  ColorF4Grey,
  /// General purpose color-coded media function key, as index 5 (brown). (`VK_COLORED_KEY_5`)
  ColorF5Brown,
  /// Toggle the display of Closed Captions. (`VK_CC`, `KEYCODE_CAPTIONS`)
  ClosedCaptionToggle,
  /// Adjust brightness of device, by toggling between or cycling through states. (`VK_DIMMER`)
  Dimmer,
  /// Swap video sources. (`VK_DISPLAY_SWAP`)
  DisplaySwap,
  /// Select Digital Video Rrecorder. (`KEYCODE_DVR`)
  DVR,
  /// Exit the current application. (`VK_EXIT`)
  Exit,
  /// Clear program or content stored as favorite 0. (`VK_CLEAR_FAVORITE_0`)
  FavoriteClear0,
  /// Clear program or content stored as favorite 1. (`VK_CLEAR_FAVORITE_1`)
  FavoriteClear1,
  /// Clear program or content stored as favorite 2. (`VK_CLEAR_FAVORITE_2`)
  FavoriteClear2,
  /// Clear program or content stored as favorite 3. (`VK_CLEAR_FAVORITE_3`)
  FavoriteClear3,
  /// Select (recall) program or content stored as favorite 0. (`VK_RECALL_FAVORITE_0`)
  FavoriteRecall0,
  /// Select (recall) program or content stored as favorite 1. (`VK_RECALL_FAVORITE_1`)
  FavoriteRecall1,
  /// Select (recall) program or content stored as favorite 2. (`VK_RECALL_FAVORITE_2`)
  FavoriteRecall2,
  /// Select (recall) program or content stored as favorite 3. (`VK_RECALL_FAVORITE_3`)
  FavoriteRecall3,
  /// Store current program or content as favorite 0. (`VK_STORE_FAVORITE_0`)
  FavoriteStore0,
  /// Store current program or content as favorite 1. (`VK_STORE_FAVORITE_1`)
  FavoriteStore1,
  /// Store current program or content as favorite 2. (`VK_STORE_FAVORITE_2`)
  FavoriteStore2,
  /// Store current program or content as favorite 3. (`VK_STORE_FAVORITE_3`)
  FavoriteStore3,
  /// Toggle display of program or content guide. (`VK_GUIDE`, `KEYCODE_GUIDE`)
  Guide,
  /// If guide is active and displayed, then display next day’s content. (`VK_NEXT_DAY`)
  GuideNextDay,
  /// If guide is active and displayed, then display previous day’s content. (`VK_PREV_DAY`)
  GuidePreviousDay,
  /// Toggle display of information about currently selected context or media. (`VK_INFO`,
  /// `KEYCODE_INFO`)
  Info,
  /// Toggle instant replay. (`VK_INSTANT_REPLAY`)
  InstantReplay,
  /// Launch linked content, if available and appropriate. (`VK_LINK`)
  Link,
  /// List the current program. (`VK_LIST`)
  ListProgram,
  /// Toggle display listing of currently available live content or programs. (`VK_LIVE`)
  LiveContent,
  /// Lock or unlock current content or program. (`VK_LOCK`)
  Lock,
  /// Show a list of media applications: audio/video players and image viewers. (`VK_APPS`)
  ///
  /// Note: Do not confuse this key value with the Windows' `VK_APPS` / `VK_CONTEXT_MENU` key,
  /// which is encoded as `"ContextMenu"`.
  MediaApps,
  /// Audio track key. (`KEYCODE_MEDIA_AUDIO_TRACK`)
  MediaAudioTrack,
  /// Select previously selected channel or media. (`VK_LAST`, `KEYCODE_LAST_CHANNEL`)
  MediaLast,
  /// Skip backward to next content or program. (`KEYCODE_MEDIA_SKIP_BACKWARD`)
  MediaSkipBackward,
  /// Skip forward to next content or program. (`VK_SKIP`, `KEYCODE_MEDIA_SKIP_FORWARD`)
  MediaSkipForward,
  /// Step backward to next content or program. (`KEYCODE_MEDIA_STEP_BACKWARD`)
  MediaStepBackward,
  /// Step forward to next content or program. (`KEYCODE_MEDIA_STEP_FORWARD`)
  MediaStepForward,
  /// Media top menu. (`KEYCODE_MEDIA_TOP_MENU`)
  MediaTopMenu,
  /// Navigate in. (`KEYCODE_NAVIGATE_IN`)
  NavigateIn,
  /// Navigate to next key. (`KEYCODE_NAVIGATE_NEXT`)
  NavigateNext,
  /// Navigate out. (`KEYCODE_NAVIGATE_OUT`)
  NavigateOut,
  /// Navigate to previous key. (`KEYCODE_NAVIGATE_PREVIOUS`)
  NavigatePrevious,
  /// Cycle to next favorite channel (in favorites list). (`VK_NEXT_FAVORITE_CHANNEL`)
  NextFavoriteChannel,
  /// Cycle to next user profile (if there are multiple user profiles). (`VK_USER`)
  NextUserProfile,
  /// Access on-demand content or programs. (`VK_ON_DEMAND`)
  OnDemand,
  /// Pairing key to pair devices. (`KEYCODE_PAIRING`)
  Pairing,
  /// Move picture-in-picture window down. (`VK_PINP_DOWN`)
  PinPDown,
  /// Move picture-in-picture window. (`VK_PINP_MOVE`)
  PinPMove,
  /// Toggle display of picture-in-picture window. (`VK_PINP_TOGGLE`)
  PinPToggle,
  /// Move picture-in-picture window up. (`VK_PINP_UP`)
  PinPUp,
  /// Decrease media playback speed. (`VK_PLAY_SPEED_DOWN`)
  PlaySpeedDown,
  /// Reset playback to normal speed. (`VK_PLAY_SPEED_RESET`)
  PlaySpeedReset,
  /// Increase media playback speed. (`VK_PLAY_SPEED_UP`)
  PlaySpeedUp,
  /// Toggle random media or content shuffle mode. (`VK_RANDOM_TOGGLE`)
  RandomToggle,
  /// Not a physical key, but this key code is sent when the remote control battery is low.
  /// (`VK_RC_LOW_BATTERY`)
  RcLowBattery,
  /// Toggle or cycle between media recording speeds. (`VK_RECORD_SPEED_NEXT`)
  RecordSpeedNext,
  /// Toggle RF (radio frequency) input bypass mode (pass RF input directly to the RF output).
  /// (`VK_RF_BYPASS`)
  RfBypass,
  /// Toggle scan channels mode. (`VK_SCAN_CHANNELS_TOGGLE`)
  ScanChannelsToggle,
  /// Advance display screen mode to next available mode. (`VK_SCREEN_MODE_NEXT`)
  ScreenModeNext,
  /// Toggle display of device settings screen. (`VK_SETTINGS`, `KEYCODE_SETTINGS`)
  Settings,
  /// Toggle split screen mode. (`VK_SPLIT_SCREEN_TOGGLE`)
  SplitScreenToggle,
  /// Switch the input mode on an external STB (set top box). (`KEYCODE_STB_INPUT`)
  STBInput,
  /// Toggle the power on an external STB (set top box). (`KEYCODE_STB_POWER`)
  STBPower,
  /// Toggle display of subtitles, if available. (`VK_SUBTITLE`)
  Subtitle,
  /// Toggle display of teletext, if available (`VK_TELETEXT`, `KEYCODE_TV_TELETEXT`).
  Teletext,
  /// Advance video mode to next available mode. (`VK_VIDEO_MODE_NEXT`)
  VideoModeNext,
  /// Cause device to identify itself in some manner, e.g., audibly or visibly. (`VK_WINK`)
  Wink,
  /// Toggle between full-screen and scaled content, or alter magnification level. (`VK_ZOOM`,
  /// `KEYCODE_TV_ZOOM_MODE`)
  ZoomToggle,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F1,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F2,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F3,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F4,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F5,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F6,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F7,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F8,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F9,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F10,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F11,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F12,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F13,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F14,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F15,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F16,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F17,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F18,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F19,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F20,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F21,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F22,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F23,
  /// General-purpose function key.
  /// Usually found at the top of the keyboard.
  F24,
  /// General-purpose function key.
  F25,
  /// General-purpose function key.
  F26,
  /// General-purpose function key.
  F27,
  /// General-purpose function key.
  F28,
  /// General-purpose function key.
  F29,
  /// General-purpose function key.
  F30,
  /// General-purpose function key.
  F31,
  /// General-purpose function key.
  F32,
  /// General-purpose function key.
  F33,
  /// General-purpose function key.
  F34,
  /// General-purpose function key.
  F35,
}

impl<'a> Key<'a> {
  pub fn to_text(&self) -> Option<&'a str> {
    match self {
      Key::Character(ch) => Some(*ch),
      Key::Enter => Some("\r"),
      Key::Backspace => Some("\x08"),
      Key::Tab => Some("\t"),
      Key::Space => Some(" "),
      Key::Escape => Some("\x1b"),
      _ => None,
    }
  }
}

impl<'a> From<&'a str> for Key<'a> {
  fn from(src: &'a str) -> Key<'a> {
    Key::Character(src)
  }
}

/// Represents the location of a physical key.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum KeyLocation {
  Standard,
  Left,
  Right,
  Numpad,
}
