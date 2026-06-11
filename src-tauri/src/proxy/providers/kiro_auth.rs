//! Kiro Authentication Module
//!
//! 实现 AWS Builder ID / IAM Identity Center OIDC 认证流程，以及 kiro-cli SQLite 凭证共享。

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};

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
    user_code: String,
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
        let manager = Self {
            local_accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            pending_logins: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path,
        };

        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[KiroAuth] 加载存储失败: {e}");
        }

        manager
    }

    fn load_from_disk_sync(&self) -> Result<(), String> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)
            .map_err(|e| format!("读取文件失败: {e}"))?;
        #[derive(Deserialize)]
        struct SavedData {
            accounts: HashMap<String, KiroAccountData>,
            default_account_id: Option<String>,
        }
        let data: SavedData = serde_json::from_str(&content)
            .map_err(|e| format!("解析 JSON 失败: {e}"))?;

        // 过滤掉已保存的 kiro-cli 账号，它们应该动态从 DB 中读取
        let mut local = HashMap::new();
        for (k, v) in data.accounts {
            if v.source != "kiro-cli" {
                local.insert(k, v);
            }
        }

        let mut accounts_guard = self.local_accounts.blocking_write();
        *accounts_guard = local;

        let mut default_guard = self.default_account_id.blocking_write();
        *default_guard = data.default_account_id;

        Ok(())
    }

    fn save_to_disk_sync(&self) -> Result<(), String> {
        let accounts = self.local_accounts.blocking_read().clone();
        let default_account_id = self.default_account_id.blocking_read().clone();

        #[derive(Serialize)]
        struct SavedData {
            accounts: HashMap<String, KiroAccountData>,
            default_account_id: Option<String>,
        }

        let data = SavedData {
            accounts,
            default_account_id,
        };

        let content = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("序列化失败: {e}"))?;

        let mut file = fs::File::create(&self.storage_path)
            .map_err(|e| format!("创建文件失败: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("写入数据失败: {e}"))?;

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
        let region = val.get("region").and_then(|v| v.as_str()).unwrap_or("us-east-1").to_string();
        let profile_arn = val.get("profile_arn").or_else(|| val.get("profileArn")).and_then(|v| v.as_str()).map(String::from);
        let start_url = val.get("start_url").or_else(|| val.get("startUrl")).and_then(|v| v.as_str()).map(String::from);

        let expires_at_ms = if let Some(expires_at_str) = val.get("expires_at").and_then(|v| v.as_str()) {
            chrono::DateTime::parse_from_rfc3339(expires_at_str)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis() + 3600_000)
        } else {
            chrono::Utc::now().timestamp_millis() + 3600_000
        };

        let mut client_id = String::new();
        let mut client_secret = String::new();

        if method == "idc" {
            if let Some(dev_reg) = self.read_kiro_cli_token(&db_path, "kirocli:odic:device-registration") {
                if let Some(cid) = dev_reg.get("client_id").or_else(|| dev_reg.get("clientId")).and_then(|v| v.as_str()) {
                    client_id = cid.to_string();
                }
                if let Some(csec) = dev_reg.get("client_secret").or_else(|| dev_reg.get("clientSecret")).and_then(|v| v.as_str()) {
                    client_secret = csec.to_string();
                }
            }
        }

        let account_id = format!("kiro_cli_{method}");
        let login = if method == "desktop" {
            "kiro-cli (Social)".to_string()
        } else {
            format!("kiro-cli (Builder ID / IdC)")
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
        let mut stmt = conn.prepare("SELECT value FROM auth_kv WHERE key = ?1").ok()?;
        let val_str: String = stmt.query_row([key], |row| row.get(0)).ok()?;
        serde_json::from_str(&val_str).ok()
    }

    fn write_kiro_cli_token(&self, token_key: &str, value: &serde_json::Value) -> Result<(), String> {
        let db_path = self.get_kiro_cli_db_path()
            .ok_or_else(|| "kiro-cli database not found".to_string())?;
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("打开数据库失败: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO auth_kv (key, value) VALUES (?1, ?2)",
            (token_key, serde_json::to_string(value).unwrap_or_default()),
        ).map_err(|e| format!("写入数据库失败: {e}"))?;
        Ok(())
    }

    /// 列出所有已认证的账号（合并本地和 kiro-cli 的账号）
    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let mut map = self.local_accounts.read().await.clone();

        // 动态读取 kiro-cli
        if let Some(cli_idc) = self.get_kiro_cli_account("idc") {
            map.insert(cli_idc.account_id.clone(), cli_idc);
        }
        if let Some(cli_social) = self.get_kiro_cli_account("desktop") {
            map.insert(cli_social.account_id.clone(), cli_social);
        }

        let default_account_id = self.default_account_id.read().await.clone();
        let mut list = Vec::new();
        for (_, val) in map {
            let is_default = default_account_id.as_deref() == Some(val.account_id.as_str())
                || (default_account_id.is_none() && val.account_id.starts_with("kiro_cli_idc")); // default to kiro-cli idc if no default set

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

    /// 设置默认账号
    pub async fn set_default_account(&self, account_id: &str) -> Result<(), String> {
        let mut default_guard = self.default_account_id.write().await;
        *default_guard = Some(account_id.to_string());
        let _ = self.save_to_disk_sync();
        Ok(())
    }

    /// 移除账号
    pub async fn remove_account(&self, account_id: &str) -> Result<(), String> {
        let mut local = self.local_accounts.write().await;
        if local.remove(account_id).is_some() {
            let _ = self.save_to_disk_sync();
        }
        Ok(())
    }

    /// 登出全部（清除本地，不清理 kiro-cli，仅取消默认关联）
    pub async fn logout(&self) -> Result<(), String> {
        let mut local = self.local_accounts.write().await;
        local.clear();
        let mut default_guard = self.default_account_id.write().await;
        *default_guard = None;
        let _ = self.save_to_disk_sync();
        Ok(())
    }

    /// 获取指定账号的有效 Token
    pub async fn get_valid_token_for_account(&self, account_id: &str) -> Result<String, String> {
        // 1. 如果是 kiro-cli 账号，先检查 kiro-cli 本地 DB 中是否有未过期的 token
        if account_id.starts_with("kiro_cli_") {
            let method = if account_id.ends_with("social") { "desktop" } else { "idc" };
            if let Some(cli_acc) = self.get_kiro_cli_account(method) {
                let now = chrono::Utc::now().timestamp_millis();
                if cli_acc.expires_at_ms - now > 5 * 60 * 1000 {
                    return Ok(cli_acc.access_token);
                }
            }
        }

        // 2. 查找内存/配置中的账号详情进行刷新
        let mut acc = {
            let local = self.local_accounts.read().await;
            if let Some(a) = local.get(account_id) {
                a.clone()
            } else if account_id.starts_with("kiro_cli_") {
                let method = if account_id.ends_with("social") { "desktop" } else { "idc" };
                self.get_kiro_cli_account(method)
                    .ok_or_else(|| format!("kiro-cli 凭证已失效且无法在 DB 中找到"))?
            } else {
                return Err(format!("账号 {account_id} 未找到"));
            }
        };

        let now = chrono::Utc::now().timestamp_millis();
        if acc.expires_at_ms - now > 5 * 60 * 1000 {
            return Ok(acc.access_token.clone());
        }

        // 获取刷新锁
        let lock = {
            let mut locks = self.refresh_locks.write().await;
            locks.entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        // Double check after acquiring lock
        if account_id.starts_with("kiro_cli_") {
            let method = if account_id.ends_with("social") { "desktop" } else { "idc" };
            if let Some(cli_acc) = self.get_kiro_cli_account(method) {
                if cli_acc.expires_at_ms - now > 5 * 60 * 1000 {
                    return Ok(cli_acc.access_token);
                }
            }
        } else {
            let local = self.local_accounts.read().await;
            if let Some(a) = local.get(account_id) {
                if a.expires_at_ms - now > 5 * 60 * 1000 {
                    return Ok(a.access_token.clone());
                }
            }
        }

        // 开始请求刷新
        log::info!("[KiroAuth] 正在刷新 Kiro 凭证: {account_id}");
        let (new_access, new_refresh, expires_in, profile_arn) = if acc.auth_method == "desktop" {
            // Desktop Refresh
            let url = format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", acc.region);
            let res = self.http_client.post(&url)
                .header("Content-Type", "application/json")
                .header("User-Agent", "cc-switch-kiro")
                .json(&serde_json::json!({ "refreshToken": acc.refresh_token }))
                .send()
                .await
                .map_err(|e| format!("Desktop 刷新网络错误: {e}"))?;

            if !res.status().is_success() {
                return Err(format!("Desktop 刷新失败: {}", res.status()));
            }

            let data: DesktopRefreshResponse = res.json()
                .await
                .map_err(|e| format!("解析 Desktop 刷新响应失败: {e}"))?;

            (
                data.access_token,
                data.refresh_token.unwrap_or(acc.refresh_token.clone()),
                data.expires_in,
                data.profile_arn,
            )
        } else {
            // IDC OIDC Refresh
            let sso_endpoint = format!("https://oidc.{}.amazonaws.com", acc.region);
            let res = self.http_client.post(format!("{sso_endpoint}/token"))
                .header("Content-Type", "application/json")
                .header("User-Agent", "cc-switch-kiro")
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

            let data: TokenResponse = res.json()
                .await
                .map_err(|e| format!("解析 IDC OIDC 刷新响应失败: {e}"))?;

            (
                data.access_token,
                data.refresh_token,
                data.expires_in,
                acc.profile_arn.clone(),
            )
        };

        acc.access_token = new_access.clone();
        acc.refresh_token = new_refresh;
        acc.profile_arn = profile_arn;
        acc.expires_at_ms = chrono::Utc::now().timestamp_millis() + (expires_in as i64) * 1000;

        if acc.source == "kiro-cli" {
            // 写回 kiro-cli SQLite DB
            let token_key = if acc.auth_method == "desktop" {
                "kirocli:social:token"
            } else {
                "kirocli:odic:token"
            };

            let updated_value = serde_json::json!({
                "access_token": acc.access_token,
                "refresh_token": acc.refresh_token,
                "expires_at": chrono::DateTime::from_timestamp(acc.expires_at_ms / 1000, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                "region": acc.region,
                "profile_arn": acc.profile_arn,
                "start_url": acc.start_url
            });

            if let Err(e) = self.write_kiro_cli_token(token_key, &updated_value) {
                log::warn!("[KiroAuth] 写入 kiro-cli DB 失败: {e}");
            }
        } else {
            // 保存至本地文件
            let mut local = self.local_accounts.write().await;
            local.insert(account_id.to_string(), acc.clone());
            let _ = self.save_to_disk_sync();
        }

        Ok(new_access)
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
        if id.starts_with("kiro_cli_") {
            let method = if id.ends_with("social") { "desktop" } else { "idc" };
            if let Some(a) = self.get_kiro_cli_account(method) {
                return Some(a.region.clone());
            }
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

        let local = self.local_accounts.read().await;
        if let Some(a) = local.get(&id) {
            return a.profile_arn.clone();
        }
        if id.starts_with("kiro_cli_") {
            let method = if id.ends_with("social") { "desktop" } else { "idc" };
            if let Some(a) = self.get_kiro_cli_account(method) {
                return a.profile_arn.clone();
            }
        }
        None
    }

    /// 获取首个/默认账号的 profileArn
    pub async fn get_profile_arn(&self) -> Option<String> {
        self.get_profile_arn_for_account(None).await
    }

    /// 启动 AWS OIDC 设备码登录流
    pub async fn start_device_flow(
        &self,
        start_url: Option<&str>,
        region: Option<&str>,
    ) -> Result<GitHubDeviceCodeResponse, String> {
        let start_url = start_url.filter(|s| !s.trim().is_empty()).unwrap_or(DEFAULT_START_URL);
        let region = region.filter(|s| !s.trim().is_empty()).unwrap_or(DEFAULT_REGION);

        log::info!("[KiroAuth] 注册 OIDC Client (region={region})");
        let oidc_endpoint = format!("https://oidc.{region}.amazonaws.com");

        // 1. 注册客户端
        let reg_res = self.http_client.post(format!("{oidc_endpoint}/client/register"))
            .header("Content-Type", "application/json")
            .header("User-Agent", "cc-switch-kiro")
            .json(&serde_json::json!({
                "clientName": "cc-switch",
                "clientType": "public",
                "scopes": SSO_SCOPES,
                "grantTypes": ["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"]
            }))
            .send()
            .await
            .map_err(|e| format!("注册 OIDC 客户端网络错误: {e}"))?;

        if !reg_res.status().is_success() {
            return Err(format!("注册 OIDC 客户端失败: {}", reg_res.status()));
        }

        let reg_data: ClientRegisterResponse = reg_res.json()
            .await
            .map_err(|e| format!("解析注册客户端响应失败: {e}"))?;

        // 2. 发起设备授权
        log::info!("[KiroAuth] 获取设备授权 (start_url={start_url})");
        let auth_res = self.http_client.post(format!("{oidc_endpoint}/device_authorization"))
            .header("Content-Type", "application/json")
            .header("User-Agent", "cc-switch-kiro")
            .json(&serde_json::json!({
                "clientId": reg_data.client_id,
                "clientSecret": reg_data.client_secret,
                "startUrl": start_url
            }))
            .send()
            .await
            .map_err(|e| format!("请求设备授权网络错误: {e}"))?;

        if !auth_res.status().is_success() {
            return Err(format!("请求设备授权失败: {}", auth_res.status()));
        }

        let auth_data: DeviceAuthResponse = auth_res.json()
            .await
            .map_err(|e| format!("解析设备授权响应失败: {e}"))?;

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
                    region: region.to_string(),
                    start_url: start_url.to_string(),
                    user_code: auth_data.user_code.clone(),
                    expires_at_ms,
                },
            );
        }

        let verification_uri = auth_data.verification_uri_complete
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
    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<GitHubAccount>, String> {
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
        let res = self.http_client.post(format!("{oidc_endpoint}/token"))
            .header("Content-Type", "application/json")
            .header("User-Agent", "cc-switch-kiro")
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
                if err_res.error == "authorization_pending" {
                    return Ok(None);
                } else if err_res.error == "slow_down" {
                    return Ok(None);
                }
                return Err(format!("OIDC token 授权失败: {}", err_res.error));
            }
            return Err("OIDC token 授权失败".to_string());
        }

        if !status.is_success() {
            return Err(format!("OIDC token 授权服务器错误: {status}"));
        }

        let token_data: TokenResponse = res.json()
            .await
            .map_err(|e| format!("解析 Token 失败: {e}"))?;

        // 成功获取 Token，清理 pending 任务
        {
            let mut pending = self.pending_logins.write().await;
            pending.remove(device_code);
        }

        // 尝试获取 profileArn
        let profile_arn = self.fetch_profile_arn(&token_data.access_token, &info.region).await;

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
            expires_at_ms: chrono::Utc::now().timestamp_millis() + (token_data.expires_in as i64) * 1000,
            authenticated_at: chrono::Utc::now().timestamp(),
            source: "local".to_string(),
        };

        // 保存到 local_accounts
        {
            let mut local = self.local_accounts.write().await;
            local.insert(account_id.clone(), new_account);
        }
        let _ = self.save_to_disk_sync();

        Ok(Some(GitHubAccount {
            id: account_id,
            login,
            avatar_url: None,
            authenticated_at: chrono::Utc::now().timestamp(),
            github_domain: "kiro.dev".to_string(),
        }))
    }

    /// 获取 AWS CodeWhisperer profileArn
    async fn fetch_profile_arn(&self, access_token: &str, region: &str) -> Option<String> {
        let management_url = format!("https://management.{region}.kiro.dev/");
        let res = self.http_client.post(&management_url)
            .header("Content-Type", "application/x-amz-json-1.0")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("X-Amz-Target", "AmazonCodeWhispererService.ListAvailableProfiles")
            .body("{}")
            .send()
            .await
            .ok()?;

        if !res.status().is_success() {
            return None;
        }

        #[derive(Deserialize)]
        struct Profile {
            arn: Option<String>,
        }
        #[derive(Deserialize)]
        struct ListProfilesResponse {
            profiles: Option<Vec<Profile>>,
        }

        let data: ListProfilesResponse = res.json().await.ok()?;
        data.profiles?.into_iter().find(|p| p.arn.is_some())?.arn
    }
}
