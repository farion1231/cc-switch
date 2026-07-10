use http::header::{HeaderValue, InvalidHeaderValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// SSOT 模式：不再写供应商副本文件

/// 供应商结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "websiteUrl")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "sortIndex")]
    pub sort_index: Option<usize>,
    /// 备注信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// 供应商元数据（不写入 live 配置，仅存于 ~/.cc-switch/config.json）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
    /// 图标名称（如 "openai", "anthropic"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// 图标颜色（Hex 格式，如 "#00A67E"）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "iconColor")]
    pub icon_color: Option<String>,
    /// 是否加入故障转移队列
    #[serde(default)]
    #[serde(rename = "inFailoverQueue")]
    pub in_failover_queue: bool,
}

impl Provider {
    /// 从现有ID创建供应商
    pub fn with_id(
        id: String,
        name: String,
        settings_config: Value,
        website_url: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            settings_config,
            website_url,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    pub fn is_codex_oauth(&self) -> bool {
        self.provider_type() == Some("codex_oauth")
    }

    pub fn is_github_copilot(&self) -> bool {
        self.provider_type() == Some("github_copilot")
            || self.claude_base_url_contains("githubcopilot.com")
    }

    pub fn uses_managed_account_auth(&self) -> bool {
        self.is_github_copilot()
            || self.is_codex_oauth()
            || self.claude_base_url_contains("chatgpt.com/backend-api/codex")
    }

    fn provider_type(&self) -> Option<&str> {
        self.meta.as_ref().and_then(|m| m.provider_type.as_deref())
    }

    fn claude_base_url_contains(&self, needle: &str) -> bool {
        self.settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|base_url| base_url.contains(needle))
            .unwrap_or(false)
    }

    pub fn codex_fast_mode_enabled(&self) -> bool {
        self.meta
            .as_ref()
            .map(|m| m.codex_fast_mode_enabled())
            .unwrap_or(false)
    }

    pub fn has_usage_script_enabled(&self) -> bool {
        self.meta
            .as_ref()
            .and_then(|m| m.usage_script.as_ref())
            .map(|s| s.enabled)
            .unwrap_or(false)
    }

    /// Resolve `(base_url, api_key)` for usage queries (native balance /
    /// coding-plan and the JS-script `{{apiKey}}`/`{{baseUrl}}` fallback)
    /// from the stored provider config.
    ///
    /// Each app persists credentials in a different shape, so callers must pass
    /// the owning app type. This mirrors the frontend `getProviderCredentials`
    /// in `UsageScriptModal.tsx`.
    pub fn resolve_usage_credentials(
        &self,
        app_type: &crate::app_config::AppType,
    ) -> (String, String) {
        use crate::app_config::AppType;

        let settings = &self.settings_config;
        let str_at =
            |value: Option<&Value>| value.and_then(|v| v.as_str()).unwrap_or("").to_string();

        // First present, non-empty string among `keys`, mirroring the frontend's
        // `a || b || c` — JS `||` skips empty strings, and presets seed fields like
        // `ANTHROPIC_AUTH_TOKEN` as present-but-empty placeholders, so a plain
        // `.get().or_else()` chain (which only skips *absent* keys) would stop short.
        fn first_non_empty(env: Option<&Value>, keys: &[&str]) -> String {
            let Some(env) = env else {
                return String::new();
            };
            for key in keys {
                if let Some(s) = env.get(key).and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        return s.to_string();
                    }
                }
            }
            String::new()
        }

        let (base_url, api_key) = match app_type {
            // Codex keeps its key in `auth.OPENAI_API_KEY` and its base URL
            // inside a TOML `config` string, not in an `env` map.
            AppType::Codex => {
                let auth = settings.get("auth");
                let config_text = settings.get("config").and_then(|v| v.as_str());
                let api_key = crate::codex_config::extract_codex_api_key(auth, config_text)
                    .unwrap_or_default();
                let base_url = config_text
                    .and_then(crate::codex_config::extract_codex_base_url)
                    .unwrap_or_default();
                (base_url, api_key)
            }
            // Gemini uses Google-specific env keys (with a legacy GOOGLE_API_KEY fallback).
            AppType::Gemini => {
                let env = settings.get("env");
                let base_url = str_at(env.and_then(|e| e.get("GOOGLE_GEMINI_BASE_URL")));
                let api_key = first_non_empty(env, &["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
                (base_url, api_key)
            }
            // Hermes (config.yaml) flattens credentials at the top level, snake_case.
            AppType::Hermes => (
                str_at(settings.get("base_url")),
                str_at(settings.get("api_key")),
            ),
            // OpenClaw (openclaw.json) flattens credentials at the top level, camelCase.
            AppType::OpenClaw => (
                str_at(settings.get("baseUrl")),
                str_at(settings.get("apiKey")),
            ),
            // OpenCode (OMO) nests credentials under `options` (the SDK options object).
            AppType::OpenCode => {
                let options = settings.get("options");
                (
                    str_at(options.and_then(|o| o.get("baseURL"))),
                    str_at(options.and_then(|o| o.get("apiKey"))),
                )
            }
            // Claude and Claude Desktop both use the Anthropic-style env map, keeping
            // the OpenRouter/Google key fallbacks the JS-script path relies on.
            // Listed explicitly (not `_`) so a new AppType fails to compile here.
            AppType::Claude | AppType::ClaudeDesktop => {
                let env = settings.get("env");
                let base_url = str_at(env.and_then(|e| e.get("ANTHROPIC_BASE_URL")));
                let api_key = first_non_empty(
                    env,
                    &[
                        "ANTHROPIC_AUTH_TOKEN",
                        "ANTHROPIC_API_KEY",
                        "OPENROUTER_API_KEY",
                        "GOOGLE_API_KEY",
                    ],
                );
                (base_url, api_key)
            }
        };

        // Normalize like the JS-script path (extract_base_url_from_provider) so a
        // future delegation from services/provider/usage.rs is behavior-preserving
        // and `{{baseUrl}}/path` concatenation never produces a double slash.
        (base_url.trim_end_matches('/').to_string(), api_key)
    }
}

/// 供应商管理器
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderManager {
    pub providers: IndexMap<String, Provider>,
    pub current: String,
}

/// 用量查询脚本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageScript {
    pub enabled: bool,
    pub language: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// 用量查询专用的 API Key（通用模板使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    /// 用量查询专用的 Base URL（通用和 NewAPI 模板使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    /// 访问令牌（用于需要登录的接口，NewAPI 模板使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "accessToken")]
    pub access_token: Option<String>,
    /// 用户ID（用于需要用户标识的接口，NewAPI 模板使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    /// 模板类型（用于后端判断验证规则）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "templateType")]
    pub template_type: Option<String>,
    /// 自动查询间隔（单位：分钟，0 表示禁用自动查询）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "autoQueryInterval")]
    pub auto_query_interval: Option<u64>,
    /// Coding Plan 供应商标识（如 "kimi", "zhipu", "minimax"）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "codingPlanProvider")]
    pub coding_plan_provider: Option<String>,
    /// 火山方舟控制面 OpenAPI 的 AccessKey ID（用量查询签名用，与推理 Key 是两套凭据）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "accessKeyId")]
    pub access_key_id: Option<String>,
    /// 火山方舟控制面 OpenAPI 的 SecretAccessKey（同上）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "secretAccessKey")]
    pub secret_access_key: Option<String>,
    /// 智谱团队套餐（Team Plan）的组织 ID（用量查询请求头 bigmodel-organization）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "teamOrganizationId")]
    pub team_organization_id: Option<String>,
    /// 智谱团队套餐（Team Plan）的项目 ID（用量查询请求头 bigmodel-project）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "teamProjectId")]
    pub team_project_id: Option<String>,
}

/// 用量数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "planName")]
    pub plan_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "isValid")]
    pub is_valid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "invalidMessage")]
    pub invalid_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// 用量查询结果（支持多套餐）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<UsageData>>, // 支持返回多个套餐
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 供应商单独的连通检测配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderTestConfig {
    /// 是否启用单独配置（false 时使用全局配置）
    #[serde(default)]
    pub enabled: bool,
    /// 超时时间（秒）
    #[serde(rename = "timeoutSecs", skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// 降级阈值（毫秒）
    #[serde(
        rename = "degradedThresholdMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub degraded_threshold_ms: Option<u64>,
    /// 最大重试次数
    #[serde(rename = "maxRetries", skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

/// 认证绑定来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthBindingSource {
    /// 从 provider 自身配置读取认证信息（默认）
    #[default]
    ProviderConfig,
    /// 使用托管账号认证（如 GitHub Copilot OAuth）
    ManagedAccount,
}

/// 通用认证绑定
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthBinding {
    /// 认证来源
    #[serde(default)]
    pub source: AuthBindingSource,
    /// 托管认证供应商标识（如 github_copilot）
    #[serde(rename = "authProvider", skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    /// 托管账号 ID；为空表示跟随该认证供应商的默认账号
    #[serde(rename = "accountId", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

/// Claude Desktop 3P 写入模式。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeDesktopMode {
    Direct,
    Proxy,
}

/// Claude Desktop 本地路由模式下暴露给 Desktop 的安全模型路由。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopModelRoute {
    /// 真实上游模型名，只保存在 CC Switch 内部，不写入 Claude Desktop profile。
    pub model: String,
    /// Claude Desktop 模型菜单显示名；写入 profile 的 `labelOverride`。
    #[serde(rename = "labelOverride", skip_serializing_if = "Option::is_none")]
    pub label_override: Option<String>,
    /// Claude Desktop 3P 识别的 1M 上下文能力标记。
    #[serde(rename = "supports1m", skip_serializing_if = "Option::is_none")]
    pub supports_1m: Option<bool>,
}

/// Codex Responses -> Chat Completions 的 reasoning 能力描述。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CodexChatReasoningConfig {
    #[serde(rename = "supportsThinking", skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(rename = "supportsEffort", skip_serializing_if = "Option::is_none")]
    pub supports_effort: Option<bool>,
    #[serde(rename = "thinkingParam", skip_serializing_if = "Option::is_none")]
    pub thinking_param: Option<String>,
    #[serde(rename = "effortParam", skip_serializing_if = "Option::is_none")]
    pub effort_param: Option<String>,
    #[serde(rename = "effortValueMode", skip_serializing_if = "Option::is_none")]
    pub effort_value_mode: Option<String>,
    /// 声明性字段：标注上游 reasoning 的回传位置（reasoning_content / reasoning /
    /// reasoning_details / think_tags）。当前响应侧 `extract_reasoning_field_text`
    /// 靠穷举字段提取、并不读取本字段；保留作文档说明与未来按格式分发（如 think_tags）的预留。
    #[serde(rename = "outputFormat", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
}

/// Local proxy request overrides applied after route/protocol transforms.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalProxyRequestOverrides {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl LocalProxyRequestOverrides {
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.body.is_none()
    }
}

/// 供应商元数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderMeta {
    /// 自定义端点列表（按 URL 去重存储）
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints: HashMap<String, crate::settings::CustomEndpoint>,
    /// 是否在写入 live 时应用通用配置片段
    #[serde(
        rename = "commonConfigEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub common_config_enabled: Option<bool>,
    /// Claude Desktop 3P 写入模式：direct（直连）或 proxy（预留）
    #[serde(rename = "claudeDesktopMode", skip_serializing_if = "Option::is_none")]
    pub claude_desktop_mode: Option<ClaudeDesktopMode>,
    /// Claude Desktop proxy 模式的模型路由映射：Claude-safe route -> upstream model。
    #[serde(
        default,
        rename = "claudeDesktopModelRoutes",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub claude_desktop_model_routes: HashMap<String, ClaudeDesktopModelRoute>,
    /// 用量查询脚本配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<UsageScript>,
    /// 请求地址管理：测速后自动选择最佳端点
    #[serde(rename = "endpointAutoSelect", skip_serializing_if = "Option::is_none")]
    pub endpoint_auto_select: Option<bool>,
    /// 合作伙伴标记（前端使用 isPartner，保持字段名一致）
    #[serde(rename = "isPartner", skip_serializing_if = "Option::is_none")]
    pub is_partner: Option<bool>,
    /// 合作伙伴促销 key，用于识别 PackyCode 等特殊供应商
    #[serde(
        rename = "partnerPromotionKey",
        skip_serializing_if = "Option::is_none"
    )]
    pub partner_promotion_key: Option<String>,
    /// 成本倍数（用于计算实际成本）
    #[serde(rename = "costMultiplier", skip_serializing_if = "Option::is_none")]
    pub cost_multiplier: Option<String>,
    /// 计费模式来源（response/request）
    #[serde(rename = "pricingModelSource", skip_serializing_if = "Option::is_none")]
    pub pricing_model_source: Option<String>,
    /// 每日消费限额（USD）
    #[serde(rename = "limitDailyUsd", skip_serializing_if = "Option::is_none")]
    pub limit_daily_usd: Option<String>,
    /// 每月消费限额（USD）
    #[serde(rename = "limitMonthlyUsd", skip_serializing_if = "Option::is_none")]
    pub limit_monthly_usd: Option<String>,
    /// 供应商单独的模型测试配置
    #[serde(rename = "testConfig", skip_serializing_if = "Option::is_none")]
    pub test_config: Option<ProviderTestConfig>,
    /// Claude API 格式（仅 Claude 供应商使用）
    /// - "anthropic": 原生 Anthropic Messages API，直接透传
    /// - "openai_chat": OpenAI Chat Completions 格式，需要转换
    /// - "openai_responses": OpenAI Responses API 格式，需要转换
    #[serde(rename = "apiFormat", skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    /// 通用认证绑定（provider_config / managed_account）
    ///
    /// 新代码应只写入该字段；githubAccountId 仅保留兼容读取。
    #[serde(rename = "authBinding", skip_serializing_if = "Option::is_none")]
    pub auth_binding: Option<AuthBinding>,
    /// Claude 认证字段名（"ANTHROPIC_AUTH_TOKEN" 或 "ANTHROPIC_API_KEY"）
    #[serde(rename = "apiKeyField", skip_serializing_if = "Option::is_none")]
    pub api_key_field: Option<String>,
    /// 是否将 base_url 视为完整 API 端点（不拼接 endpoint 路径）
    #[serde(rename = "isFullUrl", skip_serializing_if = "Option::is_none")]
    pub is_full_url: Option<bool>,
    /// Prompt cache key for OpenAI Responses-compatible endpoints.
    /// When set, injected into converted Responses requests to improve cache hit rate.
    /// If not set, Claude -> Responses conversions use a client-provided session/thread
    /// identity when available; generated session IDs are not sent upstream.
    #[serde(rename = "promptCacheKey", skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    /// Codex OAuth FAST mode: inject `service_tier = "priority"` for ChatGPT Codex requests.
    #[serde(rename = "codexFastMode", skip_serializing_if = "Option::is_none")]
    pub codex_fast_mode: Option<bool>,
    /// Codex Responses -> Chat Completions reasoning capability metadata.
    #[serde(rename = "codexChatReasoning", skip_serializing_if = "Option::is_none")]
    pub codex_chat_reasoning: Option<CodexChatReasoningConfig>,
    /// Custom User-Agent for local proxy routing.
    #[serde(rename = "customUserAgent", skip_serializing_if = "Option::is_none")]
    pub custom_user_agent: Option<String>,
    /// Local proxy request overrides applied to the transformed upstream request.
    #[serde(
        rename = "localProxyRequestOverrides",
        skip_serializing_if = "Option::is_none"
    )]
    pub local_proxy_request_overrides: Option<LocalProxyRequestOverrides>,
    /// 累加模式应用中，该 provider 是否已写入 live config。
    /// `None` 表示旧数据/未知状态，`Some(false)` 表示明确仅存在于数据库中。
    #[serde(rename = "liveConfigManaged", skip_serializing_if = "Option::is_none")]
    pub live_config_managed: Option<bool>,
    /// 供应商类型标识（用于特殊供应商检测）
    /// - "github_copilot": GitHub Copilot 供应商
    #[serde(rename = "providerType", skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    /// GitHub Copilot 关联账号 ID（仅 github_copilot 供应商使用）
    /// 用于多账号支持，关联到特定的 GitHub 账号
    #[serde(rename = "githubAccountId", skip_serializing_if = "Option::is_none")]
    pub github_account_id: Option<String>,
}

/// 解析 Provider 级自定义 User-Agent 字符串（单一真理来源）。
///
/// 转发（forwarder）、流式检测（stream_check）、获取模型列表（model_fetch）三条路径
/// 共用同一口径，避免出现"某条路径用了 UA、另一条没用 / 报错"的不一致。
///
/// 合法性由 `http::HeaderValue::from_str` 按**字节**判定（`b >= 32 && b != 127 || b == '\t'`），
/// 与前端 `src/lib/userAgent.ts::isValidUserAgentHeader` 严格一致：
/// - `Ok(None)`：未设置或纯空白（trim 后为空）。
/// - `Ok(Some(hv))`：合法。制表符、可见 ASCII（0x20–0x7E）、以及任意非 ASCII 字符
///   （UTF-8 字节均 ≥ 0x80）都合法。
/// - `Err(_)`：仅含控制字符时——除 `\t` 外的 0x00–0x1F（含换行）与 0x7F（DEL）。
///
/// 非法值的处理：三条运行时路径**均静默忽略**（`.ok().flatten()`，绝不让某条路径报错而
/// 另一条放行）；前端在输入框处给出非阻断提示。当前**不在保存时阻断**——deeplink 导入等
/// 非表单路径应宽容，运行时静默忽略即为安全网。
pub fn parse_custom_user_agent(
    raw: Option<&str>,
) -> Result<Option<HeaderValue>, InvalidHeaderValue> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(ua) => HeaderValue::from_str(ua).map(Some),
        None => Ok(None),
    }
}

impl ProviderMeta {
    /// Codex OAuth FAST mode 是否启用。默认关闭，因为 `service_tier="priority"`
    /// 会按更高速率消耗 ChatGPT 订阅配额，用户需显式开启以换取更低延迟。
    pub fn codex_fast_mode_enabled(&self) -> bool {
        self.codex_fast_mode.unwrap_or(false)
    }

    /// 经校验的 Provider 级自定义 User-Agent。见 [`parse_custom_user_agent`]。
    pub fn custom_user_agent_header(&self) -> Result<Option<HeaderValue>, InvalidHeaderValue> {
        parse_custom_user_agent(self.custom_user_agent.as_deref())
    }

    /// 解析指定托管认证供应商绑定的账号 ID。
    ///
    /// 新版优先读取 authBinding，旧版继续兼容 githubAccountId。
    pub fn managed_account_id_for(&self, auth_provider: &str) -> Option<String> {
        if let Some(binding) = self.auth_binding.as_ref() {
            if binding.source == AuthBindingSource::ManagedAccount
                && binding.auth_provider.as_deref() == Some(auth_provider)
            {
                return binding.account_id.clone();
            }
        }

        if auth_provider == "github_copilot" {
            return self.github_account_id.clone();
        }

        None
    }
}

impl ProviderManager {
    /// 获取所有供应商
    pub fn get_all_providers(&self) -> &IndexMap<String, Provider> {
        &self.providers
    }
}

// ============================================================================
// 统一供应商（Universal Provider）- 跨应用共享配置
// ============================================================================

/// 统一供应商的应用启用状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UniversalProviderApps {
    #[serde(default)]
    pub claude: bool,
    #[serde(default)]
    pub codex: bool,
    #[serde(default)]
    pub gemini: bool,
}

/// Claude 模型配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaudeModelConfig {
    /// 主模型
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Haiku 默认模型
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "haikuModel")]
    pub haiku_model: Option<String>,
    /// Sonnet 默认模型
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "sonnetModel")]
    pub sonnet_model: Option<String>,
    /// Opus 默认模型
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "opusModel")]
    pub opus_model: Option<String>,
}

/// Codex 模型配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexModelConfig {
    /// 模型名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 推理强度
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
}

/// Gemini 模型配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeminiModelConfig {
    /// 模型名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// 各应用的模型配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UniversalProviderModels {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude: Option<ClaudeModelConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexModelConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<GeminiModelConfig>,
}

/// 统一供应商（跨应用共享配置）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalProvider {
    /// 唯一标识
    pub id: String,
    /// 供应商名称
    pub name: String,
    /// 供应商类型（如 "newapi", "custom"）
    #[serde(rename = "providerType")]
    pub provider_type: String,
    /// 应用启用状态
    pub apps: UniversalProviderApps,
    /// API 基础地址
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    /// API 密钥
    #[serde(rename = "apiKey")]
    pub api_key: String,
    /// 各应用的模型配置
    #[serde(default)]
    pub models: UniversalProviderModels,
    /// 网站链接
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "websiteUrl")]
    pub website_url: Option<String>,
    /// 备注信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// 图标名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// 图标颜色
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "iconColor")]
    pub icon_color: Option<String>,
    /// 元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
    /// 创建时间戳
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "createdAt")]
    pub created_at: Option<i64>,
    /// 排序索引
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "sortIndex")]
    pub sort_index: Option<usize>,
}

impl UniversalProvider {
    /// 创建新的统一供应商
    pub fn new(
        id: String,
        name: String,
        provider_type: String,
        base_url: String,
        api_key: String,
    ) -> Self {
        Self {
            id,
            name,
            provider_type,
            apps: UniversalProviderApps::default(),
            base_url,
            api_key,
            models: UniversalProviderModels::default(),
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            meta: None,
            created_at: Some(chrono::Utc::now().timestamp_millis()),
            sort_index: None,
        }
    }

    /// 生成 Claude 供应商配置
    pub fn to_claude_provider(&self) -> Option<Provider> {
        if !self.apps.claude {
            return None;
        }

        let models = self.models.claude.as_ref();
        let model = models
            .and_then(|m| m.model.clone())
            .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
        let haiku = models
            .and_then(|m| m.haiku_model.clone())
            .unwrap_or_else(|| model.clone());
        let sonnet = models
            .and_then(|m| m.sonnet_model.clone())
            .unwrap_or_else(|| model.clone());
        let opus = models
            .and_then(|m| m.opus_model.clone())
            .unwrap_or_else(|| model.clone());

        let settings_config = serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": self.base_url,
                "ANTHROPIC_AUTH_TOKEN": self.api_key,
                "ANTHROPIC_MODEL": model,
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": haiku,
                "ANTHROPIC_DEFAULT_SONNET_MODEL": sonnet,
                "ANTHROPIC_DEFAULT_OPUS_MODEL": opus,
            }
        });

        Some(Provider {
            id: format!("universal-claude-{}", self.id),
            name: self.name.clone(),
            settings_config,
            website_url: self.website_url.clone(),
            category: Some("aggregator".to_string()),
            created_at: self.created_at,
            sort_index: self.sort_index,
            notes: self.notes.clone(),
            meta: self.meta.clone(),
            icon: self.icon.clone(),
            icon_color: self.icon_color.clone(),
            in_failover_queue: false,
        })
    }

    /// 生成 Codex 供应商配置
    pub fn to_codex_provider(&self) -> Option<Provider> {
        if !self.apps.codex {
            return None;
        }

        let models = self.models.codex.as_ref();
        let model = models
            .and_then(|m| m.model.clone())
            .unwrap_or_else(|| "gpt-4o".to_string());
        let reasoning_effort = models
            .and_then(|m| m.reasoning_effort.clone())
            .unwrap_or_else(|| "high".to_string());

        // Codex/OpenAI 的 base_url 既可能是纯 origin（需要补 /v1），也可能包含自定义前缀（不应强行补版本）
        let base_trimmed = self.base_url.trim_end_matches('/');
        let origin_only = match base_trimmed.split_once("://") {
            Some((_scheme, rest)) => !rest.contains('/'),
            None => !base_trimmed.contains('/'),
        };
        let codex_base_url = if base_trimmed.ends_with("/v1") {
            base_trimmed.to_string()
        } else if origin_only {
            format!("{base_trimmed}/v1")
        } else {
            base_trimmed.to_string()
        };

        // 生成 Codex 的 config.toml 内容
        let config_toml = format!(
            r#"model_provider = "custom"
model = "{model}"
model_reasoning_effort = "{reasoning_effort}"
disable_response_storage = true

[model_providers.custom]
name = "NewAPI"
base_url = "{codex_base_url}"
wire_api = "responses"
requires_openai_auth = true"#
        );

        let settings_config = serde_json::json!({
            "auth": {
                "OPENAI_API_KEY": self.api_key
            },
            "config": config_toml
        });

        Some(Provider {
            id: format!("universal-codex-{}", self.id),
            name: self.name.clone(),
            settings_config,
            website_url: self.website_url.clone(),
            category: Some("aggregator".to_string()),
            created_at: self.created_at,
            sort_index: self.sort_index,
            notes: self.notes.clone(),
            meta: self.meta.clone(),
            icon: self.icon.clone(),
            icon_color: self.icon_color.clone(),
            in_failover_queue: false,
        })
    }

    /// 生成 Gemini 供应商配置
    pub fn to_gemini_provider(&self) -> Option<Provider> {
        if !self.apps.gemini {
            return None;
        }

        let models = self.models.gemini.as_ref();
        let model = models
            .and_then(|m| m.model.clone())
            .unwrap_or_else(|| "gemini-2.5-pro".to_string());

        let settings_config = serde_json::json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": self.base_url,
                "GEMINI_API_KEY": self.api_key,
                "GEMINI_MODEL": model,
            }
        });

        Some(Provider {
            id: format!("universal-gemini-{}", self.id),
            name: self.name.clone(),
            settings_config,
            website_url: self.website_url.clone(),
            category: Some("aggregator".to_string()),
            created_at: self.created_at,
            sort_index: self.sort_index,
            notes: self.notes.clone(),
            meta: self.meta.clone(),
            icon: self.icon.clone(),
            icon_color: self.icon_color.clone(),
            in_failover_queue: false,
        })
    }
}

// ============================================================================
// OpenCode 供应商配置结构
// ============================================================================

/// OpenCode 供应商的 settings_config 结构
///
/// OpenCode 使用 AI SDK 包名来指定供应商类型，与其他应用的配置格式不同。
/// 配置示例：
/// ```json
/// {
///   "npm": "@ai-sdk/openai-compatible",
///   "options": { "baseURL": "https://api.example.com/v1", "apiKey": "sk-xxx" },
///   "models": { "gpt-4o": { "name": "GPT-4o" } }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeProviderConfig {
    /// AI SDK 包名，如 "@ai-sdk/openai-compatible", "@ai-sdk/anthropic"
    pub npm: String,

    /// 供应商名称（可选，用于显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 供应商选项（API 密钥、基础 URL 等）
    #[serde(default)]
    pub options: OpenCodeProviderOptions,

    /// 模型定义映射
    #[serde(default)]
    pub models: HashMap<String, OpenCodeModel>,
}

impl Default for OpenCodeProviderConfig {
    fn default() -> Self {
        Self {
            npm: "@ai-sdk/openai-compatible".to_string(),
            name: None,
            options: OpenCodeProviderOptions::default(),
            models: HashMap::new(),
        }
    }
}

/// OpenCode 供应商选项
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenCodeProviderOptions {
    /// API 基础 URL
    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API 密钥（支持环境变量引用，如 "{env:API_KEY}"）
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// 自定义请求头
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    /// 额外选项（timeout, setCacheKey 等）
    /// 使用 flatten 捕获所有未明确定义的字段
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

/// OpenCode 模型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModel {
    /// 模型显示名称
    pub name: String,

    /// 模型限制（上下文和输出 token 数）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<OpenCodeModelLimit>,

    /// 模型额外选项（provider 路由等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<HashMap<String, Value>>,

    /// 额外字段（cost、modalities、thinking、variants 等）
    /// 使用 flatten 捕获所有未明确定义的字段
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

/// OpenCode 模型限制
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenCodeModelLimit {
    /// 上下文 token 限制
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,

    /// 输出 token 限制
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeModelConfig, CodexModelConfig, GeminiModelConfig, LocalProxyRequestOverrides,
        OpenCodeProviderConfig, Provider, ProviderManager, ProviderMeta, UniversalProvider,
    };
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn provider_meta_serializes_pricing_model_source() {
        let meta = ProviderMeta {
            pricing_model_source: Some("response".to_string()),
            ..ProviderMeta::default()
        };

        let value = serde_json::to_value(&meta).expect("serialize ProviderMeta");

        assert_eq!(
            value
                .get("pricingModelSource")
                .and_then(|item| item.as_str()),
            Some("response")
        );
        assert!(value.get("pricing_model_source").is_none());
    }

    #[test]
    fn provider_meta_omits_pricing_model_source_when_none() {
        let meta = ProviderMeta::default();
        let value = serde_json::to_value(&meta).expect("serialize ProviderMeta");

        assert!(value.get("pricingModelSource").is_none());
    }

    #[test]
    fn provider_meta_roundtrips_local_proxy_request_overrides() {
        let meta = ProviderMeta {
            local_proxy_request_overrides: Some(LocalProxyRequestOverrides {
                headers: HashMap::from([("X-Test".to_string(), "yes".to_string())]),
                body: Some(json!({ "temperature": 0.2 })),
            }),
            ..ProviderMeta::default()
        };

        let value = serde_json::to_value(&meta).expect("serialize ProviderMeta");
        assert_eq!(
            value["localProxyRequestOverrides"]["headers"]["X-Test"],
            "yes"
        );
        assert_eq!(
            value["localProxyRequestOverrides"]["body"]["temperature"],
            0.2
        );

        let decoded: ProviderMeta =
            serde_json::from_value(value).expect("deserialize ProviderMeta");
        let overrides = decoded.local_proxy_request_overrides.unwrap();
        assert_eq!(overrides.headers.get("X-Test"), Some(&"yes".to_string()));
        assert_eq!(overrides.body.unwrap()["temperature"], 0.2);
    }

    #[test]
    fn provider_with_id_populates_defaults() {
        let settings_config = json!({
            "env": { "API_KEY": "test" }
        });
        let provider = Provider::with_id(
            "provider-1".to_string(),
            "Provider".to_string(),
            settings_config.clone(),
            Some("https://example.com".to_string()),
        );

        assert_eq!(provider.id, "provider-1");
        assert_eq!(provider.name, "Provider");
        assert_eq!(provider.settings_config, settings_config);
        assert_eq!(provider.website_url.as_deref(), Some("https://example.com"));
        assert!(provider.category.is_none());
        assert!(provider.created_at.is_none());
        assert!(provider.sort_index.is_none());
        assert!(provider.notes.is_none());
        assert!(provider.meta.is_none());
        assert!(provider.icon.is_none());
        assert!(provider.icon_color.is_none());
        assert!(!provider.in_failover_queue);
    }

    #[test]
    fn provider_managed_account_auth_detection_uses_type_or_known_endpoint() {
        let mut copilot = Provider::with_id(
            "copilot".to_string(),
            "Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            None,
        );
        assert!(copilot.is_github_copilot());
        assert!(copilot.uses_managed_account_auth());

        let mut codex = Provider::with_id(
            "codex".to_string(),
            "Codex".to_string(),
            json!({ "env": {} }),
            None,
        );
        codex.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        assert!(codex.is_codex_oauth());
        assert!(codex.uses_managed_account_auth());

        let codex_endpoint = Provider::with_id(
            "codex-endpoint".to_string(),
            "Codex Endpoint".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            None,
        );
        assert!(codex_endpoint.uses_managed_account_auth());

        copilot.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });
        assert!(copilot.is_github_copilot());
    }

    #[test]
    fn provider_manager_get_all_providers_returns_map() {
        let mut manager = ProviderManager::default();
        let provider = Provider::with_id(
            "provider-1".to_string(),
            "Provider".to_string(),
            json!({ "env": {} }),
            None,
        );
        manager.providers.insert("provider-1".to_string(), provider);

        assert_eq!(manager.get_all_providers().len(), 1);
        assert!(manager.get_all_providers().contains_key("provider-1"));
    }

    #[test]
    fn universal_provider_to_claude_provider_uses_models() {
        let mut universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );
        universal.apps.claude = true;
        universal.models.claude = Some(ClaudeModelConfig {
            model: Some("claude-main".to_string()),
            haiku_model: Some("claude-haiku".to_string()),
            sonnet_model: Some("claude-sonnet".to_string()),
            opus_model: Some("claude-opus".to_string()),
        });

        let provider = universal.to_claude_provider().expect("claude provider");

        assert_eq!(provider.id, "universal-claude-u1");
        assert_eq!(provider.name, "Universal");
        assert_eq!(provider.category.as_deref(), Some("aggregator"));
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_MODEL")
                .and_then(|item| item.as_str()),
            Some("claude-main")
        );
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|item| item.as_str()),
            Some("claude-haiku")
        );
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|item| item.as_str()),
            Some("claude-sonnet")
        );
        assert_eq!(
            provider
                .settings_config
                .pointer("/env/ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|item| item.as_str()),
            Some("claude-opus")
        );
    }

    #[test]
    fn universal_provider_to_claude_provider_disabled_returns_none() {
        let universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );

        assert!(universal.to_claude_provider().is_none());
    }

    #[test]
    fn universal_provider_to_codex_provider_appends_v1() {
        let mut universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );
        universal.apps.codex = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("gpt-4o-mini".to_string()),
            reasoning_effort: Some("low".to_string()),
        });

        let provider = universal.to_codex_provider().expect("codex provider");
        let config = provider
            .settings_config
            .get("config")
            .and_then(|item| item.as_str())
            .expect("config toml");

        assert!(config.contains("base_url = \"https://api.example.com/v1\""));
        assert_eq!(
            provider
                .settings_config
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(|item| item.as_str()),
            Some("api-key")
        );
    }

    #[test]
    fn universal_provider_to_codex_provider_keeps_v1_suffix() {
        let mut universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com/v1".to_string(),
            "api-key".to_string(),
        );
        universal.apps.codex = true;

        let provider = universal.to_codex_provider().expect("codex provider");
        let config = provider
            .settings_config
            .get("config")
            .and_then(|item| item.as_str())
            .expect("config toml");

        assert!(config.contains("base_url = \"https://api.example.com/v1\""));
    }

    #[test]
    fn universal_provider_to_codex_provider_disabled_returns_none() {
        let universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );

        assert!(universal.to_codex_provider().is_none());
    }

    #[test]
    fn universal_provider_to_gemini_provider_defaults_model() {
        let mut universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );
        universal.apps.gemini = true;

        let provider = universal.to_gemini_provider().expect("gemini provider");

        assert_eq!(
            provider
                .settings_config
                .pointer("/env/GEMINI_MODEL")
                .and_then(|item| item.as_str()),
            Some("gemini-2.5-pro")
        );
    }

    #[test]
    fn universal_provider_to_gemini_provider_uses_model() {
        let mut universal = UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "api-key".to_string(),
        );
        universal.apps.gemini = true;
        universal.models.gemini = Some(GeminiModelConfig {
            model: Some("gemini-custom".to_string()),
        });

        let provider = universal.to_gemini_provider().expect("gemini provider");

        assert_eq!(
            provider
                .settings_config
                .pointer("/env/GEMINI_MODEL")
                .and_then(|item| item.as_str()),
            Some("gemini-custom")
        );
    }

    #[test]
    fn opencode_provider_config_defaults() {
        let config = OpenCodeProviderConfig::default();
        assert_eq!(config.npm, "@ai-sdk/openai-compatible");
        assert!(config.name.is_none());
        assert!(config.models.is_empty());
        assert!(config.options.base_url.is_none());
        assert!(config.options.api_key.is_none());
        assert!(config.options.headers.is_none());
        assert!(config.options.extra.is_empty());
    }

    #[test]
    fn universal_codex_provider_origin_base_url_adds_v1() {
        let mut p = UniversalProvider::new(
            "id".to_string(),
            "Test".to_string(),
            "custom".to_string(),
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
        );
        p.apps.codex = true;

        let provider = p.to_codex_provider().expect("should build codex provider");
        let toml = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config should be a toml string");

        assert!(toml.contains("base_url = \"https://api.openai.com/v1\""));
    }

    #[test]
    fn universal_codex_provider_custom_prefix_does_not_force_v1() {
        let mut p = UniversalProvider::new(
            "id".to_string(),
            "Test".to_string(),
            "custom".to_string(),
            "https://example.com/openai".to_string(),
            "sk-test".to_string(),
        );
        p.apps.codex = true;

        let provider = p.to_codex_provider().expect("should build codex provider");
        let toml = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config should be a toml string");

        assert!(toml.contains("base_url = \"https://example.com/openai\""));
        assert!(!toml.contains("https://example.com/openai/v1"));
    }

    // ── resolve_usage_credentials (per-app credential extraction) ──

    use crate::app_config::AppType;

    fn provider_with(settings_config: serde_json::Value) -> Provider {
        Provider::with_id("p".to_string(), "P".to_string(), settings_config, None)
    }

    #[test]
    fn resolve_credentials_claude_env() {
        let p = provider_with(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-claude",
            }
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::Claude),
            (
                "https://api.deepseek.com/anthropic".to_string(),
                "sk-claude".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_claude_openrouter_fallback() {
        // OpenRouter-on-Claude keeps its key in OPENROUTER_API_KEY; the superset
        // fallback must still find it (regression guard for the per-app refactor).
        let p = provider_with(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1",
                "OPENROUTER_API_KEY": "sk-or",
            }
        }));
        let (base_url, api_key) = p.resolve_usage_credentials(&AppType::Claude);
        assert_eq!(base_url, "https://openrouter.ai/api/v1");
        assert_eq!(api_key, "sk-or");
    }

    #[test]
    fn resolve_credentials_codex_auth_and_toml() {
        let p = provider_with(json!({
            "auth": { "OPENAI_API_KEY": "sk-codex" },
            "config": "model_provider = \"deepseek\"\n\
                       [model_providers.deepseek]\n\
                       base_url = \"https://api.deepseek.com\"\n",
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::Codex),
            (
                "https://api.deepseek.com".to_string(),
                "sk-codex".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_gemini_env_with_google_fallback() {
        let p = provider_with(json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com",
                "GOOGLE_API_KEY": "g-legacy",
            }
        }));
        let (base_url, api_key) = p.resolve_usage_credentials(&AppType::Gemini);
        assert_eq!(base_url, "https://generativelanguage.googleapis.com");
        assert_eq!(api_key, "g-legacy");
    }

    #[test]
    fn resolve_credentials_claude_skips_empty_primary_key() {
        // Presets seed ANTHROPIC_AUTH_TOKEN as a present-but-empty placeholder.
        // The fallback chain must skip empty values (matching the frontend's
        // `a || b` semantics), not just absent keys.
        let p = provider_with(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1",
                "ANTHROPIC_AUTH_TOKEN": "",
                "ANTHROPIC_API_KEY": "",
                "OPENROUTER_API_KEY": "sk-or",
            }
        }));
        let (_, api_key) = p.resolve_usage_credentials(&AppType::Claude);
        assert_eq!(api_key, "sk-or");
    }

    #[test]
    fn resolve_credentials_gemini_skips_empty_primary_key() {
        let p = provider_with(json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com",
                "GEMINI_API_KEY": "",
                "GOOGLE_API_KEY": "g-real",
            }
        }));
        let (_, api_key) = p.resolve_usage_credentials(&AppType::Gemini);
        assert_eq!(api_key, "g-real");
    }

    #[test]
    fn resolve_credentials_hermes_snake_case() {
        let p = provider_with(json!({
            "base_url": "https://api.deepseek.com",
            "api_key": "sk-hermes",
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::Hermes),
            (
                "https://api.deepseek.com".to_string(),
                "sk-hermes".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_openclaw_camel_case() {
        let p = provider_with(json!({
            "baseUrl": "https://api.deepseek.com",
            "apiKey": "sk-openclaw",
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::OpenClaw),
            (
                "https://api.deepseek.com".to_string(),
                "sk-openclaw".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_opencode_options() {
        // OpenCode (OMO) nests creds under options.{baseURL,apiKey}; useOpencodeFormState
        // writes config.options.apiKey, so the stored provider keeps them there.
        let p = provider_with(json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": {
                "baseURL": "https://api.deepseek.com/v1",
                "apiKey": "sk-opencode",
                "setCacheKey": true,
            }
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::OpenCode),
            (
                "https://api.deepseek.com/v1".to_string(),
                "sk-opencode".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_claude_desktop_uses_env() {
        // ClaudeDesktop persists the Anthropic env shape (ClaudeDesktopProviderForm
        // reads env.ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN), so it resolves via
        // the default env branch — it is NOT unsupported.
        let p = provider_with(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-desktop",
            }
        }));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::ClaudeDesktop),
            (
                "https://api.deepseek.com/anthropic".to_string(),
                "sk-desktop".to_string()
            )
        );
    }

    #[test]
    fn resolve_credentials_trims_trailing_slash_on_base_url() {
        let p = provider_with(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic/",
                "ANTHROPIC_AUTH_TOKEN": "sk-claude",
            }
        }));
        let (base_url, _) = p.resolve_usage_credentials(&AppType::Claude);
        assert_eq!(base_url, "https://api.deepseek.com/anthropic");
    }

    #[test]
    fn resolve_credentials_missing_fields_yield_empty() {
        let p = provider_with(json!({}));
        assert_eq!(
            p.resolve_usage_credentials(&AppType::Claude),
            (String::new(), String::new())
        );
    }
}
