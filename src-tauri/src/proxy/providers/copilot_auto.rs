//! GitHub Copilot Auto (/auto) router client
//!
//! Provides DTOs, CLI fingerprint headers, token parsing, prompt summary extraction,
//! and resolution client for GitHub Copilot Auto model routing.

use crate::proxy::error::ProxyError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

pub const COPILOT_AUTO_CLI_USER_AGENT: &str =
    "copilot/1.0.107 (linux v24.22.1) term/unknown client/github/cli";
pub const COPILOT_AUTO_CLI_EDITOR_VERSION: &str = "copilot/1.0.107";
pub const COPILOT_AUTO_CLI_OPENAI_INTENT: &str = "conversation-agent";
pub const COPILOT_AUTO_CLI_INTEGRATION_ID: &str = "copilot-developer-cli";
pub const COPILOT_AUTO_CLI_HARNESS_ID: &str = "copilot-sdk";
pub const COPILOT_AUTO_API_VERSION: &str = "2026-08-01";
pub const COPILOT_AUTO_VIRTUAL_MODEL_ID: &str = "copilot-fusion-auto";
pub const COPILOT_AUTO_VIRTUAL_DISPLAY_NAME: &str = "GitHub Copilot Auto";

/// Supported request protocols for prompt extraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestProtocol {
    AnthropicMessages,
    CodexResponses,
    ChatCompletions,
}

/// DTO for POST /auto resolution request
#[derive(Serialize)]
struct CopilotAutoRequest<'a> {
    prompt: &'a str,
    has_image: bool,
}

/// Selected model payload supporting both object form `{"id": "...", ...}` and string `"..."`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SelectedModelPayload {
    Object { id: String },
    String(String),
}

impl SelectedModelPayload {
    pub fn model_id(&self) -> &str {
        match self {
            Self::Object { id } => id,
            Self::String(s) => s.as_str(),
        }
    }
}

/// DTO for POST /auto resolution response
#[derive(Deserialize)]
pub struct CopilotAutoResponse {
    pub session_token: String,
    pub selected_model: SelectedModelPayload,
    /// 顶层授权期限。服务端字段合同尚未完全验证，保留原始值，
    /// 由 session 侧按已知形态（unix 秒 / RFC 3339）解析。
    #[serde(default)]
    pub expires_at: Option<serde_json::Value>,
}

/// 顶层 `expires_at` → unix 秒。缺失（或 null）返回 `Ok(None)`；存在但无法解析
/// 返回 `Err`：期限不可证时不能虚构默认值，由调用方决定是否拒绝该决议。
pub fn parse_expires_at_field(raw: Option<&serde_json::Value>) -> Result<Option<i64>, String> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if let Some(secs) = raw.as_i64().or_else(|| raw.as_f64().map(|f| f as i64)) {
        return Ok(Some(normalize_expires_at_secs(secs)));
    }
    if let Some(text) = raw.as_str() {
        let text = text.trim();
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(text) {
            return Ok(Some(dt.timestamp()));
        }
        if let Ok(secs) = text.parse::<i64>() {
            return Ok(Some(normalize_expires_at_secs(secs)));
        }
        return Err("expires_at 不是可识别的时间格式".to_string());
    }
    Err("expires_at 类型不受支持".to_string())
}

/// 秒级时间戳到 2286 年约 8.6e10；明显更大的整数按毫秒解释。
fn normalize_expires_at_secs(secs: i64) -> i64 {
    if secs > 99_999_999_999 {
        secs / 1000
    } else {
        secs
    }
}

// Redact session_token in Debug representations
impl std::fmt::Debug for CopilotAutoResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopilotAutoResponse")
            .field("session_token", &"[REDACTED]")
            .field("selected_model", &self.selected_model)
            .finish()
    }
}

/// JWT payload decoded from session_token
#[derive(Debug, Clone)]
pub struct ParsedSessionToken {
    pub exp: Option<u64>,
}

impl ParsedSessionToken {
    pub fn parse(token: &str) -> Option<Self> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        let payload_b64 = parts[1].trim_end_matches('=');
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .or_else(|_| {
                use base64::engine::general_purpose::STANDARD_NO_PAD;
                STANDARD_NO_PAD.decode(payload_b64)
            })
            .ok()?;

        let json_val: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
        let exp = json_val
            .get("exp")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
        Some(Self { exp })
    }
}

fn append_text_block(blocks: &mut Vec<String>, text: &str) {
    let text = strip_system_reminders(text);
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(trimmed.to_string());
    }
}

/// Claude Code 把 CLAUDE.md、记忆、git 状态等上下文包在 `<system-reminder>` 里放进用户消息，
/// 首轮常常排在任务前面、超过摘要预算。它们不是用户的任务：去掉后再拼摘要，免得 /auto
/// 只看到这些样板。没有闭合标签的片段保持原样。
fn strip_system_reminders(text: &str) -> std::borrow::Cow<'_, str> {
    const OPEN: &str = "<system-reminder>";
    const CLOSE: &str = "</system-reminder>";
    if !text.contains(OPEN) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let Some(close) = rest[start..].find(CLOSE) else {
            break;
        };
        kept.push_str(&rest[..start]);
        rest = &rest[start + close + CLOSE.len()..];
    }
    kept.push_str(rest);
    std::borrow::Cow::Owned(kept)
}

fn truncate_to_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// Assembles prompt messages under `max_bytes` budget, prioritizing the latest user task.
/// If history exceeds budget, the latest message is fully preserved while preceding messages
/// fill the remaining budget from newest to oldest (preserving chronological order when output).
fn assemble_prompt_with_budget(messages: Vec<String>, max_bytes: usize) -> String {
    if messages.is_empty() {
        return "Assist with coding task".to_string();
    }
    let total_len: usize =
        messages.iter().map(|p| p.len()).sum::<usize>() + messages.len().saturating_sub(1);
    if total_len <= max_bytes {
        return messages.join("\n");
    }

    let mut messages = messages;
    let mut latest = messages.pop().unwrap_or_default();
    if latest.len() >= max_bytes {
        truncate_to_char_boundary(&mut latest, max_bytes);
        return latest;
    }

    let remaining_budget = max_bytes.saturating_sub(latest.len());
    let mut selected_earlier: Vec<String> = Vec::new();
    let mut remaining = remaining_budget;

    for message in messages.into_iter().rev() {
        let needed = message.len() + 1;
        if remaining >= needed {
            remaining -= needed;
            selected_earlier.push(message);
        } else if remaining > 32 {
            let avail = remaining.saturating_sub(1);
            let mut slice = message;
            if slice.len() > avail {
                let start = slice.len().saturating_sub(avail);
                let mut valid_start = start;
                while valid_start < slice.len() && !slice.is_char_boundary(valid_start) {
                    valid_start += 1;
                }
                slice = slice[valid_start..].to_string();
            }
            if !slice.is_empty() {
                selected_earlier.push(slice);
            }
            break;
        } else {
            break;
        }
    }

    selected_earlier.reverse();
    selected_earlier.push(latest);
    selected_earlier.join("\n")
}

/// Extract prompt text and image presence across Anthropic Messages, Codex Responses,
/// and Chat Completions protocols. Safely bounded to 4000 bytes on valid UTF-8 boundaries,
/// prioritizing the most recent user turn.
///
/// Prompts are grouped by user message before budget allocation so that multi-part content
/// (e.g. an instruction followed by a large code block) preserves the user's task instruction.
pub fn extract_prompt_summary_for_auto(
    request_body: &serde_json::Value,
    protocol: RequestProtocol,
) -> (String, bool) {
    let mut user_messages: Vec<String> = Vec::new();
    let mut has_image = false;

    match protocol {
        RequestProtocol::AnthropicMessages => {
            if let Some(messages) = request_body.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    if role == "user" {
                        let mut msg_blocks: Vec<String> = Vec::new();
                        if let Some(content) = msg.get("content") {
                            if let Some(text) = content.as_str() {
                                append_text_block(&mut msg_blocks, text);
                            } else if let Some(msg_parts) = content.as_array() {
                                for part in msg_parts {
                                    let part_type =
                                        part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    if part_type == "image" {
                                        has_image = true;
                                    } else if part_type == "text" {
                                        if let Some(text) =
                                            part.get("text").and_then(|v| v.as_str())
                                        {
                                            append_text_block(&mut msg_blocks, text);
                                        }
                                    }
                                }
                            }
                        }
                        if !msg_blocks.is_empty() {
                            user_messages.push(msg_blocks.join("\n"));
                        }
                    }
                }
            }
        }
        RequestProtocol::CodexResponses => {
            if let Some(input_val) = request_body.get("input") {
                if let Some(text) = input_val.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        user_messages.push(trimmed.to_string());
                    }
                } else if let Some(input) = input_val.as_array() {
                    let mut top_level_blocks: Vec<String> = Vec::new();
                    for item in input {
                        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if item_type == "input_text" || item_type == "text" {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                append_text_block(&mut top_level_blocks, text);
                            }
                        } else if item_type == "input_image"
                            || item_type == "image_url"
                            || item_type == "image"
                        {
                            has_image = true;
                        } else {
                            if !top_level_blocks.is_empty() {
                                user_messages.push(top_level_blocks.join("\n"));
                                top_level_blocks.clear();
                            }
                            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
                            if role == "user" || (role.is_empty() && item_type == "message") {
                                let mut msg_blocks: Vec<String> = Vec::new();
                                if let Some(content) = item.get("content") {
                                    if let Some(text) = content.as_str() {
                                        append_text_block(&mut msg_blocks, text);
                                    } else if let Some(item_parts) = content.as_array() {
                                        for part in item_parts {
                                            let part_type = part
                                                .get("type")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            if part_type == "input_image"
                                                || part_type == "image_url"
                                                || part_type == "image"
                                            {
                                                has_image = true;
                                            } else if part_type == "input_text"
                                                || part_type == "text"
                                            {
                                                if let Some(text) =
                                                    part.get("text").and_then(|v| v.as_str())
                                                {
                                                    append_text_block(&mut msg_blocks, text);
                                                }
                                            }
                                        }
                                    }
                                }
                                if !msg_blocks.is_empty() {
                                    user_messages.push(msg_blocks.join("\n"));
                                }
                            }
                        }
                    }
                    if !top_level_blocks.is_empty() {
                        user_messages.push(top_level_blocks.join("\n"));
                    }
                }
            }
        }
        RequestProtocol::ChatCompletions => {
            if let Some(messages) = request_body.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    if role == "user" {
                        let mut msg_blocks: Vec<String> = Vec::new();
                        if let Some(content) = msg.get("content") {
                            if let Some(text) = content.as_str() {
                                append_text_block(&mut msg_blocks, text);
                            } else if let Some(msg_parts) = content.as_array() {
                                for part in msg_parts {
                                    let part_type =
                                        part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    if part_type == "image_url" {
                                        has_image = true;
                                    } else if part_type == "text" {
                                        if let Some(text) =
                                            part.get("text").and_then(|v| v.as_str())
                                        {
                                            append_text_block(&mut msg_blocks, text);
                                        }
                                    }
                                }
                            }
                        }
                        if !msg_blocks.is_empty() {
                            user_messages.push(msg_blocks.join("\n"));
                        }
                    }
                }
            }
        }
    }

    let prompt = assemble_prompt_with_budget(user_messages, 4000);
    (prompt, has_image)
}

/// Client for calling POST /auto
pub struct CopilotAutoClient;

/// reqwest 自带的超时和 `get_or_resolve` 外层的 tokio 超时等长：谁先到都按超时报，
/// 不能因为计时器先后变成「连不上 /auto」或「响应解析失败」。
fn auto_timeout_or(
    error: reqwest::Error,
    other: impl FnOnce(reqwest::Error) -> ProxyError,
) -> ProxyError {
    if error.is_timeout() {
        ProxyError::Timeout(format!("Copilot /auto request timed out: {error}"))
    } else {
        other(error)
    }
}

impl CopilotAutoClient {
    pub async fn resolve(
        client: &reqwest::Client,
        api_base: &str,
        copilot_token: &str,
        prompt: &str,
        has_image: bool,
        timeout: std::time::Duration,
    ) -> Result<CopilotAutoResponse, ProxyError> {
        let clean_base = api_base.trim_end_matches('/');
        let url = format!("{clean_base}/auto");
        let interaction_id = uuid::Uuid::new_v4().to_string();

        let request_body = CopilotAutoRequest { prompt, has_image };

        let response = client
            .post(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {copilot_token}"),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, COPILOT_AUTO_CLI_USER_AGENT)
            .header("Editor-Version", COPILOT_AUTO_CLI_EDITOR_VERSION)
            .header("Openai-Intent", COPILOT_AUTO_CLI_OPENAI_INTENT)
            .header("Copilot-Integration-Id", COPILOT_AUTO_CLI_INTEGRATION_ID)
            .header("Copilot-Harness-Id", COPILOT_AUTO_CLI_HARNESS_ID)
            .header("X-Github-Api-Version", COPILOT_AUTO_API_VERSION)
            .header("X-Initiator", "user")
            .header("X-Interaction-Id", interaction_id)
            .timeout(timeout)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                auto_timeout_or(e, |e| {
                    ProxyError::ForwardFailed(format!("Failed to reach Copilot /auto: {e}"))
                })
            })?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(ProxyError::UpstreamError {
                status: status.as_u16(),
                body: Some(err_body),
            });
        }

        let parsed: CopilotAutoResponse = response.json().await.map_err(|e| {
            auto_timeout_or(e, |e| {
                ProxyError::TransformError(format!(
                    "Failed to parse Copilot /auto response JSON: {e}"
                ))
            })
        })?;

        if parsed.session_token.trim().is_empty() {
            return Err(ProxyError::TransformError(
                "Copilot /auto response missing session_token".to_string(),
            ));
        }
        if parsed.selected_model.model_id().trim().is_empty() {
            return Err(ProxyError::TransformError(
                "Copilot /auto response missing selected_model ID".to_string(),
            ));
        }

        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deserialize_auto_response_with_model_object() {
        let fixture = json!({
            "session_token": "header.eyJleHAiOjE3OTQxMTY0NTIsImF1dG9fc2Vzc2lvbl9pZCI6IjEyMyIsImF1dG9fdGllciI6ImJhbGFuY2UiLCJleGVjdXRpb25fbW9kZSI6ImZ1c2lvbiJ9.sig",
            "selected_model": {
                "id": "gpt-5.6-luna",
                "name": "Luna",
                "capabilities": {"streaming": true}
            },
            "selection_reason": "scored best for code intent",
        });

        let resp: CopilotAutoResponse = serde_json::from_value(fixture).unwrap();
        assert_eq!(resp.selected_model.model_id(), "gpt-5.6-luna");

        // Test debug does not leak session_token
        let debug_str = format!("{resp:?}");
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("eyJleHAi"));
    }

    #[test]
    fn test_deserialize_auto_response_with_model_string() {
        let fixture = json!({
            "session_token": "token123",
            "selected_model": "gpt-5.6-luna"
        });

        let resp: CopilotAutoResponse = serde_json::from_value(fixture).unwrap();
        assert_eq!(resp.selected_model.model_id(), "gpt-5.6-luna");
    }

    #[test]
    fn test_parsed_session_token() {
        let payload = json!({
            "exp": 1794116452,
            "auto_session_id": "session-xyz",
            "auto_tier": "balance"
        });
        let payload_str = serde_json::to_string(&payload).unwrap();
        let b64 = URL_SAFE_NO_PAD.encode(payload_str.as_bytes());
        let token = format!("eyJhbGciOiJFUzI1NiJ9.{b64}.sig");

        let parsed = ParsedSessionToken::parse(&token).unwrap();
        assert_eq!(parsed.exp, Some(1794116452));
        assert_eq!(parsed.exp, Some(1794116452));
    }

    #[test]
    fn test_extract_prompt_summary_anthropic() {
        let req = json!({
            "model": "claude-3-5-sonnet",
            "messages": [
                {"role": "user", "content": "What is 2+2?"}
            ]
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert_eq!(prompt, "What is 2+2?");
        assert!(!has_image);
    }

    #[test]
    fn test_extract_prompt_summary_responses() {
        let req = json!({
            "model": "auto",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "Fix this bug"}
                    ]
                }
            ]
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert_eq!(prompt, "Fix this bug");
        assert!(!has_image);
    }

    #[test]
    fn test_extract_prompt_summary_chat() {
        let req = json!({
            "model": "auto",
            "messages": [
                {"role": "system", "content": "You are a bot"},
                {"role": "user", "content": [
                    {"type": "text", "text": "Explain this diagram"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                ]}
            ]
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::ChatCompletions);
        assert_eq!(prompt, "Explain this diagram");
        assert!(has_image);
    }

    #[test]
    fn test_utf8_cjk_truncation_does_not_panic() {
        // Chinese character '中' is 3 bytes (0xE4 0xB8 0xAD).
        // 1334 * 3 = 4002 bytes. At 4000 bytes, it cuts in the middle of a character.
        let large_cjk = "中".repeat(1500);
        let req = json!({
            "messages": [
                {"role": "user", "content": large_cjk}
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert!(prompt.len() <= 4000);
        // Valid UTF-8 check
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn test_long_history_preserves_latest_user_task() {
        // Turn 1 is very long (> 4000 bytes)
        let turn1 = "A".repeat(5000);
        let turn2 = "LATEST_USER_QUESTION: please refactor my code";
        let req = json!({
            "messages": [
                {"role": "user", "content": turn1},
                {"role": "assistant", "content": "Done."},
                {"role": "user", "content": turn2}
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert!(
            prompt.ends_with("LATEST_USER_QUESTION: please refactor my code"),
            "prompt should preserve the latest task, but was: {prompt}"
        );
        assert!(prompt.len() <= 4000);
    }

    #[test]
    fn test_parsed_session_token_with_base64_padding() {
        let payload = json!({
            "exp": 1794116452
        });
        // Force base64 with '=' padding
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let mut payload = payload;
        let mut padding = String::new();
        let b64_padded = loop {
            payload["padding"] = json!(padding);
            let encoded = STANDARD.encode(serde_json::to_vec(&payload).unwrap());
            if encoded.contains('=') {
                break encoded;
            }
            padding.push('x');
        };

        let token = format!("eyJhbGciOiJFUzI1NiJ9.{b64_padded}.sig");
        let parsed = ParsedSessionToken::parse(&token).unwrap();
        assert_eq!(parsed.exp, Some(1794116452));
    }

    #[test]
    fn test_deserialize_auto_response_ignores_uncontracted_multi_turn_fields() {
        let fixture = json!({
            "session_token": "token123",
            "selected_model": "gpt-5.6-luna",
            "multi_turn": {
                "enabled": true,
                "backoff_coefficient": 2.5
            }
        });
        let resp: CopilotAutoResponse = serde_json::from_value(fixture).unwrap();
        assert_eq!(resp.selected_model.model_id(), "gpt-5.6-luna");
    }

    #[test]
    fn test_extract_prompt_summary_responses_plain_string() {
        let req = json!({
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": "A direct string content without array"
                }
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert_eq!(prompt, "A direct string content without array");
    }

    #[test]
    fn test_extract_prompt_summary_responses_top_level_string_input() {
        let req = json!({
            "input": "Direct top-level input string"
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert_eq!(prompt, "Direct top-level input string");
        assert!(!has_image);
    }

    #[test]
    fn test_extract_prompt_summary_responses_top_level_items() {
        let req = json!({
            "input": [
                {"type": "input_text", "text": "Top level item"},
                {"type": "input_image", "image_url": "https://example.com/test.png"}
            ]
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert_eq!(prompt, "Top level item");
        assert!(has_image);
    }

    #[test]
    fn test_extract_prompt_summary_responses_standard_untyped_user_message() {
        let req = json!({
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Standard Responses text"},
                        {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                    ]
                }
            ]
        });
        let (prompt, has_image) =
            extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert_eq!(prompt, "Standard Responses text");
        assert!(has_image);
    }

    #[test]
    fn test_parsed_session_token_with_float_exp() {
        let payload = json!({
            "exp": 1794116452.5,
            "auto_session_id": "session-xyz"
        });
        let payload_str = serde_json::to_string(&payload).unwrap();
        let b64 = URL_SAFE_NO_PAD.encode(payload_str.as_bytes());
        let token = format!("eyJhbGciOiJFUzI1NiJ9.{b64}.sig");
        let parsed = ParsedSessionToken::parse(&token).unwrap();
        assert_eq!(parsed.exp, Some(1794116452));
    }

    #[test]
    fn test_parse_expires_at_field_forms() {
        let unix_secs = json!(1794116452u64);
        assert_eq!(
            parse_expires_at_field(Some(&unix_secs)),
            Ok(Some(1794116452))
        );

        let millis = json!(1794116452000i64);
        assert_eq!(parse_expires_at_field(Some(&millis)), Ok(Some(1794116452)));

        let rfc3339 = json!("2026-11-10T12:00:00Z");
        let parsed = parse_expires_at_field(Some(&rfc3339)).unwrap().unwrap();
        assert!(parsed > 1_700_000_000, "RFC 3339 must parse to unix secs");

        let numeric_string = json!("1794116452");
        assert_eq!(
            parse_expires_at_field(Some(&numeric_string)),
            Ok(Some(1794116452))
        );

        let numeric_string_millis = json!("1794116452000");
        assert_eq!(
            parse_expires_at_field(Some(&numeric_string_millis)),
            Ok(Some(1794116452))
        );

        assert_eq!(parse_expires_at_field(None), Ok(None));
        assert_eq!(parse_expires_at_field(Some(&json!(null))), Ok(None));

        assert!(parse_expires_at_field(Some(&json!("not-a-date"))).is_err());
        assert!(parse_expires_at_field(Some(&json!({"t": 1}))).is_err());
    }

    #[test]
    fn test_deserialize_auto_response_with_top_level_expires_at() {
        let fixture = json!({
            "session_token": "token123",
            "selected_model": "gpt-5.6-luna",
            "expires_at": "2020-01-01T00:00:00Z"
        });
        let resp: CopilotAutoResponse = serde_json::from_value(fixture).unwrap();
        assert_eq!(
            parse_expires_at_field(resp.expires_at.as_ref()).unwrap(),
            Some(1577836800)
        );
    }

    #[test]
    fn test_extract_prompt_summary_multi_turn_aggregation() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "First question"},
                {"role": "assistant", "content": "First answer"},
                {"role": "user", "content": [{"type": "text", "text": "Follow up question"}]}
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert_eq!(prompt, "First question\nFollow up question");
    }

    #[test]
    fn test_extract_prompt_summary_message_with_instruction_and_large_code_block() {
        let large_code =
            "fn do_something() {\n".to_string() + &"    println!(\"hello\");\n".repeat(300) + "}";
        assert!(large_code.len() > 4000);
        let req = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "请审查下面代码"},
                        {"type": "text", "text": large_code}
                    ]
                }
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert!(
            prompt.starts_with("请审查下面代码"),
            "Prompt must preserve user instruction at the start of the message, but was: {prompt}"
        );
        assert!(prompt.len() <= 4000);
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn test_extract_prompt_summary_responses_message_with_instruction_and_large_code_block() {
        let large_code =
            "fn do_something() {\n".to_string() + &"    println!(\"hello\");\n".repeat(300) + "}";
        assert!(large_code.len() > 4000);
        let req = json!({
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "请审查下面代码"},
                        {"type": "input_text", "text": large_code}
                    ]
                }
            ]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::CodexResponses);
        assert!(
            prompt.starts_with("请审查下面代码"),
            "Prompt must preserve user instruction at the start of the message, but was: {prompt}"
        );
        assert!(prompt.len() <= 4000);
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn test_extract_prompt_summary_skips_claude_code_system_reminders() {
        // Claude Code's first turn: injected context first, the actual task last.
        let reminder = format!(
            "<system-reminder>\n# claudeMd\n{}</system-reminder>",
            "Project rule: keep commits atomic.\n".repeat(150)
        );
        assert!(reminder.len() > 4000);
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": reminder},
                    {"type": "text", "text": "Fix the failing login test\n<system-reminder>todo list is empty</system-reminder>"}
                ]
            }]
        });
        let (prompt, _) = extract_prompt_summary_for_auto(&req, RequestProtocol::AnthropicMessages);
        assert_eq!(prompt, "Fix the failing login test");
    }

    #[test]
    fn test_strip_system_reminders_keeps_unclosed_tags() {
        assert_eq!(
            strip_system_reminders("a <system-reminder>b</system-reminder> c"),
            "a  c"
        );
        assert_eq!(
            strip_system_reminders("explain <system-reminder> tags"),
            "explain <system-reminder> tags"
        );
    }

    #[tokio::test]
    async fn resolve_reports_a_slow_auto_response_as_timeout() {
        let app = axum::Router::new().route(
            "/auto",
            axum::routing::post(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                axum::Json(json!({ "session_token": "t", "selected_model": "m" }))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind /auto mock");
        let base = format!("http://{}", listener.local_addr().expect("mock address"));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve /auto mock");
        });

        let result = CopilotAutoClient::resolve(
            &crate::proxy::http_client::get_for_url(&base),
            &base,
            "token",
            "hello",
            false,
            std::time::Duration::from_millis(200),
        )
        .await;
        server.abort();

        assert!(
            matches!(result, Err(ProxyError::Timeout(_))),
            "reqwest's own timeout must surface as Timeout, got {result:?}"
        );
    }
}
