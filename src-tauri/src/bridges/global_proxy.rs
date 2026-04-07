use crate::error::AppError;
use crate::store::AppState;

use super::support::with_core_state;

pub fn legacy_get_proxy_url(state: &AppState) -> Result<Option<String>, AppError> {
    state.db.get_global_proxy_url()
}

pub fn get_proxy_url() -> Result<Option<String>, AppError> {
    with_core_state(|state| cc_switch_core::GlobalProxyService::get_proxy_url(&state.db))
}

pub fn legacy_set_proxy_url(state: &AppState, url: &str) -> Result<(), AppError> {
    let trimmed = url.trim();
    let url_opt = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };

    crate::proxy::http_client::validate_proxy(url_opt).map_err(AppError::Message)?;
    state.db.set_global_proxy_url(url_opt)?;
    crate::proxy::http_client::apply_proxy(url_opt).map_err(AppError::Message)?;
    Ok(())
}

pub fn set_proxy_url(url: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::GlobalProxyService::set_proxy_url(&state.db, url))
}

pub async fn legacy_test_proxy_url(url: &str) -> Result<cc_switch_core::ProxyTestResult, AppError> {
    let proxy = reqwest::Proxy::all(url)
        .map_err(|e| AppError::Message(format!("Invalid proxy URL: {e}")))?;
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Message(format!("Failed to build client: {e}")))?;

    let test_urls = [
        "https://httpbin.org/get",
        "https://www.google.com",
        "https://api.anthropic.com",
    ];

    let mut last_error = None;
    for test_url in test_urls {
        match client.head(test_url).send().await {
            Ok(_) => {
                return Ok(cc_switch_core::ProxyTestResult {
                    success: true,
                    latency_ms: start.elapsed().as_millis() as u64,
                    error: None,
                });
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }

    Ok(cc_switch_core::ProxyTestResult {
        success: false,
        latency_ms: start.elapsed().as_millis() as u64,
        error: last_error,
    })
}

pub async fn test_proxy_url(url: &str) -> Result<cc_switch_core::ProxyTestResult, AppError> {
    cc_switch_core::GlobalProxyService::test_proxy_url(url)
        .await
        .map_err(|err| AppError::Message(err.to_string()))
}

pub fn legacy_get_status() -> cc_switch_core::UpstreamProxyStatus {
    let url = crate::proxy::http_client::get_current_proxy_url();
    cc_switch_core::UpstreamProxyStatus {
        enabled: url.is_some(),
        proxy_url: url,
    }
}

pub fn get_status() -> Result<cc_switch_core::UpstreamProxyStatus, AppError> {
    Ok(cc_switch_core::GlobalProxyService::get_status())
}

pub async fn legacy_scan_local_proxies() -> Vec<cc_switch_core::DetectedProxy> {
    const PORTS: &[(u16, &str, bool)] = &[
        (7890, "http", true),
        (7891, "socks5", false),
        (1080, "socks5", false),
        (8080, "http", false),
        (8888, "http", false),
        (3128, "http", false),
        (10808, "socks5", false),
        (10809, "http", false),
    ];

    tokio::task::spawn_blocking(|| {
        let mut found = Vec::new();
        for &(port, primary_type, is_mixed) in PORTS {
            let address = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, port);
            if std::net::TcpStream::connect_timeout(
                &address.into(),
                std::time::Duration::from_millis(100),
            )
            .is_ok()
            {
                found.push(cc_switch_core::DetectedProxy {
                    url: format!("{primary_type}://127.0.0.1:{port}"),
                    proxy_type: primary_type.to_string(),
                    port,
                });
                if is_mixed {
                    let alt_type = if primary_type == "http" {
                        "socks5"
                    } else {
                        "http"
                    };
                    found.push(cc_switch_core::DetectedProxy {
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

pub async fn scan_local_proxies() -> Result<Vec<cc_switch_core::DetectedProxy>, AppError> {
    Ok(cc_switch_core::GlobalProxyService::scan_local_proxies().await)
}
