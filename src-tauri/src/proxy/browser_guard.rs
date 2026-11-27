//! Reject requests that a web page made, so an open browser tab cannot spend the
//! user's credentials through the local proxy.
//!
//! The proxy swaps whatever credentials a request carries for the stored provider's,
//! so anything that can reach the port gets to use them. CLI clients never send
//! `Origin`, while browsers attach it to every POST (including `no-cors` ones that
//! skip preflight) and to every WebSocket handshake, so rejecting it stops a page from
//! calling the proxy cross-site. A DNS-rebinding page is same-origin and can read
//! GET responses, but it has to reach us under its own domain name, so on a loopback
//! listener — or for any loopback / same-host peer reaching a wildcard listener —
//! only hosts no public DNS can point at are accepted.
//!
//! Only headers are read: the request itself passes through untouched.

use super::server::ProxyState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};

/// Per-connection endpoint addresses attached by the accept loop so `browser_guard`
/// can still identify loopback and same-host peers when listening on `0.0.0.0` / `::`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ConnectionAddrs {
    pub remote: SocketAddr,
    pub local: Option<SocketAddr>,
}

/// Claude Desktop's namespace already requires its gateway token, which a page
/// cannot know, so the desktop app is not held to the header rules.
const TOKEN_PROTECTED_PREFIX: &str = "/claude-desktop/";

pub(super) async fn reject_browser_requests(
    State(state): State<ProxyState>,
    request: Request,
    next: Next,
) -> Response {
    let listen_address = state.config.read().await.listen_address.clone();
    let client_host = crate::settings::get_proxy_client_host();
    let conn_addrs = request.extensions().get::<ConnectionAddrs>().copied();
    match check_request(
        request.uri().path(),
        request.headers(),
        &listen_address,
        conn_addrs,
        client_host.as_deref(),
    ) {
        Ok(()) => next.run(request).await,
        Err(reason) => {
            log::warn!(
                "[ProxyServer] rejected {} {}: {reason}",
                request.method(),
                request.uri().path()
            );
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": {
                        "type": "forbidden",
                        "message": format!("CC Switch local proxy: {reason}"),
                    }
                })),
            )
                .into_response()
        }
    }
}

fn check_request(
    path: &str,
    headers: &HeaderMap,
    listen_address: &str,
    conn_addrs: Option<ConnectionAddrs>,
    client_host: Option<&str>,
) -> Result<(), &'static str> {
    if !path.starts_with(TOKEN_PROTECTED_PREFIX) && headers.contains_key(header::ORIGIN) {
        return Err("requests from web pages are not accepted");
    }
    if is_loopback_listener(listen_address) || is_same_host_peer(conn_addrs) {
        if let Some(host) = headers.get(header::HOST) {
            let host = host.to_str().ok().and_then(host_name);
            if !host.is_some_and(|host| is_local_host(&host, client_host)) {
                return Err("unexpected Host header");
            }
        }
    }
    Ok(())
}

/// Even when the proxy binds `0.0.0.0` or `::` for WSL/Docker/LAN clients, a browser
/// tab on the same machine reaches it via loopback or the host's own interface IP
/// (`remote_ip == local_ip`), so DNS-rebinding Host checks must still apply.
fn is_same_host_peer(addrs: Option<ConnectionAddrs>) -> bool {
    let Some(addrs) = addrs else {
        return false;
    };
    let remote_ip = addrs.remote.ip().to_canonical();
    if remote_ip.is_loopback() {
        return true;
    }
    addrs
        .local
        .is_some_and(|local| remote_ip == local.ip().to_canonical())
}

/// A listener on a LAN address is reached by remote peers under whatever names that
/// network uses, so the Host rule is relaxed for remote peers while still enforced
/// for loopback listeners and same-host peers via `is_same_host_peer`.
fn is_loopback_listener(listen_address: &str) -> bool {
    let address = listen_address.trim().trim_matches(['[', ']']);
    address.eq_ignore_ascii_case("localhost")
        // `to_canonical`：`::ffff:127.0.0.1` 也绑在回环上，但 IPv6 的 is_loopback 不认它
        || address
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.to_canonical().is_loopback())
}

/// The host part of a Host header, lowercased, without port, brackets or trailing dot.
fn host_name(value: &str) -> Option<String> {
    let authority: http::uri::Authority = value.trim().parse().ok()?;
    let host = authority
        .host()
        .trim_matches(['[', ']'])
        .trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Names a rebinding page cannot use: IP literals (rebinding needs a domain),
/// `localhost` and the reserved private-use `.localhost` / `.local` / `.internal`
/// (the last covers `host.docker.internal`), plus the client address the user set.
fn is_local_host(host: &str, client_host: Option<&str>) -> bool {
    host.parse::<IpAddr>().is_ok()
        || host == "localhost"
        || [".localhost", ".local", ".internal"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
        || client_host.is_some_and(|client| {
            client
                .trim()
                .trim_matches(['[', ']'])
                .trim_end_matches('.')
                .eq_ignore_ascii_case(host)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn cli_requests_pass() {
        for host in ["127.0.0.1:15721", "localhost:15721", "[::1]:15721"] {
            let h = headers(&[("host", host)]);
            assert_eq!(
                check_request("/v1/messages", &h, "127.0.0.1", None, None),
                Ok(())
            );
        }
        // No Host at all (HTTP/1.0 clients) is not a browser.
        assert_eq!(
            check_request("/v1/messages", &HeaderMap::new(), "127.0.0.1", None, None),
            Ok(())
        );
    }

    #[test]
    fn any_origin_is_rejected_outside_claude_desktop() {
        for origin in ["https://evil.example", "null", "tauri://localhost"] {
            let h = headers(&[("host", "127.0.0.1:15721"), ("origin", origin)]);
            for path in ["/v1/messages", "/v1/responses", "/v1beta/models", "/status"] {
                assert!(check_request(path, &h, "127.0.0.1", None, None).is_err());
            }
            // LAN listeners are covered too: the Origin rule does not depend on Host.
            assert!(check_request("/v1/messages", &h, "0.0.0.0", None, None).is_err());
        }
    }

    #[test]
    fn claude_desktop_keeps_its_token_check_only() {
        let h = headers(&[("host", "127.0.0.1:15721"), ("origin", "app://claude")]);
        assert_eq!(
            check_request("/claude-desktop/v1/messages", &h, "127.0.0.1", None, None),
            Ok(())
        );
        // Rebinding still applies there: the Host rule is not exempted.
        let h = headers(&[("host", "rebind.evil.example:15721")]);
        assert!(check_request("/claude-desktop/v1/models", &h, "127.0.0.1", None, None).is_err());
    }

    #[test]
    fn rebinding_hosts_are_rejected_on_loopback_listener() {
        for host in [
            "rebind.evil.example:15721",
            "evil.example.",
            "localhost.evil.example",
        ] {
            let h = headers(&[("host", host)]);
            assert!(
                check_request("/status", &h, "127.0.0.1", None, None).is_err(),
                "{host}"
            );
            assert!(
                check_request("/status", &h, "::1", None, None).is_err(),
                "{host}"
            );
            assert!(
                check_request("/status", &h, "[::ffff:127.0.0.1]", None, None).is_err(),
                "{host}"
            );
        }
    }

    #[test]
    fn local_names_and_configured_client_host_pass() {
        for host in [
            "host.docker.internal:15721",
            "my-mac.local:15721",
            "app.localhost:15721",
            "LOCALHOST.:15721",
            "172.25.144.1:15721",
        ] {
            let h = headers(&[("host", host)]);
            assert_eq!(
                check_request("/v1/messages", &h, "127.0.0.1", None, None),
                Ok(()),
                "{host}"
            );
        }
        let h = headers(&[("host", "Devbox.Example.com:15721")]);
        assert_eq!(
            check_request(
                "/v1/messages",
                &h,
                "127.0.0.1",
                None,
                Some(" devbox.example.com ")
            ),
            Ok(())
        );
    }

    #[test]
    fn lan_listener_accepts_remote_peer_host_names_but_rejects_same_host_rebinding() {
        let h = headers(&[("host", "devbox.corp.example:15721")]);
        let remote_lan = Some(ConnectionAddrs {
            remote: "192.168.1.42:54321".parse().unwrap(),
            local: Some("192.168.1.5:15721".parse().unwrap()),
        });
        assert_eq!(
            check_request("/v1/messages", &h, "0.0.0.0", remote_lan, None),
            Ok(())
        );
        assert_eq!(
            check_request("/v1/messages", &h, "192.168.1.5", remote_lan, None),
            Ok(())
        );

        let rebind = headers(&[("host", "rebind.evil.example:15721")]);
        for same_host in [
            ConnectionAddrs {
                remote: "127.0.0.1:54321".parse().unwrap(),
                local: Some("127.0.0.1:15721".parse().unwrap()),
            },
            ConnectionAddrs {
                remote: "[::1]:54321".parse().unwrap(),
                local: Some("[::1]:15721".parse().unwrap()),
            },
            ConnectionAddrs {
                remote: "[::ffff:127.0.0.1]:54321".parse().unwrap(),
                local: Some("[::]:15721".parse().unwrap()),
            },
            ConnectionAddrs {
                remote: "192.168.1.5:54321".parse().unwrap(),
                local: Some("192.168.1.5:15721".parse().unwrap()),
            },
            ConnectionAddrs {
                remote: "[::ffff:192.168.1.5]:54321".parse().unwrap(),
                local: Some("[::ffff:192.168.1.5]:15721".parse().unwrap()),
            },
        ] {
            assert!(
                check_request("/status", &rebind, "0.0.0.0", Some(same_host), None).is_err(),
                "0.0.0.0 listener must reject rebinding Host from same-host peer {same_host:?}"
            );
            assert!(
                check_request("/status", &rebind, "::", Some(same_host), None).is_err(),
                ":: listener must reject rebinding Host from same-host peer {same_host:?}"
            );
        }

        let mapped_remote_lan = Some(ConnectionAddrs {
            remote: "[::ffff:192.168.1.42]:54321".parse().unwrap(),
            local: Some("[::ffff:192.168.1.5]:15721".parse().unwrap()),
        });
        assert_eq!(
            check_request("/v1/messages", &h, "::", mapped_remote_lan, None),
            Ok(())
        );

        let same_host_lan = Some(ConnectionAddrs {
            remote: "192.168.1.5:54321".parse().unwrap(),
            local: Some("192.168.1.5:15721".parse().unwrap()),
        });
        assert!(
            check_request("/v1/messages", &h, "0.0.0.0", same_host_lan, None).is_err(),
            "same-host peer using custom hostname on 0.0.0.0 requires proxy_client_host"
        );
        assert_eq!(
            check_request(
                "/v1/messages",
                &h,
                "0.0.0.0",
                same_host_lan,
                Some("devbox.corp.example"),
            ),
            Ok(())
        );
    }
}
