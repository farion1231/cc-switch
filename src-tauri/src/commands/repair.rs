#![allow(non_snake_case)]

use crate::codex_repair::{
    CodexAuthRepairResult, CodexRepairStatus, CodexStoreRepairResult,
};

/// Codex 修复工具：整体诊断（商店 / config.toml / auth.json）。
#[tauri::command]
pub fn codex_repair_status() -> Result<CodexRepairStatus, String> {
    crate::codex_repair::codex_repair_status().map_err(|e| e.to_string())
}

/// Codex 修复工具：修复插件商店（重新下载 openai/plugins 并清理被忽略的市场注册）。
#[tauri::command]
pub async fn repair_codex_plugin_store() -> Result<CodexStoreRepairResult, String> {
    crate::codex_repair::repair_codex_plugin_store()
        .await
        .map_err(|e| e.to_string())
}

/// Codex 修复工具：修复损坏的 auth.json（备份后写回合法 JSON）。
#[tauri::command]
pub fn repair_codex_auth_json() -> Result<CodexAuthRepairResult, String> {
    crate::codex_repair::repair_codex_auth_json().map_err(|e| e.to_string())
}
