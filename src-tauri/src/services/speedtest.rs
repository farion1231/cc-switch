use futures::future::join_all;
use reqwest::{Client, Url};
use serde::Serialize;
use std::time::Instant;

use crate::error::AppError;
use crate::settings;

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

fn current_language() -> String {
    settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string())
}

fn localized_text(zh: &str, en: &str, ja: &str, ru: &str) -> String {
    match current_language().as_str() {
        "en" => en.to_string(),
        "ja" => ja.to_string(),
        "ru" => ru.to_string(),
        _ => zh.to_string(),
    }
}

impl SpeedtestService {
    /// 测试一组端点的响应延迟。
    pub async fn test_endpoints(
        urls: Vec<String>,
        timeout_secs: Option<u64>,
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
                    error: Some(localized_text(
                        "URL 不能为空",
                        "URL cannot be empty",
                        "URL を入力してください",
                        "URL не может быть пустым",
                    )),
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
                        error: Some(localized_text(
                            &format!("URL 无效: {err}"),
                            &format!("Invalid URL: {err}"),
                            &format!("URL が無効です: {err}"),
                            &format!("URL недействителен: {err}"),
                        )),
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
            async move {
                // 先进行一次热身请求，忽略结果，仅用于复用连接/绕过首包惩罚。
                let _ = client
                    .get(parsed_url.clone())
                    .timeout(request_timeout)
                    .send()
                    .await;

                // 第二次请求开始计时，并将其作为结果返回。
                let start = Instant::now();
                let latency = match client.get(parsed_url).timeout(request_timeout).send().await {
                    Ok(resp) => EndpointLatency {
                        url: trimmed,
                        latency: Some(start.elapsed().as_millis()),
                        status: Some(resp.status().as_u16()),
                        error: None,
                    },
                    Err(err) => {
                        let status = err.status().map(|s| s.as_u16());
                        let error_message = if err.is_timeout() {
                            localized_text(
                                "请求超时",
                                "Request timed out",
                                "リクエストがタイムアウトしました",
                                "Время ожидания запроса истекло",
                            )
                        } else if err.is_connect() {
                            localized_text(
                                "连接失败",
                                "Connection failed",
                                "接続に失敗しました",
                                "Не удалось подключиться",
                            )
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
        // 使用全局 HTTP 客户端（已包含代理配置）
        // 返回 timeout Duration 供请求级别使用
        let timeout = std::time::Duration::from_secs(timeout_secs);
        Ok((crate::proxy::http_client::get(), timeout))
    }

    fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
        let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
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
        let result =
            tauri::async_runtime::block_on(SpeedtestService::test_endpoints(Vec::new(), Some(5)))
                .expect("empty list should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_endpoints_reports_invalid_url() {
        let result = tauri::async_runtime::block_on(SpeedtestService::test_endpoints(
            vec!["not a url".into(), "".into()],
            None,
        ))
        .expect("invalid inputs should still succeed");

        assert_eq!(result.len(), 2);
        assert!(
            result[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("URL"),
            "invalid url should yield parse error"
        );
        assert!(
            result[1]
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("URL"),
            "empty url should report validation error"
        );
    }
}
