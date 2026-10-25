//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::Duration;

/// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();

/// 不走任何代理的客户端：上游是本机地址（Ollama、LM Studio 等）时使用
static DIRECT_CLIENT: OnceCell<Client> = OnceCell::new();

// Certificate overrides never share pools with verified clients. At most two
// override pools are retained: direct routes and the current proxy configuration.
// Authentication remains on individual requests, never in client defaults.
static OVERRIDE_CLIENTS: LazyLock<Mutex<TlsOverrideClients>> =
    LazyLock::new(|| Mutex::new(TlsOverrideClients::default()));

#[derive(Default)]
struct TlsOverrideClients {
    direct: Option<Client>,
    configured: Option<(Option<String>, Client)>,
}

impl TlsOverrideClients {
    fn get(&mut self, direct: bool, proxy_url: Option<&str>) -> Result<Client, String> {
        if direct {
            if self.direct.is_none() {
                self.direct = Some(
                    base_builder()
                        .danger_accept_invalid_certs(true)
                        .no_proxy()
                        .build()
                        .map_err(|e| format!("Failed to build HTTP client: {e}"))?,
                );
            }
            return Ok(self.direct.as_ref().unwrap().clone());
        }
        if let Some((cached_url, client)) = &self.configured {
            if cached_url.as_deref() == proxy_url {
                return Ok(client.clone());
            }
        }
        let client = build_client_with_tls(proxy_url, true)?;
        self.configured = Some((proxy_url.map(str::to_owned), client.clone()));
        Ok(client)
    }
}

fn invalidate_override_clients() {
    if let Ok(mut clients) = OVERRIDE_CLIENTS.lock() {
        clients.configured = None;
    }
}

/// 当前代理 URL（用于日志和状态查询）
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

/// 设置 CC Switch 代理服务器的监听端口
///
/// 应在代理服务器启动时调用，以便系统代理检测能正确识别自己的端口
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
    invalidate_override_clients();
}

/// 获取 CC Switch 代理服务器的监听端口
fn get_proxy_port() -> u16 {
    CC_SWITCH_PROXY_PORT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|port| *port)
        .unwrap_or(15721) // 默认端口作为回退
}

/// 初始化全局 HTTP 客户端
///
/// 应在应用启动时调用一次。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，如 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080`
///   传入 None 或空字符串表示直连
pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let client = build_client(effective_url)?;

    // 尝试初始化全局客户端，如果已存在则记录警告并使用 apply_proxy 更新
    if GLOBAL_CLIENT.set(RwLock::new(client.clone())).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        // 已初始化，改用 apply_proxy 更新
        return apply_proxy(proxy_url);
    }

    // 初始化代理 URL 记录
    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(|s| s.to_string())));

    invalidate_override_clients();

    log::info!(
        "[GlobalProxy] Initialized: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 验证代理配置（不应用）
///
/// 只验证代理 URL 是否有效，不实际更新全局客户端。
/// 用于在持久化之前验证配置的有效性。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，None 或空字符串表示直连
///
/// # Returns
/// 验证成功返回 Ok(())，失败返回错误信息
pub fn validate_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    // 只调用 build_client 来验证，但不应用
    build_client(effective_url)?;
    Ok(())
}

/// 应用代理配置（假设已验证）
///
/// 直接应用代理配置到全局客户端，不做额外验证。
/// 应在 validate_proxy 成功后调用。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，None 或空字符串表示直连
pub fn apply_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let new_client = build_client(effective_url)?;

    // 更新客户端
    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        *client = new_client;
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
    }

    // 更新代理 URL 记录
    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(|s| s.to_string());
    }

    invalidate_override_clients();

    log::info!(
        "[GlobalProxy] Applied: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 更新代理配置（热更新）
///
/// 可在运行时调用以更改代理设置，无需重启应用。
/// 注意：此函数同时验证和应用，如果需要先验证后持久化再应用，
/// 请使用 validate_proxy + apply_proxy 组合。
///
/// # Arguments
/// * `proxy_url` - 新的代理 URL，None 或空字符串表示直连
#[allow(dead_code)]
pub fn update_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    apply_proxy(proxy_url)
}

/// 获取全局 HTTP 客户端
///
/// 返回配置了代理的客户端（如果已配置代理），否则返回跟随系统代理的客户端。
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

/// 按目标地址取客户端：本机地址和系统代理例外直连，其余同 [`get`]
///
/// reqwest 不会自动绕开本机地址：配了全局代理时 `Proxy::all` 对所有目标生效，
/// 跟随系统代理时也只认 `NO_PROXY` 环境变量（macOS 不读「不使用代理的主机」）。
/// 本机推理服务因此会被送进代理，代理不能代连本机时请求就失败。
pub fn get_for_url(url: &str) -> Client {
    if bypass_proxy_for_url(url) {
        direct_client()
    } else {
        get()
    }
}

// Environment proxies take precedence over WinINet in reqwest, including its bypass
// list. Preserve Windows' explicit direct routes when no app proxy was configured.
fn bypass_proxy_for_url(url: &str) -> bool {
    is_loopback_url(url) || (get_current_proxy_url().is_none() && system_proxy_bypasses_url(url))
}

#[cfg(windows)]
fn system_proxy_bypasses_url(url: &str) -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let Ok(settings) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
    else {
        return false;
    };
    if settings.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 0 {
        return false;
    }
    settings
        .get_value::<String, _>("ProxyOverride")
        .map(|rules| matches_windows_proxy_bypass(url, &rules))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn system_proxy_bypasses_url(_url: &str) -> bool {
    false
}

#[cfg(any(windows, test))]
fn matches_windows_proxy_bypass(url: &str, rules: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.');
    rules.split(';').any(|rule| {
        let rule = rule.trim();
        if rule.eq_ignore_ascii_case("<local>") {
            return !host.contains('.') && !host.contains(':');
        }
        // <-loopback> controls Chromium's implicit bypass, not a host pattern.
        if rule.is_empty() || rule.starts_with('<') {
            return false;
        }
        let (scheme, pattern) = rule
            .split_once("://")
            .map_or((None, rule), |(scheme, pattern)| (Some(scheme), pattern));
        if scheme.is_some_and(|scheme| !scheme.eq_ignore_ascii_case(url.scheme())) {
            return false;
        }
        let (pattern, port) = pattern
            .rsplit_once(':')
            .filter(|(host, port)| !host.contains(':') && port.parse::<u16>().is_ok())
            .map_or((pattern, None), |(host, port)| {
                (host, port.parse::<u16>().ok())
            });
        if port.is_some() && port != url.port_or_known_default() {
            return false;
        }
        let pattern = regex::escape(pattern).replace(r"\*", ".*");
        regex::RegexBuilder::new(&format!("^{pattern}$"))
            .case_insensitive(true)
            .build()
            .map(|pattern| pattern.is_match(host))
            .unwrap_or(false)
    })
}

/// 发往 `url` 时应使用的上游代理：本机地址返回 None（直连），其余返回当前全局代理
pub fn proxy_url_for(url: &str) -> Option<String> {
    upstream_proxy_for(url, get_current_proxy_url())
}

fn upstream_proxy_for(url: &str, current: Option<String>) -> Option<String> {
    if is_loopback_url(url) {
        None
    } else {
        current
    }
}

fn direct_client() -> Client {
    DIRECT_CLIENT
        .get_or_init(|| {
            base_builder().no_proxy().build().unwrap_or_else(|e| {
                log::warn!("[GlobalProxy] Failed to build direct client: {e}");
                Client::new()
            })
        })
        .clone()
}

/// 地址的主机是不是本机：`localhost`、`*.localhost`、`127.0.0.0/8`、`::1`
pub fn is_loopback_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(domain)) => host_is_loopback(domain),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        None => false,
    }
}

fn host_is_loopback(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// 获取当前代理 URL
///
/// 返回当前配置的代理 URL，None 表示直连。
pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

/// 检查是否正在使用代理
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

/// 两种客户端共用的参数
fn base_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        // 禁用 reqwest 自动解压：防止 reqwest 覆盖客户端原始 accept-encoding header。
        // 响应解压由 response_processor 根据 content-encoding 手动处理。
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
}

/// 构建 HTTP 客户端
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_with_tls(proxy_url, false)
}

/// Apply a provider's explicit TLS policy without altering the verified pools.
pub fn get_for_url_with_tls(url: &str, skip_verify: bool) -> Result<Client, String> {
    if !skip_verify {
        return Ok(get_for_url(url));
    }
    let mut clients = OVERRIDE_CLIENTS
        .lock()
        .map_err(|_| "Failed to acquire TLS override client lock".to_string())?;
    // Snapshot routing while holding the cache lock. A concurrent proxy update
    // invalidates this cache after publishing its URL, so later requests refresh.
    let proxy_url = get_current_proxy_url();
    let direct = is_loopback_url(url) || (proxy_url.is_none() && system_proxy_bypasses_url(url));
    clients.get(direct, proxy_url.as_deref())
}

fn build_client_with_tls(proxy_url: Option<&str>, skip_tls: bool) -> Result<Client, String> {
    let mut builder = base_builder().danger_accept_invalid_certs(skip_tls);

    // 有代理地址则使用代理，否则跟随系统代理
    if let Some(url) = proxy_url {
        // 先验证 URL 格式和 scheme
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
    } else {
        // 未设置全局代理时，让 reqwest 自动检测系统代理（环境变量）
        // 若系统代理指向本机，禁用系统代理避免自环
        if system_proxy_points_to_loopback() {
            builder = builder.no_proxy();
            log::warn!(
                "[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion"
            );
        } else {
            log::debug!("[GlobalProxy] Following system proxy (no explicit proxy configured)");
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
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
    // 检查是否指向 CC Switch 自己的代理端口
    // 只有指向自己的代理才需要跳过，避免递归
    fn is_cc_switch_proxy_port(port: Option<u16>) -> bool {
        let cc_switch_port = get_proxy_port();
        port == Some(cc_switch_port)
    }

    if let Ok(parsed) = url::Url::parse(value) {
        if let Some(host) = parsed.host_str() {
            // 只有当主机是 loopback 且端口是 CC Switch 的端口时才返回 true
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

/// 隐藏 URL 中的敏感信息（用于日志）
pub fn mask_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        // 隐藏用户名和密码，保留 scheme、host 和端口
        let host = parsed.host_str().unwrap_or("?");
        match parsed.port() {
            Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
            None => format!("{}://{}", parsed.scheme(), host),
        }
    } else {
        // URL 解析失败，返回部分内容。截断点回退到最近的字符边界，
        // 避免在多字节 UTF-8 字符中间切割导致 panic。
        if url.len() > 20 {
            let cut = (0..=20)
                .rev()
                .find(|&i| url.is_char_boundary(i))
                .unwrap_or(0);
            format!("{}...", &url[..cut])
        } else {
            url.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn test_windows_proxy_bypass_rules() {
        let rules = "*.huawei.com;127.0.0.1*;<local>;<-loopback>";
        assert!(matches_windows_proxy_bypass(
            "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
            rules
        ));
        assert!(matches_windows_proxy_bypass(
            "https://API.HUAWEI.COM./v1",
            rules
        ));
        assert!(matches_windows_proxy_bypass(
            "http://127.0.0.1:15721",
            rules
        ));
        assert!(matches_windows_proxy_bypass("http://intranet/api", rules));
        for url in [
            "https://huawei.com.evil.test",
            "https://evilhuawei.com",
            "https://api.example.com",
        ] {
            assert!(!matches_windows_proxy_bypass(url, rules));
        }
        assert!(matches_windows_proxy_bypass(
            "https://api.example.com",
            "https://api.example.com:443"
        ));
        assert!(!matches_windows_proxy_bypass(
            "http://api.example.com",
            "https://api.example.com:443"
        ));
        assert!(!matches_windows_proxy_bypass(
            "https://api.example.com:8443",
            "api.example.com:443"
        ));
        assert!(!matches_windows_proxy_bypass(
            "http://127.0.0.1",
            "<-loopback>"
        ));
        assert!(!matches_windows_proxy_bypass("invalid", "*"));
    }
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
        // 无端口的 URL 不应显示 ":?"
        assert_eq!(
            mask_url("http://proxy.example.com"),
            "http://proxy.example.com"
        );
        assert_eq!(
            mask_url("https://user:pass@proxy.example.com"),
            "https://proxy.example.com"
        );
    }

    #[test]
    fn test_mask_url_does_not_panic_on_multibyte_boundary() {
        // 一个无法被 Url::parse 解析、且在字节 20 处正好切在多字节字符中间的字符串。
        // 回归 https://github.com/farion1231/cc-switch 的 mask_url 越界 panic。
        let bad = "这是一个无效的代理地址不能解析";
        assert!(bad.len() > 20 && !bad.is_char_boundary(20));
        let masked = mask_url(bad);
        assert!(masked.ends_with("..."));
    }

    #[test]
    fn test_is_loopback_url() {
        for url in [
            "http://localhost:11434",
            "http://LOCALHOST",
            "http://localhost./v1",
            "http://127.0.0.1:1234/v1",
            "http://127.1.2.3",
            "http://[::1]:8000",
            "http://[::ffff:127.0.0.1]:8000",
            "http://foo.localhost",
            "https://Api.LocalHost:8443/v1",
        ] {
            assert!(is_loopback_url(url), "{url} should be loopback");
        }
        for url in [
            "http://192.168.1.10:11434",
            "http://10.0.0.2",
            "https://example.com",
            "https://localhost.evil.com",
            "https://evillocalhost",
            "http://[::2]",
            "localhost:11434",
            "",
        ] {
            assert!(!is_loopback_url(url), "{url} should not be loopback");
        }
    }

    #[test]
    fn test_upstream_proxy_for_skips_loopback() {
        let current = Some("http://127.0.0.1:7890".to_string());
        assert_eq!(
            upstream_proxy_for("http://localhost:11434/v1/messages", current.clone()),
            None
        );
        assert_eq!(
            upstream_proxy_for("http://127.0.0.1:1234/v1/chat/completions", current.clone()),
            None
        );
        assert_eq!(
            upstream_proxy_for("https://api.example.com/v1/messages", current.clone()),
            current
        );
        assert_eq!(
            upstream_proxy_for("https://api.example.com/v1/messages", None),
            None
        );
    }

    /// 起一个只回 200 的一次性本机 HTTP 服务，返回它的地址
    fn spawn_ok_server() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(4) {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }
        });
        format!("http://{addr}/v1/models")
    }

    #[tokio::test]
    async fn test_loopback_target_bypasses_unreachable_proxy() {
        let url = spawn_ok_server();
        // 代理指向一个连不上的端口：经代理的客户端连本机服务也会失败（缺口本身）
        let proxied = build_client(Some("http://127.0.0.1:1")).unwrap();
        assert!(proxied
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .is_err());
        // 本机地址选到的直连客户端不受代理影响
        assert!(is_loopback_url(&url));
        let resp = direct_client()
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .expect("direct client should reach the local server");
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn test_build_client_direct() {
        let result = build_client(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let result = build_client(Some("http://127.0.0.1:7890"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let result = build_client(Some("socks5://127.0.0.1:1080"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        // reqwest::Proxy::all 对某些无效 URL 不会立即报错
        // 使用明确无效的 scheme 来触发错误
        let result = build_client(Some("invalid-scheme://127.0.0.1:7890"));
        assert!(result.is_err(), "Should reject invalid proxy scheme");
    }

    #[test]
    #[serial_test::serial]
    fn test_proxy_points_to_loopback() {
        // 设置 CC Switch 代理端口为 15721（默认值）
        set_proxy_port(15721);

        // 只有指向 CC Switch 自己端口的 loopback 地址才返回 true
        assert!(proxy_points_to_loopback("http://127.0.0.1:15721"));
        assert!(proxy_points_to_loopback("socks5://localhost:15721"));
        assert!(proxy_points_to_loopback("127.0.0.1:15721"));

        // 其他 loopback 端口不应该被跳过（允许使用其他本地代理工具）
        assert!(!proxy_points_to_loopback("http://127.0.0.1:7890"));
        assert!(!proxy_points_to_loopback("socks5://localhost:1080"));

        // 非 loopback 地址不应该被跳过
        assert!(!proxy_points_to_loopback("http://192.168.1.10:7890"));
        assert!(!proxy_points_to_loopback("http://192.168.1.10:15721"));
    }

    #[test]
    #[serial_test::serial]
    fn test_system_proxy_points_to_loopback() {
        let _guard = env_lock().lock().unwrap();

        // 设置 CC Switch 代理端口
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
            std::env::remove_var(key);
        }

        // 指向 CC Switch 端口的代理应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:15721");
        assert!(system_proxy_points_to_loopback());

        // 指向其他端口的本地代理不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(!system_proxy_points_to_loopback());

        // 非 loopback 地址不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://10.0.0.2:7890");
        assert!(!system_proxy_points_to_loopback());

        for key in &keys {
            std::env::remove_var(key);
        }
    }
}

#[cfg(test)]
#[path = "http_client_pool_tests.rs"]
mod pool_tests;
