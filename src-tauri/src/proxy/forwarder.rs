//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持故障转移

use super::{
    body_filter::filter_private_params_with_whitelist,
    error::*,
    failover_switch::FailoverSwitchManager,
    provider_router::ProviderRouter,
    providers::{get_adapter, ProviderAdapter},
    types::ProxyStatus,
    ProxyError,
};
use crate::{app_config::AppType, provider::Provider};
use reqwest::{Client, Response};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Headers 黑名单 - 不透传到上游的 Headers
///
/// 精简版黑名单，只过滤必须覆盖或可能导致问题的 header
/// 参考成功透传的请求，保留更多原始 header
///
/// 注意：客户端 IP 类（x-forwarded-for, x-real-ip）默认透传
const HEADER_BLACKLIST: &[&str] = &[
    // 认证类（会被覆盖）
    "authorization",
    "x-api-key",
    // 连接类（由 HTTP 客户端管理）
    "host",
    "content-length",
    "transfer-encoding",
    // 编码类（会被覆盖为 identity）
    "accept-encoding",
    // 代理转发类（保留 x-forwarded-for 和 x-real-ip）
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "forwarded",
    // CDN/云服务商特定头
    "cf-connecting-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "true-client-ip",
    "fastly-client-ip",
    "x-azure-clientip",
    "x-azure-fdid",
    "x-azure-ref",
    "akamai-origin-hop",
    "x-akamai-config-log-detail",
    // 请求追踪类
    "x-request-id",
    "x-correlation-id",
    "x-trace-id",
    "x-amzn-trace-id",
    "x-b3-traceid",
    "x-b3-spanid",
    "x-b3-parentspanid",
    "x-b3-sampled",
    "traceparent",
    "tracestate",
    // anthropic 特定头单独处理，避免重复
    "anthropic-beta",
    "anthropic-version",
    // 客户端 IP 单独处理（默认透传）
    "x-forwarded-for",
    "x-real-ip",
];

pub struct ForwardResult {
    pub response: Response,
    pub provider: Provider,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
}

pub struct RequestForwarder {
    client: Client,
    /// 共享的 ProviderRouter（持有熔断器状态）
    router: Arc<ProviderRouter>,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    /// 故障转移切换管理器
    failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle，用于发射事件和更新托盘
    app_handle: Option<tauri::AppHandle>,
    /// 请求开始时的"当前供应商 ID"（用于判断是否需要同步 UI/托盘）
    current_provider_id_at_start: String,
}

impl RequestForwarder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Arc<ProviderRouter>,
        non_streaming_timeout: u64,
        status: Arc<RwLock<ProxyStatus>>,
        current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
        failover_manager: Arc<FailoverSwitchManager>,
        app_handle: Option<tauri::AppHandle>,
        current_provider_id_at_start: String,
        _streaming_first_byte_timeout: u64,
        _streaming_idle_timeout: u64,
    ) -> Self {
        // 全局超时设置为 1800 秒（30 分钟），确保业务层超时配置能正常工作
        // 参考 Claude Code Hub 的 undici 全局超时设计
        const GLOBAL_TIMEOUT_SECS: u64 = 1800;

        let mut client_builder = Client::builder();
        if non_streaming_timeout > 0 {
            // 使用配置的非流式超时
            client_builder = client_builder.timeout(Duration::from_secs(non_streaming_timeout));
        } else {
            // 禁用超时时使用全局超时作为保底
            client_builder = client_builder.timeout(Duration::from_secs(GLOBAL_TIMEOUT_SECS));
        }

        let client = client_builder
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            router,
            status,
            current_providers,
            failover_manager,
            app_handle,
            current_provider_id_at_start,
        }
    }

    /// 转发请求（带故障转移）
    ///
    /// # Arguments
    /// * `app_type` - 应用类型
    /// * `endpoint` - API 端点
    /// * `body` - 请求体
    /// * `headers` - 请求头
    /// * `providers` - 已选择的 Provider 列表（由 RequestContext 提供，避免重复调用 select_providers）
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        // 获取适配器
        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        log::info!(
            "[{}] 故障转移链: {} 个可用供应商",
            app_type_str,
            providers.len()
        );

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 单 Provider 场景下跳过熔断器检查（故障转移关闭时）
        let bypass_circuit_breaker = providers.len() == 1;

        // 依次尝试每个供应商
        for provider in providers.iter() {
            // 发起请求前先获取熔断器放行许可（HalfOpen 会占用探测名额）
            // 单 Provider 场景下跳过此检查，避免熔断器阻塞所有请求
            let (allowed, used_half_open_permit) = if bypass_circuit_breaker {
                (true, false)
            } else {
                let permit = self
                    .router
                    .allow_provider_request(&provider.id, app_type_str)
                    .await;
                (permit.allowed, permit.used_half_open_permit)
            };

            if !allowed {
                log::debug!(
                    "[{}] Provider {} 熔断器拒绝本次请求，跳过",
                    app_type_str,
                    provider.name
                );
                continue;
            }

            attempted_providers += 1;

            log::info!(
                "[{}] 尝试 {}/{} - 使用Provider: {} (sort_index: {})",
                app_type_str,
                attempted_providers,
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
            }

            let start = Instant::now();

            // 转发请求（每个 Provider 只尝试一次，重试由客户端控制）
            match self
                .forward(provider, endpoint, &body, &headers, adapter.as_ref())
                .await
            {
                Ok(response) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // 成功：记录成功并更新熔断器
                    if let Err(e) = self
                        .router
                        .record_result(
                            &provider.id,
                            app_type_str,
                            used_half_open_permit,
                            true,
                            None,
                        )
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
                        let should_switch =
                            self.current_provider_id_at_start.as_str() != provider.id.as_str();
                        if should_switch {
                            status.failover_count += 1;
                            log::info!(
                                "[{}] 代理目标已切换到 Provider: {} (耗时: {}ms)",
                                app_type_str,
                                provider.name,
                                latency
                            );

                            // 异步触发供应商切换，更新 UI/托盘，并把“当前供应商”同步为实际使用的 provider
                            let fm = self.failover_manager.clone();
                            let ah = self.app_handle.clone();
                            let pid = provider.id.clone();
                            let pname = provider.name.clone();
                            let at = app_type_str.to_string();

                            tokio::spawn(async move {
                                if let Err(e) = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await
                                {
                                    log::error!("[Failover] 切换供应商失败: {e}");
                                }
                            });
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

                    return Ok(ForwardResult {
                        response,
                        provider: provider.clone(),
                    });
                }
                Err(e) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // 失败：记录失败并更新熔断器
                    if let Err(record_err) = self
                        .router
                        .record_result(
                            &provider.id,
                            app_type_str,
                            used_half_open_permit,
                            false,
                            Some(e.to_string()),
                        )
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
                            last_provider = Some(provider.clone());
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
                            return Err(ForwardError {
                                error: e,
                                provider: Some(provider.clone()),
                            });
                        }
                    }
                }
            }
        }

        if attempted_providers == 0 {
            // providers 列表非空，但全部被熔断器拒绝（典型：HalfOpen 探测名额被占用）
            {
                let mut status = self.status.write().await;
                status.failed_requests += 1;
                status.last_error = Some("所有供应商暂时不可用（熔断器限制）".to_string());
                if status.total_requests > 0 {
                    status.success_rate =
                        (status.success_requests as f32 / status.total_requests as f32) * 100.0;
                }
            }
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
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

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
        })
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

        // 检查是否需要格式转换
        let needs_transform = adapter.needs_transform(provider);

        let effective_endpoint =
            if needs_transform && adapter.name() == "Claude" && endpoint == "/v1/messages" {
                "/v1/chat/completions"
            } else {
                endpoint
            };

        // 使用适配器构建 URL
        let url = adapter.build_url(&base_url, effective_endpoint);

        // 记录原始请求 JSON
        log::info!(
            "[{}] ====== 请求开始 ======\n>>> 原始请求 JSON:\n{}",
            adapter.name(),
            serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string())
        );

        // 应用模型映射（独立于格式转换）
        let (mapped_body, _original_model, mapped_model) =
            super::model_mapper::apply_model_mapping(body.clone(), provider);

        if let Some(ref mapped) = mapped_model {
            log::info!(
                "[{}] >>> 模型映射后的请求 JSON:\n{}",
                adapter.name(),
                serde_json::to_string_pretty(&mapped_body).unwrap_or_default()
            );
            log::info!("[{}] 模型已映射到: {}", adapter.name(), mapped);
        }

        // 转换请求体（如果需要）
        let request_body = if needs_transform {
            log::info!("[{}] 转换请求格式 (Anthropic → OpenAI)", adapter.name());
            let transformed = adapter.transform_request(mapped_body, provider)?;
            log::info!(
                "[{}] >>> 转换后的请求 JSON:\n{}",
                adapter.name(),
                serde_json::to_string_pretty(&transformed).unwrap_or_default()
            );
            transformed
        } else {
            mapped_body
        };

        // 过滤私有参数（以 `_` 开头的字段），防止内部信息泄露到上游
        // 默认使用空白名单，过滤所有 _ 前缀字段
        let filtered_body = filter_private_params_with_whitelist(request_body, &[]);

        // ========== 请求体日志（截断显示） ==========
        let body_str = serde_json::to_string_pretty(&filtered_body)
            .unwrap_or_else(|_| filtered_body.to_string());
        let body_preview = if body_str.len() > 2000 {
            format!(
                "{}...\n[截断，总长度: {} 字符]",
                &body_str[..2000],
                body_str.len()
            )
        } else {
            body_str
        };
        log::info!(
            "[{}] ====== 最终请求体 ======\n{}",
            adapter.name(),
            body_preview
        );

        log::info!(
            "[{}] 转发请求: {} -> {}",
            adapter.name(),
            provider.name,
            url
        );

        // 构建请求
        let mut request = self.client.post(&url);

        // ========== 详细 Headers 日志 ==========
        log::info!("[{}] ====== 客户端原始 Headers ======", adapter.name());
        for (key, value) in headers {
            log::info!(
                "[{}]   {}: {:?}",
                adapter.name(),
                key.as_str(),
                value.to_str().unwrap_or("<binary>")
            );
        }

        // 过滤黑名单 Headers，保护隐私并避免冲突
        let mut filtered_headers: Vec<String> = Vec::new();
        let mut passed_headers: Vec<(String, String)> = Vec::new();

        for (key, value) in headers {
            let key_str = key.as_str().to_lowercase();
            if HEADER_BLACKLIST.contains(&key_str.as_str()) {
                filtered_headers.push(key_str);
                continue;
            }
            let value_str = value.to_str().unwrap_or("<binary>").to_string();
            passed_headers.push((key.as_str().to_string(), value_str.clone()));
            request = request.header(key, value);
        }

        if !filtered_headers.is_empty() {
            log::info!(
                "[{}] ====== 被过滤的 Headers ({}) ======",
                adapter.name(),
                filtered_headers.len()
            );
            for h in &filtered_headers {
                log::info!("[{}]   - {}", adapter.name(), h);
            }
        }

        // 处理 anthropic-beta Header（仅 Claude）
        // 关键：确保包含 claude-code-20250219 标记，这是上游服务验证请求来源的依据
        // 如果客户端发送的 beta 标记中没有包含 claude-code-20250219，需要补充
        if adapter.name() == "Claude" {
            const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
            let beta_value = if let Some(beta) = headers.get("anthropic-beta") {
                if let Ok(beta_str) = beta.to_str() {
                    // 检查是否已包含 claude-code-20250219
                    if beta_str.contains(CLAUDE_CODE_BETA) {
                        beta_str.to_string()
                    } else {
                        // 补充 claude-code-20250219
                        format!("{CLAUDE_CODE_BETA},{beta_str}")
                    }
                } else {
                    CLAUDE_CODE_BETA.to_string()
                }
            } else {
                // 如果客户端没有发送，使用默认值
                CLAUDE_CODE_BETA.to_string()
            };
            request = request.header("anthropic-beta", &beta_value);
            passed_headers.push(("anthropic-beta".to_string(), beta_value.clone()));
            log::info!("[{}] 设置 anthropic-beta: {}", adapter.name(), beta_value);
        }

        // 客户端 IP 透传（默认开启）
        if let Some(xff) = headers.get("x-forwarded-for") {
            if let Ok(xff_str) = xff.to_str() {
                request = request.header("x-forwarded-for", xff_str);
                passed_headers.push(("x-forwarded-for".to_string(), xff_str.to_string()));
                log::debug!("[{}] 透传 x-forwarded-for: {}", adapter.name(), xff_str);
            }
        }
        if let Some(real_ip) = headers.get("x-real-ip") {
            if let Ok(real_ip_str) = real_ip.to_str() {
                request = request.header("x-real-ip", real_ip_str);
                passed_headers.push(("x-real-ip".to_string(), real_ip_str.to_string()));
                log::debug!("[{}] 透传 x-real-ip: {}", adapter.name(), real_ip_str);
            }
        }

        // 禁用压缩，避免 gzip 流式响应解析错误
        // 参考 CCH: undici 在连接提前关闭时会对不完整的 gzip 流抛出错误
        request = request.header("accept-encoding", "identity");
        passed_headers.push(("accept-encoding".to_string(), "identity".to_string()));

        // 使用适配器添加认证头
        if let Some(auth) = adapter.extract_auth(provider) {
            log::debug!(
                "[{}] 使用认证: {:?} (key: {})",
                adapter.name(),
                auth.strategy,
                auth.masked_key()
            );
            request = adapter.add_auth_headers(request, &auth);
            // 记录认证头（脱敏）
            passed_headers.push((
                "authorization".to_string(),
                format!("Bearer {}...", &auth.api_key[..8.min(auth.api_key.len())]),
            ));
            passed_headers.push((
                "x-api-key".to_string(),
                format!("{}...", &auth.api_key[..8.min(auth.api_key.len())]),
            ));
        } else {
            log::error!(
                "[{}] 未找到 API Key！Provider: {}",
                adapter.name(),
                provider.name
            );
        }

        // anthropic-version 统一处理（仅 Claude）：优先使用客户端的版本号，否则使用默认值
        // 注意：只设置一次，避免重复
        if adapter.name() == "Claude" {
            let version_str = headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("2023-06-01");
            request = request.header("anthropic-version", version_str);
            passed_headers.push(("anthropic-version".to_string(), version_str.to_string()));
            log::info!(
                "[{}] 设置 anthropic-version: {}",
                adapter.name(),
                version_str
            );
        }

        // ========== 最终发送的 Headers 日志 ==========
        log::info!(
            "[{}] ====== 最终发送的 Headers ({}) ======",
            adapter.name(),
            passed_headers.len()
        );
        for (k, v) in &passed_headers {
            log::info!("[{}]   {}: {}", adapter.name(), k, v);
        }

        // 发送请求
        log::info!("[{}] 发送请求到: {}", adapter.name(), url);
        let response = request.json(&filtered_body).send().await.map_err(|e| {
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

    fn categorize_proxy_error(&self, error: &ProxyError) -> ErrorCategory {
        match error {
            // 网络和上游错误：都应该尝试下一个供应商
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
            // 上游 HTTP 错误：无论状态码如何，都尝试下一个供应商
            // 原因：不同供应商有不同的限制和认证，一个供应商的 4xx 错误
            // 不代表其他供应商也会失败
            ProxyError::UpstreamError { .. } => ErrorCategory::Retryable,
            // Provider 级配置/转换问题：换一个 Provider 可能就能成功
            ProxyError::ConfigError(_) => ErrorCategory::Retryable,
            ProxyError::TransformError(_) => ErrorCategory::Retryable,
            ProxyError::AuthError(_) => ErrorCategory::Retryable,
            ProxyError::StreamIdleTimeout(_) => ErrorCategory::Retryable,
            // 无可用供应商：所有供应商都试过了，无法重试
            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            // 其他错误（数据库/内部错误等）：不是换供应商能解决的问题
            _ => ErrorCategory::NonRetryable,
        }
    }
}
