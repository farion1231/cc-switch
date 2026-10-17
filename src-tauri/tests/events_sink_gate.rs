//! Executable gate for the `EventSink` evidence chain.
//!
//! This file freezes the four one-off manual findings about the remote event
//! channel into assertions that CI can falsify:
//!
//! 1. `src/remote/events.rs` exists and is non-empty.
//! 2. `src/remote/mod.rs` declares `mod events;` and stores the sink as
//!    `Arc<dyn EventSink>` (trait object, not a concrete sink).
//! 3. `src/remote/mod.rs` has a production `AppHandleSink::new` construction.
//! 4. `src/remote/handlers.rs` never emits through a bare `app_handle.emit(`.
//!
//! Every detector is a pure function over source *text*, so the negative
//! fixtures below can prove the gate can go red without mutating the repo.
//! Comment lines (`//`, `///`) are never scanned: `mod.rs` and `events.rs` both
//! mention `app_handle.emit(` inside doc comments, and a gate that flagged
//! those would be unactionable noise.
//!
//! Source paths are anchored with `CARGO_MANIFEST_DIR`, never cwd-relative:
//! the cwd of a test binary depends on the harness that launched it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// The four evidence claims this gate is responsible for.
/// Used by `scan_is_not_vacuous` so an empty/duplicated contract cannot pass
/// silently.
const EVIDENCE_ASSERTIONS: [&str; 4] = [
    "events_module_file_present",
    "mod_rs_declares_events_module_and_dyn_sink_field",
    "mod_rs_has_production_app_handle_sink_construction",
    "handlers_rs_has_no_bare_app_handle_emit",
];

/// Directory holding the remote module sources.
fn remote_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("remote")
}

/// Read one file from the remote module, panicking with a useful message.
fn read_remote_source(file_name: &str) -> String {
    let path = remote_dir().join(file_name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Pure detectors — no I/O, so negative fixtures can exercise them directly.
// ---------------------------------------------------------------------------

/// True for a line that is entirely a comment (`//` covers `///` and `//!`).
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// All `(1-based line number, trimmed text)` pairs that are real code.
fn code_lines(src: &str) -> Vec<(usize, &str)> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| !is_comment(line))
        .map(|(i, line)| (i + 1, line.trim()))
        .collect()
}

/// 1-based line numbers of code lines that emit directly via `app_handle.emit(`.
fn scan_bare_emit(src: &str) -> Vec<usize> {
    code_lines(src)
        .into_iter()
        .filter(|(_, line)| line.contains("app_handle.emit("))
        .map(|(n, _)| n)
        .collect()
}

/// True when a code line declares the `events` submodule.
/// Matches the declaration itself, so a `use events::...` import or a
/// `mod events_shim;` sibling does not satisfy it.
fn has_mod_declaration(src: &str) -> bool {
    code_lines(src)
        .into_iter()
        .any(|(_, line)| line == "mod events;" || line == "pub mod events;")
}

/// True when the `sink` field is stored as a trait object, not a concrete sink.
///
/// Requires all three markers on one line: the field name, `Arc<dyn`, and the
/// trait bound. `Arc<AppHandleSink>` fails on `Arc<dyn`, which is the point.
fn sink_field_is_dyn(src: &str) -> bool {
    code_lines(src).into_iter().any(|(_, line)| {
        line.contains("sink:") && line.contains("Arc<dyn") && line.contains("EventSink")
    })
}

/// True when production code constructs the AppHandle-backed sink.
fn has_production_construction(src: &str) -> bool {
    code_lines(src)
        .into_iter()
        .any(|(_, line)| line.contains("AppHandleSink::new"))
}

// ---------------------------------------------------------------------------
// Positive evidence: what the repository must actually contain.
// ---------------------------------------------------------------------------

#[test]
fn events_module_file_present() {
    let path = remote_dir().join("events.rs");
    assert!(
        path.exists(),
        "{} must exist — it holds the EventSink seam the auth layer depends on",
        path.display()
    );

    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read: {e}"));
    assert!(
        !content.trim().is_empty(),
        "events.rs exists but is empty; the seam is a stub, not an abstraction"
    );
    assert!(
        content.contains("trait EventSink"),
        "events.rs must define the `EventSink` trait"
    );
}

#[test]
fn mod_rs_declares_events_module_and_dyn_sink_field() {
    let src = read_remote_source("mod.rs");

    assert!(
        has_mod_declaration(&src),
        "mod.rs must declare `mod events;` or the sink module is never compiled in"
    );
    assert!(
        sink_field_is_dyn(&src),
        "RemoteState::sink must be `Arc<dyn EventSink>`; a concrete sink \
         re-couples the auth path to the GUI runtime"
    );
}

#[test]
fn mod_rs_has_production_app_handle_sink_construction() {
    let src = read_remote_source("mod.rs");
    assert!(
        has_production_construction(&src),
        "mod.rs must construct `AppHandleSink::new`; otherwise the production \
         path has no event outlet and the abstraction is unused"
    );
}

#[test]
fn handlers_rs_has_no_bare_app_handle_emit() {
    let src = read_remote_source("handlers.rs");
    let bare = scan_bare_emit(&src);
    assert!(
        bare.is_empty(),
        "handlers.rs emits directly instead of going through the sink at line(s) {bare:?}"
    );
}

// ---------------------------------------------------------------------------
// Negative fixtures: the gate must be able to go red.
// ---------------------------------------------------------------------------

#[test]
fn gate_rejects_fixture_missing_mod_declaration() {
    let fixture =
        "mod handlers;\nmod html;\n\npub struct RemoteState { pub sink: Arc<dyn EventSink> }\n";
    assert!(
        !has_mod_declaration(fixture),
        "gate must reject a mod.rs that never declares `mod events;`"
    );
}

#[test]
fn gate_rejects_fixture_with_bare_emit() {
    let fixture =
        "fn handler(app_handle: &AppHandle) {\n    app_handle.emit(EVENT, payload).ok();\n}\n";
    let bare = scan_bare_emit(fixture);
    assert_eq!(
        bare,
        vec![2],
        "gate must report the exact line of a bare app_handle.emit("
    );
}

#[test]
fn gate_rejects_fixture_with_concrete_sink_field() {
    let fixture = "pub struct RemoteState {\n    pub sink: Arc<AppHandleSink>,\n}\n";
    assert!(
        !sink_field_is_dyn(fixture),
        "gate must reject a concrete `Arc<AppHandleSink>` field — that is the \
         exact coupling the EventSink seam exists to remove"
    );
}

// ---------------------------------------------------------------------------
// Anti-vacuity: a gate that is green because it scanned nothing is worse than
// no gate at all. This repo has already shipped a "green but empty" check.
// ---------------------------------------------------------------------------

#[test]
fn scan_is_not_vacuous() {
    // 1. Each scanned file must have real content AND real code lines. If the
    //    comment filter swallowed everything, every detector below would be
    //    reporting on an empty set.
    for file_name in ["handlers.rs", "mod.rs", "events.rs"] {
        let src = read_remote_source(file_name);
        let total = src.lines().count();
        let code = code_lines(&src).len();
        assert!(
            total > 0,
            "{file_name} has zero lines — the scanner would loop zero times"
        );
        assert!(
            code > 0,
            "{file_name} has zero code lines — the comment filter swallowed the file"
        );
    }

    let handlers = read_remote_source("handlers.rs");
    assert!(
        code_lines(&handlers).len() > 100,
        "handlers.rs has only {} code lines; we are almost certainly reading \
         the wrong file or a truncated one",
        code_lines(&handlers).len()
    );

    // 2. The bare-emit scanner must actually walk real lines: appending one
    //    synthetic bare emit to the real handlers.rs must yield exactly one hit.
    let probed = format!(
        "{handlers}\nfn probe(app_handle: &AppHandle) {{\n    app_handle.emit(E, p);\n}}\n"
    );
    assert_eq!(
        scan_bare_emit(&probed).len(),
        1,
        "scanner found no bare emit even when one was appended to the real \
         handlers.rs — scan_bare_emit is vacuous"
    );

    // 3. The comment filter must be doing real work: mod.rs mentions
    //    `app_handle.emit()` inside a `///` doc comment and must NOT be flagged.
    let mod_rs = read_remote_source("mod.rs");
    assert!(
        mod_rs.contains("app_handle.emit("),
        "mod.rs no longer mentions app_handle.emit( in a doc comment; the \
         comment-filter evidence in scan_is_not_vacuous has gone stale"
    );
    assert!(
        scan_bare_emit(&mod_rs).is_empty(),
        "a doc comment mentioning app_handle.emit( must not be reported as a \
         bare emit — the comment filter is broken"
    );

    // 4. The contract itself must be non-empty and free of duplicate names.
    assert_eq!(EVIDENCE_ASSERTIONS.len(), 4, "evidence set must not shrink");
    assert!(
        EVIDENCE_ASSERTIONS.iter().all(|n| !n.trim().is_empty()),
        "an empty assertion name means the gate covers nothing"
    );
    let mut unique = EVIDENCE_ASSERTIONS.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        EVIDENCE_ASSERTIONS.len(),
        "duplicate evidence assertion names — a slot is being wasted"
    );
}

// ---------------------------------------------------------------------------
// Harness-shell executability gate.
//
// The CI harness runs every task's `test_commands` through `/bin/sh -c`. A
// non-interactive `/bin/sh` does not load the user's profile, so `cargo` —
// which is a rustup shim living in `$HOME/.cargo/bin` — is NOT on PATH there.
// A bare `cargo test ...` therefore exits 127 no matter how healthy the code
// under test is. The gate below pins the PATH-immunity shim so the command
// form stays executable, and proves the seam by *running* the target test
// rather than by scanning source text.
// ---------------------------------------------------------------------------

/// Path prefix that puts the rustup shim ahead of whatever the harness's
/// non-interactive shell starts with.
///
/// `$HOME` is used rather than a literal path: the repo is cloned onto many
/// machines. Prepending is idempotent — if `$HOME/.cargo/bin` does not exist,
/// the entry is simply never matched and `cargo` still resolves from the
/// inherited PATH, exactly as before.
const CARGO_PATH_SHIM: &str = r#"PATH="$HOME/.cargo/bin:$PATH""#;

/// `PATH` with every directory that resolves a `cargo` binary removed.
///
/// This is what makes the gate non-vacuous. `cargo test` PREPENDS
/// `$HOME/.cargo/bin` to `PATH` for the test binary it spawns, so a child
/// `/bin/sh` would inherit a `PATH` that already resolves `cargo` — and
/// `CARGO_PATH_SHIM` would be decorative, passing no matter what it contained.
/// Scrubbing restores the harness's actual starting state (a non-interactive
/// `/bin/sh` that never loaded a profile), which is the only state in which
/// the shim is load-bearing.
///
/// `HOME` is preserved: the shim expands `$HOME` inside the child shell, so
/// removing it would break the thing under test rather than harden it.
fn harness_path() -> String {
    let inherited = std::env::var("PATH").unwrap_or_default();
    let kept: Vec<&str> = inherited
        .split(':')
        .filter(|dir| !dir.is_empty())
        .filter(|dir| !Path::new(dir).join("cargo").exists())
        .collect();

    assert!(
        !kept.is_empty(),
        "every PATH entry contained a cargo binary; the scrubbed environment \
         would be unusable"
    );
    kept.join(":")
}

/// Run a command the way the CI harness does: through a non-interactive
/// `/bin/sh -c`, starting from the scrubbed PATH above.
fn run_in_harness_shell(script: &str) -> Output {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env("PATH", harness_path())
        .output()
        .expect("cannot spawn /bin/sh")
}

/// stdout + stderr merged, so assertions work regardless of which stream
/// the toolchain happens to use.
fn merged(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `cargo` must be resolvable inside the harness's shell once the shim is
/// applied. This is the precondition every Rust test command depends on, and
/// the seam shared with the source-level gate above.
#[test]
fn cargo_is_resolvable_in_sh() {
    let out = run_in_harness_shell(&format!("{CARGO_PATH_SHIM} command -v cargo"));
    let text = merged(&out);

    assert!(
        out.status.success(),
        "`command -v cargo` failed in /bin/sh even with the PATH shim ({CARGO_PATH_SHIM}).\n{text}"
    );
    assert!(
        !text.trim().is_empty(),
        "`command -v cargo` succeeded but printed nothing — nothing was resolved"
    );
    assert!(
        text.trim_end().ends_with("cargo"),
        "expected `command -v cargo` to resolve to a cargo binary, got: {}",
        text.trim()
    );
}

/// The actual target command, executed end-to-end under the harness shell.
///
/// A source scan cannot prove a seam is *usable* — only running it can. So
/// this shells out exactly as CI does and requires a green run of the four
/// `remote::events` cases, not merely a zero exit from a shell that could not
/// find the binary at all.
#[test]
fn remote_events_target_test_passes_under_sh_with_path_shim() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let script = format!(
        r#"{CARGO_PATH_SHIM} cargo test --manifest-path "{}" --lib remote::events"#,
        manifest.display()
    );

    let out = run_in_harness_shell(&script);
    let text = merged(&out);

    assert!(
        out.status.success(),
        "target command must exit 0 under the harness's /bin/sh.\n\
         command: {script}\noutput:\n{text}"
    );
    assert!(
        text.contains("test result: ok."),
        "target command exited 0 without a green libtest summary — a shell that \
         cannot find `cargo` also exits non-zero, so the summary is the proof \
         the four cases really ran.\noutput:\n{text}"
    );
    assert!(
        text.contains("4 passed"),
        "expected the 4 remote::events cases to pass; got:\n{text}"
    );
    assert!(
        text.contains("0 failed"),
        "target run reported failures:\n{text}"
    );
}

/// Negative fixture: the failure mode this gate exists to catch.
///
/// Without the shim, the harness shell cannot resolve `cargo` at all. This
/// proves the shim is load-bearing rather than decorative — if the shim were
/// dropped from `test_commands`, the assertion below would still hold while
/// the real command returned 127, and the gate would be protecting nothing.
#[test]
fn bare_cargo_is_not_resolvable_in_shell_without_shim() {
    static BASE_PATH_HAS_CARGO: OnceLock<bool> = OnceLock::new();
    let base_has_cargo = *BASE_PATH_HAS_CARGO
        .get_or_init(|| run_in_harness_shell("command -v cargo").status.success());

    if base_has_cargo {
        // Some environments export cargo onto PATH directly (CI images, shell
        // profiles). The shim is then redundant but harmless; asserting the
        // failure here would be asserting a property of the machine, not of
        // the command form.
        return;
    }

    let out = run_in_harness_shell("cargo test --lib remote::events");
    assert!(
        !out.status.success(),
        "bare `cargo` unexpectedly resolved in /bin/sh; this environment does \
         not reproduce the 127 the shim defends against, so the negative \
         fixture is vacuous here"
    );
}

// ---------------------------------------------------------------------------
// Clippy gate: `remote/events.rs` must contribute ZERO diagnostics.
//
// The source-level gate above proves the `AppHandleSink` text exists. Text is
// not liveness: an abstraction that is only ever constructed by tests compiles
// fine and still shows up in production as `dead_code`. The only machine-checked
// statement that the production side really *uses* `AppHandle::emit` is clippy
// staying silent about that file while linting the lib target.
//
// `--lib`, not `--all-targets`: dead code means "unused by the production
// build", and test-target artifacts are not production use. Test-only types
// (RecordingSink/NullSink) are exactly what must NOT be judged here.
//
// Scope: events.rs only, never the whole crate. The repo carries unrelated
// pre-existing warnings (is_tailscale_available, stop_remote, ...); those are
// a CI-wide `-D warnings` concern and would drown the signal this gate exists
// to carry.
// ---------------------------------------------------------------------------

/// Path fragment used to attribute a diagnostic to the events module.
/// Relative, never absolute — the repo moves between machines.
const EVENTS_RS_FRAGMENT: &str = "remote/events.rs";

/// Lines of a lint output that blame `remote/events.rs`.
///
/// Only `--> file:line:col` spans count. A bare substring match would also
/// fire on a diagnostic that merely *quotes* the path (e.g. an unused-import
/// note), and the gate must measure "a diagnostic is reported here".
fn events_rs_diagnostics(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("-->") && line.contains(EVENTS_RS_FRAGMENT))
        .map(str::to_string)
        .collect()
}

/// True when the lint output reports a compilation error (as opposed to a
/// warning). A clippy run that dies on `error[E0433]` prints no events.rs
/// diagnostic, so without this the "zero hits" assertion would be satisfied by
/// a build that never got as far as linting.
fn has_compile_error(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim_start().starts_with("error[E") || line.contains("could not compile"))
}

/// Run the exact lint command this gate is about, through the harness shell.
fn run_events_rs_clippy() -> Output {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let script = format!(
        r#"{CARGO_PATH_SHIM} cargo clippy --manifest-path "{}" --lib"#,
        manifest.display()
    );
    run_in_harness_shell(&script)
}

/// `AppHandleSink` really wraps `AppHandle::emit` → clippy reports nothing for
/// `remote/events.rs`.
///
/// Anti-vacuity first: the run must have actually driven cargo. An empty or
/// `cargo: not found` output trivially satisfies "zero events.rs hits", which is
/// precisely the false pass this test must not be able to produce.
#[test]
fn clippy_reports_no_warning_for_events_rs() {
    let out = run_events_rs_clippy();
    let text = merged(&out);

    // The lint really ran. Two shapes are legitimate and BOTH must be accepted:
    //   - `Checking`/`Compiling cc-switch` — the lib unit was dirty, clippy
    //     re-linted it and named the crate;
    //   - `Finished` alone — the lib unit was already fresh. CI runs
    //     `cargo clippy --all-targets` (which includes `--lib`) immediately
    //     before this gate, so on a clean tree the nested `--lib` run has
    //     nothing to do and cargo prints only its completion line.
    // What this assertion actually defends against is a shell that could not
    // find `cargo` at all: that prints neither, and would otherwise satisfy
    // "zero events.rs hits" vacuously.
    let cargo_ran = text.contains("Checking cc-switch")
        || text.contains("Compiling cc-switch")
        || text.contains("Finished");
    assert!(
        cargo_ran,
        "clippy output neither names the crate under lint nor reports a \
         completed cargo run — the command probably never ran (missing PATH \
         shim?).\ncommand output:\n{text}"
    );

    let hits = events_rs_diagnostics(&text);
    assert!(
        hits.is_empty(),
        "clippy reports {} diagnostic(s) for remote/events.rs, so the \
         `AppHandleSink` abstraction is not clean on the production lib target.\n\
         hit(s):\n{}\nfull output:\n{text}",
        hits.len(),
        hits.join("\n")
    );
}

/// The `EventSink` abstraction must not have broken the crate: clippy must get
/// past compilation. A non-zero clippy exit caused by *warnings* is fine — the
/// gate is scoped to events.rs, not to the whole crate.
#[test]
fn crate_still_compiles_under_clippy() {
    let out = run_events_rs_clippy();
    let text = merged(&out);

    assert!(
        !has_compile_error(&text),
        "clippy hit a compilation error, so the zero-hits result in \
         clippy_reports_no_warning_for_events_rs would be vacuous.\n\
         full output:\n{text}"
    );
}

/// Negative fixtures: the events.rs detector must be able to go red.
///
/// Without these, a typo in `events_rs_diagnostics` (a filter that matches
/// nothing, a fragment that no diagnostic ever carries) leaves the real gate
/// permanently green while protecting nothing.
#[test]
fn gate_rejects_fixture_with_events_rs_warning() {
    let fixture = "warning: function `x` is never used\n \
                    --> src/remote/events.rs:88:5\n  |\n88 | fn x() {}\n";
    assert_eq!(
        events_rs_diagnostics(fixture).len(),
        1,
        "detector missed a real events.rs diagnostic — the gate can never go red"
    );
}

#[test]
fn gate_ignores_events_rs_mention_that_is_not_a_span() {
    // A diagnostic that only *mentions* the path must not be counted as one
    // attributed to events.rs, otherwise unrelated lints could fail this gate.
    let fixture = "note: items defined in src/remote/events.rs are unused\n";
    assert!(
        events_rs_diagnostics(fixture).is_empty(),
        "a prose mention of remote/events.rs was counted as a diagnostic span"
    );

    // And a span for some other file must not be swept in either.
    let other = "warning: unused import\n --> src/remote/handlers.rs:12:5\n";
    assert!(
        events_rs_diagnostics(other).is_empty(),
        "a handlers.rs span was attributed to events.rs"
    );
}

#[test]
fn compile_error_detector_fires_on_both_shapes() {
    assert!(
        has_compile_error("error[E0433]: failed to resolve\n"),
        "E-coded rustc error not detected"
    );
    assert!(
        has_compile_error("error: could not compile `cc-switch`"),
        "cargo's 'could not compile' summary not detected"
    );
    assert!(
        !has_compile_error("warning: function `apply_remote_config` is never used\n"),
        "a plain warning must not be read as a compile error — the crate has \
         pre-existing warnings that this gate must tolerate"
    );
}
