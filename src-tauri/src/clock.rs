//! Wall-clock reads.
//!
//! Every read of the wall clock goes through this module, so that a parity build can pin the
//! time the app records and shows. Pick the function by what the value is for:
//!
//! - [`now`], [`now_local`] and [`system_now`] give the app's time: what it writes into files,
//!   the database and the UI, what it puts into file names, and what it compares with timestamps
//!   it stored itself (expiry, retention, "today").
//! - [`os_now`] gives the operating system's time, for comparing with times the OS keeps (file
//!   modification times, lock ages), for setting those times, and for values that only need to
//!   be unique. Its value is never stored or shown.
//!
//! Durations, timeouts and deadlines use [`std::time::Instant`], which this module leaves alone.
//!
//! In a build with the `parity` feature, when `CC_SWITCH_PARITY_CLOCK` names a file, the app's
//! time is the RFC 3339 instant in that file and stays there until the file changes. Other
//! builds read the system clock and ignore the variable.
#![allow(clippy::disallowed_methods)]

use chrono::{DateTime, Local, Utc};
use std::time::SystemTime;

/// The app's time.
pub fn now() -> DateTime<Utc> {
    #[cfg(feature = "parity")]
    if let Some(t) = pinned::now() {
        return t;
    }
    Utc::now()
}

/// The app's time in the local time zone.
pub fn now_local() -> DateTime<Local> {
    now().with_timezone(&Local)
}

/// The app's time in SQLite's `datetime('now')` form, for binding into a statement instead of
/// letting SQLite read the system clock.
pub fn sqlite_datetime() -> String {
    now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The app's time as a [`SystemTime`].
pub fn system_now() -> SystemTime {
    #[cfg(feature = "parity")]
    if let Some(t) = pinned::now() {
        return t.into();
    }
    SystemTime::now()
}

/// The operating system's time, for comparing with or setting times the OS keeps and for values
/// that only need to be unique. Never store or show it.
pub fn os_now() -> SystemTime {
    SystemTime::now()
}

/// The parity suite's pinned clock.
#[cfg(feature = "parity")]
pub(crate) mod pinned {
    use chrono::{DateTime, Utc};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::Duration;

    const ENV: &str = "CC_SWITCH_PARITY_CLOCK";

    /// The clock file, when the harness set one.
    pub(crate) fn path() -> Option<&'static Path> {
        static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PATH.get_or_init(|| {
            std::env::var_os(ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .as_deref()
    }

    /// The instant in the clock file, or `None` when there is no clock file. A clock file that
    /// cannot be read is a harness fault, and falling back to the system clock would hide it, so
    /// the app stops.
    pub(crate) fn now() -> Option<DateTime<Utc>> {
        let path = path()?;
        let mut last = String::new();
        // The harness replaces the file by renaming a new one over it, so a read sees the old
        // instant or the new one. The retries cover a reader that races the rename on Windows.
        for _ in 0..5 {
            match read(path) {
                Ok(t) => return Some(t),
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("parity clock {}: {last}", path.display());
        std::process::abort()
    }

    fn read(path: &Path) -> Result<DateTime<Utc>, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        DateTime::parse_from_rfc3339(text.trim())
            .map(|t| t.with_timezone(&Utc))
            .map_err(|e| format!("{e}: {:?}", text.trim()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// SQLite's own clock functions bypass this module, so statements bind the time instead. The
    /// column defaults in `database/schema.rs` stay, because removing one needs a table rebuild.
    #[test]
    fn sql_binds_the_time_instead_of_reading_sqlite_clock() {
        const READS: [&str; 5] = [
            "datetime('now'",
            "date('now'",
            "time('now'",
            "unixepoch(",
            "CURRENT_TIMESTAMP",
        ];
        let mut found = Vec::new();
        visit(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut |path, text| {
                let test_file = path
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().ends_with("tests"));
                if path.ends_with("database/schema.rs")
                    || test_file
                    || path.components().any(|c| c.as_os_str() == "tests")
                {
                    return;
                }
                // Test code may use SQLite's clock; it never runs in a parity build.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for (i, line) in code.lines().enumerate() {
                    if READS.iter().any(|r| line.contains(r))
                        && !line.trim_start().starts_with("//")
                    {
                        found.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            },
        );
        assert!(
            found.is_empty(),
            "bind crate::clock::sqlite_datetime() or crate::clock::now() instead:\n{}",
            found.join("\n")
        );
    }

    fn visit(dir: &Path, f: &mut dyn FnMut(&Path, &str)) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, f);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("clock.rs") {
                f(&path, &std::fs::read_to_string(&path).unwrap());
            }
        }
    }
}
