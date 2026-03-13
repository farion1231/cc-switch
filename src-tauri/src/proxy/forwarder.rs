//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持故障转移

use super::{
    body_filter::filter_private_params_with_whitelist,
    error::*,
    failover_switch::FailoverSwitchManager,
    log_codes::fwd as log_fwd,
    provider_router::ProviderRouter,
    providers::{get_adapter, AuthInfo, AuthStrategy, ProviderAdapter, ProviderType},
    thinking_budget_rectifier::{rectify_thinking_budget, should_rectify_thinking_budget},
    thinking_rectifier::{
        normalize_thinking_type, rectify_anthropic_request, should_rectify_thinking_signature,
    },
    types::{OptimizerConfig, ProxyStatus, RectifierConfig},
    ProxyError,
};
use crate::commands::CopilotAuthState;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::{app_config::AppType, provider::Provider};
use reqwest::Response;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;
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
    "x-goog-api-key",
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
    /// 整流器配置
    rectifier_config: RectifierConfig,
    /// 优化器配置
    optimizer_config: OptimizerConfig,
    /// 非流式请求超时（秒）
    non_streaming_timeout: std::time::Duration,
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
        rectifier_config: RectifierConfig,
        optimizer_config: OptimizerConfig,
    ) -> Self {
        Self {
            router,
            status,
            current_providers,
            failover_manager,
            app_handle,
            current_provider_id_at_start,
            rectifier_config,
            optimizer_config,
            non_streaming_timeout: std::time::Duration::from_secs(non_streaming_timeout),
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

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 整流器重试标记：确保整流最多触发一次
        let mut rectifier_retried = false;
        let mut budget_rectifier_retried = false;

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
                continue;
            }

            // PRE-SEND 优化器：每个 provider 独立决定是否优化
            // clone body 以避免 Bedrock 优化字段泄漏到非 Bedrock provider（failover 场景）
            let mut provider_body =
                if self.optimizer_config.enabled && is_bedrock_provider(provider) {
                    let mut b = body.clone();
                    if self.optimizer_config.thinking_optimizer {
                        super::thinking_optimizer::optimize(&mut b, &self.optimizer_config);
                    }
                    if self.optimizer_config.cache_injection {
                        super::cache_injector::inject(&mut b, &self.optimizer_config);
                    }
                    b
                } else {
                    body.clone()
                };

            attempted_providers += 1;

            // 更新状态中的当前Provider信息
            {
                let mut status = self.status.write().await;
                status.current_provider = Some(provider.name.clone());
                status.current_provider_id = Some(provider.id.clone());
                status.total_requests += 1;
                status.last_request_at = Some(chrono::Utc::now().to_rfc3339());
            }

            // 转发请求（每个 Provider 只尝试一次，重试由客户端控制）
            match self
                .forward(
                    provider,
                    endpoint,
                    &provider_body,
                    &headers,
                    adapter.as_ref(),
                )
                .await
            {
                Ok(response) => {
                    // 成功：记录成功并更新熔断器
                    let _ = self
                        .router
                        .record_result(
                            &provider.id,
                            app_type_str,
                            used_half_open_permit,
                            true,
                            None,
                        )
                        .await;

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

                            // 异步触发供应商切换，更新 UI/托盘，并把“当前供应商”同步为实际使用的 provider
                            let fm = self.failover_manager.clone();
                            let ah = self.app_handle.clone();
                            let pid = provider.id.clone();
                            let pname = provider.name.clone();
                            let at = app_type_str.to_string();

                            tokio::spawn(async move {
                                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
                            });
                        }
                        // 重新计算成功率
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    return Ok(ForwardResult {
                        response,
                        provider: provider.clone(),
                    });
                }
                Err(e) => {
                    // 检测是否需要触发整流器（仅 Claude/ClaudeAuth 供应商）
                    let provider_type = ProviderType::from_app_type_and_config(app_type, provider);
                    let is_anthropic_provider = matches!(
                        provider_type,
                        ProviderType::Claude | ProviderType::ClaudeAuth
                    );
                    let mut signature_rectifier_non_retryable_client_error = false;

                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_signature(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if rectifier_retried {
                                log::warn!("[{app_type_str}] [RECT-005] 整流器已触发过，不再重试");
                                // 释放 HalfOpen permit（不记录熔断器，这是客户端兼容性问题）
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            // 首次触发：整流请求体
                            let rectified = rectify_anthropic_request(&mut provider_body);

                            // 整流未生效：继续尝试 budget 整流路径，避免误判后短路
                            if !rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-006] thinking 签名整流器触发但无可整流内容，继续检查 budget；若 budget 也未命中则按客户端错误返回"
                                );
                                signature_rectifier_non_retryable_client_error = true;
                            } else {
                                log::info!(
                                    "[{}] [RECT-001] thinking 签名整流器触发, 移除 {} thinking blocks, {} redacted_thinking blocks, {} signature fields",
                                    app_type_str,
                                    rectified.removed_thinking_blocks,
                                    rectified.removed_redacted_thinking_blocks,
                                    rectified.removed_signature_fields
                                );

                                // 标记已重试（当前逻辑下重试后必定 return，保留标记以备将来扩展）
                                let _ = std::mem::replace(&mut rectifier_retried, true);

                                // 使用同一供应商重试（不计入熔断器）
                                match self
                                    .forward(
                                        provider,
                                        endpoint,
                                        &provider_body,
                                        &headers,
                                        adapter.as_ref(),
                                    )
                                    .await
                                {
                                    Ok(response) => {
                                        log::info!("[{app_type_str}] [RECT-002] 整流重试成功");
                                        // 记录成功
                                        let _ = self
                                            .router
                                            .record_result(
                                                &provider.id,
                                                app_type_str,
                                                used_half_open_permit,
                                                true,
                                                None,
                                            )
                                            .await;

                                        // 更新当前应用类型使用的 provider
                                        {
                                            let mut current_providers =
                                                self.current_providers.write().await;
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
                                                self.current_provider_id_at_start.as_str()
                                                    != provider.id.as_str();
                                            if should_switch {
                                                status.failover_count += 1;

                                                // 异步触发供应商切换，更新 UI/托盘
                                                let fm = self.failover_manager.clone();
                                                let ah = self.app_handle.clone();
                                                let pid = provider.id.clone();
                                                let pname = provider.name.clone();
                                                let at = app_type_str.to_string();

                                                tokio::spawn(async move {
                                                    let _ = fm
                                                        .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                        .await;
                                                });
                                            }
                                            if status.total_requests > 0 {
                                                status.success_rate = (status.success_requests
                                                    as f32
                                                    / status.total_requests as f32)
                                                    * 100.0;
                                            }
                                        }

                                        return Ok(ForwardResult {
                                            response,
                                            provider: provider.clone(),
                                        });
                                    }
                                    Err(retry_err) => {
                                        // 整流重试仍失败：区分错误类型决定是否记录熔断器
                                        log::warn!(
                                            "[{app_type_str}] [RECT-003] 整流重试仍失败: {retry_err}"
                                        );

                                        // 区分错误类型：Provider 问题记录失败，客户端问题仅释放 permit
                                        let is_provider_error = match &retry_err {
                                            ProxyError::Timeout(_)
                                            | ProxyError::ForwardFailed(_) => true,
                                            ProxyError::UpstreamError { status, .. } => {
                                                *status >= 500
                                            }
                                            _ => false,
                                        };

                                        if is_provider_error {
                                            // Provider 问题：记录失败到熔断器
                                            let _ = self
                                                .router
                                                .record_result(
                                                    &provider.id,
                                                    app_type_str,
                                                    used_half_open_permit,
                                                    false,
                                                    Some(retry_err.to_string()),
                                                )
                                                .await;
                                        } else {
                                            // 客户端问题：仅释放 permit，不记录熔断器
                                            self.router
                                                .release_permit_neutral(
                                                    &provider.id,
                                                    app_type_str,
                                                    used_half_open_permit,
                                                )
                                                .await;
                                        }

                                        let mut status = self.status.write().await;
                                        status.failed_requests += 1;
                                        status.last_error = Some(retry_err.to_string());
                                        if status.total_requests > 0 {
                                            status.success_rate = (status.success_requests as f32
                                                / status.total_requests as f32)
                                                * 100.0;
                                        }
                                        return Err(ForwardError {
                                            error: retry_err,
                                            provider: Some(provider.clone()),
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // 检测是否需要触发 budget 整流器（仅 Claude/ClaudeAuth 供应商）
                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_budget(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if budget_rectifier_retried {
                                log::warn!(
                                    "[{app_type_str}] [RECT-013] budget 整流器已触发过，不再重试"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            let budget_rectified = rectify_thinking_budget(&mut provider_body);
                            if !budget_rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-014] budget 整流器触发但无可整流内容，不做无意义重试"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            log::info!(
                                "[{}] [RECT-010] thinking budget 整流器触发, before={:?}, after={:?}",
                                app_type_str,
                                budget_rectified.before,
                                budget_rectified.after
                            );

                            let _ = std::mem::replace(&mut budget_rectifier_retried, true);

                            // 使用同一供应商重试（不计入熔断器）
                            match self
                                .forward(
                                    provider,
                                    endpoint,
                                    &provider_body,
                                    &headers,
                                    adapter.as_ref(),
                                )
                                .await
                            {
                                Ok(response) => {
                                    log::info!("[{app_type_str}] [RECT-011] budget 整流重试成功");
                                    let _ = self
                                        .router
                                        .record_result(
                                            &provider.id,
                                            app_type_str,
                                            used_half_open_permit,
                                            true,
                                            None,
                                        )
                                        .await;

                                    {
                                        let mut current_providers =
                                            self.current_providers.write().await;
                                        current_providers.insert(
                                            app_type_str.to_string(),
                                            (provider.id.clone(), provider.name.clone()),
                                        );
                                    }

                                    {
                                        let mut status = self.status.write().await;
                                        status.success_requests += 1;
                                        status.last_error = None;
                                        let should_switch =
                                            self.current_provider_id_at_start.as_str()
                                                != provider.id.as_str();
                                        if should_switch {
                                            status.failover_count += 1;
                                            let fm = self.failover_manager.clone();
                                            let ah = self.app_handle.clone();
                                            let pid = provider.id.clone();
                                            let pname = provider.name.clone();
                                            let at = app_type_str.to_string();
                                            tokio::spawn(async move {
                                                let _ = fm
                                                    .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                    .await;
                                            });
                                        }
                                        if status.total_requests > 0 {
                                            status.success_rate = (status.success_requests as f32
                                                / status.total_requests as f32)
                                                * 100.0;
                                        }
                                    }

                                    return Ok(ForwardResult {
                                        response,
                                        provider: provider.clone(),
                                    });
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [RECT-012] budget 整流重试仍失败: {retry_err}"
                                    );

                                    let is_provider_error = match &retry_err {
                                        ProxyError::Timeout(_) | ProxyError::ForwardFailed(_) => {
                                            true
                                        }
                                        ProxyError::UpstreamError { status, .. } => *status >= 500,
                                        _ => false,
                                    };

                                    if is_provider_error {
                                        let _ = self
                                            .router
                                            .record_result(
                                                &provider.id,
                                                app_type_str,
                                                used_half_open_permit,
                                                false,
                                                Some(retry_err.to_string()),
                                            )
                                            .await;
                                    } else {
                                        self.router
                                            .release_permit_neutral(
                                                &provider.id,
                                                app_type_str,
                                                used_half_open_permit,
                                            )
                                            .await;
                                    }

                                    let mut status = self.status.write().await;
                                    status.failed_requests += 1;
                                    status.last_error = Some(retry_err.to_string());
                                    if status.total_requests > 0 {
                                        status.success_rate = (status.success_requests as f32
                                            / status.total_requests as f32)
                                            * 100.0;
                                    }
                                    return Err(ForwardError {
                                        error: retry_err,
                                        provider: Some(provider.clone()),
                                    });
                                }
                            }
                        }
                    }

                    if signature_rectifier_non_retryable_client_error {
                        self.router
                            .release_permit_neutral(
                                &provider.id,
                                app_type_str,
                                used_half_open_permit,
                            )
                            .await;
                        let mut status = self.status.write().await;
                        status.failed_requests += 1;
                        status.last_error = Some(e.to_string());
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                        return Err(ForwardError {
                            error: e,
                            provider: Some(provider.clone()),
                        });
                    }

                    // 失败：记录失败并更新熔断器
                    let _ = self
                        .router
                        .record_result(
                            &provider.id,
                            app_type_str,
                            used_half_open_permit,
                            false,
                            Some(e.to_string()),
                        )
                        .await;

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

                            let (log_code, log_message) = build_retryable_failure_log(
                                &provider.name,
                                attempted_providers,
                                providers.len(),
                                &e,
                            );
                            log::warn!("[{app_type_str}] [{log_code}] {log_message}");

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

        if let Some((log_code, log_message)) =
            build_terminal_failure_log(attempted_providers, providers.len(), last_error.as_ref())
        {
            log::warn!("[{app_type_str}] [{log_code}] {log_message}");
        }

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

        // 检查是否需要格式转换
        let needs_transform = adapter.needs_transform(provider);

        // 确定有效端点
        // GitHub Copilot API 使用 /chat/completions（无 /v1 前缀）
        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || base_url.contains("githubcopilot.com");
        let effective_endpoint =
            if needs_transform && adapter.name() == "Claude" && endpoint == "/v1/messages" {
                if is_copilot {
                    // GitHub Copilot uses /chat/completions without /v1 prefix
                    "/chat/completions"
                } else {
                    // 根据 api_format 选择目标端点
                    let api_format = super::providers::get_claude_api_format(provider);
                    if api_format == "openai_responses" {
                        "/v1/responses"
                    } else {
                        "/v1/chat/completions"
                    }
                }
            } else {
                endpoint
            };

        // 使用适配器构建 URL
        let url = adapter.build_url(&base_url, effective_endpoint);

        // 应用模型映射（独立于格式转换）
        let (mapped_body, _original_model, _mapped_model) =
            super::model_mapper::apply_model_mapping(body.clone(), provider);

        // 与 CCH 对齐：请求前不做 thinking 主动改写（仅保留兼容入口）
        let mapped_body = normalize_thinking_type(mapped_body);

        // 转换请求体（如果需要）
        let request_body = if needs_transform {
            adapter.transform_request(mapped_body, provider)?
        } else {
            mapped_body
        };

        // 过滤私有参数（以 `_` 开头的字段），防止内部信息泄露到上游
        // 默认使用空白名单，过滤所有 _ 前缀字段
        let filtered_body = filter_private_params_with_whitelist(request_body, &[]);

        // 获取 HTTP 客户端：优先使用供应商单独代理配置，否则使用全局客户端
        let proxy_config = provider.meta.as_ref().and_then(|m| m.proxy_config.as_ref());
        let client = super::http_client::get_for_provider(proxy_config);
        let mut request = client.post(&url);

        // 只有当 timeout > 0 时才设置请求超时
        // Duration::ZERO 在 reqwest 中表示"立刻超时"而不是"禁用超时"
        // 故障转移关闭时会传入 0，此时应该使用 client 的默认超时（600秒）
        if !self.non_streaming_timeout.is_zero() {
            request = request.timeout(self.non_streaming_timeout);
        }

        // 过滤黑名单 Headers，保护隐私并避免冲突
        for (key, value) in headers {
            if HEADER_BLACKLIST
                .iter()
                .any(|h| key.as_str().eq_ignore_ascii_case(h))
            {
                continue;
            }
            request = request.header(key, value);
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
        }

        // 客户端 IP 透传（默认开启）
        if let Some(xff) = headers.get("x-forwarded-for") {
            if let Ok(xff_str) = xff.to_str() {
                request = request.header("x-forwarded-for", xff_str);
            }
        }
        if let Some(real_ip) = headers.get("x-real-ip") {
            if let Ok(real_ip_str) = real_ip.to_str() {
                request = request.header("x-real-ip", real_ip_str);
            }
        }

        // 流式请求保守禁用压缩，避免上游压缩 SSE 在连接中断时触发解压错误。
        // 非流式请求不显式设置 Accept-Encoding，让 reqwest 自动协商压缩并透明解压。
        if should_force_identity_encoding(effective_endpoint, &filtered_body, headers) {
            request = request.header("accept-encoding", "identity");
        }

        // 使用适配器添加认证头
        if let Some(mut auth) = adapter.extract_auth(provider) {
            // GitHub Copilot 特殊处理：从 CopilotAuthManager 获取真实 token
            if auth.strategy == AuthStrategy::GitHubCopilot {
                if let Some(app_handle) = &self.app_handle {
                    let copilot_state = app_handle.state::<CopilotAuthState>();
                    let copilot_auth: tokio::sync::RwLockReadGuard<'_, CopilotAuthManager> =
                        copilot_state.0.read().await;

                    // 从 provider.meta 获取关联的 GitHub 账号 ID（多账号支持）
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("github_copilot"));

                    // 根据账号 ID 获取对应 token（向后兼容：无账号 ID 时使用第一个账号）
                    let token_result = match &account_id {
                        Some(id) => {
                            log::debug!("[Copilot] 使用指定账号 {id} 获取 token");
                            copilot_auth.get_valid_token_for_account(id).await
                        }
                        None => {
                            log::debug!("[Copilot] 使用默认账号获取 token");
                            copilot_auth.get_valid_token().await
                        }
                    };

                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::GitHubCopilot);
                            log::debug!(
                                "[Copilot] 成功获取 Copilot token (account={})",
                                account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[Copilot] 获取 Copilot token 失败 (account={}): {e}",
                                account_id.as_deref().unwrap_or("default")
                            );
                            return Err(ProxyError::AuthError(format!(
                                "GitHub Copilot 认证失败: {e}"
                            )));
                        }
                    }
                } else {
                    log::error!("[Copilot] AppHandle 不可用");
                    return Err(ProxyError::AuthError(
                        "GitHub Copilot 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }
            request = adapter.add_auth_headers(request, &auth);
        }

        // anthropic-version 统一处理（仅 Claude）：优先使用客户端的版本号，否则使用默认值
        // 注意：只设置一次，避免重复
        if adapter.name() == "Claude" {
            let version_str = headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("2023-06-01");
            request = request.header("anthropic-version", version_str);
        }

        // 输出请求信息日志
        let tag = adapter.name();
        let request_model = filtered_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        log::info!("[{tag}] >>> 请求 URL: {url} (model={request_model})");
        if let Ok(body_str) = serde_json::to_string(&filtered_body) {
            log::debug!(
                "[{tag}] >>> 请求体内容 ({}字节): {}",
                body_str.len(),
                body_str
            );
        }

        // 发送请求
        let response = request.json(&filtered_body).send().await.map_err(|e| {
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

/// 从 ProxyError 中提取错误消息
fn extract_error_message(error: &ProxyError) -> Option<String> {
    match error {
        ProxyError::UpstreamError { body, .. } => body.clone(),
        _ => Some(error.to_string()),
    }
}

/// 检测 Provider 是否为 Bedrock（通过 CLAUDE_CODE_USE_BEDROCK 环境变量判断）
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn build_retryable_failure_log(
    provider_name: &str,
    attempted_providers: usize,
    total_providers: usize,
    error: &ProxyError,
) -> (&'static str, String) {
    let error_summary = summarize_proxy_error(error);

    if total_providers <= 1 {
        (
            log_fwd::SINGLE_PROVIDER_FAILED,
            format!("Provider {provider_name} 请求失败: {error_summary}"),
        )
    } else {
        (
            log_fwd::PROVIDER_FAILED_RETRY,
            format!(
                "Provider {provider_name} 失败，继续尝试下一个 ({attempted_providers}/{total_providers}): {error_summary}"
            ),
        )
    }
}

fn build_terminal_failure_log(
    attempted_providers: usize,
    total_providers: usize,
    last_error: Option<&ProxyError>,
) -> Option<(&'static str, String)> {
    if total_providers <= 1 {
        return None;
    }

    let error_summary = last_error
        .map(summarize_proxy_error)
        .unwrap_or_else(|| "未知错误".to_string());

    Some((
        log_fwd::ALL_PROVIDERS_FAILED,
        format!(
            "已尝试 {attempted_providers}/{total_providers} 个 Provider，均失败。最后错误: {error_summary}"
        ),
    ))
}

fn summarize_proxy_error(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let body_summary = body
                .as_deref()
                .map(summarize_upstream_body)
                .filter(|summary| !summary.is_empty());

            match body_summary {
                Some(summary) => format!("上游 HTTP {status}: {summary}"),
                None => format!("上游 HTTP {status}"),
            }
        }
        ProxyError::Timeout(message) => {
            format!("请求超时: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ForwardFailed(message) => {
            format!("请求转发失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::TransformError(message) => {
            format!("响应转换失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ConfigError(message) => {
            format!("配置错误: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::AuthError(message) => {
            format!("认证失败: {}", summarize_text_for_log(message, 180))
        }
        _ => summarize_text_for_log(&error.to_string(), 180),
    }
}

fn summarize_upstream_body(body: &str) -> String {
    if let Ok(json_body) = serde_json::from_str::<Value>(body) {
        if let Some(message) = extract_json_error_message(&json_body) {
            return summarize_text_for_log(&message, 180);
        }

        if let Ok(compact_json) = serde_json::to_string(&json_body) {
            return summarize_text_for_log(&compact_json, 180);
        }
    }

    summarize_text_for_log(body, 180)
}

fn extract_json_error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.pointer("/message"),
        body.pointer("/detail"),
        body.pointer("/error"),
    ];

    candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().map(ToString::to_string))
}

fn should_force_identity_encoding(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

fn summarize_text_for_log(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    let truncated = truncated.trim_end();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::ACCEPT, HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn single_provider_retryable_log_uses_single_provider_code() {
        let error = ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
        };

        let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

        assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
        assert!(message.contains("Provider PackyCode-response 请求失败"));
        assert!(message.contains("上游 HTTP 429"));
        assert!(message.contains("rate limit exceeded"));
        assert!(!message.contains("切换下一个"));
    }

    #[test]
    fn multi_provider_retryable_log_keeps_failover_wording() {
        let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

        let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

        assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
        assert!(message.contains("继续尝试下一个 (1/3)"));
        assert!(message.contains("请求超时"));
    }

    #[test]
    fn single_provider_has_no_terminal_all_failed_log() {
        assert!(build_terminal_failure_log(1, 1, None).is_none());
    }

    #[test]
    fn multi_provider_terminal_log_contains_last_error_summary() {
        let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

        let (code, message) =
            build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

        assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
        assert!(message.contains("已尝试 2/2 个 Provider，均失败"));
        assert!(message.contains("connection reset by peer"));
    }

    #[test]
    fn summarize_upstream_body_prefers_json_message() {
        let body = json!({
            "error": {
                "message": "invalid_request_error: unsupported field"
            },
            "request_id": "req_123"
        });

        let summary = summarize_upstream_body(&body.to_string());

        assert_eq!(summary, "invalid_request_error: unsupported field");
    }

    #[test]
    fn summarize_text_for_log_collapses_whitespace_and_truncates() {
        let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

        assert_eq!(summary, "line1 line2...");
    }

    #[test]
    fn force_identity_for_stream_flag_requests() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "stream": true }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_gemini_stream_endpoints() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_sse_accept_header() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    #[test]
    fn non_streaming_requests_allow_automatic_compression() {
        let headers = HeaderMap::new();

        assert!(!should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }
}
