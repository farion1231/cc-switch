//! API 格式枚举定义
//!
//! 定义支持的 API 格式类型，用于格式转换配置

use serde::{Deserialize, Serialize};

/// API 格式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    /// Anthropic Messages API
    Anthropic,
    /// OpenAI Chat Completions API
    OpenAI,
    /// Google Gemini API (预留)
    Gemini,
}

impl ApiFormat {
    /// 从字符串解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "codex" => Some(Self::OpenAI),
            "gemini" | "google" => Some(Self::Gemini),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Gemini => "gemini",
        }
    }
}

impl std::fmt::Display for ApiFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        assert_eq!(ApiFormat::from_str("anthropic"), Some(ApiFormat::Anthropic));
        assert_eq!(ApiFormat::from_str("claude"), Some(ApiFormat::Anthropic));
        assert_eq!(ApiFormat::from_str("openai"), Some(ApiFormat::OpenAI));
        assert_eq!(ApiFormat::from_str("codex"), Some(ApiFormat::OpenAI));
        assert_eq!(ApiFormat::from_str("gemini"), Some(ApiFormat::Gemini));
        assert_eq!(ApiFormat::from_str("google"), Some(ApiFormat::Gemini));
        assert_eq!(ApiFormat::from_str("unknown"), None);
    }

    #[test]
    fn test_as_str() {
        assert_eq!(ApiFormat::Anthropic.as_str(), "anthropic");
        assert_eq!(ApiFormat::OpenAI.as_str(), "openai");
        assert_eq!(ApiFormat::Gemini.as_str(), "gemini");
    }
}
