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
//! ## 多账号支持 (v2)
//! - 每个 GitHub 账号独立存储 token
//! - Provider 通过 meta.github_account_id 关联账号
//! - 自动迁移 v1 单账号格式到 v2 多账号格式

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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
const COPILOT_EDITOR_VERSION: &str = "vscode/1.96.0";
const COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const COPILOT_API_VERSION: &str = "2025-04-01";

/// Copilot 使用量 API URL
const COPILOT_USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

/// Copilot 使用量响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotUsageResponse {
    /// Copilot 计划类型
    pub copilot_plan: String,
    /// 配额重置日期
    pub quota_reset_date: String,
    /// 配额快照
    pub quota_snapshots: QuotaSnapshots,
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
    pub github_token: String,
    /// 用户信息
    pub user: GitHubUser,
    /// 认证时间戳
    pub authenticated_at: i64,
}

/// 持久化存储结构（v2 多账号格式）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CopilotAuthStore {
    /// 存储格式版本（2 = 多账号格式）
    #[serde(default)]
    version: u32,
    /// 多账号数据（key = GitHub user ID）
    #[serde(default)]
    accounts: HashMap<String, GitHubAccountData>,
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
    /// Copilot Token 缓存（key = GitHub user ID，内存缓存，自动刷新）
    copilot_tokens: Arc<RwLock<HashMap<String, CopilotToken>>>,
    /// HTTP 客户端
    http_client: Client,
    /// 存储路径
    storage_path: PathBuf,
    /// 待迁移的旧格式 token
    pending_migration: Arc<RwLock<Option<String>>>,
}

impl CopilotAuthManager {
    /// 创建新的认证管理器
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("copilot_auth.json");

        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            copilot_tokens: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path,
            pending_migration: Arc::new(RwLock::new(None)),
        };

        // 尝试从磁盘加载（同步，不发起网络请求）
        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[CopilotAuth] 加载存储失败: {}", e);
        }

        manager
    }

    // ==================== 多账号管理方法 ====================

    /// 列出所有已认证的账号
    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let accounts = self.accounts.read().await;
        accounts.values().map(GitHubAccount::from).collect()
    }

    /// 获取指定账号信息
    pub async fn get_account(&self, account_id: &str) -> Option<GitHubAccount> {
        let accounts = self.accounts.read().await;
        accounts.get(account_id).map(GitHubAccount::from)
    }

    /// 移除指定账号
    pub async fn remove_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        log::info!("[CopilotAuth] 移除账号: {}", account_id);

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

        // 持久化
        self.save_to_disk().await?;

        Ok(())
    }

    /// 添加新账号（内部方法，在 OAuth 完成后调用）
    async fn add_account_internal(
        &self,
        github_token: String,
        user: GitHubUser,
    ) -> Result<GitHubAccount, CopilotAuthError> {
        let account_id = user.id.to_string();
        let now = chrono::Utc::now().timestamp();

        let account_data = GitHubAccountData {
            github_token,
            user: user.clone(),
            authenticated_at: now,
        };

        let account = GitHubAccount {
            id: account_id.clone(),
            login: user.login.clone(),
            avatar_url: user.avatar_url.clone(),
            authenticated_at: now,
        };

        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(account_id, account_data);
        }

        // 持久化
        self.save_to_disk().await?;

        log::info!("[CopilotAuth] 添加账号成功: {}", user.login);

        Ok(account)
    }

    // ==================== 设备码流程 ====================

    /// 启动设备码流程
    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, CopilotAuthError> {
        log::info!("[CopilotAuth] 启动设备码流程");

        let response = self
            .http_client
            .post(GITHUB_DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .form(&[("client_id", GITHUB_CLIENT_ID), ("scope", "read:user")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CopilotAuthError::NetworkError(format!(
                "GitHub 设备码请求失败: {} - {}",
                status, text
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

        // 添加账号
        let account = self.add_account_internal(access_token, user).await?;

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
        log::info!(
            "[CopilotAuth] 账号 {} 的 Copilot Token 需要刷新",
            account_id
        );

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
        tokens
            .get(account_id)
            .map(|t| t.token.clone())
            .ok_or(CopilotAuthError::CopilotTokenFetchFailed(
                "刷新后仍无令牌".to_string(),
            ))
    }

    /// 获取有效的 Copilot Token（向后兼容：使用第一个账号）
    pub async fn get_valid_token(&self) -> Result<String, CopilotAuthError> {
        // 确保迁移完成
        self.ensure_migration_complete().await?;

        let first_account_id = {
            let accounts = self.accounts.read().await;
            accounts.keys().next().cloned()
        };

        match first_account_id {
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
        let copilot_token = self.get_valid_token_for_account(account_id).await?;

        log::info!("[CopilotAuth] 获取账号 {} 的 Copilot 可用模型", account_id);

        let response = self
            .http_client
            .get(COPILOT_MODELS_URL)
            .header("Authorization", format!("Bearer {}", copilot_token))
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
                "获取模型列表失败: {} - {}",
                status, text
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

    /// 获取 Copilot 可用模型列表（向后兼容：使用第一个账号）
    pub async fn fetch_models(&self) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        let first_account_id = {
            let accounts = self.accounts.read().await;
            accounts.keys().next().cloned()
        };

        match first_account_id {
            Some(id) => self.fetch_models_for_account(&id).await,
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

        log::info!("[CopilotAuth] 获取账号 {} 的 Copilot 使用量", account_id);

        let response = self
            .http_client
            .get(COPILOT_USAGE_URL)
            .header("Authorization", format!("token {}", github_token))
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
                "获取使用量失败: {} - {}",
                status, text
            )));
        }

        let usage: CopilotUsageResponse = response
            .json()
            .await
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        log::info!(
            "[CopilotAuth] 获取使用量成功，计划: {}, 重置日期: {}",
            usage.copilot_plan,
            usage.quota_reset_date
        );

        Ok(usage)
    }

    /// 获取 Copilot 使用量信息（向后兼容：使用第一个账号）
    pub async fn fetch_usage(&self) -> Result<CopilotUsageResponse, CopilotAuthError> {
        let first_account_id = {
            let accounts = self.accounts.read().await;
            accounts.keys().next().cloned()
        };

        match first_account_id {
            Some(id) => self.fetch_usage_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    // ==================== 状态查询 ====================

    /// 获取认证状态（支持多账号）
    pub async fn get_status(&self) -> CopilotAuthStatus {
        // 确保迁移完成
        let _ = self.ensure_migration_complete().await;

        let accounts = self.accounts.read().await;
        let copilot_tokens = self.copilot_tokens.read().await;

        let account_list: Vec<GitHubAccount> = accounts.values().map(GitHubAccount::from).collect();
        let authenticated = !account_list.is_empty();
        let username = account_list.first().map(|a| a.login.clone());

        // 获取第一个账号的过期时间
        let expires_at = accounts
            .keys()
            .next()
            .and_then(|id| copilot_tokens.get(id))
            .map(|t| t.expires_at);

        CopilotAuthStatus {
            accounts: account_list,
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

        {
            let mut accounts = self.accounts.write().await;
            accounts.clear();
        }
        {
            let mut tokens = self.copilot_tokens.write().await;
            tokens.clear();
        }

        // 删除存储文件
        if self.storage_path.exists() {
            std::fs::remove_file(&self.storage_path)?;
        }

        Ok(())
    }

    // ==================== 内部方法 ====================

    /// 使用指定 token 获取 GitHub 用户信息
    async fn fetch_user_info_with_token(
        &self,
        github_token: &str,
    ) -> Result<GitHubUser, CopilotAuthError> {
        let response = self
            .http_client
            .get(GITHUB_USER_URL)
            .header("Authorization", format!("token {}", github_token))
            .header("User-Agent", "CC-Switch")
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
        log::debug!("[CopilotAuth] 获取账号 {} 的 Copilot Token", account_id);

        let response = self
            .http_client
            .get(COPILOT_TOKEN_URL)
            .header("Authorization", format!("token {}", github_token))
            .header("User-Agent", "CC-Switch")
            .header("Editor-Version", "vscode/1.85.0")
            .header("Editor-Plugin-Version", "copilot/1.150.0")
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
                "{}: {}",
                status, text
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
        let store: CopilotAuthStore =
            serde_json::from_str(&content).map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        if store.version >= 2 {
            // v2 多账号格式
            if let Ok(mut accounts) = self.accounts.try_write() {
                *accounts = store.accounts;
                log::info!(
                    "[CopilotAuth] 从磁盘加载 {} 个账号",
                    accounts.len()
                );
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
                        log::warn!("[CopilotAuth] 迁移时验证 Copilot 订阅失败: {}", e);
                    }

                    // 添加账号
                    self.add_account_internal(legacy_token, user).await?;

                    log::info!("[CopilotAuth] 旧格式迁移完成");
                }
                Err(e) => {
                    log::warn!("[CopilotAuth] 迁移失败，旧 token 可能已失效: {}", e);
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
        let accounts = self.accounts.read().await;

        let store = CopilotAuthStore {
            version: 2,
            accounts: accounts.clone(),
            github_token: None,
            authenticated_at: None,
        };

        // 确保目录存在
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&store)
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        std::fs::write(&self.storage_path, content)?;

        log::info!("[CopilotAuth] 保存到磁盘成功（{} 个账号）", accounts.len());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            authenticated: true,
            username: Some("testuser".to_string()),
            expires_at: Some(1234567890),
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: CopilotAuthStatus = serde_json::from_str(&json).unwrap();

        assert!(parsed.authenticated);
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
            GitHubAccountData {
                github_token: "gho_test_token".to_string(),
                user: GitHubUser {
                    login: "alice".to_string(),
                    id: 12345,
                    avatar_url: Some("https://example.com/alice.png".to_string()),
                },
                authenticated_at: 1700000000,
            },
        );
        accounts.insert(
            "67890".to_string(),
            GitHubAccountData {
                github_token: "gho_test_token_2".to_string(),
                user: GitHubUser {
                    login: "bob".to_string(),
                    id: 67890,
                    avatar_url: None,
                },
                authenticated_at: 1700000001,
            },
        );

        let store = CopilotAuthStore {
            version: 2,
            accounts,
            github_token: None,
            authenticated_at: None,
        };

        let json = serde_json::to_string_pretty(&store).unwrap();
        let parsed: CopilotAuthStore = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 2);
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
        let data = GitHubAccountData {
            github_token: "gho_test".to_string(),
            user: GitHubUser {
                login: "testuser".to_string(),
                id: 99999,
                avatar_url: Some("https://example.com/avatar.png".to_string()),
            },
            authenticated_at: 1700000000,
        };

        let account = GitHubAccount::from(&data);
        assert_eq!(account.id, "99999");
        assert_eq!(account.login, "testuser");
        assert_eq!(
            account.avatar_url,
            Some("https://example.com/avatar.png".to_string())
        );
        assert_eq!(account.authenticated_at, 1700000000);
    }
}
