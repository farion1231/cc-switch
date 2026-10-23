//! Claude Code 官方订阅账号的状态与专用命令（加号、列表、删号走授权中心的通用 `auth_*` 命令）。

use std::sync::Arc;

use crate::services::claude_account::ClaudeAccountManager;

pub struct ClaudeAccountState(pub Arc<ClaudeAccountManager>);

/// 浏览器回不到本机时，把落地页地址或登录页上的代码交给正在等待的 `claude auth login`。
#[tauri::command(rename_all = "camelCase")]
pub async fn claude_account_submit_paste(
    flow_id: String,
    pasted: String,
    state: tauri::State<'_, ClaudeAccountState>,
) -> Result<(), String> {
    state
        .0
        .submit_paste(&flow_id, &pasted)
        .await
        .map_err(|error| error.to_string())
}
