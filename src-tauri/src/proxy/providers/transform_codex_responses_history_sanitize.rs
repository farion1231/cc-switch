//! Compatibility cleanup for Codex history replayed to third-party Responses APIs.

use serde_json::Value;

const PRIVATE_TOOL_SEARCH_HISTORY_TYPES: &[&str] = &["tool_search_call", "tool_search_output"];

/// Drops Codex-private tool-search history items from a native Responses body.
///
/// Third-party Responses servers do not recognize these client-executed item
/// types, while standard function-call pairs remain valid and must be kept.
/// Both halves of every tool-search interaction use one of these types, so
/// filtering them together cannot leave a dangling standard tool call/output.
pub(crate) fn strip_private_tool_search_history_items(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };

    let original_len = input.len();
    input.retain(|item| {
        !item
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|item_type| PRIVATE_TOOL_SEARCH_HISTORY_TYPES.contains(&item_type))
    });
    original_len - input.len()
}

#[cfg(test)]
mod tests {
    use super::strip_private_tool_search_history_items;
    use serde_json::json;

    #[test]
    fn strips_tool_search_pairs_without_touching_supported_history() {
        let mut body = json!({
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "function_call", "call_id": "function-1", "name": "read", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "function-1", "output": "ok"},
                {"type": "tool_search_call", "call_id": "search-1", "arguments": {"query": "tools"}},
                {"type": "tool_search_output", "call_id": "search-1", "tools": [{"type": "function", "name": "find"}]},
                {"type": "reasoning", "summary": []}
            ]
        });

        assert_eq!(strip_private_tool_search_history_items(&mut body), 2);
        assert_eq!(
            body["input"],
            json!([
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "function_call", "call_id": "function-1", "name": "read", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "function-1", "output": "ok"},
                {"type": "reasoning", "summary": []}
            ])
        );
    }
}
