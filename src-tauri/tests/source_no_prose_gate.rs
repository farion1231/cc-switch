//! Executable gate for prose/diff contamination of `.rs` sources
//! (`src-tauri/src/**/*.rs` and `src-tauri/tests/**/*.rs`).
//!
//! Why a static gate at all: a commit once wrote its own commit description
//! *into* `src/remote/pairing/handlers.rs`, replacing 1073 lines of real
//! handler code with 10 lines of English prose and pasted diff fragments. The
//! compiler did catch it — but only as `error: unexpected closing delimiter`,
//! and only once someone happened to run `cargo check`. In the meantime the file
//! sat broken while other checks kept reporting on test assertions that could
//! never have run, because the crate did not build.
//!
//! So the failure this gate closes is not "prose in a .rs file" (the compiler
//! owns that). It is **"the crate stops building and the only signal is a
//! compiler error that a verification pass may not run"**. A gate is the thing
//! that goes red from a plain `cargo test --test source_no_prose_gate`, without
//! anyone having to notice a missing `ok` line somewhere else.
//!
//! Boundaries locked here:
//!
//! 1. `prose_lines` — no line of any `.rs` file may begin, after trimming, with
//!    one of the commit-description / diff markers, in **either** language the
//!    wreckage has been written in: `(only change:`, `Files changed:`, a bare
//!    `@@`, a diff's `-fn `, a `+++ b/` / `--- a/` header, or the Chinese
//!    `（改动` / `（整文件` / `FILE:` family. These are line-anchored because
//!    that is exactly how the contamination arrived: the marker is the first
//!    non-whitespace character of the line.
//! 2. `rust_shape_violations` — every `.rs` file must be recognisable as Rust:
//!    it starts at least one line with a Rust item head (`use `, `mod `,
//!    `fn `, `impl `, `#[derive`, …). A file holding only pasted fragments fails
//!    this even when no single marker trips rule 1.
//! 3. `diff_prefixed_fn_tokens` — the marker list in rule 1 is a fixed list, so
//!    it can be outgrown. This detector generalises it: a `fn` **word token**
//!    preceded by `-` or `+` is a diff line in any spelling, listed or not. In
//!    Rust a function name is never introduced by an arithmetic operator, so
//!    this has no false positive to speak of.
//! 4. `scan_surface_is_real` — the walk must actually reach files. A gate that
//!    silently scans zero files reports every detector as empty and reads
//!    green, which is the same class of failure as the one this file replaces.
//!
//! Two ways this gate has failed open shaped rules 1 and the scan surface:
//!
//! - One wave of wreckage was written in **Chinese**. `PROSE_MARKERS` was
//!   calibrated on the English wreckage and matched none of it, while rule 2
//!   passed on `remote.rs` because one surviving `pub fn apply_exposure_config(`
//!   line satisfied it. So a marker list in one language is a gate covering one
//!   language; [`CJK_PROSE_MARKERS`] exists for the same reason rule 3 does.
//! - That same wreckage landed in **`tests/`**, while the walk was pinned to
//!   `src/`. A gate that cannot see a file cannot fail on it, so "green" came
//!   to mean "the half nobody looked at is clean". [`scan_roots`] now walks
//!   both trees, and [`scan_surface_reaches_the_tests_dir`] proves the `tests/`
//!   root is genuinely reached rather than merely listed.
//!
//! A note on how rule 2 is phrased, because the obvious phrasing is wrong: the
//! stricter "must contain `fn ` **and** a `use `/`mod `" rule was tried first
//! and rejected — 17 legitimate files in this tree have no `fn ` definition at
//! all (`src/prompt.rs` is a bare struct, `src/commands/mod.rs` is a list of
//! `mod` declarations, `src/remote/html.rs` is one `pub const`). A gate that
//! reds on those teaches its readers to ignore it. Rule 2 is therefore an
//! existential over item heads, not a conjunction, and the coverage of all 244
//! files was checked before the rule was written down here.
//!
//! Comments **and literal contents** are blanked before rule 1 runs, by the
//! single pass in [`strip_non_code`]. Comments are obvious — a doc comment
//! *explaining* these markers must not trip the gate. Literals are the load-
//! bearing half: prose inside a string is data a program may legitimately print
//! (`"-fn "` as a negative fixture, `--- ` as a markdown ruler), and every
//! marker hit a `tests/` scan produces on a healthy tree is inside one. Without
//! this, extending the surface to `tests/` would have made the gate
//! permanently red — and the tempting response to that is to narrow the surface
//! again, which is how the hole gets reopened.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! machine-local path: the repo is cloned onto many machines and CI runners.

use std::fs;
use std::path::{Path, PathBuf};

/// `src-tauri/` — the two Rust trees this gate watches: the crate's own sources
/// under `src/`, and the integration-test gates under `tests/`.
///
/// `tests/` was added after a later instance of this failure. Wreckage landed
/// in `tests/remote_config_command_gate.rs`, and with the surface pinned to
/// `src/` the file was invisible here: the entire contents
/// of a deleted-by-prose integration gate — `（整文件删除：tauri::test::mock_builder…` —
/// passed a gate whose whole purpose is catching exactly that. A gate that
/// cannot see a file cannot fail on it, and "the gate is green" then reads as
/// "the tree is clean" while covering less than half of it.
fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The roots walked by [`scan_surface`], in report order.
fn scan_roots() -> [PathBuf; 2] {
    let root = crate_root();
    [root.join("src"), root.join("tests")]
}

/// Every `.rs` file under both scan roots.
///
/// Reported relative to `src-tauri/`, so a violation names `tests/foo.rs` and
/// not an absolute path — the report is read on machines other than the one
/// that produced it.
///
/// The `\` to `/` fold happens here, at the single point where the relative
/// label is built. Everything downstream (`contains("tests/x.rs")`,
/// `starts_with("src/")`) is written against POSIX separators, so a native
/// Windows label would match nothing and the containment assertions below
/// would fail on a walk that had in fact reached everything. Same spelling as
/// `privacy_gate::label`.
fn scan_surface() -> Vec<(String, PathBuf)> {
    let base = crate_root();
    let mut out = Vec::new();
    for dir in scan_roots() {
        for file in rust_sources(&dir) {
            let relative = file
                .strip_prefix(&base)
                .unwrap_or(file.as_path())
                .display()
                .to_string()
                .replace('\\', "/");
            out.push((relative, file));
        }
    }
    out
}

/// Line-anchored markers that mean "this file contains a commit message or a
/// pasted diff", not Rust.
///
/// `-fn ` and `@@` are the two that actually appeared in the first wreckage;
/// the others are the same family (a commit body, a diff file header) and cost
/// nothing to reject. Kept as data rather than as `if` chains so a new marker
/// is one line of addition and the negative fixtures below can enumerate them.
const PROSE_MARKERS: [&str; 5] = ["(only change:", "Files changed:", "@@", "-fn ", "+++ "];

/// One more marker that cannot be spelled inline in the array above without
/// reading like a diff itself, so it is matched separately.
const DIFF_REMOVED_HEADER: &str = "--- ";

/// Commit-description markers written in Chinese, as the rounds that produced
/// this gate actually wrote them.
///
/// The English list above was calibrated on the *first* wreckage and nothing
/// else. The later rounds pasted commit descriptions in Chinese — `（改动 1：新增
/// pub fn apply_exposure_config…`, `（整文件删除：tauri::test::mock_builder…`,
/// `FILE: src-tauri/Cargo.toml` — and every one of them sailed through, because
/// none of them begins with an English marker.
///
/// The full-width `（` is what makes these safe to match. Rust has no
/// line-opening syntax beginning with `（`, and comments and string literals are
/// blanked before this runs, so a genuine `（改动…` in code position can only be
/// contamination. The ASCII-capable entry `FILE:` is listed here for the same
/// reason it is listed above: it is a commit-description section header, and no
/// Rust line opens with it.
const CJK_PROSE_MARKERS: [&str; 6] = ["（改动", "（整文件", "（新增", "（删除", "（修改", "FILE:"];

/// One more CJK marker that cannot be spelled inline without reading like the
/// thing it detects, so it is matched separately.
const CJK_WHOLE_FILE_MARKER: &str = "（整文件";

/// Recursively collect every `.rs` file under `dir`, stably sorted.
///
/// Sorting is not fastidiousness: violation reports are printed in file order,
/// so an unsorted walk makes two runs over the same tree print different
/// reports and there is no way to compare them line by line.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("无法读取 {}：{e}", current.display()));
        for entry in entries {
            let path = entry.expect("read_dir 条目不可读").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Blanking out everything that is not code
// ---------------------------------------------------------------------------

/// What the scanner is currently inside.
enum Scan {
    /// Rust code — the only state whose bytes survive.
    Code,
    /// `// …` up to (but not including) the newline.
    LineComment,
    /// `/* … */`, carrying the nesting depth.
    BlockComment(usize),
    /// `"…"`, with backslash escapes.
    Str,
    /// `r"…"`, `r#"…"#`, `r##"…"##`, …, carrying the hash count.
    RawStr(usize),
}

/// Whether `r` at `i` starts a raw string rather than being part of a name.
///
/// `let r = 1` and `var` must not be read as `r` + quote. The byte before must
/// not be an identifier character.
fn raw_string_start(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'r') {
        return None;
    }
    if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    (b.get(j) == Some(&b'"')).then_some(j - (i + 1))
}

/// Length in bytes of the character literal at `i`, or `None` if the `'` there
/// is not one.
///
/// The lifetimes in `&'static str` are the reason this needs a rule rather than
/// a scan: `'` is not reliably a literal opener. A literal is `'x'`, `'\\n'`,
/// `'\\u{1F600}'` — a short, *closed* shape — so the test is "does a closing
/// quote follow within a couple of bytes". `'s` in `&'static str` has no closing
/// quote after it and is correctly left alone.
fn char_literal_len(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    match b.get(j) {
        Some(b'\\') => {
            j += 1;
            if b.get(j) == Some(&b'u') && b.get(j + 1) == Some(&b'{') {
                while j < b.len() && b[j] != b'}' {
                    j += 1;
                }
                j += 1;
            } else {
                j += 1;
            }
        }
        Some(c) if *c != b'\n' => j += 1,
        _ => return None,
    }
    (b.get(j) == Some(&b'\'')).then_some(j + 1 - i)
}

/// Emit `c` as blank, except newlines, which are kept so line numbers survive.
fn blank(out: &mut Vec<u8>, c: u8) {
    out.push(if c == b'\n' { b'\n' } else { b' ' });
}

/// Replace everything that is not code — comments, and the *contents* of string,
/// character and raw-string literals — with spaces, keeping every newline so
/// `code_lines` can still report true 1-based source line numbers.
///
/// Literal *delimiters* survive; only what is between them is blanked. That is
/// what keeps the scan honest rather than merely quiet: prose inside a string is
/// data the program may legitimately print (`"-fn "` as a fixture, `--- ` as a
/// markdown ruler), while prose in code position is contamination.
///
/// **One pass, not two.** Comments and literals are mutually dependent — a `//`
/// inside a string is not a comment, and a `"` inside a comment is not a string.
/// Two sequential passes let either one misread the other's syntax, and the
/// failure is silent and severe: one swallowed quote unbalances every literal
/// after it, blanking real code and hiding the violations behind it. A single
/// state machine has no such seam.
fn strip_non_code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    let mut state = Scan::Code;

    while i < b.len() {
        let c = b[i];
        match state {
            Scan::Code => {
                if c == b'/' && b.get(i + 1) == Some(&b'/') {
                    state = Scan::LineComment;
                    out.extend_from_slice(b"  ");
                    i += 2;
                } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
                    state = Scan::BlockComment(1);
                    out.extend_from_slice(b"  ");
                    i += 2;
                } else if let Some(hashes) = raw_string_start(b, i) {
                    state = Scan::RawStr(hashes);
                    out.push(b'r');
                    out.extend(std::iter::repeat_n(b'#', hashes));
                    out.push(b'"');
                    i += hashes + 2;
                } else if c == b'"' {
                    state = Scan::Str;
                    out.push(b'"');
                    i += 1;
                } else if c == b'\'' {
                    match char_literal_len(b, i) {
                        Some(len) => {
                            // Blanked wholesale: the character itself is data.
                            for k in 0..len {
                                blank(&mut out, b[i + k]);
                            }
                            i += len;
                        }
                        None => {
                            out.push(b'\'');
                            i += 1;
                        }
                    }
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            Scan::LineComment => {
                if c == b'\n' {
                    out.push(b'\n');
                    state = Scan::Code;
                } else {
                    blank(&mut out, c);
                }
                i += 1;
            }
            Scan::BlockComment(depth) => {
                if c == b'/' && b.get(i + 1) == Some(&b'*') {
                    out.extend_from_slice(b"  ");
                    state = Scan::BlockComment(depth + 1);
                    i += 2;
                } else if c == b'*' && b.get(i + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    state = if depth <= 1 {
                        Scan::Code
                    } else {
                        Scan::BlockComment(depth - 1)
                    };
                    i += 2;
                } else {
                    blank(&mut out, c);
                    i += 1;
                }
            }
            Scan::Str => {
                if c == b'\\' {
                    blank(&mut out, c);
                    if let Some(next) = b.get(i + 1) {
                        blank(&mut out, *next);
                    }
                    i += 2;
                } else if c == b'"' {
                    out.push(b'"');
                    state = Scan::Code;
                    i += 1;
                } else {
                    blank(&mut out, c);
                    i += 1;
                }
            }
            Scan::RawStr(hashes) => {
                if c == b'"' {
                    // The literal ends only if exactly `hashes` `#` follow;
                    // `r#"a"#b"#` ends at the second quote, not the first.
                    let mut n = 0usize;
                    while b.get(i + 1 + n) == Some(&b'#') {
                        n += 1;
                    }
                    if n == hashes {
                        out.push(b'"');
                        out.extend(std::iter::repeat_n(b'#', hashes));
                        state = Scan::Code;
                        i += 1 + hashes;
                    } else {
                        blank(&mut out, c);
                        i += 1;
                    }
                } else {
                    blank(&mut out, c);
                    i += 1;
                }
            }
        }
    }

    // Every kept byte was copied verbatim from valid UTF-8, so this cannot fail;
    // falling back to the input would be worse than losing the blanking, so it
    // never happens in practice.
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// One offending line, reported as `path:lineno: the line` so a failure is
/// actionable without opening anything.
struct Violation {
    location: String,
    detail: String,
}

/// Code-only lines of a `.rs` file, paired with 1-based source line numbers.
///
/// Blank lines are dropped: an empty line cannot begin with a marker, so keeping
/// them would only inflate the vector. Line numbers are the *original* ones,
/// because [`strip_non_code`] preserves every newline.
fn code_lines(src: &str) -> Vec<(usize, String)> {
    strip_non_code(src)
        .lines()
        .enumerate()
        .filter_map(|(idx, raw)| {
            let code = raw.trim();
            (!code.is_empty()).then(|| (idx + 1, code.to_string()))
        })
        .collect()
}

/// Every line that opens with a commit-description or diff marker, in either
/// language the wreckage has been written in.
///
/// Line-anchored on purpose. A prose marker buried mid-line is a comment-like
/// string in real code; a prose marker *opening* the line is the shape the
/// wreckage had, and that is what this gate is calibrated to catch.
fn prose_lines(src: &str) -> Vec<Violation> {
    code_lines(src)
        .into_iter()
        .filter(|(_, code)| {
            PROSE_MARKERS.iter().any(|marker| code.starts_with(marker))
                || CJK_PROSE_MARKERS
                    .iter()
                    .any(|marker| code.starts_with(marker))
                || code.starts_with(DIFF_REMOVED_HEADER)
                || code.starts_with(CJK_WHOLE_FILE_MARKER)
        })
        .map(|(line, code)| Violation {
            location: format!("line {line}"),
            detail: code.to_string(),
        })
        .collect()
}

/// Line-starting tokens that open a Rust item or attribute.
///
/// An **existential** over this set, not a conjunction — see the note in the
/// module header for why the stricter phrasing was rejected. The set is a
/// superset of the minimum that covers all 244 files in `src/`, deliberately:
/// a gate should fail on contamination, not on a spelling choice.
const ITEM_HEADS: [&str; 19] = [
    "use ",
    "pub use ",
    "mod ",
    "pub mod ",
    "pub(crate) mod ",
    "fn ",
    "pub fn ",
    "pub(crate) fn ",
    "async fn ",
    "unsafe fn ",
    "impl ",
    "struct ",
    "pub struct ",
    "enum ",
    "pub enum ",
    "const ",
    "pub const ",
    "static ",
    "type ",
];

/// Attributes count as item heads too: `#[derive]` opens a struct and `#[cfg]`
/// opens an item. Listed separately so a shorter attribute prefix
/// (`#[allow(`, `#[test]`) does not widen the set.
const ATTRIBUTE_HEADS: [&str; 2] = ["#[derive", "#[cfg"];

/// Whether `src` starts at least one line with a Rust item head — i.e. whether
/// it is recognisable as Rust at all.
fn has_rust_item_head(src: &str) -> bool {
    code_lines(src).into_iter().any(|(_, code)| {
        ITEM_HEADS
            .iter()
            .chain(ATTRIBUTE_HEADS.iter())
            .any(|head| code.starts_with(head))
    })
}

/// `fn` word tokens immediately preceded by a diff operator.
///
/// The generalisation of `-fn ` in [`PROSE_MARKERS`]: a fixed list can be
/// outgrown by whatever the next agent pastes in, but the *shape* of a diff
/// line — an operator glued to the front of a keyword — does not vary. In Rust
/// a function name is introduced by whitespace, `(`, `,` or `&`, never by `-`
/// or `+`, so this has no false positive available to it.
fn diff_prefixed_fn_tokens(src: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    for (line, code) in code_lines(src) {
        let bytes = code.as_bytes();
        let mut from = 0usize;
        while let Some(rel) = code[from..].find("fn") {
            let at = from + rel;
            let before_ok = code[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            let after = at + 2;
            let after_ok = bytes
                .get(after)
                .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_');
            // The byte immediately before `fn` must be the diff operator itself,
            // with nothing between — `a - fn_value` is arithmetic and must not be
            // swept in, while a real `fn` is introduced by whitespace, `pub`,
            // `(`, `,` or `&`. `checked_sub` keeps a leading `fn` (whose `at` is
            // 0) from reading out of bounds.
            let prefix = at.checked_sub(1).map(|i| bytes[i]);
            if before_ok && after_ok && matches!(prefix, Some(b'-') | Some(b'+')) {
                out.push(Violation {
                    location: format!("line {line}"),
                    detail: format!(
                        "`{}` — a `fn` keyword glued to a diff operator is a pasted \
                         diff line, not Rust",
                        code
                    ),
                });
            }
            from = at + 2;
        }
    }
    out
}

/// Violations of the Rust-shape rule, one entry per offending file.
fn rust_shape_violations(src: &str) -> Vec<Violation> {
    if has_rust_item_head(src) {
        Vec::new()
    } else {
        vec![Violation {
            location: "whole file".to_string(),
            detail: "must open at least one line with a Rust item head (`use `, \
                     `mod `, `fn `, `impl `, `#[derive`, …) to be recognisable as \
                     Rust"
                .to_string(),
        }]
    }
}

// ---------------------------------------------------------------------------
// Tests: the real tree
// ---------------------------------------------------------------------------

#[test]
fn no_rust_source_contains_prose_or_diff_markers() {
    let surface = scan_surface();
    assert!(
        !surface.is_empty(),
        "the walk over {} found no .rs files — the scan surface has degraded \
         and every detector below would read as green",
        crate_root().display()
    );

    let mut offenders: Vec<String> = Vec::new();
    for (relative, file) in &surface {
        let src = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", file.display()));

        for violation in prose_lines(&src) {
            offenders.push(format!(
                "{relative}: {}: {}",
                violation.location, violation.detail
            ));
        }
        for violation in diff_prefixed_fn_tokens(&src) {
            offenders.push(format!(
                "{relative}: {}: {}",
                violation.location, violation.detail
            ));
        }
        for violation in rust_shape_violations(&src) {
            offenders.push(format!(
                "{relative}: {}: {}",
                violation.location, violation.detail
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "a .rs file may not contain commit-description prose or pasted diff \
         fragments. A commit once shipped exactly that in \
         src/remote/pairing/handlers.rs and it went undetected for a while, \
         because the only signal was a compiler error nobody was looking for. \
         Offending lines: {offenders:#?}"
    );
}

/// The scan surface must be real and must include the files that actually broke.
///
/// A gate whose walk silently degraded to zero files would report every
/// detector as empty and read green — the same failure mode as the empty
/// static gate this file exists to follow. So the surface is asserted to be
/// non-empty and to contain the exact files that were destroyed.
#[test]
fn scan_surface_reaches_the_file_that_broke() {
    let surface = scan_surface();
    let paths: Vec<&PathBuf> = surface.iter().map(|(_, path)| path).collect();
    let root = crate_root();

    assert!(
        paths.len() > 1,
        "the walk must reach more than one file: it found {}",
        paths.len()
    );

    let pairing_handlers = root.join("src/remote/pairing/handlers.rs");
    assert!(
        paths.contains(&&pairing_handlers),
        "the walk must reach {} — that is the file a commit replaced with \
         prose; if it is missing from the scan surface this gate is not \
         watching the case it was written for",
        pairing_handlers.display()
    );

    // And it must be readable and non-trivial: the wrecked version was 10 lines.
    let src = fs::read_to_string(&pairing_handlers).expect("pairing handlers readable");
    assert!(
        src.lines().count() > 100,
        "src/remote/pairing/handlers.rs is {} lines — the four pairing endpoints \
         do not fit in that; the file has been truncated again",
        src.lines().count()
    );
}

/// The scan surface must include the integration-test tree, not just `src/`.
///
/// This is the hole that opened once the contamination moved into `tests/`. A
/// commit description was written into `tests/remote_config_command_gate.rs` —
/// and the gate read green, because `source_root()` was `src-tauri/src` and the
/// file was not on the walk at all. A gate that cannot see a file cannot fail on
/// it; "green" then means "the half nobody looked at is clean".
///
/// Asserted as a *containment* property against real files on disk rather than
/// against the roots list, because a root that was added but never walked looks
/// identical to a root that was added and walked.
#[test]
fn scan_surface_reaches_the_tests_dir() {
    let surface = scan_surface();
    let relative: Vec<&str> = surface.iter().map(|(rel, _)| rel.as_str()).collect();

    // Some real gates in tests/ must be on the walk.
    for expected in [
        "tests/auth_token_gate.rs",
        "tests/source_no_prose_gate.rs",
        "tests/fmt_gate.rs",
    ] {
        assert!(
            relative.contains(&expected),
            "the walk must reach {expected}: it is an integration gate whose own \
             prose would go unreported. Files on the walk: {relative:?}"
        );
    }

    // And the src/ tree must still be covered — extending the surface must not
    // have been done by swapping one root for another.
    assert!(
        relative.iter().any(|rel| rel.starts_with("src/")),
        "the src/ tree has dropped off the scan surface entirely"
    );

    let tests_count = relative.iter().filter(|r| r.starts_with("tests/")).count();
    assert!(
        tests_count > 5,
        "only {tests_count} files under tests/ are on the walk — the surface is \
         nominally extended but is not actually reaching the directory"
    );
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof the gate can go red
// ---------------------------------------------------------------------------

/// Fixture A — the original wreckage, verbatim in shape.
///
/// Without this, a detector that always returns empty leaves the gate green
/// while protecting nothing.
#[test]
fn gate_rejects_the_wrecked_handlers_fixture() {
    let fixture = r#"(only change: three dead lines removed at former line 532)

     .route("/pairing/revoke", post(pairing_revoke))
     .with_state(state)
 }

-fn PLANTED(a: ClientDigest, b: ClientDigest) -> bool {
-    a == b
-}
#[cfg(test)]
mod tests {
"#;

    let lines = prose_lines(fixture);
    let joined: Vec<String> = lines
        .iter()
        .map(|v| format!("{}: {}", v.location, v.detail))
        .collect();
    assert_eq!(
        joined,
        vec![
            "line 1: (only change: three dead lines removed at former line 532)".to_string(),
            "line 7: -fn PLANTED(a: ClientDigest, b: ClientDigest) -> bool {".to_string(),
        ],
        "both prose markers must be reported, each at its own line number — and \
         the stray brace on line 5 is *not* one, because a brace alone is not a \
         marker"
    );

    assert_eq!(
        diff_prefixed_fn_tokens(fixture)
            .iter()
            .map(|v| v.location.clone())
            .collect::<Vec<_>>(),
        vec!["line 7".to_string()],
        "the `-fn` token must be caught by the shape generalisation as well as by \
         the fixed marker list — a list alone can be outgrown"
    );

    assert!(
        has_rust_item_head(fixture),
        "the wreckage's last two lines are `#[cfg(test)]` and a `mod` declaration, which \
         *do* open items — so the item-head rule is honestly expected to pass here, \
         and rules 1 and 3 are what catch it. Recording this so nobody later reads \
         the item-head rule as a detector this file depends on."
    );
}

/// Fixture A' — a wreckage with **no** marker on any line, so the item-head rule
/// has to be what rejects it.
///
/// The original wreckage happens to survive rule 2, because its last two lines
/// are a `#[cfg]` attribute and a `mod`. That is luck, not coverage: a slightly
/// different paste would not have it. This fixture is the case rule 2 exists for.
#[test]
fn gate_rejects_marker_free_non_rust_fixture() {
    let fixture = r#"
         .route("/pairing/revoke", post(pairing_revoke))
         .with_state(state)
 }
"#;
    assert!(
        prose_lines(fixture).is_empty(),
        "no line starts with a marker here — this fixture isolates rule 2"
    );
    assert!(
        diff_prefixed_fn_tokens(fixture).is_empty(),
        "and there is no diff-prefixed `fn` either — so rule 2 is the only rule \
         standing, which is the point"
    );
    assert!(
        !has_rust_item_head(fixture),
        "a file made of a stray brace and half a route registration opens no item"
    );
    assert_eq!(
        rust_shape_violations(fixture).len(),
        1,
        "the shape violation must be reported once, for the file"
    );

    // `fn` glued into an identifier is not an item head.
    let glued = "xfn_ptr_field();\nroute_table.len();\n";
    assert!(
        !has_rust_item_head(glued),
        "`xfn_ptr_field()` is not a `fn ` item head"
    );

    // Every head in the list actually opens an item — enumerated so a typo in
    // the list cannot hide behind a real file elsewhere.
    for head in ITEM_HEADS.iter().chain(ATTRIBUTE_HEADS.iter()) {
        let src = format!("{head}whatever\n");
        assert!(
            has_rust_item_head(&src),
            "`{head}` is declared as an item head but does not match itself"
        );
    }

    // The shapes that actually exist in this tree must pass. These are the files
    // that made the stricter "needs a `fn `" phrasing unusable: a bare struct, a
    // bare `mod` list, and a single `pub const`.
    let bare_struct = "use serde::Serialize;\n\n#[derive(Serialize)]\npub struct Prompt {\n    pub id: String,\n}\n";
    assert!(
        has_rust_item_head(bare_struct),
        "a file of struct definitions has no `fn ` and must still pass"
    );
    let bare_mod_list = "mod auth;\nmod balance;\npub mod remote;\n";
    assert!(
        has_rust_item_head(bare_mod_list),
        "a file of `mod` declarations has no `fn ` and must still pass"
    );
    let bare_const = "pub const REMOTE_HTML: &str = \"<html></html>\";\n";
    assert!(
        has_rust_item_head(bare_const),
        "a file that is one `pub const` has no `fn ` and must still pass"
    );
}

/// Fixture B — each marker, individually, and the clean shape.
///
/// A gate that rejects the whole wreckage proves only that it noticed the
/// wreckage; it does not prove the *rule* is what rejected it. So each marker
/// is checked on its own, and a real Rust file is checked to produce nothing.
#[test]
fn gate_rejects_each_marker_individually() {
    let mut detected = 0usize;
    for marker in PROSE_MARKERS
        .iter()
        .copied()
        .chain(CJK_PROSE_MARKERS.iter().copied())
        .chain([DIFF_REMOVED_HEADER, CJK_WHOLE_FILE_MARKER])
    {
        let src = format!("{marker} trailing text\nfn noop() {{}}\n");
        assert_eq!(
            prose_lines(&src).len(),
            1,
            "`{marker}` must be reported as a prose marker on its own"
        );
        detected += 1;
    }
    assert_eq!(
        detected,
        PROSE_MARKERS.len() + CJK_PROSE_MARKERS.len() + 2,
        "every declared marker must have been exercised"
    );

    // Indentation must not hide a marker: the wreckage's markers were indented.
    assert_eq!(
        prose_lines("        -fn PLANTED(a: u8) -> bool {\n").len(),
        1,
        "an indented marker is still a marker — line-anchored means anchored on \
         the first non-whitespace character"
    );

    // A real module produces nothing.
    let clean = r#"
use std::path::PathBuf;

fn handler() -> PathBuf {
    PathBuf::new()
}
"#;
    assert!(
        prose_lines(clean).is_empty(),
        "an ordinary module produces no prose markers"
    );
    assert!(
        has_rust_item_head(clean),
        "an ordinary module has the Rust shape"
    );

    // A doc comment *explaining* the markers is not contamination.
    let documented = r#"
//! Reject lines beginning with `-fn ` or `@@`, which mean a pasted diff.
//! Note: this comment mentions `Files changed:` on purpose.
use std::path::PathBuf;

fn handler() -> PathBuf {
    PathBuf::new()
}
"#;
    assert!(
        prose_lines(documented).is_empty(),
        "a comment naming the markers must not trip the gate — comments are \
         stripped first, and a comment cannot break the build"
    );
    assert!(
        has_rust_item_head(documented),
        "comment stripping must not remove the `use `/`fn ` that give the file \
         its shape"
    );

    // A `//` inside a string literal is not a comment start.
    assert!(
        prose_lines("fn url() -> &'static str { \"https://example.invalid/@@\" }\n").is_empty(),
        "`//` inside a string literal must not hide a following marker from the \
         scanner's comment handling"
    );

    // And rule 3 must not fire on the ordinary ways Rust writes a function.
    for honest in [
        "fn handler() -> u8 {\n    0\n}\n",
        "pub fn handler() -> u8 {\n    0\n}\n",
        "async fn handler() {}\n",
        "unsafe fn handler() {}\n",
        "let f: fn(u8) -> u8 = |x| x;\n",
        "let g: &fn() = &noop;\n",
    ] {
        assert!(
            diff_prefixed_fn_tokens(honest).is_empty(),
            "`{}` is ordinary Rust and has no diff-prefixed `fn`",
            honest.trim()
        );
    }
    assert_eq!(
        diff_prefixed_fn_tokens("+fn added() -> u8 {\n").len(),
        1,
        "`+fn` is the other half of a unified diff and must be caught by the same \
         generalisation — this is the case a fixed `-fn ` marker alone would miss"
    );
}

// ---------------------------------------------------------------------------
// The Chinese commit-description wreckage, verbatim in shape
// ---------------------------------------------------------------------------

/// Fixture C — wreckage written in Chinese, which the English marker list
/// missed entirely.
///
/// A commit and its successors wrote their commit descriptions into
/// `src/commands/remote.rs` and `tests/remote_config_command_gate.rs`. The whole
/// of `remote.rs` became lines like `（改动 1：新增 pub fn apply_exposure_config…）`
/// and `FILE: src-tauri/Cargo.toml`; the whole of the gate file became
/// `（整文件删除：tauri::test::mock_builder…）`. Not one begins with `(only
/// change:`, `-fn ` or `@@`.
///
/// Worse, both files *passed* this gate — and not by accident. Rule 2 requires
/// an opening Rust item head, and the wreckage happened to retain exactly one
/// surviving line, `pub fn apply_exposure_config(`. So the file satisfied the
/// shape rule by luck, and the marker rule had no entry in the right language.
/// A gate whose every rule misses is indistinguishable from a clean tree.
#[test]
fn prose_gate_rejects_cjk_commit_description() {
    // The shape of `src/commands/remote.rs` as committed.
    let remote_rs = "\
（改动 1：新增 pub fn apply_exposure_config，集中收敛暴露配置）
（改动 2：tests 模块顶部新增 gate 用例）
FILE: src-tauri/Cargo.toml
";

    let violations = prose_lines(remote_rs);
    let joined: Vec<String> = violations
        .iter()
        .map(|v| format!("{}: {}", v.location, v.detail))
        .collect();

    assert_eq!(
        joined.len(),
        3,
        "all three commit-description lines must be reported. Reported: {joined:?}"
    );
    assert!(
        joined[0].starts_with("line 1: （改动 1："),
        "the first line must be reported at its own line number: {}",
        joined[0]
    );
    assert!(
        joined[1].starts_with("line 2: （改动 2："),
        "the second line must be reported at its own line number: {}",
        joined[1]
    );
    assert!(
        joined[2].starts_with("line 3: FILE:"),
        "the commit-description file header must be reported too: {}",
        joined[2]
    );

    // The shape of the deleted `tests/remote_config_command_gate.rs`.
    let gate_rs = "（整文件删除：tauri::test::mock_builder…）\n";
    assert_eq!(
        prose_lines(gate_rs).len(),
        1,
        "a file whose entire contents are one Chinese deletion note must be \
         reported — this is the whole of the wrecked gate file"
    );

    // And the rule that let it through: the lone surviving `pub fn` line is an
    // item head, so rule 2 passes on this wreckage. Recorded so nobody later
    // mistakes rule 2 for a defence here.
    let with_lone_fn = "\
（改动 1：新增 pub fn apply_exposure_config）
pub fn apply_exposure_config(
";
    assert!(
        has_rust_item_head(with_lone_fn),
        "the surviving `pub fn` line genuinely satisfies the item-head rule — \
         which is exactly why rule 2 could not have caught this wreckage and \
         rules 1 and 4 had to"
    );
}

/// Rule 1 and rule 3 must both work on the Chinese wreckage, not just rule 1.
#[test]
fn cjk_wreckage_is_caught_by_shape_generalisation_too() {
    let fixture = "（改动 1：新增 handler）\n-fn PLANTED(a: u8) -> bool {\n";

    assert_eq!(
        prose_lines(fixture).len(),
        2,
        "both lines are prose markers: the Chinese description line *and* the \
         pasted `-fn` body beneath it"
    );
    assert_eq!(
        diff_prefixed_fn_tokens(fixture)
            .iter()
            .map(|v| v.location.clone())
            .collect::<Vec<_>>(),
        vec!["line 2".to_string()],
        "the pasted `-fn` body is caught by the diff-operator shape \
         generalisation, which is language-independent"
    );
}

// ---------------------------------------------------------------------------
// Blank-before-scan: the load-bearing primitive behind the extended surface
// ---------------------------------------------------------------------------

/// Extending the scan to `tests/` only works if prose *inside a literal* is not
/// mistaken for prose in code.
///
/// This is not hypothetical. Every one of the marker hits that a `tests/`
/// scan produces on a healthy tree is inside a string: the gates' own negative
/// fixtures hold `"-fn PLANTED…"`, `fmt_gate`'s report format holds `--- cargo
/// fmt --check output ---`, and a multi-line `format!` string puts `--- ` at the
/// start of a *physical* line. Without blanking literal contents, extending the
/// surface makes the gate permanently red — and the tempting "fix" is to narrow
/// the surface again, which is how the hole gets reopened.
#[test]
fn prose_inside_literals_is_not_misread_as_code() {
    // A fixture string holding a diff marker — the shape this very file uses.
    let in_plain_string = "let fixture = \"-fn PLANTED(a: u8) -> bool {\\n\";\n";
    assert!(
        prose_lines(in_plain_string).is_empty(),
        "a marker inside a string literal is data the program may print"
    );
    assert!(
        diff_prefixed_fn_tokens(in_plain_string).is_empty(),
        "and the diff-operator rule must not see it either"
    );

    // The same marker in code position must still be caught — blanking must not
    // have become "return empty always".
    assert_eq!(
        prose_lines("-fn PLANTED(a: u8) -> bool {\n").len(),
        1,
        "the very same line in code position must still be a violation"
    );

    // A raw string spanning several lines: every line is literal content.
    let raw =
        "let f = r#\"line one\n-fn PLANTED(a: u8) -> bool {\nline three\n\"#;\nfn real() {}\n";
    assert!(
        prose_lines(raw).is_empty(),
        "every line of a multi-line raw string is literal content, including \
         the middle one"
    );
    assert!(
        diff_prefixed_fn_tokens(raw).is_empty(),
        "and the shape rule must not reach inside it either"
    );

    // Code after a multi-line raw string must still be scanned.
    let raw_then_wreck = "let f = r#\"\nnot code\n\"#;\n（改动 1：新增 handler）\n";
    assert_eq!(
        prose_lines(raw_then_wreck).len(),
        1,
        "the scanner must return to code position after a raw string closes, \
         or extending the surface would blank the rest of every file"
    );

    // `//` inside a string must not start a comment and unbalance everything
    // after it. This is the failure mode that motivated the single-pass design.
    let url_then_code = "let u = \"https://example.invalid/@@\";\n（改动 1：新增 handler）\n";
    assert_eq!(
        prose_lines(url_then_code).len(),
        1,
        "a `//` inside a string literal must not blank the rest of the file"
    );

    // A block comment is blanked too.
    let block = "/*\n-fn PLANTED(a: u8) -> bool {\n*/\nfn real() {}\n";
    assert!(
        prose_lines(block).is_empty(),
        "a marker inside a block comment is a comment, not code"
    );
    assert!(
        has_rust_item_head(block),
        "and comment blanking must not hide the real `fn ` that follows it"
    );

    // A lifetime is not a character literal. Getting this wrong blanks the rest
    // of the line, or worse the rest of the file.
    let lifetime = "fn f(s: &'static str) -> &'static str { s }\n（改动 1：新增 handler）\n";
    assert_eq!(
        prose_lines(lifetime).len(),
        1,
        "`&'static str` is a lifetime, not an unterminated char literal — \
         misreading it would swallow everything after it"
    );

    // A real char literal, by contrast, is data and is blanked.
    assert!(
        prose_lines("let c = '（';\nfn real() {}\n").is_empty(),
        "a character literal is data, and must not open a string that blanks \
         the remainder of the file"
    );
}

/// Line numbers must survive blanking, or every report points at the wrong line.
#[test]
fn blanking_preserves_source_line_numbers() {
    let src = "fn a() {}\n\nlet s = \"multi\nline\nstring\";\n（改动 1：新增 handler）\n";

    let violations = prose_lines(src);
    assert_eq!(
        violations.len(),
        1,
        "exactly the one Chinese marker line should be reported"
    );
    assert_eq!(
        violations[0].location, "line 6",
        "the marker is on source line 6 — the three-line string literal above \
         it occupies lines 3 to 5; a blanking pass that dropped or collapsed \
         newlines would report a different line and send the reader to the \
         wrong place"
    );
}

/// Every marker this gate declares must be reachable, including the Chinese
/// ones — a marker list that is longer than the list actually matched is a gate
/// quietly protecting nothing.
#[test]
fn every_declared_marker_is_exercised() {
    let mut seen = 0usize;
    for marker in CJK_PROSE_MARKERS.iter() {
        let src = format!("{marker} rest of the description\n");
        assert_eq!(
            prose_lines(&src).len(),
            1,
            "declared CJK marker `{marker}` is not matched — it is decoration, \
             not a rule"
        );
        seen += 1;
    }
    assert_eq!(
        seen,
        CJK_PROSE_MARKERS.len(),
        "every declared CJK marker must have been exercised"
    );

    // A Chinese marker buried mid-line is not a marker — the rule is
    // line-anchored, same as the English ones. Real code may legitimately hold
    // a `（改动…` inside a nested expression.
    assert!(
        prose_lines("let note = \"（改动 1：…）\";\n").is_empty(),
        "line-anchored means anchored on the first character; a mid-line \
         occurrence is not a marker"
    );
}
