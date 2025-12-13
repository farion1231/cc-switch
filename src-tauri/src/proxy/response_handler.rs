//! Response Handler - 统一响应处理
//!
//! 提供流式和非流式响应的统一处理接口

use super::session::ProxySession;
use super::usage::parser::TokenUsage;
use super::ProxyError;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// 响应类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ResponseType {
    /// 流式响应 (SSE)
    Stream,
    /// 非流式响应
    NonStream,
}

impl ResponseType {
    /// 从 Content-Type 检测响应类型
    #[allow(dead_code)]
    pub fn from_content_type(content_type: &str) -> Self {
        if content_type.contains("text/event-stream") {
            ResponseType::Stream
        } else {
            ResponseType::NonStream
        }
    }
}

/// 流式响应处理器
#[allow(dead_code)]
pub struct StreamHandler {
    /// 空闲超时时间
    idle_timeout: Duration,
    /// 收集的事件
    events: Arc<Mutex<Vec<Value>>>,
}

#[allow(dead_code)]
impl StreamHandler {
    /// 创建新的流式处理器
    pub fn new(idle_timeout_secs: u64) -> Self {
        Self {
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 处理流式响应，返回分流后的客户端流
    ///
    /// 客户端流立即返回，内部流在后台收集事件
    pub fn handle_stream<S>(
        &self,
        stream: S,
    ) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        let events = self.events.clone();
        let idle_timeout = self.idle_timeout;

        async_stream::stream! {
            let mut _last_activity = Instant::now();
            let mut buffer = String::new();

            tokio::pin!(stream);

            loop {
                let chunk_result = timeout(idle_timeout, stream.next()).await;

                match chunk_result {
                    Ok(Some(Ok(bytes))) => {
                        _last_activity = Instant::now();

                        // 解析 SSE 事件
                        let text = String::from_utf8_lossy(&bytes);
                        buffer.push_str(&text);

                        // 提取完整事件
                        while let Some(pos) = buffer.find("\n\n") {
                            let event_text = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            for line in event_text.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data.trim() != "[DONE]" {
                                        if let Ok(json) = serde_json::from_str::<Value>(data) {
                                            let mut guard = events.lock().await;
                                            guard.push(json);
                                        }
                                    }
                                }
                            }
                        }

                        yield Ok(bytes);
                    }
                    Ok(Some(Err(e))) => {
                        log::error!("流错误: {e}");
                        yield Err(std::io::Error::other(e.to_string()));
                        break;
                    }
                    Ok(None) => {
                        // 流结束
                        break;
                    }
                    Err(_) => {
                        // 空闲超时
                        log::warn!("流式响应空闲超时: {idle_timeout:?} 无数据");
                        yield Err(std::io::Error::other("Stream idle timeout"));
                        break;
                    }
                }
            }
        }
    }

    /// 获取收集的事件
    pub async fn get_events(&self) -> Vec<Value> {
        let guard = self.events.lock().await;
        guard.clone()
    }

    /// 从收集的事件中提取 Token 使用量
    pub async fn extract_usage(&self, session: &ProxySession) -> Option<TokenUsage> {
        let events = self.get_events().await;

        match session.client_format {
            super::session::ClientFormat::Claude => TokenUsage::from_claude_stream_events(&events),
            super::session::ClientFormat::Codex => TokenUsage::from_codex_stream_events(&events),
            super::session::ClientFormat::Gemini | super::session::ClientFormat::GeminiCli => {
                TokenUsage::from_gemini_stream_chunks(&events)
            }
            _ => None,
        }
    }
}

/// 非流式响应处理器
#[allow(dead_code)]
pub struct NonStreamHandler;

#[allow(dead_code)]
impl NonStreamHandler {
    /// 处理非流式响应
    ///
    /// 克隆响应体用于后台解析，原始响应立即返回
    pub async fn handle_response(
        body: &[u8],
        session: &ProxySession,
    ) -> Result<Option<TokenUsage>, ProxyError> {
        let json: Value = serde_json::from_slice(body)
            .map_err(|e| ProxyError::TransformError(format!("Failed to parse response: {e}")))?;

        let usage = match session.client_format {
            super::session::ClientFormat::Claude => TokenUsage::from_claude_response(&json),
            super::session::ClientFormat::Codex => TokenUsage::from_codex_response_adjusted(&json),
            super::session::ClientFormat::Gemini | super::session::ClientFormat::GeminiCli => {
                TokenUsage::from_gemini_response(&json)
            }
            super::session::ClientFormat::OpenAI => TokenUsage::from_openrouter_response(&json),
            _ => None,
        };

        Ok(usage)
    }
}

/// 统一响应分发器
#[allow(dead_code)]
pub struct ResponseDispatcher;

#[allow(dead_code)]
impl ResponseDispatcher {
    /// 判断响应类型
    pub fn detect_type(content_type: &str) -> ResponseType {
        ResponseType::from_content_type(content_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_type_detection() {
        assert_eq!(
            ResponseType::from_content_type("text/event-stream"),
            ResponseType::Stream
        );
        assert_eq!(
            ResponseType::from_content_type("text/event-stream; charset=utf-8"),
            ResponseType::Stream
        );
        assert_eq!(
            ResponseType::from_content_type("application/json"),
            ResponseType::NonStream
        );
    }

    #[test]
    fn test_stream_handler_creation() {
        let handler = StreamHandler::new(30);
        assert_eq!(handler.idle_timeout, Duration::from_secs(30));
    }
}
