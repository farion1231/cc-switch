//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持重试和故障转移

use super::{
    error::*,
    providers::{get_adapter, ProviderAdapter},
    router::ProviderRouter,
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

        // 获取适配器
        let adapter = get_adapter(app_type);

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
            match self
                .forward(&provider, endpoint, &body, &headers, adapter.as_ref())
                .await
            {
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
