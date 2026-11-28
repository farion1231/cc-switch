//! 模型映射模块
//!
//! 在请求转发前，根据 Provider 配置替换请求中的模型名称

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use crate::provider::Provider;
use serde_json::Value;

/// 模型映射配置
pub struct ModelMapping {
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub fable_model: Option<String>,
    pub subagent_model: Option<String>,
    pub default_model: Option<String>,
}

impl ModelMapping {
    /// 从 Provider 配置中提取模型映射
    pub fn from_provider(provider: &Provider) -> Self {
        let env = provider.settings_config.get("env");

        Self {
            haiku_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            sonnet_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            opus_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            fable_model: env
                .and_then(|e| e.get("ANTHROPIC_DEFAULT_FABLE_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            subagent_model: env
                .and_then(|e| e.get("CLAUDE_CODE_SUBAGENT_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            default_model: env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
        }
    }

    /// 检查是否配置了任何模型映射
    pub fn has_mapping(&self) -> bool {
        self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
            || self.fable_model.is_some()
            || self.subagent_model.is_some()
            || self.default_model.is_some()
    }

    /// 根据原始模型名称获取映射后的模型
    pub fn map_model(&self, original_model: &str) -> String {
        let model_lower = original_model.to_lowercase();

        // 1. 按模型类型匹配
        if model_lower.contains("fable") {
            if let Some(ref m) = self.fable_model {
                return m.clone();
            }
            // 未单独配置 fable 档时归入 opus 档，与 Claude Code 官方
            // 分类器降级方向一致（fable→opus），避免落到 default 失去层级。
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("haiku") {
            if let Some(ref m) = self.haiku_model {
                return m.clone();
            }
        }
        if model_lower.contains("opus") {
            if let Some(ref m) = self.opus_model {
                return m.clone();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(ref m) = self.sonnet_model {
                return m.clone();
            }
        }

        if let Some(ref m) = self.subagent_model {
            if strip_one_m_suffix_for_upstream(original_model) == strip_one_m_suffix_for_upstream(m)
            {
                return original_model.to_string();
            }
            // 接管时子代理模型会换成 Claude Code 目录里的 id（`claude-opus-5.5` →
            // `claude-opus-5-5`）。那一档没配时下面的默认模型会把它吞掉，所以目录形态
            // 也要认成同一个子代理模型，送回行里配置的原值。
            if strip_one_m_suffix_for_upstream(original_model)
                == strip_one_m_suffix_for_upstream(
                    &crate::live::project::claude::client_model_for_subagent(m),
                )
            {
                return m.clone();
            }
        }

        // 2. 默认模型
        if let Some(ref m) = self.default_model {
            return m.clone();
        }

        // 3. 无映射，保持原样
        original_model.to_string()
    }
}

/// 对请求体应用模型映射
///
/// 返回 (映射后的请求体, 原始模型名, 映射后模型名)
pub fn apply_model_mapping(
    mut body: Value,
    provider: &Provider,
) -> (Value, Option<String>, Option<String>) {
    let mapping = ModelMapping::from_provider(provider);

    // 如果没有配置映射，直接返回
    if !mapping.has_mapping() {
        let original = body.get("model").and_then(|m| m.as_str()).map(String::from);
        return (body, original, None);
    }

    // 提取原始模型名
    let original_model = body.get("model").and_then(|m| m.as_str()).map(String::from);

    if let Some(ref original) = original_model {
        let mapped = mapping.map_model(original);

        if mapped != *original {
            log::debug!("[ModelMapper] 模型映射: {original} → {mapped}");
            body["model"] = serde_json::json!(mapped);
            return (body, Some(original.clone()), Some(mapped));
        }
    }

    (body, original_model, None)
}

/// Claude Code 通过 `[1M]` 后缀声明 100 万上下文能力；上游 API
/// 通常不接受这个本地能力标记，转发前需要剥离。
pub fn strip_one_m_suffix_for_upstream(model: &str) -> &str {
    let trimmed = model.trim_end();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    model
}

/// Codex 故障转移换家时的模型名。
///
/// Codex 的客户端配置写的是路由那家的真实模型名（没有 Claude 那样的档位别名），故障转移
/// 换到别家时原样发过去，别家多半不认（MiniMax 回 400 unknown model，队列就此停下）。
/// 这家发布的模型（目录里的，或行里的 `model`）里没有请求的模型名时，换成这家行里的
/// `model`；认得的照发。只在故障转移开着时调用：关着时只有一家，用户选的模型原样发。
/// 转移成功后路由会记到新的那家，客户端却还发原来的名字，所以按「这家认不认」判断，
/// 不按「是不是路由那家」判断。
pub fn map_codex_failover_model(mut body: Value, provider: &Provider) -> Value {
    let Some(requested) = body.get("model").and_then(Value::as_str) else {
        return body;
    };
    let config = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(own) = crate::codex_config::codex_top_level_model(config) else {
        // 行里没有模型（官方卡等）：不知道该换成什么，原样发。
        return body;
    };
    if requested == own
        || crate::codex_config::codex_published_models(&provider.settings_config, config)
            .iter()
            .any(|model| model == requested)
    {
        return body;
    }
    log::info!(
        "[ModelMapper] Codex 故障转移到 {}：这家没有模型 {requested}，改用 {own}",
        provider.name
    );
    body["model"] = serde_json::json!(own);
    body
}

pub fn strip_one_m_suffix_for_upstream_from_body(mut body: Value) -> Value {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return body;
    };

    let stripped = strip_one_m_suffix_for_upstream(model);
    if stripped != model {
        log::debug!("[ModelMapper] 去除本地 1M 标记: {model} → {stripped}");
        body["model"] = serde_json::json!(stripped);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_with_settings(name: &str, settings_config: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: name.to_string(),
            settings_config,
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

    fn full_mapping_provider() -> Provider {
        provider_with_settings(
            "Test",
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "haiku-mapped",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "sonnet-mapped",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped",
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-mapped"
                }
            }),
        )
    }

    #[test]
    fn apply_model_mapping_tiers_thinking_and_fallbacks() {
        let provider = full_mapping_provider();

        // 档位、大小写不敏感、fable[1m]（#3980）及未知模型回退默认
        for (incoming, expected) in [
            ("claude-sonnet-4-5-20250929", "sonnet-mapped"),
            ("Claude-SONNET-4-5", "sonnet-mapped"),
            ("claude-haiku-4-5", "haiku-mapped"),
            ("claude-opus-4-5", "opus-mapped"),
            ("claude-fable-5", "fable-mapped"),
            ("claude-fable-5[1m]", "fable-mapped"),
            ("some-unknown-model", "default-model"),
        ] {
            let (result, original, mapped) =
                apply_model_mapping(json!({ "model": incoming }), &provider);
            assert_eq!(result["model"], expected, "{incoming}");
            assert_eq!(original.as_deref(), Some(incoming), "{incoming}");
            assert_eq!(mapped.as_deref(), Some(expected), "{incoming}");
        }

        // Issue #2081: thinking 参数不影响模型映射
        for thinking_type in ["enabled", "adaptive", "disabled"] {
            let body = json!({
                "model": "claude-sonnet-4-5",
                "thinking": { "type": thinking_type }
            });
            let (result, _, mapped) = apply_model_mapping(body, &provider);
            assert_eq!(result["model"], "sonnet-mapped", "{thinking_type}");
            assert_eq!(mapped.as_deref(), Some("sonnet-mapped"), "{thinking_type}");
        }

        // fable 未单独配置时先回退 opus，再回退 default
        let opus_only = provider_with_settings(
            "Test",
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-mapped"
                }
            }),
        );
        let (res, _, mapped) =
            apply_model_mapping(json!({ "model": "claude-fable-5" }), &opus_only);
        assert_eq!(res["model"], "opus-mapped");
        assert_eq!(mapped.as_deref(), Some("opus-mapped"));

        let default_only = provider_with_settings(
            "Test",
            json!({ "env": { "ANTHROPIC_MODEL": "default-model" } }),
        );
        let (res, _, mapped) =
            apply_model_mapping(json!({ "model": "claude-fable-5" }), &default_only);
        assert_eq!(res["model"], "default-model");
        assert_eq!(mapped.as_deref(), Some("default-model"));

        // 未配置任何映射时保持原样
        let empty = provider_with_settings("Test", json!({}));
        let (res, original, mapped) =
            apply_model_mapping(json!({ "model": "claude-sonnet-4-5" }), &empty);
        assert_eq!(res["model"], "claude-sonnet-4-5");
        assert_eq!(original.as_deref(), Some("claude-sonnet-4-5"));
        assert!(mapped.is_none());
    }

    #[test]
    fn subagent_model_is_preserved_including_one_m_and_catalog_id() {
        let provider = provider_with_settings(
            "Test",
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "default-model",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "gpt-5.4-mini"
                }
            }),
        );
        for incoming in ["gpt-5.4-mini", "gpt-5.4-mini[1M]"] {
            let (result, original, mapped) =
                apply_model_mapping(json!({ "model": incoming }), &provider);
            assert_eq!(result["model"], incoming, "{incoming}");
            assert_eq!(original.as_deref(), Some(incoming), "{incoming}");
            assert!(mapped.is_none(), "{incoming}");
        }

        // 接管将子代理 id 换成目录形态后，缺档位也不能被默认模型吞掉；同代非小版本仍走默认模型
        let catalog_provider = provider_with_settings(
            "Test",
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "glm-5",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "anthropic/claude-opus-5.5"
                }
            }),
        );
        for (incoming, expected) in [
            ("claude-opus-5-5", "anthropic/claude-opus-5.5"),
            ("claude-opus-5-5[1M]", "anthropic/claude-opus-5.5"),
            ("anthropic/claude-opus-5.5", "anthropic/claude-opus-5.5"),
            ("claude-opus-5", "glm-5"),
        ] {
            let (result, _, _) =
                apply_model_mapping(json!({ "model": incoming }), &catalog_provider);
            assert_eq!(result["model"], expected, "{incoming}");
        }
    }

    #[test]
    fn map_codex_failover_model_swaps_only_unrecognized_models() {
        // 目标行不发布请求模型 -> 换成目标行自己的 model，且保留其余字段
        let minimax = provider_with_settings(
            "MiniMax",
            json!({"config": "model = \"MiniMax-M3\"\nmodel_provider = \"custom\"\n"}),
        );
        let result = map_codex_failover_model(json!({"model": "kimi-k3", "input": []}), &minimax);
        assert_eq!(result["model"], "MiniMax-M3");
        assert_eq!(result["input"], json!([]));

        // 目标行或目录已发布的模型原样保留
        let kimi = provider_with_settings(
            "Kimi",
            json!({
                "config": "model = \"k3\"\n",
                "modelCatalog": {"models": [{"model": "k3"}, {"model": "k3.1-pro"}]}
            }),
        );
        for model in ["k3", "k3.1-pro"] {
            let result = map_codex_failover_model(json!({ "model": model }), &kimi);
            assert_eq!(result["model"], model);
        }

        // 无模型的官方行原样保留
        let official = provider_with_settings("OpenAI Official", json!({"config": ""}));
        let result = map_codex_failover_model(json!({"model": "gpt-6-astra"}), &official);
        assert_eq!(result["model"], "gpt-6-astra");
    }

    #[test]
    fn strip_one_m_suffix_for_upstream_cases() {
        assert_eq!(
            strip_one_m_suffix_for_upstream_from_body(json!({"model": "deepseek-v4-pro[1M]"}))
                ["model"],
            "deepseek-v4-pro"
        );
        assert_eq!(
            strip_one_m_suffix_for_upstream_from_body(json!({"model": "deepseek-v4-pro"}))["model"],
            "deepseek-v4-pro"
        );

        let provider = provider_with_settings(
            "Test",
            json!({ "env": { "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro [1M]" } }),
        );
        let (mapped, _, _) = apply_model_mapping(json!({"model": "claude-sonnet-4-6"}), &provider);
        assert_eq!(
            strip_one_m_suffix_for_upstream_from_body(mapped)["model"],
            "deepseek-v4-pro"
        );
    }
}
