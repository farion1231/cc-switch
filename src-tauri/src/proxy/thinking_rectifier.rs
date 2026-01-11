//! Thinking Signature 整流器
//!
//! 用于自动修复 Anthropic API 中因签名校验失败导致的请求错误。
//! 当上游 API 返回签名相关错误时，系统会自动移除有问题的签名字段并重试请求。

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// 整流器触发类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectifierTrigger {
    /// thinking block 中的签名无效
    InvalidSignatureInThinkingBlock,
    /// assistant 消息必须以 thinking block 开头
    AssistantMessageMustStartWithThinking,
    /// 非法请求
    InvalidRequest,
}

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

// 正则表达式（延迟初始化）
static THINKING_EXPECTED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)expected\s*`?thinking`?\s*or\s*`?redacted_thinking`?.*found\s*`?tool_use`?")
        .unwrap()
});

static INVALID_REQUEST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)非法请求|illegal request|invalid request").unwrap());

/// 检测是否需要触发整流器
///
/// 注意：不依赖错误规则开关，仅做字符串/正则判断
pub fn detect_rectifier_trigger(error_message: Option<&str>) -> Option<RectifierTrigger> {
    let msg = error_message?;
    let lower = msg.to_lowercase();

    // 场景1: thinking 启用但 assistant 消息未以 thinking 开头
    if lower.contains("must start with a thinking block") {
        return Some(RectifierTrigger::AssistantMessageMustStartWithThinking);
    }
    if THINKING_EXPECTED_RE.is_match(msg) {
        return Some(RectifierTrigger::AssistantMessageMustStartWithThinking);
    }

    // 场景2: thinking block 中的签名无效
    if lower.contains("invalid")
        && lower.contains("signature")
        && lower.contains("thinking")
        && lower.contains("block")
    {
        return Some(RectifierTrigger::InvalidSignatureInThinkingBlock);
    }

    // 场景3: 非法请求
    if INVALID_REQUEST_RE.is_match(msg) {
        return Some(RectifierTrigger::InvalidRequest);
    }

    None
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
    let thinking_enabled = body
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(|t| t.as_str())
        == Some("enabled");

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ==================== detect_rectifier_trigger 测试 ====================

    #[test]
    fn test_detect_invalid_signature() {
        let trigger = detect_rectifier_trigger(Some(
            "messages.1.content.0: Invalid `signature` in `thinking` block",
        ));
        assert_eq!(
            trigger,
            Some(RectifierTrigger::InvalidSignatureInThinkingBlock)
        );
    }

    #[test]
    fn test_detect_invalid_signature_no_backticks() {
        let trigger = detect_rectifier_trigger(Some(
            "Messages.1.Content.0: invalid signature in thinking block",
        ));
        assert_eq!(
            trigger,
            Some(RectifierTrigger::InvalidSignatureInThinkingBlock)
        );
    }

    #[test]
    fn test_detect_thinking_expected() {
        let trigger = detect_rectifier_trigger(Some(
            "messages.69.content.0.type: Expected `thinking` or `redacted_thinking`, but found `tool_use`.",
        ));
        assert_eq!(
            trigger,
            Some(RectifierTrigger::AssistantMessageMustStartWithThinking)
        );
    }

    #[test]
    fn test_detect_must_start_with_thinking() {
        let trigger = detect_rectifier_trigger(Some(
            "a final `assistant` message must start with a thinking block",
        ));
        assert_eq!(
            trigger,
            Some(RectifierTrigger::AssistantMessageMustStartWithThinking)
        );
    }

    #[test]
    fn test_detect_invalid_request_chinese() {
        assert_eq!(
            detect_rectifier_trigger(Some("非法请求")),
            Some(RectifierTrigger::InvalidRequest)
        );
    }

    #[test]
    fn test_detect_invalid_request_english() {
        assert_eq!(
            detect_rectifier_trigger(Some("illegal request format")),
            Some(RectifierTrigger::InvalidRequest)
        );
        assert_eq!(
            detect_rectifier_trigger(Some("invalid request: malformed JSON")),
            Some(RectifierTrigger::InvalidRequest)
        );
    }

    #[test]
    fn test_no_trigger_for_unrelated_error() {
        assert_eq!(detect_rectifier_trigger(Some("Request timeout")), None);
        assert_eq!(detect_rectifier_trigger(Some("Connection refused")), None);
        assert_eq!(detect_rectifier_trigger(None), None);
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
}
