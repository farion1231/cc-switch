//! 请求日志捕获模块
//!
//! 在代理转发流程中捕获 HTTP 请求/响应的完整内容（重点是 request body 中的 system prompt），
//! 存储在内存环形缓冲区中，并通过 Tauri Event 实时推送给前端。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 单条请求日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequestLogEntry {
    /// 唯一 ID
    pub id: String,
    /// 时间戳 (ISO 8601)
    pub timestamp: String,
    /// 应用类型 (claude / codex / gemini / hermes / opencode / openclaw)
    pub app_type: String,
    /// Provider 名称
    pub provider_name: String,
    /// Provider ID
    pub provider_id: String,
    /// HTTP 方法
    pub method: String,
    /// 请求端点
    pub endpoint: String,
    /// 请求模型
    pub model: String,
    /// 是否流式请求
    pub is_stream: bool,
    /// 请求 body（完整 JSON）
    pub request_body: Value,
    /// 响应 body（非流式为完整 JSON，流式为拼接后的 SSE data 数组）
    pub response_body: Option<Value>,
    /// 响应状态码（转发完成后回填）
    pub status_code: Option<u16>,
    /// 耗时（毫秒）
    pub latency_ms: Option<u64>,
    /// Session ID
    pub session_id: Option<String>,
    /// 提取的 system prompt（便于快速查看）
    pub system_prompt: Option<String>,
}

/// 推送给前端的事件 payload（精简版，不含完整 body）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogEventPayload {
    pub id: String,
    pub timestamp: String,
    pub app_type: String,
    pub provider_name: String,
    pub method: String,
    pub endpoint: String,
    pub model: String,
    pub is_stream: bool,
    pub status_code: Option<u16>,
    pub latency_ms: Option<u64>,
    pub has_system_prompt: bool,
    /// system prompt 预览（截取前 200 字符）
    pub system_prompt_preview: Option<String>,
}

impl From<&ProxyRequestLogEntry> for RequestLogEventPayload {
    fn from(entry: &ProxyRequestLogEntry) -> Self {
        let system_prompt_preview = entry.system_prompt.as_ref().map(|prompt| {
            if prompt.len() > 200 {
                let mut end = 200;
                while end > 0 && !prompt.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &prompt[..end])
            } else {
                prompt.clone()
            }
        });
        Self {
            id: entry.id.clone(),
            timestamp: entry.timestamp.clone(),
            app_type: entry.app_type.clone(),
            provider_name: entry.provider_name.clone(),
            method: entry.method.clone(),
            endpoint: entry.endpoint.clone(),
            model: entry.model.clone(),
            is_stream: entry.is_stream,
            status_code: entry.status_code,
            latency_ms: entry.latency_ms,
            has_system_prompt: entry.system_prompt.is_some(),
            system_prompt_preview,
        }
    }
}

/// 默认最大保留条数
const DEFAULT_MAX_LOG_ENTRIES: usize = 200;

/// 请求日志存储（内存环形缓冲区）
pub struct RequestLogStore {
    entries: Arc<RwLock<VecDeque<ProxyRequestLogEntry>>>,
    enabled: Arc<AtomicBool>,
    max_entries: Arc<AtomicUsize>,
}

impl RequestLogStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(DEFAULT_MAX_LOG_ENTRIES))),
            enabled: Arc::new(AtomicBool::new(false)),
            max_entries: Arc::new(AtomicUsize::new(DEFAULT_MAX_LOG_ENTRIES)),
        }
    }

    /// 是否启用日志捕获
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 设置是否启用日志捕获
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// 获取最大保留条数
    pub fn get_max_entries(&self) -> usize {
        self.max_entries.load(Ordering::Relaxed)
    }

    /// 设置最大保留条数，并立即淘汰超出的旧日志
    pub async fn set_max_entries(&self, max: usize) {
        let max = max.max(1); // 至少保留 1 条
        self.max_entries.store(max, Ordering::Relaxed);
        let mut entries = self.entries.write().await;
        while entries.len() > max {
            entries.pop_front();
        }
    }

    /// 添加一条日志
    pub async fn push(&self, entry: ProxyRequestLogEntry) {
        if !self.is_enabled() {
            return;
        }
        let max = self.max_entries.load(Ordering::Relaxed);
        let mut entries = self.entries.write().await;
        while entries.len() >= max {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// 更新已有日志的响应信息（status_code, latency_ms, response_body）
    pub async fn update_response(&self, id: &str, status_code: u16, latency_ms: u64, response_body: Option<Value>) {
        if !self.is_enabled() {
            return;
        }
        let mut entries = self.entries.write().await;
        // 从后往前搜索（最新的在后面）
        for entry in entries.iter_mut().rev() {
            if entry.id == id {
                entry.status_code = Some(status_code);
                entry.latency_ms = Some(latency_ms);
                if response_body.is_some() {
                    entry.response_body = response_body;
                }
                break;
            }
        }
    }

    /// 获取所有日志（按时间倒序）
    pub async fn get_all(&self) -> Vec<ProxyRequestLogEntry> {
        let entries = self.entries.read().await;
        entries.iter().rev().cloned().collect()
    }

    /// 获取单条日志详情
    pub async fn get_by_id(&self, id: &str) -> Option<ProxyRequestLogEntry> {
        let entries = self.entries.read().await;
        entries.iter().find(|e| e.id == id).cloned()
    }

    /// 清空所有日志
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }
}

impl Default for RequestLogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 从请求 body 中提取 system prompt
///
/// 支持多种 API 格式：
/// - Anthropic (Claude): `body.system` (string 或 array)
/// - OpenAI Chat: `body.messages[0]` where role=system
/// - OpenAI Responses: `body.instructions`
/// - Gemini: `body.systemInstruction.parts[0].text`
pub fn extract_system_prompt(body: &Value) -> Option<String> {
    // Anthropic: body.system (string)
    if let Some(system) = body.get("system").and_then(|v| v.as_str()) {
        return Some(system.to_string());
    }

    // Anthropic: body.system (array of content blocks)
    if let Some(system_arr) = body.get("system").and_then(|v| v.as_array()) {
        let texts: Vec<&str> = system_arr
            .iter()
            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
            .collect();
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }

    // OpenAI Chat: messages[0].role == "system" or "developer"
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "system" || role == "developer" {
                if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                    return Some(content.to_string());
                }
                // content 也可能是 array
                if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                    let texts: Vec<&str> = content_arr
                        .iter()
                        .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if !texts.is_empty() {
                        return Some(texts.join("\n"));
                    }
                }
            }
        }
    }

    // OpenAI Responses: body.instructions
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        return Some(instructions.to_string());
    }

    // Gemini: body.systemInstruction.parts[].text
    if let Some(parts) = body
        .pointer("/systemInstruction/parts")
        .and_then(|v| v.as_array())
    {
        let texts: Vec<&str> = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
            .collect();
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }

    None
}

/// 创建一条请求日志条目
pub fn create_log_entry(
    app_type: &str,
    provider_name: &str,
    provider_id: &str,
    method: &str,
    endpoint: &str,
    model: &str,
    is_stream: bool,
    body: &Value,
    session_id: Option<String>,
) -> ProxyRequestLogEntry {
    let system_prompt = extract_system_prompt(body);
    ProxyRequestLogEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        app_type: app_type.to_string(),
        provider_name: provider_name.to_string(),
        provider_id: provider_id.to_string(),
        method: method.to_string(),
        endpoint: endpoint.to_string(),
        model: model.to_string(),
        is_stream,
        request_body: body.clone(),
        response_body: None,
        status_code: None,
        latency_ms: None,
        session_id,
        system_prompt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_anthropic_system_string() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "hi"}]
        });
        assert_eq!(
            extract_system_prompt(&body).unwrap(),
            "You are a helpful assistant."
        );
    }

    #[test]
    fn extract_anthropic_system_array() {
        let body = json!({
            "system": [
                {"type": "text", "text": "Part 1"},
                {"type": "text", "text": "Part 2"}
            ]
        });
        assert_eq!(extract_system_prompt(&body).unwrap(), "Part 1\nPart 2");
    }

    #[test]
    fn extract_openai_system_message() {
        let body = json!({
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "hello"}
            ]
        });
        assert_eq!(extract_system_prompt(&body).unwrap(), "Be concise.");
    }

    #[test]
    fn extract_openai_developer_message() {
        let body = json!({
            "messages": [
                {"role": "developer", "content": "Developer instructions here."},
                {"role": "user", "content": "hello"}
            ]
        });
        assert_eq!(
            extract_system_prompt(&body).unwrap(),
            "Developer instructions here."
        );
    }

    #[test]
    fn extract_openai_responses_instructions() {
        let body = json!({
            "instructions": "You are a coding assistant.",
            "input": "write hello world"
        });
        assert_eq!(
            extract_system_prompt(&body).unwrap(),
            "You are a coding assistant."
        );
    }

    #[test]
    fn extract_gemini_system_instruction() {
        let body = json!({
            "systemInstruction": {
                "parts": [{"text": "Gemini system prompt"}]
            }
        });
        assert_eq!(
            extract_system_prompt(&body).unwrap(),
            "Gemini system prompt"
        );
    }

    #[test]
    fn extract_no_system_prompt() {
        let body = json!({"messages": [{"role": "user", "content": "hi"}]});
        assert!(extract_system_prompt(&body).is_none());
    }

    #[tokio::test]
    async fn store_push_and_get() {
        let store = RequestLogStore::new();
        store.set_enabled(true);
        let entry = create_log_entry(
            "claude",
            "Test Provider",
            "test-id",
            "POST",
            "/v1/messages",
            "claude-sonnet-4-20250514",
            true,
            &json!({"system": "test prompt"}),
            None,
        );
        let entry_id = entry.id.clone();
        store.push(entry).await;

        let all = store.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].system_prompt.as_deref(), Some("test prompt"));

        let detail = store.get_by_id(&entry_id).await;
        assert!(detail.is_some());
    }

    #[tokio::test]
    async fn store_disabled_does_not_push() {
        let store = RequestLogStore::new();
        // enabled 默认 false
        let entry = create_log_entry(
            "claude", "P", "id", "POST", "/v1/messages", "m", false, &json!({}), None,
        );
        store.push(entry).await;
        assert!(store.get_all().await.is_empty());
    }

    #[tokio::test]
    async fn store_ring_buffer_eviction() {
        let store = RequestLogStore::new();
        store.set_enabled(true);
        for i in 0..510 {
            let entry = create_log_entry(
                "claude",
                "P",
                "id",
                "POST",
                "/v1/messages",
                &format!("model-{i}"),
                false,
                &json!({}),
                None,
            );
            store.push(entry).await;
        }
        let all = store.get_all().await;
        assert_eq!(all.len(), 500);
        // 最新的在前面（倒序）
        assert_eq!(all[0].model, "model-509");
    }
}
