// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use std::{cell::UnsafeCell, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, ptr};

use windows::{
  core::implement,
  Win32::{
    Foundation::{self as win32f, HWND, POINTL},
    System::{
      Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL},
      Ole::{
        IDropTarget, IDropTarget_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
      },
      SystemServices::MODIFIERKEYS_FLAGS,
    },
    UI::Shell::{DragFinish, DragQueryFileW, HDROP},
  },
};

use crate::platform_impl::platform::WindowId;

use crate::{event::Event, window::WindowId as SuperWindowId};

#[implement(IDropTarget)]
pub struct FileDropHandler {
  window: HWND,
  send_event: Box<dyn Fn(Event<'static, ()>)>,
  cursor_effect: UnsafeCell<DROPEFFECT>,
  hovered_is_valid: UnsafeCell<bool>, /* If the currently hovered item is not valid there must not be any `HoveredFileCancelled` emitted */
}

impl FileDropHandler {
  pub fn new(window: HWND, send_event: Box<dyn Fn(Event<'static, ()>)>) -> FileDropHandler {
    Self {
      window,
      send_event,
      cursor_effect: DROPEFFECT_NONE.into(),
      hovered_is_valid: false.into(),
    }
  }

  unsafe fn iterate_filenames<F>(
    data_obj: windows_core::Ref<'_, IDataObject>,
    callback: F,
  ) -> Option<HDROP>
  where
    F: Fn(PathBuf),
  {
    let drop_format = FORMATETC {
      cfFormat: CF_HDROP.0,
      ptd: ptr::null_mut(),
      dwAspect: DVASPECT_CONTENT.0,
      lindex: -1,
      tymed: TYMED_HGLOBAL.0 as u32,
    };

    match data_obj
      .as_ref()
      .expect("Received null IDataObject")
      .GetData(&drop_format)
    {
      Ok(medium) => {
        let hglobal = medium.u.hGlobal;
        let hdrop = HDROP(hglobal.0 as _);

        // The second parameter (0xFFFFFFFF) instructs the function to return the item count
        let mut lpsz_file = [];
        let item_count = DragQueryFileW(hdrop, 0xFFFFFFFF, Some(&mut lpsz_file));

        for i in 0..item_count {
          // Get the length of the path string NOT including the terminating null character.
          // Previously, this was using a fixed size array of MAX_PATH length, but the
          // Windows API allows longer paths under certain circumstances.
          let character_count = DragQueryFileW(hdrop, i, Some(&mut lpsz_file)) as usize;
          let str_len = character_count + 1;

          // Fill path_buf with the null-terminated file name
          let mut path_buf = Vec::with_capacity(str_len);
          DragQueryFileW(hdrop, i, std::mem::transmute(path_buf.spare_capacity_mut()));
          path_buf.set_len(str_len);

          callback(OsString::from_wide(&path_buf[0..character_count]).into());
        }

        Some(hdrop)
      }
      Err(error) => {
        debug!(
          "{}",
          match error.code() {
            win32f::DV_E_FORMATETC => {
              // If the dropped item is not a file this error will occur.
              // In this case it is OK to return without taking further action.
              "Error occured while processing dropped/hovered item: item is not a file."
            }
            _ => "Unexpected error occured while processing dropped/hovered item.",
          }
        );
        None
      }
    }
  }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for FileDropHandler_Impl {
  fn DragEnter(
    &self,
    pDataObj: windows_core::Ref<'_, IDataObject>,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    _pt: &POINTL,
    pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    use crate::event::WindowEvent::HoveredFile;
    unsafe {
      let hdrop = FileDropHandler::iterate_filenames(pDataObj, |filename| {
        (self.send_event)(Event::WindowEvent {
          window_id: SuperWindowId(WindowId(self.window.0 as _)),
          event: HoveredFile(filename),
        });
      });
      let hovered_is_valid = hdrop.is_some();
      let cursor_effect = if hovered_is_valid {
        DROPEFFECT_COPY
      } else {
        DROPEFFECT_NONE
      };
      *self.hovered_is_valid.get() = hovered_is_valid;
      *self.cursor_effect.get() = cursor_effect;
      *pdwEffect = cursor_effect;
    }
    Ok(())
  }

  fn DragOver(
    &self,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    _pt: &POINTL,
    pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    unsafe {
      *pdwEffect = *self.cursor_effect.get();
    }
    Ok(())
  }

  fn DragLeave(&self) -> windows::core::Result<()> {
    use crate::event::WindowEvent::HoveredFileCancelled;
    if unsafe { *self.hovered_is_valid.get() } {
      (self.send_event)(Event::WindowEvent {
        window_id: SuperWindowId(WindowId(self.window.0 as _)),
        event: HoveredFileCancelled,
      });
    }
    Ok(())
  }

  fn Drop(
    &self,
    pDataObj: windows_core::Ref<'_, IDataObject>,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    _pt: &POINTL,
    _pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    use crate::event::WindowEvent::DroppedFile;
    unsafe {
      let hdrop = FileDropHandler::iterate_filenames(pDataObj, |filename| {
        (self.send_event)(Event::WindowEvent {
          window_id: SuperWindowId(WindowId(self.window.0 as _)),
          event: DroppedFile(filename),
        });
      });
      if let Some(hdrop) = hdrop {
        DragFinish(hdrop);
      }
    }
    Ok(())
  }
}
