//! Proxy Session - 请求会话管理
//!
//! 为每个代理请求创建会话上下文，在整个请求生命周期中跟踪状态和元数据。
//!
//! ## Session ID 提取
//!
//! 支持从客户端请求中提取 Session ID，用于关联同一对话的多个请求：
//! - Claude: 从 `metadata.user_id` (格式: `user_xxx_session_yyy`) 或 `metadata.session_id` 提取
//! - Codex: 从 headers 中的 `session-id` / `thread-id` / `session_id` / `x-session-id` 或 `metadata.session_id` 提取；压缩见证另提取 thread 级隔离键（`SessionIdResult::witness_key`）
//! - Grok Build: 从 headers 中的 `x-grok-conv-id` / `x-grok-session-id` 提取
//! - 其他: 生成新的 UUID

use axum::http::HeaderMap;
use uuid::Uuid;

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
    /// 从 headers 提取
    Header,
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
    /// 压缩见证的隔离键（仅 Codex Responses）。
    ///
    /// 见证记录（`CompactionReplayStore`）按此键隔离。上游 `response_id` 不保证全局
    /// 唯一，隔离面过粗就会把不同的对话链混在一起：Codex 的 `thread-id` 是"这个
    /// thread 的具体身份"（codex-rs `session.rs`），而 `session-id` 由 root 与所有
    /// descendant 线程共享——多个 sibling / descendant 线程（如子 Agent）带同一个
    /// session-id 请求时，只凭它会互相命中对方的压缩见证。因此隔离键由 `session-id`
    /// 拼写（含历史兼容拼写）与 `thread-id` 复合而成；缺少任一维度时为 `None`——
    /// 见证记录 / 查询 / 作废全部 fail-closed（不得仅凭 session-id 授权游标恢复）。
    /// GrokBuild 没有 thread 维度，沿用其对话身份（与 `session_id` 同值）。
    pub witness_key: Option<String>,
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
/// 1. Headers: `session-id` / `thread-id` / `session_id` / `x-session-id`
/// 2. `metadata.session_id`
/// 3. 生成新 UUID
///
/// `session_id` 是会话身份（日志 / 缓存键 / 官方 OAuth 透传用）；压缩见证另用
/// thread 级隔离键 `witness_key`（见 `SessionIdResult`）。
///
/// ### Grok Build 请求
/// 1. Headers: `x-grok-conv-id` 或 `x-grok-session-id`
/// 2. `metadata.session_id`
/// 3. 生成新 UUID
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
    if client_format == "claude" {
        if let Some(result) = extract_claude_session(headers, body) {
            return result;
        }
    }

    // Responses 请求特殊处理。Grok Build 使用与 Codex 相同的客户端协议，
    // 但保留独立前缀，避免统计和缓存键跨应用碰撞。
    if matches!(client_format, "codex" | "openai" | "grokbuild") {
        let prefix = if client_format == "grokbuild" {
            "grokbuild"
        } else {
            "codex"
        };
        if let Some(result) = extract_responses_session(headers, body, prefix) {
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

/// 提取 Claude Session ID
fn extract_claude_session(
    headers: &HeaderMap,
    body: &serde_json::Value,
) -> Option<SessionIdResult> {
    for header_name in &["x-claude-code-session-id", "claude-code-session-id"] {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(session_id) = value.to_str() {
                if !session_id.is_empty() {
                    return Some(SessionIdResult {
                        session_id: session_id.to_string(),
                        source: SessionIdSource::Header,
                        client_provided: true,
                        witness_key: None,
                    });
                }
            }
        }
    }

    extract_from_metadata(body)
}

/// 提取 Responses 客户端的 Session ID
fn extract_responses_session(
    headers: &HeaderMap,
    body: &serde_json::Value,
    prefix: &str,
) -> Option<SessionIdResult> {
    // 1. 从 headers 提取
    if prefix == "grokbuild" {
        // Conversation ID 跨多轮请求保持稳定；session ID 作为客户端缺少
        // conversation ID 时的回退。x-grok-req-id 是逐请求 ID，不能用于聚合。
        // GrokBuild 没有 thread 维度：见证隔离沿用其对话身份（与 session_id 同值）。
        for header_name in &["x-grok-conv-id", "x-grok-session-id"] {
            if let Some(value) = headers.get(*header_name) {
                if let Ok(identity) = value.to_str() {
                    let identity = identity.trim();
                    if identity.len() > 20 {
                        let prefixed = format!("{prefix}_{identity}");
                        return Some(SessionIdResult {
                            session_id: prefixed.clone(),
                            source: SessionIdSource::Header,
                            client_provided: true,
                            witness_key: Some(prefixed),
                        });
                    }
                }
            }
        }
    } else if let Some(identity) = first_usable_header(
        headers,
        &["session-id", "thread-id", "session_id", "x-session-id"],
    ) {
        // Codex CLI（codex-rs）的 Responses 客户端发送连字符拼写（build_session_headers，
        // 见 codex-rs/codex-api/src/requests/headers.rs，经 endpoint/responses.rs 注入
        // 每个请求）。`session_id` 字段的提取顺序与历史一致：session-id 优先、
        // thread-id 兜底、下划线拼写为兼容形态；它是会话身份（日志 / 缓存 / 官方
        // OAuth 透传），不能单独作为见证隔离面——session-id 由 root 与全部 descendant
        // 线程共享，sibling thread 撞 response id 时会互相命中对方的压缩见证。
        //
        // 见证隔离键另算：由 session 拼写与 thread-id（thread 的具体身份）复合。
        // 缺少 thread-id 或 session 拼写时为 None——见证链路 fail-closed。
        let session_spelling =
            first_usable_header(headers, &["session-id", "session_id", "x-session-id"]);
        let thread = first_usable_header(headers, &["thread-id"]);
        let witness_key = match (session_spelling, thread) {
            (Some(session), Some(thread)) => Some(format!("{prefix}_{session}:{thread}")),
            _ => None,
        };
        return Some(SessionIdResult {
            session_id: format!("{prefix}_{identity}"),
            source: SessionIdSource::Header,
            client_provided: true,
            witness_key,
        });
    }

    // 2. 从 body.metadata.session_id 提取。metadata 身份没有 thread 维度：
    //    见证隔离键保持 None，游标恢复对它 fail-closed。
    if let Some(session_id) = body
        .get("metadata")
        .and_then(|m| m.get("session_id"))
        .and_then(|v| v.as_str())
    {
        let session_id = session_id.trim();
        if session_id.len() > 10 {
            return Some(SessionIdResult {
                session_id: format!("{prefix}_{session_id}"),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
                witness_key: None,
            });
        }
    }

    // previous_response_id 是 Responses 协议里的响应游标，不是稳定会话身份。
    // Chat/Responses 桥接时该值通常来自上游每轮返回的随机 response id；
    // 若把它当 prompt_cache_key 或 Codex session header，会导致每轮请求换缓存 key。

    None
}

/// 按顺序取第一个可用（trim 后非空、长度 > 20）的 header 值。
///
/// 长度门槛来自历史行为：Responses 客户端的身份值是长 UUID；太短的标识
/// （请求内序号等）不能当稳定身份。
fn first_usable_header<'a>(headers: &'a HeaderMap, names: &[&str]) -> Option<&'a str> {
    for name in names {
        if let Some(value) = headers.get(*name) {
            if let Ok(value) = value.to_str() {
                let value = value.trim();
                if value.len() > 20 {
                    return Some(value);
                }
            }
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
                witness_key: None,
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
                witness_key: None,
            });
        }
    }

    None
}

/// 从 user_id 解析 session_id
///
/// 格式: `user_identifier_session_actual_session_id`
pub(super) fn parse_session_from_user_id(user_id: &str) -> Option<String> {
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
        witness_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn test_extract_session_from_claude_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "d937243f-2702-4f20-97b6-c9682235ab81".parse().unwrap(),
        );
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "d937243f-2702-4f20-97b6-c9682235ab81");
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_header_precedes_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "header-session-123".parse().unwrap(),
        );
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "session_id": "my-session-123"
            }
        });

        let result = extract_session_id(&headers, &body, "claude");

        assert_eq!(result.session_id, "header-session-123");
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    #[test]
    fn test_codex_previous_response_id_is_not_stable_session_identity() {
        let headers = HeaderMap::new();
        let body = json!({
            "input": "Write a function",
            "previous_response_id": "resp_abc123def456789"
        });

        let result = extract_session_id(&headers, &body, "codex");

        assert!(!result.session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_codex_recognizes_all_session_header_spellings() {
        let body = json!({ "input": "Write a function" });

        for header_name in ["session-id", "thread-id", "session_id", "x-session-id"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header_name,
                "d937243f-2702-4f20-97b6-c9682235ab81".parse().unwrap(),
            );

            let result = extract_session_id(&headers, &body, "codex");

            assert_eq!(
                result.session_id,
                "codex_d937243f-2702-4f20-97b6-c9682235ab81"
            );
            assert_eq!(result.source, SessionIdSource::Header);
            assert!(result.client_provided);
            // 四个拼写单独出现都不构成 thread 级隔离键：见证链路必须 fail-closed
            assert!(result.witness_key.is_none());
        }
    }

    /// Codex CLI（codex-rs）实际发送连字符拼写的 `session-id`（codex-api 的
    /// `build_session_headers`）。这个拼写必须被识别为客户端提供的稳定会话身份
    /// （日志 / 缓存键 / 官方 OAuth 透传）；见证隔离键另按 thread 级构造
    /// （需 thread-id 一起出现），只有 session-id 时见证链路 fail-closed。
    #[test]
    fn test_codex_hyphenated_session_id_header_is_a_client_identity() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "session-id",
            "8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10".parse().unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "codex");

        assert_eq!(
            result.session_id,
            "codex_8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10"
        );
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
        // 只有会话身份、没有 thread 头：见证隔离键必须缺席（fail-closed）
        assert!(result.witness_key.is_none());
    }

    /// 同时出现 session-id 与 thread-id：`session_id` 字段沿用历史优先级取
    /// session-id（会话身份，日志 / 缓存 / 官方 OAuth 透传用）；见证隔离键另算——
    /// 它是 thread 级复合，不取 session-id 单值（root 与 descendant 线程共享
    /// 同一个 session-id）。
    #[test]
    fn test_codex_splits_session_identity_from_thread_scoped_witness_key() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "session-id",
            "8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10".parse().unwrap(),
        );
        headers.insert(
            "thread-id",
            "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f".parse().unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "codex");

        assert_eq!(
            result.session_id,
            "codex_8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10"
        );
        assert_eq!(
            result.witness_key.as_deref(),
            Some(
                "codex_8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10:\
                 1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f"
            )
        );
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
    }

    /// sibling thread（同一 root session、不同 thread-id）必须得到不同的见证隔离键：
    /// 上游 response id 撞车时，一个 thread 的压缩见证不能被另一个线程命中。
    #[test]
    fn test_codex_witness_key_separates_sibling_threads_sharing_a_session() {
        let body = json!({ "input": "Write a function" });
        let extract = |thread: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                "session-id",
                "8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10".parse().unwrap(),
            );
            headers.insert("thread-id", thread.parse().unwrap());
            extract_session_id(&headers, &body, "codex").witness_key
        };

        let key_a = extract("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f");
        let key_b = extract("2d3e4f5a-6b7c-8d9e-0f1a-2b3c4d5e6f70");

        assert_eq!(
            key_a.as_deref(),
            Some(
                "codex_8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10:\
                 1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f"
            )
        );
        assert_eq!(
            key_b.as_deref(),
            Some(
                "codex_8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10:\
                 2d3e4f5a-6b7c-8d9e-0f1a-2b3c4d5e6f70"
            )
        );
        assert_ne!(
            key_a, key_b,
            "同一 session-id 下不同 thread 的见证键必须不同"
        );
    }

    /// 缺少完整组合（thread 与会话拼写）的请求没有见证隔离键：只有会话级身份
    /// （任一拼写）不能授权游标恢复——session-id 被 root 与 descendant 线程共享，
    /// 单独用它会把 sibling 线程的见证混在一起；只有 thread-id 没有会话拼写时同样
    /// fail-closed。
    #[test]
    fn test_codex_requests_without_a_complete_identity_pair_have_no_witness_key() {
        let body = json!({ "input": "Write a function" });
        let cases: &[&[&str]] = &[
            &["session-id"],
            &["session_id"],
            &["x-session-id"],
            &["thread-id"],
            &["session-id", "session_id"],
        ];

        for names in cases {
            let mut headers = HeaderMap::new();
            for name in names.iter() {
                headers.insert(
                    *name,
                    "d937243f-2702-4f20-97b6-c9682235ab81".parse().unwrap(),
                );
            }
            let result = extract_session_id(&headers, &body, "codex");
            assert!(result.client_provided);
            assert!(
                result.witness_key.is_none(),
                "{names:?} 不含 thread 与会话拼写的完整组合，不得构造见证隔离键"
            );
        }
    }

    #[test]
    fn test_grokbuild_prefers_conversation_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-grok-conv-id",
            "conv-724f4275-584e-43af-ad46-b5e7509a3ca2".parse().unwrap(),
        );
        headers.insert(
            "x-grok-session-id",
            "session-d937243f-2702-4f20-97b6-c9682235ab81"
                .parse()
                .unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "grokbuild");

        assert_eq!(
            result.session_id,
            "grokbuild_conv-724f4275-584e-43af-ad46-b5e7509a3ca2"
        );
        assert_eq!(result.source, SessionIdSource::Header);
        assert!(result.client_provided);
        // GrokBuild 没有 thread 维度：见证隔离沿用其对话身份（与 session_id 同值）
        assert_eq!(
            result.witness_key.as_deref(),
            Some("grokbuild_conv-724f4275-584e-43af-ad46-b5e7509a3ca2")
        );
    }

    #[test]
    fn test_grokbuild_falls_back_to_session_header() {
        let body = json!({ "input": "Write a function" });

        for conversation_id in ["", "                         "] {
            let mut headers = HeaderMap::new();
            headers.insert("x-grok-conv-id", conversation_id.parse().unwrap());
            headers.insert(
                "x-grok-session-id",
                "session-d937243f-2702-4f20-97b6-c9682235ab81"
                    .parse()
                    .unwrap(),
            );

            let result = extract_session_id(&headers, &body, "grokbuild");

            assert_eq!(
                result.session_id,
                "grokbuild_session-d937243f-2702-4f20-97b6-c9682235ab81"
            );
            assert_eq!(result.source, SessionIdSource::Header);
            assert!(result.client_provided);
        }
    }

    #[test]
    fn test_grokbuild_ignores_request_and_codex_session_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-grok-req-id",
            "request-724f4275-584e-43af-ad46-b5e7509a3ca2"
                .parse()
                .unwrap(),
        );
        headers.insert(
            "x-session-id",
            "codex-d937243f-2702-4f20-97b6-c9682235ab81"
                .parse()
                .unwrap(),
        );
        let body = json!({ "input": "Write a function" });

        let result = extract_session_id(&headers, &body, "grokbuild");

        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
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
