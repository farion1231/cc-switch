use futures::StreamExt;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::time::Instant;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderProxyConfig};
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub degraded_threshold_ms: u64,
    pub claude_model: String,
    pub codex_model: String,
    pub gemini_model: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthInfo {
    Anthropic(String),
    Bearer(String),
    GoogleApiKey(String),
    GoogleOAuth(String),
}

pub struct StreamCheckService;

impl StreamCheckService {
    pub fn get_config(state: &AppState) -> Result<StreamCheckConfig, AppError> {
        state.db.get_stream_check_config()
    }

    pub fn save_config(state: &AppState, config: &StreamCheckConfig) -> Result<(), AppError> {
        state.db.save_stream_check_config(config)
    }

    pub async fn check_provider(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
    ) -> Result<StreamCheckResult, AppError> {
        let config = Self::get_config(state)?;
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

        let result = Self::check_with_retry(&app_type, provider, &config).await?;
        let _ =
            state
                .db
                .save_stream_check_log(provider_id, &provider.name, app_type.as_str(), &result);
        Ok(result)
    }

    pub async fn check_all_providers(
        state: &AppState,
        app_type: AppType,
        proxy_targets_only: bool,
    ) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
        let config = Self::get_config(state)?;
        let providers = state.db.get_all_providers(app_type.as_str())?;

        let allowed_ids: Option<HashSet<String>> = if proxy_targets_only {
            let mut ids = HashSet::new();
            if let Some(current_id) = state.db.get_current_provider(app_type.as_str())? {
                ids.insert(current_id);
            }
            for item in state.db.get_failover_queue(app_type.as_str())? {
                ids.insert(item.provider_id);
            }
            Some(ids)
        } else {
            None
        };

        let mut results = Vec::new();
        for (provider_id, provider) in providers {
            if let Some(ids) = &allowed_ids {
                if !ids.contains(&provider_id) {
                    continue;
                }
            }

            let result = Self::check_with_retry(&app_type, &provider, &config)
                .await
                .unwrap_or_else(|error| StreamCheckResult {
                    status: HealthStatus::Failed,
                    success: false,
                    message: error.to_string(),
                    response_time_ms: None,
                    http_status: None,
                    model_used: String::new(),
                    tested_at: chrono::Utc::now().timestamp(),
                    retry_count: 0,
                });

            let _ = state.db.save_stream_check_log(
                &provider_id,
                &provider.name,
                app_type.as_str(),
                &result,
            );

            results.push((provider_id, result));
        }

        Ok(results)
    }

    pub async fn check_with_retry(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> Result<StreamCheckResult, AppError> {
        let effective_config = Self::merge_provider_config(provider, config);
        let mut last_result = None;

        for attempt in 0..=effective_config.max_retries {
            match Self::check_once(app_type, provider, &effective_config).await {
                Ok(success) if success.success => {
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..success
                    });
                }
                Ok(failed) => {
                    if Self::should_retry(&failed.message) && attempt < effective_config.max_retries
                    {
                        last_result = Some(failed);
                        continue;
                    }
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..failed
                    });
                }
                Err(error) => {
                    if Self::should_retry(&error.to_string())
                        && attempt < effective_config.max_retries
                    {
                        continue;
                    }
                    return Err(error);
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

    fn merge_provider_config(
        provider: &Provider,
        global_config: &StreamCheckConfig,
    ) -> StreamCheckConfig {
        let test_config = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.test_config.as_ref())
            .filter(|config| config.enabled);

        match test_config {
            Some(config) => StreamCheckConfig {
                timeout_secs: config.timeout_secs.unwrap_or(global_config.timeout_secs),
                max_retries: config.max_retries.unwrap_or(global_config.max_retries),
                degraded_threshold_ms: config
                    .degraded_threshold_ms
                    .unwrap_or(global_config.degraded_threshold_ms),
                claude_model: config
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.claude_model.clone()),
                codex_model: config
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.codex_model.clone()),
                gemini_model: config
                    .test_model
                    .clone()
                    .unwrap_or_else(|| global_config.gemini_model.clone()),
                test_prompt: config
                    .test_prompt
                    .clone()
                    .unwrap_or_else(|| global_config.test_prompt.clone()),
            },
            None => global_config.clone(),
        }
    }

    async fn check_once(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> Result<StreamCheckResult, AppError> {
        let start = Instant::now();
        let client = build_client(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.proxy_config.as_ref()),
        )?;
        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let model_to_test = Self::resolve_test_model(app_type, provider, config);
        let result = match app_type {
            AppType::Claude => {
                let base_url = extract_claude_base_url(provider)?;
                let auth = extract_claude_auth(provider)
                    .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;
                Self::check_claude_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    &config.test_prompt,
                    timeout,
                    provider,
                )
                .await
            }
            AppType::Codex => {
                let base_url = extract_codex_base_url(provider)?;
                let auth = extract_codex_auth(provider)
                    .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;
                Self::check_codex_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    &config.test_prompt,
                    timeout,
                )
                .await
            }
            AppType::Gemini => {
                let base_url = extract_gemini_base_url(provider);
                let auth = extract_gemini_auth(provider)
                    .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;
                Self::check_gemini_stream(
                    &client,
                    &base_url,
                    &auth,
                    &model_to_test,
                    &config.test_prompt,
                    timeout,
                )
                .await
            }
            AppType::OpenCode => {
                return Err(AppError::localized(
                    "opencode_no_stream_check",
                    "OpenCode 暂不支持健康检查",
                    "OpenCode does not support health check yet",
                ));
            }
            AppType::OpenClaw => {
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
            Ok((status_code, model)) => Ok(StreamCheckResult {
                status: Self::determine_status(response_time, config.degraded_threshold_ms),
                success: true,
                message: "Check succeeded".to_string(),
                response_time_ms: Some(response_time),
                http_status: Some(status_code),
                model_used: model,
                tested_at,
                retry_count: 0,
            }),
            Err(error) => Ok(StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: error.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                model_used: String::new(),
                tested_at,
                retry_count: 0,
            }),
        }
    }

    async fn check_claude_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
        provider: &Provider,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let api_format = claude_api_format(provider);
        let is_openai_chat = api_format == "openai_chat";

        let url = if is_openai_chat {
            if base.ends_with("/v1") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            }
        } else if base.ends_with("/v1") {
            format!("{base}/messages?beta=true")
        } else {
            format!("{base}/v1/messages?beta=true")
        };

        let body = json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": test_prompt }],
            "stream": true
        });

        let mut request = client.post(&url);
        match auth {
            AuthInfo::Anthropic(api_key) => {
                request = request
                    .header("authorization", format!("Bearer {api_key}"))
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header(
                        "anthropic-beta",
                        "claude-code-20250219,interleaved-thinking-2025-05-14",
                    )
                    .header("anthropic-dangerous-direct-browser-access", "true")
                    .header("content-type", "application/json")
                    .header("accept", "application/json")
                    .header("accept-encoding", "identity")
                    .header("accept-language", "*")
                    .header("user-agent", "claude-cli/2.1.2 (external, cli)")
                    .header("x-app", "cli")
                    .header("x-stainless-lang", "js")
                    .header("x-stainless-package-version", "0.70.0")
                    .header("x-stainless-os", Self::get_os_name())
                    .header("x-stainless-arch", Self::get_arch_name())
                    .header("x-stainless-runtime", "node")
                    .header("x-stainless-runtime-version", "v22.20.0")
                    .header("x-stainless-retry-count", "0")
                    .header("x-stainless-timeout", "600")
                    .header("sec-fetch-mode", "cors")
                    .header("connection", "keep-alive");
            }
            AuthInfo::Bearer(api_key) => {
                request = request
                    .header("authorization", format!("Bearer {api_key}"))
                    .header("content-type", "application/json")
                    .header("accept", "application/json");
            }
            _ => {
                return Err(AppError::Message(
                    "Unsupported Claude auth strategy".to_string(),
                ));
            }
        }

        let response = request
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
                Err(error) => Err(AppError::Message(format!("Stream read failed: {error}"))),
            }
        } else {
            Err(AppError::Message("No response data received".to_string()))
        }
    }

    async fn check_codex_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        let AuthInfo::Bearer(api_key) = auth else {
            return Err(AppError::Message(
                "Unsupported Codex auth strategy".to_string(),
            ));
        };

        let base = base_url.trim_end_matches('/');
        let urls = if base.ends_with("/v1") {
            vec![format!("{base}/responses")]
        } else {
            vec![format!("{base}/responses"), format!("{base}/v1/responses")]
        };

        let (actual_model, reasoning_effort) = Self::parse_model_with_effort(model);
        let mut body = json!({
            "model": actual_model,
            "input": [{ "role": "user", "content": test_prompt }],
            "stream": true
        });
        if let Some(effort) = reasoning_effort {
            body["reasoning"] = json!({ "effort": effort });
        }

        for (index, url) in urls.iter().enumerate() {
            let response = client
                .post(url)
                .header("authorization", format!("Bearer {api_key}"))
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .header("accept-encoding", "identity")
                .header(
                    "user-agent",
                    format!(
                        "codex_cli_rs/0.80.0 ({} 15.7.2; {}) Terminal",
                        Self::get_os_name(),
                        Self::get_arch_name()
                    ),
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
                if index == 0 && status == 404 && urls.len() > 1 {
                    continue;
                }
                return Err(AppError::Message(format!("HTTP {status}: {error_text}")));
            }

            let mut stream = response.bytes_stream();
            if let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(_) => return Ok((status, actual_model)),
                    Err(error) => {
                        return Err(AppError::Message(format!("Stream read failed: {error}")));
                    }
                }
            }
            return Err(AppError::Message("No response data received".to_string()));
        }

        Err(AppError::Message(
            "No valid Codex responses endpoint found".to_string(),
        ))
    }

    async fn check_gemini_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let url = if base.contains("/v1beta") || base.contains("/v1/") {
            format!("{base}/models/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse")
        };

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": test_prompt }]
            }]
        });

        let request = match auth {
            AuthInfo::GoogleApiKey(api_key) => client.post(&url).header("x-goog-api-key", api_key),
            AuthInfo::GoogleOAuth(token) => client
                .post(&url)
                .header("Authorization", format!("Bearer {token}")),
            _ => {
                return Err(AppError::Message(
                    "Unsupported Gemini auth strategy".to_string(),
                ));
            }
        };

        let response = request
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
                Err(error) => Err(AppError::Message(format!("Stream read failed: {error}"))),
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

    fn parse_model_with_effort(model: &str) -> (String, Option<String>) {
        if let Some(position) = model.find('@').or_else(|| model.find('#')) {
            let actual_model = model[..position].to_string();
            let effort = model[position + 1..].to_string();
            if !effort.is_empty() {
                return (actual_model, Some(effort));
            }
        }
        (model.to_string(), None)
    }

    fn should_retry(message: &str) -> bool {
        let lower = message.to_lowercase();
        lower.contains("timeout") || lower.contains("abort") || lower.contains("timed out")
    }

    fn map_request_error(error: reqwest::Error) -> AppError {
        if error.is_timeout() {
            AppError::Message("Request timeout".to_string())
        } else if error.is_connect() {
            AppError::Message(format!("Connection failed: {error}"))
        } else {
            AppError::Message(error.to_string())
        }
    }

    fn resolve_test_model(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> String {
        match app_type {
            AppType::Claude => extract_env_model(provider, "ANTHROPIC_MODEL")
                .unwrap_or_else(|| config.claude_model.clone()),
            AppType::Codex => {
                extract_codex_model(provider).unwrap_or_else(|| config.codex_model.clone())
            }
            AppType::Gemini => extract_env_model(provider, "GEMINI_MODEL")
                .unwrap_or_else(|| config.gemini_model.clone()),
            AppType::OpenCode => {
                extract_opencode_model(provider).unwrap_or_else(|| "gpt-4o".to_string())
            }
            AppType::OpenClaw => {
                extract_openclaw_model(provider).unwrap_or_else(|| "gpt-4o".to_string())
            }
        }
    }

    fn get_os_name() -> &'static str {
        match std::env::consts::OS {
            "macos" => "MacOS",
            "linux" => "Linux",
            "windows" => "Windows",
            other => other,
        }
    }

    fn get_arch_name() -> &'static str {
        match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            "x86" => "x86",
            other => other,
        }
    }
}

fn build_client(proxy_config: Option<&ProviderProxyConfig>) -> Result<Client, AppError> {
    let mut builder = Client::builder();

    if let Some(proxy_config) = proxy_config.filter(|config| config.enabled) {
        let host = proxy_config
            .proxy_host
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::InvalidInput("代理已启用但缺少 proxyHost".to_string()))?;
        let port = proxy_config
            .proxy_port
            .ok_or_else(|| AppError::InvalidInput("代理已启用但缺少 proxyPort".to_string()))?;

        let scheme = match proxy_config.proxy_type.as_deref() {
            Some("http") | None => "http",
            Some("https") => "https",
            Some("socks5") | Some("socks5h") => "socks5h",
            Some(other) => {
                return Err(AppError::InvalidInput(format!("不支持的代理类型: {other}")))
            }
        };

        let mut proxy = reqwest::Proxy::all(format!("{scheme}://{host}:{port}"))
            .map_err(|error| AppError::Message(format!("构建代理配置失败: {error}")))?;

        if let Some(username) = proxy_config.proxy_username.as_deref() {
            if !username.is_empty() {
                proxy = proxy.basic_auth(
                    username,
                    proxy_config.proxy_password.as_deref().unwrap_or(""),
                );
            }
        }

        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|error| AppError::Message(format!("构建 HTTP 客户端失败: {error}")))
}

fn claude_api_format(provider: &Provider) -> &'static str {
    if let Some(meta) = provider.meta.as_ref() {
        if let Some(api_format) = meta.api_format.as_deref() {
            return if api_format == "openai_chat" {
                "openai_chat"
            } else {
                "anthropic"
            };
        }
    }

    if let Some(api_format) = provider
        .settings_config
        .get("api_format")
        .and_then(|value| value.as_str())
    {
        return if api_format == "openai_chat" {
            "openai_chat"
        } else {
            "anthropic"
        };
    }

    let compat_mode = provider.settings_config.get("openrouter_compat_mode");
    let enabled = match compat_mode {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::Number(value)) => value.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(value)) => {
            matches!(value.trim().to_lowercase().as_str(), "1" | "true")
        }
        _ => false,
    };

    if enabled {
        "openai_chat"
    } else {
        "anthropic"
    }
}

fn extract_claude_base_url(provider: &Provider) -> Result<String, AppError> {
    provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|value| value.as_str())
        .or_else(|| {
            provider
                .settings_config
                .get("base_url")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("baseURL")
                .and_then(|value| value.as_str())
        })
        .map(|value| value.trim_end_matches('/').to_string())
        .ok_or_else(|| AppError::Message("Claude Provider 缺少 base_url 配置".to_string()))
}

fn extract_claude_auth(provider: &Provider) -> Option<AuthInfo> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(|value| value.as_object());
    let bearer_only = provider
        .settings_config
        .get("auth_mode")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value == "bearer_only")
        || env
            .and_then(|object| object.get("AUTH_MODE"))
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "bearer_only");

    let key = env
        .and_then(|object| {
            object
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| object.get("ANTHROPIC_API_KEY"))
                .or_else(|| object.get("OPENROUTER_API_KEY"))
                .or_else(|| object.get("OPENAI_API_KEY"))
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiKey")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("api_key")
                .and_then(|value| value.as_str())
        })?
        .to_string();

    let uses_bearer_only = bearer_only
        || extract_claude_base_url(provider)
            .map(|base_url| base_url.contains("openrouter.ai"))
            .unwrap_or(false);

    Some(
        if uses_bearer_only || claude_api_format(provider) == "openai_chat" {
            AuthInfo::Bearer(key)
        } else {
            AuthInfo::Anthropic(key)
        },
    )
}

fn extract_codex_base_url(provider: &Provider) -> Result<String, AppError> {
    if let Some(value) = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|value| value.as_str())
    {
        return Ok(value.trim_end_matches('/').to_string());
    }

    if let Some(config) = provider.settings_config.get("config") {
        if let Some(value) = config.get("base_url").and_then(|value| value.as_str()) {
            return Ok(value.trim_end_matches('/').to_string());
        }

        if let Some(config_text) = config.as_str() {
            if let Some(captures) = Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#)
                .ok()
                .and_then(|regex| regex.captures(config_text))
            {
                if let Some(url) = captures.get(1) {
                    return Ok(url.as_str().trim_end_matches('/').to_string());
                }
            }
        }
    }

    Err(AppError::Message(
        "Codex Provider 缺少 base_url 配置".to_string(),
    ))
}

fn extract_codex_auth(provider: &Provider) -> Option<AuthInfo> {
    provider
        .settings_config
        .pointer("/env/OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .or_else(|| {
            provider
                .settings_config
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiKey")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("api_key")
                .and_then(|value| value.as_str())
        })
        .map(|value| AuthInfo::Bearer(value.to_string()))
}

fn extract_gemini_base_url(provider: &Provider) -> String {
    provider
        .settings_config
        .pointer("/env/GOOGLE_GEMINI_BASE_URL")
        .and_then(|value| value.as_str())
        .or_else(|| {
            provider
                .settings_config
                .get("base_url")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("baseURL")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("https://generativelanguage.googleapis.com")
        .trim_end_matches('/')
        .to_string()
}

fn extract_gemini_auth(provider: &Provider) -> Option<AuthInfo> {
    let raw = provider
        .settings_config
        .pointer("/env/GEMINI_API_KEY")
        .and_then(|value| value.as_str())
        .or_else(|| {
            provider
                .settings_config
                .get("apiKey")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("api_key")
                .and_then(|value| value.as_str())
        })?;

    if raw.starts_with("ya29.") {
        return Some(AuthInfo::GoogleOAuth(raw.to_string()));
    }

    if raw.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(access_token) = value.get("access_token").and_then(|value| value.as_str()) {
                if !access_token.is_empty() {
                    return Some(AuthInfo::GoogleOAuth(access_token.to_string()));
                }
            }
        }
    }

    Some(AuthInfo::GoogleApiKey(raw.to_string()))
}

fn extract_env_model(provider: &Provider, key: &str) -> Option<String> {
    provider
        .settings_config
        .pointer(&format!("/env/{key}"))
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

    let regex = Regex::new(r#"(?m)^model\s*=\s*["']([^"']+)["']"#).ok()?;
    regex
        .captures(config_text)
        .and_then(|captures| captures.get(1))
        .map(|match_| match_.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn extract_opencode_model(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("models")
        .and_then(|value| value.as_object())
        .and_then(|models| models.keys().next().map(|key| key.to_string()))
}

fn extract_openclaw_model(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("models")
        .and_then(|value| value.as_array())
        .and_then(|models| models.first())
        .and_then(|model| model.get("id"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(settings_config: serde_json::Value) -> Provider {
        Provider {
            id: "p1".to_string(),
            name: "Provider".to_string(),
            settings_config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn parse_model_with_effort_supports_at_and_hash() {
        assert_eq!(
            StreamCheckService::parse_model_with_effort("gpt-5.1-codex@low"),
            ("gpt-5.1-codex".to_string(), Some("low".to_string()))
        );
        assert_eq!(
            StreamCheckService::parse_model_with_effort("o1-preview#high"),
            ("o1-preview".to_string(), Some("high".to_string()))
        );
        assert_eq!(
            StreamCheckService::parse_model_with_effort("gpt-4o-mini"),
            ("gpt-4o-mini".to_string(), None)
        );
    }

    #[test]
    fn merge_provider_config_prefers_enabled_provider_overrides() {
        let mut provider = provider(json!({}));
        provider.meta = Some(crate::provider::ProviderMeta {
            test_config: Some(crate::provider::ProviderTestConfig {
                enabled: true,
                test_model: Some("claude-custom".to_string()),
                timeout_secs: Some(12),
                test_prompt: Some("ping".to_string()),
                degraded_threshold_ms: Some(1234),
                max_retries: Some(4),
            }),
            ..Default::default()
        });

        let merged =
            StreamCheckService::merge_provider_config(&provider, &StreamCheckConfig::default());
        assert_eq!(merged.timeout_secs, 12);
        assert_eq!(merged.max_retries, 4);
        assert_eq!(merged.test_prompt, "ping");
        assert_eq!(merged.claude_model, "claude-custom");
    }

    #[test]
    fn extract_codex_base_url_reads_toml_string() {
        let provider = provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-test" },
            "config": "base_url = \"https://api.openai.com/v1\"\nmodel = \"gpt-5\""
        }));

        let url = extract_codex_base_url(&provider).expect("base_url should parse");
        assert_eq!(url, "https://api.openai.com/v1");
    }

    #[test]
    fn extract_gemini_auth_understands_oauth_json() {
        let provider = provider(json!({
            "env": {
                "GEMINI_API_KEY": "{\"access_token\":\"ya29.test-token\"}"
            }
        }));

        let auth = extract_gemini_auth(&provider).expect("auth should exist");
        assert_eq!(auth, AuthInfo::GoogleOAuth("ya29.test-token".to_string()));
    }

    #[test]
    fn build_client_rejects_unknown_proxy_type() {
        let result = build_client(Some(&ProviderProxyConfig {
            enabled: true,
            proxy_type: Some("ftp".to_string()),
            proxy_host: Some("127.0.0.1".to_string()),
            proxy_port: Some(8080),
            proxy_username: None,
            proxy_password: None,
        }));
        assert!(result.is_err());
    }
}
