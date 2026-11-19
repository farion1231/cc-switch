//! Codex freeform `custom` tools over native Responses passthrough.
//!
//! Codex sends `apply_patch` (and, under `tool_mode = "code_mode_only"`, the
//! code-mode `exec` tool) as `{"type":"custom"}` tools. Many third-party
//! Responses gateways only accept JSON `function` tools and either reject the
//! request or silently drop the tool, so the model never sees it (#9280).
//!
//! When a provider opts in (`codexCustomToolsAsFunctions`), the request side
//! declares each custom tool as a function with one string argument (`input`),
//! using the same wording as the Chat bridge so both routes prompt identically,
//! and rewrites `custom_tool_call` history into `function_call`. The response
//! side turns those function calls back into `custom_tool_call` items, because
//! Codex rejects a `function_call` addressed to a custom tool ("incompatible
//! payload"). Argument deltas for converted calls are dropped: Codex reads the
//! finished item from `response.output_item.done`.

use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use super::transform_codex_chat::{
    custom_tool_input_from_chat_arguments, responses_custom_tool_description, responses_tool_name,
    CUSTOM_TOOL_INPUT_DESCRIPTION, CUSTOM_TOOL_INPUT_FIELD,
};
use crate::proxy::sse::{strip_sse_field, SseBuffer};

/// Names of the custom tools a Codex request declares: top-level `tools`,
/// namespace children, and `additional_tools` / `tool_search_output` carriers in
/// `input`. Call on the original request, before [`convert_custom_tools_to_functions`].
pub(crate) fn custom_tool_names(body: &Value) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut visit = |tool: &Value| {
        if is_custom_tool(tool) {
            if let Some(name) = custom_tool_name(tool) {
                names.insert(name);
            }
        }
    };
    for_each_declared_tool(body, &mut visit);
    names
}

/// Rewrite custom tool declarations and history so a function-only Responses
/// gateway accepts the request. Returns whether anything changed.
pub(crate) fn convert_custom_tools_to_functions(body: &mut Value) -> bool {
    let mut changed = false;
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        changed |= convert_tool_list(tools);
    }
    if let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) {
        for item in input.iter_mut() {
            changed |= convert_input_item(item);
        }
    }
    changed
}

/// Turn a `function_call` addressed to a converted custom tool back into a
/// `custom_tool_call`. Returns whether the item was rewritten.
pub(crate) fn restore_custom_tool_call_item(item: &mut Value, names: &HashSet<String>) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return false;
    }
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return false;
    };
    if !names.contains(name) {
        return false;
    }
    let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
    let mut restored = Map::new();
    restored.insert("type".into(), json!("custom_tool_call"));
    for key in ["id", "status", "call_id", "name"] {
        if let Some(value) = item.get(key) {
            restored.insert(key.into(), value.clone());
        }
    }
    restored.insert(
        "input".into(),
        json!(custom_tool_input_from_chat_arguments(arguments)),
    );
    *item = Value::Object(restored);
    true
}

/// Restore converted calls in a complete (non-streaming) Responses body.
pub(crate) fn restore_response_custom_tool_calls(
    body: &mut Value,
    names: &HashSet<String>,
) -> bool {
    let mut changed = false;
    if let Some(output) = body.get_mut("output").and_then(Value::as_array_mut) {
        for item in output.iter_mut() {
            changed |= restore_custom_tool_call_item(item, names);
        }
    }
    changed
}

/// SSE wrapper that restores converted calls. Run it after late-argument repair
/// so `output_item.done` already carries the full arguments.
pub(crate) fn create_custom_tool_restore_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    names: HashSet<String>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut sse = SseBuffer::new();
        // Converted call item id -> tool name. Argument events carry only the id,
        // and some upstreams omit `name` on the final `output_item.done`.
        let mut converted_ids: HashMap<String, String> = HashMap::new();
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    sse.push_bytes(&bytes);
                    while let Some(block) = sse.next_block() {
                        if block.trim().is_empty() {
                            continue;
                        }
                        if let Some(out) = rewrite_sse_block(block, &names, &mut converted_ids) {
                            yield Ok(out);
                        }
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    return;
                }
            }
        }

        let tail = sse.take_tail();
        if !tail.trim().is_empty() {
            if let Some(out) = rewrite_sse_block(&tail, &names, &mut converted_ids) {
                yield Ok(out);
            }
        }
    }
}

/// `None` drops the event (argument deltas of a converted call).
fn rewrite_sse_block(
    block: &str,
    names: &HashSet<String>,
    converted_ids: &mut HashMap<String, String>,
) -> Option<Bytes> {
    let passthrough = || Some(Bytes::from(format!("{block}\n\n")));
    let mut event_name: Option<&str> = None;
    let mut data_parts: Vec<&str> = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.trim());
        }
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data);
        }
    }
    if data_parts.is_empty() {
        return passthrough();
    }
    let data = data_parts.join("\n");
    let Ok(mut event) = serde_json::from_str::<Value>(&data) else {
        return passthrough();
    };

    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
    if event_type.starts_with("response.function_call_arguments.") {
        let converted = event
            .get("item_id")
            .and_then(Value::as_str)
            .is_some_and(|id| converted_ids.contains_key(id));
        return if converted { None } else { passthrough() };
    }

    let changed = match event_type {
        "response.output_item.added" | "response.output_item.done" => {
            let Some(item) = event.get_mut("item") else {
                return passthrough();
            };
            let id = item.get("id").and_then(Value::as_str).map(str::to_string);
            if item.get("name").is_none() {
                if let Some(name) = id.as_ref().and_then(|id| converted_ids.get(id)) {
                    item["name"] = json!(name);
                }
            }
            let restored = restore_custom_tool_call_item(item, names);
            if let (true, Some(id)) = (restored, id) {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                converted_ids.insert(id, name.to_string());
            }
            restored
        }
        _ => match event.get_mut("response") {
            Some(response) => restore_response_custom_tool_calls(response, names),
            None => false,
        },
    };
    if !changed {
        return passthrough();
    }

    let rewritten = serde_json::to_string(&event).unwrap_or(data);
    let mut out = String::new();
    if let Some(name) = event_name {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(&rewritten);
    out.push_str("\n\n");
    Some(Bytes::from(out))
}

fn is_custom_tool(tool: &Value) -> bool {
    // The Chat bridge treats bare-string tool entries as custom tools too.
    tool.is_string() || tool.get("type").and_then(Value::as_str) == Some("custom")
}

fn custom_tool_name(tool: &Value) -> Option<String> {
    match tool {
        Value::String(name) => Some(name.trim().to_string()).filter(|n| !n.is_empty()),
        _ => responses_tool_name(tool),
    }
}

fn custom_tool_as_function(tool: &Value) -> Option<Value> {
    let name = custom_tool_name(tool)?;
    let definition = match tool {
        Value::String(_) => json!({ "type": "custom", "name": name }),
        _ => tool.clone(),
    };
    Some(json!({
        "type": "function",
        "name": name,
        "description": responses_custom_tool_description(&definition),
        "parameters": {
            "type": "object",
            "properties": {
                CUSTOM_TOOL_INPUT_FIELD: {
                    "type": "string",
                    "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                }
            },
            "required": [CUSTOM_TOOL_INPUT_FIELD]
        },
        "strict": false
    }))
}

fn convert_tool_list(tools: &mut [Value]) -> bool {
    let mut changed = false;
    for tool in tools.iter_mut() {
        if is_custom_tool(tool) {
            if let Some(function) = custom_tool_as_function(tool) {
                *tool = function;
                changed = true;
            }
            continue;
        }
        if tool.get("type").and_then(Value::as_str) == Some("namespace") {
            if let Some(children) = tool.get_mut("tools").and_then(Value::as_array_mut) {
                changed |= convert_tool_list(children);
            }
        }
    }
    changed
}

fn convert_input_item(item: &mut Value) -> bool {
    match item.get("type").and_then(Value::as_str) {
        Some("custom_tool_call") => {
            let input = item.get("input").and_then(Value::as_str).unwrap_or("");
            let arguments = json!({ CUSTOM_TOOL_INPUT_FIELD: input }).to_string();
            let mut call = Map::new();
            call.insert("type".into(), json!("function_call"));
            for key in ["call_id", "name"] {
                if let Some(value) = item.get(key) {
                    call.insert(key.into(), value.clone());
                }
            }
            call.insert("arguments".into(), json!(arguments));
            *item = Value::Object(call);
            true
        }
        Some("custom_tool_call_output") => {
            let mut output = Map::new();
            output.insert("type".into(), json!("function_call_output"));
            for key in ["call_id", "output"] {
                if let Some(value) = item.get(key) {
                    output.insert(key.into(), value.clone());
                }
            }
            *item = Value::Object(output);
            true
        }
        Some("additional_tools" | "tool_search_output") => item
            .get_mut("tools")
            .and_then(Value::as_array_mut)
            .is_some_and(|tools| convert_tool_list(tools)),
        _ => false,
    }
}

fn for_each_declared_tool(body: &Value, visit: &mut impl FnMut(&Value)) {
    fn walk(tools: &[Value], visit: &mut impl FnMut(&Value)) {
        for tool in tools {
            visit(tool);
            if tool.get("type").and_then(Value::as_str) == Some("namespace") {
                if let Some(children) = tool.get("tools").and_then(Value::as_array) {
                    walk(children, visit);
                }
            }
        }
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        walk(tools, visit);
    }
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if matches!(
            item.get("type").and_then(Value::as_str),
            Some("additional_tools" | "tool_search_output")
        ) {
            if let Some(tools) = item.get("tools").and_then(Value::as_array) {
                walk(tools, visit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "*** Begin Patch\n*** Add File: a.txt\n+hi\n*** End Patch\n";

    fn request() -> Value {
        json!({
            "model": "deepseek/deepseek-v4.1-flash",
            "tools": [
                { "type": "function", "name": "exec_command", "parameters": { "type": "object" } },
                {
                    "type": "custom",
                    "name": "apply_patch",
                    "description": "Edit files.",
                    "format": { "type": "grammar", "syntax": "lark", "definition": "start: patch" }
                },
                {
                    "type": "namespace",
                    "name": "code",
                    "tools": [{ "type": "custom", "name": "exec", "description": "Run JS." }]
                }
            ],
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "go" }] },
                {
                    "type": "additional_tools",
                    "tools": [{ "type": "custom", "name": "late_tool" }]
                },
                { "type": "custom_tool_call", "id": "ctc_1", "status": "completed", "call_id": "call_1", "name": "apply_patch", "input": PATCH },
                { "type": "custom_tool_call_output", "id": "ctco_1", "call_id": "call_1", "output": "Success." }
            ]
        })
    }

    #[test]
    fn collects_names_from_every_declaration_site() {
        let names = custom_tool_names(&request());
        let expected: HashSet<String> = ["apply_patch", "exec", "late_tool"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn converts_declarations_and_history() {
        let mut body = request();
        assert!(convert_custom_tools_to_functions(&mut body));

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "exec_command");
        let patch = &tools[1];
        assert_eq!(patch["type"], "function");
        assert_eq!(patch["name"], "apply_patch");
        assert_eq!(patch["parameters"]["required"], json!(["input"]));
        // The original definition (including the grammar) stays visible to the model.
        assert!(patch["description"]
            .as_str()
            .unwrap()
            .contains("start: patch"));
        assert_eq!(tools[2]["tools"][0]["type"], "function");
        assert_eq!(body["input"][1]["tools"][0]["type"], "function");

        let call = &body["input"][2];
        assert_eq!(
            call,
            &json!({
                "type": "function_call",
                "call_id": "call_1",
                "name": "apply_patch",
                "arguments": json!({ "input": PATCH }).to_string()
            })
        );
        assert_eq!(
            body["input"][3],
            json!({ "type": "function_call_output", "call_id": "call_1", "output": "Success." })
        );
        assert!(custom_tool_names(&body).is_empty());
        assert!(body["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| !item["type"].as_str().unwrap().starts_with("custom")));
    }

    #[test]
    fn leaves_function_only_requests_untouched() {
        let mut body = json!({
            "tools": [{ "type": "function", "name": "exec_command" }],
            "input": [{ "type": "function_call", "call_id": "c", "name": "exec_command", "arguments": "{}" }]
        });
        let before = body.clone();
        assert!(!convert_custom_tools_to_functions(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn restores_calls_and_falls_back_to_raw_arguments() {
        let names: HashSet<String> = ["apply_patch".to_string()].into();
        let mut item = json!({
            "type": "function_call", "id": "fc_1", "status": "completed",
            "call_id": "call_1", "name": "apply_patch",
            "arguments": json!({ "input": PATCH }).to_string()
        });
        assert!(restore_custom_tool_call_item(&mut item, &names));
        assert_eq!(
            item,
            json!({
                "type": "custom_tool_call", "id": "fc_1", "status": "completed",
                "call_id": "call_1", "name": "apply_patch", "input": PATCH
            })
        );

        let mut raw = json!({ "type": "function_call", "call_id": "c", "name": "apply_patch", "arguments": "not json" });
        assert!(restore_custom_tool_call_item(&mut raw, &names));
        assert_eq!(raw["input"], "not json");

        let mut other = json!({ "type": "function_call", "call_id": "c", "name": "exec_command", "arguments": "{}" });
        assert!(!restore_custom_tool_call_item(&mut other, &names));
    }

    fn sse(events: &[Value]) -> Vec<Result<Bytes, std::io::Error>> {
        events
            .iter()
            .map(|e| {
                Ok(Bytes::from(format!(
                    "event: {}\ndata: {}\n\n",
                    e["type"].as_str().unwrap(),
                    e
                )))
            })
            .collect()
    }

    async fn collect(events: &[Value], names: &[&str]) -> Vec<Value> {
        let names = names.iter().map(|n| n.to_string()).collect();
        let stream =
            create_custom_tool_restore_sse_stream(futures::stream::iter(sse(events)), names);
        let chunks: Vec<_> = stream.collect().await;
        let text: String = chunks
            .into_iter()
            .map(|c| String::from_utf8(c.unwrap().to_vec()).unwrap())
            .collect();
        text.split("\n\n")
            .filter_map(|block| block.lines().find_map(|l| l.strip_prefix("data: ")))
            .map(|data| serde_json::from_str(data).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn stream_restores_custom_calls_and_drops_their_argument_events() {
        let args = json!({ "input": "text('ok')" }).to_string();
        let call = json!({ "id": "fc_9", "type": "function_call", "call_id": "call_9", "name": "exec", "arguments": args, "status": "completed" });
        let mut nameless = call.clone();
        nameless.as_object_mut().unwrap().remove("name");
        let events = [
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": { "id": "fc_9", "type": "function_call", "call_id": "call_9", "name": "exec", "arguments": "", "status": "in_progress" } }),
            json!({ "type": "response.function_call_arguments.delta", "item_id": "fc_9", "output_index": 0, "delta": args }),
            json!({ "type": "response.function_call_arguments.done", "item_id": "fc_9", "output_index": 0, "arguments": args }),
            // Some upstreams omit `name` on later events; the id recorded at `added` still matches.
            json!({ "type": "response.output_item.done", "output_index": 0, "item": nameless }),
            json!({ "type": "response.function_call_arguments.delta", "item_id": "fc_other", "delta": "{}" }),
            json!({ "type": "response.completed", "response": { "id": "r", "output": [call] } }),
        ];
        let out = collect(&events, &["exec"]).await;
        let types: Vec<&str> = out.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(
            types,
            [
                "response.output_item.added",
                "response.output_item.done",
                "response.function_call_arguments.delta",
                "response.completed"
            ]
        );
        assert_eq!(out[0]["item"]["type"], "custom_tool_call");
        assert_eq!(out[1]["item"]["type"], "custom_tool_call");
        assert_eq!(out[1]["item"]["input"], "text('ok')");
        assert_eq!(out[1]["item"]["name"], "exec");
        assert_eq!(out[2]["item_id"], "fc_other");
        assert_eq!(out[3]["response"]["output"][0]["type"], "custom_tool_call");
    }

    #[tokio::test]
    async fn stream_passes_unrelated_events_through_unchanged() {
        let events = [json!({ "type": "response.output_text.delta", "delta": "hi" })];
        let names = HashSet::from(["exec".to_string()]);
        let stream =
            create_custom_tool_restore_sse_stream(futures::stream::iter(sse(&events)), names);
        let chunks: Vec<_> = stream.collect().await;
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].as_ref().unwrap().as_ref(),
            format!("event: response.output_text.delta\ndata: {}\n\n", events[0]).as_bytes()
        );
    }
}
