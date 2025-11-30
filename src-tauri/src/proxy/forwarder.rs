//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持重试和故障转移

use super::{error::*, router::ProviderRouter, types::ProxyStatus, ProxyError};
use crate::{app_config::AppType, database::Database, provider::Provider};
use reqwest::{Client, Response};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct RequestForwarder {
    client: Client,
    router: ProviderRouter,
    max_retries: u8,
    status: Arc<RwLock<ProxyStatus>>,
}

impl RequestForwarder {
    pub fn new(
        db: Arc<Database>,
        timeout_secs: u64,
        max_retries: u8,
        status: Arc<RwLock<ProxyStatus>>,
    ) -> Self {
        let mut client_builder = Client::builder();
        if timeout_secs > 0 {
            client_builder = client_builder.timeout(Duration::from_secs(timeout_secs));
        }

        let client = client_builder
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            router: ProviderRouter::new(db),
            max_retries,
            status,
        }
    }

    /// 转发请求（带重试和故障转移）
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        let mut failed_ids = Vec::new();
        let mut failover_happened = false;

        for attempt in 0..self.max_retries {
            // 选择Provider
            let provider = self.router.select_provider(app_type, &failed_ids).await?;

            log::debug!(
                "尝试 {} - 使用Provider: {} ({})",
                attempt + 1,
                provider.name,
                provider.id
            );

            // 更新状态中的当前Provider信息
            {
                let mut status = self.status.write().await;
                status.current_provider = Some(provider.name.clone());
                status.current_provider_id = Some(provider.id.clone());
                status.total_requests += 1;
                status.last_request_at = Some(chrono::Utc::now().to_rfc3339());
                if attempt > 0 {
                    failover_happened = true;
                }
            }

            let start = Instant::now();

            // 转发请求
            match self.forward(&provider, endpoint, &body, &headers).await {
                Ok(response) => {
                    let _latency = start.elapsed().as_millis() as u64;

                    // 成功：更新健康状态
                    self.router
                        .update_health(&provider, app_type, true, None)
                        .await;

                    // 更新成功统计
                    {
                        let mut status = self.status.write().await;
                        status.success_requests += 1;
                        status.last_error = None;
                        if failover_happened {
                            status.failover_count += 1;
                        }
                        // 重新计算成功率
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    return Ok(response);
                }
                Err(e) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // 失败：分类错误
                    let category = self.categorize_proxy_error(&e);

                    match category {
                        ErrorCategory::Retryable => {
                            // 可重试：更新健康状态，添加到失败列表
                            self.router
                                .update_health(&provider, app_type, false, Some(e.to_string()))
                                .await;
                            failed_ids.push(provider.id.clone());

                            // 更新错误信息
                            {
                                let mut status = self.status.write().await;
                                status.last_error =
                                    Some(format!("Provider {} 失败: {}", provider.name, e));
                            }

                            log::warn!(
                                "请求失败（可重试）: Provider {} - {} - {}ms",
                                provider.name,
                                e,
                                latency
                            );
                            continue;
                        }
                        ErrorCategory::NonRetryable | ErrorCategory::ClientAbort => {
                            // 不可重试：更新失败统计并返回
                            {
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                            }
                            log::error!("请求失败（不可重试）: {e}");
                            return Err(e);
                        }
                    }
                }
            }
        }

        // 所有重试都失败
        {
            let mut status = self.status.write().await;
            status.failed_requests += 1;
            status.last_error = Some("已达到最大重试次数".to_string());
            if status.total_requests > 0 {
                status.success_rate =
                    (status.success_requests as f32 / status.total_requests as f32) * 100.0;
            }
        }

        Err(ProxyError::MaxRetriesExceeded)
    }

    /// 转发单个请求
    async fn forward(
        &self,
        provider: &Provider,
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        // 提取 base_url
        let base_url = self.extract_base_url(provider)?;

        // 智能拼接 URL，避免重复的 /v1
        let url = if base_url.ends_with("/v1") && endpoint.starts_with("/v1") {
            format!("{}{}", base_url.trim_end_matches("/v1"), endpoint)
        } else {
            format!("{base_url}{endpoint}")
        };

        // 构建请求
        let mut request = self.client.post(&url);

        // 透传 Headers
        for (key, value) in headers {
            let key_str = key.as_str().to_lowercase();
            // 过滤掉一些不应该直接转发的 Header
            if key_str == "host" 
                || key_str == "content-length" 
                || key_str == "accept-encoding"
                // 过滤认证相关 Header
                || key_str == "x-api-key"
                || key_str == "authorization"
                || key_str == "x-goog-api-key"
                || key_str == "anthropic-version"
            {
                continue;
            }

            request = request.header(key, value);
        }

        // 确保 Content-Type 是 json
        request = request.header("Content-Type", "application/json");

        // 添加认证头
        request = self.add_auth_headers(request, provider)?;

        // 发送请求
        let response = request.json(body).send().await.map_err(|e| {
            log::error!("Request Failed: {e}");
            if e.is_timeout() {
                ProxyError::Timeout(format!("请求超时: {e}"))
            } else if e.is_connect() {
                ProxyError::ForwardFailed(format!("连接失败: {e}"))
            } else {
                ProxyError::ForwardFailed(e.to_string())
            }
        })?;

        // 检查响应状态
        let status = response.status();

        if status.is_success() {
            Ok(response)
        } else {
            let status_code = status.as_u16();
            let body_text = response.text().await.ok();

            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
    }

    /// 添加认证头
    fn add_auth_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        provider: &Provider,
    ) -> Result<reqwest::RequestBuilder, ProxyError> {
        // 提取 apiKey 和认证类型
        if let Some((api_key, auth_type)) = self.extract_api_key(provider) {
            // 遮蔽 key 用于日志
            let _masked_key = if api_key.len() > 8 {
                format!("{}...{}", &api_key[..4], &api_key[api_key.len() - 4..])
            } else {
                "***".to_string()
            };

            match auth_type {
                AuthType::Anthropic => {
                    request = request.header("x-api-key", api_key);
                    request = request.header("anthropic-version", "2023-06-01");
                }
                AuthType::Gemini => {
                    request = request.header("x-goog-api-key", api_key);
                }
                AuthType::Bearer => {
                    request = request.header("Authorization", format!("Bearer {api_key}"));
                }
            }
        } else {
            log::error!("✗ 未找到 API Key！将发送未认证的请求（会失败）");
            log::error!("Provider 配置: {:?}", provider.settings_config);
        }

        Ok(request)
    }

    /// 从 Provider 配置中提取 base_url
    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        log::debug!("Extracting base_url for provider: {}", provider.name);

        // 1. 尝试直接获取 base_url 字段 (Codex CLI 常用格式)
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            log::debug!("Found base_url in direct field: {url}");
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 2. 尝试从 env 中获取 (Claude / Gemini)
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
                log::debug!("Found base_url in env.ANTHROPIC_BASE_URL: {url}");
                return Ok(url.trim_end_matches('/').to_string());
            }
            if let Some(url) = env.get("GOOGLE_GEMINI_BASE_URL").and_then(|v| v.as_str()) {
                log::debug!("Found base_url in env.GOOGLE_GEMINI_BASE_URL: {url}");
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 3. 尝试其他通用字段
        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            log::debug!("Found base_url in baseURL: {url}");
            return Ok(url.trim_end_matches('/').to_string());
        }
        if let Some(url) = provider
            .settings_config
            .get("apiEndpoint")
            .and_then(|v| v.as_str())
        {
            log::debug!("Found base_url in apiEndpoint: {url}");
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 4. 尝试从 config 对象中获取 (Codex - JSON 格式)
        if let Some(config) = provider.settings_config.get("config") {
            // 如果 config 是一个对象
            if let Some(url) = config.get("base_url").and_then(|v| v.as_str()) {
                log::debug!("Found base_url in config.base_url: {url}");
                return Ok(url.trim_end_matches('/').to_string());
            }

            // 如果 config 是一个字符串，尝试解析
            if let Some(config_str) = config.as_str() {
                // 尝试双引号
                if let Some(start) = config_str.find("base_url = \"") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('"') {
                        let url = rest[..end].trim_end_matches('/').to_string();
                        log::debug!("Found base_url in config string (double quotes): {url}");
                        return Ok(url);
                    }
                }
                // 尝试单引号
                if let Some(start) = config_str.find("base_url = '") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('\'') {
                        let url = rest[..end].trim_end_matches('/').to_string();
                        log::debug!("Found base_url in config string (single quotes): {url}");
                        return Ok(url);
                    }
                }
            }
        }

        log::error!(
            "Failed to extract base_url from config: {:?}",
            provider.settings_config
        );
        Err(ProxyError::ConfigError(
            "Provider缺少base_url配置".to_string(),
        ))
    }

    /// 从 Provider 配置中提取 api_key
    fn extract_api_key(&self, provider: &Provider) -> Option<(String, AuthType)> {
        // 1. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            // Claude/Anthropic
            if let Some(key) = env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()) {
                return Some((key.to_string(), AuthType::Anthropic));
            }

            // Gemini
            if let Some(key) = env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
                return Some((key.to_string(), AuthType::Gemini));
            }

            // OpenAI/Codex (env 中的 OPENAI_API_KEY)
            if let Some(key) = env.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                return Some((key.to_string(), AuthType::Bearer));
            }
        }

        // 2. 尝试从 auth 中获取 (Codex CLI 格式)
        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                return Some((key.to_string(), AuthType::Bearer));
            }
        }

        // 3. 尝试直接获取 (支持 apiKey 和 api_key)
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            return Some((key.to_string(), AuthType::Bearer));
        }

        // 4. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(key) = config
                .get("api_key")
                .or_else(|| config.get("apiKey"))
                .and_then(|v| v.as_str())
            {
                return Some((key.to_string(), AuthType::Bearer));
            }
        }

        log::error!("✗ 所有位置都未找到 API Key！");
        log::error!("完整配置结构: {:?}", provider.settings_config);
        None
    }

    /// 分类ProxyError
    fn categorize_proxy_error(&self, error: &ProxyError) -> ErrorCategory {
        match error {
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            ProxyError::UpstreamError { status, .. } => {
                if *status >= 500 {
                    ErrorCategory::Retryable
                } else if *status >= 400 && *status < 500 {
                    ErrorCategory::NonRetryable
                } else {
                    ErrorCategory::Retryable
                }
            }
            ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            _ => ErrorCategory::NonRetryable,
        }
    }
}

enum AuthType {
    Anthropic,
    Gemini,
    Bearer,
}

impl RequestForwarder {
    /// 转发 GET 请求（带重试和故障转移）
    pub async fn forward_get_request(
        &self,
        app_type: &AppType,
        endpoint: &str,
        headers: axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        let mut failed_ids = Vec::new();

        for attempt in 0..self.max_retries {
            let provider = self.router.select_provider(app_type, &failed_ids).await?;

            log::debug!(
                "GET 尝试 {} - 使用Provider: {} ({})",
                attempt + 1,
                provider.name,
                provider.id
            );

            match self.forward_get(&provider, endpoint, &headers).await {
                Ok(response) => {
                    self.router
                        .update_health(&provider, app_type, true, None)
                        .await;
                    return Ok(response);
                }
                Err(e) => {
                    let category = self.categorize_proxy_error(&e);
                    match category {
                        ErrorCategory::Retryable => {
                            self.router
                                .update_health(&provider, app_type, false, Some(e.to_string()))
                                .await;
                            failed_ids.push(provider.id.clone());
                            continue;
                        }
                        _ => return Err(e),
                    }
                }
            }
        }

        Err(ProxyError::MaxRetriesExceeded)
    }

    /// 转发 DELETE 请求（带重试和故障转移）
    pub async fn forward_delete_request(
        &self,
        app_type: &AppType,
        endpoint: &str,
        headers: axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        let mut failed_ids = Vec::new();

        for attempt in 0..self.max_retries {
            let provider = self.router.select_provider(app_type, &failed_ids).await?;

            log::debug!(
                "DELETE 尝试 {} - 使用Provider: {} ({})",
                attempt + 1,
                provider.name,
                provider.id
            );

            match self.forward_delete(&provider, endpoint, &headers).await {
                Ok(response) => {
                    self.router
                        .update_health(&provider, app_type, true, None)
                        .await;
                    return Ok(response);
                }
                Err(e) => {
                    let category = self.categorize_proxy_error(&e);
                    match category {
                        ErrorCategory::Retryable => {
                            self.router
                                .update_health(&provider, app_type, false, Some(e.to_string()))
                                .await;
                            failed_ids.push(provider.id.clone());
                            continue;
                        }
                        _ => return Err(e),
                    }
                }
            }
        }

        Err(ProxyError::MaxRetriesExceeded)
    }

    /// 转发单个 GET 请求
    async fn forward_get(
        &self,
        provider: &Provider,
        endpoint: &str,
        headers: &axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        let base_url = self.extract_base_url(provider)?;
        let url = if base_url.ends_with("/v1") && endpoint.starts_with("/v1") {
            format!("{}{}", base_url.trim_end_matches("/v1"), endpoint)
        } else {
            format!("{base_url}{endpoint}")
        };

        log::info!("Proxy GET Request URL: {url}");

        let mut request = self.client.get(&url);

        // 透传 Headers
        for (key, value) in headers {
            let key_str = key.as_str().to_lowercase();
            if key_str == "host"
                || key_str == "content-length"
                || key_str == "accept-encoding"
                || key_str == "x-api-key"
                || key_str == "authorization"
                || key_str == "x-goog-api-key"
                || key_str == "anthropic-version"
            {
                continue;
            }
            request = request.header(key, value);
        }

        request = self.add_auth_headers(request, provider)?;

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ProxyError::Timeout(format!("请求超时: {e}"))
            } else if e.is_connect() {
                ProxyError::ForwardFailed(format!("连接失败: {e}"))
            } else {
                ProxyError::ForwardFailed(e.to_string())
            }
        })?;

        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let status_code = status.as_u16();
            let body_text = response.text().await.ok();
            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
    }

    /// 转发单个 DELETE 请求
    async fn forward_delete(
        &self,
        provider: &Provider,
        endpoint: &str,
        headers: &axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        let base_url = self.extract_base_url(provider)?;
        let url = if base_url.ends_with("/v1") && endpoint.starts_with("/v1") {
            format!("{}{}", base_url.trim_end_matches("/v1"), endpoint)
        } else {
            format!("{base_url}{endpoint}")
        };

        log::info!("Proxy DELETE Request URL: {url}");

        let mut request = self.client.delete(&url);

        // 透传 Headers
        for (key, value) in headers {
            let key_str = key.as_str().to_lowercase();
            if key_str == "host"
                || key_str == "content-length"
                || key_str == "accept-encoding"
                || key_str == "x-api-key"
                || key_str == "authorization"
                || key_str == "x-goog-api-key"
                || key_str == "anthropic-version"
            {
                continue;
            }
            request = request.header(key, value);
        }

        request = self.add_auth_headers(request, provider)?;

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ProxyError::Timeout(format!("请求超时: {e}"))
            } else if e.is_connect() {
                ProxyError::ForwardFailed(format!("连接失败: {e}"))
            } else {
                ProxyError::ForwardFailed(e.to_string())
            }
        })?;

        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let status_code = status.as_u16();
            let body_text = response.text().await.ok();
            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
    }
}
