//! 全局出站代理相关命令
//!
//! 提供获取、设置和测试全局代理的 Tauri 命令。

use crate::proxy::http_client;
use crate::store::AppState;
use crate::{database::Database, error::AppError};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const DEFAULT_WATCHDOG_PROXY_URL: &str = "http://127.0.0.1:7890";
const WATCHDOG_INTERVAL_SECS: u64 = 30;
const WATCHDOG_TEST_URLS: &[&str] = &[
    "https://www.gstatic.com/generate_204",
    "https://www.google.com/generate_204",
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://www.bing.com/",
];

static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_RUNTIME: Lazy<Mutex<ProxyWatchdogRuntime>> =
    Lazy::new(|| Mutex::new(ProxyWatchdogRuntime::default()));

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProxyWatchdogMode {
    Auto,
    ManualOn,
    ManualOff,
}

impl ProxyWatchdogMode {
    fn as_setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ManualOn => "manual_on",
            Self::ManualOff => "manual_off",
        }
    }

    fn from_setting(value: &str) -> Self {
        match value {
            "auto" => Self::Auto,
            "manual_off" | "manualOff" | "off" => Self::ManualOff,
            _ => Self::ManualOn,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyWatchdogConfig {
    pub mode: ProxyWatchdogMode,
    pub proxy_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyWatchdogStatus {
    pub config: ProxyWatchdogConfig,
    pub effective_proxy_url: Option<String>,
    pub is_proxying: bool,
    pub last_probe_success: Option<bool>,
    pub last_checked_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ProxyWatchdogRuntime {
    last_probe_success: Option<bool>,
    last_checked_at: Option<String>,
    last_error: Option<String>,
}

fn read_watchdog_config(db: &Database) -> Result<ProxyWatchdogConfig, AppError> {
    let mode = ProxyWatchdogMode::from_setting(&db.get_proxy_watchdog_mode()?);
    let proxy_url = db
        .get_proxy_watchdog_proxy_url()?
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WATCHDOG_PROXY_URL.to_string());

    Ok(ProxyWatchdogConfig { mode, proxy_url })
}

fn write_watchdog_config(db: &Database, config: &ProxyWatchdogConfig) -> Result<(), AppError> {
    db.set_proxy_watchdog_mode(config.mode.as_setting())?;
    db.set_proxy_watchdog_proxy_url(Some(&config.proxy_url))?;
    Ok(())
}

fn runtime_snapshot() -> ProxyWatchdogRuntime {
    WATCHDOG_RUNTIME
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

fn update_runtime(success: Option<bool>, error: Option<String>) {
    if let Ok(mut guard) = WATCHDOG_RUNTIME.lock() {
        guard.last_probe_success = success;
        guard.last_error = error;
        guard.last_checked_at = Some(chrono::Utc::now().to_rfc3339());
    }
}

fn make_watchdog_status(db: &Database) -> Result<ProxyWatchdogStatus, AppError> {
    let config = read_watchdog_config(db)?;
    let runtime = runtime_snapshot();
    let effective_proxy_url = db.get_global_proxy_url()?;
    let is_proxying = effective_proxy_url.is_some();
    Ok(ProxyWatchdogStatus {
        config,
        effective_proxy_url,
        is_proxying,
        last_probe_success: runtime.last_probe_success,
        last_checked_at: runtime.last_checked_at,
        last_error: runtime.last_error,
    })
}

fn apply_effective_proxy(db: &Database, proxy_url: Option<&str>) -> Result<(), String> {
    db.set_global_proxy_url(proxy_url)
        .map_err(|e| e.to_string())?;
    http_client::apply_proxy(proxy_url)
}

async fn request_path_usable(client: &reqwest::Client, url: &str) -> Result<bool, String> {
    match client.get(url).send().await {
        Ok(resp) => Ok(resp.status().as_u16() < 500),
        Err(e) => Err(e.to_string()),
    }
}

async fn probe_watchdog_proxy(proxy_url: &str) -> (Option<bool>, Option<String>) {
    let proxy = match reqwest::Proxy::all(proxy_url) {
        Ok(proxy) => proxy,
        Err(e) => return (Some(false), Some(format!("Invalid proxy URL: {e}"))),
    };

    let proxied_client = match reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return (
                Some(false),
                Some(format!("Failed to build proxy client: {e}")),
            )
        }
    };

    let direct_client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(e) => return (None, Some(format!("Failed to build direct client: {e}"))),
    };

    let mut saw_direct_success = false;
    let mut errors = Vec::new();

    for url in WATCHDOG_TEST_URLS {
        match request_path_usable(&proxied_client, url).await {
            Ok(true) => return (Some(true), None),
            Ok(false) => errors.push(format!("{url} via proxy returned 5xx")),
            Err(e) => errors.push(format!("{url} via proxy failed: {e}")),
        }

        if let Ok(true) = request_path_usable(&direct_client, url).await {
            saw_direct_success = true;
        }
    }

    if saw_direct_success {
        (Some(false), Some(errors.join(" | ")))
    } else {
        (None, Some(errors.join(" | ")))
    }
}

async fn apply_watchdog_config(
    db: Arc<Database>,
    config: ProxyWatchdogConfig,
) -> Result<(), String> {
    match config.mode {
        ProxyWatchdogMode::ManualOn => {
            http_client::validate_proxy(Some(&config.proxy_url))?;
            apply_effective_proxy(&db, Some(&config.proxy_url))?;
            update_runtime(None, Some("manual_on".to_string()));
        }
        ProxyWatchdogMode::ManualOff => {
            apply_effective_proxy(&db, None)?;
            update_runtime(None, Some("manual_off".to_string()));
        }
        ProxyWatchdogMode::Auto => {
            run_watchdog_step(db).await?;
        }
    }
    Ok(())
}

async fn run_watchdog_step(db: Arc<Database>) -> Result<(), String> {
    let config = read_watchdog_config(&db).map_err(|e| e.to_string())?;
    if config.mode != ProxyWatchdogMode::Auto {
        return Ok(());
    }

    let (probe, error) = probe_watchdog_proxy(&config.proxy_url).await;
    update_runtime(probe, error.clone());

    match probe {
        Some(true) => {
            if db
                .get_global_proxy_url()
                .map_err(|e| e.to_string())?
                .as_deref()
                != Some(config.proxy_url.as_str())
            {
                apply_effective_proxy(&db, Some(&config.proxy_url))?;
                log::info!(
                    "[ProxyWatchdog] proxy usable; enabled {}",
                    http_client::mask_url(&config.proxy_url)
                );
            }
        }
        Some(false) => {
            if db
                .get_global_proxy_url()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                apply_effective_proxy(&db, None)?;
                log::info!("[ProxyWatchdog] proxy unavailable; switched to direct connection");
            }
        }
        None => {
            log::warn!(
                "[ProxyWatchdog] proxy state unknown; keeping current global proxy. {}",
                error.unwrap_or_else(|| "no details".to_string())
            );
        }
    }

    Ok(())
}

pub fn start_proxy_watchdog(db: Arc<Database>) {
    if WATCHDOG_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        log::info!("[ProxyWatchdog] started");
        let mut interval = tokio::time::interval(Duration::from_secs(WATCHDOG_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(e) = run_watchdog_step(db.clone()).await {
                update_runtime(None, Some(e.clone()));
                log::warn!("[ProxyWatchdog] step failed: {e}");
            }
        }
    });
}

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

    let mode = if url_opt.is_some() {
        ProxyWatchdogMode::ManualOn
    } else {
        ProxyWatchdogMode::ManualOff
    };
    state
        .db
        .set_proxy_watchdog_mode(mode.as_setting())
        .map_err(|e| e.to_string())?;
    if let Some(url) = url_opt {
        state
            .db
            .set_proxy_watchdog_proxy_url(Some(url))
            .map_err(|e| e.to_string())?;
    }

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

#[tauri::command]
pub fn get_proxy_watchdog_config(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyWatchdogConfig, String> {
    read_watchdog_config(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_proxy_watchdog_config(
    state: tauri::State<'_, AppState>,
    config: ProxyWatchdogConfig,
) -> Result<ProxyWatchdogStatus, String> {
    if config.proxy_url.trim().is_empty() && config.mode != ProxyWatchdogMode::ManualOff {
        return Err("Proxy URL is required unless manual off is selected".to_string());
    }

    let normalized = ProxyWatchdogConfig {
        mode: config.mode,
        proxy_url: if config.proxy_url.trim().is_empty() {
            DEFAULT_WATCHDOG_PROXY_URL.to_string()
        } else {
            config.proxy_url.trim().to_string()
        },
    };

    if normalized.mode != ProxyWatchdogMode::ManualOff {
        http_client::validate_proxy(Some(&normalized.proxy_url))?;
    }

    write_watchdog_config(&state.db, &normalized).map_err(|e| e.to_string())?;
    apply_watchdog_config(state.db.clone(), normalized).await?;
    make_watchdog_status(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_proxy_watchdog_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyWatchdogStatus, String> {
    make_watchdog_status(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_proxy_watchdog(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyWatchdogStatus, String> {
    let config = read_watchdog_config(&state.db).map_err(|e| e.to_string())?;
    if config.mode != ProxyWatchdogMode::Auto {
        return make_watchdog_status(&state.db).map_err(|e| e.to_string());
    }
    apply_watchdog_config(state.db.clone(), config).await?;
    make_watchdog_status(&state.db).map_err(|e| e.to_string())
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
