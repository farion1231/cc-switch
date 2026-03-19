//! Codex (OpenAI) Provider Adapter
//!
//! 仅透传模式，支持直连 OpenAI API
//!
//! ## 客户端检测
//! 支持检测官方 Codex 客户端 (codex_vscode, codex_cli_rs)

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use regex::Regex;
use reqwest::RequestBuilder;
use std::sync::LazyLock;

/// 官方 Codex 客户端 User-Agent 正则
#[allow(dead_code)]
static CODEX_CLIENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(codex_vscode|codex_cli_rs)/[\d.]+").unwrap());

/// Azure OpenAI 默认 API 版本
const AZURE_DEFAULT_API_VERSION: &str = "2025-03-01-preview";

/// Codex 适配器
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 检测是否为官方 Codex 客户端
    ///
    /// 匹配 User-Agent 模式: `^(codex_vscode|codex_cli_rs)/[\d.]+`
    #[allow(dead_code)]
    pub fn is_official_client(user_agent: &str) -> bool {
        CODEX_CLIENT_REGEX.is_match(user_agent)
    }

    /// 检测 URL 是否为 Azure OpenAI 端点
    fn is_azure_url(url: &str) -> bool {
        url.contains(".cognitiveservices.azure.com")
            || url.contains(".openai.azure.com")
    }

    /// 检测 Provider 的 base_url 是否为 Azure OpenAI 端点
    fn is_azure_endpoint(&self, provider: &Provider) -> bool {
        self.extract_base_url(provider)
            .map(|url| Self::is_azure_url(&url))
            .unwrap_or(false)
    }

    /// 从 Provider 配置中提取 API Key
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        const KEY_NAMES: [&str; 1] = ["OPENAI_API_KEY"];

        // 1. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = KEY_NAMES
                .iter()
                .find_map(|name| env.get(name).and_then(|v| v.as_str()))
            {
                return Some(key.to_string());
            }
        }

        // 2. 尝试从 auth 中获取 (Codex CLI 格式)
        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = KEY_NAMES
                .iter()
                .find_map(|name| auth.get(name).and_then(|v| v.as_str()))
            {
                return Some(key.to_string());
            }
        }

        // 3. 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            return Some(key.to_string());
        }

        // 4. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(key) = config
                .get("api_key")
                .or_else(|| config.get("apiKey"))
                .and_then(|v| v.as_str())
            {
                return Some(key.to_string());
            }
        }

        None
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // 1. 尝试直接获取 base_url 字段
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 2. 尝试 baseURL
        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 3. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(url) = config.get("base_url").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }

            // 尝试解析 TOML 字符串格式
            if let Some(config_str) = config.as_str() {
                if let Some(start) = config_str.find("base_url = \"") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('"') {
                        return Ok(rest[..end].trim_end_matches('/').to_string());
                    }
                }
                if let Some(start) = config_str.find("base_url = '") {
                    let rest = &config_str[start + 12..];
                    if let Some(end) = rest.find('\'') {
                        return Ok(rest[..end].trim_end_matches('/').to_string());
                    }
                }
            }
        }

        Err(ProxyError::ConfigError(
            "Codex Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let strategy = if self.is_azure_endpoint(provider) {
            AuthStrategy::AzureApiKey
        } else {
            AuthStrategy::Bearer
        };
        self.extract_key(provider)
            .map(|key| AuthInfo::new(key, strategy))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let base_trimmed = base_url.trim_end_matches('/');
        let endpoint_trimmed = endpoint.trim_start_matches('/');

        // OpenAI/Codex 的 base_url 可能是：
        // - 纯 origin: https://api.openai.com  (需要自动补 /v1)
        // - 已含 /v1: https://api.openai.com/v1 (直接拼接)
        // - 自定义前缀: https://xxx/openai (不添加 /v1，直接拼接)

        // 检查 base_url 是否已经包含 /v1
        let already_has_v1 = base_trimmed.ends_with("/v1");

        // 检查是否是纯 origin（没有路径部分）
        let origin_only = match base_trimmed.split_once("://") {
            Some((_scheme, rest)) => !rest.contains('/'),
            None => !base_trimmed.contains('/'),
        };

        let mut url = if already_has_v1 {
            // 已经有 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        } else if origin_only {
            // 纯 origin，添加 /v1
            format!("{base_trimmed}/v1/{endpoint_trimmed}")
        } else {
            // 自定义前缀，不添加 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        };

        // 去除重复的 /v1/v1（可能由 base_url 与 endpoint 都带版本导致）
        while url.contains("/v1/v1") {
            url = url.replace("/v1/v1", "/v1");
        }

        // Azure OpenAI 旧版 API（不含 /v1 路径）需要 api-version 查询参数
        // 新版 v1 API（路径含 /openai/v1）不需要 api-version
        if Self::is_azure_url(base_trimmed)
            && !base_trimmed.contains("/openai/v1")
            && !url.contains("api-version")
        {
            let separator = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{separator}api-version={AZURE_DEFAULT_API_VERSION}");
        }

        url
    }

    fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
        match auth.strategy {
            AuthStrategy::AzureApiKey => request.header("api-key", &auth.api_key),
            _ => request.header("Authorization", format!("Bearer {}", auth.api_key)),
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
            name: "Test Codex".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("codex".to_string()),
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
    fn test_extract_base_url_direct() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://api.openai.com/v1"
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_extract_auth_from_auth_field() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-test-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_from_env() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "OPENAI_API_KEY": "sk-env-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-env-key-12345678");
    }

    #[test]
    fn test_build_url() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com/v1", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_build_url_origin_adds_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_build_url_custom_prefix_no_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://example.com/openai", "/responses");
        assert_eq!(url, "https://example.com/openai/responses");
    }

    #[test]
    fn test_build_url_dedup_v1() {
        let adapter = CodexAdapter::new();
        // base_url 已包含 /v1，endpoint 也包含 /v1
        let url = adapter.build_url("https://www.packyapi.com/v1", "/v1/responses");
        assert_eq!(url, "https://www.packyapi.com/v1/responses");
    }

    // 官方客户端检测测试
    #[test]
    fn test_is_official_client_vscode() {
        assert!(CodexAdapter::is_official_client("codex_vscode/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_vscode/2.3.4"));
        assert!(CodexAdapter::is_official_client("codex_vscode/0.1"));
    }

    #[test]
    fn test_is_official_client_cli() {
        assert!(CodexAdapter::is_official_client("codex_cli_rs/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_cli_rs/0.5.2"));
    }

    #[test]
    fn test_is_not_official_client() {
        assert!(!CodexAdapter::is_official_client("Mozilla/5.0"));
        assert!(!CodexAdapter::is_official_client("curl/7.68.0"));
        assert!(!CodexAdapter::is_official_client("python-requests/2.25.1"));
        assert!(!CodexAdapter::is_official_client("codex_other/1.0.0"));
        assert!(!CodexAdapter::is_official_client(""));
    }

    #[test]
    fn test_build_url_azure_v1_no_api_version() {
        // 新版 v1 API 不需要 api-version
        let adapter = CodexAdapter::new();
        let url = adapter.build_url(
            "https://myinstance.cognitiveservices.azure.com/openai/v1",
            "/responses",
        );
        assert_eq!(
            url,
            "https://myinstance.cognitiveservices.azure.com/openai/v1/responses"
        );
        assert!(!url.contains("api-version"));
    }

    #[test]
    fn test_build_url_azure_legacy_appends_api_version() {
        // 旧版 API（不含 /openai/v1）自动追加 api-version
        let adapter = CodexAdapter::new();
        let url = adapter.build_url(
            "https://myinstance.openai.azure.com/openai/deployments/gpt-4o",
            "/chat/completions",
        );
        assert!(url.contains(&format!("api-version={AZURE_DEFAULT_API_VERSION}")));
    }

    #[test]
    fn test_build_url_azure_legacy_preserves_existing_api_version() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url(
            "https://myinstance.openai.azure.com/openai/deployments/gpt-4o?api-version=2024-10-21",
            "/chat/completions",
        );
        assert!(!url.contains(&format!("api-version={AZURE_DEFAULT_API_VERSION}")));
        assert!(url.contains("api-version=2024-10-21"));
    }

    #[test]
    fn test_build_url_non_azure_no_api_version() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com/v1", "/responses");
        assert!(!url.contains("api-version"));
    }

    #[test]
    fn test_extract_auth_azure_cognitive_services() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://myinstance.cognitiveservices.azure.com/openai/v1",
            "env": {
                "OPENAI_API_KEY": "azure-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "azure-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::AzureApiKey);
    }

    #[test]
    fn test_extract_auth_azure_openai_domain() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://myinstance.openai.azure.com/openai/v1",
            "apiKey": "azure-key-87654321"
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::AzureApiKey);
    }

    #[test]
    fn test_extract_auth_non_azure_uses_bearer() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://api.openai.com/v1",
            "env": {
                "OPENAI_API_KEY": "sk-regular-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_add_auth_headers_azure() {
        let adapter = CodexAdapter::new();
        let auth = AuthInfo::new("azure-test-key".to_string(), AuthStrategy::AzureApiKey);
        let client = reqwest::Client::new();
        let request = client.get("https://example.com");
        let request = adapter.add_auth_headers(request, &auth);
        let built = request.build().unwrap();
        assert_eq!(
            built.headers().get("api-key").unwrap().to_str().unwrap(),
            "azure-test-key"
        );
        assert!(built.headers().get("Authorization").is_none());
    }

    #[test]
    fn test_add_auth_headers_bearer() {
        let adapter = CodexAdapter::new();
        let auth = AuthInfo::new("sk-test-key".to_string(), AuthStrategy::Bearer);
        let client = reqwest::Client::new();
        let request = client.get("https://example.com");
        let request = adapter.add_auth_headers(request, &auth);
        let built = request.build().unwrap();
        assert_eq!(
            built.headers().get("Authorization").unwrap().to_str().unwrap(),
            "Bearer sk-test-key"
        );
        assert!(built.headers().get("api-key").is_none());
    }

    #[test]
    fn test_is_official_client_partial_match() {
        // 必须从开头匹配
        assert!(!CodexAdapter::is_official_client("some codex_vscode/1.0.0"));
        assert!(!CodexAdapter::is_official_client(
            "prefix_codex_cli_rs/1.0.0"
        ));
    }
}
