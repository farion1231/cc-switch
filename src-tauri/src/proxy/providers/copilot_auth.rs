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

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::proxy::error::ProxyError;

/// GitHub OAuth 客户端 ID（VS Code）- 用于 github.com
const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

/// GitHub OAuth 客户端 ID（与 OpenCode 相同）- 在所有 GHES Copilot 实例上预注册
const GITHUB_CLIENT_ID_GHES: &str = "Ov23li8tweQw6odWQebz";

/// 默认 GitHub 域名
const DEFAULT_GITHUB_DOMAIN: &str = "github.com";

/// 根据域名选择 OAuth 客户端 ID
fn github_client_id(domain: &str) -> &'static str {
    if domain == DEFAULT_GITHUB_DOMAIN {
        GITHUB_CLIENT_ID
    } else {
        GITHUB_CLIENT_ID_GHES
    }
}

fn default_github_domain() -> String {
    DEFAULT_GITHUB_DOMAIN.to_string()
}

/// GitHub 设备码 URL
fn github_device_code_url(domain: &str) -> String {
    format!("https://{domain}/login/device/code")
}

/// GitHub OAuth Token URL
fn github_oauth_token_url(domain: &str) -> String {
    format!("https://{domain}/login/oauth/access_token")
}

/// GitHub API 基础 URL（github.com 用 api.github.com，GHES 用 {domain}/api/v3）
fn github_api_base(domain: &str) -> String {
    if domain == DEFAULT_GITHUB_DOMAIN {
        "https://api.github.com".to_string()
    } else {
        format!("https://{domain}/api/v3")
    }
}

/// Copilot Token URL
fn copilot_token_url(domain: &str) -> String {
    format!("{}/copilot_internal/v2/token", github_api_base(domain))
}

/// GitHub User API URL
fn github_user_url(domain: &str) -> String {
    format!("{}/user", github_api_base(domain))
}

/// Copilot 使用量 API URL
fn copilot_usage_url(domain: &str) -> String {
    format!("{}/copilot_internal/user", github_api_base(domain))
}

/// Copilot API 基础地址（github.com 用 api.githubcopilot.com，GHES 用 copilot-api.{domain}）
fn copilot_api_base(domain: &str) -> String {
    if domain == DEFAULT_GITHUB_DOMAIN {
        "https://api.githubcopilot.com".to_string()
    } else {
        format!("https://copilot-api.{domain}")
    }
}

/// Token 刷新提前量（秒）
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;

/// GitHub / Copilot 认证元数据请求超时（避免复用代理流式请求的 600s 超时长期阻塞迁移锁）
const GITHUB_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// 判断是否为 GitHub Enterprise Server（非 github.com）
fn is_ghes(domain: &str) -> bool {
    domain != DEFAULT_GITHUB_DOMAIN
}

/// 归一化 GitHub 域名（SSOT）：
/// - 小写化
/// - 剥离协议（https:// http://）
/// - 剥离尾斜杠、path、query、fragment
/// - 拒绝包含 userinfo（@）的输入
/// - 保留端口号（如有）
fn normalize_github_domain(raw: &str) -> Result<String, CopilotAuthError> {
    let s = raw.trim();
    // 剥离协议
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    // 取 host 部分（到第一个 / 或 ? 或 #）
    let host = s.split(&['/', '?', '#'][..]).next().unwrap_or(s);
    // 拒绝 userinfo
    if host.contains('@') {
        return Err(CopilotAuthError::InvalidDomain(raw.to_string()));
    }
    let normalized = host.to_lowercase();
    if normalized.is_empty() {
        return Err(CopilotAuthError::InvalidDomain(raw.to_string()));
    }
    Ok(normalized)
}

/// 生成复合账号 ID，确保不同 GHES 实例的 user ID 不会冲突。
/// github.com 账号保持原格式（向后兼容），GHES 账号使用 `domain:user_id` 格式。
fn composite_account_id(domain: &str, user_id: u64) -> String {
    if domain == DEFAULT_GITHUB_DOMAIN {
        user_id.to_string()
    } else {
        format!("{}:{}", domain, user_id)
    }
}

/// Copilot API Header 常量
pub const COPILOT_EDITOR_VERSION: &str = "vscode/1.110.1";
pub const COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.38.2";
pub const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.38.2";
pub const COPILOT_API_VERSION: &str = "2025-10-01";
pub const COPILOT_INTEGRATION_ID: &str = "vscode-chat";

/// Build the Copilot request identity shared by every proxy adapter.
///
/// The forwarder adds the session-derived interaction ID separately because
/// it is not available at the provider-adapter seam.
pub fn build_copilot_request_headers(
    token: &str,
) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
    use super::adapter::auth_header_value;

    let mut bearer = String::from("Bearer ");
    bearer.push_str(token);
    let request_id = uuid::Uuid::new_v4().to_string();
    Ok(vec![
        (
            http::HeaderName::from_static("authorization"),
            auth_header_value(&bearer)?,
        ),
        (
            http::HeaderName::from_static("editor-version"),
            http::HeaderValue::from_static(COPILOT_EDITOR_VERSION),
        ),
        (
            http::HeaderName::from_static("editor-plugin-version"),
            http::HeaderValue::from_static(COPILOT_PLUGIN_VERSION),
        ),
        (
            http::HeaderName::from_static("copilot-integration-id"),
            http::HeaderValue::from_static(COPILOT_INTEGRATION_ID),
        ),
        (
            http::HeaderName::from_static("user-agent"),
            http::HeaderValue::from_static(COPILOT_USER_AGENT),
        ),
        (
            http::HeaderName::from_static("x-github-api-version"),
            http::HeaderValue::from_static(COPILOT_API_VERSION),
        ),
        (
            http::HeaderName::from_static("openai-intent"),
            http::HeaderValue::from_static("conversation-agent"),
        ),
        (
            http::HeaderName::from_static("x-initiator"),
            http::HeaderValue::from_static("user"),
        ),
        (
            http::HeaderName::from_static("x-interaction-type"),
            http::HeaderValue::from_static("conversation-agent"),
        ),
        (
            http::HeaderName::from_static("x-vscode-user-agent-library-version"),
            http::HeaderValue::from_static("electron-fetch"),
        ),
        (
            http::HeaderName::from_static("x-request-id"),
            auth_header_value(&request_id)?,
        ),
        (
            http::HeaderName::from_static("x-agent-task-id"),
            auth_header_value(&request_id)?,
        ),
    ])
}

/// Copilot 使用量响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotUsageResponse {
    /// Copilot 计划类型（Free 也报 `individual`，认套餐要先看 `access_type_sku`）
    pub copilot_plan: String,
    /// 订阅 SKU：`free_limited_copilot`（Free）、`max_monthly_subscriber_quota`（Max）…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_type_sku: Option<String>,
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
    /// 当前计划是否含这一项：Free 的 premium_interactions 是 false（总额 0，不是用完了）；
    /// 旧响应没有这个字段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_quota: Option<bool>,
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
    #[serde(default)]
    pub model_picker_enabled: bool,
    /// Copilot-reported maximum prompt limit for this exact model ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Upstream protocols supported by this exact model ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_endpoints: Vec<String>,
    /// Whether the model supports parallel tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tool_calls: Option<bool>,
    /// Copilot-reported image-input support; absence means unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    /// Copilot-reported reasoning effort levels supported by this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<Vec<CopilotReasoningEffort>>,
    /// Billing metadata reported by the account model catalogue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing: Option<CopilotModelBilling>,
}

/// User-selectable models plus account-wide Auto capability. Hidden models are
/// deliberately omitted from `models`, while they may still establish Auto.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotModelOptions {
    pub models: Vec<CopilotModel>,
    pub auto_routing_supported: bool,
}

impl CopilotModel {
    pub fn supports_auto_routing(&self) -> bool {
        self.billing
            .as_ref()
            .and_then(|billing| billing.auto_discount)
            .is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CopilotModelBilling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_discount: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CopilotReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

/// Copilot Models API 响应
#[derive(Debug, Deserialize)]
struct CopilotModelsResponse {
    data: Vec<CopilotModelsResponseItem>,
}

/// Copilot Models API 响应项
#[derive(Debug, Clone, Deserialize)]
struct CopilotModelsResponseItem {
    id: String,
    name: String,
    vendor: String,
    #[serde(default)]
    model_picker_enabled: bool,
    #[serde(default)]
    supported_endpoints: Vec<String>,
    #[serde(default)]
    capabilities: Option<Value>,
    #[serde(default)]
    billing: Option<CopilotModelBilling>,
    #[serde(default)]
    policy: Option<Value>,
}

impl CopilotModelsResponseItem {
    fn is_usable(&self) -> bool {
        !self
            .policy
            .as_ref()
            .and_then(|policy| policy.get("state"))
            .and_then(Value::as_str)
            .is_some_and(|state| state.eq_ignore_ascii_case("disabled"))
    }
}

fn positive_capability_limit(capabilities: &Value, pointer: &str) -> Option<u64> {
    capabilities
        .pointer(pointer)?
        .as_u64()
        .filter(|tokens| *tokens > 0)
}

fn extract_copilot_prompt_limit(capabilities: Option<&Value>) -> Option<u64> {
    let capabilities = capabilities?;
    positive_capability_limit(capabilities, "/limits/max_prompt_tokens")
        .or_else(|| positive_capability_limit(capabilities, "/limits/max_context_window_tokens"))
}

fn extract_copilot_parallel_tool_calls(capabilities: Option<&Value>) -> Option<bool> {
    capabilities?
        .pointer("/supports/parallel_tool_calls")?
        .as_bool()
}

fn extract_copilot_vision(capabilities: Option<&Value>) -> Option<bool> {
    capabilities?.pointer("/supports/vision")?.as_bool()
}

fn extract_copilot_reasoning_effort(
    capabilities: Option<&Value>,
) -> Option<Vec<CopilotReasoningEffort>> {
    let efforts = capabilities?
        .pointer("/supports/reasoning_effort")
        .or_else(|| capabilities?.get("reasoning_effort"))?
        .as_array()?;
    Some(
        efforts
            .iter()
            .filter_map(|effort| match effort.as_str()? {
                "none" => Some(CopilotReasoningEffort::None),
                "minimal" => Some(CopilotReasoningEffort::Minimal),
                "low" => Some(CopilotReasoningEffort::Low),
                "medium" => Some(CopilotReasoningEffort::Medium),
                "high" => Some(CopilotReasoningEffort::High),
                "xhigh" => Some(CopilotReasoningEffort::Xhigh),
                "max" => Some(CopilotReasoningEffort::Max),
                "ultra" => Some(CopilotReasoningEffort::Ultra),
                _ => None,
            })
            .collect(),
    )
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

    #[error("Copilot /models returned HTTP 401")]
    ModelsUnauthorized,

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

    #[error("无效的 GitHub 域名: {0}")]
    InvalidDomain(String),
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
    /// GitHub 域名（github.com 或 GHES 域名）
    #[serde(default = "default_github_domain")]
    pub github_domain: String,
    /// 托管账号是否需要重新登录以补全缺失的凭据。
    /// Codex：缺少持久化 id_token 的旧账号为 true；Copilot 恒为 false。
    #[serde(default)]
    pub reauth_required: bool,
}

impl From<&GitHubAccountData> for GitHubAccount {
    fn from(data: &GitHubAccountData) -> Self {
        GitHubAccount {
            id: composite_account_id(&data.github_domain, data.user.id),
            login: data.user.login.clone(),
            avatar_url: data.user.avatar_url.clone(),
            authenticated_at: data.authenticated_at,
            github_domain: data.github_domain.clone(),
            reauth_required: false,
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
    /// GitHub 域名（github.com 或 GHES 域名）
    #[serde(default = "default_github_domain")]
    pub github_domain: String,
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

/// 按磁盘上的账号表给出 `(account_id -> GitHub 用户名, 默认账号 id)`（同 [`GitHubAccount::from`]）：托盘同步读状态用。
pub fn labels_on_disk(data_dir: &std::path::Path) -> (HashMap<String, String>, Option<String>) {
    let Ok(content) = fs::read_to_string(data_dir.join("copilot_auth.json")) else {
        return (HashMap::new(), None);
    };
    let Ok(store) = serde_json::from_str::<CopilotAuthStore>(&content) else {
        return (HashMap::new(), None);
    };
    if store.version < 2 {
        return (HashMap::new(), None);
    }
    let default_id = store
        .default_account_id
        .filter(|id| store.accounts.contains_key(id))
        .or_else(|| CopilotAuthManager::fallback_default_account_id(&store.accounts));
    let labels = store
        .accounts
        .iter()
        .map(|(id, data)| (id.clone(), GitHubAccount::from(data).login))
        .collect();
    (labels, default_id)
}

type CopilotModelCache = HashMap<(String, &'static str), Vec<CopilotModel>>;

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
    /// Copilot Models 进程内缓存，按账号 ID 和 API 版本区分。
    copilot_models: Arc<RwLock<CopilotModelCache>>,
    /// 目录里看不出 Auto 时，实际调一次 /auto 的结论（key = GitHub user ID）。
    /// 只存确定的答案，网络错误下次再探；和模型目录一起清。
    auto_routing_probes: Arc<RwLock<HashMap<String, bool>>>,
    /// Copilot API 端点缓存（key = GitHub user ID，从 /copilot_internal/user 获取）
    api_endpoints: Arc<RwLock<HashMap<String, String>>>,
    /// 每个账号的端点拉取锁，避免并发拉取重复打 GitHub API
    endpoint_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// 存储路径
    storage_path: PathBuf,
    /// 待迁移的旧格式 token
    pending_migration: Arc<RwLock<Option<String>>>,
    /// 旧认证数据迁移失败时的状态消息
    migration_error: Arc<RwLock<Option<String>>>,
    /// 凭证代次（账号添加/删除/切换默认时自增，使失效的 Auto 会话感知）
    credential_generation: Arc<AtomicU64>,
    /// 串行化旧认证迁移的网络请求
    migration_lock: Arc<Mutex<()>>,
    /// 串行化账号写盘和迁移状态提交，不跨网络请求持有
    auth_write_lock: Arc<Mutex<()>>,
    /// 登录成功或登出时递增，阻止在飞的旧迁移写回账号
    migration_epoch: Arc<AtomicU64>,
    /// GitHub / Copilot 认证请求超时
    request_timeout: Duration,
    /// 测试用 GitHub API 基址覆盖
    #[cfg(test)]
    github_api_base_override: Option<String>,
}

impl CopilotAuthManager {
    /// 创建新的认证管理器
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("copilot_auth.json");

        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            copilot_tokens: Arc::new(RwLock::new(HashMap::new())),
            copilot_models: Arc::new(RwLock::new(HashMap::new())),
            auto_routing_probes: Arc::new(RwLock::new(HashMap::new())),
            api_endpoints: Arc::new(RwLock::new(HashMap::new())),
            endpoint_locks: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
            pending_migration: Arc::new(RwLock::new(None)),
            migration_error: Arc::new(RwLock::new(None)),
            credential_generation: Arc::new(AtomicU64::new(1)),
            migration_lock: Arc::new(Mutex::new(())),
            auth_write_lock: Arc::new(Mutex::new(())),
            migration_epoch: Arc::new(AtomicU64::new(0)),
            request_timeout: GITHUB_HTTP_TIMEOUT,
            #[cfg(test)]
            github_api_base_override: None,
        };

        // 尝试从磁盘加载（同步，不发起网络请求）
        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[CopilotAuth] 加载存储失败: {e}");
        }

        manager
    }

    #[cfg(test)]
    pub(crate) async fn with_mock_server(data_dir: PathBuf, api_base: &str) -> Self {
        let auth = Self::with_test_github_api_base(data_dir, api_base, GITHUB_HTTP_TIMEOUT);
        let account_id = "123".to_string();
        auth.accounts.write().await.insert(
            account_id.clone(),
            GitHubAccountData {
                github_token: "mock-github-token".into(),
                user: GitHubUser {
                    id: 123,
                    login: "mock-user".into(),
                    avatar_url: None,
                },
                authenticated_at: 1,
                github_domain: DEFAULT_GITHUB_DOMAIN.into(),
            },
        );
        auth.api_endpoints
            .write()
            .await
            .insert(account_id.clone(), api_base.into());
        *auth.default_account_id.write().await = Some(account_id);
        auth
    }

    /// 获取凭证代次
    pub fn get_credential_generation(&self) -> u64 {
        self.credential_generation.load(Ordering::Relaxed)
    }

    /// 递增凭证代次（在账号变更、登出或关键凭证重置时调用）
    pub fn bump_credential_generation(&self) {
        self.credential_generation.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn with_test_github_api_base(
        data_dir: PathBuf,
        api_base: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        let mut manager = Self::new(data_dir);
        manager.github_api_base_override = Some(api_base.into());
        manager.request_timeout = timeout;
        manager
    }

    // ==================== 多账号管理方法 ====================

    /// 列出所有已认证的账号
    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let _ = self.ensure_migration_for_default().await;
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    /// 指定账号的 GitHub OAuth Token 和域名，给 Copilot CLI 的 `COPILOT_GITHUB_TOKEN` 用。
    /// 只在打开终端时经子进程环境传出，不写进任何文件或命令行。
    pub async fn github_credentials_for_account(
        &self,
        account_id: &str,
    ) -> Option<(String, String)> {
        let _ = self.ensure_migration_for_account(account_id).await;
        let accounts = self.accounts.read().await;
        accounts
            .get(account_id)
            .map(|account| (account.github_token.clone(), account.github_domain.clone()))
    }

    /// 移除指定账号
    pub async fn remove_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        log::info!("[CopilotAuth] 移除账号: {account_id}");

        let _write_guard = self.auth_write_lock.lock().await;
        if self.accounts.write().await.remove(account_id).is_none() {
            return Err(CopilotAuthError::AccountNotFound(account_id.to_string()));
        }

        // 同时移除缓存的 Copilot token
        {
            let mut tokens = self.copilot_tokens.write().await;
            tokens.remove(account_id);
        }
        {
            let mut models = self.copilot_models.write().await;
            models.retain(|(id, _), _| id != account_id);
        }
        self.auto_routing_probes.write().await.remove(account_id);
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

        {
            let accounts = self.accounts.read().await;
            let mut default_account_id = self.default_account_id.write().await;
            if default_account_id.as_deref() == Some(account_id) {
                *default_account_id = Self::fallback_default_account_id(&accounts);
            }
        }

        // 持久化
        self.save_to_disk().await?;
        self.bump_credential_generation();

        Ok(())
    }

    /// 持有 auth_write_lock 时，先持久化新账号，再发布认证状态。
    async fn add_account_internal(
        &self,
        github_token: String,
        user: GitHubUser,
        github_domain: String,
        copilot_token: Option<CopilotToken>,
    ) -> Result<GitHubAccount, CopilotAuthError> {
        let account_data = GitHubAccountData {
            github_token,
            user,
            authenticated_at: chrono::Utc::now().timestamp(),
            github_domain,
        };
        let account = GitHubAccount::from(&account_data);

        let mut accounts = self.accounts.write().await;
        let mut default_account_id = self.default_account_id.write().await;
        let mut tokens = self.copilot_tokens.write().await;
        let mut pending = self.pending_migration.write().await;
        let mut migration_error = self.migration_error.write().await;

        let mut store = CopilotAuthStore {
            version: 3,
            accounts: accounts.clone(),
            ..Default::default()
        };
        store.accounts.insert(account.id.clone(), account_data);
        store.default_account_id = match default_account_id.as_ref() {
            Some(id) if store.accounts.contains_key(id) => Some(id.clone()),
            Some(_) => Self::fallback_default_account_id(&store.accounts),
            None => Some(account.id.clone()),
        };
        self.write_store(&store)?;

        self.migration_epoch.fetch_add(1, Ordering::AcqRel);
        *pending = None;
        *migration_error = None;
        if let Some(token) = copilot_token {
            tokens.insert(account.id.clone(), token);
        }
        *default_account_id = store.default_account_id;
        *accounts = store.accounts;

        self.bump_credential_generation();
        log::info!("[CopilotAuth] 添加账号成功: {}", account.login);
        Ok(account)
    }

    /// 设置默认账号
    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        let _write_guard = self.auth_write_lock.lock().await;
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
        self.bump_credential_generation();
        Ok(())
    }

    // ==================== 设备码流程 ====================

    /// 启动设备码流程
    pub async fn start_device_flow(
        &self,
        github_domain: Option<&str>,
    ) -> Result<GitHubDeviceCodeResponse, CopilotAuthError> {
        let domain = match github_domain {
            Some(d) => normalize_github_domain(d)?,
            None => DEFAULT_GITHUB_DOMAIN.to_string(),
        };
        log::info!("[CopilotAuth] 启动设备码流程 (domain: {domain})");

        let response = crate::proxy::http_client::get()
            .post(github_device_code_url(&domain))
            .header("Accept", "application/json")
            .header("User-Agent", COPILOT_USER_AGENT)
            .form(&[
                ("client_id", github_client_id(&domain)),
                ("scope", "read:user"),
            ])
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
        github_domain: Option<&str>,
    ) -> Result<Option<GitHubAccount>, CopilotAuthError> {
        let domain = match github_domain {
            Some(d) => normalize_github_domain(d)?,
            None => DEFAULT_GITHUB_DOMAIN.to_string(),
        };
        log::debug!("[CopilotAuth] 轮询 OAuth Token (domain: {domain})");

        let response = crate::proxy::http_client::get()
            .post(github_oauth_token_url(&domain))
            .header("Accept", "application/json")
            .header("User-Agent", COPILOT_USER_AGENT)
            .form(&[
                ("client_id", github_client_id(&domain)),
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
        let user = self
            .fetch_user_info_with_token(&access_token, &domain)
            .await?;

        // GHES 无需换取 Copilot Token，直接使用 OAuth token 作为 Bearer
        // 参考 OpenCode 的实现：GHE Copilot 直接用 OAuth token 调用 copilot-api.{domain}
        let copilot_token = if is_ghes(&domain) {
            log::info!("[CopilotAuth] GHES 账号，跳过 Copilot Token 兑换，直接使用 OAuth token");
            None
        } else {
            Some(
                self.fetch_copilot_token_with_github_token(
                    &access_token,
                    &user.id.to_string(),
                    &domain,
                )
                .await?,
            )
        };

        // 新登录成功写盘后，旧认证迁移不再有权提交。
        let _write_guard = self.auth_write_lock.lock().await;
        let account = self
            .add_account_internal(access_token, user, domain, copilot_token)
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
        self.ensure_migration_for_account(account_id).await?;

        // GHES 账号直接使用 GitHub OAuth token，无需 Copilot token 交换
        let domain = self.get_account_domain(account_id).await;
        if is_ghes(&domain) {
            let accounts = self.accounts.read().await;
            return accounts
                .get(account_id)
                .map(|a| a.github_token.clone())
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()));
        }

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
        let (github_token, domain) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?;
            (account.github_token.clone(), account.github_domain.clone())
        };

        // 刷新 Copilot token
        let refreshed = self
            .fetch_copilot_token_with_github_token(&github_token, account_id, &domain)
            .await?;
        let token = refreshed.token.clone();
        self.copilot_tokens
            .write()
            .await
            .insert(account_id.to_string(), refreshed);
        Ok(token)
    }

    /// 获取有效的 Copilot Token（向后兼容：使用第一个账号）
    pub async fn get_valid_token(&self) -> Result<String, CopilotAuthError> {
        // 确保迁移完成
        self.ensure_migration_for_default().await?;

        match self.resolve_default_account_id().await {
            Some(id) => self.get_valid_token_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    /// 仅当指定账号当前缓存的 Copilot Bearer Token 与 expected_token 匹配时才失效缓存。
    /// 防止并发请求 A 刷新出新 Bearer 后，B 的旧请求迟到返回 401 误删新凭证。
    pub async fn invalidate_token_if_matches(
        &self,
        account_id: &str,
        expected_token: &str,
    ) -> bool {
        let mut tokens = self.copilot_tokens.write().await;
        if let Some(cached) = tokens.get(account_id) {
            if cached.token == expected_token {
                tokens.remove(account_id);
                log::info!(
                    "[CopilotAuth] 已失效账号 {account_id} 匹配版本的 Copilot Bearer Token 缓存"
                );
                return true;
            } else {
                log::info!(
                    "[CopilotAuth] 保留账号 {account_id} 较新的 Copilot Bearer Token（与 401 失败令牌版本不一致）"
                );
            }
        }
        false
    }

    // ==================== 模型和使用量 ====================

    /// 获取指定账号的 Copilot 可用模型列表
    pub async fn fetch_models_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        let mut models = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        models.retain(|model| model.model_picker_enabled);
        Ok(models)
    }

    pub async fn fetch_model_options_for_account(
        &self,
        account_id: &str,
    ) -> Result<CopilotModelOptions, CopilotAuthError> {
        let catalogue = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        let auto_routing_supported = catalogue.iter().any(CopilotModel::supports_auto_routing)
            || self
                .fetch_catalog_for_account(account_id)
                .await
                .is_ok_and(|models| models.iter().any(CopilotModel::supports_auto_routing))
            || self.probe_auto_routing(account_id).await;
        Ok(CopilotModelOptions {
            models: catalogue
                .into_iter()
                .filter(|model| model.model_picker_enabled)
                .collect(),
            auto_routing_supported,
        })
    }

    /// Free 账号的目录不带 `auto_discount`（那是给付费档的计价字段），但 /auto 照样能选模型。
    /// 目录看不出来时实际调一次 /auto（只选模型，不推理）：成功就算支持，上游明确拒绝算不支持。
    async fn probe_auto_routing(&self, account_id: &str) -> bool {
        if let Some(known) = self.auto_routing_probes.read().await.get(account_id) {
            return *known;
        }
        let Ok(token) = self.get_valid_token_for_account(account_id).await else {
            return false;
        };
        let api_base = self.get_api_endpoint(account_id).await;
        let result = super::copilot_auto::CopilotAutoClient::resolve(
            &crate::proxy::http_client::get_for_url(&api_base),
            &api_base,
            &token,
            "hi",
            false,
            self.request_timeout,
        )
        .await;
        let known = match result {
            Ok(_) => Some(true),
            Err(ProxyError::UpstreamError {
                status: 400 | 403 | 404,
                ..
            }) => Some(false),
            Err(ProxyError::UpstreamError { status: 401, .. }) => {
                self.invalidate_token_if_matches(account_id, &token).await;
                None
            }
            Err(err) => {
                log::debug!("[CopilotAuth] 账号 {account_id} 的 /auto 探测失败，下次再试: {err}");
                None
            }
        };
        if let Some(known) = known {
            self.auto_routing_probes
                .write()
                .await
                .insert(account_id.to_string(), known);
        }
        known.unwrap_or(false)
    }

    /// Auto may need the newer billing metadata. Keep the established /models
    /// contract and its cache for fixed-model requests.
    pub async fn fetch_catalog_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        let models = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        if models.iter().any(CopilotModel::supports_auto_routing) {
            return Ok(models);
        }
        self.fetch_catalog_version(account_id, super::copilot_auto::COPILOT_AUTO_API_VERSION)
            .await
    }

    async fn fetch_catalog_version(
        &self,
        account_id: &str,
        api_version: &'static str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        self.ensure_migration_for_account(account_id).await?;
        let key = (account_id.to_string(), api_version);
        if let Some(cached) = self.copilot_models.read().await.get(&key) {
            return Ok(cached.clone());
        }
        let models = self
            .fetch_models_for_account_uncached(account_id, api_version)
            .await?;
        self.copilot_models
            .write()
            .await
            .insert(key, models.clone());
        Ok(models)
    }

    async fn fetch_models_for_account_uncached(
        &self,
        account_id: &str,
        api_version: &'static str,
    ) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        let copilot_token = self.get_valid_token_for_account(account_id).await?;

        // 使用 get_api_endpoint() 动态解析 Copilot API 基础 URL。
        // 对于 github.com 账号，会查询 /copilot_internal/user 获取 endpoints.api 字段。
        // 对于 GHES 账号，/copilot_internal/user 可能不返回 endpoints——此时
        // get_api_endpoint() 会回退到 copilot_api_base(&domain)，与之前的静态 URL
        // 拼接结果一致。该回退行为是安全且符合预期的。
        let api_base = self.get_api_endpoint(account_id).await;
        let models_url = format!("{}/models", api_base);

        log::info!("[CopilotAuth] 获取账号 {account_id} 的 Copilot 可用模型");

        let response = crate::proxy::http_client::get()
            .get(&models_url)
            .header("Authorization", format!("Bearer {copilot_token}"))
            .header("Content-Type", "application/json")
            .header("copilot-integration-id", "vscode-chat")
            .header("editor-version", COPILOT_EDITOR_VERSION)
            .header("editor-plugin-version", COPILOT_PLUGIN_VERSION)
            .header("user-agent", COPILOT_USER_AGENT)
            .header("x-github-api-version", api_version)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.invalidate_token_if_matches(account_id, &copilot_token)
                .await;
            return Err(CopilotAuthError::ModelsUnauthorized);
        }
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
            .filter(CopilotModelsResponseItem::is_usable)
            .map(|m| CopilotModel {
                id: m.id,
                name: m.name,
                vendor: m.vendor,
                model_picker_enabled: m.model_picker_enabled,
                context_window: extract_copilot_prompt_limit(m.capabilities.as_ref()),
                supported_endpoints: m.supported_endpoints,
                supports_parallel_tool_calls: extract_copilot_parallel_tool_calls(
                    m.capabilities.as_ref(),
                ),
                supports_vision: extract_copilot_vision(m.capabilities.as_ref()),
                reasoning_effort: extract_copilot_reasoning_effort(m.capabilities.as_ref()),
                billing: m.billing,
            })
            .collect();

        log::info!(
            "[CopilotAuth] 从 /models 获取到 {} 个模型（未按模型选择器过滤）",
            models.len()
        );

        Ok(models)
    }

    pub async fn get_model_vendor_for_account(
        &self,
        account_id: &str,
        model_id: &str,
    ) -> Result<Option<String>, CopilotAuthError> {
        let models = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        Ok(super::copilot_model_map::resolve_model(model_id, &models).map(|model| model.vendor))
    }

    /// Vendor and upstream endpoints Copilot advertises for `model_id`.
    pub async fn get_model_record_for_account(
        &self,
        account_id: &str,
        model_id: &str,
    ) -> Result<Option<CopilotModel>, CopilotAuthError> {
        let models = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        Ok(super::copilot_model_map::resolve_model_record(model_id, &models).cloned())
    }

    pub async fn get_model_record(
        &self,
        model_id: &str,
    ) -> Result<Option<CopilotModel>, CopilotAuthError> {
        self.ensure_migration_for_default().await?;

        match self.resolve_default_account_id().await {
            Some(id) => self.get_model_record_for_account(&id, model_id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    pub async fn resolve_model_for_account(
        &self,
        account_id: &str,
        model_id: &str,
        api_format: crate::provider::CodexCopilotApiFormat,
    ) -> Result<Option<super::copilot_model_map::ResolvedCopilotModel>, CopilotAuthError> {
        let models = self
            .fetch_catalog_version(account_id, COPILOT_API_VERSION)
            .await?;
        Ok(super::copilot_model_map::resolve_model_with_format(
            model_id, &models, api_format,
        ))
    }

    /// 获取 Copilot 可用模型列表（向后兼容：使用第一个账号）
    pub async fn fetch_models(&self) -> Result<Vec<CopilotModel>, CopilotAuthError> {
        self.ensure_migration_for_default().await?;

        match self.resolve_default_account_id().await {
            Some(id) => self.fetch_models_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    pub async fn fetch_model_options(&self) -> Result<CopilotModelOptions, CopilotAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.fetch_model_options_for_account(&id).await,
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

    pub async fn resolve_model(
        &self,
        model_id: &str,
        api_format: crate::provider::CodexCopilotApiFormat,
    ) -> Result<Option<super::copilot_model_map::ResolvedCopilotModel>, CopilotAuthError> {
        self.ensure_migration_for_default().await?;

        match self.resolve_default_account_id().await {
            Some(id) => {
                self.resolve_model_for_account(&id, model_id, api_format)
                    .await
            }
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    /// 获取指定账号的 Copilot 使用量信息
    pub async fn fetch_usage_for_account(
        &self,
        account_id: &str,
    ) -> Result<CopilotUsageResponse, CopilotAuthError> {
        self.ensure_migration_for_account(account_id).await?;

        let (github_token, domain) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?;
            (account.github_token.clone(), account.github_domain.clone())
        };

        log::info!("[CopilotAuth] 获取账号 {account_id} 的 Copilot 使用量");

        let response = crate::proxy::http_client::get()
            .get(copilot_usage_url(&domain))
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
            "[CopilotAuth] 获取使用量成功，计划: {}, SKU: {}, 重置日期: {}",
            usage.copilot_plan,
            usage.access_type_sku.as_deref().unwrap_or("未返回"),
            usage.quota_reset_date
        );

        Ok(usage)
    }

    /// 获取 Copilot 使用量信息（向后兼容：使用第一个账号）
    pub async fn fetch_usage(&self) -> Result<CopilotUsageResponse, CopilotAuthError> {
        self.ensure_migration_for_default().await?;

        match self.resolve_default_account_id().await {
            Some(id) => self.fetch_usage_for_account(&id).await,
            None => Err(CopilotAuthError::GitHubTokenInvalid),
        }
    }

    // ==================== 状态查询 ====================

    /// 获取指定账号的 API 端点（缓存命中直接返回，未命中则从 API 惰性拉取）
    pub async fn get_api_endpoint(&self, account_id: &str) -> String {
        let _ = self.ensure_migration_for_account(account_id).await;

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
                let domain = self.get_account_domain(account_id).await;
                copilot_api_base(&domain)
            }
        }
    }

    /// 获取默认账号的 API 端点
    pub async fn get_default_api_endpoint(&self) -> String {
        let _ = self.ensure_migration_for_default().await;

        match self.resolve_default_account_id().await {
            Some(id) => self.get_api_endpoint(&id).await,
            None => {
                // 无账号时回退到 github.com 的默认端点
                copilot_api_base(DEFAULT_GITHUB_DOMAIN)
            }
        }
    }

    async fn fetch_and_cache_endpoint(&self, account_id: &str) -> Result<String, CopilotAuthError> {
        let (github_token, domain) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CopilotAuthError::AccountNotFound(account_id.to_string()))?;
            (account.github_token.clone(), account.github_domain.clone())
        };

        log::debug!("[CopilotAuth] 为账号 {account_id} 惰性拉取动态 API 端点");

        let response = crate::proxy::http_client::get()
            .get(copilot_usage_url(&domain))
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
            None => copilot_api_base(&domain),
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
        let _ = self.ensure_migration_for_default().await;

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

    /// 清除所有认证（登出所有账号）
    pub async fn clear_auth(&self) -> Result<(), CopilotAuthError> {
        log::info!("[CopilotAuth] 清除所有认证");

        let _write_guard = self.auth_write_lock.lock().await;
        self.clear_pending_migration().await;

        // 先清理内存状态，确保即使文件删除失败用户也能看到已登出
        {
            let mut accounts = self.accounts.write().await;
            accounts.clear();
        }
        {
            let mut default_account_id = self.default_account_id.write().await;
            default_account_id.take();
        }
        {
            let mut tokens = self.copilot_tokens.write().await;
            tokens.clear();
        }
        {
            let mut models = self.copilot_models.write().await;
            models.clear();
        }
        self.auto_routing_probes.write().await.clear();
        {
            let mut refresh_locks = self.refresh_locks.write().await;
            refresh_locks.clear();
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

        // 最后删除存储文件
        if self.storage_path.exists() {
            std::fs::remove_file(&self.storage_path)?;
        }
        self.bump_credential_generation();

        Ok(())
    }

    // ==================== 内部方法 ====================

    fn github_user_url_for(&self, domain: &str) -> String {
        #[cfg(test)]
        if let Some(base) = self.github_api_base_override.as_deref() {
            return format!("{}/user", base.trim_end_matches('/'));
        }
        github_user_url(domain)
    }

    fn copilot_token_url_for(&self, domain: &str) -> String {
        #[cfg(test)]
        if let Some(base) = self.github_api_base_override.as_deref() {
            return format!("{}/copilot_internal/v2/token", base.trim_end_matches('/'));
        }
        copilot_token_url(domain)
    }

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

    pub async fn resolve_default_account_id(&self) -> Option<String> {
        let stored_default = self.default_account_id.read().await.clone();
        let accounts = self.accounts.read().await;

        if let Some(default_id) = stored_default {
            if accounts.contains_key(&default_id) {
                return Some(default_id);
            }
        }

        Self::fallback_default_account_id(&accounts)
    }

    /// 获取指定账号的 GitHub 域名
    async fn get_account_domain(&self, account_id: &str) -> String {
        let accounts = self.accounts.read().await;
        accounts
            .get(account_id)
            .map(|a| a.github_domain.clone())
            .unwrap_or_else(|| DEFAULT_GITHUB_DOMAIN.to_string())
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

    async fn set_migration_error(&self, message: Option<String>) {
        let mut migration_error = self.migration_error.write().await;
        *migration_error = message;
    }

    /// 持有 auth_write_lock 时使旧迁移失效。
    async fn clear_pending_migration(&self) {
        self.migration_epoch.fetch_add(1, Ordering::AcqRel);
        *self.pending_migration.write().await = None;
        self.set_migration_error(None).await;
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
        domain: &str,
    ) -> Result<GitHubUser, CopilotAuthError> {
        let user_url = self.github_user_url_for(domain);
        let response = crate::proxy::http_client::get_for_url(&user_url)
            .get(&user_url)
            .timeout(self.request_timeout)
            .header("Authorization", format!("token {github_token}"))
            .header("User-Agent", COPILOT_USER_AGENT)
            .header("Editor-Version", COPILOT_EDITOR_VERSION)
            .header("Editor-Plugin-Version", COPILOT_PLUGIN_VERSION)
            .send()
            .await?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        if status == reqwest::StatusCode::FORBIDDEN {
            let rate_limited_header = response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim() == "0")
                .unwrap_or(false)
                || response
                    .headers()
                    .contains_key(reqwest::header::RETRY_AFTER);
            let body = response.text().await.unwrap_or_default();
            let lower = body.to_ascii_lowercase();
            if rate_limited_header
                || lower.contains("rate limit")
                || lower.contains("secondary rate")
                || lower.contains("abuse detection")
            {
                return Err(CopilotAuthError::NetworkError(format!(
                    "GitHub 用户信息请求被限流: HTTP {status}"
                )));
            }
            return Err(CopilotAuthError::GitHubTokenInvalid);
        }

        if !status.is_success() {
            return Err(CopilotAuthError::NetworkError(format!(
                "GitHub 用户信息请求失败: HTTP {status}"
            )));
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
        domain: &str,
    ) -> Result<CopilotToken, CopilotAuthError> {
        log::debug!("[CopilotAuth] 获取账号 {account_id} 的 Copilot Token (domain: {domain})");

        let token_url = self.copilot_token_url_for(domain);
        let response = crate::proxy::http_client::get_for_url(&token_url)
            .get(&token_url)
            .timeout(self.request_timeout)
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

        Ok(CopilotToken {
            token: token_response.token,
            expires_at: token_response.expires_at,
        })
    }

    // ==================== 存储和迁移 ====================

    /// 从磁盘加载（仅加载 token，不发起网络请求）
    fn load_from_disk_sync(&self) -> Result<(), CopilotAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.storage_path)?;
        let store: CopilotAuthStore = serde_json::from_str(&content)
            .map_err(|e| CopilotAuthError::ParseError(e.to_string()))?;

        if store.version >= 2 {
            // v2 多账号格式
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
        } else if store.github_token.is_some() {
            // v1 单账号格式，标记待迁移
            log::info!("[CopilotAuth] 检测到旧格式，将在首次访问时迁移");
            if let Ok(mut pending) = self.pending_migration.try_write() {
                *pending = store.github_token;
            }
        }

        Ok(())
    }

    async fn ensure_migration_for_account(&self, account_id: &str) -> Result<(), CopilotAuthError> {
        if self.accounts.read().await.contains_key(account_id) {
            Ok(())
        } else {
            self.ensure_migration_complete().await
        }
    }

    async fn ensure_migration_for_default(&self) -> Result<(), CopilotAuthError> {
        if !self.accounts.read().await.is_empty() {
            Ok(())
        } else {
            self.ensure_migration_complete().await
        }
    }

    /// 确保迁移完成
    async fn ensure_migration_complete(&self) -> Result<(), CopilotAuthError> {
        if self.pending_migration.read().await.is_none() {
            return Ok(());
        }

        let _migration_guard = self.migration_lock.lock().await;
        let epoch_before = self.migration_epoch.load(Ordering::Acquire);
        let Some(legacy_token) = self.pending_migration.read().await.clone() else {
            return Ok(());
        };

        log::info!("[CopilotAuth] 执行旧格式迁移");

        let user_result = self
            .fetch_user_info_with_token(&legacy_token, DEFAULT_GITHUB_DOMAIN)
            .await;
        let (user, account_id) = {
            let _write_guard = self.auth_write_lock.lock().await;
            if self.migration_epoch.load(Ordering::Acquire) != epoch_before {
                return Ok(());
            }
            let user = match user_result {
                Ok(user) => user,
                Err(e) => {
                    if matches!(e, CopilotAuthError::GitHubTokenInvalid) {
                        *self.pending_migration.write().await = None;
                    }
                    self.set_migration_error(Some(format!(
                        "Legacy Copilot auth migration failed: {e}"
                    )))
                    .await;
                    log::warn!("[CopilotAuth] 迁移失败: {e}");
                    return Err(e);
                }
            };
            let account_id = composite_account_id(DEFAULT_GITHUB_DOMAIN, user.id);
            if self.accounts.read().await.contains_key(&account_id) {
                self.clear_pending_migration().await;
                return Ok(());
            }
            (user, account_id)
        };

        // 验证订阅为尽力而为。完成写盘前不要把旧凭据的 token 发布到缓存。
        let copilot_token = match self
            .fetch_copilot_token_with_github_token(
                &legacy_token,
                &account_id,
                DEFAULT_GITHUB_DOMAIN,
            )
            .await
        {
            Ok(token) => Some(token),
            Err(e) => {
                log::warn!("[CopilotAuth] 迁移时验证 Copilot 订阅失败: {e}");
                None
            }
        };

        let _write_guard = self.auth_write_lock.lock().await;
        if self.migration_epoch.load(Ordering::Acquire) != epoch_before {
            return Ok(());
        }
        if let Err(e) = self
            .add_account_internal(
                legacy_token,
                user,
                DEFAULT_GITHUB_DOMAIN.to_string(),
                copilot_token,
            )
            .await
        {
            self.set_migration_error(Some(format!("Legacy Copilot auth migration failed: {e}")))
                .await;
            return Err(e);
        }
        log::info!("[CopilotAuth] 旧格式迁移完成");
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

        self.write_store(&store)
    }

    fn write_store(&self, store: &CopilotAuthStore) -> Result<(), CopilotAuthError> {
        let content = serde_json::to_string_pretty(store)
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
    use crate::provider::CodexCopilotApiFormat;
    use tempfile::tempdir;
    use tokio::sync::Notify;

    #[test]
    fn extracts_model_capabilities_and_supported_endpoints() {
        let item: CopilotModelsResponseItem = serde_json::from_value(serde_json::json!({
            "id": "gpt-5.6",
            "name": "GPT-5.6",
            "vendor": "OpenAI",
            "model_picker_enabled": true,
            "supported_endpoints": ["/responses", "/chat/completions"],
            "capabilities": {
                "limits": {
                    "max_prompt_tokens": 922000,
                    "max_context_window_tokens": 400000
                },
                "supports": {
                    "parallel_tool_calls": true,
                    "vision": true,
                    "reasoning_effort": ["none", "low", "turbo", "max", 7]
                }
            }
        }))
        .unwrap();

        assert_eq!(
            extract_copilot_prompt_limit(item.capabilities.as_ref()),
            Some(922_000)
        );
        assert_eq!(
            extract_copilot_parallel_tool_calls(item.capabilities.as_ref()),
            Some(true)
        );
        assert_eq!(
            extract_copilot_vision(item.capabilities.as_ref()),
            Some(true)
        );
        assert_eq!(
            extract_copilot_reasoning_effort(item.capabilities.as_ref()),
            Some(vec![
                CopilotReasoningEffort::None,
                CopilotReasoningEffort::Low,
                CopilotReasoningEffort::Max,
            ])
        );
        assert_eq!(
            item.supported_endpoints,
            vec!["/responses", "/chat/completions"]
        );
    }

    #[test]
    fn model_directory_keeps_incomplete_capabilities_and_auto_zero() {
        let usable = serde_json::from_value::<CopilotModelsResponseItem>(serde_json::json!({
            "id":"kimi-k3", "name":"Kimi K3", "vendor":"Moonshot",
            "model_picker_enabled":true, "supported_endpoints":["/v1/messages"],
            "capabilities":{"limits":{"max_prompt_tokens":128000,"max_output_tokens":8192},"supports":{"tool_calls":true}},
            "billing":{"auto_discount":0}
        })).unwrap();
        assert!(usable.is_usable());
        assert!(CopilotModel {
            id: usable.id.clone(),
            name: usable.name.clone(),
            vendor: usable.vendor.clone(),
            model_picker_enabled: usable.model_picker_enabled,
            context_window: None,
            supported_endpoints: usable.supported_endpoints.clone(),
            supports_parallel_tool_calls: None,
            supports_vision: None,
            reasoning_effort: None,
            billing: usable.billing.clone()
        }
        .supports_auto_routing());

        let mut disabled = usable.clone();
        disabled.policy = Some(serde_json::json!({"state":"disabled"}));
        assert!(!disabled.is_usable());
        let mut incomplete = usable;
        incomplete.capabilities = Some(
            serde_json::json!({"limits":{"max_prompt_tokens":128000},"supports":{"tool_calls":true}}),
        );
        assert!(incomplete.is_usable());
    }

    #[test]
    fn vision_capability_preserves_false_and_unknown() {
        assert_eq!(extract_copilot_vision(None), None);
        for (capabilities, expected) in [
            (serde_json::json!({}), None),
            (serde_json::json!({ "supports": {} }), None),
            (
                serde_json::json!({ "supports": { "vision": true } }),
                Some(true),
            ),
            (
                serde_json::json!({ "supports": { "vision": false } }),
                Some(false),
            ),
            (serde_json::json!({ "supports": { "vision": null } }), None),
            (
                serde_json::json!({ "supports": { "vision": "true" } }),
                None,
            ),
            (serde_json::json!({ "supports": { "vision": 1 } }), None),
            (serde_json::json!({ "supports": { "vision": [] } }), None),
        ] {
            assert_eq!(
                extract_copilot_vision(Some(&capabilities)),
                expected,
                "{capabilities}"
            );
        }
    }

    #[test]
    fn prompt_limit_falls_back_to_positive_context_window() {
        let capabilities = serde_json::json!({
            "limits": {
                "max_prompt_tokens": 0,
                "max_context_window_tokens": 400000
            }
        });

        assert_eq!(
            extract_copilot_prompt_limit(Some(&capabilities)),
            Some(400_000)
        );
    }

    #[test]
    fn reasoning_effort_preserves_absent_and_explicit_empty_capabilities() {
        assert_eq!(extract_copilot_reasoning_effort(None), None);
        assert_eq!(
            extract_copilot_reasoning_effort(Some(&serde_json::json!({
                "supports": { "reasoning_effort": [] }
            }))),
            Some(Vec::new())
        );
    }

    #[test]
    fn serializes_optional_live_model_capabilities() {
        let model = CopilotModel {
            id: "gpt-5.6-sol".to_string(),
            name: "GPT-5.6 Sol".to_string(),
            vendor: "OpenAI".to_string(),
            model_picker_enabled: true,
            context_window: Some(922_000),
            supported_endpoints: vec!["/responses".to_string()],
            supports_parallel_tool_calls: Some(true),
            supports_vision: Some(true),
            reasoning_effort: Some(vec![
                CopilotReasoningEffort::None,
                CopilotReasoningEffort::Xhigh,
                CopilotReasoningEffort::Max,
            ]),
            billing: None,
        };

        assert_eq!(
            serde_json::to_value(model).unwrap(),
            serde_json::json!({
                "id": "gpt-5.6-sol",
                "name": "GPT-5.6 Sol",
                "vendor": "OpenAI",
                "model_picker_enabled": true,
                "context_window": 922000,
                "supported_endpoints": ["/responses"],
                "supports_parallel_tool_calls": true,
                "supports_vision": true,
                "reasoning_effort": ["none", "xhigh", "max"]
            })
        );
    }

    #[test]
    fn vision_capability_serialization_preserves_false_and_unknown() {
        let mut model: CopilotModel = serde_json::from_value(serde_json::json!({
            "id": "copilot-model",
            "name": "Copilot Model",
            "vendor": "OpenAI",
            "model_picker_enabled": true
        }))
        .unwrap();
        assert_eq!(model.supports_vision, None);
        assert!(serde_json::to_value(&model)
            .unwrap()
            .get("supports_vision")
            .is_none());

        model.supports_vision = Some(false);
        let serialized = serde_json::to_value(model).unwrap();
        assert_eq!(serialized["supports_vision"], serde_json::json!(false));
        assert_eq!(
            serde_json::from_value::<CopilotModel>(serialized)
                .unwrap()
                .supports_vision,
            Some(false)
        );
    }

    #[test]
    fn shared_request_headers_include_copilot_identity() {
        let headers = build_copilot_request_headers("test-token").unwrap();
        let headers = headers.into_iter().collect::<http::HeaderMap>();

        assert_eq!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-token")
        );
        assert_eq!(
            headers
                .get("editor-version")
                .and_then(|value| value.to_str().ok()),
            Some(COPILOT_EDITOR_VERSION)
        );
        assert_eq!(
            headers
                .get("copilot-integration-id")
                .and_then(|value| value.to_str().ok()),
            Some(COPILOT_INTEGRATION_ID)
        );
        assert!(headers.contains_key("x-request-id"));
        assert_eq!(headers.get("x-request-id"), headers.get("x-agent-task-id"));
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
                github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
                reauth_required: false,
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
            GitHubAccountData {
                github_token: "gho_test_token".to_string(),
                user: GitHubUser {
                    login: "alice".to_string(),
                    id: 12345,
                    avatar_url: Some("https://example.com/alice.png".to_string()),
                },
                authenticated_at: 1700000000,
                github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
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
                github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
            },
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
        let data = GitHubAccountData {
            github_token: "gho_test".to_string(),
            user: GitHubUser {
                login: "testuser".to_string(),
                id: 99999,
                avatar_url: Some("https://example.com/avatar.png".to_string()),
            },
            authenticated_at: 1700000000,
            github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
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

    #[test]
    fn test_fallback_default_account_prefers_latest_authenticated() {
        let mut accounts = HashMap::new();
        accounts.insert(
            "12345".to_string(),
            GitHubAccountData {
                github_token: "gho_test_token".to_string(),
                user: GitHubUser {
                    login: "alice".to_string(),
                    id: 12345,
                    avatar_url: None,
                },
                authenticated_at: 1700000000,
                github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
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
                github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
            },
        );

        assert_eq!(
            CopilotAuthManager::fallback_default_account_id(&accounts),
            Some("67890".to_string())
        );
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
                GitHubAccountData {
                    github_token: "gho_test".to_string(),
                    user: GitHubUser {
                        login: "alice".to_string(),
                        id: 12345,
                        avatar_url: None,
                    },
                    authenticated_at: 1700000000,
                    github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
                },
            );
        }
        {
            let mut models = manager.copilot_models.write().await;
            models.insert(
                ("12345".to_string(), COPILOT_API_VERSION),
                vec![
                    CopilotModel {
                        id: "gpt-5.4".to_string(),
                        name: "GPT-5.4".to_string(),
                        vendor: "OpenAI".to_string(),
                        model_picker_enabled: true,
                        context_window: None,
                        supported_endpoints: Vec::new(),
                        supports_parallel_tool_calls: None,
                        supports_vision: None,
                        reasoning_effort: None,
                        billing: None,
                    },
                    CopilotModel {
                        id: "claude-sonnet-4".to_string(),
                        name: "Claude Sonnet 4".to_string(),
                        vendor: "Anthropic".to_string(),
                        model_picker_enabled: true,
                        context_window: None,
                        supported_endpoints: Vec::new(),
                        supports_parallel_tool_calls: None,
                        supports_vision: None,
                        reasoning_effort: None,
                        billing: None,
                    },
                    CopilotModel {
                        id: "gpt-hidden".to_string(),
                        name: "GPT Hidden".to_string(),
                        vendor: "OpenAI".to_string(),
                        model_picker_enabled: false,
                        context_window: Some(128_000),
                        supported_endpoints: vec![
                            "/v1/chat/completions".to_string(),
                            "/responses".to_string(),
                        ],
                        supports_parallel_tool_calls: None,
                        supports_vision: None,
                        reasoning_effort: None,
                        billing: None,
                    },
                ],
            );
        }

        let vendor = manager
            .get_model_vendor_for_account("12345", "gpt-5.4")
            .await
            .unwrap();
        assert_eq!(vendor.as_deref(), Some("OpenAI"));

        let default_vendor = manager
            .resolve_model("claude-sonnet-4", CodexCopilotApiFormat::Auto)
            .await
            .unwrap()
            .map(|model| model.vendor);
        assert_eq!(default_vendor.as_deref(), Some("Anthropic"));

        let hidden = manager
            .resolve_model_for_account("12345", "gpt-hidden", CodexCopilotApiFormat::Auto)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            hidden.transport,
            Some(super::super::copilot_model_map::CopilotTransport {
                protocol: super::super::copilot_model_map::CopilotProtocol::Responses,
                endpoint: "/responses".to_string(),
            })
        );
        for (api_format, protocol, endpoint) in [
            (
                CodexCopilotApiFormat::OpenaiChat,
                super::super::copilot_model_map::CopilotProtocol::Chat,
                "/v1/chat/completions",
            ),
            (
                CodexCopilotApiFormat::OpenaiResponses,
                super::super::copilot_model_map::CopilotProtocol::Responses,
                "/responses",
            ),
        ] {
            let account_model = manager
                .resolve_model_for_account("12345", "GPT-HIDDEN", api_format)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(account_model.id, "gpt-hidden");
            assert_eq!(
                account_model.transport,
                Some(super::super::copilot_model_map::CopilotTransport {
                    protocol,
                    endpoint: endpoint.to_string(),
                })
            );
            assert_eq!(
                manager
                    .resolve_model("GPT-HIDDEN", api_format)
                    .await
                    .unwrap(),
                Some(account_model)
            );
        }
        assert!(manager
            .fetch_models_for_account("12345")
            .await
            .unwrap()
            .iter()
            .all(|model| model.id != "gpt-hidden"));
        let models = manager
            .fetch_catalog_version("12345", COPILOT_API_VERSION)
            .await
            .unwrap();
        assert_eq!(models.len(), 3);
        let hidden = models.iter().find(|m| m.id == "gpt-hidden").unwrap();
        assert!(!hidden.model_picker_enabled);
        assert_eq!(hidden.context_window, Some(128_000));
        assert_eq!(
            hidden.supported_endpoints,
            vec!["/v1/chat/completions", "/responses"]
        );
    }

    #[tokio::test]
    async fn test_cached_vendor_fallback_is_independent_of_codex_transport_filter() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());
        let model = |endpoints: &[&str]| CopilotModel {
            id: "claude-opus-4.7".to_string(),
            name: "Claude Opus 4.7".to_string(),
            vendor: "Anthropic".to_string(),
            model_picker_enabled: false,
            context_window: Some(1_000_000),
            supported_endpoints: endpoints
                .iter()
                .map(|endpoint| endpoint.to_string())
                .collect(),
            supports_parallel_tool_calls: None,
            supports_vision: None,
            reasoning_effort: None,
            billing: None,
        };
        manager.copilot_models.write().await.extend([
            (
                ("messages-only".to_string(), COPILOT_API_VERSION),
                vec![model(&["/v1/messages"])],
            ),
            (
                ("no-metadata".to_string(), COPILOT_API_VERSION),
                vec![model(&[])],
            ),
            (
                ("chat-capable".to_string(), COPILOT_API_VERSION),
                vec![model(&["/chat/completions"])],
            ),
        ]);

        for account in ["messages-only", "no-metadata"] {
            assert!(manager
                .fetch_models_for_account(account)
                .await
                .unwrap()
                .is_empty());
            let models = manager
                .fetch_catalog_version(account, COPILOT_API_VERSION)
                .await
                .unwrap();
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].id, "claude-opus-4.7");
            assert!(!models[0].model_picker_enabled);
            assert_eq!(
                manager
                    .get_model_vendor_for_account(account, "claude-opus-4-8")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("Anthropic"),
                "{account}"
            );
            for format in [
                CodexCopilotApiFormat::Auto,
                CodexCopilotApiFormat::OpenaiChat,
                CodexCopilotApiFormat::OpenaiResponses,
            ] {
                assert_eq!(
                    manager
                        .resolve_model_for_account(account, "claude-opus-4-8", format)
                        .await
                        .unwrap(),
                    None,
                    "{account}: {format:?}"
                );
            }
        }

        let resolved = manager
            .resolve_model_for_account(
                "chat-capable",
                "claude-opus-4-8",
                CodexCopilotApiFormat::Auto,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.id, "claude-opus-4.7");
        assert_eq!(
            resolved.transport,
            Some(super::super::copilot_model_map::CopilotTransport {
                protocol: super::super::copilot_model_map::CopilotProtocol::Chat,
                endpoint: "/chat/completions".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn auto_support_falls_back_to_probing_auto_when_catalogue_is_silent() {
        use axum::{http::StatusCode, response::IntoResponse, Json};
        use std::sync::atomic::{AtomicU16, AtomicUsize};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let auto_status = Arc::new(AtomicU16::new(200));
        let auto_hits = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn({
            let auto_status = auto_status.clone();
            let auto_hits = auto_hits.clone();
            async move {
                let app = axum::Router::new()
                    .route(
                        "/copilot_internal/v2/token",
                        axum::routing::get(|| async {
                            Json(serde_json::json!({"token": "tid=copilot", "expires_at": 4102444800i64}))
                        }),
                    )
                    .route(
                        "/auto",
                        axum::routing::post(move || {
                            let auto_status = auto_status.clone();
                            let auto_hits = auto_hits.clone();
                            async move {
                                auto_hits.fetch_add(1, Ordering::SeqCst);
                                let status = auto_status.load(Ordering::SeqCst);
                                if status != 200 {
                                    return StatusCode::from_u16(status).unwrap().into_response();
                                }
                                Json(serde_json::json!({
                                    "selected_model": {"id": "mai-code-2-flash"},
                                    "session_token": "session",
                                    "expires_at": 4102444800i64
                                }))
                                .into_response()
                            }
                        }),
                    );
                axum::serve(listener, app).await.unwrap();
            }
        });

        // Free 账号的目录：两种 API 版本都没有 auto_discount。
        let silent: Vec<CopilotModel> = serde_json::from_value(serde_json::json!([
            {"id":"mai-code-2-flash","name":"MAI","vendor":"Microsoft","model_picker_enabled":true,
             "supported_endpoints":["/chat/completions","/responses"]}
        ]))
        .unwrap();
        let manager_with = |dir: &std::path::Path| {
            let silent = silent.clone();
            let base = base.clone();
            let dir = dir.to_path_buf();
            async move {
                let manager = CopilotAuthManager::with_mock_server(dir, &base).await;
                manager.copilot_models.write().await.extend([
                    (("123".into(), COPILOT_API_VERSION), silent.clone()),
                    (
                        (
                            "123".into(),
                            super::super::copilot_auto::COPILOT_AUTO_API_VERSION,
                        ),
                        silent,
                    ),
                ]);
                manager
            }
        };

        // /auto 能选模型就算支持，结论缓存起来，不再重复探测。
        let dir = tempdir().unwrap();
        let manager = manager_with(dir.path()).await;
        assert!(
            manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        assert!(
            manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        assert_eq!(auto_hits.load(Ordering::SeqCst), 1);

        // 上游明确拒绝：不支持，同样只探一次。
        auto_status.store(403, Ordering::SeqCst);
        let dir = tempdir().unwrap();
        let manager = manager_with(dir.path()).await;
        assert!(
            !manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        assert!(
            !manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        assert_eq!(auto_hits.load(Ordering::SeqCst), 2);

        // 临时故障不下结论，下次再探。
        auto_status.store(503, Ordering::SeqCst);
        let dir = tempdir().unwrap();
        let manager = manager_with(dir.path()).await;
        assert!(
            !manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        auto_status.store(200, Ordering::SeqCst);
        assert!(
            manager
                .fetch_model_options()
                .await
                .unwrap()
                .auto_routing_supported
        );
        assert_eq!(auto_hits.load(Ordering::SeqCst), 4);

        server.abort();
    }

    #[tokio::test]
    async fn test_fetch_catalog_returns_hidden_models_with_account_isolation() {
        let dir = tempdir().unwrap();
        let manager =
            CopilotAuthManager::with_mock_server(dir.path().to_path_buf(), "http://unused").await;
        let models: Vec<CopilotModel> = serde_json::from_value(serde_json::json!([
            {"id":"visible","name":"Visible","vendor":"OpenAI","model_picker_enabled":true},
            {"id":"hidden","name":"Hidden","vendor":"Moonshot AI","billing":{"auto_discount":0}}
        ]))
        .unwrap();
        let mut unsupported = models[1].clone();
        unsupported.billing = None;
        manager.copilot_models.write().await.extend([
            (("123".into(), COPILOT_API_VERSION), models),
            (
                ("other".into(), COPILOT_API_VERSION),
                vec![unsupported.clone()],
            ),
            (
                (
                    "other".into(),
                    super::super::copilot_auto::COPILOT_AUTO_API_VERSION,
                ),
                vec![unsupported],
            ),
        ]);

        let options = manager.fetch_model_options().await.unwrap();
        assert!(options.auto_routing_supported);
        assert_eq!(options.models.len(), 1);
        assert_eq!(options.models[0].id, "visible");
        assert_eq!(manager.fetch_models().await.unwrap()[0].id, "visible");
        let directory = manager.fetch_catalog_for_account("123").await.unwrap();
        assert_eq!(directory[1].id, "hidden");
        assert!(directory[1].supports_auto_routing());
        let other = manager
            .fetch_model_options_for_account("other")
            .await
            .unwrap();
        assert!(other.models.is_empty());
        assert!(!other.auto_routing_supported);
    }

    #[test]
    fn test_copilot_models_response_item_defaults_missing_picker_flag() {
        let json_with_missing_picker = r#"{
            "data": [
                {
                    "id": "test-model-1",
                    "name": "Test Model 1",
                    "vendor": "Test Vendor",
                    "supported_endpoints": ["/chat/completions"],
                    "billing": {"auto_discount": 0.3}
                }
            ]
        }"#;

        let parsed: CopilotModelsResponse = serde_json::from_str(json_with_missing_picker).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, "test-model-1");
        assert!(!parsed.data[0].model_picker_enabled);
        assert_eq!(
            parsed.data[0].billing.as_ref().unwrap().auto_discount,
            Some(0.3)
        );
    }

    #[test]
    fn auto_routing_requires_discount_field_even_when_zero() {
        let model = |discount: Option<f64>| CopilotModel {
            id: "m".into(),
            name: "M".into(),
            vendor: "OpenAI".into(),
            model_picker_enabled: true,
            context_window: None,
            supported_endpoints: vec![],
            supports_parallel_tool_calls: None,
            supports_vision: None,
            reasoning_effort: None,
            billing: discount.map(|auto_discount| CopilotModelBilling {
                auto_discount: Some(auto_discount),
            }),
        };
        assert!(model(Some(0.3)).supports_auto_routing());
        assert!(model(Some(0.0)).supports_auto_routing());
        assert!(!model(None).supports_auto_routing());
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
                GitHubAccountData {
                    github_token: "gho_test".to_string(),
                    user: GitHubUser {
                        login: "alice".to_string(),
                        id: 12345,
                        avatar_url: None,
                    },
                    authenticated_at: 1700000000,
                    github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
                },
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
                GitHubAccountData {
                    github_token: "gho_test".to_string(),
                    user: GitHubUser {
                        login: "alice".to_string(),
                        id: 12345,
                        avatar_url: None,
                    },
                    authenticated_at: 1700000000,
                    github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
                },
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
    async fn test_clear_auth_cleans_memory_even_when_file_removal_fails() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // Create a directory at storage_path so remove_file fails
        std::fs::create_dir_all(&manager.storage_path).unwrap();

        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "12345".to_string(),
                GitHubAccountData {
                    github_token: "gho_test".to_string(),
                    user: GitHubUser {
                        login: "alice".to_string(),
                        id: 12345,
                        avatar_url: None,
                    },
                    authenticated_at: 1700000000,
                    github_domain: DEFAULT_GITHUB_DOMAIN.to_string(),
                },
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
        // Should still return an error for the file deletion failure
        assert!(result.is_err());

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
    async fn test_credential_generation_bumps_on_logout_and_removal() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());
        let initial_gen = manager.get_credential_generation();

        // Adding account data
        {
            let mut accounts = manager.accounts.write().await;
            accounts.insert(
                "acc_1".to_string(),
                GitHubAccountData {
                    github_token: "gho_test".to_string(),
                    user: GitHubUser {
                        login: "alice".to_string(),
                        id: 1,
                        avatar_url: None,
                    },
                    authenticated_at: 100,
                    github_domain: "github.com".to_string(),
                },
            );
        }

        manager.remove_account("acc_1").await.unwrap();
        let gen_after_remove = manager.get_credential_generation();
        assert!(gen_after_remove > initial_gen);

        manager.clear_auth().await.unwrap();
        let gen_after_clear = manager.get_credential_generation();
        assert!(gen_after_clear > gen_after_remove);
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
        assert_eq!(endpoint, copilot_api_base(DEFAULT_GITHUB_DOMAIN));
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

    #[test]
    fn test_normalize_github_domain() {
        // 基本用法
        assert_eq!(normalize_github_domain("github.com").unwrap(), "github.com");
        assert_eq!(
            normalize_github_domain("company.ghe.com").unwrap(),
            "company.ghe.com"
        );

        // 剥离协议
        assert_eq!(
            normalize_github_domain("https://company.ghe.com").unwrap(),
            "company.ghe.com"
        );
        assert_eq!(
            normalize_github_domain("http://company.ghe.com").unwrap(),
            "company.ghe.com"
        );

        // 小写化
        assert_eq!(normalize_github_domain("GitHub.COM").unwrap(), "github.com");
        assert_eq!(
            normalize_github_domain("Company.GHE.Com").unwrap(),
            "company.ghe.com"
        );

        // 剥离尾斜杠和 path
        assert_eq!(
            normalize_github_domain("company.ghe.com/").unwrap(),
            "company.ghe.com"
        );
        assert_eq!(
            normalize_github_domain("company.ghe.com/api/v3").unwrap(),
            "company.ghe.com"
        );

        // 剥离 query 和 fragment
        assert_eq!(
            normalize_github_domain("company.ghe.com?foo=bar").unwrap(),
            "company.ghe.com"
        );
        assert_eq!(
            normalize_github_domain("company.ghe.com#section").unwrap(),
            "company.ghe.com"
        );

        // 保留端口
        assert_eq!(
            normalize_github_domain("company.ghe.com:8443").unwrap(),
            "company.ghe.com:8443"
        );

        // 拒绝 userinfo
        assert!(normalize_github_domain("user@company.ghe.com").is_err());

        // 拒绝空输入
        assert!(normalize_github_domain("").is_err());
        assert!(normalize_github_domain("   ").is_err());
    }

    #[test]
    fn test_composite_account_id() {
        // github.com 保持原格式（向后兼容）
        assert_eq!(composite_account_id("github.com", 12345), "12345");

        // GHES 使用复合格式
        assert_eq!(
            composite_account_id("company.ghe.com", 12345),
            "company.ghe.com:12345"
        );

        // 不同 GHES 实例，相同 user ID，不冲突
        assert_ne!(
            composite_account_id("a.ghe.com", 1),
            composite_account_id("b.ghe.com", 1)
        );
    }

    #[test]
    fn test_github_account_from_data_ghes_uses_composite_id() {
        let data = GitHubAccountData {
            github_token: "gho_test".to_string(),
            user: GitHubUser {
                login: "testuser".to_string(),
                id: 99999,
                avatar_url: None,
            },
            authenticated_at: 1700000000,
            github_domain: "company.ghe.com".to_string(),
        };

        let account = GitHubAccount::from(&data);
        assert_eq!(account.id, "company.ghe.com:99999");
    }

    #[tokio::test]
    async fn test_invalidate_token_if_matches() {
        let temp_dir = tempdir().unwrap();
        let manager = CopilotAuthManager::new(temp_dir.path().to_path_buf());

        // 插入 token-v2
        {
            let mut tokens = manager.copilot_tokens.write().await;
            tokens.insert(
                "acc-1".to_string(),
                CopilotToken {
                    token: "bearer-token-v2".to_string(),
                    expires_at: 1800000000,
                },
            );
        }

        // 旧的 401 请求携带 bearer-token-v1，尝试失效 -> 不应删除 v2
        let removed = manager
            .invalidate_token_if_matches("acc-1", "bearer-token-v1")
            .await;
        assert!(!removed);
        {
            let tokens = manager.copilot_tokens.read().await;
            assert_eq!(
                tokens.get("acc-1").map(|t| t.token.as_str()),
                Some("bearer-token-v2")
            );
        }

        // 别的账号尝试失效 -> 不影响 acc-1
        let removed = manager
            .invalidate_token_if_matches("acc-2", "bearer-token-v2")
            .await;
        assert!(!removed);

        // 匹配当前 token-v2 -> 成功失效
        let removed = manager
            .invalidate_token_if_matches("acc-1", "bearer-token-v2")
            .await;
        assert!(removed);
        {
            let tokens = manager.copilot_tokens.read().await;
            assert!(tokens.get("acc-1").is_none());
        }
    }

    enum MockGitHubAction {
        DropConnection,
        Timeout(Duration),
        Respond {
            status_line: &'static str,
            extra_headers: &'static str,
            body: String,
        },
    }

    async fn spawn_mock_github_server(
        initial_actions: Vec<MockGitHubAction>,
        token_exchange_gate: Option<(Arc<Notify>, Arc<Notify>)>,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use std::collections::VecDeque;
        use std::sync::atomic::AtomicUsize;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let actions = Arc::new(Mutex::new(VecDeque::from(initial_actions)));
        let user_hits = Arc::new(AtomicUsize::new(0));
        let user_hits_for_loop = Arc::clone(&user_hits);

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let actions = Arc::clone(&actions);
                let user_hits = Arc::clone(&user_hits_for_loop);
                let token_exchange_gate = token_exchange_gate.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::with_capacity(1024);
                    let mut chunk = [0u8; 512];
                    loop {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&chunk[..n]);
                                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 8192 {
                                    break;
                                }
                            }
                        }
                    }

                    let request_text = String::from_utf8_lossy(&buf);
                    let first_line = request_text.lines().next().unwrap_or("");

                    if first_line.starts_with("GET /copilot_internal/v2/token") {
                        if let Some((started, release)) = token_exchange_gate {
                            started.notify_one();
                            release.notified().await;
                        }
                        let body = r#"{"token":"tid=copilot-jwt","expires_at":4102444800}"#;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len(),
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.flush().await;
                        let _ = stream.shutdown().await;
                        return;
                    }

                    if first_line.starts_with("GET /user") {
                        user_hits.fetch_add(1, Ordering::SeqCst);
                        let next_action = actions.lock().await.pop_front();
                        match next_action {
                            Some(MockGitHubAction::DropConnection) => {
                                let _ = stream.shutdown().await;
                            }
                            Some(MockGitHubAction::Timeout(delay)) => {
                                tokio::time::sleep(delay).await;
                            }
                            Some(MockGitHubAction::Respond {
                                status_line,
                                extra_headers,
                                body,
                            }) => {
                                let response = format!(
                                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len(),
                                );
                                let _ = stream.write_all(response.as_bytes()).await;
                                let _ = stream.flush().await;
                                let _ = stream.shutdown().await;
                            }
                            None => {
                                let body = r#"{"message":"unexpected /user request"}"#;
                                let response = format!(
                                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len(),
                                );
                                let _ = stream.write_all(response.as_bytes()).await;
                                let _ = stream.flush().await;
                                let _ = stream.shutdown().await;
                            }
                        }
                    }
                });
            }
        });

        (format!("http://{addr}"), user_hits, handle)
    }

    fn write_legacy_v1_copilot_store(dir: &std::path::Path, token: &str) {
        let legacy = serde_json::json!({
            "github_token": token,
            "authenticated_at": 1700000000_i64
        });
        fs::write(
            dir.join("copilot_auth.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn legacy_migration_retains_pending_on_transient_errors_and_succeeds_on_retry() {
        let dir = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir.path(), "gho_legacy_retry");

        let (base_url, user_hits, server) = spawn_mock_github_server(
            vec![
            MockGitHubAction::DropConnection,
            MockGitHubAction::Timeout(Duration::from_millis(250)),
            MockGitHubAction::Respond {
                status_line: "503 Service Unavailable",
                extra_headers: "",
                body: r#"{"message":"service unavailable"}"#.to_string(),
            },
            MockGitHubAction::Respond {
                status_line: "403 Forbidden",
                extra_headers: "x-ratelimit-remaining: 0\r\n",
                body: r#"{"message":"API rate limit exceeded"}"#.to_string(),
            },
            MockGitHubAction::Respond {
                status_line: "200 OK",
                extra_headers: "",
                body:
                    r#"{"login":"octocat","id":4242,"avatar_url":"https://avatars.example/u/4242"}"#
                        .to_string(),
            },
        ],
            None,
        )
        .await;

        let manager = CopilotAuthManager::with_test_github_api_base(
            dir.path().to_path_buf(),
            base_url,
            Duration::from_millis(80),
        );
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_retry")
        );

        // 1st attempt: transport connection drop -> pending_migration retained
        let status1 = manager.get_status().await;
        assert!(!status1.authenticated);
        assert!(status1.migration_error.is_some());
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_retry")
        );

        // 2nd attempt: request timeout -> pending_migration retained
        let status2 = manager.get_status().await;
        assert!(!status2.authenticated);
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_retry")
        );

        // 3rd attempt: HTTP 503 -> get_valid_token surfaces NetworkError and retains pending_migration
        let token_err = manager.get_valid_token().await.unwrap_err();
        assert!(
            matches!(token_err, CopilotAuthError::NetworkError(_)),
            "expected NetworkError on 503, got {token_err:?}"
        );
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_retry")
        );

        // 4th attempt: HTTP 403 rate-limited -> pending_migration retained
        let status4 = manager.get_status().await;
        assert!(!status4.authenticated);
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_retry")
        );

        // 5th attempt (concurrent callers): network recovers -> serialized migration succeeds once
        let (ok_a, ok_b) = tokio::join!(manager.get_status(), manager.get_status());
        assert!(ok_a.authenticated);
        assert!(ok_b.authenticated);
        assert_eq!(ok_a.username.as_deref(), Some("octocat"));
        assert_eq!(ok_a.default_account_id.as_deref(), Some("4242"));
        assert!(ok_a.migration_error.is_none());
        assert!(manager.pending_migration.read().await.is_none());
        assert_eq!(user_hits.load(Ordering::SeqCst), 5);

        // Verify persisted multi-account store on disk (v3 format, legacy github_token cleared)
        let persisted: CopilotAuthStore = serde_json::from_str(
            &fs::read_to_string(dir.path().join("copilot_auth.json")).unwrap(),
        )
        .unwrap();
        assert!(persisted.version >= 2);
        assert!(persisted.github_token.is_none());
        assert_eq!(persisted.default_account_id.as_deref(), Some("4242"));
        assert_eq!(
            persisted
                .accounts
                .get("4242")
                .map(|account| account.github_token.as_str()),
            Some("gho_legacy_retry")
        );

        let reloaded = CopilotAuthManager::new(dir.path().to_path_buf());
        assert!(reloaded.pending_migration.read().await.is_none());
        assert!(!reloaded.list_accounts().await.is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn legacy_migration_clears_pending_on_deterministic_401_and_bad_credentials_403() {
        // 1. Deterministic 401 Unauthorized clears pending_migration
        let dir_401 = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir_401.path(), "gho_revoked_401");

        let (base_401, hits_401, server_401) = spawn_mock_github_server(
            vec![
                MockGitHubAction::Respond {
                    status_line: "401 Unauthorized",
                    extra_headers: "",
                    body: r#"{"message":"Bad credentials"}"#.to_string(),
                },
                MockGitHubAction::Respond {
                    status_line: "200 OK",
                    extra_headers: "",
                    body: r#"{"login":"should-not-run","id":1,"avatar_url":null}"#.to_string(),
                },
            ],
            None,
        )
        .await;

        let manager_401 = CopilotAuthManager::with_test_github_api_base(
            dir_401.path().to_path_buf(),
            base_401,
            Duration::from_secs(2),
        );
        let first = manager_401.get_status().await;
        assert!(!first.authenticated);
        assert!(first.migration_error.is_some());
        assert!(manager_401.pending_migration.read().await.is_none());

        let second = manager_401.get_status().await;
        assert!(!second.authenticated);
        assert_eq!(hits_401.load(Ordering::SeqCst), 1);
        server_401.abort();

        // 2. Deterministic 403 Forbidden (bad credentials, not rate-limited) clears pending_migration
        let dir_403 = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir_403.path(), "gho_forbidden_403");

        let (base_403, hits_403, server_403) = spawn_mock_github_server(
            vec![
                MockGitHubAction::Respond {
                    status_line: "403 Forbidden",
                    extra_headers: "",
                    body: r#"{"message":"Resource not accessible by integration"}"#.to_string(),
                },
                MockGitHubAction::Respond {
                    status_line: "200 OK",
                    extra_headers: "",
                    body: r#"{"login":"should-not-run","id":2,"avatar_url":null}"#.to_string(),
                },
            ],
            None,
        )
        .await;

        let manager_403 = CopilotAuthManager::with_test_github_api_base(
            dir_403.path().to_path_buf(),
            base_403,
            Duration::from_secs(2),
        );
        let status_403 = manager_403.get_status().await;
        assert!(!status_403.authenticated);
        assert!(manager_403.pending_migration.read().await.is_none());
        let _ = manager_403.get_status().await;
        assert_eq!(hits_403.load(Ordering::SeqCst), 1);
        server_403.abort();
    }

    #[tokio::test]
    async fn explicit_logout_clears_pending_migration_after_transient_failure() {
        let dir = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir.path(), "gho_legacy_logout");

        let (base_url, user_hits, server) = spawn_mock_github_server(
            vec![
                MockGitHubAction::Respond {
                    status_line: "503 Service Unavailable",
                    extra_headers: "",
                    body: r#"{"message":"temporary outage"}"#.to_string(),
                },
                MockGitHubAction::Respond {
                    status_line: "200 OK",
                    extra_headers: "",
                    body: r#"{"login":"octocat","id":4242,"avatar_url":null}"#.to_string(),
                },
            ],
            None,
        )
        .await;

        let manager = CopilotAuthManager::with_test_github_api_base(
            dir.path().to_path_buf(),
            base_url,
            Duration::from_secs(2),
        );

        // Transient failure retains pending_migration
        let status_before = manager.get_status().await;
        assert!(!status_before.authenticated);
        assert_eq!(
            manager.pending_migration.read().await.as_deref(),
            Some("gho_legacy_logout")
        );

        // Explicit logout clears pending_migration, migration_error, and disk store permanently
        manager.clear_auth().await.unwrap();
        assert!(manager.pending_migration.read().await.is_none());
        assert!(manager.migration_error.read().await.is_none());
        assert!(!dir.path().join("copilot_auth.json").exists());

        // Subsequent get_status must not retry migrating the logged-out legacy token
        let status_after = manager.get_status().await;
        assert!(!status_after.authenticated);
        assert!(status_after.accounts.is_empty());
        assert!(status_after.migration_error.is_none());
        assert_eq!(user_hits.load(Ordering::SeqCst), 1);

        server.abort();
    }

    #[tokio::test]
    async fn explicit_login_supersedes_an_in_flight_legacy_migration() {
        let dir = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir.path(), "legacy-oauth");
        let exchange_started = Arc::new(Notify::new());
        let release_exchange = Arc::new(Notify::new());
        let deadline = Duration::from_secs(5);
        let (base_url, user_hits, server) = spawn_mock_github_server(
            vec![
                MockGitHubAction::Respond {
                    status_line: "503 Service Unavailable",
                    extra_headers: "",
                    body: r#"{"message":"temporary outage"}"#.to_string(),
                },
                MockGitHubAction::Respond {
                    status_line: "200 OK",
                    extra_headers: "",
                    body: r#"{"login":"octocat","id":4242,"avatar_url":null}"#.to_string(),
                },
            ],
            Some((exchange_started.clone(), release_exchange.clone())),
        )
        .await;
        let manager = Arc::new(CopilotAuthManager::with_test_github_api_base(
            dir.path().to_path_buf(),
            base_url,
            deadline * 2,
        ));
        assert!(manager.get_status().await.migration_error.is_some());

        let migration = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.ensure_migration_complete().await })
        };
        tokio::time::timeout(deadline, exchange_started.notified())
            .await
            .expect("legacy token exchange started");

        // Commit the same account with the credentials returned by a fresh device login.
        {
            let _write_guard = manager.auth_write_lock.lock().await;
            manager
                .add_account_internal(
                    "fresh-oauth".to_string(),
                    GitHubUser {
                        login: "octocat".to_string(),
                        id: 4242,
                        avatar_url: None,
                    },
                    DEFAULT_GITHUB_DOMAIN.to_string(),
                    Some(CopilotToken {
                        token: "fresh-copilot".to_string(),
                        expires_at: 4102444800,
                    }),
                )
                .await
                .unwrap();
        }
        let status = tokio::time::timeout(deadline, manager.get_status())
            .await
            .expect("status should not wait for the obsolete migration");
        assert!(status.authenticated);
        assert!(status.migration_error.is_none());

        release_exchange.notify_one();
        tokio::time::timeout(deadline, migration)
            .await
            .expect("legacy migration finished")
            .unwrap()
            .unwrap();

        assert!(manager.pending_migration.read().await.is_none());
        assert!(manager.get_status().await.migration_error.is_none());
        assert_eq!(
            manager.get_valid_token_for_account("4242").await.unwrap(),
            "fresh-copilot"
        );
        let persisted = CopilotAuthManager::new(dir.path().to_path_buf());
        assert_eq!(
            persisted.github_credentials_for_account("4242").await,
            Some(("fresh-oauth".to_string(), DEFAULT_GITHUB_DOMAIN.to_string()))
        );
        assert_eq!(user_hits.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn logout_during_legacy_token_exchange_prevents_a_late_commit() {
        let dir = tempdir().unwrap();
        write_legacy_v1_copilot_store(dir.path(), "legacy-oauth");
        let exchange_started = Arc::new(Notify::new());
        let release_exchange = Arc::new(Notify::new());
        let deadline = Duration::from_secs(5);
        let (base_url, _, server) = spawn_mock_github_server(
            vec![MockGitHubAction::Respond {
                status_line: "200 OK",
                extra_headers: "",
                body: r#"{"login":"octocat","id":4242,"avatar_url":null}"#.to_string(),
            }],
            Some((exchange_started.clone(), release_exchange.clone())),
        )
        .await;
        let manager = Arc::new(CopilotAuthManager::with_test_github_api_base(
            dir.path().to_path_buf(),
            base_url,
            deadline * 2,
        ));
        let migration = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.ensure_migration_complete().await })
        };
        tokio::time::timeout(deadline, exchange_started.notified())
            .await
            .expect("legacy token exchange started");

        tokio::time::timeout(deadline, manager.clear_auth())
            .await
            .expect("logout should not wait for the legacy exchange")
            .unwrap();
        release_exchange.notify_one();
        tokio::time::timeout(deadline, migration)
            .await
            .expect("legacy migration finished")
            .unwrap()
            .unwrap();

        let status = manager.get_status().await;
        assert!(!status.authenticated);
        assert!(status.migration_error.is_none());
        assert!(manager.pending_migration.read().await.is_none());
        assert!(manager.copilot_tokens.read().await.is_empty());
        assert!(!manager.storage_path.exists());
        server.abort();
    }
}
