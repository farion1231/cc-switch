//! CodeBuddy Code 官方账号的状态与专用命令（列号、切号、删号走授权中心的通用 `auth_*` 命令）。

use std::sync::Arc;

use crate::commands::auth::{map_codebuddy_account, ManagedAuthAccount};
use crate::services::codebuddy_account::{
    CodebuddyAccountManager, CodebuddyLiveSummary, CodebuddyQuota,
};

pub struct CodebuddyAccountState(pub Arc<CodebuddyAccountManager>);

/// 终端里跑的命令：用户在里面执行 `/login`。优先用解析出的绝对路径，绕开终端 PATH 里
/// 已失效的版本管理器 shim；找不到安装时退回裸命令名。
fn login_command() -> String {
    let program = crate::commands::misc::locate_cli_for_terminal("codebuddy");
    login_command_for(program.as_deref())
}

#[cfg(target_os = "windows")]
fn login_command_for(program: Option<&std::path::Path>) -> String {
    match program {
        Some(path) => format!("call \"{}\"", path.display()),
        None => "call codebuddy".into(),
    }
}

#[cfg(not(target_os = "windows"))]
fn login_command_for(program: Option<&std::path::Path>) -> String {
    // 单引号包住，内部的单引号按 '\'' 转义
    fn quote(value: &std::path::Path) -> String {
        format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
    }
    match program {
        // `codebuddy` 是 `#!/usr/bin/env node` 脚本：把它所在目录（同一个 Node 安装的 bin）
        // 放到 PATH 最前，`node` 也不会再落到失效的 shim 上
        Some(path) => match path.parent() {
            Some(dir) => format!("PATH={}:\"$PATH\" {}", quote(dir), quote(path)),
            None => quote(path),
        },
        None => "codebuddy".into(),
    }
}

/// 打开用户首选终端运行 `codebuddy`，让用户执行 `/login`。
pub(crate) async fn open_codebuddy_login_terminal() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        crate::commands::misc::launch_terminal_running(&login_command(), "codebuddy_login")
    })
    .await
    .map_err(|e| format!("launch task join error: {e}"))?
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::login_command_for;
    use std::path::Path;

    #[test]
    fn codebuddy_login_command_quotes_the_resolved_path() {
        assert_eq!(login_command_for(None), "codebuddy");
        assert_eq!(
            login_command_for(Some(Path::new("/opt/node/bin/codebuddy"))),
            "PATH='/opt/node/bin':\"$PATH\" '/opt/node/bin/codebuddy'"
        );
        assert_eq!(
            login_command_for(Some(Path::new("/tmp/it's/codebuddy"))),
            "PATH='/tmp/it'\\''s':\"$PATH\" '/tmp/it'\\''s/codebuddy'"
        );
    }
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

/// 授权中心账号行的套餐与积分：用这个账号副本里的令牌只读查询，不切号、不续期。
/// 5 分钟内重复查询走缓存，`force` 为 true（手动刷新）时跳过缓存。
#[tauri::command(rename_all = "camelCase")]
pub async fn codebuddy_account_quota(
    account_id: String,
    lang: Option<String>,
    force: Option<bool>,
    state: tauri::State<'_, CodebuddyAccountState>,
) -> Result<CodebuddyQuota, String> {
    state
        .0
        .quota(
            &account_id,
            lang.as_deref().unwrap_or("zh"),
            force.unwrap_or(false),
        )
        .await
        .map_err(|e| e.to_string())
}
