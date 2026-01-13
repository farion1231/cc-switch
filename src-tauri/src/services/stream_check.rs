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
use crate::proxy::providers::{get_adapter, AuthInfo};

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
    pub async fn check_with_retry(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> Result<StreamCheckResult, AppError> {
        let mut last_result = None;

        for attempt in 0..=config.max_retries {
            let result = Self::check_once(app_type, provider, config).await;

            match &result {
                Ok(r) if r.success => {
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..r.clone()
                    });
                }
                Ok(r) => {
                    // 失败但非异常，判断是否重试
                    if Self::should_retry(&r.message) && attempt < config.max_retries {
                        last_result = Some(r.clone());
                        continue;
                    }
                    return Ok(StreamCheckResult {
                        retry_count: attempt,
                        ..r.clone()
                    });
                }
                Err(e) => {
                    if Self::should_retry(&e.to_string()) && attempt < config.max_retries {
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
            retry_count: config.max_retries,
        }))
    }

    /// 单次流式检查
    async fn check_once(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
    ) -> Result<StreamCheckResult, AppError> {
        let start = Instant::now();
        let adapter = get_adapter(app_type);

        let base_url = adapter
            .extract_base_url(provider)
            .map_err(|e| AppError::Message(format!("Failed to extract base_url: {e}")))?;

        let auth = adapter
            .extract_auth(provider)
            .ok_or_else(|| AppError::Message("API Key not found".to_string()))?;

        // 使用全局 HTTP 客户端（已包含代理配置）
        let client = crate::proxy::http_client::get();
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
    /// 严格按照 Claude CLI 真实请求格式构建请求
    async fn check_claude_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        // URL 必须包含 ?beta=true 参数（某些中转服务依赖此参数验证请求来源）
        let url = if base.ends_with("/v1") {
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

        // 获取本地系统信息
        let os_name = Self::get_os_name();
        let arch_name = Self::get_arch_name();

        // 严格按照 Claude CLI 请求格式设置 headers
        let response = client
            .post(&url)
            // 认证 headers（双重认证）
            .header("authorization", format!("Bearer {}", auth.api_key))
            .header("x-api-key", &auth.api_key)
            // Anthropic 必需 headers
            .header("anthropic-version", "2023-06-01")
            .header(
                "anthropic-beta",
                "claude-code-20250219,interleaved-thinking-2025-05-14",
            )
            .header("anthropic-dangerous-direct-browser-access", "true")
            // 内容类型 headers
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .header("accept-language", "*")
            // 客户端标识 headers
            .header("user-agent", "claude-cli/2.1.2 (external, cli)")
            .header("x-app", "cli")
            // x-stainless SDK headers（动态获取本地系统信息）
            .header("x-stainless-lang", "js")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-os", os_name)
            .header("x-stainless-arch", arch_name)
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v22.20.0")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-timeout", "600")
            // 其他 headers
            .header("sec-fetch-mode", "cors")
            .header("connection", "keep-alive")
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
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        // Codex CLI 使用 /v1/responses 端点 (OpenAI Responses API)
        let url = if base.ends_with("/v1") {
            format!("{base}/responses")
        } else {
            format!("{base}/v1/responses")
        };

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

        // 严格按照 Codex CLI 请求格式设置 headers
        let response = client
            .post(&url)
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

    /// Gemini 流式检查
    async fn check_gemini_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
        test_prompt: &str,
        timeout: std::time::Duration,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/v1/chat/completions");

        let body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": test_prompt }],
            "max_tokens": 1,
            "temperature": 0,
            "stream": true
        });

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", auth.api_key))
            .header("Content-Type", "application/json")
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
        }
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
}
