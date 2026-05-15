//! Tool Use ID 整流器
//!
//! 在发送到 Anthropic/Claude 严格校验链路前，修复历史消息里的
//! `tool_use.id` 与 `tool_result.tool_use_id` 兼容性问题。

use serde_json::Value;
use std::collections::{HashMap, HashSet};

const MAX_ID_LEN: usize = 64;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ToolUseIdRectifyResult {
    pub applied: bool,
    pub rewritten_tool_use_ids: usize,
    pub rewritten_tool_result_ids: usize,
}

/// 对 Anthropic 请求体做 tool_use id 兼容整流。
pub fn rectify_anthropic_tool_use_ids(body: &mut Value) -> ToolUseIdRectifyResult {
    let mut result = ToolUseIdRectifyResult::default();

    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return result;
    };

    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut counter: usize = 0;

    // Pass 1: 处理 tool_use.id
    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let Some(obj) = block.as_object_mut() else { continue };
            let old_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let new_id = get_or_create_mapping(&old_id, &mut id_map, &mut used, &mut counter);
            if new_id != old_id {
                obj.insert("id".to_string(), Value::String(new_id));
                result.rewritten_tool_use_ids += 1;
                result.applied = true;
            }
        }
    }

    // Pass 2: 用同一映射修正 tool_result.tool_use_id
    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let Some(obj) = block.as_object_mut() else { continue };
            let old_id = obj.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let new_id = get_or_create_mapping(&old_id, &mut id_map, &mut used, &mut counter);
            if new_id != old_id {
                obj.insert("tool_use_id".to_string(), Value::String(new_id));
                result.rewritten_tool_result_ids += 1;
                result.applied = true;
            }
        }
    }

    result
}

fn get_or_create_mapping(
    old_id: &str,
    id_map: &mut HashMap<String, String>,
    used: &mut HashSet<String>,
    counter: &mut usize,
) -> String {
    if let Some(existing) = id_map.get(old_id) {
        return existing.clone();
    }
    if is_valid_id(old_id) && !used.contains(old_id) {
        used.insert(old_id.to_string());
        id_map.insert(old_id.to_string(), old_id.to_string());
        return old_id.to_string();
    }
    let new_id = make_unique_id(old_id, used, counter);
    id_map.insert(old_id.to_string(), new_id.clone());
    new_id
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn make_unique_id(original: &str, used: &mut HashSet<String>, counter: &mut usize) -> String {
    // 清理非法字符
    let mut base: String = original
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if base.is_empty() {
        base = "toolu".to_string();
    }
    if base.len() > MAX_ID_LEN {
        base.truncate(MAX_ID_LEN);
    }

    if !used.contains(&base) {
        used.insert(base.clone());
        return base;
    }

    // 碰撞时加计数器后缀
    loop {
        *counter += 1;
        let suffix = format!("_{counter}");
        let max_base = MAX_ID_LEN.saturating_sub(suffix.len());
        let mut candidate = base.clone();
        candidate.truncate(max_base);
        candidate.push_str(&suffix);
        if !used.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rectifies_invalid_ids() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_functions.Bash:10", "name": "Bash", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_functions.Bash:10", "content": "ok"}
                ]}
            ]
        });

        let result = rectify_anthropic_tool_use_ids(&mut body);
        assert!(result.applied);
        assert_eq!(result.rewritten_tool_use_ids, 1);
        assert_eq!(result.rewritten_tool_result_ids, 1);

        let new_id = body["messages"][0]["content"][0]["id"].as_str().unwrap();
        let ref_id = body["messages"][1]["content"][0]["tool_use_id"].as_str().unwrap();
        assert_eq!(new_id, ref_id);
        assert!(is_valid_id(new_id));
    }

    #[test]
    fn keeps_valid_ids_unchanged() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_abc_123", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_abc_123", "content": "ok"}
                ]}
            ]
        });

        let result = rectify_anthropic_tool_use_ids(&mut body);
        assert!(!result.applied);
        assert_eq!(body["messages"][0]["content"][0]["id"], "toolu_abc_123");
    }

    #[test]
    fn resolves_collisions() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "a:b", "name": "Read", "input": {}},
                    {"type": "tool_use", "id": "a.b", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "a:b", "content": "1"},
                    {"type": "tool_result", "tool_use_id": "a.b", "content": "2"}
                ]}
            ]
        });

        let result = rectify_anthropic_tool_use_ids(&mut body);
        assert!(result.applied);

        let id_a = body["messages"][0]["content"][0]["id"].as_str().unwrap();
        let id_b = body["messages"][0]["content"][1]["id"].as_str().unwrap();
        assert_ne!(id_a, id_b);
        assert!(is_valid_id(id_a));
        assert!(is_valid_id(id_b));
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], id_a);
        assert_eq!(body["messages"][1]["content"][1]["tool_use_id"], id_b);
    }
}
