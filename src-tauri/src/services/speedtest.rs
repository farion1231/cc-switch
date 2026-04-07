use futures::future::join_all;
use reqwest::Url;
use serde::Serialize;
use std::time::Instant;

use crate::error::AppError;

const DEFAULT_TIMEOUT_SECS: u64 = 8;
const MAX_TIMEOUT_SECS: u64 = 30;
const MIN_TIMEOUT_SECS: u64 = 2;

/// 端点测速结果
#[derive(Debug, Clone, Serialize)]
pub struct EndpointLatency {
    pub url: String,
    pub latency: Option<u128>,
    pub status: Option<u16>,
    pub error: Option<String>,
}

/// 网络测速相关业务
pub struct SpeedtestService;

impl SpeedtestService {
    /// 测试一组端点的响应延迟。
    pub async fn test_endpoints(
        urls: Vec<String>,
        timeout_secs: Option<u64>,
        connection_override: Option<String>,
    ) -> Result<Vec<EndpointLatency>, AppError> {
        if urls.is_empty() {
            return Ok(vec![]);
        }

        let mut results: Vec<Option<EndpointLatency>> = vec![None; urls.len()];
        let mut valid_targets = Vec::new();

        for (idx, raw_url) in urls.into_iter().enumerate() {
            let trimmed = raw_url.trim().to_string();

            if trimmed.is_empty() {
                results[idx] = Some(EndpointLatency {
                    url: raw_url,
                    latency: None,
                    status: None,
                    error: Some("URL 不能为空".to_string()),
                });
                continue;
            }

            match Url::parse(&trimmed) {
                Ok(parsed_url) => valid_targets.push((idx, trimmed, parsed_url)),
                Err(err) => {
                    results[idx] = Some(EndpointLatency {
                        url: trimmed,
                        latency: None,
                        status: None,
                        error: Some(format!("URL 无效: {err}")),
                    });
                }
            }
        }

        if valid_targets.is_empty() {
            return Ok(results.into_iter().flatten().collect::<Vec<_>>());
        }

        let timeout = Self::sanitize_timeout(timeout_secs);
        let request_timeout = Self::build_timeout(timeout);
        let override_text = connection_override
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        let tasks = valid_targets.into_iter().map(|(idx, trimmed, parsed_url)| {
            let override_text = override_text.clone();
            async move {
                let semantic_host = if override_text.is_some() {
                    Some(
                        crate::proxy::http_client::request_authority(parsed_url.as_str())
                            .map_err(AppError::Message)?,
                    )
                } else {
                    None
                };
                let effective_url = crate::proxy::http_client::apply_connection_override_to_url(
                    parsed_url.as_str(),
                    override_text.as_deref(),
                )
                .map_err(AppError::Message)?;
                let effective_parsed_url =
                    Url::parse(&effective_url).map_err(|e| AppError::Message(e.to_string()))?;

                let client = crate::proxy::http_client::get_for_provider_with_override(
                    None,
                    Some(effective_parsed_url.as_str()),
                    override_text.as_deref(),
                )
                .map_err(AppError::Message)?;

                // 先进行一次热身请求，忽略结果，仅用于复用连接/绕过首包惩罚。
                let mut warmup = client
                    .get(effective_parsed_url.clone())
                    .timeout(request_timeout);
                if let Some(host) = semantic_host.as_deref() {
                    warmup = warmup.header(reqwest::header::HOST, host);
                }
                let _ = warmup.send().await;

                // 第二次请求开始计时，并将其作为结果返回。
                let start = Instant::now();
                let mut request = client.get(effective_parsed_url).timeout(request_timeout);
                if let Some(host) = semantic_host.as_deref() {
                    request = request.header(reqwest::header::HOST, host);
                }
                let latency = match request.send().await {
                    Ok(resp) => EndpointLatency {
                        url: trimmed,
                        latency: Some(start.elapsed().as_millis()),
                        status: Some(resp.status().as_u16()),
                        error: None,
                    },
                    Err(err) => {
                        let status = err.status().map(|s| s.as_u16());
                        let error_message = Self::classify_request_error(&err);

                        EndpointLatency {
                            url: trimmed,
                            latency: None,
                            status,
                            error: Some(error_message),
                        }
                    }
                };

                Ok::<(usize, EndpointLatency), AppError>((idx, latency))
            }
        });

        for item in join_all(tasks).await {
            let (idx, latency) = item?;
            results[idx] = Some(latency);
        }

        Ok(results.into_iter().flatten().collect::<Vec<_>>())
    }

    fn build_timeout(timeout_secs: u64) -> std::time::Duration {
        std::time::Duration::from_secs(timeout_secs)
    }

    fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
        let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    }

    fn classify_request_error(err: &reqwest::Error) -> String {
        if err.is_timeout() {
            return "请求超时".to_string();
        }
        let message = err.to_string();
        let lower = message.to_lowercase();
        if err.is_connect() {
            if lower.contains("certificate")
                || lower.contains("tls")
                || lower.contains("handshake")
                || lower.contains("hostname")
                || lower.contains("sni")
            {
                return "证书/SNI 不匹配".to_string();
            }
            return "连接失败".to_string();
        }
        if lower.contains("certificate")
            || lower.contains("tls")
            || lower.contains("handshake")
            || lower.contains("hostname")
            || lower.contains("sni")
        {
            return "证书/SNI 不匹配".to_string();
        }
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_timeout_clamps_values() {
        assert_eq!(
            SpeedtestService::sanitize_timeout(Some(1)),
            MIN_TIMEOUT_SECS
        );
        assert_eq!(
            SpeedtestService::sanitize_timeout(Some(999)),
            MAX_TIMEOUT_SECS
        );
        assert_eq!(
            SpeedtestService::sanitize_timeout(Some(10)),
            10.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
        );
        assert_eq!(
            SpeedtestService::sanitize_timeout(None),
            DEFAULT_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_endpoints_handles_empty_list() {
        let result = tauri::async_runtime::block_on(SpeedtestService::test_endpoints(
            Vec::new(),
            Some(5),
            None,
        ))
        .expect("empty list should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_endpoints_reports_invalid_url() {
        let result = tauri::async_runtime::block_on(SpeedtestService::test_endpoints(
            vec!["not a url".into(), "".into()],
            None,
            None,
        ))
        .expect("invalid inputs should still succeed");

        assert_eq!(result.len(), 2);
        assert!(
            result[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .starts_with("URL 无效"),
            "invalid url should yield parse error"
        );
        assert_eq!(
            result[1].error.as_deref(),
            Some("URL 不能为空"),
            "empty url should report validation error"
        );
    }

    #[test]
    fn test_endpoints_rejects_invalid_connection_override() {
        let result = tauri::async_runtime::block_on(SpeedtestService::test_endpoints(
            vec!["https://example.com".into()],
            None,
            Some("invalid-host:443".into()),
        ));

        assert!(result.is_err());
    }
}
