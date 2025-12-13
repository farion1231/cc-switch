//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持重试和故障转移

use super::{
    error::*,
    provider_router::ProviderRouter as NewProviderRouter,
    providers::{get_adapter, ProviderAdapter},
    types::ProxyStatus,
    ProxyError,
};
use crate::{app_config::AppType, database::Database, provider::Provider};
use reqwest::{Client, Response};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct RequestForwarder {
    client: Client,
    router: Arc<NewProviderRouter>,
    #[allow(dead_code)]
    max_retries: u8,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
}

impl RequestForwarder {
    pub fn new(
        db: Arc<Database>,
        timeout_secs: u64,
        max_retries: u8,
        status: Arc<RwLock<ProxyStatus>>,
        current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
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
            router: Arc::new(NewProviderRouter::new(db)),
            max_retries,
            status,
            current_providers,
        }
    }

    /// 转发请求（带故障转移）
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
    ) -> Result<Response, ProxyError> {
        // 获取适配器
        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();

        // 使用新的 ProviderRouter 选择所有可用供应商
        let providers = self
            .router
            .select_providers(app_type_str)
            .await
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        if providers.is_empty() {
            return Err(ProxyError::NoAvailableProvider);
        }

        log::info!(
            "[{}] 故障转移链: {} 个可用供应商",
            app_type_str,
            providers.len()
        );

        let mut last_error = None;
        let mut failover_happened = false;

        // 依次尝试每个供应商
        for (attempt, provider) in providers.iter().enumerate() {
            log::info!(
                "[{}] 尝试 {}/{} - 使用Provider: {} (sort_index: {})",
                app_type_str,
                attempt + 1,
                providers.len(),
                provider.name,
                provider.sort_index.unwrap_or(999999)
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
            match self
                .forward(provider, endpoint, &body, &headers, adapter.as_ref())
                .await
            {
                Ok(response) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // 成功：记录成功并更新熔断器
                    if let Err(e) = self
                        .router
                        .record_result(&provider.id, app_type_str, true, None)
                        .await
                    {
                        log::warn!("Failed to record success: {e}");
                    }

                    // 更新当前应用类型使用的 provider
                    {
                        let mut current_providers = self.current_providers.write().await;
                        current_providers.insert(
                            app_type_str.to_string(),
                            (provider.id.clone(), provider.name.clone()),
                        );
                    }

                    // 更新成功统计
                    {
                        let mut status = self.status.write().await;
                        status.success_requests += 1;
                        status.last_error = None;
                        if failover_happened {
                            status.failover_count += 1;
                            log::info!(
                                "[{}] 故障转移成功！切换到 Provider: {} (耗时: {}ms)",
                                app_type_str,
                                provider.name,
                                latency
                            );
                        }
                        // 重新计算成功率
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    log::info!(
                        "[{}] 请求成功 - Provider: {} - {}ms",
                        app_type_str,
                        provider.name,
                        latency
                    );

                    return Ok(response);
                }
                Err(e) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // 失败：记录失败并更新熔断器
                    if let Err(record_err) = self
                        .router
                        .record_result(&provider.id, app_type_str, false, Some(e.to_string()))
                        .await
                    {
                        log::warn!("Failed to record failure: {record_err}");
                    }

                    // 分类错误
                    let category = self.categorize_proxy_error(&e);

                    match category {
                        ErrorCategory::Retryable => {
                            // 可重试：更新错误信息，继续尝试下一个供应商
                            {
                                let mut status = self.status.write().await;
                                status.last_error =
                                    Some(format!("Provider {} 失败: {}", provider.name, e));
                            }

                            log::warn!(
                                "[{}] Provider {} 失败（可重试）: {} - {}ms",
                                app_type_str,
                                provider.name,
                                e,
                                latency
                            );

                            last_error = Some(e);
                            // 继续尝试下一个供应商
                            continue;
                        }
                        ErrorCategory::NonRetryable | ErrorCategory::ClientAbort => {
                            // 不可重试：直接返回错误
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
                            log::error!(
                                "[{}] Provider {} 失败（不可重试）: {}",
                                app_type_str,
                                provider.name,
                                e
                            );
                            return Err(e);
                        }
                    }
                }
            }
        }

        // 所有供应商都失败了
        {
            let mut status = self.status.write().await;
            status.failed_requests += 1;
            status.last_error = Some("所有供应商都失败".to_string());
            if status.total_requests > 0 {
                status.success_rate =
                    (status.success_requests as f32 / status.total_requests as f32) * 100.0;
            }
        }

        log::error!(
            "[{}] 所有 {} 个供应商都失败了",
            app_type_str,
            providers.len()
        );

        Err(last_error.unwrap_or(ProxyError::MaxRetriesExceeded))
    }

    /// 转发单个请求（使用适配器）
    async fn forward(
        &self,
        provider: &Provider,
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
        adapter: &dyn ProviderAdapter,
    ) -> Result<Response, ProxyError> {
        // 使用适配器提取 base_url
        let base_url = adapter.extract_base_url(provider)?;
        log::info!("[{}] base_url: {}", adapter.name(), base_url);

        // 使用适配器构建 URL
        let url = adapter.build_url(&base_url, endpoint);

        // 检查是否需要格式转换
        let needs_transform = adapter.needs_transform(provider);

        // 记录原始请求 JSON
        log::info!(
            "[{}] ====== 请求开始 ======\n>>> 原始请求 JSON:\n{}",
            adapter.name(),
            serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string())
        );

        // 转换请求体（如果需要）
        let request_body = if needs_transform {
            log::info!("[{}] 转换请求格式 (Anthropic → OpenAI)", adapter.name());
            let transformed = adapter.transform_request(body.clone(), provider)?;
            log::info!(
                "[{}] >>> 转换后的请求 JSON:\n{}",
                adapter.name(),
                serde_json::to_string_pretty(&transformed).unwrap_or_default()
            );
            transformed
        } else {
            body.clone()
        };

        log::info!(
            "[{}] 转发请求: {} -> {}",
            adapter.name(),
            provider.name,
            url
        );

        // 构建请求
        let mut request = self.client.post(&url);

        // 只透传必要的 Headers（白名单模式）
        let allowed_headers = [
            "accept",
            "user-agent",
            "x-request-id",
            "x-stainless-arch",
            "x-stainless-lang",
            "x-stainless-os",
            "x-stainless-package-version",
            "x-stainless-runtime",
            "x-stainless-runtime-version",
        ];

        for (key, value) in headers {
            let key_str = key.as_str().to_lowercase();
            if allowed_headers.contains(&key_str.as_str()) {
                request = request.header(key, value);
            }
        }

        // 确保 Content-Type 是 json
        request = request.header("Content-Type", "application/json");

        // 使用适配器添加认证头
        if let Some(auth) = adapter.extract_auth(provider) {
            log::debug!(
                "[{}] 使用认证: {:?} (key: {})",
                adapter.name(),
                auth.strategy,
                auth.masked_key()
            );
            request = adapter.add_auth_headers(request, &auth);
        } else {
            log::error!(
                "[{}] 未找到 API Key！Provider: {}",
                adapter.name(),
                provider.name
            );
        }

        // 发送请求
        log::info!("[{}] 发送请求到: {}", adapter.name(), url);
        let response = request.json(&request_body).send().await.map_err(|e| {
            log::error!("[{}] 请求失败: {}", adapter.name(), e);
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
        log::info!("[{}] 响应状态: {}", adapter.name(), status);

        if status.is_success() {
            Ok(response)
        } else {
            let status_code = status.as_u16();
            let body_text = response.text().await.ok();
            log::error!(
                "[{}] 上游错误 ({}): {:?}",
                adapter.name(),
                status_code,
                body_text
            );

            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
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
