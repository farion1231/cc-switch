//! Executable gate for the PATH-immunity of the `remote::auth::policy`
//! test command.
//!
//! The CI harness runs every task's `test_commands` in a non-interactive shell
//! that never loads the user's profile, so `cargo` — a rustup shim living in
//! `$HOME/.cargo/bin` — is not guaranteed to be resolvable there. A bare
//! `cargo test ...` can then fail with "command not found" no matter how
//! healthy the code under test is.
//!
//! This file pins the PATH-immune invocation *shape* and proves it by running
//! it, not by scanning source text. The three claims:
//!
//! 1. The invocation carries the PATH shim (pure detector, no I/O).
//! 2. `cargo` is reachable once the shim directory is on PATH.
//! 3. The full target invocation runs green there, with every case passing.
//!
//! **No shell is involved.** The previous version drove cargo through a POSIX
//! shell, passing a `PATH=` prefix assignment and a bare `cargo` as script
//! text, which forced the whole file behind `#![cfg(unix)]` — Windows has no
//! such shell, so the gate silently became zero assertions on that platform,
//! which is the one place it most needs to run. The shell was only ever doing
//! two things there: expanding the `PATH=` prefix assignment, and resolving the
//! bare name `cargo` against PATH. Both are expressed natively now —
//! `.env("PATH", …)` for the first, an absolute program path plus the OS's own
//! resolver for the second — so nothing is left for a shell to do and the file
//! builds and runs everywhere.
//!
//! Anti-vacuity: claim 3 requires the libtest summary itself. A run that could
//! not find `cargo` exits non-zero, but "exit 0" alone would also be satisfied
//! by a filter that matched nothing, so the case count is parsed out of the
//! summary and compared against the number of `#[test]` attributes actually
//! present in `policy.rs`.
//!
//! That expected count is **derived, never written down**. A hardcoded `8`
//! turned this gate into a tripwire on policy authoring: the eleventh case
//! someone adds to `policy.rs` makes the gate red even though nothing regressed,
//! and the only way to green it is to edit a number in this file — so the
//! number goes stale, the gate goes red for a reason unrelated to its purpose,
//! and readers learn to ignore it. Counting the source instead means the
//! expected total follows the file by construction, and adding or removing a
//! case needs no edit here at all. The count is still *asserted to be
//! non-zero* and is still compared exactly, so it keeps its anti-vacuity job:
//! it can no longer be satisfied by a filter that matched nothing.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// The cargo binary that is building this very test, as an absolute path.
///
/// Cargo exports `CARGO` at build time, so this is the path on the *target*
/// machine — `cargo.exe` under `.cargo/bin` on a Windows runner — and never
/// depends on the caller's PATH. That is what makes claim 1 satisfiable without
/// a shell: the program is named absolutely, so PATH resolution never has to
/// happen for *it*.
fn cargo_bin() -> &'static Path {
    Path::new(env!("CARGO"))
}

/// The rustup shim directory — the entry the PATH shim puts ahead of whatever
/// the harness starts with.
///
/// `$HOME` is never written down as a literal path: the repo is cloned onto
/// many machines. Prepending is idempotent — if the directory does not exist
/// the entry is simply never matched, and `cargo` still resolves from the
/// inherited PATH exactly as before.
fn cargo_bin_dir() -> PathBuf {
    cargo_bin()
        .parent()
        .expect("CARGO must have a parent directory")
        .to_path_buf()
}

/// Test-name filter for the authorization decision table.
const POLICY_FILTER: &str = "remote::auth::policy";

/// A lib-test case in this crate that collides with a locally running
/// cc-switch on 127.0.0.1:15721. It fails with `Address already in use` for
/// environmental reasons, so the filter above must keep it out of this gate.
/// Its absence is asserted, not assumed.
const PORT_CONFLICT_CASE: &str =
    "services::provider::tests::update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active";

fn manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// `src-tauri/src/remote/auth/policy.rs` — the file whose cases this gate counts.
///
/// Anchored at `CARGO_MANIFEST_DIR` like everything else here, so the repo
/// stays cloneable anywhere.
fn policy_source_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("remote")
        .join("auth")
        .join("policy.rs")
}

fn policy_source() -> String {
    let path = policy_source_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// How many libtest cases `src` declares, counted from its attributes.
///
/// `#[test]` and `#[tokio::test]` are two spellings of the same thing to
/// libtest — one case each — so both are counted. Counting the *attribute*
/// rather than the function name is what makes the total follow the file: a
/// case exists iff it carries one of these, so adding or removing a case adds
/// or removes exactly one match, and no second list has to be kept in sync.
///
/// Line-anchored on the whole attribute, not a substring: `#[test]` must be the
/// entire trimmed line. That keeps `#[ignore] #[test]`-style decoration and a
/// doc comment *mentioning* `#[test]` from inflating the total, and it means
/// the count only moves when a real case is added or removed.
fn test_attribute_case_count(src: &str) -> usize {
    src.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed == "#[test]" || trimmed == "#[tokio::test]"
        })
        .count()
}

/// The number of cases `remote::auth::policy` must run, derived from the file.
///
/// Asserted non-zero: a zero here would mean the walk failed, and a
/// "0 passed, 0 failed" summary would then satisfy the comparison below while
/// proving nothing.
fn expected_policy_case_count() -> usize {
    let count = test_attribute_case_count(&policy_source());
    assert!(
        count > 0,
        "no `#[test]` attributes found in {} — the derivation has silently \
         degraded and the expected total would vacuously be 0.",
        policy_source_path().display()
    );
    count
}

/// One cargo invocation, as data, so the detector and the execution below
/// cannot drift apart.
///
/// `path` is written into the child's environment explicitly rather than being
/// assembled as a `PATH=…` shell prefix; `args` is already split, so there is
/// no shell syntax anywhere in this type. The negative fixtures can therefore
/// build and inspect an invocation without spawning anything.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CargoRun {
    /// Program to spawn. Absolute for the real invocations; a bare name for
    /// the ones whose entire point is that the OS has to resolve them.
    program: PathBuf,
    /// Arguments, already split.
    args: Vec<String>,
    /// The `PATH` handed to the child explicitly.
    path: OsString,
}

/// PATH with the rustup shim directory prepended to everything inherited.
///
/// Prepending rather than replacing: the nested cargo still needs the rest of
/// the PATH (linker, cc, …), and dropping it would break the thing under test
/// rather than harden it. `join_paths` is what makes this correct on Windows,
/// where the separator is `;` and not `:`.
fn shimmed_path() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![cargo_bin_dir()];
    dirs.extend(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).expect("PATH entries must be joinable")
}

/// `PATH` with every directory that resolves a `cargo` binary removed.
///
/// This is what makes the gate non-vacuous. `cargo test` PREPENDS the rustup
/// shim directory to `PATH` for the test binary it spawns, so a child process
/// would otherwise inherit a PATH that already resolves `cargo` — and the shim
/// would be decorative, passing no matter what it contained. Scrubbing
/// restores the harness's real starting state, which is the only state in
/// which the shim is load-bearing.
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

/// Run an invocation, panicking if the process could not be started at all.
///
/// The working directory is pinned to `CARGO_MANIFEST_DIR` rather than
/// inherited: the cwd of a test binary depends on whatever launched it, and
/// every path this gate touches is meant to be repo-relative-free.
fn spawn(run: &CargoRun) -> std::io::Result<Output> {
    Command::new(&run.program)
        .args(&run.args)
        .env("PATH", &run.path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
}

fn run(run: &CargoRun) -> Output {
    spawn(run).unwrap_or_else(|e| panic!("cannot spawn {:?}: {e}", run.program))
}

/// stdout + stderr merged, so assertions hold regardless of which stream the
/// toolchain happens to use.
fn merged(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// True when the invocation is PATH-immune.
///
/// Both halves are load-bearing: naming `cargo` absolutely means the child
/// never has to resolve the bare name, and the shim directory on PATH means
/// everything *the child itself* spawns can still resolve it. Drop either and
/// the harness's profile-less PATH can still take the run down.
fn is_path_immune(run: &CargoRun) -> bool {
    run.program.is_absolute() && std::env::split_paths(&run.path).any(|dir| dir == cargo_bin_dir())
}

/// The `test_commands` form this gate exists to protect, as data.
fn policy_target_run() -> CargoRun {
    CargoRun {
        program: cargo_bin().to_path_buf(),
        args: policy_test_args(),
        path: shimmed_path(),
    }
}

fn policy_test_args() -> Vec<String> {
    vec![
        "test".to_string(),
        "--manifest-path".to_string(),
        manifest_path().display().to_string(),
        "--lib".to_string(),
        POLICY_FILTER.to_string(),
    ]
}

/// A bare `cargo` with the shimmed PATH — the cross-platform stand-in for
/// `command -v cargo`, letting the OS's own resolver do the finding.
fn bare_cargo_run(args: &[&str], path: OsString) -> CargoRun {
    CargoRun {
        program: PathBuf::from("cargo"),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        path,
    }
}

/// Parse the `N passed` / `N failed` counters out of a libtest summary field.
///
/// The field is `"<count> <label>"`, e.g. `"8 passed"`. Returns `None` when
/// the label does not match, so a summary lacking a counter is not silently
/// read as zero.
fn count_field(summary: &str, label: &str) -> Option<usize> {
    summary.split(';').find_map(|field| {
        let mut words = field.split_whitespace();
        let count = words.next()?.parse::<usize>().ok()?;
        (words.next()? == label).then_some(count)
    })
}

/// Parse `(passed, failed)` out of the libtest summary line.
///
/// Returns `None` when the line is absent or is not an `ok.` summary, which is
/// exactly the state in which "exit 0" must not be accepted as a passing run.
fn libtest_case_counts(output: &str) -> Option<(usize, usize)> {
    let summary = output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("test result: ok."))?;

    // Strip the verdict first: it shares the first `;` field with the passed
    // counter, so leaving it in would make that field unparseable as a count.
    let counters = summary.trim_start_matches("test result: ok.");

    Some((
        count_field(counters, "passed")?,
        count_field(counters, "failed")?,
    ))
}

// ---------------------------------------------------------------------------
// Claim 1: the invocation shape itself is PATH-immune.
// ---------------------------------------------------------------------------

#[test]
fn target_command_form_is_path_immune() {
    let target = policy_target_run();
    assert!(
        is_path_immune(&target),
        "the target invocation must name cargo by absolute path and carry the \
         rustup shim directory on its PATH, so it survives a harness PATH that \
         never loaded a profile.\nprogram: {}\nPATH: {:?}",
        target.program.display(),
        target.path
    );
    assert!(
        target.args.iter().any(|arg| arg == POLICY_FILTER),
        "the target invocation lost its test-name filter; a bare `--lib` run \
         would pull in environment-flaky cases outside this gate.\nargs: {:?}",
        target.args
    );
}

/// Negative fixture: the detector must reject the forms this gate replaced.
///
/// Without it, a detector that always returns true would leave the gate green
/// while protecting nothing.
#[test]
fn bare_command_form_is_rejected_by_detector() {
    assert!(
        !is_path_immune(&bare_cargo_run(&["test"], shimmed_path())),
        "a bare `cargo` left to the inherited PATH is exactly the form this \
         gate replaced — it is not PATH-immune even with the shim applied"
    );

    let absolute_but_unshimmed = CargoRun {
        program: cargo_bin().to_path_buf(),
        args: policy_test_args(),
        path: scrubbed_path(),
    };
    assert!(
        !is_path_immune(&absolute_but_unshimmed),
        "an absolute program without the shim directory on PATH must not \
         satisfy the detector — the shim has to be load-bearing, not decorative"
    );

    assert!(
        !is_path_immune(&CargoRun {
            program: PathBuf::new(),
            args: vec!["test".to_string()],
            path: shimmed_path(),
        }),
        "an empty program must not be read as PATH-immune"
    );
}

// ---------------------------------------------------------------------------
// Claim 2: the seam is usable — cargo is reachable once the shim is applied.
// ---------------------------------------------------------------------------

#[test]
fn cargo_is_reachable_with_the_shim() {
    // Cross-platform stand-in for `command -v cargo`: hand the child only the
    // shimmed PATH and let the OS resolve the bare name (unix: PATH; Windows:
    // PATH + PATHEXT). A spawn failure here is the same fact as a non-zero
    // `command -v`, so `run`'s panic is the right failure mode.
    let probe = bare_cargo_run(&["--version"], shimmed_path());
    let out = run(&probe);
    let text = merged(&out);

    assert!(
        out.status.success(),
        "spawning a bare `cargo` failed even with the rustup shim directory on \
         PATH.\nprogram: {}\nPATH: {:?}\n{text}",
        probe.program.display(),
        probe.path
    );
    assert!(
        text.to_lowercase().contains("cargo"),
        "expected the resolved binary to identify as cargo, got: {}",
        text.trim()
    );
}

// ---------------------------------------------------------------------------
// Claim 3: the target invocation runs green with the shim.
// ---------------------------------------------------------------------------

#[test]
fn remote_auth_policy_target_passes_with_path_shim() {
    let target = policy_target_run();
    let out = run(&target);
    let text = merged(&out);

    assert!(
        out.status.success(),
        "target invocation must exit 0.\nprogram: {}\nPATH: {:?}\noutput:\n{text}",
        target.program.display(),
        target.path
    );

    let (passed, failed) = libtest_case_counts(&text).unwrap_or_else(|| {
        panic!(
            "no parseable `test result:` summary — exit 0 alone does not prove \
             the cases ran.\noutput:\n{text}"
        )
    });

    let expected = expected_policy_case_count();
    assert_eq!(
        passed, expected,
        "expected {expected} remote::auth::policy cases to pass — that is the \
         number of `#[test]` attributes currently in src/remote/auth/policy.rs.\n{text}"
    );
    assert_eq!(failed, 0, "target run reported failures.\n{text}");

    // The filter is load-bearing: it is what keeps the port-conflicting
    // provider case (red for environmental reasons on a machine with a
    // running cc-switch) out of this gate.
    assert!(
        text.contains("filtered out"),
        "no `N filtered out` in the summary — the filter may have been dropped.\n{text}"
    );
    assert!(
        !text.contains(PORT_CONFLICT_CASE),
        "the port-conflicting provider case {PORT_CONFLICT_CASE} is in scope for \
         this gate; the filter is not narrowing the run as intended.\n{text}"
    );
}

// ---------------------------------------------------------------------------
// Negative fixture: the shim must be load-bearing, not decorative.
// ---------------------------------------------------------------------------

#[test]
fn bare_cargo_is_unreachable_without_the_shim() {
    static BASE_PATH_HAS_CARGO: OnceLock<bool> = OnceLock::new();
    let base_has_cargo = *BASE_PATH_HAS_CARGO.get_or_init(|| {
        matches!(
            spawn(&bare_cargo_run(&["--version"], scrubbed_path())),
            Ok(out) if out.status.success()
        )
    });

    if base_has_cargo {
        // Some environments export cargo onto PATH directly (CI images, shell
        // profiles). The shim is then redundant but harmless; asserting the
        // failure here would assert a property of the machine, not of the
        // invocation shape.
        return;
    }

    let args = policy_test_args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = spawn(&bare_cargo_run(&args, scrubbed_path()));
    assert!(
        !matches!(&out, Ok(child) if child.status.success()),
        "bare `cargo` unexpectedly resolved on a PATH that had every cargo \
         directory removed; this environment does not reproduce the \
         command-not-found the shim defends against, so the negative fixture \
         is vacuous here"
    );
}

// ---------------------------------------------------------------------------
// Summary parsing must reject output that never ran the cases.
// ---------------------------------------------------------------------------

#[test]
fn libtest_summary_parser_rejects_non_run_output() {
    // No summary at all: a launcher that never found cargo prints only its
    // own "command not found" line, whatever shell or API reported it.
    assert_eq!(
        libtest_case_counts("cargo: command not found"),
        None,
        "a command-not-found from a launcher that cannot resolve cargo must \
         not parse as a pass"
    );
    assert_eq!(
        libtest_case_counts(
            "'cargo' is not recognized as an internal or \
         external command"
        ),
        None,
        "the Windows spelling of the same failure must not parse as a pass \
         either"
    );
    // A failing summary does not carry the `ok.` prefix.
    assert_eq!(
        libtest_case_counts("test result: FAILED. 7 passed; 1 failed; 0 ignored;"),
        None,
        "a FAILED summary must not parse as a passing run"
    );
    // Sanity: the real shape parses.
    assert_eq!(
        libtest_case_counts(
            "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 2845 filtered out;"
        ),
        Some((3, 0)),
        "the parser must understand the real libtest summary shape"
    );
}

// ---------------------------------------------------------------------------
// The expected count must track policy.rs, not a constant.
// ---------------------------------------------------------------------------

/// The regression this section exists to prevent, stated as an executable test.
///
/// The gate used to compare the run against a literal `EXPECTED_CASE_COUNT =
/// 8`. The eleventh case added to `policy.rs` then made the gate red with
/// nothing regressed, and the only remedy was to edit the constant — so every
/// policy-authoring round risked a red gate, and a red gate nobody can act on
/// is a gate people stop reading. These assertions mutate a copy of the real
/// source and require the derived count to move with it, which is the property
/// the constant could not have.
#[test]
fn expected_case_count_follows_the_source_file() {
    let real = policy_source();
    let baseline = test_attribute_case_count(&real);

    assert!(
        baseline > 0,
        "{} declares no cases at all; a derivation over an empty file would \
         satisfy every comparison below vacuously",
        policy_source_path().display()
    );

    // Adding a case must move the expected total by exactly one.
    let with_extra = format!("{real}\n#[test]\nfn added_case_here() {{\n    assert!(true);\n}}\n");
    assert_eq!(
        test_attribute_case_count(&with_extra),
        baseline + 1,
        "adding one `#[test]` to policy.rs must raise the expected count by \
         exactly one — if the count is still derived from a constant, this is \
         the assertion that fails"
    );

    // Removing a case must move it back down. This is the direction that used
    // to strand the gate: a deleted case left the constant too high.
    let without_last = {
        let mut lines: Vec<&str> = real.lines().collect();
        let last_test = lines
            .iter()
            .rposition(|line| line.trim() == "#[test]")
            .expect("policy.rs must contain at least one `#[test]`");
        lines.remove(last_test);
        lines.join("\n")
    };
    assert_eq!(
        test_attribute_case_count(&without_last),
        baseline - 1,
        "removing one `#[test]` from policy.rs must lower the expected count by \
         exactly one — a constant left at the old value is what went stale"
    );

    // And the live derivation agrees with the pure counter, so the gate is not
    // comparing against some other number entirely.
    assert_eq!(
        expected_policy_case_count(),
        baseline,
        "the derivation used by the gate must agree with the counter exercised \
         above"
    );
}

/// The counter must not inflate on text that merely *mentions* the attribute.
///
/// Without this, a doc comment in `policy.rs` saying "the previous gate
/// hardcoded `#[test]`" would raise the expected count and turn the gate red —
/// i.e. the counter would have reintroduced exactly the fragility it replaced.
#[test]
fn case_counter_ignores_mentions_of_the_attribute() {
    // A doc comment mentioning it.
    assert_eq!(
        test_attribute_case_count("/// Each `#[test]` case is one decision cell.\n"),
        0,
        "a comment mentioning the attribute is not a case"
    );
    // A string literal containing it.
    assert_eq!(
        test_attribute_case_count("const S: &str = \"#[test]\";\n"),
        0,
        "a string literal holding the attribute is not a case"
    );
    // Indented but real — the attribute must still count.
    assert_eq!(
        test_attribute_case_count("    #[test]\n    fn real_case() {}\n"),
        1,
        "indentation is normal inside a `mod tests`; the attribute is still a case"
    );
    // `#[tokio::test]` is the async spelling of the same thing and counts too,
    // otherwise an async case would pass silently unchecked.
    assert_eq!(
        test_attribute_case_count("    #[tokio::test]\n    async fn async_case() {}\n"),
        1,
        "`#[tokio::test]` declares a case just as `#[test]` does"
    );
    // A different attribute must not be swept in.
    assert_eq!(
        test_attribute_case_count("#[ignore]\nfn ignored() {}\n"),
        0,
        "`#[ignore]` declares no case"
    );
}
