//! 转换器注册表
//!
//! 管理和获取格式转换器

use super::{format::ApiFormat, traits::FormatTransformer};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

/// 转换器注册表
pub struct TransformerRegistry {
    transformers: HashMap<(ApiFormat, ApiFormat), Arc<dyn FormatTransformer>>,
}

impl TransformerRegistry {
    /// 创建新的注册表
    pub fn new() -> Self {
        let mut registry = Self {
            transformers: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// 注册默认转换器
    fn register_defaults(&mut self) {
        use super::anthropic_openai::{AnthropicToOpenAITransformer, OpenAIToAnthropicTransformer};

        // Anthropic → OpenAI
        self.register(Arc::new(AnthropicToOpenAITransformer::new()));

        // OpenAI → Anthropic
        self.register(Arc::new(OpenAIToAnthropicTransformer::new()));
    }

    /// 注册转换器
    pub fn register(&mut self, transformer: Arc<dyn FormatTransformer>) {
        let key = (transformer.source_format(), transformer.target_format());
        self.transformers.insert(key, transformer);
    }

    /// 获取转换器
    pub fn get(&self, source: ApiFormat, target: ApiFormat) -> Option<Arc<dyn FormatTransformer>> {
        self.transformers.get(&(source, target)).cloned()
    }

    /// 检查是否支持指定的转换
    #[cfg(test)]
    pub fn supports(&self, source: ApiFormat, target: ApiFormat) -> bool {
        self.transformers.contains_key(&(source, target))
    }
}

impl Default for TransformerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局转换器注册表
pub static TRANSFORMER_REGISTRY: LazyLock<TransformerRegistry> =
    LazyLock::new(TransformerRegistry::new);

/// 获取转换器的便捷函数
pub fn get_transformer(source: ApiFormat, target: ApiFormat) -> Option<Arc<dyn FormatTransformer>> {
    TRANSFORMER_REGISTRY.get(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_default_transformers() {
        let registry = TransformerRegistry::new();

        // Anthropic → OpenAI
        assert!(registry.supports(ApiFormat::Anthropic, ApiFormat::OpenAI));

        // OpenAI → Anthropic
        assert!(registry.supports(ApiFormat::OpenAI, ApiFormat::Anthropic));

        // 不支持的转换
        assert!(!registry.supports(ApiFormat::Gemini, ApiFormat::OpenAI));
    }

    #[test]
    fn test_get_transformer() {
        let transformer = get_transformer(ApiFormat::Anthropic, ApiFormat::OpenAI);
        assert!(transformer.is_some());

        let t = transformer.unwrap();
        assert_eq!(t.source_format(), ApiFormat::Anthropic);
        assert_eq!(t.target_format(), ApiFormat::OpenAI);
    }
}
