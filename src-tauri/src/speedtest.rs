use futures::future::join_all;
use reqwest::{Client, Url};
use serde::Serialize;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 8;
const MAX_TIMEOUT_SECS: u64 = 30;
const MIN_TIMEOUT_SECS: u64 = 2;

#[derive(Debug, Clone, Serialize)]
pub struct EndpointLatency {
    pub url: String,
    pub latency: Option<u128>,
    pub status: Option<u16>,
    pub error: Option<String>,
}

fn build_client(timeout_secs: u64) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("cc-switch-speedtest/1.0")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
    let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

pub async fn test_endpoints(
    urls: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<EndpointLatency>, String> {
    if urls.is_empty() {
        return Ok(vec![]);
    }

    let timeout = sanitize_timeout(timeout_secs);
    let client = build_client(timeout)?;

    let tasks = urls.into_iter().map(|raw_url| {
        let client = client.clone();
        async move {
            let trimmed = raw_url.trim().to_string();
            if trimmed.is_empty() {
                return EndpointLatency {
                    url: raw_url,
                    latency: None,
                    status: None,
                    error: Some("URL 不能为空".to_string()),
                };
            }

            let parsed_url = match Url::parse(&trimmed) {
                Ok(url) => url,
                Err(err) => {
                    return EndpointLatency {
                        url: trimmed,
                        latency: None,
                        status: None,
                        error: Some(format!("URL 无效: {err}")),
                    };
                }
            };

            let start = Instant::now();
            match client.get(parsed_url).send().await {
                Ok(resp) => {
                    let latency = start.elapsed().as_millis();
                    EndpointLatency {
                        url: trimmed,
                        latency: Some(latency),
                        status: Some(resp.status().as_u16()),
                        error: None,
                    }
                }
                Err(err) => {
                    let status = err.status().map(|s| s.as_u16());
                    let error_message = if err.is_timeout() {
                        "请求超时".to_string()
                    } else if err.is_connect() {
                        "连接失败".to_string()
                    } else {
                        err.to_string()
                    };

                    EndpointLatency {
                        url: trimmed,
                        latency: None,
                        status,
                        error: Some(error_message),
                    }
                }
            }
        }
    });

    let results = join_all(tasks).await;
    Ok(results)
}
