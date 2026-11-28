//! Antigravity CLI 的官方账号：在 CC Switch 里加号、切号。
//!
//! agy 只有一份全局登录，存在系统钥匙串里（见 `keychain`）。CC Switch 自己存每个账号的整份
//! 登录数据（`agy_accounts.json`，0600，不随云同步），切换就是把目标账号那份写进钥匙串，
//! agy 下次启动就是这个号。账号按 id_token 的 `sub` 区分（邮箱可能相同前缀，名字可改）；
//! agy 1.5 起钥匙串里没有 id_token，按 refresh token / 邮箱对回已收录的号（见 `read_live`）。
//!
//! 几条规矩：
//! - 写之前先把钥匙串里现在那份收回来：是已收录的号就更新它（agy 续期后写回的新 access
//!   token）；不认识的号直接收录，切走也不会丢。认不出格式的整条暂存到 `agy-login-stash.json`。
//! - 从 CC Switch 删账号只删自己的副本，永远不动钥匙串（不替 agy 登出）。
//! - 不自动换号：额度用完、请求出错都不切，切换只由用户触发。
//! - 不发任何网络请求，不用 agy 的身份：不用 agy 的 OAuth 客户端登录或续期，也不拿 agy 的令牌
//!   去问 Google。加号是在 agy 里登录（`start_login` 打开终端运行 agy，钥匙串变了就收录）；
//!   认账号只解本地的 id_token / 邮箱；令牌由 agy 自己续期（Google 不轮换 refresh token，
//!   副本里过期的 access token 切过去后 agy 会自己换新）。
//! - 额度只问 agy 自己的 `/usage`，所以只查得到 agy 当前登着的号；查到的按账号记下
//!   （`last_quota`），切走的号显示切走前最后一次查到的。

pub mod identity;
pub mod keychain;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::services::subscription::SubscriptionQuota;
use crate::sync_utils::MutexExt;
use keychain::AgyLoginStore;

const STORE_FILENAME: &str = "agy_accounts.json";
const STASH_FILENAME: &str = "agy-login-stash.json";
/// 在 agy 里登录最多等这么久，过了就当放弃。
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Antigravity 桌面端或 `agy` CLI 是否正在运行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgyRunningState {
    pub cli: bool,
    pub desktop: bool,
}

impl AgyRunningState {
    pub fn any(self) -> bool {
        self.cli || self.desktop
    }
}

pub type RunningCheck = Arc<dyn Fn() -> AgyRunningState + Send + Sync>;

/// 正在进行的登录/切号互斥操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgyAuthOp {
    Switching,
    LoginPending,
}

struct SwitchGuard(Arc<AtomicBool>);

impl SwitchGuard {
    fn enter(flag: &Arc<AtomicBool>) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self(Arc::clone(flag))
    }
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 最近一次读到的账号表和 agy 当前登录，见 [`last_known_status`]。
static LAST_KNOWN: std::sync::Mutex<Option<AgyAccountStatus>> = std::sync::Mutex::new(None);

/// 最近一次读到的账号表和 agy 当前登录。托盘建菜单是同步的，也不该每次都起子进程读钥匙串，
/// 就读这份：账号表每次存盘后更新，当前登录在查看状态、切号、查额度读过钥匙串后更新。
/// 还没读过钥匙串时 `current_account_id` 为 `None`。
pub fn last_known_status() -> Option<AgyAccountStatus> {
    LAST_KNOWN.lock_recover().clone()
}

/// 记下账号表；`current` 为 `Some` 时连同刚从钥匙串读到的当前登录一起记，`None` 沿用上次的。
fn remember(store: &StoreFile, current: Option<Option<String>>) {
    let mut last = LAST_KNOWN.lock_recover();
    let current = match current {
        Some(current) => current,
        None => last
            .as_ref()
            .and_then(|status| status.current_account_id.clone()),
    }
    .filter(|id| store.accounts.contains_key(id));
    *last = Some(AgyAccountStatus {
        accounts: sorted_accounts(store),
        current_account_id: current,
        supported: keychain::supported(),
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountData {
    sub: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
    /// agy 钥匙串里那份 JSON 原样保存（以后 agy 新增的字段也跟着走）
    credential: Value,
    authenticated_at: i64,
    #[serde(default)]
    requires_reauth: bool,
    /// agy 登着这个号时最后一次查到的额度（`/usage`），切走后照样显示
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_quota: Option<SubscriptionQuota>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, AccountData>,
    /// 用户在 CC Switch 里删掉、但 agy 还登着的号：查看状态时不再自动收回来。
    /// 切走它时照样收录（切换绝不弄丢登录），收录后就从这里去掉。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dismissed: Vec<String>,
    /// 已删账号的 `sub -> email`（不含令牌）：agy 1.5 钥匙串里不再带 `id_token`，
    /// 只有 `id_token_email`；留着邮箱才能在不发网的情况下认出钥匙串里还是这个被删掉的号，
    /// 切走时也能按原 `sub` 收回。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    dismissed_emails: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgyAccount {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub authenticated_at: i64,
    pub requires_reauth: bool,
}

impl AgyAccount {
    /// 界面上显示的名字：邮箱 > 名字 > `Google (<sub 前 6 位>)`。
    pub fn label(&self) -> String {
        self.email
            .clone()
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| {
                let short: String = self.id.chars().take(6).collect();
                format!("Google ({short})")
            })
    }
}

impl From<&AccountData> for AgyAccount {
    fn from(data: &AccountData) -> Self {
        Self {
            id: data.sub.clone(),
            email: data.email.clone(),
            name: data.name.clone(),
            picture: data.picture.clone(),
            authenticated_at: data.authenticated_at,
            requires_reauth: data.requires_reauth,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgyAccountStatus {
    pub accounts: Vec<AgyAccount>,
    /// agy 当前登录的是哪个已收录的号；没登录、或读不到钥匙串时为 `None`
    pub current_account_id: Option<String>,
    /// 这个平台能不能切号（macOS、Windows 和 Linux）。
    pub supported: bool,
}

#[derive(Debug)]
pub struct LoginStart {
    pub flow_id: String,
    pub expires_in: u64,
}

/// 一次「添加账号」：记着开始时钥匙串里那份登录，变了（或观察到登出后重新登录）就是用户在 agy 里登录了新的。
struct PendingLogin {
    baseline: Option<String>,
    saw_logged_out: bool,
    deadline: Instant,
}

/// 钥匙串里当前那份登录，解出来认得是谁时带上身份。
enum LiveLogin {
    None,
    Known {
        credential: Value,
        claims: identity::IdentityClaims,
    },
    /// 读得到但解不开或认不出账号
    Unrecognized(String),
}

impl LiveLogin {
    /// 用来判断钥匙串里的登录换没换：认得出的按 refresh token，认不出的按原文。
    fn fingerprint(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Known { credential, .. } => refresh_token_of(credential).map(str::to_string),
            Self::Unrecognized(raw) => Some(raw.clone()),
        }
    }
}

pub struct AgyAccountManager {
    data_dir: PathBuf,
    login: Arc<dyn AgyLoginStore>,
    running: RunningCheck,
    desktop_user_data_dir: Option<PathBuf>,
    /// 账号表；同时充当所有改动（含写钥匙串）的互斥锁
    store: Mutex<StoreFile>,
    pending: Mutex<HashMap<String, PendingLogin>>,
    switching: Arc<AtomicBool>,
}

impl AgyAccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let standalone_token = crate::config::get_home_dir()
            .join(".gemini")
            .join(keychain::STANDALONE_TOKEN_FILENAME);
        Self::with_parts(
            data_dir,
            Arc::new(keychain::AgyLogin::new(standalone_token)),
            Arc::new(agy_running_state),
            keychain::default_desktop_user_data_dir(),
        )
    }

    pub fn with_login_store(data_dir: PathBuf, login: Arc<dyn AgyLoginStore>) -> Self {
        Self::with_parts(data_dir, login, Arc::new(AgyRunningState::default), None)
    }

    #[cfg(test)]
    pub(crate) fn with_test_env(
        data_dir: PathBuf,
        login: Arc<dyn AgyLoginStore>,
        running: RunningCheck,
        desktop_user_data_dir: Option<PathBuf>,
    ) -> Self {
        Self::with_parts(data_dir, login, running, desktop_user_data_dir)
    }

    fn with_parts(
        data_dir: PathBuf,
        login: Arc<dyn AgyLoginStore>,
        running: RunningCheck,
        desktop_user_data_dir: Option<PathBuf>,
    ) -> Self {
        let path = data_dir.join(STORE_FILENAME);
        let store = crate::config::load_json_or_set_aside(&path, "AgyAccount");
        remember(&store, None);
        Self {
            data_dir,
            login,
            running,
            desktop_user_data_dir,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
            switching: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_switching(&self) -> bool {
        self.switching.load(Ordering::SeqCst)
    }

    pub async fn has_pending_login(&self) -> bool {
        let now = Instant::now();
        let mut pending = self.pending.lock().await;
        pending.retain(|_, entry| now < entry.deadline);
        !pending.is_empty()
    }

    pub async fn active_auth_operation(&self) -> Option<AgyAuthOp> {
        if self.is_switching() {
            Some(AgyAuthOp::Switching)
        } else if self.has_pending_login().await {
            Some(AgyAuthOp::LoginPending)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn set_switching_for_test(&self, active: bool) {
        self.switching.store(active, Ordering::SeqCst);
    }

    // ---------- 在 agy 里登录 ----------

    /// 开始「添加账号」：先检查 Antigravity 桌面端与 CLI 是否已退出，再把 agy 现在登着的号收进来
    /// （用户接着在 agy 里 `/logout`，原来的登录就只剩 CC Switch 这份），再记下钥匙串现在的样子。
    /// 终端由调用方打开。
    pub async fn start_login(&self) -> Result<LoginStart, AppError> {
        if !keychain::supported() {
            return Err(keychain::unsupported_error());
        }
        let check = Arc::clone(&self.running);
        let running = run_blocking(move || Ok(check())).await?;
        if running.any() {
            return Err(running_login_error(running));
        }
        let baseline = {
            let mut store = self.store.lock().await;
            let live = self.read_live(&store, true).await?;
            let baseline = live.fingerprint();
            if let LiveLogin::Unrecognized(raw) = &live {
                self.stash_unrecognized(raw)?;
            }
            let (current, changed) = absorb(&mut store, live, false);
            if changed {
                self.persist(&store)?;
            }
            remember(&store, Some(current));
            baseline
        };
        let flow_id = uuid::Uuid::new_v4().to_string();
        let mut pending = self.pending.lock().await;
        // 同时只留一个进行中的登录：再点一次「添加账号」就丢掉上一个
        pending.clear();
        pending.insert(
            flow_id.clone(),
            PendingLogin {
                baseline,
                saw_logged_out: false,
                deadline: Instant::now() + LOGIN_TIMEOUT,
            },
        );
        Ok(LoginStart {
            flow_id,
            expires_in: LOGIN_TIMEOUT.as_secs(),
        })
    }

    /// 钥匙串里还是开始时那份（或没登录）时返回 `Ok(None)`；换成认得出的登录（或观察到登出后
    /// 重新登录同一账号）就收录、同步回退文件与 Desktop scope 并返回它。
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<AgyAccount>, AppError> {
        let (baseline, saw_logged_out) = {
            let mut pending = self.pending.lock().await;
            let Some(entry) = pending.get(flow_id) else {
                return Err(login_ended());
            };
            if Instant::now() >= entry.deadline {
                pending.remove(flow_id);
                return Err(login_ended());
            }
            (entry.baseline.clone(), entry.saw_logged_out)
        };
        let result = {
            let mut store = self.store.lock().await;
            let live = match self.read_live(&store, false).await {
                Ok(live) => live,
                Err(error) => {
                    log::debug!("[AgyAccount] 等待登录时读取钥匙串失败: {error}");
                    return Ok(None);
                }
            };
            if matches!(live, LiveLogin::None) {
                drop(store);
                let mut pending = self.pending.lock().await;
                if let Some(entry) = pending.get_mut(flow_id) {
                    entry.saw_logged_out = true;
                }
                return Ok(None);
            }
            if !matches!(live, LiveLogin::Known { .. })
                || (!saw_logged_out && live.fingerprint() == baseline)
            {
                return Ok(None);
            }
            let (id, changed) = absorb(&mut store, live, true);
            let Some(id) = id else {
                return Ok(None);
            };
            if changed {
                self.persist(&store)?;
            }
            remember(&store, Some(Some(id.clone())));
            store
                .accounts
                .get(&id)
                .map(|data| (AgyAccount::from(data), data.credential.clone()))
        };
        if let Some((account, credential)) = result {
            let login = Arc::clone(&self.login);
            let _ = run_blocking(move || {
                login.sync_fallback(&credential);
                Ok(())
            })
            .await;
            self.clear_desktop_credential_scope();
            self.pending.lock().await.remove(flow_id);
            Ok(Some(account))
        } else {
            self.pending.lock().await.remove(flow_id);
            Ok(None)
        }
    }

    pub async fn cancel_login(&self, flow_id: &str) -> bool {
        self.pending.lock().await.remove(flow_id).is_some()
    }

    // ---------- 状态与切换 ----------

    /// 列出账号，并顺手把钥匙串里的最新登录收回来（agy 续期后的 access token、还没收录的号）。
    pub async fn status(&self) -> Result<AgyAccountStatus, AppError> {
        let mut store = self.store.lock().await;
        let mut current = None;
        if keychain::supported() {
            match self.read_live(&store, false).await {
                Ok(live) => {
                    let (id, changed) = absorb(&mut store, live, false);
                    current = id;
                    if changed {
                        self.persist(&store)?;
                    }
                }
                Err(error) => log::warn!("[AgyAccount] 读取 agy 当前登录失败: {error}"),
            }
            remember(&store, Some(current.clone()));
        }
        Ok(AgyAccountStatus {
            accounts: sorted_accounts(&store),
            current_account_id: current,
            supported: keychain::supported(),
        })
    }

    /// 把 agy 的全局登录换成这个账号。已经是它时什么都不写。
    pub async fn switch_to(&self, account_id: &str) -> Result<(), AppError> {
        if !keychain::supported() {
            return Err(keychain::unsupported_error());
        }
        let _switch_guard = SwitchGuard::enter(&self.switching);
        let mut store = self.store.lock().await;
        let target = store
            .accounts
            .get(account_id)
            .cloned()
            .ok_or_else(|| account_not_found(account_id))?;
        if target.requires_reauth {
            return Err(AppError::localized(
                "agy.account.requires_reauth",
                "这个账号的登录已失效，请重新添加",
                "This account's sign-in has expired. Add it again.",
            ));
        }

        let live = self.read_live(&store, true).await?;
        if let LiveLogin::Unrecognized(raw) = &live {
            self.stash_unrecognized(raw)?;
        }
        let (current, changed) = absorb(&mut store, live, true);
        if changed {
            self.persist(&store)?;
        }
        if current.as_deref() == Some(account_id) {
            remember(&store, Some(current));
            return Ok(());
        }

        let check = Arc::clone(&self.running);
        let running = run_blocking(move || Ok(check())).await?;
        if running.any() {
            return Err(running_switch_error(running));
        }

        let credential = target.credential;
        let expected = refresh_token_of(&credential).map(str::to_string);
        let login = Arc::clone(&self.login);
        let credential_for_write = credential.clone();
        run_blocking(move || login.write_credential(&credential_for_write)).await?;

        // 核验失败时只重读，避免再次覆盖 agy 可能已经更新过的登录。
        for _ in 0..2 {
            if let Ok(LiveLogin::Known { credential, .. }) = self.read_live(&store, false).await {
                let written = refresh_token_of(&credential);
                if written.is_some() && written == expected.as_deref() {
                    let login = Arc::clone(&self.login);
                    let fallback_credential = credential.clone();
                    let _ = run_blocking(move || {
                        login.sync_fallback(&fallback_credential);
                        Ok(())
                    })
                    .await;
                    self.clear_desktop_credential_scope();
                    log::info!("[AgyAccount] 已切换 Antigravity 登录");
                    remember(&store, Some(Some(account_id.to_string())));
                    return Ok(());
                }
            }
        }
        Err(AppError::localized(
            "agy.account.write_verify_failed",
            "已写入钥匙串，但无法确认 agy 当前登录的账号。请先在授权中心检查当前登录，再重试。",
            "The keychain write completed, but agy's current account could not be verified. Check the current sign-in in Auth Center before retrying.",
        ))
    }

    /// 只删 CC Switch 里的副本。删的是 agy 正登着的号时记下来，免得查看状态时又被收回来。
    pub async fn remove(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.store.lock().await;
        let Some(removed) = store.accounts.remove(account_id) else {
            return Err(account_not_found(account_id));
        };
        if !store.dismissed.iter().any(|id| id == account_id) {
            store.dismissed.push(account_id.to_string());
        }
        if let Some(email) = removed.email.filter(|email| !email.is_empty()) {
            store.dismissed_emails.insert(account_id.to_string(), email);
        }
        self.persist(&store)
    }

    pub async fn remove_all(&self) -> Result<Vec<String>, AppError> {
        let mut store = self.store.lock().await;
        let removed: Vec<String> = store.accounts.keys().cloned().collect();
        for (id, account) in std::mem::take(&mut store.accounts) {
            if !store.dismissed.contains(&id) {
                store.dismissed.push(id.clone());
            }
            if let Some(email) = account.email.filter(|email| !email.is_empty()) {
                store.dismissed_emails.insert(id, email);
            }
        }
        self.persist(&store)?;
        Ok(removed)
    }

    pub async fn list(&self) -> Vec<AgyAccount> {
        sorted_accounts(&*self.store.lock().await)
    }

    // ---------- 额度 ----------

    /// agy 现在登着哪个已收录的号（顺手把钥匙串里的登录收进账号表）。没登录、读不到、
    /// 格式不认识、或用户在 CC Switch 里删过这个号时为 `None`。
    pub async fn current_account_id(&self) -> Option<String> {
        if !keychain::supported() {
            return None;
        }
        let mut store = self.store.lock().await;
        let live = match self.read_live(&store, false).await {
            Ok(live) => live,
            Err(error) => {
                log::warn!("[AgyAccount] 读取 agy 当前登录失败: {error}");
                return None;
            }
        };
        let (id, changed) = absorb(&mut store, live, false);
        if changed {
            if let Err(error) = self.persist(&store) {
                log::warn!("[AgyAccount] 保存账号文件失败: {error}");
            }
        }
        remember(&store, Some(id.clone()));
        id
    }

    /// 记下 agy 登着这个号时查到的额度。
    pub async fn record_quota(&self, account_id: &str, quota: &SubscriptionQuota) {
        let mut store = self.store.lock().await;
        let Some(account) = store.accounts.get_mut(account_id) else {
            return;
        };
        account.last_quota = Some(quota.clone());
        if let Err(error) = self.persist(&store) {
            log::warn!("[AgyAccount] 保存账号文件失败: {error}");
        }
    }

    /// 这个号最后一次查到的额度（可能是很久以前的，看 `queried_at`）。
    pub async fn last_quota(&self, account_id: &str) -> Option<SubscriptionQuota> {
        self.store
            .lock()
            .await
            .accounts
            .get(account_id)?
            .last_quota
            .clone()
    }

    // ---------- 内部 ----------

    /// 读钥匙串里 agy 当前的登录并认出是谁（只看本地数据，见 [`identify`]）。
    async fn read_live(
        &self,
        store: &StoreFile,
        allow_unlock: bool,
    ) -> Result<LiveLogin, AppError> {
        let login = Arc::clone(&self.login);
        let raw = run_blocking(move || {
            if allow_unlock {
                login.read_for_switch()
            } else {
                login.read()
            }
        })
        .await?;
        let Some(raw) = raw else {
            return Ok(LiveLogin::None);
        };
        let Some(credential) = keychain::decode(&raw)
            .ok()
            .filter(|credential| refresh_token_of(credential).is_some())
        else {
            return Ok(LiveLogin::Unrecognized(raw));
        };
        Ok(match identify(store, &credential) {
            Some(claims) => LiveLogin::Known { credential, claims },
            None => LiveLogin::Unrecognized(raw),
        })
    }

    fn stash_unrecognized(&self, raw: &str) -> Result<(), AppError> {
        let content = serde_json::to_vec_pretty(&serde_json::json!({
            "raw": raw,
            "saved_at": chrono::Utc::now().timestamp_millis(),
        }))
        .map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&self.data_dir.join(STASH_FILENAME), &content)
    }

    fn persist(&self, store: &StoreFile) -> Result<(), AppError> {
        let snapshot = StoreFile {
            version: 1,
            accounts: store.accounts.clone(),
            dismissed: store.dismissed.clone(),
            dismissed_emails: store.dismissed_emails.clone(),
        };
        let content = serde_json::to_vec_pretty(&snapshot)
            .map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&self.data_dir.join(STORE_FILENAME), &content)?;
        remember(store, None);
        Ok(())
    }

    fn clear_desktop_credential_scope(&self) {
        if let Some(dir) = &self.desktop_user_data_dir {
            keychain::clear_desktop_user_scope_in(dir);
        }
    }
}

fn running_switch_error(running: AgyRunningState) -> AppError {
    if running.desktop && running.cli {
        AppError::localized(
            "agy.account.running_both",
            "请先完全退出 Antigravity 桌面端和 agy CLI 再切换账号，否则运行中的进程不会刷新且可能把旧登录写回去。",
            "Quit Antigravity Desktop and agy CLI completely before switching accounts, or running sessions will keep the previous sign-in and may write it back.",
        )
    } else if running.desktop {
        AppError::localized(
            "agy.account.running_desktop",
            "请先完全退出 Antigravity 桌面端（含托盘/后台进程）再切换账号，否则它不会刷新且可能把旧登录写回去。",
            "Quit Antigravity Desktop completely (including tray/background processes) before switching accounts, or it will not refresh and may write its current sign-in back.",
        )
    } else {
        AppError::localized(
            "agy.account.running_cli",
            "请先退出正在运行的 agy CLI 再切换账号，否则运行中的会话仍会使用旧账号并可能把它写回去。",
            "Quit running agy CLI sessions before switching accounts, or they will keep using the previous sign-in and may write it back.",
        )
    }
}

fn running_login_error(running: AgyRunningState) -> AppError {
    if running.desktop && running.cli {
        AppError::localized(
            "agy.account.login_running_both",
            "请先完全退出 Antigravity 桌面端和 agy CLI 再添加或重新登录账号，否则运行中的进程可能把旧登录写回去。",
            "Quit Antigravity Desktop and agy CLI completely before adding or re-signing in, or running sessions may write the previous sign-in back.",
        )
    } else if running.desktop {
        AppError::localized(
            "agy.account.login_running_desktop",
            "请先完全退出 Antigravity 桌面端（含托盘/后台进程）再添加或重新登录账号，否则它可能把旧登录写回去。",
            "Quit Antigravity Desktop completely (including tray/background processes) before adding or re-signing in, or it may write its current sign-in back.",
        )
    } else {
        AppError::localized(
            "agy.account.login_running_cli",
            "请先退出正在运行的 agy CLI 再添加或重新登录账号，否则运行中的会话可能把旧登录写回去。",
            "Quit running agy CLI sessions before adding or re-signing in, or they may write the previous sign-in back.",
        )
    }
}

/// 把钥匙串里的登录并进账号表，返回它对应的账号 id 和表有没有变。
/// `import_dismissed`：切换时为 true，被用户删过的号也收回来（切走不能弄丢登录）。
fn absorb(
    store: &mut StoreFile,
    live: LiveLogin,
    import_dismissed: bool,
) -> (Option<String>, bool) {
    let LiveLogin::Known { credential, claims } = live else {
        return (None, false);
    };
    let sub = claims.sub.clone().unwrap_or_default();
    if let Some(existing) = store.accounts.get_mut(&sub) {
        let mut changed = false;
        if existing.credential != credential {
            existing.credential = credential;
            existing.requires_reauth = false;
            changed = true;
        }
        // 续期后的 id_token 不带名字和头像，有新值才更新
        changed |= fill(&mut existing.email, claims.email);
        changed |= fill(&mut existing.name, claims.name);
        changed |= fill(&mut existing.picture, claims.picture);
        return (Some(sub), changed);
    }
    let dismissed = store.dismissed.contains(&sub);
    if dismissed && !import_dismissed {
        // 旧版删过的号只有 `dismissed` 里的 sub、没有邮箱：经 id_token 认出后顺手把邮箱
        // 补进 `dismissed_emails`，agy 换成只存邮箱的格式后也能靠 `match_stored` 认出来。
        let changed = claims
            .email
            .or_else(|| live_email(&credential))
            .filter(|email| !email.is_empty())
            .is_some_and(|email| {
                if store.dismissed_emails.get(&sub) == Some(&email) {
                    false
                } else {
                    store.dismissed_emails.insert(sub, email);
                    true
                }
            });
        return (None, changed);
    }
    store.dismissed.retain(|id| *id != sub);
    store.dismissed_emails.remove(&sub);
    upsert(store, credential, &claims);
    (Some(sub), true)
}

fn fill(slot: &mut Option<String>, value: Option<String>) -> bool {
    match value.filter(|v| !v.is_empty()) {
        Some(value) if slot.as_deref() != Some(value.as_str()) => {
            *slot = Some(value);
            true
        }
        _ => false,
    }
}

fn upsert(
    store: &mut StoreFile,
    credential: Value,
    claims: &identity::IdentityClaims,
) -> AgyAccount {
    let sub = claims.sub.clone().unwrap_or_default();
    store.dismissed.retain(|id| *id != sub);
    store.dismissed_emails.remove(&sub);
    let now = chrono::Utc::now().timestamp();
    let entry = store
        .accounts
        .entry(sub.clone())
        .or_insert_with(|| AccountData {
            sub: sub.clone(),
            email: None,
            name: None,
            picture: None,
            credential: Value::Null,
            authenticated_at: now,
            requires_reauth: false,
            last_quota: None,
        });
    entry.credential = credential;
    entry.requires_reauth = false;
    entry.authenticated_at = now;
    fill(&mut entry.email, claims.email.clone());
    fill(&mut entry.name, claims.name.clone());
    fill(&mut entry.picture, claims.picture.clone());
    AgyAccount::from(&*entry)
}

fn sorted_accounts(store: &StoreFile) -> Vec<AgyAccount> {
    let mut accounts: Vec<AgyAccount> = store.accounts.values().map(AgyAccount::from).collect();
    accounts.sort_by_key(|account| account.authenticated_at);
    accounts
}

fn refresh_token_of(credential: &Value) -> Option<&str> {
    credential
        .pointer("/token/refresh_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
}

/// agy 1.5 起写进钥匙串的邮箱（替代了 id_token）；没有 `id_token_email` 时兜底从 `id_token` 解出 `email`。
fn live_email(credential: &Value) -> Option<String> {
    credential
        .get("id_token_email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string)
        .or_else(|| {
            credential["id_token"]
                .as_str()
                .and_then(identity::identity)
                .and_then(|claims| claims.email)
                .map(|email| email.trim().to_string())
                .filter(|email| !email.is_empty())
        })
}

/// 这份登录对应哪个已收录（或已删但仍在钥匙串里）的号：同一个 refresh token 优先，
/// 其次邮箱相同（在 agy 里重新登录过同一个号，或刚在 CC Switch 里删掉的号）。
fn match_stored(store: &StoreFile, credential: &Value) -> Option<identity::IdentityClaims> {
    let refresh_token = refresh_token_of(credential)?;
    let email = live_email(credential);
    if let Some(account) = store
        .accounts
        .values()
        .find(|account| refresh_token_of(&account.credential) == Some(refresh_token))
        .or_else(|| {
            let email = email.as_deref()?;
            store.accounts.values().find(|account| {
                account
                    .email
                    .as_deref()
                    .is_some_and(|stored| stored.eq_ignore_ascii_case(email))
            })
        })
    {
        return Some(identity::IdentityClaims {
            sub: Some(account.sub.clone()),
            email: email.or_else(|| account.email.clone()),
            name: None,
            picture: None,
        });
    }
    let email = email?;
    let (sub, stored_email) = store
        .dismissed_emails
        .iter()
        .find(|(_, stored)| stored.eq_ignore_ascii_case(&email))?;
    Some(identity::IdentityClaims {
        sub: Some(sub.clone()),
        email: Some(stored_email.clone()),
        name: None,
        picture: None,
    })
}

/// 钥匙串里这份登录是谁：新凭据自带 `id_token.sub` 且仓库已有（或已删）对应 `sub` 键时优先精确匹配，
/// 消除 `sub` 与 `email:` 混用产生的重复账号；否则再按已收录（或已删）的 refresh token / 邮箱对回，
/// 再没有就用 `id_token` 或 `id_token_email` 当 id。都没有的认不出。
fn identify(store: &StoreFile, credential: &Value) -> Option<identity::IdentityClaims> {
    let token_claims = credential["id_token"].as_str().and_then(identity::identity);
    if let Some(claims) = token_claims.as_ref() {
        if let Some(sub) = claims.sub.as_deref() {
            if store.accounts.contains_key(sub) || store.dismissed.iter().any(|id| id == sub) {
                return Some(claims.clone());
            }
        }
    }
    match (match_stored(store, credential), token_claims) {
        (Some(stored), Some(mut claims)) => {
            claims.sub = stored.sub;
            claims.email = claims.email.or(stored.email);
            Some(claims)
        }
        (Some(stored), None) => Some(stored),
        (None, Some(claims)) => Some(claims),
        (None, None) => live_email(credential).map(|email| identity::IdentityClaims {
            sub: Some(identity::email_account_id(&email)),
            email: Some(email),
            name: None,
            picture: None,
        }),
    }
}

fn login_ended() -> AppError {
    AppError::localized(
        "agy.account.login_expired",
        "这次登录已结束，请重新添加账号",
        "This sign-in has ended. Add the account again.",
    )
}

async fn run_blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|error| AppError::Message(format!("钥匙串操作中断: {error}")))?
}

fn account_not_found(account_id: &str) -> AppError {
    AppError::Message(format!("找不到这个 Antigravity 账号: {account_id}"))
}

/// Antigravity 桌面端或 `agy` CLI 有没有在跑。在跑的进程会继续用切换前的账号，续期时还可能把旧登录写回钥匙串。
pub fn agy_running_state() -> AgyRunningState {
    #[cfg(target_os = "macos")]
    {
        let Ok(output) = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .stdin(std::process::Stdio::null())
            .output()
        else {
            return AgyRunningState::default();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        classify_unix_process_lines(text.lines())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux 的 `ps -axo comm=` 会把进程名截断到 15 字节（`language_server_linux_x64` 会变成 `language_server`），
        // 改用 `args=` 保留完整可执行路径。
        let Ok(output) = std::process::Command::new("ps")
            .args(["-axo", "args="])
            .stdin(std::process::Stdio::null())
            .output()
        else {
            return AgyRunningState::default();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        classify_unix_process_lines(text.lines())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .output()
        else {
            return AgyRunningState::default();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let entries = parse_windows_tasklist_entries(&text);
        let mut state = AgyRunningState::default();
        let mut desktop_pids = Vec::new();
        let mut language_server_pids = Vec::new();
        for (image, pid) in entries {
            match classify_windows_image_name(&image) {
                WindowsProcessKind::Cli => state.cli = true,
                WindowsProcessKind::Desktop => desktop_pids.push(pid),
                WindowsProcessKind::LanguageServer => language_server_pids.push(pid),
                WindowsProcessKind::Other => {}
            }
        }
        for pid in desktop_pids {
            if let Some(path) = windows_process_image_path(pid) {
                if !is_antigravity_ide_path(&path) {
                    state.desktop = true;
                    break;
                }
            } else {
                // 无法读取提权进程路径时，按 Antigravity.exe 保守视为桌面端在运行。
                state.desktop = true;
                break;
            }
        }
        if !state.desktop {
            state.desktop = language_server_pids
                .into_iter()
                .filter_map(windows_process_image_path)
                .any(|path| is_antigravity_desktop_language_server_path(&path));
        }
        state
    }
    #[cfg(not(any(unix, windows)))]
    {
        AgyRunningState::default()
    }
}

pub fn agy_running() -> bool {
    agy_running_state().any()
}

#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
fn is_antigravity_ide_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("antigravity-ide") || lower.contains("antigravity ide")
}

#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
fn is_antigravity_desktop_language_server_path(path: &str) -> bool {
    if is_antigravity_ide_path(path) {
        return false;
    }
    path.to_ascii_lowercase()
        .split(['\\', '/'])
        .any(|segment| segment == "antigravity" || segment == "antigravity.app")
}

#[cfg_attr(not(unix), allow(dead_code))]
fn unix_executable_token(line: &str) -> &str {
    let trimmed = line.trim();
    if let Some(app_idx) = trimmed.find(".app/") {
        let after_app = &trimmed[app_idx + 5..];
        let end_rel = after_app.find(" --").or_else(|| after_app.find(" -"));
        return match end_rel {
            Some(rel) => trimmed[..app_idx + 5 + rel].trim_end(),
            None => trimmed,
        };
    }
    trimmed.split_whitespace().next().unwrap_or(trimmed)
}

#[cfg_attr(not(unix), allow(dead_code))]
fn classify_unix_process_lines<'a>(lines: impl Iterator<Item = &'a str>) -> AgyRunningState {
    let mut state = AgyRunningState::default();
    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let exe = unix_executable_token(trimmed);
        let path = std::path::Path::new(exe);
        let file_name = path.file_name().and_then(|name| name.to_str());
        if file_name == Some("agy") {
            state.cli = true;
            continue;
        }
        let in_ide_bundle = path.components().any(|part| {
            let s = part.as_os_str().to_string_lossy();
            s.eq_ignore_ascii_case("Antigravity-IDE.app")
                || s.eq_ignore_ascii_case("Antigravity IDE.app")
        }) || is_antigravity_ide_path(exe);
        if in_ide_bundle {
            continue;
        }
        let in_desktop_bundle = path
            .components()
            .any(|part| part.as_os_str() == "Antigravity.app");
        let is_antigravity_bin =
            file_name.is_some_and(|name| name.eq_ignore_ascii_case("antigravity"));
        let is_antigravity_ls = file_name
            .is_some_and(|name| name == "language_server" || name.starts_with("language_server_"))
            && is_antigravity_desktop_language_server_path(exe);
        if in_desktop_bundle || is_antigravity_bin || is_antigravity_ls {
            state.desktop = true;
        }
    }
    state
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsProcessKind {
    Cli,
    Desktop,
    LanguageServer,
    Other,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn classify_windows_image_name(image: &str) -> WindowsProcessKind {
    if image.eq_ignore_ascii_case("agy.exe") {
        WindowsProcessKind::Cli
    } else if image.eq_ignore_ascii_case("Antigravity.exe") {
        WindowsProcessKind::Desktop
    } else {
        let lower = image.to_ascii_lowercase();
        if lower == "language_server.exe"
            || (lower.starts_with("language_server_windows") && lower.ends_with(".exe"))
        {
            WindowsProcessKind::LanguageServer
        } else {
            WindowsProcessKind::Other
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_windows_tasklist_entries(output: &str) -> Vec<(String, u32)> {
    output
        .lines()
        .filter_map(|line| {
            let mut cols = line.split("\",\"").map(|col| col.trim_matches('"').trim());
            let image = cols.next()?;
            let pid = cols.next()?.parse::<u32>().ok()?;
            if image.is_empty() {
                None
            } else {
                Some((image.to_string(), pid))
            }
        })
        .collect()
}

#[cfg(windows)]
fn windows_process_image_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess opens a handle with limited query rights; we always close it before returning.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buf is valid for len u16 elements and len receives the written character count.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
    // SAFETY: handle is non-null and no longer needed.
    unsafe { CloseHandle(handle) };
    if ok == 0 || len == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

#[cfg(test)]
mod tests;
