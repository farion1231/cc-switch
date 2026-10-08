// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

// TODO: Upstream these

#![allow(
  dead_code,
  non_snake_case,
  non_upper_case_globals,
  clippy::enum_variant_names
)]

use std::{ffi::c_void, ptr};

use core_foundation::{
  array::CFArrayRef, data::CFDataRef, dictionary::CFDictionaryRef, string::CFStringRef,
  uuid::CFUUIDRef,
};
use core_graphics::{
  base::CGError,
  display::{boolean_t, CGDirectDisplayID, CGDisplayConfigRef},
  geometry::{CGPoint, CGRect},
};
use objc2::{
  encode::{Encode, Encoding},
  runtime::{AnyObject, Bool},
};
use objc2_foundation::NSInteger;

#[allow(non_camel_case_types)]
pub type id = *mut AnyObject;
pub const nil: id = ptr::null_mut();

#[allow(non_camel_case_types)]
pub type BOOL = Bool;
#[allow(deprecated)]
pub const YES: Bool = Bool::YES;
#[allow(deprecated)]
pub const NO: Bool = Bool::NO;

pub const NSNotFound: NSInteger = NSInteger::MAX;

pub const kCGBaseWindowLevelKey: NSInteger = 0;
pub const kCGMinimumWindowLevelKey: NSInteger = 1;
pub const kCGDesktopWindowLevelKey: NSInteger = 2;
pub const kCGBackstopMenuLevelKey: NSInteger = 3;
pub const kCGNormalWindowLevelKey: NSInteger = 4;
pub const kCGFloatingWindowLevelKey: NSInteger = 5;
pub const kCGTornOffMenuWindowLevelKey: NSInteger = 6;
pub const kCGDockWindowLevelKey: NSInteger = 7;
pub const kCGMainMenuWindowLevelKey: NSInteger = 8;
pub const kCGStatusWindowLevelKey: NSInteger = 9;
pub const kCGModalPanelWindowLevelKey: NSInteger = 10;
pub const kCGPopUpMenuWindowLevelKey: NSInteger = 11;
pub const kCGDraggingWindowLevelKey: NSInteger = 12;
pub const kCGScreenSaverWindowLevelKey: NSInteger = 13;
pub const kCGMaximumWindowLevelKey: NSInteger = 14;
pub const kCGOverlayWindowLevelKey: NSInteger = 15;
pub const kCGHelpWindowLevelKey: NSInteger = 16;
pub const kCGUtilityWindowLevelKey: NSInteger = 17;
pub const kCGDesktopIconWindowLevelKey: NSInteger = 18;
pub const kCGCursorWindowLevelKey: NSInteger = 19;
pub const kCGNumberOfWindowLevelKeys: NSInteger = 20;

#[derive(Debug, Clone, Copy)]
#[repr(isize)]
pub enum NSWindowLevel {
  BelowNormalWindowLevel = (kCGBaseWindowLevelKey - 1) as _,
  NSNormalWindowLevel = kCGBaseWindowLevelKey as _,
  NSFloatingWindowLevel = kCGFloatingWindowLevelKey as _,
  NSTornOffMenuWindowLevel = kCGTornOffMenuWindowLevelKey as _,
  NSModalPanelWindowLevel = kCGModalPanelWindowLevelKey as _,
  NSMainMenuWindowLevel = kCGMainMenuWindowLevelKey as _,
  NSStatusWindowLevel = kCGStatusWindowLevelKey as _,
  NSPopUpMenuWindowLevel = kCGPopUpMenuWindowLevelKey as _,
  NSScreenSaverWindowLevel = kCGScreenSaverWindowLevelKey as _,
}

unsafe impl Encode for NSWindowLevel {
  const ENCODING: Encoding = isize::ENCODING;
}

pub type CGDisplayFadeInterval = f32;
pub type CGDisplayReservationInterval = f32;
pub type CGDisplayBlendFraction = f32;

pub const kCGDisplayBlendNormal: f32 = 0.0;
pub const kCGDisplayBlendSolidColor: f32 = 1.0;

pub type CGDisplayFadeReservationToken = u32;
pub const kCGDisplayFadeReservationInvalidToken: CGDisplayFadeReservationToken = 0;

pub type Boolean = u8;
pub const FALSE: Boolean = 0;
pub const TRUE: Boolean = 1;

pub const kCGErrorSuccess: i32 = 0;
pub const kCGErrorFailure: i32 = 1000;
pub const kCGErrorIllegalArgument: i32 = 1001;
pub const kCGErrorInvalidConnection: i32 = 1002;
pub const kCGErrorInvalidContext: i32 = 1003;
pub const kCGErrorCannotComplete: i32 = 1004;
pub const kCGErrorNotImplemented: i32 = 1006;
pub const kCGErrorRangeCheck: i32 = 1007;
pub const kCGErrorTypeCheck: i32 = 1008;
pub const kCGErrorInvalidOperation: i32 = 1010;
pub const kCGErrorNoneAvailable: i32 = 1011;

pub const IO1BitIndexedPixels: &str = "P";
pub const IO2BitIndexedPixels: &str = "PP";
pub const IO4BitIndexedPixels: &str = "PPPP";
pub const IO8BitIndexedPixels: &str = "PPPPPPPP";
pub const IO16BitDirectPixels: &str = "-RRRRRGGGGGBBBBB";
pub const IO32BitDirectPixels: &str = "--------RRRRRRRRGGGGGGGGBBBBBBBB";

pub const kIO30BitDirectPixels: &str = "--RRRRRRRRRRGGGGGGGGGGBBBBBBBBBB";
pub const kIO64BitDirectPixels: &str = "-16R16G16B16";

pub const kIO16BitFloatPixels: &str = "-16FR16FG16FB16";
pub const kIO32BitFloatPixels: &str = "-32FR32FG32FB32";

pub const IOYUV422Pixels: &str = "Y4U2V2";
pub const IO8BitOverlayPixels: &str = "O8";

pub type CGWindowLevel = i32;
pub type CGDisplayModeRef = *mut libc::c_void;

// `CGDisplayCreateUUIDFromDisplayID` comes from the `ColorSync` framework.
// However, that framework was only introduced "publicly" in macOS 10.13.
//
// Since we want to support older versions, we can't link to `ColorSync`
// directly. Fortunately, it has always been available as a subframework of
// `ApplicationServices`, see:
// https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/OSX_Technology_Overview/SystemFrameworks/SystemFrameworks.html#//apple_ref/doc/uid/TP40001067-CH210-BBCFFIEG
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
  pub fn CGDisplayCreateUUIDFromDisplayID(display: CGDirectDisplayID) -> CFUUIDRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
  pub fn CGRestorePermanentDisplayConfiguration();
  pub fn CGDisplayCapture(display: CGDirectDisplayID) -> CGError;
  pub fn CGDisplayRelease(display: CGDirectDisplayID) -> CGError;
  pub fn CGConfigureDisplayFadeEffect(
    config: CGDisplayConfigRef,
    fadeOutSeconds: CGDisplayFadeInterval,
    fadeInSeconds: CGDisplayFadeInterval,
    fadeRed: f32,
    fadeGreen: f32,
    fadeBlue: f32,
  ) -> CGError;
  pub fn CGAcquireDisplayFadeReservation(
    seconds: CGDisplayReservationInterval,
    token: *mut CGDisplayFadeReservationToken,
  ) -> CGError;
  pub fn CGDisplayFade(
    token: CGDisplayFadeReservationToken,
    duration: CGDisplayFadeInterval,
    startBlend: CGDisplayBlendFraction,
    endBlend: CGDisplayBlendFraction,
    redBlend: f32,
    greenBlend: f32,
    blueBlend: f32,
    synchronous: Boolean,
  ) -> CGError;
  pub fn CGRectContainsPoint(rect: CGRect, point: CGPoint) -> boolean_t;
  pub fn CGReleaseDisplayFadeReservation(token: CGDisplayFadeReservationToken) -> CGError;
  pub fn CGShieldingWindowLevel() -> CGWindowLevel;
  pub fn CGDisplaySetDisplayMode(
    display: CGDirectDisplayID,
    mode: CGDisplayModeRef,
    options: CFDictionaryRef,
  ) -> CGError;
  pub fn CGDisplayCopyAllDisplayModes(
    display: CGDirectDisplayID,
    options: CFDictionaryRef,
  ) -> CFArrayRef;
  pub fn CGDisplayModeGetPixelWidth(mode: CGDisplayModeRef) -> usize;
  pub fn CGDisplayModeGetPixelHeight(mode: CGDisplayModeRef) -> usize;
  pub fn CGDisplayModeGetRefreshRate(mode: CGDisplayModeRef) -> f64;
  pub fn CGDisplayModeCopyPixelEncoding(mode: CGDisplayModeRef) -> CFStringRef;
  pub fn CGDisplayModeRetain(mode: CGDisplayModeRef);
  pub fn CGDisplayModeRelease(mode: CGDisplayModeRef);
}

#[repr(transparent)]
pub struct TISInputSource(std::ffi::c_void);
pub type TISInputSourceRef = *mut TISInputSource;

#[repr(transparent)]
pub struct UCKeyboardLayout(std::ffi::c_void);

pub type OptionBits = u32;
pub type UniCharCount = std::os::raw::c_ulong;
pub type UniChar = u16;
pub type OSStatus = i32;

#[allow(non_upper_case_globals)]
pub const kUCKeyActionDisplay: u16 = 3;
#[allow(non_upper_case_globals)]
pub const kUCKeyTranslateNoDeadKeysMask: OptionBits = 1;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
  pub static kTISPropertyUnicodeKeyLayoutData: CFStringRef;

  #[allow(non_snake_case)]
  pub fn TISGetInputSourceProperty(
    inputSource: TISInputSourceRef,
    propertyKey: CFStringRef,
  ) -> CFDataRef;

  pub fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;

  pub fn LMGetKbdType() -> u8;

  #[allow(non_snake_case)]
  pub fn UCKeyTranslate(
    keyLayoutPtr: *const UCKeyboardLayout,
    virtualKeyCode: u16,
    keyAction: u16,
    modifierKeyState: u32,
    keyboardType: u32,
    keyTranslateOptions: OptionBits,
    deadKeyState: *mut u32,
    maxStringLength: UniCharCount,
    actualStringLength: *mut UniCharCount,
    unicodeString: *mut UniChar,
  ) -> OSStatus;
}

mod core_video {
  use super::*;

  #[link(name = "CoreVideo", kind = "framework")]
  extern "C" {}

  // CVBase.h

  pub type CVTimeFlags = i32; // int32_t
  pub const kCVTimeIsIndefinite: CVTimeFlags = 1 << 0;

  #[repr(C)]
  #[derive(Debug, Clone)]
  pub struct CVTime {
    pub time_value: i64, // int64_t
    pub time_scale: i32, // int32_t
    pub flags: i32,      // int32_t
  }

  // CVReturn.h

  pub type CVReturn = i32; // int32_t
  pub const kCVReturnSuccess: CVReturn = 0;

  // CVDisplayLink.h

  pub type CVDisplayLinkRef = *mut c_void;

  extern "C" {
    pub fn CVDisplayLinkCreateWithCGDisplay(
      displayID: CGDirectDisplayID,
      displayLinkOut: *mut CVDisplayLinkRef,
    ) -> CVReturn;
    pub fn CVDisplayLinkGetNominalOutputVideoRefreshPeriod(displayLink: CVDisplayLinkRef)
      -> CVTime;
    pub fn CVDisplayLinkRelease(displayLink: CVDisplayLinkRef);
  }
}

pub use core_video::*;
