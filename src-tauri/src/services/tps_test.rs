//! Provider TPS（Token Per Second）测试服务
//!
//! 对指定 Provider 发起一次非流式请求，统计输出 token 数与响应时间，并计算 TPS。
//! 优先使用上游返回的 usage 统计；当 usage 缺失时，回退到本地估算（UTF-8 字节数 / 4 向上取整）。

use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::custom_headers::apply_custom_headers_to_request;
use crate::proxy::providers::get_adapter;
use crate::proxy::usage::parser::TokenUsage;

/// token 统计来源
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TokenSource {
    Usage,
    Estimated,
}

/// TPS 测试结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TpsTestResult {
    pub success: bool,
    pub message: String,
    pub model_used: String,
    pub http_status: Option<u16>,
    pub response_time_ms: u64,
    pub output_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
    pub token_source: Option<TokenSource>,
    pub tested_at: i64,
}

pub struct TpsTestService;

impl TpsTestService {
    const TEST_PROMPT: &'static str = "hello，介绍一下你自己";
    const MAX_TOKENS: u32 = 128;

    pub async fn test_once(app_type: &AppType, provider: &Provider, timeout_secs: u64) -> TpsTestResult {
        let start = Instant::now();
        let tested_at = chrono::Utc::now().timestamp();
        let adapter = get_adapter(app_type);

        let base_url = match adapter.extract_base_url(provider) {
            Ok(url) => url,
            Err(e) => {
                return Self::failed(
                    "提取 base_url 失败",
                    None,
                    start.elapsed().as_millis() as u64,
                    tested_at,
                    e.to_string(),
                );
            }
        };

        let auth = match adapter.extract_auth(provider) {
            Some(auth) => auth,
            None => {
                return Self::failed(
                    "未找到 API Key",
                    None,
                    start.elapsed().as_millis() as u64,
                    tested_at,
                    "missing_api_key".to_string(),
                );
            }
        };

        let client = match Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("cc-switch/1.0")
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return Self::failed(
                    "创建客户端失败",
                    None,
                    start.elapsed().as_millis() as u64,
                    tested_at,
                    e.to_string(),
                );
            }
        };

        let model_to_test = Self::resolve_test_model(app_type, provider);

        let (status, body) = match app_type {
            AppType::Claude => {
                Self::call_claude(&client, &base_url, &auth, provider, &model_to_test).await
            }
            AppType::Codex => Self::call_openai_chat(
                &client,
                &base_url,
                &auth,
                provider,
                &model_to_test,
                true,
            )
            .await,
            AppType::Gemini => Self::call_gemini_generate_content(
                &client,
                &base_url,
                &auth,
                provider,
                &model_to_test,
            )
            .await,
        };

        let response_time_ms = start.elapsed().as_millis() as u64;

        match (status, body) {
            (Ok(http_status), Ok(json_body)) => {
                let (output_text, output_tokens, token_source) =
                    Self::extract_output_tokens(app_type, provider, &json_body);

                let (output_tokens, token_source) = match (output_tokens, token_source) {
                    (Some(tokens), Some(source)) => (Some(tokens), Some(source)),
                    _ => {
                        let text = match output_text {
                            Some(t) if !t.trim().is_empty() => t,
                            _ => {
                                return TpsTestResult {
                                    success: false,
                                    message: "解析响应失败：缺少输出文本".to_string(),
                                    model_used: model_to_test,
                                    http_status: Some(http_status),
                                    response_time_ms,
                                    output_tokens: None,
                                    tokens_per_second: None,
                                    token_source: None,
                                    tested_at,
                                };
                            }
                        };
                        (
                            Some(Self::estimate_tokens_by_utf8_bytes(&text)),
                            Some(TokenSource::Estimated),
                        )
                    }
                };

                let tokens_per_second = output_tokens.and_then(|t| {
                    if response_time_ms == 0 {
                        return None;
                    }
                    Some(t as f64 / (response_time_ms as f64 / 1000.0))
                });

                TpsTestResult {
                    success: true,
                    message: "测试成功".to_string(),
                    model_used: model_to_test,
                    http_status: Some(http_status),
                    response_time_ms,
                    output_tokens,
                    tokens_per_second,
                    token_source,
                    tested_at,
                }
            }
            (Ok(http_status), Err(e)) => TpsTestResult {
                success: false,
                message: e,
                model_used: model_to_test,
                http_status: Some(http_status),
                response_time_ms,
                output_tokens: None,
                tokens_per_second: None,
                token_source: None,
                tested_at,
            },
            (Err(e), _) => TpsTestResult {
                success: false,
                message: e,
                model_used: model_to_test,
                http_status: None,
                response_time_ms,
                output_tokens: None,
                tokens_per_second: None,
                token_source: None,
                tested_at,
            },
        }
    }

    async fn call_claude(
        client: &Client,
        base_url: &str,
        auth: &crate::proxy::providers::AuthInfo,
        provider: &Provider,
        model: &str,
    ) -> (Result<u16, String>, Result<Value, String>) {
        let adapter = get_adapter(&AppType::Claude);
        let needs_transform = adapter.needs_transform(provider);

        let (endpoint, body) = if needs_transform {
            let anthropic_body = serde_json::json!({
                "model": model,
                "max_tokens": Self::MAX_TOKENS,
                "messages": [{ "role": "user", "content": Self::TEST_PROMPT }],
            });
            let openai_body = match adapter.transform_request(anthropic_body, provider) {
                Ok(b) => b,
                Err(e) => {
                    return (
                        Err("转换请求失败".to_string()),
                        Err(e.to_string()),
                    );
                }
            };
            ("/v1/chat/completions", openai_body)
        } else {
            (
                "/v1/messages",
                serde_json::json!({
                    "model": model,
                    "max_tokens": Self::MAX_TOKENS,
                    "messages": [{ "role": "user", "content": Self::TEST_PROMPT }],
                }),
            )
        };

        let url = adapter.build_url(base_url, endpoint);

        let request = client.post(&url).header("Content-Type", "application/json");
        let request = adapter.add_auth_headers(request, auth).json(&body);
        let mut built = match request.build() {
            Ok(r) => r,
            Err(e) => return (Err("构建请求失败".to_string()), Err(e.to_string())),
        };
        apply_custom_headers_to_request(provider, &mut built);

        let response = match client.execute(built).await {
            Ok(r) => r,
            Err(e) => return (Err(Self::map_request_error(e)), Err("".to_string())),
        };
        let status = response.status().as_u16();

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => return (Ok(status), Err(format!("读取响应失败: {e}"))),
        };

        if !(200..300).contains(&status) {
            return (Ok(status), Err(format!("HTTP {status}: {text}")));
        }

        match serde_json::from_str::<Value>(&text) {
            Ok(v) => (Ok(status), Ok(v)),
            Err(e) => (Ok(status), Err(format!("解析 JSON 失败: {e}"))),
        }
    }

    async fn call_openai_chat(
        client: &Client,
        base_url: &str,
        auth: &crate::proxy::providers::AuthInfo,
        provider: &Provider,
        model: &str,
        is_codex: bool,
    ) -> (Result<u16, String>, Result<Value, String>) {
        let app_type = if is_codex { AppType::Codex } else { AppType::Gemini };
        let adapter = get_adapter(&app_type);
        let url = adapter.build_url(base_url, "/v1/chat/completions");

        let (actual_model, reasoning_effort) = Self::parse_model_with_effort(model);

        let mut body = serde_json::json!({
            "model": actual_model,
            "messages": [{ "role": "user", "content": Self::TEST_PROMPT }],
            "max_tokens": Self::MAX_TOKENS,
            "temperature": 0,
            "stream": false
        });

        if is_codex {
            if let Some(effort) = reasoning_effort {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
        }

        let request = client.post(&url).header("Content-Type", "application/json");
        let request = adapter.add_auth_headers(request, auth).json(&body);
        let mut built = match request.build() {
            Ok(r) => r,
            Err(e) => return (Err("构建请求失败".to_string()), Err(e.to_string())),
        };
        apply_custom_headers_to_request(provider, &mut built);

        let response = match client.execute(built).await {
            Ok(r) => r,
            Err(e) => return (Err(Self::map_request_error(e)), Err("".to_string())),
        };
        let status = response.status().as_u16();

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => return (Ok(status), Err(format!("读取响应失败: {e}"))),
        };

        if !(200..300).contains(&status) {
            return (Ok(status), Err(format!("HTTP {status}: {text}")));
        }

        match serde_json::from_str::<Value>(&text) {
            Ok(v) => (Ok(status), Ok(v)),
            Err(e) => (Ok(status), Err(format!("解析 JSON 失败: {e}"))),
        }
    }

    async fn call_gemini_generate_content(
        client: &Client,
        base_url: &str,
        auth: &crate::proxy::providers::AuthInfo,
        provider: &Provider,
        model: &str,
    ) -> (Result<u16, String>, Result<Value, String>) {
        let adapter = get_adapter(&AppType::Gemini);
        let endpoint = format!("/v1beta/models/{model}:generateContent");
        let url = adapter.build_url(base_url, &endpoint);

        let body = serde_json::json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{ "text": Self::TEST_PROMPT }]
                }
            ],
            "generationConfig": {
                "maxOutputTokens": Self::MAX_TOKENS,
                "temperature": 0
            }
        });

        let request = client.post(&url).header("Content-Type", "application/json");
        let request = adapter.add_auth_headers(request, auth).json(&body);
        let mut built = match request.build() {
            Ok(r) => r,
            Err(e) => return (Err("构建请求失败".to_string()), Err(e.to_string())),
        };
        apply_custom_headers_to_request(provider, &mut built);

        let response = match client.execute(built).await {
            Ok(r) => r,
            Err(e) => return (Err(Self::map_request_error(e)), Err("".to_string())),
        };
        let status = response.status().as_u16();

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => return (Ok(status), Err(format!("读取响应失败: {e}"))),
        };

        if !(200..300).contains(&status) {
            return (Ok(status), Err(format!("HTTP {status}: {text}")));
        }

        match serde_json::from_str::<Value>(&text) {
            Ok(v) => (Ok(status), Ok(v)),
            Err(e) => (Ok(status), Err(format!("解析 JSON 失败: {e}"))),
        }
    }

    fn extract_output_tokens(
        app_type: &AppType,
        provider: &Provider,
        body: &Value,
    ) -> (Option<String>, Option<u64>, Option<TokenSource>) {
        match app_type {
            AppType::Claude => {
                let adapter = get_adapter(app_type);
                let needs_transform = adapter.needs_transform(provider);
                if needs_transform {
                    // OpenAI 格式（Anthropic → OpenAI 转换）
                    let output_text = Self::extract_openai_output_text(body);
                    let usage = TokenUsage::from_openai_response(body);
                    let tokens = usage.map(|u| u.output_tokens as u64);
                    return (output_text, tokens, tokens.map(|_| TokenSource::Usage));
                }

                // Anthropic 格式
                let output_text = Self::extract_claude_output_text(body);
                let usage = TokenUsage::from_claude_response(body);
                let tokens = usage.map(|u| u.output_tokens as u64);
                (output_text, tokens, tokens.map(|_| TokenSource::Usage))
            }
            AppType::Codex => {
                let output_text = Self::extract_openai_output_text(body);
                let usage = TokenUsage::from_codex_response_auto(body);
                let tokens = usage.map(|u| u.output_tokens as u64);
                (output_text, tokens, tokens.map(|_| TokenSource::Usage))
            }
            AppType::Gemini => {
                let output_text = Self::extract_gemini_output_text(body);
                let usage = TokenUsage::from_gemini_response(body);
                let tokens = usage.map(|u| u.output_tokens as u64);
                (output_text, tokens, tokens.map(|_| TokenSource::Usage))
            }
        }
    }

    fn extract_openai_output_text(body: &Value) -> Option<String> {
        let choice0 = body.get("choices")?.get(0)?;
        let content = choice0
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        if content.is_some() {
            return content;
        }

        choice0
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn extract_claude_output_text(body: &Value) -> Option<String> {
        let content = body.get("content")?;
        match content {
            Value::String(s) => Some(s.to_string()),
            Value::Array(arr) => {
                let mut out = String::new();
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        out.push_str(text);
                    }
                }
                if out.is_empty() { None } else { Some(out) }
            }
            _ => None,
        }
    }

    fn extract_gemini_output_text(body: &Value) -> Option<String> {
        let candidates = body.get("candidates")?.as_array()?;
        let first = candidates.first()?;
        let parts = first
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())?;

        let mut out = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    fn estimate_tokens_by_utf8_bytes(text: &str) -> u64 {
        let bytes = text.as_bytes().len() as u64;
        (bytes + 3) / 4
    }

    fn resolve_test_model(app_type: &AppType, provider: &Provider) -> String {
        match app_type {
            AppType::Claude => Self::extract_env_model(provider, "ANTHROPIC_MODEL")
                .unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string()),
            AppType::Codex => Self::extract_codex_model(provider)
                .unwrap_or_else(|| "gpt-5.1-codex@low".to_string()),
            AppType::Gemini => Self::extract_env_model(provider, "GEMINI_MODEL")
                .unwrap_or_else(|| "gemini-3-pro-preview".to_string()),
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
        let config_text = provider.settings_config.get("config")?.as_str()?;
        if config_text.trim().is_empty() {
            return None;
        }

        let re = Regex::new(r#"^model\s*=\s*["']([^"']+)["']"#).ok()?;
        re.captures(config_text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty())
    }

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

    fn map_request_error(e: reqwest::Error) -> String {
        if e.is_timeout() {
            "请求超时".to_string()
        } else if e.is_connect() {
            format!("连接失败: {e}")
        } else {
            e.to_string()
        }
    }

    fn failed(
        reason: &str,
        http_status: Option<u16>,
        response_time_ms: u64,
        tested_at: i64,
        detail: String,
    ) -> TpsTestResult {
        TpsTestResult {
            success: false,
            message: format!("{reason}: {detail}"),
            model_used: String::new(),
            http_status,
            response_time_ms,
            output_tokens: None,
            tokens_per_second: None,
            token_source: None,
            tested_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_estimate_tokens_by_utf8_bytes() {
        assert_eq!(TpsTestService::estimate_tokens_by_utf8_bytes(""), 0);
        assert_eq!(TpsTestService::estimate_tokens_by_utf8_bytes("abcd"), 1);
        assert_eq!(TpsTestService::estimate_tokens_by_utf8_bytes("abcde"), 2);
        assert_eq!(TpsTestService::estimate_tokens_by_utf8_bytes("你好"), 2); // UTF-8 6 bytes -> ceil(6/4)=2
    }

    #[test]
    fn test_extract_openai_output_text() {
        let body = json!({
            "choices": [{
                "message": { "content": "hello world" }
            }]
        });
        assert_eq!(
            TpsTestService::extract_openai_output_text(&body).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn test_extract_claude_output_text() {
        let body = json!({
            "content": [
                { "type": "text", "text": "你好" },
                { "type": "text", "text": "世界" }
            ]
        });
        assert_eq!(
            TpsTestService::extract_claude_output_text(&body).as_deref(),
            Some("你好世界")
        );
    }

    #[test]
    fn test_extract_output_tokens_usage_openai() {
        let provider = Provider {
            id: "p".to_string(),
            name: "p".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        let body = json!({
            "usage": { "prompt_tokens": 10, "completion_tokens": 123 },
            "choices": [{ "message": { "content": "x" } }]
        });
        let (_text, tokens, source) =
            TpsTestService::extract_output_tokens(&AppType::Codex, &provider, &body);
        assert_eq!(tokens, Some(123));
        assert_eq!(source, Some(TokenSource::Usage));
    }

    #[test]
    fn test_extract_gemini_output_text() {
        let body = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "你好" }, { "text": "世界" }] }
            }]
        });
        assert_eq!(
            TpsTestService::extract_gemini_output_text(&body).as_deref(),
            Some("你好世界")
        );
    }
}
