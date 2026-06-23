//! Codex 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换 Codex 请求中的模型名称
//! 支持简单映射和 effort 组合映射

use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Codex 模型映射配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexModelMappingConfig {
    /// 是否启用模型映射
    #[serde(default)]
    pub enabled: bool,

    /// 简单模型映射：请求模型 → 目标模型
    /// Key: 请求模型ID, Value: 目标模型ID
    #[serde(default, rename = "modelMap")]
    pub model_map: HashMap<String, String>,

    /// Effort 组合映射：(模型, effort) → 目标模型
    /// Key: "模型ID@effort", Value: 目标模型ID
    /// 例: "gpt-5.2-codex@xhigh" → "gpt-5.2-codex-xhigh"
    #[serde(default, rename = "effortMap")]
    pub effort_map: HashMap<String, String>,
}

impl CodexModelMappingConfig {
    /// 检查是否有任何映射规则
    pub fn has_mapping(&self) -> bool {
        self.enabled && (!self.model_map.is_empty() || !self.effort_map.is_empty())
    }

    /// 根据模型和effort查找映射
    ///
    /// 优先级：
    /// 1. model_map (简单模型映射) - 匹配模型ID后直接映射，不管有没有 effort
    /// 2. effort_map (模型+effort 组合) - 更细粒度的覆盖，仅当简单映射未匹配时使用
    /// 3. 无映射，返回原模型
    pub fn map_model(&self, original_model: &str, effort: Option<&str>) -> (String, bool) {
        if !self.enabled {
            return (original_model.to_string(), false);
        }

        // 1. 优先尝试简单模型映射
        if let Some(mapped) = self.model_map.get(original_model) {
            return (mapped.clone(), true);
        }

        // 2. 如果简单映射没匹配，尝试 effort 组合映射（更细粒度）
        if let Some(eff) = effort {
            let key = format!("{}@{}", original_model, eff);
            if let Some(mapped) = self.effort_map.get(&key) {
                return (mapped.clone(), true);
            }
        }

        // 3. 无映射
        (original_model.to_string(), false)
    }
}

/// 从 Provider 提取 Codex 模型映射配置
pub fn extract_mapping_config(provider: &Provider) -> CodexModelMappingConfig {
    provider
        .meta
        .as_ref()
        .and_then(|m| m.codex_model_mapping.as_ref())
        .cloned()
        .unwrap_or_default()
}

/// 从请求体中提取 effort 值
pub fn extract_effort(body: &Value) -> Option<String> {
    body.get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(|e| e.as_str())
        .map(String::from)
}

/// 应用 Codex 模型映射
///
/// 返回 (映射后的请求体, 原始模型, 映射后模型)
pub fn apply_codex_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = extract_mapping_config(provider);

    // 如果没有配置映射，直接返回
    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 提取原始模型名和 effort
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);
    let effort = extract_effort(&body);

    if let Some(ref original) = original_model {
        let (mapped, did_map) = mapping.map_model(original, effort.as_deref());

        if did_map && mapped != *original {
            log::debug!("[CodexModelMapper] 模型映射: {original} → {mapped}");
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.clone()), Some(mapped));
        }
    }

    (body, original_model, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider_with_mapping(
        model_map: HashMap<String, String>,
        effort_map: HashMap<String, String>,
    ) -> Provider {
        use crate::provider::ProviderMeta;

        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                codex_model_mapping: Some(CodexModelMappingConfig {
                    enabled: true,
                    model_map,
                    effort_map,
                }),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_provider_without_mapping() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
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
    fn test_simple_model_mapping() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-5.2-codex".to_string(), "gpt-5.2-custom".to_string());

        let provider = create_provider_with_mapping(model_map, HashMap::new());
        let body = json!({"model": "gpt-5.2-codex"});

        let (result, original, mapped) = apply_codex_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.2-custom");
        assert_eq!(original, Some("gpt-5.2-codex".to_string()));
        assert_eq!(mapped, Some("gpt-5.2-custom".to_string()));
    }

    #[test]
    fn test_effort_mapping() {
        let mut effort_map = HashMap::new();
        effort_map.insert(
            "gpt-5.2-codex@xhigh".to_string(),
            "gpt-5.2-codex-xhigh".to_string(),
        );

        let provider = create_provider_with_mapping(HashMap::new(), effort_map);
        let body = json!({
            "model": "gpt-5.2-codex",
            "reasoning": {"effort": "xhigh", "summary": "auto"}
        });

        let (result, original, mapped) = apply_codex_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.2-codex-xhigh");
        assert_eq!(original, Some("gpt-5.2-codex".to_string()));
        assert_eq!(mapped, Some("gpt-5.2-codex-xhigh".to_string()));
        // 验证 reasoning.effort 被保留
        assert_eq!(result["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn test_effort_mapping_priority_over_simple() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-5.2-codex".to_string(), "simple-mapped".to_string());

        let mut effort_map = HashMap::new();
        effort_map.insert("gpt-5.2-codex@xhigh".to_string(), "effort-mapped".to_string());

        let provider = create_provider_with_mapping(model_map, effort_map);
        let body = json!({
            "model": "gpt-5.2-codex",
            "reasoning": {"effort": "xhigh"}
        });

        let (result, _, mapped) = apply_codex_model_mapping(body, &provider);

        // effort 映射应该优先
        assert_eq!(result["model"], "effort-mapped");
        assert_eq!(mapped, Some("effort-mapped".to_string()));
    }

    #[test]
    fn test_fallback_to_simple_mapping_when_effort_not_matched() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-5.2-codex".to_string(), "simple-mapped".to_string());

        let mut effort_map = HashMap::new();
        effort_map.insert("gpt-5.2-codex@xhigh".to_string(), "effort-mapped".to_string());

        let provider = create_provider_with_mapping(model_map, effort_map);
        let body = json!({
            "model": "gpt-5.2-codex",
            "reasoning": {"effort": "high"}  // 不匹配 xhigh
        });

        let (result, _, mapped) = apply_codex_model_mapping(body, &provider);

        // 应该 fallback 到简单映射
        assert_eq!(result["model"], "simple-mapped");
        assert_eq!(mapped, Some("simple-mapped".to_string()));
    }

    #[test]
    fn test_no_mapping_configured() {
        let provider = create_provider_without_mapping();
        let body = json!({"model": "gpt-5.2-codex"});

        let (result, original, mapped) = apply_codex_model_mapping(body, &provider);

        assert_eq!(result["model"], "gpt-5.2-codex");
        assert_eq!(original, Some("gpt-5.2-codex".to_string()));
        assert!(mapped.is_none());
    }

    #[test]
    fn test_disabled_mapping() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-5.2-codex".to_string(), "mapped".to_string());

        let config = CodexModelMappingConfig {
            enabled: false,
            model_map,
            effort_map: HashMap::new(),
        };

        let (mapped, did_map) = config.map_model("gpt-5.2-codex", None);
        assert_eq!(mapped, "gpt-5.2-codex");
        assert!(!did_map);
    }

    #[test]
    fn test_extract_effort() {
        let body = json!({
            "reasoning": {"effort": "xhigh", "summary": "auto"}
        });
        assert_eq!(extract_effort(&body), Some("xhigh".to_string()));

        let body_no_effort = json!({"model": "test"});
        assert_eq!(extract_effort(&body_no_effort), None);
    }
}
