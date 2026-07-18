use std::sync::Mutex;

use serde_json::Value;

use crate::provider::Provider;

/// 监听 settings.json 后决定的当前激活模型窗口
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveModelWindow {
    pub model: String,
    pub window: u64,
}

/// 根据 settings.json 顶层 model 字段和 provider env 配置，
/// 决定要写入 ACW/MAX 的窗口值。
///
/// 返回 None 表示"不写"（model 字段无效 / 角色对应 env 不存在 / env 无后缀）。
pub(crate) fn resolve_active_model_window(
    settings: &Value,
    provider: &Provider,
) -> Option<ActiveModelWindow> {
    // 1. 读顶层 model 字段
    let model = settings.get("model").and_then(Value::as_str)?;
    // 2. 映射到 env 字段名
    let env_key = match model {
        "sonnet" => "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "opus" => "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "fable" => "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "haiku" => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "subagent" => "CLAUDE_CODE_SUBAGENT_MODEL",
        _ => return None,
    };
    // 3. 读 provider env 里对应字段
    let env_value = provider
        .settings_config
        .get("env")
        .and_then(|e| e.get(env_key))
        .and_then(Value::as_str)?;
    // 4. 解析后缀得到窗口
    let (_, window) = crate::claude_desktop_config::parse_context_window_suffix(env_value);
    window.map(|w| ActiveModelWindow {
        model: model.to_string(),
        window: w,
    })
}

/// 根据窗口值生成要写入 settings.json.env 的两个 env 项。
/// ACW = 窗口 × 0.8（向下取整），MAX = 窗口本身。
pub(crate) fn build_env_writes(window: u64) -> Vec<(&'static str, String)> {
    let acw = (window * 80) / 100;
    vec![
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw.to_string()),
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", window.to_string()),
    ]
}

/// 检查新事件的 model 字段是否需要处理（与上次不同则处理）。
/// 这是监听器防循环的核心逻辑：自己写 env 不改 model 字段 → 自动跳过。
pub(crate) fn should_process(
    state: &Mutex<Option<String>>,
    new_model: Option<&str>,
) -> bool {
    let mut guard = state.lock().expect("settings watcher mutex poisoned");
    let new_model_owned = new_model.map(|s| s.to_string());
    if *guard == new_model_owned {
        return false;
    }
    *guard = new_model_owned;
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;

    fn make_provider(env: Value) -> Provider {
        Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": env }), None)
    }

    // ========== Task 2: 角色映射测试 ==========

    #[test]
    fn resolve_maps_haiku_to_anthropic_default_haiku_model() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "haiku");
        assert_eq!(result.window, 30000);
    }

    #[test]
    fn resolve_maps_sonnet_to_anthropic_default_sonnet_model() {
        let settings = json!({ "model": "sonnet" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "sonnet");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_opus_to_anthropic_default_opus_model() {
        let settings = json!({ "model": "opus" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1000000]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "opus");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_fable_to_anthropic_default_fable_model() {
        let settings = json!({ "model": "fable" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_FABLE_MODEL": "GLM-5.2[200k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "fable");
        assert_eq!(result.window, 200000);
    }

    #[test]
    fn resolve_maps_subagent_to_claude_code_subagent_model() {
        let settings = json!({ "model": "subagent" });
        let provider = make_provider(json!({
            "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-flash"
        }));
        // subagent 没后缀 → 期望 None（无窗口可写）
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    // ========== Task 3: build_env_writes 测试 ==========

    #[test]
    fn build_writes_for_30k_window() {
        let writes = build_env_writes(30000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "24000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "30000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_1m_window() {
        let writes = build_env_writes(1000000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "800000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1000000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_200k_window() {
        let writes = build_env_writes(200000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "160000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "200000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_1_token_boundary() {
        // 最小边界：1 token → ACW=0（×0.8 = 0.8，向下取整 = 0）
        let writes = build_env_writes(1);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "0".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1".to_string()),
        ]);
    }

    // ========== Task 4: 无效输入处理测试 ==========

    #[test]
    fn resolve_returns_none_when_model_field_missing() {
        let settings = json!({});
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_value_unknown() {
        let settings = json!({ "model": "custom-alias" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_not_string() {
        let settings = json!({ "model": 123 });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_null() {
        let settings = json!({ "model": null });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_role_env_field_missing() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_env_value_not_string() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": { "name": "weird" }
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_invalid() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[invalid]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_zero() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[0]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_unsupported_unit() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1G]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_decimal() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1.5m]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_suffix_at_all() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }
}
