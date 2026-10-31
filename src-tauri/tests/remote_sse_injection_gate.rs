//! Executable gate for the **SSE sender injection** into `ProxyService`.
//!
//! The failure this pins is a real one, not a hypothetical. The remote
//! server's broadcast channel used to be handed to `ProxyService` on the
//! **app-startup path only** (`lib.rs`, and only when `remote_config.enabled`
//! was already true at launch). Enabling remote management *later* goes
//! through `start_remote_server` → `start_remote`, and that path injected
//! nothing — so `proxy_started` / `proxy_stopped` / `takeover_changed` were
//! broadcast into a channel nobody was subscribed to. The desktop UI then
//! kept showing the old takeover state after the remote control plane had
//! already changed it.
//!
//! The injection therefore has to live in `start_remote`: that is the one
//! place both the startup path and the enable-later path go through.
//!
//! Two claims, in the order they fail:
//!
//! 1. `start_remote_injects_sse_sender` — in production source, the
//!    injection helper is defined once and called exactly once, and that one
//!    call site is inside `start_remote`. The channel it injects comes from
//!    the server's own state (not from a re-entrant accessor — see below).
//! 2. `lib_startup_no_longer_injects_inline` — the startup branch in
//!    `lib.rs` no longer injects on its own. A second injection site is
//!    exactly the shape that made the enable-later path miss it.
//!
//! The deadlock hazard the call site must avoid: `start_remote` holds the
//! `ManagedRemoteServer` **write** lock for its whole body, and
//! `ManagedRemoteServer::sse_tx()` takes the **read** lock of the same
//! `RwLock` — a re-entrant acquire on a `tokio::sync::RwLock` deadlocks.
//! The channel is therefore taken from the `RemoteServer` handle that the
//! caller already holds, never through the managed wrapper.
//!
//! The gate is falsifiable: claim 1 runs the same detector over a copy of the
//! production source with the call deleted, and claim 2 asserts the detector
//! recognises an injection call when one is present.
//!
//! Source paths are anchored with `CARGO_MANIFEST_DIR`, never cwd-relative:
//! the cwd of a test binary depends on the harness that launched it.

use std::path::{Path, PathBuf};

/// 读 `src-tauri/` 下的一个源文件。
fn read_source(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

/// 剥掉整行的行尾注释（`//` / `///`），让断言读的是代码而不是说明。
///
/// 这份 gate 要在注释里点名 `inject_proxy_sse_tx` —— 不剥注释的话，注释本身
/// 就会把 detector 喂绿。
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let code = match line.find("//") {
                Some(at) => &line[..at],
                None => line,
            };
            code.trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `fn <name>(...)` 的函数体（到下一个顶格 `}` 为止）。
fn fn_body(stripped_src: &str, name: &str) -> String {
    let needle = format!("fn {name}(");
    let start = stripped_src
        .find(&needle)
        .unwrap_or_else(|| panic!("源文件里必须有 {needle}"));
    let rest = &stripped_src[start..];
    let end = rest[1..]
        .find("\n}")
        .map(|rel| rel + 1)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// 注入调用点：helper 名字后面紧跟 `app` 实参。
///
/// 这里刻意只认 `inject_proxy_sse_tx(app` 这一种写法：helper 的**定义**写成
/// 换行参数列表（见生产代码），因此定义处不会出现这段文本，全文计数就是
/// 真正的调用点数 —— 「调用点恰好一处」这句话可以被机械地复核。
fn call_sites(src: &str) -> usize {
    src.matches("inject_proxy_sse_tx(app").count()
}

/// `lib.rs` 的启动分支是否还自己注入 SSE 通道。
fn injects_inline(src: &str) -> bool {
    src.contains("proxy_service.set_sse_tx")
}

/// 注入必须落在 `start_remote` 里：首次开启远程管理走的是
/// `start_remote_server` → `start_remote`，app 启动时的那条路上一次都没注入过，
/// 远程改了接管模式时桌面 UI 就停在旧值。
#[test]
fn start_remote_injects_sse_sender() {
    let stripped = strip_line_comments(&read_source("src/remote/mod.rs"));

    // helper 只能有一个定义：两处定义意味着两条注入路径，又回到「只有一条
    // 路径注入」的旧形状。
    let defs = stripped.matches("fn inject_proxy_sse_tx(").count();
    assert_eq!(
        defs, 1,
        "inject_proxy_sse_tx 必须恰好定义一次，实际 {defs} 处：两处定义等于两条 \
         注入路径，又会漏掉其中一条"
    );

    // 调用点恰好一处，且就在 start_remote 里。
    let calls = call_sites(&stripped);
    assert_eq!(
        calls, 1,
        "注入调用必须恰好一处，实际 {calls} 处：start_remote 的两条分支共用同一个 \
         注入点（通道由分支各自取出后交过来），重复注入只会让通道来源变得不可追溯"
    );

    let body = fn_body(&stripped, "start_remote");
    assert!(
        body.contains("inject_proxy_sse_tx(app"),
        "注入调用必须落在 start_remote 内：首次开启远程管理走的是 start_remote，\
         注入放在别处就漏掉了这条路径"
    );

    // 通道取自 server 自己的 state。
    assert!(
        body.contains("state().sse_tx.clone()"),
        "注入的通道必须取自 server 的 state：start_remote 的两条分支（增量更新 / \
         首次启动）拿到的 RemoteState 可能不是同一个，通道也就不是同一个"
    );

    // 不得走 ManagedRemoteServer::sse_tx()：那把锁此刻被本函数以写模式持着，
    // 再取读锁就是重入死锁。
    assert!(
        !body.contains(".sse_tx()"),
        "start_remote 持有 ManagedRemoteServer 写锁，不得调用 ManagedRemoteServer::sse_tx()：\
         那是同一把 RwLock 的读锁，重入即死锁"
    );

    // 负控：把调用删掉，detector 必须能红。
    let broken = stripped.replace("inject_proxy_sse_tx(app", "inject_proxy_sse_tx_removed(app");
    assert_ne!(
        broken, stripped,
        "负控失效：源码里找不到 `inject_proxy_sse_tx(app` 这段文本，detector 会对着同一份 \
         内容跑两遍，两次都绿"
    );
    assert_eq!(
        call_sites(&broken),
        0,
        "负控必须能红：删掉注入调用后 detector 仍数到调用点，说明它数的东西不是 \
         真正承重的那一处"
    );
    assert!(
        !fn_body(&broken, "start_remote").contains("inject_proxy_sse_tx(app"),
        "负控必须能红：删掉注入调用后 start_remote 里仍找得到它"
    );
}

/// app 启动分支不再自己注入：那是「只有启动路径注入」这个缺口的来源。
#[test]
fn lib_startup_no_longer_injects_inline() {
    let lib = read_source("src/lib.rs");

    // 负控：检测器必须认得出注入调用，否则下面那条否定断言是空转。
    assert!(
        injects_inline("state.proxy_service.set_sse_tx(sse_tx);"),
        "负控失效：检测器连手写的注入调用都认不出来，下面的否定断言就没有意义"
    );

    assert!(
        !injects_inline(&lib),
        "lib.rs 不得再自己注入 sse_tx：注入统一由 start_remote 负责，lib.rs 再来一次 \
         只会让「哪条路径注入了」变得不可判定"
    );

    assert!(
        !lib.contains("inject_proxy_sse_tx"),
        "lib.rs 不得直接调用 inject_proxy_sse_tx：注入点只有 start_remote 一处"
    );
}
