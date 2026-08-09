//! 统一模型别名管理
//!
//! 管理虚拟模型别名，保护会话在供应商切换时的连续性

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 统一模型别名配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAliasConfig {
    /// 是否启用统一别名
    pub enabled: bool,

    /// 统一别名名称（Agent 看到的模型名）
    pub alias_name: String,

    /// 实际供应商映射（alias_name -> 实际供应商 ID）
    #[serde(default)]
    pub provider_mapping: HashMap<String, String>,

    /// 是否自动隐藏上游模型名
    pub hide_upstream_model: bool,
}

impl Default for UnifiedAliasConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            alias_name: "auto".to_string(),
            provider_mapping: HashMap::new(),
            hide_upstream_model: true,
        }
    }
}

/// 模型别名管理器
pub struct ModelAliasManager {
    config: UnifiedAliasConfig,
    /// 当前活跃的供应商（alias -> provider_id）
    current_provider: Arc<RwLock<HashMap<String, String>>>,
    /// 供应商池（可用供应商列表）
    provider_pool: Vec<String>,
}

impl ModelAliasManager {
    /// 创建新的别名管理器
    pub fn new(config: UnifiedAliasConfig, provider_pool: Vec<String>) -> Self {
        Self {
            config,
            current_provider: Arc::new(RwLock::new(HashMap::new())),
            provider_pool,
        }
    }

    /// 检查模型名是否为统一别名
    pub fn is_unified_alias(&self, model_name: &str) -> bool {
        if !self.config.enabled {
            return false;
        }
        model_name == self.config.alias_name
    }

    /// 获取当前活跃的供应商
    pub async fn get_current_provider(&self, alias: &str) -> Option<String> {
        let current = self.current_provider.read().await;
        current.get(alias).cloned()
    }

    /// 设置当前活跃的供应商
    pub async fn set_current_provider(&self, alias: &str, provider_id: String) {
        let mut current = self.current_provider.write().await;
        current.insert(alias.to_string(), provider_id);
    }

    /// 获取下一个可用供应商
    pub fn get_next_provider(&self, current_provider_id: &str) -> Option<String> {
        if self.provider_pool.is_empty() {
            return None;
        }

        // 找到当前供应商的索引
        let current_idx = self
            .provider_pool
            .iter()
            .position(|id| id == current_provider_id);

        // 切换到下一个
        let next_idx = match current_idx {
            Some(idx) => (idx + 1) % self.provider_pool.len(),
            None => 0, // 当前不在池中，使用第一个
        };

        self.provider_pool.get(next_idx).cloned()
    }

    /// 轮换供应商（用于故障转移）
    pub async fn rotate_provider(&self, alias: &str) -> Option<String> {
        let current = self.get_current_provider(alias).await;

        if let Some(current_id) = current {
            if let Some(next_id) = self.get_next_provider(&current_id) {
                self.set_current_provider(alias, next_id.clone()).await;
                return Some(next_id);
            }
        }

        // 没有当前供应商，使用池中第一个
        if let Some(first_id) = self.provider_pool.first() {
            self.set_current_provider(alias, first_id.clone()).await;
            return Some(first_id.clone());
        }

        None
    }

    /// 重置到主供应商（池中第一个）
    pub async fn reset_to_primary(&self, alias: &str) -> Option<String> {
        if let Some(primary_id) = self.provider_pool.first() {
            self.set_current_provider(alias, primary_id.clone()).await;
            return Some(primary_id.clone());
        }
        None
    }

    /// 映射模型名（将别名转换为实际模型）
    ///
    /// 返回：Some(实际模型名) 或 None（不需要映射）
    pub fn map_model_name(&self, requested_model: &str) -> Option<String> {
        if self.is_unified_alias(requested_model) {
            // 返回别名本身（让代理层处理实际路由）
            Some(requested_model.to_string())
        } else {
            None // 不是别名，不映射
        }
    }

    /// 隐藏上游模型名（在响应中替换为别名）
    pub fn hide_upstream_model(&self, upstream_model: &str) -> String {
        if self.config.hide_upstream_model && self.config.enabled {
            self.config.alias_name.clone()
        } else {
            upstream_model.to_string()
        }
    }

    /// 获取配置
    pub fn get_config(&self) -> &UnifiedAliasConfig {
        &self.config
    }

    /// 更新供应商池
    pub fn update_provider_pool(&mut self, new_pool: Vec<String>) {
        self.provider_pool = new_pool;
    }

    /// 获取供应商池
    pub fn get_provider_pool(&self) -> &[String] {
        &self.provider_pool
    }
}

/// 会话兼容性管理器
///
/// 确保会话历史在不同供应商间保持兼容
pub struct SessionCompatibilityManager {
    /// 支持的供应商特性
    pub capabilities: HashMap<String, ProviderCapabilities>,
}

/// 供应商能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// 是否支持 thinking 块
    pub supports_thinking: bool,

    /// 是否支持工具调用
    pub supports_tool_calls: bool,

    /// 最大上下文长度
    pub max_context_length: usize,

    /// 供应商类型（anthropic | openai | custom）
    pub provider_type: String,
}

impl SessionCompatibilityManager {
    /// 创建新的兼容性管理器
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
        }
    }

    /// 注册供应商能力
    pub fn register_capabilities(&mut self, provider_id: String, capabilities: ProviderCapabilities) {
        self.capabilities.insert(provider_id, capabilities);
    }

    /// 检查会话历史是否兼容
    pub fn is_history_compatible(
        &self,
        source_provider: &str,
        target_provider: &str,
        history_contains_thinking: bool,
        history_contains_tools: bool,
    ) -> bool {
        let source = match self.capabilities.get(source_provider) {
            Some(cap) => cap,
            None => return true, // 未知源供应商，假设兼容
        };

        let target = match self.capabilities.get(target_provider) {
            Some(cap) => cap,
            None => return true, // 未知目标供应商，假设兼容
        };

        // 检查 thinking 兼容性
        if history_contains_thinking && !target.supports_thinking {
            return false;
        }

        // 检查工具调用兼容性
        if history_contains_tools && !target.supports_tool_calls {
            return false;
        }

        true
    }

    /// 获取供应商能力
    pub fn get_capabilities(&self, provider_id: &str) -> Option<&ProviderCapabilities> {
        self.capabilities.get(provider_id)
    }

    /// 清理会话历史（移除不兼容的内容）
    pub fn sanitize_history(
        &self,
        history: &serde_json::Value,
        target_provider: &str,
    ) -> serde_json::Value {
        let target_caps = match self.capabilities.get(target_provider) {
            Some(cap) => cap,
            None => return history.clone(), // 未知目标，不清理
        };

        let mut sanitized = history.clone();

        // 如果目标不支持 thinking，清理 thinking 块
        if !target_caps.supports_thinking {
            sanitized = self.remove_thinking_blocks(sanitized);
        }

        // 如果目标不支持工具调用，清理工具调用
        if !target_caps.supports_tool_calls {
            sanitized = self.remove_tool_calls(sanitized);
        }

        sanitized
    }

    /// 移除 thinking 块
    fn remove_thinking_blocks(&self, mut value: serde_json::Value) -> serde_json::Value {
        if let Some(content) = value.get_mut("content") {
            if let Some(arr) = content.as_array_mut() {
                arr.retain(|item| {
                    if let Some(text_item) = item.as_object() {
                        if let Some(block_type) = text_item.get("type") {
                            return block_type != "thinking";
                        }
                    }
                    true
                });
            }
        }
        value
    }

    /// 移除工具调用
    fn remove_tool_calls(&self, mut value: serde_json::Value) -> serde_json::Value {
        if let Some(content) = value.get_mut("content") {
            if let Some(arr) = content.as_array_mut() {
                arr.retain(|item| {
                    if let Some(text_item) = item.as_object() {
                        if let Some(block_type) = text_item.get("type") {
                            return block_type != "tool_use";
                        }
                    }
                    true
                });
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_alias_detection() {
        let config = UnifiedAliasConfig {
            enabled: true,
            alias_name: "auto".to_string(),
            ..Default::default()
        };

        let manager = ModelAliasManager::new(config, vec![
            "provider1".to_string(),
            "provider2".to_string(),
        ]);

        assert!(manager.is_unified_alias("auto"));
        assert!(!manager.is_unified_alias("gpt-4"));
    }

    #[test]
    fn test_provider_rotation() {
        let config = UnifiedAliasConfig {
            enabled: true,
            alias_name: "auto".to_string(),
            ..Default::default()
        };

        let manager = ModelAliasManager::new(
            config,
            vec!["provider1".to_string(), "provider2".to_string(), "provider3".to_string()],
        );

        // 初始状态
        assert!(manager.get_current_provider("auto").await.is_none());

        // 设置初始供应商
        manager.set_current_provider("auto", "provider1".to_string()).await;
        assert_eq!(manager.get_current_provider("auto").await, Some("provider1".to_string()));

        // 轮换到下一个
        let next = manager.rotate_provider("auto").await;
        assert_eq!(next, Some("provider2".to_string()));

        // 再次轮换
        let next = manager.rotate_provider("auto").await;
        assert_eq!(next, Some("provider3".to_string()));

        // 循环回到第一个
        let next = manager.rotate_provider("auto").await;
        assert_eq!(next, Some("provider1".to_string()));
    }

    #[test]
    fn test_session_compatibility() {
        let mut manager = SessionCompatibilityManager::new();

        // 注册供应商能力
        manager.register_capabilities(
            "provider1".to_string(),
            ProviderCapabilities {
                supports_thinking: true,
                supports_tool_calls: true,
                max_context_length: 200000,
                provider_type: "anthropic".to_string(),
            },
        );

        manager.register_capabilities(
            "provider2".to_string(),
            ProviderCapabilities {
                supports_thinking: false,
                supports_tool_calls: true,
                max_context_length: 128000,
                provider_type: "openai".to_string(),
            },
        );

        // 测试兼容性
        assert!(manager.is_history_compatible(
            "provider1",
            "provider2",
            false,
            true,
        ));

        // 包含 thinking 的历史不应该兼容不支持 thinking 的供应商
        assert!(!manager.is_history_compatible(
            "provider1",
            "provider2",
            true,
            false,
        ));
    }

    #[test]
    fn test_history_sanitization() {
        let mut manager = SessionCompatibilityManager::new();

        manager.register_capabilities(
            "openai_provider".to_string(),
            ProviderCapabilities {
                supports_thinking: false,
                supports_tool_calls: true,
                max_context_length: 128000,
                provider_type: "openai".to_string(),
            },
        );

        // 测试 thinking 块清理
        let history_with_thinking = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "thinking", "thinking": "This is internal"}
            ]
        });

        let sanitized = manager.sanitize_history(&history_with_thinking, "openai_provider");

        // 验证 thinking 块被移除
        assert!(sanitized["content"].as_array().unwrap().len() == 1);
    }
}
