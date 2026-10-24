//! Executable gate for the **Host gate / credential ordering** in
//! `remote::auth::middleware::evaluate`.
//!
//! The regression this pins is a self-inflicted one. The Host gate was placed
//! *before* the decision table and applied to every request, because the cell a
//! DNS-rebinding attacker wants to borrow is "loopback + no `Origin` + no
//! credential". But that placement also 403s a client that arrives over a
//! tailnet by its MagicDNS name or a `.local` address: its `Host` is not one of
//! the authorities this listener answers to, and it ends up locked out of its
//! own peer — while holding a perfectly valid Bearer token. A request that
//! already carries a credential is not in the cell being defended against, so
//! the Host gate must not apply to it.
//!
//! Two claims:
//!
//! 1. `host_check_only_applies_to_anonymous` — in production source, the
//!    credential is resolved *before* the `Host` lookup, and that lookup sits
//!    inside a `credential.is_none()` guard. Reading it as a shape rather than
//!    as behaviour is deliberate: the two are one line apart, and the shape is
//!    what the next refactor would silently undo.
//! 2. `loopback_hosts_still_allowed` — the credentialed bypass did not hollow
//!    out the allow-list. The three loopback authorities are still in it, so the
//!    anonymous request that legitimately arrives at `127.0.0.1` is unaffected.
//!
//! The gate is falsifiable: claim 1 runs the same detector over a copy of the
//! production source with the guard deleted, and over a body with both markers
//! removed. A detector that could not go red would make the green run worth
//! nothing.
//!
//! Source paths are anchored with `CARGO_MANIFEST_DIR`, never cwd-relative: the
//! cwd of a test binary depends on the harness that launched it.
//!
//! The behavioural half — valid Bearer + `Host: evil.com` → 200, anonymous +
//! `Host: evil.com` → 403 — cannot live here: `register(...)` and
//! `auth_layer(...)` are private to `remote`, and `mod remote` is private in
//! `lib.rs`, so an integration-test crate cannot build a router carrying the
//! auth layer. Those cases are `remote::auth::middleware::tests`'
//! `credentialed_host_bypass` and `anonymous_foreign_host_is_forbidden`.

use std::path::{Path, PathBuf};

use cc_switch_lib::{allowed_hosts, BindScope, CorsMode, RemoteConfig};

/// `middleware.rs` 的路径，按 `CARGO_MANIFEST_DIR` 锚定。
fn middleware_source() -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("remote")
        .join("auth")
        .join("middleware.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

/// 剥掉整行的行尾注释（`//` / `///`），让断言读的是代码而不是说明。
///
/// 这份 gate 要在注释里点名 `credential_of` 与 `hosts.allows` 的先后 ——
/// 不剥注释的话，注释本身就会把 detector 喂绿。
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

/// `fn evaluate(...)` 的函数体（到下一个顶格 `fn` 为止）。
fn evaluate_body(src: &str) -> String {
    let stripped = strip_line_comments(src);
    let start = stripped
        .find("fn evaluate(")
        .expect("middleware.rs 里必须有 fn evaluate");
    let rest = &stripped[start..];
    let end = rest[1..]
        .find("\nfn ")
        .map(|rel| rel + 1)
        .expect("fn evaluate 之后必须还有别的顶格函数");
    rest[..end].to_string()
}

/// 生产代码的形状判定。`Err` 就是被 gate 拒掉的那一种形状。
///
/// 两条独立的要求，少一条都不够：
///
/// - **顺序**：`credential_of(req` 在 `hosts.allows(` 之前 —— Host 闸门排在
///   取凭据前面的话，带凭据的请求照样 403（就是本文件要钉的那个回归）。
/// - **守卫**：`hosts.allows(` 所在的那条 `if` 必须带 `credential.is_none()`
///   —— 只把顺序调过来而忘了守卫，所有请求仍会被 Host 拦下，只是换个理由。
fn evaluate_shape(body: &str) -> Result<(), &'static str> {
    let credential = body
        .find("credential_of(req")
        .ok_or("evaluate 里没有 credential_of(req, …) 调用")?;
    let hosts = body
        .find("hosts.allows(")
        .ok_or("evaluate 里没有 hosts.allows( 调用")?;
    if credential > hosts {
        return Err("credential_of 排在 hosts.allows 之后：带凭据的请求会先被 Host 403");
    }
    let guard_start = body[..hosts]
        .rfind("if ")
        .ok_or("hosts.allows 不在任何一个 if 里")?;
    if !body[guard_start..hosts].contains("credential.is_none()") {
        return Err("hosts.allows 不在 credential.is_none() 守卫内");
    }
    Ok(())
}

/// Host 闸门只兜无凭据那一格。
#[test]
fn host_check_only_applies_to_anonymous() {
    let real = middleware_source();
    assert!(
        evaluate_shape(&evaluate_body(&real)).is_ok(),
        "生产代码里 credential_of 必须先于 hosts.allows，且后者在 \
         credential.is_none() 守卫内；实际 evaluate 形状不合格"
    );

    // 负控一：守卫被删掉（只剩调用顺序）。这是最省事也最危险的那种改动 ——
    // 测试读起来没变，行为却退回「Host 闸门对所有请求生效」。
    let guardless = real.replace("credential.is_none() && ", "");
    assert_ne!(
        guardless, real,
        "负控失效：源码里找不到 `credential.is_none() && ` 这段守卫文本，\
         detector 会对着同一份内容跑两遍，两次都绿"
    );
    assert!(
        evaluate_shape(&evaluate_body(&guardless)).is_err(),
        "负控必须能红：去掉 credential.is_none() 守卫后这个 detector 仍然判绿，\
         说明它检不出真正承重的那一半"
    );

    // 负控二：两个标记都不在。空壳子判绿 = detector 什么都没看。
    assert!(
        evaluate_shape("fn evaluate() { let _ = 1; }").is_err(),
        "负控必须能红：一个既不取凭据也不查 Host 的函数体判绿，\
         说明 detector 对「找不到标记」是放行的"
    );
}

/// 让出 Host 闸门给带凭据的请求，没有顺手把回环 authority 掏空。
#[test]
fn loopback_hosts_still_allowed() {
    let cfg = RemoteConfig {
        port: 4000,
        bind_scope: BindScope::Loopback,
        cors_mode: CorsMode::SameOrigin,
        ..Default::default()
    };
    let hosts = allowed_hosts(&cfg);

    for authority in [
        format!("127.0.0.1:{}", cfg.port),
        format!("localhost:{}", cfg.port),
        format!("[::1]:{}", cfg.port),
    ] {
        assert!(
            hosts.contains(&authority),
            "{authority:?} 必须仍在 allowed_hosts 里，实际 {hosts:?}：本机的匿名请求 \
             （内嵌 Web UI、配对之前）正是走这三个 authority，掏空它们等于把 \
             桌面端自己的页面锁在外面"
        );
    }
}
