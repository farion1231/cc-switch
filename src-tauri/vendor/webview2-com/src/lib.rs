extern crate webview2_com_sys;
pub use webview2_com_sys::Microsoft;

#[macro_use]
extern crate webview2_com_macros;

mod callback;
mod options;
mod pwstr;

use std::{fmt, sync::mpsc::{self, TryRecvError}};

use windows::{
    core::{HRESULT, PCWSTR},
    Win32::{
        Foundation::CloseHandle,
        System::{
            Com::{CoWaitForMultipleHandles, COWAIT_DISPATCH_CALLS, COWAIT_DISPATCH_WINDOW_MESSAGES},
            Threading::CreateEventW,
        },
    },
};

pub use callback::*;
pub use options::*;
pub use pwstr::*;

#[derive(Debug)]
pub enum Error {
    WindowsError(windows::core::Error),
    CallbackError(String),
    TaskCanceled,
    SendError,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<windows::core::Error> for Error {
    fn from(err: windows::core::Error) -> Self {
        Self::WindowsError(err)
    }
}

impl From<HRESULT> for Error {
    fn from(err: HRESULT) -> Self {
        Self::WindowsError(windows::core::Error::from(err))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Wait for a WebView2 callback while allowing COM to re-enter the STA.
///
/// WebView2 delivers several completion callbacks through COM apartment
/// re-entrancy. A nested `GetMessage` loop can consume the messages without
/// allowing the callback to run, which leaves initialization stuck forever.
/// `CoWaitForMultipleHandles` is the COM-supported wait primitive for this
/// situation. The short timeout lets us also poll the channel used by the
/// generated callback helpers.
pub fn wait_with_pump<T>(rx: mpsc::Receiver<T>) -> Result<T> {
    let wake_event = unsafe { CreateEventW(None, true, false, PCWSTR::null())? };
    let flags = (COWAIT_DISPATCH_CALLS.0 | COWAIT_DISPATCH_WINDOW_MESSAGES.0) as u32;

    loop {
        match rx.try_recv() {
            Ok(result) => {
                unsafe { CloseHandle(wake_event)? };
                return Ok(result);
            }
            Err(TryRecvError::Disconnected) => {
                unsafe { CloseHandle(wake_event)? };
                return Err(Error::TaskCanceled);
            }
            Err(TryRecvError::Empty) => {}
        }

        let _ = unsafe { CoWaitForMultipleHandles(flags, 50, &[wake_event]) };
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_with_pump, Error};
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn disconnected_callback_channel_returns_cancellation() {
        let (sender, receiver) = mpsc::channel::<()>();
        drop(sender);

        let (result_sender, result_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            result_sender
                .send(wait_with_pump(receiver))
                .expect("the test receiver should still be available");
        });

        let result = result_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("wait_with_pump should return after the callback channel disconnects");

        assert!(matches!(result, Err(Error::TaskCanceled)));
    }
}
