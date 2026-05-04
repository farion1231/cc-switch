//! Claude (Anthropic) Provider Adapter
//!
//! 支持透传模式和 OpenAI 格式转换模式
//!
//! ## API 格式
//! - **anthropic** (默认): Anthropic Messages API 格式，直接透传
//! - **openai_chat**: OpenAI Chat Completions 格式，需要 Anthropic ↔ OpenAI 转换
//! - **openai_responses**: OpenAI Responses API 格式，需要 Anthropic ↔ Responses 转换
//! - **gemini_native**: Google Gemini Native generateContent 格式，需要 Anthropic ↔ Gemini 转换
//!
//! ## 认证模式
//! - **Claude**: Anthropic 官方 API (x-api-key + anthropic-version)
//! - **ClaudeAuth**: 中转服务 (仅 Bearer 认证，无 x-api-key)
//! - **OpenRouter**: 已支持 Claude Code 兼容接口，默认透传
//! - **GitHubCopilot**: GitHub Copilot (OAuth + Copilot Token)

use super::{AuthInfo, AuthStrategy, ProviderAdapter, ProviderType};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;

/// 获取 Claude 供应商的 API 格式
///
/// 供 handler/forwarder 外部使用的公开函数。
/// 优先级：meta.apiFormat > settings_config.api_format > openrouter_compat_mode > 默认 "anthropic"
pub fn get_claude_api_format(provider: &Provider) -> &'static str {
    // 0) Codex OAuth 强制使用 openai_responses（不可被覆盖）
    if let Some(meta) = provider.meta.as_ref() {
        if meta.provider_type.as_deref() == Some("codex_oauth") {
            return "openai_responses";
        }
    }

    // 1) Preferred: meta.apiFormat (SSOT, never written to Claude Code config)
    if let Some(meta) = provider.meta.as_ref() {
        if let Some(api_format) = meta.api_format.as_deref() {
            return match api_format {
                "openai_chat" => "openai_chat",
                "openai_responses" => "openai_responses",
                "gemini_native" => "gemini_native",
                _ => "anthropic",
            };
        }
    }

    // 2) Backward compatibility: legacy settings_config.api_format
    if let Some(api_format) = provider
        .settings_config
        .get("api_format")
        .and_then(|v| v.as_str())
    {
        return match api_format {
            "openai_chat" => "openai_chat",
            "openai_responses" => "openai_responses",
            "gemini_native" => "gemini_native",
            _ => "anthropic",
        };
    }

    // 3) Backward compatibility: legacy openrouter_compat_mode (bool/number/string)
    let raw = provider.settings_config.get("openrouter_compat_mode");
    let enabled = match raw {
        Some(serde_json::Value::Bool(v)) => *v,
        Some(serde_json::Value::Number(num)) => num.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(value)) => {
            let normalized = value.trim().to_lowercase();
            normalized == "true" || normalized == "1"
        }
        _ => false,
    };

    if enabled {
        "openai_chat"
    } else {
        "anthropic"
    }
}

pub fn claude_api_format_needs_transform(api_format: &str) -> bool {
    matches!(
        api_format,
        "openai_chat" | "openai_responses" | "gemini_native"
    )
}

fn is_moonshot_or_kimi_identifier(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("moonshot") || value.contains("kimi")
}

fn should_preserve_reasoning_content_for_openai_chat(
    provider: &Provider,
    body: &serde_json::Value,
) -> bool {
    if body
        .get("model")
        .and_then(|m| m.as_str())
        .is_some_and(is_moonshot_or_kimi_identifier)
    {
        return true;
    }

    let settings = &provider.settings_config;
    let base_urls = [
        settings
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str()),
        settings.get("base_url").and_then(|v| v.as_str()),
        settings.get("baseURL").and_then(|v| v.as_str()),
        settings.get("apiEndpoint").and_then(|v| v.as_str()),
    ];

    base_urls
        .into_iter()
        .flatten()
        .any(is_moonshot_or_kimi_identifier)
}

pub fn transform_claude_request_for_api_format(
    body: serde_json::Value,
    provider: &Provider,
    api_format: &str,
    session_id: Option<&str>,
    shadow_store: Option<&super::gemini_shadow::GeminiShadowStore>,
) -> Result<serde_json::Value, ProxyError> {
    let is_codex_oauth = provider.is_codex_oauth();

    // Copilot 场景：优先从 metadata.user_id 提取 session ID 作为 cache key
    // 格式: "uuid_sessionId" → 提取 "_" 后面的部分作为 session 标识
    // 同一会话的请求共享 cache key，提升 Copilot 缓存命中率
    let is_copilot = provider
        .meta
        .as_ref()
        .and_then(|m| m.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .is_some_and(|u| u.contains("githubcopilot.com"));
    let session_cache_key: Option<String> = if is_copilot {
        let metadata = body.get("metadata");
        // Session 提取优先级（与 forwarder 和 session.rs 统一）：
        //   1. metadata.user_id 中的 _session_ 后缀
        //   2. metadata.session_id（直接字段）
        metadata
            .and_then(|m| m.get("user_id"))
            .and_then(|v| v.as_str())
            .and_then(super::super::session::parse_session_from_user_id)
            .or_else(|| {
                metadata
                    .and_then(|m| m.get("session_id"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
    } else if is_codex_oauth {
        session_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    } else {
        None
    };

    let explicit_cache_key = provider
        .meta
        .as_ref()
        .and_then(|m| m.prompt_cache_key.as_deref());
    let cache_key = if is_codex_oauth {
        explicit_cache_key
            .or(session_cache_key.as_deref())
            .unwrap_or(&provider.id)
    } else {
        session_cache_key
            .as_deref()
            .or(explicit_cache_key)
            .unwrap_or(&provider.id)
    };
    match api_format {
        "openai_responses" => {
            // Codex OAuth (ChatGPT Plus/Pro 反代) 需要在请求体里强制 store: false
            // + include: ["reasoning.encrypted_content"]，由 transform 层统一处理。
            let codex_fast_mode = provider.codex_fast_mode_enabled();
            super::transform_responses::anthropic_to_responses(
                body,
                Some(cache_key),
                is_codex_oauth,
                codex_fast_mode,
            )
        }
        "openai_chat" => {
            let preserve_reasoning_content =
                should_preserve_reasoning_content_for_openai_chat(provider, &body);
            let mut result = super::transform::anthropic_to_openai_with_reasoning_content(
                body,
                preserve_reasoning_content,
            )?;
            // Inject prompt_cache_key only if explicitly configured in meta
            if let Some(key) = provider
                .meta
                .as_ref()
                .and_then(|m| m.prompt_cache_key.as_deref())
            {
                result["prompt_cache_key"] = serde_json::json!(key);
            }
            Ok(result)
        }
        "gemini_native" => super::transform_gemini::anthropic_to_gemini_with_shadow(
            body,
            shadow_store,
            Some(&provider.id),
            session_id,
        ),
        _ => Ok(body),
    }
}

/// Claude 适配器
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 获取供应商类型
    ///
    /// 根据 base_url 和 auth_mode 检测具体的供应商类型：
    /// - GitHubCopilot: meta.provider_type 为 github_copilot 或 base_url 包含 githubcopilot.com
    /// - CodexOAuth: meta.provider_type 为 codex_oauth
    /// - OpenRouter: base_url 包含 openrouter.ai
    /// - ClaudeAuth: auth_mode 为 bearer_only
    /// - Claude: 默认 Anthropic 官方
    pub fn provider_type(&self, provider: &Provider) -> ProviderType {
        // 检测 Gemini Native 格式
        if self.get_api_format(provider) == "gemini_native" {
            return match self.extract_key(provider) {
                Some(key) if key.starts_with("ya29.") || key.starts_with('{') => {
                    ProviderType::GeminiCli
                }
                _ => ProviderType::Gemini,
            };
        }

        // 检测 Codex OAuth (ChatGPT Plus/Pro)
        if self.is_codex_oauth(provider) {
            return ProviderType::CodexOAuth;
        }

        // 检测 GitHub Copilot
        if self.is_github_copilot(provider) {
            return ProviderType::GitHubCopilot;
        }

        // 检测 OpenRouter
        if self.is_openrouter(provider) {
            return ProviderType::OpenRouter;
        }

        // 检测 ClaudeAuth (仅 Bearer 认证)
        if self.is_bearer_only_mode(provider) {
            return ProviderType::ClaudeAuth;
        }

        ProviderType::Claude
    }

    /// 检测是否为 Codex OAuth 供应商（ChatGPT Plus/Pro 反代）
    fn is_codex_oauth(&self, provider: &Provider) -> bool {
        if let Some(meta) = provider.meta.as_ref() {
            if meta.provider_type.as_deref() == Some("codex_oauth") {
                return true;
            }
        }
        false
    }

    /// 检测是否为 GitHub Copilot 供应商
    fn is_github_copilot(&self, provider: &Provider) -> bool {
        // 方式1: 检查 meta.provider_type
        if let Some(meta) = provider.meta.as_ref() {
            if meta.provider_type.as_deref() == Some("github_copilot") {
                return true;
            }
        }

        // 方式2: 检查 base_url（兼容旧数据的 fallback，后续应优先依赖 providerType）
        if let Ok(base_url) = self.extract_base_url(provider) {
            if base_url.contains("githubcopilot.com") {
                return true;
            }
        }

        false
    }

    /// 检测是否使用 OpenRouter
    fn is_openrouter(&self, provider: &Provider) -> bool {
        if let Ok(base_url) = self.extract_base_url(provider) {
            return base_url.contains("openrouter.ai");
        }
        false
    }

    /// 获取 API 格式
    ///
    /// 从 provider.meta.api_format 读取格式设置：
    /// - "anthropic" (默认): Anthropic Messages API 格式，直接透传
    /// - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
    /// - "openai_responses": OpenAI Responses API 格式，需要格式转换
    fn get_api_format(&self, provider: &Provider) -> &'static str {
        get_claude_api_format(provider)
    }

    /// 检测是否为仅 Bearer 认证模式
    fn is_bearer_only_mode(&self, provider: &Provider) -> bool {
        // 检查 settings_config 中的 auth_mode
        if let Some(auth_mode) = provider
            .settings_config
            .get("auth_mode")
            .and_then(|v| v.as_str())
        {
            if auth_mode == "bearer_only" {
                return true;
            }
        }

        // 检查 env 中的 AUTH_MODE
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(auth_mode) = env.get("AUTH_MODE").and_then(|v| v.as_str()) {
                if auth_mode == "bearer_only" {
                    return true;
                }
            }
        }

        false
    }

    /// 从 Provider 配置中提取 API Key
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        if let Some(env) = provider.settings_config.get("env") {
            // Anthropic 标准 key
            if let Some(key) = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 ANTHROPIC_AUTH_TOKEN");
                return Some(key.to_string());
            }
            if let Some(key) = env
                .get("ANTHROPIC_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 ANTHROPIC_API_KEY");
                return Some(key.to_string());
            }
            // OpenRouter key
            if let Some(key) = env
                .get("OPENROUTER_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 OPENROUTER_API_KEY");
                return Some(key.to_string());
            }
            // 备选 OpenAI key (用于 OpenRouter)
            if let Some(key) = env
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 OPENAI_API_KEY");
                return Some(key.to_string());
            }
        }

        // 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            log::debug!("[Claude] 使用 apiKey/api_key");
            return Some(key.to_string());
        }

        log::warn!("[Claude] 未找到有效的 API Key");
        None
    }

    /// 根据 env 中填写的变量名推断 Anthropic 默认走哪种鉴权策略。
    ///
    /// 与 Anthropic SDK 原生语义保持一致：
    /// - `ANTHROPIC_AUTH_TOKEN` → `ClaudeAuth`（发送 `Authorization: Bearer`）
    /// - `ANTHROPIC_API_KEY`    → `Anthropic` （发送 `x-api-key`）
    ///
    /// 优先级与 [`extract_key`] 一致；两者都缺时返回 `None` 由调用方决定 fallback。
    fn infer_anthropic_auth_strategy(&self, provider: &Provider) -> Option<AuthStrategy> {
        let env = provider.settings_config.get("env")?;

        let has_value = |key: &str| -> bool {
            env.get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some()
        };

        if has_value("ANTHROPIC_AUTH_TOKEN") {
            return Some(AuthStrategy::ClaudeAuth);
        }
        if has_value("ANTHROPIC_API_KEY") {
            return Some(AuthStrategy::Anthropic);
        }
        None
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "Claude"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // Codex OAuth: 强制使用 ChatGPT 后端 API 端点（忽略用户配置的 base_url）
        if self.is_codex_oauth(provider) {
            return Ok("https://chatgpt.com/backend-api/codex".to_string());
        }

        // 1. 从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 2. 尝试直接获取
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("apiEndpoint")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        Err(ProxyError::ConfigError(
            "Claude Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let provider_type = self.provider_type(provider);

        // GitHub Copilot 使用特殊的认证策略
        // 实际的 token 会在代理请求时动态获取
        if provider_type == ProviderType::GitHubCopilot {
            // 返回一个占位符，实际 token 由 CopilotAuthManager 动态提供
            return Some(AuthInfo::new(
                "copilot_placeholder".to_string(),
                AuthStrategy::GitHubCopilot,
            ));
        }

        // Codex OAuth (ChatGPT Plus/Pro) 同样使用占位符
        // 实际的 access_token 由 CodexOAuthManager 动态提供
        if provider_type == ProviderType::CodexOAuth {
            return Some(AuthInfo::new(
                "codex_oauth_placeholder".to_string(),
                AuthStrategy::CodexOAuth,
            ));
        }

        let key = self.extract_key(provider)?;

        match provider_type {
            ProviderType::GeminiCli => {
                // Parse stored OAuth JSON and only attach access_token when
                // it's actually usable. `parse_oauth_credentials` accepts
                // refresh-token-only JSON (which is legitimate before the
                // first refresh) and also surfaces `{"access_token": "", ...}`
                // for expired credentials. In both cases we would otherwise
                // send `Authorization: Bearer ` to upstream and get a 401.
                //
                // CC Switch does not currently exchange the refresh_token for
                // a fresh access_token. Until that path exists, degrade to
                // plain GoogleOAuth strategy (which still sends the raw key
                // as a fallback) and log loudly so users know to refresh
                // their `~/.gemini/oauth_creds.json`.
                match super::gemini::GeminiAdapter::new().parse_oauth_credentials(&key) {
                    Some(creds) if !creds.access_token.is_empty() => {
                        Some(AuthInfo::with_access_token(key, creds.access_token))
                    }
                    Some(_) => {
                        log::warn!(
                            "[Gemini OAuth] access_token missing or empty for provider `{}`; \
                             bearer auth will likely fail with 401. Refresh \
                             ~/.gemini/oauth_creds.json via the gemini CLI to obtain a new token.",
                            provider.id
                        );
                        Some(AuthInfo::new(key, AuthStrategy::GoogleOAuth))
                    }
                    None => Some(AuthInfo::new(key, AuthStrategy::GoogleOAuth)),
                }
            }
            ProviderType::Gemini => Some(AuthInfo::new(key, AuthStrategy::Google)),
            ProviderType::OpenRouter => Some(AuthInfo::new(key, AuthStrategy::Bearer)),
            ProviderType::ClaudeAuth => Some(AuthInfo::new(key, AuthStrategy::ClaudeAuth)),
            _ => {
                // 按 env 中的变量名推断鉴权策略，对齐 Anthropic SDK 语义：
                // ANTHROPIC_AUTH_TOKEN → Authorization: Bearer
                // ANTHROPIC_API_KEY    → x-api-key
                // 其他来源（apiKey 直填等）默认走 x-api-key（Anthropic 官方协议）。
                let strategy = self
                    .infer_anthropic_auth_strategy(provider)
                    .unwrap_or(AuthStrategy::Anthropic);
                Some(AuthInfo::new(key, strategy))
            }
        }
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        // Codex OAuth: 所有请求统一走 /responses 端点
        if base_url == "https://chatgpt.com/backend-api/codex" {
            let _ = endpoint; // 忽略原始 endpoint
            return "https://chatgpt.com/backend-api/codex/responses".to_string();
        }

        // NOTE:
        // 过去 OpenRouter 只有 OpenAI Chat Completions 兼容接口，需要把 Claude 的 `/v1/messages`
        // 映射到 `/v1/chat/completions`，并做 Anthropic ↔ OpenAI 的格式转换。
        //
        // 现在 OpenRouter 已推出 Claude Code 兼容接口，因此默认直接透传 endpoint。
        // 如需回退旧逻辑，可在 forwarder 中根据 needs_transform 改写 endpoint。
        //
        let mut base = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );

        // 去除重复的 /v1/v1（可能由 base_url 与 endpoint 都带版本导致）
        while base.contains("/v1/v1") {
            base = base.replace("/v1/v1", "/v1");
        }

        base
    }

    fn get_auth_headers(&self, auth: &AuthInfo) -> Vec<(http::HeaderName, http::HeaderValue)> {
        use http::{HeaderName, HeaderValue};
        // 注意：anthropic-version 由 forwarder.rs 统一处理（透传客户端值或设置默认值）
        let bearer = format!("Bearer {}", auth.api_key);
        match auth.strategy {
            AuthStrategy::Anthropic => {
                vec![(
                    HeaderName::from_static("x-api-key"),
                    HeaderValue::from_str(&auth.api_key).unwrap(),
                )]
            }
            AuthStrategy::ClaudeAuth | AuthStrategy::Bearer => {
                vec![(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&bearer).unwrap(),
                )]
            }
            AuthStrategy::Google => vec![(
                HeaderName::from_static("x-goog-api-key"),
                HeaderValue::from_str(&auth.api_key).unwrap(),
            )],
            AuthStrategy::GoogleOAuth => {
                let token = auth.access_token.as_ref().unwrap_or(&auth.api_key);
                vec![
                    (
                        HeaderName::from_static("authorization"),
                        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
                    ),
                    (
                        HeaderName::from_static("x-goog-api-client"),
                        HeaderValue::from_static("GeminiCLI/1.0"),
                    ),
                ]
            }
            AuthStrategy::CodexOAuth => {
                // 注意：bearer token 由 forwarder 动态注入到 auth.api_key
                // ChatGPT-Account-Id 由 forwarder 注入额外 header
                vec![
                    (
                        HeaderName::from_static("authorization"),
                        HeaderValue::from_str(&bearer).unwrap(),
                    ),
                    (
                        HeaderName::from_static("originator"),
                        HeaderValue::from_static("cc-switch"),
                    ),
                ]
            }
            AuthStrategy::GitHubCopilot => {
                // 生成请求追踪 ID
                let request_id = uuid::Uuid::new_v4().to_string();
                vec![
                    (
                        HeaderName::from_static("authorization"),
                        HeaderValue::from_str(&bearer).unwrap(),
                    ),
                    (
                        HeaderName::from_static("editor-version"),
                        HeaderValue::from_static(super::copilot_auth::COPILOT_EDITOR_VERSION),
                    ),
                    (
                        HeaderName::from_static("editor-plugin-version"),
                        HeaderValue::from_static(super::copilot_auth::COPILOT_PLUGIN_VERSION),
                    ),
                    (
                        HeaderName::from_static("copilot-integration-id"),
                        HeaderValue::from_static(super::copilot_auth::COPILOT_INTEGRATION_ID),
                    ),
                    (
                        HeaderName::from_static("user-agent"),
                        HeaderValue::from_static(super::copilot_auth::COPILOT_USER_AGENT),
                    ),
                    (
                        HeaderName::from_static("x-github-api-version"),
                        HeaderValue::from_static(super::copilot_auth::COPILOT_API_VERSION),
                    ),
                    // 26-04-01新增的copilot关键 headers
                    (
                        HeaderName::from_static("openai-intent"),
                        HeaderValue::from_static("conversation-agent"),
                    ),
                    (
                        HeaderName::from_static("x-initiator"),
                        HeaderValue::from_static("user"),
                    ),
                    (
                        HeaderName::from_static("x-interaction-type"),
                        HeaderValue::from_static("conversation-agent"),
                    ),
                    // x-interaction-id 由 forwarder 按需注入（仅在有 session 时）
                    (
                        HeaderName::from_static("x-vscode-user-agent-library-version"),
                        HeaderValue::from_static("electron-fetch"),
                    ),
                    (
                        HeaderName::from_static("x-request-id"),
                        HeaderValue::from_str(&request_id).unwrap(),
                    ),
                    (
                        HeaderName::from_static("x-agent-task-id"),
                        HeaderValue::from_str(&request_id).unwrap(),
                    ),
                ]
            }
        }
    }

    fn needs_transform(&self, provider: &Provider) -> bool {
        // GitHub Copilot 总是需要格式转换 (Anthropic → OpenAI)
        if self.is_github_copilot(provider) {
            return true;
        }

        // Codex OAuth 总是需要格式转换 (Anthropic → OpenAI Responses API)
        if self.is_codex_oauth(provider) {
            return true;
        }

        // 根据 api_format 配置决定是否需要格式转换
        // - "anthropic" (默认): 直接透传，无需转换
        // - "openai_chat": 需要 Anthropic ↔ OpenAI Chat Completions 格式转换
        // - "openai_responses": 需要 Anthropic ↔ OpenAI Responses API 格式转换
        matches!(
            self.get_api_format(provider),
            "openai_chat" | "openai_responses" | "gemini_native"
        )
    }

    fn transform_request(
        &self,
        body: serde_json::Value,
        provider: &Provider,
    ) -> Result<serde_json::Value, ProxyError> {
        transform_claude_request_for_api_format(
            body,
            provider,
            self.get_api_format(provider),
            None,
            None,
        )
    }

    fn transform_response(&self, body: serde_json::Value) -> Result<serde_json::Value, ProxyError> {
        // Heuristic: detect response format by presence of top-level fields.
        // The ProviderAdapter trait's transform_response doesn't receive the Provider
        // config, so we can't check api_format here. Instead we rely on the fact that
        // Responses API always returns "output" while Chat Completions returns "choices".
        // This is safe because the two formats are structurally disjoint.
        if body.get("candidates").is_some() || body.get("promptFeedback").is_some() {
            super::transform_gemini::gemini_to_anthropic(body)
        } else if body.get("output").is_some() {
            super::transform_responses::responses_to_anthropic(body)
        } else {
            super::transform::openai_to_anthropic(body)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Claude".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("claude".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_with_meta(config: serde_json::Value, meta: ProviderMeta) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Claude".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("claude".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_extract_base_url_from_env() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://api.anthropic.com");
    }

    #[test]
    fn test_extract_auth_anthropic_auth_token_uses_claude_auth_strategy() {
        // ANTHROPIC_AUTH_TOKEN 在 Anthropic SDK 里语义就是 Authorization: Bearer，
        // 因此走 ClaudeAuth strategy 而不是 Anthropic（x-api-key）。
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-ant-test-key");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    #[test]
    fn test_extract_auth_anthropic_api_key() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_API_KEY": "sk-ant-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-ant-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
    }

    #[test]
    fn test_extract_auth_both_env_vars_prefer_auth_token() {
        // 两个变量都填时，extract_key 选 AUTH_TOKEN，strategy 推断也必须保持一致。
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-from-auth-token",
                "ANTHROPIC_API_KEY": "sk-from-api-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-from-auth-token");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    #[test]
    fn test_extract_auth_apikey_field_fallback_uses_anthropic_strategy() {
        // 当用户没填任一 ANTHROPIC_* env，而是直接使用 apiKey 字段时，
        // 视为没有显式语义偏好，默认走 Anthropic 官方协议（x-api-key）。
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "apiKey": "sk-direct",
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-direct");
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
    }

    #[test]
    fn test_get_auth_headers_anthropic_emits_x_api_key() {
        let adapter = ClaudeAdapter::new();
        let auth = AuthInfo::new("sk-ant-test".to_string(), AuthStrategy::Anthropic);

        let headers = adapter.get_auth_headers(&auth);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "x-api-key");
        assert_eq!(headers[0].1.to_str().unwrap(), "sk-ant-test");
    }

    #[test]
    fn test_get_auth_headers_claude_auth_emits_authorization_bearer() {
        let adapter = ClaudeAdapter::new();
        let auth = AuthInfo::new("sk-relay-test".to_string(), AuthStrategy::ClaudeAuth);

        let headers = adapter.get_auth_headers(&auth);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "authorization");
        assert_eq!(headers[0].1.to_str().unwrap(), "Bearer sk-relay-test");
    }

    #[test]
    fn test_get_auth_headers_bearer_emits_authorization_bearer() {
        let adapter = ClaudeAdapter::new();
        let auth = AuthInfo::new("sk-or-test".to_string(), AuthStrategy::Bearer);

        let headers = adapter.get_auth_headers(&auth);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "authorization");
        assert_eq!(headers[0].1.to_str().unwrap(), "Bearer sk-or-test");
    }

    #[test]
    fn test_extract_auth_openrouter() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
                "OPENROUTER_API_KEY": "sk-or-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-or-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_claude_auth_mode() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key"
            },
            "auth_mode": "bearer_only"
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-proxy-key");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    #[test]
    fn test_extract_auth_claude_auth_env_mode() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key",
                "AUTH_MODE": "bearer_only"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-proxy-key");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    /// Regression: a Gemini OAuth credential JSON that carries only a
    /// refresh_token (no active access_token) must not be surfaced as an
    /// `AuthInfo` whose bearer would be empty. Without the guard, downstream
    /// header injection produces `Authorization: Bearer ` and a deterministic
    /// 401 from upstream.
    #[test]
    fn test_extract_auth_gemini_cli_refresh_only_json_does_not_expose_empty_bearer() {
        let adapter = ClaudeAdapter::new();
        let refresh_only_json =
            r#"{"refresh_token":"rt-abc","client_id":"cid","client_secret":"cs"}"#;
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": refresh_only_json
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );

        let auth = adapter.extract_auth(&provider).unwrap();
        // access_token must not be surfaced as `Some("")` — the OAuth header
        // builder uses `access_token.as_ref().unwrap_or(&api_key)`, so a
        // `Some("")` would win over the raw key and emit `Bearer `.
        assert!(
            auth.access_token.as_deref().is_none_or(|t| !t.is_empty()),
            "empty access_token leaked into AuthInfo"
        );
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
    }

    /// Companion case: a JSON credential with an empty-string `access_token`
    /// field (the shape an expired credential can take after partial writes)
    /// must degrade the same way.
    #[test]
    fn test_extract_auth_gemini_cli_empty_access_token_degrades_to_raw_key() {
        let adapter = ClaudeAdapter::new();
        let expired_json = r#"{"access_token":"","refresh_token":"rt-abc","client_id":"cid","client_secret":"cs"}"#;
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": expired_json
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );

        let auth = adapter.extract_auth(&provider).unwrap();
        assert!(
            auth.access_token.as_deref().is_none_or(|t| !t.is_empty()),
            "empty access_token leaked into AuthInfo"
        );
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
    }

    /// Counter-case: a well-formed JSON credential with a non-empty
    /// access_token must still flow through the OAuth path unchanged.
    #[test]
    fn test_extract_auth_gemini_cli_valid_json_keeps_access_token() {
        let adapter = ClaudeAdapter::new();
        let valid_json = r#"{"access_token":"ya29.valid","refresh_token":"rt"}"#;
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": valid_json
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.access_token.as_deref(), Some("ya29.valid"));
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
    }

    /// 回归:从 oauth_creds.json 复制时常带前导换行/空格。未 trim 时
    /// `starts_with('{')` 会落空,导致误分类为 `ProviderType::Gemini`,再
    /// 以 raw JSON 当 `x-goog-api-key` 发出去触发 401。trim 应在 provider
    /// 类型判定和 OAuth 解析前统一生效。
    #[test]
    fn test_extract_auth_gemini_cli_json_with_leading_whitespace_classifies_correctly() {
        let adapter = ClaudeAdapter::new();
        let valid_json = r#"{"access_token":"ya29.valid","refresh_token":"rt"}"#;
        let key_with_whitespace = format!("\n  {valid_json}\n");
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": key_with_whitespace
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(adapter.provider_type(&provider), ProviderType::GeminiCli);

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.access_token.as_deref(), Some("ya29.valid"));
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
    }

    /// 回归:裸 `ya29.` access_token 若带前导换行,也应被 trim 后识别为
    /// Gemini CLI OAuth,避免前导空白把 `starts_with("ya29.")` 检查顶穿。
    #[test]
    fn test_extract_auth_gemini_cli_access_token_with_leading_newline_classifies_correctly() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": "\nya29.raw-token-value\n"
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(adapter.provider_type(&provider), ProviderType::GeminiCli);

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.access_token.as_deref(), Some("ya29.raw-token-value"));
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
    }

    #[test]
    fn test_provider_type_detection() {
        let adapter = ClaudeAdapter::new();

        // Anthropic 官方
        let anthropic = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-test"
            }
        }));
        assert_eq!(adapter.provider_type(&anthropic), ProviderType::Claude);

        // OpenRouter
        let openrouter = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
                "OPENROUTER_API_KEY": "sk-or-test"
            }
        }));
        assert_eq!(adapter.provider_type(&openrouter), ProviderType::OpenRouter);

        // ClaudeAuth
        let claude_auth = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            },
            "auth_mode": "bearer_only"
        }));
        assert_eq!(
            adapter.provider_type(&claude_auth),
            ProviderType::ClaudeAuth
        );
    }

    #[test]
    fn test_build_url_anthropic() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://api.anthropic.com", "/v1/messages");
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_build_url_openrouter() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://openrouter.ai/api", "/v1/messages");
        assert_eq!(url, "https://openrouter.ai/api/v1/messages");
    }

    #[test]
    fn test_build_url_no_beta_for_other_endpoints() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://api.anthropic.com", "/v1/complete");
        assert_eq!(url, "https://api.anthropic.com/v1/complete");
    }

    #[test]
    fn test_build_url_preserve_existing_query() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://api.anthropic.com", "/v1/messages?foo=bar");
        assert_eq!(url, "https://api.anthropic.com/v1/messages?foo=bar");
    }

    #[test]
    fn test_build_url_no_beta_for_github_copilot() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://api.githubcopilot.com", "/v1/messages");
        assert_eq!(url, "https://api.githubcopilot.com/v1/messages");
    }

    #[test]
    fn test_build_url_no_beta_for_openai_chat_completions() {
        let adapter = ClaudeAdapter::new();
        let url = adapter.build_url("https://integrate.api.nvidia.com", "/v1/chat/completions");
        assert_eq!(url, "https://integrate.api.nvidia.com/v1/chat/completions");
    }

    #[test]
    fn test_needs_transform() {
        let adapter = ClaudeAdapter::new();

        // Default: no transform (anthropic format) - no meta
        let anthropic_provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }));
        assert!(!adapter.needs_transform(&anthropic_provider));

        // Explicit anthropic format in meta: no transform
        let explicit_anthropic = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            ProviderMeta {
                api_format: Some("anthropic".to_string()),
                ..Default::default()
            },
        );
        assert!(!adapter.needs_transform(&explicit_anthropic));

        // Legacy settings_config.api_format: openai_chat should enable transform
        let legacy_settings_api_format = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "api_format": "openai_chat"
        }));
        assert!(adapter.needs_transform(&legacy_settings_api_format));

        // Legacy openrouter_compat_mode: bool/number/string should enable transform
        let legacy_openrouter_bool = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "openrouter_compat_mode": true
        }));
        assert!(adapter.needs_transform(&legacy_openrouter_bool));

        let legacy_openrouter_num = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "openrouter_compat_mode": 1
        }));
        assert!(adapter.needs_transform(&legacy_openrouter_num));

        let legacy_openrouter_str = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "openrouter_compat_mode": "true"
        }));
        assert!(adapter.needs_transform(&legacy_openrouter_str));

        // OpenAI Chat format in meta: needs transform
        let openai_chat_provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
        assert!(adapter.needs_transform(&openai_chat_provider));

        // OpenAI Responses format in meta: needs transform
        let openai_responses_provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                ..Default::default()
            },
        );
        assert!(adapter.needs_transform(&openai_responses_provider));

        let gemini_native_provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );
        assert!(adapter.needs_transform(&gemini_native_provider));
        assert_eq!(
            adapter.provider_type(&gemini_native_provider),
            ProviderType::Gemini
        );

        // meta takes precedence over legacy settings_config fields
        let meta_precedence_over_settings = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                },
                "api_format": "openai_chat",
                "openrouter_compat_mode": true
            }),
            ProviderMeta {
                api_format: Some("anthropic".to_string()),
                ..Default::default()
            },
        );
        assert!(!adapter.needs_transform(&meta_precedence_over_settings));

        // Unknown format in meta: default to anthropic (no transform)
        let unknown_format = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            ProviderMeta {
                api_format: Some("unknown".to_string()),
                ..Default::default()
            },
        );
        assert!(!adapter.needs_transform(&unknown_format));
    }

    #[test]
    fn test_github_copilot_detection_by_url() {
        let adapter = ClaudeAdapter::new();

        // GitHub Copilot by base_url
        let copilot = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }));
        assert_eq!(adapter.provider_type(&copilot), ProviderType::GitHubCopilot);
    }

    #[test]
    fn test_github_copilot_detection_by_meta() {
        let adapter = ClaudeAdapter::new();

        // GitHub Copilot by meta.provider_type
        let copilot_meta = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(
            adapter.provider_type(&copilot_meta),
            ProviderType::GitHubCopilot
        );
    }

    #[test]
    fn test_github_copilot_auth() {
        let adapter = ClaudeAdapter::new();

        let copilot = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }));

        let auth = adapter.extract_auth(&copilot).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::GitHubCopilot);
    }

    #[test]
    fn test_github_copilot_needs_transform() {
        let adapter = ClaudeAdapter::new();

        let copilot = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }));

        // GitHub Copilot always needs transform
        assert!(adapter.needs_transform(&copilot));
    }

    #[test]
    fn test_transform_claude_request_for_api_format_responses() {
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }));
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 128
        });

        let transformed = transform_claude_request_for_api_format(
            body,
            &provider,
            "openai_responses",
            None,
            None,
        )
        .unwrap();

        assert_eq!(transformed["model"], "gpt-5.4");
        assert!(transformed.get("input").is_some());
        assert!(transformed.get("max_output_tokens").is_some());
    }

    #[test]
    fn test_transform_claude_request_for_codex_oauth_uses_session_cache_key() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                provider_type: Some("codex_oauth".to_string()),
                ..ProviderMeta::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 128
        });

        let transformed = transform_claude_request_for_api_format(
            body,
            &provider,
            "openai_responses",
            Some("session-123"),
            None,
        )
        .unwrap();

        assert_eq!(transformed["prompt_cache_key"], "session-123");
    }

    #[test]
    fn test_transform_claude_request_for_codex_oauth_without_session_falls_back_to_provider_id() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                provider_type: Some("codex_oauth".to_string()),
                ..ProviderMeta::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 128
        });

        let transformed = transform_claude_request_for_api_format(
            body,
            &provider,
            "openai_responses",
            None,
            None,
        )
        .unwrap();

        assert_eq!(transformed["prompt_cache_key"], provider.id);
    }

    #[test]
    fn test_transform_claude_request_for_codex_oauth_keeps_explicit_cache_key() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                provider_type: Some("codex_oauth".to_string()),
                prompt_cache_key: Some("explicit-cache-key".to_string()),
                ..ProviderMeta::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 128
        });

        let transformed = transform_claude_request_for_api_format(
            body,
            &provider,
            "openai_responses",
            Some("session-123"),
            None,
        )
        .unwrap();

        assert_eq!(transformed["prompt_cache_key"], "explicit-cache-key");
    }

    #[test]
    fn test_transform_claude_request_for_api_format_codex_oauth_fast_mode_off() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            ProviderMeta {
                provider_type: Some("codex_oauth".to_string()),
                codex_fast_mode: Some(false),
                ..ProviderMeta::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 128
        });

        let transformed = transform_claude_request_for_api_format(
            body,
            &provider,
            "openai_responses",
            None,
            None,
        )
        .unwrap();

        assert_eq!(transformed["store"], json!(false));
        assert!(transformed.get("service_tier").is_none());
        assert_eq!(
            transformed["include"],
            json!(["reasoning.encrypted_content"])
        );
    }

    #[test]
    fn test_transform_claude_request_for_api_format_gemini_native() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://generativelanguage.googleapis.com",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("gemini_native".to_string()),
                ..Default::default()
            },
        );
        let body = json!({
            "model": "gemini-2.5-pro",
            "system": "You are helpful.",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 64
        });

        let transformed =
            transform_claude_request_for_api_format(body, &provider, "gemini_native", None, None)
                .unwrap();

        assert!(transformed.get("contents").is_some());
        assert_eq!(
            transformed["systemInstruction"]["parts"][0]["text"],
            "You are helpful."
        );
        assert_eq!(transformed["generationConfig"]["maxOutputTokens"], 64);
    }

    #[test]
    fn test_transform_claude_request_for_api_format_openai_chat_skips_prompt_cache_key_by_default()
    {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 64
        });

        let transformed =
            transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
                .unwrap();

        assert!(transformed.get("prompt_cache_key").is_none());
    }

    #[test]
    fn test_transform_claude_request_for_api_format_openai_chat_keeps_explicit_prompt_cache_key() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                prompt_cache_key: Some("claude-cache-route".to_string()),
                ..Default::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 64
        });

        let transformed =
            transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
                .unwrap();

        assert_eq!(transformed["prompt_cache_key"], "claude-cache-route");
    }

    #[test]
    fn test_transform_openai_chat_skips_reasoning_content_for_generic_provider() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
        let body = json!({
            "model": "gpt-5.4",
            "max_tokens": 64,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let transformed =
            transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
                .unwrap();

        let msg = &transformed["messages"][0];
        assert!(msg.get("tool_calls").is_some());
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn test_transform_openai_chat_preserves_reasoning_content_for_kimi_provider() {
        let provider = create_provider_with_meta(
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.moonshot.cn/v1",
                    "ANTHROPIC_API_KEY": "test-key"
                }
            }),
            ProviderMeta {
                api_format: Some("openai_chat".to_string()),
                ..Default::default()
            },
        );
        let body = json!({
            "model": "kimi-k2.6",
            "max_tokens": 64,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let transformed =
            transform_claude_request_for_api_format(body, &provider, "openai_chat", None, None)
                .unwrap();

        let msg = &transformed["messages"][0];
        assert_eq!(msg["reasoning_content"], "I should call the tool.");
        assert!(msg.get("tool_calls").is_some());
    }
}
