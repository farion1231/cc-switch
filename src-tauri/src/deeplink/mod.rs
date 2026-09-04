//! Deep link import functionality for CC Switch
//!
//! This module implements the ccswitch:// protocol for importing configurations
//! via deep links. Supports importing:
//! - Provider configurations (Claude/Codex/Gemini)
//! - MCP server configurations
//! - Prompts
//! - Skills
//!

mod mcp;
mod parser;
mod prompt;
mod provider;
mod skill;
mod utils;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

// Re-export public API
pub use mcp::import_mcp_from_deeplink;
pub use parser::parse_deeplink_url;
pub use prompt::import_prompt_from_deeplink;
pub use provider::{import_provider_from_deeplink, parse_and_merge_config};
pub use skill::import_skill_from_deeplink;

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
    /// Whether to enable after import (default: false)
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
    /// Optional provider icon name (maps to built-in SVG)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
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
    /// Whether to enable usage query. Defaults to **disabled** — carrying a script
    /// is not itself a decision to run it; the link must say `usageEnabled=true`.
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

    // ============ Grok Build inline config fields ============
    /// Sanitized model catalog extracted from an inline Grok Build config
    /// (`grokbuild_models`). Deeplink-internal only: populated by config
    /// merging, never parsed from or serialized to the URL itself.
    #[serde(skip)]
    pub grokbuild_models: Option<GrokDeeplinkModels>,
}

/// 从 deeplink 内联 config.toml 提取的 Grok Build 模型目录（已消毒）。
///
/// 只保留可安全透传的展示/容量字段。`api_key` / `env_key` / `base_url` /
/// `api_backend` 有意不进入这里：导入结果必须统一使用 deeplink 显式端点与
/// 密钥（或既有的合并回退），不能让不可信链接按档案夹带凭据或私有 backend。
#[derive(Debug, Clone, PartialEq)]
pub struct GrokDeeplinkModels {
    /// 内联 config 原有的 `[models] default`（指向档案键）
    pub default_profile: String,
    /// 全部 `[model.*]` 档案
    pub profiles: Vec<GrokDeeplinkModel>,
}

/// 单个模型档案的安全字段（`[model.<profile>]` 表）。
#[derive(Debug, Clone, PartialEq)]
pub struct GrokDeeplinkModel {
    /// `[model.<profile>]` 表键，`[models] default` 指向的就是它
    pub profile: String,
    /// 模型 id（`model` 字段）；缺省回落到档案键
    pub model: String,
    /// 展示名（`name` 字段）；缺省回落到档案键
    pub name: String,
    /// 可选模型描述（`description` 字段）
    pub description: Option<String>,
    /// 上下文窗口；缺失或非法时回落到 `DEFAULT_CONTEXT_WINDOW`
    pub context_window: i64,
}
