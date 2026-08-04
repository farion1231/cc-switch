//! auto mode 安全分类器请求的模型路由
//!
//! Claude Code 的 auto mode 在放行工具调用前，会先发一次"安全分类"请求给模型，
//! 由它给出 `<severity>N</severity>` 评分。该请求用的不是 haiku 档，而是回落到
//! `ANTHROPIC_DEFAULT_SONNET_MODEL`——也就是绝大多数第三方配置里的主模型。
//!
//! 这个请求有两个要命的特性：
//! 1. **非流式**，且带上完整会话 transcript（几万 token 起步）；
//! 2. stage1 有 60s 硬超时，超时即判定分类器不可用，工具调用被连带拦下，
//!    用户侧只看到 `<model> is temporarily unavailable`（上游状态码被丢弃）。
//!
//! 主模型一旦延迟不稳（实测某内网 LiteLLM 网关上的 GLM 在 21~86s 之间抖动），
//! 分类器就会间歇性失败，且"稍后重试"时灵时不灵。让 Provider 单独指定一个
//! 快而稳的分类器模型，可以把这类请求与主对话模型解耦。

use crate::provider::Provider;
use serde_json::Value;

/// stage1 请求独有的停止序列。Claude Code 侧对应
/// `...stop_sequences:[u?"</severity>":"</block>"]`。
const CLASSIFIER_STOP_SEQUENCES: [&str; 2] = ["</severity>", "</block>"];

/// 分类器提示词特征串。stage2 不带 stop_sequences，只能靠它兜底识别。
/// 普通对话不会出现这些串，误判风险可忽略。
const CLASSIFIER_PROMPT_MARKERS: [&str; 3] = [
    "Output <severity>N</severity> where N is an integer 0-100",
    "Your ENTIRE response MUST begin with <block>",
    "Respond with <severity>N</severity> ONLY",
];

/// 识别 Claude Code auto mode 的安全分类器请求。
///
/// 两个锚点任一命中即可：stop_sequences（stage1 最强信号）、
/// 末条 user 消息中的提示词特征串（覆盖 stage2）。
pub fn is_auto_mode_classifier_request(body: &Value) -> bool {
    if has_classifier_stop_sequence(body) {
        return true;
    }
    last_user_text_has_marker(body)
}

fn has_classifier_stop_sequence(body: &Value) -> bool {
    body.get("stop_sequences")
        .and_then(Value::as_array)
        .is_some_and(|seqs| {
            seqs.iter().filter_map(Value::as_str).any(|s| {
                CLASSIFIER_STOP_SEQUENCES
                    .iter()
                    .any(|marker| s.eq_ignore_ascii_case(marker))
            })
        })
}

fn last_user_text_has_marker(body: &Value) -> bool {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return false;
    };
    let Some(last_user) = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return false;
    };

    match last_user.get("content") {
        Some(Value::String(text)) => text_has_marker(text),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(text_has_marker)
        }),
        _ => false,
    }
}

fn text_has_marker(text: &str) -> bool {
    CLASSIFIER_PROMPT_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
}

/// 命中分类器请求时，把 model 覆写成 Provider 配置的分类器模型。
///
/// 未配置 `classifierModel` 时原样返回。覆写发生在模型映射之后、`[1m]` 后缀
/// 剥离之前，因此这里写入的值仍会走后续的统一清洗。
pub fn apply_classifier_model_override(mut body: Value, provider: &Provider) -> Value {
    let Some(classifier_model) = provider
        .meta
        .as_ref()
        .and_then(|m| m.classifier_model.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return body;
    };

    if !is_auto_mode_classifier_request(&body) {
        return body;
    }

    let original = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if original == classifier_model {
        return body;
    }

    // 用 info 级别：默认日志级别是 Info（见 lib.rs 的 set_max_level），debug 记不下来，
    // 排查"覆写到底有没有命中"时就只能靠猜。每次分类器请求仅一条，量可忽略。
    log::info!("[ClassifierRoute] 分类器模型覆写: {original} → {classifier_model}");
    body["model"] = Value::String(classifier_model.to_string());
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn provider_with_classifier(model: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({"env": {"ANTHROPIC_MODEL": "glm-5.2-anthropic"}}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            classifier_model: model.map(String::from),
            ..Default::default()
        });
        provider
    }

    fn stage1_body() -> Value {
        json!({
            "model": "glm-5.2-anthropic",
            "max_tokens": 8192,
            "stop_sequences": ["</severity>"],
            "messages": [{"role": "user", "content": "The agent wants to run: ls -la"}]
        })
    }

    fn stage2_body() -> Value {
        json!({
            "model": "glm-5.2-anthropic",
            "max_tokens": 8192,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "Output <severity>N</severity> where N is an integer 0-100 and 50 is \
                             exactly the allow/block boundary under the rules above."
                }]
            }]
        })
    }

    #[test]
    fn detects_stage1_by_stop_sequence() {
        assert!(is_auto_mode_classifier_request(&stage1_body()));
        let block_variant = json!({"stop_sequences": ["</block>"]});
        assert!(is_auto_mode_classifier_request(&block_variant));
    }

    #[test]
    fn detects_stage2_by_prompt_marker() {
        assert!(is_auto_mode_classifier_request(&stage2_body()));
    }

    #[test]
    fn ignores_normal_request() {
        let body = json!({
            "model": "glm-5.2-anthropic",
            "stop_sequences": ["\n\nHuman:"],
            "messages": [{"role": "user", "content": "帮我看一下这个函数"}]
        });
        assert!(!is_auto_mode_classifier_request(&body));
    }

    #[test]
    fn ignores_marker_in_earlier_turn() {
        // 特征串只在历史消息里出现（例如正在讨论分类器本身），不应命中
        let body = json!({
            "messages": [
                {"role": "user", "content": "Respond with <severity>N</severity> ONLY"},
                {"role": "assistant", "content": "好的"},
                {"role": "user", "content": "继续写代码"}
            ]
        });
        assert!(!is_auto_mode_classifier_request(&body));
    }

    #[test]
    fn overrides_model_for_classifier_request() {
        let provider = provider_with_classifier(Some("qwen3.7-max-anthropic"));
        let result = apply_classifier_model_override(stage1_body(), &provider);
        assert_eq!(result["model"], "qwen3.7-max-anthropic");

        let result = apply_classifier_model_override(stage2_body(), &provider);
        assert_eq!(result["model"], "qwen3.7-max-anthropic");
    }

    #[test]
    fn leaves_normal_request_untouched() {
        let provider = provider_with_classifier(Some("qwen3.7-max-anthropic"));
        let body = json!({
            "model": "glm-5.2-anthropic",
            "messages": [{"role": "user", "content": "写个测试"}]
        });
        let result = apply_classifier_model_override(body, &provider);
        assert_eq!(result["model"], "glm-5.2-anthropic");
    }

    #[test]
    fn no_op_when_unconfigured() {
        for meta in [None, Some("  ")] {
            let provider = provider_with_classifier(meta);
            let result = apply_classifier_model_override(stage1_body(), &provider);
            assert_eq!(result["model"], "glm-5.2-anthropic");
        }
    }

    #[test]
    fn keeps_one_m_marker_for_downstream_strip() {
        // 覆写值里的 [1m] 交给后续统一剥离，这里不提前处理
        let provider = provider_with_classifier(Some("qwen3.7-max-anthropic[1m]"));
        let result = apply_classifier_model_override(stage1_body(), &provider);
        assert_eq!(result["model"], "qwen3.7-max-anthropic[1m]");
    }
}
