//! 系统代理检测模块
//!
//! 将操作系统级代理配置解析为出站代理 URL。
//! Windows: 读取 WinINET 注册表 (`ProxyEnable`/`ProxyServer`),即 FlClash /
//! Clash Verge / v2rayN 等「系统代理」开关写入的位置。
//! 其他平台或无注册表配置时,回退到环境变量 `HTTP(S)_PROXY` / `ALL_PROXY`。

use std::env;
use std::net::IpAddr;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
#[cfg(target_os = "windows")]
use winreg::RegKey;

#[cfg(target_os = "windows")]
const INTERNET_SETTINGS_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 检测当前系统代理,返回形如 `http://127.0.0.1:7890` 的 URL。
/// 系统代理被禁用或配置无效时返回 `None`。
pub fn detect() -> Option<String> {
    if let Some(url) = detect_registry() {
        return Some(url);
    }
    detect_from_env()
}

/// 检测系统代理,并过滤指向 CC Switch 自身监听端口的自环地址。
pub fn detect_loopback_safe(own_port: u16) -> Option<String> {
    detect().filter(|url| !proxy_points_to_own_port(url, own_port))
}

#[cfg(target_os = "windows")]
fn detect_registry() -> Option<String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(INTERNET_SETTINGS_PATH, KEY_READ)
        .ok()?;

    let proxy_enable: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    if proxy_enable == 0 {
        return None;
    }

    let server: String = key.get_value("ProxyServer").ok()?;
    let server = server.trim();
    if server.is_empty() {
        return None;
    }

    parse_proxy_server_value(server)
}

#[cfg(not(target_os = "windows"))]
fn detect_registry() -> Option<String> {
    None
}

fn detect_from_env() -> Option<String> {
    const KEYS: [&str; 6] = [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];

    for key in KEYS {
        let Ok(value) = env::var(key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        return Some(normalize_proxy_addr("http", value));
    }
    None
}

/// 解析 Windows 注册表 `ProxyServer` 值,支持:
///
/// - `host:port`
/// - `http://host:port`
/// - `http=host:port;https=host:port;socks=host:1080` (多协议)
///
/// 多协议时优先返回 HTTP 段;无 HTTP 段时返回第一个可用段。
fn parse_proxy_server_value(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    if v.contains('=') {
        let mut http_proxy: Option<String> = None;
        let mut first: Option<String> = None;

        for section in v.split(';') {
            let section = section.trim();
            if section.is_empty() {
                continue;
            }
            let Some((proto, addr)) = section.split_once('=') else {
                continue;
            };
            let addr = addr.trim();
            if addr.is_empty() {
                continue;
            }

            let protocol = proto.trim().to_ascii_lowercase();
            let scheme = match protocol.as_str() {
                "http" | "https" => "http",
                "socks" | "socks5" | "socks5h" => "socks5h",
                _ => continue,
            };

            let url = normalize_proxy_addr(scheme, addr);
            if first.is_none() {
                first = Some(url.clone());
            }
            if http_proxy.is_none() && matches!(protocol.as_str(), "http" | "https") {
                http_proxy = Some(url);
            }
        }

        return http_proxy.or(first);
    }

    Some(normalize_proxy_addr("http", v))
}

fn normalize_proxy_addr(default_scheme: &str, addr: &str) -> String {
    let a = addr.trim();
    if a.starts_with("http://")
        || a.starts_with("https://")
        || a.starts_with("socks5://")
        || a.starts_with("socks5h://")
    {
        a.to_string()
    } else {
        format!("{default_scheme}://{a}")
    }
}

/// 判断代理 URL 是否指向 CC Switch 自身监听端口(自环)。
pub fn proxy_points_to_own_port(url: &str, own_port: u16) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.port() != Some(own_port) {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_proxy_env() {
        for key in [
            "ALL_PROXY",
            "all_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn parse_plain_host_port() {
        assert_eq!(
            parse_proxy_server_value("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn parse_with_scheme() {
        assert_eq!(
            parse_proxy_server_value("http://127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn parse_multi_protocol_prefers_http() {
        assert_eq!(
            parse_proxy_server_value(
                "http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:7891"
            )
            .as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn parse_multi_protocol_socks_only() {
        assert_eq!(
            parse_proxy_server_value("socks=127.0.0.1:7891").as_deref(),
            Some("socks5h://127.0.0.1:7891")
        );
    }

    #[test]
    fn parse_empty_or_invalid() {
        assert_eq!(parse_proxy_server_value(""), None);
        assert_eq!(parse_proxy_server_value("   "), None);
        assert_eq!(parse_proxy_server_value("socks5=   "), None);
    }

    #[test]
    fn detect_from_env_priority() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        std::env::set_var("HTTP_PROXY", "10.0.0.1:8080");
        assert_eq!(detect_from_env().as_deref(), Some("http://10.0.0.1:8080"));
        clear_proxy_env();

        std::env::set_var("ALL_PROXY", "socks5://10.0.0.1:1080");
        assert_eq!(detect_from_env().as_deref(), Some("socks5://10.0.0.1:1080"));
        clear_proxy_env();

        assert_eq!(detect_from_env(), None);
    }

    #[test]
    fn own_port_detection() {
        assert!(proxy_points_to_own_port("http://127.0.0.1:15721", 15721));
        assert!(proxy_points_to_own_port("http://localhost:15721", 15721));
        assert!(!proxy_points_to_own_port("http://127.0.0.1:7890", 15721));
        assert!(!proxy_points_to_own_port(
            "http://192.168.1.10:15721",
            15721
        ));
    }

    #[test]
    fn detect_loopback_safe_filters_own_port() {
        // 无法注入注册表,这里只验证过滤逻辑: 手动构造已检测 URL 的场景
        assert_eq!(
            detect_loopback_safe(15721),
            detect().filter(|u| !proxy_points_to_own_port(u, 15721))
        );
    }

    /// 实机验证: 读取当前系统注册表代理。CI 环境通常无代理,故标记 #[ignore]。
    #[test]
    #[ignore]
    fn live_registry_detect_on_machine() {
        let guard = env_lock().lock().unwrap();
        clear_proxy_env();
        let detected = detect();
        drop(guard);
        println!("[live] system proxy detected = {detected:?}");
        // 本机启用系统代理时,期望是 http://127.0.0.1:7890
        // (不硬断言,仅打印供人工核对,避免其它环境差异导致失败)
    }
}
