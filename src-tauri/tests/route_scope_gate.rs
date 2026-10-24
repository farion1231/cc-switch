//! Executable gate for route-scope completeness
//! (`src-tauri/src/remote/mod.rs`).
//!
//! Why a static gate at all: the point of the scope table is that **every**
//! business route is registered through the controlled entry with a declared
//! scope. A bare counting gate is the wrong instrument — a string-count scan
//! misses `format!`/`concat!` paths, so a route can be added *and* registered
//! while the count still balances. Worse, "the gate found nothing" and "the
//! gate is green" look identical in CI output. This repo has already shipped
//! one all-green static gate that was empty because it only covered two shapes.
//! So this gate is written to be **falsifiable**: three negative fixtures prove
//! each detector can actually go red.
//!
//! Boundaries locked here:
//!
//! 1. `registered_route_paths` — the count of route path literals at the
//!    controlled registration sites equals the number of `ROUTE_SCOPE_TABLE`
//!    entries. A "registration site" is either a bare `.route("/…")` call or a
//!    `register(router, "/…", …)` call — the controlled entry that is the only
//!    way a business route may reach the `Router`. A new route missing from the
//!    table breaks the equality; a table entry with no route breaks it the other
//!    way. `unregistered_routes` names the offender instead of only counting it.
//! 2. `dynamic_path_args` — no registration site's path argument may be built by
//!    `format!`/`concat!`/a non-literal. A dynamic path is invisible to a
//!    literal count, so it would silently escape boundary 1. The one `.route()`
//!    inside `register()`'s own body is exempt — it forwards register's `path`
//!    parameter, whose `&'static str` type makes a dynamic path a compile error;
//!    `register_path_is_static` gates that exemption instead of assuming it.
//! 3. `route_sites_outside_register` — `.route(` may only appear inside the
//!    controlled `register()` body.
//! 4. `scope_mismatches` — the `AuthScope` passed at a `register()` call site
//!    must be the scope the table declares for that path. Without this the
//!    table could say `Protected` while the assembly passes `Public`, and every
//!    count in this file would still balance.
//! 5. Scale — `Protected + BrowserSafe + GET /` is exactly 14, the
//!    machine-checkable form of "no business route was added or widened".
//! 6. `scope_downgrades` — a non-root table entry relabelled to `Public` must be
//!    identified (fixture C). The root page is legitimately `Public` (it is
//!    where an operator lands before authenticating), so it is exempt by path,
//!    not by having its token ignored.
//!
//! Comments are stripped before scanning, so a doc comment explaining *why*
//! `.route(` is confined to `register()` cannot trip the gate.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal machine-local
//! path: the repo is cloned onto many machines and CI runners.

use std::path::Path;

const REMOTE_MOD_RS: &str = "src/remote/mod.rs";

/// The real source files this gate reads. Non-empty by construction, and the
/// real-tree test asserts each contributes lines and that the remote module is
/// among them — so a scan surface that silently degrades to zero files cannot
/// read as green.
const SCAN_SURFACE: [&str; 1] = [REMOTE_MOD_RS];

/// The route count the table must carry: 17 business routes, none added.
///
/// 14 pre-existing routes + the three two-phase bind-scope endpoints
/// (request / confirm / reject), all `Protected`.
const EXPECTED_ROUTES: usize = 17;

/// Label for an entry whose scope token this gate does not recognise.
const UNCLASSIFIED: &str = "UNCLASSIFIED";

/// Label for the root `GET /` entry, which is counted apart from the two
/// business scopes in the scale assertion.
const ROOT_GET: &str = "GET /";

/// Path of that same entry. Named separately from `ROOT_GET` because the root
/// is also the one path allowed to carry `Public`.
const ROOT_PATH: &str = "/";

/// Scope tokens the table may carry, weakest last. `Public` is the downgrade.
const SCOPE_TOKENS: [&str; 3] = ["Protected", "BrowserSafe", "Public"];

const METHOD_TOKENS: [&str; 5] = ["Get", "Post", "Put", "Delete", "Patch"];

fn read_src(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Comment-stripping scan surface
// ---------------------------------------------------------------------------

/// Drop a trailing `//` comment, honouring string and char literals so that the
/// `//` inside `"http://x"` is not mistaken for a comment.
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

/// Comment-stripped source joined into one text, keeping a map back to 1-based
/// source line numbers.
///
/// Joining matters: a `.route(` call spans several source lines in `mod.rs`, and
/// a literal count that only looked at single lines would see none of them.
/// Because comment lines are dropped from `text`, a path can never be picked
/// up out of prose.
struct Scan {
    text: String,
    lines: Vec<usize>,
}

fn scan(src: &str) -> Scan {
    let mut text = String::new();
    let mut lines = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let code = strip_line_comment(raw).trim();
        if code.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        lines.push(idx + 1);
        text.push_str(code);
    }
    Scan { text, lines }
}

impl Scan {
    /// 1-based source line for a byte offset into `text`.
    fn line_at(&self, byte: usize) -> usize {
        let line_index = self.text[..byte.min(self.text.len())].matches('\n').count();
        self.lines
            .get(line_index)
            .copied()
            .unwrap_or_else(|| self.lines.last().copied().unwrap_or(0))
    }
}

/// Read the string literal starting at `start`: `(unescaped contents, index
/// just past the closing quote)`.
fn read_string_literal(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut out = String::new();
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                out.push(*bytes.get(i + 1)? as char);
                i += 2;
            }
            b'"' => return Some((out, i + 1)),
            _ => {
                let ch = text[i..].chars().next()?;
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    None
}

/// Skip a `'x'` / `'\n'` char literal. Conservative: bails out on anything that
/// does not look like one, so a stray apostrophe cannot desynchronise the scan.
fn skip_char_literal(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let is_closed = *bytes.get(start + 2)? == b'\'';
    is_closed.then_some(start + 3)
}

/// Bracket-match forward from an opening bracket, honouring string and char
/// literals. Returns the index just past the matching close bracket.
fn match_bracket(text: &str, open: usize) -> Option<usize> {
    let open_ch = text.as_bytes()[open];
    let close_ch = match open_ch {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        _ => return None,
    };
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = read_string_literal(text, i)?.1,
            b'\'' => match skip_char_literal(text, i) {
                Some(next) => i = next,
                None => i += 1,
            },
            b if b == open_ch => {
                depth += 1;
                i += 1;
            }
            b if b == close_ch => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// The `(start, end)` byte range of a `fn <name>` body, located by `marker`.
fn fn_body_range(text: &str, marker: &str) -> Option<(usize, usize)> {
    let at = text.find(marker)?;
    let open = text[at..].find('{')? + at;
    let end = match_bracket(text, open)?;
    Some((open, end))
}

// ---------------------------------------------------------------------------
// Boundary 1: bare `.route("/…")` literals
// ---------------------------------------------------------------------------

/// Every `.route(` call site: `(source line, byte index of its `(`)`.
fn route_call_sites(scan: &Scan) -> Vec<(usize, usize)> {
    let mut sites = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = scan.text[from..].find(".route(") {
        let paren = from + rel + ".route".len();
        sites.push((scan.line_at(paren), paren));
        from = paren + 1;
    }
    sites
}

/// Every top-level argument of the call whose `(` is at `open_paren`.
///
/// Nesting- and literal-aware: splits only at a `,` outside any bracket, and
/// never splits inside a string that contains a comma or paren. A trailing
/// comma yields a final empty argument, which every caller trims away.
fn call_args(text: &str, open_paren: usize) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut seg_start = open_paren + 1;
    let mut i = seg_start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => match read_string_literal(text, i) {
                Some((_, after)) => i = after,
                None => i += 1,
            },
            b'\'' => match skip_char_literal(text, i) {
                Some(next) => i = next,
                None => i += 1,
            },
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => {
                args.push(text[seg_start..i].trim().to_string());
                i += 1;
                seg_start = i;
            }
            _ => i += 1,
        }
    }
    args.push(text[seg_start..i].trim().to_string());
    args
}

/// The first argument of the `.route(` call whose `(` is at `open_paren`,
/// together with the byte index just past it.
///
/// Scanning is nesting- and literal-aware: it stops at the first `,` or `)`
/// outside any nesting and never splits a string that contains a paren.
fn route_first_arg(text: &str, open_paren: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let start = open_paren + 1;
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = read_string_literal(text, i)?.1,
            b'\'' => match skip_char_literal(text, i) {
                Some(next) => i = next,
                None => i += 1,
            },
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => break,
            _ => i += 1,
        }
    }
    Some((text[start..i].trim().to_string(), i))
}

/// True when `fn register`'s signature pins its `path` parameter to
/// `&'static str`.
///
/// The one `.route()` inside `register`'s own body forwards that parameter, so
/// its argument is an identifier rather than a literal. Boundary 2 exempts that
/// site — but only on the strength of *this* check. Without it the exemption
/// would be "assume the controlled entry is well typed", which is exactly the
/// kind of assumption that reads as a green gate.
fn register_path_is_static(src: &str) -> bool {
    let scan = scan(src);
    let Some(at) = scan.text.find("fn register") else {
        return false;
    };
    let Some(open) = scan.text[at..].find('{').map(|o| o + at) else {
        return false;
    };
    let signature = &scan.text[at..open];
    has_token(signature, "path") && signature.contains("'static")
}

/// `(source line, byte index of the `(`)` for every `register(...)` **call**.
///
/// The `fn register(` definition itself is excluded, as is any identifier that
/// merely ends in `register` — otherwise the controlled entry's own signature
/// would be counted as a registration site and the boundary-1 counts would
/// inflate by one on every file.
fn register_call_sites(scan: &Scan) -> Vec<(usize, usize)> {
    let mut sites = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = scan.text[from..].find("register(") {
        let at = from + rel;
        let paren = at + "register".len();
        let prev_is_ident = scan.text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let prev_word = scan.text[..at]
            .split_whitespace()
            .next_back()
            .unwrap_or_default();
        if !prev_is_ident && prev_word != "fn" {
            sites.push((scan.line_at(paren), paren));
        }
        from = paren + 1;
    }
    sites
}

/// One controlled-entry call, with the three arguments that carry the contract:
/// `register(router, path, method, scope, handler)`.
struct RegistrationCall {
    path: String,
    method: String,
    scope: String,
}

/// Every `register(...)` call in the file, in source order.
fn register_calls(src: &str) -> Vec<RegistrationCall> {
    let scan = scan(src);
    register_call_sites(&scan)
        .into_iter()
        .map(|(_, paren)| {
            let args = call_args(&scan.text, paren);
            let path_arg = args.get(1).cloned().unwrap_or_default();
            RegistrationCall {
                // Unquoted so a call site can be matched against a table row:
                // the table stores literal *contents*, the call site stores the
                // literal itself. Comparing the raw texts would report every
                // route as unmatched.
                path: read_string_literal(&path_arg, 0)
                    .map_or(path_arg.clone(), |(contents, _)| contents),
                method: args.get(2).cloned().unwrap_or_default(),
                scope: args.get(3).cloned().unwrap_or_default(),
            }
        })
        .collect()
}

/// The `AuthScope` variant an argument names, or `None` when it names none of
/// the three. `AuthScope::Protected` -> `Some("Protected")`.
fn scope_variant(arg: &str) -> Option<&'static str> {
    SCOPE_TOKENS
        .iter()
        .copied()
        .find(|token| has_token(arg, token))
}

/// True when a method argument names a GET — `Method::GET` and bare `Get` both
/// count, because the table fixture and the assembly express it differently.
fn method_is_get(arg: &str) -> bool {
    has_token(arg, "GET") || has_token(arg, "Get")
}

/// True when a `.route()` argument is a bare string literal — the only shape
/// the table can be compared against.
///
/// `"…"` passes. `format!("…")`, `concat!("…")`, `path.as_str()`, and even
/// `"a".to_string()` do not: each is assembled or dereferenced at runtime and
/// therefore escapes a literal count.
fn is_literal_arg(arg: &str) -> bool {
    match read_string_literal(arg, 0) {
        Some((_, end)) => end == arg.len(),
        None => false,
    }
}

/// Which family of registration site a path came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SiteKind {
    /// `router.route(path, …)`
    Route,
    /// `router = register(router, path, …)`
    Register,
}

/// One place a route path is declared, with the argument text it declared.
struct DeclSite {
    line: usize,
    /// Byte index of the call's `(`, used to place the site relative to
    /// `register`'s body when deciding whether the dynamic-path exemption
    /// applies.
    paren: usize,
    arg: String,
    kind: SiteKind,
}

/// Every place a route path is declared, in source order.
///
/// Both families count: the controlled `register()` entry that every business
/// route must go through, and a bare `.route(…)`. Counting only one of them is
/// the failure this gate exists to prevent — a file whose routes all bypass the
/// controlled entry must show the mismatch, not balance against an empty side.
fn declaration_sites(src: &str) -> Vec<DeclSite> {
    let scan = scan(src);
    let mut sites: Vec<DeclSite> = Vec::new();

    for (line, paren) in route_call_sites(&scan) {
        let (arg, _) = route_first_arg(&scan.text, paren).unwrap_or_default();
        sites.push(DeclSite {
            line,
            paren,
            arg,
            kind: SiteKind::Route,
        });
    }
    for (line, paren) in register_call_sites(&scan) {
        let arg = call_args(&scan.text, paren)
            .get(1)
            .cloned()
            .unwrap_or_default();
        sites.push(DeclSite {
            line,
            paren,
            arg,
            kind: SiteKind::Register,
        });
    }

    sites.sort_by_key(|site| site.paren);
    sites
}

/// 1-based source lines of literal route-path arguments, at either kind of
/// registration site.
///
/// Deliberately returns **line numbers, not paths**: the gate's job here is the
/// count that boundary 1 compares against the table size. The offending paths
/// themselves come from `bare_route_paths`, so a failure can say *which* route
/// is missing rather than only how many are.
fn bare_route_literals(src: &str) -> Vec<usize> {
    declaration_sites(src)
        .into_iter()
        .filter(|site| is_literal_arg(&site.arg))
        .map(|site| site.line)
        .collect()
}

/// The literal route-path arguments, in source order.
fn bare_route_paths(src: &str) -> Vec<String> {
    declaration_sites(src)
        .into_iter()
        .filter(|site| is_literal_arg(&site.arg))
        .map(|site| read_string_literal(&site.arg, 0).unwrap_or_default().0)
        .collect()
}

/// Registered paths that have no `ROUTE_SCOPE_TABLE` entry — the named form of
/// boundary 1's count equality, for when a failure has to be actionable.
fn unregistered_routes(src: &str) -> Vec<String> {
    let table = table_entries(src);
    bare_route_paths(src)
        .into_iter()
        .filter(|path| !table.contains(path))
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 2: dynamic path arguments
// ---------------------------------------------------------------------------

/// The path argument text of every registration site whose path is not a bare
/// string literal, e.g. `format!("/api/{name}")`.
///
/// This boundary exists because boundary 1 cannot see these: a dynamic path is
/// not counted as a literal, so adding one leaves the two counts equal while
/// the route has still escaped the table entirely.
///
/// The exemption is narrow and earned, not assumed: the single `.route()` in
/// `register`'s own body forwards register's `path` parameter, so its argument
/// is an identifier by construction. It is skipped only when
/// `register_path_is_static` confirms the parameter is pinned to `&'static str`,
/// which makes a dynamic path a compile error rather than a review question.
fn find_dynamic_path_args(src: &str) -> Vec<String> {
    let scan = scan(src);
    let exempt = register_path_is_static(src)
        .then(|| fn_body_range(&scan.text, "fn register"))
        .flatten();

    declaration_sites(src)
        .into_iter()
        .filter(|site| !site.arg.is_empty() && !is_literal_arg(&site.arg))
        .filter(|site| {
            let forwarded = site.kind == SiteKind::Route
                && exempt.is_some_and(|(open, end)| site.paren > open && site.paren < end);
            !forwarded
        })
        .map(|site| site.arg)
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 3: routes outside the controlled entry
// ---------------------------------------------------------------------------

/// 1-based source lines of `.route(` calls that are not inside `fn register`'s
/// body.
///
/// A file with no `fn register` reports every site: the gate must go red rather
/// than pass by finding nothing to complain about.
fn route_sites_outside_register(src: &str) -> Vec<usize> {
    let scan = scan(src);
    let sites = route_call_sites(&scan);

    let Some((open, end)) = fn_body_range(&scan.text, "fn register") else {
        return sites.into_iter().map(|(line, _)| line).collect();
    };

    sites
        .into_iter()
        .filter(|(_, paren)| *paren < open || *paren >= end)
        .map(|(line, _)| line)
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary 4/5: table contents
// ---------------------------------------------------------------------------

/// The `ROUTE_SCOPE_TABLE` array's byte range in comment-stripped source.
///
/// Anchored on the `=` rather than the declaration head, so the `[&str; 14]`
/// in the type annotation is not mistaken for the array body.
fn table_region(scan: &Scan) -> Option<(usize, usize)> {
    let at = scan.text.find("ROUTE_SCOPE_TABLE")?;
    let eq = scan.text[at..].find('=')? + at;
    let open = scan.text[eq..].find('[')? + eq;
    Some((open, match_bracket(&scan.text, open)?))
}

/// One `ROUTE_SCOPE_TABLE` entry: its path literal plus the raw source slice it
/// owns, so `scope_downgrades` can classify without re-parsing the region.
struct TableRow {
    path: String,
    body: String,
}

/// Parse the table into rows. Returns empty when the table is absent — which is
/// why the real-tree test asserts its presence explicitly instead of trusting a
/// `0 == 0` count match.
fn table_rows(src: &str) -> Vec<TableRow> {
    let scan = scan(src);
    let Some((open, end)) = table_region(&scan) else {
        return Vec::new();
    };
    let region = &scan.text[open..end];

    // Collect the path literals in order first.
    let mut literals: Vec<(String, usize, usize)> = Vec::new();
    let bytes = region.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if let Some((contents, after)) = read_string_literal(region, i) {
                literals.push((contents, i, after));
                i = after;
                continue;
            }
        }
        i += 1;
    }

    literals
        .iter()
        .enumerate()
        .map(|(idx, (path, _, after))| {
            // An entry spans from its own path literal to the next entry's.
            let slice_end = literals
                .get(idx + 1)
                .map_or(region.len(), |(_, start, _)| *start);
            TableRow {
                path: path.clone(),
                body: region[*after..slice_end].to_string(),
            }
        })
        .collect()
}

/// Route paths declared by `ROUTE_SCOPE_TABLE`, in declaration order.
fn table_entries(src: &str) -> Vec<String> {
    table_rows(src).into_iter().map(|row| row.path).collect()
}

/// Standalone-identifier occurrences of `token` in `body`.
///
/// Word-bounded so `Protected` cannot be matched inside `IsProtected` and
/// `Get` cannot be matched inside `Target`.
fn has_token(body: &str, token: &str) -> bool {
    body.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| word == token)
}

/// The scope token a table entry declares, if any.
fn scope_of(body: &str) -> Option<&'static str> {
    SCOPE_TOKENS
        .iter()
        .copied()
        .find(|token| has_token(body, token))
}

fn method_of(body: &str) -> Option<&'static str> {
    METHOD_TOKENS
        .iter()
        .copied()
        .find(|token| has_token(body, token))
}

/// Paths of table entries relabelled away from a restricted scope — i.e. those
/// carrying the `Public` token. Classified from the entry's own source slice,
/// never from the row index, so reordering the table cannot hide a downgrade.
///
/// The root page is the one legitimate `Public`: it is where an operator lands
/// before holding a credential, and it carries no business data. The exemption
/// is by path, so a `Public` label on any `/api/…` entry still reds.
fn scope_downgrades(src: &str) -> Vec<String> {
    table_rows(src)
        .into_iter()
        .filter(|row| row.path != ROOT_PATH)
        .filter(|row| scope_of(&row.body) == Some("Public"))
        .map(|row| row.path)
        .collect()
}

/// The `register()` call declaring `path`, if any.
fn register_call_for<'a>(
    calls: &'a [RegistrationCall],
    path: &str,
) -> Option<&'a RegistrationCall> {
    calls.iter().find(|call| call.path == path)
}

/// Paths whose `AuthScope` at the `register()` call site differs from the scope
/// the table declares — the pairing down the middle of the contract.
///
/// Without this, the table is decorative: `ROUTE_SCOPE_TABLE` could say
/// `Protected` for `/api/switch` while the assembly passes `AuthScope::Public`,
/// and every count in this file would still balance. A table entry with no
/// matching `register()` call is reported too, since boundary 1's equality
/// already covers that case separately and would otherwise be the only thing
/// standing between a declared scope and no assembly at all.
fn scope_mismatches(src: &str) -> Vec<String> {
    let calls = register_calls(src);
    table_rows(src)
        .into_iter()
        .filter_map(|row| {
            let declared = scope_of(&row.body);
            let assembled =
                register_call_for(&calls, &row.path).and_then(|call| scope_variant(&call.scope));
            match (declared, assembled) {
                (Some(declared), Some(assembled)) if declared != assembled => Some(format!(
                    "{row_path}：表声明 {declared}，装配传 {assembled}",
                    row_path = row.path
                )),
                (Some(_), None) if !calls.is_empty() => Some(format!(
                    "{}：表有条目但没有对应的 register() 调用",
                    row.path
                )),
                (None, _) => Some(format!("{}：表条目没有声明任何 scope", row.path)),
                _ => None,
            }
        })
        .collect()
}

/// One label per table entry, for the scale assertion. The root `GET /` is
/// counted apart from the two business scopes so the assertion reads the way
/// the requirement is written: `Protected + BrowserSafe + GET / == 14`.
///
/// Method and scope are read from the `register()` call that declares the path
/// when one exists, because that is where the assembly states them; the table
/// row is the fallback for sources with no controlled entry to read. Reading
/// both and cross-checking them is `scope_mismatches`' job, not this one's.
fn table_entry_labels(src: &str) -> Vec<String> {
    let calls = register_calls(src);
    table_rows(src)
        .into_iter()
        .map(|row| {
            let call = register_call_for(&calls, &row.path);
            let scope = call
                .and_then(|c| scope_variant(&c.scope))
                .or_else(|| scope_of(&row.body));
            let method_is_get = call.is_some_and(|c| method_is_get(&c.method))
                || method_of(&row.body) == Some("Get");

            if row.path == ROOT_PATH && method_is_get {
                ROOT_GET.to_string()
            } else {
                scope.unwrap_or(UNCLASSIFIED).to_string()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests: the real tree
// ---------------------------------------------------------------------------

#[test]
fn every_route_is_registered_through_the_scope_table() {
    let src = read_src(REMOTE_MOD_RS);

    // Presence first: without it the count comparison below would degenerate to
    // a `0 == 0` match, which is precisely how an empty gate reads as green.
    assert!(
        src.contains("ROUTE_SCOPE_TABLE"),
        "{REMOTE_MOD_RS} must declare ROUTE_SCOPE_TABLE — this gate compares the \
         registered routes against it, and without the table there is nothing to \
         compare against"
    );

    // The controlled entry must exist and must pin its path parameter, or the
    // "all routes go through one entry" claim below has no referent.
    assert!(
        register_path_is_static(&src),
        "{REMOTE_MOD_RS} must define `fn register` with a `path: &'static str` \
         parameter — that is what makes a dynamically built path a compile error \
         rather than a review question"
    );

    let routes = bare_route_literals(&src);
    let table = table_entries(&src);
    assert_eq!(
        routes.len(),
        table.len(),
        "every registered route path literal must have a ROUTE_SCOPE_TABLE entry \
         and vice versa ({} route sites vs {} table entries); unregistered: {:?}",
        routes.len(),
        table.len(),
        unregistered_routes(&src)
    );
    assert_eq!(
        routes.len(),
        EXPECTED_ROUTES,
        "the remote server must expose exactly {EXPECTED_ROUTES} business routes; \
         changing this number is a product decision, not a refactor. \
         Registered: {:?}",
        bare_route_paths(&src)
    );
    assert!(
        unregistered_routes(&src).is_empty(),
        "routes registered outside the table: {:?}",
        unregistered_routes(&src)
    );

    assert!(
        find_dynamic_path_args(&src).is_empty(),
        "route paths must be string literals — a `format!`/`concat!` path is \
         invisible to the literal count and so escapes the table entirely. \
         Offending args: {:?}",
        find_dynamic_path_args(&src)
    );
    assert!(
        route_sites_outside_register(&src).is_empty(),
        "`.route(` may only appear inside the controlled register() body; \
         offending lines: {:?}",
        route_sites_outside_register(&src)
    );
    assert!(
        scope_mismatches(&src).is_empty(),
        "each register() call must pass the AuthScope its ROUTE_SCOPE_TABLE entry \
         declares: {:?}",
        scope_mismatches(&src)
    );
    assert!(
        scope_downgrades(&src).is_empty(),
        "no ROUTE_SCOPE_TABLE entry other than the root page may be labelled \
         Public: {:?}",
        scope_downgrades(&src)
    );

    let labels = table_entry_labels(&src);
    assert!(
        !labels.iter().any(|l| l == UNCLASSIFIED),
        "every table entry must declare one of Protected / BrowserSafe / the root \
         GET /; unclassified: {:?} (labels: {labels:?})",
        labels
            .iter()
            .filter(|l| *l == UNCLASSIFIED)
            .collect::<Vec<_>>()
    );
    let protected = labels.iter().filter(|l| *l == "Protected").count();
    let browser_safe = labels.iter().filter(|l| *l == "BrowserSafe").count();
    let root = labels.iter().filter(|l| *l == ROOT_GET).count();
    assert_eq!(
        protected + browser_safe + root,
        EXPECTED_ROUTES,
        "Protected ({protected}) + BrowserSafe ({browser_safe}) + {ROOT_GET} \
         ({root}) must stay at {EXPECTED_ROUTES}: no business route added, none \
         widened"
    );
}

/// The scan surface must be real.
///
/// Without this, a gate whose file list silently degraded to zero files would
/// read every detector as empty, balance `0 == 0`, and report green — the exact
/// failure mode of the empty static gate this file was written to replace. So
/// the surface is asserted to be non-empty, to include the remote module this
/// gate is about, and to contribute lines from every file in it.
#[test]
fn scan_surface_reads_the_real_remote_module() {
    assert!(
        !SCAN_SURFACE.is_empty(),
        "the gate must name at least one real source file to scan"
    );
    assert!(
        SCAN_SURFACE.contains(&REMOTE_MOD_RS),
        "the scan surface must include {REMOTE_MOD_RS}; it is scanning {SCAN_SURFACE:?}"
    );

    let scanned: Vec<(&str, usize)> = SCAN_SURFACE
        .iter()
        .map(|relative| (*relative, scan(&read_src(relative)).lines.len()))
        .collect();
    for (relative, lines) in &scanned {
        assert!(
            *lines > 0,
            "{relative} contributed 0 scanned lines — the scan surface has \
             degraded and every detector below would read as green"
        );
    }
    assert!(
        scanned
            .iter()
            .any(|(relative, _)| *relative == REMOTE_MOD_RS),
        "the remote module must actually contribute to the scan: {scanned:?}"
    );
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof the gate can go red
// ---------------------------------------------------------------------------

/// Fixture A — a route registered outside the table, and a `.route(` outside the
/// controlled entry. Without this, a detector that always returns empty leaves
/// the gate green while protecting nothing.
#[test]
fn gate_rejects_unregistered_route_fixture() {
    let fixture = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/", scope: Scope::BrowserSafe, method: Method::Get },
    RouteScope { path: "/api/health", scope: Scope::Protected, method: Method::Get },
];

fn register(router: Router) -> Router {
    router
        .route("/", axum::routing::get(handlers::index))
        .route("/api/health", axum::routing::get(handlers::health_check))
        .route("/api/x", axum::routing::get(handlers::x))
}

fn stray_helper(router: Router) -> Router {
    router.route("/api/stray", axum::routing::get(handlers::x))
}
"#;

    assert_eq!(
        bare_route_literals(fixture),
        vec![9, 10, 11, 15],
        "every bare route literal must be located, including the one in a helper \
         outside register()"
    );
    assert_eq!(
        table_entries(fixture),
        vec!["/", "/api/health"],
        "the table must parse to its 2 declared paths"
    );
    assert_eq!(
        unregistered_routes(fixture),
        vec!["/api/x", "/api/stray"],
        "a registered route with no table entry must be named, not merely counted"
    );
    assert_eq!(
        route_sites_outside_register(fixture),
        vec![15],
        "a `.route(` outside register() must be reported at its line"
    );

    // The clean shape must pass, or the fixture above proves nothing.
    let clean = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/", scope: Scope::BrowserSafe, method: Method::Get },
    RouteScope { path: "/api/health", scope: Scope::Protected, method: Method::Get },
];

fn register(router: Router) -> Router {
    router
        .route("/", axum::routing::get(handlers::index))
        .route("/api/health", axum::routing::get(handlers::health_check))
}
"#;
    assert!(
        unregistered_routes(clean).is_empty(),
        "a fully registered router must produce no unregistered route"
    );
    assert!(
        route_sites_outside_register(clean).is_empty(),
        "routes inside register() must not be reported"
    );
    assert_eq!(
        bare_route_literals(clean).len(),
        table_entries(clean).len(),
        "the clean shape balances both sides of boundary 1"
    );
}

/// Fixture B — a `format!`-built path. It is *not* a bare literal, so the count
/// in boundary 1 stays balanced while the route has escaped the table; only
/// this detector catches it.
#[test]
fn gate_rejects_dynamic_path_fixture() {
    let fixture = r#"
fn register(router: Router, name: String) -> Router {
    router.route(format!("/api/{name}"), axum::routing::get(handlers::x))
}
"#;

    assert_eq!(
        find_dynamic_path_args(fixture),
        vec![r#"format!("/api/{name}")"#],
        "a `format!(\"/api/{{name}}\")` path must be reported with its argument text"
    );
    assert!(
        bare_route_literals(fixture).is_empty(),
        "the dynamic path is not a bare literal — this is exactly why boundary 2 \
         cannot be folded into boundary 1"
    );

    for arg in [
        r#"concat!("/api/", name)"#,
        "path.as_str()",
        r#""/api/".to_string()"#,
    ] {
        let src = format!("fn register(r: Router) -> Router {{ r.route({arg}, h) }}\n");
        assert_eq!(
            find_dynamic_path_args(&src),
            vec![arg.to_string()],
            "`{arg}` is not a bare literal and must be reported"
        );
    }

    // A literal path that merely *contains* braces is still a literal.
    let literal = "fn register(r: Router) -> Router { r.route(\"/api/{id}\", h) }\n";
    assert!(
        find_dynamic_path_args(literal).is_empty(),
        "a string literal holding braces is a literal, not a dynamic path"
    );
    assert_eq!(bare_route_paths(literal), vec!["/api/{id}"]);
}

/// Fixture C — a table entry relabelled `Protected` -> `Public`. The counts in
/// boundary 1 are unchanged, so only an explicit classification catches it.
#[test]
fn gate_rejects_downgraded_scope_fixture() {
    let fixture = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/api/switch", scope: Scope::Protected, method: Method::Post },
    RouteScope { path: "/api/icon", scope: Scope::Public, method: Method::Get },
];
"#;

    assert_eq!(
        scope_downgrades(fixture),
        vec!["/api/icon"],
        "a table entry relabelled Protected -> Public must be named"
    );
    assert_eq!(
        table_entries(fixture),
        vec!["/api/switch", "/api/icon"],
        "classification must not disturb the path parse"
    );

    // Reordering must not let a downgrade hide behind an index.
    let reordered = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/api/icon", scope: Scope::Public, method: Method::Get },
    RouteScope { path: "/api/switch", scope: Scope::Protected, method: Method::Post },
];
"#;
    assert_eq!(
        scope_downgrades(reordered),
        vec!["/api/icon"],
        "classification follows the entry, not its position"
    );

    // And an entry with no recognised scope token is unclassified, not silently
    // counted as safe.
    let unscoped = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 1] = [
    RouteScope { path: "/api/icon", method: Method::Get },
];
"#;
    assert_eq!(
        table_entry_labels(unscoped),
        vec![UNCLASSIFIED],
        "an entry with no scope token must not be silently accepted"
    );

    // The honest table must report no downgrade at all.
    let clean = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/api/switch", scope: Scope::Protected, method: Method::Post },
    RouteScope { path: "/", scope: Scope::BrowserSafe, method: Method::Get },
];
"#;
    assert!(
        scope_downgrades(clean).is_empty(),
        "Protected and BrowserSafe entries are not downgrades"
    );
    assert_eq!(
        table_entry_labels(clean),
        vec!["Protected", ROOT_GET],
        "the root GET / is counted apart from the business scopes"
    );

    // The root page is the one path allowed to be Public — but the exemption is
    // by path, so relabelling a business route must still red.
    let root_public = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/", scope: Scope::Public, method: Method::Get },
    RouteScope { path: "/api/switch", scope: Scope::Protected, method: Method::Post },
];
"#;
    assert!(
        scope_downgrades(root_public).is_empty(),
        "the root page is legitimately unauthenticated"
    );
    let api_public = r#"
const ROUTE_SCOPE_TABLE: [RouteScope; 2] = [
    RouteScope { path: "/", scope: Scope::Public, method: Method::Get },
    RouteScope { path: "/api/switch", scope: Scope::Public, method: Method::Post },
];
"#;
    assert_eq!(
        scope_downgrades(api_public),
        vec!["/api/switch"],
        "a Public label on a business route must red even when the root is Public"
    );
}

/// Fixture D — the pairing down the middle of the contract.
///
/// Every count in boundaries 1 and 5 stays perfectly balanced here: one route
/// site, one table entry, and the table still says `Protected`. Only comparing
/// what the *assembly* passes against what the *table* declares catches it.
#[test]
fn gate_rejects_scope_mismatch_fixture() {
    let fixture = r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/switch", AuthScope::Protected),
    ("/api/health", AuthScope::Protected),
];

fn register(router: Router, path: &'static str, method: Method, scope: AuthScope, handler: MethodRouter) -> Router {
    router.route(path, handler.layer(auth_layer(scope)))
}

fn build_router() -> Router {
    let mut router = Router::new();
    router = register(router, "/api/switch", Method::POST, AuthScope::Protected, axum::routing::post(handlers::switch_provider));
    router = register(router, "/api/health", Method::GET, AuthScope::Public, axum::routing::get(handlers::health_check));
    router
}
"#;

    // The counts really are balanced — that is what makes this fixture worth
    // having: no other boundary can see the downgrade.
    assert_eq!(
        bare_route_paths(fixture),
        vec!["/api/switch", "/api/health"],
        "both routes are literal and table-covered"
    );
    assert_eq!(
        table_entries(fixture),
        vec!["/api/switch", "/api/health"],
        "the table covers both routes"
    );
    assert!(
        unregistered_routes(fixture).is_empty(),
        "no route escapes the table, so boundary 1 is blind here"
    );
    assert!(
        scope_downgrades(fixture).is_empty(),
        "the table itself is still honest — it says Protected twice"
    );

    assert_eq!(
        scope_mismatches(fixture),
        vec!["/api/health：表声明 Protected，装配传 Public"],
        "a register() call passing a weaker scope than its table entry declares \
         must be named"
    );

    // Weakening in the other direction is also a mismatch, even though it fails
    // closed: the table would no longer describe what the server actually does.
    assert_eq!(
        scope_mismatches(
            r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/events", AuthScope::Protected),
];

fn build_router() -> Router {
    let mut router = Router::new();
    router = register(router, "/api/events", Method::GET, AuthScope::BrowserSafe, axum::routing::get(handlers::sse_events));
    router
}
"#
        ),
        vec!["/api/events：表声明 Protected，装配传 BrowserSafe"],
        "a stronger scope at the call site than the table declares is still a \
         mismatch — the table must describe reality, not aspire to it"
    );

    // A table entry nobody assembles is a mismatch too: boundary 1's equality
    // is the only other thing that would notice, and it says nothing about
    // which side is short.
    assert_eq!(
        scope_mismatches(
            r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/health", AuthScope::Protected),
    ("/api/orphan", AuthScope::Protected),
];

fn build_router() -> Router {
    let mut router = Router::new();
    router = register(router, "/api/health", Method::GET, AuthScope::Protected, axum::routing::get(handlers::health_check));
    router
}
"#
        ),
        vec!["/api/orphan：表有条目但没有对应的 register() 调用"],
        "a table entry with no assembly must be named, not left to the count"
    );

    // The honest pairing must be silent — otherwise the detector reports
    // everything and means nothing.
    let aligned = r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/switch", AuthScope::Protected),
    ("/api/events", AuthScope::BrowserSafe),
];

fn build_router() -> Router {
    let mut router = Router::new();
    router = register(router, "/api/switch", Method::POST, AuthScope::Protected, axum::routing::post(handlers::switch_provider));
    router = register(router, "/api/events", Method::GET, AuthScope::BrowserSafe, axum::routing::get(handlers::sse_events));
    router
}
"#;
    assert!(
        scope_mismatches(aligned).is_empty(),
        "a table and an assembly that agree produce no mismatch"
    );
    assert_eq!(
        table_entry_labels(aligned),
        vec!["Protected", "BrowserSafe"],
        "the scale assertion reads the scope the assembly passes"
    );
}

/// Fixture E — the real shape: routes declared at `register()` call sites rather
/// than as bare `.route(…)` literals.
///
/// This is the shape the controlled entry produces, so the count detectors must
/// read it. Without fixture E the positive detector could be counting only the
/// old shape and still passing on a file that had migrated away from it.
#[test]
fn gate_reads_controlled_entry_call_sites_fixture() {
    let fixture = r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/switch", AuthScope::Protected),
    ("/api/icon", AuthScope::BrowserSafe),
];

fn register<S>(router: Router<S>, path: &'static str, method: Method, scope: AuthScope, handler: MethodRouter<S>) -> Router<S> {
    router.route(path, handler.layer(auth_layer(scope)))
}

fn build_router() -> Router {
    let mut router = Router::new();
    router = register(router, "/api/switch", Method::POST, AuthScope::Protected, axum::routing::post(handlers::switch_provider));
    router = register(router, "/api/icon", Method::GET, AuthScope::BrowserSafe, axum::routing::get(handlers::get_icon));
    router
}
"#;

    assert_eq!(
        bare_route_paths(fixture),
        vec!["/api/switch", "/api/icon"],
        "paths declared at register() call sites must be counted — the \
         definition's own `register(` must not be"
    );
    assert_eq!(
        bare_route_literals(fixture).len(),
        table_entries(fixture).len(),
        "the two sides of boundary 1 balance on the controlled-entry shape"
    );
    assert!(
        register_path_is_static(fixture),
        "`path: &'static str` is what earns the forwarding site its exemption"
    );
    assert!(
        find_dynamic_path_args(fixture).is_empty(),
        "the `router.route(path, …)` inside register's body forwards a \
         `&'static str` and is exempt"
    );
    assert!(
        route_sites_outside_register(fixture).is_empty(),
        "the only `.route(` is inside the controlled entry's body"
    );

    // Without the `&'static str` pin the exemption evaporates and the forwarding
    // site is held to the literal rule — the exemption is earned, not assumed.
    let loose = fixture.replace("path: &'static str", "path: &str");
    assert!(
        !register_path_is_static(&loose),
        "a non-static path parameter does not pin the argument"
    );
    assert!(
        !find_dynamic_path_args(&loose).is_empty(),
        "without the pin, the forwarding site must be reported as a non-literal"
    );

    // A dynamically built path at a register() call site is the escape this
    // boundary exists for: it is not a literal, so it never enters boundary 1's
    // count, yet the route still reaches the router.
    let dynamic = r#"
const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/api/switch", AuthScope::Protected),
];

fn register<S>(router: Router<S>, path: &'static str, method: Method, scope: AuthScope, handler: MethodRouter<S>) -> Router<S> {
    router.route(path, handler.layer(auth_layer(scope)))
}

fn build_router(name: String) -> Router {
    let mut router = Router::new();
    router = register(router, format!("/api/{name}"), Method::GET, AuthScope::Protected, axum::routing::get(handlers::x));
    router
}
"#;
    assert_eq!(
        find_dynamic_path_args(dynamic),
        vec![r#"format!("/api/{name}")"#],
        "a dynamic path at a register() call site must be reported"
    );
    assert!(
        bare_route_paths(dynamic).is_empty(),
        "and it is invisible to the literal count — which is why boundary 2 \
         cannot be folded into boundary 1"
    );

    // An identifier passed as a path is non-literal too, and just as invisible.
    assert_eq!(
        find_dynamic_path_args(
            r#"
fn register<S>(router: Router<S>, path: &'static str, method: Method, scope: AuthScope, handler: MethodRouter<S>) -> Router<S> {
    router.route(path, handler.layer(auth_layer(scope)))
}

fn build_router(p: &str) -> Router {
    let mut router = Router::new();
    router = register(router, p, Method::GET, AuthScope::Protected, axum::routing::get(handlers::x));
    router
}
"#
        ),
        vec!["p".to_string()],
        "a bare identifier path escapes the literal count and must be reported"
    );
}
