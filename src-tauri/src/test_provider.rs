use std::time::{Duration, Instant};

use reqwest::{
    header::{HeaderMap, HeaderValue},
    Client, StatusCode,
};
use serde::Serialize;
use url::Url;

use crate::{app_config::AppType, provider::Provider};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_TIMEOUT_SECS: u64 = 10;
const MAX_RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAY_MS: u64 = 1000; // 重试延迟1秒

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
        .danger_accept_invalid_certs(false) // 验证SSL证书
        .redirect(reqwest::redirect::Policy::limited(3)) // 限制重定向次数
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))
}

fn sanitize_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = trimmed.to_string();

    // 添加协议前缀（如果缺失）
    if !normalized.starts_with("http://") && !normalized.starts_with("https://") {
        normalized = format!("https://{}", normalized);
    }

    // 移除末尾斜杠
    normalized = normalized.trim_end_matches('/').to_string();

    // 基本URL格式验证
    if let Ok(url) = url::Url::parse(&normalized) {
        // 检查主机名是否有效
        if url.host().is_none() {
            return String::new();
        }

        // 只允许http和https协议
        if url.scheme() != "http" && url.scheme() != "https" {
            return String::new();
        }

        normalized
    } else {
        // 如果解析失败，返回空字符串
        String::new()
    }
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

// 验证供应商配置
fn validate_provider_config(provider: &Provider, app_type: &AppType) -> Result<(), String> {
    match app_type {
        AppType::Claude => {
            // 检查Claude配置
            let env_config = provider.settings_config.get("env").ok_or("缺少env配置")?;

            // 检查API密钥
            let api_key = env_config
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .ok_or("缺少ANTHROPIC_AUTH_TOKEN配置")?;

            if api_key.trim().is_empty() {
                return Err("ANTHROPIC_AUTH_TOKEN为空".to_string());
            }

            if api_key.len() < 10 {
                return Err("ANTHROPIC_AUTH_TOKEN长度不足，可能无效".to_string());
            }

            // 检查API密钥格式（第三方代理可能使用不同格式）
            // 注意：第三方代理可能不遵循官方格式，所以只检查基本长度

            // 检查基础URL
            let base_url = env_config
                .get("ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str())
                .unwrap_or("https://api.anthropic.com");

            if let Err(_) = Url::parse(base_url) {
                return Err(format!("ANTHROPIC_BASE_URL格式无效: {}", base_url));
            }
        }
        AppType::Codex => {
            // 检查Codex配置
            // 检查auth配置
            let auth_config = provider.settings_config.get("auth").ok_or("缺少auth配置")?;

            let api_key = auth_config
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .ok_or("缺少OPENAI_API_KEY配置")?;

            if api_key.trim().is_empty() {
                return Err("OPENAI_API_KEY为空".to_string());
            }

            if api_key.len() < 10 {
                return Err("OPENAI_API_KEY长度不足，可能无效".to_string());
            }

            // 检查API密钥格式（第三方代理可能使用不同格式）
            // 注意：第三方代理可能不遵循官方格式，所以只检查基本长度

            // 检查base_url配置
            let config_text = provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if config_text.trim().is_empty() {
                return Err("config配置为空".to_string());
            }

            // 解析TOML配置中的base_url
            let mut found_base_url = false;
            for line in config_text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("#") || trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with("base_url") {
                    found_base_url = true;
                    if let Some(eq_index) = trimmed.find('=') {
                        let value_part = trimmed[eq_index + 1..].trim();
                        let value = value_part.trim_matches(|c| c == '"' || c == '\'').trim();
                        if value.is_empty() {
                            return Err("base_url配置为空".to_string());
                        }
                        if let Err(_) = Url::parse(value) {
                            return Err(format!("base_url格式无效: {}", value));
                        }
                    } else {
                        return Err("base_url配置格式错误".to_string());
                    }
                    break;
                }
            }

            if !found_base_url {
                return Err("未找到base_url配置".to_string());
            }
        }
    }

    Ok(())
}

// 重试机制 - 对于某些可重试的错误进行重试
async fn retry_request(
    _client: &Client,
    request_builder: reqwest::RequestBuilder,
    max_attempts: u32,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut last_error: Option<reqwest::Error> = None;

    for attempt in 1..max_attempts {
        // 克隆 request_builder 用于本次请求
        let request = match request_builder.try_clone() {
            Some(cloned) => cloned,
            None => {
                // 无法克隆，返回错误
                return Err(last_error.unwrap_or_else(|| panic!("无法克隆请求构建器")));
            }
        };

        match request.send().await {
            Ok(response) => {
                // 如果是成功响应或者是不可重试的错误码，直接返回
                if response.status().is_success() || !response.status().is_server_error() {
                    return Ok(response);
                }

                // 对于服务器错误,保存错误信息并准备重试
                let status = response.status();
                log::warn!("服务器返回错误 ({}), 第{}次重试...", status, attempt);
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            }
            Err(err) => {
                // 检查是否为可重试的错误
                let should_retry = err.is_timeout()
                    || err.is_connect()
                    || err.status().map_or(false, |s| s.is_server_error());

                if should_retry {
                    log::warn!("网络请求失败 ({}), 第{}次重试...", err, attempt);
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS * attempt as u64))
                        .await;
                    last_error = Some(err);
                } else {
                    return Err(err);
                }
            }
        }
    }

    // 最后一次尝试，使用原始的 request_builder
    match request_builder.send().await {
        Ok(response) => {
            if response.status().is_success() {
                Ok(response)
            } else {
                Err(response.error_for_status().unwrap_err())
            }
        }
        Err(err) => Err(err),
    }
}

async fn test_claude(provider: &Provider) -> ProviderTestResult {
    let base_url = anthropic_base_url(provider);

    // 验证URL有效性
    if base_url.is_empty() {
        return error_result("API基础地址无效或为空", None, None);
    }

    let api_key = match anthropic_api_key(provider) {
        Some(key) => key,
        None => {
            return error_result("API密钥缺失或为空", None, None);
        }
    };

    // 验证API密钥长度
    if api_key.len() < 10 {
        return error_result("API密钥长度不足，可能无效", None, None);
    }

    let client = match build_client() {
        Ok(c) => c,
        Err(err) => return error_result(err, None, None),
    };

    // 尝试多个可能的端点进行测试
    let test_endpoints = vec![
        format!("{}/v1/messages", base_url),
        format!("{}/v1/complete", base_url),
        format!("{}/v1/models", base_url),
        format!("{}/", base_url),
    ];

    // 构建正确的认证头 - Claude使用x-api-key而不是Authorization
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&api_key) {
        headers.insert("x-api-key", value);
    }
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    headers.insert("accept", HeaderValue::from_static("application/json"));

    let start = Instant::now();
    let mut last_response: Option<reqwest::Response> = None;
    let mut found_working_endpoint = false;

    // 尝试每个端点，直到找到可用的
    for test_url in &test_endpoints {
        let request_builder = client.head(test_url).headers(headers.clone());
        match retry_request(&client, request_builder, 1).await {
            Ok(response) => {
                let status = response.status();
                last_response = Some(response);
                // 如果状态码不是403/404，说明端点可能可用
                if status != 403 && status != 404 {
                    found_working_endpoint = true;
                    break;
                }
            }
            Err(_) => {
                continue; // 尝试下一个端点
            }
        }
    }

    let response = match last_response {
        Some(resp) => resp,
        None => {
            return error_result(
                "所有测试端点都无法访问",
                None,
                Some("该API服务可能不支持标准的测试端点".to_string()),
            );
        }
    };
    let status = response.status();
    let latency = start.elapsed().as_millis();

    if status.is_success() {
        success_result(status, latency)
    } else {
        let body = response.text().await.unwrap_or_default();

        // 对于403错误，提供特殊的处理逻辑
        if status.as_u16() == 403 && !found_working_endpoint {
            // 如果所有端点都返回403，可能API密钥有效但限制了端点访问
            return error_result(
                "API连接正常但端点受限 - 该服务可能不支持模型列表查询，但聊天功能正常",
                Some(status),
                Some("这通常是正常的，许多第三方代理服务限制了对标准端点的访问".to_string()),
            );
        }

        let error_message = match status.as_u16() {
            401 => "身份验证失败 (401) - API密钥无效或过期",
            403 => "访问被拒绝 (403) - 账户权限不足或API密钥被限制",
            404 => "API端点不存在 (404) - 请检查API地址是否正确",
            429 => "请求频率限制 (429) - 请求过于频繁，请稍后重试",
            500 => "服务器内部错误 (500) - 服务端出现问题",
            502 => "网关错误 (502) - 服务器网关问题",
            503 => "服务不可用 (503) - 服务器暂时不可用",
            _ => "服务器返回错误",
        };
        error_result(error_message, Some(status), Some(body))
    }
}

async fn test_codex(provider: &Provider) -> ProviderTestResult {
    let base_url = match codex_base_url(provider) {
        Some(url) => url,
        None => return error_result("缺少base_url配置", None, None),
    };

    // 验证URL有效性
    if base_url.is_empty() {
        return error_result("API基础地址无效或为空", None, None);
    }

    let api_key = match codex_api_key(provider) {
        Some(key) => key,
        None => return error_result("缺少OPENAI_API_KEY配置", None, None),
    };

    // 验证API密钥长度
    if api_key.len() < 10 {
        return error_result("API密钥长度不足，可能无效", None, None);
    }

    let client = match build_client() {
        Ok(c) => c,
        Err(err) => return error_result(err, None, None),
    };

    let test_url = format!("{}/models", base_url);

    // 构建认证头 - Codex使用Bearer token
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers.insert("Authorization", value);
    }
    headers.insert("accept", HeaderValue::from_static("application/json"));
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let start = Instant::now();
    let request_builder = client.get(&test_url).headers(headers.clone());
    let response = match retry_request(&client, request_builder, MAX_RETRY_ATTEMPTS).await {
        Ok(resp) => resp,
        Err(err) => {
            let total_latency = start.elapsed().as_millis();
            let detail = if err.is_timeout() {
                format!(
                    "请求超时 ({}ms后重试{}次均失败)，请检查网络连接或增加超时时间",
                    total_latency, MAX_RETRY_ATTEMPTS
                )
            } else if err.is_connect() {
                format!(
                    "连接失败 ({}ms后重试{}次均失败)，请检查API地址和网络连接",
                    total_latency, MAX_RETRY_ATTEMPTS
                )
            } else if err.is_request() {
                "请求格式错误".to_string()
            } else {
                format!("网络请求错误 (重试{}次失败): {}", MAX_RETRY_ATTEMPTS, err)
            };
            return error_result("请求失败", err.status(), Some(detail));
        }
    };
    let status = response.status();
    let latency = start.elapsed().as_millis();

    if status.is_success() {
        success_result(status, latency)
    } else {
        let body = response.text().await.unwrap_or_default();
        let error_message = match status.as_u16() {
            401 => "身份验证失败 (401) - API密钥无效或过期",
            403 => "访问被拒绝 (403) - 账户权限不足或API密钥被限制",
            404 => "API端点不存在 (404) - 请检查API地址是否正确",
            429 => "请求频率限制 (429) - 请求过于频繁，请稍后重试",
            500 => "服务器内部错误 (500) - 服务端出现问题",
            502 => "网关错误 (502) - 服务器网关问题",
            503 => "服务不可用 (503) - 服务器暂时不可用",
            _ => "服务器返回错误",
        };
        error_result(error_message, Some(status), Some(body))
    }
}

pub async fn test_provider(provider: Provider, app_type: AppType) -> ProviderTestResult {
    // 首先验证配置
    if let Err(config_error) = validate_provider_config(&provider, &app_type) {
        return error_result(
            format!("配置验证失败: {}", config_error),
            None,
            Some("请检查供应商配置是否正确和完整".to_string()),
        );
    }

    // 配置验证通过，执行测试
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
