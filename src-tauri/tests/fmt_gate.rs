//! Task 3-3-3 — the format gate standing alone as its own falsifiable fact.
//!
//! Earlier rounds folded "the tests pass" and "the tree is rustfmt-clean" into
//! a single judgement. A failing `cargo test` (exit 101) then buried a passing
//! `cargo fmt --check` (exit 0), so a green gate read as red in the report.
//! This file makes the format fact independently executable and independently
//! readable: [`fmt_check_clean_under_sh`] asserts exactly one thing — that
//! `cargo fmt --check` exits 0 — and asserts nothing whatsoever about test
//! outcomes, port conflicts, or the two deliberately-red gates. Those are
//! separate facts with separate evidence, deliberately not wired together here.
//!
//! Boundary: this gate never reorders anyone else's code. If it ever goes red,
//! that is a finding to report, not a licence to run `cargo fmt` across the
//! tree — reformatting files outside this task would smuggle unrelated changes
//! into the deletion-irrelevance proof task 3-3-2 established.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never at a literal
//! machine-local path, and the command runs through `/bin/sh` with an explicit
//! `PATH=` prefix so it survives the harness's `/bin/sh -c` invocation (a bare
//! `cargo` under that shell would exit 127 rather than checking anything).

use std::path::Path;
use std::process::Command;

/// Extract the violation headings from `cargo fmt --check` output.
///
/// rustfmt emits one `Diff in <path>:<line>:` header per file-region that
/// differs, followed by the unified diff itself. Only the headers count as
/// violations; diff body lines (`-old`, `+new`, ` `, `@@`) must not be
/// mistaken for them, and non-diff chatter such as `error:` must be ignored
/// so a cargo-level failure cannot masquerade as a formatting one.
fn fmt_violations(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("Diff in ")?;
            rest.strip_suffix(':')
                .filter(|heading| !heading.is_empty())
                .map(|heading| heading.to_string())
        })
        .collect()
}

/// The failure message this gate reports, with the parsed violations named.
///
/// Before this existed, [`fmt_check_clean_under_sh`] pasted `output` — the raw
/// unified diff — into its panic and nothing else. That output *contains* the
/// `Diff in <path>:<line>:` headings, so nothing was strictly missing… except
/// that the heading has to be found by eye, several screens into a diff full of
/// `+`/`-`/`@@` lines that look equally structural. A reader who hits this gate
/// is looking at a red build and a wall of diff; the one thing they need is the
/// list of `file:line` to open.
///
/// Parsing the headings *before* asserting means the report and the evidence
/// come from the same [`fmt_violations`] the negative fixtures below pin down —
/// a separate ad-hoc scan could drift from it and quietly report nothing.
fn fmt_failure_message(output: &str) -> String {
    let violations = fmt_violations(output);

    if violations.is_empty() {
        // Either cargo failed before rustfmt ran, or the output shape changed.
        // Saying so is more honest than reporting an empty violation list,
        // which would read as "nothing to fix".
        return format!(
            "cargo fmt --check must exit 0 on its own, but exited non-zero with \
             no `Diff in <path>:<line>:` heading in its output — cargo may have \
             failed before rustfmt ran, or the heading shape changed. If the \
             gate is red, report the offending files — do NOT run `cargo fmt` \
             over the tree to force this green, that would mix unrelated \
             reformatting into the deletion-irrelevance proof.\n\
             --- cargo fmt --check output ---\n{output}"
        );
    }

    format!(
        "cargo fmt --check must exit 0 on its own. If it does not, report the \
         offending files — do NOT run `cargo fmt` over the tree to force this \
         green, that would mix unrelated reformatting into the deletion \
         irrelevance proof.\n\
         --- {} offending region(s) ---\n{}\n\
         --- cargo fmt --check output ---\n{output}",
        violations.len(),
        violations
            .iter()
            .map(|v| format!("  {v}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The cargo binary that is building this very test, as an absolute path.
///
/// Cargo exports `CARGO` to the build, so this never depends on the caller's
/// `PATH` — which is what lets the command below stay bare-`cargo` while still
/// resolving. Only its parent directory is borrowed, to seed `PATH=`.
fn cargo_bin() -> &'static str {
    env!("CARGO")
}

/// Run `cargo fmt --manifest-path <crate>/Cargo.toml --check` through
/// `/bin/sh`, returning the exit code and the combined output.
fn run_fmt_check() -> (i32, String) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let cargo_dir = Path::new(cargo_bin())
        .parent()
        .expect("CARGO must have a parent directory")
        .display()
        .to_string();

    // `$PATH` is expanded by the shell, not by us; the literal `$PATH` here is
    // deliberate. The quotes keep a path containing spaces intact.
    let script = format!(
        "PATH=\"{cargo_dir}:$PATH\" cargo fmt --manifest-path {} --check",
        manifest.display()
    );

    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .expect("running /bin/sh must succeed");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    (out.status.code().unwrap_or(-1), combined)
}

// ---------------------------------------------------------------------------
// The gate: one fact, asserted on its own.
// ---------------------------------------------------------------------------

#[test]
fn fmt_check_clean_under_sh() {
    let (code, output) = run_fmt_check();

    assert_eq!(code, 0, "{}", fmt_failure_message(&output));
}

// ---------------------------------------------------------------------------
// Negative self-tests: proof the gate can actually go red.
// ---------------------------------------------------------------------------

#[test]
fn fmt_violations_reads_diff_headers() {
    let synthetic = "Diff in /src-tauri/src/main.rs:1:\n-fn  main() {\n+fn main() {\n \
}\n";

    assert_eq!(
        fmt_violations(synthetic),
        vec!["/src-tauri/src/main.rs:1".to_string()],
        "the header must be read as exactly one violation"
    );
}

#[test]
fn fmt_violations_counts_every_offending_file() {
    let synthetic = concat!(
        "Diff in /src-tauri/src/a.rs:4:\n-a\n+b\n",
        "Diff in /src-tauri/tests/b.rs:12:\n-c\n+d\n",
    );

    assert_eq!(
        fmt_violations(synthetic).len(),
        2,
        "two offending regions must be reported as two violations, not one"
    );
}

#[test]
fn fmt_violations_ignore_diff_body_and_non_diff_noise() {
    let synthetic = concat!(
        "error: could not compile `cc-switch`\n",
        "@@ -1,3 +1,3 @@\n",
        " unchanged context\n",
        "-removed line\n",
        "+added line\n",
    );

    assert!(
        fmt_violations(synthetic).is_empty(),
        "body lines and cargo chatter must never be counted as formatting \
         violations, or a build error would read as a fmt failure"
    );
}

#[test]
fn fmt_violations_ignore_bare_header_without_path() {
    assert!(
        fmt_violations("Diff in :\n").is_empty(),
        "a header with an empty path is malformed and must not be counted"
    );
}

/// Anti-empty-turn: the scanner must actually be capable of reporting, so a
/// future edit that guts it cannot leave the gate permanently silent.
#[test]
fn fmt_violation_scanner_is_not_vacuous() {
    let scanned = "Diff in /src-tauri/src/probe.rs:9:\n-a\n+b\n"
        .lines()
        .count();

    assert!(
        scanned > 0,
        "the scanner must consume real lines; a zero-line scan proves nothing"
    );
    assert_eq!(
        fmt_violations("Diff in /src-tauri/src/probe.rs:9:\n-a\n+b\n").len(),
        1,
        "a known-good header must still be detected"
    );
}

/// The gate's failure message must name *where to look*, not just dump the diff.
///
/// [`fmt_check_clean_under_sh`] used to put the raw `cargo fmt --check` output
/// straight into its panic message. That output does contain the
/// `Diff in <path>:<line>:` headings, but they sit inside a wall of `+`/`-`/`@@`
/// lines that read as equally structural — so the one fact a reader needs (which
/// files to open) had to be found by eye. This fixture pins the parse: the
/// message must carry both headings as locatable `file:line` strings.
#[test]
fn failure_message_names_every_violated_file_and_line() {
    let synthetic = concat!(
        "Diff in /src-tauri/tests/auth_token_gate.rs:546:\n",
        " old\n",
        "-new\n",
        "+old\n",
        "Diff in /src-tauri/tests/fmt_gate.rs:1006:\n",
        " old\n",
        "+new\n",
    );

    let message = fmt_failure_message(synthetic);

    for expected in [
        "/src-tauri/tests/auth_token_gate.rs:546",
        "/src-tauri/tests/fmt_gate.rs:1006",
    ] {
        assert!(
            message.contains(expected),
            "the failure message must name `{expected}` so the reader knows \
             which file to open.\nmessage:\n{message}"
        );
    }

    assert!(
        message.contains("2 offending region(s)"),
        "the message must count the violations rather than leaving the reader \
         to count them.\nmessage:\n{message}"
    );

    // The diff body must not be promoted into the *violation list* — only the
    // headings are locations. Checked against the list alone, not the whole
    // message: the trailing raw-output section is meant to contain the diff
    // verbatim, body and all.
    let listed = message
        .split("--- 2 offending region(s) ---")
        .nth(1)
        .and_then(|rest| rest.split("--- cargo fmt --check output ---").next())
        .expect("the message must have a list section followed by a raw output section");

    assert_eq!(
        listed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>(),
        vec![
            "/src-tauri/tests/auth_token_gate.rs:546",
            "/src-tauri/tests/fmt_gate.rs:1006",
        ],
        "the violation list must hold the two `file:line` locations and \
         nothing else — diff body lines are not locations"
    );
}

/// When cargo fails before rustfmt produces headings, the message must say so
/// rather than reporting an empty list that reads as "nothing to fix".
#[test]
fn failure_message_is_honest_when_there_are_no_headings() {
    let message = fmt_failure_message("error: could not compile `cc-switch`");

    assert!(
        !message.contains("0 offending region(s)"),
        "an empty list must not be dressed up as a count of zero — that reads \
         as \"nothing to fix\"\nmessage:\n{message}"
    );
    assert!(
        message.contains("no `Diff in <path>:<line>:` heading"),
        "the message must distinguish \"rustfmt found nothing to report\" from \
         \"rustfmt never ran\"\nmessage:\n{message}"
    );
}
