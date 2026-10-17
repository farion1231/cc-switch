//! Executable gate for the PATH-immunity of the `remote::auth::policy`
//! test command.
//!
//! The CI harness runs every task's `test_commands` through `/bin/sh -c`. A
//! non-interactive `/bin/sh` does not load the user's profile, so `cargo` — a
//! rustup shim living in `$HOME/.cargo/bin` — is not on PATH there. A bare
//! `cargo test ...` exits 127 no matter how healthy the code under test is.
//!
//! This file pins the PATH-immune command *form* and proves it by running it,
//! not by scanning source text. The three claims:
//!
//! 1. The command string carries the PATH shim (pure detector, no I/O).
//! 2. `cargo` resolves in the harness's shell once the shim is applied.
//! 3. The full target command runs green there, with every case passing.
//!
//! Anti-vacuity: claim 3 requires the libtest summary itself. A shell that
//! cannot find `cargo` exits non-zero, but "exit 0" alone would also be
//! satisfied by a filter that matched nothing, so the case count is parsed out
//! of the summary and compared against the number of `#[test]` attributes
//! actually present in `policy.rs`.
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

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// Path prefix that puts the rustup shim ahead of whatever the harness's
/// non-interactive shell starts with.
///
/// `$HOME`, never a literal path: the repo is cloned onto many machines.
/// Prepending is idempotent — if `$HOME/.cargo/bin` does not exist the entry
/// is simply never matched, and `cargo` still resolves from the inherited
/// PATH exactly as before.
const CARGO_PATH_SHIM: &str = r#"PATH="$HOME/.cargo/bin:$PATH""#;

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

/// `PATH` with every directory that resolves a `cargo` binary removed.
///
/// This is what makes the gate non-vacuous. `cargo test` PREPENDS
/// `$HOME/.cargo/bin` to `PATH` for the test binary it spawns, so a child
/// `/bin/sh` would inherit a PATH that already resolves `cargo` — and the shim
/// would be decorative, passing no matter what it contained. Scrubbing
/// restores the harness's real starting state, which is the only state in
/// which the shim is load-bearing.
///
/// `HOME` is preserved: the shim expands `$HOME` inside the child shell, so
/// dropping it would break the thing under test rather than harden it.
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

/// stdout + stderr merged, so assertions hold regardless of which stream the
/// toolchain happens to use.
fn merged(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// True when a command string starts with the PATH shim.
///
/// Pure over the command text, so the negative fixtures below can exercise it
/// without spawning a shell.
fn is_path_immune(command: &str) -> bool {
    command.trim_start().starts_with(CARGO_PATH_SHIM)
}

/// The `test_commands` form this gate exists to protect.
///
/// Assembled from named parts so the detector above and the executed command
/// below cannot drift apart.
fn policy_target_command() -> String {
    format!(
        r#"{CARGO_PATH_SHIM} cargo test --manifest-path "{}" --lib {POLICY_FILTER}"#,
        manifest_path().display()
    )
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
// Claim 1: the command form itself is PATH-immune.
// ---------------------------------------------------------------------------

#[test]
fn target_command_form_is_path_immune() {
    let command = policy_target_command();
    assert!(
        is_path_immune(&command),
        "the target command must begin with the PATH shim so it is executable \
         under the harness's non-interactive /bin/sh.\ncommand: {command}"
    );
    assert!(
        command.contains(POLICY_FILTER),
        "the target command lost its test-name filter; a bare `--lib` run would \
         pull in environment-flaky cases outside this gate.\ncommand: {command}"
    );
}

/// Negative fixture: the detector must reject the form this task replaced.
///
/// Without it, a detector that always returns true would leave the gate green
/// while protecting nothing.
#[test]
fn bare_command_form_is_rejected_by_detector() {
    assert!(
        !is_path_immune(&format!(
            "cargo test --manifest-path x --lib {POLICY_FILTER}"
        )),
        "the bare form this task replaced must not satisfy the detector — \
         that form exits 127 under /bin/sh"
    );
    assert!(
        !is_path_immune(""),
        "an empty command must not be read as PATH-immune"
    );
}

// ---------------------------------------------------------------------------
// Claim 2: the seam is usable — cargo resolves once the shim is applied.
// ---------------------------------------------------------------------------

#[test]
fn cargo_is_resolvable_in_sh() {
    let out = run_in_harness_shell(&format!("{CARGO_PATH_SHIM} command -v cargo"));
    let text = merged(&out);

    assert!(
        out.status.success(),
        "`command -v cargo` failed in /bin/sh even with the PATH shim \
         ({CARGO_PATH_SHIM}).\n{text}"
    );
    assert!(
        !text.trim().is_empty(),
        "`command -v cargo` succeeded but printed nothing — nothing resolved"
    );
    assert!(
        text.trim_end().ends_with("cargo"),
        "expected `command -v cargo` to resolve to a cargo binary, got: {}",
        text.trim()
    );
}

// ---------------------------------------------------------------------------
// Claim 3: the target command runs green under the harness shell.
// ---------------------------------------------------------------------------

#[test]
fn remote_auth_policy_target_passes_under_sh_with_path_shim() {
    let script = policy_target_command();
    let out = run_in_harness_shell(&script);
    let text = merged(&out);

    assert!(
        out.status.success(),
        "target command must exit 0 under the harness's /bin/sh.\n\
         command: {script}\noutput:\n{text}"
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
fn bare_cargo_is_not_resolvable_in_shell_without_shim() {
    static BASE_PATH_HAS_CARGO: OnceLock<bool> = OnceLock::new();
    let base_has_cargo = *BASE_PATH_HAS_CARGO
        .get_or_init(|| run_in_harness_shell("command -v cargo").status.success());

    if base_has_cargo {
        // Some environments export cargo onto PATH directly (CI images, shell
        // profiles). The shim is then redundant but harmless; asserting the
        // failure here would assert a property of the machine, not of the
        // command form.
        return;
    }

    let out = run_in_harness_shell(&format!(
        r#"cargo test --manifest-path "{}" --lib {POLICY_FILTER}"#,
        manifest_path().display()
    ));
    assert!(
        !out.status.success(),
        "bare `cargo` unexpectedly resolved in /bin/sh; this environment does \
         not reproduce the 127 the shim defends against, so the negative \
         fixture is vacuous here"
    );
}

// ---------------------------------------------------------------------------
// Summary parsing must reject output that never ran the cases.
// ---------------------------------------------------------------------------

#[test]
fn libtest_summary_parser_rejects_non_run_output() {
    // No summary at all: a shell that never found cargo prints this.
    assert_eq!(
        libtest_case_counts("/bin/sh: cargo: command not found"),
        None,
        "a 127 from a shell that cannot resolve cargo must not parse as a pass"
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
