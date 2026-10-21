//! Executable gate for the approval-evidence chain
//! (`src/remote/pairing/handlers.rs` + `src/commands/remote.rs`).
//!
//! Why a static gate at all: the requirement is not "there is an
//! `ApprovalEvidence` type" and not "someone calls `evidence_for` somewhere" —
//! it is that a pairing approval carries **probed** evidence from the request
//! point all the way into the state machine. That is a *wiring* property across
//! two files, and wiring is exactly what a per-file behavioural test cannot
//! pin: swapping `super::evidence_for(peer)` for a hardcoded
//! `ApprovalEvidence::loopback(..)` still compiles, still type-checks, and still
//! pushes a perfectly well-formed event — the only thing that changed is that
//! the event stopped being evidence.
//!
//! Two boundaries are locked here:
//!
//! 1. `emit_evidence_breaks` — inside `pairing_request`'s body, the argument
//!    the desktop event actually carries must be a name bound in that same body
//!    by an `evidence_for(..)` call. It is located by slicing the function body,
//!    not by grepping the file: an `evidence_for(` elsewhere in the file must
//!    not be able to vouch for an `emit_approval(` that passes a literal.
//! 2. `approve_evidence_breaks` — the `approve(..)` call on the approval path
//!    must pass `Some(&evidence)` where `evidence` is bound by `evidence_for(..)`
//!    in the same body. A bare `None` reds.
//!
//! Both are written to be **falsifiable**: two negative fixtures rewrite the
//! real shapes into the two broken ones this gate exists to catch, and assert
//! the detectors go red on them. Without those, a detector that always returns
//! empty leaves the gate green while protecting nothing — which is the exact
//! shape of the all-green-but-empty static gate this file was written to replace.
//!
//! Comments are stripped before scanning, so a doc comment explaining *why* the
//! evidence is probed cannot trip the gate, and cannot vouch for it either.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal machine-local
//! path: the repo is cloned onto many machines and CI runners.

use std::path::Path;

const HANDLERS_RS: &str = "src/remote/pairing/handlers.rs";
const REMOTE_RS: &str = "src/commands/remote.rs";

/// The real source files this gate reads. Non-empty by construction, and the
/// real-tree test asserts each contributes lines — so a scan surface that
/// silently degrades to zero files cannot read as green.
const SCAN_SURFACE: [&str; 2] = [HANDLERS_RS, REMOTE_RS];

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
/// Joining matters: both the `emit_approval` and the `approve` call span several
/// source lines in the real files, and a detector that only looked at single
/// lines would see neither.
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

/// The `(start, end)` byte range of the body whose declaration contains
/// `marker`, located by that marker.
fn fn_body_range(text: &str, marker: &str) -> Option<(usize, usize)> {
    let at = text.find(marker)?;
    let open = text[at..].find('{')? + at;
    let end = match_bracket(text, open)?;
    Some((open, end))
}

// ---------------------------------------------------------------------------
// Call-site and argument helpers
// ---------------------------------------------------------------------------

/// Byte index of the `(` of every `name(` **call** in `text`.
///
/// Word-bounded so `approve(` cannot be matched inside `approve_bound(` or
/// `xapprove(`, and `fn` is excluded so the callee's own declaration is not
/// counted as a call site. Without both, the real tree's `approve_bound`
/// delegation would read as evidence plumbing inside `approve_pairing`.
fn call_parens(text: &str, name: &str) -> Vec<usize> {
    let needle = format!("{name}(");
    let mut sites = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(&needle) {
        let at = from + rel;
        let paren = at + name.len();
        let prev_is_ident = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let prev_word = text[..at]
            .split_whitespace()
            .next_back()
            .unwrap_or_default();
        if !prev_is_ident && prev_word != "fn" {
            sites.push(paren);
        }
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

/// Collapse internal whitespace runs to a single space and trim, so a
/// multi-line argument compares equal to its one-line spelling.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The identifier a `let <name> = …` statement binds.
fn let_binding(stmt: &str) -> Option<String> {
    let rest = stmt.trim().strip_prefix("let ")?;
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Names bound in `body` by a `let` whose right-hand side calls `evidence_for(`.
///
/// This is the whole of "the evidence came from a probe": a name is accepted
/// here only if some `let` in the *same body* produced it from `evidence_for`.
fn evidence_for_bindings(body: &str) -> Vec<String> {
    body.split(';')
        .filter(|stmt| stmt.contains("evidence_for("))
        .filter_map(let_binding)
        .collect()
}

/// The identifier inside `Some(&<name>)`, if the argument is that shape.
fn some_ref_name(arg: &str) -> Option<String> {
    let squeezed = squeeze(arg);
    let rest = squeezed.strip_prefix("Some(&")?.strip_suffix(')')?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------
// Boundary 1: the pushed event must carry probed evidence
// ---------------------------------------------------------------------------

/// Ways in which `pairing_request`'s `emit_approval` call fails to carry probed
/// evidence. Empty means the chain holds.
fn emit_evidence_breaks(src: &str) -> Vec<String> {
    let scan = scan(src);
    let Some((open, end)) = fn_body_range(&scan.text, "pub async fn pairing_request") else {
        return vec![
            "pairing_request handler is missing — the request point cannot \
                     be audited at all"
                .to_string(),
        ];
    };
    let body = &scan.text[open..end];

    let parens = call_parens(body, "emit_approval");
    if parens.is_empty() {
        // Zero call sites must read as a break, not as a pass: "found nothing to
        // complain about" and "the wiring holds" look identical otherwise.
        return vec![
            "pairing_request has no emit_approval( call site — nothing is \
                     pushed to the desktop, so nothing can be audited"
                .to_string(),
        ];
    }

    let bindings = evidence_for_bindings(body);
    let mut breaks = Vec::new();

    for paren in parens {
        let args = call_args(body, paren);
        let line = scan.line_at(open + paren);
        // The pushed evidence is whichever argument is a name that
        // `evidence_for(` produced in this body. A literal
        // `ApprovalEvidence::loopback(..)` is not a name and cannot match, which
        // is the point: the type is the same, only the provenance differs.
        let probed = args.iter().any(|arg| bindings.contains(&squeeze(arg)));
        if !probed {
            breaks.push(format!(
                "line {line}: emit_approval( must receive an argument bound by \
                 evidence_for(..) in this same function body; got {args:?} \
                 (evidence_for bindings in body: {bindings:?})"
            ));
        }
    }

    breaks
}

// ---------------------------------------------------------------------------
// Boundary 2: the approval path must pass the probe onward
// ---------------------------------------------------------------------------

/// The bodies that make up the `approve_pairing` evidence chain, nearest first.
///
/// `approve_pairing` itself only assembles an `AppHandle` and delegates: in the
/// real tree it contains neither `approve(` nor `evidence_for(`, because the
/// command layer's job is to resolve state and hand off. The evidence is
/// recomputed one function down, in `approve_bound` — the peer address recorded
/// at request time has to be looked up again there, since the approving command
/// arrives from the desktop, not from the pairing peer.
///
/// So the boundary follows exactly one delegation hop. It does not follow an
/// arbitrary chain: a gate that walks the whole call graph would end up
/// asserting something about code that merely happens to be reachable, and
/// could not name which hop lost the evidence.
fn approval_chain_bodies(scan: &Scan) -> Result<Vec<String>, String> {
    let Some((open, end)) = fn_body_range(&scan.text, "pub async fn approve_pairing") else {
        return Err(
            "approve_pairing command is missing — the approval path cannot \
                    be audited at all"
                .to_string(),
        );
    };

    let mut bodies = vec![scan.text[open..end].to_string()];
    if call_parens(&scan.text[open..end], "approve_bound").is_empty() {
        return Ok(bodies);
    }
    match fn_body_range(&scan.text, "async fn approve_bound") {
        Some((bo, be)) => bodies.push(scan.text[bo..be].to_string()),
        None => {
            return Err("approve_pairing calls approve_bound( but no such function \
                        exists — the evidence chain is unresolvable"
                .to_string())
        }
    }
    Ok(bodies)
}

/// Ways in which the `approve(..)` call on the approval path fails to carry
/// probed evidence. Empty means the chain holds.
fn approve_evidence_breaks(src: &str) -> Vec<String> {
    let scan = scan(src);
    let bodies = match approval_chain_bodies(&scan) {
        Ok(bodies) => bodies,
        Err(why) => return vec![why],
    };

    // The hop that actually touches the evidence: the first body that either
    // calls `approve(` or probes with `evidence_for(`.
    let Some(sink) = bodies
        .iter()
        .find(|body| !call_parens(body, "approve").is_empty() || body.contains("evidence_for("))
    else {
        return vec![
            "neither approve_pairing nor approve_bound calls approve( or \
                     evidence_for( — no evidence reaches the state machine"
                .to_string(),
        ];
    };

    let mut breaks = Vec::new();
    let bindings = evidence_for_bindings(sink);

    if !sink.contains("evidence_for(") {
        breaks.push(format!(
            "the approve(..) call site must be preceded by an evidence_for(..) probe \
             in the same function body; evidence_for bindings found: {bindings:?}"
        ));
    }

    let parens = call_parens(sink, "approve");
    if parens.is_empty() {
        breaks.push(
            "the evidence sink calls no approve(..) at all — the state machine is \
             never reached"
                .to_string(),
        );
        return breaks;
    }

    for paren in parens {
        let args = call_args(sink, paren);
        let third = squeeze(args.get(2).map_or("", String::as_str));
        match some_ref_name(&third) {
            Some(name) if bindings.contains(&name) => {}
            Some(name) => breaks.push(format!(
                "approve(..) third argument is `Some(&{name})`, but `{name}` is not \
                 bound by evidence_for(..) in this body; evidence_for bindings: \
                 {bindings:?}"
            )),
            None => breaks.push(format!(
                "approve(..) third argument must be `Some(&evidence)` where \
                 `evidence` comes from evidence_for(..); got `{third}` (full \
                 argument list: {args:?})"
            )),
        }
    }

    breaks
}

// ---------------------------------------------------------------------------
// Tests: the real tree
// ---------------------------------------------------------------------------

#[test]
fn pairing_request_pushes_probed_evidence_not_a_hardcoded_one() {
    let src = read_src(HANDLERS_RS);
    assert!(
        src.contains("emit_approval"),
        "{HANDLERS_RS} must define/use the approval push point — this gate audits \
         what that push carries, and with no push there is nothing to carry"
    );
    assert!(
        emit_evidence_breaks(&src).is_empty(),
        "the approval event must carry evidence probed by evidence_for(..): {:?}",
        emit_evidence_breaks(&src)
    );
}

#[test]
fn approval_command_passes_probed_evidence_and_never_none() {
    let src = read_src(REMOTE_RS);
    assert!(
        !approval_chain_bodies(&scan(&src))
            .unwrap_or_default()
            .is_empty(),
        "{REMOTE_RS} must expose a resolvable approve_pairing chain"
    );
    assert!(
        approve_evidence_breaks(&src).is_empty(),
        "the approve(..) call must receive Some(&evidence) from evidence_for(..), \
         never a bare None: {:?}",
        approve_evidence_breaks(&src)
    );
}

/// The scan surface must be real.
///
/// Without this, a gate whose file list silently degraded to zero files would
/// read every detector as empty and report green — the exact failure mode of
/// the empty static gate this file was written to replace.
#[test]
fn scan_surface_reads_the_real_chain_sources() {
    assert!(
        !SCAN_SURFACE.is_empty(),
        "the gate must name at least one real source file to scan"
    );

    let scanned: Vec<(&str, usize)> = SCAN_SURFACE
        .iter()
        .map(|relative| (*relative, scan(&read_src(relative)).lines.len()))
        .collect();
    for (relative, lines) in &scanned {
        assert!(
            *lines > 0,
            "{relative} contributed 0 scanned lines — the scan surface has degraded \
             and every detector would read as green"
        );
    }
    assert!(
        scanned.iter().any(|(relative, _)| *relative == HANDLERS_RS)
            && scanned.iter().any(|(relative, _)| *relative == REMOTE_RS),
        "both halves of the evidence chain must contribute to the scan: {scanned:?}"
    );
}

// ---------------------------------------------------------------------------
// Negative fixtures: proof the gate can go red
// ---------------------------------------------------------------------------

/// Fixture A — the same request handler with the probe swapped out for a
/// hardcoded loopback evidence, and no `evidence_for(` anywhere in it.
///
/// This is the change the gate exists to catch: it compiles, the pushed event
/// has a perfectly well-formed `ApprovalEvidence`, and the event has simply
/// stopped being evidence.
#[test]
fn gate_rejects_hardcoded_emit_evidence_fixture() {
    let fixture = r#"
pub async fn pairing_request(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    Json(body): Json<PairingRequestBody>,
) -> Response {
    let ticket = state.store.request_from_peer("10.9.9.9", &body.client_name, &[], now)?;
    let evidence = ApprovalEvidence::loopback("127.0.0.1:5000");
    super::emit_approval(
        state.sink.as_ref(),
        &ticket.request_id,
        &ticket.client_name,
        &ticket.user_code,
        evidence,
    );
    Json(ticket).into_response()
}
"#;

    let breaks = emit_evidence_breaks(fixture);
    assert_eq!(
        breaks.len(),
        1,
        "exactly the one emit_approval call site must be reported: {breaks:?}"
    );
    assert!(
        breaks[0].contains("evidence_for"),
        "the report must name what the argument should have come from: {breaks:?}"
    );

    // The clean shape must pass, or the fixture above proves nothing.
    let clean = r#"
pub async fn pairing_request(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    Json(body): Json<PairingRequestBody>,
) -> Response {
    let ticket = state.store.request_from_peer("10.9.9.9", &body.client_name, &[], now)?;
    let evidence = super::evidence_for(peer_addr(peer.as_ref())).await;
    super::emit_approval(
        state.sink.as_ref(),
        &ticket.request_id,
        &ticket.client_name,
        &ticket.user_code,
        evidence,
    );
    Json(ticket).into_response()
}
"#;
    assert!(
        emit_evidence_breaks(clean).is_empty(),
        "an emit_approval fed by an evidence_for binding must not be reported"
    );
    assert_eq!(
        evidence_for_bindings(&scan(clean).text),
        vec!["evidence".to_string()],
        "the binding extractor must see the probe, or the positive case is vacuous"
    );

    // An `evidence_for(` call in the file but *not* in this body must not vouch
    // for the emit site — the whole reason the scan is sliced to the body.
    let elsewhere =
        format!("{clean}\nfn helper() {{\n    let _ = evidence_for(addr);\n    let _ = 1;\n}}\n");
    assert!(
        emit_evidence_breaks(&elsewhere).is_empty(),
        "an extra helper outside the handler body must not change the verdict for \
         the handler itself"
    );
    let probe_moved_out = r#"
fn helper() {
    let evidence = evidence_for(addr);
    let _ = evidence;
}

pub async fn pairing_request(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    Json(body): Json<PairingRequestBody>,
) -> Response {
    let ticket = state.store.request_from_peer("10.9.9.9", &body.client_name, &[], now)?;
    super::emit_approval(
        state.sink.as_ref(),
        &ticket.request_id,
        &ticket.client_name,
        &ticket.user_code,
        evidence,
    );
    Json(ticket).into_response()
}
"#;
    assert_eq!(
        emit_evidence_breaks(probe_moved_out).len(),
        1,
        "a probe bound in another function must not vouch for this emit site"
    );

    // A handler that stopped pushing at all must red rather than pass by finding
    // nothing.
    assert_eq!(
        emit_evidence_breaks(
            r#"
pub async fn pairing_request(state: State<PairingApiState>) -> Response {
    Json(ticket).into_response()
}
"#
        )
        .len(),
        1,
        "a handler with no emit_approval call site must be reported, not silently \
         pass"
    );
}

/// Fixture B — the approval path passing a bare `None`, plus the case where the
/// probe is replaced by a hardcoded loopback evidence on that side too.
#[test]
fn gate_rejects_none_approve_evidence_fixture() {
    // The real shape, with the probe intact: this is the delegation the real
    // tree uses, and it must be green — otherwise the negative below proves
    // nothing.
    let aligned = r#"
pub async fn approve_pairing(
    app: AppHandle,
    request_id: String,
    user_code: String,
) -> Result<(), String> {
    let pairing = pairing_state(&app).await?;
    approve_bound(&pairing.store, &request_id, &user_code).await?;
    Ok(())
}

async fn approve_bound(
    store: &PairingStore,
    request_id: &str,
    user_code: &str,
) -> Result<(), String> {
    let peer = store.peer_for_request(request_id)?;
    let evidence = crate::remote::pairing::evidence_for(SocketAddr::new(peer, 0)).await;
    let decision = approve(
        store,
        user_code,
        Some(&evidence),
        true,
        true,
        Instant::now(),
    )?;
    Ok(decision)
}
"#;
    assert_eq!(
        approval_chain_bodies(&scan(aligned))
            .expect("chain must resolve")
            .len(),
        2,
        "the chain must resolve across the approve_pairing -> approve_bound hop"
    );
    assert!(
        approve_evidence_breaks(aligned).is_empty(),
        "the aligned two-function chain must produce no break: {:?}",
        approve_evidence_breaks(aligned)
    );

    // Bare `None` at the third argument.
    let none_evidence = aligned.replace("Some(&evidence)", "None");
    let breaks = approve_evidence_breaks(&none_evidence);
    assert_eq!(
        breaks.len(),
        1,
        "exactly the one approve(..) call site must be reported: {breaks:?}"
    );
    assert!(
        breaks[0].contains("Some(&evidence)"),
        "the report must name the required shape: {breaks:?}"
    );

    // Probe replaced by a hardcoded loopback evidence: the value is still an
    // `ApprovalEvidence`, but nothing probed it.
    let loopback = aligned.replace(
        "let evidence = crate::remote::pairing::evidence_for(SocketAddr::new(peer, 0)).await;",
        "let evidence = ApprovalEvidence::loopback(\"127.0.0.1:5000\");",
    );
    let breaks = approve_evidence_breaks(&loopback);
    assert_eq!(
        breaks.len(),
        2,
        "a missing probe reds twice — once for the absent evidence_for, once for \
         the unproven binding: {breaks:?}"
    );

    // Probe in a *different* function than the approve call: the boundary is
    // same-body, so this reds.
    let split_hops = r#"
pub async fn approve_pairing(app: AppHandle) -> Result<(), String> {
    let pairing = pairing_state(&app).await?;
    approve_bound(&pairing.store, &pairing.request_id).await?;
    Ok(())
}

async fn approve_bound(store: &PairingStore, request_id: &str) -> Result<(), String> {
    let evidence = evidence_for(addr).await;
    let _ = evidence;
    approve_inner(store)
}
"#;
    let breaks = approve_evidence_breaks(split_hops);
    assert_eq!(
        breaks.len(),
        1,
        "a sink that probes but never calls approve(..) must be reported: {breaks:?}"
    );

    // A command that delegates to nothing must red rather than pass by having no
    // approve call to inspect.
    assert_eq!(
        approve_evidence_breaks(
            r#"
pub async fn approve_pairing(app: AppHandle) -> Result<(), String> {
    let pairing = pairing_state(&app).await?;
    Ok(())
}
"#
        )
        .len(),
        1,
        "a chain with no approve(..) anywhere must be reported"
    );

    // A delegating command whose callee was renamed away is unresolvable, not
    // silently green.
    assert_eq!(
        approve_evidence_breaks(
            r#"
pub async fn approve_pairing(app: AppHandle) -> Result<(), String> {
    approve_bound(&pairing.store).await?;
    Ok(())
}
"#
        )
        .len(),
        1,
        "approve_pairing delegating to a missing approve_bound must be reported"
    );
}
