//! GitHub Copilot Authentication Module
//!
//! 实现 GitHub OAuth 设备码流程和 Copilot 令牌管理。
//! 支持多账号认证，每个 Provider 可关联不同的 GitHub 账号。
//!
//! ## 认证流程
//! 1. 启动设备码流程，获取 device_code 和 user_code
//! 2. 用户在浏览器中完成 GitHub 授权
//! 3. 轮询获取 access_token
//! 4. 使用 GitHub token 获取 Copilot token
//! 5. 自动刷新 Copilot token（到期前 60 秒）
//!
//! ## 多账号支持 (v3)
//! - 每个 GitHub 账号独立存储 token
//! - Provider 通过 meta.authBinding 关联账号
//! - 自动迁移 v1 单账号格式到 v3 多账号 + 默认账号格式

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// GitHub OAuth 客户端 ID（VS Code 使用的 ID）
const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

/// GitHub 设备码 URL
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";

/// GitHub OAuth Token URL
const GITHUB_OAUTH_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// Copilot Token URL
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// GitHub User API URL
const GITHUB_USER_URL: &str = "https://api.github.com/user";

/// Token 刷新提前量（秒）
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;

/// Copilot API 端点
const COPILOT_MODELS_URL: &str = "https://api.githubcopilot.com/models";

/// Copilot API Header 常量
pub const COPILOT_EDITOR_VERSION: &str = "vscode/1.110.1";
pub const COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.38.2";
pub const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.38.2";
pub const COPILOT_API_VERSION: &str = "2025-10-01";
pub const COPILOT_INTEGRATION_ID: &str = "vscode-chat";

/// Copilot 使用量 API URL
const COPILOT_USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

/// 默认 Copilot API 端点
const DEFAULT_COPILOT_API_ENDPOINT: &str = "https://api.githubcopilot.com";

/// Copilot 使用量响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotUsageResponse {
    /// Copilot 计划类型
    pub copilot_plan: String,
    /// 配额重置日期
    pub quota_reset_date: String,
    /// 配额快照
    pub quota_snapshots: QuotaSnapshots,
    /// API 端点信息 (用于动态获取 API URL)
    #[serde(default)]
    pub endpoints: Option<CopilotEndpoints>,
}

/// Copilot API 端点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotEndpoints {
    /// API 端点 URL
    pub api: String,
    /// Telemetry 端点 URL
    #[serde(default)]
    pub telemetry: Option<String>,
}

/// 配额快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSnapshots {
    /// Chat 配额
    pub chat: QuotaDetail,
    /// Completions 配额
    pub completions: QuotaDetail,
    /// Premium 交互配额
    pub premium_interactions: QuotaDetail,
}

/// 配额详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaDetail {
    /// 总配额
    pub entitlement: i64,
    /// 剩余配额
    pub remaining: i64,
    /// 剩余百分比
    pub percent_remaining: f64,
    /// 是否无限
    pub unlimited: bool,
}

/// Copilot 可用模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotModel {
    /// 模型 ID（用于 API 调用）
    pub id: String,
    /// 模型显示名称
    pub name: String,
    /// 模型供应商
    pub vendor: String,
    /// 是否在模型选择器中显示
    pub model_picker_enabled: bool,
}

/// Copilot Models API 响应
#[derive(Debug, Deserialize)]
struct CopilotModelsResponse {
    data: Vec<CopilotModelsResponseItem>,
}

/// Copilot Models API 响应项
#[derive(Debug, Deserialize)]
struct CopilotModelsResponseItem {
    id: String,
    name: String,
    vendor: String,
    model_picker_enabled: bool,
}

/// Copilot 认证错误
#[derive(Debug, thiserror::Error)]
pub enum CopilotAuthError {
    #[error("设备码流程未启动")]
    DeviceFlowNotStarted,

    #[error("等待用户授权中")]
    AuthorizationPending,

    #[error("用户拒绝授权")]
    AccessDenied,

    #[error("设备码已过期")]
    ExpiredToken,

    #[error("GitHub 令牌无效或已过期")]
    GitHubTokenInvalid,

    #[error("Copilot 令牌获取失败: {0}")]
    CopilotTokenFetchFailed(String),

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("用户未订阅 Copilot")]
    NoCopilotSubscription,

    #[error("账号不存在: {0}")]
    AccountNotFound(String),
}

impl From<reqwest::Error> for CopilotAuthError {
    fn from(err: reqwest::Error) -> Self {
        CopilotAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CopilotAuthError {
    fn from(err: std::io::Error) -> Self {
        CopilotAuthError::IoError(err.to_string())
    }
}

/// GitHub 设备码响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDeviceCodeResponse {
    /// 设备码（用于轮询）
    pub device_code: String,
    /// 用户码（显示给用户）
    pub user_code: String,
    /// 验证 URL
    pub verification_uri: String,
    /// 过期时间（秒）
    pub expires_in: u64,
    /// 轮询间隔（秒）
    pub interval: u64,
}

/// GitHub OAuth Token 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubOAuthResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Copilot Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotToken {
    /// JWT Token
    pub token: String,
    /// 过期时间戳（Unix 秒）
    pub expires_at: i64,
}

impl CopilotToken {
    /// 检查令牌是否即将过期（提前 60 秒）
    pub fn is_expiring_soon(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at - now < TOKEN_REFRESH_BUFFER_SECONDS
    }
}

/// Copilot Token API 响应
#[derive(Debug, Deserialize)]
struct CopilotTokenResponse {
    token: String,
    expires_at: i64,
    #[allow(dead_code)]
    refresh_in: Option<i64>,
}

/// GitHub 用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub login: String,
    pub id: u64,
    pub avatar_url: Option<String>,
}

/// GitHub 账号（公开信息，返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAccount {
    /// GitHub 用户 ID（字符串形式，作为唯一标识）
    pub id: String,
    /// GitHub 用户名
    pub login: String,
    /// 头像 URL
    pub avatar_url: Option<String>,
    /// 认证时间戳
    pub authenticated_at: i64,
}

impl From<&GitHubAccountData> for GitHubAccount {
    fn from(data: &GitHubAccountData) -> Self {
        GitHubAccount {
            id: data.user.id.to_string(),
            login: data.user.login.clone(),
            avatar_url: data.user.avatar_url.clone(),
            authenticated_at: data.authenticated_at,
        }
    }
}

/// Copilot 认证状态（支持多账号）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotAuthStatus {
    /// 所有已认证的账号
    pub accounts: Vec<GitHubAccount>,
    /// 默认账号 ID（显式状态，避免依赖 HashMap 顺序）
    pub default_account_id: Option<String>,
    /// 旧认证数据迁移失败时的状态消息（用于前端提示）
    pub migration_error: Option<String>,
    /// 是否已认证（向后兼容：有任意账号即为 true）
    pub authenticated: bool,
    /// GitHub 用户名（向后兼容：第一个账号的用户名）
    pub username: Option<String>,
    /// Copilot 令牌过期时间（向后兼容：第一个账号的过期时间）
    pub expires_at: Option<i64>,
}

/// 账号数据（内部存储结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubAccountData {
    /// GitHub OAuth Token
    ///
    /// 安全说明：为了复用登录状态，本地会持久化该令牌。
    /// 当前实现未接入系统钥匙串，依赖私有文件权限（Unix 下 0600）保护。
    pub github_token: String,
    /// 用户信息
    pub user: GitHubUser,
    /// 认证时间戳
    pub authenticated_at: i64,
    /// 机器 ID（基于 MAC 地址的 SHA256 哈希，每个账号独立）
    #[serde(default)]
    pub machine_id: Option<String>,
    /// 会话 ID（UUID + 时间戳，每小时刷新）
    #[serde(default)]
    pub session_id: Option<String>,
    /// Session ID 上次刷新时间
    #[serde(default)]
    pub session_refreshed_at: Option<i64>,
}

/// 持久化存储结构（v3 多账号 + 默认账号格式）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CopilotAuthStore {
    /// 存储格式版本（3 = 多账号 + 默认账号格式）
    #[serde(default)]
    version: u32,
    /// 多账号数据（key = GitHub user ID）
    #[serde(default)]
    accounts: HashMap<String, GitHubAccountData>,
    /// 默认账号 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
    /// 兼容 v1 单账号格式的字段
    #[serde(skip_serializing_if = "Option::is_none")]
    github_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticated_at: Option<i64>,
}

/// Copilot 认证管理器（支持多账号）
pub struct CopilotAuthManager {
    /// 所有 GitHub 账号（key = GitHub user ID）
    accounts: Arc<RwLock<HashMap<String, GitHubAccountData>>>,
    /// 默认账号 ID
    default_account_id: Arc<RwLock<Option<String>>>,
    /// 每个账号的刷新锁，避免并发刷新重复打 GitHub API
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// Copilot Token 缓存（key = GitHub user ID，内存缓存，自动刷新）
    copilot_tokens: Arc<RwLock<HashMap<String, CopilotToken>>>,
    /// Copilot Models 缓存（key = GitHub user ID，仅进程内复用）
    copilot_models: Arc<RwLock<HashMap<String, Vec<CopilotModel>>>>,
    /// Copilot API 端点缓存（key = GitHub user ID，从 /copilot_internal/user 获取）
    api_endpoints: Arc<RwLock<HashMap<String, String>>>,
    /// 每个账号的端点拉取锁，避免并发拉取重复打 GitHub API
    endpoint_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// HTTP 客户端
    http_client: Client,
    /// 存储路径
    storage_path: PathBuf,
    /// 待迁移的旧格式 token
    pending_migration: Arc<RwLock<Option<String>>>,
    /// 旧认证数据迁移失败时的状态消息
    migration_error: Arc<RwLock<Option<String>>>,
    /// Session ID 刷新任务句柄（key = GitHub user ID）
    session_refresh_tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl CopilotAuthManager {
    const SESSION_REFRESH_BASE_SECS: i64 = 3600;

    /// 创建新的认证管理器
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("copilot_auth.json");

        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            copilot_tokens: Arc::new(RwLock::new(HashMap::new())),
            copilot_models: Arc::new(RwLock::new(HashMap::new())),
            api_endpoints: Arc::new(RwLock::new(HashMap::new())),
            endpoint_locks: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path,
            pending_migration: Arc::new(RwLock::new(None)),
            migration_error: Arc::new(RwLock::new(None)),
            session_refresh_tasks: Arc::new(RwLock::new(HashMap::new())),
        };

        // 尝试从磁盘加载（同步，不发起网络请求）
        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[CopilotAuth] 加载存储失败: {e}");
        }

        manager
    }

    /// 启动已加载账号的后台 Session ID 刷新任务。
    ///
    /// 这个方法要求调用方已经处于可用的异步运行时中。
    pub async fn initialize_background_tasks(&self) {
        let account_ids = {
            let accounts_guard = self.accounts.read().await;
            accounts_guard.keys().cloned().collect::<Vec<_>>()
        };

        for account_id in account_ids {
            Self::spawn_session_refresh_task(
                account_id,
                Arc::clone(&self.accounts),
                Arc::clone(&self.session_refresh_tasks),
            )
            .await;
        }
    }

    // ==================== 多账号管理方法 ====================

    /// 列出所有已认证的账号
    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    /// 获取指定账号信息
    pub async fn get_account(&self, account_id: &str) -> Option<GitHubAccount> {
        let accounts = self.accounts.read().await;
        accounts.get(account_id).map(GitHubAccount::from)
    }

    /// 移除指定账号
    pub async fn remove_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        log::info!("[CopilotAuth] 移除账号: {account_id}");

        {
            let mut accounts = self.accounts.write().await;
            if accounts.remove(account_id).is_none() {
                return Err(CopilotAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        // 同时移除缓存的 Copilot token
        {
            let mut tokens = self.copilot_tokens.write().await;
            tokens.remove(account_id);
        }
        {
            let mut models = self.copilot_models.write().await;
            models.remove(account_id);
        }
        {
            let mut refresh_locks = self.refresh_locks.write().await;
            refresh_locks.remove(account_id);
        }
        // 清理 API 端点缓存
        {
            let mut api_endpoints = self.api_endpoints.write().await;
            api_endpoints.remove(account_id);
        }
        {
            let mut endpoint_locks = self.endpoint_locks.write().await;
            endpoint_locks.remove(account_id);
        }

        // 停止 Session ID 刷新任务
        {
            let mut tasks = self.session_refresh_tasks.write().await;
            if let Some(task) = tasks.remove(account_id) {
                task.abort();
                log::debug!(
                    "[CopilotAuth] 已停止账号 {} 的 Session ID 刷新任务",
                    account_id
                );
            }
        }

        {
            let accounts = self.accounts.read().await;
            let mut default_account_id = self.default_account_id.write().await;
            if default_account_id.as_deref() == Some(account_id) {
                *default_account_id = Self::fallback_default_account_id(&accounts);
            }
        }

        // 持久化
        self.save_to_disk().await?;

        Ok(())
    }

    /// 添加新账号（内部方法，在 OAuth 完成后调用）
    async fn add_account_internal(
        &self,
        github_token: String,
        user: GitHubUser,
        machine_id: String,
    ) -> Result<GitHubAccount, CopilotAuthError> {
        let account_id = user.id.to_string();
        let now = chrono::Utc::now().timestamp();

        // 生成初始 Session ID
        let session_id = Self::generate_session_id();

        let account_data = GitHubAccountData {
            github_token,
            user: user.clone(),
            authenticated_at: now,
            machine_id: Some(machine_id),
            session_id: Some(session_id.clone()),
            session_refreshed_at: Some(now),
        };

        let account = GitHubAccount {
            id: account_id.clone(),
            login: user.login.clone(),
            avatar_url: user.avatar_url.clone(),
            authenticated_at: now,
        };

        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(account_id.clone(), account_data);
        }

        {
            let mut default_account_id = self.default_account_id.write().await;
            if default_account_id.is_none() {
                *default_account_id = Some(account.id.clone());
            }
        }

        self.set_migration_error(None).await;

        // 持久化
        self.save_to_disk().await?;

        // 启动 Session ID 刷新任务
        Self::spawn_session_refresh_task(
            account_id.clone(),
            Arc::clone(&self.accounts),
            Arc::clone(&self.session_refresh_tasks),
        )
        .await;

        log::info!("[CopilotAuth] 添加账号成功: {}", user.login);

        Ok(account)
    }

    /// 设置默认账号
    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CopilotAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let mut default_account_id = self.default_account_id.write().await;
            *default_account_id = Some(account_id.to_string());
        }

        self.save_to_disk().await?;
        Ok(())
    }

    // ==================== 设备码流程 ====================

    /// 启动设备码流程
    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, CopilotAuthError> {
        log::info!("[CopilotAuth] 启动设备码流程");

        let response = self
            .http_client
            .post(GITHUB_DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .header("User-Agent", COPILOT_USER_AGENT)
            .form(&[("client_id", GITHUB_CLIENT_ID), ("scope", "read:user")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CopilotAuthError::NetworkError(format!(
                "GitHub 设备码请求失败: {status} - {text}"
            )));
        }

        let device_code: GitHubDeviceCodeResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        log::info!(
            "[CopilotAuth] 获取设备码成功，user_code: {}",
            device_code.user_code
        );

        Ok(device_code)
    }

    /// 轮询获取 OAuth Token（返回新添加的账号，如果成功）
    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<GitHubAccount>, CopilotAuthError> {
        log::debug!("[CopilotAuth] 轮询 OAuth Token");

        let response = self
            .http_client
            .post(GITHUB_OAUTH_TOKEN_URL)
            .header("Accept", "application/json")
            .header("User-Agent", COPILOT_USER_AGENT)
            .form(&[
                ("client_id", GITHUB_CLIENT_ID),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;

        let oauth_response: GitHubOAuthResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        // 检查错误
        if let Some(error) = oauth_response.error {
            return match error.as_str() {
                "authorization_pending" => Err(CopilotAuthError::AuthorizationPending),
                "slow_down" => Err(CopilotAuthError::AuthorizationPending),
                "expired_token" => Err(CopilotAuthError::ExpiredToken),
                "access_denied" => Err(CopilotAuthError::AccessDenied),
                _ => Err(CopilotAuthError::NetworkError(format!(
                    "{}: {}",
                    error,
                    oauth_response.error_description.unwrap_or_default()
                ))),
            };
        }

        // 获取 access_token
        let access_token = oauth_response
            .access_token
            .ok_or_else(|| CopilotAuthError::ParseError("缺少 access_token".to_string()))?;

        log::info!("[CopilotAuth] OAuth Token 获取成功");

        // 获取用户信息
        let user = self.fetch_user_info_with_token(&access_token).await?;

        // 验证 Copilot 订阅（获取 Copilot Token）
        self.fetch_copilot_token_with_github_token(&access_token, &user.id.to_string())
            .await?;

        let account_id = user.id.to_string();

        // 检查账号是否已存在，以及是否需要重新生成 Machine ID
        let machine_id = {
            let accounts = self.accounts.read().await;
            if let Some(existing_account) = accounts.get(&account_id) {
                let now = chrono::Utc::now().timestamp();
                let time_since_auth = now - existing_account.authenticated_at;

                // 如果在 1 小时内重新登录，复用现有的 Machine ID
                if time_since_auth < 3600 && existing_account.machine_id.is_some() {
                    log::info!(
                        "[CopilotAuth] 账号 {} 在 1 小时内重新登录，复用现有 Machine ID",
                        account_id
                    );
                    existing_account.machine_id.clone().unwrap()
                } else {
                    // 超过 1 小时，重新生成 Machine ID
                    let new_machine_id = self.generate_machine_id(&account_id);
                    log::info!(
                        "[CopilotAuth] 账号 {} 超过 1 小时后重新登录，生成新 Machine ID: {}",
                        account_id,
                        &new_machine_id[..16]
                    );
                    new_machine_id
                }
            } else {
                // 新账号，生成 Machine ID
                let new_machine_id = self.generate_machine_id(&account_id);
                log::info!(
                    "[CopilotAuth] 为新账号 {} 生成 Machine ID: {}",
                    account_id,
                    &new_machine_id[..16]
                );
                new_machine_id
            }
        };

        // 添加账号
        let account = self
            .add_account_internal(access_token, user, machine_id)
            .await?;

        Ok(Some(account))
    }

    // ==================== Token 获取方法 ====================

    /// 获取指定账号的有效 Copilot Token（自动刷新）
    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CopilotAuthError> {
        // 确保迁移完成
        self.ensure_migration_complete().await?;

        // 检查缓存的 token
        {
            let tokens = self.copilot_tokens.read().await;
            if let Some(copilot_token) = tokens.get(account_id) {
                if !copilot_token.is_expiring_soon() {
                    return Ok(copilot_token.token.clone());
                }
            }
        }

        // 需要刷新
        log::info!("[CopilotAuth] 账号 {account_id} 的 Copilot Token 需要刷新");

        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _refresh_guard = refresh_lock.lock().await;

        // double-check：等待锁期间可能已由其他请求刷新完成
        {
            let tokens = self.copilot_tokens.read().await;
            if let Some(copilot_token) = tokens.get(account_id) {
                if !copilot_token.is_expiring_soon() {
                    return Ok(copilot_token.token.clone());
                }
            }
        }

        // 获取账号的 GitHub token
        let github_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .map(|a| a.github_token.clone())
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?
        };

        // 刷新 Copilot token
        self.fetch_copilot_token_with_github_token(&github_token, account_id)
            .await?;

        // 返回新 token
        let tokens = self.copilot_tokens.read().await;
        tokens.get(account_id).map(|t| t.token.clone()).ok_or(
            CopilotAuthError::CopilotTokenFetchFailed("刷新后仍无令牌".to_string()),
        )
    }

    /// 获取有效的 Copilot Token（向后兼容：使用第一个账号）
    pub async fn get_valid_token(&self) -> Result<String, CopilotAuthError> {
        // 确保迁移完成
        self.ensure_migration_complete().await?;

        match self.resolve_default_account_id().await {
            Some(id) => self.get_valid_token_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    // ==================== 模型和使用量 ====================

    /// 获取指定账号的 Copilot 可用模型列表
    pub async fn fetch_models_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        self.ensure_migration_complete().await?;

        {
            let models = self.copilot_models.read().await;
            if let Some(cached) = models.get(account_id) {
                return Ok(cached.clone());
            }
        }

        let models = self.fetch_models_for_account_uncached(account_id).await?;
        {
            let mut cache = self.copilot_models.write().await;
            cache.insert(account_id.to_string(), models.clone());
        }
        Ok(models)
    }

    async fn fetch_models_for_account_uncached(
        &self,
        account_id: &str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        let copilot_token = self.get_valid_token_for_account(account_id).await?;

        log::info!("[CopilotAuth] 获取账号 {account_id} 的 Copilot 可用模型");

        let response = self
            .http_client
            .get(COPILOT_MODELS_URL)
            .header("Authorization", format!("Bearer {copilot_token}"))
            .header("Content-Type", "application/json")
            .header("copilot-integration-id", "vscode-chat")
            .header("editor-version", COPILOT_EDITOR_VERSION)
            .header("editor-plugin-version", COPILOT_PLUGIN_VERSION)
            .header("user-agent", COPILOT_USER_AGENT)
            .header("x-github-api-version", COPILOT_API_VERSION)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CopilotAuthError::CopilotTokenFetchFailed(format!(
                "获取模型列表失败: {status} - {text}"
            )));
        }

        let models_response: CopilotModelsResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        let models: Vec<CopilotModel> = models_response
            .data
            .into_iter()
            .filter(|m| m.model_picker_enabled)
            .map(|m| CopilotModel {
                id: m.id,
                name: m.name,
                vendor: m.vendor,
                model_picker_enabled: m.model_picker_enabled,
            })
            .collect();

        log::info!("[CopilotAuth] 获取到 {} 个可用模型", models.len());

        Ok(models)
    }

    pub async fn get_model_vendor_for_account(
        &self,
        account_id: &str,
        model_id: &str,
    ) -> Result<Option<String>, CopilotAuthError> {
        let models = self.fetch_models_for_account(account_id).await?;
        Ok(models
            .into_iter()
            .find(|model| model.id == model_id)
            .map(|model| model.vendor))
    }

    /// 获取 Copilot 可用模型列表（向后兼容：使用第一个账号）
    pub async fn fetch_models(&self) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.fetch_models_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    pub async fn get_model_vendor(
        &self,
        model_id: &str,
    ) -> Result<Option<String>, CopilotAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.get_model_vendor_for_account(&id, model_id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    /// 获取指定账号的 Copilot 使用量信息
    pub async fn fetch_usage_for_account(
        &self,
        account_id: &str,
    ) -> Result<CopilotUsageResponse, CopilotAuthError> {
        let github_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .map(|a| a.github_token.clone())
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?
        };

        log::info!("[CopilotAuth] 获取账号 {account_id} 的 Copilot 使用量");

        let response = self
            .http_client
            .get(COPILOT_USAGE_URL)
            .header("Authorization", format!("token {github_token}"))
            .header("Content-Type", "application/json")
            .header("editor-version", COPILOT_EDITOR_VERSION)
            .header("editor-plugin-version", COPILOT_PLUGIN_VERSION)
            .header("user-agent", COPILOT_USER_AGENT)
            .header("x-github-api-version", COPILOT_API_VERSION)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CopilotAuthError::CopilotTokenFetchFailed(format!(
                "获取使用量失败: {status} - {text}"
            )));
        }

        let usage: CopilotUsageResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        // 存储动态 API 端点（如果有）
        if let Some(ref endpoints) = usage.endpoints {
            let mut api_endpoints = self.api_endpoints.write().await;
            api_endpoints.insert(account_id.to_string(), endpoints.api.clone());
            // 使用 debug 级别避免在日志中暴露企业内部域名
            log::debug!("[CopilotAuth] 账号 {account_id} 已保存动态 API 端点");
        }

        log::info!(
            "[CopilotAuth] 获取使用量成功，计划: {}, 重置日期: {}",
            usage.copilot_plan,
            usage.quota_reset_date
        );

        Ok(usage)
    }

    /// 获取 Copilot 使用量信息（向后兼容：使用第一个账号）
    pub async fn fetch_usage(&self) -> Result<CopilotUsageResponse, CopilotAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.fetch_usage_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    // ==================== 状态查询 ====================

    /// 获取指定账号的 API 端点（缓存命中直接返回，未命中则从 API 惰性拉取）
    pub async fn get_api_endpoint(&self, account_id: &str) -> String {
        let _ = self.ensure_migration_complete().await;

        {
            let endpoints = self.api_endpoints.read().await;
            if let Some(endpoint) = endpoints.get(account_id) {
                return endpoint.clone();
            }
        }

        // 用锁串行化同一账号的并发拉取，避免对 GitHub API 的重复请求
        let lock = self.get_endpoint_lock(account_id).await;
        let _guard = lock.lock().await;

        // 持锁后二次检查：可能已由其他请求填充
        {
            let endpoints = self.api_endpoints.read().await;
            if let Some(endpoint) = endpoints.get(account_id) {
                return endpoint.clone();
            }
        }

        match self.fetch_and_cache_endpoint(account_id).await {
            Ok(endpoint) => endpoint,
            Err(e) => {
                log::debug!(
                    "[CopilotAuth] 获取账号 {account_id} 动态 API 端点失败: {e}，使用默认值"
                );
                DEFAULT_COPILOT_API_ENDPOINT.to_string()
            }
        }
    }

    /// 获取默认账号的 API 端点
    pub async fn get_default_api_endpoint(&self) -> String {
        let _ = self.ensure_migration_complete().await;

        match self.resolve_default_account_id().await {
            Some(id) => self.get_api_endpoint(&id).await,
            None => DEFAULT_COPILOT_API_ENDPOINT.to_string(),
        }
    }

    async fn fetch_and_cache_endpoint(&self, account_id: &str) -> Result<String, CopilotAuthError> {
        let github_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .map(|a| a.github_token.clone())
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?
        };

        log::debug!("[CopilotAuth] 为账号 {account_id} 惰性拉取动态 API 端点");

        let response = self
            .http_client
            .get(COPILOT_USAGE_URL)
            .header("Authorization", format!("token {github_token}"))
            .header("Content-Type", "application/json")
            .header("editor-version", COPILOT_EDITOR_VERSION)
            .header("editor-plugin-version", COPILOT_PLUGIN_VERSION)
            .header("user-agent", COPILOT_USER_AGENT)
            .header("x-github-api-version", COPILOT_API_VERSION)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        if !response.status().is_success() {
            return Err(CopilotAuthError::CopilotTokenFetchFailed(format!(
                "获取 API 端点失败: {}",
                response.status()
            )));
        }

        let usage: CopilotUsageResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        let endpoint = match usage.endpoints {
            Some(endpoints) => endpoints.api.clone(),
            None => DEFAULT_COPILOT_API_ENDPOINT.to_string(),
        };

        // 缓存端点（包括默认值），避免重复请求
        let mut api_endpoints = self.api_endpoints.write().await;
        api_endpoints.insert(account_id.to_string(), endpoint.clone());
        log::debug!("[CopilotAuth] 账号 {account_id} 已缓存 API 端点");

        Ok(endpoint)
    }

    async fn get_endpoint_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        {
            let locks = self.endpoint_locks.read().await;
            if let Some(lock) = locks.get(account_id) {
                return Arc::clone(lock);
            }
        }

        let mut locks = self.endpoint_locks.write().await;
        Arc::clone(
            locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
    /// 获取认证状态（支持多账号）
    pub async fn get_status(&self) -> CopilotAuthStatus {
        // 确保迁移完成
        let _ = self.ensure_migration_complete().await;

        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let copilot_tokens = self.copilot_tokens.read().await.clone();
        let migration_error = self.migration_error.read().await.clone();

        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());
        let authenticated = !account_list.is_empty();
        let username = default_account_id
            .as_ref()
            .and_then(|id| accounts.get(id))
            .map(|a| a.user.login.clone())
            .or_else(|| account_list.first().map(|a| a.login.clone()));

        // 获取默认账号的过期时间
        let expires_at = default_account_id
            .as_ref()
            .and_then(|id| copilot_tokens.get(id))
            .map(|t| t.expires_at);

        CopilotAuthStatus {
            accounts: account_list,
            default_account_id,
            migration_error,
            authenticated,
            username,
            expires_at,
        }
    }

    /// 检查是否已认证（有任意账号）
    pub async fn is_authenticated(&self) -> bool {
        let accounts = self.accounts.read().await;
        !accounts.is_empty()
    }

    /// 清除所有认证（登出所有账号）
    pub async fn clear_auth(&self) -> Result<(), CopilotAuthError> {
        log::info!("[CopilotAuth] 清除所有认证");

        // 先清理内存状态，确保即使文件删除失败用户也能看到已登出
        {
            let mut accounts = self.accounts.write().await;
            accounts.clear();
        }
        {
            let mut default_account_id = self.default_account_id.write().await;
            default_account_id.take();
        }
        self.set_migration_error(None).await;
        {
            let mut tokens = self.copilot_tokens.write().await;
            tokens.clear();
        }
        {
            let mut models = self.copilot_models.write().await;
            models.clear();
        }
        {
            let mut refresh_locks = self.refresh_locks.write().await;
            refresh_locks.clear();
        }
        {
            let mut session_refresh_tasks = self.session_refresh_tasks.write().await;
            for (_, task) in session_refresh_tasks.drain() {
                task.abort();
            }
        }
        // 清理 API 端点缓存
        {
            let mut api_endpoints = self.api_endpoints.write().await;
            api_endpoints.clear();
        }
        {
            let mut endpoint_locks = self.endpoint_locks.write().await;
            endpoint_locks.clear();
        }

        Ok(())
    }

    // ==================== 内部方法 ====================

    fn fallback_default_account_id(
        accounts: &HashMap<String, GitHubAccountData>,
    ) -> Option<String> {
        accounts
            .iter()
            .max_by(|(id_a, a), (id_b, b)| {
                a.authenticated_at
                    .cmp(&b.authenticated_at)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, _)| id.clone())
    }

    fn sorted_accounts(
        accounts: &HashMap<String, GitHubAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<GitHubAccount> {
        let mut account_list: Vec<GitHubAccount> =
            accounts.values().map(GitHubAccount::from).collect();
        account_list.sort_by(|a, b| {
            let a_default = default_account_id == Some(a.id.as_str());
            let b_default = default_account_id == Some(b.id.as_str());

            b_default
                .cmp(&a_default)
                .then_with(|| b.authenticated_at.cmp(&a.authenticated_at))
                .then_with(|| a.login.cmp(&b.login))
        });
        account_list
    }

    async fn resolve_default_account_id(&self) -> Option<String> {
        let stored_default = self.default_account_id.read().await.clone();
        let accounts = self.accounts.read().await;

        if let Some(default_id) = stored_default {
            if accounts.contains_key(&default_id) {
                return Some(default_id);
            }
        }

        Self::fallback_default_account_id(&accounts)
    }

    /// 获取账号的 machine ID 和 session ID（公共方法）
    pub async fn get_account_ids(
        &self,
        account_id: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        match account_id {
            Some(id) => {
                let accounts = self.accounts.read().await;
                if let Some(account_data) = accounts.get(id) {
                    (
                        account_data.machine_id.clone(),
                        account_data.session_id.clone(),
                    )
                } else {
                    (None, None)
                }
            }
            None => {
                // 使用默认账号
                let default_id = self.resolve_default_account_id().await;
                if let Some(id) = default_id {
                    let accounts = self.accounts.read().await;
                    if let Some(account_data) = accounts.get(&id) {
                        (
                            account_data.machine_id.clone(),
                            account_data.session_id.clone(),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            }
        }
    }

    async fn get_refresh_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        {
            let refresh_locks = self.refresh_locks.read().await;
            if let Some(lock) = refresh_locks.get(account_id) {
                return Arc::clone(lock);
            }
        }

        let mut refresh_locks = self.refresh_locks.write().await;
        Arc::clone(
            refresh_locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    fn machine_base_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("copilot_machine_base.txt")
    }

    fn fallback_machine_base(&self) -> String {
        let machine_base_path = self.machine_base_path();

        if let Ok(existing) = std::fs::read_to_string(&machine_base_path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }

        let host_hint = std::env::var("HOSTNAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("COMPUTERNAME")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| "unknown-host".to_string());
        let machine_base = format!("{}-{}", host_hint, uuid::Uuid::new_v4());

        if let Some(parent) = machine_base_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&machine_base_path, &machine_base) {
            log::warn!(
                "[CopilotAuth] 无法持久化 machine base {}: {}",
                machine_base_path.display(),
                err
            );
        }

        machine_base
    }

    /// 生成机器 ID（每个账号独立的 Machine ID）
    ///
    /// 为了避免账号间关联，每个账号使用不同的 Machine ID：
    /// - 基于 MAC 地址（或随机 UUID）+ 账号 ID 生成
    /// - 对组合值进行 SHA256 哈希
    fn generate_machine_id(&self, account_id: &str) -> String {
        use sha2::{Digest, Sha256};

        // 尝试获取 MAC 地址作为基础
        let base = match mac_address::get_mac_address() {
            Ok(Some(mac)) => {
                // MAC 地址格式：AA:BB:CC:DD:EE:FF
                mac.to_string()
            }
            _ => {
                // 回退：使用主机唯一且持久化的基础值，避免不同设备碰撞
                log::warn!("[CopilotAuth] 无法获取 MAC 地址，使用持久化 machine base");
                self.fallback_machine_base()
            }
        };

        // 组合基础值和账号 ID，确保每个账号有不同的 Machine ID
        let source = format!("{}-{}", base, account_id);

        // SHA256 哈希
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        let result = hasher.finalize();

        // 转换为十六进制字符串
        format!("{:x}", result)
    }

    /// 生成会话 ID（UUID + 时间戳）
    ///
    /// 参考 caozhiyuan/copilot-api 的实现：
    /// state.vsCodeSessionId = randomUUID() + Date.now().toString()
    fn generate_session_id() -> String {
        let uuid = uuid::Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        format!("{}{}", uuid, timestamp)
    }

    fn should_refresh_session_id(session_refreshed_at: Option<i64>, now: i64) -> bool {
        match session_refreshed_at {
            Some(refreshed_at) => now.saturating_sub(refreshed_at) >= Self::SESSION_REFRESH_BASE_SECS,
            None => true,
        }
    }

    /// 启动 Session ID 定期刷新任务（每小时刷新）
    ///
    /// 参考 caozhiyuan/copilot-api 的实现：
    /// - 基础间隔：1 小时
    /// - 添加 0-10% 随机抖动避免集中刷新
    async fn spawn_session_refresh_task(
        account_id: String,
        accounts: Arc<RwLock<HashMap<String, GitHubAccountData>>>,
        session_refresh_tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    ) {
        use tokio::time::{sleep, Duration};

        const SESSION_REFRESH_BASE_SECS: u64 = 3600; // 1 小时

        // 停止旧任务（如果存在）
        {
            let mut tasks = session_refresh_tasks.write().await;
            if let Some(old_task) = tasks.remove(&account_id) {
                old_task.abort();
            }
        }

        let account_id_clone = account_id.clone();
        let accounts_clone = Arc::clone(&accounts);

        // 启动新任务
        let task = tokio::spawn(async move {
            loop {
                // 计算下次刷新时间（带随机抖动）
                // 使用 SystemTime 生成随机抖动，避免 thread_rng 的 Send 问题
                let jitter_nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos();
                let jitter_percent = (jitter_nanos % 100) as f64 / 1000.0; // 0-10%
                let jitter_secs = (SESSION_REFRESH_BASE_SECS as f64 * jitter_percent) as u64;
                let delay_secs = SESSION_REFRESH_BASE_SECS + jitter_secs;

                sleep(Duration::from_secs(delay_secs)).await;

                // 刷新 Session ID
                let new_session_id = Self::generate_session_id();
                let now = chrono::Utc::now().timestamp();

                let mut accounts_guard = accounts_clone.write().await;
                if let Some(account_data) = accounts_guard.get_mut(&account_id_clone) {
                    account_data.session_id = Some(new_session_id.clone());
                    account_data.session_refreshed_at = Some(now);
                    log::debug!(
                        "[CopilotAuth] 账号 {} 的 Session ID 已刷新",
                        account_id_clone
                    );
                } else {
                    // 账号已删除，退出任务
                    log::debug!(
                        "[CopilotAuth] 账号 {} 已删除，停止 Session ID 刷新任务",
                        account_id_clone
                    );
                    break;
                }
            }
        });

        // 保存任务句柄
        let mut tasks = session_refresh_tasks.write().await;
        tasks.insert(account_id, task);
    }

    async fn set_migration_error(&self, message: Option<String>) {
        let mut migration_error = self.migration_error.write().await;
        *migration_error = message;
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CopilotAuthError> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| CopilotAuthError::IoError("无效的存储路径".to_string()))?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| CopilotAuthError::IoError("无效的存储文件名".to_string()))?
            .to_string_lossy()
            .to_string();
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
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            fs::rename(&tmp_path, &self.storage_path)?;
            fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(windows)]
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            if self.storage_path.exists() {
                let _ = fs::remove_file(&self.storage_path);
            }
            fs::rename(&tmp_path, &self.storage_path)?;
        }

        Ok(())
    }

    /// 使用指定 token 获取 GitHub 用户信息
    async fn fetch_user_info_with_token(
        &self,
        github_token: &str,
    ) -> Result<GitHubUser, CopilotAuthError> {
        let response = self
            .http_client
            .get(GITHUB_USER_URL)
            .header("Authorization", format!("token {github_token}"))
            .header("User-Agent", COPILOT_USER_AGENT)
            .header("Editor-Version", COPILOT_EDITOR_VERSION)
            .header("Editor-Plugin-Version", COPILOT_PLUGIN_VERSION)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        let user: GitHubUser = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        log::info!("[CopilotAuth] 获取用户信息成功: {}", user.login);

        Ok(user)
    }

    /// 使用 GitHub token 获取 Copilot Token
    async fn fetch_copilot_token_with_github_token(
        &self,
        github_token: &str,
        account_id: &str,
    ) -> Result<(), CopilotAuthError> {
        log::debug!("[CopilotAuth] 获取账号 {account_id} 的 Copilot Token");

        let response = self
            .http_client
            .get(COPILOT_TOKEN_URL)
            .header("Authorization", format!("token {github_token}"))
            .header("User-Agent", COPILOT_USER_AGENT)
            .header("Editor-Version", COPILOT_EDITOR_VERSION)
            .header("Editor-Plugin-Version", COPILOT_PLUGIN_VERSION)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(CopilotAuthError::NoCopilotSubscription);
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CopilotAuthError::CopilotTokenFetchFailed(format!(
                "{status}: {text}"
            )));
        }

        let token_response: CopilotTokenResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        log::info!(
            "[CopilotAuth] 账号 {} 的 Copilot Token 获取成功，过期时间: {}",
            account_id,
            token_response.expires_at
        );

        let copilot_token = CopilotToken {
            token: token_response.token,
            expires_at: token_response.expires_at,
        };

        let mut tokens = self.copilot_tokens.write().await;
        tokens.insert(account_id.to_string(), copilot_token);

        Ok(())
    }

    // ==================== 存储和迁移 ====================

    /// 从磁盘加载（仅加载 token，不发起网络请求）
    fn load_from_disk_sync(&self) -> Result<(), CopilotAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.storage_path)?;
        let mut store: CopilotAuthStore = serde_json::from_str(&content)
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        if store.version >= 2 {
            // v2/v3 多账号格式

            // 为没有 machine_id 的账号补齐，并在启动恢复时纠正过期的 session_id
            let mut needs_save = false;
            let now = chrono::Utc::now().timestamp();
            for (account_id, account_data) in store.accounts.iter_mut() {
                if account_data.machine_id.is_none() {
                    account_data.machine_id = Some(self.generate_machine_id(account_id));
                    log::info!(
                        "[CopilotAuth] 为现有账号 {} 生成 Machine ID",
                        account_id
                    );
                    needs_save = true;
                }

                let had_session_id = account_data.session_id.is_some();
                if !had_session_id
                    || Self::should_refresh_session_id(account_data.session_refreshed_at, now)
                {
                    account_data.session_id = Some(Self::generate_session_id());
                    account_data.session_refreshed_at = Some(now);
                    if had_session_id {
                        log::info!(
                            "[CopilotAuth] 为现有账号 {} 刷新过期 Session ID",
                            account_id
                        );
                    } else {
                        log::info!(
                            "[CopilotAuth] 为现有账号 {} 生成 Session ID",
                            account_id
                        );
                    }
                    needs_save = true;
                }
            }

            if let Ok(mut accounts) = self.accounts.try_write() {
                *accounts = store.accounts;
                log::info!("[CopilotAuth] 从磁盘加载 {} 个账号", accounts.len());
            }
            if let Ok(mut default_account_id) = self.default_account_id.try_write() {
                *default_account_id = store.default_account_id;
                if default_account_id.is_none() {
                    if let Ok(accounts) = self.accounts.try_read() {
                        *default_account_id = Self::fallback_default_account_id(&accounts);
                    }
                }
            }

            // 如果有修改，直接同步保存回磁盘，避免在无异步运行时的启动阶段触发 panic
            if needs_save {
                let store = CopilotAuthStore {
                    version: 3,
                    accounts: self.accounts.blocking_read().clone(),
                    default_account_id: self.default_account_id.blocking_read().clone(),
                    github_token: None,
                    authenticated_at: None,
                };
                if let Ok(content) = serde_json::to_string_pretty(&store) {
                    let _ = std::fs::write(&self.storage_path, content);
                }
            }
        } else if store.github_token.is_some() {
            // v1 单账号格式，标记待迁移
            log::info!("[CopilotAuth] 检测到旧格式，将在首次访问时迁移");
            if let Ok(mut pending) = self.pending_migration.try_write() {
                *pending = store.github_token;
            }
        }

        Ok(())
    }

    /// 确保迁移完成
    async fn ensure_migration_complete(&self) -> Result<(), CopilotAuthError> {
        let pending = {
            let guard = self.pending_migration.read().await;
            guard.clone()
        };

        if let Some(legacy_token) = pending {
            log::info!("[CopilotAuth] 执行旧格式迁移");

            // 获取用户信息
            match self.fetch_user_info_with_token(&legacy_token).await {
                Ok(user) => {
                    let account_id = user.id.to_string();

                    // 尝试获取 Copilot token 验证订阅
                    if let Err(e) = self
                        .fetch_copilot_token_with_github_token(&legacy_token, &account_id)
                        .await
                    {
                        log::warn!("[CopilotAuth] 迁移时验证 Copilot 订阅失败: {e}");
                    }

                    // 生成 Machine ID（迁移时也需要，基于账号 ID）
                    let account_id = user.id.to_string();
                    let machine_id = self.generate_machine_id(&account_id);
                    log::info!(
                        "[CopilotAuth] 为迁移账号 {} 生成 Machine ID: {}",
                        account_id,
                        &machine_id[..16]
                    );

                    // 添加账号
                    self.add_account_internal(legacy_token, user, machine_id)
                        .await?;
                    self.set_migration_error(None).await;

                    log::info!("[CopilotAuth] 旧格式迁移完成");
                }
                Err(e) => {
                    self.set_migration_error(Some(format!(
                        "Legacy Copilot auth migration failed: {e}"
                    )))
                    .await;
                    log::warn!("[CopilotAuth] 迁移失败，旧 token 可能已失效: {e}");
                }
            }

            // 清除待迁移标记
            {
                let mut pending = self.pending_migration.write().await;
                *pending = None;
            }
        }

        Ok(())
    }

    /// 保存到磁盘
    async fn save_to_disk(&self) -> Result<(), CopilotAuthError> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;

        let store = CopilotAuthStore {
            version: 3,
            accounts,
            default_account_id,
            github_token: None,
            authenticated_at: None,
        };

        let content = serde_json::to_string_pretty(&store)
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        self.write_store_atomic(&content)?;

        log::info!(
            "[CopilotAuth] 保存到磁盘成功（{} 个账号）",
            store.accounts.len()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_account_data(
        token: &str,
        login: &str,
        id: u64,
        avatar_url: Option<&str>,
        authenticated_at: i64,
    ) -> GitHubAccountData {
        GitHubAccountData {
            github_token: token.to_string(),
            user: GitHubUser {
                login: login.to_string(),
                id,
                avatar_url: avatar_url.map(str::to_string),
            },
            authenticated_at,
            machine_id: Some(format!("machine-{id}")),
            session_id: Some(format!("session-{id}")),
            session_refreshed_at: Some(authenticated_at),
        }
    }

    #[test]
    fn test_copilot_token_expiry() {
        let now = chrono::Utc::now().timestamp();

        // 未过期的 token (1小时后过期，不在60秒缓冲期内)
        let token = CopilotToken {
            token: "test".to_string(),
            expires_at: now + 3600,
        };
        assert!(!token.is_expiring_soon());

        // 即将过期的 token (30秒后过期，在60秒缓冲期内)
        let token = CopilotToken {
            token: "test".to_string(),
            expires_at: now + 30,
        };
        assert!(token.is_expiring_soon());

        // 已过期的 token (也在缓冲期内)
        let token = CopilotToken {
            token: "test".to_string(),
            expires_at: now - 100,
        };
        assert!(token.is_expiring_soon());
    }

    #[test]
    fn test_auth_status_serialization() {
        let status = CopilotAuthStatus {
            accounts: vec![GitHubAccount {
                id: "12345".to_string(),
                login: "testuser".to_string(),
                avatar_url: Some("https://example.com/avatar.png".to_string()),
                authenticated_at: 1234567890,
            }],
            default_account_id: Some("12345".to_string()),
            migration_error: None,
            authenticated: true,
            username: Some("testuser".to_string()),
            expires_at: Some(1234567890),
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: CopilotAuthStatus = serde_json::from_str(&json).unwrap();

        assert!(parsed.authenticated);
        assert_eq!(parsed.default_account_id, Some("12345".to_string()));
        assert_eq!(parsed.username, Some("testuser".to_string()));
        assert_eq!(parsed.expires_at, Some(1234567890));
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].id, "12345");
        assert_eq!(parsed.accounts[0].login, "testuser");
    }

    #[test]
    fn test_multi_account_store_serialization() {
        let mut accounts = HashMap::new();
        accounts.insert(
            "12345".to_string(),
            test_account_data(
                "gho_test_token",
                "alice",
                12345,
                Some("https://example.com/alice.png"),
                1700000000,
            ),
        );
        accounts.insert(
            "67890".to_string(),
            test_account_data("gho_test_token_2", "bob", 67890, None, 1700000001),
        );

        let store = CopilotAuthStore {
            version: 3,
            accounts,
            default_account_id: Some("67890".to_string()),
            github_token: None,
            authenticated_at: None,
        };

        let json = serde_json::to_string_pretty(&store).unwrap();
        let parsed: CopilotAuthStore = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 3);
        assert_eq!(parsed.default_account_id, Some("67890".to_string()));
        assert_eq!(parsed.accounts.len(), 2);
        assert!(parsed.accounts.contains_key("12345"));
        assert!(parsed.accounts.contains_key("67890"));
        assert_eq!(parsed.accounts["12345"].user.login, "alice");
        assert_eq!(parsed.accounts["67890"].user.login, "bob");
    }

    #[test]
    fn test_legacy_format_detection() {
        // 旧格式（v1）
        let legacy_json = r#"{
            "github_token": "gho_legacy_token",
            "authenticated_at": 1700000000
        }"#;

        let store: CopilotAuthStore = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(store.version, 0); // 默认值
        assert!(store.github_token.is_some());
        assert!(store.accounts.is_empty());
    }

    #[test]
    fn test_github_account_from_data() {
        let data = test_account_data(
            "gho_test",
            "testuser",
            99999,
            Some("https://example.com/avatar.png"),
            1700000000,
        );

        let account = GitHubAccount::from(&data);
        assert_eq!(account.id, "99999");
        assert_eq!(account.login, "testuser");
        assert_eq!(
            account.avatar_url,
            Some("https://example.com/avatar.png".to_string())
        );
        assert_eq!(account.authenticated_at, 1700000000);
    }

    #[test]
    fn test_fallback_default_account_prefers_latest_authenticated() {
        let mut accounts = HashMap::new();
        accounts.insert(
            "12345".to_string(),
            test_account_data("gho_test_token", "alice", 12345, None, 1700000000),
        );
        accounts.insert(
            "67890".to_string(),
            test_account_data("gho_test_token_2", "bob", 67890, None, 1700000001),
        );

        assert_eq!(
            CopilotAuthManager::fallback_default_account_id(&accounts),
            Some("67890".to_string())
        );
    }

    #[test]
    fn test_load_from_disk_refreshes_stale_session_id() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("copilot_auth.json");
        let stale_refreshed_at =
            chrono::Utc::now().timestamp() - CopilotAuthManager::SESSION_REFRESH_BASE_SECS - 10;
        let stale_session_id = "stale-session".to_string();

        let mut accounts = HashMap::new();
        accounts.insert(
            "12345".to_string(),
            GitHubAccountData {
                session_id: Some(stale_session_id.clone()),
                session_refreshed_at: Some(stale_refreshed_at),
                ..test_account_data("gho_test", "alice", 12345, None, 1700000000)
            },
        );

        let store = CopilotAuthStore {
            version: 3,
            accounts,
            default_account_id: Some("12345".to_string()),
            github_token: None,
            authenticated_at: None,
        };
        std::fs::write(&storage_path, serde_json::to_string_pretty(&store).unwrap()).unwrap();

        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let accounts = manager.accounts.blocking_read();
        let restored = accounts.get("12345").unwrap();
        let refreshed_session_id = restored.session_id.as_ref().unwrap();
        let refreshed_at = restored.session_refreshed_at.unwrap();

        assert_ne!(refreshed_session_id, &stale_session_id);
        assert!(refreshed_at > stale_refreshed_at);
    }

    #[tokio::test]
    async fn test_get_model_vendor_from_cache() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        {
            let mut default_account_id = manager.default_account_id.write().await;
            *default_account_id = Some("12345".to_string());
        }
        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "12345".to_string(),
                test_account_data("gho_test", "alice", 12345, None, 1700000000),
            );
        }
        {
            let mut models = manager.copilot_models.write().await;
            models.insert(
                "12345".to_string(),
                vec![
                    CopilotModel {
                        id: "gpt-5.4".to_string(),
                        name: "GPT-5.4".to_string(),
                        vendor: "OpenAI".to_string(),
                        model_picker_enabled: true,
                    },
                    CopilotModel {
                        id: "claude-sonnet-4".to_string(),
                        name: "Claude Sonnet 4".to_string(),
                        vendor: "Anthropic".to_string(),
                        model_picker_enabled: true,
                    },
                ],
            );
        }

        let vendor = manager
            .get_model_vendor_for_account("12345", "gpt-5.4")
            .await
            .unwrap();
        assert_eq!(vendor.as_deref(), Some("OpenAI"));

        let default_vendor = manager.get_model_vendor("claude-sonnet-4").await.unwrap();
        assert_eq!(default_vendor.as_deref(), Some("Anthropic"));
    }

    #[tokio::test]
    async fn test_get_api_endpoint_returns_cached_value() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // 手动设置 api_endpoints 缓存
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert(
                "12345".to_string(),
                "https://copilot-api.enterprise.example.com".to_string(),
            );
        }

        let endpoint = manager.get_api_endpoint("12345").await;
        assert_eq!(endpoint, "https://copilot-api.enterprise.example.com");
    }

    #[tokio::test]
    async fn test_get_api_endpoint_returns_default_when_not_cached() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let endpoint = manager.get_api_endpoint("99999").await;
        assert_eq!(endpoint, "https://api.githubcopilot.com");
    }

    #[tokio::test]
    async fn test_get_default_api_endpoint_uses_default_account() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // 设置默认账号
        {
            let mut default_account_id = manager.default_account_id.write().await;
            *default_account_id = Some("12345".to_string());
        }
        // 添加账号数据
        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "12345".to_string(),
                test_account_data("gho_test", "alice", 12345, None, 1700000000),
            );
        }
        // 设置 API endpoint 缓存
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert(
                "12345".to_string(),
                "https://copilot-api.corp.example.com".to_string(),
            );
        }

        let endpoint = manager.get_default_api_endpoint().await;
        assert_eq!(endpoint, "https://copilot-api.corp.example.com");
    }

    #[tokio::test]
    async fn test_remove_account_clears_api_endpoint_cache() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // 添加账号数据
        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "12345".to_string(),
                test_account_data("gho_test", "alice", 12345, None, 1700000000),
            );
        }
        // 设置 API endpoint 缓存
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert(
                "12345".to_string(),
                "https://copilot-api.enterprise.example.com".to_string(),
            );
        }

        // 确认缓存存在
        {
            let api_endpoints = manager.api_endpoints.read().await;
            assert!(api_endpoints.contains_key("12345"));
        }

        // 移除账号
        manager.remove_account("12345").await.unwrap();

        // 确认缓存已清理
        {
            let api_endpoints = manager.api_endpoints.read().await;
            assert!(!api_endpoints.contains_key("12345"));
        }
    }

    #[tokio::test]
    async fn test_clear_auth_clears_all_api_endpoint_cache() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // 添加多个账号的 API endpoint 缓存
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert(
                "12345".to_string(),
                "https://copilot-api.enterprise1.example.com".to_string(),
            );
            api_endpoints.insert(
                "67890".to_string(),
                "https://copilot-api.enterprise2.example.com".to_string(),
            );
        }

        // 确认缓存存在
        {
            let api_endpoints = manager.api_endpoints.read().await;
            assert_eq!(api_endpoints.len(), 2);
        }

        // 清除所有认证
        manager.clear_auth().await.unwrap();

        // 确认缓存已清空
        {
            let api_endpoints = manager.api_endpoints.read().await;
            assert!(api_endpoints.is_empty());
        }
    }
    #[tokio::test]
    async fn test_clear_auth_aborts_session_refresh_tasks() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let task = tokio::spawn(async {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        });
        let abort_handle = task.abort_handle();

        {
            let mut session_refresh_tasks = manager.session_refresh_tasks.write().await;
            session_refresh_tasks.insert("12345".to_string(), task);
        }

        manager.clear_auth().await.unwrap();
        tokio::task::yield_now().await;

        {
            let session_refresh_tasks = manager.session_refresh_tasks.read().await;
            assert!(session_refresh_tasks.is_empty());
        }
        assert!(abort_handle.is_finished());
    }

    #[tokio::test]
    async fn test_clear_auth_cleans_memory_even_when_file_removal_fails() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // Create a directory at storage_path so remove_file fails
        std::fs::create_dir_all(&manager.storage_path).unwrap();

        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "12345".to_string(),
                test_account_data("gho_test", "alice", 12345, None, 1700000000),
            );
        }
        {
            let mut default_account_id = manager.default_account_id.write().await;
            *default_account_id = Some("12345".to_string());
        }
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert(
                "12345".to_string(),
                "https://copilot-api.enterprise.example.com".to_string(),
            );
        }

        let result = manager.clear_auth().await;
        assert!(result.is_ok());

        // But memory state should already be cleaned
        let accounts = manager.accounts.read().await;
        assert!(accounts.is_empty());
        drop(accounts);

        let default_account_id = manager.default_account_id.read().await;
        assert!(default_account_id.is_none());
        drop(default_account_id);

        let api_endpoints = manager.api_endpoints.read().await;
        assert!(api_endpoints.is_empty());
    }

    #[tokio::test]
    async fn test_get_api_endpoint_cache_hit_skips_fetch() {
        // 缓存命中时应直接返回，不发起网络请求
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let enterprise_endpoint = "https://copilot-api.enterprise.example.com".to_string();
        {
            let mut api_endpoints = manager.api_endpoints.write().await;
            api_endpoints.insert("12345".to_string(), enterprise_endpoint.clone());
        }

        // 即使没有账号数据，缓存命中也应直接返回
        let endpoint = manager.get_api_endpoint("12345").await;
        assert_eq!(endpoint, enterprise_endpoint);
    }

    #[tokio::test]
    async fn test_get_api_endpoint_returns_default_for_unknown_account() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let endpoint = manager.get_api_endpoint("12345").await;
        assert_eq!(endpoint, DEFAULT_COPILOT_API_ENDPOINT);
    }

    #[tokio::test]
    async fn test_fetch_and_cache_endpoint_requires_account() {
        // 账号不存在时 fetch_and_cache_endpoint 应返回 AccountNotFound 错误
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        let result = manager.fetch_and_cache_endpoint("nonexistent").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CopilotAuthError::AccountNotFound(id) => assert_eq!(id, "nonexistent"),
            other => panic!("期望 AccountNotFound 错误，实际: {other:?}"),
        }
    }
}
