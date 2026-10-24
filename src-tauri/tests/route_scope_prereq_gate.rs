//! Executable gate for the *prerequisites* of route-scope registration.
//!
//! Why a second gate when `route_scope_gate.rs` already exists: that one
//! describes the end state — every route goes through a scope table. It cannot
//! describe the *absence* of its own inputs. When the table, the controlled
//! entry, the auth scope enum, or the auth middleware/secret/pairing modules
//! are simply not on disk, there is nothing for the end-state gate to compare,
//! and a task list can still call the corresponding work "completed".
//!
//! That exact gap is what this file closes. A previous round discovered it by
//! hand (grepping for the table symbol, for the register entry, listing the
//! auth module directory); the finding lived only in a prose report, so the
//! next rerun had to re-derive it. Here the same judgement is compiled into
//! named assertions that go red on every run, and each red names the artifact
//! it is missing so it can be checked off one by one.
//!
//! Boundaries locked here:
//!
//! 1. `missing_prerequisites` — a route scope table and a controlled register
//!    entry in the remote server module.
//! 2. `has_register_entry` / `has_auth_scope` — the assembly-layer symbols the
//!    route table is supposed to hang off.
//! 3. `missing_artifacts` — the auth middleware, the auth secret store, and the
//!    pairing module directory, checked on disk rather than in source text.
//! 4. Falsifiability — four negative fixtures prove each detector can go red,
//!    and `scan_is_not_vacuous` proves the scan surface is non-empty so a
//!    detector that reads nothing cannot read as green.
//!
//! The real-tree test is expected to be **red** right now: that redness is the
//! named, per-artifact fact this gate exists to freeze. The fixtures are in
//! separate `#[test]` functions so they can be filtered and run green on their
//! own while the real-tree assertion stays red.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! machine-local path: the repo is cloned onto many machines and CI runners.

use std::path::{Path, PathBuf};

const REMOTE_MOD_RS: &str = "src/remote/mod.rs";
const AUTH_MOD_RS: &str = "src/remote/auth/mod.rs";

/// Names reported for each prerequisite, so a failure names the artifact
/// rather than only counting what is absent.
const PREREQ_TABLE: &str = "ROUTE_SCOPE_TABLE";
const PREREQ_REGISTER: &str = "fn register";
const PREREQ_AUTH_SCOPE: &str = "AuthScope";
const PREREQ_MIDDLEWARE: &str = "remote/auth/middleware.rs";
const PREREQ_SECRET: &str = "remote/auth/secret.rs";
const PREREQ_PAIRING: &str = "remote/pairing/";

/// Relative paths of the on-disk artifacts, and whether a directory suffices.
const ARTIFACTS: [(&str, &str, bool); 3] = [
    (PREREQ_MIDDLEWARE, "src/remote/auth/middleware.rs", false),
    (PREREQ_SECRET, "src/remote/auth/secret.rs", false),
    (PREREQ_PAIRING, "src/remote/pairing", true),
];

fn manifest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_src(relative: &str) -> String {
    let path = manifest_dir().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Comment-stripping scan surface
// ---------------------------------------------------------------------------

/// Drop a trailing `//` comment, honouring string and char literals so a `//`
/// inside a string is not mistaken for a comment.
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

/// Comment-stripped source joined into one text.
///
/// Stripping matters for this gate specifically: the missing artifacts are
/// named in prose all over this repository's task plans, so a detector that
/// read comments would find "ROUTE_SCOPE_TABLE" described in a doc comment and
/// report the prerequisite as present.
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(strip_line_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Number of comment-stripped, non-empty source lines actually handed to the
/// detectors — the anti-vacuity measure.
fn scanned_lines(src: &str) -> usize {
    strip_comments(src).lines().count()
}

/// Standalone-identifier occurrences of `token` in `text`.
fn has_token(text: &str, token: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| word == token)
}

/// True when `fn <name>` is defined, ignoring any trailing identifier
/// characters — `fn register_routes` is not `fn register`.
fn defines_fn(text: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(&needle) {
        let after = from + rel + needle.len();
        let next_is_ident = text[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !next_is_ident {
            return true;
        }
        from = after;
    }
    false
}

/// True when `name` appears preceded by the `enum` keyword — i.e. it is an enum
/// definition, not merely a mention of the type.
fn defines_enum(text: &str, name: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        let after = at + name.len();
        let next_is_ident = text[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let prev_word = text[..at]
            .split_whitespace()
            .next_back()
            .unwrap_or_default();
        if !next_is_ident && prev_word == "enum" {
            return true;
        }
        from = after;
    }
    false
}

// ---------------------------------------------------------------------------
// Detectors
// ---------------------------------------------------------------------------

/// The controlled registration entry in a remote server module.
pub fn has_register_entry(src: &str) -> bool {
    defines_fn(&strip_comments(src), "register")
}

/// Any standalone mention of the auth scope type.
pub fn has_auth_scope(src: &str) -> bool {
    has_token(&strip_comments(src), PREREQ_AUTH_SCOPE)
}

/// The auth scope type's enum definition — the stronger shape.
pub fn has_auth_scope_enum(src: &str) -> bool {
    defines_enum(&strip_comments(src), PREREQ_AUTH_SCOPE)
}

/// Prerequisite symbols missing from a remote server module, in report order.
pub fn missing_prerequisites(src: &str) -> Vec<&'static str> {
    let text = strip_comments(src);
    let mut missing = Vec::new();
    if !has_token(&text, PREREQ_TABLE) {
        missing.push(PREREQ_TABLE);
    }
    if !defines_fn(&text, "register") {
        missing.push(PREREQ_REGISTER);
    }
    missing
}

/// Prerequisite artifacts missing from the source tree, in report order.
fn missing_artifacts() -> Vec<&'static str> {
    let root = manifest_dir();
    ARTIFACTS
        .iter()
        .filter(|(_, relative, want_dir)| {
            let path = root.join(relative);
            if *want_dir {
                !path.is_dir()
            } else {
                !path.is_file()
            }
        })
        .map(|(name, _, _)| *name)
        .collect()
}

// ---------------------------------------------------------------------------
// Test: the real tree
// ---------------------------------------------------------------------------

#[test]
fn route_scope_prereq_gate_reports_missing_chain() {
    let remote_mod = read_src(REMOTE_MOD_RS);
    let auth_mod = read_src(AUTH_MOD_RS);

    // Collect every missing prerequisite under one label so the failure reads
    // as a checklist that can be ticked off one artifact at a time.
    let mut missing: Vec<&'static str> = Vec::new();
    missing.extend(missing_prerequisites(&remote_mod));
    if !has_auth_scope(&auth_mod) {
        missing.push(PREREQ_AUTH_SCOPE);
    }
    missing.extend(missing_artifacts());

    assert!(
        missing.is_empty(),
        "route-scope registration cannot be proven because its prerequisites are \
         absent. Missing, by name: {missing:?}. Each entry is a separate \
         prerequisite: the scope table, the controlled register entry, the auth \
         scope enum, the auth middleware, the auth secret store, and the pairing \
         module directory."
    );
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof each detector can go red
// ---------------------------------------------------------------------------

#[test]
fn gate_rejects_fixture_missing_table() {
    let without_table = r#"
fn register(router: Router) -> Router {
    router.route("/api/health", axum::routing::get(handlers::health_check))
}
"#;
    assert_eq!(
        missing_prerequisites(without_table),
        vec![PREREQ_TABLE],
        "a remote module with no scope table must report the table missing"
    );

    // The complementary shape must be clean, or the fixture above proves nothing.
    let with_table = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 1] = [
    RouteScope { path: "/api/health", scope: Scope::Protected, method: Method::Get },
];

fn register(router: Router) -> Router {
    router.route("/api/health", axum::routing::get(handlers::health_check))
}
"#;
    assert!(
        missing_prerequisites(with_table).is_empty(),
        "a module declaring both the table and the register entry has no missing \
         source prerequisites"
    );
}

#[test]
fn gate_rejects_fixture_missing_register() {
    let without_register = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 1] = [
    RouteScope { path: "/api/health", scope: Scope::Protected, method: Method::Get },
];
"#;
    assert_eq!(
        missing_prerequisites(without_register),
        vec![PREREQ_REGISTER],
        "a module with a table but no controlled register entry must report it"
    );

    // A near-miss name is not the controlled entry: `register_routes` must not
    // satisfy the check, or the detector could pass on an unrelated function.
    let near_miss = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 1] = [
    RouteScope { path: "/api/health", scope: Scope::Protected, method: Method::Get },
];

fn register_routes(router: Router) -> Router {
    router.route("/api/health", axum::routing::get(handlers::health_check))
}
"#;
    assert_eq!(
        missing_prerequisites(near_miss),
        vec![PREREQ_REGISTER],
        "`fn register_routes` is not the controlled register entry"
    );
}

#[test]
fn gate_rejects_fixture_missing_auth_scope() {
    let without_scope = r#"
mod policy;
mod token;

pub use policy::{decide, Decision};
"#;
    assert!(
        !has_auth_scope(without_scope),
        "an auth module that never names the scope type must not pass"
    );

    let with_scope = r#"
pub use scope::{AuthScope, ScopeDecision};
"#;
    assert!(
        has_auth_scope(with_scope),
        "any standalone mention of the scope type satisfies the weak check"
    );

    // A mention inside a comment is not a declaration.
    let commented_only = r#"
//! The auth scope is decided upstream.
mod policy;
"#;
    assert!(
        !has_auth_scope(commented_only),
        "a scope type named only in prose must not satisfy the check — this gate \
         reads code, not comments"
    );
}

#[test]
fn gate_rejects_fixture_missing_auth_scope_enum() {
    let struct_not_enum = r#"
pub struct AuthScope {
    pub protected: bool,
}

pub fn decide(scope: &AuthScope) -> bool {
    scope.protected
}
"#;
    assert!(
        has_auth_scope(struct_not_enum),
        "the weak check passes: the type is named"
    );
    assert!(
        !has_auth_scope_enum(struct_not_enum),
        "a struct definition is not the scope enum — the strong check must red"
    );

    let real_enum = r#"
pub enum AuthScope {
    Protected,
    BrowserSafe,
    Public,
}
"#;
    assert!(
        has_auth_scope_enum(real_enum),
        "an `enum AuthScope` definition satisfies the strong check"
    );

    let commented_enum = r#"
// enum AuthScope { Protected, Public }
pub struct Other;
"#;
    assert!(
        !has_auth_scope_enum(commented_enum),
        "an enum written in a comment is not a definition"
    );
}

/// The detectors must actually read something. Without this, a gate whose
/// reader returns empty strings would report every prerequisite present and
/// read as green — the same failure mode as an empty static gate.
#[test]
fn scan_is_not_vacuous() {
    let remote_mod = read_src(REMOTE_MOD_RS);
    let auth_mod = read_src(AUTH_MOD_RS);

    assert!(
        scanned_lines(&remote_mod) > 0,
        "the remote server module must contribute a non-empty scan surface"
    );
    assert!(
        scanned_lines(&auth_mod) > 0,
        "the auth module must contribute a non-empty scan surface"
    );

    // The named-checklist set must itself be non-empty and cover every label the
    // real-tree test reports.
    let covered: Vec<&'static str> = ARTIFACTS
        .iter()
        .map(|(name, _, _)| *name)
        .chain([PREREQ_TABLE, PREREQ_REGISTER, PREREQ_AUTH_SCOPE])
        .collect();
    assert_eq!(
        covered.len(),
        6,
        "the covered prerequisite set must name all six artifacts: {covered:?}"
    );
    assert!(
        covered.iter().all(|name| !name.is_empty()),
        "no prerequisite may be covered by an empty label"
    );
}
