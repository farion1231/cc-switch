//! Cross-platform LAN IPv4 address discovery modeled on Magpie's `LANHosts()`.
//!
//! Skips loopback, link-local (`169.254.0.0/16`), unspecified/multicast/broadcast,
//! and proxy TUN / FakeIP ranges (`198.18.0.0/15`, used by Clash/Mihomo/Surge/sing-box).
//! Sorts RFC 1918 private addresses first, then CGNAT/Tailscale (`100.64.0.0/10`),
//! then other globally routable addresses.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Enumerate reachable IPv4 addresses on this machine, prioritized for LAN access.
pub fn lan_ipv4_hosts() -> Vec<String> {
    let mut ips = enumerate_interface_ipv4s();
    if ips.is_empty() {
        ips.extend(probe_outbound_ipv4s());
    }
    filter_and_sort_ipv4(ips)
}

/// Pure filtering and prioritization helper (unit-testable on all platforms).
pub fn filter_and_sort_ipv4(ips: impl IntoIterator<Item = Ipv4Addr>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut private_lan = Vec::new();
    let mut tailscale_cgnat = Vec::new();
    let mut other_global = Vec::new();

    for ip in ips {
        if !is_usable_lan_ipv4(ip) || !seen.insert(ip) {
            continue;
        }
        if ip.is_private() {
            private_lan.push(ip.to_string());
        } else if is_cgnat_or_tailscale(ip) {
            tailscale_cgnat.push(ip.to_string());
        } else {
            other_global.push(ip.to_string());
        }
    }

    private_lan.extend(tailscale_cgnat);
    private_lan.extend(other_global);
    private_lan
}

/// Returns true for IPv4 addresses other devices on the LAN/tailnet can actually dial.
pub fn is_usable_lan_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || is_fake_ip_benchmark(ip)
    {
        return false;
    }
    // 0.0.0.0/8 ("this network") and 240.0.0.0/4 (reserved) are not dialable.
    let first = ip.octets()[0];
    first != 0 && first < 240
}

/// `198.18.0.0/15` (RFC 2544 benchmarking) is used by Clash, Mihomo, sing-box,
/// Surge, and Stash for TUN interfaces and FakeIP resolvers.
pub fn is_fake_ip_benchmark(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 198 && (b == 18 || b == 19)
}

/// `100.64.0.0/10` (RFC 6598 CGNAT, used by Tailscale).
pub fn is_cgnat_or_tailscale(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

#[cfg(unix)]
fn enumerate_interface_ipv4s() -> Vec<Ipv4Addr> {
    let mut result = Vec::new();
    let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();

    // SAFETY: `getifaddrs` allocates a linked list of interface addresses and writes
    // the head pointer into `ifaddrs`. We always free it with `freeifaddrs` on success.
    if unsafe { libc::getifaddrs(&mut ifaddrs) } != 0 {
        return result;
    }

    let mut cursor = ifaddrs;
    while !cursor.is_null() {
        // SAFETY: `cursor` is a valid non-null node in the list returned by `getifaddrs`.
        let ifa = unsafe { &*cursor };
        let is_up = (ifa.ifa_flags as i32 & libc::IFF_UP) != 0;
        let is_loopback = (ifa.ifa_flags as i32 & libc::IFF_LOOPBACK) != 0;

        if is_up && !is_loopback && !ifa.ifa_addr.is_null() {
            // SAFETY: `ifa_addr` is non-null; check `sa_family` before casting to `sockaddr_in`.
            let family = unsafe { (*ifa.ifa_addr).sa_family } as i32;
            if family == libc::AF_INET {
                // SAFETY: `sa_family == AF_INET` guarantees `ifa_addr` points to a `sockaddr_in`.
                let addr_in = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(addr_in.sin_addr.s_addr));
                result.push(ip);
            }
        }
        cursor = ifa.ifa_next;
    }

    // SAFETY: `ifaddrs` was returned by a successful `getifaddrs` call.
    unsafe { libc::freeifaddrs(ifaddrs) };
    result
}

#[cfg(not(unix))]
fn enumerate_interface_ipv4s() -> Vec<Ipv4Addr> {
    probe_outbound_ipv4s()
}

/// Fallback route probe (sends zero packets on UDP connect; reads kernel-selected local IP).
fn probe_outbound_ipv4s() -> Vec<Ipv4Addr> {
    let targets = ["192.168.1.1:80", "10.255.255.254:80", "100.100.100.100:80"];
    let mut ips = Vec::new();
    for target in targets {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            if sock.connect(target).is_ok() {
                if let Ok(local) = sock.local_addr() {
                    if let IpAddr::V4(v4) = local.ip() {
                        ips.push(v4);
                    }
                }
            }
        }
    }
    ips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_loopback_link_local_fakeip_and_sorts_private_then_tailscale_then_public() {
        let input = vec![
            Ipv4Addr::new(127, 0, 0, 1),    // loopback -> drop
            Ipv4Addr::new(169, 254, 10, 2), // link-local -> drop
            Ipv4Addr::new(198, 18, 0, 1),   // Clash/Mihomo FakeIP TUN -> drop
            Ipv4Addr::new(198, 19, 255, 1), // FakeIP upper half -> drop
            Ipv4Addr::new(0, 0, 0, 0),      // unspecified -> drop
            Ipv4Addr::new(224, 0, 0, 1),    // multicast -> drop
            Ipv4Addr::new(203, 0, 113, 5),  // documentation -> drop
            Ipv4Addr::new(8, 8, 8, 8),      // public -> third
            Ipv4Addr::new(100, 101, 42, 7), // Tailscale CGNAT -> second
            Ipv4Addr::new(192, 168, 1, 25), // RFC 1918 -> first
            Ipv4Addr::new(10, 0, 0, 12),    // RFC 1918 -> first
            Ipv4Addr::new(192, 168, 1, 25), // duplicate -> deduplicated
            Ipv4Addr::new(172, 16, 5, 9),   // RFC 1918 -> first
        ];

        let sorted = filter_and_sort_ipv4(input);
        assert_eq!(
            sorted,
            vec![
                "192.168.1.25",
                "10.0.0.12",
                "172.16.5.9",
                "100.101.42.7",
                "8.8.8.8",
            ]
        );
    }
}
