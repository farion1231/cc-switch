//! Provider Adapter Trait
//!
//! 定义供应商适配器的统一接口，抽象不同上游供应商的处理逻辑。

use super::auth::AuthInfo;
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use futures::future::BoxFuture;
use reqwest::RequestBuilder;
use serde_json::Value;

/// 供应商适配器 Trait
///
/// 所有供应商适配器都需要实现此 trait，提供统一的接口来处理：
/// - URL 构建
/// - 认证信息提取和头部注入
/// - 请求/响应格式转换（可选）
///
/// # 示例
///
/// ```ignore
/// pub struct ClaudeAdapter;
///
/// impl ProviderAdapter for ClaudeAdapter {
///     fn name(&self) -> &'static str { "Claude" }
///     
///     fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
///         // 从 provider 配置中提取 base_url
///     }
///     
///     fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
///         // 从 provider 配置中提取认证信息
///     }
///     
///     fn build_url(&self, base_url: &str, endpoint: &str) -> String {
///         format!("{}{}", base_url.trim_end_matches('/'), endpoint)
///     }
///     
///     fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
///         // 添加认证头
///     }
/// }
/// ```
pub trait ProviderAdapter: Send + Sync {
    /// 适配器名称（用于日志和调试）
    fn name(&self) -> &'static str;

    /// 从 Provider 配置中提取 base_url
    ///
    /// # Arguments
    /// * `provider` - Provider 配置
    ///
    /// # Returns
    /// * `Ok(String)` - 提取到的 base_url（已去除尾部斜杠）
    /// * `Err(ProxyError)` - 提取失败
    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError>;

    /// 从 Provider 配置中提取认证信息
    ///
    /// # Arguments
    /// * `provider` - Provider 配置
    ///
    /// # Returns
    /// * `Some(AuthInfo)` - 提取到的认证信息
    /// * `None` - 未找到认证信息
    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo>;

    /// 构建请求 URL
    ///
    /// # Arguments
    /// * `base_url` - 基础 URL
    /// * `endpoint` - 请求端点（如 `/v1/messages`）
    ///
    /// # Returns
    /// 完整的请求 URL
    fn build_url(&self, base_url: &str, endpoint: &str) -> String;

    /// 构建请求 URL（支持传入 Provider 配置）
    ///
    /// 默认实现调用 `build_url`，供应商可以重写此方法以支持更复杂的 URL 构建逻辑。
    /// 例如 Vertex AI 需要根据 provider 配置中的区域、模型类型等动态构建 URL。
    ///
    /// # Arguments
    /// * `provider` - Provider 配置
    /// * `base_url` - 基础 URL
    /// * `endpoint` - 请求端点（如 `/v1/messages`）
    ///
    /// # Returns
    /// 完整的请求 URL
    fn build_url_with_provider(
        &self,
        _provider: Option<&Provider>,
        base_url: &str,
        endpoint: &str,
    ) -> String {
        self.build_url(base_url, endpoint)
    }

    /// 添加认证头到请求
    ///
    /// # Arguments
    /// * `request` - reqwest RequestBuilder
    /// * `auth` - 认证信息
    ///
    /// # Returns
    /// 添加了认证头的 RequestBuilder
    fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder;

    /// 添加认证头到请求（异步版本，支持动态获取 token）
    ///
    /// 默认实现调用同步的 `add_auth_headers` 方法。
    /// 供应商可以重写此方法以支持异步认证（如 Vertex 服务账号 token 获取）。
    ///
    /// # Arguments
    /// * `provider` - Provider 配置
    /// * `request` - reqwest RequestBuilder
    ///
    /// # Returns
    /// 添加了认证头的 RequestBuilder
    fn add_auth_headers_async<'a>(
        &'a self,
        provider: &'a Provider,
        request: RequestBuilder,
    ) -> BoxFuture<'a, Result<RequestBuilder, ProxyError>> {
        Box::pin(async move {
            if let Some(auth) = self.extract_auth(provider) {
                Ok(self.add_auth_headers(request, &auth))
            } else {
                Ok(request)
            }
        })
    }

    /// 是否需要格式转换
    ///
    /// 默认返回 `false`（透传模式）。
    /// 仅当供应商需要格式转换时（如 Claude + OpenRouter 旧 OpenAI 兼容接口）才返回 `true`。
    ///
    /// # Arguments
    /// * `provider` - Provider 配置
    fn needs_transform(&self, _provider: &Provider) -> bool {
        false
    }

    /// 转换请求体
    ///
    /// 将请求体从一种格式转换为另一种格式（如 Anthropic → OpenAI）。
    /// 默认实现直接返回原始请求体（透传）。
    ///
    /// # Arguments
    /// * `body` - 原始请求体
    /// * `provider` - Provider 配置（用于获取模型映射等）
    ///
    /// # Returns
    /// * `Ok(Value)` - 转换后的请求体
    /// * `Err(ProxyError)` - 转换失败
    fn transform_request(&self, body: Value, _provider: &Provider) -> Result<Value, ProxyError> {
        Ok(body)
    }

    /// 转换响应体
    ///
    /// 将响应体从一种格式转换为另一种格式（如 OpenAI → Anthropic）。
    /// 默认实现直接返回原始响应体（透传）。
    ///
    /// # Arguments
    /// * `body` - 原始响应体
    ///
    /// # Returns
    /// * `Ok(Value)` - 转换后的响应体
    /// * `Err(ProxyError)` - 转换失败
    ///
    /// Note: 响应转换将在 handler 层集成，目前预留接口
    #[allow(dead_code)]
    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        Ok(body)
    }
}
