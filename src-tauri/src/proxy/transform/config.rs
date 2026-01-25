//! 格式转换配置
//!
//! 从 Provider 配置中提取格式转换设置

use super::format::ApiFormat;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

/// 格式转换配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformConfig {
    /// 是否启用格式转换
    pub enabled: bool,
    /// 源格式（客户端发送的格式）
    pub source_format: ApiFormat,
    /// 目标格式（上游服务期望的格式）
    pub target_format: ApiFormat,
    /// 是否转换流式响应
    pub transform_streaming: bool,
}

impl Default for TransformConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source_format: ApiFormat::Anthropic,
            target_format: ApiFormat::OpenAI,
            transform_streaming: true,
        }
    }
}

impl TransformConfig {
    /// 从 Provider 配置中提取转换配置
    ///
    /// 优先级：
    /// 1. ProviderMeta.format_transform（新配置格式，通过前端 UI 设置）
    /// 2. settings_config.format_transform（兼容旧配置）
    /// 3. settings_config.openrouter_compat_mode（兼容旧配置）
    ///
    /// 注意：如果格式解析失败，将禁用转换并记录警告，而不是静默回退到默认值
    pub fn from_provider(provider: &Provider) -> Self {
        // 1. 优先从 ProviderMeta 读取（前端 UI 设置的配置）
        if let Some(meta) = &provider.meta {
            if let Some(ft) = &meta.format_transform {
                if ft.enabled {
                    let source_str = ft.source_format.as_deref();
                    let target_str = ft.target_format.as_deref();

                    let source_format = source_str.and_then(ApiFormat::from_str);
                    let target_format = target_str.and_then(ApiFormat::from_str);

                    // 如果格式解析失败，禁用转换并记录警告
                    if source_str.is_some() && source_format.is_none() {
                        log::warn!(
                            "[TransformConfig] 无法解析 source_format: {source_str:?}，禁用格式转换"
                        );
                        return Self::default();
                    }
                    if target_str.is_some() && target_format.is_none() {
                        log::warn!(
                            "[TransformConfig] 无法解析 target_format: {target_str:?}，禁用格式转换"
                        );
                        return Self::default();
                    }

                    let transform_streaming = ft.transform_streaming.unwrap_or(true);

                    return Self {
                        enabled: true,
                        source_format: source_format.unwrap_or(ApiFormat::Anthropic),
                        target_format: target_format.unwrap_or(ApiFormat::OpenAI),
                        transform_streaming,
                    };
                }
            }
        }

        let settings = &provider.settings_config;

        // 2. 检查是否显式启用格式转换（settings_config 中的配置）
        let format_transform = settings.get("format_transform").and_then(|v| v.as_object());

        if let Some(config) = format_transform {
            let enabled = config
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if enabled {
                let source_str = config.get("source_format").and_then(|v| v.as_str());
                let target_str = config.get("target_format").and_then(|v| v.as_str());

                let source_format = source_str.and_then(ApiFormat::from_str);
                let target_format = target_str.and_then(ApiFormat::from_str);

                // 如果格式解析失败，禁用转换并记录警告
                if source_str.is_some() && source_format.is_none() {
                    log::warn!(
                        "[TransformConfig] 无法解析 source_format: {source_str:?}，禁用格式转换"
                    );
                    return Self::default();
                }
                if target_str.is_some() && target_format.is_none() {
                    log::warn!(
                        "[TransformConfig] 无法解析 target_format: {target_str:?}，禁用格式转换"
                    );
                    return Self::default();
                }

                let transform_streaming = config
                    .get("transform_streaming")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                return Self {
                    enabled,
                    source_format: source_format.unwrap_or(ApiFormat::Anthropic),
                    target_format: target_format.unwrap_or(ApiFormat::OpenAI),
                    transform_streaming,
                };
            }
        }

        // 3. 兼容旧配置：检查 openrouter_compat_mode
        let legacy_enabled = settings
            .get("openrouter_compat_mode")
            .and_then(|v| match v {
                serde_json::Value::Bool(b) => Some(*b),
                serde_json::Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
                serde_json::Value::String(s) => {
                    let normalized = s.trim().to_lowercase();
                    Some(normalized == "true" || normalized == "1")
                }
                _ => None,
            })
            .unwrap_or(false);

        if legacy_enabled {
            return Self {
                enabled: true,
                source_format: ApiFormat::Anthropic,
                target_format: ApiFormat::OpenAI,
                transform_streaming: true,
            };
        }

        Self::default()
    }

    /// 检查是否需要转换
    pub fn needs_transform(&self) -> bool {
        self.enabled && self.source_format != self.target_format
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(settings: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Provider".to_string(),
            settings_config: settings,
            website_url: None,
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

    #[test]
    fn test_default_config() {
        let provider = create_provider(json!({}));
        let config = TransformConfig::from_provider(&provider);
        assert!(!config.enabled);
        assert!(!config.needs_transform());
    }

    #[test]
    fn test_new_format_config() {
        let provider = create_provider(json!({
            "format_transform": {
                "enabled": true,
                "source_format": "anthropic",
                "target_format": "openai",
                "transform_streaming": true
            }
        }));
        let config = TransformConfig::from_provider(&provider);
        assert!(config.enabled);
        assert_eq!(config.source_format, ApiFormat::Anthropic);
        assert_eq!(config.target_format, ApiFormat::OpenAI);
        assert!(config.transform_streaming);
        assert!(config.needs_transform());
    }

    #[test]
    fn test_legacy_openrouter_compat_mode_bool() {
        let provider = create_provider(json!({
            "openrouter_compat_mode": true
        }));
        let config = TransformConfig::from_provider(&provider);
        assert!(config.enabled);
        assert_eq!(config.source_format, ApiFormat::Anthropic);
        assert_eq!(config.target_format, ApiFormat::OpenAI);
    }

    #[test]
    fn test_legacy_openrouter_compat_mode_string() {
        let provider = create_provider(json!({
            "openrouter_compat_mode": "true"
        }));
        let config = TransformConfig::from_provider(&provider);
        assert!(config.enabled);
    }

    #[test]
    fn test_legacy_openrouter_compat_mode_number() {
        let provider = create_provider(json!({
            "openrouter_compat_mode": 1
        }));
        let config = TransformConfig::from_provider(&provider);
        assert!(config.enabled);
    }

    #[test]
    fn test_same_format_no_transform() {
        let provider = create_provider(json!({
            "format_transform": {
                "enabled": true,
                "source_format": "anthropic",
                "target_format": "anthropic"
            }
        }));
        let config = TransformConfig::from_provider(&provider);
        assert!(config.enabled);
        assert!(!config.needs_transform()); // 相同格式不需要转换
    }
}
