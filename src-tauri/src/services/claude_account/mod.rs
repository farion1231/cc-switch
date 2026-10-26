//! Claude Code 的官方订阅账号：每个账号一个独立的 `CLAUDE_CONFIG_DIR`（官方推荐的多账号用法）。
//!
//! - 加号：在新账号目录里运行官方的 `claude auth login`（见 `login`），令牌由 Claude Code
//!   存进它自己的地方（macOS 钥匙串里按目录区分的条目，其他平台是目录里的 `.credentials.json`）；
//! - 认人：只读 `.claude.json` 的 `oauthAccount` 这类非机密字段（见 `identity`）；
//! - 用号：从卡片打开终端时设 `CLAUDE_CONFIG_DIR=<账号目录>`；切到绑了账号的官方卡时，
//!   把账号的登录移进默认槽位（见 `snapshot`，这是唯一经手令牌的地方，不另存、不刷新）；
//! - 删号：删目录，并按 Claude Code 的命名删掉钥匙串里对应的条目（只删除，不吊销、不登出）。
//!
//! 用户原来的 `~/.claude` 是「默认登录」，不在这里管理，也永远不会被当作账号目录。
//! 账号表 `claude_accounts.json` 只在本机（目录是本机路径），不随云同步；卡片上的绑定存的是
//! 账号 key（`accountUuid:organizationUuid`），同步到别的设备后按 key 在那台电脑上找账号。

pub mod cc_lock;
pub mod identity;
pub mod layout;
pub mod login;
pub mod shared;
pub mod snapshot;
pub mod usage;

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};

use crate::error::AppError;
use login::RunningLogin;

const STORE_FILENAME: &str = "claude_accounts.json";
/// 浏览器登录最多等这么久，过了就当放弃。
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// 登录后问一次 `claude auth status` 的时限。
const STATUS_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountData {
    local_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    org_name: Option<String>,
    /// 套餐（pro / max / team …）：登录时按 `claude auth status` 记，之后按凭据里的更新
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    /// 凭据里的 `rateLimitTier`（如 `default_claude_max_20x`），区分 Max 5x / 20x
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rate_limit_tier: Option<String>,
    added_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    /// 账号 key → 本机数据
    #[serde(default)]
    accounts: BTreeMap<String, AccountData>,
}

/// 给界面看的账号。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    /// 账号 key（`accountUuid:organizationUuid`），卡片绑定存的就是它
    pub id: String,
    pub email: Option<String>,
    pub org_name: Option<String>,
    pub plan: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub authenticated_at: i64,
    /// 目录没了或里面的登录没了，只能重新登录
    pub requires_reauth: bool,
    /// 不在默认槽位时，登录约什么时候过期（毫秒，上次记下的）
    pub login_expires_at: Option<i64>,
}

impl ClaudeAccount {
    /// 界面上显示的名字：邮箱 > 组织名 > `Claude (<key 前 6 位>)`。
    pub fn label(&self) -> String {
        self.email
            .clone()
            .or_else(|| self.org_name.clone())
            .unwrap_or_else(|| {
                let short: String = self.id.chars().take(6).collect();
                format!("Claude ({short})")
            })
    }
}

pub struct LoginStart {
    pub flow_id: String,
    pub auth_url: String,
    pub expires_in: u64,
    /// Claude Code 自己已经打开了浏览器
    pub browser_opened: bool,
}

/// 登录进程结束之后的流程状态。
enum FlowEnd {
    /// 正在收录（读套餐、同步共享层、写账号表）
    Finishing,
    /// 已收录。命令层还要刷新 live（可能等切换锁），这期间的轮询照样拿到这个结果
    Added(ClaudeAccount),
}

struct PendingLogin {
    local_id: String,
    login: RunningLogin,
}

pub struct ClaudeAccountManager {
    data_dir: PathBuf,
    /// 账号表；同时充当所有改动的互斥锁
    store: Mutex<StoreFile>,
    pending: Mutex<HashMap<String, PendingLogin>>,
    /// 登录进程已经结束的流程。前端每 1.5 秒轮询一次、不等上一次返回：收录期间来的轮询
    /// 回「还没好」，收录完的回同一个账号，都不能报「登录已结束」。下次加号时清空。
    ended: std::sync::Mutex<HashMap<String, FlowEnd>>,
}

/// 账号表文件的进程内互斥：管理器和切换（收录默认槽位里原来的登录）都会改它。
/// 改之前先从磁盘重读，改完立刻落盘，中间不跨 await。
static STORE_FILE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn store_file_guard() -> std::sync::MutexGuard<'static, ()> {
    STORE_FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn persist_store(data_dir: &Path, store: &StoreFile) -> Result<(), AppError> {
    let snapshot = StoreFile {
        version: 1,
        accounts: store.accounts.clone(),
    };
    let content = serde_json::to_vec_pretty(&snapshot)
        .map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::atomic_write_private(&data_dir.join(STORE_FILENAME), &content)
}

fn load_store(data_dir: &Path) -> StoreFile {
    let path = data_dir.join(STORE_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            log::warn!("[ClaudeAccount] 账号文件解析失败，按空表处理: {error}");
            StoreFile::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreFile::default(),
        Err(error) => {
            log::warn!("[ClaudeAccount] 账号文件读取失败，按空表处理: {error}");
            StoreFile::default()
        }
    }
}

/// 同步一个账号目录的共享层（见 [`shared`]）。失败只记日志：共享不上不该挡住用号。
pub fn sync_shared(config_dir: &Path) {
    let Some(home) = config_dir.parent() else {
        return;
    };
    if let Err(error) = shared::sync_account(&shared::Defaults::current(), config_dir, home) {
        log::warn!("[ClaudeAccount] 同步共享层失败: {error}");
    }
}

/// 同步本机全部账号目录的共享层（启动时、MCP 同步后）。
pub fn sync_all_shared(data_dir: &Path) {
    let store = load_store(data_dir);
    for data in store.accounts.values() {
        if !layout::valid_local_id(&data.local_id) {
            continue;
        }
        sync_shared(&layout::config_dir(data_dir, &data.local_id));
    }
}

/// 不经管理器、直接按磁盘上的账号表解析账号目录（写 live 的路径上拿不到 Tauri 状态）。
/// 账号表每次改动都会落盘，所以和管理器看到的一致。
pub fn config_dir_on_disk(data_dir: &Path, account_id: &str) -> Option<PathBuf> {
    let store = load_store(data_dir);
    let data = store.accounts.get(account_id)?;
    if !layout::valid_local_id(&data.local_id) {
        return None;
    }
    let dir = layout::config_dir(data_dir, &data.local_id);
    layout::assert_config_dir(data_dir, &dir).ok()?;
    dir.is_dir().then_some(dir)
}

/// 账号的订阅套餐：Claude Code 写在凭据 `claudeAiOauth` 里，刷新登录时按账号资料更新。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlan {
    /// `pro` / `max` / `team` / `enterprise`
    pub subscription_type: Option<String>,
    /// 如 `default_claude_max_5x`、`default_claude_max_20x`
    pub rate_limit_tier: Option<String>,
}

/// 把从凭据读到的套餐记进账号（尽力而为）：账号列表靠它显示套餐。凭据里没写套餐时不动原记录。
pub fn note_plan(data_dir: &Path, account: &str, plan: &AccountPlan) {
    if plan.subscription_type.is_none() {
        return;
    }
    let _file = store_file_guard();
    let mut store = load_store(data_dir);
    let Some(data) = store.accounts.get_mut(account) else {
        return;
    };
    if data.plan == plan.subscription_type && data.rate_limit_tier == plan.rate_limit_tier {
        return;
    }
    data.plan = plan.subscription_type.clone();
    data.rate_limit_tier = plan.rate_limit_tier.clone();
    if let Err(error) = persist_store(data_dir, &store) {
        log::warn!("[ClaudeAccount] 记录账号套餐失败: {error}");
    }
}

impl ClaudeAccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let store = load_store(&data_dir);
        Self {
            data_dir,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
            ended: std::sync::Mutex::new(HashMap::new()),
        }
    }

    // ---------- 加号 ----------

    /// 新建账号目录，在里面启动 `claude auth login`，拿到登录链接后返回。
    pub async fn start_login(&self) -> Result<LoginStart, AppError> {
        let local_id = layout::new_local_id();
        let home = layout::account_home(&self.data_dir, &local_id);
        let config_dir = layout::config_dir(&self.data_dir, &local_id);
        layout::assert_config_dir(&self.data_dir, &config_dir)?;
        create_private_dir(&config_dir)?;
        {
            // 登录前先接好共享目录，Claude Code 首次启动就写进共享的位置
            let config_dir = config_dir.clone();
            let _ = tokio::task::spawn_blocking(move || sync_shared(&config_dir)).await;
        }

        let login = match login::start(config_dir, home, LOGIN_TIMEOUT).await {
            Ok(login) => login,
            Err(error) => {
                self.discard_dir(&local_id).await;
                return Err(error);
            }
        };
        let flow_id = local_id.clone();
        let auth_url = login.auth_url.clone();
        let browser_opened = login.browser_opened;

        let mut pending = self.pending.lock().await;
        self.ended_flows().clear();
        // 同时只留一个进行中的登录：再点一次「添加账号」就丢掉上一个
        let stale: Vec<PendingLogin> = pending.drain().map(|(_, entry)| entry).collect();
        pending.insert(flow_id.clone(), PendingLogin { local_id, login });
        drop(pending);
        for entry in stale {
            self.abandon(entry);
        }
        Ok(LoginStart {
            flow_id,
            auth_url,
            expires_in: LOGIN_TIMEOUT.as_secs(),
            browser_opened,
        })
    }

    /// 还没登完时返回 `Ok(None)`；成功时收录账号并返回它。同一个账号再登录一次会替换掉
    /// 旧目录（旧登录通常已经失效，这就是「重新登录」）。
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<ClaudeAccount>, AppError> {
        let (local_id, exit) = {
            let mut pending = self.pending.lock().await;
            match self.ended_flows().get(flow_id) {
                Some(FlowEnd::Finishing) => return Ok(None),
                Some(FlowEnd::Added(account)) => return Ok(Some(account.clone())),
                None => {}
            }
            let Some(entry) = pending.get_mut(flow_id) else {
                return Err(AppError::localized(
                    "claude.account.login_expired",
                    "这次登录已结束，请重新添加账号",
                    "This sign-in has ended. Add the account again.",
                ));
            };
            match entry.login.done.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => return Ok(None),
                Err(oneshot::error::TryRecvError::Closed) => {
                    let entry = pending.remove(flow_id).expect("entry exists");
                    drop(pending);
                    self.discard_dir(&entry.local_id).await;
                    return Err(AppError::Message("登录被中断".into()));
                }
                Ok(exit) => {
                    let entry = pending.remove(flow_id).expect("entry exists");
                    // 还拿着 pending 锁时登记，别的轮询要么看到 pending、要么看到这里
                    self.ended_flows()
                        .insert(flow_id.to_string(), FlowEnd::Finishing);
                    (entry.local_id, exit)
                }
            }
        };
        let result = self.finish_login(local_id, exit).await;
        let mut ended = self.ended_flows();
        match &result {
            Ok(Some(account)) => {
                ended.insert(flow_id.to_string(), FlowEnd::Added(account.clone()));
            }
            _ => {
                ended.remove(flow_id);
            }
        }
        drop(ended);
        result
    }

    fn ended_flows(&self) -> std::sync::MutexGuard<'_, HashMap<String, FlowEnd>> {
        self.ended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 登录进程结束之后：成功就读身份、收录账号；失败就删掉目录。
    async fn finish_login(
        &self,
        local_id: String,
        exit: login::LoginExit,
    ) -> Result<Option<ClaudeAccount>, AppError> {
        if !exit.success {
            self.discard_dir(&local_id).await;
            return Err(AppError::Message(exit.message));
        }

        let config_dir = layout::config_dir(&self.data_dir, &local_id);
        let status = {
            let config_dir = config_dir.clone();
            tokio::task::spawn_blocking(move || auth_status(&config_dir))
                .await
                .ok()
                .flatten()
        };
        let Some(found) = identity::read_identity(&config_dir) else {
            self.discard_dir(&local_id).await;
            return Err(AppError::localized(
                "claude.account.identity_missing",
                "登录完成了，但读不到账号资料，请重试",
                "Signed in, but the account profile could not be read. Try again.",
            ));
        };

        {
            // 登录后 .claude.json 有了：补上 MCP 等
            let config_dir = config_dir.clone();
            let _ = tokio::task::spawn_blocking(move || sync_shared(&config_dir)).await;
        }

        let mut store = self.store.lock().await;
        let file = store_file_guard();
        *store = load_store(&self.data_dir);
        let data = AccountData {
            local_id: local_id.clone(),
            email: found
                .email
                .clone()
                .or_else(|| status.as_ref().and_then(|s| s.email.clone())),
            org_name: found
                .org_name
                .clone()
                .or_else(|| status.as_ref().and_then(|s| s.org_name.clone())),
            plan: status.and_then(|s| s.subscription_type),
            rate_limit_tier: None,
            added_at: chrono::Utc::now().timestamp(),
        };
        let replaced = store.accounts.insert(found.key.clone(), data);
        self.persist(&store)?;
        drop(file);
        let account = to_account(&found.key, &store.accounts[&found.key], &self.data_dir);
        drop(store);
        {
            let data_dir = self.data_dir.clone();
            let key = found.key.clone();
            let config_dir = config_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                snapshot::note_login_expiry(&data_dir, &key, &config_dir);
                if let Some(plan) = snapshot::dir_plan(&config_dir) {
                    note_plan(&data_dir, &key, &plan);
                }
            })
            .await;
        }
        if let Some(old) = replaced {
            if old.local_id != local_id {
                log::info!("[ClaudeAccount] 同一账号重新登录，替换旧目录");
                self.discard_dir(&old.local_id).await;
            }
            // 它正在默认槽位里时，默认槽位里还是旧登录：换成这次的
            let key = found.key.clone();
            let config_dir = config_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                crate::services::provider::claude_official::replace_live_login(&key, &config_dir)
            })
            .await;
        }
        log::info!("[ClaudeAccount] 已添加账号");
        Ok(Some(account))
    }

    pub async fn cancel_login(&self, flow_id: &str) -> bool {
        let entry = self.pending.lock().await.remove(flow_id);
        match entry {
            Some(entry) => {
                self.abandon(entry);
                true
            }
            None => false,
        }
    }

    /// 浏览器回不到本机时，用户粘贴回来的地址或代码。
    pub async fn submit_paste(&self, flow_id: &str, raw: &str) -> Result<(), AppError> {
        let pending = self.pending.lock().await;
        let Some(entry) = pending.get(flow_id) else {
            return Err(AppError::localized(
                "claude.account.login_expired",
                "这次登录已结束，请重新添加账号",
                "This sign-in has ended. Add the account again.",
            ));
        };
        entry.login.submit_paste(raw).await
    }

    /// 杀掉登录进程，等它退出后删掉目录。
    fn abandon(&self, entry: PendingLogin) {
        entry.login.cancel();
        let home = layout::account_home(&self.data_dir, &entry.local_id);
        let config_dir = layout::config_dir(&self.data_dir, &entry.local_id);
        let done = entry.login.done;
        tokio::spawn(async move {
            let _ = done.await;
            let _ =
                tokio::task::spawn_blocking(move || remove_account_files(&home, &config_dir)).await;
        });
    }

    // ---------- 列表、删除、解析 ----------

    pub async fn list(&self) -> Vec<ClaudeAccount> {
        let accounts: Vec<(String, AccountData)> = {
            let mut store = self.store.lock().await;
            // 切换时可能收录了新账号（默认槽位里原来的登录）
            *store = load_store(&self.data_dir);
            store
                .accounts
                .iter()
                .map(|(key, data)| (key.clone(), data.clone()))
                .collect()
        };
        let data_dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            let expiry = snapshot::load_state(&data_dir).login_expiry;
            let mut list: Vec<ClaudeAccount> = accounts
                .iter()
                .map(|(key, data)| {
                    let mut account = to_account(key, data, &data_dir);
                    account.login_expires_at = expiry
                        .get(key)
                        .copied()
                        .filter(|_| !account.requires_reauth);
                    account
                })
                .collect();
            list.sort_by_key(|account| account.label().to_lowercase());
            list
        })
        .await
        .unwrap_or_default()
    }

    /// 从 CC Switch 删掉这个账号：删目录和钥匙串里的条目。不吊销、不登出，服务端的令牌到期自然失效。
    pub async fn remove(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.store.lock().await;
        let file = store_file_guard();
        *store = load_store(&self.data_dir);
        let Some(data) = store.accounts.remove(account_id) else {
            return Err(account_not_found());
        };
        self.persist(&store)?;
        let _ = snapshot::update_state(&self.data_dir, |state| {
            state.login_expiry.remove(account_id);
        });
        drop(file);
        drop(store);
        self.discard_dir(&data.local_id).await;
        Ok(())
    }

    pub async fn remove_all(&self) -> Result<Vec<String>, AppError> {
        let mut store = self.store.lock().await;
        let file = store_file_guard();
        *store = load_store(&self.data_dir);
        let removed: Vec<(String, AccountData)> =
            std::mem::take(&mut store.accounts).into_iter().collect();
        self.persist(&store)?;
        let _ = snapshot::update_state(&self.data_dir, |state| state.login_expiry.clear());
        drop(file);
        drop(store);
        for (_, data) in &removed {
            self.discard_dir(&data.local_id).await;
        }
        Ok(removed.into_iter().map(|(key, _)| key).collect())
    }

    /// 账号在本机的 `CLAUDE_CONFIG_DIR`；不在这台电脑上（别的设备同步来的绑定、已删除）为 `None`。
    pub async fn config_dir_of(&self, account_id: &str) -> Option<PathBuf> {
        // 拿着锁读盘：和正在进行的改动串行
        let _store = self.store.lock().await;
        config_dir_on_disk(&self.data_dir, account_id)
    }

    /// 本机全部账号目录（不管存不存在）：判断 live 里的 `CLAUDE_CONFIG_DIR` 是不是 CC Switch 写的。
    pub async fn all_config_dirs(&self) -> Vec<PathBuf> {
        load_store(&self.data_dir)
            .accounts
            .values()
            .filter(|data| layout::valid_local_id(&data.local_id))
            .map(|data| layout::config_dir(&self.data_dir, &data.local_id))
            .collect()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    async fn discard_dir(&self, local_id: &str) {
        if !layout::valid_local_id(local_id) {
            return;
        }
        let home = layout::account_home(&self.data_dir, local_id);
        let config_dir = layout::config_dir(&self.data_dir, local_id);
        let _ = tokio::task::spawn_blocking(move || remove_account_files(&home, &config_dir)).await;
    }

    fn persist(&self, store: &StoreFile) -> Result<(), AppError> {
        persist_store(&self.data_dir, store)
    }
}

fn account_not_found() -> AppError {
    AppError::localized(
        "claude.account.not_found",
        "这个账号不在这台电脑上",
        "This account isn't on this computer.",
    )
}

fn to_account(key: &str, data: &AccountData, data_dir: &Path) -> ClaudeAccount {
    let config_dir = layout::config_dir(data_dir, &data.local_id);
    ClaudeAccount {
        id: key.to_string(),
        email: data.email.clone(),
        org_name: data.org_name.clone(),
        plan: data.plan.clone(),
        rate_limit_tier: data.rate_limit_tier.clone(),
        authenticated_at: data.added_at,
        // 被移进默认槽位的账号，自己的槽位是空的；槽位空了但有收回时留的备份，切换会用备份
        requires_reauth: !layout::valid_local_id(&data.local_id)
            || (!has_credentials(&config_dir)
                && !has_credentials(&snapshot::backup_dir(&config_dir))
                && !snapshot::is_live(key)),
        login_expires_at: None,
    }
}

fn create_private_dir(dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dir).map_err(|source| AppError::io(dir, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [dir, dir.parent().unwrap_or(dir)] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .map_err(|source| AppError::io(path, source))?;
        }
    }
    Ok(())
}

/// 按磁盘上的账号表找账号目录、收录新账号：切换路径上用（拿不到 Tauri 状态）。
pub struct DiskAccounts {
    pub data_dir: PathBuf,
}

impl snapshot::Accounts for DiskAccounts {
    fn dir_of(&self, account: &str) -> Option<PathBuf> {
        config_dir_on_disk(&self.data_dir, account)
    }
}

impl snapshot::Adopter for DiskAccounts {
    /// 给默认槽位里原来的登录建一个账号目录：只写资料（`oauthAccount`），凭据由切换随后放进去。
    fn adopt(
        &self,
        found: &identity::Identity,
        profile: &serde_json::Value,
    ) -> Result<PathBuf, AppError> {
        let local_id = layout::new_local_id();
        let config_dir = layout::config_dir(&self.data_dir, &local_id);
        layout::assert_config_dir(&self.data_dir, &config_dir)?;
        create_private_dir(&config_dir)?;
        let doc = serde_json::json!({
            "oauthAccount": profile,
            "hasCompletedOnboarding": true,
        });
        let bytes =
            serde_json::to_vec_pretty(&doc).map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&config_dir.join(".claude.json"), &bytes)?;
        sync_shared(&config_dir);

        let _file = store_file_guard();
        let mut store = load_store(&self.data_dir);
        store.accounts.insert(
            found.key.clone(),
            AccountData {
                local_id,
                email: found.email.clone(),
                org_name: found.org_name.clone(),
                plan: None,
                rate_limit_tier: None,
                added_at: chrono::Utc::now().timestamp(),
            },
        );
        persist_store(&self.data_dir, &store)?;
        log::info!("[ClaudeAccount] 已把默认登录收录为账号");
        Ok(config_dir)
    }
}

/// 删掉一个账号的目录和它在钥匙串里的条目。`remove_dir_all` 不跟随符号链接（共享层链到
/// 默认目录的那些只删链接本身）。
fn remove_account_files(home: &Path, config_dir: &Path) {
    delete_keychain_entry(config_dir);
    let backup = snapshot::backup_dir(config_dir);
    delete_keychain_entry(&backup);
    if let Err(error) = std::fs::remove_dir_all(&backup) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("[ClaudeAccount] 删除账号备份目录失败: {error}");
        }
    }
    if let Err(error) = std::fs::remove_dir_all(home) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("[ClaudeAccount] 删除账号目录失败: {error}");
        }
    }
}

/// 目录里还有没有登录：只看在不在，不读内容。
fn has_credentials(config_dir: &Path) -> bool {
    if !config_dir.is_dir() {
        return false;
    }
    if config_dir.join(".credentials.json").is_file() {
        return true;
    }
    keychain_entry_exists(config_dir)
}

#[cfg(target_os = "macos")]
fn keychain_entry_exists(config_dir: &Path) -> bool {
    // 只查存在性：不带 -w / -g，输出里没有密文
    std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-a",
            &layout::keychain_account(),
            "-s",
            &layout::keychain_service(config_dir),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(target_os = "macos"))]
fn keychain_entry_exists(_config_dir: &Path) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn delete_keychain_entry(config_dir: &Path) {
    let _ = std::process::Command::new("/usr/bin/security")
        .args([
            "delete-generic-password",
            "-a",
            &layout::keychain_account(),
            "-s",
            &layout::keychain_service(config_dir),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(target_os = "macos"))]
fn delete_keychain_entry(_config_dir: &Path) {}

/// 在账号目录里跑一次 `claude auth status --json`（只在刚登录完时调用：令牌是新的，
/// 不会触发刷新）。
fn auth_status(config_dir: &Path) -> Option<identity::AuthStatus> {
    let mut cmd = crate::commands::detected_tool_command(
        "claude",
        &["auth", "status", "--json"],
        STATUS_TIMEOUT,
    )
    .ok()?;
    login::apply_account_env(&mut cmd, config_dir);
    cmd.current_dir(config_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < STATUS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    identity::parse_auth_status(&output)
}

#[cfg(test)]
mod tests;
