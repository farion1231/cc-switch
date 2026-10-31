//! Executable cross-language gate for the pairing approval event's wire keys.
//!
//! ## Why this file exists
//!
//! The approval event crosses a language boundary that no compiler checks: the
//! Rust side derives its JSON key names from `#[serde(rename_all = "camelCase")]`
//! (`src/remote/events.rs`), and the desktop side reads them from a hand-written
//! TypeScript interface (`src/hooks/useApprovalEventBridge.ts`). Nothing in
//! either build ties the two together.
//!
//! That gap was not theoretical. `ApprovalEvent.user_code` shipped against a Rust
//! struct that serializes `userCode`, so the desktop read `undefined` and
//! `event.user_code.trim()` threw inside `PairingApprovalDialog`'s render on
//! **every** approval request. React's error boundary swallowed the dialog, and
//! because the component test built its fixture with the same wrong key names,
//! the whole thing stayed green. The same mismatch silently mislabelled every
//! request's origin evidence: an unknown `sourceKind` degrades to `unavailable`,
//! which renders as "could not be verified" and flips the suggested action to
//! reject -- even for a fully verified tailnet peer.
//!
//! ## What is pinned
//!
//! Both sides are read as source text and their key sets compared exactly, in
//! both directions (a key present on one side only is a failure either way):
//!
//! 1. `approval_event_keys_match` -- `ApprovalEvent` fields.
//! 2. `approval_evidence_keys_match` -- `ApprovalEvidence` fields.
//! 3. `source_kind_variants_match` -- `SourceKind` variant names. `rename_all`
//!    applies to enum variants too, so the wire values are `"tailnet"` /
//!    `"notInTailnet"`, not the PascalCase spellings.
//! 4. `gate_flags_mismatched_fixture` -- the negative control. Without it the
//!    three above could be vacuous: a parser that silently returns empty vecs
//!    would compare `[] == []` and stay green while protecting nothing. This
//!    feeds the same comparison function a synthetic pair with one renamed key
//!    and requires it to be reported.
//!
//! Paths are anchored at `CARGO_MANIFEST_DIR`, never a literal `/Users/...`.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Source anchors
// ---------------------------------------------------------------------------

/// Rust side, relative to `CARGO_MANIFEST_DIR` (which is `src-tauri/`).
const RUST_EVENTS: &str = "src/remote/events.rs";

/// TypeScript side, relative to `CARGO_MANIFEST_DIR`.
const TS_BRIDGE: &str = "../src/hooks/useApprovalEventBridge.ts";

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_source(rel: &str) -> String {
    let path = repo_path(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("gate cannot read {} ({e})", path.display()))
}

// ---------------------------------------------------------------------------
// Source parsing
// ---------------------------------------------------------------------------

/// Drop whole-line comments on both sides of the boundary.
///
/// Line-based rather than a regex over the whole file: `//` inside a string
/// literal (a URL, say) must not be treated as a comment start.
fn strip_comment_lines(src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `request_id` -> `requestId`, matching serde's `rename_all = "camelCase"`
/// **applied to a struct field** (input is snake_case).
fn to_camel_case(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper_next = false;
    for c in snake.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `NotInTailnet` -> `notInTailnet`, matching serde's `rename_all = "camelCase"`
/// **applied to an enum variant** (input is PascalCase; serde only lowercases
/// the leading character, it does not re-split on underscores).
///
/// 这两个转换必须分开写：把变体名喂给 [`to_camel_case`] 会原样返回
/// （PascalCase 里没有下划线可拆），于是整条变体断言会拿 `NotInTailnet` 去比
/// `notInTailnet`，永远红 —— 或者反过来，如果比较函数被写松，就永远绿。
fn to_lower_camel_variant(pascal: &str) -> String {
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Does the item starting at `pos` carry `#[serde(rename_all = "camelCase")]`?
///
/// Only the attribute block immediately above the item counts, so an earlier
/// struct's attribute cannot be borrowed by a later one.
fn has_camel_rename(src: &str, pos: usize) -> bool {
    let head_start = src[..pos].rfind("\n\n").map(|i| i + 1).unwrap_or(0);
    src[head_start..pos].contains("rename_all = \"camelCase\"")
}

/// Body of `pub struct <name> { ... }` / `pub enum <name> { ... }`.
fn rust_item_body<'a>(src: &'a str, kind: &str, name: &str) -> Option<(&'a str, usize)> {
    let needle = format!("pub {kind} {name} {{");
    let start = src.find(&needle)?;
    let body_start = start + needle.len();
    let end = src[body_start..].find("\n}")? + body_start;
    Some((&src[body_start..end], start))
}

/// `pub <field>: T,` names inside a struct, camelCased when the struct says so.
fn rust_struct_keys(src: &str, name: &str) -> Option<Vec<String>> {
    let (body, pos) = rust_item_body(src, "struct", name)?;
    let camel = has_camel_rename(src, pos);
    let keys = body
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            let rest = t.strip_prefix("pub ")?;
            let (field, _) = rest.split_once(':')?;
            Some(if camel {
                to_camel_case(field.trim())
            } else {
                field.trim().to_string()
            })
        })
        .collect::<Vec<_>>();
    (!keys.is_empty()).then_some(keys)
}

/// Variant names inside an enum, camelCased when the enum says so.
fn rust_enum_variants(src: &str, name: &str) -> Option<Vec<String>> {
    let (body, pos) = rust_item_body(src, "enum", name)?;
    let camel = has_camel_rename(src, pos);
    let variants = body
        .lines()
        .filter_map(|l| {
            let t = l.trim().trim_end_matches(',');
            let ok = !t.is_empty()
                && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return None;
            }
            Some(if camel {
                to_lower_camel_variant(t)
            } else {
                t.to_string()
            })
        })
        .collect::<Vec<_>>();
    (!variants.is_empty()).then_some(variants)
}

/// `key: T` / `key?: T` names inside `export interface <name> { ... }`.
fn ts_interface_keys(src: &str, name: &str) -> Option<Vec<String>> {
    let needle = format!("export interface {name} {{");
    let start = src.find(&needle)? + needle.len();
    let end = src[start..].find("\n}")? + start;
    let keys = src[start..end]
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            let (key, _) = t.split_once(':')?;
            let key = key.trim().trim_end_matches('?').trim();
            let ok = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            ok.then(|| key.to_string())
        })
        .collect::<Vec<_>>();
    (!keys.is_empty()).then_some(keys)
}

/// String members of `export type <name> = "a" | "b";`.
fn ts_string_union(src: &str, name: &str) -> Option<Vec<String>> {
    let needle = format!("export type {name} =");
    let start = src.find(&needle)? + needle.len();
    let end = src[start..].find(';')? + start;
    let members = src[start..end]
        .split('|')
        .filter_map(|m| {
            let t = m.trim();
            let inner = t.strip_prefix('"')?.strip_suffix('"')?;
            Some(inner.to_string())
        })
        .collect::<Vec<_>>();
    (!members.is_empty()).then_some(members)
}

// ---------------------------------------------------------------------------
// The comparison itself (shared by the real assertions and the negative control)
// ---------------------------------------------------------------------------

/// Compare two key sets as **sets**, so declaration order is free but a key
/// present on one side only is always a failure.
fn compare_key_sets(label: &str, rust: &[String], ts: &[String]) -> Result<(), String> {
    let mut r = rust.to_vec();
    let mut t = ts.to_vec();
    r.sort();
    r.dedup();
    t.sort();
    t.dedup();

    let missing_in_ts: Vec<&String> = r.iter().filter(|k| !t.contains(k)).collect();
    let missing_in_rust: Vec<&String> = t.iter().filter(|k| !r.contains(k)).collect();

    if missing_in_ts.is_empty() && missing_in_rust.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{label}: 两端的键集不一致 —— 后端发了但前端没读：{missing_in_ts:?}；前端读了但后端没发：{missing_in_rust:?}\n\
         后端：{r:?}\n前端：{t:?}\n\
         （Rust 侧由 serde rename_all 决定键名，这里是它的推导结果；改任一端都必须同步改另一端）"
    ))
}

fn require<T>(opt: Option<T>, what: &str) -> T {
    opt.unwrap_or_else(|| {
        panic!("gate 没能从源码里解析出 {what} —— 解析器失效了，不能把「解析不到」当成「通过」")
    })
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

#[test]
fn approval_event_keys_match() {
    let rust = strip_comment_lines(&read_source(RUST_EVENTS));
    let ts = strip_comment_lines(&read_source(TS_BRIDGE));

    let rust_keys = require(
        rust_struct_keys(&rust, "ApprovalEvent"),
        "Rust 的 ApprovalEvent 字段",
    );
    let ts_keys = require(
        ts_interface_keys(&ts, "ApprovalEvent"),
        "TS 的 ApprovalEvent",
    );
    compare_key_sets("ApprovalEvent", &rust_keys, &ts_keys).unwrap();

    // 反空转：两侧都解析到东西了，才谈得上「相等」。
    assert!(
        rust_keys.len() >= 4,
        "ApprovalEvent 只解析到 {} 个键，解析器多半失效了：{rust_keys:?}",
        rust_keys.len()
    );
}

#[test]
fn approval_evidence_keys_match() {
    let rust = strip_comment_lines(&read_source(RUST_EVENTS));
    let ts = strip_comment_lines(&read_source(TS_BRIDGE));

    let rust_keys = require(
        rust_struct_keys(&rust, "ApprovalEvidence"),
        "Rust 的 ApprovalEvidence 字段",
    );
    let ts_keys = require(
        ts_interface_keys(&ts, "ApprovalEvidence"),
        "TS 的 ApprovalEvidence",
    );
    compare_key_sets("ApprovalEvidence", &rust_keys, &ts_keys).unwrap();
    // 反空转下限跟着结构体的字段数走：`ApprovalEvidence` 目前是 6 个字段。删掉那条
    // 主机名证据字段之前这里是 7 —— 下限必须与真实字段数同步，否则删字段会让门禁
    // 以「解析器失效」为名误红，而调低下限本身不会让比较变松（键集相等仍是硬条件）。
    assert!(
        rust_keys.len() >= 6,
        "ApprovalEvidence 只解析到 {} 个键，解析器多半失效了：{rust_keys:?}",
        rust_keys.len()
    );
}

#[test]
fn source_kind_variants_match() {
    let rust = strip_comment_lines(&read_source(RUST_EVENTS));
    let ts = strip_comment_lines(&read_source(TS_BRIDGE));

    let rust_variants = require(
        rust_enum_variants(&rust, "SourceKind"),
        "Rust 的 SourceKind 变体",
    );
    let ts_members = require(ts_string_union(&ts, "SourceKind"), "TS 的 SourceKind");
    compare_key_sets("SourceKind", &rust_variants, &ts_members).unwrap();
    assert!(
        rust_variants.len() >= 4,
        "SourceKind 只解析到 {} 个变体，解析器多半失效了：{rust_variants:?}",
        rust_variants.len()
    );

    // 写死一条最容易写错的：camelCase 作用于枚举变体，所以线上是 notInTailnet。
    assert!(
        rust_variants.contains(&"notInTailnet".to_string()),
        "SourceKind 的线上取值应当是 camelCase 后的 notInTailnet，实际 {rust_variants:?}"
    );
}

/// 负控：解析器与比较函数必须能对「一端改名」这件事报错。
///
/// 没有这一条，上面三条都可能退化成 `[] == []` 的空转 —— 本仓已有过
/// 「绿着但是空的」门禁，那种失效比没有门禁更糟，因为它会让人以为查过了。
#[test]
fn gate_flags_mismatched_fixture() {
    // 合成的一对：Rust 侧发 userCode，TS 侧读 user_code。
    let rust = "#[serde(rename_all = \"camelCase\")]\npub struct ApprovalEvent {\n    pub request_id: String,\n    pub user_code: String,\n\n}\n";
    let ts = "export interface ApprovalEvent {\n  requestId: string;\n  user_code: string;\n\n}\n";

    let rust_keys = require(rust_struct_keys(rust, "ApprovalEvent"), "合成 Rust 夹具");
    let ts_keys = require(ts_interface_keys(ts, "ApprovalEvent"), "合成 TS 夹具");

    let err = compare_key_sets("ApprovalEvent", &rust_keys, &ts_keys)
        .expect_err("两端键名不一致时必须报错，否则这道门禁是空转的");
    assert!(
        err.contains("userCode") && err.contains("user_code"),
        "报错信息里应当同时点出两端各自的键名，实际：{err}"
    );

    // 反向对照：把 TS 侧改成正确的键名后必须通过 —— 说明上一条的红来自那处差异，
    // 而不是来自解析器对合成夹具的其它不满。
    let fixed =
        "export interface ApprovalEvent {\n  requestId: string;\n  userCode: string;\n\n}\n";
    let fixed_keys = require(
        ts_interface_keys(fixed, "ApprovalEvent"),
        "修正后的 TS 夹具",
    );
    compare_key_sets("ApprovalEvent", &rust_keys, &fixed_keys).expect("两端一致时必须通过");

    // 枚举变体那一侧也要能红：PascalCase 是这里最容易犯的错。
    let enum_rust = "#[serde(rename_all = \"camelCase\")]\npub enum SourceKind {\n    Tailnet,\n    NotInTailnet,\n\n}\n";
    let enum_ts = "export type SourceKind =\n  | \"tailnet\"\n  | \"NotInTailnet\";\n";
    let ev = require(rust_enum_variants(enum_rust, "SourceKind"), "合成枚举");
    let tv = require(ts_string_union(enum_ts, "SourceKind"), "合成 union");
    let err = compare_key_sets("SourceKind", &ev, &tv).expect_err("变体大小写不一致时必须报错");
    assert!(
        err.contains("NotInTailnet") && err.contains("notInTailnet"),
        "报错信息里应当同时点出两端各自的写法，实际：{err}"
    );
}
