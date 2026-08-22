//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::Duration;

use super::system_proxy;

/// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();

/// 当前代理 URL（用于日志和状态查询）
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// 当前跟随系统代理时的绕过列表（NO_PROXY / ProxyOverride，显式代理模式下为空）
static CURRENT_BYPASS: OnceCell<RwLock<Vec<String>>> = OnceCell::new();

/// 当前出站代理配置（按 scheme 区分，用于转发按上游协议选路）
static CURRENT_PROXY_MAP: OnceCell<RwLock<system_proxy::ProxySchemeMap>> = OnceCell::new();

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

/// 出站代理模式
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProxyMode {
    /// 用户显式配置的全局代理（最高优先级，不受系统代理变化影响）
    Explicit,
    /// 跟随系统代理（注册表/环境变量），由监视器动态刷新
    System,
}

static PROXY_MODE: OnceCell<RwLock<ProxyMode>> = OnceCell::new();

/// 串行化全局代理状态更新。
///
/// 系统代理 watcher（refresh_system_proxy）与显式全局代理设置（apply_proxy /
/// update_proxy）会并发修改 GLOBAL_CLIENT / CURRENT_PROXY_URL / PROXY_MODE。
/// 若不串行化，watcher 在“检查到 System 模式”之后、真正改写状态之前，可能被
/// apply_proxy 插队：refresh 会用系统代理覆盖刚写入的显式代理，而模式却停在
/// Explicit，导致后续 watcher 不再自愈、请求一直走错误的代理。
static PROXY_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
///   传入 None 或空字符串表示跟随系统代理（注册表/环境变量）
pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let explicit = proxy_url.filter(|s| !s.trim().is_empty());
    let info = resolve_proxy_info(explicit);
    let client = build_client(&info.map, &info.bypass)?;

    // 尝试初始化全局客户端，如果已存在则记录警告并使用 apply_proxy 更新
    if GLOBAL_CLIENT.set(RwLock::new(client.clone())).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            label_for(&info.url)
        );
        // 已初始化，改用 apply_proxy 更新
        return apply_proxy(proxy_url);
    }

    // 初始化代理 URL、按 scheme 映射与绕过列表记录
    let _ = CURRENT_PROXY_URL.set(RwLock::new(info.url.clone()));
    let _ = CURRENT_PROXY_MAP.set(RwLock::new(info.map.clone()));
    let _ = CURRENT_BYPASS.set(RwLock::new(info.bypass));
    set_proxy_mode(explicit.is_some());

    log::info!("[GlobalProxy] Initialized: {}", label_for(&info.url));

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
    let map = match effective_url {
        Some(url) => single_proxy_map(url),
        None => system_proxy::ProxySchemeMap::default(),
    };
    build_client(&map, &[])?;
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
    let _guard = proxy_state_guard();
    let explicit = proxy_url.filter(|s| !s.trim().is_empty());
    let info = resolve_proxy_info(explicit);
    let new_client = build_client(&info.map, &info.bypass)?;

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

    // 更新代理 URL 与绕过列表记录
    set_current_proxy_state(&info);
    set_proxy_mode(explicit.is_some());

    log::info!("[GlobalProxy] Applied: {}", label_for(&info.url));

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
    let _guard = proxy_state_guard();
    let explicit = proxy_url.filter(|s| !s.trim().is_empty());
    let info = resolve_proxy_info(explicit);
    let new_client = build_client(&info.map, &info.bypass)?;

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

    // 更新代理 URL 与绕过列表记录
    set_current_proxy_state(&info);
    set_proxy_mode(explicit.is_some());

    log::info!("[GlobalProxy] Updated: {}", label_for(&info.url));

    Ok(())
}

/// 刷新系统代理（由监视器周期性调用）
///
/// 仅在「跟随系统代理」模式下生效; 用户显式配置的全局代理优先且不受影响。
/// 检测到系统代理变化时热更新全局 HTTP 客户端。
///
/// 返回是否有变化。
pub fn refresh_system_proxy() -> bool {
    // 与 apply_proxy/update_proxy 串行化：模式检查与状态改写必须在同一临界区内，
    // 避免被“显式代理设置”插队导致覆盖刚写入的显式代理。
    let _guard = proxy_state_guard();
    if !is_system_mode() {
        return false;
    }
    // 未初始化时由 init 负责，跳过
    if GLOBAL_CLIENT.get().is_none() {
        return false;
    }

    let info = resolve_proxy_info(None);
    let current = get_current_proxy_url();
    let current_map = current_proxy_map();
    let current_bypass = current_bypass();
    if current == info.url && current_map == info.map && current_bypass == info.bypass {
        return false;
    }

    let new_client = match build_client(&info.map, &info.bypass) {
        Ok(client) => client,
        Err(e) => {
            log::warn!(
                "[GlobalProxy] [GP-012] Failed to rebuild client after system proxy change: {e}"
            );
            return false;
        }
    };

    if let Some(lock) = GLOBAL_CLIENT.get() {
        if let Ok(mut client) = lock.write() {
            *client = new_client;
        }
    }
    set_current_proxy_state(&info);

    log::info!(
        "[GlobalProxy] System proxy changed: {} -> {}",
        label_for(&current),
        label_for(&info.url)
    );
    true
}

/// 更新运行态代理状态（URL、按 scheme 映射、绕过列表；须在 PROXY_STATE_LOCK 临界区内调用）。
fn set_current_proxy_state(info: &ResolvedProxy) {
    if let Some(lock) = CURRENT_PROXY_URL.get() {
        if let Ok(mut url) = lock.write() {
            *url = info.url.clone();
        }
    }
    if let Some(lock) = CURRENT_PROXY_MAP.get() {
        if let Ok(mut map) = lock.write() {
            *map = info.map.clone();
        }
    }
    if let Some(lock) = CURRENT_BYPASS.get() {
        if let Ok(mut bypass) = lock.write() {
            *bypass = info.bypass.clone();
        }
    }
}

/// 获取代理状态串行化锁的守卫（中毒时自动恢复）。
fn proxy_state_guard() -> std::sync::MutexGuard<'static, ()> {
    PROXY_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 解析出的代理配置（代表 URL + 按 scheme 映射 + 绕过列表）。
struct ResolvedProxy {
    /// 代表 URL（状态/日志用），优先 http → https → socks
    url: Option<String>,
    /// 按 scheme 区分的代理映射（路由用）
    map: system_proxy::ProxySchemeMap,
    /// NO_PROXY / ProxyOverride 绕过列表
    bypass: Vec<String>,
}

/// 构造“单一通用代理”映射（显式全局代理、校验用）。
fn single_proxy_map(url: &str) -> system_proxy::ProxySchemeMap {
    system_proxy::ProxySchemeMap {
        http: Some(url.to_string()),
        https: Some(url.to_string()),
        socks: None,
    }
}

/// 解析出站代理：显式全局代理优先；否则跟随系统代理（含 NO_PROXY / ProxyOverride 绕过列表）。
///
/// 显式代理按“单一通用代理”处理（http/https 同 URL），与旧行为保持一致。
fn resolve_proxy_info(explicit: Option<&str>) -> ResolvedProxy {
    if let Some(url) = explicit {
        return ResolvedProxy {
            url: Some(url.to_string()),
            map: single_proxy_map(url),
            bypass: Vec::new(),
        };
    }
    let map = system_proxy::detect_map_loopback_safe(get_proxy_port());
    ResolvedProxy {
        url: map.representative(),
        map,
        bypass: system_proxy::detect_bypass(),
    }
}

fn label_for(url: &Option<String>) -> String {
    url.as_deref()
        .map(mask_url)
        .unwrap_or_else(|| "direct connection".to_string())
}

fn set_proxy_mode(explicit: bool) {
    let mode = if explicit {
        ProxyMode::Explicit
    } else {
        ProxyMode::System
    };
    match PROXY_MODE.get() {
        Some(lock) => {
            if let Ok(mut m) = lock.write() {
                *m = mode;
            }
        }
        None => {
            let _ = PROXY_MODE.set(RwLock::new(mode));
        }
    }
}

fn is_system_mode() -> bool {
    PROXY_MODE
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|m| *m == ProxyMode::System)
        .unwrap_or(true)
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
            build_client(&system_proxy::ProxySchemeMap::default(), &[]).unwrap_or_default()
        })
}

/// 获取当前代理 URL
///
/// 返回当前配置的代理 URL（代表值），None 表示直连。
pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

/// 读取当前代理绕过列表缓存。
fn current_bypass() -> Vec<String> {
    CURRENT_BYPASS
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|b| b.clone())
        .unwrap_or_default()
}

/// 读取当前代理配置映射缓存。
fn current_proxy_map() -> system_proxy::ProxySchemeMap {
    CURRENT_PROXY_MAP
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|m| m.clone())
        .unwrap_or_default()
}

/// 按代理映射与绕过列表为上游 URL 选择出站代理（纯逻辑，便于测试）。
fn pick_proxy(
    map: &system_proxy::ProxySchemeMap,
    upstream: &str,
    bypass: &[String],
) -> Option<String> {
    match url::Url::parse(upstream) {
        Ok(parsed) => {
            if let Some(host) = parsed.host_str() {
                if system_proxy::should_bypass(host, bypass) {
                    return None;
                }
            }
            map.for_scheme(parsed.scheme())
        }
        // 上游 URL 无法解析时保持保守：按代表代理处理（与旧行为一致）
        Err(_) => map.representative(),
    }
}

/// 为指定上游 URL 选择出站代理。
///
/// 命中系统代理绕过列表（NO_PROXY / ProxyOverride）时返回直达（None）；
/// 否则按上游 scheme 返回对应代理。forwarder 据此决定是否走代理隧道。
pub fn get_proxy_for_url(upstream: &str) -> Option<String> {
    get_current_proxy_url()?;
    let map = current_proxy_map();
    let bypass = current_bypass();
    pick_proxy(&map, upstream, &bypass)
}

/// 检查是否正在使用代理
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

/// 构建 HTTP 客户端
///
/// `map` 为按 scheme 区分的出站代理；`bypass` 为代理绕过列表
/// （NO_PROXY / ProxyOverride），命中时目标地址不走代理。
fn build_client(map: &system_proxy::ProxySchemeMap, bypass: &[String]) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        // 禁用 reqwest 自动解压：防止 reqwest 覆盖客户端原始 accept-encoding header。
        // 响应解压由 response_processor 根据 content-encoding 手动处理。
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();

    let attach_bypass = |proxy: reqwest::Proxy, url: &str| -> reqwest::Proxy {
        if !bypass.is_empty() {
            let proxy = proxy.no_proxy(reqwest::NoProxy::from_string(&bypass.join(",")));
            log::debug!(
                "[GlobalProxy] Proxy configured with {} bypass entrie(s)",
                bypass.len()
            );
            log::debug!("[GlobalProxy] Proxy configured: {}", mask_url(url));
            proxy
        } else {
            log::debug!("[GlobalProxy] Proxy configured: {}", mask_url(url));
            proxy
        }
    };

    // 单一通用代理：一切按旧行为（Proxy::all），保证零行为差异
    if let Some(url) = map.single_url() {
        validate_proxy_url(&url)?;
        let proxy = reqwest::Proxy::all(&url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(&url), e))?;
        builder = builder.proxy(attach_bypass(proxy, &url));
    } else if map.http.is_some() || map.https.is_some() || map.socks.is_some() {
        // 多名目：http/https 分别设置；socks 作为兜底（Proxy::all）被更具体的覆盖
        let rep = map.representative();
        if let Some(url) = &map.socks {
            validate_proxy_url(url)?;
            let proxy = reqwest::Proxy::all(url)
                .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;
            builder = builder.proxy(attach_bypass(proxy, url));
        }
        if let Some(url) = map.http.clone().or_else(|| rep.clone()) {
            validate_proxy_url(&url)?;
            let proxy = reqwest::Proxy::http(&url)
                .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(&url), e))?;
            builder = builder.proxy(attach_bypass(proxy, &url));
        }
        if let Some(url) = map.https.clone().or_else(|| rep.clone()) {
            validate_proxy_url(&url)?;
            let proxy = reqwest::Proxy::https(&url)
                .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(&url), e))?;
            builder = builder.proxy(attach_bypass(proxy, &url));
        }
    } else {
        // 未配置代理时，让 reqwest 自动检测系统代理（环境变量）
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

fn validate_proxy_url(url: &str) -> Result<(), String> {
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
    Ok(())
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
    fn test_build_client_direct() {
        let result = build_client(&system_proxy::ProxySchemeMap::default(), &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let map = single_proxy_map("http://127.0.0.1:7890");
        let result = build_client(&map, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let map = single_proxy_map("socks5://127.0.0.1:1080");
        let result = build_client(&map, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        // reqwest::Proxy::all 对某些无效 URL 不会立即报错
        // 使用明确无效的 scheme 来触发错误
        let map = single_proxy_map("invalid-scheme://127.0.0.1:7890");
        let result = build_client(&map, &[]);
        assert!(result.is_err(), "Should reject invalid proxy scheme");
    }

    #[test]
    fn test_build_client_with_bypass() {
        let bypass = vec!["localhost".to_string(), ".example.com".to_string()];
        let map = single_proxy_map("http://127.0.0.1:7890");
        let result = build_client(&map, &bypass);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_multi_scheme() {
        let map = system_proxy::ProxySchemeMap {
            http: Some("http://127.0.0.1:7890".to_string()),
            https: Some("http://127.0.0.1:7891".to_string()),
            socks: None,
        };
        assert!(build_client(&map, &[]).is_ok());
    }

    #[test]
    fn test_pick_proxy_multi_scheme() {
        let map = system_proxy::ProxySchemeMap {
            http: Some("http://127.0.0.1:7890".to_string()),
            https: Some("http://127.0.0.1:7891".to_string()),
            socks: Some("socks5h://127.0.0.1:1080".to_string()),
        };
        assert_eq!(
            pick_proxy(&map, "http://api.deepseek.com/v1/chat", &[]).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            pick_proxy(&map, "https://opencode.ai/zen/go/v1", &[]).as_deref(),
            Some("http://127.0.0.1:7891")
        );
        // 非 http/https scheme 回退 socks
        assert_eq!(
            pick_proxy(&map, "ws://push.example.com", &[]).as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn test_pick_proxy_single_is_universal() {
        // 单一代理必须对 http/https 都返回同一地址（行为零变化）
        let map = single_proxy_map("http://127.0.0.1:7890");
        assert_eq!(
            pick_proxy(&map, "https://api.example.com", &[]).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            pick_proxy(&map, "http://api.example.com", &[]).as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn test_pick_proxy_honors_bypass() {
        let map = single_proxy_map("http://127.0.0.1:7890");
        let bypass = vec![".example.com".to_string()];
        assert_eq!(
            pick_proxy(&map, "https://local.example.com/x", &bypass),
            None
        );
        assert_eq!(
            pick_proxy(&map, "https://other.io/x", &bypass).as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn test_pick_proxy_unparsable_falls_back_to_representative() {
        let map = single_proxy_map("http://127.0.0.1:7890");
        assert_eq!(
            pick_proxy(&map, "not a url", &[]).as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn test_pick_proxy_empty_map_is_direct() {
        let map = system_proxy::ProxySchemeMap::default();
        assert_eq!(pick_proxy(&map, "https://api.example.com", &[]), None);
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
}
