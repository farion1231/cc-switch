//! 流式健康检查服务
//!
//! 使用流式 API 进行快速健康检查，只需接收首个 chunk 即判定成功。

use futures::StreamExt;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

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
            message: "检查失败".to_string(),
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
            .map_err(|e| AppError::Message(format!("提取 base_url 失败: {e}")))?;

        let auth = adapter
            .extract_auth(provider)
            .ok_or_else(|| AppError::Message("未找到 API Key".to_string()))?;

        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent("cc-switch/1.0")
            .build()
            .map_err(|e| AppError::Message(format!("创建客户端失败: {e}")))?;

        let model_to_test = Self::resolve_test_model(app_type, provider, config);

        let result = match app_type {
            AppType::Claude => {
                Self::check_claude_stream(&client, &base_url, &auth, &model_to_test).await
            }
            AppType::Codex => {
                Self::check_codex_stream(&client, &base_url, &auth, &model_to_test).await
            }
            AppType::Gemini => {
                Self::check_gemini_stream(&client, &base_url, &auth, &model_to_test).await
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
                    message: "检查成功".to_string(),
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
    async fn check_claude_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        };

        let body = json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "hi" }],
            "stream": true
        });

        let response = client
            .post(&url)
            .header("x-api-key", &auth.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
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
                Err(e) => Err(AppError::Message(format!("读取流失败: {e}"))),
            }
        } else {
            Err(AppError::Message("未收到响应数据".to_string()))
        }
    }

    /// Codex 流式检查
    async fn check_codex_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        };

        // 解析模型名和推理等级 (支持 model@level 或 model#level 格式)
        let (actual_model, reasoning_effort) = Self::parse_model_with_effort(model);

        let mut body = json!({
            "model": actual_model,
            "messages": [
                { "role": "system", "content": "" },
                { "role": "assistant", "content": "" },
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 1,
            "temperature": 0,
            "stream": true
        });

        // 如果是推理模型，添加 reasoning_effort
        if let Some(effort) = reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", auth.api_key))
            .header("Content-Type", "application/json")
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
                Err(e) => Err(AppError::Message(format!("读取流失败: {e}"))),
            }
        } else {
            Err(AppError::Message("未收到响应数据".to_string()))
        }
    }

    /// Gemini 流式检查
    async fn check_gemini_stream(
        client: &Client,
        base_url: &str,
        auth: &AuthInfo,
        model: &str,
    ) -> Result<(u16, String), AppError> {
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/v1/chat/completions");

        let body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 1,
            "temperature": 0,
            "stream": true
        });

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", auth.api_key))
            .header("Content-Type", "application/json")
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
                Err(e) => Err(AppError::Message(format!("读取流失败: {e}"))),
            }
        } else {
            Err(AppError::Message("未收到响应数据".to_string()))
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
        // 查找 @ 或 # 分隔符
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
        lower.contains("timeout")
            || lower.contains("abort")
            || lower.contains("中断")
            || lower.contains("超时")
    }

    fn map_request_error(e: reqwest::Error) -> AppError {
        if e.is_timeout() {
            AppError::Message("请求超时".to_string())
        } else if e.is_connect() {
            AppError::Message(format!("连接失败: {e}"))
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
        assert!(StreamCheckService::should_retry("请求超时"));
        assert!(StreamCheckService::should_retry("request timeout"));
        assert!(!StreamCheckService::should_retry("API Key 无效"));
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
}
