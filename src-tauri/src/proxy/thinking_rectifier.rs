//! Thinking Signature 整流器
//!
//! 用于自动修复 Anthropic API 中因签名校验失败导致的请求错误。
//! 当上游 API 返回签名相关错误时，系统会自动移除有问题的签名字段并重试请求。
//!
//! 同时提供 **反应式 thinking 注入**：上游 API 要求回传 thinking 块时
//! (DeepSeek / Kimi / Moonshot 等),整流器仅在 API 实际拒绝后注入占位
//! thinking 块并重试,而非预注入——避免长上下文下的输出退化。

use super::types::RectifierConfig;
use serde_json::{json, Value};

/// Thinking 占位文本,用于反应式注入(仅在上游 API 拒绝时)。
pub const REACTIVE_THINKING_PLACEHOLDER: &str = "tool call";

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
    /// 注入的 thinking 占位块数量 (反应式,仅在上游 API 拒绝后)
    pub injected_thinking_blocks: usize,
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

// ============================================================================
// 反应式 thinking 注入
// ============================================================================
// 当上游 API (DeepSeek / Kimi / Moonshot 等) 要求回传 thinking 块时触发。
// 仅在上游实际拒绝后注入占位 thinking 块并重试——避免了预注入在长上下文下
// 的副作用 (输出退化、正文折叠到 thinking 块)。

/// 检测是否需要触发反应式 thinking 注入
///
/// 匹配 DeepSeek / Kimi / Moonshot 等厂商的 thinking 回传要求错误。
pub fn should_rectify_thinking_required(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> bool {
    if !config.enabled {
        return false;
    }
    // 复用 thinking_signature 子开关 — 语义上同属 thinking 兼容整流。
    if !config.request_thinking_signature {
        return false;
    }

    let Some(msg) = error_message else {
        return false;
    };
    let lower = msg.to_lowercase();

    // "content[].thinking in the thinking mode must be passed back to the API"
    // DeepSeek / Kimi / Moonshot 等 vendor 的典型错误消息。
    // 与 CCH 对齐：只对精确的 thinking-backpass 错误触发,避免误扩。
    lower.contains("thinking")
        && (lower.contains("must be passed back")
            || lower.contains("must start with a thinking block")
            || lower.contains("reasoning_content") && lower.contains("must be"))
}

/// 反应式注入 thinking 占位块
///
/// 给所有有 tool_use 但无 (或空) thinking 的 assistant 历史消息注入占位
/// thinking 块。仅在 `should_rectify_thinking_required` 返回 true 时调用。
///
/// 注入逻辑与已被移除的 claude.rs 预注入完全一致,但执行时机从「每个请求
/// 预先注入」改为「上游拒绝后按需注入」。
pub fn rectify_thinking_required(body: &mut Value) -> RectifyResult {
    let mut result = RectifyResult::default();

    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(m) => m,
        None => return result,
    };

    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        let content = match message.get_mut("content").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => continue,
        };

        if !content
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        {
            continue;
        }

        let mut has_thinking = false;
        for block in content.iter_mut() {
            if block.get("type").and_then(Value::as_str) == Some("thinking") {
                let is_empty = block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .is_none_or(|text| text.trim().is_empty());
                if is_empty {
                    if let Some(obj) = block.as_object_mut() {
                        obj.insert(
                            "thinking".to_string(),
                            Value::String(REACTIVE_THINKING_PLACEHOLDER.to_string()),
                        );
                        result.injected_thinking_blocks += 1;
                        result.applied = true;
                    }
                }
                has_thinking = true;
            }
        }

        if !has_thinking {
            content.insert(
                0,
                json!({
                    "type": "thinking",
                    "thinking": REACTIVE_THINKING_PLACEHOLDER
                }),
            );
            result.injected_thinking_blocks += 1;
            result.applied = true;
        }
    }

    result
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

    fn disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: false,
            request_thinking_budget: false,
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }

    fn master_disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: false,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }

    // ==================== should_rectify_thinking_signature 测试 ====================

    #[test]
    fn test_detect_invalid_signature() {
        assert!(should_rectify_thinking_signature(
            Some("messages.1.content.0: Invalid `signature` in `thinking` block"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_invalid_signature_no_backticks() {
        assert!(should_rectify_thinking_signature(
            Some("Messages.1.Content.0: invalid signature in thinking block"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_invalid_thought_signature_message() {
        assert!(should_rectify_thinking_signature(
            Some(
                "Unable to submit request because Thought signature is not valid.. Learn more: https://example.com/help"
            ),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_invalid_signature_nested_json() {
        // 测试嵌套 JSON 格式的错误消息（第三方渠道常见格式）
        let nested_error = r#"{"error":{"message":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"***.content.0: Invalid `signature` in `thinking` block\"},\"request_id\":\"req_xxx\"}"}}"#;
        assert!(should_rectify_thinking_signature(
            Some(nested_error),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_invalid_thought_signature_nested_json() {
        let nested_error = r#"{"error":{"message":"Unable to submit request because Thought signature is not valid.. Learn more: https://example.com/help","type":"upstream_error","param":"","code":400}}"#;
        assert!(should_rectify_thinking_signature(
            Some(nested_error),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_thinking_expected() {
        assert!(should_rectify_thinking_signature(
            Some("messages.69.content.0.type: Expected `thinking` or `redacted_thinking`, but found `tool_use`."),
            &enabled_config()
        ));
    }

    #[test]
    fn test_no_detect_thinking_expected_without_tool_use() {
        assert!(!should_rectify_thinking_signature(
            Some("messages.69.content.0.type: Expected `thinking` or `redacted_thinking`, but found `text`."),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_must_start_with_thinking() {
        assert!(should_rectify_thinking_signature(
            Some("a final `assistant` message must start with a thinking block"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_no_trigger_for_unrelated_error() {
        assert!(!should_rectify_thinking_signature(
            Some("Request timeout"),
            &enabled_config()
        ));
        assert!(!should_rectify_thinking_signature(
            Some("Connection refused"),
            &enabled_config()
        ));
        assert!(!should_rectify_thinking_signature(None, &enabled_config()));
    }

    #[test]
    fn test_detect_signature_field_required() {
        // 场景4: signature 字段缺失
        assert!(should_rectify_thinking_signature(
            Some("***.***.***.***.***.signature: Field required"),
            &enabled_config()
        ));
        // 嵌套 JSON 格式
        let nested_error = r#"{"error":{"type":"<nil>","message":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"***.***.***.***.***.signature: Field required\"},\"request_id\":\"req_xxx\"}"}}"#;
        assert!(should_rectify_thinking_signature(
            Some(nested_error),
            &enabled_config()
        ));
    }

    #[test]
    fn test_disabled_config() {
        // 即使错误匹配，配置关闭时也不触发
        assert!(!should_rectify_thinking_signature(
            Some("Invalid `signature` in `thinking` block"),
            &disabled_config()
        ));
    }

    #[test]
    fn test_master_disabled() {
        // 总开关关闭时，即使子开关开启也不触发
        assert!(!should_rectify_thinking_signature(
            Some("Invalid `signature` in `thinking` block"),
            &master_disabled_config()
        ));
    }

    // ==================== rectify_anthropic_request 测试 ====================

    #[test]
    fn test_rectify_removes_thinking_blocks() {
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
    }

    #[test]
    fn test_rectify_removes_top_level_thinking() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 1024 },
            "messages": [{
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "WebSearch", "input": {} }
                ]
            }, {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(result.applied);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn test_rectify_no_change_when_no_issues() {
        let mut body = json!({
            "model": "claude-test",
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(!result.applied);
        assert_eq!(result.removed_thinking_blocks, 0);
    }

    #[test]
    fn test_rectify_no_messages() {
        let mut body = json!({ "model": "claude-test" });
        let result = rectify_anthropic_request(&mut body);
        assert!(!result.applied);
    }

    #[test]
    fn test_rectify_preserves_thinking_when_prefix_exists() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled" },
            "messages": [{
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "some thought" },
                    { "type": "tool_use", "id": "toolu_1", "name": "Test", "input": {} }
                ]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        // thinking block 被移除，但顶层 thinking 不应被移除（因为原本有 thinking 前缀）
        assert!(result.applied);
        assert_eq!(result.removed_thinking_blocks, 1);
        // 注意：由于 thinking block 被移除后，首块变成了 tool_use，
        // 此时会触发删除顶层 thinking 的逻辑
        // 这是预期行为：整流后如果仍然不符合要求，就删除顶层 thinking
    }

    // ==================== 新增错误场景检测测试 ====================

    #[test]
    fn test_detect_signature_extra_inputs() {
        // 场景5: signature 字段不被接受
        assert!(should_rectify_thinking_signature(
            Some("xxx.signature: Extra inputs are not permitted"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_thinking_cannot_be_modified() {
        // 场景6: thinking blocks cannot be modified
        assert!(should_rectify_thinking_signature(
            Some("thinking or redacted_thinking blocks in the response cannot be modified"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_invalid_request() {
        // 场景7: 非法请求（与 CCH 对齐，统一触发）
        assert!(should_rectify_thinking_signature(
            Some("非法请求：thinking signature 不合法"),
            &enabled_config()
        ));
        assert!(should_rectify_thinking_signature(
            Some("illegal request: tool_use block mismatch"),
            &enabled_config()
        ));
        assert!(should_rectify_thinking_signature(
            Some("invalid request: malformed JSON"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_do_not_detect_thinking_type_tag_mismatch() {
        // 与 CCH 对齐：adaptive tag mismatch 不触发签名整流器
        assert!(!should_rectify_thinking_signature(
            Some("Input tag 'adaptive' found using 'type' does not match expected tags"),
            &enabled_config()
        ));
    }

    // ==================== adaptive thinking type 测试 ====================

    #[test]
    fn test_rectify_keeps_adaptive_when_no_legacy_blocks() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive" },
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(!result.applied);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn test_rectify_adaptive_preserves_existing_budget_tokens() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive", "budget_tokens": 5000 },
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(!result.applied);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["thinking"]["budget_tokens"], 5000);
    }

    #[test]
    fn test_rectify_does_not_change_enabled_type() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 1024 },
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(!result.applied);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_rectify_removes_top_level_thinking_adaptive() {
        // 顶层 thinking 仅在 type=enabled 且 tool_use 场景才会删除，adaptive 不删除
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive" },
            "messages": [{
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "WebSearch", "input": {} }
                ]
            }, {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }]
            }]
        });

        let result = rectify_anthropic_request(&mut body);

        assert!(!result.applied);
        assert_eq!(body["thinking"]["type"], "adaptive");
    }

    #[test]
    fn test_rectify_adaptive_still_cleans_legacy_signature_blocks() {
        let mut body = json!({
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

        let result = rectify_anthropic_request(&mut body);

        assert!(result.applied);
        assert_eq!(result.removed_thinking_blocks, 1);
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0].get("signature").is_none());
        assert_eq!(body["thinking"]["type"], "adaptive");
    }

    // ==================== normalize_thinking_type 测试 ====================

    #[test]
    fn test_normalize_thinking_type_adaptive_unchanged() {
        let body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive" }
        });

        let result = normalize_thinking_type(body);

        assert_eq!(result["thinking"]["type"], "adaptive");
        assert!(result["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn test_normalize_thinking_type_enabled_unchanged() {
        let body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 2048 }
        });

        let result = normalize_thinking_type(body);

        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 2048);
    }

    #[test]
    fn test_normalize_thinking_type_disabled_unchanged() {
        let body = json!({
            "model": "claude-test",
            "thinking": { "type": "disabled" }
        });

        let result = normalize_thinking_type(body);

        assert_eq!(result["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_normalize_thinking_type_preserves_budget() {
        let body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive", "budget_tokens": 5000 }
        });

        let result = normalize_thinking_type(body);

        assert_eq!(result["thinking"]["type"], "adaptive");
        assert_eq!(result["thinking"]["budget_tokens"], 5000);
    }

    #[test]
    fn test_normalize_thinking_type_no_thinking() {
        let body = json!({
            "model": "claude-test"
        });

        let result = normalize_thinking_type(body);

        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn test_normalize_thinking_type_unknown_unchanged() {
        let body = json!({
            "model": "claude-test",
            "thinking": { "type": "unexpected", "budget_tokens": 100 }
        });

        let result = normalize_thinking_type(body);

        assert_eq!(result["thinking"]["type"], "unexpected");
        assert_eq!(result["thinking"]["budget_tokens"], 100);
    }

    // ==================== should_rectify_thinking_required 测试 ====================

    #[test]
    fn test_detect_thinking_must_be_passed_back() {
        assert!(should_rectify_thinking_required(
            Some("content[0].thinking in the thinking mode must be passed back to the API"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_thinking_must_start_with_thinking_block() {
        assert!(should_rectify_thinking_required(
            Some("a final assistant message must start with a thinking block"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_thinking_required_no_trigger_for_unrelated_error() {
        assert!(!should_rectify_thinking_required(
            Some("Request timeout"),
            &enabled_config()
        ));
        assert!(!should_rectify_thinking_required(None, &enabled_config()));
    }

    #[test]
    fn test_disabled_config_should_not_rectify_thinking_required() {
        assert!(!should_rectify_thinking_required(
            Some("content[0].thinking in the thinking mode must be passed back to the API"),
            &disabled_config()
        ));
    }

    #[test]
    fn test_master_disabled_config_should_not_rectify_thinking_required() {
        assert!(!should_rectify_thinking_required(
            Some("content[0].thinking in the thinking mode must be passed back to the API"),
            &master_disabled_config()
        ));
    }

    // ==================== rectify_thinking_required 测试 ====================

    #[test]
    fn test_rectify_thinking_required_injects_for_missing_thinking() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "I will inspect."},
                    {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}
                ]
            }]
        });

        let result = rectify_thinking_required(&mut body);

        assert!(result.applied);
        assert_eq!(result.injected_thinking_blocks, 1);
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[0]["thinking"], REACTIVE_THINKING_PLACEHOLDER);
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[2]["type"], "tool_use");
    }

    #[test]
    fn test_rectify_thinking_required_fills_empty_thinking() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "", "signature": "sig"},
                    {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}
                ]
            }]
        });

        let result = rectify_thinking_required(&mut body);

        assert!(result.applied);
        assert_eq!(result.injected_thinking_blocks, 1);
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["thinking"], REACTIVE_THINKING_PLACEHOLDER);
    }

    #[test]
    fn test_rectify_thinking_required_noop_when_thinking_exists() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "Real reasoning here."},
                    {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}
                ]
            }]
        });

        let original = body.clone();
        let result = rectify_thinking_required(&mut body);

        assert!(!result.applied);
        assert_eq!(result.injected_thinking_blocks, 0);
        assert_eq!(body, original);
    }

    #[test]
    fn test_rectify_thinking_required_skips_non_assistant_messages() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello"}]},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}
                ]}
            ]
        });

        let result = rectify_thinking_required(&mut body);

        assert!(result.applied);
        // Only the assistant message gets injection, user message is untouched
        let assistant_content = body["messages"][1]["content"].as_array().unwrap();
        assert_eq!(assistant_content[0]["type"], "thinking");
        assert_eq!(assistant_content[0]["thinking"], REACTIVE_THINKING_PLACEHOLDER);
    }

    #[test]
    fn test_rectify_thinking_required_no_messages() {
        let mut body = json!({"model": "deepseek-v4-pro"});
        let result = rectify_thinking_required(&mut body);
        assert!(!result.applied);
    }
}
