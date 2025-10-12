use std::time::{Duration, Instant};
use std::collections::HashMap;

use reqwest::{
    header::{HeaderMap, HeaderValue},
    Client, StatusCode, Method,
};
use serde::{Serialize, Deserialize};
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

/// 通用 API 测试配置
/// 在 settings_config 中添加 "test_config" 字段来自定义测试行为
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTestConfig {
    /// 测试端点列表（相对路径，如 ["/v1/models", "/v1/chat/completions"]）
    #[serde(default)]
    pub endpoints: Vec<String>,
    
    /// 认证类型: "bearer" | "api-key" | "x-api-key" | "custom" | "auto"
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    
    /// 认证 Header 名称（auth_type 为 "custom" 时使用）
    #[serde(default)]
    pub auth_header: Option<String>,
    
    /// 认证值前缀（如 "Bearer ", 留空则无前缀）
    #[serde(default)]
    pub auth_prefix: Option<String>,
    
    /// 自定义额外的 Headers
    #[serde(default)]
    pub custom_headers: Option<HashMap<String, String>>,
    
    /// HTTP 方法: "GET" | "POST" | "HEAD"
    #[serde(default = "default_http_method")]
    pub http_method: String,
    
    /// API Key 在配置中的路径（如 "env.ANTHROPIC_AUTH_TOKEN" 或 "auth.OPENAI_API_KEY"）
    #[serde(default)]
    pub api_key_path: Option<String>,
    
    /// Base URL 在配置中的路径（如 "env.ANTHROPIC_BASE_URL" 或 "config.base_url"）
    #[serde(default)]
    pub base_url_path: Option<String>,
}

fn default_auth_type() -> String {
    "auto".to_string()
}

fn default_http_method() -> String {
    "HEAD".to_string()
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

/// 从配置中根据路径提取字符串值
/// 路径格式: "section.key" 或 "section.subsection.key"
/// 例如: "env.ANTHROPIC_AUTH_TOKEN", "auth.OPENAI_API_KEY"
fn get_config_value(provider: &Provider, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let mut current = &provider.settings_config;
    
    // 遍历路径的每一部分
    for (idx, part) in parts.iter().enumerate() {
        if idx == parts.len() - 1 {
            // 最后一个部分，提取值
            return current
                .get(*part)
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        } else {
            // 中间部分，继续深入
            current = current.get(*part)?;
        }
    }
    
    None
}

/// 尝试从 test_config 中提取 API Key
fn extract_api_key(provider: &Provider, test_config: Option<&ApiTestConfig>) -> Option<String> {
    if let Some(config) = test_config {
        if let Some(ref path) = config.api_key_path {
            return get_config_value(provider, path);
        }
    }
    None
}

/// 尝试从 test_config 中提取 Base URL
fn extract_base_url(provider: &Provider, test_config: Option<&ApiTestConfig>) -> Option<String> {
    if let Some(config) = test_config {
        if let Some(ref path) = config.base_url_path {
            let url = get_config_value(provider, path)?;
            let sanitized = sanitize_base_url(&url);
            if !sanitized.is_empty() {
                return Some(sanitized);
            }
        }
    }
    None
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

/// 根据认证类型构建 Header
fn build_auth_header(api_key: &str, auth_type: &str, custom_header: Option<&str>, custom_prefix: Option<&str>) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    
    match auth_type.to_lowercase().as_str() {
        "bearer" => {
            let prefix = custom_prefix.unwrap_or("Bearer ");
            headers.push(("Authorization".to_string(), format!("{}{}", prefix, api_key)));
        }
        "api-key" => {
            headers.push(("api-key".to_string(), api_key.to_string()));
        }
        "x-api-key" => {
            headers.push(("x-api-key".to_string(), api_key.to_string()));
        }
        "custom" => {
            if let Some(header_name) = custom_header {
                let prefix = custom_prefix.unwrap_or("");
                headers.push((header_name.to_string(), format!("{}{}", prefix, api_key)));
            }
        }
        _ => {} // "auto" 或其他情况由调用方处理
    }
    
    headers
}

/// 构建通用测试 Headers（支持自定义配置）
fn build_generic_headers(api_key: &str, test_config: Option<&ApiTestConfig>) -> Vec<HeaderMap> {
    let mut variants = Vec::new();
    
    if let Some(config) = test_config {
        // 使用用户配置的认证方式
        let auth_type = config.auth_type.as_str();
        
        if auth_type == "auto" {
            // 自动模式：尝试多种常见认证方式
            return build_auto_auth_variants(api_key, Some(config));
        }
        
        // 单一认证方式
        let auth_headers = build_auth_header(
            api_key,
            auth_type,
            config.auth_header.as_deref(),
            config.auth_prefix.as_deref()
        );
        
        let mut headers = HeaderMap::new();
        
        // 添加认证 headers
        for (name, value) in auth_headers {
            if let Ok(header_value) = HeaderValue::from_str(&value) {
                if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                    headers.insert(header_name, header_value);
                }
            }
        }
        
        // 添加自定义 headers
        if let Some(ref custom) = config.custom_headers {
            for (name, value) in custom {
                if let Ok(header_value) = HeaderValue::from_str(value) {
                    if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                        headers.insert(header_name, header_value);
                    }
                }
            }
        }
        
        // 添加标准 headers
        headers.insert("accept", HeaderValue::from_static("application/json"));
        
        variants.push(headers);
    } else {
        // 没有配置，使用自动检测
        variants = build_auto_auth_variants(api_key, None);
    }
    
    variants
}

/// 自动尝试不同的认证方式组合
fn build_auto_auth_variants(api_key: &str, test_config: Option<&ApiTestConfig>) -> Vec<HeaderMap> {
    let mut variants = Vec::new();
    
    // 变体 1: 标准 Anthropic API (x-api-key)
    let mut headers1 = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(api_key) {
        headers1.insert("x-api-key", value);
    }
    headers1.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
    headers1.insert("accept", HeaderValue::from_static("application/json"));
    variants.push(headers1);
    
    // 变体 2: Authorization Bearer (常见第三方代理和 OpenAI 兼容)
    let mut headers2 = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers2.insert("Authorization", value);
    }
    headers2.insert("accept", HeaderValue::from_static("application/json"));
    if let Some(config) = test_config {
        if let Some(ref custom) = config.custom_headers {
            for (name, val) in custom {
                if let Ok(hv) = HeaderValue::from_str(val) {
                    if let Ok(hn) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                        headers2.insert(hn, hv);
                    }
                }
            }
        }
    }
    variants.push(headers2);
    
    // 变体 3: 同时使用两种认证 + Claude Code 标识 (88code 等特殊代理)
    let mut headers3 = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(api_key) {
        headers3.insert("x-api-key", value);
    }
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers3.insert("Authorization", value);
    }
    headers3.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
    headers3.insert("accept", HeaderValue::from_static("application/json"));
    headers3.insert("user-agent", HeaderValue::from_static("Claude-Code/1.0"));
    if let Ok(value) = HeaderValue::from_str("claude-code") {
        headers3.insert("x-client-name", value.clone());
        headers3.insert("anthropic-client-name", value);
    }
    variants.push(headers3);
    
    // 变体 4: 只有 Authorization Bearer + Claude Code 标识
    let mut headers4 = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers4.insert("Authorization", value);
    }
    headers4.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
    headers4.insert("accept", HeaderValue::from_static("application/json"));
    headers4.insert("user-agent", HeaderValue::from_static("Claude-Code/1.0"));
    if let Ok(value) = HeaderValue::from_str("claude-code") {
        headers4.insert("x-client-name", value.clone());
        headers4.insert("anthropic-client-name", value);
    }
    variants.push(headers4);
    
    variants
}

/// 通用 API 测试函数 - 支持任意 API 配置
async fn test_generic_api(provider: &Provider, test_config: &ApiTestConfig) -> ProviderTestResult {
    // 提取 API Key
    let api_key = match extract_api_key(provider, Some(test_config)) {
        Some(key) => key,
        None => {
            return error_result("API密钥缺失或为空", None, None);
        }
    };

    // 验证API密钥长度
    if api_key.len() < 10 {
        return error_result("API密钥长度不足，可能无效", None, None);
    }

    // 提取 Base URL
    let base_url = match extract_base_url(provider, Some(test_config)) {
        Some(url) => url,
        None => {
            return error_result("Base URL 缺失或无效", None, None);
        }
    };

    // 验证URL有效性
    if base_url.is_empty() {
        return error_result("API基础地址无效或为空", None, None);
    }

    let client = match build_client() {
        Ok(c) => c,
        Err(err) => return error_result(err, None, None),
    };

    // 确定要测试的端点
    let test_endpoints = if !test_config.endpoints.is_empty() {
        // 使用用户配置的端点
        test_config
            .endpoints
            .iter()
            .map(|ep| format!("{}{}", base_url, ep))
            .collect()
    } else {
        // 使用默认的常见端点
        vec![
            format!("{}/v1/models", base_url),
            format!("{}/v1/chat/completions", base_url),
            format!("{}/v1/messages", base_url),
            format!("{}/models", base_url),
            format!("{}/", base_url),
        ]
    };

    // 解析 HTTP 方法
    let http_method = match test_config.http_method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "HEAD" | _ => Method::HEAD,
    };

    let start = Instant::now();
    let auth_variants = build_generic_headers(&api_key, Some(test_config));
    
    let mut best_response: Option<(reqwest::Response, u16, String)> = None;
    let mut found_success = false;
    let mut last_error: Option<(String, Option<u16>)> = None; // (错误消息, 状态码)

    // 智能测试策略: 尝试不同的端点和认证方式组合
    for test_url in &test_endpoints {
        for (variant_idx, headers) in auth_variants.iter().enumerate() {
            let request_builder = client.request(http_method.clone(), test_url).headers(headers.clone());
            
            match retry_request(&client, request_builder, 1).await {
                Ok(response) => {
                    let status = response.status();
                    let status_code = status.as_u16();
                    
                    log::debug!(
                        "测试端点 {} (认证变体 {}): 状态码 {}",
                        test_url,
                        variant_idx + 1,
                        status_code
                    );
                    
                    // 如果找到成功的响应，立即返回
                    if status.is_success() {
                        let latency = start.elapsed().as_millis();
                        return success_result(status, latency);
                    }
                    
                    // 记录最佳响应
                    let should_update = match &best_response {
                        None => true,
                        Some((_, prev_code, _)) => {
                            if status_code >= 200 && status_code < 300 {
                                true
                            } else if status_code == 400 && *prev_code != 200 {
                                true
                            } else if status_code == 403 && (*prev_code == 404 || *prev_code >= 500) {
                                true
                            } else {
                                false
                            }
                        }
                    };
                    
                    if should_update {
                        best_response = Some((response, status_code, test_url.clone()));
                    }
                    
                    if status_code != 404 && status_code != 403 {
                        found_success = true;
                    }
                }
                Err(err) => {
                    let status_code = err.status().map(|s| s.as_u16());
                    let err_msg = format!("端点: {} - 错误: {}", test_url, err);
                    log::debug!("测试端点 {} (认证变体 {}) 失败: {} (状态码: {:?})", 
                        test_url, variant_idx + 1, err, status_code);
                    last_error = Some((err_msg, status_code));
                    continue;
                }
            }
        }
    }

    let latency = start.elapsed().as_millis();

    // 分析最佳响应
    match best_response {
        Some((response, status_code, endpoint)) => {
            let status = response.status();
            
            if status.is_success() {
                return success_result(status, latency);
            }
            
            let body = response.text().await.unwrap_or_default();
            
            match status_code {
                400 => {
                    return error_result(
                        "API 连接成功 - 测试请求格式不完整，但服务可用",
                        Some(status),
                        Some(format!("端点: {}\n这是正常的，实际使用时会发送完整请求", endpoint))
                    );
                }
                401 => {
                    // 解析响应体，提供更详细的错误信息
                    let error_detail = if body.contains("<!DOCTYPE html>") || body.contains("<html") {
                        format!(
                            "端点: {}\n\n\
                            身份验证失败\n\n\
                            可能的原因:\n\
                            1. API 密钥无效、过期或格式错误\n\
                            2. API 密钥配置路径不正确\n\
                            3. 认证方式不匹配（Bearer、API-Key 等）\n\
                            4. 密钥权限不足\n\n\
                            建议:\n\
                            • 重新生成并更新 API 密钥\n\
                            • 检查密钥是否完整复制（无多余空格）\n\
                            • 确认使用了正确的认证方式\n\
                            • 查看服务商文档确认密钥格式",
                            endpoint
                        )
                    } else {
                        format!(
                            "端点: {}\n\n\
                            服务器响应:\n{}\n\n\
                            可能的原因:\n\
                            1. API 密钥无效或过期\n\
                            2. 认证方式配置错误\n\n\
                            建议:\n\
                            • 验证 API 密钥是否正确\n\
                            • 检查认证配置",
                            endpoint,
                            if body.len() > 300 { 
                                format!("{}...", &body[..300]) 
                            } else { 
                                body.clone() 
                            }
                        )
                    };
                    
                    return error_result(
                        "身份验证失败 - API密钥无效或过期",
                        Some(status),
                        Some(error_detail)
                    );
                }
                403 => {
                    if found_success {
                        return error_result(
                            "API 部分可用 - 某些端点受限，但服务可连接",
                            Some(status),
                            Some("这通常是正常的，许多第三方代理限制测试端点访问".to_string())
                        );
                    } else {
                        // 解析HTML响应体，提取有用信息
                        let error_detail = if body.contains("<!DOCTYPE html>") || body.contains("<html") {
                            // 尝试提取HTML中的标题或错误信息
                            let title = body
                                .split("<title>")
                                .nth(1)
                                .and_then(|s| s.split("</title>").next())
                                .unwrap_or("403 Forbidden");
                            
                            format!(
                                "端点: {}\n\n错误类型: {}\n\n可能的原因:\n\
                                1. API 密钥权限不足或已被限制\n\
                                2. 该端点需要特殊权限或白名单\n\
                                3. 第三方代理服务限制了测试端点访问\n\
                                4. IP 地址被限制或需要验证\n\n\
                                建议:\n\
                                • 检查 API 密钥是否有效且具有足够权限\n\
                                • 确认 Base URL 是否正确（官方 API 或第三方代理）\n\
                                • 如果使用第三方代理，联系服务商确认端点限制\n\
                                • 尝试在浏览器中访问该 URL 查看详细错误",
                                endpoint, title
                            )
                        } else {
                            // JSON或纯文本错误
                            format!(
                                "端点: {}\n\n服务器响应:\n{}\n\n可能的原因:\n\
                                1. API 密钥无效或权限不足\n\
                                2. 该端点需要特殊权限\n\
                                3. 服务配置限制\n\n\
                                建议:\n\
                                • 验证 API 密钥是否正确\n\
                                • 检查 API 密钥的权限范围\n\
                                • 确认 Base URL 配置正确",
                                endpoint,
                                if body.len() > 500 { 
                                    format!("{}...", &body[..500]) 
                                } else { 
                                    body.clone() 
                                }
                            )
                        };
                        
                        return error_result(
                            "访问被拒绝 - 权限不足或端点受限",
                            Some(status),
                            Some(error_detail)
                        );
                    }
                }
                404 => {
                    return error_result(
                        "API端点不存在 - Base URL 或端点路径配置错误",
                        Some(status),
                        Some(format!(
                            "尝试的端点: {}\n\n\
                            可能的原因:\n\
                            1. Base URL 配置错误\n\
                               • 检查是否包含了不应该有的路径部分\n\
                               • 例如: 应该是 'https://api.example.com' 而不是 'https://api.example.com/v1'\n\
                            2. 使用了错误的 API 版本路径\n\
                            3. 第三方代理的端点路径与官方不同\n\n\
                            建议:\n\
                            • 查看服务商的 API 文档确认正确的 Base URL\n\
                            • 如果使用第三方代理，确认其端点路径规范\n\
                            • 尝试在浏览器中访问该 URL 查看实际响应",
                            endpoint
                        ))
                    );
                }
                429 => {
                    return error_result(
                        "请求频率限制 - 请求过于频繁,请稍后重试",
                        Some(status),
                        Some(body)
                    );
                }
                500..=599 => {
                    return error_result(
                        "服务器错误 - 服务端出现问题",
                        Some(status),
                        Some(body)
                    );
                }
                _ => {
                    return error_result(
                        "服务器返回错误",
                        Some(status),
                        Some(body)
                    );
                }
            }
        }
        None => {
            // 没有收到任何响应，返回最后一个错误信息
            let (message, status, detail) = if let Some((err_msg, status_code)) = last_error {
                // 不在 message 中包含状态码，因为前端会单独显示 statusCode 字段
                let msg = "API 测试失败".to_string();
                
                // 构建尝试的端点列表
                let endpoints_list = test_endpoints
                    .iter()
                    .enumerate()
                    .map(|(i, ep)| format!("   {}. {}", i + 1, ep))
                    .collect::<Vec<_>>()
                    .join("\n");
                
                let detail_msg = format!(
                    "所有测试端点和认证方式都无法访问\n\n\
                    尝试的端点 ({} 个):\n{}\n\n\
                    最后一个错误:\n{}\n\n\
                    请检查:\n\
                    1. Base URL 是否正确\n\
                       • 当前: {}\n\
                       • 确保 URL 格式正确（如: https://api.example.com）\n\
                    2. 网络连接是否正常\n\
                       • 检查防火墙设置\n\
                       • 确认可以访问该域名\n\
                    3. API 密钥是否有效\n\
                       • 验证密钥格式和权限\n\
                    4. API 服务是否在线\n\
                       • 尝试在浏览器中访问 Base URL\n\
                       • 查看服务商状态页面",
                    test_endpoints.len(),
                    endpoints_list,
                    err_msg,
                    base_url
                );
                
                // 如果有状态码，构造 StatusCode 对象
                let status_obj = status_code.and_then(|code| StatusCode::from_u16(code).ok());
                
                (msg, status_obj, detail_msg)
            } else {
                let msg = "无法连接到 API 服务".to_string();
                
                // 构建尝试的端点列表
                let endpoints_list = test_endpoints
                    .iter()
                    .enumerate()
                    .map(|(i, ep)| format!("   {}. {}", i + 1, ep))
                    .collect::<Vec<_>>()
                    .join("\n");
                
                let detail_msg = format!(
                    "所有测试端点和认证方式都无法访问\n\n\
                    尝试的端点 ({} 个):\n{}\n\n\
                    请检查:\n\
                    1. Base URL 是否正确\n\
                       • 当前: {}\n\
                       • 确保 URL 格式正确（如: https://api.example.com）\n\
                    2. 网络连接是否正常\n\
                       • 检查防火墙设置\n\
                       • 确认可以访问该域名\n\
                       • 尝试 ping 或在浏览器中访问\n\
                    3. API 密钥配置路径是否正确\n\
                       • 验证配置文件中的路径\n\
                    4. API 服务是否在线\n\
                       • 查看服务商状态页面\n\
                       • 确认服务未维护",
                    test_endpoints.len(),
                    endpoints_list,
                    base_url
                );
                (msg, None, detail_msg)
            };
            
            return error_result(
                message,
                status,
                Some(detail),
            );
        }
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

    let start = Instant::now();
    
    // 优先使用实际的 API 调用进行测试（更准确）
    // 尝试发送一个最小的 messages 请求
    let messages_url = format!("{}/v1/messages", base_url);
    
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(&api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json"),
    );
    
    // 构建一个最小的测试请求体
    let test_body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": "hi"
        }]
    });
    
    let request_builder = client
        .post(&messages_url)
        .headers(headers.clone())
        .json(&test_body);
    
    // 首先尝试实际 API 调用
    match retry_request(&client, request_builder, 1).await {
        Ok(response) => {
            let status = response.status();
            let latency = start.elapsed().as_millis();
            
            // 任何非 404 的响应都说明 API 是可达的
            if status.is_success() || status.as_u16() == 400 || status.as_u16() == 401 || status.as_u16() == 403 {
                // 400: 请求格式问题（但 API 可达）
                // 401/403: 认证问题（但 API 可达）
                // 成功: API 完全可用
                return success_result(status, latency);
            }
        }
        Err(_) => {
            // 如果 POST 请求失败，回退到 HEAD 请求测试
        }
    }
    
    // 回退方案: 尝试多个可能的端点进行测试
    let test_endpoints = vec![
        format!("{}/v1/models", base_url),      // 优先测试 models 端点
        format!("{}/v1/messages", base_url),    // 然后是 messages 端点
        format!("{}/v1/complete", base_url),    // complete 端点
        format!("{}/", base_url),               // 根路径
    ];

    let auth_variants = build_auto_auth_variants(&api_key, None);
    
    let mut best_response: Option<(reqwest::Response, u16, String)> = None;
    let mut found_success = false;
    let mut last_error: Option<(String, Option<u16>)> = None; // (错误消息, 状态码)

    // 智能测试策略: 尝试不同的端点和认证方式组合
    for test_url in &test_endpoints {
        for (variant_idx, headers) in auth_variants.iter().enumerate() {
            let request_builder = client.head(test_url).headers(headers.clone());
            
            match retry_request(&client, request_builder, 1).await {
                Ok(response) => {
                    let status = response.status();
                    let status_code = status.as_u16();
                    
                    log::debug!(
                        "测试端点 {} (认证变体 {}): 状态码 {}",
                        test_url,
                        variant_idx + 1,
                        status_code
                    );
                    
                    // 如果找到成功的响应，立即返回
                    if status.is_success() {
                        let latency = start.elapsed().as_millis();
                        return success_result(status, latency);
                    }
                    
                    // 记录最佳响应 (优先级: 200 > 400 > 403 > 404 > 其他)
                    let should_update = match &best_response {
                        None => true,
                        Some((_, prev_code, _)) => {
                            // 200 系列最优
                            if status_code >= 200 && status_code < 300 {
                                true
                            }
                            // 400 系列比 403/404 更有信息量
                            else if status_code == 400 && *prev_code != 200 {
                                true
                            }
                            // 403 比 404 更好 (说明端点存在但需要权限)
                            else if status_code == 403 && (*prev_code == 404 || *prev_code >= 500) {
                                true
                            }
                            // 其他情况保持原有的
                            else {
                                false
                            }
                        }
                    };
                    
                    if should_update {
                        best_response = Some((response, status_code, test_url.clone()));
                    }
                    
                    // 如果不是 404/403，说明端点可能可用，继续尝试其他认证方式
                    if status_code != 404 && status_code != 403 {
                        found_success = true;
                    }
                }
                Err(err) => {
                    let status_code = err.status().map(|s| s.as_u16());
                    let err_msg = format!("端点: {} - 错误: {}", test_url, err);
                    log::debug!("测试端点 {} (认证变体 {}) 失败: {} (状态码: {:?})", 
                        test_url, variant_idx + 1, err, status_code);
                    last_error = Some((err_msg, status_code));
                    continue;
                }
            }
        }
    }

    let latency = start.elapsed().as_millis();

    // 分析最佳响应
    match best_response {
        Some((response, status_code, endpoint)) => {
            let status = response.status();
            
            if status.is_success() {
                return success_result(status, latency);
            }
            
            let body = response.text().await.unwrap_or_default();
            
            // 智能判断: 即使返回错误码，但如果能连接到服务器，也可能是可用的
            match status_code {
                400 => {
                    // 400 通常意味着请求格式问题，但服务器是可达的
                    return error_result(
                        "API 连接成功 - 测试请求格式不完整，但服务可用",
                        Some(status),
                        Some(format!("端点: {}\n这是正常的，实际使用时会发送完整请求", endpoint))
                    );
                }
                401 => {
                    return error_result(
                        "身份验证失败 - API密钥无效或过期",
                        Some(status),
                        Some(body)
                    );
                }
                403 => {
                    // 403 可能意味着端点受限，但 API 密钥可能有效
                    if found_success {
                        return error_result(
                            "API 部分可用 - 某些端点受限，但服务可连接",
                            Some(status),
                            Some("这通常是正常的，许多第三方代理限制测试端点访问".to_string())
                        );
                    } else {
                        return error_result(
                            "访问被拒绝 - 可能是权限不足或 API 密钥限制",
                            Some(status),
                            Some(body)
                        );
                    }
                }
                404 => {
                    return error_result(
                        "API端点不存在 - 请检查 Base URL 是否正确",
                        Some(status),
                        Some(format!("尝试的端点: {}", endpoint))
                    );
                }
                429 => {
                    return error_result(
                        "请求频率限制 - 请求过于频繁,请稍后重试",
                        Some(status),
                        Some(body)
                    );
                }
                500..=599 => {
                    return error_result(
                        "服务器错误 - 服务端出现问题",
                        Some(status),
                        Some(body)
                    );
                }
                _ => {
                    return error_result(
                        "服务器返回错误",
                        Some(status),
                        Some(body)
                    );
                }
            }
        }
        None => {
            // 没有收到任何响应，返回最后一个错误信息
            let (message, status, detail) = if let Some((err_msg, status_code)) = last_error {
                // 不在 message 中包含状态码，因为前端会单独显示 statusCode 字段
                let msg = "API 测试失败".to_string();
                
                let detail_msg = format!(
                    "所有测试端点和认证方式都无法访问\n\n最后一个错误:\n{}\n\n请检查:\n1. Base URL 是否正确\n2. 网络连接是否正常\n3. API 服务是否在线",
                    err_msg
                );
                
                let status_obj = status_code.and_then(|code| StatusCode::from_u16(code).ok());
                (msg, status_obj, detail_msg)
            } else {
                let msg = "无法连接到 API 服务".to_string();
                let detail_msg = "所有测试端点和认证方式都无法访问，请检查:\n1. Base URL 是否正确\n2. 网络连接是否正常\n3. API 服务是否在线".to_string();
                (msg, None, detail_msg)
            };
            
            return error_result(
                message,
                status,
                Some(detail),
            );
        }
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
            401 => "身份验证失败 - API密钥无效或过期",
            403 => "访问被拒绝 - 账户权限不足或API密钥被限制",
            404 => "API端点不存在 - 请检查API地址是否正确",
            429 => "请求频率限制 - 请求过于频繁,请稍后重试",
            500 => "服务器内部错误 - 服务端出现问题",
            502 => "网关错误 - 服务器网关问题",
            503 => "服务不可用 - 服务器暂时不可用",
            _ => "服务器返回错误",
        };
        error_result(error_message, Some(status), Some(body))
    }
}

pub async fn test_provider(provider: Provider, app_type: AppType) -> ProviderTestResult {
    // 首先检查是否有通用测试配置
    if let Some(test_config_value) = provider.settings_config.get("testConfig").or_else(|| provider.settings_config.get("test_config")) {
        // 尝试解析 test_config
        match serde_json::from_value::<ApiTestConfig>(test_config_value.clone()) {
            Ok(test_config) => {
                log::info!("使用通用 API 测试配置");
                return test_generic_api(&provider, &test_config).await;
            }
            Err(err) => {
                log::warn!("解析 test_config 失败: {}，将使用默认测试方法", err);
                // 解析失败，继续使用默认测试方法
            }
        }
    }
    
    // 没有通用配置或解析失败，使用传统的类型特定测试
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

/// 向供应商发送测试消息并返回完整的 API 响应
pub async fn send_test_message_to_provider(
    provider: Provider,
    app_type: AppType,
    message: String,
) -> Result<String, String> {
    let client = build_client()?;

    match app_type {
        AppType::Claude => send_claude_message(&client, &provider, message).await,
        AppType::Codex => send_codex_message(&client, &provider, message).await,
    }
}

/// 向 Claude API 发送消息
async fn send_claude_message(
    client: &Client,
    provider: &Provider,
    message: String,
) -> Result<String, String> {
    let base_url = anthropic_base_url(provider);
    if base_url.is_empty() {
        return Err("API基础地址无效或为空".to_string());
    }

    let api_key = anthropic_api_key(provider)
        .ok_or_else(|| "API密钥缺失或为空".to_string())?;

    let url = format!("{}/v1/messages", base_url);

    // 构建请求体
    let request_body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": message
        }]
    });

    // 尝试多种认证方式
    let auth_variants = build_auto_auth_variants(&api_key, None);

    // 保存最后一次错误响应
    let mut last_error_response: Option<String> = None;

    for headers in auth_variants.iter() {
        let request = client
            .post(&url)
            .headers(headers.clone())
            .json(&request_body)
            .timeout(Duration::from_secs(30));

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();
                let body = response.text().await.unwrap_or_else(|e| {
                    format!("读取响应失败: {}", e)
                });

                if status.is_success() {
                    return Ok(body);
                }

                // 保存错误响应
                let error_msg = format!(
                    "状态码: {}\n\n{}",
                    status_code,
                    body
                );
                last_error_response = Some(error_msg.clone());

                // 如果不是认证错误，直接返回当前错误响应
                if status_code != 401 && status_code != 403 {
                    return Ok(error_msg);
                }
                // 认证错误则尝试下一个认证变体
            }
            Err(e) => {
                let error_msg = format!("请求失败: {}", e);
                last_error_response = Some(error_msg.clone());

                // 如果不是认证相关错误，直接返回
                if !e.status().map_or(false, |s| s == 401 || s == 403) {
                    return Err(error_msg);
                }
            }
        }
    }

    // 返回最后一次尝试的错误响应
    Ok(last_error_response.unwrap_or_else(||
        "所有认证方式都失败了，但没有收到具体的错误响应".to_string()
    ))
}

/// 向 Codex (OpenAI-compatible) API 发送消息
async fn send_codex_message(
    client: &Client,
    provider: &Provider,
    message: String,
) -> Result<String, String> {
    let base_url = codex_base_url(provider)
        .ok_or_else(|| "缺少 base_url 配置".to_string())?;

    if base_url.is_empty() {
        return Err("API基础地址无效或为空".to_string());
    }

    let api_key = codex_api_key(provider)
        .ok_or_else(|| "缺少 OPENAI_API_KEY 配置".to_string())?;

    let url = format!("{}/chat/completions", base_url);

    // 构建请求体
    let request_body = serde_json::json!({
        "model": "gpt-4",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": message
        }]
    });

    // 构建请求头
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers.insert("Authorization", value);
    }
    headers.insert("accept", HeaderValue::from_static("application/json"));
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let request = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .timeout(Duration::from_secs(30));

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let status_code = status.as_u16();
            let body = response.text().await.unwrap_or_else(|e| {
                format!("读取响应失败: {}", e)
            });

            if status.is_success() {
                Ok(body)
            } else {
                // 返回完整的错误响应，包括状态码
                Ok(format!(
                    "状态码: {}\n\n{}",
                    status_code,
                    body
                ))
            }
        }
        Err(e) => {
            // 对于网络错误，也尝试提取状态码和响应体
            if let Some(status) = e.status() {
                Ok(format!(
                    "状态码: {}\n\n请求失败: {}",
                    status.as_u16(),
                    e
                ))
            } else {
                Err(format!("请求失败: {}", e))
            }
        }
    }
}
