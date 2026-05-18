//! 流式健康检查服务
//!
//! 使用流式 API 进行快速健康检查，只需接收首个 chunk 即判定成功。

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::gemini_url::{normalize_gemini_model_id, resolve_gemini_native_url};
use crate::proxy::providers::copilot_auth;
use crate::proxy::providers::transform::anthropic_to_openai;
use crate::proxy::providers::transform_gemini::anthropic_to_gemini;
use crate::proxy::providers::transform_responses::anthropic_to_responses;
use crate::proxy::providers::{
    get_adapter, AuthInfo, AuthStrategy, ClaudeAdapter, ProviderAdapter,
};

/// 健康状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

/// 流式检查配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub degraded_threshold_ms: u64,
    /// Claude 测试模型
    pub claude_model: String,
    /// Codex 测试模型
    pub codex_model: String,
    /// Gemini 测试模型
    pub gemini_model: String,
    /// 检查提示词
    #[serde(default = "default_test_prompt")]
    pub test_prompt: String,
}

fn default_test_prompt() -> String {
    "Who are you?".to_string()
}

impl Default for StreamCheckConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 45,
            max_retries: 2,
            degraded_threshold_ms: 6000,
            claude_model: "claude-haiku-4-5-20251001".to_string(),
            codex_model: "gpt-5.4@low".to_string(),
            gemini_model: "gemini-3-flash-preview".to_string(),
            test_prompt: default_test_prompt(),
        }
    }
}

/// 流式检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckResult {
    pub status: HealthStatus,
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub model_used: String,
    pub tested_at: i64,
    pub retry_count: u32,
    /// 细粒度错误分类（如 "modelNotFound"），前端据此渲染专门的文案
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
}

/// 流式健康检查服务
pub struct StreamCheckService;

impl StreamCheckService {
    /// 执行流式健康检查（带重试）
    ///
    /// 如果 Provider 配置了单独的测试配置（meta.testConfig），则使用该配置覆盖全局配置
    pub async fn check_with_retry(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        auth_override: Option<AuthInfo>,
        base_url_override: Option<String>,
        claude_api_format_override: Option<String>,
    ) -> Result<StreamCheckResult, AppError> {
        // 合并供应商单独配置和全局配置
        let effective_config = Self::merge_provider_config(provider, config);
        let mut last_result = None;

        for attempt in 0..=effective_config.max_retries {
            let result = Self::check_once(
                app_type,
                provider,
                &effective_config,
                auth_override.clone(),
                base_url_override.clone(),
                claude_api_format_override.clone(),
            )
            .await;

            match &result {
                Ok(r) if r.success => {
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..r.clone()
                    });
                }
                Ok(r) => {
                    // 失败但非异常，判断是否重试
                    if Self::should_retry(&r.message) && attempt < effective_config.max_retries {
                        last_result = Some(r.clone());
                        continue;
                    }
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..r.clone()
                    });
                }
                Err(e) => {
                    if Self::should_retry(&e.to_string()) && attempt < effective_config.max_retries
                    {
                        continue;
                    }
                    return Err(AppError::Message(e.to_string()));
                }
            }
        }

        Ok(last_result.unwrap_or_else(|| StreamCheckResult {
            status: HealthStatus::Failed,
            success: false,
            message: "Check failed".to_string(),
            response_time_ms: None,
            http_status: None,
            model_used: String::new(),
            tested_at: chrono::Utc::now().timestamp(),
            retry_count: effective_config.max_retries,
            error_category: None,
        }))
    }

    /// 合并供应商单独配置和全局配置
    ///
    /// 如果供应商配置了 meta.testConfig 且 enabled 为 true，则使用供应商配置覆盖全局配置
    fn merge_provider_config(
        provider: &Provider,
        global_config: &StreamCheckConfig,
    ) -> StreamCheckConfig {
        let test_config = provider
            .meta
            .as_ref()
            .and_then(|m| m.test_config.as_ref())
            .filter(|tc| tc.enabled);

        match test_config {
            Some(tc) => StreamCheckConfig {
                timeout_secs: tc.timeout_secs.unwrap_or(global_config.timeout_secs),
                max_retries: tc.max_retries.unwrap_or(global_config.max_retries),
                degraded_threshold_ms: tc
                    .degraded_threshold_ms
                    .unwrap_or(global_config.degraded_threshold_ms),
                claude_model: tc
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.claude_model.clone()),
                codex_model: tc
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.codex_model.clone()),
                gemini_model: tc
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.gemini_model.clone()),
                test_prompt: tc
                    .test_prompt
                    .clone()
                    .unwrap_or_else(|| global_config.test_prompt.clone()),
            },
            None => global_config.clone(),
        }
    }

    /// 单次流式检查
    async fn check_once(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        auth_override: Option<AuthInfo>,
        base_url_override: Option<String>,
        claude_api_format_override: Option<String>,
    ) -> Result<StreamCheckResult, AppError> {
        let start = Instant::now();

        // OpenCode / OpenClaw 的 settings_config 结构与 Claude/Codex/Gemini 不同
        // （baseUrl / apiKey 直接作为根字段而非嵌套在 env），并且协议由 `api`
        // 或 `npm` 字段显式指定。它们不走 get_adapter 路径，而是直接分发。
        if matches!(
            app_type,
            AppType::OpenCode | AppType::OpenClaw | AppType::Hermes
        ) {
            return Self::check_once_without_adapter(app_type, provider, config, start).await;
        }

        let adapter: Box<dyn ProviderAdapter> = if matches!(app_type, AppType::ClaudeDesktop) {
            Box::new(ClaudeAdapter::new())
        } else {
            get_adapter(app_type)
        };

        let base_url = match base_url_override {
            Some(base_url) => base_url,
            None => adapter
                .extract_base_url(provider)
                .map_err(|e| AppError::Message(format!("Failed to extract base_url: {e}")))?,
        };

        let auth = auth_override
            .or_else(|| adapter.extract_auth(provider))
            .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;

        // 获取 HTTP 客户端
        let client = crate::proxy::http_client::get();
        let request_timeout = std::time::Duration::from_secs(config.timeout_secs);

        let model_to_test = Self::resolve_test_model(app_type, provider, config);
        let test_prompt = &config.test_prompt;

        let result = match app_type {
            AppType::Claude | AppType::ClaudeDesktop => {
                Self::check_claude_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                    provider,
                    claude_api_format_override.as_deref(),
                    None,
                )
                .await
            }
            AppType::Codex => {
                Self::check_codex_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                    provider,
                )
                .await
            }
            AppType::Gemini => {
                Self::check_gemini_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                    None,
                )
                .await
            }
            AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => {
                // Already handled via early dispatch above
                unreachable!("OpenCode/OpenClaw/Hermes 已通过 check_once_without_adapter 处理")
            }
        };

        let response_time = start.elapsed().as_millis() as u64;
        Ok(Self::build_stream_check_result(
            result,
            response_time,
            config.degraded_threshold_ms,
            &model_to_test,
        ))
    }

    /// Claude 流式检查
    ///
    /// 根据供应商的 api_format 选择请求格式：
    /// - "anthropic" (默认): Anthropic Messages API (/v1/messages)
    /// - "openai_chat": OpenAI Chat Completions API (/v1/chat/completions)
    /// - "openai_responses": OpenAI Responses API (/v1/responses)
    /// - "gemini_native": Gemini Native streamGenerateContent
    ///
    /// `extra_headers` 是一个可选的供应商级自定义 header 集合（从 OpenClaw
    /// 的 `settings_config.headers` 或 OpenCode 的 `settings_config.options.headers`
    /// 读取），在所有内置 header 之后追加，用于覆盖或补充（例如自定义 User-Agent）。
    #[allow(clippy::too_many_arguments)]
    async fn check_claude_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
        provider: &Provider,
        claude_api_format_override: Option<&str>,
        extra_headers: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let is_github_copilot = auth.strategy == AuthStrategy::GitHubCopilot;

        // Detect api_format: meta.api_format > settings_config.api_format > default "anthropic"
        let api_format = provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .or_else(|| {
                provider
                    .settings_config
                    .get("api_format")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("anthropic");

        let effective_api_format = claude_api_format_override.unwrap_or(api_format);

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);
        let is_openai_chat = effective_api_format == "openai_chat";
        let is_openai_responses = effective_api_format == "openai_responses";
        let is_gemini_native = effective_api_format == "gemini_native";
        let url = Self::resolve_claude_stream_url(
            base,
            auth.strategy,
            effective_api_format,
            is_full_url,
            model,
        );

        let max_tokens = if is_openai_responses { 16 } else { 1 };

        // Build from Anthropic-native shape first, then convert for configured targets.
        let anthropic_body = json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [{ "role": "user", "content": test_prompt }],
            "stream": true
        });
        // Codex OAuth (ChatGPT Plus/Pro 反代) 需要 store:false + include 标记，
        // 否则 Stream Check 会和生产路径一样被服务端 400 拒绝。
        let is_codex_oauth = provider.is_codex_oauth();
        let codex_fast_mode = provider.codex_fast_mode_enabled();

        let body = if is_openai_responses {
            anthropic_to_responses(
                anthropic_body,
                Some(&provider.id),
                is_codex_oauth,
                codex_fast_mode,
            )
            .map_err(|e| AppError::Message(format!("Failed to build test request: {e}")))?
        } else if is_gemini_native {
            anthropic_to_gemini(anthropic_body)
                .map_err(|e| AppError::Message(format!("Failed to build test request: {e}")))?
        } else if is_openai_chat {
            anthropic_to_openai(anthropic_body)
                .map_err(|e| AppError::Message(format!("Failed to build test request: {e}")))?
        } else {
            anthropic_body
        };

        let mut request_builder = client.post(&url);

        if is_github_copilot {
            // 生成请求追踪 ID
            let request_id = uuid::Uuid::new_v4().to_string();
            request_builder = request_builder
                .header("authorization", format!("Bearer {}", auth.api_key))
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .header("accept-encoding", "identity")
                .header("user-agent", copilot_auth::COPILOT_USER_AGENT)
                .header("editor-version", copilot_auth::COPILOT_EDITOR_VERSION)
                .header(
                    "editor-plugin-version",
                    copilot_auth::COPILOT_PLUGIN_VERSION,
                )
                .header(
                    "copilot-integration-id",
                    copilot_auth::COPILOT_INTEGRATION_ID,
                )
                .header("x-github-api-version", copilot_auth::COPILOT_API_VERSION)
                // 260401 新增copilot 的关键 headers
                .header("openai-intent", "conversation-agent")
                .header("x-initiator", "user")
                .header("x-interaction-type", "conversation-agent")
                .header("x-vscode-user-agent-library-version", "electron-fetch")
                .header("x-request-id", &request_id)
                .header("x-agent-task-id", &request_id);
        } else if is_gemini_native {
            request_builder = match auth.strategy {
                AuthStrategy::GoogleOAuth => {
                    let token = auth.access_token.as_ref().unwrap_or(&auth.api_key);
                    request_builder
                        .header("authorization", format!("Bearer {token}"))
                        .header("x-goog-api-client", "GeminiCLI/1.0")
                        .header("content-type", "application/json")
                        .header("accept", "text/event-stream")
                        .header("accept-encoding", "identity")
                }
                _ => request_builder
                    .header("x-goog-api-key", &auth.api_key)
                    .header("content-type", "application/json")
                    .header("accept", "text/event-stream")
                    .header("accept-encoding", "identity"),
            };
        } else if is_openai_chat || is_openai_responses {
            // OpenAI-compatible targets: Bearer auth + SSE headers only
            request_builder = request_builder
                .header("authorization", format!("Bearer {}", auth.api_key))
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .header("accept-encoding", "identity");
        } else {
            // Anthropic native: full Claude CLI headers
            let os_name = Self::get_os_name();
            let arch_name = Self::get_arch_name();

            // 鉴权头复用 ClaudeAdapter::get_auth_headers，与代理路径（forwarder）保持单一真理来源。
            // - AuthStrategy::Anthropic  → x-api-key
            // - AuthStrategy::ClaudeAuth → Authorization: Bearer
            // - AuthStrategy::Bearer     → Authorization: Bearer
            // 避免之前"无条件 Bearer + 条件 x-api-key 双发"导致的假阴性 / auth conflict。
            let auth_headers = ClaudeAdapter::new()
                .get_auth_headers(auth)
                .map_err(|e| AppError::Message(format!("stream check 构造鉴权头失败: {e}")))?;
            for (name, value) in auth_headers {
                request_builder = request_builder.header(name, value);
            }

            request_builder = request_builder
                // Anthropic required headers
                .header("anthropic-version", "2023-06-01")
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,interleaved-thinking-2025-05-14",
                )
                .header("anthropic-dangerous-direct-browser-access", "true")
                // Content type headers
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .header("accept-encoding", "identity")
                .header("accept-language", "*")
                // Client identification headers
                .header("user-agent", "claude-cli/2.1.2 (external, cli)")
                .header("x-app", "cli")
                // x-stainless SDK headers (dynamic local system info)
                .header("x-stainless-lang", "js")
                .header("x-stainless-package-version", "0.70.0")
                .header("x-stainless-os", os_name)
                .header("x-stainless-arch", arch_name)
                .header("x-stainless-runtime", "node")
                .header("x-stainless-runtime-version", "v22.20.0")
                .header("x-stainless-retry-count", "0")
                .header("x-stainless-timeout", "600")
                // Other headers
                .header("sec-fetch-mode", "cors");
        }

        // 供应商自定义 headers 最后追加，允许覆盖内置默认值（例如 user-agent）
        if let Some(headers) = extra_headers {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request_builder = request_builder.header(key.as_str(), v);
                }
            }
        }

        let response = request_builder
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_request_error)?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::http_status_error(status, error_text));
        }

        // 流式读取：只需首个 chunk
        let mut stream = response.bytes_stream();
        if let Some(chunk) = stream.next().await {
            match chunk {
                Ok(_) => Ok((status, model.to_string())),
                Err(e) => Err(AppError::Message(format!("Stream read failed: {e}"))),
            }
        } else {
            Err(AppError::Message("No response data received".to_string()))
        }
    }

    /// Codex 流式检查
    ///
    /// 严格按照 Codex CLI 真实请求格式构建请求 (Responses API)
    async fn check_codex_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
        provider: &Provider,
    ) -> Result<(u16, String), AppError> {
        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);
        let urls = Self::resolve_codex_stream_urls(base_url, is_full_url);

        // 解析模型名和推理等级 (支持 model@level 或 model#level 格式)
        let (actual_model, reasoning_effort) = Self::parse_model_with_effort(model);

        // 获取本地系统信息
        let os_name = Self::get_os_name();
        let arch_name = Self::get_arch_name();

        // Responses API 请求体格式 (input 必须是数组)
        let mut body = json!({
            "model": actual_model,
            "input": [{ "role": "user", "content": test_prompt }],
            "stream": true
        });

        // 如果是推理模型，添加 reasoning_effort
        if let Some(effort) = reasoning_effort {
            body["reasoning"] = json!({ "effort": effort });
        }

        for (i, url) in urls.iter().enumerate() {
            // 严格按照 Codex CLI 请求格式设置 headers
            let response = client
                .post(url)
                .header("authorization", format!("Bearer {}", auth.api_key))
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .header("accept-encoding", "identity")
                .header(
                    "user-agent",
                    format!("codex_cli_rs/0.80.0 ({os_name} 15.7.2; {arch_name}) Terminal"),
                )
                .header("originator", "codex_cli_rs")
                .timeout(timeout)
                .json(&body)
                .send()
                .await
                .map_err(Self::map_request_error)?;

            let status = response.status().as_u16();

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                // 回退策略：仅当首选 URL 返回 404 时尝试下一个
                if i == 0 && status == 404 && urls.len() > 1 {
                    continue;
                }
                return Err(Self::http_status_error(status, error_text));
            }

            let mut stream = response.bytes_stream();
            if let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(_) => return Ok((status, actual_model)),
                    Err(e) => return Err(AppError::Message(format!("Stream read failed: {e}"))),
                }
            }

            return Err(AppError::Message("No response data received".to_string()));
        }

        Err(AppError::Message(
            "No valid Codex responses endpoint found".to_string(),
        ))
    }

    /// Gemini 流式检查
    ///
    /// 使用 Gemini 原生 API 格式 (streamGenerateContent)
    async fn check_gemini_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
        extra_headers: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        // Strip `models/` resource-name prefix from the model id — see
        // `normalize_gemini_model_id` for rationale.
        let normalized_model = normalize_gemini_model_id(model);
        // Gemini 原生 API: /v1beta/models/{model}:streamGenerateContent?alt=sse
        // 智能处理 /v1beta 路径：如果 base_url 不包含版本路径，则添加 /v1beta
        // alt=sse 参数使 API 返回 SSE 格式（text/event-stream）而非 JSON 数组
        let url = if base.contains("/v1beta") || base.contains("/v1/") {
            format!("{base}/models/{normalized_model}:streamGenerateContent?alt=sse")
        } else {
            format!("{base}/v1beta/models/{normalized_model}:streamGenerateContent?alt=sse")
        };

        // Gemini 原生请求体格式
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": test_prompt }]
            }]
        });

        let mut request_builder = client
            .post(&url)
            .header("x-goog-api-key", &auth.api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream");

        // 供应商自定义 headers 最后追加
        if let Some(headers) = extra_headers {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request_builder = request_builder.header(key.as_str(), v);
                }
            }
        }

        let response = request_builder
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_request_error)?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::http_status_error(status, error_text));
        }

        let mut stream = response.bytes_stream();
        if let Some(chunk) = stream.next().await {
            match chunk {
                Ok(_) => Ok((status, model.to_string())),
                Err(e) => Err(AppError::Message(format!("Stream read failed: {e}"))),
            }
        } else {
            Err(AppError::Message("No response data received".to_string()))
        }
    }

    /// OpenCode / OpenClaw 的独立分发入口（绕过 `get_adapter`）
    ///
    /// 这两个应用的 `settings_config` 与 Claude/Codex/Gemini 完全不同：
    /// - OpenClaw: `{ baseUrl, apiKey, api, models: [...] }`，`api` 字段标识协议
    /// - OpenCode: `{ npm, options: { baseURL, apiKey }, models: {...} }`，`npm` 字段标识协议
    ///
    /// 因此不能复用 `get_adapter`（会 fallback 到 CodexAdapter 而提取失败），
    /// 改为独立解析 base_url/api_key/协议，再分发到现有的 check_*_stream 函数。
    async fn check_once_without_adapter(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        start: Instant,
    ) -> Result<StreamCheckResult, AppError> {
        // 获取 HTTP 客户端
        let client = crate::proxy::http_client::get();
        let request_timeout = std::time::Duration::from_secs(config.timeout_secs);

        let model_to_test = Self::resolve_test_model(app_type, provider, config);
        let test_prompt = &config.test_prompt;

        let result = match app_type {
            AppType::OpenClaw => {
                Self::check_additive_app_stream(
                    &client,
                    provider,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                )
                .await
            }
            AppType::OpenCode => {
                Self::check_opencode_stream(
                    &client,
                    provider,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                )
                .await
            }
            AppType::Hermes => {
                Self::check_hermes_stream(
                    &client,
                    provider,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                )
                .await
            }
            _ => unreachable!("check_once_without_adapter 只处理 OpenCode/OpenClaw/Hermes"),
        };

        let response_time = start.elapsed().as_millis() as u64;
        Ok(Self::build_stream_check_result(
            result,
            response_time,
            config.degraded_threshold_ms,
            &model_to_test,
        ))
    }

    /// 将 check_*_stream 的原始结果包装成 StreamCheckResult
    ///
    /// 抽取自 check_once 的末尾逻辑，以便 OpenCode/OpenClaw 的独立分支复用。
    ///
    /// `model_tested` 是本次探测使用的模型名，用于在失败场景下仍能把模型信息透传给前端，
    /// 方便针对"模型不存在 / 已下架"这类错误渲染专门的提示。
    fn build_stream_check_result(
        result: Result<(u16, String), AppError>,
        response_time: u64,
        degraded_threshold_ms: u64,
        model_tested: &str,
    ) -> StreamCheckResult {
        let tested_at = chrono::Utc::now().timestamp();
        match result {
            Ok((status_code, model)) => StreamCheckResult {
                status: Self::determine_status(response_time, degraded_threshold_ms),
                success: true,
                message: "Check succeeded".to_string(),
                response_time_ms: Some(response_time),
                http_status: Some(status_code),
                model_used: model,
                tested_at,
                retry_count: 0,
                error_category: None,
            },
            Err(e) => {
                let (http_status, message, error_category) = match &e {
                    AppError::HttpStatus { status, body } => {
                        let category = Self::detect_error_category(*status, body);
                        (
                            Some(*status),
                            Self::classify_http_status(*status).to_string(),
                            category.map(|s| s.to_string()),
                        )
                    }
                    _ => (None, e.to_string(), None),
                };
                StreamCheckResult {
                    status: HealthStatus::Failed,
                    success: false,
                    message,
                    response_time_ms: Some(response_time),
                    http_status,
                    model_used: model_tested.to_string(),
                    tested_at,
                    retry_count: 0,
                    error_category,
                }
            }
        }
    }

    /// 基于 HTTP 状态码和响应体识别细粒度错误分类。
    ///
    /// 目前仅识别"模型不存在 / 已下架"：各厂商该类错误通常返回 4xx，body 中会包含
    /// 如 `model_not_found`（OpenAI）、`does not exist`、`invalid model`、`not_found_error`
    /// + `model` 字样（Anthropic）等标记。
    pub(crate) fn detect_error_category(status: u16, body: &str) -> Option<&'static str> {
        // 只检查 4xx；5xx 的错误信息里可能巧合出现"model"之类的词，容易误判
        if !(400..500).contains(&status) {
            return None;
        }
        let lower = body.to_lowercase();
        let qianfan_quota_indicators = [
            "coding_plan_hour_quota_exceeded",
            "coding_plan_week_quota_exceeded",
            "coding_plan_month_quota_exceeded",
        ];
        if qianfan_quota_indicators.iter().any(|s| lower.contains(s)) {
            return Some("quotaExceeded");
        }

        // 必须提到 "model"，避免通用 404 / 400 被误判
        if !lower.contains("model") {
            return None;
        }
        let indicators = [
            "model_not_found",
            "model not found",
            "does not exist",
            "invalid_model",
            "invalid model",
            "unknown_model",
            "unknown model",
            "is not a valid model",
            "not_found_error", // Anthropic 的 type 字段
        ];
        if indicators.iter().any(|s| lower.contains(s)) {
            return Some("modelNotFound");
        }
        None
    }

    /// OpenClaw 流式检查分发器
    ///
    /// 根据 `settings_config.api` 字段分发到对应协议的检查器。
    /// 取值参见 `openclawApiProtocols` (前端 openclawProviderPresets.ts):
    /// - `openai-completions`   → check_claude_stream + api_format="openai_chat"
    /// - `openai-responses`     → check_claude_stream + api_format="openai_responses"
    /// - `anthropic-messages`   → check_claude_stream + api_format="anthropic" (ClaudeAuth 策略)
    /// - `google-generative-ai` → check_gemini_stream (Google API Key 策略)
    /// - `bedrock-converse-stream` → 不支持（需要 AWS SigV4 签名）
    async fn check_additive_app_stream(
        client: &Client,
        provider: &Provider,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        // 自定义认证头（如 Longcat 的 `apikey` 头）不走标准 Bearer，
        // 具体头名由 OpenClaw 网关内部决定，cc-switch 无法准确构造，
        // 因此直接返回友好错误而不是让用户看到一个误导性的 401。
        if Self::additive_app_uses_auth_header(provider) {
            return Err(AppError::localized(
                "openclaw_auth_header_not_supported",
                "该供应商使用自定义认证头，暂不支持流式健康检查。建议直接通过 OpenClaw 测试。",
                "This provider uses a custom auth header; stream health check is not supported. Please test it directly via OpenClaw.",
            ));
        }

        let base_url = Self::extract_openclaw_base_url(provider)?;
        let api_key = Self::extract_openclaw_api_key(provider)?;
        let api = Self::extract_openclaw_protocol(provider);
        let extra_headers = Self::extract_openclaw_headers(provider);

        match api.as_deref() {
            Some("openai-completions") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Bearer);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("openai_chat"),
                    extra_headers,
                )
                .await
            }
            Some("openai-responses") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Bearer);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("openai_responses"),
                    extra_headers,
                )
                .await
            }
            Some("anthropic-messages") => {
                // 使用 ClaudeAuth（Bearer-only）以兼容 Claude 中转服务。
                // 某些中转同时收到 Authorization 和 x-api-key 会报错，ClaudeAuth
                // 策略保证只下发 Bearer。官方 Anthropic 也接受纯 Bearer。
                let auth = AuthInfo::new(api_key, AuthStrategy::ClaudeAuth);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("anthropic"),
                    extra_headers,
                )
                .await
            }
            Some("google-generative-ai") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Google);
                Self::check_gemini_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    extra_headers,
                )
                .await
            }
            Some("bedrock-converse-stream") => Err(AppError::localized(
                "openclaw_bedrock_not_supported",
                "AWS Bedrock 需要 SigV4 签名，当前不支持健康检查。请通过 AWS 控制台或 OpenClaw 验证连通性。",
                "AWS Bedrock requires SigV4 signing and is not supported by stream health check. Please verify connectivity via AWS console or OpenClaw.",
            )),
            Some(other) => Err(AppError::localized(
                "openclaw_protocol_not_yet_supported",
                format!("OpenClaw 暂不支持协议: {other}"),
                format!("OpenClaw protocol not yet supported: {other}"),
            )),
            None => Err(AppError::localized(
                "openclaw_protocol_missing",
                "OpenClaw 供应商缺少 api 字段",
                "OpenClaw provider is missing the `api` field",
            )),
        }
    }

    /// 判断 additive-mode 供应商是否使用自定义认证头（`authHeader: true`）
    fn additive_app_uses_auth_header(provider: &Provider) -> bool {
        provider
            .settings_config
            .get("authHeader")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// 提取 OpenClaw 供应商的自定义 headers（来自 `settings_config.headers`）
    fn extract_openclaw_headers(
        provider: &Provider,
    ) -> Option<&serde_json::Map<String, serde_json::Value>> {
        provider
            .settings_config
            .get("headers")
            .and_then(|v| v.as_object())
            .filter(|m| !m.is_empty())
    }

    fn extract_openclaw_base_url(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "openclaw_base_url_missing",
                    "OpenClaw 供应商缺少 baseUrl",
                    "OpenClaw provider is missing `baseUrl`",
                )
            })
    }

    fn extract_openclaw_api_key(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("apiKey")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "openclaw_api_key_missing",
                    "OpenClaw 供应商缺少 apiKey",
                    "OpenClaw provider is missing `apiKey`",
                )
            })
    }

    fn extract_openclaw_protocol(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("api")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    // Hermes 的 settings_config 用 snake_case（base_url / api_key / api_mode），
    // 与 OpenClaw 的 camelCase（baseUrl / apiKey / api）是两套独立命名。
    // 见 src/config/hermesProviderPresets.ts 的 HermesProviderSettingsConfig。
    fn extract_hermes_base_url(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "hermes_base_url_missing",
                    "Hermes 供应商缺少 base_url",
                    "Hermes provider is missing `base_url`",
                )
            })
    }

    fn extract_hermes_api_key(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("api_key")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "hermes_api_key_missing",
                    "Hermes 供应商缺少 api_key",
                    "Hermes provider is missing `api_key`",
                )
            })
    }

    fn extract_hermes_api_mode(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("api_mode")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Hermes 流式检查分发器
    ///
    /// Hermes 以 `api_mode` 字段显式指定协议，取值来自
    /// `HermesApiMode`（hermesProviderPresets.ts）：
    /// - `chat_completions`   → check_claude_stream + api_format="openai_chat"（Bearer）
    /// - `anthropic_messages` → check_claude_stream + api_format="anthropic"（ClaudeAuth，与 OpenClaw 的 anthropic-messages 同策略）
    /// - `codex_responses`    → check_claude_stream + api_format="openai_responses"（Bearer）
    /// - `bedrock_converse`   → 不支持（需要 AWS SigV4 签名）
    async fn check_hermes_stream(
        client: &Client,
        provider: &Provider,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        // 先把 api_mode 路由出协议格式与认证策略。
        // 纯错误路径（bedrock / 未知 / 缺失）直接 return，避免在用户
        // 选了 bedrock_converse 时被"缺 base_url"的二级错误盖住真正原因。
        let (api_format, auth_strategy) = match Self::extract_hermes_api_mode(provider).as_deref() {
            Some("chat_completions") => ("openai_chat", AuthStrategy::Bearer),
            Some("anthropic_messages") => ("anthropic", AuthStrategy::ClaudeAuth),
            Some("codex_responses") => ("openai_responses", AuthStrategy::Bearer),
            Some("bedrock_converse") => {
                return Err(AppError::localized(
                    "hermes_bedrock_not_supported",
                    "AWS Bedrock 需要 SigV4 签名，当前不支持健康检查。",
                    "AWS Bedrock requires SigV4 signing and is not supported by stream health check.",
                ));
            }
            Some(other) => {
                return Err(AppError::localized(
                    "hermes_protocol_not_yet_supported",
                    format!("Hermes 暂不支持协议: {other}"),
                    format!("Hermes protocol not yet supported: {other}"),
                ));
            }
            None => {
                return Err(AppError::localized(
                    "hermes_api_mode_missing",
                    "Hermes 供应商缺少 api_mode 字段",
                    "Hermes provider is missing the `api_mode` field",
                ));
            }
        };

        let base_url = Self::extract_hermes_base_url(provider)?;
        let api_key = Self::extract_hermes_api_key(provider)?;
        let auth = AuthInfo::new(api_key, auth_strategy);
        Self::check_claude_stream(
            client,
            &base_url,
            &auth,
            model,
            test_prompt,
            timeout,
            provider,
            Some(api_format),
            None,
        )
        .await
    }

    /// OpenCode 流式检查分发器
    ///
    /// OpenCode 用 `npm` 字段（AI SDK 包名）隐式指定协议。映射关系参见
    /// `opencodeNpmPackages` (前端 opencodeProviderPresets.ts):
    /// - `@ai-sdk/openai-compatible` → check_claude_stream + api_format="openai_chat"
    /// - `@ai-sdk/openai`            → check_claude_stream + api_format="openai_responses"
    /// - `@ai-sdk/anthropic`         → check_claude_stream + api_format="anthropic"
    /// - `@ai-sdk/google`            → check_gemini_stream (Google API Key 策略)
    /// - `@ai-sdk/amazon-bedrock`    → 不支持（需要 AWS SigV4 签名）
    ///
    /// URL/API Key 存放在 `settings_config.options.{baseURL,apiKey}`，注意
    /// `baseURL` 大写 L（与 OpenClaw 的 `baseUrl` 首字母小写 u 不同）。
    async fn check_opencode_stream(
        client: &Client,
        provider: &Provider,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        let npm = Self::extract_opencode_npm(provider);
        // 若用户未显式填 baseURL，则根据 npm 回退到 AI SDK 包自带的默认端点
        let base_url = Self::resolve_opencode_base_url(provider, npm.as_deref())?;
        let api_key = Self::extract_opencode_api_key(provider)?;
        let extra_headers = Self::extract_opencode_headers(provider);

        match npm.as_deref() {
            Some("@ai-sdk/openai-compatible") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Bearer);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("openai_chat"),
                    extra_headers,
                )
                .await
            }
            Some("@ai-sdk/openai") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Bearer);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("openai_responses"),
                    extra_headers,
                )
                .await
            }
            Some("@ai-sdk/anthropic") => {
                // 见 check_additive_app_stream 对 anthropic-messages 的处理：
                // 用 ClaudeAuth（Bearer-only）兼容中转服务。
                let auth = AuthInfo::new(api_key, AuthStrategy::ClaudeAuth);
                Self::check_claude_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    provider,
                    Some("anthropic"),
                    extra_headers,
                )
                .await
            }
            Some("@ai-sdk/google") => {
                let auth = AuthInfo::new(api_key, AuthStrategy::Google);
                Self::check_gemini_stream(
                    client,
                    &base_url,
                    &auth,
                    model,
                    test_prompt,
                    timeout,
                    extra_headers,
                )
                .await
            }
            Some("@ai-sdk/amazon-bedrock") => Err(AppError::localized(
                "opencode_bedrock_not_supported",
                "AWS Bedrock 需要 SigV4 签名，当前不支持健康检查。请通过 AWS 控制台或 OpenCode 验证连通性。",
                "AWS Bedrock requires SigV4 signing and is not supported by stream health check. Please verify connectivity via AWS console or OpenCode.",
            )),
            Some(other) => Err(AppError::localized(
                "opencode_npm_not_yet_supported",
                format!("OpenCode 暂不支持 SDK 包: {other}"),
                format!("OpenCode SDK package not yet supported: {other}"),
            )),
            None => Err(AppError::localized(
                "opencode_npm_missing",
                "OpenCode 供应商缺少 npm 字段",
                "OpenCode provider is missing the `npm` field",
            )),
        }
    }

    /// 按 OpenCode 的实际 SDK 包特性确定 baseURL：
    /// - 用户显式填写的 `options.baseURL` 总是优先
    /// - 否则根据 `npm` 返回 AI SDK 包自带的默认端点
    /// - `@ai-sdk/openai-compatible` 没有默认端点，必须显式填
    ///
    /// 注意：这里的默认端点对应 AI SDK 包的行为（例如 `@ai-sdk/openai`
    /// 自带 `/v1` 路径后缀），与 `proxy/providers/mod.rs` 里的
    /// `ProviderType::default_endpoint()` 语义不同——后者是代理层的上游
    /// 默认值，不带 `/v1`。两者维护的是不同系统的默认值，不能简单共享。
    fn resolve_opencode_base_url(
        provider: &Provider,
        npm: Option<&str>,
    ) -> Result<String, AppError> {
        if let Some(explicit) = Self::extract_opencode_base_url(provider) {
            return Ok(explicit);
        }

        let fallback = match npm {
            Some("@ai-sdk/openai") => Some("https://api.openai.com/v1"),
            Some("@ai-sdk/anthropic") => Some("https://api.anthropic.com"),
            Some("@ai-sdk/google") => Some("https://generativelanguage.googleapis.com"),
            _ => None,
        };

        fallback.map(|s| s.to_string()).ok_or_else(|| {
            AppError::localized(
                "opencode_base_url_missing",
                "OpenCode 供应商缺少 options.baseURL，且当前 SDK 包没有默认端点",
                "OpenCode provider is missing `options.baseURL` and the SDK package has no default endpoint",
            )
        })
    }

    fn extract_opencode_base_url(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("options")
            .and_then(|v| v.get("baseURL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 提取 OpenCode 供应商的自定义 headers（来自 `settings_config.options.headers`）
    fn extract_opencode_headers(
        provider: &Provider,
    ) -> Option<&serde_json::Map<String, serde_json::Value>> {
        provider
            .settings_config
            .get("options")
            .and_then(|v| v.get("headers"))
            .and_then(|v| v.as_object())
            .filter(|m| !m.is_empty())
    }

    fn extract_opencode_api_key(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("options")
            .and_then(|v| v.get("apiKey"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "opencode_api_key_missing",
                    "OpenCode 供应商缺少 options.apiKey",
                    "OpenCode provider is missing `options.apiKey`",
                )
            })
    }

    fn extract_opencode_npm(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("npm")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn determine_status(latency_ms: u64, threshold: u64) -> HealthStatus {
        if latency_ms <= threshold {
            HealthStatus::Operational
        } else {
            HealthStatus::Degraded
        }
    }

    /// 解析模型名和推理等级 (支持 model@level 或 model#level 格式)
    /// 返回 (实际模型名, Option<推理等级>)
    fn parse_model_with_effort(model: &str) -> (String, Option<String>) {
        if let Some(pos) = model.find('@').or_else(|| model.find('#')) {
            let actual_model = model[..pos].to_string();
            let effort = model[pos + 1..].to_string();
            if !effort.is_empty() {
                return (actual_model, Some(effort));
            }
        }
        (model.to_string(), None)
    }

    fn should_retry(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        lower.contains("timeout") || lower.contains("abort") || lower.contains("timed out")
    }

    fn map_request_error(e: reqwest::Error) -> AppError {
        if e.is_timeout() {
            AppError::Message("Request timeout".to_string())
        } else if e.is_connect() {
            AppError::Message(format!("Connection failed: {e}"))
        } else {
            AppError::Message(e.to_string())
        }
    }

    /// 构造 HTTP 状态码错误，截断过长的响应体
    fn http_status_error(status: u16, body: String) -> AppError {
        let body = if body.len() > 200 {
            // 安全截断：找到 200 字节内最近的 char 边界
            let mut end = 200;
            while end > 0 && !body.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &body[..end])
        } else {
            body
        };
        AppError::HttpStatus { status, body }
    }

    /// 将 HTTP 状态码映射为简短的分类标签
    pub(crate) fn classify_http_status(status: u16) -> &'static str {
        match status {
            400 => "Bad request (400)",
            401 => "Auth rejected (401)",
            402 => "Payment required (402)",
            403 => "Access denied (403)",
            404 => "Not found (404)",
            429 => "Rate limited (429)",
            500 => "Internal server error (500)",
            502 => "Bad gateway (502)",
            503 => "Service unavailable (503)",
            504 => "Gateway timeout (504)",
            s if (500..600).contains(&s) => "Server error",
            _ => "HTTP error",
        }
    }

    fn resolve_test_model(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> String {
        match app_type {
            AppType::Claude | AppType::ClaudeDesktop => {
                Self::extract_env_model(provider, "ANTHROPIC_MODEL")
                    .unwrap_or_else(|| config.claude_model.clone())
            }
            AppType::Codex => {
                Self::extract_codex_model(provider).unwrap_or_else(|| config.codex_model.clone())
            }
            AppType::Gemini => Self::extract_env_model(provider, "GEMINI_MODEL")
                .unwrap_or_else(|| config.gemini_model.clone()),
            AppType::OpenCode => {
                // OpenCode uses models map in settings_config
                // Try to extract first model from the models object
                Self::extract_opencode_model(provider).unwrap_or_else(|| "gpt-4o".to_string())
            }
            AppType::OpenClaw | AppType::Hermes => {
                // OpenClaw/Hermes use models array in settings_config
                // Try to extract first model from the models array
                Self::extract_openclaw_model(provider).unwrap_or_else(|| "gpt-4o".to_string())
            }
        }
    }

    fn extract_opencode_model(provider: &Provider) -> Option<String> {
        let models = provider
            .settings_config
            .get("models")
            .and_then(|m| m.as_object())?;

        // Return the first model ID from the models map
        models.keys().next().map(|s| s.to_string())
    }

    fn extract_openclaw_model(provider: &Provider) -> Option<String> {
        // OpenClaw uses models array: [{ "id": "model-id", "name": "Model Name" }]
        let models = provider
            .settings_config
            .get("models")
            .and_then(|m| m.as_array())?;

        // Return the first model ID from the models array
        models
            .first()
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .map(|s| s.to_string())
    }

    fn extract_env_model(provider: &Provider, key: &str) -> Option<String> {
        provider
            .settings_config
            .get("env")
            .and_then(|env| env.get(key))
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn extract_codex_model(provider: &Provider) -> Option<String> {
        let config_text = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())?;
        if config_text.trim().is_empty() {
            return None;
        }

        let table = toml::from_str::<toml::Table>(config_text).ok()?;
        table
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// 获取操作系统名称（映射为 Claude CLI 使用的格式）
    fn get_os_name() -> &'static str {
        match std::env::consts::OS {
            "macos" => "MacOS",
            "linux" => "Linux",
            "windows" => "Windows",
            other => other,
        }
    }

    /// 获取 CPU 架构名称（映射为 Claude CLI 使用的格式）
    fn get_arch_name() -> &'static str {
        match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            "x86" => "x86",
            other => other,
        }
    }

    fn resolve_claude_stream_url(
        base_url: &str,
        auth_strategy: AuthStrategy,
        api_format: &str,
        is_full_url: bool,
        model: &str,
    ) -> String {
        if api_format == "gemini_native" {
            // Strip an optional `models/` resource-name prefix so that model
            // identifiers copied from Gemini SDK outputs (e.g.
            // `models/gemini-2.5-pro`) don't produce a doubled
            // `/v1beta/models/models/...` URL.
            let normalized_model = normalize_gemini_model_id(model);
            let endpoint =
                format!("/v1beta/models/{normalized_model}:streamGenerateContent?alt=sse");
            return resolve_gemini_native_url(base_url, &endpoint, is_full_url);
        }

        if is_full_url {
            return base_url.to_string();
        }

        let base = base_url.trim_end_matches('/');
        let is_github_copilot = auth_strategy == AuthStrategy::GitHubCopilot;

        if is_github_copilot && api_format == "openai_responses" {
            format!("{base}/v1/responses")
        } else if is_github_copilot {
            format!("{base}/chat/completions")
        } else if api_format == "openai_responses" {
            if base.ends_with("/v1") {
                format!("{base}/responses")
            } else {
                format!("{base}/v1/responses")
            }
        } else if api_format == "openai_chat" {
            if base.ends_with("/v1") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            }
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }

    fn resolve_codex_stream_urls(base_url: &str, is_full_url: bool) -> Vec<String> {
        if is_full_url {
            return vec![base_url.to_string()];
        }

        let base = base_url.trim_end_matches('/');

        if base.ends_with("/v1") {
            vec![format!("{base}/responses")]
        } else {
            vec![format!("{base}/responses"), format!("{base}/v1/responses")]
        }
    }

    pub(crate) fn resolve_effective_test_model(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> String {
        let effective_config = Self::merge_provider_config(provider, config);
        Self::resolve_test_model(app_type, provider, &effective_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(settings_config: serde_json::Value) -> Provider {
        Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            settings_config,
            None,
        )
    }

    #[test]
    fn test_additive_app_uses_auth_header_true() {
        let p = make_provider(serde_json::json!({
            "baseUrl": "https://api.longcat.chat/v1",
            "apiKey": "k",
            "api": "openai-completions",
            "authHeader": true,
        }));
        assert!(StreamCheckService::additive_app_uses_auth_header(&p));
    }

    #[test]
    fn test_additive_app_uses_auth_header_default_false() {
        let p = make_provider(serde_json::json!({
            "baseUrl": "https://api.deepseek.com/v1",
            "apiKey": "k",
            "api": "openai-completions",
        }));
        assert!(!StreamCheckService::additive_app_uses_auth_header(&p));
    }

    #[test]
    fn test_resolve_opencode_base_url_explicit_wins() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai",
            "options": { "baseURL": "https://proxy.local/v1", "apiKey": "k" },
            "models": {},
        }));
        let resolved =
            StreamCheckService::resolve_opencode_base_url(&p, Some("@ai-sdk/openai")).unwrap();
        assert_eq!(resolved, "https://proxy.local/v1");
    }

    #[test]
    fn test_resolve_opencode_base_url_falls_back_for_known_npm() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai",
            "options": { "apiKey": "k" },
            "models": {},
        }));
        let resolved =
            StreamCheckService::resolve_opencode_base_url(&p, Some("@ai-sdk/openai")).unwrap();
        assert_eq!(resolved, "https://api.openai.com/v1");

        let p2 = make_provider(serde_json::json!({
            "npm": "@ai-sdk/anthropic",
            "options": { "apiKey": "k" },
            "models": {},
        }));
        let resolved2 =
            StreamCheckService::resolve_opencode_base_url(&p2, Some("@ai-sdk/anthropic")).unwrap();
        assert_eq!(resolved2, "https://api.anthropic.com");
    }

    #[test]
    fn test_resolve_opencode_base_url_errors_for_openai_compatible_without_url() {
        // @ai-sdk/openai-compatible 没有默认端点，必须显式填
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": { "apiKey": "k" },
            "models": {},
        }));
        let result =
            StreamCheckService::resolve_opencode_base_url(&p, Some("@ai-sdk/openai-compatible"));
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_openclaw_headers_preserves_map() {
        let p = make_provider(serde_json::json!({
            "baseUrl": "https://example.com/v1",
            "apiKey": "k",
            "api": "openai-completions",
            "headers": { "User-Agent": "MyBot/1.0", "X-Trace": "abc" },
        }));
        let headers = StreamCheckService::extract_openclaw_headers(&p).unwrap();
        assert_eq!(
            headers.get("User-Agent").and_then(|v| v.as_str()),
            Some("MyBot/1.0")
        );
        assert_eq!(headers.get("X-Trace").and_then(|v| v.as_str()), Some("abc"));
    }

    #[test]
    fn test_extract_openclaw_headers_ignores_empty_map() {
        let p = make_provider(serde_json::json!({
            "baseUrl": "https://example.com/v1",
            "apiKey": "k",
            "api": "openai-completions",
            "headers": {},
        }));
        assert!(StreamCheckService::extract_openclaw_headers(&p).is_none());
    }

    #[test]
    fn test_extract_opencode_headers_from_options() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": "https://example.com/v1",
                "apiKey": "k",
                "headers": { "X-Custom": "yes" },
            },
            "models": {},
        }));
        let headers = StreamCheckService::extract_opencode_headers(&p).unwrap();
        assert_eq!(
            headers.get("X-Custom").and_then(|v| v.as_str()),
            Some("yes")
        );
    }

    #[test]
    fn test_determine_status() {
        assert_eq!(
            StreamCheckService::determine_status(3000, 6000),
            HealthStatus::Operational
        );
        assert_eq!(
            StreamCheckService::determine_status(6000, 6000),
            HealthStatus::Operational
        );
        assert_eq!(
            StreamCheckService::determine_status(6001, 6000),
            HealthStatus::Degraded
        );
    }

    #[test]
    fn test_should_retry() {
        assert!(StreamCheckService::should_retry("Request timeout"));
        assert!(StreamCheckService::should_retry("request timed out"));
        assert!(StreamCheckService::should_retry("connection abort"));
        assert!(!StreamCheckService::should_retry("API Key invalid"));
    }

    #[test]
    fn test_default_config() {
        let config = StreamCheckConfig::default();
        assert_eq!(config.timeout_secs, 45);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.degraded_threshold_ms, 6000);
    }

    #[test]
    fn test_parse_model_with_effort() {
        // 带 @ 分隔符
        let (model, effort) = StreamCheckService::parse_model_with_effort("gpt-5.1-codex@low");
        assert_eq!(model, "gpt-5.1-codex");
        assert_eq!(effort, Some("low".to_string()));

        // 带 # 分隔符
        let (model, effort) = StreamCheckService::parse_model_with_effort("o1-preview#high");
        assert_eq!(model, "o1-preview");
        assert_eq!(effort, Some("high".to_string()));

        // 无分隔符
        let (model, effort) = StreamCheckService::parse_model_with_effort("gpt-4o-mini");
        assert_eq!(model, "gpt-4o-mini");
        assert_eq!(effort, None);
    }

    #[test]
    fn test_detect_model_not_found() {
        // OpenAI 典型响应：404 + model_not_found 错误码
        let openai_404 = r#"{"error":{"message":"The model `gpt-5.1-codex` does not exist or you do not have access to it","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(404, openai_404),
            Some("modelNotFound")
        );

        // Anthropic 典型响应：404 + not_found_error + 提到 model
        let anthropic_404 = r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-deprecated"}}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(404, anthropic_404),
            Some("modelNotFound")
        );

        // 400 + invalid model 也算
        let bad_req = r#"{"error":{"message":"invalid model specified"}}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(400, bad_req),
            Some("modelNotFound")
        );

        // 通用 404（比如 Base URL 错误），body 里没有 model 字样 → 不应误判
        let generic_404 = r#"{"error":"Not Found"}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(404, generic_404),
            None
        );

        // 5xx 就算 body 里有 "model does not exist" 也不分类（避免误判）
        let server_error = r#"{"error":"model does not exist"}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(500, server_error),
            None
        );

        // 401 鉴权错误（body 里没有 model 字样）
        let auth_err = r#"{"error":"Invalid API key"}"#;
        assert_eq!(
            StreamCheckService::detect_error_category(401, auth_err),
            None
        );
    }

    #[test]
    fn test_detect_qianfan_coding_plan_quota_errors() {
        let cases = [
            r#"{"error":{"code":"coding_plan_hour_quota_exceeded","message":"hour quota exceeded"}}"#,
            r#"{"error":{"code":"coding_plan_week_quota_exceeded","message":"week quota exceeded"}}"#,
            r#"{"error":{"code":"coding_plan_month_quota_exceeded","message":"month quota exceeded"}}"#,
        ];

        for body in cases {
            assert_eq!(
                StreamCheckService::detect_error_category(429, body),
                Some("quotaExceeded")
            );
        }
    }

    #[test]
    fn test_get_os_name() {
        let os_name = StreamCheckService::get_os_name();
        // 确保返回非空字符串
        assert!(!os_name.is_empty());
        // 在 macOS 上应该返回 "MacOS"
        #[cfg(target_os = "macos")]
        assert_eq!(os_name, "MacOS");
        // 在 Linux 上应该返回 "Linux"
        #[cfg(target_os = "linux")]
        assert_eq!(os_name, "Linux");
        // 在 Windows 上应该返回 "Windows"
        #[cfg(target_os = "windows")]
        assert_eq!(os_name, "Windows");
    }

    #[test]
    fn test_get_arch_name() {
        let arch_name = StreamCheckService::get_arch_name();
        // 确保返回非空字符串
        assert!(!arch_name.is_empty());
        // 在 ARM64 上应该返回 "arm64"
        #[cfg(target_arch = "aarch64")]
        assert_eq!(arch_name, "arm64");
        // 在 x86_64 上应该返回 "x86_64"
        #[cfg(target_arch = "x86_64")]
        assert_eq!(arch_name, "x86_64");
    }

    #[test]
    fn test_auth_strategy_imports() {
        // 验证 AuthStrategy 枚举可以正常使用
        let anthropic = AuthStrategy::Anthropic;
        let claude_auth = AuthStrategy::ClaudeAuth;
        let bearer = AuthStrategy::Bearer;

        // 验证不同的策略是不相等的
        assert_ne!(anthropic, claude_auth);
        assert_ne!(anthropic, bearer);
        assert_ne!(claude_auth, bearer);

        // 验证相同策略是相等的
        assert_eq!(anthropic, AuthStrategy::Anthropic);
        assert_eq!(claude_auth, AuthStrategy::ClaudeAuth);
        assert_eq!(bearer, AuthStrategy::Bearer);
    }

    #[test]
    fn test_resolve_claude_stream_url_for_full_url_mode() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://relay.example/v1/chat/completions",
            AuthStrategy::Bearer,
            "openai_chat",
            true,
            "gpt-5.4",
        );

        assert_eq!(url, "https://relay.example/v1/chat/completions");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_github_copilot() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://api.githubcopilot.com",
            AuthStrategy::GitHubCopilot,
            "openai_chat",
            false,
            "gpt-5.4",
        );

        assert_eq!(url, "https://api.githubcopilot.com/chat/completions");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_github_copilot_responses() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://api.githubcopilot.com",
            AuthStrategy::GitHubCopilot,
            "openai_responses",
            false,
            "gpt-5.4",
        );

        assert_eq!(url, "https://api.githubcopilot.com/v1/responses");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_openai_chat() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://example.com/v1",
            AuthStrategy::Bearer,
            "openai_chat",
            false,
            "gpt-5.4",
        );

        assert_eq!(url, "https://example.com/v1/chat/completions");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_openai_responses() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://example.com/v1",
            AuthStrategy::Bearer,
            "openai_responses",
            false,
            "gpt-5.4",
        );

        assert_eq!(url, "https://example.com/v1/responses");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_anthropic() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://api.anthropic.com",
            AuthStrategy::Anthropic,
            "anthropic",
            false,
            "claude-sonnet-4-6",
        );

        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_gemini_native() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://generativelanguage.googleapis.com",
            AuthStrategy::Google,
            "gemini_native",
            false,
            "gemini-2.5-flash",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_resolve_claude_stream_url_for_gemini_native_full_url_openai_compat_base() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            AuthStrategy::Google,
            "gemini_native",
            true,
            "gemini-2.5-flash",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_resolve_claude_stream_url_for_gemini_native_opaque_full_url() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://relay.example/custom/generate-content",
            AuthStrategy::Google,
            "gemini_native",
            true,
            "gemini-2.5-flash",
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }

    #[test]
    fn test_resolve_claude_stream_url_for_gemini_native_cloudflare_vertex_full_url() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent",
            AuthStrategy::Google,
            "gemini_native",
            true,
            "gemini-2.5-flash",
        );

        assert_eq!(
            url,
            "https://gateway.ai.cloudflare.com/v1/account/gateway/google-vertex-ai/v1/projects/project/locations/us-central1/publishers/google/models/gemini-3.1-pro-preview:streamGenerateContent?alt=sse"
        );
    }

    /// Regression: Gemini SDK outputs commonly surface model ids as the
    /// resource-name form `models/gemini-2.5-pro`. Interpolating that raw
    /// value used to produce `/v1beta/models/models/gemini-2.5-pro:...`
    /// which the upstream rejects and the health check records as a
    /// false-negative for an otherwise valid provider.
    #[test]
    fn test_resolve_claude_stream_url_for_gemini_native_strips_models_prefix() {
        let url = StreamCheckService::resolve_claude_stream_url(
            "https://generativelanguage.googleapis.com",
            AuthStrategy::Google,
            "gemini_native",
            false,
            "models/gemini-2.5-pro",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_resolve_codex_stream_urls_for_full_url_mode() {
        let urls = StreamCheckService::resolve_codex_stream_urls(
            "https://relay.example/custom/responses",
            true,
        );

        assert_eq!(urls, vec!["https://relay.example/custom/responses"]);
    }

    #[test]
    fn test_resolve_codex_stream_urls_for_v1_base() {
        let urls =
            StreamCheckService::resolve_codex_stream_urls("https://api.openai.com/v1", false);

        assert_eq!(urls, vec!["https://api.openai.com/v1/responses"]);
    }

    #[test]
    fn test_resolve_codex_stream_urls_for_origin_base() {
        let urls = StreamCheckService::resolve_codex_stream_urls("https://api.openai.com", false);

        assert_eq!(
            urls,
            vec![
                "https://api.openai.com/responses",
                "https://api.openai.com/v1/responses",
            ]
        );
    }
}
