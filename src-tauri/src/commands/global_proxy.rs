//! 全局出站代理相关命令
//!
//! 提供获取、设置和测试全局代理的 Tauri 命令。

use crate::proxy::http_client::{self, SystemProxySource};
use crate::store::AppState;
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 获取全局代理 URL
///
/// 返回当前配置的代理 URL，null 表示直连。
#[tauri::command]
pub fn get_global_proxy_url(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let result = state.db.get_global_proxy_url().map_err(|e| e.to_string())?;
    log::debug!(
        "[GlobalProxy] [GP-010] Read from database: {}",
        result
            .as_ref()
            .map(|u| http_client::mask_url(u))
            .unwrap_or_else(|| "None".to_string())
    );
    Ok(result)
}

/// 设置全局代理 URL
///
/// - 传入非空字符串：启用代理
/// - 传入空字符串：清除代理（直连）
///
/// 执行顺序：先验证 → 写 DB → 再应用
/// 这样确保 DB 写失败时不会出现运行态与持久化不一致的问题
#[tauri::command]
pub fn set_global_proxy_url(state: tauri::State<'_, AppState>, url: String) -> Result<(), String> {
    // 调试：显示接收到的 URL 信息（不包含敏感内容）
    let has_auth = url.contains('@') && (url.starts_with("http://") || url.starts_with("socks"));
    log::debug!(
        "[GlobalProxy] [GP-011] Received URL: length={}, has_auth={}",
        url.len(),
        has_auth
    );

    let url_opt = if url.trim().is_empty() {
        None
    } else {
        Some(url.as_str())
    };

    // 1. 先验证代理配置是否有效（不应用）
    http_client::validate_proxy(url_opt)?;

    // 2. 验证成功后保存到数据库
    state
        .db
        .set_global_proxy_url(url_opt)
        .map_err(|e| e.to_string())?;

    // 3. DB 写入成功后再应用到运行态
    http_client::apply_proxy(url_opt)?;

    log::info!(
        "[GlobalProxy] [GP-009] Configuration updated: {}",
        url_opt
            .map(http_client::mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 获取「跟随系统代理」开关
#[tauri::command]
pub fn get_follow_system_proxy(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state
        .db
        .get_follow_system_proxy()
        .map_err(|e| e.to_string())
}

/// 设置「跟随系统代理」开关
///
/// 与 set_global_proxy_url 同样的顺序：先写 DB，再应用到运行态
#[tauri::command]
pub fn set_follow_system_proxy(
    state: tauri::State<'_, AppState>,
    follow: bool,
) -> Result<(), String> {
    state
        .db
        .set_follow_system_proxy(follow)
        .map_err(|e| e.to_string())?;
    http_client::set_follow_system_proxy(follow)?;
    log::info!("[GlobalProxy] Follow system proxy: {follow}");
    Ok(())
}

/// 代理测试结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    /// 是否连接成功
    pub success: bool,
    /// 延迟（毫秒）
    pub latency_ms: u64,
    /// 错误信息
    pub error: Option<String>,
}

/// 测试代理连接
///
/// 通过指定的代理 URL 发送测试请求，返回连接结果和延迟。
/// 使用多个测试目标，任一成功即认为代理可用。
#[tauri::command]
pub async fn test_proxy_url(url: String) -> Result<ProxyTestResult, String> {
    if url.trim().is_empty() {
        return Err("Proxy URL is empty".to_string());
    }

    let start = Instant::now();

    // 构建带代理的临时客户端
    let proxy = reqwest::Proxy::all(&url).map_err(|e| format!("Invalid proxy URL: {e}"))?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build client: {e}"))?;

    // 使用多个测试目标，提高兼容性
    // 优先使用 httpbin（专门用于 HTTP 测试），回退到其他公共端点
    let test_urls = [
        "https://httpbin.org/get",
        "https://www.google.com",
        "https://api.anthropic.com",
    ];

    let mut last_error = None;

    for test_url in test_urls {
        match client.head(test_url).send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as u64;
                log::debug!(
                    "[GlobalProxy] Test successful: {} -> {} via {} ({}ms)",
                    http_client::mask_url(&url),
                    test_url,
                    resp.status(),
                    latency
                );
                return Ok(ProxyTestResult {
                    success: true,
                    latency_ms: latency,
                    error: None,
                });
            }
            Err(e) => {
                log::debug!("[GlobalProxy] Test to {test_url} failed: {e}");
                last_error = Some(e);
            }
        }
    }

    // 所有测试目标都失败
    let latency = start.elapsed().as_millis() as u64;
    let error_msg = last_error
        .map(|e| e.to_string())
        .unwrap_or_else(|| "All test targets failed".to_string());

    log::debug!(
        "[GlobalProxy] Test failed: {} -> {} ({}ms)",
        http_client::mask_url(&url),
        error_msg,
        latency
    );

    Ok(ProxyTestResult {
        success: false,
        latency_ms: latency,
        error: Some(error_msg),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutboundProxyMode {
    Explicit,
    System,
    Direct,
}

/// 出站代理状态信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProxyStatus {
    /// 是否配置了显式代理
    pub enabled: bool,
    /// 显式代理 URL
    pub proxy_url: Option<String>,
    pub follow_system_proxy: bool,
    pub mode: OutboundProxyMode,
    /// 当前客户端烘焙的系统代理（已脱敏）；None = 客户端未跟随任何系统代理
    pub system_proxy_url: Option<String>,
    /// 烘焙那一刻记下的来源，与 `system_proxy_url` 同期，不是实时重解析的结果
    pub system_proxy_source: Option<SystemProxySource>,
    /// 对烘焙代理端口的一次 TCP 探测；无法解析主机端口时为 None
    pub system_proxy_reachable: Option<bool>,
    /// 系统代理设置在客户端构建之后发生了变化，需重新应用才生效
    pub system_proxy_changed: bool,
    pub current_system_proxy_url: Option<String>,
}

/// 获取当前出站代理状态（含系统代理来源、可达性与是否已变化）
#[tauri::command]
pub async fn get_upstream_proxy_status() -> Result<UpstreamProxyStatus, String> {
    // TCP 探测与系统配置读取都是阻塞调用，不能放在主线程
    tokio::task::spawn_blocking(collect_upstream_proxy_status)
        .await
        .map_err(|e| format!("Failed to collect proxy status: {e}"))
}

fn collect_upstream_proxy_status() -> UpstreamProxyStatus {
    let explicit = http_client::get_current_proxy_url();
    let follow = http_client::follow_system_proxy();
    let mode = outbound_proxy_mode(explicit.as_deref(), follow);
    let (baked, (current, changed)) = if mode == OutboundProxyMode::System {
        (
            http_client::baked_system_proxy(),
            // 展示的「当前值」与「是否已变化」出自后端同一次解析，二者不会对不上
            http_client::system_proxy_status(),
        )
    } else {
        (None, (None, false))
    };
    let baked_url = baked.as_ref().map(|(url, _)| url.as_str());

    UpstreamProxyStatus {
        enabled: explicit.is_some(),
        proxy_url: explicit,
        follow_system_proxy: follow,
        mode,
        system_proxy_url: baked_url.map(http_client::mask_url),
        system_proxy_source: baked.as_ref().map(|(_, source)| *source),
        system_proxy_reachable: baked_url.and_then(proxy_reachable),
        system_proxy_changed: changed,
        current_system_proxy_url: current.as_deref().map(http_client::mask_url),
    }
}

pub(crate) fn outbound_proxy_mode(
    explicit: Option<&str>,
    follow_system_proxy: bool,
) -> OutboundProxyMode {
    if explicit.is_some() {
        OutboundProxyMode::Explicit
    } else if follow_system_proxy {
        OutboundProxyMode::System
    } else {
        OutboundProxyMode::Direct
    }
}

/// 解析不出主机和端口（如无端口的 socks5 地址）就不下结论，避免把「不知道」显示成「不可达」
fn proxy_reachable(proxy_url: &str) -> Option<bool> {
    let parsed = url::Url::parse(proxy_url).ok()?;
    let port = parsed.port_or_known_default()?;
    // 按 Host 分支而不是把 host_str() 交给 to_socket_addrs：IPv6 字面量的 host_str 带着方括号，
    // 解析器只会拿它当主机名去查 DNS 并失败，可达性就退化成「不知道」
    let addrs: Vec<SocketAddr> = match parsed.host()? {
        url::Host::Ipv4(ip) => vec![SocketAddr::from((ip, port))],
        url::Host::Ipv6(ip) => vec![SocketAddr::from((ip, port))],
        // 主机名可能解析出多个地址（localhost 常见 ::1 与 127.0.0.1 并存），只要任一地址
        // 能连上就算可达，否则只监听 IPv4 的代理会被误报为不可达
        url::Host::Domain(name) => (name, port).to_socket_addrs().ok()?.collect(),
    };
    if addrs.is_empty() {
        return None;
    }
    Some(
        addrs
            .iter()
            .any(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(500)).is_ok()),
    )
}

/// 检测到的代理信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProxy {
    /// 代理 URL
    pub url: String,
    /// 代理类型 (http/socks5)
    pub proxy_type: String,
    /// 端口
    pub port: u16,
}

/// 常见代理端口配置
/// 格式：(端口, 主要类型, 是否同时支持 http 和 socks5)
/// 对于 mixed 端口，会同时返回两种协议供用户选择
const PROXY_PORTS: &[(u16, &str, bool)] = &[
    (7890, "http", true),     // Clash (mixed mode)
    (7891, "socks5", false),  // Clash SOCKS only
    (1080, "socks5", false),  // 通用 SOCKS5
    (8080, "http", false),    // 通用 HTTP
    (8888, "http", false),    // Charles/Fiddler
    (3128, "http", false),    // Squid
    (10808, "socks5", false), // V2Ray SOCKS
    (10809, "http", false),   // V2Ray HTTP
];

/// 扫描本地代理
///
/// 检测常见端口是否有代理服务在运行。
/// 使用异步任务避免阻塞 UI 线程。
#[tauri::command]
pub async fn scan_local_proxies() -> Vec<DetectedProxy> {
    // 使用 spawn_blocking 避免阻塞主线程
    tokio::task::spawn_blocking(|| {
        let mut found = Vec::new();

        for &(port, primary_type, is_mixed) in PROXY_PORTS {
            let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
            if TcpStream::connect_timeout(&addr.into(), Duration::from_millis(100)).is_ok() {
                // 添加主要类型
                found.push(DetectedProxy {
                    url: format!("{primary_type}://127.0.0.1:{port}"),
                    proxy_type: primary_type.to_string(),
                    port,
                });
                // 对于 mixed 端口，同时添加另一种协议
                if is_mixed {
                    let alt_type = if primary_type == "http" {
                        "socks5"
                    } else {
                        "http"
                    };
                    found.push(DetectedProxy {
                        url: format!("{alt_type}://127.0.0.1:{port}"),
                        proxy_type: alt_type.to_string(),
                        port,
                    });
                }
            }
        }

        found
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_prefers_explicit_then_follow_flag() {
        assert_eq!(
            outbound_proxy_mode(Some("http://127.0.0.1:7890"), false),
            OutboundProxyMode::Explicit
        );
        assert_eq!(outbound_proxy_mode(None, true), OutboundProxyMode::System);
        assert_eq!(outbound_proxy_mode(None, false), OutboundProxyMode::Direct);
    }

    #[test]
    fn proxy_reachable_probes_tcp_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let live = format!("http://{}", listener.local_addr().unwrap());
        assert_eq!(proxy_reachable(&live), Some(true));

        let dead_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(
            proxy_reachable(&format!("http://127.0.0.1:{dead_port}")),
            Some(false)
        );

        assert_eq!(proxy_reachable("not a url"), None);
        // socks5 没有众所周知的默认端口，缺端口时不下结论
        assert_eq!(proxy_reachable("socks5://127.0.0.1"), None);
    }

    #[test]
    fn proxy_reachable_accepts_any_resolved_address() {
        // localhost 通常同时解析出 ::1 与 127.0.0.1；代理只监听 IPv4 也必须判为可达
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(
            proxy_reachable(&format!("http://localhost:{port}")),
            Some(true)
        );

        let dead_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(
            proxy_reachable(&format!("http://localhost:{dead_port}")),
            Some(false)
        );
    }

    #[test]
    fn proxy_reachable_probes_ipv6_literal_hosts() {
        let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
        let live = format!("http://{}", listener.local_addr().unwrap());
        assert_eq!(proxy_reachable(&live), Some(true));

        let dead_port = std::net::TcpListener::bind("[::1]:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(
            proxy_reachable(&format!("http://[::1]:{dead_port}")),
            Some(false)
        );
    }
}
