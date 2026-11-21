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
    use regex::Regex;
    use std::path::Path;

    /// SQLite reads its own clock for `'now'` in any letter case, for the date functions called
    /// with no time value, for `strftime` with only a format, and for `CURRENT_TIMESTAMP`,
    /// `CURRENT_DATE` and `CURRENT_TIME`. Matched only inside string literals, where SQL lives,
    /// so Rust calls such as `.date()` do not count.
    fn sqlite_clock_read() -> Regex {
        Regex::new(
            r"(?i)'now'|\b(datetime|date|time|julianday|unixepoch)\s*\(\s*\)|\bstrftime\s*\(\s*'[^']*'\s*\)|\bcurrent_(timestamp|date|time)\b",
        )
        .unwrap()
    }

    /// SQLite's own clock bypasses this module, so statements bind the time instead. The column
    /// defaults and migrations in `database/schema.rs` stay, because removing a default needs a
    /// table rebuild.
    #[test]
    fn sql_binds_the_time_instead_of_reading_sqlite_clock() {
        let read = sqlite_clock_read();
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
                let code = without_test_items(text);
                for (line, literal) in string_literals(&code) {
                    if read.is_match(&literal) {
                        found.push(format!("{}:{line}: {}", path.display(), literal.trim()));
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
    fn sqlite_clock_reads_are_recognised() {
        let read = sqlite_clock_read();
        for sql in [
            "datetime('now')",
            "DATE('NOW')",
            "datetime()",
            "julianday( )",
            "unixepoch()",
            "strftime('%s')",
            "current_timestamp",
            "CURRENT_DATE",
        ] {
            assert!(read.is_match(sql), "{sql}");
        }
        for sql in [
            "datetime(?1)",
            "strftime('%s', ?1)",
            "updated_at = ?3",
            "date(created_at)",
        ] {
            assert!(!read.is_match(sql), "{sql}");
        }
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
            "c 中\n",
            "struct S {\n",
            "    #[cfg(test)]\n",
            "    hook: Option<fn(u8, u8)>,\n",
            "    kept: u8,\n",
            "}\n",
            "fn g() -> S {\n",
            "    S {\n",
            "        #[cfg(test)]\n",
            "        hook: None,\n",
            "        kept: 1\n",
            "    }\n",
            "}\n",
            "fn h(S { #[cfg(test)]\n",
            "    hook, kept }: S) {}\n",
            "d\n",
        );
        let kept = without_test_items(src);
        let kept: Vec<&str> = kept.lines().collect();
        assert_eq!(
            kept,
            [
                "a",
                "",
                "",
                "b",
                "",
                "",
                "",
                "",
                "",
                "",
                "c 中",
                "struct S {",
                "    ",
                "",
                "    kept: u8,",
                "}",
                "fn g() -> S {",
                "    S {",
                "        ",
                "",
                "        kept: 1",
                "    }",
                "}",
                "fn h(S { ",
                " kept }: S) {}",
                "d",
            ]
        );
    }

    #[test]
    fn string_literals_are_found_with_their_lines() {
        let src = concat!(
            "let a = \"x\";\n",
            "// \"not\"\n",
            "let b = r#\"y\n",
            "\"z\"#; let c = '\"';\n",
            "let d = br#\"{\"q\"#; let e = \"时w\";\n",
            "let f = ['\\'','\"','中','\"']; let g = \"u\";\n",
        );
        assert_eq!(
            string_literals(src),
            [
                (1, "x".to_string()),
                (3, "y\n\"z".to_string()),
                (5, "{\"q".to_string()),
                (5, "时w".to_string()),
                (6, "u".to_string()),
            ]
        );
    }

    /// Blanks every item that a `#[cfg(test)]` or `#[cfg(all(test, ...))]` attribute applies to,
    /// keeping line numbers: a module, function, impl or other item, a struct field, a field in a
    /// struct literal, or a pattern. Brackets inside strings, raw strings, chars and comments do
    /// not count.
    fn without_test_items(src: &str) -> String {
        let b = src.as_bytes();
        let mut blank = vec![false; b.len()];
        let mut k = 0;
        while k < b.len() {
            let rest = &b[k..];
            if rest.starts_with(b"#[cfg(test)]") || rest.starts_with(b"#[cfg(all(test") {
                let attr_end = k + rest
                    .iter()
                    .position(|c| *c == b']')
                    .map_or(rest.len(), |n| n + 1);
                let end = item_end(b, attr_end);
                blank[k..end].iter_mut().for_each(|x| *x = true);
                k = end;
            } else {
                k = literal_at(b, k).map_or(k + 1, |(_, end)| end);
            }
        }
        let kept: Vec<u8> = b
            .iter()
            .zip(&blank)
            .filter(|(c, blanked)| !**blanked || **c == b'\n')
            .map(|(c, _)| *c)
            .collect();
        String::from_utf8(kept).unwrap()
    }

    /// Where the thing after an attribute ends. An item ends after its `;` or after the brace
    /// that closes its body. A field or pattern ends after the `,` that follows it, or before the
    /// bracket that closes the enclosing list.
    fn item_end(b: &[u8], from: usize) -> usize {
        let (mut k, mut depth) = (from, 0i32);
        while k < b.len() {
            if let Some((_, end)) = literal_at(b, k) {
                k = end;
                continue;
            }
            match b[k] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => return k,
                b')' | b']' => depth -= 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return k + 1;
                    }
                }
                b';' | b',' if depth == 0 => return k + 1,
                _ => {}
            }
            k += 1;
        }
        b.len()
    }

    /// A comment, string, raw string or char literal starting at `k`: the byte range of a
    /// string's contents (none for the others) and where the literal ends.
    #[allow(clippy::type_complexity)]
    fn literal_at(b: &[u8], k: usize) -> Option<(Option<(usize, usize)>, usize)> {
        let ident = |i: usize| b[i].is_ascii_alphanumeric() || b[i] == b'_';
        let after_ident = k > 0 && ident(k - 1);
        // `r` opens a raw string on its own or after a `b` or `c` prefix, as in `br#"..."#`.
        let raw_start =
            !after_ident || (matches!(b[k - 1], b'b' | b'c') && (k < 2 || !ident(k - 2)));
        match b[k] {
            b'/' if b.get(k + 1) == Some(&b'/') => {
                let end = b[k..]
                    .iter()
                    .position(|c| *c == b'\n')
                    .map_or(b.len(), |n| k + n);
                Some((None, end))
            }
            b'/' if b.get(k + 1) == Some(&b'*') => {
                let end = (k + 2..b.len().saturating_sub(1))
                    .find(|&i| b[i] == b'*' && b[i + 1] == b'/')
                    .map_or(b.len(), |i| i + 2);
                Some((None, end))
            }
            b'r' if raw_start && matches!(b.get(k + 1), Some(b'"' | b'#')) => {
                let hashes = b[k + 1..].iter().take_while(|c| **c == b'#').count();
                if b.get(k + 1 + hashes) != Some(&b'"') {
                    return None;
                }
                let start = k + 2 + hashes;
                let close = (start..b.len())
                    .find(|&i| {
                        b[i] == b'"'
                            && b[i + 1..]
                                .iter()
                                .take(hashes)
                                .filter(|c| **c == b'#')
                                .count()
                                == hashes
                    })
                    .unwrap_or(b.len());
                Some((Some((start, close)), (close + 1 + hashes).min(b.len())))
            }
            b'"' => {
                let mut i = k + 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                let close = i.min(b.len());
                Some((Some((k + 1, close)), (close + 1).min(b.len())))
            }
            // An escape takes at least two bytes, as in '\'' or '\u{1F600}'.
            b'\'' if b.get(k + 1) == Some(&b'\\') => {
                let close = (k + 3..b.len()).find(|&i| b[i] == b'\'').unwrap_or(b.len());
                Some((None, (close + 1).min(b.len())))
            }
            // One character of one to four bytes; anything else is a lifetime or a label.
            b'\'' if k + 1 < b.len() => {
                let len = match b[k + 1] {
                    c if c < 0x80 => 1,
                    c if c >= 0xF0 => 4,
                    c if c >= 0xE0 => 3,
                    _ => 2,
                };
                (b.get(k + 1 + len) == Some(&b'\'')).then_some((None, k + 2 + len))
            }
            _ => None,
        }
    }

    /// Every string literal in `src`, with the line it starts on.
    fn string_literals(src: &str) -> Vec<(usize, String)> {
        let b = src.as_bytes();
        let (mut out, mut k) = (Vec::new(), 0);
        while k < b.len() {
            match literal_at(b, k) {
                Some((range, end)) => {
                    if let Some((s, e)) = range {
                        let line = 1 + b[..k].iter().filter(|c| **c == b'\n').count();
                        out.push((line, String::from_utf8_lossy(&b[s..e]).into_owned()));
                    }
                    k = end;
                }
                None => k += 1,
            }
        }
        out
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
