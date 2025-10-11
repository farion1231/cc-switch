use std::time::{Duration, Instant};

use reqwest::{
    header::{HeaderMap, HeaderValue},
    Client, StatusCode,
};
use serde::Serialize;

use crate::{app_config::AppType, provider::Provider};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub success: bool,
    pub status: Option<u16>,
    pub latency_ms: Option<u128>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .user_agent("cc-switch-provider-test/1.0")
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))
}

fn sanitize_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = trimmed.to_string();
    if !normalized.starts_with("http://") && !normalized.starts_with("https://") {
        normalized = format!("https://{}", normalized);
    }
    normalized.trim_end_matches('/').to_string()
}

fn anthropic_base_url(provider: &Provider) -> String {
    provider
        .settings_config
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(|value| value.as_str())
        .map(sanitize_base_url)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com".to_string())
}

fn anthropic_api_key(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
        .and_then(|value| value.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn codex_base_url(provider: &Provider) -> Option<String> {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#") || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("base_url") {
            if let Some(eq_index) = rest.find('=') {
                let value_part = rest[eq_index + 1..].trim();
                let value = value_part.trim_matches(|c| c == '"' || c == '\'').trim();
                if !value.is_empty() {
                    return Some(sanitize_base_url(value));
                }
            }
        }
    }
    None
}

fn codex_api_key(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("auth")
        .and_then(|auth| auth.get("OPENAI_API_KEY"))
        .and_then(|value| value.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn success_result(status: StatusCode, latency_ms: u128) -> ProviderTestResult {
    ProviderTestResult {
        success: true,
        status: Some(status.as_u16()),
        latency_ms: Some(latency_ms),
        message: "Test passed".to_string(),
        detail: None,
    }
}

fn error_result(
    message: impl Into<String>,
    status: Option<StatusCode>,
    detail: Option<String>,
) -> ProviderTestResult {
    ProviderTestResult {
        success: false,
        status: status.map(|s| s.as_u16()),
        latency_ms: None,
        message: message.into(),
        detail,
    }
}

async fn test_claude(provider: &Provider) -> ProviderTestResult {
    let base_url = anthropic_base_url(provider);
    let api_key = match anthropic_api_key(provider) {
        Some(key) => key,
        None => {
            return error_result("Missing API key", None, None);
        }
    };

    let client = match build_client() {
        Ok(c) => c,
        Err(err) => return error_result(err, None, None),
    };

    let test_url = format!("{}/v1/models", base_url);
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&api_key) {
        headers.insert("x-api-key", value);
    }
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers.insert("Authorization", value);
    }
    headers.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
    headers.insert("accept", HeaderValue::from_static("application/json"));

    let start = Instant::now();
    let response = match client.get(&test_url).headers(headers).send().await {
        Ok(resp) => resp,
        Err(err) => {
            let detail = if err.is_timeout() {
                "Request timed out".to_string()
            } else {
                err.to_string()
            };
            return error_result("Request failed", err.status(), Some(detail));
        }
    };
    let status = response.status();
    let latency = start.elapsed().as_millis();

    if status.is_success() {
        success_result(status, latency)
    } else {
        let body = response.text().await.unwrap_or_default();
        error_result("Server error", Some(status), Some(body))
    }
}

async fn test_codex(provider: &Provider) -> ProviderTestResult {
    let base_url = match codex_base_url(provider) {
        Some(url) => url,
        None => return error_result("Missing base_url configuration", None, None),
    };
    let api_key = match codex_api_key(provider) {
        Some(key) => key,
        None => return error_result("Missing OPENAI_API_KEY", None, None),
    };

    let client = match build_client() {
        Ok(c) => c,
        Err(err) => return error_result(err, None, None),
    };

    let test_url = format!("{}/models", base_url);
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers.insert("Authorization", value);
    }
    headers.insert("accept", HeaderValue::from_static("application/json"));

    let start = Instant::now();
    let response = match client.get(&test_url).headers(headers).send().await {
        Ok(resp) => resp,
        Err(err) => {
            let detail = if err.is_timeout() {
                "Request timed out".to_string()
            } else {
                err.to_string()
            };
            return error_result("Request failed", err.status(), Some(detail));
        }
    };
    let status = response.status();
    let latency = start.elapsed().as_millis();

    if status.is_success() {
        success_result(status, latency)
    } else {
        let body = response.text().await.unwrap_or_default();
        error_result("Server error", Some(status), Some(body))
    }
}

pub async fn test_provider(provider: Provider, app_type: AppType) -> ProviderTestResult {
    match app_type {
        AppType::Claude => test_claude(&provider).await,
        AppType::Codex => test_codex(&provider).await,
    }
}

/// 批量测试所有供应商的连接
pub async fn test_all_providers(
    providers: std::collections::HashMap<String, Provider>,
    app_type: AppType,
) -> std::collections::HashMap<String, ProviderTestResult> {
    let mut results = std::collections::HashMap::new();

    for (provider_id, provider) in providers {
        let result = test_provider(provider, app_type.clone()).await;
        results.insert(provider_id, result);
    }

    results
}
