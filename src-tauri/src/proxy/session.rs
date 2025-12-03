//! Proxy Session - 请求会话管理
//!
//! 为每个代理请求创建会话上下文，在整个请求生命周期中跟踪状态和元数据。

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
}
