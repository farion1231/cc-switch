//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

#[cfg(windows)]
use super::windows_proxy;
use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::Duration;

static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

pub fn set_proxy_port(port: u16) {
    if let Some(lock) = CC_SWITCH_PROXY_PORT.get() {
        if let Ok(mut current_port) = lock.write() {
            *current_port = port;
            log::debug!("[GlobalProxy] Updated CC Switch proxy port to {port}");
        }
    } else {
        let _ = CC_SWITCH_PROXY_PORT.set(RwLock::new(port));
        log::debug!("[GlobalProxy] Initialized CC Switch proxy port to {port}");
    }
}

fn get_proxy_port() -> u16 {
    CC_SWITCH_PROXY_PORT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|port| *port)
        .unwrap_or(15721)
}

pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let client = build_client(effective_url)?;

    if GLOBAL_CLIENT.set(RwLock::new(client.clone())).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        return apply_proxy(proxy_url);
    }

    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(|s| s.to_string())));

    log::info!(
        "[GlobalProxy] Initialized: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

pub fn validate_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    build_client(effective_url)?;
    Ok(())
}

pub fn apply_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let new_client = build_client(effective_url)?;

    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        *client = new_client;
    } else {
        return init(proxy_url);
    }

    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(|s| s.to_string());
    }

    log::info!(
        "[GlobalProxy] Applied: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

#[allow(dead_code)]
pub fn update_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let new_client = build_client(effective_url)?;

    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        *client = new_client;
    } else {
        return init(proxy_url);
    }

    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(|s| s.to_string());
    }

    log::info!(
        "[GlobalProxy] Updated: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

pub fn get() -> Client {
    GLOBAL_CLIENT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|c| c.clone())
        .unwrap_or_else(|| {
            log::warn!("[GlobalProxy] [GP-004] Client not initialized, using fallback");
            build_client(None).unwrap_or_default()
        })
}

pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();

    if let Some(url) = proxy_url {
        let parsed = url::Url::parse(url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;

        let scheme = parsed.scheme();
        if !["http", "https", "socks5", "socks5h"].contains(&scheme) {
            return Err(format!(
                "Invalid proxy scheme '{}' in URL '{}'. Supported: http, https, socks5, socks5h",
                scheme,
                mask_url(url)
            ));
        }

        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;
        builder = builder.proxy(proxy);
        log::debug!("[GlobalProxy] Proxy configured: {}", mask_url(url));
    } else if system_proxy_points_to_loopback() {
        builder = builder.no_proxy();
        log::warn!(
            "[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion"
        );
    } else {
        #[cfg(windows)]
        {
            match windows_proxy::load_manual_system_proxy_config() {
                Ok(Some(config)) => {
                    let env_http_proxy = env_proxy_for_scheme("http");
                    let env_https_proxy = env_proxy_for_scheme("https");
                    let http_proxy = env_http_proxy
                        .as_deref()
                        .or_else(|| config.http_proxy());
                    let https_proxy = env_https_proxy
                        .as_deref()
                        .or_else(|| config.https_proxy());
                    let masked_http = http_proxy.map(mask_url);
                    let masked_https = https_proxy.map(mask_url);

                    let bypass = config.bypass().clone();
                    let bypass_rule_count = config.bypass_rule_count();
                    let bypasses_local = config.bypasses_local();
                    let mut applied = false;
                    builder = builder.no_proxy();

                    if http_proxy.is_some() {
                        builder = builder.proxy(custom_windows_proxy(
                            "http",
                            env_http_proxy.as_deref(),
                            config.http_proxy(),
                            bypass.clone(),
                        ));
                        applied = true;
                    }
                    if https_proxy.is_some() {
                        builder = builder.proxy(custom_windows_proxy(
                            "https",
                            env_https_proxy.as_deref(),
                            config.https_proxy(),
                            bypass.clone(),
                        ));
                        applied = true;
                    }

                    if applied {
                        log::info!(
                            "[GlobalProxy] Following Windows proxy settings with environment precedence: http={}, https={}, bypass_rules={}, bypass_local={}",
                            masked_http.unwrap_or_else(|| "none".to_string()),
                            masked_https.unwrap_or_else(|| "none".to_string()),
                            bypass_rule_count,
                            bypasses_local
                        );
                    }
                }
                Ok(None) => {
                    log::debug!(
                        "[GlobalProxy] Following system proxy (no explicit manual proxy configured)"
                    );
                }
                Err(error) => {
                    log::warn!(
                        "[GlobalProxy] Failed to load Windows manual proxy bypass rules, falling back to reqwest system proxy detection: {error}"
                    );
                }
            }
        }

        #[cfg(not(windows))]
        {
            log::debug!("[GlobalProxy] Following system proxy (no explicit proxy configured)");
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

#[cfg(windows)]
fn env_proxy_for_scheme(scheme: &str) -> Option<String> {
    let keys: &[&str] = match scheme {
        "http" => &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"],
        "https" => &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"],
        _ => &[],
    };

    keys.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

#[cfg(windows)]
fn custom_windows_proxy(
    scheme: &'static str,
    env_proxy_url: Option<&str>,
    manual_proxy_url: Option<&str>,
    bypass: windows_proxy::ProxyBypassMatcher,
) -> reqwest::Proxy {
    let env_proxy_url = env_proxy_url.map(str::to_owned);
    let manual_proxy_url = manual_proxy_url.map(str::to_owned);
    let masked_proxy = env_proxy_url
        .as_deref()
        .or(manual_proxy_url.as_deref())
        .map(mask_url)
        .unwrap_or_else(|| "none".to_string());

    reqwest::Proxy::custom(move |url| {
        if url.scheme() != scheme {
            return None;
        }

        let host = url.host_str().unwrap_or_default();
        if bypass.matches_url(url) {
            log::debug!(
                "[GlobalProxy] Windows system proxy bypass matched for {}://{}",
                scheme,
                host
            );
            None
        } else if let Some(proxy_url) = env_proxy_url.as_ref().or(manual_proxy_url.as_ref()) {
            log::debug!(
                "[GlobalProxy] Windows system proxy routing {}://{} via {}",
                scheme,
                host,
                masked_proxy
            );
            Some(proxy_url.clone())
        } else {
            None
        }
    })
}

fn system_proxy_points_to_loopback() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    KEYS.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| proxy_points_to_loopback(&value))
}

fn proxy_points_to_loopback(value: &str) -> bool {
    fn host_is_loopback(host: &str) -> bool {
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        host.parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }

    fn is_cc_switch_proxy_port(port: Option<u16>) -> bool {
        port == Some(get_proxy_port())
    }

    if let Ok(parsed) = url::Url::parse(value) {
        if let Some(host) = parsed.host_str() {
            return host_is_loopback(host) && is_cc_switch_proxy_port(parsed.port());
        }
        return false;
    }

    let with_scheme = format!("http://{value}");
    if let Ok(parsed) = url::Url::parse(&with_scheme) {
        if let Some(host) = parsed.host_str() {
            return host_is_loopback(host) && is_cc_switch_proxy_port(parsed.port());
        }
    }

    false
}

pub fn mask_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed.host_str().unwrap_or("?");
        match parsed.port() {
            Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
            None => format!("{}://{}", parsed.scheme(), host),
        }
    } else if url.len() > 20 {
        let cut = (0..=20)
            .rev()
            .find(|&i| url.is_char_boundary(i))
            .unwrap_or(0);
        format!("{}...", &url[..cut])
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_mask_url() {
        assert_eq!(mask_url("http://127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(
            mask_url("http://user:pass@127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            mask_url("socks5://admin:secret@proxy.example.com:1080"),
            "socks5://proxy.example.com:1080"
        );
        assert_eq!(mask_url("http://proxy.example.com"), "http://proxy.example.com");
        assert_eq!(
            mask_url("https://user:pass@proxy.example.com"),
            "https://proxy.example.com"
        );
    }

    #[test]
    fn test_mask_url_does_not_panic_on_multibyte_boundary() {
        let bad = "这是一个无效的代理地址不能解析";
        assert!(bad.len() > 20 && !bad.is_char_boundary(20));
        assert!(mask_url(bad).ends_with("..."));
    }

    #[test]
    fn test_build_client_direct() {
        assert!(build_client(None).is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        assert!(build_client(Some("http://127.0.0.1:7890")).is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        assert!(build_client(Some("socks5://127.0.0.1:1080")).is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        assert!(build_client(Some("invalid-scheme://127.0.0.1:7890")).is_err());
    }

    #[test]
    fn test_proxy_points_to_loopback() {
        set_proxy_port(15721);
        assert!(proxy_points_to_loopback("http://127.0.0.1:15721"));
        assert!(proxy_points_to_loopback("socks5://localhost:15721"));
        assert!(proxy_points_to_loopback("127.0.0.1:15721"));
        assert!(!proxy_points_to_loopback("http://127.0.0.1:7890"));
        assert!(!proxy_points_to_loopback("socks5://localhost:1080"));
        assert!(!proxy_points_to_loopback("http://192.168.1.10:7890"));
        assert!(!proxy_points_to_loopback("http://192.168.1.10:15721"));
    }

    #[test]
    fn test_system_proxy_points_to_loopback() {
        let _guard = env_lock().lock().unwrap();
        set_proxy_port(15721);
        let keys = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ];
        for key in &keys {
            env::remove_var(key);
        }
        env::set_var("HTTP_PROXY", "http://127.0.0.1:15721");
        assert!(system_proxy_points_to_loopback());
        env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(!system_proxy_points_to_loopback());
        env::set_var("HTTP_PROXY", "http://10.0.0.2:7890");
        assert!(!system_proxy_points_to_loopback());
        for key in &keys {
            env::remove_var(key);
        }
    }

    #[cfg(windows)]
    #[test]
    fn environment_proxy_has_priority_over_manual_proxy() {
        let _guard = env_lock().lock().unwrap();
        env::set_var("HTTP_PROXY", "http://env.proxy:8080");
        env::remove_var("HTTPS_PROXY");
        env::remove_var("ALL_PROXY");
        assert_eq!(env_proxy_for_scheme("http").as_deref(), Some("http://env.proxy:8080"));
        env::remove_var("HTTP_PROXY");
    }
}
