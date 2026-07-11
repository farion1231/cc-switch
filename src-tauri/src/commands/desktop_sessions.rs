#![allow(non_snake_case)]

use crate::desktop_sessions::{self, DesktopSessionAccount, MigrateReport};

/// 列出 Claude Desktop 各账号/组织下的会话分组（含当前登录标记）。
#[tauri::command]
pub async fn list_desktop_session_accounts() -> Result<Vec<DesktopSessionAccount>, String> {
    tauri::async_runtime::spawn_blocking(desktop_sessions::list_accounts)
        .await
        .map_err(|e| format!("扫描 Claude Desktop 会话失败: {e}"))?
        .map_err(|e| e.to_string())
}

/// 把来源账号的会话迁移（非破坏性复制）到目标账号；`toAccount` 缺省为当前登录账号。
/// `dryRun` 为真时只预览、不写入。
#[tauri::command]
pub async fn migrate_desktop_sessions(
    fromAccount: String,
    fromOrg: Option<String>,
    toAccount: Option<String>,
    toOrg: Option<String>,
    dryRun: bool,
) -> Result<MigrateReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_sessions::migrate(
            &fromAccount,
            fromOrg.as_deref(),
            toAccount.as_deref(),
            toOrg.as_deref(),
            dryRun,
        )
    })
    .await
    .map_err(|e| format!("迁移 Claude Desktop 会话失败: {e}"))?
    .map_err(|e| e.to_string())
}
