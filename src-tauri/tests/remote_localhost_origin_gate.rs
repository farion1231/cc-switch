//! Executable gate for the **loopback origins** (`127.0.0.1` / `localhost` /
//! `[::1]`) that the remote server's Origin allow-list must carry.
//!
//! The failure this pins is a real one, not a hypothetical. The README tells
//! the user to open `http://localhost:4000`, and the **Host** allow-list has
//! accepted `localhost` / `[::1]` from the start — but the **Origin**
//! allow-list only ever carried `http://127.0.0.1:<port>`. The page therefore
//! loads (the Host gate lets it through) and then every `POST /api/switch` it
//! issues is turned down by the middleware's Origin check (layer ②) with a
//! 403 that the browser reports as a CORS failure. The two lists disagree
//! about what "this machine" means, and only one of them is consulted by the
//! browser's cross-origin check.
//!
//! The three loopback spellings are byte-different origins even though they
//! name the same machine: `http://127.0.0.1:4000`, `http://localhost:4000`
//! and `http://[::1]:4000` never compare equal to one another. The
//! middleware compares origins **byte for byte** against the list, so all
//! three have to be in it.
//!
//! Claims, in the order they fail:
//!
//! 1. `loopback_origins_include_localhost_and_ipv6` — the default
//!    configuration lists all three loopback origins.
//! 2. `tls_loopback_origins_use_https` — on a TLS listener all three switch
//!    to `https` together; the scheme follows the listener, not the entry.
//! 3. `loopback_origins_follow_the_configured_port` — the port comes from
//!    `RemoteConfig::port`, never from a literal.
//! 4. `loopback_origins_carry_no_wildcard` — the entries added here are not a
//!    loophole around the "no wildcard, no `Origin: null`" rule.
//!
//! The gate is falsifiable: the claims read the list that production
//! assembles, and a list that had lost the `localhost` / `[::1]` entries (the
//! state this gate was written for) turns claim 1 red.

use cc_switch_lib::{allowed_origins, RemoteConfig};

/// The three loopback host spellings, as they appear inside an origin.
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// Flatten the assembled allow-list into plain strings.
///
/// `HeaderValue::to_str` fails on non-visible-ASCII values; those cannot be
/// origins a browser would send, so they are dropped rather than asserted on.
fn listed_origins(cfg: &RemoteConfig, tls: bool) -> Vec<String> {
    allowed_origins(cfg, tls)
        .iter()
        .filter_map(|o| o.to_str().ok().map(str::to_string))
        .collect()
}

/// All three loopback origins are in the list for the default configuration.
#[test]
fn loopback_origins_include_localhost_and_ipv6() {
    let cfg = RemoteConfig {
        port: 4000,
        ..Default::default()
    };
    let listed = listed_origins(&cfg, false);

    assert!(
        !listed.is_empty(),
        "白名单为空：Origin 名录里一条都没有，中间件第②层会把每个请求都判成 403"
    );

    for host in LOOPBACK_HOSTS {
        let want = format!("http://{host}:{}", cfg.port);
        assert!(
            listed.contains(&want),
            "{want:?} 必须进 allowed_origins，实际 {listed:?}：README 让用户打开 \
             http://localhost:{port}，那一页发来的 Origin 就是 {want:?}，\
             少了它 POST /api/switch 会被中间件判成 403（CORS 失败）",
            port = cfg.port
        );
    }
}

/// On a TLS listener all three loopback origins use `https`.
///
/// The scheme follows the listener: a mixed list would leave one spelling of
/// "this machine" permanently unusable on the TLS listener.
#[test]
fn tls_loopback_origins_use_https() {
    let cfg = RemoteConfig {
        port: 4000,
        ..Default::default()
    };
    let listed = listed_origins(&cfg, true);

    for host in LOOPBACK_HOSTS {
        let want = format!("https://{host}:{}", cfg.port);
        assert!(
            listed.contains(&want),
            "{want:?} 必须进 allowed_origins（TLS listener），实际 {listed:?}"
        );
    }

    // The scheme switched rather than merely being added: the plain-http
    // spelling of the numeric loopback is gone. (`127.0.0.1` is never one of
    // the machine's service names, so this one is not machine dependent.)
    assert!(
        !listed.contains(&format!("http://127.0.0.1:{}", cfg.port)),
        "TLS listener 上不得同时留着 http://127.0.0.1:{port}：实际 {listed:?}",
        port = cfg.port
    );
}

/// The port comes from the configuration, not from a literal.
///
/// A hard-coded `:4000` would leave every other port (the user is free to
/// change it in the settings) with a list that names a port nothing listens
/// on.
#[test]
fn loopback_origins_follow_the_configured_port() {
    let cfg = RemoteConfig {
        port: 51234,
        ..Default::default()
    };
    let listed = listed_origins(&cfg, false);

    for host in LOOPBACK_HOSTS {
        let want = format!("http://{host}:51234");
        assert!(
            listed.contains(&want),
            "{want:?} 必须进 allowed_origins，实际 {listed:?}：端口取自 \
             RemoteConfig::port"
        );
        assert!(
            !listed.contains(&format!("http://{host}:4000")),
            "端口改成 51234 之后不得再放行 http://{host}:4000：实际 {listed:?}"
        );
    }
}

/// The entries added for the loopback spellings are not a wildcard.
#[test]
fn loopback_origins_carry_no_wildcard() {
    let cfg = RemoteConfig::default();
    let listed = listed_origins(&cfg, false);

    for entry in &listed {
        assert!(
            !entry.contains('*'),
            "白名单里不得有带通配的 origin：{entry:?}"
        );
        assert_ne!(entry.trim(), "*", "白名单里不得有通配 origin：{entry:?}");
        assert_ne!(
            entry.trim(),
            "null",
            "白名单里不得有 Origin: null：{entry:?}"
        );
        assert!(!entry.trim().is_empty(), "白名单里不得有空白 origin");
    }
}
