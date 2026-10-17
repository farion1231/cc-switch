//! Executable privacy gate: no real tailnet sample value ever lands in source.
//!
//! Tailscale tests want a "real" node to assert against, and the cheapest way to
//! get one is to paste what `tailscale status --json` printed on someone's
//! laptop: a `Node.StableID`, a 100.64/10 address, a login name. That works
//! once and then leaks forever — the values are in git history, in every clone,
//! and in every tarball of the repo, and they identify one person's tailnet.
//! The repo's gate scans by tree and by commit range, so a single pasted
//! sample is a finding, not a nit.
//!
//! So the rule is: **fixtures are synthesised, never observed.** A judge must
//! be sensitive to values that read as `u-other != u-self`, not to values that
//! read as one specific operator.
//!
//! This file is the machine check for that rule, and it scans both trees the
//! task owns — `tests/**` and `src/remote/**` (its `#[cfg(test)]` modules
//! included, since that is where a fixture would land).
//!
//! Three shapes, one detector each:
//!
//! 1. **tailnet IP** — the CGNAT block Tailscale allocates from (`100.64/10`)
//!    and the ULA block it hands out on the IPv6 side. Prose like
//!    `100.64.x.x` is *not* a sample and must not trip the detector.
//! 2. **`Node.StableID`** — a real node id is `n` + a short base64-ish run that
//!    contains digits. Requiring a digit is what keeps ordinary English (`n` +
//!    ten letters) out of the results.
//! 3. **operator account name** — a real tailnet login name always lives under
//!    a `*.ts.net` domain. Test doubles legitimately use `@example.invalid` /
//!    `@evil.example`, so the reserved-TLD suffix is the discriminator.
//!
//! Two properties this gate would be worthless without:
//!
//! - **Negative fixtures.** Every detector gets a fixture that must go red, so
//!   "always returns empty" cannot masquerade as a clean tree.
//! - **Non-vacuity.** The real tree is asserted to actually contain the text
//!   the detectors key on (documentation ranges, the word `StableID`), proving
//!   the scan is reading files rather than empty input.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! `/Users/...`: the repo is cloned onto many machines and CI runners.
//!
//! Fixtures in this file are assembled from fragments so that the gate does
//! not flag its own negative fixtures — the same trick `e2e.rs` uses when it
//! scans itself for a forbidden constructor call.

use std::path::{Path, PathBuf};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

/// The two trees this gate owns.
const SCAN_ROOTS: [&str; 2] = ["tests", "src/remote"];

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Tailscale's IPv4 allocation: the CGNAT block `100.64/10`.
///
/// Two boundaries do the work. On the left, `1100.64.1.2` must not parse — a
/// detector that cries wolf on version strings gets switched off. On the right,
/// the match stops at the first character that cannot be part of an address, and
/// the text in between must be **exactly three** more octets: a fifth octet is a
/// rejection, not a silent truncation into an address nobody wrote down.
fn tailnet_ipv4(line: &str) -> bool {
    let b = line.as_bytes();
    line.match_indices("100.").any(|(start, _)| {
        if start > 0 && (is_word(b[start - 1]) || b[start - 1] == b'.') {
            return false;
        }
        let rest = &line[start + 4..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let pieces: Vec<&str> = rest[..end].split('.').collect();
        let [a, b, c] = pieces[..] else {
            return false;
        };
        let (Ok(second), Ok(third), Ok(fourth)) =
            (a.parse::<u32>(), b.parse::<u32>(), c.parse::<u32>())
        else {
            return false;
        };
        (64..=127).contains(&second) && third <= 255 && fourth <= 255
    })
}

/// Tailscale's IPv6 ULA block.
///
/// The prefix is assembled from segments on purpose: this file scans itself,
/// so writing the prefix as one literal would make the gate report its own
/// detector as a leak.
fn tailnet_ipv6(line: &str) -> bool {
    const ULA: &str = concat!("fd7a", ":115c", ":a1e0");
    line.contains(ULA)
}

/// A `Node.StableID`: `n` + a base64-ish run carrying at least one digit.
///
/// The digit requirement is load-bearing. Without it every eleven-letter word
/// starting with `n` is a "stable id" and the gate is noise; with it, an
/// observed sample is caught and prose is not.
fn node_stable_id(line: &str) -> bool {
    let b = line.as_bytes();
    for i in 0..b.len() {
        if b[i] != b'n' {
            continue;
        }
        if i > 0 && is_word(b[i - 1]) {
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_alphanumeric() {
            j += 1;
        }
        let body = &line[i + 1..j];
        if !(10..=26).contains(&body.len()) {
            continue;
        }
        if body.bytes().any(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// A real tailnet login name: `local@<tailnet>.ts.net`.
///
/// Reserved documentation domains (`.invalid`, `.example`) are what a fixture
/// *should* use, so they are deliberately not flagged.
fn operator_login_name(line: &str) -> bool {
    line.match_indices('@').any(|(at, _)| {
        let rest = &line[at + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .unwrap_or(rest.len());
        let domain = &rest[..end];
        // 必须**结束于** `.ts.net` 且前面还剩得下域名主体：`@.ts.net`（没有主体）
        // 与 `@ts.net` 都不是登录名。
        domain.ends_with(".ts.net") && domain.len() > ".ts.net".len()
    })
}

/// Every tailnet sample value in `src`, one finding per shape per line.
///
/// The finding names the **shape** and the line, never the value: echoing a
/// leaked `StableID` or login name into CI logs would spread the very thing
/// this gate exists to keep out of the tree. The maintainer opens the file at
/// the reported line.
pub fn find_tailnet_samples(src: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let no = idx + 1;
        for (shape, hit) in [
            ("tailnet IPv4", tailnet_ipv4(line)),
            ("tailnet IPv6", tailnet_ipv6(line)),
            ("Node.StableID", node_stable_id(line)),
            ("tailnet LoginName", operator_login_name(line)),
        ] {
            if hit {
                found.push(format!("{no}: {shape} 样本值"));
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Tree walk
// ---------------------------------------------------------------------------

/// Every `.rs` file under the two owned trees, sorted for a stable report.
fn scan_targets() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in SCAN_ROOTS {
        let dir = Path::new(MANIFEST).join(root);
        collect_rs(&dir, &mut out);
    }
    out.sort();
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Short label relative to `src-tauri/`, so a finding names a file the
/// maintainer can open.
fn label(path: &Path) -> String {
    path.strip_prefix(MANIFEST)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Scan the real trees. Used by the acceptance test and by the shape-counting
/// helper below.
fn scan_real_tree() -> Vec<(String, Vec<String>)> {
    scan_targets()
        .into_iter()
        .filter_map(|path| {
            let src = std::fs::read_to_string(&path).ok()?;
            let hits = find_tailnet_samples(&src);
            (!hits.is_empty()).then_some((label(&path), hits))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fixtures — assembled from fragments so this file does not flag itself
// ---------------------------------------------------------------------------

/// `{"Node":{"StableID":"n…"}}`
fn stable_id_fixture() -> String {
    format!(
        "{{\"Node\":{{\"StableID\":\"{}\"}}}}",
        ["n", "A1b2C3", "d4E5"].concat()
    )
}

/// A tailnet IPv4 literal.
fn tailnet_ipv4_fixture() -> String {
    ["100.", "64.", "1.", "2"].concat()
}

/// A `.ts.net` login name.
fn login_name_fixture() -> String {
    format!("\"user@{}\"", ["example", "ts", "net"].join("."))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn privacy_gate_flags_tailnet_stable_id_fixture() {
    let fixture = stable_id_fixture();
    let found = find_tailnet_samples(&fixture);
    assert!(
        !found.is_empty(),
        "一条带 `Node.StableID` 的样本必须被报出来，否则门禁对最要紧的那一类完全失效"
    );
    assert!(
        found.iter().any(|f| f.contains("Node.StableID")),
        "发现必须点名形态：{found:?}"
    );
    assert!(
        found.iter().any(|f| f.starts_with("1:")),
        "发现必须带 1 起始的行号：{found:?}"
    );

    // 形态被识别不等于取值被识别：StableID 的判据是 `n` + 含数字的短串。
    // 短值（现有 whois 夹具的形态）与无数字值都不该被当成样本。
    for shape in [
        r#"{"Node":{"StableID":"s1"}}"#,
        r#"{"Node":{"StableID":"nshort"}}"#,
        r#"{"Node":{"StableID":"nonotadicits"}}"#,
    ] {
        assert!(
            !find_tailnet_samples(shape)
                .iter()
                .any(|f| f.contains("StableID")),
            "自造 / 过短的 StableID 不得被当成真实样本：{shape}"
        );
    }

    // 嵌在较长标识符里（前后有词字符）也不是一个独立的 id。
    let glued = format!("peer_{}_host", ["n", "A1b2C3", "d4E5"].concat());
    assert!(
        find_tailnet_samples(&glued)
            .iter()
            .all(|f| !f.contains("StableID")),
        "被词字符粘住的字符串不是独立取值：{glued}"
    );
}

#[test]
fn privacy_gate_flags_tailnet_ip_and_login_fixtures() {
    let ip = tailnet_ipv4_fixture();
    let found = find_tailnet_samples(&format!("let peer = \"{ip}\";"));
    assert!(
        found.iter().any(|f| f.contains("tailnet IPv4")),
        "100.64/10 是 Tailscale 的分配段，必须被报出来：{found:?}"
    );

    // CGNAT 段的两端都在范围内，段外一格就不在。
    for outside in ["100.63.0.1", "100.128.0.1"] {
        assert!(
            !find_tailnet_samples(outside)
                .iter()
                .any(|f| f.contains("tailnet IPv4")),
            "{outside} 不在 CGNAT 段内"
        );
    }

    // 五段不是 IPv4：截成前四段等于认了一个源码里根本没写过的地址。
    assert!(
        !find_tailnet_samples("100.64.1.23.4")
            .iter()
            .any(|f| f.contains("tailnet IPv4")),
        "五段地址不得被截成四段后当成样本"
    );

    // 文档保留段与私网段是夹具该用的地址。
    for reserved in ["198.51.100.7", "192.168.1.10", "172.16.0.1", "10.0.0.1"] {
        assert!(
            find_tailnet_samples(reserved).is_empty(),
            "{reserved} 是文档 / 私网段，不是 tailnet 样本"
        );
    }

    // IPv6 段同理。地址由片段拼出：本文件扫自己，写死就等于自己举报自己。
    let ula = format!("{}::1", concat!("fd7a", ":115c", ":a1e0"));
    assert!(
        find_tailnet_samples(&ula)
            .iter()
            .any(|f| f.contains("tailnet IPv6")),
        "Tailscale 的 ULA 段必须被报出来"
    );

    let login = login_name_fixture();
    let found = find_tailnet_samples(&format!("let login = {login};"));
    assert!(
        found.iter().any(|f| f.contains("tailnet LoginName")),
        "真实 tailnet 的登录名一律落在保留后缀下，必须被报出来：{found:?}"
    );

    // 夹具该用的保留域名恰好都不在判据内 —— 这正是「自造值」的形状。
    for fake in [
        "peer-under-test@example.invalid",
        "attacker@evil.example",
        "user@localhost",
    ] {
        assert!(
            !find_tailnet_samples(fake)
                .iter()
                .any(|f| f.contains("tailnet LoginName")),
            "{fake} 是自造账号，不得被当成真实 tailnet 身份"
        );
    }
}

#[test]
fn privacy_gate_accepts_real_tree() {
    let targets = scan_targets();
    assert!(
        targets.len() >= 20,
        "只扫到 {} 个文件，门禁几乎没覆盖东西 —— 至少应覆盖 tests/ 与 \
         src/remote/ 全树的 .rs",
        targets.len()
    );

    let offenders = scan_real_tree();
    assert!(
        offenders.is_empty(),
        "以下文件里出现了真实 tailnet 样本值。夹具必须是自造的：判据该对什么值敏感，\
         就让它对明写着 `u-other != u-self` 的值敏感，而不是对某个人的真实节点敏感。\n  \
         {}",
        offenders
            .iter()
            .map(|(file, hits)| format!("{file}\n    {}", hits.join("\n    ")))
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    // 反证这次扫描不是空的：真实树里确实存在「长得像但不是样本」的文本。
    // `src/remote/e2e.rs` 用散文提到 CGNAT 段（形如 `100.64.x.x`），而
    // `whois.rs` / `pairing/` 里有 `StableID` 键名与自造取值。它们都必须在。
    let e2e = std::fs::read_to_string(Path::new(MANIFEST).join("src/remote/e2e.rs"))
        .expect("src/remote/e2e.rs must be readable");
    let whois = std::fs::read_to_string(Path::new(MANIFEST).join("src/remote/whois.rs"))
        .expect("src/remote/whois.rs must be readable");
    assert!(
        e2e.contains("100.64"),
        "夹具漂移：e2e.rs 应当以散文提到过 CGNAT 段，否则「散文不误报」无从证明"
    );
    assert!(
        find_tailnet_samples(&e2e).is_empty(),
        "散文里的 CGNAT 段（形如 100.64.x.x）不是样本值"
    );
    assert!(
        whois.contains("StableID"),
        "夹具漂移：whois.rs 应当仍按 JSON 路径引用 StableID 键名"
    );
    assert!(
        find_tailnet_samples(&whois).is_empty(),
        "whois.rs 引用的是键名与自造取值，不是真实节点 id"
    );
}

#[test]
fn scan_covers_both_owned_trees() {
    let labels = scan_targets().iter().map(|p| label(p)).collect::<Vec<_>>();

    assert!(
        labels.iter().any(|l| l.starts_with("tests/")),
        "门禁必须扫 tests/**"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("src/remote/")),
        "门禁必须扫 src/remote/**，其 #[cfg(test)] 模块里的夹具同样会泄露"
    );
    assert!(
        labels
            .iter()
            .all(|l| l.starts_with("tests/") || l.starts_with("src/remote/")),
        "扫描范围外泄：{labels:?}"
    );

    // 本文件自己也在扫描范围内 —— 所以它的夹具必须由片段拼出来，而不是写死。
    assert!(
        labels.iter().any(|l| l == "tests/privacy_gate.rs"),
        "门禁必须把自己也扫进去（否则真实样本可以藏在门禁文件里）"
    );
    let self_src = std::fs::read_to_string(Path::new(MANIFEST).join("tests/privacy_gate.rs"))
        .expect("this file must be readable");
    assert!(
        find_tailnet_samples(&self_src).is_empty(),
        "门禁文件自己被扫出了样本值 —— 说明负例夹具被写成了字面量"
    );
}
