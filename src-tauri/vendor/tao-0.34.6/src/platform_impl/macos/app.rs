// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{collections::VecDeque, ffi::CStr};

use objc2::runtime::{AnyClass as Class, ClassBuilder as ClassDecl, Sel};
use objc2_app_kit::{self as appkit, NSApplication, NSEvent, NSEventType};
use once_cell::sync::Lazy;

use super::{app_state::AppState, event::EventWrapper, util, DEVICE_ID};
use crate::event::{DeviceEvent, ElementState, Event};

pub struct AppClass(pub *const Class);
unsafe impl Send for AppClass {}
unsafe impl Sync for AppClass {}

pub static APP_CLASS: Lazy<AppClass> = Lazy::new(|| unsafe {
  let superclass = class!(NSApplication);
  let mut decl =
    ClassDecl::new(CStr::from_bytes_with_nul(b"TaoApp\0").unwrap(), superclass).unwrap();

  decl.add_method(sel!(sendEvent:), send_event as extern "C" fn(_, _, _));

  AppClass(decl.register())
});

// Normally, holding Cmd + any key never sends us a `keyUp` event for that key.
// Overriding `sendEvent:` like this fixes that. (https://stackoverflow.com/a/15294196)
// Fun fact: Firefox still has this bug! (https://bugzilla.mozilla.org/show_bug.cgi?id=1299553)
extern "C" fn send_event(this: &NSApplication, _sel: Sel, event: &NSEvent) {
  unsafe {
    // For posterity, there are some undocumented event types
    // (https://github.com/servo/cocoa-rs/issues/155)
    // but that doesn't really matter here.
    let event_type = event.r#type();
    let modifier_flags = event.modifierFlags();
    if event_type == appkit::NSKeyUp
      && util::has_flag(modifier_flags, appkit::NSEventModifierFlags::Command)
    {
      if let Some(key_window) = this.keyWindow() {
        key_window.sendEvent(event);
      } else {
        log::debug!("skip sending CMD keyEvent - app has no keyWindow");
      }
    } else {
      maybe_dispatch_device_event(event);
      let superclass = util::superclass(this);
      let _: () = msg_send![super(this, superclass), sendEvent: event];
    }
  }
}

unsafe fn maybe_dispatch_device_event(event: &NSEvent) {
  let event_type = event.r#type();
  match event_type {
    NSEventType::MouseMoved
    | NSEventType::LeftMouseDragged
    | NSEventType::OtherMouseDragged
    | NSEventType::RightMouseDragged => {
      let mut events = VecDeque::with_capacity(3);

      let delta_x = event.deltaX() as f64;
      let delta_y = event.deltaY() as f64;

      if delta_x != 0.0 {
        events.push_back(EventWrapper::StaticEvent(Event::DeviceEvent {
          device_id: DEVICE_ID,
          event: DeviceEvent::Motion {
            axis: 0,
            value: delta_x,
          },
        }));
      }

      if delta_y != 0.0 {
        events.push_back(EventWrapper::StaticEvent(Event::DeviceEvent {
          device_id: DEVICE_ID,
          event: DeviceEvent::Motion {
            axis: 1,
            value: delta_y,
          },
        }));
      }

      if delta_x != 0.0 || delta_y != 0.0 {
        events.push_back(EventWrapper::StaticEvent(Event::DeviceEvent {
          device_id: DEVICE_ID,
          event: DeviceEvent::MouseMotion {
            delta: (delta_x, delta_y),
          },
        }));
      }

      AppState::queue_events(events);
    }
    NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown => {
      let mut events = VecDeque::with_capacity(1);

      events.push_back(EventWrapper::StaticEvent(Event::DeviceEvent {
        device_id: DEVICE_ID,
        event: DeviceEvent::Button {
          button: event.buttonNumber() as u32,
          state: ElementState::Pressed,
        },
      }));

      AppState::queue_events(events);
    }
    NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp => {
      let mut events = VecDeque::with_capacity(1);

      events.push_back(EventWrapper::StaticEvent(Event::DeviceEvent {
        device_id: DEVICE_ID,
        event: DeviceEvent::Button {
          button: event.buttonNumber() as u32,
          state: ElementState::Released,
        },
      }));

      AppState::queue_events(events);
    }
    _ => (),
  }
}
