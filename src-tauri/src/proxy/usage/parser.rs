//! Response Parser - 从 API 响应中提取 token 使用量
//!
//! 支持多种 API 格式：
//! - Claude API (非流式和流式)
//! - OpenRouter (OpenAI 格式)
//! - Codex API (非流式和流式)
//! - Gemini API (非流式和流式)

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn openai_cache_read_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn openai_cache_write_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

/// Session 日志 request_id 前缀，与 `session_usage.rs` 中的格式保持一致
pub const SESSION_REQUEST_ID_PREFIX: &str = "session:";

fn response_id(body: &Value, field: &str) -> Option<String> {
    body.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Token 使用量统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// 从响应中提取的实际模型名称（如果可用）
    pub model: Option<String>,
    /// 从响应中提取的消息 ID（用于跨源去重）
    ///
    /// Claude API: `msg_xxx`，与 session JSONL 中的 `message.id` 一致
    #[serde(skip)]
    pub message_id: Option<String>,
}

impl TokenUsage {
    /// 生成稳定 request_id。Claude 不加作用域，以便继续与 session JSONL 的
    /// `session:{message_id}` 主键收敛；其他协议加入 app/provider 作用域，避免
    /// 不同上游复用 envelope id 时互相覆盖。
    pub fn dedup_request_id(&self, scope: Option<(&str, &str)>) -> String {
        self.message_id
            .as_ref()
            .map(|message_id| match scope {
                Some((app_type, provider_id)) => {
                    format!("{SESSION_REQUEST_ID_PREFIX}{app_type}:{provider_id}:{message_id}")
                }
                None => format!("{SESSION_REQUEST_ID_PREFIX}{message_id}"),
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }

    /// 是否产生了任一计费维度的 token。
    ///
    /// 用于在写入前过滤全 0 的空 usage：当 OpenAI 兼容上游在流式下省略 usage 时，
    /// 转换器会合成一个全 0 的终止事件，若无 message_id 则 `dedup_request_id`
    /// 退化为随机 UUID，导致每笔请求插入一条无意义的空行、虚增请求数。
    pub fn has_billable_tokens(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
    }
}

/// API 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ApiType {
    Claude,
    OpenRouter,
    Codex,
    Gemini,
}

impl TokenUsage {
    /// 从 Claude API 非流式响应解析
    pub fn from_claude_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let message_id = response_id(body, "id");

        Some(Self {
            input_tokens: usage.get("input_tokens")?.as_u64()? as u32,
            output_tokens: usage.get("output_tokens")?.as_u64()? as u32,
            cache_read_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            model,
            message_id,
        })
    }

    /// 从 Claude API 流式响应解析
    #[allow(dead_code)]
    pub fn from_claude_stream_events(events: &[Value]) -> Option<Self> {
        let mut usage = Self::default();
        let mut model: Option<String> = None;
        let mut message_id: Option<String> = None;
        let mut input_from_delta = false;

        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                match event_type {
                    "message_start" => {
                        if let Some(message) = event.get("message") {
                            if model.is_none() {
                                if let Some(m) = message.get("model").and_then(|v| v.as_str()) {
                                    model = Some(m.to_string());
                                }
                            }
                            if message_id.is_none() {
                                if let Some(id) = response_id(message, "id") {
                                    message_id = Some(id);
                                }
                            }
                        }
                        if let Some(msg_usage) = event.get("message").and_then(|m| m.get("usage")) {
                            // 从 message_start 获取 input_tokens（原生 Claude API）
                            if let Some(input) =
                                msg_usage.get("input_tokens").and_then(|v| v.as_u64())
                            {
                                usage.input_tokens = input as u32;
                            }
                            usage.cache_read_tokens = msg_usage
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as u32;
                            usage.cache_creation_tokens = msg_usage
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as u32;
                        }
                    }
                    "message_delta" => {
                        if let Some(delta_usage) = event.get("usage") {
                            // 从 message_delta 获取 output_tokens
                            if let Some(output) =
                                delta_usage.get("output_tokens").and_then(|v| v.as_u64())
                            {
                                usage.output_tokens = output as u32;
                            }

                            let delta_input = delta_usage
                                .get("input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            let delta_cache_read = delta_usage
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            let delta_cache_creation = delta_usage
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);

                            // 部分 Anthropic-compatible SSE provider 会在 message_start 上报完整上下文，
                            // 但在 message_delta 上报修正后的 fresh input。遇到更小的正数 delta input
                            // 时采用 delta；若同一 usage 块带有缓存计数，也同步采用以避免重复计数。
                            // 若 delta 缺少缓存字段，则保留 start 中已有的缓存值作为 best-effort fallback。
                            if let Some(input) = delta_input {
                                let should_use_delta_input = input > 0
                                    && (usage.input_tokens == 0
                                        || input < usage.input_tokens
                                        || (input_from_delta && input <= usage.input_tokens));

                                if should_use_delta_input {
                                    usage.input_tokens = input;
                                    input_from_delta = true;
                                    if let Some(cache_read) = delta_cache_read {
                                        usage.cache_read_tokens = cache_read;
                                    }
                                    if let Some(cache_creation) = delta_cache_creation {
                                        usage.cache_creation_tokens = cache_creation;
                                    }
                                }
                            }
                            // 从 message_delta 中处理缓存命中(cache_read_input_tokens)
                            if usage.cache_read_tokens == 0 {
                                if let Some(cache_read) = delta_cache_read {
                                    usage.cache_read_tokens = cache_read;
                                }
                            }
                            // 从 message_delta 中处理缓存创建(cache_creation_input_tokens)
                            // 注: 现在 zhipu 没有返回 cache_creation_input_tokens 字段
                            if usage.cache_creation_tokens == 0 {
                                if let Some(cache_creation) = delta_cache_creation {
                                    usage.cache_creation_tokens = cache_creation;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 用 has_billable_tokens 而非仅看 input/output：完全缓存命中、无输出的流式请求
        // （input==0 && output==0 但 cache_read>0）是真实的 cache-read 计费，必须保留。
        // Gemini→Anthropic 路径在 input 改为 fresh(promptTokenCount - cachedContentTokenCount)
        // 后尤其会出现这种全缓存场景；旧 gate 会把它当成"无 usage"丢弃。
        if usage.has_billable_tokens() {
            usage.model = model;
            usage.message_id = message_id;
            Some(usage)
        } else {
            None
        }
    }

    /// 从 OpenRouter 响应解析 (OpenAI 格式)
    #[allow(dead_code)]
    pub fn from_openrouter_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        Some(Self {
            input_tokens: usage.get("prompt_tokens")?.as_u64()? as u32,
            output_tokens: usage.get("completion_tokens")?.as_u64()? as u32,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: response_id(body, "id"),
        })
    }

    /// 从 Codex API 非流式响应解析
    pub fn from_codex_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage");
        if usage.is_none() {
            log::debug!(
                "[Codex] 响应中没有 usage 字段，body keys: {:?}",
                body.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            return None;
        }
        let usage = usage?;

        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64());
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64());

        if input_tokens.is_none() || output_tokens.is_none() {
            log::debug!("[Codex] usage 字段缺少 input_tokens 或 output_tokens，usage: {usage:?}");
            return None;
        }

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cached_tokens = openai_cache_read_tokens(usage);
        let cache_write_tokens = openai_cache_write_tokens(usage);

        Some(Self {
            input_tokens: input_tokens? as u32,
            output_tokens: output_tokens? as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: cache_write_tokens,
            model,
            message_id: response_id(body, "id"),
        })
    }

    /// 从 Codex API 响应解析并调整 input_tokens
    ///
    /// Codex 的 input_tokens 需要减去 cached_tokens 以获得实际计费的 token 数
    /// 公式: adjusted_input = max(input_tokens - cached_tokens, 0)
    #[allow(dead_code)]
    pub fn from_codex_response_adjusted(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        let input_tokens = usage.get("input_tokens")?.as_u64()? as u32;
        let output_tokens = usage.get("output_tokens")?.as_u64()? as u32;

        // 获取 cached_tokens (可能在 cache_read_input_tokens 或 input_tokens_details 中)
        let cached_tokens = openai_cache_read_tokens(usage);
        let cache_write_tokens = openai_cache_write_tokens(usage);

        // 调整 input_tokens: OpenAI total input 同时包含 cache read/write 两桶。
        let adjusted_input = input_tokens
            .saturating_sub(cached_tokens)
            .saturating_sub(cache_write_tokens);

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: adjusted_input,
            output_tokens,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: cache_write_tokens,
            model,
            message_id: response_id(body, "id"),
        })
    }

    /// 从 Codex API 流式响应解析
    #[allow(dead_code)]
    pub fn from_codex_stream_events(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 解析流式事件，共 {} 个事件", events.len());
        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                log::debug!("[Codex] 事件类型: {event_type}");
                if event_type == "response.completed" {
                    if let Some(response) = event.get("response") {
                        log::debug!("[Codex] 找到 response.completed 事件，解析 usage");
                        return Self::from_codex_response_adjusted(response);
                    }
                }
            }
        }
        log::debug!("[Codex] 未找到 response.completed 事件");
        None
    }

    /// 智能 Codex 响应解析 - 自动检测 OpenAI 或 Codex 格式
    ///
    /// Codex 支持两种 API 格式：
    /// - `/v1/responses`: 使用 input_tokens/output_tokens
    /// - `/v1/chat/completions`: 使用 prompt_tokens/completion_tokens (OpenAI 格式)
    ///
    /// 注意：记录原始 input_tokens，费用计算时再减去 cached_tokens
    pub fn from_codex_response_auto(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // 检测格式：OpenAI 使用 prompt_tokens，Codex 使用 input_tokens
        if usage.get("prompt_tokens").is_some() {
            log::debug!("[Codex] 检测到 OpenAI 格式 (prompt_tokens)");
            Self::from_openai_response(body)
        } else if usage.get("input_tokens").is_some() {
            log::debug!("[Codex] 检测到 Codex 格式 (input_tokens)");
            // 使用非调整版本，记录原始 input_tokens
            Self::from_codex_response(body)
        } else {
            log::debug!("[Codex] 无法识别响应格式，usage: {usage:?}");
            None
        }
    }

    /// 智能 Codex 流式响应解析 - 自动检测 OpenAI 或 Codex 格式
    pub fn from_codex_stream_events_auto(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 智能解析流式事件，共 {} 个事件", events.len());

        // 先尝试 Codex Responses API 格式 (response.completed 事件)
        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                if event_type == "response.completed" {
                    if let Some(response) = event.get("response") {
                        log::debug!("[Codex] 找到 response.completed 事件");
                        return Self::from_codex_response_auto(response);
                    }
                }
            }
        }

        // 回退到 OpenAI Chat Completions 格式 (最后一个 chunk 包含 usage)
        log::debug!("[Codex] 尝试 OpenAI 流式格式");
        Self::from_openai_stream_events(events)
    }

    /// 从 OpenAI Chat Completions API 响应解析 (prompt_tokens, completion_tokens)
    pub fn from_openai_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // OpenAI 使用 prompt_tokens 和 completion_tokens
        let prompt_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64())?;
        let completion_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64())?;

        // 获取 cached_tokens (可能在 prompt_tokens_details 中)
        let cached_tokens = openai_cache_read_tokens(usage);
        let cache_write_tokens = openai_cache_write_tokens(usage);

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: prompt_tokens as u32,
            output_tokens: completion_tokens as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: cache_write_tokens,
            model,
            message_id: response_id(body, "id"),
        })
    }

    /// 从 OpenAI Chat Completions API 流式响应解析
    pub fn from_openai_stream_events(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 解析 OpenAI 流式事件，共 {} 个事件", events.len());
        // OpenAI 流式响应在最后一个 chunk 中包含 usage
        for event in events.iter().rev() {
            if let Some(usage) = event.get("usage") {
                if !usage.is_null() {
                    log::debug!("[Codex] 找到 usage: {usage:?}");
                    let mut parsed = Self::from_openai_response(event)?;
                    if parsed.message_id.is_none() {
                        parsed.message_id =
                            events.iter().find_map(|chunk| response_id(chunk, "id"));
                    }
                    return Some(parsed);
                }
            }
        }
        log::debug!("[Codex] 未找到 usage 信息");
        None
    }

    /// 从 Gemini API 非流式响应解析
    pub fn from_gemini_response(body: &Value) -> Option<Self> {
        let usage = body.get("usageMetadata")?;
        // 提取实际使用的模型名称（modelVersion 字段）
        let model = body
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let prompt_tokens = usage.get("promptTokenCount")?.as_u64()? as u32;
        let total_tokens = usage.get("totalTokenCount")?.as_u64()? as u32;

        // 输出 tokens = 总 tokens - 输入 tokens
        // 这包含了 candidatesTokenCount + thoughtsTokenCount
        let output_tokens = total_tokens.saturating_sub(prompt_tokens);

        Some(Self {
            input_tokens: prompt_tokens,
            output_tokens,
            cache_read_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: 0,
            model,
            message_id: response_id(body, "responseId"),
        })
    }

    /// 从 Gemini API 流式响应解析
    #[allow(dead_code)]
    pub fn from_gemini_stream_chunks(chunks: &[Value]) -> Option<Self> {
        let mut total_input = 0u32;
        let mut total_tokens = 0u32;
        let mut total_cache_read = 0u32;
        let mut model: Option<String> = None;
        let mut message_id: Option<String> = None;

        for chunk in chunks {
            if let Some(usage) = chunk.get("usageMetadata") {
                // 输入 tokens (通常在所有 chunk 中保持不变)
                total_input = usage
                    .get("promptTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                // 总 tokens (包含输入 + 输出 + 思考)
                total_tokens = usage
                    .get("totalTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                // 缓存读取 tokens
                total_cache_read = usage
                    .get("cachedContentTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }

            // 提取实际使用的模型名称（modelVersion 字段）
            if model.is_none() {
                if let Some(model_version) = chunk.get("modelVersion").and_then(|v| v.as_str()) {
                    model = Some(model_version.to_string());
                }
            }
            if message_id.is_none() {
                message_id = response_id(chunk, "responseId");
            }
        }

        // 输出 tokens = 总 tokens - 输入 tokens
        let total_output = total_tokens.saturating_sub(total_input);

        if total_input > 0 || total_output > 0 {
            Some(Self {
                input_tokens: total_input,
                output_tokens: total_output,
                cache_read_tokens: total_cache_read,
                cache_creation_tokens: 0,
                model,
                message_id,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_ids_produce_scoped_dedup_keys_and_empty_ids_fall_back() {
        let response = json!({
            "id": "resp_123",
            "model": "gpt-5.6",
            "usage": { "input_tokens": 10, "output_tokens": 2 }
        });
        let usage = TokenUsage::from_codex_response(&response).unwrap();
        assert_eq!(usage.message_id.as_deref(), Some("resp_123"));
        assert_eq!(
            usage.dedup_request_id(Some(("codex", "provider-a"))),
            "session:codex:provider-a:resp_123"
        );

        let empty = json!({
            "id": "",
            "usage": { "input_tokens": 10, "output_tokens": 2 }
        });
        let empty_usage = TokenUsage::from_codex_response(&empty).unwrap();
        assert!(empty_usage.message_id.is_none());
        assert!(!empty_usage
            .dedup_request_id(Some(("codex", "provider-a")))
            .starts_with("session:"));
    }

    #[test]
    fn stream_parsers_recover_ids_from_envelope_chunks() {
        let openai = vec![
            json!({"id": "chatcmpl_123", "choices": []}),
            json!({
                "usage": { "prompt_tokens": 10, "completion_tokens": 2 },
                "choices": []
            }),
        ];
        assert_eq!(
            TokenUsage::from_openai_stream_events(&openai)
                .unwrap()
                .message_id
                .as_deref(),
            Some("chatcmpl_123")
        );

        let gemini = vec![json!({
            "responseId": "gemini_123",
            "usageMetadata": { "promptTokenCount": 10, "totalTokenCount": 12 }
        })];
        assert_eq!(
            TokenUsage::from_gemini_stream_chunks(&gemini)
                .unwrap()
                .message_id
                .as_deref(),
            Some("gemini_123")
        );
    }

    #[test]
    fn test_claude_response_parsing() {
        let response = json!({
            "model": "claude-sonnet-4-20250514",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_has_billable_tokens_gates_empty_usage() {
        // 全 0 usage（如上游省略 usage 时合成的全 0 终止事件）不应计费——
        // 这是 Codex 流式空行多记修复（D）的闸门依据。
        assert!(!TokenUsage::default().has_billable_tokens());
        // 仅有 cache_read 也属于真实计费 token，必须计入。
        let only_cache = TokenUsage {
            cache_read_tokens: 100,
            ..Default::default()
        };
        assert!(only_cache.has_billable_tokens());
        let normal = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        };
        assert!(normal.has_billable_tokens());
    }

    #[test]
    fn test_claude_stream_cache_only_request_is_recorded() {
        // P2 回归：完全缓存命中、无输出的流式请求（input==0 && output==0 但 cache_read>0）
        // 是真实计费，必须保留——旧 gate `input>0 || output>0` 会把它丢弃。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_cacheonly",
                    "model": "claude-opus-4-8",
                    "usage": {
                        "input_tokens": 0,
                        "cache_read_input_tokens": 50000,
                        "cache_creation_input_tokens": 0
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": { "output_tokens": 0 }
            }),
        ];
        let usage = TokenUsage::from_claude_stream_events(&events)
            .expect("cache-only 流式请求必须被记录，不能被 input/output gate 丢弃");
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 50000);
        assert_eq!(usage.message_id, Some("msg_cacheonly".to_string()));
    }

    #[test]
    fn test_codex_response_auto_returns_some_for_synthetic_all_zero() {
        // P3 回归：上游非流式 Chat 省略 usage 时转换器合成的全 0 usage，from_codex_response_auto
        // 仍返回 Some（字段存在、无 positivity check）——证明 handlers 必须用 has_billable_tokens
        // 闸门才能挡住空行，单靠 `if let Some` 不够。
        let synthetic = json!({
            "usage": { "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }
        });
        let usage = TokenUsage::from_codex_response_auto(&synthetic)
            .expect("全 0 usage 字段存在时 from_codex_response_auto 返回 Some");
        assert!(
            !usage.has_billable_tokens(),
            "全 0 usage 必须被 has_billable_tokens 判为非计费，由 handlers 闸门跳过"
        );
    }

    #[test]
    fn test_claude_response_parsing_no_model() {
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_claude_stream_parsing() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_stream_parsing_no_model() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_openrouter_response_parsing() {
        let response = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50
            }
        });

        let usage = TokenUsage::from_openrouter_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
    }

    #[test]
    fn test_gemini_response_parsing() {
        let response = json!({
            "modelVersion": "gemini-3-pro-high",
            "usageMetadata": {
                "promptTokenCount": 8383,
                "candidatesTokenCount": 50,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount = 8547 - 8383 = 164
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_gemini_response_parsing_no_model() {
        // 测试没有 modelVersion 字段的情况
        let response = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "totalTokenCount": 150,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        // output_tokens = totalTokenCount - promptTokenCount = 150 - 100 = 50
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_gemini_response_with_thoughts() {
        // 测试包含 thoughtsTokenCount 的实际响应
        // 这是用户报告的真实场景
        let response = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "",
                                "thoughtSignature": "EvcECvQE..."
                            }
                        ],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }
            ],
            "modelVersion": "gemini-3-pro-high",
            "responseId": "yupTafqLDu-PjMcPhrOx4QQ",
            "usageMetadata": {
                "candidatesTokenCount": 50,
                "promptTokenCount": 8383,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount
        // = 8547 - 8383 = 164 (包含 candidatesTokenCount 50 + thoughtsTokenCount 114)
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_codex_response_parsing_cached_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        // 非调整模式：input_tokens 保持原值，但应记录缓存命中
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_parsing_cache_write_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300,
                    "cache_write_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.cache_creation_tokens, 200);

        let adjusted = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        assert_eq!(adjusted.input_tokens, 500);
        assert_eq!(adjusted.cache_read_tokens, 300);
        assert_eq!(adjusted.cache_creation_tokens, 200);
    }

    #[test]
    fn test_codex_response_adjusted() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // input_tokens 应该被调整: 1000 - 300 = 700
        assert_eq!(usage.input_tokens, 700);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_adjusted_no_cache() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // 没有 cached_tokens，input_tokens 保持不变
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 0);
    }

    #[test]
    fn test_codex_response_adjusted_cache_read_input_tokens() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "cache_read_input_tokens": 200
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        assert_eq!(usage.input_tokens, 800);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
    }

    #[test]
    fn test_codex_response_adjusted_saturating_sub() {
        // 测试 cached_tokens > input_tokens 的边界情况
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "input_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // saturating_sub 确保不会下溢
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 200);
    }

    #[test]
    fn test_openrouter_stream_parsing() {
        // 测试 OpenRouter 转换后的流式响应解析
        // OpenRouter 流式响应经过转换后，input_tokens 在 message_delta 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": "end_turn"
                },
                "usage": {
                    "input_tokens": 150,
                    "output_tokens": 75
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 150);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_stream_prefers_smaller_delta_input_and_cache_pair() {
        // 部分 Anthropic-compatible provider 会在 message_start 给出包含缓存的总上下文，
        // 再在 message_delta 给出修正后的 fresh input，需要以 delta usage 为准。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "qwen-max",
                    "usage": {
                        "input_tokens": 200_000,
                        "cache_read_input_tokens": 180_000,
                        "cache_creation_input_tokens": 2_000
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 1_000,
                    "cache_read_input_tokens": 120_000,
                    "cache_creation_input_tokens": 500
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 80_000);
        assert_eq!(usage.output_tokens, 1_000);
        assert_eq!(usage.cache_read_tokens, 120_000);
        assert_eq!(usage.cache_creation_tokens, 500);
        assert_eq!(usage.model, Some("qwen-max".to_string()));
    }

    #[test]
    fn test_claude_stream_updates_cache_pair_from_later_delta_input() {
        // 有些 provider 会多次发送带 input 的 message_delta；一旦采用过 delta input，
        // 后续相同/更小 input 的 delta 应继续更新同一块里的缓存计数。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "qwen-max",
                    "usage": {
                        "input_tokens": 200_000,
                        "cache_read_input_tokens": 180_000,
                        "cache_creation_input_tokens": 2_000
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 100,
                    "cache_read_input_tokens": 110_000,
                    "cache_creation_input_tokens": 300
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 1_000,
                    "cache_read_input_tokens": 120_000,
                    "cache_creation_input_tokens": 500
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 80_000);
        assert_eq!(usage.output_tokens, 1_000);
        assert_eq!(usage.cache_read_tokens, 120_000);
        assert_eq!(usage.cache_creation_tokens, 500);
        assert_eq!(usage.model, Some("qwen-max".to_string()));
    }

    #[test]
    fn test_claude_stream_keeps_start_when_delta_input_is_larger() {
        // 正常 Anthropic 语义下，message_start 的 input_tokens 已经可信；
        // 如果 delta input 变大，不应覆盖 start input/cache。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 150,
                    "output_tokens": 75,
                    "cache_read_input_tokens": 30
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.cache_read_tokens, 20);
    }

    #[test]
    fn test_native_claude_stream_parsing() {
        // 测试原生 Claude API 流式响应解析
        // 原生 Claude API 的 input_tokens 在 message_start 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 200,
                        "cache_read_input_tokens": 50
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 100
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    // ============================================================================
    // 智能 Codex 解析测试
    // ============================================================================

    #[test]
    fn test_codex_response_auto_openai_format() {
        // OpenAI 格式 (prompt_tokens/completion_tokens)
        let response = json!({
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 500,
                "prompt_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_codex_response_auto_codex_format() {
        // Codex 格式 (input_tokens/output_tokens)
        let response = json!({
            "model": "o3",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_codex_format() {
        // Codex Responses API 流式格式 (response.completed 事件)
        let events = vec![
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_123"
                }
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "model": "o3",
                    "usage": {
                        "input_tokens": 1000,
                        "output_tokens": 500,
                        "input_tokens_details": {
                            "cached_tokens": 200
                        }
                    }
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_openai_format() {
        // OpenAI Chat Completions 流式格式 (最后一个 chunk 包含 usage)
        let events = vec![
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {"content": "Hello"}}]
            }),
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {}}],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }
}
