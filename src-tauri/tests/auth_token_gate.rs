//! Executable structural gate for the client-credential layer
//! (`src-tauri/src/remote/auth/token.rs`).
//!
//! The crate-internal tests in `token.rs` only run when *that* cargo
//! invocation succeeds. This gate makes the same boundaries provable from a
//! separate, self-contained integration test that reads source **text** — so
//! the claims hold even if a `cargo test --lib` run is skipped, filtered, or
//! dies for environmental reasons.
//!
//! Six boundaries, one assertion each, over `token.rs` specifically:
//!
//! 1. `direct_subtle_imports` — `subtle` is a transitive dep reached through
//!    `hmac`'s `Mac::verify_slice`. Importing it directly pins it into the
//!    public dependency surface, which the task forbade.
//! 2. `token_module_is_registered` — `mod token;` in `auth/mod.rs`; without it
//!    the module is dead code that compiles but is never reachable.
//! 3. `digest_comparison_has_single_entry` — the only digest comparison entry
//!    is `Mac::verify_slice`; a bare `==` on a `[u8; 32]` is short-circuiting
//!    and leaks the matching prefix length.
//! 4. `ct_eq_32_has_no_early_return` — the accumulate-then-compare helper must
//!    run all 32 iterations; `return`/`break`/`continue` would reintroduce the
//!    timing leak.
//! 5. `token_uses_existing_deps` — `base64`/`uuid`/`sha2`/`hmac` are already in
//!    `Cargo.toml`, and the `use` tree pulls in nothing outside std + those four.
//! 6. `no_abs_paths_in_task_files` — no machine-local absolute path ships.
//!
//! Plus boundaries that do **not** name a file:
//!
//! 7. `digest_short_circuit_lines` — `std::fs::read_dir` walks
//!    `src/remote/` recursively, collects every `.rs`, and rejects any line that
//!    carries a digest identifier *and* a short-circuiting comparison.
//!    The point is that adding a file under the subtree needs no edit here —
//!    every earlier round of this gate failed exactly because its scan surface
//!    was a hand-maintained list that each new file had to be added to.
//!
//! 8. `scope_digest_sites` / `scope_subtle_sites` — boundary 7 is a superset
//!    scan; boundary 8 narrows it to the subtrees that actually matter. Its
//!    roots are the two *directories* [`SCOPE_ROOTS`] (`src/remote/auth/`,
//!    `src/remote/pairing/`) and each is walked recursively, so the gate's
//!    surface is the subtrees rather than the two files an earlier round named.
//!    A test file named in a constant is a manifest every new file has to be
//!    appended to; a directory root is not. `pairing/handlers.rs` — where the
//!    production dedupe compares digests — is inside boundary 8's surface by
//!    construction, not because someone remembered to list it.
//!
//! 8's detector set is deliberately wider than 7's, because 7's identifier list
//!    (`digest` / `hmac_digest` / `stored_digest` / `secret_digest`) does not
//!    contain the two fields that actually hold a digest in production:
//!    `token_hash` and `cookie_hash`. A `c.token_hash == issued` slipped past 7
//!    for exactly that reason. 8 also rejects a third comparison *spelling* —
//!    `.eq(` / `!=` — so the rule is not "the two forms we happened to write
//!    down" but "any comparison other than `ct_eq` / `verify_slice`".
//!
//! Two properties the widened scope would be worthless without:
//!
//! - **Scope is proven, not assumed.** `scope_walk_is_not_vacuous` asserts the
//!   walk read more than two files and that *both* declared roots contributed —
//!   a fixture planted under a `pairing/handlers.rs`-shaped path must be caught.
//! - **Negative fixtures survive.** The existing `gate_rejects_fixture_*` set is
//!   kept unchanged, so widening the surface did not buy a green gate by
//!   weakening the detectors.
//!
//! Two properties this gate would be worthless without:
//!
//! - **Comments are excluded.** `token.rs`'s module doc explains *why* `subtle`
//!   and `verify_slice` are off-limits, so a naive substring scan would flag
//!   the explanation. Only code is scanned.
//! - **Negative fixtures.** Synthetic sources — plus, for boundary 7, a real
//!   walk over a `tempfile` tree with a violation planted two directories deep —
//!   prove the detectors can actually go red; a detector that always returns
//!   empty leaves the gate green while protecting nothing.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! `/Users/...`: the repo is cloned onto many machines and CI runners.

use std::path::Path;

// ---------------------------------------------------------------------------
// Source anchors
// ---------------------------------------------------------------------------

const TOKEN_RS: &str = "src/remote/auth/token.rs";
const AUTH_MOD_RS: &str = "src/remote/auth/mod.rs";
const CARGO_TOML: &str = "Cargo.toml";

/// The two files task 6 introduced — the ones that must stay portable.
const TASK_6_FILES: [&str; 2] = [TOKEN_RS, AUTH_MOD_RS];

/// Dependencies task 6 was allowed to *use*, all pre-existing.
const REQUIRED_DEPS: [&str; 4] = ["base64", "uuid", "sha2", "hmac"];

/// Import roots the `use` tree may name: std family plus the four allowed
/// crates. Anything else is a new dependency sneaking in through a `use`.
fn allowed_import_roots() -> Vec<&'static str> {
    let mut roots: Vec<&'static str> =
        vec!["std", "core", "alloc", "crate", "self", "super", "Self"];
    roots.extend_from_slice(&REQUIRED_DEPS);
    roots
}

fn read_src(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Lexical helpers
// ---------------------------------------------------------------------------

/// Drop a trailing `//` comment, honouring string and char literals so that a
/// `//` inside `"https://x"` is not mistaken for a comment.
///
/// Only `//` is handled (no block comments in this gate's inputs). A line that
/// is *entirely* a comment therefore collapses to empty and drops out.
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

/// `(1-based line number, comment-stripped trimmed code)` for every line that
/// still holds code. Comment lines are absent by construction — that is how
/// the module doc's `subtle` / `verify_slice` mentions stay unflagged.
fn code_lines(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .map(|(i, line)| (i + 1, strip_line_comment(line).trim().to_string()))
        .filter(|(_, code)| !code.is_empty())
        .collect()
}

/// The implementation region: everything before the first `#[cfg(test)]`.
///
/// The crate's own in-file gate scans the same region, because its assertion
/// text literally contains `"use subtle"` and would otherwise flag itself. The
/// `#[cfg(test)]` block is not shipped code and is covered by `cargo test
/// --lib` instead.
fn impl_region(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

/// First path segment of a `use` statement: `use hmac::{Hmac, Mac};` -> `hmac`.
fn first_use_segment(stmt: &str) -> String {
    let body = stmt
        .trim_start()
        .trim_start_matches("use ")
        .trim_start_matches("::");
    body.split(|c: char| c == ':' || c == '{' || c == ';' || c.is_whitespace())
        .find(|segment| !segment.is_empty())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// Boundary 1: no direct `subtle` import
// ---------------------------------------------------------------------------

fn direct_subtle_imports(src: &str) -> Vec<usize> {
    code_lines(impl_region(src))
        .into_iter()
        .filter(|(_, code)| code.contains("use subtle") || code.contains("subtle::"))
        .map(|(line, _)| line)
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 2: the module is registered
// ---------------------------------------------------------------------------

fn token_module_is_registered(auth_mod_rs: &str) -> bool {
    code_lines(auth_mod_rs)
        .into_iter()
        .any(|(_, code)| code == "mod token;")
}

// ---------------------------------------------------------------------------
// Boundary 3: digest comparison has a single entry
// ---------------------------------------------------------------------------

/// Yield `(left operand, right operand)` around the first real `==` in `code`
/// (skipping `!=`, `>=`, `<=`, `=>`).
///
/// Each operand is the *maximal munch of value characters* abutting the
/// operator, not "the rest of the line". The difference matters: in
/// `pub fn same(a: &[u8; 32], b: &[u8; 32]) -> bool { a == b }` a naive
/// rest-of-line split makes the left operand the whole signature plus `a`,
/// which is not an operand and matches nothing. Munching back from the
/// operator recovers the `a`.
fn bare_eq_operands(code: &str) -> Option<(String, String)> {
    let bytes = code.as_bytes();
    let is_value_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'=' && bytes[i + 1] == b'=' {
            if i > 0 && matches!(bytes[i - 1], b'!' | b'<' | b'>' | b'=') {
                i += 2;
                continue;
            }

            let mut start = i;
            // Skip the gap before the left operand too — `a == b` spaces *both*
            // values away from the operator, and munching straight back from it
            // would find nothing at all.
            while start > 0 && bytes[start - 1].is_ascii_whitespace() {
                start -= 1;
            }
            while start > 0 && is_value_byte(bytes[start - 1]) {
                start -= 1;
            }
            // Skip the gap before the right operand: `a == b` spaces the value
            // away from the operator, and munching from the operator alone
            // would stop on the space and find no right operand at all.
            let mut end = i + 2;
            while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            while end < bytes.len() && is_value_byte(bytes[end]) {
                end += 1;
            }

            return Some((code[start..i].to_string(), code[i + 2..end].to_string()));
        }
        i += 1;
    }
    None
}

/// True when a fragment names a value — `a`, `&b`, `self.token_hash`,
/// `*mut c` — rather than a literal. `diff == 0` fails this on the `0`, which is
/// exactly what lets the post-loop accumulator through.
fn is_value_operand(operand: &str) -> bool {
    let bare = operand
        .trim()
        .trim_start_matches('&')
        .trim_start_matches('*')
        .trim();
    let bare = bare.strip_prefix("mut ").unwrap_or(bare);
    if bare.is_empty() || matches!(bare, "true" | "false" | "None" | "Some") {
        return false;
    }
    if !bare
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        return false;
    }
    bare.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

fn digest_comparison_has_single_entry(src: &str) -> Vec<usize> {
    code_lines(impl_region(src))
        .into_iter()
        .filter(|(_, code)| !code.starts_with("#["))
        .filter(|(_, code)| match bare_eq_operands(code) {
            Some((left, right)) => is_value_operand(&left) && is_value_operand(&right),
            None => false,
        })
        .map(|(line, _)| line)
        .collect()
}

/// True when the single entry point really exists — without this, a file that
/// deleted every comparison would satisfy "no bare `==`" while comparing nothing.
fn has_verify_slice_entry(src: &str) -> bool {
    code_lines(impl_region(src))
        .into_iter()
        .any(|(_, code)| code.contains("verify_slice"))
}

// ---------------------------------------------------------------------------
// Boundary 4: `ct_eq_32` has no early return
// ---------------------------------------------------------------------------

/// Extract `fn ct_eq_32`'s body by brace matching over comment-stripped code.
fn ct_eq_32_body(src: &str) -> Option<String> {
    let cleaned: String = code_lines(impl_region(src))
        .into_iter()
        .map(|(_, code)| code)
        .collect::<Vec<_>>()
        .join("\n");
    let start = cleaned.find("fn ct_eq_32")?;
    let rest = &cleaned[start..];
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                if depth == 1 {
                    return Some(rest[..=i].to_string());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn contains_early_exit(code: &str) -> bool {
    code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|token| matches!(token, "return" | "break" | "continue"))
}

/// 1-based line numbers (within the extracted body) where the loop could exit
/// before consuming all 32 bytes.
fn ct_eq_32_has_no_early_return(src: &str) -> Vec<usize> {
    let body = ct_eq_32_body(src).unwrap_or_default();
    body.lines()
        .enumerate()
        .filter(|(_, code)| contains_early_exit(code))
        .map(|(i, _)| i + 1)
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 5: only pre-existing dependencies
// ---------------------------------------------------------------------------

/// True when `Cargo.toml` declares `dep` in `[dependencies]`, matching the
/// `name = ...` line shape (not a `[package] name = ` or a substring hit).
fn cargo_declares(cargo_toml: &str, dep: &str) -> bool {
    cargo_toml.lines().any(|line| {
        let line = line.trim();
        line.len() > dep.len()
            && line.starts_with(dep)
            && line[dep.len()..].trim_start().starts_with('=')
    })
}

/// Roots named by `use` statements in the implementation region.
fn import_roots(src: &str) -> Vec<String> {
    code_lines(impl_region(src))
        .into_iter()
        .filter(|(_, code)| code.starts_with("use "))
        .map(|(_, code)| first_use_segment(&code))
        .filter(|root| !root.is_empty())
        .collect()
}

/// `'static` leak only on the violation path, so a clean run allocates nothing
/// and the return type can name the offending root.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn token_uses_existing_deps(src: &str, cargo_toml: &str) -> Vec<&'static str> {
    let mut violations: Vec<&'static str> = Vec::new();

    for dep in REQUIRED_DEPS {
        if !cargo_declares(cargo_toml, dep) {
            violations.push(dep);
        }
    }

    let allowed = allowed_import_roots();
    for root in import_roots(src) {
        if !allowed.contains(&root.as_str()) {
            violations.push(leak(root));
        }
    }

    violations
}

// ---------------------------------------------------------------------------
// Boundary 6: nothing machine-local ships
// ---------------------------------------------------------------------------

fn abs_path_sites(src: &str) -> Vec<usize> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| {
            line.contains("/Users/") || line.contains("/home/") || line.contains("C:\\")
        })
        .map(|(i, _)| i + 1)
        .collect()
}

/// 1-based line numbers of absolute-path hits. With several sources the numbers
/// are per-source; call it with one element when the line number matters.
fn no_abs_paths_in_task_files(sources: &[&str]) -> Vec<usize> {
    sources.iter().flat_map(|src| abs_path_sites(src)).collect()
}

// ---------------------------------------------------------------------------
// Boundary 7: directory walk — no file manifest, ever
// ---------------------------------------------------------------------------

/// Digest-bearing identifiers. A line naming one of these **and** carrying a
/// non-constant-time comparison is the shape this boundary exists to reject.
///
/// `hmac_digest` / `stored_digest` / `secret_digest` are listed explicitly even
/// though `digest` is their suffix: naming the whole family is what keeps the
/// set honest when someone picks the longer spelling.
const DIGEST_IDENTIFIERS: [&str; 4] = ["digest", "hmac_digest", "stored_digest", "secret_digest"];

/// Short-circuiting comparisons on one line. `==` is the array/slice `PartialEq`
/// path; `slice::eq` is the same thing spelled as a method.
fn short_circuit_operators(code: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    if code.contains("==") {
        hits.push("==");
    }
    if code.contains("slice::eq") {
        hits.push("slice::eq");
    }
    hits
}

/// 1-based line numbers where a digest identifier shares a line with a
/// short-circuiting comparison.
fn digest_short_circuit_lines(src: &str) -> Vec<usize> {
    code_lines(src)
        .into_iter()
        .filter(|(_, code)| DIGEST_IDENTIFIERS.iter().any(|id| code.contains(id)))
        .filter(|(_, code)| !short_circuit_operators(code).is_empty())
        .map(|(line, _)| line)
        .collect()
}

/// Every `.rs` file under `src/remote/`, discovered by walking the directory.
///
/// No file names appear here — not as constants, not as literals, not in an
/// array. Adding a file under the subtree automatically widens the scan; that
/// is the whole point, and the previous rounds' failure was precisely a scan
/// whose surface had to be edited by hand every time a file was added.
fn remote_rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("dir entry must be readable").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
                out.push((path.display().to_string(), src));
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/remote");
    let mut out = Vec::new();
    walk(&root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `(file, line)` for every short-circuiting digest comparison anywhere under
/// `src/remote/`.
fn digest_short_circuit_sites_in_remote_tree() -> Vec<(String, usize)> {
    remote_rust_sources()
        .into_iter()
        .flat_map(|(path, src)| {
            digest_short_circuit_lines(&src)
                .into_iter()
                .map(move |line| (path.clone(), line))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 8: the scope that matters — directories, walked recursively
// ---------------------------------------------------------------------------

/// The two subtrees in scope: `src/remote/auth/` and
/// `src/remote/pairing/`.
///
/// These are **roots, not a file list**. An earlier round of this gate named
/// `auth/token.rs` and `auth/mod.rs` as constants and scanned only those two; the
/// production dedupe in `pairing/handlers.rs` was therefore outside the gate and
/// held only by human eyes. Naming a *directory* means a file added under either
/// subtree tomorrow is in scope without an edit here.
const SCOPE_ROOTS: [&str; 2] = ["src/remote/auth", "src/remote/pairing"];

/// Digest-bearing identifiers for boundary 8.
///
/// `digest` / `hmac_digest` / `stored_digest` / `secret_digest` come from
/// boundary 7's list. Three more are added because those are the spellings that
/// actually hold or carry a digest in production:
///
/// - `ClientDigest` — the **type** name, capitalised. A comparison written as
///   `fn f(a: ClientDigest, b: ClientDigest) { a == b }` contains no lowercase
///   `digest` substring at all, so a case-sensitive list misses the type's own
///   primary spelling;
/// - `token_hash` / `cookie_hash` — the fields that hold one. A
///   `ClientToken` field declaration carries no `digest` substring, so boundary
///   7's list missed the only digest comparisons outside `token.rs`.
const SCOPE_DIGEST_IDENTIFIERS: [&str; 7] = [
    "digest",
    "Digest",
    "hmac_digest",
    "stored_digest",
    "secret_digest",
    "token_hash",
    "cookie_hash",
];

/// Comparison spellings that are all short-circuiting on `[u8; 32]`.
///
/// `ct_eq` is deliberately absent: it is the sanctioned entry, not a violation.
/// `.eq(` is the method spelling that 7 did not cover. `assert_eq!` matters
/// because it *expands* to `==` — the source text carries no `==` token, so an
/// operator scan alone would wave `assert_eq!(a.token_hash, b)` through.
fn scope_comparison_spellings(code: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    for spelling in ["==", "!=", "slice::eq", ".eq(", "assert_eq!", "assert_ne!"] {
        if code.contains(spelling) {
            hits.push(spelling);
        }
    }
    hits
}

/// 1-based line numbers where a scope digest identifier shares a line with a
/// short-circuiting comparison.
///
/// Scans the impl region, the same split [`direct_subtle_imports`] uses and the
/// one `pairing/handlers.rs:851` already applies in production. Test code is
/// not shipped and is covered by `cargo test --lib`; scanning it here would
/// flag assertions like `assert_eq!(t.token_hash.as_bytes().len(), 32)`, which
/// compares a `usize` length and a `&str` — a digest identifier on the line but
/// no digest comparison. Flagging those would make the gate cry wolf on its own
/// crate's honest tests.
fn scope_digest_short_circuit_lines(src: &str) -> Vec<usize> {
    code_lines(impl_region(src))
        .into_iter()
        .filter(|(_, code)| SCOPE_DIGEST_IDENTIFIERS.iter().any(|id| code.contains(id)))
        .filter(|(_, code)| !scope_comparison_spellings(code).is_empty())
        .map(|(line, _)| line)
        .collect()
}

/// 1-based line numbers carrying a direct `subtle` reference in code.
///
/// Scans the impl region only: `token.rs`'s own in-file gate asserts on the
/// literal `"use subtle"`, which would otherwise flag itself. Same split its
/// sibling [`direct_subtle_imports`] uses.
fn scope_subtle_lines(src: &str) -> Vec<usize> {
    code_lines(impl_region(src))
        .into_iter()
        .filter(|(_, code)| code.contains("use subtle") || code.contains("subtle::"))
        .map(|(line, _)| line)
        .collect()
}

/// Every `.rs` file under the two declared scope roots, walked recursively.
///
/// Panics if a declared root is missing — a root that quietly stopped resolving
/// would shrink the gate's surface to whatever is left and still report green.
fn scope_rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("dir entry must be readable").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
                out.push((path.display().to_string(), src));
            }
        }
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for root in SCOPE_ROOTS {
        walk(&manifest.join(root), &mut out);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `(file, line, kind)` for every violation found anywhere in scope.
fn scope_violation_sites() -> Vec<(String, usize, String)> {
    let sources = scope_rust_sources();
    scope_violation_sites_from(&sources)
}

// ---------------------------------------------------------------------------
// Tests: the real tree
// ---------------------------------------------------------------------------

#[test]
fn token_file_is_present_and_nonempty() {
    let src = read_src(TOKEN_RS);
    assert!(!src.trim().is_empty(), "{TOKEN_RS} must not be empty");
    assert!(
        code_lines(&src).len() > 20,
        "{TOKEN_RS} scanned to {} code lines — the gate would be near-vacuous",
        code_lines(&src).len()
    );
}

#[test]
fn no_direct_subtle_import_in_real_tree() {
    let src = read_src(TOKEN_RS);
    let hits = direct_subtle_imports(&src);
    assert!(
        hits.is_empty(),
        "{TOKEN_RS} must not reference `subtle` in code — it is a transitive \
         dependency reached through `Mac::verify_slice`; a direct import would \
         pin it into the public dependency surface. Offending lines: {hits:?}"
    );

    // The module doc *does* mention `subtle`; proving that while the scan
    // stays green is what shows comments are excluded rather than lucky.
    assert!(
        src.contains("subtle") && src.contains("use subtle"),
        "fixture drifted: the doc is expected to explain `use subtle` in prose"
    );
}

#[test]
fn token_module_is_registered_in_real_mod_rs() {
    let auth_mod = read_src(AUTH_MOD_RS);
    assert!(
        token_module_is_registered(&auth_mod),
        "{AUTH_MOD_RS} must contain `mod token;` — otherwise token.rs compiles \
         but is unreachable from the auth tree"
    );
}

#[test]
fn digest_comparison_uses_verify_slice_only() {
    let src = read_src(TOKEN_RS);
    let bare = digest_comparison_has_single_entry(&src);
    assert!(
        bare.is_empty(),
        "{TOKEN_RS} must not compare digests with a bare `==` — PartialEq on \
         `[u8; 32]` short-circuits at the first differing byte and leaks the \
         matching prefix length. Offending lines: {bare:?}"
    );
    assert!(
        has_verify_slice_entry(&src),
        "{TOKEN_RS} must keep `Mac::verify_slice` as the single comparison entry; \
         a file with no bare `==` *and* no `verify_slice` compares nothing"
    );
}

#[test]
fn ct_eq_32_has_no_early_return_in_real_tree() {
    let src = read_src(TOKEN_RS);
    assert!(
        ct_eq_32_body(&src).is_some(),
        "{TOKEN_RS} must define `fn ct_eq_32` for this gate to mean anything"
    );
    let hits = ct_eq_32_has_no_early_return(&src);
    assert!(
        hits.is_empty(),
        "`ct_eq_32` must accumulate all 32 bytes and return only after the loop; \
         an early exit re-couples runtime to the matching prefix. Offending body \
         lines: {hits:?}"
    );
}

#[test]
fn deps_are_pre_existing_and_no_new_crate() {
    let src = read_src(TOKEN_RS);
    let cargo_toml = read_src(CARGO_TOML);

    let violations = token_uses_existing_deps(&src, &cargo_toml);
    assert!(
        violations.is_empty(),
        "{TOKEN_RS} may use only pre-existing deps ({}); violations: {violations:?}",
        REQUIRED_DEPS.join(", ")
    );

    for dep in REQUIRED_DEPS {
        assert!(
            cargo_declares(&cargo_toml, dep),
            "{CARGO_TOML} must already declare `{dep}`"
        );
    }
    assert!(
        !cargo_declares(&cargo_toml, "subtle"),
        "{CARGO_TOML} must not declare `subtle` — task 6 added no crate, and \
         `subtle` must stay transitive"
    );
}

#[test]
fn task_files_contain_no_absolute_paths() {
    let sources: Vec<String> = TASK_6_FILES.iter().map(|rel| read_src(rel)).collect();
    let borrowed: Vec<&str> = sources.iter().map(String::as_str).collect();

    for (relative, src) in TASK_6_FILES.iter().zip(sources.iter()) {
        let hits = no_abs_paths_in_task_files(&[src.as_str()]);
        assert!(
            hits.is_empty(),
            "{relative} must not embed a machine-local absolute path \
             (`/Users/`, `/home/`, `C:\\`); offending lines: {hits:?}"
        );
    }

    assert!(
        no_abs_paths_in_task_files(&borrowed).is_empty(),
        "multi-file scan over {:?} found absolute paths",
        TASK_6_FILES
    );
}

#[test]
fn scan_is_not_vacuous() {
    let src = read_src(TOKEN_RS);
    let cargo_toml = read_src(CARGO_TOML);
    let auth_mod = read_src(AUTH_MOD_RS);

    let scanned = code_lines(&src).len();
    assert!(
        scanned >= 40,
        "only {scanned} code lines were scanned in {TOKEN_RS}; a gate over an \
         almost-empty scan proves nothing"
    );

    // Every detector must have had real, non-trivial input to look at.
    let impl_scanned = code_lines(impl_region(&src)).len();
    assert!(
        impl_scanned > 10 && impl_scanned < scanned,
        "the impl region should be a large, non-whole share of the file \
         (impl {impl_scanned} of {scanned}) — otherwise the `#[cfg(test)]` \
         split is not doing what this gate assumes"
    );
    assert!(
        ct_eq_32_body(&src).is_some(),
        "`ct_eq_32` must be locatable or boundaries 1-4 are scanning nothing"
    );
    assert!(
        !import_roots(&src).is_empty(),
        "no `use` statements found — boundary 5 would pass on an empty set"
    );
    assert!(
        auth_mod.lines().any(|l| l.trim_start().starts_with("mod ")),
        "no `mod` declarations found in {AUTH_MOD_RS}"
    );
    assert!(
        !REQUIRED_DEPS.is_empty() && allowed_import_roots().len() > REQUIRED_DEPS.len(),
        "the allowed import-root set must be std plus the pre-existing deps"
    );
    assert!(
        cargo_toml.contains("hmac") && cargo_toml.contains("sha2"),
        "{CARGO_TOML} must still be readable and carry the pre-existing deps"
    );

    // The widened surface has to be non-vacuous *by this name too*. A two-file
    // scan is exactly the hardcoded manifest this gate replaced, so the
    // assertion belongs in the test whose name carries the claim — leaving it
    // only in a sibling would let this name keep asserting while the scope
    // quietly collapsed back to a pair of constants.
    let scope_files = scope_rust_sources();
    assert!(
        scope_files.len() > 2,
        "scan_is_not_vacuous: the scope walk read only {} .rs files under {:?} \
         — two hardcoded paths are the manifest this gate replaced, and a scan \
         that narrow proves nothing about `auth/**` and `pairing/**`",
        scope_files.len(),
        SCOPE_ROOTS
    );

    // The detector set itself, over the real tree, must be non-empty.
    let detectors: Vec<(&str, usize)> = vec![
        ("direct_subtle_imports", direct_subtle_imports(&src).len()),
        (
            "token_module_is_registered",
            usize::from(token_module_is_registered(&auth_mod)),
        ),
        (
            "digest_comparison_has_single_entry",
            digest_comparison_has_single_entry(&src).len(),
        ),
        (
            "ct_eq_32_has_no_early_return",
            ct_eq_32_has_no_early_return(&src).len(),
        ),
        (
            "token_uses_existing_deps",
            token_uses_existing_deps(&src, &cargo_toml).len(),
        ),
        (
            "no_abs_paths_in_task_files",
            no_abs_paths_in_task_files(&[src.as_str()]).len(),
        ),
    ];
    assert_eq!(
        detectors.len(),
        6,
        "all six boundaries must be covered by a detector in this gate"
    );
}

#[test]
fn no_short_circuit_digest_comparison_anywhere_in_remote_subtree() {
    let sites = digest_short_circuit_sites_in_remote_tree();
    assert!(
        sites.is_empty(),
        "no file under src/remote/ may compare a digest with `==` or \
         `slice::eq`: PartialEq on `[u8; 32]` short-circuits at the first \
         differing byte and leaks the matching prefix length. Use \
         `ClientDigest::ct_eq`. Offending sites: {sites:?}"
    );
}

#[test]
fn remote_subtree_walk_is_not_vacuous() {
    let sources = remote_rust_sources();
    assert!(
        sources.len() >= 8,
        "the walk found only {} .rs files under src/remote/ — a boundary over \
         an almost-empty scan proves nothing",
        sources.len()
    );

    // Every scanned file must have contributed code, not just bytes.
    let total_code_lines: usize = sources.iter().map(|(_, src)| code_lines(src).len()).sum();
    assert!(
        total_code_lines > 1000,
        "only {total_code_lines} code lines were scanned across {} files",
        sources.len()
    );

    // The subtrees in scope must both be inside the walk — discovered by
    // the walk, never named by this gate. If either goes missing the assertion
    // below turns the walk's breadth into a claim it can no longer support.
    let joined: String = sources
        .iter()
        .map(|(p, _)| p.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("auth") && joined.contains("pairing"),
        "the walk must reach both the auth and pairing subtrees"
    );
}

// ---------------------------------------------------------------------------
// Boundary 8 tests: the scope that matters
// ---------------------------------------------------------------------------

#[test]
fn no_digest_short_circuit_anywhere_in_scope() {
    let sites = scope_violation_sites();
    assert!(
        sites.is_empty(),
        "no file under {:?} may compare a digest with `==`, `!=`, `.eq(` or \
         `slice::eq`: PartialEq on `[u8; 32]` short-circuits at the first \
         differing byte and leaks the matching prefix length. The only \
         sanctioned entries are `Mac::verify_slice` and `ClientDigest::ct_eq`. \
         Offending sites (file, line, kind): {sites:?}",
        SCOPE_ROOTS
    );
}

#[test]
fn scope_walk_is_not_vacuous() {
    let sources = scope_rust_sources();

    // A two-file scan is exactly the manifest this boundary replaced. Demand
    // more than that, so a regression back to a hardcoded pair cannot pass.
    assert!(
        sources.len() > 2,
        "the scope walk read only {} .rs files under {:?} — the gate must \
         cover the subtrees, not a hand-maintained file list",
        sources.len(),
        SCOPE_ROOTS
    );

    // Both declared roots must have contributed. If one root stopped resolving,
    // the walk would still return the other's files and the surface would be
    // silently halved.
    let joined: String = sources
        .iter()
        .map(|(p, _)| p.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for root in SCOPE_ROOTS {
        let leaf = root.rsplit('/').next().expect("root has a leaf");
        assert!(
            joined.contains(leaf),
            "the scope walk never reached `{root}` — its files are unguarded"
        );
    }

    // Real code, not just bytes.
    let total_code_lines: usize = sources.iter().map(|(_, src)| code_lines(src).len()).sum();
    assert!(
        total_code_lines > 300,
        "only {total_code_lines} code lines were scanned across {} in-scope files",
        sources.len()
    );

    // The production dedupe that matters must be inside the walked set, and
    // must be clean — a gate that skipped it would report the same green.
    let handlers = sources
        .iter()
        .find(|(p, _)| p.ends_with("pairing/handlers.rs"))
        .expect("pairing/handlers.rs must be inside the walked scope");
    assert!(
        scope_digest_short_circuit_lines(&handlers.1).is_empty(),
        "the in-scope dedupe in pairing/handlers.rs must compare via ct_eq"
    );
    assert!(
        handlers.1.contains(".ct_eq("),
        "fixture drifted: pairing/handlers.rs is expected to dedupe via \
         `ClientDigest::ct_eq`"
    );
}

#[test]
fn gate_catches_slice_eq_planted_in_a_pairing_handlers_shaped_file() {
    // The exact regression boundary 8 exists for: a violation planted in a file
    // laid out like `pairing/handlers.rs`, at a path nobody named in a constant.
    // Boundary 7 already walked all of `src/remote/`, so this proves the *scope*
    // claim is load-bearing on its own terms — the wider identifier set is what
    // catches it, since `token_hash` carries no `digest` substring.
    let fixture = concat!(
        "fn issue_credential(state: &State, token: &str) -> Result<String, String> {\n",
        "    let mut clients = load_clients(&state.config_dir)?;\n",
        "    let mut entry = ClientToken::new(\"n\", &state.secret, token);\n",
        "    entry.cookie_hash = Some(token_hash(&state.secret, token));\n",
        "    let issued = entry.token_hash;\n",
        "    clients.retain(|c| c.token_hash != issued);\n",
        "    Ok(String::new())\n",
        "}\n",
    );
    let hits = scope_digest_short_circuit_lines(fixture);
    assert_eq!(
        hits,
        vec![6],
        "`c.token_hash != issued` in a handlers-shaped fixture must be reported — \
         `token_hash` carries no `digest` substring, so boundary 7's identifier \
         list would miss exactly this line"
    );

    // The three other spellings too, so "any third form is red" is the claim.
    // Each fixture names the value as a digest — that is the detector's actual
    // contract (identifier *and* operator on one line), and asserting otherwise
    // would be claiming coverage the boundary does not have.
    for (label, body) in [
        (
            "bare eq",
            "fn f(a: ClientDigest, b: ClientDigest) -> bool { a == b }\n",
        ),
        (
            "method eq",
            "fn f(stored_digest: &[u8], o: &[u8]) -> bool { stored_digest.eq(o) }\n",
        ),
        (
            "slice::eq",
            "fn f(hmac_digest: &[u8], o: &[u8]) -> bool { hmac_digest.slice::eq(o) }\n",
        ),
        (
            "assert_eq",
            "fn f(cookie_hash: ClientDigest) -> bool { assert_eq!(cookie_hash, x); true }\n",
        ),
    ] {
        assert_eq!(
            scope_digest_short_circuit_lines(body),
            vec![1],
            "`{label}` on a digest must be reported as a third comparison form"
        );
    }

    // And the contract's other half, stated honestly: a digest-free comparison
    // is out of scope for this boundary. `&[u8]` comparison on plain names is
    // the shape boundary 7/8 deliberately do not claim — documenting it keeps a
    // future reader from reading `a.eq(b)` as covered when it is not.
    assert!(
        scope_digest_short_circuit_lines("fn f(a: &[u8], b: &[u8]) -> bool { a.eq(b) }\n")
            .is_empty(),
        "a digest-free comparison carries no digest identifier and is out of \
         this boundary's contract — if it should be in scope, the detector needs \
         a type-based rule, not a wider identifier list"
    );

    // And a direct `subtle` import anywhere in scope is red, not just in token.rs.
    assert_eq!(
        scope_subtle_lines("use subtle::ConstantTimeEq;\nfn demo() {}\n"),
        vec![1],
        "a direct `subtle` import in any in-scope file must be reported"
    );
    assert!(
        scope_subtle_lines("//! use subtle is why this module avoids it\nfn demo() {}\n")
            .is_empty(),
        "the doc explaining `use subtle` must not be flagged — comments are excluded"
    );

    // The honest path stays green, or the fixtures above prove nothing.
    assert!(
        scope_digest_short_circuit_lines("clients.retain(|c| !c.token_hash.ct_eq(&issued));\n")
            .is_empty(),
        "the `ct_eq` dedupe must pass — it is the sanctioned entry"
    );
}

#[test]
fn scope_detectors_would_catch_a_planted_violation_in_the_real_tree() {
    // Guards against the widening having been a no-op: take the *real*
    // in-scope files, plant a violation into a copy of the shape
    // `pairing/handlers.rs` has, and prove the detector set goes red on it.
    let mut planted = 0usize;
    let mut caught = 0usize;
    for (path, src) in scope_rust_sources() {
        if !path.ends_with("pairing/handlers.rs") {
            continue;
        }
        planted += 1;
        // Baseline first. Asserting only "the planted file reports *something*"
        // would pass on a file that was already dirty, which is exactly the
        // failure mode where a detector looks like it fired but did not.
        let baseline = scope_digest_short_circuit_lines(&src);

        // Plant into the *impl region*, not the tail: appending would land after
        // this file's `#[cfg(test)]`, which the boundary deliberately skips.
        //
        // Both spellings the brief names are planted, not just the one that is
        // easy to grep for. `slice::eq` is a third form whose text carries no
        // `==` token and no `.eq(` — on a `ClientDigest` field it is the exact
        // shape that survives an operator-only detector.
        let plant = concat!(
            "fn planted_bare(c: ClientDigest, o: ClientDigest) -> bool { c == o }\n",
            "fn planted_slice(c: &ClientDigest, o: &ClientDigest) -> bool { c.0.slice::eq(&o.0) }\n",
        );
        let injected = src.replacen("#[cfg(test)]", &format!("{plant}#[cfg(test)]"), 1);
        let hits = scope_digest_short_circuit_lines(&injected);
        assert_eq!(
            hits.len(),
            baseline.len() + 2,
            "planting `c == o` *and* `c.0.slice::eq(&o.0)` into {path} must add \
             exactly two catches on top of the baseline {baseline:?}; got {hits:?}"
        );
        assert!(
            !scope_violation_sites_from(&[(path.clone(), injected)]).is_empty(),
            "the aggregate scope detector must also see both plants"
        );
        caught += 1;
    }
    assert_eq!(
        planted, 1,
        "expected exactly one pairing/handlers.rs in scope"
    );
    assert_eq!(caught, 1, "the planted violation must have been caught");
}

/// Same aggregation as [`scope_violation_sites`], over caller-supplied sources.
///
/// Split out so the planted-fixture test exercises the real aggregation path
/// rather than re-implementing it.
fn scope_violation_sites_from(sources: &[(String, String)]) -> Vec<(String, usize, String)> {
    sources
        .iter()
        .flat_map(|(path, src)| {
            let digest_hits = scope_digest_short_circuit_lines(src);
            let subtle_hits = scope_subtle_lines(src);
            digest_hits
                .into_iter()
                .map(move |l| (path.clone(), l, "digest short-circuit".to_string()))
                .chain(subtle_hits.into_iter().map({
                    let p = path.clone();
                    move |l| (p.clone(), l, "direct subtle import".to_string())
                }))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof the gate can go red
// ---------------------------------------------------------------------------

#[test]
fn gate_rejects_fixture_direct_subtle_use() {
    let fixture =
        "use subtle::ConstantTimeEq;\nfn demo(a: u8, b: u8) -> bool { a.ct_eq(&b).into() }\n";
    let hits = direct_subtle_imports(fixture);
    assert_eq!(
        hits,
        vec![1],
        "a direct `use subtle::...` must be reported at its line"
    );

    // And the shape most likely to slip past a naive scan: indented import.
    let indented = "fn demo() {}\n    use subtle::Choice;\n";
    assert_eq!(
        direct_subtle_imports(indented),
        vec![2],
        "indentation must not hide an import"
    );

    // A `return` type or a doc line must not be mistaken for one.
    assert!(
        direct_subtle_imports("// see subtle::ConstantTimeEq for the rationale\n//! use subtle is banned\nfn demo() -> Option<u8> { None }\n").is_empty(),
        "comment lines must be excluded — token.rs's own module doc explains \
         `use subtle` in prose"
    );
}

#[test]
fn gate_rejects_fixture_ct_eq_early_return() {
    let fixture = concat!(
        "fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {\n",
        "    let mut diff: u8 = 0;\n",
        "    for i in 0..32 {\n",
        "        diff |= a[i] ^ b[i];\n",
        "        if a[i] != b[i] { return false; }\n",
        "    }\n",
        "    diff == 0\n",
        "}\n",
    );
    let hits = ct_eq_32_has_no_early_return(fixture);
    assert_eq!(
        hits,
        vec![5],
        "an early `return` inside `ct_eq_32` must be reported at its body line"
    );

    let short_circuit = concat!(
        "fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {\n",
        "    for i in 0..32 {\n",
        "        if a[i] != b[i] { break; }\n",
        "    }\n",
        "    true\n",
        "}\n",
    );
    assert_eq!(
        ct_eq_32_has_no_early_return(short_circuit),
        vec![3],
        "`break` is an early exit too"
    );

    // The honest body is clean — otherwise the fixture proves nothing.
    let clean = concat!(
        "fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {\n",
        "    let mut diff: u8 = 0;\n",
        "    for i in 0..32 {\n",
        "        diff |= a[i] ^ b[i];\n",
        "    }\n",
        "    diff == 0\n",
        "}\n",
    );
    assert!(
        ct_eq_32_has_no_early_return(clean).is_empty(),
        "the accumulate-then-compare body must pass, or the fixture is void"
    );
}

#[test]
fn gate_rejects_fixture_digest_eq_in_the_whole_subtree_shape() {
    // A synthetic file laid out exactly like one under `src/remote/`, carrying
    // every spelling the boundary claims to catch. If this fixture comes back
    // clean, the walk is decoration and the real-tree test proves nothing.
    let fixture = concat!(
        "fn issue(hmac_digest: &[u8; 32], stored_digest: &[u8; 32]) -> bool {\n",
        "    hmac_digest == stored_digest\n",
        "}\n",
        "fn revoke(secret_digest: &Vec<u8>, other: &Vec<u8>) -> bool {\n",
        "    secret_digest.slice::eq(other)\n",
        "}\n",
        "fn honest(a: &ClientDigest, b: &ClientDigest) -> bool {\n",
        "    a.ct_eq(b)\n",
        "}\n",
    );
    assert_eq!(
        digest_short_circuit_lines(fixture),
        vec![2, 5],
        "both comparison spellings on digest-identifier lines must be reported, \
         and the `ct_eq` line must not be"
    );

    // And the walk itself, over a real temporary tree, must surface a violation
    // without this gate naming the file. Same code path as the real-tree test.
    let tmp = tempfile::tempdir().expect("isolated temp dir");
    let nested = tmp.path().join("auth").join("deeper");
    std::fs::create_dir_all(&nested).expect("nested dir");
    std::fs::write(
        nested.join("planted.rs"),
        "fn same(hmac_digest: [u8; 32], other: [u8; 32]) -> bool { hmac_digest == other }\n",
    )
    .expect("write fixture");
    std::fs::write(
        tmp.path().join("clean.rs"),
        "fn ok(a: u8, b: u8) -> bool { a == b }\n",
    )
    .expect("write clean file");

    fn walk_for_test(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("temp dir readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk_for_test(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push((
                    path.display().to_string(),
                    std::fs::read_to_string(&path).expect("fixture readable"),
                ));
            }
        }
    }
    let mut walked = Vec::new();
    walk_for_test(tmp.path(), &mut walked);
    let sites: Vec<(String, usize)> = walked
        .iter()
        .flat_map(|(p, src)| {
            digest_short_circuit_lines(src)
                .into_iter()
                .map(move |l| (p.clone(), l))
        })
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "the recursive walk must find the nested violation and ignore the \
         digest-free `a == b`; found {sites:?}"
    );
    assert!(
        sites[0].0.contains("planted.rs"),
        "the reported site must be the planted file: {sites:?}"
    );
}

#[test]
fn gate_rejects_fixture_missing_mod_token() {
    let without_token = "mod policy;\n";
    assert!(
        !token_module_is_registered(without_token),
        "an auth/mod.rs without `mod token;` must be rejected"
    );

    // Prose that merely mentions the declaration must not count as one.
    let prose_only = "// mod token;\nmod policy;\n";
    assert!(
        !token_module_is_registered(prose_only),
        "a commented-out `mod token;` must not satisfy the gate"
    );

    let with_token = "mod policy;\nmod token;\n";
    assert!(
        token_module_is_registered(with_token),
        "the real declaration shape must be accepted"
    );
}

#[test]
fn gate_rejects_fixture_bare_digest_eq_and_new_crate() {
    let bare_eq = "pub fn same(a: &[u8; 32], b: &[u8; 32]) -> bool { a == b }\n";
    assert_eq!(
        digest_comparison_has_single_entry(bare_eq),
        vec![1],
        "`a == b` on digests must be reported"
    );

    let field_eq =
        "impl T { fn same(&self, other: &Self) -> bool { self.token_hash == other.token_hash } }\n";
    assert_eq!(
        digest_comparison_has_single_entry(field_eq),
        vec![1],
        "field-to-field digest comparison must be reported"
    );

    let accumulator = "fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {\n    let mut diff: u8 = 0;\n    diff == 0\n}\n";
    assert!(
        digest_comparison_has_single_entry(accumulator).is_empty(),
        "the post-loop `diff == 0` accumulator comparison must be allowed"
    );

    let cargo_toml =
        "[dependencies]\nbase64 = \"0.22\"\nuuid = \"1.11\"\nsha2 = \"0.10\"\nhmac = \"0.12\"\n";
    let new_crate = "use ring::digest;\n";
    let violations = token_uses_existing_deps(new_crate, cargo_toml);
    assert_eq!(
        violations,
        vec!["ring"],
        "an import outside std + the four pre-existing deps must be reported"
    );

    let missing_decl = "use sha2::Sha256;\n";
    assert_eq!(
        token_uses_existing_deps(missing_decl, "[dependencies]\n"),
        vec!["base64", "uuid", "sha2", "hmac"],
        "every required dep missing from Cargo.toml must be reported"
    );

    assert!(
        !cargo_declares(cargo_toml, "subtle"),
        "`subtle` must not count as a declared dependency"
    );
    assert!(
        abs_path_sites("let p = \"/Users/someone/.ssh/id\";\n").len() == 1,
        "a `/Users/` literal must be reported"
    );
    assert!(
        abs_path_sites("let p = \"C:\\\\Users\\\\someone\";\n").len() == 1,
        "a `C:\\` literal must be reported"
    );
}
