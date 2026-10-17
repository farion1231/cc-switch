//! Phase 2c — global_proxy（7890 出站代理）簇。
//!
//! 镜像 `commands/global_proxy.rs`。这些命令直接读写 DB `global_proxy_url`
//! 与 `http_client` 全局客户端（即上游已有的 7890 能力，R-007）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::proxy::http_client;
use crate::proxy::types::GlobalProxyConfig;
use crate::rest::error::RestError;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/global-proxy/url", get(get_url))
        .route("/control/v1/global-proxy/url", axum::routing::put(set_url))
        .route("/control/v1/global-proxy/test", post(test_url))
        .route("/control/v1/global-proxy/status", get(get_status))
        .route("/control/v1/global-proxy/scan", post(scan))
        .route("/control/v1/global-proxy/config", get(get_config))
        .route(
            "/control/v1/global-proxy/config",
            axum::routing::put(update_config),
        )
}

async fn get_url(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let url = state.db.get_global_proxy_url()?;
    Ok(Json(ProxyUrl { url }))
}

#[derive(Serialize)]
struct ProxyUrl {
    url: Option<String>,
}

#[derive(Deserialize)]
struct SetUrlBody {
    url: String,
}

async fn set_url(
    State(state): State<AppState>,
    Json(body): Json<SetUrlBody>,
) -> Result<impl IntoResponse, RestError> {
    // 复用命令路径：写 DB + apply_proxy 热生效
    let url_opt = if body.url.trim().is_empty() {
        None
    } else {
        Some(body.url.as_str())
    };
    // 验证（不应用）
    if let Some(u) = url_opt {
        http_client::validate_proxy(Some(u))?;
    }
    state.db.set_global_proxy_url(url_opt)?;
    http_client::apply_proxy(url_opt)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TestBody {
    url: String,
}

async fn test_url(
    State(_state): State<AppState>,
    Json(body): Json<TestBody>,
) -> Result<impl IntoResponse, RestError> {
    // 镜像 commands/global_proxy.rs 的 test_proxy_url（TCP 连通 + 延迟）
    let result = test_proxy_url(&body.url);
    Ok(Json(result))
}

#[derive(Serialize)]
struct ProxyTestResult {
    success: bool,
    latency_ms: u64,
    error: Option<String>,
}

fn test_proxy_url(url: &str) -> ProxyTestResult {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};
    // 解析 host:port（支持 http://host:port 与 socks5://host:port）
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(e) => {
            return ProxyTestResult {
                success: false,
                latency_ms: 0,
                error: Some(format!("URL 解析失败: {e}")),
            };
        }
    };
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let port = parsed.port().unwrap_or(8080);
    let start = Instant::now();
    match TcpStream::connect_timeout(
        &format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], port))),
        Duration::from_secs(5),
    ) {
        Ok(_) => ProxyTestResult {
            success: true,
            latency_ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(e) => ProxyTestResult {
            success: false,
            latency_ms: start.elapsed().as_millis() as u64,
            error: Some(e.to_string()),
        },
    }
}

#[derive(Serialize)]
struct UpstreamStatus {
    enabled: bool,
    proxy_url: Option<String>,
}

async fn get_status(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let url = state.db.get_global_proxy_url()?;
    Ok(Json(UpstreamStatus {
        enabled: url.is_some(),
        proxy_url: url,
    }))
}

async fn scan(State(_state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    // 镜像 commands/global_proxy.rs scan_local_proxies：扫常见本地代理端口
    let detected = scan_local_proxies();
    Ok(Json(detected))
}

#[derive(Serialize)]
struct DetectedProxy {
    url: String,
    #[serde(rename = "proxyType")]
    proxy_type: String,
    port: u16,
}

fn scan_local_proxies() -> Vec<DetectedProxy> {
    use std::net::TcpStream;
    use std::time::Duration;
    let candidates = [
        (7890, "http", "Clash/Mihomo mixed"),
        (7891, "http", "Clash http"),
        (7892, "socks5", "Clash socks"),
        (1080, "socks5", "SOCKS5"),
        (8080, "http", "HTTP proxy"),
    ];
    candidates
        .iter()
        .filter_map(|(port, scheme, desc)| {
            let addr: std::net::SocketAddr = ([127, 0, 0, 1], *port).into();
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                Some(DetectedProxy {
                    url: format!("{scheme}://127.0.0.1:{port}"),
                    proxy_type: desc.to_string(),
                    port: *port,
                })
            } else {
                None
            }
        })
        .collect()
}

/// F-10: 补全 `get_global_proxy_config` 镜像（返 `GlobalProxyConfig`，
/// 区别于 `/status` 的 `UpstreamStatus` 运行时快照）。
async fn get_config(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let cfg = state.db.get_global_proxy_config().await?;
    Ok(Json(cfg))
}

/// F-10: 补全 `update_global_proxy_config` 镜像。
async fn update_config(
    State(state): State<AppState>,
    Json(config): Json<GlobalProxyConfig>,
) -> Result<impl IntoResponse, RestError> {
    state.db.update_global_proxy_config(config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
