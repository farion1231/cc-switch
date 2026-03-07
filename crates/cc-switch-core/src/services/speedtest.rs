//! Endpoint latency checks.

use futures::future::join_all;
use reqwest::{Client, Url};
use std::time::Instant;

use crate::error::AppError;
use crate::services::provider::EndpointLatency;

const DEFAULT_TIMEOUT_SECS: u64 = 8;
const MAX_TIMEOUT_SECS: u64 = 30;
const MIN_TIMEOUT_SECS: u64 = 2;

pub struct SpeedtestService;

impl SpeedtestService {
    pub async fn test_endpoints(
        urls: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Vec<EndpointLatency>, AppError> {
        if urls.is_empty() {
            return Ok(vec![]);
        }

        let mut results: Vec<Option<EndpointLatency>> = vec![None; urls.len()];
        let mut valid_targets = Vec::new();

        for (index, raw_url) in urls.into_iter().enumerate() {
            let trimmed = raw_url.trim().to_string();
            if trimmed.is_empty() {
                results[index] = Some(EndpointLatency {
                    url: raw_url,
                    latency_ms: None,
                    error: Some("URL 不能为空".to_string()),
                });
                continue;
            }

            match Url::parse(&trimmed) {
                Ok(parsed_url) => valid_targets.push((index, trimmed, parsed_url)),
                Err(err) => {
                    results[index] = Some(EndpointLatency {
                        url: trimmed,
                        latency_ms: None,
                        error: Some(format!("URL 无效: {err}")),
                    });
                }
            }
        }

        if valid_targets.is_empty() {
            return Ok(results.into_iter().flatten().collect());
        }

        let timeout = std::time::Duration::from_secs(Self::sanitize_timeout(timeout_secs));
        let client = Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(|e| AppError::Message(format!("构建测速客户端失败: {e}")))?;

        let tasks = valid_targets.into_iter().map(|(index, url, parsed_url)| {
            let client = client.clone();
            async move {
                let _ = client.get(parsed_url.clone()).timeout(timeout).send().await;

                let start = Instant::now();
                let result = match client.get(parsed_url).timeout(timeout).send().await {
                    Ok(_) => EndpointLatency {
                        url,
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        error: None,
                    },
                    Err(err) => EndpointLatency {
                        url,
                        latency_ms: None,
                        error: Some(if err.is_timeout() {
                            "请求超时".to_string()
                        } else if err.is_connect() {
                            "连接失败".to_string()
                        } else {
                            err.to_string()
                        }),
                    },
                };
                (index, result)
            }
        });

        for (index, result) in join_all(tasks).await {
            results[index] = Some(result);
        }

        Ok(results.into_iter().flatten().collect())
    }

    fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
        timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_timeout_clamps_values() {
        assert_eq!(SpeedtestService::sanitize_timeout(Some(1)), MIN_TIMEOUT_SECS);
        assert_eq!(SpeedtestService::sanitize_timeout(Some(999)), MAX_TIMEOUT_SECS);
        assert_eq!(SpeedtestService::sanitize_timeout(None), DEFAULT_TIMEOUT_SECS);
    }

    #[tokio::test]
    async fn test_endpoints_handles_empty_list() {
        let result = SpeedtestService::test_endpoints(Vec::new(), Some(5))
            .await
            .expect("empty list should succeed");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_endpoints_reports_invalid_url() {
        let result = SpeedtestService::test_endpoints(
            vec!["not a url".into(), "".into()],
            None,
        )
        .await
        .expect("invalid inputs should still succeed");

        assert_eq!(result.len(), 2);
        assert!(
            result[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("URL 无效")
        );
        assert_eq!(result[1].error.as_deref(), Some("URL 不能为空"));
    }
}
