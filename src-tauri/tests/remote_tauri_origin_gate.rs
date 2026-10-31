//! Executable gate for the **Windows** webview origin in the remote CORS
//! whitelist.
//!
//! The failure this pins is a real one, not a hypothetical. Tauri 2 only serves
//! the webview over HTTPS on Windows when `app.security.useHttpsScheme` is set,
//! and `src-tauri/tauri.conf.json` does not set it. So the two schemes exist
//! simultaneously on Windows:
//!
//! - `http://tauri.localhost` — the default, and therefore the origin the
//!   settings page's `checkServerStatus` `fetch` and the desktop SSE stream
//!   actually carry;
//! - `https://tauri.localhost` — the one that would appear only once someone
//!   opted into `useHttpsScheme`.
//!
//! With just the two historical entries in `TAURI_WEBVIEW_ORIGINS`, the first
//! origin matches nothing on the list. `remote::auth`'s policy layer answers 403
//! for an unlisted `Origin`, so `handleEnabledToggle` reads that as "the server
//! failed to start" — while the listener is up and healthy. The desktop's own UI
//! cannot start remote management on Windows.
//!
//! Two claims, in the order they fail:
//!
//! 1. `constant_lists_three_origins` — the constant carries all three origins,
//!    and carries **only** them: no wildcard, no `Origin: null`, no blank. The
//!    exact length is asserted, so a fourth entry cannot slip in unnoticed and
//!    an unlisted one cannot be swapped out for a listed one.
//! 2. `tauri_origins_reach_allowed_origins` — the origins the middleware layer
//!    judges by are the ones the constant declares. Without this, a constant
//!    could list all three while the assembled whitelist keeps serving the old
//!    two — the constant would read green and Windows would stay broken.
//!
//! Neither claim is platform-gated. The list is deliberately unconditional:
//! `cfg(target_os)` would compile the third entry out on the very machine that
//! needs it (a Windows-only build), and the failure mode it is guarding against
//! is decided by which origin the browser sends, not by which OS compiled the
//! binary. Asserting the same three on every platform is what makes a
//! regression on macOS or Linux CI visible before it ships to Windows.

use cc_switch_lib::{allowed_origins, RemoteConfig, TAURI_WEBVIEW_ORIGINS};

/// The origins the embedded Tauri webview can present, across both schemes.
///
/// Written as an independent literal rather than derived from the constant under
/// test: a test that asserts `origins == origins` cannot fail.
const REQUIRED_WEBVIEW_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "https://tauri.localhost",
    "http://tauri.localhost",
];

/// The constant lists exactly the three webview origins, and nothing else.
#[test]
fn constant_lists_three_origins() {
    assert_eq!(
        TAURI_WEBVIEW_ORIGINS.len(),
        REQUIRED_WEBVIEW_ORIGINS.len(),
        "webview origin 常量应当恰好三条，实际 {:?}",
        TAURI_WEBVIEW_ORIGINS
    );

    for want in REQUIRED_WEBVIEW_ORIGINS {
        assert!(
            TAURI_WEBVIEW_ORIGINS.contains(&want),
            "webview origin 常量缺 {want:?}，实际 {:?}：Windows 默认 \
             useHttpsScheme=false，缺 http://tauri.localhost 就是桌面端自己的 \
             请求被判 403",
            TAURI_WEBVIEW_ORIGINS
        );
    }

    // The whitelist is not a place to widen. `*` is another spelling of "any
    // origin", and `null` belongs to no site at all (sandbox iframe / `file:` /
    // `data:` all send it) — both are checked by
    // `remote::tests::cors_origins_exclude_wildcard_and_null`, and the entries
    // added here must not become the loophole it watches for.
    for entry in TAURI_WEBVIEW_ORIGINS {
        assert!(!entry.contains('*'), "webview origin 不得含通配：{entry:?}");
        assert_ne!(entry, "null", "webview origin 不得是 null：{entry:?}");
        assert!(
            !entry.trim().is_empty(),
            "webview origin 不得是空串：{entry:?}"
        );
    }
}

/// The three declared origins survive into the whitelist the middleware judges
/// by, on the default configuration.
#[test]
fn tauri_origins_reach_allowed_origins() {
    let origins = allowed_origins(&RemoteConfig::default(), false);
    let listed: Vec<&str> = origins
        .iter()
        .map(|o| {
            o.to_str()
                .expect("allowed_origins 只收可见 ASCII 的 origin")
        })
        .collect();

    assert!(
        !listed.is_empty(),
        "白名单为空：内嵌 webview 的 origin 不在列就是 403"
    );

    for want in REQUIRED_WEBVIEW_ORIGINS {
        assert!(
            listed.contains(&want),
            "{want:?} 没进 allowed_origins，实际 {listed:?}：白名单与 webview \
             origin 常量分叉时，中间件第②层会 403，而常量侧看着还是绿的"
        );
    }
}
