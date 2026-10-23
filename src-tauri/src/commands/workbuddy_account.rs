//! WorkBuddy 官方账号的状态与专用命令。列表、切换、删除走授权中心的通用 `auth_*` 命令
//! （provider id `workbuddy_account`），这里只放 WorkBuddy 特有的：收录当前登录、准备登录
//! 另一个账号、查运行状态、打开 WorkBuddy。

use std::sync::Arc;

use crate::commands::auth::{map_workbuddy_account, ManagedAuthAccount};
use crate::error::AppError;
use crate::services::workbuddy_account::{WorkBuddyAccountManager, WorkBuddyLiveState};

pub struct WorkBuddyAccountState(pub Arc<WorkBuddyAccountManager>);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyImportResult {
    pub account: ManagedAuthAccount,
    pub created: bool,
}

/// 账号管理器全是同步文件操作（外加 `ps` 之类的子进程），放到阻塞线程池里跑。
pub(crate) async fn run_workbuddy<T: Send + 'static>(
    manager: &Arc<WorkBuddyAccountManager>,
    job: impl FnOnce(&WorkBuddyAccountManager) -> Result<T, AppError> + Send + 'static,
) -> Result<T, String> {
    let manager = Arc::clone(manager);
    tokio::task::spawn_blocking(move || job(&manager))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn workbuddy_account_live_state(
    state: tauri::State<'_, WorkBuddyAccountState>,
) -> Result<WorkBuddyLiveState, String> {
    run_workbuddy(&state.0, |manager| Ok(manager.live_state())).await
}

/// 收录 WorkBuddy 当前登录的账号（已收录的就更新副本）。
#[tauri::command]
pub async fn workbuddy_account_import_current(
    state: tauri::State<'_, WorkBuddyAccountState>,
) -> Result<WorkBuddyImportResult, String> {
    run_workbuddy(&state.0, |manager| {
        let outcome = manager.import_current()?;
        let current = manager.status()?.current_account_id;
        Ok(WorkBuddyImportResult {
            account: map_workbuddy_account(outcome.account, current.as_deref()),
            created: outcome.created,
        })
    })
    .await
}

/// 收录当前账号后在本机写登出标记，WorkBuddy 下次打开停在登录页（不调用服务端登出）。
#[tauri::command]
pub async fn workbuddy_account_prepare_new_login(
    state: tauri::State<'_, WorkBuddyAccountState>,
) -> Result<Option<ManagedAuthAccount>, String> {
    run_workbuddy(&state.0, |manager| {
        Ok(manager
            .prepare_new_login()?
            .map(|account| map_workbuddy_account(account, None)))
    })
    .await
}

/// 打开 WorkBuddy 桌面端（让用户在里面登录）。
#[tauri::command]
pub async fn workbuddy_account_open_app() -> Result<(), String> {
    tokio::task::spawn_blocking(open_workbuddy)
        .await
        .map_err(|error| error.to_string())?
}

fn open_workbuddy() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let opened = |args: &[&str]| {
            std::process::Command::new("open")
                .args(args)
                .stdin(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        if opened(&["-b", "com.tencent.workbuddy.mac"]) || opened(&["-a", "WorkBuddy"]) {
            return Ok(());
        }
    }
    // 推断的安装位置（未实测）：%LOCALAPPDATA%\Programs\WorkBuddy\WorkBuddy.exe
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let exe = std::path::PathBuf::from(local)
                .join("Programs")
                .join("WorkBuddy")
                .join("WorkBuddy.exe");
            if exe.exists() && std::process::Command::new(&exe).spawn().is_ok() {
                return Ok(());
            }
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        if std::process::Command::new("workbuddy")
            .stdin(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(AppError::localized(
        "workbuddy.account.open_failed",
        "没能打开 WorkBuddy，请手动打开。",
        "Couldn't open WorkBuddy. Open it manually.",
    )
    .to_string())
}
