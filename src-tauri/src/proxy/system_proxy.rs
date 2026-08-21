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

/// 按协议区分（scheme）的系统代理映射。
///
/// - `http`: 用于 `http://` 上游
/// - `https`: 用于 `https://` 上游（CONNECT 隧道）
/// - `socks`: 兜底（上游 scheme 非 http/https 时）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxySchemeMap {
    pub http: Option<String>,
    pub https: Option<String>,
    pub socks: Option<String>,
}

impl ProxySchemeMap {
    /// 代表 URL（状态展示/日志用）：优先 http，其次 https，最后 socks。
    pub fn representative(&self) -> Option<String> {
        self.http
            .clone()
            .or_else(|| self.https.clone())
            .or_else(|| self.socks.clone())
    }

    /// 单一通用代理：http 与 https 为同一 URL 且无 socks 段时返回它。
    /// 此形态与「单一全局代理」等价，用于保证行为零变化。
    pub fn single_url(&self) -> Option<String> {
        if self.socks.is_none() && self.http.is_some() && self.http == self.https {
            self.http.clone()
        } else {
            None
        }
    }

    /// 按上游 scheme 选代理；缺失名目回退代表值（避免 https 因缺配置突然变直连）。
    pub fn for_scheme(&self, scheme: &str) -> Option<String> {
        let rep = self.representative();
        match scheme {
            "http" => self.http.clone().or(rep),
            "https" => self.https.clone().or(rep),
            _ => self.socks.clone().or(rep),
        }
    }
}

/// 检测系统代理（按 scheme 区分）。
///
/// Windows: 优先注册表（ProxyEnable/ProxyServer），无则回退环境变量；
/// 其他平台: 环境变量。
pub fn detect_map() -> ProxySchemeMap {
    #[cfg(target_os = "windows")]
    {
        let registry = detect_registry_map();
        if registry.representative().is_some() {
            return registry;
        }
    }
    detect_from_env_map()
}

/// 检测系统代理映射，并过滤指向 CC Switch 自身监听端口的自环地址。
pub fn detect_map_loopback_safe(own_port: u16) -> ProxySchemeMap {
    let mut map = detect_map();
    map.http = map.http.filter(|u| !proxy_points_to_own_port(u, own_port));
    map.https = map.https.filter(|u| !proxy_points_to_own_port(u, own_port));
    map.socks = map.socks.filter(|u| !proxy_points_to_own_port(u, own_port));
    map
}

/// 检测系统代理的绕过列表（直连名单）。
///
/// 合并环境变量 `NO_PROXY` / `no_proxy`（逗号分隔）与 Windows 注册表
/// `ProxyOverride`（分号分隔，`<local>` 表示本机直连）。转发决策与 reqwest
/// 客户端都应遵守该列表，避免系统代理把本地/内网等豁免地址也塞进代理隧道。
pub fn detect_bypass() -> Vec<String> {
    let mut out = Vec::new();

    for key in ["NO_PROXY", "no_proxy"] {
        if let Ok(value) = env::var(key) {
            out.extend(parse_no_proxy_list(&value));
        }
    }

    #[cfg(target_os = "windows")]
    out.extend(registry_proxy_override());

    out.sort();
    out.dedup();
    out
}

/// 判断 host 是否命中绕过列表。
///
/// 支持:
/// - 精确匹配（`api.example.com`）
/// - `.domain` 后缀（匹配自身及其子域）
/// - `*` 通配（全部绕过）
pub fn should_bypass(host: &str, bypass: &[String]) -> bool {
    let host = host.trim().to_ascii_lowercase();
    for rule in bypass {
        let rule = rule.trim().to_ascii_lowercase();
        if rule == "*" {
            return true;
        }
        if let Some(rest) = rule.strip_prefix('.') {
            let rest = rest.trim();
            if !rest.is_empty() && (host == rest || host.ends_with(&format!(".{rest}"))) {
                return true;
            }
        } else if host == rule {
            return true;
        }
    }
    false
}

/// 解析环境变量 NO_PROXY 列表（逗号分隔，纯逻辑便于测试）。
fn parse_no_proxy_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 解析 Windows 注册表 ProxyOverride（分号分隔，纯逻辑便于测试）。
/// `<local>` 按 WinINET 语义展开为本机直连地址。
fn parse_proxy_override_list(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "<local>" {
            out.push("localhost".to_string());
            out.push("127.0.0.1".to_string());
            out.push("::1".to_string());
        } else {
            out.push(part.to_string());
        }
    }
    out
}

/// 读取 Windows 注册表 ProxyOverride。
#[cfg(target_os = "windows")]
fn registry_proxy_override() -> Vec<String> {
    let Ok(key) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(INTERNET_SETTINGS_PATH, KEY_READ)
    else {
        return Vec::new();
    };
    let value: String = key.get_value("ProxyOverride").unwrap_or_default();
    parse_proxy_override_list(&value)
}

#[cfg(target_os = "windows")]
fn detect_registry_map() -> ProxySchemeMap {
    let Ok(key) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(INTERNET_SETTINGS_PATH, KEY_READ)
    else {
        return ProxySchemeMap::default();
    };

    let proxy_enable: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    if proxy_enable == 0 {
        return ProxySchemeMap::default();
    }

    let Ok(server) = key.get_value::<String, _>("ProxyServer") else {
        return ProxySchemeMap::default();
    };
    parse_proxy_server_map(&server)
}

#[cfg(not(target_os = "windows"))]
fn detect_registry_map() -> ProxySchemeMap {
    ProxySchemeMap::default()
}

/// 从环境变量构建按 scheme 的代理映射。
///
/// 优先级: `ALL_PROXY` 作为通用底座，`HTTPS_PROXY` / `HTTP_PROXY` 再分别覆盖，
/// 特性配置优先于通用配置（大小写变体同义，后写覆盖）。
fn detect_from_env_map() -> ProxySchemeMap {
    let mut map = ProxySchemeMap::default();

    for key in ["ALL_PROXY", "all_proxy"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                let url = normalize_proxy_addr("http", value);
                map.http = Some(url.clone());
                map.https = Some(url);
            }
        }
    }
    for key in ["HTTPS_PROXY", "https_proxy"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                map.https = Some(normalize_proxy_addr("http", value));
            }
        }
    }
    for key in ["HTTP_PROXY", "http_proxy"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                map.http = Some(normalize_proxy_addr("http", value));
            }
        }
    }

    map
}

/// 解析 Windows 注册表 `ProxyServer` 值为按 scheme 的映射,支持:
///
/// - `host:port`（单一地址：同时用于 http 与 https，保持旧行为）
/// - `http://host:port`
/// - `http=host:port;https=host:port;socks=host:1080` (多协议)
// 仅 Windows 注册表路径使用；非 Windows 下仅被测试引用，避免 -D warnings 误报死代码。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_proxy_server_map(value: &str) -> ProxySchemeMap {
    let v = value.trim();
    if v.is_empty() {
        return ProxySchemeMap::default();
    }

    if v.contains('=') {
        let mut map = ProxySchemeMap::default();
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

            match protocol.as_str() {
                "http" => {
                    if map.http.is_none() {
                        map.http = Some(url);
                    }
                }
                "https" => {
                    if map.https.is_none() {
                        map.https = Some(url);
                    }
                }
                _ => {
                    if map.socks.is_none() {
                        map.socks = Some(url);
                    }
                }
            }
        }
        return map;
    }

    // 单一地址同时用于 http 与 https（mixed 端口语义，与旧实现等价）
    let url = normalize_proxy_addr("http", v);
    ProxySchemeMap {
        http: Some(url.clone()),
        https: Some(url),
        socks: None,
    }
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
    fn parse_plain_host_port_is_universal() {
        let map = parse_proxy_server_map("127.0.0.1:7890");
        assert_eq!(map.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(map.https.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(map.socks, None);
        // 单一地址必须退回“单一代理”形态，保证行为零变化
        assert_eq!(map.single_url().as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn parse_with_scheme_is_universal() {
        let map = parse_proxy_server_map("http://127.0.0.1:7890");
        assert_eq!(map.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(map.https.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(map.single_url().as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn parse_multi_protocol_keeps_scheme_entries() {
        let map =
            parse_proxy_server_map("http=127.0.0.1:7890;https=127.0.0.1:7891;socks=127.0.0.1:7892");
        assert_eq!(map.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(map.https.as_deref(), Some("http://127.0.0.1:7891"));
        assert_eq!(map.socks.as_deref(), Some("socks5h://127.0.0.1:7892"));
        // 多名目形态不再视为单一代理
        assert_eq!(map.single_url(), None);
    }

    #[test]
    fn parse_multi_protocol_socks_only() {
        let map = parse_proxy_server_map("socks=127.0.0.1:7891");
        assert_eq!(map.http, None);
        assert_eq!(map.https, None);
        assert_eq!(map.socks.as_deref(), Some("socks5h://127.0.0.1:7891"));
        assert_eq!(
            map.representative().as_deref(),
            Some("socks5h://127.0.0.1:7891")
        );
    }

    #[test]
    fn parse_empty_or_invalid() {
        assert_eq!(parse_proxy_server_map(""), ProxySchemeMap::default());
        assert_eq!(parse_proxy_server_map("   "), ProxySchemeMap::default());
        assert_eq!(
            parse_proxy_server_map("socks5=   "),
            ProxySchemeMap::default()
        );
    }

    #[test]
    fn parse_missing_https_falls_back() {
        // 只有 http 段时，https 上游必须回退到代表值，不能变成直连
        let map = parse_proxy_server_map("http=127.0.0.1:7890");
        assert_eq!(map.https, None);
        assert_eq!(
            map.for_scheme("https").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            map.for_scheme("http").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn detect_from_env_map_priority() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        // 仅 HTTP_PROXY：全体回退到它
        std::env::set_var("HTTP_PROXY", "10.0.0.1:8080");
        let map = detect_from_env_map();
        assert_eq!(map.http.as_deref(), Some("http://10.0.0.1:8080"));
        assert_eq!(
            map.for_scheme("https").as_deref(),
            Some("http://10.0.0.1:8080")
        );
        clear_proxy_env();

        // ALL_PROXY 底座 + HTTPS_PROXY 覆盖 https
        std::env::set_var("ALL_PROXY", "socks5://10.0.0.1:1080");
        std::env::set_var("HTTPS_PROXY", "10.0.0.2:8443");
        let map = detect_from_env_map();
        assert_eq!(
            map.for_scheme("https").as_deref(),
            Some("http://10.0.0.2:8443")
        );
        assert_eq!(
            map.for_scheme("http").as_deref(),
            Some("socks5://10.0.0.1:1080")
        );
        clear_proxy_env();

        assert_eq!(detect_from_env_map(), ProxySchemeMap::default());
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
    fn detect_map_loopback_safe_filters_own_port() {
        // 无法注入注册表,这里只验证过滤逻辑逐槽生效
        let full = detect_map();
        let safe = detect_map_loopback_safe(15721);
        assert_eq!(
            safe.http,
            full.http.filter(|u| !proxy_points_to_own_port(u, 15721))
        );
        assert_eq!(
            safe.https,
            full.https.filter(|u| !proxy_points_to_own_port(u, 15721))
        );
        assert_eq!(
            safe.socks,
            full.socks.filter(|u| !proxy_points_to_own_port(u, 15721))
        );
    }

    /// 实机验证: 读取当前系统注册表代理。CI 环境通常无代理,故标记 #[ignore]。
    #[test]
    #[ignore]
    fn live_registry_detect_on_machine() {
        let guard = env_lock().lock().unwrap();
        clear_proxy_env();
        let detected = detect_map().representative();
        drop(guard);
        println!("[live] system proxy detected = {detected:?}");
        // 本机启用系统代理时,期望是 http://127.0.0.1:7890
        // (不硬断言,仅打印供人工核对,避免其它环境差异导致失败)
    }

    #[test]
    fn parse_no_proxy_env_list() {
        assert_eq!(
            parse_no_proxy_list("api.example.com, .cnw.com ,127.0.0.1"),
            ["api.example.com", ".cnw.com", "127.0.0.1"]
        );
        assert_eq!(parse_no_proxy_list("  ,  ,"), Vec::<String>::new());
    }

    #[test]
    fn parse_proxy_override_expands_local() {
        let r = parse_proxy_override_list("<local>;*.mycorp.com;10.0.0.0/8");
        assert!(r.contains(&"localhost".to_string()));
        assert!(r.contains(&"127.0.0.1".to_string()));
        assert!(r.contains(&"*.mycorp.com".to_string()));
        assert!(r.contains(&"10.0.0.0/8".to_string()));
        assert!(!r.contains(&"".to_string()));
    }

    #[test]
    fn should_bypass_rules() {
        let bypass: Vec<String> = [
            "localhost",
            "127.0.0.1",
            "::1",
            ".example.com",
            "api.pure.io",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        assert!(should_bypass("localhost", &bypass));
        assert!(should_bypass("127.0.0.1", &bypass));
        assert!(should_bypass("sub.example.com", &bypass));
        assert!(should_bypass("example.com", &bypass));
        assert!(should_bypass("api.pure.io", &bypass));
        assert!(!should_bypass("api.other.com", &bypass));
        assert!(!should_bypass("anything.io", &bypass));

        // "*" 全绕过（独立列表，避免影响上面的非绕过断言）
        let wildcard: Vec<String> = vec!["*".to_string()];
        assert!(should_bypass("whatever.net", &wildcard));

        // 空列表不绕过
        assert!(!should_bypass("api.example.com", &[]));
    }

    #[test]
    fn detect_bypass_merges_env() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        std::env::set_var("NO_PROXY", "localhost,.example.com");
        let list = detect_bypass();
        assert!(list.contains(&"localhost".to_string()));
        assert!(list.contains(&".example.com".to_string()));

        // 优先级: ALL_PROXY 等不影响 NO_PROXY 探测之外的合并结果（仅验证 NO_PROXY 被纳入）
        std::env::set_var("HTTP_PROXY", "http://10.0.0.1:8080");
        assert!(detect_bypass().contains(&".example.com".to_string()));
        clear_proxy_env();
    }
}
