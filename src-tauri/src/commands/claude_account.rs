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

/// 「切卡时切换 Claude Code 的登录」开关的状态。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLoginSwitchStatus {
    pub enabled: bool,
    /// 开关开着、没被挡住：切卡时真的会切换登录
    pub active: bool,
    /// 确认过风险说明（第一次打开时）
    pub risk_acknowledged: bool,
    /// 不能打开的原因：`"overrideDir"`（设了 Claude 配置目录覆盖）
    pub blocked: Option<String>,
    /// Claude Code 现在登录的账号 key（按 `~/.claude.json` 的资料）
    pub live: Option<String>,
}

fn login_switch_status(data_dir: &std::path::Path) -> ClaudeLoginSwitchStatus {
    use crate::services::claude_account::snapshot;
    let state = snapshot::load_state(data_dir);
    let blocked = snapshot::blocked_reason();
    ClaudeLoginSwitchStatus {
        enabled: state.enabled,
        active: state.enabled && blocked.is_none(),
        risk_acknowledged: state.risk_ack_at.is_some(),
        blocked: blocked.map(str::to_string),
        live: snapshot::default_owner(&snapshot::store::Slot::default_slot()),
    }
}

#[tauri::command]
pub async fn get_claude_login_switch() -> Result<ClaudeLoginSwitchStatus, String> {
    let data_dir = crate::config::get_app_config_dir();
    tokio::task::spawn_blocking(move || login_switch_status(&data_dir))
        .await
        .map_err(|error| error.to_string())
}

/// 打开或关掉「切卡时切换 Claude Code 的登录」。打开时（界面已确认风险说明）立刻按当前直连的
/// 卡切一次；切不过去就保持关闭并报错。关掉不动任何登录：默认槽位里是谁就还是谁。
#[tauri::command]
pub async fn set_claude_login_switch(
    enabled: bool,
    state: tauri::State<'_, crate::store::AppState>,
    accounts: tauri::State<'_, ClaudeAccountState>,
) -> Result<ClaudeLoginSwitchStatus, String> {
    use crate::services::claude_account::snapshot;
    let data_dir = crate::config::get_app_config_dir();
    if enabled {
        if let Some(reason) = snapshot::blocked_reason() {
            return Err(format!("blocked: {reason}"));
        }
    }
    // 只改开关的字段：切换、恢复可能同时在改切换日志
    snapshot::update_state(&data_dir, |current| {
        current.enabled = enabled;
        if enabled && current.risk_ack_at.is_none() {
            current.risk_ack_at = Some(chrono::Utc::now().timestamp());
        }
    })
    .map_err(|error| error.to_string())?;
    if enabled {
        if let Err(error) =
            crate::services::provider::claude_official::refresh_live_after_account_change(
                state.inner(),
                &accounts.0,
            )
            .await
        {
            let _ = snapshot::update_state(&data_dir, |current| current.enabled = false);
            return Err(error);
        }
    }
    let data_dir_for_status = data_dir.clone();
    tokio::task::spawn_blocking(move || login_switch_status(&data_dir_for_status))
        .await
        .map_err(|error| error.to_string())
}
