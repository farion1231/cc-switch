//! CodeBuddy Code 官方账号的状态与专用命令（列号、切号、删号走授权中心的通用 `auth_*` 命令）。

use std::sync::Arc;

use crate::commands::auth::{map_codebuddy_account, ManagedAuthAccount};
use crate::services::codebuddy_account::{CodebuddyAccountManager, CodebuddyLiveSummary};

pub struct CodebuddyAccountState(pub Arc<CodebuddyAccountManager>);

/// 终端里跑的命令：用户在里面执行 `/login`。硬编码，可信。
#[cfg(target_os = "windows")]
const LOGIN_COMMAND: &str = "call codebuddy";
#[cfg(not(target_os = "windows"))]
const LOGIN_COMMAND: &str = "codebuddy";

/// 打开用户首选终端运行 `codebuddy`，让用户执行 `/login`。
pub(crate) async fn open_codebuddy_login_terminal() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        crate::commands::misc::launch_terminal_running(LOGIN_COMMAND, "codebuddy_login")
    })
    .await
    .map_err(|e| format!("launch task join error: {e}"))?
}

/// CodeBuddy 凭据文件现在的状态：没登录 / 已登出 / 登着已收录或未收录的号 / 格式认不出。
#[tauri::command]
pub async fn codebuddy_account_live(
    state: tauri::State<'_, CodebuddyAccountState>,
) -> Result<CodebuddyLiveSummary, String> {
    state.0.live_summary().await.map_err(|e| e.to_string())
}

/// 收录 CodeBuddy 当前登录的账号（已收录就更新副本）。
#[tauri::command]
pub async fn codebuddy_account_import_current(
    state: tauri::State<'_, CodebuddyAccountState>,
) -> Result<ManagedAuthAccount, String> {
    let account = state.0.import_current().await.map_err(|e| e.to_string())?;
    // 刚收录的就是 CodeBuddy 现在登着的号
    let current = Some(account.id.clone());
    Ok(map_codebuddy_account(account, current.as_deref()))
}

/// 登录等待中「重新打开终端」：只再开一个终端，不重置这次登录。
#[tauri::command]
pub async fn codebuddy_account_open_login_terminal() -> Result<(), String> {
    open_codebuddy_login_terminal().await
}
