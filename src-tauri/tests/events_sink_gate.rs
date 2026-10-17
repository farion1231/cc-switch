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

use std::ffi::OsString;
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
// Harness-executability gate.
//
// The CI harness runs every task's `test_commands` in a non-interactive shell
// that never loads the user's profile, so `cargo` — which is a rustup shim
// living in `$HOME/.cargo/bin` — is not guaranteed to be resolvable there. A
// bare `cargo test ...` can then fail with "command not found" no matter how
// healthy the code under test is. The gate below pins the PATH-immune
// invocation so the command form stays executable, and proves the seam by
// *running* the target test rather than by scanning source text.
//
// **No shell is involved.** The previous version drove cargo through a POSIX
// shell, passing a `PATH=` prefix assignment and a bare `cargo` as script text,
// which forced the whole file behind `#![cfg(unix)]` — Windows has no such
// shell, so the gate silently became zero assertions there, which is the one
// place it most needs to run. The shell was only ever doing two things:
// expanding the `PATH=` prefix assignment, and resolving the bare name `cargo`
// against PATH. Both are native now (`.env("PATH", …)` and an absolute program
// path plus the OS's own resolver), so nothing is left for a shell to do and
// the file builds and runs on every platform.
// ---------------------------------------------------------------------------

/// The cargo binary that is building this very test, as an absolute path.
///
/// Cargo exports `CARGO` at build time, so this is the path on the *target*
/// machine — `cargo.exe` under `.cargo/bin` on a Windows runner — and never
/// depends on the caller's PATH.
fn cargo_bin() -> &'static Path {
    Path::new(env!("CARGO"))
}

/// The rustup shim directory — the entry the PATH shim puts ahead of whatever
/// the harness starts with.
///
/// `$HOME` is never written down as a literal path: the repo is cloned onto
/// many machines. Prepending is idempotent — if the directory does not exist,
/// the entry is simply never matched and `cargo` still resolves from the
/// inherited PATH, exactly as before.
fn cargo_bin_dir() -> PathBuf {
    cargo_bin()
        .parent()
        .expect("CARGO must have a parent directory")
        .to_path_buf()
}

/// PATH with the rustup shim directory prepended to everything inherited.
///
/// `join_paths` is what makes this correct on Windows, where the separator is
/// `;` and not `:`.
fn shimmed_path() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![cargo_bin_dir()];
    dirs.extend(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).expect("PATH entries must be joinable")
}

/// `PATH` with every directory that resolves a `cargo` binary removed.
///
/// This is what makes the negative fixture non-vacuous. `cargo test` PREPENDS
/// the rustup shim directory to `PATH` for the test binary it spawns, so a
/// child process would otherwise inherit a PATH that already resolves `cargo` —
/// and the shim would be decorative, passing no matter what it contained.
/// Scrubbing restores the harness's actual starting state, which is the only
/// state in which the shim is load-bearing.
fn scrubbed_path() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let shim_dir = cargo_bin_dir();
    let kept: Vec<PathBuf> = std::env::split_paths(&inherited)
        .filter(|dir| dir != &shim_dir)
        .filter(|dir| !dir.join("cargo").exists())
        // Windows resolves `cargo` through PATHEXT, so a `.exe` copy in some
        // other directory would keep the scrub from taking effect there.
        .filter(|dir| !dir.join("cargo.exe").exists())
        .collect();

    assert!(
        !kept.is_empty(),
        "every PATH entry contained a cargo binary; the scrubbed environment \
         would be unusable"
    );
    std::env::join_paths(kept).expect("PATH entries must be joinable")
}

/// Run a command with the shimmed PATH above.
///
/// The working directory is pinned to `CARGO_MANIFEST_DIR` rather than
/// inherited: the cwd of a test binary depends on whatever launched it.
fn run_with_shimmed_path(program: &Path, args: &[&str], path: OsString) -> Output {
    Command::new(program)
        .args(args)
        .env("PATH", &path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap_or_else(|e| panic!("cannot spawn {}: {e}", program.display()))
}

/// Spawn without asserting on failure — the negative fixture needs to observe
/// "could not be started at all", which is what a launcher reports when it
/// cannot resolve the program.
fn try_spawn_with_path(program: &Path, args: &[&str], path: OsString) -> std::io::Result<Output> {
    Command::new(program)
        .args(args)
        .env("PATH", &path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
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

fn manifest_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// `cargo` must be reachable with only the shimmed PATH applied. This is the
/// precondition every Rust test command depends on, and the seam shared with
/// the source-level gate above.
#[test]
fn cargo_is_reachable_with_the_shim() {
    // Cross-platform stand-in for `command -v cargo`: hand the child only the
    // shimmed PATH and let the OS resolve the bare name (unix: PATH; Windows:
    // PATH + PATHEXT).
    let probe = Path::new("cargo");
    let out = run_with_shimmed_path(probe, &["--version"], shimmed_path());
    let text = merged(&out);

    assert!(
        out.status.success(),
        "spawning a bare `cargo` failed even with the rustup shim directory on \
         PATH.\n{text}"
    );
    assert!(
        text.to_lowercase().contains("cargo"),
        "expected the resolved binary to identify as cargo, got: {}",
        text.trim()
    );
}

/// The actual target command, executed end-to-end with the PATH shim.
///
/// A source scan cannot prove a seam is *usable* — only running it can. So
/// this spawns exactly what CI does and requires a green run of the four
/// `remote::events` cases, not merely a zero exit from a launcher that could
/// not find the binary at all.
#[test]
fn remote_events_target_test_passes_with_path_shim() {
    let manifest = manifest_file().display().to_string();
    let out = run_with_shimmed_path(
        cargo_bin(),
        &[
            "test",
            "--manifest-path",
            &manifest,
            "--lib",
            "remote::events",
        ],
        shimmed_path(),
    );
    let text = merged(&out);

    assert!(
        out.status.success(),
        "target command must exit 0.\noutput:\n{text}"
    );
    assert!(
        text.contains("test result: ok."),
        "target command exited 0 without a green libtest summary — a launcher \
         that cannot find `cargo` also exits non-zero, so the summary is the \
         proof the four cases really ran.\noutput:\n{text}"
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
/// Without the shim, the harness cannot resolve `cargo` at all. This proves
/// the shim is load-bearing rather than decorative — if the shim were dropped
/// from `test_commands`, the assertion below would still hold while the real
/// command returned command-not-found, and the gate would be protecting
/// nothing.
#[test]
fn bare_cargo_is_unreachable_without_the_shim() {
    static BASE_PATH_HAS_CARGO: OnceLock<bool> = OnceLock::new();
    let base_has_cargo = *BASE_PATH_HAS_CARGO.get_or_init(|| {
        matches!(
            try_spawn_with_path(Path::new("cargo"), &["--version"], scrubbed_path()),
            Ok(out) if out.status.success()
        )
    });

    if base_has_cargo {
        // Some environments export cargo onto PATH directly (CI images, shell
        // profiles). The shim is then redundant but harmless; asserting the
        // failure here would be asserting a property of the machine, not of the
        // invocation shape.
        return;
    }

    let out = try_spawn_with_path(
        Path::new("cargo"),
        &["test", "--lib", "remote::events"],
        scrubbed_path(),
    );
    assert!(
        !matches!(&out, Ok(child) if child.status.success()),
        "bare `cargo` unexpectedly resolved on a PATH that had every cargo \
         directory removed; this environment does not reproduce the \
         command-not-found the shim defends against, so the negative fixture \
         is vacuous here"
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

/// Run the exact lint command this gate is about, with the PATH shim applied.
///
/// **整个测试进程只跑一次**，结果由所有断言共享。
///
/// 两条用例（`clippy_reports_no_warning_for_events_rs` 和
/// `crate_still_compiles_under_clippy`）问的是**同一条命令的同一次输出**的不同侧面：
/// 一条看 events.rs 的诊断条数，一条看有没有编译错误。它们此前各自 spawn 一次
/// `cargo clippy`，在 nextest 下就是两个进程各跑一遍完整 lint —— 实测同一批里
/// 并发时把 package cache 锁排成队，单条从 0.7s 涨到 70s，CI 上两条合计 220s
/// 直接撞穿 180s 的门禁上限。而它们断言的其实是**一个事实**：这次 lint 跑过了、
/// 且 events.rs 干净。把一次运行的输出共享出去，既省掉整轮重跑，也让两条断言
/// 不再可能因为「各自那次跑出来的结果不一样」而互相矛盾。
///
/// `OnceLock` 而非每测一次：门禁要检的是**这次 lint 的输出**，不是「每个测试
/// 进程各自 lint 一遍」。`std::sync::OnceLock` 的 `get_or_init` 在并发下只跑一次
/// 初始化，因此即使两条用例被排到不同线程上也只有一次 spawn。
fn run_events_rs_clippy() -> &'static Output {
    static CLIPPY: OnceLock<Output> = OnceLock::new();
    CLIPPY.get_or_init(|| {
        let manifest = manifest_file().display().to_string();
        run_with_shimmed_path(
            cargo_bin(),
            &["clippy", "--manifest-path", &manifest, "--lib"],
            shimmed_path(),
        )
    })
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
    let text = merged(out);

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
    let text = merged(out);

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
