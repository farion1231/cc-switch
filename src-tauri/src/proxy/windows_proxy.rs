use std::net::{IpAddr, Ipv6Addr};

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowsSystemProxyConfig {
    http_proxy: Option<String>,
    https_proxy: Option<String>,
    socks_proxy: Option<String>,
    fallback_proxy: Option<String>,
    bypass: ProxyBypassMatcher,
}

impl WindowsSystemProxyConfig {
    pub(crate) fn parse(proxy_server: &str, proxy_override: &str) -> Result<Option<Self>, String> {
        let trimmed = proxy_server.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let mut config = if trimmed.contains('=') {
            let mut http_proxy = None;
            let mut https_proxy = None;
            let mut socks_proxy = None;
            let mut fallback_proxy = None;

            for raw_entry in trimmed.split(';') {
                let entry = raw_entry.trim();
                if entry.is_empty() {
                    continue;
                }

                let (kind, value) = match entry.split_once('=') {
                    Some((kind, value)) => (kind.trim().to_ascii_lowercase(), value.trim()),
                    None => {
                        fallback_proxy = Some(normalize_proxy_url(entry, ProxyKind::Http)?);
                        continue;
                    }
                };

                if value.is_empty() {
                    continue;
                }

                let slot = match kind.as_str() {
                    "http" => &mut http_proxy,
                    "https" => &mut https_proxy,
                    "socks" | "socks5" | "socks5h" => &mut socks_proxy,
                    "all" => &mut fallback_proxy,
                    _ => continue,
                };

                *slot = Some(normalize_proxy_url(value, ProxyKind::from_label(&kind))?);
            }

            Self {
                http_proxy,
                https_proxy,
                socks_proxy,
                fallback_proxy,
                bypass: ProxyBypassMatcher::from_windows_override(proxy_override),
            }
        } else {
            Self {
                http_proxy: None,
                https_proxy: None,
                socks_proxy: None,
                fallback_proxy: Some(normalize_proxy_url(trimmed, ProxyKind::Http)?),
                bypass: ProxyBypassMatcher::from_windows_override(proxy_override),
            }
        };

        config
            .bypass
            .extend(ProxyBypassMatcher::from_no_proxy_env());

        if !config.has_proxy() {
            return Ok(None);
        }

        Ok(Some(config))
    }

    pub(crate) fn has_proxy(&self) -> bool {
        self.http_proxy.is_some()
            || self.https_proxy.is_some()
            || self.socks_proxy.is_some()
            || self.fallback_proxy.is_some()
    }

    pub(crate) fn http_proxy(&self) -> Option<&str> {
        self.http_proxy
            .as_deref()
            .or(self.fallback_proxy.as_deref())
            .or(self.socks_proxy.as_deref())
    }

    pub(crate) fn https_proxy(&self) -> Option<&str> {
        self.https_proxy
            .as_deref()
            .or(self.fallback_proxy.as_deref())
            .or(self.socks_proxy.as_deref())
    }

    pub(crate) fn bypass(&self) -> &ProxyBypassMatcher {
        &self.bypass
    }

    pub(crate) fn bypass_rule_count(&self) -> usize {
        self.bypass.rule_count()
    }

    pub(crate) fn bypasses_local(&self) -> bool {
        self.bypass.bypasses_local()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyBypassMatcher {
    rules: Vec<ProxyBypassRule>,
}

impl Default for ProxyBypassMatcher {
    fn default() -> Self {
        Self { rules: Vec::new() }
    }
}

impl ProxyBypassMatcher {
    pub(crate) fn from_windows_override(raw: &str) -> Self {
        let mut matcher = Self::default();
        for entry in split_bypass_entries(raw) {
            matcher.push_windows_rule(entry);
        }
        matcher
    }

    pub(crate) fn from_no_proxy_env() -> Self {
        let raw = std::env::var("NO_PROXY")
            .or_else(|_| std::env::var("no_proxy"))
            .unwrap_or_default();
        let mut matcher = Self::default();
        for entry in split_bypass_entries(&raw) {
            matcher.push_no_proxy_rule(entry);
        }
        matcher
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.rules.extend(other.rules);
    }

    pub(crate) fn matches_url(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(host) => host,
            None => return false,
        };
        self.matches_host(host)
    }

    pub(crate) fn matches_host(&self, host: &str) -> bool {
        self.rules.iter().any(|rule| rule.matches(host))
    }

    pub(crate) fn bypasses_local(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| matches!(rule, ProxyBypassRule::Local))
    }

    pub(crate) fn rule_count(&self) -> usize {
        self.rules.len()
    }

    fn push_windows_rule(&mut self, entry: &str) {
        let lower = entry.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return;
        }
        if lower == "<local>" {
            self.rules.push(ProxyBypassRule::Local);
            return;
        }
        if lower == "*" || lower.contains('*') {
            self.rules.push(ProxyBypassRule::Wildcard(lower));
            return;
        }
        self.rules.push(ProxyBypassRule::Exact(lower));
    }

    fn push_no_proxy_rule(&mut self, entry: &str) {
        let lower = entry.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return;
        }
        if lower == "*" {
            self.rules.push(ProxyBypassRule::Wildcard(lower));
            return;
        }
        if let Some(rule) = parse_cidr_rule(&lower) {
            self.rules.push(rule);
            return;
        }
        if lower.contains('*') {
            self.rules.push(ProxyBypassRule::Wildcard(lower));
            return;
        }
        if lower.parse::<IpAddr>().is_ok() {
            self.rules.push(ProxyBypassRule::Exact(lower));
            return;
        }
        self.rules.push(ProxyBypassRule::Domain(
            lower.trim_start_matches('.').to_string(),
        ));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProxyBypassRule {
    Local,
    Exact(String),
    Domain(String),
    Wildcard(String),
    Cidr(IpAddr, u8),
}

impl ProxyBypassRule {
    fn matches(&self, host: &str) -> bool {
        let lower = host
            .trim()
            .trim_matches('[')
            .trim_matches(']')
            .to_ascii_lowercase();
        match self {
            Self::Local => is_local_intranet_host(&lower),
            Self::Exact(expected) => lower == *expected,
            Self::Domain(suffix) => domain_matches(&lower, suffix),
            Self::Wildcard(pattern) => wildcard_matches(&lower, pattern),
            Self::Cidr(network, prefix) => ip_matches_cidr(&lower, network, *prefix),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyKind {
    Http,
    Https,
    Socks5,
}

impl ProxyKind {
    fn from_label(label: &str) -> Self {
        match label {
            "https" => Self::Https,
            "socks" | "socks5" | "socks5h" => Self::Socks5,
            _ => Self::Http,
        }
    }

    fn scheme(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5 => "socks5",
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn load_manual_system_proxy_config() -> Result<Option<WindowsSystemProxyConfig>, String>
{
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .map_err(|error| format!("failed to open Internet Settings registry key: {error}"))?;

    let enabled = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 0;
    if !enabled {
        return Ok(None);
    }

    let proxy_server = key
        .get_value::<String, _>("ProxyServer")
        .map_err(|error| format!("failed to read ProxyServer from registry: {error}"))?;
    let proxy_override = key
        .get_value::<String, _>("ProxyOverride")
        .unwrap_or_default();

    WindowsSystemProxyConfig::parse(&proxy_server, &proxy_override)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn load_manual_system_proxy_config() -> Result<Option<WindowsSystemProxyConfig>, String>
{
    Ok(None)
}

fn normalize_proxy_url(raw: &str, kind: ProxyKind) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("proxy URL is empty".to_string());
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("{}://{trimmed}", kind.scheme())
    };

    let parsed = Url::parse(&candidate)
        .map_err(|error| format!("invalid system proxy URL '{trimmed}': {error}"))?;
    let scheme = parsed.scheme();
    if !matches!(scheme, "http" | "https" | "socks5" | "socks5h") {
        return Err(format!(
            "unsupported system proxy scheme '{scheme}' in '{trimmed}'"
        ));
    }
    if parsed.host_str().is_none() {
        return Err(format!("system proxy URL '{trimmed}' is missing a host"));
    }

    Ok(parsed.to_string())
}

fn split_bypass_entries(raw: &str) -> impl Iterator<Item = &str> {
    raw.split([';', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

fn parse_cidr_rule(raw: &str) -> Option<ProxyBypassRule> {
    let (addr, prefix) = raw.split_once('/')?;
    let network = addr.parse::<IpAddr>().ok()?;
    let prefix = prefix.parse::<u8>().ok()?;
    let max = match network {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    (prefix <= max).then_some(ProxyBypassRule::Cidr(network, prefix))
}

fn is_local_intranet_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
            IpAddr::V6(ip) => {
                ip.is_loopback() || ip.is_unicast_link_local() || is_unique_local_ipv6(ip)
            }
        };
    }

    !host.contains('.')
}

fn is_unique_local_ipv6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn domain_matches(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn wildcard_matches(host: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let host = host.as_bytes();
    let pattern = pattern.as_bytes();
    let (mut hi, mut pi) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_match = 0usize;

    while hi < host.len() {
        if pi < pattern.len() && pattern[pi] == host[hi] {
            hi += 1;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star = Some(pi);
            pi += 1;
            star_match = hi;
        } else if let Some(star_idx) = star {
            pi = star_idx + 1;
            star_match += 1;
            hi = star_match;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

fn ip_matches_cidr(host: &str, network: &IpAddr, prefix: u8) -> bool {
    let ip = match host.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => return false,
    };

    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(network)) => {
            let ip = u32::from(ip);
            let network = u32::from(*network);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            (ip & mask) == (network & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(network)) => {
            let ip = u128::from_be_bytes(ip.octets());
            let network = u128::from_be_bytes(network.octets());
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            (ip & mask) == (network & mask)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_override_matches_private_ipv4_regression() {
        let matcher = ProxyBypassMatcher::from_windows_override(
            "localhost;127.*;10.*;172.16.*;192.168.*;<local>",
        );

        assert!(matcher.matches_host("192.168.137.163"));
        assert!(matcher.matches_host("printer"));
        assert!(matcher.matches_host("127.0.0.1"));
        assert!(!matcher.matches_host("203.0.113.9"));
    }

    #[test]
    fn no_proxy_rules_match_domains_and_cidrs() {
        let mut matcher = ProxyBypassMatcher::default();
        matcher.push_no_proxy_rule(".example.com");
        matcher.push_no_proxy_rule("192.168.0.0/16");

        assert!(matcher.matches_host("api.example.com"));
        assert!(matcher.matches_host("example.com"));
        assert!(matcher.matches_host("192.168.137.163"));
        assert!(!matcher.matches_host("198.51.100.4"));
    }

    #[test]
    fn parse_windows_proxy_server_supports_per_scheme_and_bypass_rules() {
        let config = WindowsSystemProxyConfig::parse(
            "http=proxy.local:8080;https=secure.proxy.local:8443;socks=127.0.0.1:1080",
            "*.corp.local;192.168.*;<local>",
        )
        .expect("parse")
        .expect("config");

        assert_eq!(config.http_proxy(), Some("http://proxy.local:8080/"));
        assert_eq!(
            config.https_proxy(),
            Some("https://secure.proxy.local:8443/")
        );
        assert!(config.bypass().matches_host("service.corp.local"));
        assert!(config.bypass().matches_host("192.168.137.163"));
        assert!(config.bypasses_local());
    }

    #[test]
    fn parse_windows_proxy_server_uses_fallback_for_all_protocols() {
        let config = WindowsSystemProxyConfig::parse("172.171.16.221:7890", "")
            .expect("parse")
            .expect("config");

        assert_eq!(config.http_proxy(), Some("http://172.171.16.221:7890/"));
        assert_eq!(config.https_proxy(), Some("http://172.171.16.221:7890/"));
        assert_eq!(config.bypass_rule_count(), 0);
    }

    #[test]
    fn local_rule_treats_private_addresses_as_intranet() {
        let matcher = ProxyBypassMatcher::from_windows_override("<local>");

        assert!(matcher.matches_host("192.168.137.163"));
        assert!(matcher.matches_host("10.0.0.4"));
        assert!(matcher.matches_host("localhost"));
        assert!(matcher.matches_host("fileserver"));
        assert!(!matcher.matches_host("api.example.com"));
    }
}
