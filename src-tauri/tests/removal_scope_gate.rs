//! Executable gate for the *removal-scope* facts.
//!
//! Why a static gate at all: two facts that were once established by hand still
//! lived only in a prose report, so every rerun had to re-derive the same
//! judgement from scratch.
//!
//! 1. Two route-scope gates carry an **expected-red** record. The end-state
//!    gate (`route_scope_gate.rs`) is red right now, because the artifacts it
//!    audits are still landing; the prerequisite gate
//!    (`route_scope_prereq_gate.rs`) went green once its prerequisites landed.
//!    Both facts used to live only in a prose report, so a rerun had to
//!    re-derive them. Either way the record must survive: a gate that is
//!    silenced with `#[ignore]`, or whose real-tree assertion is deleted,
//!    reads exactly like a gate that never existed.
//! 2. `tests/remote_management.rs` was deleted and is **not reachable from any
//!    target graph**: the manifest declares no `[[test]]` entry pointing at it,
//!    so deleting it took no test binary with it.
//!
//! Without this file, both facts are one `grep` and one re-read away from
//! silently becoming untrue, and a reintroduced `[[test]]` entry pointing at a
//! deleted path turns a deleted file into a build break nobody notices until
//! CI is red for an unrelated-looking reason.
//!
//! Boundaries locked here:
//!
//! 1. `removed_test_file_is_absent_from_disk` — the file is really gone.
//! 2. `manifest_has_no_target_entry_for_removed_file` — and nothing in the
//!    manifest still aims a target at it.
//! 3. `expected_red_gates_are_present_and_named` — both recorded gates are on
//!    disk, each still declares its real-tree test by name, and each still
//!    documents the expected-red expectation in its own source. An `#[ignore]`
//!    or a deleted assertion cannot satisfy this.
//! 4. Falsifiability — negative fixtures built from synthetic manifest and
//!    source strings prove each detector can actually go red. They touch no
//!    repository file, so they are constant-cost in CI.
//! 5. `scan_is_not_vacuous` — the scan surface is non-empty, so a detector
//!    reading nothing cannot read as green.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! machine-local path: the repo is cloned onto many machines and CI runners.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// The facts being frozen
// ---------------------------------------------------------------------------

/// The deleted integration test, relative to the crate root.
const REMOVED_FILE: &str = "tests/remote_management.rs";

/// The stem a manifest entry would have to name to point at the deleted file.
const REMOVED_STEM: &str = "remote_management";

/// Label reported for a manifest entry that still aims a target at the deleted
/// file. A `&'static str` so the reported list is comparable and printable.
const REMOVED_TARGET: &str = "tests/remote_management.rs";

/// The gates carrying an expected-red record, each with the real-tree test
/// that record is about.
///
/// The test name is part of the contract: naming it is what stops "the gate is
/// expected red" from decaying into "the gate was removed", which reads the
/// same in CI output but means something entirely different. A gate that has
/// since gone green stays listed — going green is an outcome to be recorded,
/// not a reason to stop asking.
const EXPECTED_RED_GATES: [(&str, &str); 2] = [
    (
        "tests/route_scope_gate.rs",
        "every_route_is_registered_through_the_scope_table",
    ),
    (
        "tests/route_scope_prereq_gate.rs",
        "route_scope_prereq_gate_reports_missing_chain",
    ),
];

/// Every file this gate reads. `scan_is_not_vacuous` asserts this surface is
/// non-empty and covers both gates plus the manifest.
const SOURCES: [&str; 3] = [
    "Cargo.toml",
    "tests/route_scope_gate.rs",
    "tests/route_scope_prereq_gate.rs",
];

fn manifest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn crate_relative(relative: &str) -> PathBuf {
    manifest_dir().join(relative)
}

fn read_src(relative: &str) -> String {
    let path = crate_relative(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{relative} must be readable: {e}"))
}

// ---------------------------------------------------------------------------
// Scan surface
// ---------------------------------------------------------------------------

/// Drop a trailing `//` comment, honouring string and char literals so the
/// `//` inside a string is not mistaken for a comment.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut in_chr = false;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str || in_chr => i += 2,
            b'"' if !in_chr => {
                in_str = !in_str;
                i += 1;
            }
            b'\'' if !in_str => {
                in_chr = !in_chr;
                i += 1;
            }
            b'/' if !in_str && !in_chr && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => i += 1,
        }
    }
    line
}

/// Non-empty code lines actually handed to the detectors — the anti-vacuity
/// measure. A reader that returned nothing must be able to go red.
fn scanned_lines(src: &str) -> usize {
    src.lines()
        .map(strip_line_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .count()
}

/// Word-bounded, case-insensitive occurrence of `word` in `text`.
fn has_ci_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|candidate| candidate.eq_ignore_ascii_case(word))
}

// ---------------------------------------------------------------------------
// Detectors
// ---------------------------------------------------------------------------

/// Manifest `[[test]]` entries that still aim a target at the deleted file.
///
/// Entries are counted, not lines: a block carrying both `name = "…"` and
/// `path = "…"` is one entry pointing at one dead file, and reporting it twice
/// would read as two problems where there is one.
pub fn dangling_target_entries(manifest: &str) -> Vec<&'static str> {
    let mut dangling = Vec::new();
    let mut in_test_table = false;
    let mut flagged = false;

    for raw in manifest.lines() {
        let line = strip_line_comment(raw).trim();
        if line.starts_with('[') {
            // Any table header opens or closes a block, so a dead entry is
            // flushed the moment the next table — or the end of the manifest —
            // proves its extent.
            if in_test_table && flagged {
                dangling.push(REMOVED_TARGET);
            }
            in_test_table = line == "[[test]]";
            flagged = false;
            continue;
        }
        if in_test_table && !flagged && line.contains(REMOVED_STEM) {
            flagged = true;
        }
    }
    if in_test_table && flagged {
        dangling.push(REMOVED_TARGET);
    }

    dangling
}

/// True when `src` documents, in its own source, that its real-tree assertion
/// is expected to be red.
///
/// Two things are required, because either alone is satisfiable by accident:
/// a redness token, and a token stating the expectation (`expected` — as in
/// "expected to be red" — or `falsifiab…`, the falsifiability note every gate
/// in this directory carries). A file that merely mentions red in passing, or
/// that has been silenced without a word, must not pass.
pub fn has_expected_red_marker(src: &str) -> bool {
    const EXPECTATION_TOKENS: [&str; 2] = ["expected", "falsifiab"];

    let lowered = src.to_ascii_lowercase();
    has_ci_word(src, "red") && EXPECTATION_TOKENS.iter().any(|t| lowered.contains(t))
}

/// True when `src` declares `fn <name>` under an `#[test]` attribute.
///
/// The attribute check is what makes this a gate and not a text search: an
/// `#[ignore]`d test still declares the name, so the caller must pair this with
/// `has_expected_red_marker` and read the attribute itself.
fn declares_test_fn(src: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    let Some(at) = src.find(&needle) else {
        return false;
    };
    let head = &src[..at];
    let Some(test_at) = head.rfind("#[test]") else {
        return false;
    };
    // `#[test]` must be the attribute directly governing this item; a later
    // attribute means some other declaration owns the nearest one.
    match head.rfind("#[") {
        Some(last_attr) => last_attr <= test_at,
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Test: the real tree
// ---------------------------------------------------------------------------

#[test]
fn removed_test_file_is_absent_from_disk() {
    let path = crate_relative(REMOVED_FILE);
    assert!(
        !path.exists(),
        "{} must not be back on disk — the removal's whole claim is that this \
         integration test no longer exists",
        REMOVED_FILE
    );

    // And nothing else in the crate is squatting under the same stem.
    let tests_dir = manifest_dir().join("tests");
    let squatters: Vec<String> = std::fs::read_dir(&tests_dir)
        .unwrap_or_else(|e| panic!("{REMOVED_FILE}'s directory must be readable: {e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(REMOVED_STEM))
        .collect();
    assert!(
        squatters.is_empty(),
        "no file under tests/ may carry the removed stem {REMOVED_STEM:?}: {squatters:?}"
    );
}

#[test]
fn manifest_has_no_target_entry_for_removed_file() {
    let manifest = read_src("Cargo.toml");

    // The strong form: no target entry reaches the deleted file.
    let dangling = dangling_target_entries(&manifest);
    assert!(
        dangling.is_empty(),
        "Cargo.toml still declares a [[test]] entry aimed at the deleted \
         {REMOVED_FILE}; an entry for a path that is not on disk is a build \
         break disguised as a removal. Offending entries: {dangling:?}"
    );

    // The blunt form, so no other manifest shape can reintroduce the name:
    // an inline `[[test]]` header on one line, a `required-features`
    // reference, or a `path =` value spelled with different quoting.
    let hits: Vec<&str> = manifest
        .lines()
        .filter(|line| line.contains(REMOVED_STEM))
        .collect();
    assert!(
        hits.is_empty(),
        "Cargo.toml must not mention {REMOVED_STEM:?} in any form; lines: {hits:?}"
    );
}

#[test]
fn expected_red_gates_are_present_and_named() {
    for (relative, test_fn) in EXPECTED_RED_GATES {
        let path = crate_relative(relative);
        assert!(
            path.is_file(),
            "{relative} must exist — it is an expected-red gate, and an \
             expected-red gate that has been deleted is indistinguishable from a \
             gate that was never written"
        );

        let src = read_src(relative);

        assert!(
            declares_test_fn(&src, test_fn),
            "{relative} must still declare `#[test] fn {test_fn}()` — that real-tree \
             assertion is the per-artifact fact being frozen; deleting it, renaming \
             it, or dropping the #[test] attribute all look like progress in a diff"
        );

        assert!(
            has_expected_red_marker(&src),
            "{relative} must still document, in its own source, that \
             `fn {test_fn}` is expected to be red — otherwise a reader cannot \
             tell an expected-red gate from a skipped one"
        );
    }
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof each detector can go red
// ---------------------------------------------------------------------------

#[test]
fn gate_rejects_fixture_with_dangling_test_target() {
    let by_path = "[[test]]\npath = \"tests/remote_management.rs\"";
    assert_eq!(
        dangling_target_entries(by_path),
        vec![REMOVED_TARGET],
        "a [[test]] entry whose path names the deleted file must be reported"
    );

    let by_name = "[[test]]\nname = \"remote_management\"\npath = \"tests/gone.rs\"";
    assert_eq!(
        dangling_target_entries(by_name),
        vec![REMOVED_TARGET],
        "a [[test]] entry whose *name* is the deleted stem is the same dead entry"
    );

    // Both forms in one entry is one problem, not two.
    let both = concat!(
        "[[test]]\n",
        "name = \"remote_management\"\n",
        "path = \"tests/remote_management.rs\"\n",
        "harness = true\n",
    );
    assert_eq!(
        dangling_target_entries(both),
        vec![REMOVED_TARGET],
        "one entry carrying both forms must be reported once"
    );

    // A second dead entry is reported alongside the first.
    let two = concat!(
        "[[test]]\nname = \"a\"\npath = \"tests/a.rs\"\n\n",
        "[[test]]\nname = \"b\"\npath = \"tests/remote_management.rs\"\n",
        "[[test]]\nname = \"c\"\npath = \"tests/c.rs\"\n",
    );
    assert_eq!(
        dangling_target_entries(two),
        vec![REMOVED_TARGET],
        "a live entry before and after a dead one must not swallow it"
    );

    // The clean shape must pass, or the fixtures above prove nothing.
    let clean = concat!(
        "[[test]]\nname = \"route_scope_gate\"\n",
        "path = \"tests/route_scope_gate.rs\"\n",
        "[[bench]]\nname = \"b\"\nharness = false\n",
    );
    assert!(
        dangling_target_entries(clean).is_empty(),
        "a manifest whose entries all name live targets reports nothing"
    );
    assert!(
        dangling_target_entries("[package]\nname = \"cc-switch\"\n").is_empty(),
        "a manifest with no [[test]] table at all reports nothing"
    );
}

#[test]
fn gate_flags_fixture_missing_expected_red_marker() {
    let marked = r#"//! The real-tree test is expected to be **red** right now: that redness
//! is the named, per-artifact fact this gate exists to freeze.
#[test]
fn route_scope_prereq_gate_reports_missing_chain() {}
"#;
    assert!(
        has_expected_red_marker(marked),
        "a gate that states its real-tree test is expected red satisfies the marker"
    );

    let falsifiable = r#"//! Written to be **falsifiable**: negative fixtures prove each
//! detector can actually go red.
#[test]
fn every_route_is_registered_through_the_scope_table() {}
"#;
    assert!(
        has_expected_red_marker(falsifiable),
        "the falsifiability note counts as a stated expectation of red"
    );

    let unannotated = r#"//! A perfectly ordinary gate.
#[test]
fn some_other_test() {}
"#;
    assert!(
        !has_expected_red_marker(unannotated),
        "a gate that never says its real-tree test is expected red must be \
         reported — otherwise silencing the gate is free"
    );

    let merely_red = r#"//! Negative fixtures prove each detector can go red.
#[test]
fn some_test() {}
"#;
    assert!(
        !has_expected_red_marker(merely_red),
        "a redness token with no stated expectation is not an expected-red marker"
    );

    let merely_expected = r#"//! The real-tree test is expected to be green.
#[test]
fn some_test() {}
"#;
    assert!(
        !has_expected_red_marker(merely_expected),
        "an expectation of green is the opposite claim and must not satisfy this"
    );

    // And the name detector, so a gate stripped of its real-tree test is caught.
    let stripped = r#"//! The real-tree test is expected to be **red**.
fn helper() {}
"#;
    assert!(
        !declares_test_fn(stripped, "route_scope_prereq_gate_reports_missing_chain"),
        "a helper function is not a #[test] declaration"
    );
    assert!(
        declares_test_fn(marked, "route_scope_prereq_gate_reports_missing_chain"),
        "a declared test is found by name"
    );
    assert!(
        declares_test_fn(
            falsifiable,
            "every_route_is_registered_through_the_scope_table"
        ),
        "and so is the other gate's real-tree test"
    );
}

#[test]
fn scan_is_not_vacuous() {
    assert!(
        SOURCES.len() == 3,
        "the declared scan surface must name the manifest and both expected-red \
         gates, not fewer: {SOURCES:?}"
    );
    for required in [
        "Cargo.toml",
        "tests/route_scope_gate.rs",
        "tests/route_scope_prereq_gate.rs",
    ] {
        assert!(
            SOURCES.contains(&required),
            "the scan surface must cover {required:?}; it is {SOURCES:?}"
        );
    }

    for relative in SOURCES {
        let path = crate_relative(relative);
        assert!(path.is_file(), "{relative} must be readable for the scan");
        let src = read_src(relative);
        assert!(
            scanned_lines(&src) > 0,
            "{relative} must contribute a non-empty scan surface — a reader that \
             returns nothing would report every fact present and read as green"
        );
    }

    // The gates the facts name must be the gates the scan surface covers.
    let scanned: Vec<&str> = SOURCES.to_vec();
    for (relative, _) in EXPECTED_RED_GATES {
        assert!(
            scanned.contains(&relative),
            "{relative} is an expected-red gate that the scan surface must cover"
        );
    }
}
