// Implemented by T04a–T07
use serde_json::{json, Value};

pub(crate) const UNSUPPORTED_BLOCK_TYPES: &[&str] = &[
    "image",
    "document",
    "search_result",
    "server_tool_use",
    "web_search_tool_result",
    "code_execution_tool_result",
    "mcp_tool_use",
    "mcp_tool_result",
    "container_upload",
];

fn is_unsupported_block(v: &Value) -> bool {
    v.get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| UNSUPPORTED_BLOCK_TYPES.contains(&t))
}

pub(crate) fn strip_unsupported_attachments(messages: &mut Vec<Value>) {
    const PLACEHOLDER_TEXT: &str = "[attachment omitted: DeepSeek does not support image, document, search_result, server_tool_use, web_search_tool_result, code_execution_tool_result, mcp_tool_use, mcp_tool_result, container_upload]";

    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) else {
            continue;
        };

        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                if let Some(inner) = block.get_mut("content").and_then(|v| v.as_array_mut()) {
                    inner.retain(|b| !is_unsupported_block(b));
                    if inner.is_empty() {
                        inner.push(json!({"type": "text", "text": PLACEHOLDER_TEXT}));
                    }
                }
            }
        }

        content.retain(|b| !is_unsupported_block(b));
        if content.is_empty() {
            content.push(json!({"type": "text", "text": PLACEHOLDER_TEXT}));
        }
    }
}

#[cfg(test)]
mod tests_strip_unsupported {
    use super::*;
    use serde_json::json;

    const PLACEHOLDER: &str = "[attachment omitted: DeepSeek does not support image, document, search_result, server_tool_use, web_search_tool_result, code_execution_tool_result, mcp_tool_use, mcp_tool_result, container_upload]";

    #[test]
    fn test_image_removed_from_user_content() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "data": "abc"}},
                {"type": "text", "text": "describe this"}
            ]
        })];
        strip_unsupported_attachments(&mut messages);
        let content = messages[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_all_9_types_removed() {
        for block_type in UNSUPPORTED_BLOCK_TYPES {
            let mut messages = vec![json!({
                "role": "user",
                "content": [
                    {"type": block_type},
                    {"type": "text", "text": "ok"}
                ]
            })];
            strip_unsupported_attachments(&mut messages);
            let content = messages[0]["content"].as_array().unwrap();
            assert_eq!(content.len(), 1, "type={} should be removed", block_type);
            assert_eq!(content[0]["type"], "text");
        }
    }

    #[test]
    fn test_empty_content_gets_placeholder() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{"type": "image", "source": {}}]
        })];
        strip_unsupported_attachments(&mut messages);
        let content = messages[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"].as_str().unwrap(), PLACEHOLDER);
    }

    #[test]
    fn test_tool_result_inner_image_removed() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [
                    {"type": "image", "source": {}},
                    {"type": "text", "text": "result text"}
                ]
            }]
        })];
        strip_unsupported_attachments(&mut messages);
        let tool_result = &messages[0]["content"][0];
        let inner = tool_result["content"].as_array().unwrap();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0]["type"], "text");
    }

    #[test]
    fn test_tool_result_inner_all_removed_gets_placeholder() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [{"type": "image", "source": {}}]
            }]
        })];
        strip_unsupported_attachments(&mut messages);
        let inner = messages[0]["content"][0]["content"].as_array().unwrap();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0]["type"], "text");
        assert_eq!(inner[0]["text"].as_str().unwrap(), PLACEHOLDER);
    }

    #[test]
    fn test_mcp_tool_use_vs_mcp_servers_independent() {
        let mut messages = vec![json!({
            "role": "assistant",
            "content": [{"type": "mcp_tool_use", "id": "x"}]
        })];
        strip_unsupported_attachments(&mut messages);
        let content = messages[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_text_block_preserved() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{"type": "text", "text": "hello"}]
        })];
        strip_unsupported_attachments(&mut messages);
        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn test_string_content_not_touched() {
        let mut messages = vec![json!({
            "role": "user",
            "content": "plain string"
        })];
        strip_unsupported_attachments(&mut messages);
        assert_eq!(messages[0]["content"], "plain string");
    }
}

pub(crate) fn sanitize_thinking_blocks(
    messages: &mut Vec<serde_json::Value>,
    effective_thinking_enabled: bool,
) {
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        content.retain(|block| {
            let Some(t) = block.get("type").and_then(|s| s.as_str()) else {
                return true;
            };
            if t.starts_with("redacted_thinking") {
                return false;
            }
            if t == "thinking" && !effective_thinking_enabled {
                return false;
            }
            true
        });
    }
}

pub(crate) fn strip_reasoning_content(messages: &mut Vec<serde_json::Value>) {
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            obj.remove("reasoning_content");
        }
    }
}

#[cfg(test)]
mod tests_thinking_blocks {
    use super::*;
    use serde_json::json;

    fn make_assistant_msg(blocks: serde_json::Value) -> serde_json::Value {
        json!({"role": "assistant", "content": blocks})
    }

    #[test]
    fn test_keeps_text_blocks_unchanged() {
        let mut msgs = vec![make_assistant_msg(json!([{"type":"text","text":"hi"}]))];
        sanitize_thinking_blocks(&mut msgs, false);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_removes_redacted_thinking_always() {
        let mut msgs = vec![make_assistant_msg(json!([
            {"type":"redacted_thinking","data":"x"},
            {"type":"text","text":"hi"}
        ]))];
        sanitize_thinking_blocks(&mut msgs, true);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 1);
        assert_eq!(msgs[0]["content"][0]["type"], "text");
    }

    #[test]
    fn test_removes_thinking_when_disabled() {
        let mut msgs = vec![make_assistant_msg(json!([
            {"type":"thinking","thinking":"..."},
            {"type":"text","text":"hi"}
        ]))];
        sanitize_thinking_blocks(&mut msgs, false);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_keeps_thinking_when_enabled() {
        let mut msgs = vec![make_assistant_msg(json!([
            {"type":"thinking","thinking":"..."},
            {"type":"text","text":"hi"}
        ]))];
        sanitize_thinking_blocks(&mut msgs, true);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_skips_non_assistant_roles() {
        let mut msgs = vec![
            json!({"role":"user","content":[{"type":"thinking","thinking":"..."}]}),
        ];
        sanitize_thinking_blocks(&mut msgs, false);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_strip_reasoning_content_removes_field() {
        let mut msgs = vec![
            json!({"role":"assistant","content":"hi","reasoning_content":"secret"}),
        ];
        strip_reasoning_content(&mut msgs);
        assert!(msgs[0].get("reasoning_content").is_none());
    }

    #[test]
    fn test_strip_reasoning_content_noop_when_absent() {
        let mut msgs = vec![json!({"role":"user","content":"hello"})];
        strip_reasoning_content(&mut msgs);
        assert_eq!(msgs[0]["role"], "user");
    }
}
