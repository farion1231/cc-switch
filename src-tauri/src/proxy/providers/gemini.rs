//! Gemini (Google) Provider Adapter
//!
//! 支持 API Key 和 OAuth 两种认证方式
//!
//! ## 认证模式
//! - **Gemini**: API Key 认证 (x-goog-api-key)
//! - **GeminiCli**: OAuth Bearer 认证 (用于 Gemini CLI)
//!
//! ## 上游 API 格式（`meta.apiFormat`）
//! - **gemini_native**（默认）: 上游原生支持 Gemini 协议，请求/响应原样透传
//! - **openai_chat**: 上游仅支持 OpenAI Chat Completions（如 DeepSeek、Moonshot），
//!   代理将 Gemini 请求转换为 Anthropic 中间格式再转换为 OpenAI Chat，响应反向转换
//! - **anthropic**: 上游支持 Anthropic Messages API，请求 Gemini→Anthropic 转换，响应反向

use super::{AuthInfo, AuthStrategy, ProviderAdapter, ProviderType};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::Value;

/// Gemini 适配器
pub struct GeminiAdapter;

/// 读取供应商配置的上游 API 格式（`meta.apiFormat`），默认 `gemini_native`（透传）。
pub fn get_upstream_api_format(provider: &Provider) -> &'static str {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .map(|format| match format {
            "openai_chat" => "openai_chat",
            "anthropic" => "anthropic",
            _ => "gemini_native",
        })
        .unwrap_or("gemini_native")
}

/// 将 Gemini 入口 endpoint 重写为转换后上游的 endpoint。
///
/// `/v1beta/models/{model}:generateContent` →
/// - `openai_chat`: `/v1/chat/completions`
/// - `anthropic`: `/v1/messages`
///
/// Gemini 的 `?key=` / `?alt=sse` 查询参数不透传给上游（认证改由适配器注入，
/// 流式语义由请求体 `stream` 字段表达）。
pub fn rewrite_gemini_transform_endpoint(endpoint: &str, api_format: &str) -> String {
    let upstream_path = match api_format {
        "openai_chat" => "/v1/chat/completions",
        "anthropic" => "/v1/messages",
        _ => return endpoint.to_string(),
    };
    if endpoint.contains(":generateContent") || endpoint.contains(":streamGenerateContent") {
        upstream_path.to_string()
    } else {
        endpoint.to_string()
    }
}

/// OAuth 凭证结构
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[allow(dead_code)]
impl OAuthCredentials {
    /// 检查是否需要刷新 token（有 refresh_token 但没有有效的 access_token）
    pub fn needs_refresh(&self) -> bool {
        self.refresh_token.is_some() && self.access_token.is_empty()
    }

    /// 检查是否可以刷新 token
    pub fn can_refresh(&self) -> bool {
        self.refresh_token.is_some() && self.client_id.is_some() && self.client_secret.is_some()
    }
}

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 获取供应商类型
    ///
    /// 根据 API Key 格式检测：
    /// - GeminiCli: access_token (ya29. 开头) 或 JSON 格式凭证
    /// - Gemini: 普通 API Key
    pub fn provider_type(&self, provider: &Provider) -> ProviderType {
        if let Some(key) = self.extract_key_raw(provider) {
            // OAuth access_token 以 ya29. 开头
            if key.starts_with("ya29.") {
                return ProviderType::GeminiCli;
            }
            // JSON 格式的 OAuth 凭证
            if key.starts_with('{') {
                return ProviderType::GeminiCli;
            }
        }
        ProviderType::Gemini
    }

    /// 检测认证类型
    pub fn detect_auth_type(&self, provider: &Provider) -> AuthStrategy {
        match self.provider_type(provider) {
            ProviderType::GeminiCli => AuthStrategy::GoogleOAuth,
            _ => AuthStrategy::Google,
        }
    }

    /// 解析 OAuth 凭证
    pub fn parse_oauth_credentials(&self, key: &str) -> Option<OAuthCredentials> {
        // 防御性 trim:前端在 input 事件中会 trim,但 JSON 编辑器 / deeplink
        // 导入 / live 回填等路径会绕过。带前导换行的 oauth_creds.json 粘贴
        // 是常见场景,此处统一兜底。
        let key = key.trim();

        // 直接是 access_token
        if key.starts_with("ya29.") {
            return Some(OAuthCredentials {
                access_token: key.to_string(),
                refresh_token: None,
                client_id: None,
                client_secret: None,
            });
        }

        // JSON 格式
        if key.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(key) {
                let access_token = json
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let refresh_token = json
                    .get("refresh_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let client_id = json
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let client_secret = json
                    .get("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // 如果有 access_token 或 refresh_token，返回凭证
                if !access_token.is_empty() || refresh_token.is_some() {
                    return Some(OAuthCredentials {
                        access_token,
                        refresh_token,
                        client_id,
                        client_secret,
                    });
                }
            }
        }

        None
    }

    /// 从 Provider 配置中提取原始 API Key
    fn extract_key_raw(&self, provider: &Provider) -> Option<String> {
        if let Some(env) = provider.settings_config.get("env") {
            // 使用 GEMINI_API_KEY
            if let Some(key) = env
                .get("GEMINI_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
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
            return Some(key.to_string());
        }

        None
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &'static str {
        "Gemini"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // 从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("GOOGLE_GEMINI_BASE_URL").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 尝试直接获取
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

        Err(ProxyError::ConfigError(
            "Gemini Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let key = self.extract_key_raw(provider)?;
        // 上游格式非 gemini_native 时，上游是 OpenAI/Anthropic 协议网关，
        // 改用 Bearer / x-api-key 认证而不是 x-goog-api-key。
        match get_upstream_api_format(provider) {
            "openai_chat" => return Some(AuthInfo::new(key, AuthStrategy::Bearer)),
            "anthropic" => return Some(AuthInfo::new(key, AuthStrategy::Anthropic)),
            _ => {}
        }
        let strategy = self.detect_auth_type(provider);

        match strategy {
            AuthStrategy::GoogleOAuth => {
                // 解析 OAuth 凭证
                if let Some(creds) = self.parse_oauth_credentials(&key) {
                    Some(AuthInfo::with_access_token(key, creds.access_token))
                } else {
                    // 回退到普通 API Key
                    Some(AuthInfo::new(key, AuthStrategy::Google))
                }
            }
            _ => Some(AuthInfo::new(key, AuthStrategy::Google)),
        }
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let base_trimmed = base_url.trim_end_matches('/');
        let endpoint_trimmed = endpoint.trim_start_matches('/');

        let mut url = format!("{base_trimmed}/{endpoint_trimmed}");

        // 处理 /v1beta 路径去重
        let version_patterns = ["/v1beta", "/v1"];
        for pattern in &version_patterns {
            let duplicate = format!("{pattern}{pattern}");
            if url.contains(&duplicate) {
                url = url.replace(&duplicate, pattern);
            }
        }

        url
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        use super::adapter::auth_header_value as hv;
        use http::{HeaderName, HeaderValue};
        Ok(match auth.strategy {
            AuthStrategy::GoogleOAuth => {
                let token = auth.access_token.as_ref().unwrap_or(&auth.api_key);
                vec![
                    (
                        HeaderName::from_static("authorization"),
                        hv(&format!("Bearer {token}"))?,
                    ),
                    (
                        HeaderName::from_static("x-goog-api-client"),
                        HeaderValue::from_static("GeminiCLI/1.0"),
                    ),
                ]
            }
            // OpenAI Chat 兼容上游（DeepSeek 等）：Bearer 认证
            AuthStrategy::Bearer | AuthStrategy::ClaudeAuth => {
                vec![(
                    HeaderName::from_static("authorization"),
                    hv(&format!("Bearer {}", auth.api_key))?,
                )]
            }
            // Anthropic 上游：x-api-key 认证（anthropic-version 由 forwarder 统一处理）
            AuthStrategy::Anthropic => {
                vec![(HeaderName::from_static("x-api-key"), hv(&auth.api_key)?)]
            }
            _ => vec![(
                HeaderName::from_static("x-goog-api-key"),
                hv(&auth.api_key)?,
            )],
        })
    }

    fn needs_transform(&self, provider: &Provider) -> bool {
        get_upstream_api_format(provider) != "gemini_native"
    }

    fn transform_request(&self, body: Value, provider: &Provider) -> Result<Value, ProxyError> {
        // Gemini 请求 → Anthropic 中间格式（模型名已由 handle_gemini 注入 body.model）
        let anthropic = super::transform_gemini::gemini_request_to_anthropic(body)?;
        match get_upstream_api_format(provider) {
            "openai_chat" => {
                let mut chat =
                    super::transform::anthropic_to_openai_with_reasoning_content(anthropic, false)?;
                super::transform::inject_openai_stream_include_usage(&mut chat);
                Ok(chat)
            }
            _ => Ok(anthropic),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Gemini".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("gemini".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_extract_base_url_from_env() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com/v1beta"
            }
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://generativelanguage.googleapis.com/v1beta");
    }

    #[test]
    fn test_extract_auth_api_key() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-test-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "AIza-test-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::Google);
        assert!(auth.access_token.is_none());
    }

    #[test]
    fn test_extract_auth_oauth_access_token() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "ya29.test-access-token-12345"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
        assert_eq!(
            auth.access_token,
            Some("ya29.test-access-token-12345".to_string())
        );
    }

    #[test]
    fn test_extract_auth_oauth_json() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "{\"access_token\":\"ya29.test-token\",\"refresh_token\":\"1//refresh\"}"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
        assert_eq!(auth.access_token, Some("ya29.test-token".to_string()));
    }

    #[test]
    fn test_provider_type_detection() {
        let adapter = GeminiAdapter::new();

        // API Key
        let api_key_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-test-key"
            }
        }));
        assert_eq!(
            adapter.provider_type(&api_key_provider),
            ProviderType::Gemini
        );

        // OAuth access_token
        let oauth_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "ya29.test-token"
            }
        }));
        assert_eq!(
            adapter.provider_type(&oauth_provider),
            ProviderType::GeminiCli
        );

        // OAuth JSON
        let oauth_json_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "{\"access_token\":\"ya29.test\"}"
            }
        }));
        assert_eq!(
            adapter.provider_type(&oauth_json_provider),
            ProviderType::GeminiCli
        );
    }

    #[test]
    fn test_extract_auth_fallback() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-fallback-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "AIza-fallback-key");
    }

    #[test]
    fn test_build_url_dedup() {
        let adapter = GeminiAdapter::new();
        // 模拟 base_url 已包含 /v1beta，endpoint 也包含 /v1beta
        let url = adapter.build_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-pro:generateContent",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent"
        );
    }

    #[test]
    fn test_build_url_normal() {
        let adapter = GeminiAdapter::new();
        let url = adapter.build_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/models/gemini-pro:generateContent",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent"
        );
    }

    #[test]
    fn test_parse_oauth_credentials_direct_token() {
        let adapter = GeminiAdapter::new();
        let creds = adapter
            .parse_oauth_credentials("ya29.test-access-token")
            .unwrap();
        assert_eq!(creds.access_token, "ya29.test-access-token");
        assert!(creds.refresh_token.is_none());
    }

    #[test]
    fn test_parse_oauth_credentials_json() {
        let adapter = GeminiAdapter::new();
        let creds = adapter
            .parse_oauth_credentials(
                "{\"access_token\":\"ya29.test\",\"refresh_token\":\"1//refresh\"}",
            )
            .unwrap();
        assert_eq!(creds.access_token, "ya29.test");
        assert_eq!(creds.refresh_token, Some("1//refresh".to_string()));
    }

    #[test]
    fn test_parse_oauth_credentials_invalid() {
        let adapter = GeminiAdapter::new();
        assert!(adapter.parse_oauth_credentials("AIza-api-key").is_none());
        assert!(adapter.parse_oauth_credentials("invalid-json{").is_none());
    }

    fn create_provider_with_format(
        config: serde_json::Value,
        api_format: Option<&str>,
    ) -> Provider {
        let mut provider = create_provider(config);
        if let Some(format) = api_format {
            provider.meta = Some(crate::provider::ProviderMeta {
                api_format: Some(format.to_string()),
                ..Default::default()
            });
        }
        provider
    }

    #[test]
    fn test_get_upstream_api_format_defaults_to_native() {
        let provider = create_provider(json!({
            "env": { "GEMINI_API_KEY": "sk-test" }
        }));
        assert_eq!(get_upstream_api_format(&provider), "gemini_native");

        let unknown = create_provider_with_format(
            json!({ "env": { "GEMINI_API_KEY": "sk-test" } }),
            Some("something_else"),
        );
        assert_eq!(get_upstream_api_format(&unknown), "gemini_native");
    }

    #[test]
    fn test_needs_transform_only_for_non_native_upstream() {
        let adapter = GeminiAdapter::new();
        let native = create_provider_with_format(
            json!({ "env": { "GEMINI_API_KEY": "sk-test" } }),
            Some("gemini_native"),
        );
        assert!(!adapter.needs_transform(&native));

        let openai = create_provider_with_format(
            json!({ "env": { "GEMINI_API_KEY": "sk-test" } }),
            Some("openai_chat"),
        );
        assert!(adapter.needs_transform(&openai));
    }

    #[test]
    fn test_extract_auth_openai_upstream_uses_bearer() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider_with_format(
            json!({ "env": { "GEMINI_API_KEY": "sk-deepseek-key" } }),
            Some("openai_chat"),
        );
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Bearer);

        let headers = adapter.get_auth_headers(&auth).unwrap();
        assert!(headers.iter().any(|(name, value)| name == "authorization"
            && value.to_str().unwrap() == "Bearer sk-deepseek-key"));
    }

    #[test]
    fn test_extract_auth_native_upstream_keeps_google_strategy() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": { "GEMINI_API_KEY": "AIza-test-key" }
        }));
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Google);
    }

    #[test]
    fn test_transform_request_openai_chat_converts_gemini_body() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider_with_format(
            json!({ "env": { "GEMINI_API_KEY": "sk-test" } }),
            Some("openai_chat"),
        );
        let gemini_request = json!({
            "model": "deepseek-flash",
            "contents": [{ "role": "user", "parts": [{ "text": "say ok" }] }]
        });
        let chat = adapter
            .transform_request(gemini_request, &provider)
            .unwrap();
        // OpenAI Chat 形状：model + messages
        assert_eq!(chat["model"], "deepseek-flash");
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "say ok");
    }

    #[test]
    fn test_rewrite_gemini_transform_endpoint() {
        assert_eq!(
            rewrite_gemini_transform_endpoint(
                "/v1beta/models/deepseek-flash:generateContent",
                "openai_chat"
            ),
            "/v1/chat/completions"
        );
        assert_eq!(
            rewrite_gemini_transform_endpoint(
                "/v1beta/models/m:streamGenerateContent?alt=sse",
                "openai_chat"
            ),
            "/v1/chat/completions"
        );
        assert_eq!(
            rewrite_gemini_transform_endpoint(
                "/v1beta/models/gemini-pro:generateContent?key=PROXY_MANAGED",
                "anthropic"
            ),
            "/v1/messages"
        );
        // gemini_native 不重写
        assert_eq!(
            rewrite_gemini_transform_endpoint(
                "/v1beta/models/gemini-pro:generateContent",
                "gemini_native"
            ),
            "/v1beta/models/gemini-pro:generateContent"
        );
        // 非 generateContent 端点不重写
        assert_eq!(
            rewrite_gemini_transform_endpoint("/v1beta/models", "openai_chat"),
            "/v1beta/models"
        );
    }
}
