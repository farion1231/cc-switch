//! CodeBuddy Code 的官方账号：在 CC Switch 里收录、切换。
//!
//! CodeBuddy 只有一份全局登录：`<数据目录>/CodeBuddyExtension/Data/Public/auth/` 下的
//! `Tencent-Cloud.coding-copilot.info`（明文 JSON，0600；国内版、国际版共用，靠 `auth.domain`
//! 区分）。运行中的 CLI 监听这个目录，文件一变就换号。CC Switch 自己存每个账号的整份文件
//! （`codebuddy_accounts.json`，0600，不随云同步），切换就是把目标账号那份写回去。账号按
//! `account.uid` + `auth.domain` 区分。
//!
//! 几条规矩：
//! - 官方登录走 CodeBuddy 私有接口，CC Switch 不自己做 OAuth：加号靠「收录当前登录」或在终端里
//!   运行 `codebuddy` 让用户登录，等凭据文件变了再收录。已经登着号时 CLI 不会再进登录界面，
//!   所以先收录现在的号，再写一份登出标记（和 CLI 的 `/logout` 一样：凭据文件不动、不调服务端，
//!   CLI 见到标记就当没登录）；用户放弃时删掉自己写的标记，原来的登录就回来了。
//! - 令牌由 CodeBuddy 自己续期，CC Switch 从不续期。所以写之前先把文件里现在那份收回来（CLI 可能
//!   已经续过期）；不认识的号直接收录，切走也不会丢。
//! - 认不出格式的内容（比如 WorkBuddy 托管时的 `$wbEncrypted` 加密信封）整份原样暂存到
//!   `codebuddy-login-stash.json`，不解析，也不拿别的号覆盖它。
//! - 写文件前拿 CodeBuddy / WorkBuddy 共用的目录锁，临时文件 → fsync → rename，写完删掉登出标记
//!   再读回核对。
//! - 从 CC Switch 删账号只删自己的副本，永远不动 CodeBuddy 的文件（不替它登出）。
//! - 不往 settings.json 的 env 写 `CODEBUDDY_API_KEY` / `CODEBUDDY_AUTH_TOKEN`。
//! - 额度只用副本里的 accessToken 只读查询，不续期（见 [`quota`]）。

mod quota;

pub use quota::CodebuddyQuota;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::AppError;

const STORE_FILENAME: &str = "codebuddy_accounts.json";
const STASH_FILENAME: &str = "codebuddy-login-stash.json";
/// 暂存最多留这么多份（同样的内容只留一份）
const STASH_LIMIT: usize = 5;

pub const CREDENTIAL_FILENAME: &str = "Tencent-Cloud.coding-copilot.info";
const LOGGED_OUT_FILENAME: &str = "Tencent-Cloud.coding-copilot.info.logged-out";
/// proper-lockfile 风格的目录锁：mkdir 成功即持锁。
const LOCK_DIRNAME: &str = ".Tencent-Cloud.coding-copilot.info.workbuddy.lock";
/// proper-lockfile 默认 10 秒没更新 mtime 就算持锁方已经死了。
const LOCK_STALE: Duration = Duration::from_secs(10);
/// 拿锁最多等这么久
const LOCK_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};
const LOCK_RETRY: Duration = Duration::from_millis(100);

/// 在终端里登录最多等这么久，过了就当放弃。
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// 覆盖凭据目录（测试、调试用）。
pub const AUTH_DIR_ENV: &str = "CC_SWITCH_CODEBUDDY_AUTH_DIR";

/// CodeBuddy 凭据文件所在目录。可用 [`AUTH_DIR_ENV`] 覆盖。
pub fn default_auth_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(AUTH_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    platform_data_dir()
        .join("CodeBuddyExtension")
        .join("Data")
        .join("Public")
        .join("auth")
}

fn platform_data_dir() -> PathBuf {
    let home = crate::config::get_home_dir();
    #[cfg(target_os = "macos")]
    {
        home.join("Library").join("Application Support")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".local").join("share")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountData {
    uid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
    /// refresh token 到期时间（毫秒）；文件里没有时为 `None`，按可用处理
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_expires_at: Option<i64>,
    /// 凭据文件整份原样保存（以后 CodeBuddy 新增的字段也跟着走）
    raw: String,
    /// 收录时间（秒）
    authenticated_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, AccountData>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyAccount {
    /// `<uid>@<domain>`
    pub id: String,
    pub uid: String,
    pub domain: Option<String>,
    pub nickname: Option<String>,
    pub refresh_expires_at: Option<i64>,
    pub authenticated_at: i64,
    /// refresh token 已过期：切过去也登不上
    pub expired: bool,
}

impl CodebuddyAccount {
    /// 界面上显示的名字：昵称 > uid。
    pub fn label(&self) -> String {
        self.nickname
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.uid.clone())
    }

    fn from_data(id: &str, data: &AccountData, now_ms: i64) -> Self {
        Self {
            id: id.to_string(),
            uid: data.uid.clone(),
            domain: data.domain.clone(),
            nickname: data.nickname.clone(),
            refresh_expires_at: data.refresh_expires_at,
            authenticated_at: data.authenticated_at,
            expired: is_expired(data.refresh_expires_at, now_ms),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyAccountStatus {
    pub accounts: Vec<CodebuddyAccount>,
    /// CodeBuddy 当前登录的是哪个已收录的号；没登录、已登出或还没收录时为 `None`
    pub current_account_id: Option<String>,
}

/// CodeBuddy 凭据文件现在的状态（授权中心顶部提示用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// 没有凭据文件
    None,
    /// 有登出标记
    LoggedOut,
    /// 登着的号已经收录
    Collected,
    /// 登着一个还没收录的号
    Uncollected,
    /// 文件格式认不出（比如 WorkBuddy 托管时的加密信封）
    Unrecognized,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyLiveSummary {
    pub state: LiveState,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub domain: Option<String>,
}

pub struct LoginStart {
    pub flow_id: String,
    pub expires_in: u64,
}

struct PendingLogin {
    baseline: Baseline,
    started: Instant,
    /// 这次登录为了让 CLI 进登录界面写下的登出标记内容；没写过时为 `None`
    marker: Option<String>,
}

/// 开始登录时凭据文件的样子。之后文件变成一个认得出的登录、且不只是同一会话的续期，
/// 才算登录完成：换了号，或者同一个号的 `sessionState` 变了（重新登录会开新会话，续期不会）；
/// 文件里没有 `sessionState` 时退回比较整份内容。
#[derive(Default)]
struct Baseline {
    bytes: Option<Vec<u8>>,
    key: Option<String>,
    session_state: Option<String>,
}

impl Baseline {
    fn of(live: &LiveLogin) -> Self {
        match live {
            LiveLogin::Known { raw, identity } => Self {
                bytes: Some(raw.as_bytes().to_vec()),
                key: Some(identity.key()),
                session_state: identity.session_state.clone(),
            },
            LiveLogin::Unrecognized(bytes) => Self {
                bytes: Some(bytes.clone()),
                ..Self::default()
            },
            LiveLogin::None | LiveLogin::LoggedOut => Self::default(),
        }
    }

    fn completed_by(&self, raw: &str, identity: &Identity) -> bool {
        if self.bytes.as_deref() == Some(raw.as_bytes()) {
            return false;
        }
        if self.key.as_deref() != Some(identity.key().as_str()) {
            return true;
        }
        match (&self.session_state, &identity.session_state) {
            (Some(before), Some(after)) => before != after,
            _ => true,
        }
    }
}

/// 从凭据文件里认出来的身份。
#[derive(Debug, Clone)]
struct Identity {
    uid: String,
    domain: Option<String>,
    nickname: Option<String>,
    refresh_expires_at: Option<i64>,
    session_state: Option<String>,
}

impl Identity {
    fn key(&self) -> String {
        account_key(&self.uid, self.domain.as_deref())
    }
}

/// 凭据文件现在的内容。
enum LiveLogin {
    None,
    LoggedOut,
    Known {
        raw: String,
        identity: Identity,
    },
    /// 认不出的内容，按字节原样留着（可能不是 UTF-8）
    Unrecognized(Vec<u8>),
}

pub struct CodebuddyAccountManager {
    data_dir: PathBuf,
    auth_dir: PathBuf,
    /// 账号表；同时充当所有改动（含写凭据文件）的进程内互斥锁
    store: Mutex<StoreFile>,
    pending: Mutex<HashMap<String, PendingLogin>>,
    /// 额度缓存：`(账号 id, 语言)` → (查询时刻, 结果)
    quota_cache: Mutex<HashMap<(String, String), (Instant, CodebuddyQuota)>>,
}

impl CodebuddyAccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_auth_dir(data_dir, default_auth_dir())
    }

    pub fn with_auth_dir(data_dir: PathBuf, auth_dir: PathBuf) -> Self {
        let path = data_dir.join(STORE_FILENAME);
        let store = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("[CodebuddyAccount] 账号文件解析失败，按空表处理: {error}");
                StoreFile::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreFile::default(),
            Err(error) => {
                log::warn!("[CodebuddyAccount] 账号文件读取失败，按空表处理: {error}");
                StoreFile::default()
            }
        };
        Self {
            data_dir,
            auth_dir,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
            quota_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn auth_dir(&self) -> &Path {
        &self.auth_dir
    }

    // ---------- 状态 ----------

    /// 列出账号，并顺手把文件里已收录账号的最新内容收回来（CodeBuddy 续期后的令牌）。
    /// 还没收录的号不自动收录，由用户点「收录」。
    pub async fn status(&self) -> Result<CodebuddyAccountStatus, AppError> {
        let mut store = self.store.lock().await;
        let live = self.read_live();
        let (current, changed) = absorb(&mut store, &live, false);
        if changed {
            self.persist(&store)?;
        }
        Ok(CodebuddyAccountStatus {
            accounts: sorted_accounts(&store),
            current_account_id: current,
        })
    }

    pub async fn live_summary(&self) -> Result<CodebuddyLiveSummary, AppError> {
        let mut store = self.store.lock().await;
        let live = self.read_live();
        let (_, changed) = absorb(&mut store, &live, false);
        if changed {
            self.persist(&store)?;
        }
        Ok(match live {
            LiveLogin::None => summary(LiveState::None, None),
            LiveLogin::LoggedOut => summary(LiveState::LoggedOut, None),
            LiveLogin::Unrecognized(_) => summary(LiveState::Unrecognized, None),
            LiveLogin::Known { identity, .. } => {
                let state = if store.accounts.contains_key(&identity.key()) {
                    LiveState::Collected
                } else {
                    LiveState::Uncollected
                };
                summary(state, Some(&identity))
            }
        })
    }

    /// 把 CodeBuddy 现在登着的号收进来（已收录就更新它的副本）。
    pub async fn import_current(&self) -> Result<CodebuddyAccount, AppError> {
        let mut store = self.store.lock().await;
        let live = self.read_live();
        match &live {
            LiveLogin::None | LiveLogin::LoggedOut => return Err(not_logged_in()),
            LiveLogin::Unrecognized(raw) => {
                self.stash_unrecognized(raw)?;
                return Err(unrecognized_format());
            }
            LiveLogin::Known { .. } => {}
        }
        let (current, changed) = absorb(&mut store, &live, true);
        if changed {
            self.persist(&store)?;
        }
        let id = current.ok_or_else(not_logged_in)?;
        account_of(&store, &id)
    }

    // ---------- 终端登录 ----------

    /// 开始一次终端登录：先把现在登着的号收进来（新登录会覆盖文件，不收就丢了），再记下文件
    /// 内容，之后文件变了且认得出就算登录完成。打开终端由调用方负责。
    ///
    /// 现在登着一个认得出的号时，拿目录锁写一份登出标记，让 CLI 直接进登录界面；凭据文件
    /// 不动（CLI restore 时还要靠它），也不调任何服务端接口。放弃时 [`Self::cancel_login`]
    /// 会删掉这份标记。认不出的内容（WorkBuddy 托管）只暂存，不写标记。
    pub async fn start_login(&self) -> Result<LoginStart, AppError> {
        let mut pending = self.pending.lock().await;
        // 同时只留一个进行中的登录：再点一次「添加账号」就丢掉上一个（先恢复它写的标记）
        for (_, previous) in pending.drain() {
            if let Err(error) = self.restore_marker(&previous).await {
                log::warn!("[CodebuddyAccount] 恢复上一次登录前的登出标记失败: {error}");
            }
        }
        let (baseline, marker) = {
            let mut store = self.store.lock().await;
            // 只有要写标记时才拿锁（没登录时连 CodeBuddy 的目录都不建）；拿到锁再读一遍
            let mut live = self.read_live();
            let _lock = match live {
                LiveLogin::Known { .. } => {
                    let lock = DirLock::acquire(&self.auth_dir).await?;
                    live = self.read_live();
                    Some(lock)
                }
                _ => None,
            };
            if let LiveLogin::Unrecognized(raw) = &live {
                self.stash_unrecognized(raw)?;
            }
            let (_, changed) = absorb(&mut store, &live, true);
            if changed {
                self.persist(&store)?;
            }
            let marker = match &live {
                LiveLogin::Known { raw, .. } => Some(self.write_logout_marker(raw)?),
                _ => None,
            };
            (Baseline::of(&live), marker)
        };
        let flow_id = uuid::Uuid::new_v4().to_string();
        pending.insert(
            flow_id.clone(),
            PendingLogin {
                baseline,
                started: Instant::now(),
                marker,
            },
        );
        Ok(LoginStart {
            flow_id,
            expires_in: LOGIN_TIMEOUT.as_secs(),
        })
    }

    /// 还没登录完时返回 `Ok(None)`；文件变成一个认得出的登录时收录它并返回。
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<CodebuddyAccount>, AppError> {
        let mut pending = self.pending.lock().await;
        let Some(entry) = pending.get(flow_id) else {
            return Err(AppError::localized(
                "codebuddy.account.login_expired",
                "这次登录已结束，请重新添加账号",
                "This sign-in has ended. Add the account again.",
            ));
        };
        if entry.started.elapsed() > LOGIN_TIMEOUT {
            if let Some(expired) = pending.remove(flow_id) {
                if let Err(error) = self.restore_marker(&expired).await {
                    log::warn!("[CodebuddyAccount] 登录超时后恢复原登录失败: {error}");
                }
            }
            return Err(AppError::localized(
                "codebuddy.account.login_timeout",
                "等待登录超时，请重新添加账号",
                "Timed out waiting for the sign-in. Add the account again.",
            ));
        }
        let mut store = self.store.lock().await;
        let live = self.read_live();
        let LiveLogin::Known { raw, identity } = &live else {
            return Ok(None);
        };
        if !entry.baseline.completed_by(raw, identity) {
            return Ok(None);
        }
        pending.remove(flow_id);
        drop(pending);
        let (current, changed) = absorb(&mut store, &live, true);
        if changed {
            self.persist(&store)?;
        }
        let id = current.ok_or_else(not_logged_in)?;
        account_of(&store, &id).map(Some)
    }

    /// 放弃这次登录。开始时写过登出标记、而且标记还是那一份时删掉它，原来的登录就回来了
    /// （CLI 已经登上新号时它自己会删标记，这里什么都不动）。返回有没有这次登录。
    pub async fn cancel_login(&self, flow_id: &str) -> Result<bool, AppError> {
        let mut pending = self.pending.lock().await;
        let Some(entry) = pending.remove(flow_id) else {
            return Ok(false);
        };
        if let Err(error) = self.restore_marker(&entry).await {
            // 没恢复成（比如 CodeBuddy 正拿着锁）：留着这次登录，用户可以再点一次
            pending.insert(flow_id.to_string(), entry);
            return Err(error);
        }
        Ok(true)
    }

    // ---------- 切换与删除 ----------

    /// 把 CodeBuddy 的全局登录换成这个账号。已经是它时什么都不写。
    pub async fn switch_to(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.store.lock().await;
        let target = store
            .accounts
            .get(account_id)
            .ok_or_else(|| account_not_found(account_id))?;
        if is_expired(target.refresh_expires_at, now_ms()) {
            return Err(AppError::localized(
                "codebuddy.account.expired",
                "这个账号的登录已过期，请在 CodeBuddy 里重新登录后再收录",
                "This account's sign-in has expired. Sign in again in CodeBuddy and add it again.",
            ));
        }

        let _lock = DirLock::acquire(&self.auth_dir).await?;

        // 先把文件里现在那份收回来：CodeBuddy 可能已经续过期
        let live = self.read_live();
        if let LiveLogin::Unrecognized(raw) = &live {
            self.stash_unrecognized(raw)?;
            return Err(unrecognized_format());
        }
        let (current, changed) = absorb(&mut store, &live, true);
        if changed {
            self.persist(&store)?;
        }
        if current.as_deref() == Some(account_id) {
            return Ok(());
        }

        let raw = store
            .accounts
            .get(account_id)
            .map(|account| account.raw.clone())
            .ok_or_else(|| account_not_found(account_id))?;
        let path = self.credential_path();
        crate::config::stage_write(&path, raw.as_bytes(), Some(0o600), true)?.commit()?;
        let marker = self.auth_dir.join(LOGGED_OUT_FILENAME);
        match std::fs::remove_file(&marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(&marker, error)),
        }

        // 写完读回来核对
        let written = std::fs::read_to_string(&path).ok();
        if written.as_deref() != Some(raw.as_str()) || marker.exists() {
            return Err(AppError::localized(
                "codebuddy.account.write_verify_failed",
                "写入后核对不一致，CodeBuddy 的登录可能没有切换成功",
                "The written sign-in did not verify; CodeBuddy may not have switched accounts.",
            ));
        }
        log::info!("[CodebuddyAccount] 已切换 CodeBuddy 登录");
        Ok(())
    }

    /// 只删 CC Switch 里的副本，不替 CodeBuddy 登出。
    pub async fn remove(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.store.lock().await;
        if store.accounts.remove(account_id).is_none() {
            return Err(account_not_found(account_id));
        }
        self.quota_cache
            .lock()
            .await
            .retain(|(id, _), _| id != account_id);
        self.persist(&store)
    }

    // ---------- 额度 ----------

    /// 这个账号的套餐与积分。先把凭据文件里现在那份收回来（当前登录的号用 CodeBuddy 刚续过的
    /// 令牌），再用副本里的 accessToken 只读查询；令牌过期时不查也不续期。`force` 跳过缓存。
    pub async fn quota(
        &self,
        account_id: &str,
        lang: &str,
        force: bool,
    ) -> Result<CodebuddyQuota, AppError> {
        let lang = if lang.to_ascii_lowercase().starts_with("zh") {
            "zh"
        } else {
            "en"
        };
        let key = (account_id.to_string(), lang.to_string());
        if !force {
            if let Some((at, cached)) = self.quota_cache.lock().await.get(&key) {
                if at.elapsed() < quota::QUOTA_CACHE_TTL {
                    return Ok(cached.clone());
                }
            }
        }

        let raw = {
            let mut store = self.store.lock().await;
            let live = self.read_live();
            let (_, changed) = absorb(&mut store, &live, false);
            if changed {
                self.persist(&store)?;
            }
            store
                .accounts
                .get(account_id)
                .map(|account| account.raw.clone())
                .ok_or_else(|| account_not_found(account_id))?
        };
        let Some(creds) = quota::credentials_of(&raw) else {
            return Ok(CodebuddyQuota::failed(
                crate::services::subscription::CredentialStatus::ParseError,
                "这个账号的登录信息认不出",
            ));
        };
        let result = match quota::expired_status(&creds, now_ms()) {
            Some(status) => {
                let message = match status {
                    crate::services::subscription::CredentialStatus::Expired => {
                        "CodeBuddy 登录已过期"
                    }
                    _ => "令牌待 CodeBuddy 续期",
                };
                CodebuddyQuota::failed(status, message)
            }
            None => quota::fetch(&creds, lang).await,
        };
        // 只缓存查到的；失败的下次再试
        if result.success {
            self.quota_cache
                .lock()
                .await
                .insert(key, (Instant::now(), result.clone()));
        }
        Ok(result)
    }

    // ---------- 内部 ----------

    /// 写登出标记（内容是当前时间，临时文件 + rename），返回写下的内容。调用方持有目录锁。
    /// 凭据文件一个字节都不动，写完核对它还是原样。
    fn write_logout_marker(&self, raw: &str) -> Result<String, AppError> {
        let content = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let marker = self.auth_dir.join(LOGGED_OUT_FILENAME);
        crate::config::atomic_write_private(&marker, content.as_bytes())?;
        let credential = std::fs::read_to_string(self.credential_path()).ok();
        if credential.as_deref() != Some(raw) {
            log::warn!("[CodebuddyAccount] 写登出标记时凭据文件被别人改了");
        }
        log::info!("[CodebuddyAccount] 已写登出标记，等待在终端登录新账号");
        Ok(content)
    }

    /// 删掉这次登录自己写的登出标记（拿目录锁；标记内容不是自己写的那份就不碰）。
    async fn restore_marker(&self, entry: &PendingLogin) -> Result<bool, AppError> {
        let Some(ours) = entry.marker.as_deref() else {
            return Ok(false);
        };
        let _lock = DirLock::acquire(&self.auth_dir).await?;
        let marker = self.auth_dir.join(LOGGED_OUT_FILENAME);
        match std::fs::read_to_string(&marker) {
            Ok(content) if content == ours => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(AppError::io(&marker, error)),
        }
        match std::fs::remove_file(&marker) {
            Ok(()) => {
                log::info!("[CodebuddyAccount] 已删除登出标记，恢复原来的登录");
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::io(&marker, error)),
        }
    }

    fn credential_path(&self) -> PathBuf {
        self.auth_dir.join(CREDENTIAL_FILENAME)
    }

    fn read_live(&self) -> LiveLogin {
        if self.auth_dir.join(LOGGED_OUT_FILENAME).exists() {
            return LiveLogin::LoggedOut;
        }
        let path = self.credential_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LiveLogin::None,
            Err(error) => {
                // 读不出来就当认不出，绝不拿别的号覆盖它
                log::warn!("[CodebuddyAccount] 读取 CodeBuddy 凭据失败: {error}");
                return LiveLogin::Unrecognized(Vec::new());
            }
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return LiveLogin::None;
        }
        let raw = match String::from_utf8(bytes) {
            Ok(raw) => raw,
            Err(error) => return LiveLogin::Unrecognized(error.into_bytes()),
        };
        match parse_identity(&raw) {
            Some(identity) => LiveLogin::Known { raw, identity },
            None => LiveLogin::Unrecognized(raw.into_bytes()),
        }
    }

    /// 认不出的内容整份原样存一份（同样的内容只存一次），最多留 [`STASH_LIMIT`] 份。
    /// UTF-8 文本存在 `raw`，否则 base64 后存在 `raw_base64`。
    fn stash_unrecognized(&self, bytes: &[u8]) -> Result<(), AppError> {
        use base64::Engine as _;
        if bytes.is_empty() {
            return Ok(());
        }
        let (field, raw) = match std::str::from_utf8(bytes) {
            Ok(text) => ("raw", text.to_string()),
            Err(_) => (
                "raw_base64",
                base64::engine::general_purpose::STANDARD.encode(bytes),
            ),
        };
        let path = self.data_dir.join(STASH_FILENAME);
        let mut entries: Vec<Value> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value.get("entries").and_then(Value::as_array).cloned())
            .unwrap_or_default();
        if entries
            .iter()
            .any(|entry| entry.get(field).and_then(Value::as_str) == Some(raw.as_str()))
        {
            return Ok(());
        }
        let mut entry = serde_json::Map::new();
        entry.insert(field.to_string(), Value::String(raw));
        entry.insert(
            "saved_at".to_string(),
            Value::from(chrono::Utc::now().timestamp_millis()),
        );
        entries.push(Value::Object(entry));
        let overflow = entries.len().saturating_sub(STASH_LIMIT);
        entries.drain(..overflow);
        let content = serde_json::to_vec_pretty(&serde_json::json!({ "entries": entries }))
            .map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&path, &content)
    }

    fn persist(&self, store: &StoreFile) -> Result<(), AppError> {
        let snapshot = StoreFile {
            version: 1,
            accounts: store.accounts.clone(),
        };
        let content = serde_json::to_vec_pretty(&snapshot)
            .map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&self.data_dir.join(STORE_FILENAME), &content)
    }
}

/// 把文件里的登录并进账号表，返回它对应的已收录账号 id 和表有没有变。
/// `import_unknown`：没收录的号也收进来（切换、终端登录、手动收录时）。
fn absorb(store: &mut StoreFile, live: &LiveLogin, import_unknown: bool) -> (Option<String>, bool) {
    let LiveLogin::Known { raw, identity } = live else {
        return (None, false);
    };
    let key = identity.key();
    if let Some(existing) = store.accounts.get_mut(&key) {
        let mut changed = false;
        if existing.raw != *raw {
            existing.raw = raw.clone();
            existing.refresh_expires_at = identity.refresh_expires_at;
            changed = true;
        }
        if let Some(nickname) = identity.nickname.clone().filter(|name| !name.is_empty()) {
            if existing.nickname.as_deref() != Some(nickname.as_str()) {
                existing.nickname = Some(nickname);
                changed = true;
            }
        }
        return (Some(key), changed);
    }
    if !import_unknown {
        return (None, false);
    }
    store.accounts.insert(
        key.clone(),
        AccountData {
            uid: identity.uid.clone(),
            domain: identity.domain.clone(),
            nickname: identity.nickname.clone().filter(|name| !name.is_empty()),
            refresh_expires_at: identity.refresh_expires_at,
            raw: raw.clone(),
            authenticated_at: chrono::Utc::now().timestamp(),
        },
    );
    (Some(key), true)
}

fn summary(state: LiveState, identity: Option<&Identity>) -> CodebuddyLiveSummary {
    CodebuddyLiveSummary {
        state,
        account_id: identity.map(Identity::key),
        label: identity.map(|identity| {
            identity
                .nickname
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| identity.uid.clone())
        }),
        domain: identity.and_then(|identity| identity.domain.clone()),
    }
}

fn account_key(uid: &str, domain: Option<&str>) -> String {
    format!("{uid}@{}", domain.unwrap_or_default())
}

fn account_of(store: &StoreFile, id: &str) -> Result<CodebuddyAccount, AppError> {
    store
        .accounts
        .get(id)
        .map(|data| CodebuddyAccount::from_data(id, data, now_ms()))
        .ok_or_else(|| account_not_found(id))
}

fn sorted_accounts(store: &StoreFile) -> Vec<CodebuddyAccount> {
    let now = now_ms();
    let mut accounts: Vec<CodebuddyAccount> = store
        .accounts
        .iter()
        .map(|(id, data)| CodebuddyAccount::from_data(id, data, now))
        .collect();
    accounts.sort_by_key(|account| account.authenticated_at);
    accounts
}

/// 认出凭据文件里的身份。任何一处是 `$wbEncrypted` 信封、缺令牌或缺 uid 都当认不出。
fn parse_identity(raw: &str) -> Option<Identity> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let root = value.as_object()?;
    if root.contains_key("$wbEncrypted") {
        return None;
    }
    let auth = root.get("auth")?.as_object()?;
    let account = root.get("account")?.as_object()?;
    if auth.contains_key("$wbEncrypted") || account.contains_key("$wbEncrypted") {
        return None;
    }
    let non_empty = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
    };
    non_empty(auth.get("accessToken"))?;
    non_empty(auth.get("refreshToken"))?;
    let uid = match account.get("uid")? {
        Value::String(uid) if !uid.trim().is_empty() => uid.trim().to_string(),
        Value::Number(uid) => uid.to_string(),
        _ => return None,
    };
    let refresh_expires_at = timestamp_ms(auth.get("refreshExpiresAt")).or_else(|| {
        let refreshed = timestamp_ms(auth.get("lastRefreshTime"))?;
        let seconds = number(auth.get("refreshExpiresIn"))?;
        Some(refreshed + (seconds * 1000.0) as i64)
    });
    Some(Identity {
        uid,
        domain: non_empty(auth.get("domain")),
        nickname: non_empty(account.get("nickname")),
        refresh_expires_at,
        session_state: non_empty(auth.get("sessionState")),
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .filter(|number| number.is_finite() && *number > 0.0)
}

/// 时间戳统一成毫秒：小于 1e12 的按秒算。也认 RFC 3339 字符串。
fn timestamp_ms(value: Option<&Value>) -> Option<i64> {
    if let Some(Value::String(text)) = value {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text.trim()) {
            return Some(parsed.timestamp_millis());
        }
    }
    let number = number(value)?;
    Some(if number < 1e12 {
        (number * 1000.0) as i64
    } else {
        number as i64
    })
}

fn is_expired(refresh_expires_at: Option<i64>, now_ms: i64) -> bool {
    refresh_expires_at.is_some_and(|at| at <= now_ms)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn account_not_found(account_id: &str) -> AppError {
    AppError::Message(format!("找不到这个 CodeBuddy 账号: {account_id}"))
}

fn not_logged_in() -> AppError {
    AppError::localized(
        "codebuddy.account.not_logged_in",
        "CodeBuddy 当前没有登录",
        "CodeBuddy is not signed in.",
    )
}

fn unrecognized_format() -> AppError {
    AppError::localized(
        "codebuddy.account.unrecognized",
        "CodeBuddy 的登录文件格式认不出（可能由 WorkBuddy 加密托管），已原样暂存，没有改动它",
        "CodeBuddy's sign-in file is in a format CC Switch does not recognize (it may be encrypted by WorkBuddy). It was backed up as-is and left untouched.",
    )
}

/// CodeBuddy / WorkBuddy 共用的目录锁（proper-lockfile 风格）。离开作用域时释放。
struct DirLock {
    path: PathBuf,
}

impl DirLock {
    async fn acquire(auth_dir: &Path) -> Result<Self, AppError> {
        let path = auth_dir.join(LOCK_DIRNAME);
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(auth_dir).map_err(|e| AppError::io(auth_dir, e))?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) && std::fs::remove_dir(&path).is_ok() {
                        log::warn!("[CodebuddyAccount] 清理了过期的凭据文件锁");
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(AppError::localized(
                            "codebuddy.account.locked",
                            "CodeBuddy 正在写登录文件，请稍后再试",
                            "CodeBuddy is writing its sign-in file. Try again shortly.",
                        ));
                    }
                    tokio::time::sleep(LOCK_RETRY).await;
                }
                Err(error) => return Err(AppError::io(&path, error)),
            }
        }
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > LOCK_STALE)
}

#[cfg(test)]
mod tests;
