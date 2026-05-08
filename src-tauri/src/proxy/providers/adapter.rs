//! Provider Adapter Trait
//!
//! 定义供应商适配器的统一接口，抽象不同上游供应商的处理逻辑。

use super::auth::AuthInfo;
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::Value;

/// 供应商适配器 Trait
///
/// 所有供应商适配器都需要实现此 trait，提供统一的接口来处理：
/// - URL 构建
/// - 认证信息提取和头部注入
/// - 请求/响应格式转换（可选）
pub trait ProviderAdapter: Send + Sync {
    /// 适配器名称（用于日志和调试）
    fn name(&self) -> &'static str;

    /// 从 Provider 配置中提取 base_url
    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError>;

    /// 从 Provider 配置中提取认证信息
    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo>;

    /// 构建请求 URL
    fn build_url(&self, base_url: &str, endpoint: &str) -> String;

    /// Return auth headers as `(name, value)` pairs.
    ///
    /// The forwarder inserts these at the position of the original auth header
    /// so that header order is preserved.
    fn get_auth_headers(&self, auth: &AuthInfo) -> Vec<(http::HeaderName, http::HeaderValue)>;

    /// 是否需要格式转换
    fn needs_transform(&self, _provider: &Provider) -> bool {
        false
    }

    /// 转换请求体
    fn transform_request(&self, body: Value, _provider: &Provider) -> Result<Value, ProxyError> {
        Ok(body)
    }

    /// 转换响应体
    #[allow(dead_code)]
    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        Ok(body)
    }
}
