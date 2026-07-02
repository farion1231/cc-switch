//! Kiro Authentication Module
//!
//! 实现 AWS Builder ID / IAM Identity Center OIDC 认证流程，以及 kiro-cli SQLite 凭证共享。

use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};

/// User-Agent 组件，对齐官方 kiro-cli 2.7.0 的真实流量。
///
/// kiro-cli 由 AWS Rust SDK 构造两个相关头：
/// - `user-agent`：携带 `md/appVersion-<KIRO_VERSION>`
/// - `x-amz-user-agent`：携带 metrics 段 `m/<...>`
///
/// 二者共享 `aws-sdk-rust/<SDK> ua/2.1 api/<service>/<API> os/macos
/// lang/rust/<RUST> ... app/AmazonQ-For-CLI` 骨架。`api/<service>` 段按端点不同：
/// - codewhispererstreaming → GenerateAssistantResponse, InvokeMCP
/// - codewhispererruntime → ListAvailableModels / GetProfile / ListAvailableProfiles
/// - ssooidc → RegisterClient / token（登录 + 刷新）
///
/// 取值来自真实 kiro-cli 2.7.0 抓包。
const KIRO_SDK_VERSION: &str = "1.3.15";
const KIRO_OIDC_SDK_VERSION: &str = "1.3.10";
const KIRO_API_VERSION: &str = "0.1.16551";
const KIRO_OIDC_API_VERSION: &str = "1.92.0";
const KIRO_RUST_VERSION: &str = "1.92.0";
const KIRO_OS: &str = "macos";
const KIRO_VERSION: &str = "2.7.0";
const KIRO_APP: &str = "AmazonQ-For-CLI";

/// Kiro desktop 认证服务（auth.desktop.kiro.dev）不是 AWS SDK 端点 ——
/// 官方 kiro-cli 向它发送纯 `Kiro-CLI` User-Agent。
const KIRO_DESKTOP_USER_AGENT: &str = "Kiro-CLI";

const KIRO_CLIENT_NAME: &str = "Kiro CLI";

/// Kiro/AWS 服务端点族，用于选择 SDK 版本与 `api/<service>` 段。
#[derive(Clone, Copy)]
enum KiroSdkApi {
    #[allow(dead_code)]
    CodewhispererStreaming,
    CodewhispererRuntime,
    Ssooidc,
}

impl KiroSdkApi {
    fn name(self) -> &'static str {
        match self {
            KiroSdkApi::CodewhispererStreaming => "codewhispererstreaming",
            KiroSdkApi::CodewhispererRuntime => "codewhispererruntime",
            KiroSdkApi::Ssooidc => "ssooidc",
        }
    }
}

/// 为指定的 Kiro/AWS 服务端点构造 `(user-agent, x-amz-user-agent)` 头对，
/// 对齐官方 kiro-cli 流量。
///
/// `metrics` 是仅进入 `x-amz-user-agent` 的 `m/...` 段（如 streaming 用 "F"，
/// ListAvailableModels/GetProfile 用 "F,C"，OIDC 用 "E"）。
///
/// 注意：OIDC 端点特殊 —— 其 `user-agent` 为裸的
/// `aws-sdk-rust/<sdk> os/macos lang/rust/<rust>` 形式（无 `ua/`、`api/`、
/// `md/`、`app/` 段），仅 `x-amz-user-agent` 携带完整形式；codewhisperer
/// 端点则两头均为完整形式。
fn kiro_user_agent(api: KiroSdkApi, metrics: &str) -> (String, String) {
    let sdk = match api {
        KiroSdkApi::Ssooidc => KIRO_OIDC_SDK_VERSION,
        _ => KIRO_SDK_VERSION,
    };
    let api_ver = match api {
        KiroSdkApi::Ssooidc => KIRO_OIDC_API_VERSION,
        _ => KIRO_API_VERSION,
    };
    let base = format!(
        "aws-sdk-rust/{sdk} ua/2.1 api/{}/{api_ver} os/{KIRO_OS} lang/rust/{KIRO_RUST_VERSION}",
        api.name()
    );
    let user_agent = match api {
        KiroSdkApi::Ssooidc => {
            format!("aws-sdk-rust/{sdk} os/{KIRO_OS} lang/rust/{KIRO_RUST_VERSION}")
        }
        _ => format!("{base} md/appVersion-{KIRO_VERSION} app/{KIRO_APP}"),
    };
    let x_amz_user_agent = format!("{base} m/{metrics} app/{KIRO_APP}");
    (user_agent, x_amz_user_agent)
}

/// Kiro OIDC 范围
const SSO_SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];

const DEFAULT_START_URL: &str = "https://view.awsapps.com/start";
const DEFAULT_REGION: &str = "us-east-1";

/// AWS Builder ID 共享 profileArn 兑底。
///
/// Builder ID 账号的 `ListAvailableProfiles` 有时返回空（或不返回可用 profile），
/// 导致 runtime 请求缺少 profileArn 而失败。此时回退到 Builder ID 用户共享的
/// 固定 profileArn（抓包确认）。仅适用于 Builder ID（DEFAULT_START_URL），
/// 企业 IdC 用户有自己组织的 profile，不适用。
const BUILDER_ID_PROFILE_ARN: &str =
    "arn:aws:codewhisperer:us-east-1:638616132270:profile/AAAACCCCXXXX";

/// 当账号是 AWS Builder ID（DEFAULT_START_URL 的 IdC）且未能动态获取到 profileArn 时，
/// 返回共享兑底 profileArn；其他情况返回 None。
fn builder_id_fallback_profile_arn(auth_method: &str, start_url: Option<&str>) -> Option<String> {
    if auth_method == "idc" && start_url == Some(DEFAULT_START_URL) {
        Some(BUILDER_ID_PROFILE_ARN.to_string())
    } else {
        None
    }
}

/// 刷新缓冲（5 分钟）：在实际过期前提前刷新，避免边界请求 403
const EXPIRES_BUFFER_MS: i64 = 5 * 60 * 1000;

/// 自动检测 IAM Identity Center OIDC region 时探测的常见 region，按可能性排序。
const IDC_PROBE_REGIONS: &[&str] = &[
    "us-east-1",
    "ap-northeast-1",
    "ap-southeast-1",
    "eu-central-1",
    "eu-north-1",
    "eu-west-1",
    "eu-west-2",
    "eu-west-3",
    "us-east-2",
    "us-west-1",
    "us-west-2",
];

/// 将 SSO/OIDC region 映射为 Kiro Q API 实际部署的 region。
///
/// Kiro Q API（management / runtime）仅部署在少数 region（us-east-1 与 eu-central-1）。
/// 由其他 region 的 SSO 实例签发的 token 必须发送到对应的 API region，
/// 否则会出现 DNS / 连接错误（如 management.ap-northeast-1.kiro.dev 不存在）。
/// 这与 kiro-cli 内部通过 AWS SDK partition resolver 做的端点解析一致。
pub fn resolve_api_region(sso_region: Option<&str>) -> String {
    let region = match sso_region {
        Some(r) if !r.is_empty() => r,
        _ => return "us-east-1".to_string(),
    };
    let mapped = match region {
        "us-west-1" | "us-west-2" | "us-east-2" | "ap-southeast-1" | "ap-southeast-2"
        | "ap-northeast-1" | "ap-northeast-2" | "ap-northeast-3" | "ap-south-1" => "us-east-1",
        "eu-west-1" | "eu-west-2" | "eu-west-3" | "eu-north-1" | "eu-south-1" | "eu-south-2"
        | "eu-central-2" => "eu-central-1",
        other => other,
    };
    mapped.to_string()
}

/// Kiro API key 是以 `ksk_` 前缀的长期有效 bearer token。
pub fn is_api_key(token: &str) -> bool {
    token.starts_with("ksk_")
}

/// 宽松解析过期时间为 epoch 毫秒。
///
/// kiro-cli / Kiro IDE 在不同版本中可能以 RFC3339 字符串、RFC2822 字符串、
/// 或数字（秒 / 毫秒）形式存储 `expires_at`。严格只认 RFC3339 会在解析失败时
/// 把已过期 token 当作有效（now + 1h），导致上游 403。这里逐一尝试常见格式，
/// 全部失败才回退。
fn parse_expires_to_ms(value: &serde_json::Value) -> Option<i64> {
    // 数字：可能是秒或毫秒
    if let Some(n) = value.as_i64() {
        // 大于 ~1e12 视为毫秒，否则视为秒
        if n > 1_000_000_000_000 {
            return Some(n);
        }
        return Some(n * 1000);
    }
    if let Some(f) = value.as_f64() {
        let n = f as i64;
        if n > 1_000_000_000_000 {
            return Some(n);
        }
        return Some(n * 1000);
    }
    // 字符串：尝试 RFC3339 → RFC2822 → 纯数字
    if let Some(s) = value.as_str() {
        let s = s.trim();
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp_millis());
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
            return Some(dt.timestamp_millis());
        }
        if let Ok(n) = s.parse::<i64>() {
            if n > 1_000_000_000_000 {
                return Some(n);
            }
            return Some(n * 1000);
        }
    }
    None
}

/// Kiro 账号的持久化数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroAccountData {
    pub account_id: String,
    pub login: String,
    pub auth_method: String, // "idc" or "desktop"
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
    pub region: String,
    pub profile_arn: Option<String>,
    pub start_url: Option<String>,
    pub expires_at_ms: i64,
    pub authenticated_at: i64,
    pub source: String, // "local" or "kiro-cli"
}

/// 待处理的登录状态
#[derive(Debug, Clone)]
struct PendingKiroLogin {
    client_id: String,
    client_secret: String,
    region: String,
    start_url: String,
    expires_at_ms: i64,
}

/// OIDC Client Registration 响应
#[derive(Debug, Deserialize)]
struct ClientRegisterResponse {
    #[serde(rename = "clientId")]
    client_id: String,
    #[serde(rename = "clientSecret")]
    client_secret: String,
}

/// OIDC Device Authorization 响应
#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    device_code: String,
    #[serde(rename = "userCode")]
    user_code: String,
    #[serde(rename = "verificationUri")]
    verification_uri: String,
    #[serde(rename = "verificationUriComplete")]
    verification_uri_complete: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
    #[serde(rename = "interval")]
    interval: Option<u64>,
}

/// OIDC Token 响应
#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
}

/// Desktop 刷新响应
#[derive(Debug, Deserialize)]
struct DesktopRefreshResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
    #[serde(rename = "profileArn")]
    profile_arn: Option<String>,
}

/// Kiro 认证管理器
pub struct KiroAuthManager {
    /// 本地保存的账号（来自 kiro_auth.json）
    local_accounts: Arc<RwLock<HashMap<String, KiroAccountData>>>,
    /// 默认账号 ID
    default_account_id: Arc<RwLock<Option<String>>>,
    /// 进行中的登录会话
    pending_logins: Arc<RwLock<HashMap<String, PendingKiroLogin>>>,
    /// 刷新锁
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// HTTP 客户端
    http_client: Client,
    /// 存储路径
    storage_path: PathBuf,
}

impl KiroAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("kiro_auth.json");

        // 在构造共享锁之前同步加载磁盘数据，避免在 tokio runtime 上调用 blocking_write。
        let (loaded_accounts, loaded_default) = match Self::read_from_disk(&storage_path) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[KiroAuth] 加载存储失败: {e}");
                (HashMap::new(), None)
            }
        };

        Self {
            local_accounts: Arc::new(RwLock::new(loaded_accounts)),
            default_account_id: Arc::new(RwLock::new(loaded_default)),
            pending_logins: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path,
        }
    }

    /// 同步读取磁盘凭证（不涉及任何 tokio 锁），供构造函数使用。
    fn read_from_disk(
        storage_path: &Path,
    ) -> Result<(HashMap<String, KiroAccountData>, Option<String>), String> {
        if !storage_path.exists() {
            return Ok((HashMap::new(), None));
        }
        let content = fs::read_to_string(storage_path).map_err(|e| format!("读取文件失败: {e}"))?;
        #[derive(Deserialize)]
        struct SavedData {
            accounts: HashMap<String, KiroAccountData>,
            default_account_id: Option<String>,
        }
        let data: SavedData =
            serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {e}"))?;

        // 全部账号都从磁盘恢复（包括用户主动导入的 kiro-cli / kiro-ide 快照）。
        // 导入的动态账号仍带有 source 标记，token 刷新时会通过 read_dynamic_account
        // 读取对应来源的最新凭证以保持新鲜。
        let local = data.accounts;
        Ok((local, data.default_account_id))
    }

    /// 异步保存：先获取读锁并克隆数据，再执行纯文件写入（不在持锁期间阻塞）。
    async fn save_to_disk(&self) -> Result<(), String> {
        let accounts = self.local_accounts.read().await.clone();
        let default_account_id = self.default_account_id.read().await.clone();
        self.write_accounts_to_disk(accounts, default_account_id)
    }

    /// 纯文件写入：只负责把给定数据原子写入磁盘，不做任何加锁，可在已持锁的上下文中调用。
    fn write_accounts_to_disk(
        &self,
        accounts: HashMap<String, KiroAccountData>,
        default_account_id: Option<String>,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        struct SavedData {
            accounts: HashMap<String, KiroAccountData>,
            default_account_id: Option<String>,
        }

        let data = SavedData {
            accounts,
            default_account_id,
        };

        let content =
            serde_json::to_string_pretty(&data).map_err(|e| format!("序列化失败: {e}"))?;

        // 凭证文件包含 access_token/refresh_token/client_secret，需限制为仅属主可读写，
        // 并通过临时文件 + rename 实现原子写入，避免并发读取到半写状态。
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| "无法确定存储目录".to_string())?;
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| format!("创建存储目录失败: {e}"))?;
        }
        let file_name = self
            .storage_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("kiro_auth.json");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = parent.join(format!("{file_name}.tmp.{ts}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp_path)
                .map_err(|e| format!("创建文件失败: {e}"))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("写入数据失败: {e}"))?;
            file.flush().map_err(|e| format!("刷新数据失败: {e}"))?;

            fs::rename(&tmp_path, &self.storage_path).map_err(|e| format!("保存文件失败: {e}"))?;
            fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("设置文件权限失败: {e}"))?;
        }

        #[cfg(not(unix))]
        {
            let mut file = fs::File::create(&tmp_path).map_err(|e| format!("创建文件失败: {e}"))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("写入数据失败: {e}"))?;
            file.flush().map_err(|e| format!("刷新数据失败: {e}"))?;
            fs::rename(&tmp_path, &self.storage_path).map_err(|e| format!("保存文件失败: {e}"))?;
        }

        Ok(())
    }

    /// 获取 kiro-cli 的 SQLite DB 路径
    fn get_kiro_cli_db_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let path = if cfg!(target_os = "windows") {
            dirs::data_dir()?.join("kiro-cli").join("data.sqlite3")
        } else if cfg!(target_os = "macos") {
            home.join("Library")
                .join("Application Support")
                .join("kiro-cli")
                .join("data.sqlite3")
        } else {
            home.join(".local")
                .join("share")
                .join("kiro-cli")
                .join("data.sqlite3")
        };
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// 从 kiro-cli DB 中加载凭证
    fn get_kiro_cli_account(&self, method: &str) -> Option<KiroAccountData> {
        let db_path = self.get_kiro_cli_db_path()?;
        let token_key = if method == "desktop" {
            "kirocli:social:token"
        } else {
            "kirocli:odic:token"
        };

        let val = self.read_kiro_cli_token(&db_path, token_key)?;
        let access_token = val.get("access_token")?.as_str()?.to_string();
        let refresh_token = val.get("refresh_token")?.as_str()?.to_string();
        let region = val
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-east-1")
            .to_string();
        let profile_arn = val
            .get("profile_arn")
            .or_else(|| val.get("profileArn"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let start_url = val
            .get("start_url")
            .or_else(|| val.get("startUrl"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let expires_at_ms = val
            .get("expires_at")
            .and_then(parse_expires_to_ms)
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() + 3_600_000);

        let mut client_id = String::new();
        let mut client_secret = String::new();

        if method == "idc" {
            if let Some(dev_reg) =
                self.read_kiro_cli_token(&db_path, "kirocli:odic:device-registration")
            {
                if let Some(cid) = dev_reg
                    .get("client_id")
                    .or_else(|| dev_reg.get("clientId"))
                    .and_then(|v| v.as_str())
                {
                    client_id = cid.to_string();
                }
                if let Some(csec) = dev_reg
                    .get("client_secret")
                    .or_else(|| dev_reg.get("clientSecret"))
                    .and_then(|v| v.as_str())
                {
                    client_secret = csec.to_string();
                }
            }
        }

        let account_id = format!("kiro_cli_{method}");
        let login = if method == "desktop" {
            "kiro-cli (Social)".to_string()
        } else {
            "kiro-cli (Builder ID / IdC)".to_string()
        };

        Some(KiroAccountData {
            account_id,
            login,
            auth_method: method.to_string(),
            access_token,
            refresh_token,
            client_id,
            client_secret,
            region,
            profile_arn,
            start_url,
            expires_at_ms,
            authenticated_at: chrono::Utc::now().timestamp(),
            source: "kiro-cli".to_string(),
        })
    }

    fn read_kiro_cli_token(&self, db_path: &Path, key: &str) -> Option<serde_json::Value> {
        let conn = rusqlite::Connection::open(db_path).ok()?;
        let mut stmt = conn
            .prepare("SELECT value FROM auth_kv WHERE key = ?1")
            .ok()?;
        let val_str: String = stmt.query_row([key], |row| row.get(0)).ok()?;
        serde_json::from_str(&val_str).ok()
    }

    fn write_kiro_cli_token(
        &self,
        token_key: &str,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        let db_path = self
            .get_kiro_cli_db_path()
            .ok_or_else(|| "kiro-cli database not found".to_string())?;
        let conn =
            rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO auth_kv (key, value) VALUES (?1, ?2)",
            (token_key, serde_json::to_string(value).unwrap_or_default()),
        )
        .map_err(|e| format!("写入数据库失败: {e}"))?;
        Ok(())
    }

    /// 获取 Kiro IDE 凭证文件路径（`~/.aws/sso/cache/kiro-auth-token.json`）。
    /// 该路径在 Windows/macOS/Linux 上一致。
    fn get_kiro_ide_sso_cache_dir(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        Some(home.join(".aws").join("sso").join("cache"))
    }

    /// 从 Kiro IDE 的 SSO 缓存读取凭证。
    ///
    /// IDE 在每次登录后写入 `kiro-auth-token.json`（包含 IAM Identity Center 与 Builder ID），
    /// 并在 `{clientIdHash}.json` 中保存 OIDC clientId/clientSecret 供静默刷新。
    fn get_kiro_ide_account(&self) -> Option<KiroAccountData> {
        let cache_dir = self.get_kiro_ide_sso_cache_dir()?;
        let token_path = cache_dir.join("kiro-auth-token.json");
        if !token_path.exists() {
            return None;
        }
        let content = fs::read_to_string(&token_path).ok()?;
        let token_data: serde_json::Value = serde_json::from_str(&content).ok()?;

        let access_token = token_data.get("accessToken")?.as_str()?.to_string();
        let refresh_token = token_data.get("refreshToken")?.as_str()?.to_string();
        if access_token.is_empty() || refresh_token.is_empty() {
            return None;
        }

        let region = token_data
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_REGION)
            .to_string();

        let expires_at_ms = token_data
            .get("expiresAt")
            .and_then(parse_expires_to_ms)
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() + 3_600_000);

        // 读取伴随的 OIDC client 注册文件以支持静默刷新
        let mut client_id = String::new();
        let mut client_secret = String::new();
        if let Some(hash) = token_data.get("clientIdHash").and_then(|v| v.as_str()) {
            let reg_path = cache_dir.join(format!("{hash}.json"));
            if let Ok(reg_content) = fs::read_to_string(&reg_path) {
                if let Ok(reg) = serde_json::from_str::<serde_json::Value>(&reg_content) {
                    if let Some(cid) = reg.get("clientId").and_then(|v| v.as_str()) {
                        client_id = cid.to_string();
                    }
                    if let Some(csec) = reg.get("clientSecret").and_then(|v| v.as_str()) {
                        client_secret = csec.to_string();
                    }
                }
            }
        }

        let provider = token_data
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let start_url = if region == DEFAULT_REGION && provider != "enterprise" {
            Some(DEFAULT_START_URL.to_string())
        } else {
            None
        };

        Some(KiroAccountData {
            account_id: "kiro_ide".to_string(),
            login: "Kiro IDE".to_string(),
            auth_method: "idc".to_string(),
            access_token,
            refresh_token,
            client_id,
            client_secret,
            region,
            profile_arn: None,
            start_url,
            expires_at_ms,
            authenticated_at: chrono::Utc::now().timestamp(),
            source: "kiro-ide".to_string(),
        })
    }

    /// 列出所有已认证的账号（合并本地和 kiro-cli 的账号）
    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        // 仅返回已保存的账号（设备码登录 / 社交登录 / 主动导入的 kiro-cli / kiro-ide 快照）。
        // 不再每次自动读取 kiro-cli / kiro-ide 凭证，避免删除后又被重新注入。
        let map = self.local_accounts.read().await.clone();

        let mut list = Vec::new();
        for (_, val) in map {
            list.push(GitHubAccount {
                id: val.account_id,
                login: val.login,
                avatar_url: None,
                authenticated_at: val.authenticated_at,
                github_domain: "kiro.dev".to_string(),
            });
        }
        list
    }

    /// 主动导入 kiro-cli / kiro-ide 凭证（仅在用户点击按钮时调用）。
    ///
    /// 读取本地 kiro-cli SQLite 与 Kiro IDE 缓存，将检测到的账号快照写入本地存储，
    /// 之后这些账号像普通账号一样可被删除；删除后不会自动重新出现。
    /// 返回本次新导入（之前不存在）的账号列表。
    pub async fn import_dynamic_accounts(&self) -> Vec<GitHubAccount> {
        let mut imported: Vec<KiroAccountData> = Vec::new();

        if let Some(ide) = self.get_kiro_ide_account() {
            imported.push(ide);
        }
        if let Some(cli_idc) = self.get_kiro_cli_account("idc") {
            imported.push(cli_idc);
        }
        if let Some(cli_social) = self.get_kiro_cli_account("desktop") {
            imported.push(cli_social);
        }

        let mut newly: Vec<GitHubAccount> = Vec::new();
        {
            let mut local = self.local_accounts.write().await;
            for acc in imported {
                let is_new = !local.contains_key(&acc.account_id);
                if is_new {
                    newly.push(GitHubAccount {
                        id: acc.account_id.clone(),
                        login: acc.login.clone(),
                        avatar_url: None,
                        authenticated_at: acc.authenticated_at,
                        github_domain: "kiro.dev".to_string(),
                    });
                }
                // 覆盖写入快照（更新 login / region / profile_arn 等元信息）
                local.insert(acc.account_id.clone(), acc);
            }
        }
        let _ = self.save_to_disk().await;
        newly
    }

    /// 获取默认账号 ID
    pub async fn default_account_id(&self) -> Option<String> {
        self.default_account_id.read().await.clone()
    }

    /// 设置默认账号
    pub async fn set_default_account(&self, account_id: &str) -> Result<(), String> {
        {
            let mut default_guard = self.default_account_id.write().await;
            *default_guard = Some(account_id.to_string());
        }
        let _ = self.save_to_disk().await;
        Ok(())
    }

    /// 移除账号
    pub async fn remove_account(&self, account_id: &str) -> Result<(), String> {
        let removed = {
            let mut local = self.local_accounts.write().await;
            local.remove(account_id).is_some()
        };
        if removed {
            let _ = self.save_to_disk().await;
        }
        Ok(())
    }

    /// 登出全部（清除本地，不清理 kiro-cli，仅取消默认关联）
    pub async fn logout(&self) -> Result<(), String> {
        {
            let mut local = self.local_accounts.write().await;
            local.clear();
        }
        {
            let mut default_guard = self.default_account_id.write().await;
            *default_guard = None;
        }
        let _ = self.save_to_disk().await;
        Ok(())
    }

    /// 获取指定账号的有效 Token
    pub async fn get_valid_token_for_account(&self, account_id: &str) -> Result<String, String> {
        let now = chrono::Utc::now().timestamp_millis();

        // 仅当账号仍注册在 local_accounts（即用户已导入且未删除）时，才允许读取
        // kiro-cli / kiro-ide 源头凭证。否则删除后的账号仍会从源头通过鉴权，
        // 违背导入/删除语义（删除后不应再生效）。
        let registered = {
            let local = self.local_accounts.read().await;
            local.get(account_id).cloned()
        };
        let registered = match registered {
            Some(a) => a,
            None => return Err(format!("账号 {account_id} 未找到")),
        };

        // API key（ksk_）是长期 bearer token，无需也无法刷新，直接返回。
        if registered.auth_method == "apikey" || is_api_key(&registered.access_token) {
            return Ok(registered.access_token);
        }

        // 动态账号（kiro-cli / kiro-ide）：先从源头读取，如果未过期直接返回
        if let Some(acc) = self.read_dynamic_account(account_id) {
            if acc.expires_at_ms - now > EXPIRES_BUFFER_MS {
                return Ok(acc.access_token);
            }
        }

        // 用于刷新的账号详情：优先源头最新凭证，回退到已注册快照
        let mut acc = self.read_dynamic_account(account_id).unwrap_or(registered);

        if acc.expires_at_ms - now > EXPIRES_BUFFER_MS {
            return Ok(acc.access_token.clone());
        }

        // 获取刷新锁
        let lock = {
            let mut locks = self.refresh_locks.write().await;
            locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        let now = chrono::Utc::now().timestamp_millis();

        // 获锁后二次检查（可能其他任务已刷新或源头已更新）
        if let Some(dyn_acc) = self.read_dynamic_account(account_id) {
            if dyn_acc.expires_at_ms - now > EXPIRES_BUFFER_MS {
                return Ok(dyn_acc.access_token);
            }
            // 源头可能轮换了 refresh token，采用最新的凭证进行刷新
            acc = dyn_acc;
        } else {
            let local = self.local_accounts.read().await;
            if let Some(a) = local.get(account_id) {
                if a.expires_at_ms - now > EXPIRES_BUFFER_MS {
                    return Ok(a.access_token.clone());
                }
                acc = a.clone();
            }
        }

        // 开始请求刷新
        log::info!("[KiroAuth] 正在刷新 Kiro 凭证: {account_id}");
        match self.refresh_token_direct(&acc).await {
            Ok(refreshed) => {
                self.persist_refreshed(account_id, refreshed.clone()).await;
                Ok(refreshed.access_token)
            }
            Err(refresh_err) => {
                // 回退层 1：kiro-cli 可能在我们刷新期间自行轮换了 token，重新读取源头
                if let Some(dyn_acc) = self.read_dynamic_account(account_id) {
                    let now = chrono::Utc::now().timestamp_millis();
                    if dyn_acc.expires_at_ms - now > EXPIRES_BUFFER_MS {
                        return Ok(dyn_acc.access_token);
                    }
                    // 回退层 2：用源头更新后的（可能更新）refresh token 重试刷新
                    if dyn_acc.refresh_token != acc.refresh_token {
                        if let Ok(refreshed) = self.refresh_token_direct(&dyn_acc).await {
                            self.persist_refreshed(account_id, refreshed.clone()).await;
                            return Ok(refreshed.access_token);
                        }
                    }
                }

                // 回退层 3：优雅降级 —— 我们的过期时间带有 5 分钟缓冲，
                // 实际 AWS token 可能仍有效，返回以争取时间
                let now = chrono::Utc::now().timestamp_millis();
                if !acc.access_token.is_empty() && now < acc.expires_at_ms + EXPIRES_BUFFER_MS {
                    log::warn!("[KiroAuth] 刷新失败，使用缓冲期内的现有 token: {refresh_err}");
                    return Ok(acc.access_token.clone());
                }

                Err(refresh_err)
            }
        }
    }

    /// 读取动态账号（kiro-cli / kiro-ide）的最新凭证。不是动态账号则返回 None。
    fn read_dynamic_account(&self, account_id: &str) -> Option<KiroAccountData> {
        if account_id == "kiro_ide" {
            self.get_kiro_ide_account()
        } else if account_id.starts_with("kiro_cli_") {
            let method = if account_id.ends_with("social") {
                "desktop"
            } else {
                "idc"
            };
            self.get_kiro_cli_account(method)
        } else {
            None
        }
    }

    /// 直接调用 AWS / Kiro 端点刷新凭证，返回更新后的账号数据（不修改存储）。
    async fn refresh_token_direct(&self, acc: &KiroAccountData) -> Result<KiroAccountData, String> {
        let (new_access, new_refresh, expires_in, profile_arn) = if acc.auth_method == "desktop" {
            // Desktop Refresh
            let url = format!(
                "https://prod.{}.auth.desktop.kiro.dev/refreshToken",
                acc.region
            );
            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("User-Agent", KIRO_DESKTOP_USER_AGENT)
                .json(&serde_json::json!({ "refreshToken": acc.refresh_token }))
                .send()
                .await
                .map_err(|e| format!("Desktop 刷新网络错误: {e}"))?;

            if !res.status().is_success() {
                return Err(format!("Desktop 刷新失败: {}", res.status()));
            }

            let data: DesktopRefreshResponse = res
                .json()
                .await
                .map_err(|e| format!("解析 Desktop 刷新响应失败: {e}"))?;

            (
                data.access_token,
                data.refresh_token
                    .unwrap_or_else(|| acc.refresh_token.clone()),
                data.expires_in,
                data.profile_arn.or_else(|| acc.profile_arn.clone()),
            )
        } else {
            // IDC OIDC Refresh
            let sso_endpoint = format!("https://oidc.{}.amazonaws.com", acc.region);
            let (oidc_ua, oidc_amz_ua) = kiro_user_agent(KiroSdkApi::Ssooidc, "E");
            let res = self
                .http_client
                .post(format!("{sso_endpoint}/token"))
                .header("Content-Type", "application/json")
                .header("User-Agent", oidc_ua)
                .header("x-amz-user-agent", oidc_amz_ua)
                .json(&serde_json::json!({
                    "clientId": acc.client_id,
                    "clientSecret": acc.client_secret,
                    "refreshToken": acc.refresh_token,
                    "grantType": "refresh_token"
                }))
                .send()
                .await
                .map_err(|e| format!("IDC OIDC 刷新网络错误: {e}"))?;

            if !res.status().is_success() {
                return Err(format!("IDC OIDC 刷新失败: {}", res.status()));
            }

            let data: TokenResponse = res
                .json()
                .await
                .map_err(|e| format!("解析 IDC OIDC 刷新响应失败: {e}"))?;

            (
                data.access_token,
                data.refresh_token,
                data.expires_in,
                acc.profile_arn.clone(),
            )
        };

        let mut updated = acc.clone();
        updated.access_token = new_access;
        updated.refresh_token = new_refresh;
        updated.profile_arn = profile_arn;
        // 减去 5 分钟缓冲，提前刷新
        updated.expires_at_ms =
            chrono::Utc::now().timestamp_millis() + (expires_in as i64) * 1000 - EXPIRES_BUFFER_MS;
        Ok(updated)
    }

    /// 将刷新后的凭证写回相应来源（kiro-cli DB / kiro-ide 不回写，本地写文件）。
    async fn persist_refreshed(&self, account_id: &str, acc: KiroAccountData) {
        match acc.source.as_str() {
            "kiro-cli" => {
                let token_key = if acc.auth_method == "desktop" {
                    "kirocli:social:token"
                } else {
                    "kirocli:odic:token"
                };
                // 回写时恢复近似实际过期时间（加回缓冲）
                let actual_expires_ms = acc.expires_at_ms + EXPIRES_BUFFER_MS;
                let updated_value = serde_json::json!({
                    "access_token": acc.access_token,
                    "refresh_token": acc.refresh_token,
                    "expires_at": chrono::DateTime::from_timestamp(actual_expires_ms / 1000, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default(),
                    "region": acc.region,
                    "profile_arn": acc.profile_arn,
                    "start_url": acc.start_url
                });
                if let Err(e) = self.write_kiro_cli_token(token_key, &updated_value) {
                    log::warn!("[KiroAuth] 写入 kiro-cli DB 失败: {e}");
                }
            }
            "kiro-ide" => {
                // Kiro IDE 由 IDE 自己维护凭证文件，不回写，仅使用本次刷新结果
            }
            _ => {
                {
                    let mut local = self.local_accounts.write().await;
                    local.insert(account_id.to_string(), acc);
                }
                let _ = self.save_to_disk().await;
            }
        }
    }

    /// 获取首个可用账号的有效 Token
    pub async fn get_valid_token(&self) -> Result<String, String> {
        let accounts = self.list_accounts().await;
        if accounts.is_empty() {
            return Err("未配置任何 Kiro 账号".to_string());
        }

        let default_id = self.default_account_id.read().await.clone();
        let target_id = default_id
            .as_deref()
            .and_then(|id| accounts.iter().find(|a| a.id == id))
            .map(|a| a.id.as_str())
            .unwrap_or_else(|| accounts[0].id.as_str());

        self.get_valid_token_for_account(target_id).await
    }

    /// 获取特定账号的 Region
    pub async fn get_region_for_account(&self, account_id: Option<&str>) -> Option<String> {
        let id = match account_id {
            Some(id) => id.to_string(),
            None => {
                let default_id = self.default_account_id.read().await.clone();
                if let Some(did) = default_id {
                    did
                } else {
                    let accounts = self.list_accounts().await;
                    if accounts.is_empty() {
                        return None;
                    }
                    accounts[0].id.clone()
                }
            }
        };

        let local = self.local_accounts.read().await;
        if let Some(a) = local.get(&id) {
            return Some(a.region.clone());
        }
        if let Some(a) = self.read_dynamic_account(&id) {
            return Some(a.region.clone());
        }
        None
    }

    /// 获取特定账号的 profileArn
    pub async fn get_profile_arn_for_account(&self, account_id: Option<&str>) -> Option<String> {
        let id = match account_id {
            Some(id) => id.to_string(),
            None => {
                let default_id = self.default_account_id.read().await.clone();
                if let Some(did) = default_id {
                    did
                } else {
                    let accounts = self.list_accounts().await;
                    if accounts.is_empty() {
                        return None;
                    }
                    accounts[0].id.clone()
                }
            }
        };

        // 1) 优先返回已存储的 profileArn
        {
            let local = self.local_accounts.read().await;
            if let Some(a) = local.get(&id) {
                if a.profile_arn.is_some() {
                    return a.profile_arn.clone();
                }
            }
        }
        if let Some(a) = self.read_dynamic_account(&id) {
            if a.profile_arn.is_some() {
                return a.profile_arn.clone();
            }
        }

        // 2) 未存储（如从 kiro-cli / kiro-ide 导入的账号）：通过 ListAvailableProfiles 拉取并回写缓存
        let token = self.get_valid_token_for_account(&id).await.ok()?;
        let region = self
            .get_region_for_account(Some(&id))
            .await
            .unwrap_or_else(|| DEFAULT_REGION.to_string());
        // 读取账号的 auth_method / start_url 用于 Builder ID 兑底判断
        let (auth_method, start_url) = {
            let local = self.local_accounts.read().await;
            local
                .get(&id)
                .map(|a| (a.auth_method.clone(), a.start_url.clone()))
                .or_else(|| {
                    self.read_dynamic_account(&id)
                        .map(|a| (a.auth_method, a.start_url))
                })
                .unwrap_or_else(|| (String::new(), None))
        };
        let fetched = self
            .fetch_profile_arn(&token, &region)
            .await
            .or_else(|| builder_id_fallback_profile_arn(&auth_method, start_url.as_deref()));
        if let Some(arn) = fetched.as_ref() {
            // 回写到本地快照（如果该账号在 local_accounts 中），避免重复拉取
            let updated = {
                let mut local = self.local_accounts.write().await;
                if let Some(a) = local.get_mut(&id) {
                    a.profile_arn = Some(arn.clone());
                    true
                } else {
                    false
                }
            };
            if updated {
                let _ = self.save_to_disk().await;
            }
        }
        fetched
    }

    /// 获取首个/默认账号的 profileArn
    pub async fn get_profile_arn(&self) -> Option<String> {
        self.get_profile_arn_for_account(None).await
    }

    /// 在指定 region 注册 OIDC 客户端并发起设备授权。
    /// region 拒绝该 startUrl 时返回 Ok(None)，以便调用方继续探测其他 region。
    async fn try_register_and_authorize(
        &self,
        start_url: &str,
        region: &str,
    ) -> Result<Option<(ClientRegisterResponse, DeviceAuthResponse, String)>, String> {
        let oidc_endpoint = format!("https://oidc.{region}.amazonaws.com");

        let (oidc_ua, oidc_amz_ua) = kiro_user_agent(KiroSdkApi::Ssooidc, "E");
        let reg_res = self
            .http_client
            .post(format!("{oidc_endpoint}/client/register"))
            .header("Content-Type", "application/json")
            .header("User-Agent", oidc_ua.clone())
            .header("x-amz-user-agent", oidc_amz_ua.clone())
            .json(&serde_json::json!({
                "clientName": KIRO_CLIENT_NAME,
                "clientType": "public",
                "scopes": SSO_SCOPES,
                "grantTypes": ["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"]
            }))
            .send()
            .await
            .map_err(|e| format!("注册 OIDC 客户端网络错误: {e}"))?;

        if !reg_res.status().is_success() {
            log::debug!(
                "[KiroAuth] register 被 region={region} 拒绝: {}",
                reg_res.status()
            );
            return Ok(None);
        }

        let reg_data: ClientRegisterResponse = reg_res
            .json()
            .await
            .map_err(|e| format!("解析注册客户端响应失败: {e}"))?;

        let auth_res = self
            .http_client
            .post(format!("{oidc_endpoint}/device_authorization"))
            .header("Content-Type", "application/json")
            .header("User-Agent", oidc_ua)
            .header("x-amz-user-agent", oidc_amz_ua)
            .json(&serde_json::json!({
                "clientId": reg_data.client_id,
                "clientSecret": reg_data.client_secret,
                "startUrl": start_url
            }))
            .send()
            .await
            .map_err(|e| format!("请求设备授权网络错误: {e}"))?;

        if !auth_res.status().is_success() {
            log::debug!(
                "[KiroAuth] device_authorization 被 region={region} 拒绝: {}",
                auth_res.status()
            );
            return Ok(None);
        }

        let auth_data: DeviceAuthResponse = auth_res
            .json()
            .await
            .map_err(|e| format!("解析设备授权响应失败: {e}"))?;

        Ok(Some((reg_data, auth_data, region.to_string())))
    }

    /// 探测多个 AWS region，找到接受该 startUrl 的 OIDC 端点。
    async fn detect_region_and_authorize(
        &self,
        start_url: &str,
    ) -> Result<(ClientRegisterResponse, DeviceAuthResponse, String), String> {
        log::info!("[KiroAuth] 正在检测 Identity Center region (start_url={start_url})");
        for region in IDC_PROBE_REGIONS {
            if let Some(result) = self.try_register_and_authorize(start_url, region).await? {
                log::info!("[KiroAuth] 检测到 region={region}");
                return Ok(result);
            }
        }
        Err(format!(
            "未找到接受 {start_url} 的 AWS region（已尝试: {}），请检查 start URL 后重试",
            IDC_PROBE_REGIONS.join(", ")
        ))
    }

    /// 启动 AWS OIDC 设备码登录流
    pub async fn start_device_flow(
        &self,
        start_url: Option<&str>,
        region: Option<&str>,
    ) -> Result<GitHubDeviceCodeResponse, String> {
        let start_url = start_url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(DEFAULT_START_URL);
        let region = region.filter(|s| !s.trim().is_empty());

        log::info!("[KiroAuth] 启动设备码登录 (start_url={start_url})");

        // 显式指定 region 则直接使用；Builder ID 默认 us-east-1；
        // 其他 IdC start_url 未指定 region 时逐个探测常见 region。
        let (reg_data, auth_data, resolved_region) = if let Some(region) = region {
            self.try_register_and_authorize(start_url, region)
                .await?
                .ok_or_else(|| format!("设备授权失败 (region={region})"))?
        } else if start_url == DEFAULT_START_URL {
            self.try_register_and_authorize(start_url, DEFAULT_REGION)
                .await?
                .ok_or_else(|| format!("设备授权失败 (region={DEFAULT_REGION})"))?
        } else {
            self.detect_region_and_authorize(start_url).await?
        };
        let interval = auth_data.interval.unwrap_or(5);
        let expires_in = auth_data.expires_in;
        let expires_at_ms = chrono::Utc::now().timestamp_millis() + (expires_in as i64) * 1000;

        // 3. 保存待轮询状态
        {
            let mut pending = self.pending_logins.write().await;
            pending.insert(
                auth_data.device_code.clone(),
                PendingKiroLogin {
                    client_id: reg_data.client_id,
                    client_secret: reg_data.client_secret,
                    region: resolved_region.clone(),
                    start_url: start_url.to_string(),
                    expires_at_ms,
                },
            );
        }

        let verification_uri = auth_data
            .verification_uri_complete
            .unwrap_or(auth_data.verification_uri);

        Ok(GitHubDeviceCodeResponse {
            device_code: auth_data.device_code,
            user_code: auth_data.user_code,
            verification_uri,
            expires_in,
            interval,
        })
    }

    /// 轮询授权结果
    pub async fn poll_for_token(&self, device_code: &str) -> Result<Option<GitHubAccount>, String> {
        let login_info = {
            let pending = self.pending_logins.read().await;
            pending.get(device_code).cloned()
        };

        let info = login_info.ok_or_else(|| "未找到对应的登录流程，请重新启动登录".to_string())?;

        if info.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            let mut pending = self.pending_logins.write().await;
            pending.remove(device_code);
            return Err("Device code expired".to_string());
        }

        let oidc_endpoint = format!("https://oidc.{}.amazonaws.com", info.region);
        let (oidc_ua, oidc_amz_ua) = kiro_user_agent(KiroSdkApi::Ssooidc, "E");
        let res = self
            .http_client
            .post(format!("{oidc_endpoint}/token"))
            .header("Content-Type", "application/json")
            .header("User-Agent", oidc_ua)
            .header("x-amz-user-agent", oidc_amz_ua)
            .json(&serde_json::json!({
                "clientId": info.client_id,
                "clientSecret": info.client_secret,
                "deviceCode": device_code,
                "grantType": "urn:ietf:params:oauth:grant-type:device_code"
            }))
            .send()
            .await
            .map_err(|e| format!("轮询 OIDC token 网络错误: {e}"))?;

        let status = res.status();
        if status == reqwest::StatusCode::BAD_REQUEST {
            // 400 错误通常是等待用户授权中 (authorization_pending) 或者慢速重试 (slow_down)
            #[derive(Deserialize)]
            struct ErrRes {
                error: String,
            }
            if let Ok(err_res) = res.json::<ErrRes>().await {
                if err_res.error == "authorization_pending" || err_res.error == "slow_down" {
                    return Ok(None);
                }
                return Err(format!("OIDC token 授权失败: {}", err_res.error));
            }
            return Err("OIDC token 授权失败".to_string());
        }

        if !status.is_success() {
            return Err(format!("OIDC token 授权服务器错误: {status}"));
        }

        let token_data: TokenResponse = res
            .json()
            .await
            .map_err(|e| format!("解析 Token 失败: {e}"))?;

        // 成功获取 Token，清理 pending 任务
        {
            let mut pending = self.pending_logins.write().await;
            pending.remove(device_code);
        }

        // 尝试获取 profileArn；Builder ID 拿不到时回退到共享固定 profileArn
        let profile_arn = self
            .fetch_profile_arn(&token_data.access_token, &info.region)
            .await
            .or_else(|| builder_id_fallback_profile_arn("idc", Some(&info.start_url)));

        let account_id = profile_arn.clone().unwrap_or_else(|| {
            // fallback: generate a uuid
            uuid::Uuid::new_v4().to_string()
        });

        let login = if info.start_url == DEFAULT_START_URL {
            "AWS Builder ID".to_string()
        } else {
            // parse start url host
            url::Url::parse(&info.start_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()))
                .unwrap_or(info.start_url.clone())
        };

        let new_account = KiroAccountData {
            account_id: account_id.clone(),
            login: login.clone(),
            auth_method: "idc".to_string(),
            access_token: token_data.access_token,
            refresh_token: token_data.refresh_token,
            client_id: info.client_id,
            client_secret: info.client_secret,
            region: info.region,
            profile_arn,
            start_url: Some(info.start_url),
            expires_at_ms: chrono::Utc::now().timestamp_millis()
                + (token_data.expires_in as i64) * 1000
                - EXPIRES_BUFFER_MS,
            authenticated_at: chrono::Utc::now().timestamp(),
            source: "local".to_string(),
        };

        // 保存到 local_accounts
        {
            let mut local = self.local_accounts.write().await;
            local.insert(account_id.clone(), new_account);
        }
        let _ = self.save_to_disk().await;

        Ok(Some(GitHubAccount {
            id: account_id,
            login,
            avatar_url: None,
            authenticated_at: chrono::Utc::now().timestamp(),
            github_domain: "kiro.dev".to_string(),
        }))
    }

    /// 获取 AWS CodeWhisperer profileArn。
    ///
    /// - OIDC / social token：通过 ListAvailableProfiles（返回 profiles 数组）
    /// - API key (ksk_)：必须用 GetProfile（返回单个 profile），ListAvailableProfiles
    ///   会被拒绝（Invalid token）；且管理面调用需额外的 tokentype: API_KEY 头
    async fn fetch_profile_arn(&self, access_token: &str, region: &str) -> Option<String> {
        // Kiro Q API 仅部署在 us-east-1 / eu-central-1，需映射 SSO region
        let api_region = resolve_api_region(Some(region));
        let management_url = format!("https://management.{api_region}.kiro.dev/");
        let use_api_key = is_api_key(access_token);
        let target = if use_api_key {
            "AmazonCodeWhispererService.GetProfile"
        } else {
            "AmazonCodeWhispererService.ListAvailableProfiles"
        };
        let mut req = self
            .http_client
            .post(&management_url)
            .header("Content-Type", "application/x-amz-json-1.0")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("X-Amz-Target", target);
        let (runtime_ua, runtime_amz_ua) = kiro_user_agent(KiroSdkApi::CodewhispererRuntime, "F,C");
        req = req
            .header("User-Agent", runtime_ua)
            .header("x-amz-user-agent", runtime_amz_ua);
        if use_api_key {
            req = req.header("tokentype", "API_KEY");
        }
        let res = req.body("{}").send().await.ok()?;

        if !res.status().is_success() {
            return None;
        }

        #[derive(Deserialize)]
        struct Profile {
            arn: Option<String>,
        }
        if use_api_key {
            #[derive(Deserialize)]
            struct GetProfileResponse {
                profile: Option<Profile>,
            }
            let data: GetProfileResponse = res.json().await.ok()?;
            data.profile?.arn
        } else {
            #[derive(Deserialize)]
            struct ListProfilesResponse {
                profiles: Option<Vec<Profile>>,
            }
            let data: ListProfilesResponse = res.json().await.ok()?;
            data.profiles?.into_iter().find(|p| p.arn.is_some())?.arn
        }
    }

    /// 使用 KIRO_API_KEY（ksk_ 格式）登录。
    ///
    /// API key 本身就是长期有效的 bearer token —— 无需 OIDC 交换、无需 kiro-cli。
    /// 仅做一次 GetProfile 校验并解析 profileArn，然后作为本地账号保存。
    pub async fn apikey_login(&self, api_key: &str) -> Result<GitHubAccount, String> {
        let api_key = api_key.trim();
        if !is_api_key(api_key) {
            return Err("无效的 API Key 格式，Kiro API Key 以 'ksk_' 开头".to_string());
        }

        // API key 由 us-east-1 控制面签发
        let region = "us-east-1".to_string();

        // GetProfile 校验 key 并解析 profileArn（同时验证 key 是否有效）
        let profile_arn = self.fetch_profile_arn(api_key, &region).await;
        if profile_arn.is_none() {
            return Err("API Key 被 Kiro 拒绝，请确认 key 有效且未过期".to_string());
        }

        let now = chrono::Utc::now().timestamp_millis();
        let account_id = profile_arn
            .clone()
            .unwrap_or_else(|| format!("kiro_apikey_{}", uuid::Uuid::new_v4().simple()));
        let account = KiroAccountData {
            account_id: account_id.clone(),
            login: "Kiro API Key".to_string(),
            auth_method: "apikey".to_string(),
            access_token: api_key.to_string(),
            // API key 同时充当 access 与 refresh，标记为 apikey 以跳过 OIDC 刷新
            refresh_token: format!("{api_key}|apikey"),
            client_id: String::new(),
            client_secret: String::new(),
            region,
            profile_arn,
            start_url: None,
            // API key 长期有效，给一个远期过期时间；被吊销时上游会返回 401
            expires_at_ms: now + 365 * 24 * 60 * 60 * 1000,
            authenticated_at: now,
            source: "local".to_string(),
        };

        {
            let mut local = self.local_accounts.write().await;
            local.insert(account_id.clone(), account.clone());
        }
        self.save_to_disk().await.ok();

        Ok(GitHubAccount {
            id: account.account_id,
            login: account.login,
            avatar_url: None,
            authenticated_at: chrono::Utc::now().timestamp(),
            github_domain: "kiro.dev".to_string(),
        })
    }

    /// 社交登录（Google / GitHub / 个人账号）—— PKCE + localhost:3128 回调。
    ///
    /// `on_url` 会在本地服务器启动后被调用一次，传入需要在浏览器中打开的授权 URL。
    /// `provider` 可为 `"google"` / `"github"`，为 None 时让用户在页面上选择。
    pub async fn social_login<F>(
        &self,
        provider: Option<&str>,
        on_url: F,
    ) -> Result<GitHubAccount, String>
    where
        F: FnOnce(String),
    {
        const REGION: &str = DEFAULT_REGION;
        const REDIRECT_URI: &str = "http://localhost:3128";

        // 1. 生成 state 与 PKCE
        let state = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let code_verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let code_challenge = {
            let mut hasher = Sha256::new();
            hasher.update(code_verifier.as_bytes());
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
        };

        let redirect_enc = urlencode(REDIRECT_URI);
        let provider_part = match provider {
            Some(p) => format!("&login_option={p}"),
            None => String::new(),
        };
        let auth_url = format!(
            "https://app.kiro.dev/signin?state={state}&code_challenge={code_challenge}&code_challenge_method=S256&redirect_uri={redirect_enc}&redirect_from=kirocli{provider_part}"
        );

        // 2. 启动本地回调服务器
        let listener = tokio::net::TcpListener::bind("127.0.0.1:3128")
            .await
            .map_err(|e| format!("启动本地 OAuth 服务器失败 (端口 3128): {e}"))?;

        // 服务器就绪后才通知调用方打开浏览器
        on_url(auth_url);

        // 3. 等待回调（带 10 分钟超时）
        let accept_fut = self.accept_social_callback(listener, &state, &code_verifier, REGION);
        let (creds, account) =
            match tokio::time::timeout(std::time::Duration::from_secs(600), accept_fut).await {
                Ok(res) => res?,
                Err(_) => return Err("社交登录超时".to_string()),
            };

        // 4. 保存账号
        {
            let mut local = self.local_accounts.write().await;
            local.insert(creds.account_id.clone(), creds);
        }
        let _ = self.save_to_disk().await;

        Ok(account)
    }

    /// 接受单个本地 HTTP 回调，解析授权码并换取 token。
    async fn accept_social_callback(
        &self,
        listener: tokio::net::TcpListener,
        expected_state: &str,
        code_verifier: &str,
        region: &str,
    ) -> Result<(KiroAccountData, GitHubAccount), String> {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("接受连接失败: {e}"))?;

            // 读取请求行（只需要第一行的 GET 路径）
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let request_line = req.lines().next().unwrap_or("");
            let path = request_line.split_whitespace().nth(1).unwrap_or("");

            // 解析查询参数
            let full_url = format!("http://localhost{path}");
            let parsed = url::Url::parse(&full_url).ok();
            let params: HashMap<String, String> = parsed
                .as_ref()
                .map(|u| u.query_pairs().into_owned().collect())
                .unwrap_or_default();

            let only_path = parsed
                .as_ref()
                .map(|u| u.path().to_string())
                .unwrap_or_default();
            // 忽略非回调路径（如 favicon）
            let allowed = ["/", "/oauth/callback", "/signin/callback"];
            if !allowed.contains(&only_path.as_str()) {
                write_http_response(&mut stream, 404, "text/plain", "Not Found").await;
                continue;
            }

            // 校验 state
            let state_param = params.get("state").map(|s| s.as_str()).unwrap_or("");
            if state_param != expected_state {
                write_http_response(
                    &mut stream,
                    400,
                    "text/html",
                    "<h3>Authentication failed: invalid state</h3>",
                )
                .await;
                return Err("state 不匹配".to_string());
            }

            let code = match params.get("code") {
                Some(c) if !c.is_empty() => c.clone(),
                _ => {
                    write_http_response(
                        &mut stream,
                        400,
                        "text/html",
                        "<h3>Authentication failed: missing authorization code</h3>",
                    )
                    .await;
                    return Err("缺少授权码".to_string());
                }
            };

            // 返回成功页面
            write_http_response(
                &mut stream,
                200,
                "text/html",
                "<!DOCTYPE html><html><head><title>Kiro Sign In</title></head>\
                 <body style=\"font-family:-apple-system,sans-serif;text-align:center;padding:50px\">\
                 <h2 style=\"color:#2ecc71\">Sign In Successful!</h2>\
                 <p>You can now close this tab and return to cc-switch.</p></body></html>",
            )
            .await;

            // 换取 token
            let login_option = params.get("login_option").cloned();
            let actual_redirect_uri = format!(
                "http://localhost:3128{}{}",
                if only_path == "/" {
                    ""
                } else {
                    only_path.as_str()
                },
                login_option
                    .as_ref()
                    .map(|o| format!("?login_option={o}"))
                    .unwrap_or_default()
            );

            let token_url = format!("https://prod.{region}.auth.desktop.kiro.dev/oauth/token");
            let res = self
                .http_client
                .post(&token_url)
                .header("Content-Type", "application/json")
                .header("User-Agent", KIRO_DESKTOP_USER_AGENT)
                .json(&serde_json::json!({
                    "code": code,
                    "code_verifier": code_verifier,
                    "redirect_uri": actual_redirect_uri
                }))
                .send()
                .await
                .map_err(|e| format!("换取 token 网络错误: {e}"))?;

            if !res.status().is_success() {
                return Err(format!("换取 token 失败: {}", res.status()));
            }

            let data: DesktopRefreshResponse = res
                .json()
                .await
                .map_err(|e| format!("解析 token 响应失败: {e}"))?;

            if data.access_token.is_empty() {
                return Err("响应中缺少 accessToken".to_string());
            }

            let account_id = data
                .profile_arn
                .clone()
                .unwrap_or_else(|| format!("kiro_social_{}", uuid::Uuid::new_v4().simple()));
            let login = match login_option.as_deref() {
                Some("google") => "Kiro (Google)".to_string(),
                Some("github") => "Kiro (GitHub)".to_string(),
                _ => "Kiro (Social)".to_string(),
            };
            let now = chrono::Utc::now();

            let creds = KiroAccountData {
                account_id: account_id.clone(),
                login: login.clone(),
                auth_method: "desktop".to_string(),
                access_token: data.access_token,
                refresh_token: data.refresh_token.unwrap_or_default(),
                client_id: String::new(),
                client_secret: String::new(),
                region: region.to_string(),
                profile_arn: data.profile_arn,
                start_url: None,
                expires_at_ms: now.timestamp_millis() + (data.expires_in as i64) * 1000
                    - EXPIRES_BUFFER_MS,
                authenticated_at: now.timestamp(),
                source: "local".to_string(),
            };

            let account = GitHubAccount {
                id: account_id,
                login,
                avatar_url: None,
                authenticated_at: now.timestamp(),
                github_domain: "kiro.dev".to_string(),
            };

            return Ok((creds, account));
        }
    }
}

/// 最小 URL 编码（仅针对 redirect_uri）
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 写出一个简单的 HTTP 响应
async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

#[cfg(test)]
mod tests {
    use super::{is_api_key, resolve_api_region};

    #[test]
    fn is_api_key_detects_ksk_prefix() {
        assert!(is_api_key("ksk_abc123"));
        assert!(!is_api_key("aoaAAAAtoken"));
        assert!(!is_api_key(""));
        assert!(!is_api_key("sk-ant-xxx"));
    }

    #[test]
    fn builder_id_fallback_only_for_builder_id() {
        use super::{builder_id_fallback_profile_arn, BUILDER_ID_PROFILE_ARN, DEFAULT_START_URL};
        // Builder ID（idc + 默认 start_url）→ 兑底
        assert_eq!(
            builder_id_fallback_profile_arn("idc", Some(DEFAULT_START_URL)).as_deref(),
            Some(BUILDER_ID_PROFILE_ARN)
        );
        // 企业 IdC（其他 start_url）→ 不兑底
        assert_eq!(
            builder_id_fallback_profile_arn("idc", Some("https://my-org.awsapps.com/start")),
            None
        );
        // 社交 / apikey → 不兑底
        assert_eq!(builder_id_fallback_profile_arn("desktop", None), None);
        assert_eq!(builder_id_fallback_profile_arn("apikey", None), None);
        // idc 但无 start_url → 不兑底
        assert_eq!(builder_id_fallback_profile_arn("idc", None), None);
    }

    #[test]
    fn resolve_api_region_maps_to_deployed_regions() {
        // 默认 / 空
        assert_eq!(resolve_api_region(None), "us-east-1");
        assert_eq!(resolve_api_region(Some("")), "us-east-1");
        // 已部署 region 原样
        assert_eq!(resolve_api_region(Some("us-east-1")), "us-east-1");
        assert_eq!(resolve_api_region(Some("eu-central-1")), "eu-central-1");
        // 美洲 / 亚太 -> us-east-1
        assert_eq!(resolve_api_region(Some("ap-northeast-1")), "us-east-1");
        assert_eq!(resolve_api_region(Some("us-west-2")), "us-east-1");
        assert_eq!(resolve_api_region(Some("ap-southeast-1")), "us-east-1");
        // 欧洲 -> eu-central-1
        assert_eq!(resolve_api_region(Some("eu-west-1")), "eu-central-1");
        assert_eq!(resolve_api_region(Some("eu-north-1")), "eu-central-1");
        // 未知 region 原样保留
        assert_eq!(resolve_api_region(Some("xx-region-9")), "xx-region-9");
    }
}
