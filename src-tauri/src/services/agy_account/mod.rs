//! Antigravity CLI 的官方账号：在 CC Switch 里加号、切号。
//!
//! agy 只有一份全局登录，存在系统钥匙串里（见 `keychain`）。CC Switch 自己存每个账号的整份
//! 登录数据（`agy_accounts.json`，0600，不随云同步），切换就是把目标账号那份写进钥匙串，
//! agy 下次启动就是这个号。账号按 id_token 的 `sub` 区分（邮箱可能相同前缀，名字可改）。
//!
//! 几条规矩：
//! - 写之前先把钥匙串里现在那份收回来：是已收录的号就更新它（agy 续期后写回的新 access
//!   token）；不认识的号直接收录，切走也不会丢。认不出格式的整条暂存到 `agy-login-stash.json`。
//! - 从 CC Switch 删账号只删自己的副本，永远不动钥匙串（不替 agy 登出）。
//! - 不自动换号：额度用完、请求出错都不切，切换只由用户触发。
//! - 查额度时令牌快过期就续期，续期结果只存 CC Switch 的副本，不写钥匙串：agy 在用的号由
//!   agy 自己续期（Google 不轮换 refresh token，两边各自的 access token 同时有效）。

pub mod keychain;
pub mod oauth;
pub mod quota;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{oneshot, Mutex};

use crate::error::AppError;
use crate::services::subscription::QuotaTier;
use keychain::AgyLoginStore;
use oauth::{AgyOAuthError, TokenResponse};

const STORE_FILENAME: &str = "agy_accounts.json";
const STASH_FILENAME: &str = "agy-login-stash.json";
/// 浏览器登录最多等这么久，过了就当放弃。
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// access token 剩不到这么久就先续期再查额度。
const TOKEN_MARGIN: Duration = Duration::from_secs(120);

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
    /// 这个平台能不能切号（目前只有 macOS）
    pub supported: bool,
}

/// 查某个账号额度失败的原因。
#[derive(Debug)]
pub enum AccountQuotaError {
    NotFound,
    /// refresh token 失效，只能重新登录
    NeedsReauth,
    Failed(String),
}

pub struct LoginStart {
    pub flow_id: String,
    pub auth_url: String,
    pub expires_in: u64,
}

struct PendingLogin {
    result: oneshot::Receiver<Result<TokenResponse, AgyOAuthError>>,
    task: tokio::task::AbortHandle,
}

/// 钥匙串里当前那份登录，解出来认得是谁时带上身份。
enum LiveLogin {
    None,
    Known {
        credential: Value,
        claims: oauth::IdentityClaims,
    },
    /// 读得到但解不开或认不出账号
    Unrecognized(String),
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
        let store = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("[AgyAccount] 账号文件解析失败，按空表处理: {error}");
                StoreFile::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreFile::default(),
            Err(error) => {
                log::warn!("[AgyAccount] 账号文件读取失败，按空表处理: {error}");
                StoreFile::default()
            }
        };
        Self {
            data_dir,
            login,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
        }
    }

    // ---------- 浏览器登录 ----------

    pub async fn start_login(&self) -> Result<LoginStart, AppError> {
        if !keychain::supported() {
            return Err(keychain::unsupported_error());
        }
        let (listener, port) = oauth::bind_callback()
            .await
            .map_err(|error| AppError::Message(error.to_string()))?;
        let pkce = oauth::PkcePair::generate();
        let redirect_uri = oauth::redirect_uri(port);
        let auth_url = oauth::authorization_url(&redirect_uri, &pkce);
        let flow_id = pkce.state.clone();

        let (sender, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let outcome = match tokio::time::timeout(
                LOGIN_TIMEOUT,
                oauth::wait_for_code(listener, &pkce.state),
            )
            .await
            {
                Ok(Ok(code)) => oauth::exchange_code(&code, &redirect_uri, &pkce.verifier).await,
                Ok(Err(error)) => Err(error),
                Err(_) => Err(AgyOAuthError::Denied("timeout".into())),
            };
            let _ = sender.send(outcome);
        });

        let mut pending = self.pending.lock().await;
        // 同时只留一个进行中的登录：再点一次「添加账号」就丢掉上一个
        for (_, old) in pending.drain() {
            old.task.abort();
        }
        pending.insert(
            flow_id.clone(),
            PendingLogin {
                result: receiver,
                task: task.abort_handle(),
            },
        );
        Ok(LoginStart {
            flow_id,
            auth_url,
            expires_in: LOGIN_TIMEOUT.as_secs(),
        })
    }

    /// 还没回来时返回 `Ok(None)`；成功时收录账号并返回它。
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<AgyAccount>, AppError> {
        let outcome = {
            let mut pending = self.pending.lock().await;
            let Some(entry) = pending.get_mut(flow_id) else {
                return Err(AppError::localized(
                    "agy.account.login_expired",
                    "这次登录已结束，请重新添加账号",
                    "This sign-in has ended. Add the account again.",
                ));
            };
            match entry.result.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => return Ok(None),
                Err(oneshot::error::TryRecvError::Closed) => {
                    pending.remove(flow_id);
                    return Err(AppError::Message("登录被中断".into()));
                }
                Ok(outcome) => {
                    pending.remove(flow_id);
                    outcome
                }
            }
        };
        let tokens = outcome.map_err(|error| AppError::Message(error.to_string()))?;
        let credential = oauth::new_credential(&tokens).ok_or_else(|| {
            AppError::localized(
                "agy.account.missing_refresh_token",
                "Google 没有返回长期登录凭据，请重新添加并在授权页同意全部权限",
                "Google did not return a long-lived credential. Add the account again and accept all permissions.",
            )
        })?;
        let claims = credential["id_token"]
            .as_str()
            .and_then(oauth::identity)
            .ok_or_else(|| AppError::Message("Google 返回的身份信息不完整".into()))?;
        let mut store = self.store.lock().await;
        let account = upsert(&mut store, credential, &claims);
        self.persist(&store)?;
        Ok(Some(account))
    }

    pub async fn cancel_login(&self, flow_id: &str) -> bool {
        match self.pending.lock().await.remove(flow_id) {
            Some(entry) => {
                entry.task.abort();
                true
            }
            None => false,
        }
    }

    // ---------- 状态与切换 ----------

    /// 列出账号，并顺手把钥匙串里的最新登录收回来（agy 续期后的 access token、还没收录的号）。
    pub async fn status(&self) -> Result<AgyAccountStatus, AppError> {
        let mut store = self.store.lock().await;
        let mut current = None;
        if keychain::supported() {
            match self.read_live().await {
                Ok(live) => {
                    let (id, changed) = absorb(&mut store, live, false);
                    current = id;
                    if changed {
                        self.persist(&store)?;
                    }
                }
                Err(error) => log::warn!("[AgyAccount] 读取 agy 当前登录失败: {error}"),
            }
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

        let live = self.read_live().await?;
        if let LiveLogin::Unrecognized(raw) = &live {
            self.stash_unrecognized(raw)?;
        }
        let (current, changed) = absorb(&mut store, live, true);
        if changed {
            self.persist(&store)?;
        }
        if current.as_deref() == Some(account_id) {
            return Ok(());
        }

        let credential = target.credential;
        let expected = refresh_token_of(&credential).map(str::to_string);
        let encoded = keychain::encode(&credential)?;
        let login = Arc::clone(&self.login);
        run_blocking(move || login.write(&encoded)).await?;

        // 写完读回来核对，钥匙串偶尔会静默失败
        let written = match self.read_live().await? {
            LiveLogin::Known { credential, .. } => {
                refresh_token_of(&credential).map(str::to_string)
            }
            _ => None,
        };
        if written.is_none() || written != expected {
            return Err(AppError::localized(
                "agy.account.write_verify_failed",
                "写入后核对不一致，agy 的登录可能没有切换成功",
                "The written sign-in did not verify; agy may not have switched accounts.",
            ));
        }
        log::info!("[AgyAccount] 已切换 agy 登录");
        Ok(())
    }

    /// 只删 CC Switch 里的副本。删的是 agy 正登着的号时记下来，免得查看状态时又被收回来。
    pub async fn remove(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.store.lock().await;
        if store.accounts.remove(account_id).is_none() {
            return Err(account_not_found(account_id));
        }
        if !store.dismissed.iter().any(|id| id == account_id) {
            store.dismissed.push(account_id.to_string());
        }
        self.persist(&store)
    }

    pub async fn remove_all(&self) -> Result<Vec<String>, AppError> {
        let mut store = self.store.lock().await;
        let removed: Vec<String> = store.accounts.keys().cloned().collect();
        for id in &removed {
            if !store.dismissed.contains(id) {
                store.dismissed.push(id.clone());
            }
        }
        store.accounts.clear();
        self.persist(&store)?;
        Ok(removed)
    }

    pub async fn list(&self) -> Vec<AgyAccount> {
        sorted_accounts(&*self.store.lock().await)
    }

    // ---------- 额度 ----------

    /// 用这个账号自己的令牌查剩余额度（不用先切号）。令牌被拒时强制续期再试一次。
    pub async fn quota(&self, account_id: &str) -> Result<Vec<QuotaTier>, AccountQuotaError> {
        let token = self.access_token(account_id).await?;
        match quota::fetch(&token).await {
            Ok(tiers) => Ok(tiers),
            Err(quota::QuotaError::Unauthorized) => {
                let token = self.refresh_account(account_id).await?;
                quota::fetch(&token)
                    .await
                    .map_err(|error| AccountQuotaError::Failed(error.to_string()))
            }
            Err(error) => Err(AccountQuotaError::Failed(error.to_string())),
        }
    }

    /// agy 现在登着的号的额度（跟随 agy 登录的卡片用）。钥匙串里没有认得出的已收录账号时
    /// 返回 `None`（没登录、读不到、格式不认识、或用户在 CC Switch 里删过这个号），由调用方
    /// 改问 agy 自己。
    pub async fn live_quota(&self) -> Option<Result<Vec<QuotaTier>, AccountQuotaError>> {
        let account_id = self.live_account_id().await?;
        Some(self.quota(&account_id).await)
    }

    /// 把钥匙串里的登录收进账号表，返回它对应的已收录账号。
    async fn live_account_id(&self) -> Option<String> {
        if !keychain::supported() {
            return None;
        }
        let mut store = self.store.lock().await;
        let live = match self.read_live().await {
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
        id
    }

    /// 这个账号还能用的 access token。agy 在用的号先从钥匙串收回 agy 续过的那份。
    async fn access_token(&self, account_id: &str) -> Result<String, AccountQuotaError> {
        {
            let mut store = self.store.lock().await;
            if keychain::supported() {
                if let Ok(live) = self.read_live().await {
                    if absorb(&mut store, live, false).1 {
                        if let Err(error) = self.persist(&store) {
                            log::warn!("[AgyAccount] 保存账号文件失败: {error}");
                        }
                    }
                }
            }
            let account = store
                .accounts
                .get(account_id)
                .ok_or(AccountQuotaError::NotFound)?;
            if account.requires_reauth {
                return Err(AccountQuotaError::NeedsReauth);
            }
            if let Some(token) = oauth::usable_access_token(&account.credential, TOKEN_MARGIN) {
                return Ok(token.to_string());
            }
        }
        self.refresh_account(account_id).await
    }

    /// 续期并存回 CC Switch 的副本（不写钥匙串）。续期时不持锁，存回前确认副本没被换掉。
    async fn refresh_account(&self, account_id: &str) -> Result<String, AccountQuotaError> {
        let refresh_token = {
            let store = self.store.lock().await;
            let account = store
                .accounts
                .get(account_id)
                .ok_or(AccountQuotaError::NotFound)?;
            refresh_token_of(&account.credential)
                .ok_or(AccountQuotaError::NeedsReauth)?
                .to_string()
        };
        let outcome = oauth::refresh(&refresh_token).await;
        let mut store = self.store.lock().await;
        let account = store.accounts.get_mut(account_id).filter(|account| {
            refresh_token_of(&account.credential) == Some(refresh_token.as_str())
        });
        let result = match outcome {
            Ok(tokens) => {
                if let Some(account) = account {
                    oauth::apply_refresh(&mut account.credential, &tokens);
                }
                Ok(tokens.access_token)
            }
            Err(AgyOAuthError::InvalidGrant) => {
                if let Some(account) = account {
                    account.requires_reauth = true;
                }
                Err(AccountQuotaError::NeedsReauth)
            }
            Err(error) => return Err(AccountQuotaError::Failed(error.to_string())),
        };
        if let Err(error) = self.persist(&store) {
            log::warn!("[AgyAccount] 保存账号文件失败: {error}");
        }
        result
    }

    // ---------- 内部 ----------

    async fn read_live(&self) -> Result<LiveLogin, AppError> {
        let login = Arc::clone(&self.login);
        let raw = run_blocking(move || login.read()).await?;
        let Some(raw) = raw else {
            return Ok(LiveLogin::None);
        };
        let parsed = keychain::decode(&raw).ok().and_then(|credential| {
            let claims = credential["id_token"].as_str().and_then(oauth::identity)?;
            refresh_token_of(&credential)?;
            Some((credential, claims))
        });
        Ok(match parsed {
            Some((credential, claims)) => LiveLogin::Known { credential, claims },
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
        };
        let content = serde_json::to_vec_pretty(&snapshot)
            .map_err(|source| AppError::JsonSerialize { source })?;
        crate::config::atomic_write_private(&self.data_dir.join(STORE_FILENAME), &content)
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
        return (None, false);
    }
    store.dismissed.retain(|id| *id != sub);
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

fn upsert(store: &mut StoreFile, credential: Value, claims: &oauth::IdentityClaims) -> AgyAccount {
    let sub = claims.sub.clone().unwrap_or_default();
    store.dismissed.retain(|id| *id != sub);
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
