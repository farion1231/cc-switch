//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use crate::provider::ProviderProxyConfig;
use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::Duration;

/// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();

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

    log::info!(
        "[GlobalProxy] Updated: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
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

/// 构建 HTTP 客户端
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        // 禁用 reqwest 自动解压：防止 reqwest 覆盖客户端原始 accept-encoding header。
        // 响应解压由 response_processor 根据 content-encoding 手动处理。
        .no_gzip()
        .no_brotli()
        .no_deflate();

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
    fn host_is_loopback(host: &str) -> bool {
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        host.parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }

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
        // URL 解析失败，返回部分内容
        if url.len() > 20 {
            format!("{}...", &url[..20])
        } else {
            url.to_string()
        }
    }
}

/// 根据供应商单独代理配置构建代理 URL
///
/// 将 ProviderProxyConfig 转换为代理 URL 字符串
pub fn build_proxy_url_from_config(config: &ProviderProxyConfig) -> Option<String> {
    let proxy_type = config.proxy_type.as_deref().unwrap_or("http");
    let host = config.proxy_host.as_deref()?;
    let port = config.proxy_port?;

    // 构建带认证的代理 URL
    if let (Some(username), Some(password)) = (&config.proxy_username, &config.proxy_password) {
        if !username.is_empty() && !password.is_empty() {
            return Some(format!(
                "{proxy_type}://{username}:{password}@{host}:{port}"
            ));
        }
    }

    Some(format!("{proxy_type}://{host}:{port}"))
}

/// 根据供应商单独代理配置构建 HTTP 客户端
///
/// 如果供应商配置了单独代理（enabled = true），则使用该代理构建客户端；
/// 否则返回 None，调用方应使用全局客户端。
///
/// # Arguments
/// * `proxy_config` - 供应商的代理配置
///
/// # Returns
/// 如果配置有效则返回 Some(Client)，否则返回 None
pub fn build_client_for_provider(proxy_config: Option<&ProviderProxyConfig>) -> Option<Client> {
    let config = proxy_config.filter(|c| c.enabled)?;

    let proxy_url = build_proxy_url_from_config(config)?;

    log::debug!(
        "[ProviderProxy] Building client with proxy: {}",
        mask_url(&proxy_url)
    );

    // 构建带代理的客户端
    let proxy = match reqwest::Proxy::all(&proxy_url) {
        Ok(p) => p,
        Err(e) => {
            log::error!(
                "[ProviderProxy] Failed to create proxy from '{}': {}",
                mask_url(&proxy_url),
                e
            );
            return None;
        }
    };

    match Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .proxy(proxy)
        .build()
    {
        Ok(client) => {
            log::info!(
                "[ProviderProxy] Client built with proxy: {}",
                mask_url(&proxy_url)
            );
            Some(client)
        }
        Err(e) => {
            log::error!("[ProviderProxy] Failed to build client: {e}");
            None
        }
    }
}

fn apply_connection_override(
    mut builder: reqwest::ClientBuilder,
    request_url: Option<&str>,
    connection_override: Option<&str>,
) -> Result<reqwest::ClientBuilder, String> {
    let Some(override_addr) =
        super::connection_override::parse_connection_override(connection_override)?
    else {
        return Ok(builder);
    };

    let Some(url_text) = request_url else {
        return Err("Connection override requires a valid request URL".to_string());
    };

    let parsed =
        url::Url::parse(url_text).map_err(|e| format!("Invalid request URL for override: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Request URL missing host for connection override".to_string())?;

    // Keep URL semantic host as SNI/Host while routing TCP to override IP:port.
    builder = builder.resolve(host, override_addr);
    Ok(builder)
}

/// Build effective request URL when connection override is enabled.
///
/// The override host/IP routing is handled by DNS resolve override. This function
/// additionally applies the override port to request URL so requests originally
/// targeting `domain:old_port` can connect to `override_ip:new_port`.
pub fn apply_connection_override_to_url(
    request_url: &str,
    connection_override: Option<&str>,
) -> Result<String, String> {
    let Some(override_addr) =
        super::connection_override::parse_connection_override(connection_override)?
    else {
        return Ok(request_url.to_string());
    };

    let mut parsed = url::Url::parse(request_url)
        .map_err(|e| format!("Invalid request URL for override: {e}"))?;
    parsed
        .set_port(Some(override_addr.port()))
        .map_err(|_| "Unable to apply override port to request URL".to_string())?;
    Ok(parsed.to_string())
}

/// Extract the original URL authority used for the HTTP Host header.
pub fn request_authority(request_url: &str) -> Result<String, String> {
    request_url
        .parse::<http::Uri>()
        .map_err(|e| format!("Invalid request URL for authority: {e}"))?
        .authority()
        .map(|authority| authority.as_str().to_string())
        .ok_or_else(|| "Request URL missing authority".to_string())
}

fn build_provider_client(
    proxy_config: Option<&ProviderProxyConfig>,
    request_url: Option<&str>,
    connection_override: Option<&str>,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60));

    if let Some(config) = proxy_config.filter(|c| c.enabled) {
        if let Some(proxy_url) = build_proxy_url_from_config(config) {
            let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
                format!(
                    "Invalid provider proxy URL '{}': {}",
                    mask_url(&proxy_url),
                    e
                )
            })?;
            builder = builder.proxy(proxy);
        }
    }

    builder = apply_connection_override(builder, request_url, connection_override)?;
    builder
        .build()
        .map_err(|e| format!("Failed to build provider HTTP client: {e}"))
}

/// 获取供应商专用的 HTTP 客户端
///
/// 优先使用供应商单独代理配置，如果未启用则返回全局客户端。
///
/// # Arguments
/// * `proxy_config` - 供应商的代理配置
///
/// # Returns
/// 返回适合该供应商的 HTTP 客户端
pub fn get_for_provider(proxy_config: Option<&ProviderProxyConfig>) -> Client {
    // 优先使用供应商单独代理
    if let Some(client) = build_client_for_provider(proxy_config) {
        return client;
    }

    // 回退到全局客户端
    get()
}

pub fn get_for_provider_with_override(
    proxy_config: Option<&ProviderProxyConfig>,
    request_url: Option<&str>,
    connection_override: Option<&str>,
) -> Result<Client, String> {
    match build_provider_client(proxy_config, request_url, connection_override) {
        Ok(client) => Ok(client),
        Err(err) => {
            // Fallback: no provider-specific settings, keep existing global behavior.
            let has_provider_proxy = proxy_config.is_some_and(|c| c.enabled);
            let has_override = connection_override
                .map(str::trim)
                .is_some_and(|text| !text.is_empty());
            if !has_provider_proxy && !has_override {
                return Ok(get_for_provider(proxy_config));
            }
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
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

    #[test]
    fn test_get_for_provider_with_override_valid() {
        let client = get_for_provider_with_override(
            None,
            Some("https://test.example.com/v1/chat/completions"),
            Some("1.2.3.4:443"),
        );
        assert!(client.is_ok());
    }

    #[test]
    fn test_get_for_provider_with_override_invalid_override() {
        let client = get_for_provider_with_override(
            None,
            Some("https://test.example.com/v1/chat/completions"),
            Some("invalid-host:443"),
        );
        assert!(client.is_err());
    }

    #[test]
    fn test_get_for_provider_with_override_missing_url() {
        let client = get_for_provider_with_override(None, None, Some("1.2.3.4:443"));
        assert!(client.is_err());
    }

    #[test]
    fn test_override_routes_connection_but_preserves_host_header() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let port = listener.local_addr().expect("listener addr").port();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept connection");
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
            stream.write_all(response).expect("write response");
            request
        });

        let url = format!("http://semantic.invalid:{port}/ping");
        let client =
            get_for_provider_with_override(None, Some(&url), Some(&format!("127.0.0.1:{port}")))
                .expect("build client with override");

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let response = rt
            .block_on(async { client.get(&url).send().await })
            .expect("request with override should succeed");
        assert_eq!(response.status().as_u16(), 200);

        let request_text = server.join().expect("join server thread");
        let request_lower = request_text.to_lowercase();
        assert!(
            request_lower.contains(&format!("host: semantic.invalid:{port}")),
            "expected host header to keep original semantic host, got: {request_text}"
        );
    }

    #[test]
    fn test_apply_connection_override_to_url_replaces_port() {
        let rewritten = apply_connection_override_to_url(
            "https://api.example.test:8443/responses",
            Some("203.0.113.10:26101"),
        )
        .expect("rewrite url");

        assert_eq!(rewritten, "https://api.example.test:26101/responses");
    }

    #[test]
    fn test_apply_connection_override_to_url_keeps_original_without_override() {
        let original = "https://api.example.test:8443/responses";
        let rewritten =
            apply_connection_override_to_url(original, None).expect("rewrite url without override");
        assert_eq!(rewritten, original);
    }

    #[test]
    fn test_request_authority_preserves_original_url_authority() {
        assert_eq!(
            request_authority("https://api.example.test/v1/models").expect("authority"),
            "api.example.test"
        );
        assert_eq!(
            request_authority("https://api.example.test:8443/v1/models").expect("authority"),
            "api.example.test:8443"
        );
    }

    #[test]
    fn test_without_override_keeps_old_behavior() {
        let url = "http://semantic.invalid:6553/ping";
        let client = get_for_provider_with_override(None, Some(url), None).expect("build client");
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let result = rt.block_on(async { client.get(url).send().await });
        assert!(
            result.is_err(),
            "without override, unresolved semantic host should fail"
        );
    }
}
