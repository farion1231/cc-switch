use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ModelVerifyProtocol {
    OpenAiChat,
    AnthropicMessages,
    GeminiGenerateContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyRequest {
    pub protocol: ModelVerifyProtocol,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProbeStatus {
    Passed,
    Warning,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceLevel {
    Weak,
    Medium,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyProbe {
    pub id: String,
    pub label: String,
    pub group: String,
    pub weight: u8,
    pub status: ProbeStatus,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyProbeGroup {
    pub id: String,
    pub label: String,
    pub score: u8,
    pub max_score: u8,
    pub probes: Vec<ModelVerifyProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyScores {
    pub knowledge_qa_score: u8,
    pub model_feature_score: u8,
    pub protocol_consistency_score: u8,
    pub response_structure_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyMetrics {
    pub latency_ms: Option<u64>,
    pub latency_seconds: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVerifyResult {
    pub success: bool,
    pub tested_at: i64,
    pub model_requested: String,
    pub protocol: ModelVerifyProtocol,
    pub confidence_score: u8,
    pub mismatch_risk: u8,
    pub overall_confidence: u8,
    pub dilution_risk: u8,
    pub evidence_level: EvidenceLevel,
    pub scores: ModelVerifyScores,
    pub metrics: ModelVerifyMetrics,
    pub summary: String,
    pub total_latency_ms: Option<u64>,
    pub probes: Vec<ModelVerifyProbe>,
    pub probe_groups: Vec<ModelVerifyProbeGroup>,
    pub diagnostics: Vec<ModelVerifyProbe>,
}

fn default_timeout_secs() -> u64 {
    45
}

pub struct ModelVerifyService;

impl ModelVerifyService {
    pub async fn verify(request: ModelVerifyRequest) -> Result<ModelVerifyResult, AppError> {
        Self::validate_request(&request)?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(request.timeout_secs))
            .build()
            .map_err(|e| AppError::Message(format!("创建 HTTP 客户端失败: {e}")))?;

        let started = Instant::now();
        let probes = match request.protocol {
            ModelVerifyProtocol::OpenAiChat => Self::verify_openai_chat(&client, &request).await,
            ModelVerifyProtocol::AnthropicMessages => {
                Self::verify_anthropic_messages(&client, &request).await
            }
            ModelVerifyProtocol::GeminiGenerateContent => {
                Self::verify_gemini_generate_content(&client, &request).await
            }
        };

        let mut result = Self::score_result(request, probes);
        result.total_latency_ms = Some(started.elapsed().as_millis() as u64);
        Ok(result)
    }

    fn validate_request(request: &ModelVerifyRequest) -> Result<(), AppError> {
        if request.base_url.trim().is_empty() {
            return Err(AppError::InvalidInput("Base URL 不能为空".to_string()));
        }
        if request.api_key.trim().is_empty() {
            return Err(AppError::InvalidInput("API Key 不能为空".to_string()));
        }
        if request.model.trim().is_empty() {
            return Err(AppError::InvalidInput("模型名称不能为空".to_string()));
        }
        if !(5..=180).contains(&request.timeout_secs) {
            return Err(AppError::InvalidInput(
                "超时时间必须在 5 到 180 秒之间".to_string(),
            ));
        }
        reqwest::Url::parse(&request.base_url)
            .map_err(|_| AppError::InvalidInput("Base URL 格式无效".to_string()))?;
        Ok(())
    }

    async fn verify_openai_chat(client: &Client, request: &ModelVerifyRequest) -> Vec<ModelVerifyProbe> {
        vec![
            Self::run_openai_chat_probe(
                client,
                request,
                "protocol.openai.shape",
                "响应结构",
                "Reply with exactly: CCSWITCH_OK",
                "responseStructure",
                25,
                |content, response_model, raw| {
                    let has_choices = raw.get("choices").and_then(Value::as_array).is_some();
                    let has_usage = raw.get("usage").is_some();
                    if content.trim() == "CCSWITCH_OK" && has_choices {
                        (
                            ProbeStatus::Passed,
                            format!("OpenAI 响应结构可解析，响应模型字段为 {response_model}，usage={has_usage}"),
                        )
                    } else if has_choices {
                        (
                            ProbeStatus::Warning,
                            "响应结构可解析，但短指令或 usage 字段不稳定".to_string(),
                        )
                    } else {
                        (ProbeStatus::Failed, "响应缺少 choices 结构".to_string())
                    }
                },
            )
            .await,
            Self::run_openai_error_probe(client, request).await,
            Self::run_openai_chat_probe(
                client,
                request,
                "capability.json",
                "型号特征校验",
                "Return only compact JSON: {\"sum\": 579, \"language\": \"rust\"}.",
                "modelFeatures",
                25,
                |content, _response_model, _raw| match serde_json::from_str::<Value>(content.trim()) {
                    Ok(value)
                        if value.get("sum").and_then(Value::as_i64) == Some(579)
                            && value.get("language").and_then(Value::as_str) == Some("rust") =>
                    {
                        (ProbeStatus::Passed, "严格返回预期 JSON 能力探针".to_string())
                    }
                    Ok(_) => (
                        ProbeStatus::Warning,
                        "返回了 JSON，但字段值与预期不完全一致".to_string(),
                    ),
                    Err(_) => (
                        ProbeStatus::Warning,
                        "未严格返回可解析 JSON".to_string(),
                    ),
                },
            )
            .await,
            Self::run_openai_chat_probe(
                client,
                request,
                "knowledge.arithmetic",
                "知识问答校验",
                "Answer with only the number: what is 193 + 386?",
                "knowledgeQa",
                25,
                |content, _response_model, _raw| {
                    if content.trim().contains("579") {
                        (ProbeStatus::Passed, "基础知识/算术校验通过".to_string())
                    } else {
                        (ProbeStatus::Warning, "基础知识/算术答案与预期不一致".to_string())
                    }
                },
            )
            .await,
            Self::run_openai_chat_probe(
                client,
                request,
                "protocol.openai.consistency",
                "协议一致性",
                "Reply with exactly: OK",
                "protocolConsistency",
                25,
                |content, _response_model, raw| {
                    if raw.get("choices").is_some() && raw.get("id").is_some() {
                        (ProbeStatus::Passed, "OpenAI 协议字段一致".to_string())
                    } else if content.trim() == "OK" {
                        (ProbeStatus::Warning, "响应可用，但协议字段不完整".to_string())
                    } else {
                        (ProbeStatus::Warning, "协议一致性证据不足".to_string())
                    }
                },
            )
            .await,
            Self::run_openai_chat_probe(
                client,
                request,
                "identity.declaration",
                "身份声明一致性",
                "State your exact model identity in one short sentence.",
                "identity",
                10,
                |content, response_model, _raw| {
                    if content.to_lowercase().contains(&response_model.to_lowercase()) {
                        (
                            ProbeStatus::Passed,
                            "模型声明与响应 model 字段一致".to_string(),
                        )
                    } else {
                        (
                            ProbeStatus::Warning,
                            format!("模型声明未直接包含响应 model 字段 {response_model}"),
                        )
                    }
                },
            )
            .await,
        ]
    }

    async fn verify_anthropic_messages(
        client: &Client,
        request: &ModelVerifyRequest,
    ) -> Vec<ModelVerifyProbe> {
        vec![
            Self::run_anthropic_probe(
                client,
                request,
                "protocol.anthropic.shape",
                "响应结构",
                "Reply with exactly: CCSWITCH_OK",
                "responseStructure",
                25,
            )
            .await,
            Self::run_anthropic_error_probe(client, request).await,
            Self::run_anthropic_probe(
                client,
                request,
                "capability.anthropic.json",
                "型号特征校验",
                "Return only compact JSON: {\"sum\":579,\"language\":\"rust\"}.",
                "modelFeatures",
                25,
            )
            .await,
            Self::run_anthropic_probe(
                client,
                request,
                "knowledge.anthropic.arithmetic",
                "知识问答校验",
                "Answer with only the number: what is 193 + 386?",
                "knowledgeQa",
                25,
            )
            .await,
            Self::run_anthropic_probe(
                client,
                request,
                "protocol.anthropic.consistency",
                "协议一致性",
                "Reply with exactly: OK",
                "protocolConsistency",
                25,
            )
            .await,
        ]
    }

    async fn verify_gemini_generate_content(
        client: &Client,
        request: &ModelVerifyRequest,
    ) -> Vec<ModelVerifyProbe> {
        vec![
            Self::run_gemini_probe(
                client,
                request,
                "protocol.gemini.shape",
                "响应结构",
                "Reply with exactly: CCSWITCH_OK",
                "responseStructure",
                25,
            )
            .await,
            Self::run_gemini_error_probe(client, request).await,
            Self::run_gemini_probe(
                client,
                request,
                "capability.gemini.json",
                "型号特征校验",
                "Return only compact JSON: {\"sum\":579,\"language\":\"rust\"}.",
                "modelFeatures",
                25,
            )
            .await,
            Self::run_gemini_probe(
                client,
                request,
                "knowledge.gemini.arithmetic",
                "知识问答校验",
                "Answer with only the number: what is 193 + 386?",
                "knowledgeQa",
                25,
            )
            .await,
            Self::run_gemini_probe(
                client,
                request,
                "protocol.gemini.consistency",
                "协议一致性",
                "Reply with exactly: OK",
                "protocolConsistency",
                25,
            )
            .await,
        ]
    }

    async fn run_openai_chat_probe<F>(
        client: &Client,
        request: &ModelVerifyRequest,
        id: &str,
        label: &str,
        prompt: &str,
        group: &str,
        weight: u8,
        classify: F,
    ) -> ModelVerifyProbe
    where
        F: FnOnce(&str, &str, &Value) -> (ProbeStatus, String),
    {
        let started = Instant::now();
        let url = Self::openai_chat_url(&request.base_url);
        let mut builder = client
            .post(url)
            .bearer_auth(request.api_key.trim())
            .json(&json!({
                "model": request.model.trim(),
                "messages": [{ "role": "user", "content": prompt }],
                "temperature": 0,
                "max_tokens": 120,
                "stream": false
            }));

        if let Some(org) = request.organization.as_deref() {
            if !org.trim().is_empty() {
                builder = builder.header("OpenAI-Organization", org.trim());
            }
        }

        match builder.send().await {
            Ok(response) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Self::probe(
                        id,
                        label,
                        group,
                        weight,
                        ProbeStatus::Failed,
                        Some(latency_ms),
                        format!("HTTP {}", status.as_u16()),
                        Some(body),
                    );
                }

                match serde_json::from_str::<Value>(&body) {
                    Ok(value) => {
                        let content = value
                            .pointer("/choices/0/message/content")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        let response_model = value
                            .get("model")
                            .and_then(Value::as_str)
                            .unwrap_or(request.model.as_str())
                            .to_string();
                        let (status, message) = classify(&content, &response_model, &value);
                        let excerpt = json!({
                            "content": content,
                            "model": response_model,
                            "usage": value.get("usage").cloned()
                        })
                        .to_string();
                        Self::probe(
                            id,
                            label,
                            group,
                            weight,
                            status,
                            Some(latency_ms),
                            message,
                            Some(excerpt),
                        )
                    }
                    Err(_) => Self::probe(
                        id,
                        label,
                        group,
                        weight,
                        ProbeStatus::Failed,
                        Some(latency_ms),
                        "响应不是有效 JSON".to_string(),
                        Some(body),
                    ),
                }
            }
            Err(error) => Self::probe(
                id,
                label,
                group,
                weight,
                ProbeStatus::Failed,
                None,
                error.to_string(),
                None,
            ),
        }
    }

    async fn run_openai_error_probe(
        client: &Client,
        request: &ModelVerifyRequest,
    ) -> ModelVerifyProbe {
        let started = Instant::now();
        let url = Self::openai_chat_url(&request.base_url);
        let response = client
            .post(url)
            .bearer_auth(request.api_key.trim())
            .json(&json!({
                "model": request.model.trim(),
                "messages": [{ "role": "user", "content": "probe" }],
                "temperature": "invalid"
            }))
            .send()
            .await;

        Self::classify_error_probe(
            "error.openai.shape",
            "错误形态",
            "errorShape",
            25,
            started,
            response,
            |status, body| {
                let parsed = serde_json::from_str::<Value>(body).ok();
                let has_openai_error = parsed
                    .as_ref()
                    .and_then(|v| v.get("error"))
                    .and_then(|v| v.get("message"))
                    .is_some();
                if status.is_client_error() && has_openai_error {
                    (ProbeStatus::Passed, "错误结构符合 OpenAI 风格".to_string())
                } else if status.is_client_error() {
                    (ProbeStatus::Warning, "返回客户端错误，但结构不完全符合 OpenAI 风格".to_string())
                } else {
                    (ProbeStatus::Warning, format!("非法参数未触发预期客户端错误: HTTP {}", status.as_u16()))
                }
            },
        )
        .await
    }

    async fn run_anthropic_probe(
        client: &Client,
        request: &ModelVerifyRequest,
        id: &str,
        label: &str,
        prompt: &str,
        group: &str,
        weight: u8,
    ) -> ModelVerifyProbe {
        let started = Instant::now();
        let response = client
            .post(Self::anthropic_messages_url(&request.base_url))
            .header("x-api-key", request.api_key.trim())
            .header(
                "anthropic-version",
                request
                    .api_version
                    .as_deref()
                    .unwrap_or("2023-06-01")
                    .trim(),
            )
            .json(&json!({
                "model": request.model.trim(),
                "max_tokens": 120,
                "temperature": 0,
                "messages": [{ "role": "user", "content": prompt }]
            }))
            .send()
            .await;

        Self::classify_success_probe(id, label, group, weight, started, response, |value| {
            let content = value
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            let excerpt = json!({
                "content": content,
                "usage": value.get("usage").cloned()
            })
            .to_string();
            let response_type = value.get("type").and_then(Value::as_str);
            if response_type == Some("message") && !content.is_empty() {
                (ProbeStatus::Passed, "Anthropic Messages 响应结构可解析".to_string(), excerpt)
            } else {
                (ProbeStatus::Warning, "Anthropic 响应结构不完整".to_string(), excerpt)
            }
        })
        .await
    }

    async fn run_anthropic_error_probe(
        client: &Client,
        request: &ModelVerifyRequest,
    ) -> ModelVerifyProbe {
        let started = Instant::now();
        let response = client
            .post(Self::anthropic_messages_url(&request.base_url))
            .header("x-api-key", request.api_key.trim())
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": request.model.trim(),
                "max_tokens": 0,
                "messages": [{ "role": "user", "content": "probe" }]
            }))
            .send()
            .await;

        Self::classify_error_probe(
            "error.anthropic.shape",
            "错误形态",
            "errorShape",
            25,
            started,
            response,
            |status, body| {
                let parsed = serde_json::from_str::<Value>(body).ok();
                let has_anthropic_error = parsed
                    .as_ref()
                    .and_then(|v| v.get("error"))
                    .and_then(|v| v.get("type"))
                    .is_some();
                if status.is_client_error() && has_anthropic_error {
                    (ProbeStatus::Passed, "错误结构符合 Anthropic 风格".to_string())
                } else if status.is_client_error() {
                    (ProbeStatus::Warning, "返回客户端错误，但结构不完全符合 Anthropic 风格".to_string())
                } else {
                    (ProbeStatus::Warning, format!("非法参数未触发预期客户端错误: HTTP {}", status.as_u16()))
                }
            },
        )
        .await
    }

    async fn run_gemini_probe(
        client: &Client,
        request: &ModelVerifyRequest,
        id: &str,
        label: &str,
        prompt: &str,
        group: &str,
        weight: u8,
    ) -> ModelVerifyProbe {
        let started = Instant::now();
        let response = client
            .post(Self::gemini_generate_url(&request.base_url, &request.model))
            .header("x-goog-api-key", request.api_key.trim())
            .json(&json!({
                "contents": [{ "parts": [{ "text": prompt }] }],
                "generationConfig": { "temperature": 0, "maxOutputTokens": 120 }
            }))
            .send()
            .await;

        Self::classify_success_probe(id, label, group, weight, started, response, |value| {
            let content = value
                .pointer("/candidates/0/content/parts/0/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            let excerpt = json!({
                "content": content,
                "usageMetadata": value.get("usageMetadata").cloned()
            })
            .to_string();
            let has_candidates = value.get("candidates").and_then(Value::as_array).is_some();
            if has_candidates && !content.is_empty() {
                (ProbeStatus::Passed, "Gemini generateContent 响应结构可解析".to_string(), excerpt)
            } else {
                (ProbeStatus::Warning, "Gemini 响应结构不完整".to_string(), excerpt)
            }
        })
        .await
    }

    async fn run_gemini_error_probe(
        client: &Client,
        request: &ModelVerifyRequest,
    ) -> ModelVerifyProbe {
        let started = Instant::now();
        let response = client
            .post(Self::gemini_generate_url(&request.base_url, &request.model))
            .header("x-goog-api-key", request.api_key.trim())
            .json(&json!({ "contents": [] }))
            .send()
            .await;

        Self::classify_error_probe(
            "error.gemini.shape",
            "错误形态",
            "errorShape",
            25,
            started,
            response,
            |status, body| {
                let parsed = serde_json::from_str::<Value>(body).ok();
                let has_google_error = parsed
                    .as_ref()
                    .and_then(|v| v.get("error"))
                    .and_then(|v| v.get("status"))
                    .is_some();
                if status.is_client_error() && has_google_error {
                    (ProbeStatus::Passed, "错误结构符合 Gemini/Google 风格".to_string())
                } else if status.is_client_error() {
                    (ProbeStatus::Warning, "返回客户端错误，但结构不完全符合 Gemini 风格".to_string())
                } else {
                    (ProbeStatus::Warning, format!("非法参数未触发预期客户端错误: HTTP {}", status.as_u16()))
                }
            },
        )
        .await
    }

    async fn classify_success_probe<F>(
        id: &str,
        label: &str,
        group: &str,
        weight: u8,
        started: Instant,
        response: Result<reqwest::Response, reqwest::Error>,
        classify: F,
    ) -> ModelVerifyProbe
    where
        F: FnOnce(&Value) -> (ProbeStatus, String, String),
    {
        match response {
            Ok(response) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Self::probe(
                        id,
                        label,
                        group,
                        weight,
                        ProbeStatus::Failed,
                        Some(latency_ms),
                        format!("HTTP {}", status.as_u16()),
                        Some(body),
                    );
                }
                match serde_json::from_str::<Value>(&body) {
                    Ok(value) => {
                        let (status, message, excerpt) = classify(&value);
                        Self::probe(
                            id,
                            label,
                            group,
                            weight,
                            status,
                            Some(latency_ms),
                            message,
                            Some(excerpt),
                        )
                    }
                    Err(_) => Self::probe(
                        id,
                        label,
                        group,
                        weight,
                        ProbeStatus::Failed,
                        Some(latency_ms),
                        "响应不是有效 JSON".to_string(),
                        Some(body),
                    ),
                }
            }
            Err(error) => Self::probe(
                id,
                label,
                group,
                weight,
                ProbeStatus::Failed,
                None,
                error.to_string(),
                None,
            ),
        }
    }

    async fn classify_error_probe<F>(
        id: &str,
        label: &str,
        group: &str,
        weight: u8,
        started: Instant,
        response: Result<reqwest::Response, reqwest::Error>,
        classify: F,
    ) -> ModelVerifyProbe
    where
        F: FnOnce(reqwest::StatusCode, &str) -> (ProbeStatus, String),
    {
        match response {
            Ok(response) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let (probe_status, message) = classify(status, &body);
                Self::probe(
                    id,
                    label,
                    group,
                    weight,
                    probe_status,
                    Some(latency_ms),
                    message,
                    Some(body),
                )
            }
            Err(error) => Self::probe(
                id,
                label,
                group,
                weight,
                ProbeStatus::Failed,
                None,
                error.to_string(),
                None,
            ),
        }
    }

    fn probe(
        id: &str,
        label: &str,
        group: &str,
        weight: u8,
        status: ProbeStatus,
        latency_ms: Option<u64>,
        message: String,
        excerpt: Option<String>,
    ) -> ModelVerifyProbe {
        ModelVerifyProbe {
            id: id.to_string(),
            label: label.to_string(),
            group: group.to_string(),
            weight,
            status,
            latency_ms,
            message,
            excerpt: excerpt.map(|value| Self::sanitize_excerpt(&value)),
        }
    }

    fn openai_chat_url(base_url: &str) -> String {
        let trimmed = base_url.trim().trim_end_matches('/');
        if trimmed.ends_with("/chat/completions") {
            trimmed.to_string()
        } else if trimmed.ends_with("/v1") {
            format!("{trimmed}/chat/completions")
        } else {
            format!("{trimmed}/v1/chat/completions")
        }
    }

    fn anthropic_messages_url(base_url: &str) -> String {
        let trimmed = base_url.trim().trim_end_matches('/');
        if trimmed.ends_with("/messages") {
            trimmed.to_string()
        } else if trimmed.ends_with("/v1") {
            format!("{trimmed}/messages")
        } else {
            format!("{trimmed}/v1/messages")
        }
    }

    fn gemini_generate_url(base_url: &str, model: &str) -> String {
        let trimmed = base_url.trim().trim_end_matches('/');
        if trimmed.contains(":generateContent") {
            trimmed.to_string()
        } else if trimmed.ends_with("/v1beta") || trimmed.ends_with("/v1") {
            format!("{trimmed}/models/{model}:generateContent")
        } else {
            format!("{trimmed}/v1beta/models/{model}:generateContent")
        }
    }

    fn sanitize_excerpt(input: &str) -> String {
        let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
        let redacted = compact
            .split(' ')
            .map(|part| {
                if part.starts_with("sk-")
                    || part.starts_with("sk-ant-")
                    || part.starts_with("AIza")
                    || part.eq_ignore_ascii_case("bearer")
                {
                    "[redacted]"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        redacted.chars().take(500).collect()
    }

    pub fn score_result(
        request: ModelVerifyRequest,
        probes: Vec<ModelVerifyProbe>,
    ) -> ModelVerifyResult {
        let groups = Self::build_probe_groups(&probes);
        let metrics = Self::build_metrics(&probes);
        let scores = ModelVerifyScores {
            knowledge_qa_score: Self::score_group(&probes, "knowledgeQa", 25),
            model_feature_score: Self::score_group(&probes, "modelFeatures", 25),
            protocol_consistency_score: Self::score_group(&probes, "protocolConsistency", 25),
            response_structure_score: Self::score_group(&probes, "responseStructure", 25),
        };
        let mut overall_confidence = scores.knowledge_qa_score
            + scores.model_feature_score
            + scores.protocol_consistency_score
            + scores.response_structure_score;

        let protocol_failed = probes
            .iter()
            .any(|probe| probe.group == "protocolConsistency" && probe.status == ProbeStatus::Failed);
        if protocol_failed {
            overall_confidence = overall_confidence.min(40);
        }

        let dilution_risk = 100u8.saturating_sub(overall_confidence).min(95);
        let success = !protocol_failed && probes.iter().any(|probe| probe.status == ProbeStatus::Passed);
        let evidence_level = if overall_confidence >= 85 && scores.protocol_consistency_score >= 20 {
            EvidenceLevel::Strong
        } else if overall_confidence >= 60 {
            EvidenceLevel::Medium
        } else {
            EvidenceLevel::Weak
        };
        let summary = match evidence_level {
            EvidenceLevel::Strong => "知识问答、型号特征、协议一致性和响应结构整体通过；这是较强黑盒证据，但仍不是后端身份的确定证明。",
            EvidenceLevel::Medium => "部分主探针一致，但仍存在转发、混路由或能力不匹配风险。",
            EvidenceLevel::Weak => "主探针证据较弱或协议一致性失败，不能判断真实模型。",
        }
        .to_string();
        let diagnostics: Vec<ModelVerifyProbe> = probes
            .iter()
            .filter(|probe| probe.group == "errorShape" || probe.group == "identity")
            .cloned()
            .collect();

        ModelVerifyResult {
            success,
            tested_at: chrono::Utc::now().timestamp(),
            model_requested: request.model,
            protocol: request.protocol,
            confidence_score: overall_confidence,
            mismatch_risk: dilution_risk,
            overall_confidence,
            dilution_risk,
            evidence_level,
            scores,
            metrics,
            summary,
            total_latency_ms: None,
            probes,
            probe_groups: groups,
            diagnostics,
        }
    }

    fn score_group(probes: &[ModelVerifyProbe], group: &str, max_score: u8) -> u8 {
        let group_probes: Vec<&ModelVerifyProbe> =
            probes.iter().filter(|probe| probe.group == group).collect();
        if group_probes.is_empty() {
            return 0;
        }
        let total_weight: u16 = group_probes.iter().map(|probe| probe.weight as u16).sum();
        if total_weight == 0 {
            return 0;
        }
        let earned: u16 = group_probes
            .iter()
            .map(|probe| match probe.status {
                ProbeStatus::Passed => probe.weight as u16,
                ProbeStatus::Warning => probe.weight as u16 / 2,
                ProbeStatus::Failed => 0,
            })
            .sum();
        ((earned * max_score as u16) / total_weight).min(max_score as u16) as u8
    }

    fn build_probe_groups(probes: &[ModelVerifyProbe]) -> Vec<ModelVerifyProbeGroup> {
        let definitions = [
            ("knowledgeQa", "知识问答校验", 25),
            ("modelFeatures", "型号特征校验", 25),
            ("protocolConsistency", "协议一致性", 25),
            ("responseStructure", "响应结构", 25),
        ];

        definitions
            .iter()
            .filter_map(|(id, label, max_score)| {
                let group_probes: Vec<ModelVerifyProbe> = probes
                    .iter()
                    .filter(|probe| probe.group == *id)
                    .cloned()
                    .collect();
                if group_probes.is_empty() {
                    None
                } else {
                    Some(ModelVerifyProbeGroup {
                        id: (*id).to_string(),
                        label: (*label).to_string(),
                        score: Self::score_group(probes, id, *max_score),
                        max_score: *max_score,
                        probes: group_probes,
                    })
                }
            })
            .collect()
    }

    fn build_metrics(probes: &[ModelVerifyProbe]) -> ModelVerifyMetrics {
        let latency_ms = probes.iter().filter_map(|probe| probe.latency_ms).max();
        let latency_seconds = latency_ms.map(|value| value as f64 / 1000.0);
        let mut metrics = ModelVerifyMetrics {
            latency_ms,
            latency_seconds,
            ..ModelVerifyMetrics::default()
        };

        for probe in probes {
            let Some(excerpt) = &probe.excerpt else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(excerpt) else {
                continue;
            };
            if let Some(usage) = value.get("usage") {
                metrics.input_tokens = metrics.input_tokens.or_else(|| {
                    usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .or_else(|| usage.get("input_tokens").and_then(Value::as_u64))
                });
                metrics.output_tokens = metrics.output_tokens.or_else(|| {
                    usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .or_else(|| usage.get("output_tokens").and_then(Value::as_u64))
                });
                metrics.cached_input_tokens = metrics.cached_input_tokens.or_else(|| {
                    usage
                        .pointer("/prompt_tokens_details/cached_tokens")
                        .and_then(Value::as_u64)
                        .or_else(|| usage.get("cache_read_input_tokens").and_then(Value::as_u64))
                });
            }
            if let Some(usage) = value.get("usageMetadata") {
                metrics.input_tokens = metrics
                    .input_tokens
                    .or_else(|| usage.get("promptTokenCount").and_then(Value::as_u64));
                metrics.output_tokens = metrics
                    .output_tokens
                    .or_else(|| usage.get("candidatesTokenCount").and_then(Value::as_u64));
                metrics.cached_input_tokens = metrics
                    .cached_input_tokens
                    .or_else(|| usage.get("cachedContentTokenCount").and_then(Value::as_u64));
            }
        }

        if let (Some(output_tokens), Some(seconds)) = (metrics.output_tokens, metrics.latency_seconds) {
            if seconds > 0.0 {
                metrics.tokens_per_second = Some(((output_tokens as f64 / seconds) * 10.0).round() / 10.0);
            }
        }

        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ModelVerifyRequest {
        ModelVerifyRequest {
            protocol: ModelVerifyProtocol::OpenAiChat,
            base_url: "https://api.example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-test".to_string(),
            organization: None,
            api_version: None,
            timeout_secs: 30,
        }
    }

    fn grouped_probe(group: &str, status: ProbeStatus, weight: u8) -> ModelVerifyProbe {
        ModelVerifyProbe {
            id: format!("{group}-probe"),
            label: "Probe".to_string(),
            group: group.to_string(),
            weight,
            status,
            latency_ms: Some(10),
            message: "ok".to_string(),
            excerpt: None,
        }
    }

    fn usage_probe(group: &str, usage: Value, latency_ms: u64) -> ModelVerifyProbe {
        ModelVerifyProbe {
            id: format!("{group}-usage"),
            label: "Usage".to_string(),
            group: group.to_string(),
            weight: 1,
            status: ProbeStatus::Passed,
            latency_ms: Some(latency_ms),
            message: "usage".to_string(),
            excerpt: Some(usage.to_string()),
        }
    }

    #[test]
    fn score_is_high_when_reference_groups_pass() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Passed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Passed, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Passed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
            ],
        );
        assert!(result.success);
        assert!(result.confidence_score >= 90);
        assert!(result.mismatch_risk <= 10);
    }

    #[test]
    fn score_is_medium_when_reference_group_warns() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Passed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Warning, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Passed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
            ],
        );
        assert!(result.success);
        assert!(result.confidence_score < 90);
        assert!(result.mismatch_risk > 10);
    }

    #[test]
    fn score_is_low_when_probe_fails() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Failed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Warning, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Passed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
            ],
        );
        assert!(result.success);
        assert!(result.confidence_score < 80);
        assert!(result.mismatch_risk > 20);
    }

    #[test]
    fn supports_all_planned_protocols() {
        assert_eq!(
            serde_json::to_string(&ModelVerifyProtocol::OpenAiChat).unwrap(),
            "\"openAiChat\""
        );
        assert_eq!(
            serde_json::to_string(&ModelVerifyProtocol::AnthropicMessages).unwrap(),
            "\"anthropicMessages\""
        );
        assert_eq!(
            serde_json::to_string(&ModelVerifyProtocol::GeminiGenerateContent).unwrap(),
            "\"geminiGenerateContent\""
        );
    }

    #[test]
    fn protocol_consistency_failure_caps_overall_confidence() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Passed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Passed, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Failed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
            ],
        );

        assert!(result.overall_confidence <= 40);
        assert!(result.dilution_risk >= 60);
        assert_eq!(result.evidence_level, EvidenceLevel::Weak);
    }

    #[test]
    fn diagnostics_do_not_affect_main_score() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Passed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Passed, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Passed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
                grouped_probe("errorShape", ProbeStatus::Warning, 25),
                grouped_probe("identity", ProbeStatus::Warning, 10),
            ],
        );

        assert_eq!(result.scores.knowledge_qa_score, 25);
        assert_eq!(result.scores.model_feature_score, 25);
        assert_eq!(result.scores.protocol_consistency_score, 25);
        assert_eq!(result.scores.response_structure_score, 25);
        assert_eq!(result.overall_confidence, 100);
        assert_eq!(result.probe_groups.len(), 4);
        assert_eq!(result.diagnostics.len(), 2);
    }

    #[test]
    fn parses_openai_usage_metrics_and_tokens_per_second() {
        let result = ModelVerifyService::score_result(
            request(),
            vec![
                grouped_probe("knowledgeQa", ProbeStatus::Passed, 25),
                grouped_probe("modelFeatures", ProbeStatus::Passed, 25),
                grouped_probe("protocolConsistency", ProbeStatus::Passed, 25),
                grouped_probe("responseStructure", ProbeStatus::Passed, 25),
                usage_probe(
                    "responseStructure",
                    json!({
                        "usage": {
                            "prompt_tokens": 100,
                            "completion_tokens": 50,
                            "prompt_tokens_details": { "cached_tokens": 20 }
                        }
                    }),
                    2_000,
                ),
            ],
        );

        assert_eq!(result.metrics.input_tokens, Some(100));
        assert_eq!(result.metrics.output_tokens, Some(50));
        assert_eq!(result.metrics.cached_input_tokens, Some(20));
        assert_eq!(result.metrics.tokens_per_second, Some(25.0));
    }

    #[test]
    fn sanitize_excerpt_redacts_api_key_like_values() {
        let excerpt = ModelVerifyService::sanitize_excerpt(
            "Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz1234567890 token=abc",
        );

        assert!(!excerpt.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(excerpt.contains("[redacted]"));
    }
}
