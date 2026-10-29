//! Reject provider URLs that would send a request back to this proxy listener.

use super::ProxyError;
use std::net::{IpAddr, UdpSocket};

pub(super) async fn reject_self_forwarding(
    target: &str,
    listen_address: &str,
    listen_port: u16,
) -> Result<(), ProxyError> {
    let Ok(url) = url::Url::parse(target) else {
        return Ok(());
    };
    if listen_port == 0 || url.port_or_known_default() != Some(listen_port) {
        return Ok(());
    }
    let Ok(listener) = listen_address.trim_matches(['[', ']']).parse::<IpAddr>() else {
        return Ok(());
    };
    let targets_listener = match url.host() {
        Some(url::Host::Ipv4(ip)) => is_listener_address(ip.into(), listener),
        Some(url::Host::Ipv6(ip)) => is_listener_address(ip.into(), listener),
        Some(url::Host::Domain(host)) => {
            if super::http_client::is_loopback_url(target) {
                is_listener_address(std::net::Ipv4Addr::LOCALHOST.into(), listener)
                    || is_listener_address(std::net::Ipv6Addr::LOCALHOST.into(), listener)
            } else {
                // Resolve aliases only when the port matches our listener. Ordinary
                // upstream requests keep their existing DNS/proxy behavior.
                tokio::net::lookup_host((host, listen_port))
                    .await
                    .is_ok_and(|mut addresses| {
                        addresses.any(|address| is_listener_address(address.ip(), listener))
                    })
            }
        }
        None => false,
    };
    if targets_listener {
        // Do not include the URL: provider paths and queries can contain API keys.
        return Err(ProxyError::ConfigError(
            "Upstream URL points to CC Switch's own proxy listener. Set the provider base URL to the real upstream service.".to_string(),
        ));
    }
    Ok(())
}

fn is_listener_address(target: IpAddr, listener: IpAddr) -> bool {
    fn normalize(ip: IpAddr) -> IpAddr {
        match ip {
            IpAddr::V6(ip) => ip.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip.into()),
            ip => ip,
        }
    }
    let target = normalize(target);
    let listener = normalize(listener);
    if listener.is_ipv4() && target.is_ipv6()
        || listener.is_ipv6() && target.is_ipv4() && !listener.is_unspecified()
    {
        return false;
    }
    if target == listener {
        return true;
    }
    if listener.is_unspecified() {
        // A wildcard listener also accepts connections to local interface IPs.
        // Binding an ephemeral UDP socket checks address ownership without sending
        // traffic or adding a dependency for enumerating network interfaces.
        return target.is_loopback()
            || target.is_unspecified()
            || UdpSocket::bind((target, 0)).is_ok();
    }
    target.is_unspecified() && listener.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_own_listener_including_custom_ports_and_url_aliases() {
        for (target, address, port) in [
            ("http://127.0.0.1:3688/v1/responses", "127.0.0.1", 3688),
            ("http://localhost:15721/v1", "127.0.0.1", 15721),
            ("http://LOCALHOST.:15721/v1", "127.0.0.1", 15721),
            ("http://proxy.localhost:15721/v1", "127.0.0.1", 15721),
            ("http://127.1:15721/v1", "127.0.0.1", 15721),
            ("http://[::ffff:127.0.0.1]:15721/v1", "127.0.0.1", 15721),
            ("http://[::1]:15721/v1", "::1", 15721),
            ("http://localhost:15721/v1", "::1", 15721),
            ("http://127.0.0.1:15721/v1", "0.0.0.0", 15721),
            ("http://[::1]:15721/v1", "::", 15721),
            ("http://192.0.2.10:15721/v1", "192.0.2.10", 15721),
            ("http://0.0.0.0:15721/v1", "127.0.0.1", 15721),
            ("http://127.0.0.1/v1", "127.0.0.1", 80),
        ] {
            assert!(
                matches!(
                    reject_self_forwarding(target, address, port).await,
                    Err(ProxyError::ConfigError(_))
                ),
                "{target} via {address}:{port}",
            );
        }
    }

    #[tokio::test]
    async fn allows_other_services_and_does_not_disclose_upstream_secrets() {
        for target in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "http://[::1]:11434/v1",
            "http://[::1]:15721/v1",
            "http://192.0.2.10:15721/v1",
            "http://127.0.0.2:15721/v1",
            "https://api.example.com/v1",
        ] {
            assert!(
                reject_self_forwarding(target, "127.0.0.1", 15721)
                    .await
                    .is_ok(),
                "{target}"
            );
        }
        assert!(
            reject_self_forwarding("http://localhost:15721/v1", "127.0.0.1", 0)
                .await
                .is_ok()
        );
        let error = reject_self_forwarding(
            "http://user:secret@127.0.0.1:15721/private-key/v1?key=private-token",
            "127.0.0.1",
            15721,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("own proxy listener"));
        for secret in ["secret", "private-key", "private-token"] {
            assert!(!error.contains(secret));
        }
    }
}
