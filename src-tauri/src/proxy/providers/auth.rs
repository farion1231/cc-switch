//! Authentication Types
//!
//! 定义认证信息和认证策略，支持多种上游供应商的认证方式。

/// 认证信息
///
/// 包含 API Key 和对应的认证策略
#[derive(Debug, Clone)]
pub struct AuthInfo {
    /// API Key
    pub api_key: String,
    /// 认证策略
    pub strategy: AuthStrategy,
}

impl AuthInfo {
    /// 创建新的认证信息
    pub fn new(api_key: String, strategy: AuthStrategy) -> Self {
        Self { api_key, strategy }
    }

    /// 返回遮蔽后的 API Key（用于日志输出）
    ///
    /// 显示前4位和后4位，中间用 `...` 代替
    /// 如果 key 长度不足8位，则返回 `***`
    pub fn masked_key(&self) -> String {
        if self.api_key.len() > 8 {
            format!(
                "{}...{}",
                &self.api_key[..4],
                &self.api_key[self.api_key.len() - 4..]
            )
        } else {
            "***".to_string()
        }
    }
}

/// 认证策略
///
/// 不同供应商使用不同的认证方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStrategy {
    /// Anthropic 认证方式
    /// - Header: `x-api-key: <api_key>`
    /// - Header: `anthropic-version: 2023-06-01`
    Anthropic,

    /// Bearer Token 认证方式（OpenAI 等）
    /// - Header: `Authorization: Bearer <api_key>`
    Bearer,

    /// Google 认证方式
    /// - Header: `x-goog-api-key: <api_key>`
    Google,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_masked_key_long() {
        let auth = AuthInfo::new("sk-1234567890abcdef".to_string(), AuthStrategy::Bearer);
        assert_eq!(auth.masked_key(), "sk-1...cdef");
    }

    #[test]
    fn test_masked_key_short() {
        let auth = AuthInfo::new("short".to_string(), AuthStrategy::Bearer);
        assert_eq!(auth.masked_key(), "***");
    }

    #[test]
    fn test_masked_key_exactly_8() {
        let auth = AuthInfo::new("12345678".to_string(), AuthStrategy::Bearer);
        assert_eq!(auth.masked_key(), "***");
    }

    #[test]
    fn test_masked_key_9_chars() {
        let auth = AuthInfo::new("123456789".to_string(), AuthStrategy::Bearer);
        assert_eq!(auth.masked_key(), "1234...6789");
    }

    #[test]
    fn test_auth_strategy_equality() {
        assert_eq!(AuthStrategy::Anthropic, AuthStrategy::Anthropic);
        assert_ne!(AuthStrategy::Anthropic, AuthStrategy::Bearer);
        assert_ne!(AuthStrategy::Bearer, AuthStrategy::Google);
    }
}
