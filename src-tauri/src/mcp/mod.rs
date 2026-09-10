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
//! - `hermes` - Hermes MCP 同步和导入

mod claude;
mod codex;
mod gemini;
mod grokbuild;
mod hermes;
mod opencode;
mod validation;

pub(crate) fn replace_toml_mcp_sections(
    current: &str,
    incoming: &str,
) -> Result<String, crate::error::AppError> {
    use toml_edit::{DocumentMut, Item};

    let current_doc = if current.trim().is_empty() {
        DocumentMut::new()
    } else {
        current.parse::<DocumentMut>().map_err(|e| {
            crate::error::AppError::Message(format!("Invalid current TOML configuration: {e}"))
        })?
    };
    let mut incoming_doc = if incoming.trim().is_empty() {
        DocumentMut::new()
    } else {
        incoming.parse::<DocumentMut>().map_err(|e| {
            crate::error::AppError::Message(format!("Invalid incoming TOML configuration: {e}"))
        })?
    };

    let current_standard = current_doc.as_table().get("mcp_servers").cloned();
    incoming_doc.as_table_mut().remove("mcp_servers");
    if let Some(item) = current_standard {
        incoming_doc.as_table_mut().insert("mcp_servers", item);
    }

    let current_legacy = current_doc
        .as_table()
        .get("mcp")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("servers"))
        .cloned();

    let remove_empty_mcp = incoming_doc
        .as_table_mut()
        .get_mut("mcp")
        .and_then(Item::as_table_like_mut)
        .map(|table| {
            table.remove("servers");
            table.is_empty()
        })
        .unwrap_or(false);
    if remove_empty_mcp {
        incoming_doc.as_table_mut().remove("mcp");
    }
    if let Some(servers) = current_legacy {
        if !incoming_doc.as_table().contains_key("mcp") {
            incoming_doc["mcp"] = Item::Table(toml_edit::Table::new());
        }
        let mcp = incoming_doc
            .as_table_mut()
            .get_mut("mcp")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| {
                crate::error::AppError::Message(
                    "TOML configuration has a non-table 'mcp' value".to_string(),
                )
            })?;
        mcp.insert("servers", servers);
    }

    Ok(incoming_doc.to_string())
}

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
pub use grokbuild::{
    import_from_grokbuild, remove_server_from_grokbuild, sync_single_server_to_grokbuild,
};
pub use hermes::{import_from_hermes, remove_server_from_hermes, sync_single_server_to_hermes};
pub use opencode::{
    import_from_opencode, remove_server_from_opencode, sync_single_server_to_opencode,
};
