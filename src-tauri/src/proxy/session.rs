//! Proxy Session - 请求会话管理
//!
//! 为每个代理请求创建会话上下文，在整个请求生命周期中跟踪状态和元数据。
//!
//! ## Session ID 提取
//!
//! 支持从客户端请求中提取 Session ID，用于关联同一对话的多个请求：
//! - Claude: 从 `metadata.user_id` (格式: `user_xxx_session_yyy`) 或 `metadata.session_id` 提取
//! - Codex: 从 `previous_response_id` 或 headers 中的 `session_id` 提取
//! - 其他: 生成新的 UUID

use axum::http::HeaderMap;
use std::time::Instant;
use uuid::Uuid;

/// 客户端请求格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ClientFormat {
    /// Claude Messages API (/v1/messages)
    Claude,
    /// Codex Response API (/v1/responses)
    Codex,
    /// OpenAI Chat Completions API (/v1/chat/completions)
    OpenAI,
    /// Gemini API (/v1beta/models/*/generateContent)
    Gemini,
    /// Gemini CLI API (/v1internal/models/*/generateContent)
    GeminiCli,
    /// 未知格式
    Unknown,
}

#[allow(dead_code)]
impl ClientFormat {
    /// 从请求路径检测格式
    pub fn from_path(path: &str) -> Self {
        if path.contains("/v1/messages") {
            ClientFormat::Claude
        } else if path.contains("/v1/responses") {
            ClientFormat::Codex
        } else if path.contains("/v1/chat/completions") {
            ClientFormat::OpenAI
        } else if path.contains("/v1internal/") && path.contains("generateContent") {
            // Gemini CLI 使用 /v1internal/ 路径
            ClientFormat::GeminiCli
        } else if (path.contains("/v1beta/") || path.contains("/v1/"))
            && path.contains("generateContent")
        {
            // Gemini API 使用 /v1beta/ 或 /v1/ 路径
            ClientFormat::Gemini
        } else if path.contains("generateContent") {
            // 通用 Gemini 端点
            ClientFormat::Gemini
        } else {
            ClientFormat::Unknown
        }
    }

    /// 从请求体内容检测格式（回退方案）
    pub fn from_body(body: &serde_json::Value) -> Self {
        // Claude 格式特征: messages 数组 + model 字段 + 无 response_format
        if body.get("messages").is_some()
            && body.get("model").is_some()
            && body.get("response_format").is_none()
            && body.get("contents").is_none()
        {
            // 区分 Claude 和 OpenAI
            if body.get("max_tokens").is_some() {
                return ClientFormat::Claude;
            }
            return ClientFormat::OpenAI;
        }

        // Codex 格式特征: input 字段
        if body.get("input").is_some() {
            return ClientFormat::Codex;
        }

        // Gemini 格式特征: contents 数组
        if body.get("contents").is_some() {
            return ClientFormat::Gemini;
        }

        ClientFormat::Unknown
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            ClientFormat::Claude => "claude",
            ClientFormat::Codex => "codex",
            ClientFormat::OpenAI => "openai",
            ClientFormat::Gemini => "gemini",
            ClientFormat::GeminiCli => "gemini_cli",
            ClientFormat::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for ClientFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 代理会话
///
/// 包含请求全生命周期的上下文数据
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProxySession {
    /// 唯一会话 ID
    pub session_id: String,
    /// 请求开始时间
    pub start_time: Instant,
    /// HTTP 方法
    pub method: String,
    /// 请求 URL
    pub request_url: String,
    /// User-Agent
    pub user_agent: Option<String>,
    /// 客户端请求格式
    pub client_format: ClientFormat,
    /// 选定的供应商 ID
    pub provider_id: Option<String>,
    /// 模型名称
    pub model: Option<String>,
    /// 是否为流式请求
    pub is_streaming: bool,
}

#[allow(dead_code)]
impl ProxySession {
    /// 从请求创建会话
    pub fn from_request(
        method: &str,
        request_url: &str,
        user_agent: Option<&str>,
        body: Option<&serde_json::Value>,
    ) -> Self {
        // 检测客户端格式
        let mut client_format = ClientFormat::from_path(request_url);
        if client_format == ClientFormat::Unknown {
            if let Some(body) = body {
                client_format = ClientFormat::from_body(body);
            }
        }

        // 检测是否为流式请求
        let is_streaming = body
            .and_then(|b| b.get("stream"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 提取模型名称
        let model = body
            .and_then(|b| b.get("model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            session_id: Uuid::new_v4().to_string(),
            start_time: Instant::now(),
            method: method.to_string(),
            request_url: request_url.to_string(),
            user_agent: user_agent.map(|s| s.to_string()),
            client_format,
            provider_id: None,
            model,
            is_streaming,
        }
    }

    /// 设置供应商 ID
    pub fn with_provider(mut self, provider_id: &str) -> Self {
        self.provider_id = Some(provider_id.to_string());
        self
    }

    /// 获取请求延迟（毫秒）
    pub fn latency_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

// ============================================================================
// Session ID 提取器
// ============================================================================

/// Session ID 来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdSource {
    /// 从 metadata.user_id 提取 (Claude)
    MetadataUserId,
    /// 从 metadata.session_id 提取
    MetadataSessionId,
    /// 从 headers 提取 (Codex)
    Header,
    /// 从 previous_response_id 提取 (Codex)
    PreviousResponseId,
    /// 新生成
    Generated,
}

/// Session ID 提取结果
#[derive(Debug, Clone)]
pub struct SessionIdResult {
    /// 提取或生成的 Session ID
    pub session_id: String,
    /// Session ID 来源
    pub source: SessionIdSource,
    /// 是否为客户端提供的 ID（非新生成）
    pub client_provided: bool,
}

/// 从请求中提取或生成 Session ID
///
/// 轻量化实现，仅提取 session_id 用于日志记录，不做复杂的 Session 管理。
///
/// ## 提取优先级
///
/// ### Claude 请求
/// 1. `metadata.user_id` (格式: `user_xxx_session_yyy`) → 提取 `yyy` 部分
/// 2. `metadata.session_id` → 直接使用
/// 3. 生成新 UUID
///
/// ### Codex 请求
/// 1. Headers: `session_id` 或 `x-session-id`
/// 2. `metadata.session_id`
/// 3. `previous_response_id` (对话延续)
/// 4. 生成新 UUID
///
/// ## 示例
///
/// ```ignore
/// let result = extract_session_id(&headers, &body, "claude");
/// println!("Session ID: {} (from {:?})", result.session_id, result.source);
/// ```
pub fn extract_session_id(
    headers: &HeaderMap,
    body: &serde_json::Value,
    client_format: &str,
) -> SessionIdResult {
    // Codex 请求特殊处理
    if client_format == "codex" || client_format == "openai" {
        if let Some(result) = extract_codex_session(headers, body) {
            return result;
        }
    }

    // Claude 请求：从 metadata 提取
    if let Some(result) = extract_from_metadata(body) {
        return result;
    }

    // 兜底：生成新 Session ID
    generate_new_session_id()
}

/// 提取 Codex Session ID
fn extract_codex_session(headers: &HeaderMap, body: &serde_json::Value) -> Option<SessionIdResult> {
    // 1. 从 headers 提取
    for header_name in &["session_id", "x-session-id"] {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(session_id) = value.to_str() {
                // Codex Session ID 通常较长（UUID 格式）
                if session_id.len() > 20 {
                    return Some(SessionIdResult {
                        session_id: format!("codex_{session_id}"),
                        source: SessionIdSource::Header,
                        client_provided: true,
                    });
                }
            }
        }
    }

    // 2. 从 body.metadata.session_id 提取
    if let Some(session_id) = body
        .get("metadata")
        .and_then(|m| m.get("session_id"))
        .and_then(|v| v.as_str())
    {
        if session_id.len() > 10 {
            return Some(SessionIdResult {
                session_id: format!("codex_{session_id}"),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
    }

    // 3. 从 previous_response_id 提取（对话延续）
    if let Some(prev_id) = body.get("previous_response_id").and_then(|v| v.as_str()) {
        if prev_id.len() > 10 {
            return Some(SessionIdResult {
                session_id: format!("codex_{prev_id}"),
                source: SessionIdSource::PreviousResponseId,
                client_provided: true,
            });
        }
    }

    None
}

/// 从 metadata 提取 Session ID (Claude)
fn extract_from_metadata(body: &serde_json::Value) -> Option<SessionIdResult> {
    let metadata = body.get("metadata")?;

    // 1. 从 metadata.user_id 提取（格式: user_xxx_session_yyy）
    if let Some(user_id) = metadata.get("user_id").and_then(|v| v.as_str()) {
        if let Some(session_id) = parse_session_from_user_id(user_id) {
            return Some(SessionIdResult {
                session_id,
                source: SessionIdSource::MetadataUserId,
                client_provided: true,
            });
        }
    }

    // 2. 直接从 metadata.session_id 提取
    if let Some(session_id) = metadata.get("session_id").and_then(|v| v.as_str()) {
        if !session_id.is_empty() {
            return Some(SessionIdResult {
                session_id: session_id.to_string(),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
    }

    None
}

/// 从 user_id 解析 session_id
///
/// 格式: `user_identifier_session_actual_session_id`
fn parse_session_from_user_id(user_id: &str) -> Option<String> {
    // 查找 "_session_" 分隔符
    if let Some(pos) = user_id.find("_session_") {
        let session_id = &user_id[pos + 9..]; // "_session_" 长度为 9
        if !session_id.is_empty() {
            return Some(session_id.to_string());
        }
    }
    None
}

/// 生成新的 Session ID
fn generate_new_session_id() -> SessionIdResult {
    SessionIdResult {
        session_id: Uuid::new_v4().to_string(),
        source: SessionIdSource::Generated,
        client_provided: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_client_format_from_path_claude() {
        assert_eq!(
            ClientFormat::from_path("/v1/messages"),
            ClientFormat::Claude
        );
        assert_eq!(
            ClientFormat::from_path("/api/v1/messages"),
            ClientFormat::Claude
        );
    }

    #[test]
    fn test_client_format_from_path_codex() {
        assert_eq!(
            ClientFormat::from_path("/v1/responses"),
            ClientFormat::Codex
        );
    }

    #[test]
    fn test_client_format_from_path_openai() {
        assert_eq!(
            ClientFormat::from_path("/v1/chat/completions"),
            ClientFormat::OpenAI
        );
    }

    #[test]
    fn test_client_format_from_path_gemini() {
        assert_eq!(
            ClientFormat::from_path("/v1beta/models/gemini-pro:generateContent"),
            ClientFormat::Gemini
        );
    }

    #[test]
    fn test_client_format_from_path_gemini_cli() {
        assert_eq!(
            ClientFormat::from_path("/v1internal/models/gemini-pro:generateContent"),
            ClientFormat::GeminiCli
        );
    }

    #[test]
    fn test_client_format_from_body_claude() {
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        });
        assert_eq!(ClientFormat::from_body(&body), ClientFormat::Claude);
    }

    #[test]
    fn test_client_format_from_body_codex() {
        let body = json!({
            "input": "Write a function"
        });
        assert_eq!(ClientFormat::from_body(&body), ClientFormat::Codex);
    }

    #[test]
    fn test_client_format_from_body_gemini() {
        let body = json!({
            "contents": [{"parts": [{"text": "Hello"}]}]
        });
        assert_eq!(ClientFormat::from_body(&body), ClientFormat::Gemini);
    }

    #[test]
    fn test_session_id_uniqueness() {
        let session1 = ProxySession::from_request("POST", "/v1/messages", None, None);
        let session2 = ProxySession::from_request("POST", "/v1/messages", None, None);
        assert_ne!(session1.session_id, session2.session_id);
    }

    #[test]
    fn test_session_from_request() {
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024,
            "stream": true
        });

        let session =
            ProxySession::from_request("POST", "/v1/messages", Some("Mozilla/5.0"), Some(&body));

        assert_eq!(session.method, "POST");
        assert_eq!(session.request_url, "/v1/messages");
        assert_eq!(session.user_agent, Some("Mozilla/5.0".to_string()));
        assert_eq!(session.client_format, ClientFormat::Claude);
        assert_eq!(session.model, Some("claude-3-5-sonnet".to_string()));
        assert!(session.is_streaming);
    }

    #[test]
    fn test_session_with_provider() {
        let session = ProxySession::from_request("POST", "/v1/messages", None, None)
            .with_provider("provider-123");

        assert_eq!(session.provider_id, Some("provider-123".to_string()));
    }

    #[test]
    fn test_client_format_as_str() {
        assert_eq!(ClientFormat::Claude.as_str(), "claude");
        assert_eq!(ClientFormat::Codex.as_str(), "codex");
        assert_eq!(ClientFormat::OpenAI.as_str(), "openai");
        assert_eq!(ClientFormat::Gemini.as_str(), "gemini");
        assert_eq!(ClientFormat::GeminiCli.as_str(), "gemini_cli");
        assert_eq!(ClientFormat::Unknown.as_str(), "unknown");
    }

    // ========== Session ID 提取测试 ==========

    #[test]
    fn test_extract_session_from_claude_metadata_user_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "user_id": "user_john_doe_session_abc123def456"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "abc123def456");
        assert_eq!(result.source, SessionIdSource::MetadataUserId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_metadata_session_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "session_id": "my-session-123"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "my-session-123");
        assert_eq!(result.source, SessionIdSource::MetadataSessionId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_codex_previous_response_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "input": "Write a function",
            "previous_response_id": "resp_abc123def456789"
        });

        let result = extract_session_id(&headers, &body, "codex");

        assert_eq!(result.session_id, "codex_resp_abc123def456789");
        assert_eq!(result.source, SessionIdSource::PreviousResponseId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_generates_new_when_not_found() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert!(!result.session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_parse_session_from_user_id() {
        assert_eq!(
            parse_session_from_user_id("user_john_session_abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            parse_session_from_user_id("my_app_session_xyz789"),
            Some("xyz789".to_string())
        );
        // 注意: "_session_" 是分隔符，所以下面的字符串会匹配
        assert_eq!(
            parse_session_from_user_id("no_session_marker"),
            Some("marker".to_string())
        );
        // 没有 "_session_" 分隔符的情况
        assert_eq!(parse_session_from_user_id("user_john_abc123"), None);
        assert_eq!(parse_session_from_user_id("_session_"), None);
    }
}
