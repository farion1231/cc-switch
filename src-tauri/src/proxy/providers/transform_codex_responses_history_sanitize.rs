//! Compatibility cleanup for Codex history replayed to known strict Responses APIs.

use serde_json::Value;
use url::Url;

const PRIVATE_TOOL_SEARCH_HISTORY_TYPES: &[&str] = &["tool_search_call", "tool_search_output"];

/// Drops tool-search history only for an upstream confirmed to reject those items.
/// OpenAI's Responses API and other compatible endpoints must receive the items
/// unchanged because `tool_search_output.tools` may be the sole definition of a
/// dynamically loaded client tool.
pub(crate) fn strip_tool_search_history_for_upstream(body: &mut Value, base_url: &str) -> usize {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return 0;
    };
    if url.host_str() != Some("88api.ai") {
        return 0;
    }
    strip_private_tool_search_history_items(body)
}

/// Drops tool-search history items from a native Responses body.
///
/// The known strict upstream does not recognize these client-executed item
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
    use super::{strip_private_tool_search_history_items, strip_tool_search_history_for_upstream};
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

    #[test]
    fn openai_api_key_responses_preserve_client_loaded_tools() {
        let mut body = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search", "execution": "client"}],
            "input": [
                {"type": "message", "role": "user", "content": "Find an ETA tool"},
                {"type": "tool_search_call", "execution": "client", "call_id": "search-1", "arguments": {"goal": "shipping ETA"}},
                {"type": "tool_search_output", "execution": "client", "call_id": "search-1", "tools": [{"type": "function", "name": "get_shipping_eta", "parameters": {"type": "object"}}]}
            ]
        });
        let original = body.clone();

        assert_eq!(
            strip_tool_search_history_for_upstream(&mut body, "https://api.openai.com/v1"),
            0
        );
        assert_eq!(body, original, "the loaded tool definition must survive");
    }

    #[test]
    fn known_strict_responses_upstream_drops_only_search_history() {
        let mut body = json!({
            "input": [
                {"type": "message", "role": "user", "content": "hello"},
                {"type": "tool_search_call", "call_id": "search-1"},
                {"type": "tool_search_output", "call_id": "search-1", "tools": [{"type": "function", "name": "find"}]},
                {"type": "function_call", "call_id": "function-1", "name": "read", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "function-1", "output": "ok"}
            ]
        });

        assert_eq!(
            strip_tool_search_history_for_upstream(&mut body, "https://88api.ai/v1"),
            2
        );
        assert_eq!(body["input"].as_array().unwrap().len(), 3);
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][2]["type"], "function_call_output");
    }
}
