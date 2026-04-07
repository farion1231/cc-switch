//! Codex OAuth Tauri Commands
//!
//! 提供 OpenAI ChatGPT Plus/Pro OAuth 认证相关的 Tauri 命令。
//!
//! 注意：实际的命令通过通用 `auth_*` 命令（参见 `commands::auth`）暴露给前端，
//! 此处仅定义 State wrapper。

use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Codex OAuth 认证状态
pub struct CodexOAuthState(pub Arc<RwLock<CodexOAuthManager>>);
