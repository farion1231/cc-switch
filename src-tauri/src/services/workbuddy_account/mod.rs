//! WorkBuddy 桌面端的官方账号：在 CC Switch 里收录、切换。
//!
//! WorkBuddy（Electron）由桌面端自己完成登录（微信 / 企业微信 / 腾讯云账号，私有接口），
//! CC Switch 发起不了登录，只能把桌面端已经登好的那份凭据「拍快照」，以后再还原回去。
//!
//! 凭据是一组文件，必须一起存、一起还原：
//! - `<auth 目录>/workbuddy-desktop.info`：登录会话（`account` / `auth` / `accounts` …）。
//!   敏感字段是 WorkBuddy 的字段级加密信封 `{"$wbEncrypted":1,"envelope":"…"}`，CC Switch
//!   不解密，只读明文字段做展示（uid、类型、到期时间）；整份按字节原样保存。
//! - `<auth 目录>/workbuddy-desktop.info.<hash>.observation.json`：WorkBuddy 的登录观测记录。
//! - `~/.workbuddy/keyblob`：解密上面那些信封用的密钥包，可能会轮换，所以跟 `.info` 绑在一起。
//!
//! 登出标记是 `workbuddy-desktop.info.logged-out`（WorkBuddy `main/file-authentication-storage.js`
//! 的 `hasLogoutMarker` → `dev-env-override.js` 的 `getLogoutMarkerPath`：`${path}.logged-out`）。
//! 标记在时 WorkBuddy 无视 `.info`、当作没登录；还原账号时要删掉它。
//!
//! 几条规矩：
//! - 快照只在本机有效：加密信封依赖 keyblob 和系统级保护，换台电脑大概率解不开。
//!   账号表存在 CC Switch 配置目录（`workbuddy_accounts.json`，0600），不参与云同步。
//! - 切换前 WorkBuddy 必须已退出：运行中的 WorkBuddy 会 fs.watch 这些文件并回写覆盖。
//! - 切换前先把当前那组文件收回到当前账号的副本（WorkBuddy 可能已经刷新过 token）。
//!   认不出账号的整组原样暂存到 `workbuddy-login-stash.json`，切走也不会丢。
//! - 从 CC Switch 删账号只删自己的副本，不替 WorkBuddy 登出。
//! - 只管 `workbuddy-desktop`：不要和 WorkBuddy AI（`workbuddy-desktop-ai.info`、
//!   `~/.workbuddy-ai`）或 CodeBuddy（`Tencent-Cloud.coding-copilot.info`）混淆。
//! - 额度只能经运行中的 WorkBuddy 的本地 IPC 只读查当前登着的号，结果缓存进账号表（见 [`quota`]）。

mod quota;

#[cfg(test)]
use quota::QuotaUnavailable;
pub use quota::{account_quota, WorkBuddyQuota, WorkBuddyQuotaSnapshot};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

const STORE_FILENAME: &str = "workbuddy_accounts.json";
const STASH_FILENAME: &str = "workbuddy-login-stash.json";
const INFO_FILENAME: &str = "workbuddy-desktop.info";
const LOGOUT_MARKER_FILENAME: &str = "workbuddy-desktop.info.logged-out";
/// WorkBuddy 写凭据时拿的锁（proper-lockfile 建的目录，`log-acl-guard.js` 的
/// `resolveCredentialFileLockPath`：`.${basename}.workbuddy.lock`）。
const LOCK_DIRNAME: &str = ".workbuddy-desktop.info.workbuddy.lock";
const OBSERVATION_PREFIX: &str = "workbuddy-desktop.info.";
const OBSERVATION_SUFFIX: &str = ".observation.json";
/// 锁目录这么久内被碰过就当 WorkBuddy 还在写（proper-lockfile 默认 10 秒内续一次）。
const LOCK_FRESH: Duration = Duration::from_secs(30);

/// WorkBuddy 凭据所在的位置。测试用临时目录覆盖，绝不碰真实文件。
#[derive(Debug, Clone)]
pub struct WorkBuddyPaths {
    /// `CodeBuddyExtension/Data/Public/auth`
    pub auth_dir: PathBuf,
    /// `~/.workbuddy/keyblob`
    pub keyblob: PathBuf,
    /// `~/.workbuddy/storage/skeleton/account-snapshot.json`：WorkBuddy 自己维护的当前登录画像
    /// （明文 uid / 昵称 / 版型）。只读，用来补上 `.info` 里加密读不到的昵称和套餐。
    pub profile_snapshot: PathBuf,
    /// `~/.workbuddy/wbipc/endpoint.json`：WorkBuddy 给本机客户端的 IPC 发现文件（端点 + 握手用的
    /// ticket，0600）。只在查额度时读，ticket 不出 [`quota`] 模块。
    pub wbipc_discovery: PathBuf,
}

impl WorkBuddyPaths {
    /// 本机的真实位置。macOS 已确认；Windows / Linux 按 Electron 惯例推断，未实测。
    pub fn system() -> Self {
        let data_dir = crate::workbuddy_config::data_dir();
        Self {
            auth_dir: system_shared_data_dir().join("auth"),
            keyblob: data_dir.join("keyblob"),
            profile_snapshot: data_dir.join("storage/skeleton/account-snapshot.json"),
            wbipc_discovery: data_dir.join("wbipc/endpoint.json"),
        }
    }

    fn info(&self) -> PathBuf {
        self.auth_dir.join(INFO_FILENAME)
    }

    fn logout_marker(&self) -> PathBuf {
        self.auth_dir.join(LOGOUT_MARKER_FILENAME)
    }

    fn lock_dir(&self) -> PathBuf {
        self.auth_dir.join(LOCK_DIRNAME)
    }
}

/// WorkBuddy 的 `filePathService.sharedDataPath`。
fn system_shared_data_dir() -> PathBuf {
    let home = crate::config::get_home_dir();
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/CodeBuddyExtension/Data/Public")
    }
    // 推断：%LOCALAPPDATA%\CodeBuddyExtension\Data\Public（未实测）
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join("AppData").join("Local"))
            .join("CodeBuddyExtension")
            .join("Data")
            .join("Public")
    }
    // 推断：$XDG_DATA_HOME（默认 ~/.local/share）/CodeBuddyExtension/Data/Public（未实测）
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"))
            .join("CodeBuddyExtension/Data/Public")
    }
}

/// 一组凭据文件的原样字节（base64 存在账号表里）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Bundle {
    info: String,
    /// 文件名 → 内容
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    observations: BTreeMap<String, String>,
    /// 收录时 keyblob 不存在就是 `None`，还原时不动 keyblob
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keyblob: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountData {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
    /// 版型 / 套餐（来自 WorkBuddy 明文画像的 `editionType`，如 free / pro）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    /// 会话到期（秒）；明文才有
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
    /// 刷新凭据到期（秒）；过了就切不过去了
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_expires_at: Option<i64>,
    bundle: Bundle,
    /// 第一次收录的时间（秒）
    added_at: i64,
    /// 最近一次收回副本的时间（秒）
    captured_at: i64,
    /// 最近一次经 WorkBuddy 查到的套餐与积分（不敏感）；WorkBuddy 没运行时照样显示它
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quota: Option<WorkBuddyQuotaSnapshot>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, AccountData>,
    /// 用户手动删掉的账号 id：别再被自动收录悄悄加回来（切换时例外，见 `absorb_known`）。
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    dismissed: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkBuddyAccount {
    pub id: String,
    pub kind: Option<String>,
    pub nickname: Option<String>,
    /// 版型 / 套餐（WorkBuddy 明文画像里的 `editionType`，如 free / pro）
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
    pub refresh_expires_at: Option<i64>,
    pub added_at: i64,
    pub captured_at: i64,
    /// 明文的刷新到期时间已经过了：还原回去 WorkBuddy 也得重新登录
    pub expired: bool,
}

impl WorkBuddyAccount {
    /// 界面上显示的名字：明文昵称 > uid。
    pub fn label(&self) -> String {
        self.nickname.clone().unwrap_or_else(|| self.id.clone())
    }
}

impl From<&AccountData> for WorkBuddyAccount {
    fn from(data: &AccountData) -> Self {
        Self {
            id: data.id.clone(),
            kind: data.kind.clone(),
            nickname: data.nickname.clone(),
            plan: data.plan.clone(),
            expires_at: data.expires_at,
            refresh_expires_at: data.refresh_expires_at,
            added_at: data.added_at,
            captured_at: data.captured_at,
            expired: data
                .refresh_expires_at
                .is_some_and(|at| at <= chrono::Utc::now().timestamp()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkBuddyAccountStatus {
    pub accounts: Vec<WorkBuddyAccount>,
    /// WorkBuddy 现在登着的、已收录的号
    pub current_account_id: Option<String>,
}

/// WorkBuddy 这边当前的登录情况（界面提示用）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyLiveState {
    pub running: bool,
    /// 有 `.info` 且没有登出标记
    pub signed_in: bool,
    /// 当前登录认得出是谁（uid 是明文）
    pub recognized: bool,
    /// 当前登录已经收录过
    pub imported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportOutcome {
    pub account: WorkBuddyAccount,
    /// 新收录的（否则是更新了已有账号的副本）
    pub created: bool,
}

/// 从 `.info` 明文字段里认出来的身份。
#[derive(Debug, Clone, PartialEq)]
struct Identity {
    id: String,
    kind: Option<String>,
    nickname: Option<String>,
    /// 版型 / 套餐；`.info` 里没有明文，从 WorkBuddy 的明文画像补
    plan: Option<String>,
    expires_at: Option<i64>,
    refresh_expires_at: Option<i64>,
}

/// WorkBuddy 自己维护的明文登录画像（`account-snapshot.json` 的 `primary`）。只取展示用的
/// uid / 昵称 / 版型；绝不碰 token、手机号等任何加密或敏感字段。
#[derive(Debug, Clone)]
struct ProfileSnapshot {
    uid: String,
    nickname: Option<String>,
    plan: Option<String>,
}

/// 读 WorkBuddy 的明文画像。读不到、解析失败、没有 uid 都当没有，调用方安全降级到 uid 显示。
fn read_profile_snapshot(path: &Path) -> Option<ProfileSnapshot> {
    let bytes = std::fs::read(path).ok()?;
    let doc: Value = serde_json::from_slice(&bytes).ok()?;
    let primary = doc.get("primary")?.as_object()?;
    let uid = plain(primary.get("uid"))?;
    Some(ProfileSnapshot {
        uid,
        nickname: plain(primary.get("nickname")),
        plan: plain(primary.get("editionType")),
    })
}

enum LiveLogin {
    /// 没有 `.info`，或者有登出标记
    None,
    Known {
        bundle: Bundle,
        identity: Identity,
    },
    /// 读得到但认不出账号（uid 也加密了、格式变了）
    Unrecognized(Bundle),
}

pub type RunningCheck = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct WorkBuddyAccountManager {
    data_dir: PathBuf,
    paths: WorkBuddyPaths,
    running: RunningCheck,
    /// 账号表；同时充当所有改动（含写 WorkBuddy 的文件）的互斥锁
    store: Mutex<StoreFile>,
    /// 额度查询同一时间只开一条 WBIPC 会话
    quota_gate: tokio::sync::Mutex<()>,
}

impl WorkBuddyAccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_paths(
            data_dir,
            WorkBuddyPaths::system(),
            Arc::new(workbuddy_running),
        )
    }

    pub fn with_paths(data_dir: PathBuf, paths: WorkBuddyPaths, running: RunningCheck) -> Self {
        let path = data_dir.join(STORE_FILENAME);
        let store = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("[WorkBuddyAccount] 账号文件解析失败，按空表处理: {error}");
                StoreFile::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreFile::default(),
            Err(error) => {
                log::warn!("[WorkBuddyAccount] 账号文件读取失败，按空表处理: {error}");
                StoreFile::default()
            }
        };
        Self {
            data_dir,
            paths,
            running,
            store: Mutex::new(store),
            quota_gate: tokio::sync::Mutex::new(()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreFile> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 列出账号，并把 WorkBuddy 当前登着的那个号自动收录进来（没收录过就加，已收录就把
    /// 快照更新成最新——WorkBuddy 会刷新 token）。用户删过的号不会被自动加回来。只写
    /// CC Switch 自己的账号表，绝不碰 WorkBuddy 的凭据文件。
    pub fn status(&self) -> Result<WorkBuddyAccountStatus, AppError> {
        let mut store = self.lock();
        let current = match self.read_live() {
            Ok(LiveLogin::Known { bundle, identity }) => {
                let (id, changed) = absorb_known(&mut store, bundle, identity, false);
                if changed {
                    self.persist(&store)?;
                }
                id
            }
            Ok(_) => None,
            Err(error) => {
                log::warn!("[WorkBuddyAccount] 读取 WorkBuddy 当前登录失败: {error}");
                None
            }
        };
        Ok(WorkBuddyAccountStatus {
            accounts: sorted_accounts(&store),
            current_account_id: current,
        })
    }

    pub fn live_state(&self) -> WorkBuddyLiveState {
        let running = (self.running)();
        let store = self.lock();
        let (signed_in, recognized, imported) = match self.read_live() {
            Ok(LiveLogin::Known { identity, .. }) => {
                (true, true, store.accounts.contains_key(&identity.id))
            }
            Ok(LiveLogin::Unrecognized(_)) => (true, false, false),
            Ok(LiveLogin::None) | Err(_) => (false, false, false),
        };
        WorkBuddyLiveState {
            running,
            signed_in,
            recognized,
            imported,
        }
    }

    /// 收录 WorkBuddy 当前登录的账号（已收录的就更新副本）。
    pub fn import_current(&self) -> Result<ImportOutcome, AppError> {
        let mut store = self.lock();
        match self.read_live()? {
            LiveLogin::None => Err(AppError::localized(
                "workbuddy.account.not_signed_in",
                "WorkBuddy 当前没有登录。请先在 WorkBuddy 里登录，再回来收录。",
                "WorkBuddy is not signed in. Sign in in WorkBuddy first, then add the account here.",
            )),
            LiveLogin::Unrecognized(bundle) => {
                self.stash_unrecognized(&bundle)?;
                Err(AppError::localized(
                    "workbuddy.account.unrecognized",
                    "认不出 WorkBuddy 当前登录的账号（格式可能变了），暂时无法收录；这组凭据已原样暂存。",
                    "Couldn't recognize the account WorkBuddy is signed in with (the format may have changed). It was stashed as-is but can't be added yet.",
                ))
            }
            LiveLogin::Known { bundle, identity } => {
                let created = !store.accounts.contains_key(&identity.id);
                // 手动收录是明确意愿，解除之前的「删除」标记
                store.dismissed.remove(&identity.id);
                let account = upsert(&mut store, bundle, identity);
                self.persist(&store)?;
                Ok(ImportOutcome { account, created })
            }
        }
    }

    /// 把 WorkBuddy 的登录换成这个账号：确认未运行 → 收回当前那组 → 原子还原目标组 →
    /// 清登出标记 → 读回校验。已经是它时什么都不写。
    pub fn switch_to(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.lock();
        let target = store
            .accounts
            .get(account_id)
            .cloned()
            .ok_or_else(|| account_not_found(account_id))?;
        if WorkBuddyAccount::from(&target).expired {
            return Err(AppError::localized(
                "workbuddy.account.expired",
                "这个账号的登录已过期，请在 WorkBuddy 里重新登录后再收录。",
                "This account's sign-in has expired. Sign in again in WorkBuddy, then add it again.",
            ));
        }
        self.ensure_quiet()?;

        let live = self.read_live()?;
        let previous = match &live {
            LiveLogin::Known { bundle, .. } | LiveLogin::Unrecognized(bundle) => {
                Some(bundle.clone())
            }
            LiveLogin::None => None,
        };
        let current = match live {
            LiveLogin::Known { bundle, identity } => {
                let id = identity.id.clone();
                // 当前登着的号即便之前被删过也要收回来，免得切走后这份登录彻底丢了
                let was_dismissed = store.dismissed.remove(&id);
                if was_dismissed
                    || store.accounts.get(&id).map(|data| &data.bundle) != Some(&bundle)
                {
                    upsert(&mut store, bundle, identity);
                    self.persist(&store)?;
                }
                Some(id)
            }
            LiveLogin::Unrecognized(bundle) => {
                self.stash_unrecognized(&bundle)?;
                None
            }
            LiveLogin::None => None,
        };
        if current.as_deref() == Some(account_id) {
            return Ok(());
        }
        // 收回后用最新的副本（目标就是刚收回的那个时上面已经返回了）
        let target_bundle = store
            .accounts
            .get(account_id)
            .map(|data| data.bundle.clone())
            .unwrap_or(target.bundle);

        if let Err(error) = self.restore(&target_bundle) {
            // 尽量把原来那组放回去，免得 WorkBuddy 落在半新半旧的状态
            if let Some(previous) = &previous {
                if let Err(rollback) = self.restore(previous) {
                    log::error!("[WorkBuddyAccount] 切换失败后回滚也失败: {rollback}");
                }
            }
            return Err(error);
        }
        if !self.verify(&target_bundle)? {
            return Err(AppError::localized(
                "workbuddy.account.write_verify_failed",
                "写入后核对不一致，WorkBuddy 的登录可能没有切换成功。",
                "The written sign-in did not verify; WorkBuddy may not have switched accounts.",
            ));
        }
        log::info!("[WorkBuddyAccount] 已切换 WorkBuddy 登录");
        Ok(())
    }

    /// 让 WorkBuddy 下次打开时停在登录页，好登录另一个账号：先收录当前账号，再写登出标记。
    /// 只在本机写标记（和 WorkBuddy 自己登出时一样），不调用服务端登出，`.info` 原样留着。
    pub fn prepare_new_login(&self) -> Result<Option<WorkBuddyAccount>, AppError> {
        let mut store = self.lock();
        self.ensure_quiet()?;
        let captured = match self.read_live()? {
            LiveLogin::Known { bundle, identity } => {
                store.dismissed.remove(&identity.id);
                let account = upsert(&mut store, bundle, identity);
                self.persist(&store)?;
                Some(account)
            }
            LiveLogin::Unrecognized(bundle) => {
                self.stash_unrecognized(&bundle)?;
                None
            }
            LiveLogin::None => return Ok(None),
        };
        let marker = chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .into_bytes();
        crate::config::atomic_write_private(&self.paths.logout_marker(), &marker)?;
        Ok(captured)
    }

    /// 只删 CC Switch 里的副本，不碰 WorkBuddy 的文件。记进 `dismissed`，这样即便它正是
    /// WorkBuddy 当前登着的号，也不会被自动收录马上加回来（想找回点「收录当前登录」即可）。
    pub fn remove(&self, account_id: &str) -> Result<(), AppError> {
        let mut store = self.lock();
        if store.accounts.remove(account_id).is_none() {
            return Err(account_not_found(account_id));
        }
        store.dismissed.insert(account_id.to_string());
        self.persist(&store)
    }

    // ---------- 额度 ----------

    /// 查额度前要知道的：上次缓存、它是不是 WorkBuddy 当前登着的号、WorkBuddy 在不在运行。
    /// 顺手把当前登录收录 / 更新进账号表（同 `status`）。
    fn quota_context(&self, account_id: &str) -> Result<quota::QuotaContext, AppError> {
        let current = self.status()?.current_account_id;
        let cached = {
            let store = self.lock();
            store
                .accounts
                .get(account_id)
                .ok_or_else(|| account_not_found(account_id))?
                .quota
                .clone()
        };
        let current = current.as_deref() == Some(account_id);
        Ok(quota::QuotaContext {
            cached,
            current,
            running: current && (self.running)(),
            discovery: self.paths.wbipc_discovery.clone(),
        })
    }

    /// 把查到的额度记进账号表。查询期间 WorkBuddy 换了号（当前登录已经不是它）就不记，返回 false。
    fn save_quota(
        &self,
        account_id: &str,
        snapshot: WorkBuddyQuotaSnapshot,
    ) -> Result<bool, AppError> {
        let mut store = self.lock();
        let still_current = matches!(
            self.read_live(),
            Ok(LiveLogin::Known { ref identity, .. }) if identity.id == account_id
        );
        let Some(account) = store.accounts.get_mut(account_id) else {
            return Ok(false);
        };
        if !still_current {
            return Ok(false);
        }
        account.quota = Some(snapshot);
        self.persist(&store)?;
        Ok(true)
    }

    // ---------- 内部 ----------

    /// WorkBuddy 没在运行、也没在写凭据。
    fn ensure_quiet(&self) -> Result<(), AppError> {
        if (self.running)() {
            return Err(AppError::localized(
                "workbuddy.account.running",
                "WorkBuddy 正在运行。请先完全退出 WorkBuddy（含菜单栏/托盘），再切换账号。",
                "WorkBuddy is running. Quit WorkBuddy completely (including the menu bar / tray) before switching accounts.",
            ));
        }
        let fresh_lock = std::fs::metadata(self.paths.lock_dir())
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age < LOCK_FRESH);
        if fresh_lock {
            return Err(AppError::localized(
                "workbuddy.account.locked",
                "WorkBuddy 正在写入登录凭据，请稍后再试。",
                "WorkBuddy is writing its sign-in right now. Try again in a moment.",
            ));
        }
        Ok(())
    }

    fn read_live(&self) -> Result<LiveLogin, AppError> {
        if self.paths.logout_marker().exists() {
            return Ok(LiveLogin::None);
        }
        let info_path = self.paths.info();
        let info = match std::fs::read(&info_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LiveLogin::None)
            }
            Err(error) => return Err(AppError::io(&info_path, error)),
        };
        let identity = serde_json::from_slice::<Value>(&info)
            .ok()
            .and_then(|doc| identity(&doc))
            .map(|mut identity| {
                // 用 WorkBuddy 明文画像补上加密读不到的昵称和版型（仅当 uid 对得上当前登录）。
                // 昵称不算敏感，可以缓存进账号表的显示名；画像读不到就保持 uid 显示。
                if let Some(profile) = read_profile_snapshot(&self.paths.profile_snapshot) {
                    if profile.uid == identity.id {
                        if identity.nickname.is_none() {
                            identity.nickname = profile.nickname;
                        }
                        identity.plan = profile.plan;
                    }
                }
                identity
            });

        let mut observations = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.paths.auth_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !is_observation_name(&name) {
                    continue;
                }
                match std::fs::read(entry.path()) {
                    Ok(bytes) => {
                        observations.insert(name, STANDARD.encode(bytes));
                    }
                    Err(error) => {
                        log::warn!("[WorkBuddyAccount] 读取观测文件失败，跳过: {error}");
                    }
                }
            }
        }
        let keyblob = match std::fs::read(&self.paths.keyblob) {
            Ok(bytes) => Some(STANDARD.encode(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::io(&self.paths.keyblob, error)),
        };
        let bundle = Bundle {
            info: STANDARD.encode(info),
            observations,
            keyblob,
        };
        Ok(match identity {
            Some(identity) => LiveLogin::Known { bundle, identity },
            None => LiveLogin::Unrecognized(bundle),
        })
    }

    /// 把一组文件写回去：keyblob → 观测文件 → `.info` → 删登出标记。每个文件都是临时文件 + rename。
    fn restore(&self, bundle: &Bundle) -> Result<(), AppError> {
        let info = decode(&bundle.info)?;
        let keyblob = bundle.keyblob.as_deref().map(decode).transpose()?;
        let mut observations = Vec::new();
        for (name, content) in &bundle.observations {
            if !is_observation_name(name) {
                return Err(AppError::Message(format!("观测文件名不合法: {name}")));
            }
            observations.push((name.as_str(), decode(content)?));
        }

        if let Some(keyblob) = keyblob {
            crate::config::atomic_write_private(&self.paths.keyblob, &keyblob)?;
        }
        // 上一个账号的观测文件已经收进它的副本，这里换成目标账号那一份
        if let Ok(entries) = std::fs::read_dir(&self.paths.auth_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_observation_name(&name) && !bundle.observations.contains_key(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        for (name, content) in observations {
            crate::config::atomic_write_private(&self.paths.auth_dir.join(name), &content)?;
        }
        crate::config::atomic_write_private(&self.paths.info(), &info)?;
        let marker = self.paths.logout_marker();
        match std::fs::remove_file(&marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(&marker, error)),
        }
        Ok(())
    }

    fn verify(&self, bundle: &Bundle) -> Result<bool, AppError> {
        if self.paths.logout_marker().exists() {
            return Ok(false);
        }
        let info = std::fs::read(self.paths.info()).ok();
        if info.as_deref() != Some(decode(&bundle.info)?.as_slice()) {
            return Ok(false);
        }
        if let Some(keyblob) = &bundle.keyblob {
            let written = std::fs::read(&self.paths.keyblob).ok();
            if written.as_deref() != Some(decode(keyblob)?.as_slice()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn stash_unrecognized(&self, bundle: &Bundle) -> Result<(), AppError> {
        let content = serde_json::to_vec_pretty(&serde_json::json!({
            "bundle": bundle,
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

/// 把识别出的当前登录并进账号表。返回它的 id 和账号表是否真的变了（变了才需要落盘）。
/// `import_dismissed` 为 false 时，用户删过的号不会被自动加回来；切换/收录等明确意愿为 true。
fn absorb_known(
    store: &mut StoreFile,
    bundle: Bundle,
    identity: Identity,
    import_dismissed: bool,
) -> (Option<String>, bool) {
    let id = identity.id.clone();
    if let Some(existing) = store.accounts.get(&id) {
        // 已收录：只有快照或明文字段真的变了才算「变化」，避免每次轮询都写盘
        let unchanged = existing.bundle == bundle
            && (identity.kind.is_none() || existing.kind == identity.kind)
            && (identity.nickname.is_none() || existing.nickname == identity.nickname)
            && (identity.plan.is_none() || existing.plan == identity.plan)
            && existing.expires_at == identity.expires_at
            && existing.refresh_expires_at == identity.refresh_expires_at;
        if unchanged {
            return (Some(id), false);
        }
        upsert(store, bundle, identity);
        return (Some(id), true);
    }
    if store.dismissed.contains(&id) && !import_dismissed {
        return (None, false);
    }
    store.dismissed.remove(&id);
    upsert(store, bundle, identity);
    (Some(id), true)
}

fn upsert(store: &mut StoreFile, bundle: Bundle, identity: Identity) -> WorkBuddyAccount {
    let now = chrono::Utc::now().timestamp();
    let entry = store
        .accounts
        .entry(identity.id.clone())
        .or_insert_with(|| AccountData {
            id: identity.id.clone(),
            kind: None,
            nickname: None,
            plan: None,
            expires_at: None,
            refresh_expires_at: None,
            bundle: bundle.clone(),
            added_at: now,
            captured_at: now,
            quota: None,
        });
    entry.bundle = bundle;
    entry.captured_at = now;
    // 新会话里加密了的字段读不出来时保留旧值
    if identity.kind.is_some() {
        entry.kind = identity.kind;
    }
    if identity.nickname.is_some() {
        entry.nickname = identity.nickname;
    }
    // 画像暂时读不到时保留旧版型
    if identity.plan.is_some() {
        entry.plan = identity.plan;
    }
    entry.expires_at = identity.expires_at;
    entry.refresh_expires_at = identity.refresh_expires_at;
    WorkBuddyAccount::from(&*entry)
}

fn sorted_accounts(store: &StoreFile) -> Vec<WorkBuddyAccount> {
    let mut accounts: Vec<WorkBuddyAccount> = store
        .accounts
        .values()
        .map(WorkBuddyAccount::from)
        .collect();
    accounts.sort_by_key(|account| account.added_at);
    accounts
}

/// 明文字段：字符串（非空）或数字。加密信封（对象）当作读不到。
fn plain(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// 明文时间戳统一成秒（毫秒的也认）。
fn plain_timestamp(value: Option<&Value>) -> Option<i64> {
    let raw = match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64))?,
        Value::String(text) => text.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    if raw <= 0 {
        return None;
    }
    Some(if raw > 100_000_000_000 {
        raw / 1000
    } else {
        raw
    })
}

fn identity(doc: &Value) -> Option<Identity> {
    let account = doc.get("account")?.as_object()?;
    let id = plain(account.get("uid"))
        .or_else(|| plain(account.get("uin")).map(|uin| format!("uin:{uin}")))?;
    let auth = doc.get("auth");
    Some(Identity {
        id,
        kind: plain(account.get("type")),
        nickname: plain(account.get("nickname")),
        // `.info` 里没有明文版型；交给 read_live 用明文画像补
        plan: None,
        expires_at: plain_timestamp(auth.and_then(|auth| auth.get("expiresAt"))),
        refresh_expires_at: plain_timestamp(auth.and_then(|auth| auth.get("refreshExpiresAt"))),
    })
}

fn is_observation_name(name: &str) -> bool {
    name.len() > OBSERVATION_PREFIX.len() + OBSERVATION_SUFFIX.len()
        && name.starts_with(OBSERVATION_PREFIX)
        && name.ends_with(OBSERVATION_SUFFIX)
        && !name.contains(['/', '\\'])
        && !name.contains("..")
}

fn decode(content: &str) -> Result<Vec<u8>, AppError> {
    STANDARD
        .decode(content)
        .map_err(|error| AppError::Message(format!("WorkBuddy 账号副本已损坏: {error}")))
}

fn account_not_found(account_id: &str) -> AppError {
    AppError::Message(format!("找不到这个 WorkBuddy 账号: {account_id}"))
}

/// macOS 上 WorkBuddy 的 bundle id。
#[cfg(target_os = "macos")]
const MAC_BUNDLE_ID: &str = "com.tencent.workbuddy.mac";

/// WorkBuddy 有没有在运行。macOS 查 bundle id 和 `WorkBuddy.app` 里的进程（主进程叫
/// `Electron`，只看进程名认不出来）；其他平台按进程名尽力而为。
pub fn workbuddy_running() -> bool {
    #[cfg(target_os = "macos")]
    {
        let by_bundle = std::process::Command::new("lsappinfo")
            .args(["find", &format!("bundleid={MAC_BUNDLE_ID}")])
            .stdin(std::process::Stdio::null())
            .output()
            .is_ok_and(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty());
        by_bundle || ps_lines().iter().any(|line| is_workbuddy_process(line))
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
            return false;
        };
        String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.trim_start_matches('"')
                .split('"')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("WorkBuddy.exe"))
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        ps_lines().iter().any(|line| is_workbuddy_process(line))
    }
}

#[cfg(unix)]
fn ps_lines() -> Vec<String> {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "comm="])
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .collect()
}

/// `ps` 的一行是不是 WorkBuddy 的进程：路径里有一段正好叫 `WorkBuddy.app`（别把
/// `CC Switch Dev WorkBuddy.app` 这类名字算进去），或进程名就是 workbuddy。
#[cfg_attr(not(unix), allow(dead_code))]
fn is_workbuddy_process(line: &str) -> bool {
    let path = Path::new(line.trim());
    if path
        .components()
        .any(|part| part.as_os_str() == "WorkBuddy.app")
    {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("workbuddy"))
}

#[cfg(test)]
mod tests;
