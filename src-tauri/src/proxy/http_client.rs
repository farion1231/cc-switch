//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

/// 全局代理 URL、主客户端和辅助客户端缓存共享同一把锁。
struct ProxyHttpState {
    proxy_url: Option<String>,
    client: Option<Client>,
    generation: u64,
    no_redirect: Option<Client>,
    https_only: Option<Client>,
}

static PROXY_STATE: RwLock<ProxyHttpState> = RwLock::new(ProxyHttpState {
    proxy_url: None,
    client: None,
    generation: 0,
    no_redirect: None,
    https_only: None,
});

/// 不走任何代理的客户端：上游是本机地址（Ollama、LM Studio 等）时使用
static DIRECT_CLIENT: OnceLock<Client> = OnceLock::new();

/// 不走代理且不跟随重定向的本机直连客户端
static DIRECT_NO_REDIRECT_CLIENT: OnceLock<Client> = OnceLock::new();

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceLock<RwLock<u16>> = OnceLock::new();

/// 当前活跃的 CC Switch 代理监听地址与端口（用于防止内部请求回打自身代理）
static ACTIVE_PROXY_LISTENERS: OnceLock<RwLock<Vec<(String, u16)>>> = OnceLock::new();

/// 活跃代理监听器的 RAII 注册句柄，释放时自动反注册
pub(crate) struct ActiveProxyListenerGuard {
    address: String,
    port: u16,
}

impl Drop for ActiveProxyListenerGuard {
    fn drop(&mut self) {
        if let Some(lock) = ACTIVE_PROXY_LISTENERS.get() {
            if let Ok(mut list) = lock.write() {
                if let Some(pos) = list
                    .iter()
                    .position(|(addr, port)| addr == &self.address && *port == self.port)
                {
                    list.swap_remove(pos);
                }
            }
        }
    }
}

/// 注册当前正在监听的 CC Switch 代理地址与端口
pub(crate) fn register_active_proxy_listener(
    address: impl Into<String>,
    port: u16,
) -> ActiveProxyListenerGuard {
    let address = address.into();
    let lock = ACTIVE_PROXY_LISTENERS.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut list) = lock.write() {
        list.push((address.clone(), port));
    }
    ActiveProxyListenerGuard { address, port }
}

/// 获取当前活跃的 CC Switch 代理监听地址列表
pub(crate) fn active_proxy_listeners() -> Vec<(String, u16)> {
    ACTIVE_PROXY_LISTENERS
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|list| list.clone())
        .unwrap_or_default()
}

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
    apply_proxy(proxy_url)
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
    let client = build_client(effective_url)?;
    let mut state = PROXY_STATE.write().map_err(|e| {
        log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
        "Failed to update proxy: lock poisoned".to_string()
    })?;
    let next = ProxyHttpState {
        proxy_url: effective_url.map(str::to_owned),
        client: Some(client),
        generation: state.generation.wrapping_add(1),
        no_redirect: None,
        https_only: None,
    };
    let previous = std::mem::replace(&mut *state, next);
    drop(state);
    drop(previous);

    log::info!(
        "[GlobalProxy] Applied: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

fn fallback_client() -> Client {
    log::warn!("[GlobalProxy] [GP-004] Client not initialized, using fallback");
    build_client(None).unwrap_or_default()
}

/// 获取配置了代理的客户端。未设置代理时，客户端跟随系统代理。
pub fn get() -> Client {
    PROXY_STATE
        .read()
        .ok()
        .and_then(|state| state.client.clone())
        .unwrap_or_else(fallback_client)
}

/// 按目标地址取客户端：本机地址直连，其余同 [`get`]
///
/// reqwest 不会自动绕开本机地址：配了全局代理时 `Proxy::all` 对所有目标生效，
/// 跟随系统代理时也只认 `NO_PROXY` 环境变量（macOS 不读「不使用代理的主机」）。
/// 本机推理服务因此会被送进代理，代理不能代连本机时请求就失败。
pub fn get_for_url(url: &str) -> Client {
    if is_loopback_url(url) {
        direct_client()
    } else {
        get()
    }
}

/// 从同一份配置读取转发所需的代理 URL 和客户端。本机地址使用直连客户端。
pub(crate) fn upstream_for_url(url: &str) -> (Option<String>, Client) {
    if is_loopback_url(url) {
        return (None, direct_client());
    }
    PROXY_STATE
        .read()
        .ok()
        .and_then(|state| Some((state.proxy_url.clone(), state.client.clone()?)))
        .unwrap_or_else(|| (None, fallback_client()))
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
    PROXY_STATE
        .read()
        .ok()
        .and_then(|state| state.proxy_url.clone())
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

#[derive(Clone, Copy)]
enum SecondarySlot {
    NoRedirect,
    HttpsOnly,
}

fn build_no_redirect_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_from(
        base_builder().redirect(reqwest::redirect::Policy::none()),
        proxy_url,
    )
}

fn build_https_only_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_from(base_builder().https_only(true), proxy_url)
}

fn get_or_init_secondary(slot: SecondarySlot) -> Result<Client, String> {
    loop {
        let (generation, proxy_url) = {
            let state = PROXY_STATE
                .read()
                .map_err(|_| "Failed to read HTTP client state: lock poisoned".to_string())?;
            let cached = match slot {
                SecondarySlot::NoRedirect => state.no_redirect.as_ref(),
                SecondarySlot::HttpsOnly => state.https_only.as_ref(),
            };
            if let Some(client) = cached {
                return Ok(client.clone());
            }
            (state.generation, state.proxy_url.clone())
        };

        let client = match slot {
            SecondarySlot::NoRedirect => build_no_redirect_client(proxy_url.as_deref())?,
            SecondarySlot::HttpsOnly => build_https_only_client(proxy_url.as_deref())?,
        };

        let mut state = PROXY_STATE
            .write()
            .map_err(|_| "Failed to update HTTP client state: lock poisoned".to_string())?;
        if state.generation != generation {
            // 构建期间代理配置已变更：丢弃旧实例并按新配置重试，避免暴露过期客户端或覆写新代际
            continue;
        }
        let target = match slot {
            SecondarySlot::NoRedirect => &mut state.no_redirect,
            SecondarySlot::HttpsOnly => &mut state.https_only,
        };
        return Ok(target.get_or_insert(client).clone());
    }
}

fn direct_no_redirect_client() -> Result<Client, String> {
    if let Some(client) = DIRECT_NO_REDIRECT_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = base_builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    let _ = DIRECT_NO_REDIRECT_CLIENT.set(client.clone());
    Ok(DIRECT_NO_REDIRECT_CLIENT.get().cloned().unwrap_or(client))
}

/// 不跟随重定向的客户端，代理设置与全局客户端一致。带自定义凭据请求头（如 `x-dsh-auth-token`）
/// 的请求用它：reqwest 跨主机重定向时只剥掉标准认证头，自定义头会被带到新主机。
pub fn no_redirect() -> Result<Client, String> {
    get_or_init_secondary(SecondarySlot::NoRedirect)
}

/// 只走 https 的客户端（跳转到 http 直接报错），代理设置与全局客户端一致。
/// 带 `Authorization` 的请求用它：reqwest 只在换主机或换端口时去掉认证头，同主机
/// 同端口从 https 跳到 http（非默认端口时会发生）会把凭据明文发出去。
pub fn https_only() -> Result<Client, String> {
    get_or_init_secondary(SecondarySlot::HttpsOnly)
}

/// A one-off no-redirect client for an explicit proxy (`None` follows the system
/// proxy), leaving the saved global proxy untouched.
pub(crate) fn no_redirect_with_proxy(
    proxy_url: Option<&str>,
    timeout: Duration,
) -> Result<Client, String> {
    build_client_from(
        base_builder()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none()),
        proxy_url,
    )
}

/// 不跟随重定向、按目标地址选代理的客户端：本机地址直连，其余同 [`no_redirect`]。
pub fn no_redirect_for_url(url: &str) -> Result<Client, String> {
    if is_loopback_url(url) {
        direct_no_redirect_client()
    } else {
        no_redirect()
    }
}

/// 构建 HTTP 客户端
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_from(base_builder(), proxy_url)
}

fn build_client_from(
    mut builder: reqwest::ClientBuilder,
    proxy_url: Option<&str>,
) -> Result<Client, String> {
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
        let (proxy_url, client) = upstream_for_url(&url);
        assert!(proxy_url.is_none());
        let resp = client
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

    #[tokio::test]
    async fn test_no_redirect_for_url_reuses_keep_alive_connection() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let accepts = Arc::new(AtomicUsize::new(0));
        let accepts_srv = accepts.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                accepts_srv.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = stream.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let resp =
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                        if stream.write_all(resp).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });

        let url = format!("http://{addr}/ping");
        for _ in 0..10 {
            let body = no_redirect_for_url(&url)
                .unwrap()
                .get(&url)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            assert_eq!(body.as_ref(), b"ok");
        }
        server.abort();

        assert!(
            accepts.load(Ordering::SeqCst) <= 2,
            "cached no_redirect_for_url client should reuse keep-alive TCP connection"
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_secondary_clients_lazy_init_and_invalidate_on_proxy_change() {
        // 先确保全局客户端处于直连态；辅助槽位应保持未构建
        apply_proxy(None).expect("reset proxy to direct");
        let gen_before = {
            let state = PROXY_STATE.read().unwrap();
            assert!(state.no_redirect.is_none());
            assert!(state.https_only.is_none());
            state.generation
        };

        // 首次调用按需构建对应槽位，不触碰另一个槽位
        let _ = no_redirect().expect("lazy build no_redirect");
        {
            let state = PROXY_STATE.read().unwrap();
            assert!(state.no_redirect.is_some());
            assert!(state.https_only.is_none());
        }

        let _ = https_only().expect("lazy build https_only");
        {
            let state = PROXY_STATE.read().unwrap();
            assert!(state.no_redirect.is_some());
            assert!(state.https_only.is_some());
        }

        // 无效代理 URL 不能破坏现有缓存与代理记录
        assert!(apply_proxy(Some("://bad-proxy")).is_err());
        {
            let state = PROXY_STATE.read().unwrap();
            assert_eq!(state.generation, gen_before);
            assert!(state.no_redirect.is_some());
            assert!(state.https_only.is_some());
        }

        // 切换有效代理后递增代际并清空辅助槽位，后续首次调用按新代理重建
        apply_proxy(Some("http://127.0.0.1:7890")).expect("apply valid proxy");
        {
            let state = PROXY_STATE.read().unwrap();
            assert_eq!(state.generation, gen_before.wrapping_add(1));
            assert_eq!(state.proxy_url.as_deref(), Some("http://127.0.0.1:7890"));
            assert!(state.no_redirect.is_none());
            assert!(state.https_only.is_none());
        }

        let _ = no_redirect().expect("rebuild no_redirect with new proxy");
        assert!(PROXY_STATE.read().unwrap().no_redirect.is_some());

        // 还原直连状态，避免影响同进程其他测试
        apply_proxy(None).expect("restore direct proxy");
    }
}
