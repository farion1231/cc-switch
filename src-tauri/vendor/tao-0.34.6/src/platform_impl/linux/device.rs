use std::{
  os::raw::{c_int, c_uchar},
  ptr,
};

use gtk::glib;
use x11_dl::{xinput2, xlib};

use crate::event::{DeviceEvent, ElementState, RawKeyEvent};

use super::keycode_from_scancode;

/// Spawn Device event thread. Only works on x11 since wayland doesn't have such global events.
pub fn spawn(device_tx: glib::Sender<DeviceEvent>) {
  std::thread::spawn(move || unsafe {
    let xlib = xlib::Xlib::open().unwrap();
    let xinput2 = xinput2::XInput2::open().unwrap();
    let display = (xlib.XOpenDisplay)(ptr::null());
    let root = (xlib.XDefaultRootWindow)(display);
    // TODO Add more device event mask
    let mask = xinput2::XI_RawKeyPressMask | xinput2::XI_RawKeyReleaseMask;
    let mut event_mask = xinput2::XIEventMask {
      deviceid: xinput2::XIAllMasterDevices,
      mask: &mask as *const _ as *mut c_uchar,
      mask_len: std::mem::size_of_val(&mask) as c_int,
    };
    (xinput2.XISelectEvents)(display, root, &mut event_mask as *mut _, 1);

    #[allow(clippy::uninit_assumed_init)]
    let mut event: xlib::XEvent = std::mem::MaybeUninit::uninit().assume_init();
    loop {
      (xlib.XNextEvent)(display, &mut event);

      // XFilterEvent tells us when an event has been discarded by the input method.
      // Specifically, this involves all of the KeyPress events in compose/pre-edit sequences,
      // along with an extra copy of the KeyRelease events. This also prevents backspace and
      // arrow keys from being detected twice.
      if xlib::True == {
        (xlib.XFilterEvent)(&mut event, {
          let xev: &xlib::XAnyEvent = event.as_ref();
          xev.window
        })
      } {
        continue;
      }

      let event_type = event.get_type();
      if event_type == xlib::GenericEvent {
        let mut xev = event.generic_event_cookie;
        if (xlib.XGetEventData)(display, &mut xev) == xlib::True {
          match xev.evtype {
            xinput2::XI_RawKeyPress | xinput2::XI_RawKeyRelease => {
              let xev: &xinput2::XIRawEvent = &*(xev.data as *const _);
              let physical_key = keycode_from_scancode(xev.detail as u32);
              let state = match xev.evtype {
                xinput2::XI_RawKeyPress => ElementState::Pressed,
                xinput2::XI_RawKeyRelease => ElementState::Released,
                _ => unreachable!(),
              };

              let event = RawKeyEvent {
                physical_key,
                state,
              };

              if let Err(e) = device_tx.send(DeviceEvent::Key(event)) {
                log::info!("Failed to send device event {} since receiver is closed. Closing x11 thread along with it", e);
                break;
              }
            }
            _ => {}
          }
        }
      }
    }
  });
}
