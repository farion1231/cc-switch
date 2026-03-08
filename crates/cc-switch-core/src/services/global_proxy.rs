use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

use crate::database::Database;
use crate::error::AppError;
use crate::proxy::http_client;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub success: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProxyStatus {
    pub enabled: bool,
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProxy {
    pub url: String,
    pub proxy_type: String,
    pub port: u16,
}

const PROXY_PORTS: &[(u16, &str, bool)] = &[
    (7890, "http", true),
    (7891, "socks5", false),
    (1080, "socks5", false),
    (8080, "http", false),
    (8888, "http", false),
    (3128, "http", false),
    (10808, "socks5", false),
    (10809, "http", false),
];

pub struct GlobalProxyService;

impl GlobalProxyService {
    pub fn get_proxy_url(db: &Database) -> Result<Option<String>, AppError> {
        db.get_global_proxy_url()
    }

    pub fn set_proxy_url(db: &Database, url: &str) -> Result<(), AppError> {
        let trimmed = url.trim();
        let url_opt = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };

        http_client::validate_proxy(url_opt).map_err(AppError::Message)?;
        db.set_global_proxy_url(url_opt)?;
        http_client::apply_proxy(url_opt).map_err(AppError::Message)?;

        Ok(())
    }

    pub fn apply_persisted_proxy_url(db: &Database) -> Result<Option<String>, AppError> {
        let url = db.get_global_proxy_url()?;
        http_client::apply_proxy(url.as_deref()).map_err(AppError::Message)?;
        Ok(url)
    }

    pub fn get_status() -> UpstreamProxyStatus {
        let url = http_client::get_current_proxy_url();
        UpstreamProxyStatus {
            enabled: url.is_some(),
            proxy_url: url,
        }
    }

    pub async fn test_proxy_url(url: &str) -> Result<ProxyTestResult, AppError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput("Proxy URL is empty".to_string()));
        }

        let start = Instant::now();
        let proxy = reqwest::Proxy::all(trimmed)
            .map_err(|e| AppError::InvalidInput(format!("Invalid proxy URL: {e}")))?;

        let client = reqwest::Client::builder()
            .proxy(proxy)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(10))
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
                Ok(response) => {
                    let latency = start.elapsed().as_millis() as u64;
                    log::debug!(
                        "[GlobalProxy] Test successful: {} -> {} via {} ({}ms)",
                        http_client::mask_url(trimmed),
                        test_url,
                        response.status(),
                        latency
                    );
                    return Ok(ProxyTestResult {
                        success: true,
                        latency_ms: latency,
                        error: None,
                    });
                }
                Err(error) => {
                    log::debug!("[GlobalProxy] Test to {test_url} failed: {error}");
                    last_error = Some(error);
                }
            }
        }

        let latency = start.elapsed().as_millis() as u64;
        Ok(ProxyTestResult {
            success: false,
            latency_ms: latency,
            error: Some(
                last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "All test targets failed".to_string()),
            ),
        })
    }

    pub async fn scan_local_proxies() -> Vec<DetectedProxy> {
        tokio::task::spawn_blocking(|| {
            let mut found = Vec::new();

            for &(port, primary_type, is_mixed) in PROXY_PORTS {
                let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
                if TcpStream::connect_timeout(&address.into(), Duration::from_millis(100)).is_ok() {
                    found.push(DetectedProxy {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn set_proxy_url_persists_and_can_clear() -> Result<(), AppError> {
        let db = Database::memory()?;

        GlobalProxyService::set_proxy_url(&db, "http://127.0.0.1:7890")?;
        assert_eq!(
            GlobalProxyService::get_proxy_url(&db)?,
            Some("http://127.0.0.1:7890".to_string())
        );
        assert!(GlobalProxyService::get_status().enabled);

        GlobalProxyService::set_proxy_url(&db, "")?;
        assert_eq!(GlobalProxyService::get_proxy_url(&db)?, None);
        assert!(!GlobalProxyService::get_status().enabled);

        Ok(())
    }

    #[test]
    #[serial]
    fn invalid_proxy_url_is_rejected_before_persisting() -> Result<(), AppError> {
        let db = Database::memory()?;
        let error = GlobalProxyService::set_proxy_url(&db, "not-a-valid-url")
            .expect_err("invalid proxy URL should fail");
        assert!(error.to_string().contains("Invalid proxy URL"));
        assert_eq!(GlobalProxyService::get_proxy_url(&db)?, None);

        Ok(())
    }

    #[tokio::test]
    async fn empty_proxy_url_cannot_be_tested() {
        let error = GlobalProxyService::test_proxy_url("")
            .await
            .expect_err("empty URL should fail");
        assert!(error.to_string().contains("Proxy URL is empty"));
    }
}
