//! 内置插件（v1：把现有硬编码变换器收编为插件，行为与现状等价）
//!
//! - `builtin:cache-injector`：PreSend，包装 `cache_injector::inject`；
//! - `builtin:thinking-optimizer`：PreSend，包装 `thinking_optimizer::optimize`。
//!
//! 两个插件持有 `Arc<Database>`，每次变换时实时读取 `OptimizerConfig`
//!（读取失败时 log::warn 并跳过，fail-open），保证设置改动即时生效。
//! Bedrock 门在插件内部实现：`ctx.provider` 缺失或 `is_bedrock == false`
//! 时不做任何改写（原 forwarder.rs 挂点处的 `is_bedrock_provider` gate
//! 已下沉到这里）。

use std::sync::Arc;

use super::types::{PluginError, PluginRequestContext, PluginStage};
use super::ProxyPlugin;
use crate::database::Database;
use crate::proxy::types::OptimizerConfig;

/// 读取优化器配置快照；数据库出错时 log::warn 并返回 None（跳过本次变换，fail-open）
fn load_config(db: &Database) -> Option<OptimizerConfig> {
    match db.get_optimizer_config() {
        Ok(config) => Some(config),
        Err(e) => {
            log::warn!("[PLUGIN] 读取优化器配置失败，本次跳过内置优化插件(fail-open): {e}");
            None
        }
    }
}

/// Bedrock 门：provider 缺失或非 Bedrock 时不执行（provider 为 None 视为非 Bedrock）
fn bedrock_gate(ctx: &PluginRequestContext) -> bool {
    ctx.provider
        .as_ref()
        .map(|p| p.is_bedrock)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// builtin:cache-injector
// ---------------------------------------------------------------------------

/// Cache 断点注入插件（PreSend）
pub struct BuiltinCacheInjectorPlugin {
    db: Arc<Database>,
}

impl BuiltinCacheInjectorPlugin {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl ProxyPlugin for BuiltinCacheInjectorPlugin {
    fn id(&self) -> &str {
        "builtin:cache-injector"
    }

    fn display_name(&self) -> &str {
        "Cache 断点注入"
    }

    fn description(&self) -> &str {
        "在请求转发前自动注入 cache_control 标记（仅 Bedrock 供应商生效）"
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn stages(&self) -> &'static [PluginStage] {
        &[PluginStage::PreSend]
    }

    fn default_priority(&self) -> i32 {
        // 现状顺序：先 thinking 后 cache，故 cache 排在 thinking 之后
        200
    }

    fn transform_request(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        if !bedrock_gate(ctx) {
            return Ok(false);
        }
        let Some(config) = load_config(&self.db) else {
            return Ok(false);
        };
        // 与现状一致：optimizer_config.enabled && optimizer_config.cache_injection
        if !config.enabled || !config.cache_injection {
            return Ok(false);
        }
        // inject() 返回 ()，通过前后对比判断是否修改
        let before = body.clone();
        super::super::cache_injector::inject(body, &config);
        Ok(&before != body)
    }
}

// ---------------------------------------------------------------------------
// builtin:thinking-optimizer
// ---------------------------------------------------------------------------

/// Thinking 优化插件（PreSend）
pub struct BuiltinThinkingOptimizerPlugin {
    db: Arc<Database>,
}

impl BuiltinThinkingOptimizerPlugin {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl ProxyPlugin for BuiltinThinkingOptimizerPlugin {
    fn id(&self) -> &str {
        "builtin:thinking-optimizer"
    }

    fn display_name(&self) -> &str {
        "Thinking 优化"
    }

    fn description(&self) -> &str {
        "根据模型类型自动优化 thinking 配置（仅 Bedrock 供应商生效）"
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn stages(&self) -> &'static [PluginStage] {
        &[PluginStage::PreSend]
    }

    fn default_priority(&self) -> i32 {
        // 现状顺序：先 thinking 后 cache
        100
    }

    fn transform_request(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        if !bedrock_gate(ctx) {
            return Ok(false);
        }
        let Some(config) = load_config(&self.db) else {
            return Ok(false);
        };
        // 与现状一致：optimizer_config.thinking_optimizer（受总开关控制的判断
        // 已在 optimize() 内部，这里保持同样语义：总开关关闭时不做任何事）
        if !config.enabled || !config.thinking_optimizer {
            return Ok(false);
        }
        let before = body.clone();
        super::super::thinking_optimizer::optimize(body, &config);
        Ok(&before != body)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::proxy::plugins::types::PluginProviderInfo;

    fn ctx(is_bedrock: bool) -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage: PluginStage::PreSend,
            provider: Some(PluginProviderInfo {
                id: "p1".to_string(),
                name: "Provider 1".to_string(),
                is_bedrock,
            }),
        }
    }

    fn db_with_config(config: &OptimizerConfig) -> Arc<Database> {
        let db = Arc::new(Database::memory().expect("memory db"));
        db.set_optimizer_config(config).expect("set optimizer config");
        db
    }

    fn full_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: true,
            cache_injection: true,
        }
    }

    fn sample_body() -> serde_json::Value {
        json!({
            "model": "claude-sonnet-4",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]}
            ]
        })
    }

    #[test]
    fn test_builtin_metadata() {
        let db = db_with_config(&full_config());
        let cache = BuiltinCacheInjectorPlugin::new(db.clone());
        let thinking = BuiltinThinkingOptimizerPlugin::new(db);

        assert_eq!(cache.id(), "builtin:cache-injector");
        assert!(cache.is_builtin());
        assert_eq!(cache.stages(), &[PluginStage::PreSend]);
        assert!(cache.default_enabled());
        assert_eq!(cache.version(), None);

        assert_eq!(thinking.id(), "builtin:thinking-optimizer");
        assert_eq!(thinking.stages(), &[PluginStage::PreSend]);
        assert_eq!(thinking.default_priority(), 100);
        assert!(thinking.default_priority() < cache.default_priority());
    }

    #[test]
    fn test_non_bedrock_provider_skips() {
        // 非 Bedrock（或 provider 缺失）时不执行，即使配置全开
        let db = db_with_config(&full_config());
        let cache = BuiltinCacheInjectorPlugin::new(db.clone());
        let thinking = BuiltinThinkingOptimizerPlugin::new(db);

        let mut body = sample_body();
        assert!(!cache.transform_request(&ctx(false), &mut body).unwrap());
        assert_eq!(body, sample_body());

        let mut no_provider_ctx = ctx(true);
        no_provider_ctx.provider = None;
        let mut body = sample_body();
        assert!(!cache
            .transform_request(&no_provider_ctx, &mut body)
            .unwrap());
        assert!(!thinking
            .transform_request(&no_provider_ctx, &mut body)
            .unwrap());
        assert_eq!(body, sample_body());
    }

    #[test]
    fn test_cache_injector_disabled_returns_no_modification() {
        // 总开关关闭（默认状态）
        let db = db_with_config(&OptimizerConfig::default());
        let plugin = BuiltinCacheInjectorPlugin::new(db);
        let mut body = sample_body();
        assert!(!plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert_eq!(body, sample_body());

        // 总开关开但 cache 子开关关
        let config = OptimizerConfig {
            enabled: true,
            cache_injection: false,
            ..OptimizerConfig::default()
        };
        let db = db_with_config(&config);
        let plugin = BuiltinCacheInjectorPlugin::new(db);
        let mut body = sample_body();
        assert!(!plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert_eq!(body, sample_body());
    }

    #[test]
    fn test_cache_injector_enabled_injects() {
        let db = db_with_config(&full_config());
        let plugin = BuiltinCacheInjectorPlugin::new(db);
        let mut body = sample_body();
        assert!(plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_thinking_optimizer_disabled_returns_no_modification() {
        let db = db_with_config(&OptimizerConfig::default());
        let plugin = BuiltinThinkingOptimizerPlugin::new(db);
        let mut body = sample_body();
        assert!(!plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert_eq!(body, sample_body());

        let config = OptimizerConfig {
            enabled: true,
            thinking_optimizer: false,
            ..OptimizerConfig::default()
        };
        let db = db_with_config(&config);
        let plugin = BuiltinThinkingOptimizerPlugin::new(db);
        let mut body = sample_body();
        assert!(!plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert_eq!(body, sample_body());
    }

    #[test]
    fn test_thinking_optimizer_enabled_modifies() {
        let db = db_with_config(&full_config());
        let plugin = BuiltinThinkingOptimizerPlugin::new(db);
        let mut body = sample_body();
        assert!(plugin.transform_request(&ctx(true), &mut body).unwrap());
        assert_eq!(body["thinking"]["type"], json!("enabled"));
    }
}
