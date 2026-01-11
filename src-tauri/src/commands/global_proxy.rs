//! 全局出站代理相关命令
//!
//! 提供获取、设置和测试全局代理的 Tauri 命令。

use crate::proxy::http_client;
use crate::store::AppState;
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

/// 获取全局代理 URL
///
/// 返回当前配置的代理 URL，null 表示直连。
#[tauri::command]
pub fn get_global_proxy_url(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    state.db.get_global_proxy_url().map_err(|e| e.to_string())
}

/// 设置全局代理 URL
///
/// - 传入非空字符串：启用代理
/// - 传入空字符串：清除代理（直连）
#[tauri::command]
pub fn set_global_proxy_url(state: tauri::State<'_, AppState>, url: String) -> Result<(), String> {
    let url_opt = if url.trim().is_empty() {
        None
    } else {
        Some(url.as_str())
    };

    // 1. 保存到数据库
    state
        .db
        .set_global_proxy_url(url_opt)
        .map_err(|e| e.to_string())?;

    // 2. 热更新全局 HTTP 客户端
    http_client::update_proxy(url_opt)?;

    log::info!(
        "[GlobalProxy] Configuration updated: {}",
        url_opt.unwrap_or("direct connection")
    );

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

    // 测试连接到 api.anthropic.com
    // 使用 HEAD 请求，即使返回 401 也说明网络通了
    let test_url = "https://api.anthropic.com";

    match client.head(test_url).send().await {
        Ok(resp) => {
            let latency = start.elapsed().as_millis() as u64;
            log::debug!(
                "[GlobalProxy] Test successful: {} -> {} ({}ms)",
                url,
                resp.status(),
                latency
            );
            Ok(ProxyTestResult {
                success: true,
                latency_ms: latency,
                error: None,
            })
        }
        Err(e) => {
            let latency = start.elapsed().as_millis() as u64;
            log::debug!("[GlobalProxy] Test failed: {url} -> {e} ({latency}ms)");
            Ok(ProxyTestResult {
                success: false,
                latency_ms: latency,
                error: Some(e.to_string()),
            })
        }
    }
}

/// 获取当前出站代理状态
///
/// 返回当前是否启用了出站代理以及代理 URL。
#[tauri::command]
pub fn get_upstream_proxy_status() -> UpstreamProxyStatus {
    let url = http_client::get_current_proxy_url();
    UpstreamProxyStatus {
        enabled: url.is_some(),
        proxy_url: url,
    }
}

/// 出站代理状态信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProxyStatus {
    /// 是否启用代理
    pub enabled: bool,
    /// 代理 URL
    pub proxy_url: Option<String>,
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
const PROXY_PORTS: &[(u16, &str)] = &[
    (7890, "http"),    // Clash
    (7891, "socks5"),  // Clash SOCKS
    (1080, "socks5"),  // 通用 SOCKS5
    (8080, "http"),    // 通用 HTTP
    (8888, "http"),    // Charles/Fiddler
    (3128, "http"),    // Squid
    (10808, "socks5"), // V2Ray
    (10809, "http"),   // V2Ray HTTP
];

/// 扫描本地代理
///
/// 检测常见端口是否有代理服务在运行。
#[tauri::command]
pub fn scan_local_proxies() -> Vec<DetectedProxy> {
    let mut found = Vec::new();

    for &(port, proxy_type) in PROXY_PORTS {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        if TcpStream::connect_timeout(&addr.into(), Duration::from_millis(100)).is_ok() {
            found.push(DetectedProxy {
                url: format!("{proxy_type}://127.0.0.1:{port}"),
                proxy_type: proxy_type.to_string(),
                port,
            });
        }
    }

    found
}
