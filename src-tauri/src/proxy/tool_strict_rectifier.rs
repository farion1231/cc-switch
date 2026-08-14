//! Tool `strict` compatibility rectifier.
//!
//! Some Anthropic-compatible gateways ultimately invoke Claude through AWS
//! Bedrock. Bedrock rejects tool-level `strict` for some Claude models even
//! though native Anthropic accepts it. Keep the field on the first attempt and
//! only remove it after the upstream reports the exact unsupported-field error.

use super::ProxyError;
use serde_json::Value;

/// Returns true when an upstream explicitly rejects tool-level `strict`.
pub fn should_rectify_tool_strict(error: &ProxyError) -> bool {
    let ProxyError::UpstreamError {
        status: 400,
        body: Some(body),
    } = error
    else {
        return false;
    };

    body.to_ascii_lowercase()
        .contains(".strict: extra inputs are not permitted")
}

/// Removes tool declaration `strict` fields from supported request shapes.
///
/// Handles:
/// - OpenAI Responses / Anthropic tools: `tools[*].strict`
/// - OpenAI Chat tools: `tools[*].function.strict`
/// - Namespace and `tool_search_output` tool arrays nested under `tools`
///
/// JSON Schema contents are not traversed, so a business property named
/// `strict` remains intact.
pub fn rectify_tool_strict(body: &mut Value) -> usize {
    strip_nested_tool_arrays(body)
}

fn strip_nested_tool_arrays(value: &mut Value) -> usize {
    match value {
        Value::Object(object) => {
            let mut removed = 0;
            for (key, child) in object {
                if key == "tools" {
                    removed += strip_tool_array(child);
                } else if !matches!(key.as_str(), "parameters" | "input_schema") {
                    removed += strip_nested_tool_arrays(child);
                }
            }
            removed
        }
        Value::Array(values) => values.iter_mut().map(strip_nested_tool_arrays).sum(),
        _ => 0,
    }
}

fn strip_tool_array(value: &mut Value) -> usize {
    let Some(tools) = value.as_array_mut() else {
        return 0;
    };

    tools
        .iter_mut()
        .map(|tool| {
            let Some(object) = tool.as_object_mut() else {
                return 0;
            };

            let mut removed = usize::from(object.remove("strict").is_some());
            if let Some(function) = object.get_mut("function").and_then(Value::as_object_mut) {
                removed += usize::from(function.remove("strict").is_some());
            }
            if let Some(nested_tools) = object.get_mut("tools") {
                removed += strip_tool_array(nested_tools);
            }
            removed
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upstream_error(status: u16, message: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: Some(json!({ "error": { "message": message } }).to_string()),
        }
    }

    #[test]
    fn detects_bedrock_tool_strict_rejection() {
        for message in [
            "tools.0.custom.strict: Extra inputs are not permitted",
            "***.***.***.strict: Extra inputs are not permitted",
        ] {
            assert!(should_rectify_tool_strict(&upstream_error(400, message)));
        }
    }

    #[test]
    fn ignores_unrelated_or_non_400_errors() {
        assert!(!should_rectify_tool_strict(&upstream_error(
            400,
            "tools.0.input_schema: Extra inputs are not permitted"
        )));
        assert!(!should_rectify_tool_strict(&upstream_error(
            422,
            "tools.0.custom.strict: Extra inputs are not permitted"
        )));
    }

    #[test]
    fn strips_responses_chat_and_nested_tool_strict() {
        let mut body = json!({
            "tools": [
                {
                    "type": "function",
                    "name": "top_level",
                    "strict": true,
                    "parameters": {
                        "type": "object",
                        "properties": { "strict": { "type": "boolean" } }
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "chat_tool",
                        "strict": false,
                        "parameters": { "type": "object" }
                    }
                },
                {
                    "type": "namespace",
                    "name": "nested",
                    "tools": [{
                        "type": "function",
                        "name": "child",
                        "strict": true,
                        "parameters": { "type": "object" }
                    }]
                }
            ],
            "input": [{
                "type": "tool_search_output",
                "tools": [{
                    "type": "function",
                    "name": "dynamic",
                    "strict": true,
                    "parameters": { "type": "object" }
                }]
            }]
        });

        assert_eq!(rectify_tool_strict(&mut body), 4);
        assert!(body["tools"][0].get("strict").is_none());
        assert_eq!(
            body["tools"][0]["parameters"]["properties"]["strict"]["type"],
            "boolean"
        );
        assert!(body["tools"][1]["function"].get("strict").is_none());
        assert!(body["tools"][2]["tools"][0].get("strict").is_none());
        assert!(body["input"][0]["tools"][0].get("strict").is_none());
    }

    #[test]
    fn reports_no_change_when_tools_have_no_strict_field() {
        let mut body = json!({
            "tools": [{
                "name": "plain",
                "input_schema": { "type": "object", "properties": {} }
            }]
        });

        assert_eq!(rectify_tool_strict(&mut body), 0);
    }
}
