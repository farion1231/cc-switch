//! Executable gate for the CI shape this round's guarantees rest on.
//!
//! Three facts used to live only in prose — a report said "the remote-auth E2E
//! lives in its own job" and nothing in the repository would have noticed when
//! the job stopped existing.
//!
//! 1. **A dedicated job exists and demands the tailnet side.**
//!    `remote-auth-e2e` runs on a runner that can actually bind a second
//!    non-loopback address, with `CC_SWITCH_REQUIRE_TAILNET=1` so a `SKIP:` becomes a
//!    failure. A job that exists but forgets the env var is the worst shape: it
//!    looks like the side is covered, and it silently covers nothing.
//! 2. **No `#[ignore]` in the integration-test tree.** An `#[ignore]`d E2E or
//!    canary case is a case that no longer runs, and a case that does not run
//!    reads exactly like a case that passed. The rule this round fixed on is
//!    "default `cargo test` covers everything new".
//! 3. **The default `cargo test` is the one CI runs.** A suite can satisfy
//!    every per-file rule and still be invisible if the job that runs it
//!    narrowed the selection with `--ignored` or a filter.
//!
//! Two properties that make the gate worth more than the `grep` it replaces:
//!
//! - **A detector that cannot go red is a detector that does not exist.** Each
//!   scanner gets negative fixtures built from synthetic strings, so "always
//!   returns empty" cannot masquerade as a clean tree.
//! - **The scan surface is proved non-empty.** `no_new_test_file_uses_ignore_
//!   attribute` asserts the files this round added are actually on disk *and*
//!   inside the scanned set — a tree walk that silently stopped descending
//!   would otherwise read as "zero violations".
//!
//! One boundary worth stating because it looks like a contradiction: the
//! pre-existing `#[ignore]` on `config::tests::atomic_write_replaces_existing_
//! wsl_unc_file` stays exactly where it is, and the dedicated
//! `backend-windows-wsl2` job keeps running it explicitly. This gate scans
//! `tests/**` only. The claim being frozen is narrower than "the repository has
//! no `#[ignore]`" on purpose — it is "the E2E and canary cases added by this
//! round are not silenced". Widening it to the unit-test tree would mean
//! rewriting six unrelated tests that predate this work, which is scope this
//! round does not have.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal
//! `/Users/...`: the repo is cloned onto many machines and CI runners.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// The facts being frozen
// ---------------------------------------------------------------------------

/// The crate root, exported by cargo. Every path below is anchored here, never
/// at a literal `/Users/...`.
const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

/// The workflow under audit. `CARGO_MANIFEST_DIR` is `src-tauri/`, so the repo
/// root is one level up.
const WORKFLOW: &str = "../.github/workflows/ci.yml";

/// The job that owns the tailnet side. Naming it in a constant means the gate
/// and the workflow cannot drift apart by a typo in one of them.
const E2E_JOB: &str = "remote-auth-e2e";

/// The env var that turns `SKIP:` into a failure. Defined in production code as
/// `crate::remote::e2e::REQUIRE_TAILNET_ENV`; the literal here is the contract.
const REQUIRE_ENV: &str = "CC_SWITCH_REQUIRE_TAILNET";

/// The job that runs the default Rust suite on the three-OS matrix.
const BACKEND_JOB: &str = "backend";

/// The job that decides which change surfaces were touched. Its `outputs:` are
/// the other legal place to guarantee "定时 / 手动那一次一定跑" — see
/// `non_pr_events_run_anyway`.
const CHANGES_JOB: &str = "changes";

/// The job that legitimately runs one pre-existing `#[ignore]`d case, because
/// it needs a WSL2 UNC path no other runner provides.
const WSL_JOB: &str = "backend-windows-wsl2";

/// The one test that job exists for. Asserted present so "we stopped ignoring
/// things" can never be achieved by quietly dropping the case.
const WSL_IGNORED_TEST: &str = "atomic_write_replaces_existing_wsl_unc_file";

/// The integration-test files this round added. `no_new_test_file_uses_ignore_
// attribute` proves each is on disk and inside the scanned set, which is what
/// makes "zero violations" mean something.
const NEW_ROUND_TEST_FILES: [&str; 11] = [
    "tests/ci_workflow_gate.rs",
    "tests/app_type_serde.rs",
    "tests/auth_policy_gate.rs",
    "tests/auth_token_gate.rs",
    "tests/contract_docs_gate.rs",
    "tests/events_sink_gate.rs",
    "tests/fmt_gate.rs",
    "tests/privacy_gate.rs",
    "tests/removal_scope_gate.rs",
    "tests/route_scope_gate.rs",
    "tests/route_scope_prereq_gate.rs",
];

/// Change-surface globs the layering must key on. `remote_auth` is the new
/// tier: touching the tailnet path runs the Rust suite *and* every static gate
/// *and* the environment-demanding job; touching the UI runs vitest and the
/// locale gate. Both changed means both run.
const REQUIRED_FILTER_GLOBS: [&str; 5] = [
    "src-tauri/src/remote/**",
    "src-tauri/src/remote/auth/**",
    "src-tauri/src/remote/pairing/**",
    "src/**",
    "src/i18n/**",
];

// ---------------------------------------------------------------------------
// Reading the workflow
// ---------------------------------------------------------------------------

fn read_workflow() -> String {
    let path = Path::new(MANIFEST).join(WORKFLOW);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// The body of one `jobs:` entry, as raw text.
///
/// Jobs live at two-space indent, so a body ends at the next line indented no
/// further than its header. Blank lines are kept so multi-line `run:` scripts
/// stay readable in a failure message.
fn job_block(yaml: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    let header = format!("  {name}:");
    let start = lines.iter().position(|l| l.trim_end() == header)?;
    let indent = lines[start].len() - lines[start].trim_start().len();

    let mut body = String::new();
    for line in &lines[start + 1..] {
        if line.trim().is_empty() {
            body.push('\n');
            continue;
        }
        if line.len() - line.trim_start().len() <= indent {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    Some(body)
}

/// The raw RHS of a `key:` assignment inside a job, e.g. `"1"` for
/// `CC_SWITCH_REQUIRE_TAILNET: "1"`.
///
/// The value is returned **unparsed on purpose**. The gate wants the YAML
/// literal, not the Rust string: `CC_SWITCH_REQUIRE_TAILNET: 1` (bare int) and
/// `CC_SWITCH_REQUIRE_TAILNET: "1"` both become the string `"1"` once GitHub coerces
/// them, but only the quoted form says what the author meant, and the gate is
/// a statement about intent. A commented-out `# CC_SWITCH_REQUIRE_TAILNET: "1"` does
/// not match — `trim_start` leaves the `#` in place.
fn job_env_value(job: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    job.lines()
        .find_map(|l| l.trim_start().strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
}

/// The job's `if:` expression, without the key.
fn job_condition(job: &str) -> Option<String> {
    job.lines()
        .find_map(|l| l.trim_start().strip_prefix("if:"))
        .map(|v| v.trim().to_string())
}

/// Drop YAML comments, keeping the `#` that is inside a quoted scalar.
///
/// Only lines whose `#` is at the very start or preceded by whitespace count —
/// that is what separates a comment from a `#` sitting inside an unquoted run
/// script or a value. Quote state is tracked per line rather than across lines,
/// which is enough for this workflow and errs toward *keeping* text: an
/// unterminated quote on one line only means the next line is scanned without
/// quote awareness, never that real content is deleted.
///
/// This matters because the workflow documents its own rules. `backend`'s
/// command line literally reads "do not pass `--ignored` here", and a gate that
/// greps raw text would fail the workflow for writing down the rule it enforces.
fn strip_yaml_comments(job: &str) -> String {
    let mut out = String::new();
    for line in job.lines() {
        let bytes: Vec<char> = line.chars().collect();
        let mut quote: Option<char> = None;
        let mut cut = bytes.len();
        for (i, c) in bytes.iter().enumerate() {
            match quote {
                Some(q) if *c == q => quote = None,
                Some(_) => {}
                None if *c == '\'' || *c == '"' => quote = Some(*c),
                None if *c == '#' && (i == 0 || bytes[i - 1].is_whitespace()) => {
                    cut = i;
                    break;
                }
                None => {}
            }
        }
        let kept: String = bytes[..cut].iter().collect();
        out.push_str(kept.trim_end());
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Detector: `#[ignore]` attributes, comments and string literals excluded
// ---------------------------------------------------------------------------

/// The needle, assembled from fragments.
///
/// This file scans itself, so the attribute cannot be written out literally:
/// doing so would make the gate report its own detector as the violation it
/// exists to catch. Same trick `privacy_gate.rs` uses for its prefixes.
const IGNORE_ATTR: &str = concat!("#[", "ignore");

/// Blank out comments and string literals, preserving length and line breaks.
///
/// Length-preserving so a hit's index still maps to a line number; newlines are
/// restored explicitly because the skip loops below jump over them.
///
/// What it has to understand, and why each case is load-bearing:
///
/// - line comments (`//`, `//!`, `///`) — `removal_scope_gate.rs` *discusses*
///   `#[ignore]` in prose, and a naive grep would report the gate that forbids
///   it as a violation of it;
/// - nested block comments — `/* /* */ */` is legal and must not end early;
/// - raw strings — `r"#[ignore]"` is data, not an attribute;
/// - char literals — and, critically, **lifetimes**: `'static` opens with the
///   same byte as `'a'`, and a scanner that treats it as a char literal will
///   blank out arbitrary real code, turning a real violation invisible.
///
/// Blanking a literal is not a weakening: a `#[ignore]` attribute can never
/// appear inside one.
fn strip_comments_and_literals(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = vec![' '; b.len()];
    let mut i = 0;

    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let boundary = |k: usize| k == 0 || !(is_word(b[k - 1]));

    while i < b.len() {
        let c = b[i];

        // r"..." / r#"..."# / br#"..."# — but not the `r` of an identifier.
        let raw_start = if c == 'r' && boundary(i) {
            Some(i + 1)
        } else if c == 'b' && b.get(i + 1) == Some(&'r') && boundary(i) {
            Some(i + 2)
        } else {
            None
        };
        if let Some(mut j) = raw_start {
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&'"') {
                let closer: Vec<char> = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let mut k = j + 1;
                while k + closer.len() <= b.len() && b[k..k + closer.len()] != closer[..] {
                    k += 1;
                }
                i = (k + closer.len()).min(b.len());
                continue;
            }
            // Not a raw string after all (e.g. the `r` in `r#fn`). Fall through.
        }

        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if c == '/' && b.get(i + 1) == Some(&'*') {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i += 1;
            }
            continue;
        }

        if c == '"' {
            let mut j = i + 1;
            while j < b.len() {
                match b[j] {
                    '\\' => j += 2,
                    '"' => break,
                    _ => j += 1,
                }
            }
            i = (j + 1).min(b.len());
            continue;
        }

        if c == '\'' {
            if b.get(i + 1) == Some(&'\\') {
                // Escape sequence: skip to the closing quote.
                let mut j = i + 2;
                while j < b.len() {
                    match b[j] {
                        '\\' => j += 2,
                        '\'' => break,
                        _ => j += 1,
                    }
                }
                i = (j + 1).min(b.len());
                continue;
            }
            if b.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
            // A lifetime (`'static`, `'a`) — not a literal, leave the byte alone.
        }

        out[i] = c;
        i += 1;
    }

    // Newlines are structure, not content: keep them even where they were
    // inside a skipped span, so reported line numbers stay true.
    for (k, c) in b.iter().enumerate() {
        if *c == '\n' {
            out[k] = '\n';
        }
    }
    out.into_iter().collect()
}

/// 1-based line numbers of every `#[ignore]` attribute in real code.
fn find_ignore_attrs(src: &str) -> Vec<usize> {
    let stripped = strip_comments_and_literals(src);
    let mut hits = Vec::new();
    let bytes: Vec<char> = stripped.chars().collect();
    let needle: Vec<char> = IGNORE_ATTR.chars().collect();
    if needle.is_empty() || bytes.len() < needle.len() {
        return hits;
    }
    let mut line = 1usize;
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if bytes[i] == '\n' {
            line += 1;
        }
        if bytes[i..i + needle.len()] == needle[..] {
            hits.push(line);
        }
        i += 1;
    }
    hits
}

/// Every `.rs` file under `tests/`, sorted for a stable report.
fn scan_targets() -> Vec<PathBuf> {
    let dir = Path::new(MANIFEST).join("tests");
    let mut out = Vec::new();
    collect_rs(&dir, &mut out);
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

fn label(path: &Path) -> String {
    path.strip_prefix(MANIFEST)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// 1. The dedicated job exists, and it demands the tailnet side
// ---------------------------------------------------------------------------

#[test]
fn workflow_defines_remote_auth_e2e_job() {
    let yaml = read_workflow();
    let job = job_block(&yaml, E2E_JOB).unwrap_or_else(|| {
        panic!(
            "ci.yml 必须有一个 `{E2E_JOB}` job。没有它，远端认证那一侧的用例就只剩 \
                默认 `cargo test` 里的可见 SKIP —— 看上去跑了，其实什么也没验"
        )
    });

    let env = job_env_value(&job, REQUIRE_ENV).unwrap_or_else(|| {
        panic!(
            "`{E2E_JOB}` job 必须设 {REQUIRE_ENV}。缺了它，缺环境的机器会打印一行 \
                SKIP 后安静退出，这个 job 就退化成一份看起来像覆盖了 tailnet 的摆设"
        )
    });
    assert_eq!(
        env, "\"1\"",
        "{REQUIRE_ENV} 必须是带引号的 \"1\"：裸 int 依赖 GitHub 的类型强转，而带引号的形式\
         才说清了作者要的是一个值为 1 的字符串。读到的是 `{env}`"
    );

    assert!(
        job.contains("runs-on: macos-latest"),
        "{E2E_JOB} 跑在 macos-latest 上：只有它自带一块可绑定的非 loopback 地址，\
         ubuntu/windows runner 只有 lo0，远端那一侧根本无从跑起\n--- job ---\n{job}"
    );

    // 负向夹具：判据必须能红。env 缺失 / 值写成 0 / 注释掉，三种都不算配好。
    for (yaml, why) in [
        (
            "jobs:\n  remote-auth-e2e:\n    runs-on: macos-latest\n",
            "job 在但没设 env",
        ),
        (
            "jobs:\n  remote-auth-e2e:\n    env:\n      CC_SWITCH_REQUIRE_TAILNET: \"0\"\n",
            "env 设了但值不是 1",
        ),
        (
            "jobs:\n  remote-auth-e2e:\n    env:\n      # CC_SWITCH_REQUIRE_TAILNET: \"1\"\n",
            "env 被注释掉了",
        ),
    ] {
        let job = job_block(yaml, E2E_JOB).expect("fixture declares the job");
        let value = job_env_value(&job, REQUIRE_ENV);
        let ok = value.as_deref() == Some("\"1\"");
        assert!(
            !ok,
            "负向夹具「{why}」必须被判为不合规，否则 env 判据不会生效：{value:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Nothing added this round is silenced
// ---------------------------------------------------------------------------

#[test]
fn no_new_test_file_uses_ignore_attribute() {
    let targets = scan_targets();
    assert!(
        targets.len() >= NEW_ROUND_TEST_FILES.len(),
        "只扫到 {} 个文件，扫描面本身就不足以覆盖本轮新增的 {} 个",
        targets.len(),
        NEW_ROUND_TEST_FILES.len()
    );

    let labels: Vec<String> = targets.iter().map(|p| label(p)).collect();
    for expected in NEW_ROUND_TEST_FILES {
        assert!(
            labels.iter().any(|l| l == expected),
            "本轮新增的 {expected} 不在扫描面里 —— 扫描面退化了，「零违规」就只是没读到文件"
        );
    }

    let offenders: Vec<String> = targets
        .iter()
        .filter_map(|path| {
            let src = std::fs::read_to_string(path).ok()?;
            let hits = find_ignore_attrs(&src);
            (!hits.is_empty()).then(|| {
                format!(
                    "{}\n    第 {} 行",
                    label(path),
                    hits.iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join("、")
                )
            })
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "以下文件用 `#[ignore]` 关掉了用例。默认 `cargo test` 必须覆盖本轮全部新增用例：\
         一个被 ignore 的 E2E / canary 用例，和一个跑通了的用例，在 CI 界面上长得一模一样。\n  \
         {}",
        offenders.join("\n  ")
    );
}

/// The detector must go red on a real attribute and stay quiet on prose.
///
/// `removal_scope_gate.rs` genuinely contains the characters `#[ignore]` — in
/// doc comments explaining what the gate forbids. Without this test, "zero
/// findings" could be an artefact of a scanner that also misses real ones.
#[test]
fn ignore_detector_separates_code_from_prose() {
    let real = format!("#[test]\n{}\nfn probe() {{}}\n", IGNORE_ATTR);
    assert_eq!(
        find_ignore_attrs(&real),
        vec![2],
        "真实的属性必须被报出来，且行号要准"
    );

    let with_reason = format!("#[test]\n{} = \"network\"\nfn probe() {{}}\n", IGNORE_ATTR);
    assert_eq!(
        find_ignore_attrs(&with_reason),
        vec![2],
        "带 reason 的属性同样是属性"
    );

    for prose in [
        "//! A gate silenced with `#[ignore]` reads like a gate that never existed.\n",
        "/// `#[ignore]`d test still declares the name.\n",
        "/* nested /* #[ignore] */ still a comment */\n#[test]\nfn probe() {}\n",
        "const MSG: &str = \"#[ignore] this one only\";\n#[test]\nfn probe() {}\n",
        "const RAW: &str = r#\"#[ignore]\"#;\n#[test]\nfn probe() {}\n",
        "fn f<'a>(x: &'a str) -> &'a str { x }\n#[test]\nfn probe() {}\n",
        "let c = '#';\n#[test]\nfn probe() {}\n",
    ] {
        assert!(
            find_ignore_attrs(prose).is_empty(),
            "注释、字符串与字面量里的文本不是属性，不得被算成违规：\n{prose}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. The default `cargo test` is what CI runs
// ---------------------------------------------------------------------------

/// 这个 job 真的在跑「默认那一套 Rust 测试」吗 —— 命令是 `cargo test`（不额外收窄）
/// 还是 `cargo nextest run`（同样不额外收窄）都算，两者只是 runner 不同。
///
/// 判据的原意是「默认测试套件必须由 CI 真跑，新增用例不许只活在某个额外命令里」，
/// 不是把某个二进制钉死。上游 4.0.14 把 `backend` 换成了 nextest（每个用例一个进程
/// + 超时），所以这里跟随上游的工具链放宽到 nextest —— **放宽的只有 runner，
///   没有放宽「必须不带收窄参数」那一半**（收窄仍由下面的 narrowing 断言把关）。
fn runs_default_test_suite(runnable: &str) -> bool {
    const CARGO_TEST: &str = "cargo test --manifest-path src-tauri/Cargo.toml";
    const NEXTEST: &str = "cargo nextest run --manifest-path src-tauri/Cargo.toml";
    runnable.contains(CARGO_TEST) || runnable.contains(NEXTEST)
}

#[test]
fn rust_job_runs_default_cargo_test() {
    let yaml = read_workflow();

    for job_name in [BACKEND_JOB, E2E_JOB] {
        let job =
            job_block(&yaml, job_name).unwrap_or_else(|| panic!("ci.yml 必须有 `{job_name}` job"));
        // 注释里写着「不要加 --ignored」不算加了。
        let runnable = strip_yaml_comments(&job);

        assert!(
            runs_default_test_suite(&runnable),
            "{job_name} 必须跑不带过滤参数的默认测试套件（`cargo test` 或 \
             `cargo nextest run`，都要点名 src-tauri/Cargo.toml）—— 本轮新增的用例全在它里面\n\
             --- job ---\n{job}"
        );

        for narrowing in ["--ignored", "--test ", "-- --exact", "--skip"] {
            assert!(
                !runnable.contains(narrowing),
                "{job_name} 用了 `{narrowing}` 收窄了测试选择。被收窄掉的用例等于不存在：\
                 新增用例必须由默认 `cargo test` 覆盖，而不是由某个额外命令覆盖\n\
                 --- job ---\n{job}"
            );
        }
    }

    // `runs_default_test_suite` 的负控：判据必须能红。
    //
    // 这一条覆盖的是「默认套件跑没跑」这个判据本身。没有它，判据可以被悄悄换成
    // 一个恒真的形式（或者反过来，两个 runner 的名字都被写错），而门禁一声不吭。
    for fixture in [
        // 只跑了前端，Rust 侧完全没跑
        "  backend:\n    steps:\n      - run: pnpm test:unit\n",
        // nextest 但没点名 crate（`cd` 进去再跑）—— 与本文件对 `cargo test` 的
        // 要求同一把尺子：必须显式写 --manifest-path src-tauri/Cargo.toml
        "  backend:\n    steps:\n      - run: cargo nextest run --profile ci\n",
        // 既不是 cargo test 也不是 nextest
        "  backend:\n    steps:\n      - run: cargo build --manifest-path src-tauri/Cargo.toml\n",
    ] {
        let job = job_block(fixture, BACKEND_JOB).expect("fixture declares backend");
        assert!(
            !runs_default_test_suite(&strip_yaml_comments(&job)),
            "负向夹具必须被判为没跑默认测试套件，否则判据不会生效：\n{fixture}"
        );
    }

    // 正控：两个 runner 的合法形态都必须被认下，否则「放宽到 nextest」只是嘴上说说。
    for fixture in [
        "  backend:\n    steps:\n      - run: cargo test --manifest-path src-tauri/Cargo.toml\n",
        "  backend:\n    steps:\n      - run: cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci\n",
    ] {
        let job = job_block(fixture, BACKEND_JOB).expect("fixture declares backend");
        assert!(
            runs_default_test_suite(&strip_yaml_comments(&job)),
            "正控夹具必须被认下（否则判据过严，等于把上游的 nextest 判成违规）：\n{fixture}"
        );
    }

    // 没有回退既有门禁：那一条 `#[ignore]` 是有理由的（要 WSL2 UNC 路径），
    // 专用 job 必须还在显式跑它。断言用例名在场，是为了防止「不再 ignore 了」
    // 这个假象靠删掉用例达成。
    let wsl =
        job_block(&yaml, WSL_JOB).unwrap_or_else(|| panic!("ci.yml 必须保留 `{WSL_JOB}` job"));
    assert!(
        wsl.contains(WSL_IGNORED_TEST),
        "{WSL_JOB} 必须仍然点名 {WSL_IGNORED_TEST} —— 这条用例要 WSL2 UNC 路径，只能在这里跑"
    );
    assert!(
        wsl.contains("--ignored"),
        "{WSL_JOB} 必须仍然显式带 `--ignored` 跑那一条预存用例；本轮的规则针对新增用例，\
         不是把既有门禁顺手拆掉"
    );

    // 负向夹具：判据必须能红。
    let narrowed = concat!(
        "  backend:\n    steps:\n      - run: cargo test --manifest-path src-tauri/Cargo.toml --ignored\n",
        "  frontend:\n    steps:\n      - run: pnpm test:unit\n",
    );
    let job = job_block(narrowed, BACKEND_JOB).expect("fixture declares backend");
    assert!(
        strip_yaml_comments(&job).contains("--ignored"),
        "负向夹具必须真的带 --ignored，否则收窄判据无从证明会生效"
    );

    // 注释里的 `--ignored` 不算 —— 但同一行注释下的真实命令仍然算。
    // 两半都必须成立，否则「读掉注释」要么是掩耳盗铃，要么反过来把真命令放过。
    let commented = "  backend:\n    steps:\n\
                    \x20     # 默认 cargo test，不加 --ignored\n\
                    \x20     - run: cargo test --manifest-path src-tauri/Cargo.toml\n\
                    \x20  frontend:\n    steps:\n      - run: pnpm test:unit\n";
    let job = job_block(commented, BACKEND_JOB).expect("fixture declares backend");
    assert!(
        !strip_yaml_comments(&job).contains("--ignored"),
        "注释不是命令：写着「不要加 --ignored」不该把自己判成违规"
    );

    let commented_but_narrowed = "  backend:\n    steps:\n\
                                 \x20     # 放心，没问题\n\
                                 \x20     - run: cargo test --manifest-path src-tauri/Cargo.toml --ignored\n\
                                 \x20  frontend:\n    steps:\n      - run: pnpm test:unit\n";
    let job = job_block(commented_but_narrowed, BACKEND_JOB).expect("fixture declares backend");
    assert!(
        strip_yaml_comments(&job).contains("--ignored"),
        "注释遮不住命令：真收窄了测试选择就必须被报出来"
    );
}

// ---------------------------------------------------------------------------
// 4. Triggering is layered by change surface
// ---------------------------------------------------------------------------

/// 定时 / 手动那一次（非 PR 事件）这个改动面保证会跑吗 —— 缓存要靠它养热。
///
/// 两种写法语义等价，都算数，因为两边的 CI 各用了一种：
///
/// - **job 自己短路**：`if: github.event_name != 'pull_request' || needs.changes.outputs.X == 'true'`；
/// - **`changes` 输出兜底**：`X: ${{ steps.filter.outputs.X || 'true' }}` —— 过滤步骤只在
///   PR / push 上跑，定时与手动触发下它被跳过、输出为空，`|| 'true'` 把空值补成真。
///
/// 只认其中一种，就会把另一种形态判成违规。**两种都没有才判红**（负控见下）——
/// 那意味着定时全量那次这个 job 静默不跑，而报告里看不出来。
fn non_pr_events_run_anyway(cond: &str, changes_job: &str, filter: &str) -> bool {
    cond.contains("github.event_name != 'pull_request'")
        || changes_job.contains(&format!("outputs.{filter} || 'true'"))
}

#[test]
fn workflow_triggers_by_change_surface() {
    let yaml = read_workflow();

    for glob in REQUIRED_FILTER_GLOBS {
        assert!(
            yaml.contains(&format!("- \"{glob}\"")),
            "ci.yml 的 filters 缺少改动面 `{glob}`。触发必须按改动面分层：\
             remote/auth/pairing 改动走 Rust 全套 + 静态门禁，前端 src/i18n 改动走 vitest"
        );
    }

    let changes_job = job_block(&yaml, CHANGES_JOB)
        .unwrap_or_else(|| panic!("ci.yml 必须有 `{CHANGES_JOB}` job"));

    // 每个 job 的 `if:` 必须挂在对应的改动面上，而不是无差别跑或无差别不跑。
    let expectations = [
        (BACKEND_JOB, "backend"),
        (E2E_JOB, "remote_auth"),
        ("frontend", "frontend"),
    ];
    for (job_name, filter) in expectations {
        let job =
            job_block(&yaml, job_name).unwrap_or_else(|| panic!("ci.yml 必须有 `{job_name}` job"));
        let cond = job_condition(&job)
            .unwrap_or_else(|| panic!("{job_name} 必须有 if:，否则改动面分层不成立"));
        assert!(
            cond.contains(&format!("changes.outputs.{filter}")),
            "{job_name} 的 if: 必须读 `changes.outputs.{filter}`，实际是：{cond}"
        );
        assert!(
            non_pr_events_run_anyway(&cond, &changes_job, filter),
            "{job_name} 在定时全量 / push 到 main / 手动触发时必须跑完，维持缓存温度。\
             要么 `if:` 里写 `github.event_name != 'pull_request' || ...`，要么 `{CHANGES_JOB}` job 的 \
             `outputs.{filter}` 兜底成 `|| 'true'`；两种都没有时，那一次它静默不跑。\
             实际 if: = {cond}"
        );
    }

    // 负向夹具：判据必须能红。
    let unlayered = "  backend:\n    if: github.event_name != 'pull_request'\n";
    let job = job_block(unlayered, BACKEND_JOB).expect("fixture declares backend");
    let cond = job_condition(&job).expect("fixture has if:");
    assert!(
        !cond.contains("changes.outputs.backend"),
        "负向夹具（if: 不读改动面）必须被判为不合规，否则分层判据不会生效：{cond}"
    );

    // 负控二：两种「非 PR 也跑」的写法都没有 → 必须判红。
    let no_fallback = concat!(
        "  changes:\n    outputs:\n      backend: ${{ steps.filter.outputs.backend }}\n",
        "  backend:\n    if: needs.changes.outputs.backend == 'true'\n",
    );
    let changes_job = job_block(no_fallback, CHANGES_JOB).expect("fixture declares changes");
    let job = job_block(no_fallback, BACKEND_JOB).expect("fixture declares backend");
    let cond = job_condition(&job).expect("fixture has if:");
    assert!(
        !non_pr_events_run_anyway(&cond, &changes_job, "backend"),
        "负控：if: 不看事件、changes 输出也不兜底时必须判红，\
         否则「定时全量真的会跑」这条无从证明"
    );

    // 正控：两条合法路径各自都必须被认下，否则门禁会把其中一种形态误判成违规。
    let via_cond = concat!(
        "  changes:\n    outputs:\n      backend: ${{ steps.filter.outputs.backend }}\n",
        "  backend:\n    if: github.event_name != 'pull_request' || needs.changes.outputs.backend == 'true'\n",
    );
    let changes_job = job_block(via_cond, CHANGES_JOB).expect("fixture declares changes");
    let cond = job_condition(&job_block(via_cond, BACKEND_JOB).expect("fixture")).expect("if:");
    assert!(
        non_pr_events_run_anyway(&cond, &changes_job, "backend"),
        "正控：`if:` 里短路这一种形态必须被认下"
    );

    let via_outputs = concat!(
        "  changes:\n    outputs:\n      backend: ${{ steps.filter.outputs.backend || 'true' }}\n",
        "  backend:\n    if: needs.changes.outputs.backend == 'true'\n",
    );
    let changes_job = job_block(via_outputs, CHANGES_JOB).expect("fixture declares changes");
    let cond = job_condition(&job_block(via_outputs, BACKEND_JOB).expect("fixture")).expect("if:");
    assert!(
        non_pr_events_run_anyway(&cond, &changes_job, "backend"),
        "正控：`changes` 输出兜底这一种形态必须被认下"
    );
}
