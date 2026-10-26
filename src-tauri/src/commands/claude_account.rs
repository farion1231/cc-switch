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

/// 绑了账号的 Claude 官方卡的额度：按那个账号自己的登录查，带上它的套餐。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_claude_account_quota(
    account_id: String,
) -> Result<crate::services::subscription::ClaudeAccountQuota, String> {
    crate::services::subscription::get_claude_account_quota(
        crate::config::get_app_config_dir(),
        account_id,
    )
    .await
}

/// Claude Code 默认登录的套餐（没绑账号的官方卡显示）；没登录或凭据里没写时为 `None`。
#[tauri::command]
pub async fn get_claude_default_plan(
) -> Result<Option<crate::services::claude_account::AccountPlan>, String> {
    tokio::task::spawn_blocking(crate::services::subscription::read_claude_default_plan)
        .await
        .map_err(|error| error.to_string())
}

/// 切卡时切换 Claude Code 登录的状态。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLoginSwitchStatus {
    /// 切到绑号卡时会切换登录（没被挡住）
    pub active: bool,
    /// 不切换登录的原因：`"overrideDir"`（设了 Claude 配置目录覆盖）
    pub blocked: Option<String>,
    /// Claude Code 现在登录的账号 key（按 `~/.claude.json` 的资料）
    pub live: Option<String>,
}

fn login_switch_status() -> ClaudeLoginSwitchStatus {
    use crate::services::claude_account::snapshot;
    let blocked = snapshot::blocked_reason();
    ClaudeLoginSwitchStatus {
        active: blocked.is_none(),
        blocked: blocked.map(str::to_string),
        live: snapshot::default_owner(&snapshot::store::Slot::default_slot()),
    }
}

#[tauri::command]
pub async fn get_claude_login_switch() -> Result<ClaudeLoginSwitchStatus, String> {
    tokio::task::spawn_blocking(login_switch_status)
        .await
        .map_err(|error| error.to_string())
}
