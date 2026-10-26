//! 切卡时切换 Claude Code 的登录。
//!
//! 每个账号仍有自己的 `CLAUDE_CONFIG_DIR`（登录、「打开终端」都照旧）。切到绑了账号 T 的
//! 官方卡时，把 T 的登录「移」进默认槽位：之后任何地方新启动的 `claude`（终端、
//! IDE 插件、脚本）都是 T，不用设环境变量。
//!
//! 一次切换（[`check_out`]）：
//! 1. 收回：默认槽位里现在是谁（O），就把它的账号键写回 O 的账号槽位；O 还不是 CC Switch
//!    管的账号时，先给它建一个账号目录（[`Adopter`]）。
//! 2. 放入：默认槽位 = 原来的共享键（MCP 的 OAuth 等，属于这台电脑）+ T 的账号键。这一步
//!    是提交点，在 Claude Code 的写锁里先比对再写（期间被 Claude Code 改过就放弃）。
//! 3. 默认的 `.claude.json`：`oauthAccount` 换成 T 的，删掉跟账号走的缓存。
//! 4. 清空 T 的账号槽位：同一个号的令牌只留一份，免得两份各自刷新、互相作废。
//!
//! 收回时 O 的账号槽位里如果还有另一份能用的登录（同一个号的另一次登录，或者认错了号），
//! 先把它挪到备份槽位（[`backup_dir`]）留一份，不直接覆盖掉；T 的槽位没有能用的登录时，
//! 切换会改用它的备份。
//!
//! 全程持有 Claude Code 自己的刷新锁（挡住它这时刷新令牌）。每一步之前先把进度写进切换日志
//! （[`Journal`]，只有指纹，没有令牌），崩溃后下次启动或下次切换时补完（[`recover`]）。
//!
//! 不联网、不刷新令牌、不登出；令牌只在 Claude Code 的槽位格式里（钥匙串 / `.credentials.json`）
//! 移动，CC Switch 自己的文件里只有指纹和过期时间。

mod creds;
mod files;
mod recover;
mod state;
pub mod store;
mod switch;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use super::identity;
use crate::error::AppError;
use store::{CredentialStore, Slot};

pub use files::backup_dir;
pub use recover::recover;
pub use state::{confirm, load_state, update_state, Journal, State, Step};
pub use switch::{check_out, replace_live, vacate};

#[cfg(test)]
use creds::{account_fingerprint, login_expiry, merge};
#[cfg(test)]
use state::save_state;
#[cfg(test)]
use store::Secret;

/// 跟账号走的凭据键：切换时整组搬走，其余键（`mcpOAuth`、`pluginSecrets` 等）留在槽位里。
/// 和 Claude Code `/login` 删除的键一致（2.1.309 复核；后两个是 2.1.302 起新增的）。
pub const ACCOUNT_KEYS: &[&str] = &[
    "claudeAiOauth",
    "organizationUuid",
    "trustedDeviceToken",
    "enterpriseGateway",
    "designOauth",
    "trustedDeviceEnrolledAfterRefusalAtMs",
    "cloudSessionsOauth",
];

/// 默认 `.claude.json` 里跟账号走的缓存：和 Claude Code 登出时清的一致（2.1.309），换号后
/// 由它重新拉。
pub const ACCOUNT_CACHE_KEYS: &[&str] = &[
    "additionalModelOptionsCache",
    "additionalModelOptionsAnsweredAt",
    "additionalModelCostsCache",
    "modelAccessCache",
    "orgModelDefaultCache",
    "cachedArtifactRoster",
    "artifactRosterDenied",
    "lastSeenOrgDefaultUpdatedAt",
    "clientDataCache",
    "clientDataCacheSlots",
    "autoCompactWindowsCache",
    "cachedUsageUtilization",
    "metricsStatusCache",
    "metricsStatusCacheByPrincipal",
    "githubWebConnectionStatusCache",
    "unusedLimitResetStatusCache",
    "startupPrefetchedAt",
    "cachedExtraUsageDisabledReason",
    "penguinModeOrgEnabled",
];

/// 刷新锁持有到提交点的上限：远小于 Claude Code 判它过期的 60 秒（我们不续命，超了它会
/// 当死锁抢走、照常刷新）。
const COMMIT_DEADLINE: Duration = Duration::from_secs(30);

/// 同一进程里的切换串行。
static SWITCH: Mutex<()> = Mutex::new(());

/// 切卡时为什么不能切换登录：设了 Claude 配置目录覆盖（默认槽位不在 `~/.claude`，例如指向
/// WSL）。这时绑号卡只能从卡片「打开终端」使用。
pub fn blocked_reason() -> Option<&'static str> {
    crate::settings::get_claude_override_dir().map(|_| "overrideDir")
}

/// 切卡时会不会切换登录。
pub fn active() -> bool {
    blocked_reason().is_none()
}

/// 默认槽位现在登的是不是账号 `account`（被移进去以后它的账号目录是空的，用它要走默认目录）。
pub fn is_live(account: &str) -> bool {
    default_owner(&Slot::default_slot()).as_deref() == Some(account)
}

/// 默认槽位现在登的是哪个账号（只看 `.claude.json` 的资料，不读凭据）。
pub fn default_owner(default: &Slot) -> Option<String> {
    let profile = files::read_profile(&default.global_config())?;
    identity::identity_from_profile(&profile).map(|found| found.key)
}

/// 账号 `account` 现在的登录在哪个槽位，查额度用：它在默认槽位里就是默认槽位，否则是它自己
/// 的账号目录。本机没有这个账号时为 `None`。
pub fn account_slot(data_dir: &Path, account: &str) -> Option<Slot> {
    if is_live(account) {
        return Some(Slot::default_slot());
    }
    crate::services::claude_account::config_dir_on_disk(data_dir, account).map(Slot::Account)
}

/// 账号目录里有没有能用的登录（读凭据判断；钥匙串条目在、里面只剩 MCP 授权也算没有）。
pub fn dir_has_login(config_dir: &Path) -> bool {
    creds::read_login(
        store::platform_store().as_ref(),
        &Slot::Account(config_dir.to_path_buf()),
    )
    .ok()
    .flatten()
    .is_some()
}

/// 凭据里 Claude Code 记的套餐。
pub fn credential_plan(secret: &store::Secret) -> crate::services::claude_account::AccountPlan {
    creds::credential_plan(secret)
}

/// 账号目录里的登录记的套餐；读不到凭据时为 `None`。
pub fn dir_plan(config_dir: &Path) -> Option<crate::services::claude_account::AccountPlan> {
    let secret = store::platform_store()
        .read(&Slot::Account(config_dir.to_path_buf()))
        .ok()??;
    Some(creds::credential_plan(&secret))
}

/// 刚登录完的账号：从它的槽位记下登录过期时间（尽力而为）。
pub fn note_login_expiry(data_dir: &Path, account: &str, config_dir: &Path) {
    let store = store::platform_store();
    let Ok(Some(secret)) = store.read(&Slot::Account(config_dir.to_path_buf())) else {
        return;
    };
    let _ = update_state(data_dir, |state| {
        creds::note_expiry(state, account, &secret)
    });
}

/// 默认槽位里的登录属于一个还没收录的账号时，给它建账号目录并收录，返回目录。
/// `profile` 是默认 `.claude.json` 里的 `oauthAccount`。
pub trait Adopter {
    fn adopt(&self, identity: &identity::Identity, profile: &Value) -> Result<PathBuf, AppError>;
}

/// 账号 key → 本机账号目录（已收录的）。
pub trait Accounts {
    fn dir_of(&self, account: &str) -> Option<PathBuf>;
}

pub struct Ctx<'a> {
    pub data_dir: &'a Path,
    /// 默认槽位
    pub default: Slot,
    pub store: &'a dyn CredentialStore,
    pub accounts: &'a dyn Accounts,
    pub adopter: &'a dyn Adopter,
}

/// 切换的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 默认槽位里本来就是 T
    AlreadyLive,
    Switched {
        /// 原来默认槽位里的号刚被收录成新账号（第一次切换时常见）
        adopted: Option<String>,
        /// 原来默认槽位里的号和它被收回到的目录：后面的步骤失败时用它切回去
        previous: Option<(String, PathBuf)>,
    },
}

#[cfg(test)]
mod tests;
