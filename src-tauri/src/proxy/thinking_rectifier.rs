//! Thinking Signature 整流器
//!
//! 用于自动修复 Anthropic API 中因签名校验失败导致的请求错误。
//! 当上游 API 返回签名相关错误时，系统会自动移除有问题的签名字段并重试请求。

use super::types::RectifierConfig;
use serde_json::Value;

/// 整流结果
#[derive(Debug, Clone, Default)]
pub struct RectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 移除的 thinking block 数量
    pub removed_thinking_blocks: usize,
    /// 移除的 redacted_thinking block 数量
    pub removed_redacted_thinking_blocks: usize,
    /// 移除的 signature 字段数量
    pub removed_signature_fields: usize,
}

/// 检测是否需要触发 thinking 签名整流器
///
/// 返回 `true` 表示需要触发整流器，`false` 表示不需要。
/// 会检查配置开关。
pub fn should_rectify_thinking_signature(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> bool {
    // 检查总开关
    if !config.enabled {
        return false;
    }
    // 检查子开关
    if !config.request_thinking_signature {
        return false;
    }

    // 检测错误类型
    let Some(msg) = error_message else {
        return false;
    };
    let lower = msg.to_lowercase();

    // 场景1: thinking block 中的签名无效
    // 错误示例: "Invalid 'signature' in 'thinking' block"
    if lower.contains("invalid")
        && lower.contains("signature")
        && lower.contains("thinking")
        && lower.contains("block")
    {
        return true;
    }

    // 场景1b: Gemini/第三方渠道返回 "Thought signature is not valid"
    // 错误示例: "Unable to submit request because Thought signature is not valid"
    if lower.contains("thought signature")
        && (lower.contains("not valid") || lower.contains("invalid"))
    {
        return true;
    }

    // 场景2: assistant 消息必须以 thinking block 开头
    // 错误示例: "must start with a thinking block"
    if lower.contains("must start with a thinking block") {
        return true;
    }

    // 场景3: expected thinking or redacted_thinking, found tool_use
    // 与 CCH 对齐：要求明确包含 tool_use，避免过宽匹配。
    // 错误示例: "Expected `thinking` or `redacted_thinking`, but found `tool_use`"
    if lower.contains("expected")
        && (lower.contains("thinking") || lower.contains("redacted_thinking"))
        && lower.contains("found")
        && lower.contains("tool_use")
    {
        return true;
    }

    // 场景4: signature 字段必需但缺失
    // 错误示例: "signature: Field required"
    if lower.contains("signature") && lower.contains("field required") {
        return true;
    }

    // 场景5: signature 字段不被接受（第三方渠道）
    // 错误示例: "xxx.signature: Extra inputs are not permitted"
    if lower.contains("signature") && lower.contains("extra inputs are not permitted") {
        return true;
    }

    // 场景6: thinking/redacted_thinking 块被修改
    // 错误示例: "thinking or redacted_thinking blocks ... cannot be modified"
    if (lower.contains("thinking") || lower.contains("redacted_thinking"))
        && lower.contains("cannot be modified")
    {
        return true;
    }

    // 场景7: 非法请求（与 CCH 对齐，按 invalid request 统一兜底）
    if lower.contains("非法请求")
        || lower.contains("illegal request")
        || lower.contains("invalid request")
    {
        return true;
    }

    false
}

/// 对 Anthropic 请求体做最小侵入整流
///
/// - 移除 messages[*].content 中的 thinking/redacted_thinking block
/// - 移除非 thinking block 上遗留的 signature 字段
/// - 特定条件下删除顶层 thinking 字段
///
/// 注意：该函数会原地修改 body 对象
pub fn rectify_anthropic_request(body: &mut Value) -> RectifyResult {
    let mut result = RectifyResult::default();

    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(m) => m,
        None => return result,
    };

    // 遍历所有消息
    for msg in messages.iter_mut() {
        let content = match msg.get_mut("content").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => continue,
        };

        let mut new_content = Vec::with_capacity(content.len());
        let mut content_modified = false;

        for block in content.iter() {
            let block_type = block.get("type").and_then(|t| t.as_str());

            match block_type {
                Some("thinking") => {
                    result.removed_thinking_blocks += 1;
                    content_modified = true;
                    continue;
                }
                Some("redacted_thinking") => {
                    result.removed_redacted_thinking_blocks += 1;
                    content_modified = true;
                    continue;
                }
                _ => {}
            }

            // 移除非 thinking block 上的 signature 字段
            if block.get("signature").is_some() {
                let mut block_clone = block.clone();
                if let Some(obj) = block_clone.as_object_mut() {
                    obj.remove("signature");
                    result.removed_signature_fields += 1;
                    content_modified = true;
                    new_content.push(Value::Object(obj.clone()));
                    continue;
                }
            }

            new_content.push(block.clone());
        }

        if content_modified {
            result.applied = true;
            *content = new_content;
        }
    }

    // 兜底处理：thinking 启用 + 工具调用链路中最后一条 assistant 消息未以 thinking 开头
    let messages_snapshot: Vec<Value> = body
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default();

    if should_remove_top_level_thinking(body, &messages_snapshot) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("thinking");
            result.applied = true;
        }
    }

    result
}

/// 判断是否需要删除顶层 thinking 字段
fn should_remove_top_level_thinking(body: &Value, messages: &[Value]) -> bool {
    // 检查 thinking 是否启用
    let thinking_type = body
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(|t| t.as_str());

    // 与 CCH 对齐：仅 type=enabled 视为开启
    let thinking_enabled = thinking_type == Some("enabled");

    if !thinking_enabled {
        return false;
    }

    // 找到最后一条 assistant 消息
    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"));

    let last_assistant_content = match last_assistant
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        Some(c) if !c.is_empty() => c,
        _ => return false,
    };

    // 检查首块是否为 thinking/redacted_thinking
    let first_block_type = last_assistant_content
        .first()
        .and_then(|b| b.get("type"))
        .and_then(|t| t.as_str());

    let missing_thinking_prefix =
        first_block_type != Some("thinking") && first_block_type != Some("redacted_thinking");

    if !missing_thinking_prefix {
        return false;
    }

    // 检查是否存在 tool_use
    last_assistant_content
        .iter()
        .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
}

/// 与 CCH 对齐：请求前不做 thinking type 主动改写。
pub fn normalize_thinking_type(body: Value) -> Value {
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }

    #[test]
    fn should_rectify_thinking_signature_rules() {
        let cfg = enabled_config();
        let nested_sig = r#"{"error":{"message":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"***.content.0: Invalid `signature` in `thinking` block\"},\"request_id\":\"req_xxx\"}"}}"#;
        let nested_thought = r#"{"error":{"message":"Unable to submit request because Thought signature is not valid.. Learn more: https://example.com/help","type":"upstream_error","param":"","code":400}}"#;
        let nested_required = r#"{"error":{"type":"<nil>","message":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"***.***.***.***.***.signature: Field required\"},\"request_id\":\"req_xxx\"}"}}"#;

        for (case, msg, expected) in [
            (
                "invalid_signature_backticks",
                Some("messages.1.content.0: Invalid `signature` in `thinking` block"),
                true,
            ),
            (
                "invalid_signature_plain_case_insensitive",
                Some("Messages.1.Content.0: invalid signature in thinking block"),
                true,
            ),
            (
                "invalid_thought_signature",
                Some("Unable to submit request because Thought signature is not valid.. Learn more: https://example.com/help"),
                true,
            ),
            ("invalid_signature_nested_json", Some(nested_sig), true),
            ("invalid_thought_nested_json", Some(nested_thought), true),
            (
                "expected_thinking_found_tool_use",
                Some("messages.69.content.0.type: Expected `thinking` or `redacted_thinking`, but found `tool_use`."),
                true,
            ),
            (
                "expected_thinking_without_tool_use",
                Some("messages.69.content.0.type: Expected `thinking` or `redacted_thinking`, but found `text`."),
                false,
            ),
            (
                "must_start_with_thinking",
                Some("a final `assistant` message must start with a thinking block"),
                true,
            ),
            (
                "signature_field_required",
                Some("***.***.***.***.***.signature: Field required"),
                true,
            ),
            ("signature_field_required_nested", Some(nested_required), true),
            (
                "signature_extra_inputs",
                Some("xxx.signature: Extra inputs are not permitted"),
                true,
            ),
            (
                "thinking_cannot_be_modified",
                Some("thinking or redacted_thinking blocks in the response cannot be modified"),
                true,
            ),
            (
                "chinese_invalid_request",
                Some("非法请求：thinking signature 不合法"),
                true,
            ),
            (
                "illegal_request",
                Some("illegal request: tool_use block mismatch"),
                true,
            ),
            (
                "invalid_request",
                Some("invalid request: malformed JSON"),
                true,
            ),
            (
                "adaptive_tag_mismatch_ignored",
                Some("Input tag 'adaptive' found using 'type' does not match expected tags"),
                false,
            ),
            ("unrelated_timeout", Some("Request timeout"), false),
            ("unrelated_conn_refused", Some("Connection refused"), false),
            ("none_message", None, false),
        ] {
            assert_eq!(
                should_rectify_thinking_signature(msg, &cfg),
                expected,
                "{case}"
            );
        }

        // 总开关或子开关关闭时不触发
        let sub_off = RectifierConfig {
            request_thinking_signature: false,
            ..cfg.clone()
        };
        let master_off = RectifierConfig {
            enabled: false,
            ..cfg
        };
        assert!(!should_rectify_thinking_signature(
            Some("Invalid `signature` in `thinking` block"),
            &sub_off
        ));
        assert!(!should_rectify_thinking_signature(
            Some("Invalid `signature` in `thinking` block"),
            &master_off
        ));
    }

    #[test]
    fn rectify_anthropic_request_strips_blocks_signatures_and_invalid_enabled_thinking() {
        // 1. 移除 thinking / redacted_thinking 块并剥离非 thinking 块上的 signature
        let mut body = json!({
            "model": "claude-test",
            "messages": [{
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "t", "signature": "sig" },
                    { "type": "text", "text": "hello", "signature": "sig_text" },
                    { "type": "tool_use", "id": "toolu_1", "name": "WebSearch", "input": {}, "signature": "sig_tool" },
                    { "type": "redacted_thinking", "data": "r", "signature": "sig_redacted" }
                ]
            }]
        });
        let result = rectify_anthropic_request(&mut body);
        assert!(result.applied);
        assert_eq!(result.removed_thinking_blocks, 1);
        assert_eq!(result.removed_redacted_thinking_blocks, 1);
        assert_eq!(result.removed_signature_fields, 2);
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0].get("signature").is_none());
        assert_eq!(content[1]["type"], "tool_use");
        assert!(content[1].get("signature").is_none());

        // 2. type=enabled 且 tool_use 链路缺少 thinking 前缀（或移除 thinking 块后首块变成 tool_use）-> 删顶层 thinking
        for (case, assistant_content, expected_removed_thinking) in [
            (
                "missing_prefix_before_tool_use",
                json!([{ "type": "tool_use", "id": "toolu_1", "name": "WebSearch", "input": {} }]),
                0,
            ),
            (
                "stripped_prefix_leaves_tool_use_first",
                json!([
                    { "type": "thinking", "thinking": "some thought" },
                    { "type": "tool_use", "id": "toolu_1", "name": "Test", "input": {} }
                ]),
                1,
            ),
        ] {
            let mut body = json!({
                "model": "claude-test",
                "thinking": { "type": "enabled", "budget_tokens": 1024 },
                "messages": [
                    { "role": "assistant", "content": assistant_content },
                    { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }] }
                ]
            });
            let result = rectify_anthropic_request(&mut body);
            assert!(result.applied, "{case}");
            assert_eq!(
                result.removed_thinking_blocks, expected_removed_thinking,
                "{case}"
            );
            assert!(body.get("thinking").is_none(), "{case}");
        }

        // 3. adaptive 模式下仍清理历史 thinking / signature 块，但保留顶层 adaptive
        let mut adaptive = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive" },
            "messages": [{
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "t", "signature": "sig_thinking" },
                    { "type": "text", "text": "hello", "signature": "sig_text" }
                ]
            }]
        });
        let result = rectify_anthropic_request(&mut adaptive);
        assert!(result.applied);
        assert_eq!(result.removed_thinking_blocks, 1);
        assert_eq!(result.removed_signature_fields, 1);
        assert_eq!(adaptive["thinking"]["type"], "adaptive");
        let content = adaptive["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0].get("signature").is_none());
    }

    #[test]
    fn rectify_anthropic_request_noop_cases() {
        let user_msg =
            json!([{ "role": "user", "content": [{ "type": "text", "text": "hello" }] }]);
        for (case, mut body) in [
            (
                "clean_user_message",
                json!({ "model": "claude-test", "messages": user_msg }),
            ),
            ("no_messages", json!({ "model": "claude-test" })),
            (
                "enabled_without_tool_use",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "enabled", "budget_tokens": 1024 },
                    "messages": user_msg
                }),
            ),
            (
                "adaptive_without_budget",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "adaptive" },
                    "messages": user_msg
                }),
            ),
            (
                "adaptive_with_budget",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "adaptive", "budget_tokens": 5000 },
                    "messages": user_msg
                }),
            ),
            // 顶层 thinking 仅在 type=enabled 且 tool_use 场景删除，adaptive 不删
            (
                "adaptive_with_tool_use",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "adaptive" },
                    "messages": [
                        { "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_1", "name": "WebSearch", "input": {} }] },
                        { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }] }
                    ]
                }),
            ),
        ] {
            let original = body.clone();
            let result = rectify_anthropic_request(&mut body);
            assert!(!result.applied, "{case}");
            assert_eq!(result.removed_thinking_blocks, 0, "{case}");
            assert_eq!(body, original, "{case}");
        }
    }

    #[test]
    fn normalize_thinking_type_is_a_noop() {
        for body in [
            json!({ "model": "claude-test", "thinking": { "type": "adaptive" } }),
            json!({ "model": "claude-test", "thinking": { "type": "adaptive", "budget_tokens": 5000 } }),
            json!({ "model": "claude-test", "thinking": { "type": "enabled", "budget_tokens": 2048 } }),
            json!({ "model": "claude-test", "thinking": { "type": "unexpected", "budget_tokens": 100 } }),
            json!({ "model": "claude-test", "thinking": { "type": "disabled" } }),
            json!({ "model": "claude-test" }),
        ] {
            assert_eq!(normalize_thinking_type(body.clone()), body);
        }
    }
}
