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

pub struct LoginStart {
    pub flow_id: String,
    pub expires_in: u64,
}

/// 一次「添加账号」：记着开始时钥匙串里那份登录，变了就是用户在 agy 里登录了新的。
struct PendingLogin {
    baseline: Option<String>,
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
    /// 账号表；同时充当所有改动（含写钥匙串）的互斥锁
    store: Mutex<StoreFile>,
    pending: Mutex<HashMap<String, PendingLogin>>,
}

impl AgyAccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_login_store(data_dir, Arc::new(keychain::SystemKeychain::agy()))
    }

    pub fn with_login_store(data_dir: PathBuf, login: Arc<dyn AgyLoginStore>) -> Self {
        let path = data_dir.join(STORE_FILENAME);
        let store = crate::config::load_json_or_set_aside(&path, "AgyAccount");
        remember(&store, None);
        Self {
            data_dir,
            login,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
        }
    }

    // ---------- 在 agy 里登录 ----------

    /// 开始「添加账号」：先把 agy 现在登着的号收进来（用户接着在 agy 里 `/logout`，
    /// 原来的登录就只剩 CC Switch 这份），再记下钥匙串现在的样子。终端由调用方打开。
    pub async fn start_login(&self) -> Result<LoginStart, AppError> {
        if !keychain::supported() {
            return Err(keychain::unsupported_error());
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
                deadline: Instant::now() + LOGIN_TIMEOUT,
            },
        );
        Ok(LoginStart {
            flow_id,
            expires_in: LOGIN_TIMEOUT.as_secs(),
        })
    }

    /// 钥匙串里还是开始时那份（或没登录）时返回 `Ok(None)`；换成认得出的登录就收录并返回它。
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<AgyAccount>, AppError> {
        let baseline = {
            let mut pending = self.pending.lock().await;
            let Some(entry) = pending.get(flow_id) else {
                return Err(login_ended());
            };
            if Instant::now() >= entry.deadline {
                pending.remove(flow_id);
                return Err(login_ended());
            }
            entry.baseline.clone()
        };
        let account = {
            let mut store = self.store.lock().await;
            let live = match self.read_live(&store, false).await {
                Ok(live) => live,
                Err(error) => {
                    log::debug!("[AgyAccount] 等待登录时读取钥匙串失败: {error}");
                    return Ok(None);
                }
            };
            if !matches!(live, LiveLogin::Known { .. }) || live.fingerprint() == baseline {
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
            store.accounts.get(&id).map(AgyAccount::from)
        };
        self.pending.lock().await.remove(flow_id);
        Ok(account)
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

        let credential = target.credential;
        let expected = refresh_token_of(&credential).map(str::to_string);
        let encoded = keychain::encode(&credential)?;
        let login = Arc::clone(&self.login);
        run_blocking(move || login.write(&encoded)).await?;

        // 核验失败时只重读，避免再次覆盖 agy 可能已经更新过的登录。
        for _ in 0..2 {
            if let Ok(LiveLogin::Known { credential, .. }) = self.read_live(&store, false).await {
                let written = refresh_token_of(&credential);
                if written.is_some() && written == expected.as_deref() {
                    log::info!("[AgyAccount] 已切换 agy 登录");
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

/// agy 1.5 起写进钥匙串的邮箱（替代了 id_token）。
fn live_email(credential: &Value) -> Option<String> {
    credential
        .get("id_token_email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string)
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

/// 钥匙串里这份登录是谁：先对已收录（或已删）的号——同一个 refresh token、再同一个邮箱，
/// 账号 id 沿用已有的；对不上时看 id_token 的 `sub`（agy 1.5 以前）；再没有就用
/// `id_token_email` 当 id（agy 1.5 起只存邮箱）。都没有的认不出。
fn identify(store: &StoreFile, credential: &Value) -> Option<identity::IdentityClaims> {
    let token_claims = credential["id_token"].as_str().and_then(identity::identity);
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

/// agy 有没有在跑。在跑的 agy 会继续用切换前的账号，续期时还可能把旧登录写回钥匙串。
pub fn agy_running() -> bool {
    #[cfg(unix)]
    {
        let Ok(output) = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .stdin(std::process::Stdio::null())
            .output()
        else {
            return false;
        };
        String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            std::path::Path::new(line.trim())
                .file_name()
                .is_some_and(|name| name == "agy")
        })
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
