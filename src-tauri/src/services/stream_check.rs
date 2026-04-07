//! 流式健康检查服务
//!
//! 使用流式 API 进行快速健康检查，只需接收首个 chunk 即判定成功。

use futures::StreamExt;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::providers::copilot_auth;
use crate::proxy::providers::transform::anthropic_to_openai;
use crate::proxy::providers::transform_responses::anthropic_to_responses;
use crate::proxy::providers::{get_adapter, AuthInfo, AuthStrategy};

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
            codex_model: "gpt-5.1-codex@low".to_string(),
            gemini_model: "gemini-3-pro-preview".to_string(),
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
        let adapter = get_adapter(app_type);

        let base_url = match base_url_override {
            Some(base_url) => base_url,
            None => adapter
                .extract_base_url(provider)
                .map_err(|e| AppError::Message(format!("Failed to extract base_url: {e}")))?,
        };

        let auth = auth_override
            .or_else(|| adapter.extract_auth(provider))
            .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;

        // 获取 HTTP 客户端：优先使用供应商单独代理配置，否则使用全局客户端
        let proxy_config = provider.meta.as_ref().and_then(|m| m.proxy_config.as_ref());
        let client = crate::proxy::http_client::get_for_provider(proxy_config);
        let request_timeout = std::time::Duration::from_secs(config.timeout_secs);

        let model_to_test = Self::resolve_test_model(app_type, provider, config);
        let test_prompt = &config.test_prompt;

        let result = match app_type {
            AppType::Claude => {
                Self::check_claude_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    test_prompt,
                    request_timeout,
                    provider,
                    claude_api_format_override.as_deref(),
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
                )
                .await
            }
            AppType::OpenCode => {
                // OpenCode doesn't support stream check yet
                return Err(AppError::localized(
                    "opencode_no_stream_check",
                    "OpenCode 暂不支持健康检查",
                    "OpenCode does not support health check yet",
                ));
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support stream check yet
                return Err(AppError::localized(
                    "openclaw_no_stream_check",
                    "OpenClaw 暂不支持健康检查",
                    "OpenClaw does not support health check yet",
                ));
            }
        };

        let response_time = start.elapsed().as_millis() as u64;
        let tested_at = chrono::Utc::now().timestamp();

        match result {
            Ok((status_code, model)) => {
                let health_status =
                    Self::determine_status(response_time, config.degraded_threshold_ms);
                Ok(StreamCheckResult {
                    status: health_status,
                    success: true,
                    message: "Check succeeded".to_string(),
                    response_time_ms: Some(response_time),
                    http_status: Some(status_code),
                    model_used: model,
                    tested_at,
                    retry_count: 0,
                })
            }
            Err(e) => Ok(StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: e.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                model_used: String::new(),
                tested_at,
                retry_count: 0,
            }),
        }
    }

    /// Claude 流式检查
    ///
    /// 根据供应商的 api_format 选择请求格式：
    /// - "anthropic" (默认): Anthropic Messages API (/v1/messages)
    /// - "openai_chat": OpenAI Chat Completions API (/v1/chat/completions)
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
        let url =
            Self::resolve_claude_stream_url(base, auth.strategy, effective_api_format, is_full_url);

        let max_tokens = if is_openai_responses { 16 } else { 1 };

        // Build from Anthropic-native shape first, then convert for configured targets.
        let anthropic_body = json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [{ "role": "user", "content": test_prompt }],
            "stream": true
        });
        let body = if is_openai_responses {
            anthropic_to_responses(anthropic_body, Some(&provider.id))
                .map_err(|e| AppError::Message(format!("Failed to build test request: {e}")))?
        } else if is_openai_chat {
            anthropic_to_openai(anthropic_body, Some(&provider.id))
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

            request_builder =
                request_builder.header("authorization", format!("Bearer {}", auth.api_key));

            // Only Anthropic official strategy adds x-api-key
            if auth.strategy == AuthStrategy::Anthropic {
                request_builder = request_builder.header("x-api-key", &auth.api_key);
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
                .header("sec-fetch-mode", "cors")
                .header("connection", "keep-alive");
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
            return Err(AppError::Message(format!("HTTP {status}: {error_text}")));
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
                return Err(AppError::Message(format!("HTTP {status}: {error_text}")));
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
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        // Gemini 原生 API: /v1beta/models/{model}:streamGenerateContent?alt=sse
        // 智能处理 /v1beta 路径：如果 base_url 不包含版本路径，则添加 /v1beta
        // alt=sse 参数使 API 返回 SSE 格式（text/event-stream）而非 JSON 数组
        let url = if base.contains("/v1beta") || base.contains("/v1/") {
            format!("{base}/models/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse")
        };

        // Gemini 原生请求体格式
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": test_prompt }]
            }]
        });

        let response = client
            .post(&url)
            .header("x-goog-api-key", &auth.api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_request_error)?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Message(format!("HTTP {status}: {error_text}")));
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

    fn resolve_test_model(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> String {
        match app_type {
            AppType::Claude => Self::extract_env_model(provider, "ANTHROPIC_MODEL")
                .unwrap_or_else(|| config.claude_model.clone()),
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
            AppType::OpenClaw => {
                // OpenClaw uses models array in settings_config
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

        let re = Regex::new(r#"^model\s*=\s*["']([^"']+)["']"#).ok()?;
        re.captures(config_text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().trim().to_string())
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
    ) -> String {
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
        );

        assert_eq!(url, "https://api.anthropic.com/v1/messages");
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
