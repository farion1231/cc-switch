//! Deep link import functionality for CC Switch
//!
//! This module implements the ccswitch:// protocol for importing configurations
//! via deep links. Supports importing:
//! - Provider configurations (Claude/Codex/Gemini)
//! - MCP server configurations
//! - Prompts
//! - Skills
//!

mod codex_link;
mod grok_link;
mod icon_fetch;
mod mcp;
mod parser;
mod prompt;
mod provider;
mod skill;
mod utils;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// Re-export public API
pub use icon_fetch::fetch_deeplink_icon;
pub use mcp::import_mcp_from_deeplink;
pub use parser::parse_deeplink_url;
pub use prompt::import_prompt_from_deeplink;
pub use provider::{import_provider_from_deeplink, parse_and_merge_config};
pub use skill::import_skill_from_deeplink;

/// 等前端处理的深链接（解析结果或解析错误）
///
/// 链接经常在前端还没注册监听时到达：冷启动、轻量模式重建窗口。事件一发出去就没了，
/// 所以先存在这里，再发 `deeplink-pending` 通知；前端在挂载时和收到通知时都调
/// `peek_pending_deeplink` 读一次，真正显示之后再按 `id` 调 `ack_pending_deeplink` 清除。
///
/// 读取不清除：前端读到之后、显示之前可能被打断（启动时校正语言会重建 effect、窗口被销毁），
/// 读即清的话链接就丢了。同一条链接收到多次通知，前端按 `id` 去重，只弹一次框。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PendingDeepLink {
    Import { request: Box<DeepLinkImportRequest> },
    Error { url: String, error: String },
}

/// 带编号的待处理深链接，编号用于确认清除和前端去重
#[derive(Debug, Clone, Serialize)]
pub struct PendingDeepLinkEntry {
    pub id: u64,
    pub link: PendingDeepLink,
}

static PENDING_DEEPLINK: Mutex<Option<PendingDeepLinkEntry>> = Mutex::new(None);
static NEXT_PENDING_ID: AtomicU64 = AtomicU64::new(1);

fn pending_slot() -> std::sync::MutexGuard<'static, Option<PendingDeepLinkEntry>> {
    PENDING_DEEPLINK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 暂存一条待处理的深链接，覆盖还没处理的旧链接（只保留最新一条）
pub fn set_pending_deeplink(link: PendingDeepLink) {
    let id = NEXT_PENDING_ID.fetch_add(1, Ordering::Relaxed);
    *pending_slot() = Some(PendingDeepLinkEntry { id, link });
}

/// 读取待处理的深链接，不清除
pub fn peek_pending_deeplink() -> Option<PendingDeepLinkEntry> {
    pending_slot().clone()
}

/// 前端显示之后确认清除；编号对不上（期间来了新链接）就不动
pub fn ack_pending_deeplink(id: u64) {
    let mut slot = pending_slot();
    if slot.as_ref().is_some_and(|entry| entry.id == id) {
        *slot = None;
    }
}

/// Deep link import request model
///
/// Represents a parsed ccswitch:// URL ready for processing.
/// This struct contains all possible fields for all resource types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkImportRequest {
    /// Protocol version (e.g., "v1")
    pub version: String,
    /// Resource type to import: "provider" | "prompt" | "mcp" | "skill"
    pub resource: String,

    // ============ Common fields ============
    /// Target application (claude/codex/gemini) - for provider, prompt, skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Resource name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 导入后是否立即生效：供应商（切换式应用）切过去、累加式应用写进配置文件、提示词启用。
    /// 确认框里有对应开关，链接里的值只是开关的初始值，导入时以开关为准。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    // ============ Provider-specific fields ============
    /// Provider homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// API endpoint/base URL (supports comma-separated multiple URLs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional built-in provider icon name; anything else is dropped by the parser
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// 供应商图标的下载地址（https）。确认框打开时由 `fetch_deeplink_icon` 下载、前端重新编码后
    /// 放进 `icon`；导入本身从不下载它，下载失败就用 `icon` 里的内置图标或首字母
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Optional model name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional notes/description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Optional Haiku model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku_model: Option<String>,
    /// Optional Sonnet model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet_model: Option<String>,
    /// Optional Opus model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_model: Option<String>,
    /// 供应商的接口格式：`anthropic` | `openai_chat` | `openai_responses` | `gemini_native`，
    /// 按应用换算成各自的字段（见 `provider::ApiFormat`）。没写时用各应用的默认格式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,

    // ============ Prompt-specific fields ============
    /// Base64 encoded Markdown content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    // ============ MCP-specific fields ============
    /// Target applications for MCP (comma-separated: "claude,codex,gemini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<String>,

    // ============ Skill-specific fields ============
    /// GitHub repository (format: "owner/name")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Skill directory name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    // ============ Config file fields (v3.8+) ============
    /// Base64 encoded config content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Config format (json/toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_format: Option<String>,
    /// Remote config URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_url: Option<String>,

    // ============ Usage script fields (v3.9+) ============
    /// Whether to enable usage query. When omitted, defaults to enabled if the link
    /// carries a usage script, disabled otherwise; `usageEnabled=false` opts out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_enabled: Option<bool>,
    /// Base64 encoded usage query script code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<String>,
    /// Usage query API key (if different from provider API key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_api_key: Option<String>,
    /// Usage query base URL (if different from provider endpoint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_base_url: Option<String>,
    /// Usage query access token (for NewAPI template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_access_token: Option<String>,
    /// Usage query user ID (for NewAPI template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_user_id: Option<String>,
    /// Auto query interval in minutes (0 to disable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_auto_interval: Option<u64>,
}
