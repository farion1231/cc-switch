//! Vertex AI Provider Adapter
//!
//! 支持 API Key 和服务账号 (Service Account) 两种认证方式
//!
//! ## 认证模式
//! - **API Key**: 使用 API Key 认证 (key=xxx)
//! - **Service Account**: 使用 GCP 服务账号 JSON 文件认证 (Bearer token)
//!
//! ## 请求模式
//! - **Gemini**: Google Gemini 模型
//! - **Claude**: Anthropic Claude 模型
//! - **OpenSource**: 开源模型

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use futures::future::BoxFuture;
use gcp_auth::{CustomServiceAccount, Token, TokenProvider};
use once_cell::sync::Lazy;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局 Token 缓存
static TOKEN_CACHE: Lazy<Arc<RwLock<HashMap<String, Arc<Token>>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Vertex AI 适配器
pub struct VertexAdapter;

/// 请求模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMode {
    /// Gemini 模型
    Gemini,
    /// Claude 模型
    Claude,
    /// 开源模型
    OpenSource,
}

/// 服务账号凭证
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountCredentials {
    #[serde(rename = "type")]
    pub account_type: String,
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
}

impl VertexAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 检测请求模式
    pub fn detect_request_mode(&self, model_name: &str) -> RequestMode {
        if model_name.starts_with("claude") {
            RequestMode::Claude
        } else if model_name.contains("llama") || model_name.contains("-maas") {
            RequestMode::OpenSource
        } else {
            RequestMode::Gemini
        }
    }

    /// 检测是否使用服务账号认证
    fn is_service_account(&self, provider: &Provider) -> bool {
        if let Some(key) = self.extract_key_raw(provider) {
            // 服务账号 JSON 以 { 开头
            return key.trim().starts_with('{');
        }
        false
    }

    /// 解析服务账号凭证
    fn parse_service_account(&self, json_str: &str) -> Result<ServiceAccountCredentials, ProxyError> {
        serde_json::from_str(json_str).map_err(|e| {
            ProxyError::ConfigError(format!("解析服务账号 JSON 失败: {}", e))
        })
    }

    /// 获取或刷新服务账号 Token
    pub async fn get_service_account_token(
        &self,
        provider_id: &str,
        json_str: &str,
    ) -> Result<Arc<Token>, ProxyError> {
        // 检查缓存
        {
            let cache = TOKEN_CACHE.read().await;
            if let Some(token) = cache.get(provider_id) {
                // 检查是否过期
                if !token.has_expired() {
                    log::debug!("[Vertex] 使用缓存的 token for provider: {}", provider_id);
                    return Ok(token.clone());
                }
            }
        }

        // Token 过期或不存在，重新获取
        log::info!("[Vertex] 获取新的服务账号 token for provider: {}", provider_id);

        let creds = CustomServiceAccount::from_json(json_str).map_err(|e| {
            ProxyError::ConfigError(format!("加载服务账号失败: {}", e))
        })?;

        let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
        let token = creds.token(scopes).await.map_err(|e| {
            ProxyError::ConfigError(format!("获取服务账号 token 失败: {}", e))
        })?;

        // 更新缓存
        {
            let mut cache = TOKEN_CACHE.write().await;
            cache.insert(provider_id.to_string(), token.clone());
        }

        Ok(token)
    }

    /// 从 Provider 配置中提取原始 API Key 或服务账号 JSON
    fn extract_key_raw(&self, provider: &Provider) -> Option<String> {
        // 优先从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = env.get("VERTEX_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
            if let Some(key) = env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
            if let Some(key) = env.get("GOOGLE_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }

        // 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            return Some(key.to_string());
        }

        None
    }

    /// 从服务账号 JSON 中提取 project_id
    fn extract_project_id(&self, json_str: &str) -> Option<String> {
        if let Ok(creds) = self.parse_service_account(json_str) {
            return Some(creds.project_id);
        }
        None
    }

    /// 从 Provider 配置中提取区域
    fn extract_region(&self, provider: &Provider) -> Option<String> {
        // 从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(region) = env.get("VERTEX_REGION").and_then(|v| v.as_str()) {
                return Some(region.to_string());
            }
        }

        // 从顶层配置获取
        if let Some(region) = provider
            .settings_config
            .get("region")
            .and_then(|v| v.as_str())
        {
            return Some(region.to_string());
        }

        None
    }

    /// 构建 Vertex AI 请求 URL
    ///
    /// 参考 Go 代码的 getRequestUrl 方法实现
    pub fn build_vertex_url(
        &self,
        provider: &Provider,
        model_name: &str,
        suffix: &str,
    ) -> Result<String, ProxyError> {
        let request_mode = self.detect_request_mode(model_name);
        let region = self.extract_region(provider).unwrap_or_else(|| "global".to_string());
        let is_service_account = self.is_service_account(provider);

        // 服务账号模式需要 project_id
        if is_service_account {
            let key = self.extract_key_raw(provider).ok_or_else(|| {
                ProxyError::ConfigError("未找到服务账号配置".to_string())
            })?;

            let project_id = self.extract_project_id(&key).ok_or_else(|| {
                ProxyError::ConfigError("无法从服务账号 JSON 中提取 project_id".to_string())
            })?;

            let url = match request_mode {
                RequestMode::Gemini => {
                    if region == "global" {
                        format!(
                            "https://aiplatform.googleapis.com/v1/projects/{}/locations/global/publishers/google/models/{}:{}",
                            project_id, model_name, suffix
                        )
                    } else {
                        format!(
                            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:{}",
                            region, project_id, region, model_name, suffix
                        )
                    }
                }
                RequestMode::Claude => {
                    if region == "global" {
                        format!(
                            "https://aiplatform.googleapis.com/v1/projects/{}/locations/global/publishers/anthropic/models/{}:{}",
                            project_id, model_name, suffix
                        )
                    } else {
                        format!(
                            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/anthropic/models/{}:{}",
                            region, project_id, region, model_name, suffix
                        )
                    }
                }
                RequestMode::OpenSource => {
                    format!(
                        "https://aiplatform.googleapis.com/v1beta1/projects/{}/locations/{}/endpoints/openapi/chat/completions",
                        project_id, region
                    )
                }
            };

            Ok(url)
        } else {
            // API Key 模式（快速模式）
            let key_suffix = if suffix.contains('?') { "&" } else { "?" };
            let api_key = self.extract_key_raw(provider).ok_or_else(|| {
                ProxyError::ConfigError("未找到 API Key".to_string())
            })?;

            let url = if region == "global" {
                format!(
                    "https://aiplatform.googleapis.com/v1/publishers/google/models/{}:{}{}key={}",
                    model_name, suffix, key_suffix, api_key
                )
            } else {
                format!(
                    "https://{}-aiplatform.googleapis.com/v1/publishers/google/models/{}:{}{}key={}",
                    region, model_name, suffix, key_suffix, api_key
                )
            };

            Ok(url)
        }
    }
}

impl Default for VertexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for VertexAdapter {
    fn name(&self) -> &'static str {
        "Vertex"
    }

    fn extract_base_url(&self, _provider: &Provider) -> Result<String, ProxyError> {
        // Vertex AI 不需要传统的 base_url，URL 由 build_url_with_provider 动态构建
        // 这里返回一个占位符
        Ok("https://aiplatform.googleapis.com".to_string())
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let key = self.extract_key_raw(provider)?;

        if self.is_service_account(provider) {
            // 服务账号模式
            Some(AuthInfo::new(key, AuthStrategy::GoogleOAuth))
        } else {
            // API Key 模式
            Some(AuthInfo::new(key, AuthStrategy::Google))
        }
    }

    fn build_url(&self, _base_url: &str, endpoint: &str) -> String {
        // Vertex AI 的 URL 需要根据 provider 配置动态构建
        // 这个方法不应该被调用，实际使用 build_url_with_provider
        format!("https://aiplatform.googleapis.com{}", endpoint)
    }

    fn build_url_with_provider(
        &self,
        provider: Option<&Provider>,
        _base_url: &str,
        endpoint: &str,
    ) -> String {
        let provider = match provider {
            Some(p) => p,
            None => return self.build_url(_base_url, endpoint),
        };

        // 从 endpoint 中提取模型名称和 suffix
        // endpoint 格式示例: /v1/models/gemini-pro:generateContent
        let parts: Vec<&str> = endpoint.split('/').collect();

        // 查找 models/ 后面的部分
        let mut model_and_suffix = "";
        for (i, part) in parts.iter().enumerate() {
            if *part == "models" && i + 1 < parts.len() {
                model_and_suffix = parts[i + 1];
                break;
            }
        }

        if model_and_suffix.is_empty() {
            log::warn!("[Vertex] 无法从 endpoint 提取模型信息: {}", endpoint);
            return self.build_url(_base_url, endpoint);
        }

        // 分离模型名称和 suffix (如 gemini-pro:generateContent)
        let (model_name, suffix) = if let Some(pos) = model_and_suffix.find(':') {
            let (model, suf) = model_and_suffix.split_at(pos);
            (model, &suf[1..]) // 去掉冒号
        } else {
            (model_and_suffix, "generateContent") // 默认 suffix
        };

        // 使用 build_vertex_url 构建完整 URL
        match self.build_vertex_url(provider, model_name, suffix) {
            Ok(url) => url,
            Err(e) => {
                log::error!("[Vertex] 构建 URL 失败: {}", e);
                self.build_url(_base_url, endpoint)
            }
        }
    }

    fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
        match auth.strategy {
            // 服务账号 Bearer 认证
            // Token 会在 add_auth_headers_async 中异步获取并设置
            AuthStrategy::GoogleOAuth => request,
            // API Key 认证已经在 URL 中，不需要额外的 header
            _ => request,
        }
    }

    fn add_auth_headers_async<'a>(
        &'a self,
        provider: &'a Provider,
        request: RequestBuilder,
    ) -> BoxFuture<'a, Result<RequestBuilder, ProxyError>> {
        Box::pin(async move {
            if let Some(auth) = self.extract_auth(provider) {
                match auth.strategy {
                    AuthStrategy::GoogleOAuth => {
                        // 服务账号模式：异步获取 token
                        let token = self.get_service_account_token(&provider.id, &auth.api_key).await?;
                        let mut req = request.header("Authorization", format!("Bearer {}", token.as_str()));

                        // 添加 x-goog-user-project header
                        if let Some(project_id) = self.extract_project_id(&auth.api_key) {
                            req = req.header("x-goog-user-project", project_id);
                        }

                        Ok(req)
                    }
                    _ => {
                        // API Key 模式：已经在 URL 中，不需要额外处理
                        Ok(request)
                    }
                }
            } else {
                Ok(request)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test-vertex".to_string(),
            name: "Test Vertex".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("vertex".to_string()),
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
    fn test_detect_request_mode() {
        let adapter = VertexAdapter::new();

        assert_eq!(
            adapter.detect_request_mode("claude-3-sonnet"),
            RequestMode::Claude
        );
        assert_eq!(
            adapter.detect_request_mode("gemini-pro"),
            RequestMode::Gemini
        );
        assert_eq!(
            adapter.detect_request_mode("llama-3-70b"),
            RequestMode::OpenSource
        );
        assert_eq!(
            adapter.detect_request_mode("model-maas"),
            RequestMode::OpenSource
        );
    }

    #[test]
    fn test_is_service_account() {
        let adapter = VertexAdapter::new();

        // API Key
        let api_key_provider = create_provider(json!({
            "env": {
                "VERTEX_API_KEY": "AIza-test-key"
            }
        }));
        assert!(!adapter.is_service_account(&api_key_provider));

        // Service Account JSON
        let sa_provider = create_provider(json!({
            "env": {
                "VERTEX_API_KEY": r#"{"type":"service_account","project_id":"test"}"#
            }
        }));
        assert!(adapter.is_service_account(&sa_provider));
    }

    #[test]
    fn test_extract_region() {
        let adapter = VertexAdapter::new();

        // 从 env 中提取
        let provider1 = create_provider(json!({
            "env": {
                "VERTEX_REGION": "us-central1"
            }
        }));
        assert_eq!(adapter.extract_region(&provider1), Some("us-central1".to_string()));

        // 从顶层配置提取
        let provider2 = create_provider(json!({
            "region": "europe-west1"
        }));
        assert_eq!(adapter.extract_region(&provider2), Some("europe-west1".to_string()));

        // 未配置
        let provider3 = create_provider(json!({}));
        assert_eq!(adapter.extract_region(&provider3), None);
    }
}
