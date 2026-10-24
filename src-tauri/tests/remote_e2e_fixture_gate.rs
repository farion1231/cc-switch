//! 可执行门禁：**手工 E2E 夹具**与**HTTP 验收脚本**这两件东西真的存在、真的能跑。
//!
//! 背景：远程管理这一侧的端到端验收此前只有 crate 内部的 E2E 用例
//! （`src/remote/e2e.rs`），它们直接调私有 API，验的是「判定表那一格」。缺了
//! 「一个真的 HTTP 客户端从外面打进来会看到什么」这一层，Host 闸门、Origin
//! 白名单、凭据通道这三件事在真实客户端眼里对不对，全靠人肉推理 —— 而这三件
//! 恰恰是最近几轮改动动得最多的地方。
//!
//! 这一层由两件东西组成：
//!
//! 1. `src/remote/e2e.rs` 里的 `#[ignore]` 用例 `remote_e2e_fixture`：起一个
//!    headless remote server，把 `{base_url, session_cookie, bearer}` 写到
//!    `/tmp/cc-switch-remote-e2e/fixture.json`，然后**保活**，等外部脚本连上来。
//! 2. `tests/remote-acceptance.mjs`：纯 Node（只用内置 `http` / `assert`，无新
//!    依赖）的断言脚本，对着夹具打真实 HTTP，断言六条出口。
//!
//! 三条门禁：
//!
//! 1. `fixture_is_ignored_and_addressable` —— 夹具存在、路径固定成验收脚本
//!    硬编码的那一条、且**必须**是 `#[ignore]` 的。它不是一个自包含用例：它要
//!    一直开着，等另一个终端里的脚本连上来。默认 `cargo test` 跑它就等于永远
//!    挂住 —— 所以 `#[ignore]` 不是可选的礼貌，是它能存在的前提。
//! 2. `acceptance_script_self_test` —— 脚本的 `--self-test` 自检模式真的绿。
//!    它在进程内起一个 mock server 跑**同一套**断言引擎，并做负控（一次故意
//!    错的 mock 响应必须被判 FAIL）—— 没有负控的话，「断言引擎恒真」也能绿。
//! 3. `acceptance_script_reports_missing_fixture` —— 夹具文件缺失时，脚本必须
//!    打印起夹具的命令并以非 0 退出。这条是操作者第一次跑它时唯一会撞上的
//!    分支，不钉住它就会变成一句看不出来的建议。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 验收脚本的落点：仓库根下的 `tests/remote-acceptance.mjs`。
const SCRIPT_RELATIVE: &str = "tests/remote-acceptance.mjs";

/// 夹具文件路径 —— 验收脚本硬编码的同一个字面量。
///
/// 两边各写一份路径常量而没有共同来源，所以这里把它当**契约**来钉：脚本里
/// 的那个字面量与 Rust 侧的必须逐字相同，否则操作者按提示起了夹具，脚本却
/// 去别处找文件，得到的结论是「夹具没写出来」——而真实原因是两边路径不一致。
const FIXTURE_PATH: &str = "/tmp/cc-switch-remote-e2e/fixture.json";

/// 仓库根目录（`CARGO_MANIFEST_DIR` 是 `src-tauri/`）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri/ 必须在仓库根下")
        .to_path_buf()
}

/// 读 `src-tauri/` 下的一个源文件。
fn read_source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

/// `fn <name>` 正上方那一段**紧邻**的属性行。
///
/// 只认紧邻的那一段：从 `fn` 往上走，遇到第一行不是 `#[…]` 的就停。因此上面
/// 一段文档注释会终止扫描 —— 文档注释里提到 `#[ignore]` 不会把这条检测喂绿。
fn attributes_above(src: &str, needle: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("源文件里必须有 {needle}"));
    let mut attrs: Vec<String> = Vec::new();
    for line in lines[..at].iter().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            attrs.push(trimmed.to_string());
        } else {
            break;
        }
    }
    attrs.reverse();
    attrs
}

/// 这个夹具用例是不是 `#[ignore]` 的。
fn is_ignored(src: &str) -> bool {
    attributes_above(src, "fn remote_e2e_fixture(")
        .iter()
        .any(|attr| attr.starts_with("#[ignore"))
}

/// 夹具文件是不是真的被写出来了（写的是那个固定路径常量）。
///
/// 匹配前先把空白全部去掉：rustfmt 会把 `fs::write(` 与它的第一个实参拆到两行
/// （实参列表超过 `fn_call_width` 就竖排），按原文逐字匹配的话，这条检测会红在
/// 「格式化」上，而不是红在「没写文件」上 —— 后者才是它要抓的东西。
fn writes_fixture_file(src: &str) -> bool {
    let squeezed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains("fs::write(REMOTE_E2E_FIXTURE_PATH")
}

/// 夹具用例存在、路径固定、且是 `#[ignore]` 的。
#[test]
fn fixture_is_ignored_and_addressable() {
    let src = read_source("src/remote/e2e.rs");

    // 夹具用例本身必须在。
    assert!(
        src.contains("fn remote_e2e_fixture("),
        "src/remote/e2e.rs 里必须有 remote_e2e_fixture 用例：外部验收脚本要有一个\
         能起服务、能交付凭据的夹具"
    );

    // 路径是契约：与验收脚本里硬编码的那一条逐字相同。
    assert!(
        src.contains(&format!("\"{FIXTURE_PATH}\"")),
        "夹具路径必须逐字是 {FIXTURE_PATH}：验收脚本按这个路径找文件，两边不一致时\
         操作者只会看到「夹具没写出来」"
    );

    // 路径常量确实被写进那个文件（不是只声明了一个常量）。
    assert!(
        writes_fixture_file(&src),
        "夹具必须真的写到那个路径上：只声明常量不写文件，脚本永远读不到"
    );

    // `#[ignore]` 是它能存在的前提：默认 `cargo test` 跑它就等于永远挂住。
    assert!(
        is_ignored(&src),
        "remote_e2e_fixture 必须带 #[ignore]：它不是自包含用例，它要保活等另一个\
         终端的脚本连上来，默认套件跑它会挂死整个测试进程"
    );

    // 负控一：把 `#[ignore]` 删掉，「被忽略」这条判定必须能红。
    let broken: String = src
        .lines()
        .filter(|line| !line.trim_start().starts_with("#[ignore"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(
        broken, src,
        "负控失效：源文件里找不到 `#[ignore` 这一行，检测器会对着同一份内容跑两遍，\
         两次都绿"
    );
    assert!(
        !is_ignored(&broken),
        "负控必须能红：删掉 #[ignore] 之后检测器仍说它被忽略了"
    );
    // 这次改动只该动 `#[ignore]` 一处：「写文件」那条判定若也跟着变，两条判定
    // 就是在测同一件事，其中一条必然是空转。
    assert!(
        writes_fixture_file(&broken),
        "负控只该动 #[ignore] 这一处：「写文件」这条判定跟着变了，说明两条判定\
         测的是同一件事"
    );

    // 负控二：把写文件那一句删掉，「写文件」这条判定必须能红。
    let without_write: String = src
        .lines()
        .filter(|line| !line.trim_start().starts_with("std::fs::write("))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(
        without_write, src,
        "负控失效：源文件里找不到 `std::fs::write(` 这一行"
    );
    assert!(
        !writes_fixture_file(&without_write),
        "负控必须能红：删掉写文件那一句之后检测器仍说它写了"
    );
}

/// 跑一次验收脚本，返回 (stdout, stderr, exit code)。
fn run_script(args: &[&str], env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let script = repo_root().join(SCRIPT_RELATIVE);
    assert!(
        script.exists(),
        "验收脚本必须在 {}：HTTP 端到端这一层全靠它",
        script.display()
    );

    let mut command = Command::new("node");
    command.arg(&script);
    command.args(args);
    // 环境里的这几个变量会盖掉夹具文件的内容 —— 门禁要验的是「按默认路径
    // 读夹具」这条路径，所以先清干净，别让外部环境插一脚。
    for name in [
        "CC_SWITCH_E2E_FIXTURE",
        "CC_SWITCH_E2E_BASE_URL",
        "CC_SWITCH_E2E_SESSION_COOKIE",
        "CC_SWITCH_E2E_BEARER",
    ] {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }

    let output = command
        .output()
        .unwrap_or_else(|e| panic!("跑不动 node（{}）：{e}", script.display()));
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code(),
    )
}

/// 验收脚本的自检模式：进程内 mock server 全绿 → 退出码 0。
#[test]
fn acceptance_script_self_test() {
    let (stdout, stderr, code) = run_script(&["--self-test"], &[]);
    assert_eq!(
        code,
        Some(0),
        "node {} --self-test 必须以 0 退出\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        SCRIPT_RELATIVE
    );
    assert!(
        stdout.contains("SELF-TEST"),
        "自检模式必须明确报出 SELF-TEST：跑的人要能从输出里一眼看出它验过的是\
         mock 而不是真服务\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

/// 夹具文件缺失时：打印起夹具的命令，并以非 0 退出。
#[test]
fn acceptance_script_reports_missing_fixture() {
    let missing = std::env::temp_dir().join("cc-switch-remote-e2e-missing-fixture.json");
    let _ = std::fs::remove_file(&missing);
    assert!(
        !missing.exists(),
        "负控前提不成立：{} 本来就存在，这条用例就成了一条空跑",
        missing.display()
    );

    let (stdout, stderr, code) =
        run_script(&[], &[("CC_SWITCH_E2E_FIXTURE", missing.to_str().unwrap())]);
    assert_ne!(
        code,
        Some(0),
        "夹具缺失时必须以非 0 退出：退出码 0 会让「验收通过」与「压根没跑」看起来\
         一回事\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("cargo test --lib remote_e2e_fixture"),
        "夹具缺失时必须打印起夹具的那条命令：操作者第一次跑它时唯一能做的事就是\
         照着那条命令起夹具\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}
