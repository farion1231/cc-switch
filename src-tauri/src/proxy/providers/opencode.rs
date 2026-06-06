//! OpenCode Provider Adapter
//!
//! 支持 OpenAI 兼容格式的 API（如 OMO、DeepSeek 等）
//!
//! ## 环境变量
//! - `OPENAI_API_KEY`: API 密钥
//! - `OPENAI_BASE_URL`: API 基础 URL
//! - `OPENAI_MODEL`: 默认模型（可选）

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::Value;

/// OpenCode 适配器
pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    pub fn new() -> Self {
        OpenCodeAdapter
    }
}

impl ProviderAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "OpenCode"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // 1. 尝试从 settings_config.base_url 获取
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 2. 尝试从 settings_config.baseURL 获取
        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 3. 尝试从 settings_config.apiEndpoint 获取
        if let Some(url) = provider
            .settings_config
            .get("apiEndpoint")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 4. 尝试从 provider.baseUrl 获取
        if let Some(provider_config) = provider.settings_config.get("provider") {
            if let Some(url) = provider_config.get("baseUrl").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
            if let Some(url) = provider_config.get("base_url").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 5. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("OPENAI_BASE_URL").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        Err(ProxyError::ConfigError(
            "OpenCode Provider missing base_url configuration".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        // 1. 尝试从 settings_config.api_key 获取
        if let Some(key) = provider
            .settings_config
            .get("api_key")
            .and_then(|v| v.as_str())
        {
            if !key.is_empty() {
                return Some(AuthInfo {
                    api_key: key.to_string(),
                    strategy: AuthStrategy::Bearer,
                    access_token: None,
                });
            }
        }

        // 2. 尝试从 settings_config.apiKey 获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .and_then(|v| v.as_str())
        {
            if !key.is_empty() {
                return Some(AuthInfo {
                    api_key: key.to_string(),
                    strategy: AuthStrategy::Bearer,
                    access_token: None,
                });
            }
        }

        // 3. 尝试从 provider.apiKey 获取
        if let Some(provider_config) = provider.settings_config.get("provider") {
            if let Some(key) = provider_config.get("apiKey").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(AuthInfo {
                        api_key: key.to_string(),
                        strategy: AuthStrategy::Bearer,
                        access_token: None,
                    });
                }
            }
            if let Some(key) = provider_config.get("api_key").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(AuthInfo {
                        api_key: key.to_string(),
                        strategy: AuthStrategy::Bearer,
                        access_token: None,
                    });
                }
            }
        }

        // 4. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = env.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(AuthInfo {
                        api_key: key.to_string(),
                        strategy: AuthStrategy::Bearer,
                        access_token: None,
                    });
                }
            }
        }

        None
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let endpoint = if endpoint.is_empty() {
            "/v1/chat/completions"
        } else {
            endpoint
        };

        if base_url.ends_with('/') {
            format!("{}{}", base_url.trim_end_matches('/'), endpoint)
        } else {
            format!("{}{}", base_url, endpoint)
        }
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        let value = super::adapter::auth_header_value(&format!("Bearer {}", auth.api_key))?;
        Ok(vec![(http::header::AUTHORIZATION, value)])
    }

    fn needs_transform(&self, _provider: &Provider) -> bool {
        false
    }

    fn transform_request(&self, body: Value, _provider: &Provider) -> Result<Value, ProxyError> {
        Ok(body)
    }

    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        Ok(body)
    }
}
