//! Executable static gate for the remote-client auth **contract document**
//! (`docs/guides/remote-client-auth-{zh,en}.md`).
//!
//! The client half of this feature ("store the plaintext token in the macOS
//! Keychain") cannot be verified by code in this repo: there is no `[[bin]]`,
//! `src-tauri/src/bin` does not exist, and nothing in the tree reads or writes a
//! keychain/keyring. It is therefore delivered as an **external contract** for
//! third-party integrators — and an assertion nobody can falsify is worth
//! nothing, so the contract's fixed wording is pinned by this gate.
//!
//! Three properties, and the third is the one that makes the first two honest:
//!
//! 1. `contract_doc_contains_three_fixed_sentences` — both language versions
//!    carry all three fixed sentences.
//! 2. `readme_points_to_contract_doc` — README links the contract, so a
//!    third party can actually find it.
//! 3. `gate_flags_doc_missing_sentence_fixture` — a synthetic doc missing one
//!    sentence is reported. Without this the detector could return an empty vec
//!    unconditionally and leave the gate green while protecting nothing.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! `/Users/...`: the repo is cloned onto many machines and CI runners.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Source anchors
// ---------------------------------------------------------------------------

const ZH_DOC: &str = "docs/guides/remote-client-auth-zh.md";
const EN_DOC: &str = "docs/guides/remote-client-auth-en.md";
const README: &str = "README.md";

/// The three fixed sentences the contract must carry, Chinese version.
///
/// These are the load-bearing claims: where the plaintext token goes, how that
/// is achieved, and what no client may ever do. Rewording any of them is a
/// contract change and must be a deliberate edit here too.
const SENTENCES_ZH: [&str; 3] = ["存入 Keychain", "/usr/bin/security", "不得放进 URL"];

/// The same three sentences, English version. Same three claims, same order.
const SENTENCES_EN: [&str; 3] = [
    "store the plaintext token in the Keychain",
    "/usr/bin/security",
    "must never place credentials in a URL",
];

/// Beyond the three fixed sentences, each version must also say outright that
/// nothing here is verified by code, that no reference client is shipped, that
/// no correctness-only snippet is shipped, and that both client carriers are
/// covered — a programmatic one (`Authorization: Bearer`) and a browser one
/// (session cookie, no Keychain capability).
const EXTRA_ZH: [&str; 7] = [
    "不做代码验证",
    "不提供参考客户端",
    "不提供只做正确性校验的参考片段",
    "Authorization: Bearer",
    "cookie",
    "Keychain",
    "浏览器不具 Keychain 能力",
];

const EXTRA_EN: [&str; 7] = [
    "no code verification",
    "no reference client",
    "no correctness-only reference snippet",
    "Authorization: Bearer",
    "cookie",
    "Keychain",
    "browser has no Keychain capability",
];

fn extras_for(relative: &str) -> &'static [&'static str] {
    match relative {
        ZH_DOC => &EXTRA_ZH,
        EN_DOC => &EXTRA_EN,
        other => panic!("未登记的契约文档：{other}"),
    }
}

/// Repo root: `src-tauri/` is the cargo manifest dir, docs and README live one
/// level above it.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR 必须有父目录（仓库根）")
        .to_path_buf()
}

fn read_repo_file(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 {relative} 失败（{}）：{e}", path.display()))
}

/// Which sentences a given document must carry.
fn sentences_for(relative: &str) -> &'static [&'static str] {
    match relative {
        ZH_DOC => &SENTENCES_ZH,
        EN_DOC => &SENTENCES_EN,
        other => panic!("未登记的契约文档：{other}"),
    }
}

/// Collapse every run of whitespace into a single space.
///
/// Markdown prose is hard-wrapped by hand, so a fixed sentence can legitimately
/// straddle a line break in the source file while being one unbroken sentence
/// to a reader. Matching raw bytes would make that a gate failure — i.e. the
/// detector would punish re-wrapping a paragraph, which is the wrong thing for
/// a gate to punish. Normalizing both sides makes the check about the wording
/// rather than about where the line breaks happen to fall.
fn normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            in_space = true;
            continue;
        }
        if in_space && !out.is_empty() {
            out.push(' ');
        }
        in_space = false;
        out.push(ch);
    }
    out
}

/// The reusable detector: which of the document's required fixed sentences are
/// absent.
///
/// Returns the missing sentences themselves (not just a count) so the caller
/// can name exactly which claim the contract stopped making.
pub fn find_missing_sentences(doc: &str) -> Vec<&'static str> {
    find_missing_for(doc, &SENTENCES_ZH)
}

/// Same detector, for an explicitly chosen sentence set.
fn find_missing_for(doc: &str, sentences: &[&'static str]) -> Vec<&'static str> {
    let haystack = normalize(doc);
    sentences
        .iter()
        .copied()
        .filter(|sentence| !haystack.contains(&normalize(sentence)))
        .collect()
}

fn assert_clean(label: &str, doc: &str, sentences: &[&'static str]) {
    let missing = find_missing_for(doc, sentences);
    assert!(
        missing.is_empty(),
        "{label} 缺少契约固定措辞：{missing:?}\n三句必须齐全：{sentences:?}"
    );
}

// ---------------------------------------------------------------------------
// 1. The three fixed sentences
// ---------------------------------------------------------------------------

#[test]
fn contract_doc_contains_three_fixed_sentences() {
    for relative in [ZH_DOC, EN_DOC] {
        let doc = read_repo_file(relative);
        assert_clean(relative, &doc, sentences_for(relative));
    }

    // The client half is a contract, not code. Each language version has to say
    // so in its own words, cover both client carriers, and refuse to ship a
    // reference client — otherwise a reader assumes something in this repo
    // verifies it.
    for relative in [ZH_DOC, EN_DOC] {
        let doc = read_repo_file(relative);
        assert_clean(&format!("{relative} 附加条款"), &doc, extras_for(relative));
    }
}

// ---------------------------------------------------------------------------
// 2. README points at the contract
// ---------------------------------------------------------------------------

#[test]
fn readme_points_to_contract_doc() {
    let readme = read_repo_file(README);
    for relative in [ZH_DOC, EN_DOC] {
        assert!(
            readme.contains(relative),
            "README 必须指向契约文档 {relative}，否则第三方接入方找不到这份契约"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Negative fixture — the detector can actually go red
// ---------------------------------------------------------------------------

#[test]
fn gate_flags_doc_missing_sentence_fixture() {
    // A doc carrying sentences 1 and 3 but not 2.
    let fixture = concat!(
        "macOS 上应把明文 token 存入 Keychain。\n",
        "实现方式由接入方自选（本仓不约束）。\n",
        "任何客户端都不得放进 URL、提交进仓库或写进 shell history。\n",
    );

    let missing = find_missing_sentences(fixture);
    assert_eq!(
        missing,
        vec!["/usr/bin/security"],
        "缺一句时门禁必须精确报出缺的那一句"
    );

    // A doc missing all three must report all three — a detector that returns
    // one arbitrary item would pass the single-missing case above.
    assert_eq!(find_missing_sentences("").len(), 3);

    // And a doc carrying all three must be clean, so the fixture above is not
    // trivially "always reports something".
    let complete = concat!(
        "macOS 上应把明文 token 存入 Keychain。\n",
        "实现方式为调用 /usr/bin/security。\n",
        "任何客户端都不得放进 URL、提交进仓库或写进 shell history。\n",
    );
    assert!(
        find_missing_sentences(complete).is_empty(),
        "三句齐全的样本不得被误报"
    );
}
