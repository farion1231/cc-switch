//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// 路由状态快照：把「用于选择 transport 的 explicit proxy 元数据」与
/// 「对应的 following/pooled Client」放进同一个不可分割的快照。
///
/// 所有写者（init/apply_proxy/update_proxy）构建完整的新快照后整体替换 Arc，
/// 读者一次读取即拿到自洽的 (metadata, client) 组合——热更新期间不存在
/// 「旧元数据 + 新 client」或反向的撕裂配对（review finding P2-B）。
pub struct RouteState {
    /// 单调递增的发布序号（诊断与测试证据用）。
    generation: u64,
    /// 当前 proxy 配置对应的 following/pooled 客户端
    /// （非 forwarder 消费者使用；保留 reqwest 默认 redirect 行为）。
    client: Client,
    /// forwarder 专用客户端（禁用 reqwest 自动 redirect；redirect 由
    /// forwarder 统一显式状态机处理，跨越所有 hop 共享单一预算与 deadline）。
    route_client: Client,
    /// 当前配置的 explicit proxy URL；None = 直连/跟随系统代理。
    explicit_proxy_url: Option<String>,
}

static ROUTE_STATE: OnceCell<RwLock<Arc<RouteState>>> = OnceCell::new();

/// 路由状态发布序号计数器。
static ROUTE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

/// 全局直连 HTTP 客户端（永不使用任何代理，包括系统代理）。
/// 用于 loopback upstream 目标：本地目标必须直连，否则会被系统代理截获。
#[cfg(feature = "test-hooks")]
static DIRECT_CLIENT: OnceCell<Client> = OnceCell::new();

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

/// 构建新一代路由状态快照（尚未发布）。
fn build_route_state(proxy_url: Option<&str>) -> Result<Arc<RouteState>, String> {
    Ok(Arc::new(RouteState {
        generation: ROUTE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
        client: build_client(proxy_url)?,
        route_client: build_route_client(proxy_url)?,
        explicit_proxy_url: proxy_url.map(|s| s.to_string()),
    }))
}

/// 读取当前路由状态快照。一次读取即原子自洽：元数据与 client 来自同一代。
pub fn route_snapshot() -> Arc<RouteState> {
    let snapshot = ROUTE_STATE
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|guard| guard.clone());
    match snapshot {
        Some(state) => state,
        None => {
            log::warn!("[GlobalProxy] [GP-004] Route state not initialized, using fallback");
            Arc::new(RouteState {
                generation: 0,
                client: build_client(None).unwrap_or_default(),
                route_client: build_client_with(None, false, false).unwrap_or_default(),
                explicit_proxy_url: None,
            })
        }
    }
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
    let state = build_route_state(effective_url)?;

    // 尝试初始化路由状态，如果已存在则记录警告并使用 apply_proxy 更新
    #[cfg(feature = "test-hooks")]
    crate::proxy::test_hooks::record_publish(state.generation, state.explicit_proxy_url.as_deref());
    if ROUTE_STATE.set(RwLock::new(state)).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        // 已初始化，改用 apply_proxy 更新
        return apply_proxy(proxy_url);
    }

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
    let new_state = build_route_state(effective_url)?;

    // 发布：在写锁临界区内整体替换快照；读者要么看到旧快照要么看到新快照。
    let generation = new_state.generation;
    if let Some(lock) = ROUTE_STATE.get() {
        let mut guard = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        #[cfg(feature = "test-hooks")]
        crate::proxy::test_hooks::record_publish(
            new_state.generation,
            new_state.explicit_proxy_url.as_deref(),
        );
        *guard = new_state;
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
    }

    log::info!(
        "[GlobalProxy] Applied (gen {generation}): {}",
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
    let new_state = build_route_state(effective_url)?;

    // 发布：与 apply_proxy 相同——单锁临界区内整体替换快照。
    if let Some(lock) = ROUTE_STATE.get() {
        let mut guard = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        #[cfg(feature = "test-hooks")]
        crate::proxy::test_hooks::record_publish(
            new_state.generation,
            new_state.explicit_proxy_url.as_deref(),
        );
        *guard = new_state;
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
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
    route_snapshot().client.clone()
}

/// 获取全局直连 HTTP 客户端（永不使用任何代理，包括系统代理/环境变量代理）。
///
/// 用于 loopback upstream 目标：本地环回地址必须直连，
/// 避免被系统代理（如 ClashX / v2rayN）截获导致请求无法到达本地服务。
#[cfg(feature = "test-hooks")]
pub fn get_direct() -> Client {
    DIRECT_CLIENT
        .get_or_init(|| {
            build_client_with(None, true, true).unwrap_or_else(|e| {
                log::warn!(
                    "[GlobalProxy] [GP-009] Direct client build failed, using fallback: {e}"
                );
                Client::builder().no_proxy().build().unwrap_or_default()
            })
        })
        .clone()
}

/// forwarder 直连路由客户端：无代理 + 禁用自动 redirect（redirect 由
/// forwarder 显式状态机处理；见 `resolve_route_for_url`）。
static DIRECT_ROUTE_CLIENT: OnceCell<Client> = OnceCell::new();

fn get_direct_route_client() -> Client {
    DIRECT_ROUTE_CLIENT
        .get_or_init(|| {
            build_client_with(None, true, false).unwrap_or_else(|e| {
                log::warn!(
                    "[GlobalProxy] [GP-010] Direct route client build failed, using fallback: {e}"
                );
                Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap_or_default()
            })
        })
        .clone()
}

/// 判断最终 upstream URL 的 host 是否为 loopback 目标。
///
/// 使用标准 URL/IP 解析（url crate + IpAddr::is_loopback），不做字符串前缀粗判：
/// - localhost（大小写不敏感）
/// - IPv4 loopback（127.0.0.0/8，例如 127.0.0.1）
/// - IPv6 loopback（::1）
pub fn is_loopback_upstream_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// 判断给定 upstream 目标是否必须使用「直连客户端」。
///
/// 仅当未配置用户显式 upstream 代理（explicit_proxy_url == None）且目标为 loopback
/// 时返回 true：
/// - loopback 目标不得继承系统代理（系统代理会截获回环请求）；
/// - 用户显式配置的代理语义保持不变（存在显式代理时始终返回 false）。
pub fn should_direct_connect_to(url: &str, explicit_proxy_url: Option<&str>) -> bool {
    explicit_proxy_url.is_none() && is_loopback_upstream_url(url)
}

/// 单个 hop 的显式代理形态（由同一快照的 explicit proxy URL 派生）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteProxyKind {
    /// 无 explicit proxy：直连 / 跟随系统代理。
    None,
    /// explicit HTTP(S) 代理。
    Http,
    /// explicit SOCKS5 代理。
    Socks5,
}

/// 单个 hop 的路由决策：所有字段来自**同一次**原子快照读取。
///
/// 同一个 hop 内 reqwest/hyper 分支判断、SOCKS/HTTP/direct 判断、
/// explicit proxy 元数据、实际使用的 Client 与 loopback-direct 决策不得来自
/// 不同世代（review finding MAJOR-2：禁止「先读 metadata、再独立选 client」）。
/// redirect 到下一 hop 时允许重新解析一个新的原子决策（proxy 配置可能在两个
/// hop 之间改变）。ONE HOP = ONE COHERENT ROUTE SNAPSHOT。
#[derive(Clone)]
pub struct RouteDecision {
    /// 本次决策所用快照的发布序号。
    pub generation: u64,
    /// 与 client 同源的 explicit proxy URL（None = 无 explicit proxy）。
    pub explicit_proxy_url: Option<String>,
    /// explicit proxy 形态（与 explicit_proxy_url 同源派生）。
    pub proxy_kind: RouteProxyKind,
    /// 本目标在此快照下是否应直连（loopback 且无 explicit proxy）。
    pub loopback_direct: bool,
    /// 本 hop 应使用的 forwarder 客户端（无自动 redirect）。
    pub client: Client,
}

/// 代理形态分类：必须与 `build_client_with` 接受 URL 的方式一致（解析后的
/// scheme，大小写不敏感；SOCKS 族含 socks4/4a/5/5h——hyper 的 HTTP-CONNECT
/// 无法表达任何一种 SOCKS URL，必须统一走 reqwest 客户端）。
fn route_proxy_kind(explicit_proxy_url: Option<&str>) -> RouteProxyKind {
    match explicit_proxy_url {
        None => RouteProxyKind::None,
        Some(url) => match reqwest::Url::parse(url) {
            Ok(parsed) if parsed.scheme().to_ascii_lowercase().starts_with("socks") => {
                RouteProxyKind::Socks5
            }
            _ => RouteProxyKind::Http,
        },
    }
}

/// 解析该 upstream 目标的一个原子路由决策（ONE HOP = ONE SNAPSHOT）。
///
/// 元数据（explicit proxy / proxy kind / loopback-direct）与对应 Client 来自
/// 同一次 `route_snapshot()` 读取，热更新期间不存在撕裂配对。
pub fn resolve_route_for_url(url: &str) -> RouteDecision {
    let snapshot = route_snapshot();
    #[cfg(feature = "test-hooks")]
    crate::proxy::test_hooks::pause_point();
    let loopback_direct = should_direct_connect_to(url, snapshot.explicit_proxy_url.as_deref());
    let proxy_kind = route_proxy_kind(snapshot.explicit_proxy_url.as_deref());
    #[cfg(feature = "test-hooks")]
    crate::proxy::test_hooks::record_resolution(
        snapshot.generation,
        loopback_direct,
        is_loopback_upstream_url(url),
    );
    let client = if loopback_direct {
        log::debug!(
            "[GlobalProxy] Loopback upstream detected; using direct client (inherited proxy bypassed)"
        );
        get_direct_route_client()
    } else {
        snapshot.route_client.clone()
    };
    RouteDecision {
        generation: snapshot.generation,
        explicit_proxy_url: snapshot.explicit_proxy_url.clone(),
        proxy_kind,
        loopback_direct,
        client,
    }
}

/// 测试证据入口（test-hooks）：返回 (是否选择直连, 所用快照的发布序号)。
///
/// 与 `resolve_route_for_url` 使用同一次快照读取，供并发/原子性测试验证
/// 「决策必须与其快照世代的发布内容一致」。
#[cfg(feature = "test-hooks")]
#[allow(dead_code)]
pub fn select_for_test(url: &str) -> (bool, u64) {
    let snapshot = route_snapshot();
    crate::proxy::test_hooks::pause_point();
    let direct = should_direct_connect_to(url, snapshot.explicit_proxy_url.as_deref());
    (direct, snapshot.generation)
}

/// 获取当前代理 URL
///
/// 返回当前配置的代理 URL，None 表示直连。
pub fn get_current_proxy_url() -> Option<String> {
    route_snapshot().explicit_proxy_url.clone()
}

/// 检查是否正在使用代理
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

/// 构建 HTTP 客户端（非 forwarder 消费者；保留 reqwest 默认 redirect 行为）。
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_with(proxy_url, false, true)
}

/// 构建 forwarder 路由客户端：禁用 reqwest 自动 redirect。
///
/// forwarder 的 redirect 由统一显式状态机处理（跨越 automatic/manual 边界
/// 共享同一 logical budget 与 request-level deadline），客户端本身绝不自动
/// 跟随 redirect（review finding MAJOR-1 的单一预算语义）。
fn build_route_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_with(proxy_url, false, false)
}

/// 构建 HTTP 客户端（force_direct = true 时禁用一切代理）。
///
/// force_direct 为 true 时客户端永不使用任何代理（包括系统代理/环境变量代理），
/// 用于 loopback upstream：本地目标必须直连，避免被系统代理截获。
fn build_client_with(
    proxy_url: Option<&str>,
    force_direct: bool,
    follow_redirects: bool,
) -> Result<Client, String> {
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
    if !follow_redirects {
        // forwarder 路径：redirect 由统一显式状态机处理（单一 logical budget /
        // request-level deadline），客户端绝不自动跟随。
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }

    if force_direct {
        // 直连语义：显式禁用一切代理，不进入下方的代理选择逻辑。
        builder = builder.no_proxy();
        log::debug!("[GlobalProxy] Direct client built (all proxies disabled)");
        return builder
            .build()
            .map_err(|e| format!("Failed to build direct HTTP client: {e}"));
    }

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
    use std::sync::{Arc, Mutex, OnceLock};

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

    const OPENAI_CHAT_BODY: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":0,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;

    /// 启动一个记录请求的 mock 服务器（可充当 mock 上游或 mock 代理），固定返回 200 + OPENAI_CHAT_BODY。
    async fn spawn_recording_server(
        record: Arc<Mutex<Vec<String>>>,
        body: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let record = record.clone();
            async move {
                record
                    .lock()
                    .unwrap()
                    .push(format!("{} {}", req.method(), req.uri()));
                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    body,
                )
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, handle)
    }

    #[test]
    fn test_is_loopback_upstream_url_classification() {
        // 1) localhost（大小写不敏感）
        assert!(is_loopback_upstream_url(
            "http://localhost:8080/v1/chat/completions"
        ));
        assert!(is_loopback_upstream_url("http://LOCALHOST/v1"));
        assert!(is_loopback_upstream_url("http://LocalHost/v1"));
        // 2) 127.0.0.1
        assert!(is_loopback_upstream_url("http://127.0.0.1:8080/v1"));
        // 3) 任意 127/8 地址
        assert!(is_loopback_upstream_url("http://127.8.8.8:9/v1"));
        assert!(is_loopback_upstream_url("http://127.255.255.254/v1"));
        // 4) IPv6 ::1
        assert!(is_loopback_upstream_url("http://[::1]:8080/v1"));
        // 5) 公网/私网 IPv4 不是 loopback
        assert!(!is_loopback_upstream_url("https://8.8.8.8/v1"));
        assert!(!is_loopback_upstream_url("https://192.168.1.10:7890/v1"));
        assert!(!is_loopback_upstream_url("https://10.0.0.2/v1"));
        // 6) 普通主机名不是 loopback
        assert!(!is_loopback_upstream_url("https://api.openai.com/v1"));
        assert!(!is_loopback_upstream_url("https://localhost.evil.com/v1"));
        // 非法输入保守返回 false
        assert!(!is_loopback_upstream_url("not a url"));
    }

    #[test]
    fn test_should_direct_connect_to_policy() {
        // 8) loopback + 无显式代理 → 直连（不继承系统代理）
        assert!(should_direct_connect_to("http://127.0.0.1:8080/v1", None));
        assert!(should_direct_connect_to("http://localhost:8080/v1", None));
        assert!(should_direct_connect_to("http://[::1]:8080/v1", None));
        // 9) 外部目标 + 无显式代理 → 保持既有语义（跟随系统/继承代理）
        assert!(!should_direct_connect_to("https://api.openai.com/v1", None));
        assert!(!should_direct_connect_to("http://192.0.2.1/v1", None));
        // 10) loopback + 显式代理 → 保持显式代理语义
        assert!(!should_direct_connect_to(
            "http://127.0.0.1:8080/v1",
            Some("http://127.0.0.1:7890")
        ));
        // 11) 外部 + 显式代理 → 保持显式代理语义
        assert!(!should_direct_connect_to(
            "https://api.openai.com/v1",
            Some("socks5://127.0.0.1:1080")
        ));
    }

    /// 12) loopback 目标不得被「继承代理」（环境变量/系统代理）截获：
    /// 直连客户端在继承代理存在时仍必须直连到本地服务器。
    #[tokio::test]
    async fn test_direct_client_ignores_inherited_env_proxy_for_loopback_target() {
        let _guard = env_lock().lock().unwrap();
        let prev_http = env::var("HTTP_PROXY").ok();
        let prev_http_lower = env::var("http_proxy").ok();

        let upstream_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (upstream_addr, upstream_handle) =
            spawn_recording_server(upstream_rec.clone(), OPENAI_CHAT_BODY).await;
        let proxy_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (proxy_addr, proxy_handle) =
            spawn_recording_server(proxy_rec.clone(), OPENAI_CHAT_BODY).await;

        env::set_var(
            "HTTP_PROXY",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        );
        env::set_var(
            "http_proxy",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        );

        let direct = get_direct();
        let result = direct
            .get(format!("http://127.0.0.1:{}/", upstream_addr.port()))
            .send()
            .await;

        match prev_http {
            Some(v) => env::set_var("HTTP_PROXY", v),
            None => env::remove_var("HTTP_PROXY"),
        }
        match prev_http_lower {
            Some(v) => env::set_var("http_proxy", v),
            None => env::remove_var("http_proxy"),
        }

        let resp = result.expect("direct client must bypass inherited proxy");
        assert_eq!(resp.status(), 200);
        assert_eq!(
            upstream_rec.lock().unwrap().len(),
            1,
            "loopback target must be reached directly"
        );
        assert_eq!(
            proxy_rec.lock().unwrap().len(),
            0,
            "inherited proxy must not be used for a loopback target"
        );
        upstream_handle.abort();
        proxy_handle.abort();
    }

    /// 13) 未配置显式代理时，外部目标仍遵循继承代理（既有语义不变）。
    #[tokio::test]
    async fn test_following_client_still_uses_inherited_env_proxy_for_external_target() {
        let _guard = env_lock().lock().unwrap();
        let prev_http = env::var("HTTP_PROXY").ok();
        let prev_http_lower = env::var("http_proxy").ok();

        let proxy_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (proxy_addr, proxy_handle) =
            spawn_recording_server(proxy_rec.clone(), OPENAI_CHAT_BODY).await;

        env::set_var(
            "HTTP_PROXY",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        );
        env::set_var(
            "http_proxy",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        );

        let following = build_client(None).expect("build following client");
        // 非 loopback 目标（TEST-NET-1 不可路由；由 mock 代理直接应答，无需真实网络）
        let result = following.get("http://192.0.2.1/v1").send().await;

        match prev_http {
            Some(v) => env::set_var("HTTP_PROXY", v),
            None => env::remove_var("HTTP_PROXY"),
        }
        match prev_http_lower {
            Some(v) => env::set_var("http_proxy", v),
            None => env::remove_var("http_proxy"),
        }

        let resp = result.expect("external request should go through the inherited proxy");
        assert_eq!(resp.status(), 200);
        let rec = proxy_rec.lock().unwrap().clone();
        assert_eq!(
            rec.len(),
            1,
            "external target must keep following the inherited proxy"
        );
        assert!(rec[0].contains("192.0.2.1"), "{}", rec[0]);
        proxy_handle.abort();
    }

    /// 10/11) 显式代理语义保持：loopback 与外部目标都经显式代理，不被本 patch 静默覆盖。
    #[tokio::test]
    async fn test_explicit_proxy_is_used_for_loopback_and_external_targets() {
        let proxy_rec: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (proxy_addr, proxy_handle) =
            spawn_recording_server(proxy_rec.clone(), OPENAI_CHAT_BODY).await;

        let proxy_url = format!("http://127.0.0.1:{}", proxy_addr.port());
        let client = build_client(Some(proxy_url.as_str())).expect("build explicit-proxy client");

        // loopback 目标：不得被静默改为直连（目标端口无需真实监听——由显式代理应答）
        let resp = client
            .get("http://127.0.0.1:9/loopback/check")
            .send()
            .await
            .expect("request via explicit proxy");
        assert_eq!(resp.status(), 200);
        // 外部目标：同样遵循显式代理
        let resp2 = client
            .get("http://192.0.2.1/external/check")
            .send()
            .await
            .expect("external request via explicit proxy");
        assert_eq!(resp2.status(), 200);

        let rec = proxy_rec.lock().unwrap().clone();
        assert_eq!(
            rec.len(),
            2,
            "both requests must go through the explicit proxy"
        );
        assert!(rec[0].contains("127.0.0.1:9/loopback/check"), "{}", rec[0]);
        assert!(rec[1].contains("192.0.2.1/external/check"), "{}", rec[1]);
        proxy_handle.abort();
    }
}
