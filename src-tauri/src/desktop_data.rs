//! Descriptor-relative reads of existing Claude Desktop profiles.
//! Format provenance: docs/desktop-format-provenance.md.
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(crate) struct Dir {
    pub path: PathBuf,
    pub(crate) fd: File,
}

pub(crate) fn component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid Desktop account, organization, session, or task identifier".into());
    }
    Ok(())
}

pub(crate) fn name(value: &str) -> io::Result<CString> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename",
        ));
    }
    CString::new(value).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid filename"))
}

impl Dir {
    pub fn open(path: &Path) -> io::Result<Self> {
        let fd = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            fd,
        })
    }

    pub(crate) fn open_at(&self, value: &str, flags: i32) -> io::Result<File> {
        let value = name(value)?;
        // SAFETY: both descriptors and the NUL-terminated name remain alive for the call.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                value.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned an owned descriptor; File closes it exactly once.
        Ok(unsafe { File::from_raw_fd(raw) })
    }

    pub fn child(&self, value: &str) -> io::Result<Self> {
        let fd = self.open_at(value, libc::O_RDONLY | libc::O_DIRECTORY)?;
        Ok(Self {
            path: self.path.join(value),
            fd,
        })
    }

    pub fn entries(&self) -> io::Result<Vec<String>> {
        // Enumeration supplies names only. Every subsequent read/write opens relative
        // to the held descriptor, never trusts an entry's path or follows its symlink.
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.path)? {
            if let Some(name) = entry?.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn read(&self, value: &str, limit: u64) -> io::Result<Vec<u8>> {
        let file = self.open_at(value, libc::O_RDONLY | libc::O_NONBLOCK)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.nlink() != 1 || meta.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not a bounded, single-link regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File grew beyond its size limit",
            ));
        }
        Ok(bytes)
    }

    pub fn optional(&self, value: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
        match self.read(value, limit) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RootKind {
    Default,
    Managed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileKey {
    pub root: RootKind,
    pub account: String,
    pub org: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Profile {
    pub key: ProfileKey,
    pub last_known_account: bool,
}

pub(crate) fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
pub(crate) fn parse(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(error)
}
pub(crate) fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}
pub(crate) fn live_roots() -> Result<[PathBuf; 2], String> {
    let (normal, managed) = crate::claude_desktop_config::desktop_data_dirs().map_err(error)?;
    Ok([normal, managed])
}
pub(crate) fn root(roots: &[PathBuf; 2], kind: RootKind) -> &Path {
    &roots[match kind {
        RootKind::Default => 0,
        RootKind::Managed => 1,
    }]
}
pub(crate) fn profile_dirs(roots: &[PathBuf; 2], key: &ProfileKey) -> Result<(Dir, Dir), String> {
    component(&key.account)?;
    component(&key.org)?;
    let account = Dir::open(root(roots, key.root))
        .map_err(error)?
        .child("claude-code-sessions")
        .map_err(error)?
        .child(&key.account)
        .map_err(error)?;
    let org = account.child(&key.org).map_err(error)?;
    Ok((account, org))
}
pub(crate) fn discover(roots: &[PathBuf; 2]) -> Result<(Vec<Profile>, Vec<String>), String> {
    let mut profiles = vec![];
    let mut warnings = vec![];
    for kind in [RootKind::Default, RootKind::Managed] {
        let data = match Dir::open(root(roots, kind)) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                warnings.push(error(e));
                continue;
            }
        };
        let current = data
            .optional("config.json", 4 * 1024 * 1024)
            .ok()
            .flatten()
            .and_then(|b| parse(&b).ok())
            .and_then(|v| {
                v.get("lastKnownAccountUuid")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        let base = match data.child("claude-code-sessions") {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                warnings.push(error(e));
                continue;
            }
        };
        for account in base.entries().map_err(error)? {
            if component(&account).is_err() {
                continue;
            }
            let dir = match base.child(&account) {
                Ok(dir) => dir,
                Err(e) => {
                    warnings.push(error(e));
                    continue;
                }
            };
            for org in dir.entries().map_err(error)? {
                if component(&org).is_err() {
                    continue;
                }
                if let Err(e) = dir.child(&org) {
                    warnings.push(error(e));
                    continue;
                }
                profiles.push(Profile {
                    key: ProfileKey {
                        root: kind,
                        account: account.clone(),
                        org,
                    },
                    last_known_account: current.as_ref() == Some(&account),
                });
            }
        }
    }
    Ok((profiles, warnings))
}
