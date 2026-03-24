//! MCP (Model Context Protocol) 服务器管理模块
//!
//! 本模块负责 MCP 服务器配置的验证、同步和导入导出。
//!
//! ## 模块结构
//!
//! - `validation` - 服务器配置验证
//! - `claude` - Claude MCP 同步和导入
//! - `codex` - Codex MCP 同步和导入（含 TOML 转换）
//! - `gemini` - Gemini MCP 同步和导入
//! - `opencode` - OpenCode MCP 同步和导入（含 local/remote 格式转换）

mod claude;
mod codex;
mod gemini;
mod importing;
mod opencode;
mod validation;

// 重新导出公共 API
pub use claude::{
    import_from_claude, remove_server_from_claude, sync_enabled_to_claude,
    sync_single_server_to_claude,
};
pub use codex::{
    import_from_codex, remove_server_from_codex, sync_enabled_to_codex, sync_single_server_to_codex,
};
pub use gemini::{
    import_from_gemini, remove_server_from_gemini, sync_enabled_to_gemini,
    sync_single_server_to_gemini,
};
pub use importing::{McpImportIssue, McpImportIssueKind, McpImportResult};
pub use opencode::{
    import_from_opencode, remove_server_from_opencode, sync_single_server_to_opencode,
};
pub(crate) use claude::parse_import_from_claude;
pub(crate) use codex::parse_import_from_codex;
pub(crate) use gemini::parse_import_from_gemini;
pub(crate) use importing::{
    apply_parsed_import, build_imported_server, conflict_issue, invalid_issue, reconcile_imported_server,
    ImportMergeAction, ParsedImport,
};
pub(crate) use opencode::parse_import_from_opencode;
pub(crate) use validation::normalize_server_spec;
