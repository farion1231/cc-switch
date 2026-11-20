//! Atomic Desktop recovery publication. Callers must preview and verify source state.
use crate::desktop_data::{error, name, Dir};
use std::{
    io::{self, Write},
    os::fd::AsRawFd,
    process::Command,
    sync::Mutex,
};
pub(crate) static RECOVERY_LOCK: Mutex<()> = Mutex::new(());
#[derive(Debug)]
pub(crate) struct PublicationError {
    cause: io::Error,
    pub published: bool,
}

impl PublicationError {
    fn before(cause: io::Error) -> Self {
        Self {
            cause,
            published: false,
        }
    }
    pub fn kind(&self) -> io::ErrorKind {
        self.cause.kind()
    }
}

impl std::fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(formatter)
    }
}

pub(crate) fn write(
    dir: &Dir,
    value: &str,
    bytes: &[u8],
    expected: Option<&[u8]>,
) -> Result<(), PublicationError> {
    if let Some(expected) = expected {
        if dir
            .read(value, 16 * 1024 * 1024)
            .map_err(PublicationError::before)?
            != expected
        {
            return Err(PublicationError::before(io::Error::other(
                "Desktop registry changed; preview again",
            )));
        }
    }
    publish(dir, value, bytes, expected.is_some())
}
fn publish(dir: &Dir, value: &str, bytes: &[u8], replace: bool) -> Result<(), PublicationError> {
    let temp = format!(".cc-switch-{}.tmp", uuid::Uuid::new_v4());
    let mut published = false;
    let result = (|| {
        let mut file = dir.open_at(&temp, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let from = name(&temp)?;
        let to = name(value)?;
        let fd = dir.fd.as_raw_fd();
        // SAFETY: filenames and the directory descriptor stay alive. linkat has
        // exclusive destination semantics, including an existing dangling symlink.
        let result = unsafe {
            if replace {
                libc::renameat(fd, from.as_ptr(), fd, to.as_ptr())
            } else {
                libc::linkat(fd, from.as_ptr(), fd, to.as_ptr(), 0)
            }
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        published = true;
        Ok(())
    })();
    if let Ok(temp) = name(&temp) {
        // SAFETY: only the temporary basename created by this operation is removed.
        unsafe {
            libc::unlinkat(dir.fd.as_raw_fd(), temp.as_ptr(), 0);
        }
    }
    // Even if directory synchronization fails, the receipt must disclose the
    // published destination instead of implying that no write happened.
    result
        .and_then(|()| dir.fd.sync_all())
        .map_err(|cause| PublicationError { cause, published })
}
pub(crate) fn ensure_stopped() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Recovery writes are currently supported on macOS only".into());
    }
    let output = Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
        .map_err(error)?;
    if !output.status.success() {
        return Err("Could not verify that Claude Desktop is closed".into());
    }
    let list = String::from_utf8(output.stdout).map_err(error)?;
    if list.lines().any(|line| {
        let line = line.trim();
        line.contains("Claude.app/Contents/")
            || line.contains("Claude-3p.app/Contents/")
            || line == "Claude"
            || line == "Claude-3p"
    }) {
        return Err("Quit Claude Desktop before applying recovery, then preview again".into());
    }
    Ok(())
}
