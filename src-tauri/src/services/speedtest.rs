use futures::future::join_all;
use reqwest::{Client, Url};
use serde::Serialize;
use std::time::Instant;

use crate::app_config::AppType;
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
        app_type: Option<&AppType>,
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
        let (client, request_timeout) = Self::build_client(timeout)?;

        let tasks = valid_targets.into_iter().map(|(idx, trimmed, parsed_url)| {
            let client = client.clone();
            let probe_urls = Self::build_probe_urls(&trimmed, &parsed_url, app_type);
            let auth_status_reachable = Self::allow_auth_status_as_reachable(app_type, &parsed_url);
            async move {
                // 先进行一次热身请求，忽略结果，仅用于复用连接/绕过首包惩罚。
                let _ = Self::send_probe_request(&client, &probe_urls, request_timeout).await;

                // 第二次请求开始计时，并将其作为结果返回。
                let start = Instant::now();
                let latency = match Self::send_probe_request(&client, &probe_urls, request_timeout)
                    .await
                {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        let latency = start.elapsed().as_millis();
                        let error = if Self::is_reachable_status(status, auth_status_reachable) {
                            None
                        } else {
                            Some(format!("HTTP {status}"))
                        };

                        EndpointLatency {
                            url: trimmed,
                            latency: Some(latency),
                            status: Some(status),
                            error,
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
                };

                (idx, latency)
            }
        });

        for (idx, latency) in join_all(tasks).await {
            results[idx] = Some(latency);
        }

        Ok(results.into_iter().flatten().collect::<Vec<_>>())
    }

    fn build_client(timeout_secs: u64) -> Result<(Client, std::time::Duration), AppError> {
        // 使用测速专用客户端：当全局代理指向 CC Switch 自身时自动绕过，避免递归代理干扰结果。
        // 返回 timeout Duration 供请求级别使用
        let timeout = std::time::Duration::from_secs(timeout_secs);
        Ok((crate::proxy::http_client::get_for_speedtest(), timeout))
    }

    fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
        let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    }

    fn build_probe_urls(
        base_url: &str,
        parsed_url: &Url,
        app_type: Option<&AppType>,
    ) -> Vec<String> {
        match app_type {
            // Codex endpoint management should validate Responses API availability,
            // not just whether the host returns a generic 200 page.
            Some(AppType::Codex) if Self::is_azure_openai_host(parsed_url) => {
                let base = base_url.trim_end_matches('/');
                let mut urls = if base.ends_with("/v1") {
                    vec![format!("{base}/responses")]
                } else {
                    vec![format!("{base}/responses"), format!("{base}/v1/responses")]
                };
                urls.dedup();
                urls
            }
            _ => vec![parsed_url.to_string()],
        }
    }

    fn is_reachable_status(status: u16, allow_auth_status: bool) -> bool {
        (200..300).contains(&status) || (allow_auth_status && matches!(status, 401 | 403 | 405))
    }

    fn allow_auth_status_as_reachable(app_type: Option<&AppType>, parsed_url: &Url) -> bool {
        matches!(app_type, Some(AppType::Codex)) && Self::is_azure_openai_host(parsed_url)
    }

    fn is_azure_openai_host(parsed_url: &Url) -> bool {
        let Some(host) = parsed_url.host_str() else {
            return false;
        };
        host.ends_with(".openai.azure.com") || host.ends_with(".cognitiveservices.azure.com")
    }

    async fn send_probe_request(
        client: &Client,
        probe_urls: &[String],
        timeout: std::time::Duration,
    ) -> Result<reqwest::Response, reqwest::Error> {
        for (idx, url) in probe_urls.iter().enumerate() {
            match client.get(url).timeout(timeout).send().await {
                Ok(resp) => {
                    // 对于多候选路径，仅在 404 时继续尝试下一个路径
                    if resp.status().as_u16() == 404 && idx + 1 < probe_urls.len() {
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    // 路径回退只处理 HTTP 404；连接类错误直接返回
                    if err.status().map(|s| s.as_u16()) == Some(404) && idx + 1 < probe_urls.len()
                    {
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        unreachable!("probe_urls should never be empty")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;

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
        let result =
            tauri::async_runtime::block_on(SpeedtestService::test_endpoints(
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
    fn build_probe_urls_for_codex_prefers_responses_path() {
        let parsed = Url::parse("https://example.openai.azure.com/openai").expect("valid url");
        let urls = SpeedtestService::build_probe_urls(
            "https://example.openai.azure.com/openai",
            &parsed,
            Some(&AppType::Codex),
        );
        assert_eq!(
            urls,
            vec![
                "https://example.openai.azure.com/openai/responses".to_string(),
                "https://example.openai.azure.com/openai/v1/responses".to_string()
            ]
        );
    }

    #[test]
    fn build_probe_urls_for_non_azure_codex_uses_original_url() {
        let parsed = Url::parse("https://api.openai.com/v1").expect("valid url");
        let urls =
            SpeedtestService::build_probe_urls("https://api.openai.com/v1", &parsed, Some(&AppType::Codex));
        assert_eq!(urls, vec!["https://api.openai.com/v1".to_string()]);
    }

    #[test]
    fn reachable_status_auth_errors_only_when_allowed() {
        assert!(SpeedtestService::is_reachable_status(200, false));
        assert!(SpeedtestService::is_reachable_status(204, false));
        assert!(!SpeedtestService::is_reachable_status(401, false));
        assert!(!SpeedtestService::is_reachable_status(403, false));
        assert!(!SpeedtestService::is_reachable_status(405, false));
        assert!(SpeedtestService::is_reachable_status(401, true));
        assert!(SpeedtestService::is_reachable_status(403, true));
        assert!(SpeedtestService::is_reachable_status(405, true));
        assert!(!SpeedtestService::is_reachable_status(404, true));
        assert!(!SpeedtestService::is_reachable_status(500, true));
    }
}
