//! Wall-clock reads.
//!
//! Every read of the wall clock goes through this module, so that a parity build can pin the
//! time the app records and shows. Pick the function by what the value is for:
//!
//! - [`now`], [`now_local`] and [`system_now`] give the app's time: what it writes into files,
//!   the database and the UI, what it puts into file names, and what it compares with timestamps
//!   it stored itself (expiry, retention, "today").
//! - [`os_now`] and [`os_now_utc`] give the operating system's time, for comparing with times the
//!   OS keeps (file modification times, lock ages), for setting those times, for values that only
//!   need to be unique, and for deadlines and poll throttles kept only in memory, which measure how
//!   much time has passed. Their value is never stored or shown.
//!
//! New durations, timeouts and deadlines use [`std::time::Instant`], which this module leaves
//! alone.
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

/// The operating system's time, for comparing with or setting times the OS keeps, for values that
/// only need to be unique, and for deadlines kept only in memory. Never store or show it.
pub fn os_now() -> SystemTime {
    SystemTime::now()
}

/// [`os_now`] as a chrono time, for in-memory deadlines kept in Unix milliseconds.
pub fn os_now_utc() -> DateTime<Utc> {
    Utc::now()
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
    /// column defaults and migrations in `database/schema.rs` stay, because removing a default
    /// needs a table rebuild.
    #[test]
    fn sql_binds_the_time_instead_of_reading_sqlite_clock() {
        // `'now'` covers datetime, date, time, julianday, strftime and unixepoch with 'now'.
        const READS: [&str; 5] = [
            "'now'",
            "unixepoch()",
            "CURRENT_TIMESTAMP",
            "CURRENT_DATE",
            "CURRENT_TIME",
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
                for (i, line) in without_test_items(text).lines().enumerate() {
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

    #[test]
    fn test_items_are_blanked_and_the_rest_is_kept() {
        let src = concat!(
            "a\n",
            "#[cfg(test)]\n",
            "mod tests;\n",
            "b\n",
            "#[cfg(all(test, unix))]\n",
            "mod t {\n",
            "    let s = \"}\"; let r = r#\"{\n",
            "    \"x\": }\"#; // }\n",
            "    fn f() { '{'; }\n",
            "}\n",
            "c\n",
        );
        let kept = without_test_items(src);
        let kept: Vec<&str> = kept.lines().collect();
        assert_eq!(kept, ["a", "", "", "b", "", "", "", "", "", "", "c"]);
    }

    /// Blanks every item that a `#[cfg(test)]` or `#[cfg(all(test, ...))]` attribute applies to,
    /// keeping line numbers: a `mod name;` line, or a braced item up to its closing brace. Braces
    /// inside strings, raw strings, chars and comments do not count.
    fn without_test_items(src: &str) -> String {
        let b = src.as_bytes();
        let mut blank = vec![false; b.len()];
        let mut line_start = 0;
        while line_start < b.len() {
            let line_end = src[line_start..]
                .find('\n')
                .map_or(b.len(), |n| line_start + n);
            let t = src[line_start..line_end].trim_start();
            if t.starts_with("#[cfg(test)]") || t.starts_with("#[cfg(all(test") {
                let end = item_end(b, line_end);
                blank[line_start..end].iter_mut().for_each(|x| *x = true);
                line_start = end;
            } else {
                line_start = line_end + 1;
            }
        }
        b.iter()
            .zip(&blank)
            .filter(|(c, blanked)| !**blanked || **c == b'\n')
            .map(|(c, _)| *c as char)
            .collect()
    }

    /// Where the item after the attribute line ending at `from` ends: after its `;` when it has no
    /// body, or after the brace that closes its body.
    fn item_end(b: &[u8], from: usize) -> usize {
        let (mut k, mut depth) = (from, 0i32);
        while k < b.len() {
            match b[k] {
                b'/' if b.get(k + 1) == Some(&b'/') => {
                    while k < b.len() && b[k] != b'\n' {
                        k += 1;
                    }
                }
                b'/' if b.get(k + 1) == Some(&b'*') => {
                    k += 2;
                    while k + 1 < b.len() && !(b[k] == b'*' && b[k + 1] == b'/') {
                        k += 1;
                    }
                    k += 1;
                }
                b'r' if matches!(b.get(k + 1), Some(b'"' | b'#'))
                    && !b[k.saturating_sub(1)].is_ascii_alphanumeric() =>
                {
                    let hashes = b[k + 1..].iter().take_while(|c| **c == b'#').count();
                    if b.get(k + 1 + hashes) == Some(&b'"') {
                        k += 2 + hashes;
                        while k < b.len()
                            && !(b[k] == b'"'
                                && b[k + 1..]
                                    .iter()
                                    .take(hashes)
                                    .filter(|c| **c == b'#')
                                    .count()
                                    == hashes)
                        {
                            k += 1;
                        }
                        k += hashes;
                    }
                }
                b'"' => {
                    k += 1;
                    while k < b.len() && b[k] != b'"' {
                        k += if b[k] == b'\\' { 2 } else { 1 };
                    }
                }
                b'\'' if b.get(k + 2) == Some(&b'\'') => k += 2,
                b'\'' if b.get(k + 1) == Some(&b'\\') => {
                    while k + 1 < b.len() && b[k + 1] != b'\'' {
                        k += 1;
                    }
                    k += 1;
                }
                b';' if depth == 0 => return k + 1,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return k + 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        b.len()
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
