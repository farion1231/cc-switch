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

pub(crate) fn normalize_tool_result_content(messages: &mut Vec<serde_json::Value>) {
    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let Some(obj) = block.as_object_mut() else { continue; };
            let normalized: String = match obj.get("content") {
                None => String::new(),
                Some(serde_json::Value::String(_)) => continue,
                Some(serde_json::Value::Array(arr)) => {
                    arr.iter()
                        .map(|item| {
                            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                item.get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                serde_json::to_string(item).unwrap_or_default()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                Some(other) => serde_json::to_string(other).unwrap_or_default(),
            };
            obj.insert("content".into(), serde_json::Value::String(normalized));
        }
    }
}

#[cfg(test)]
mod tests_normalize_tool_result {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_string_content_unchanged() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "already string"}]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "already string");
    }

    #[test]
    fn test_array_of_text_blocks_joined() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [
                    {"type": "text", "text": "line1"},
                    {"type": "text", "text": "line2"}
                ]
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "line1\nline2");
    }

    #[test]
    fn test_array_with_non_text_block_json_serialized() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [
                    {"type": "text", "text": "result:"},
                    {"type": "image", "source": {"type": "url", "url": "http://x"}}
                ]
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        let s = msgs[0]["content"][0]["content"].as_str().unwrap();
        assert!(s.contains("result:"));
        assert!(s.contains("image"));
    }

    #[test]
    fn test_dict_content_json_serialized() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": {"key": "val"}
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        let s = msgs[0]["content"][0]["content"].as_str().unwrap();
        assert!(s.contains("key"));
        assert!(s.contains("val"));
    }

    #[test]
    fn test_null_content_becomes_empty_string() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "t1"}]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "");
    }

    #[test]
    fn test_non_tool_result_blocks_not_touched() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "tool_result", "tool_use_id": "t1", "content": ["should be string"]}
            ]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["text"], "hi");
    }
}

pub(crate) fn filter_context_management_edits(body: &mut serde_json::Value) {
    let Some(obj) = body.as_object_mut() else { return; };
    let Some(cm) = obj.get_mut("context_management").and_then(|v| v.as_object_mut()) else {
        return;
    };

    if let Some(edits) = cm.get_mut("edits").and_then(|v| v.as_array_mut()) {
        edits.retain(|e| {
            e.get("type")
                .and_then(|t| t.as_str())
                .map(|t| !t.starts_with("clear_thinking_"))
                .unwrap_or(true)
        });
    }

    let edits_empty = cm
        .get("edits")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(false);
    if edits_empty {
        cm.remove("edits");
    }

    if cm.is_empty() {
        obj.remove("context_management");
    }
}

#[cfg(test)]
mod tests_context_management {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_clear_thinking_edit_removed() {
        let mut body = json!({
            "context_management": {
                "edits": [
                    {"type": "clear_thinking_blocks"},
                    {"type": "keep_this"}
                ]
            }
        });
        filter_context_management_edits(&mut body);
        let edits = body["context_management"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["type"], "keep_this");
    }

    #[test]
    fn test_edits_all_removed_field_deleted() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_history"}]
            }
        });
        filter_context_management_edits(&mut body);
        assert!(body["context_management"].get("edits").is_none());
    }

    #[test]
    fn test_context_management_empty_object_deleted() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_blocks"}]
            }
        });
        filter_context_management_edits(&mut body);
        assert!(body.get("context_management").is_none());
    }

    #[test]
    fn test_context_management_other_fields_kept() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_blocks"}],
                "other_field": "value"
            }
        });
        filter_context_management_edits(&mut body);
        assert_eq!(body["context_management"]["other_field"], "value");
        assert!(body["context_management"].get("edits").is_none());
    }

    #[test]
    fn test_no_context_management_no_panic() {
        let mut body = json!({"model": "m", "messages": []});
        filter_context_management_edits(&mut body);
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn test_multiple_clear_thinking_variants_all_removed() {
        let mut body = json!({
            "context_management": {
                "edits": [
                    {"type": "clear_thinking_blocks"},
                    {"type": "clear_thinking_history"},
                    {"type": "clear_thinking_foo_bar"},
                    {"type": "normal_edit"}
                ]
            }
        });
        filter_context_management_edits(&mut body);
        let edits = body["context_management"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["type"], "normal_edit");
    }
}

fn detect_tool_history(body: &serde_json::Value) -> bool {
    let Some(messages) = body.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };
    messages.iter().any(|msg| {
        msg.get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            })
            .unwrap_or(false)
    })
}

fn detect_replayable_thinking_before_tool_use(body: &serde_json::Value) -> bool {
    let Some(messages) = body.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let Some(content) = msg.get("content").and_then(|c| c.as_array()) else {
            continue;
        };
        let has_tool_use = content
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
        if !has_tool_use {
            continue;
        }
        let has_thinking = content
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"));
        if has_thinking {
            return true;
        }
    }
    false
}

pub(crate) fn rebuild_thinking_field(body: &mut serde_json::Value, target_model: &str) -> bool {
    // Run immutable checks before taking mutable borrow
    let has_tool_history = detect_tool_history(body);
    let has_replayable = detect_replayable_thinking_before_tool_use(body);

    let obj = body.as_object_mut().expect("body must be object");

    let original_thinking = obj.remove("thinking");
    let client_intent: Option<bool> = original_thinking
        .as_ref()
        .and_then(|t| t.get("type"))
        .and_then(|s| s.as_str())
        .and_then(|s| match s {
            "enabled" => Some(true),
            "disabled" => Some(false),
            _ => None,
        });

    let target_default =
        crate::proxy::providers::deepseek_anthropic::model_mapping::is_reasoner_target(
            target_model,
        );
    let intended = client_intent.unwrap_or(target_default);

    let unsafe_tool_followup = has_tool_history && !has_replayable;

    let effective = intended && !unsafe_tool_followup;

    if effective {
        let budget_tokens = original_thinking
            .as_ref()
            .and_then(|t| t.get("budget_tokens"))
            .cloned();
        let mut thinking_obj = serde_json::Map::new();
        thinking_obj.insert(
            "type".into(),
            serde_json::Value::String("enabled".into()),
        );
        if let Some(bt) = budget_tokens {
            thinking_obj.insert("budget_tokens".into(), bt);
        }
        obj.insert(
            "thinking".into(),
            serde_json::Value::Object(thinking_obj),
        );
    } else {
        obj.insert(
            "thinking".into(),
            serde_json::json!({"type": "disabled"}),
        );
    }

    effective
}

#[cfg(test)]
mod tests_thinking_rebuild {
    use super::*;
    use serde_json::json;

    fn make_body_with_tool_history(has_thinking: bool) -> serde_json::Value {
        json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {
                    "role": "assistant",
                    "content": if has_thinking {
                        json!([
                            {"type": "thinking", "thinking": "chain"},
                            {"type": "tool_use", "id": "t1", "name": "bash", "input": {}}
                        ])
                    } else {
                        json!([
                            {"type": "tool_use", "id": "t1", "name": "bash", "input": {}}
                        ])
                    }
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]
                },
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "next turn"}]
                }
            ]
        })
    }

    #[test]
    fn test_detect_tool_history_true_when_tool_result_present() {
        let body = make_body_with_tool_history(false);
        assert!(detect_tool_history(&body));
    }

    #[test]
    fn test_detect_tool_history_false_when_no_tool_result() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "hello"}]}
            ]
        });
        assert!(!detect_tool_history(&body));
    }

    #[test]
    fn test_detect_replayable_thinking_true_when_thinking_before_tool_use() {
        let body = make_body_with_tool_history(true);
        assert!(detect_replayable_thinking_before_tool_use(&body));
    }

    #[test]
    fn test_detect_replayable_thinking_false_when_no_thinking() {
        let body = make_body_with_tool_history(false);
        assert!(!detect_replayable_thinking_before_tool_use(&body));
    }

    #[test]
    fn test_pro_no_tool_history_default_enabled() {
        let mut body = json!({"model": "deepseek-v4-pro", "messages": []});
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_pro_unsafe_tool_followup_forced_disabled() {
        let mut body = make_body_with_tool_history(false);
        body["thinking"] = json!(null);
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_pro_explicit_disabled_respected() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [],
            "thinking": {"type": "disabled"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_flash_no_client_intent_default_disabled() {
        let mut body = json!({"model": "deepseek-v4-flash", "messages": []});
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_flash_client_explicit_enabled_respected() {
        let mut body = json!({
            "model": "deepseek-v4-flash",
            "messages": [],
            "thinking": {"type": "enabled"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_unknown_thinking_type_falls_back_to_target_default() {
        let mut body = json!({
            "model": "deepseek-v4-flash",
            "messages": [],
            "thinking": {"type": "future_type"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(!enabled);
    }

    #[test]
    fn test_pro_with_replayable_thinking_stays_enabled() {
        let mut body = make_body_with_tool_history(true);
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }
}

pub(crate) fn filter_server_tools(body: &mut serde_json::Value) {
    let Some(tools) = body.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return;
    };
    tools.retain(|tool| {
        let type_val = tool.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if type_val.starts_with("web_search")
            || type_val.starts_with("web_fetch")
            || type_val.starts_with("computer_")
            || type_val.starts_with("text_editor_")
        {
            return false;
        }
        let name_val = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name_val == "web_search" || name_val == "web_fetch" {
            return false;
        }
        true
    });
}

#[cfg(test)]
mod tests_tools_blacklist {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plain_client_tool_preserved() {
        let mut body = json!({
            "tools": [{"name": "Bash", "description": "run bash", "input_schema": {}}]
        });
        filter_server_tools(&mut body);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tools"][0]["name"], "Bash");
    }

    #[test]
    fn test_web_search_type_removed() {
        let mut body = json!({
            "tools": [
                {"type": "web_search_20250305", "name": "web_search"},
                {"name": "Bash", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Bash");
    }

    #[test]
    fn test_web_search_name_double_guard_removed() {
        let mut body = json!({
            "tools": [
                {"name": "web_search", "input_schema": {}},
                {"name": "Bash", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Bash");
    }

    #[test]
    fn test_computer_type_removed() {
        let mut body = json!({
            "tools": [{"type": "computer_20250124", "name": "computer"}]
        });
        filter_server_tools(&mut body);
        assert!(body["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_text_editor_type_removed() {
        let mut body = json!({
            "tools": [{"type": "text_editor_20250124", "name": "str_replace_based_edit_tool"}]
        });
        filter_server_tools(&mut body);
        assert!(body["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_no_tools_field_no_panic() {
        let mut body = json!({"model": "m"});
        filter_server_tools(&mut body);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_web_fetch_type_removed() {
        let mut body = json!({
            "tools": [
                {"type": "web_fetch", "name": "web_fetch"},
                {"name": "mcp_tool", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "mcp_tool");
    }
}
