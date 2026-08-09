//! OpenCode Go compatibility for native OpenAI Responses passthrough.
//!
//! OpenCode Go accepts and emits the Responses protocol, but its hosted web
//! search history uses `action.queries` instead of the standard singular
//! `action.query`. Its SSE stream also includes provider keepalive events and
//! may omit the action from an in-progress `web_search_call`. Strict Responses
//! clients reject those shapes before they can consume the model output.
//!
//! Provider keepalives (`event: ping`) are not forwarded to the client — strict
//! clients fail to deserialize them — but they are also not silently dropped:
//! the rectifier replaces each keepalive with a spec-compliant SSE comment line
//! (`: keepalive`) so the proxy's streaming timeout layer still observes
//! upstream activity while clients never see a non-Responses event.
//!
//! Keep this compatibility pass narrowly gated to Grok Build requests using
//! the official OpenCode Go origin. Other clients and Responses providers
//! retain byte-for-byte passthrough.

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use super::{CodexAdapter, ProviderAdapter};
use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

const OPENCODE_HOST: &str = "opencode.ai";
const OPENCODE_GO_PATH: &str = "/zen/go";
const RESPONSE_EXTENSION_FIELDS: &[&str] = &["cost", "moderation"];

/// Whether this request belongs to the narrowly scoped Grok Build compatibility
/// path for the official OpenCode Go Responses endpoint.
pub(crate) fn should_rectify_opencode_go_responses(
    app_type: &AppType,
    provider: &Provider,
) -> bool {
    if !matches!(app_type, AppType::GrokBuild) {
        return false;
    }

    let Ok(base_url) = CodexAdapter::new().extract_base_url(provider) else {
        return false;
    };
    is_opencode_go_base_url(&base_url)
}

fn is_opencode_go_base_url(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    if !url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(OPENCODE_HOST))
    {
        return false;
    }

    let path = url.path().trim_end_matches('/');
    path == OPENCODE_GO_PATH
        || path
            .strip_prefix(OPENCODE_GO_PATH)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Convert standard replayed web-search history to OpenCode Go's accepted
/// request shape. Tool declarations remain standard `{ "type": "web_search" }`.
pub(crate) fn rectify_opencode_go_responses_request(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut changed = false;
    for item in input {
        if item.get("type").and_then(Value::as_str) != Some("web_search_call") {
            continue;
        }
        let Some(action) = item.get_mut("action").and_then(Value::as_object_mut) else {
            continue;
        };
        if action.get("type").and_then(Value::as_str) != Some("search") {
            continue;
        }
        let Some(query) = action.get("query").and_then(Value::as_str) else {
            continue;
        };
        let query = query.to_string();

        if !action.get("queries").is_some_and(Value::is_array) {
            action.insert("queries".to_string(), json!([query]));
        }
        action.remove("query");
        changed = true;
    }
    changed
}

/// Normalize a complete OpenCode Go Responses object for strict clients.
pub(crate) fn rectify_opencode_go_response(response: &mut Value) -> bool {
    let Some(response) = response.as_object_mut() else {
        return false;
    };

    let mut changed = remove_response_extensions(response);
    if let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) {
        for item in output {
            changed |= normalize_output_item(item);
        }
    }
    changed
}

fn rectify_opencode_go_sse_event(event: &mut Value) -> bool {
    let Some(event) = event.as_object_mut() else {
        return false;
    };

    let mut changed = remove_response_extensions(event);
    if let Some(response) = event.get_mut("response") {
        changed |= rectify_opencode_go_response(response);
    }
    if let Some(item) = event.get_mut("item") {
        changed |= normalize_output_item(item);
    }
    changed
}

fn remove_response_extensions(object: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    for field in RESPONSE_EXTENSION_FIELDS {
        changed |= object.remove(*field).is_some();
    }
    changed
}

fn normalize_output_item(item: &mut Value) -> bool {
    let Some(item) = item.as_object_mut() else {
        return false;
    };

    match item.get("type").and_then(Value::as_str) {
        Some("message") => item.remove("phase").is_some(),
        Some("web_search_call") => normalize_web_search_call(item),
        _ => false,
    }
}

fn normalize_web_search_call(item: &mut serde_json::Map<String, Value>) -> bool {
    let needs_action = match item.get("action") {
        None | Some(Value::Null) => true,
        Some(Value::Object(action)) => action.is_empty(),
        Some(_) => return false,
    };

    let mut changed = false;
    if needs_action {
        item.insert("action".to_string(), json!({"type": "search", "query": ""}));
        changed = true;
    }

    let Some(action) = item.get_mut("action").and_then(Value::as_object_mut) else {
        return changed;
    };

    if action.get("type").and_then(Value::as_str).is_none()
        && action.get("queries").is_some_and(Value::is_array)
    {
        action.insert("type".to_string(), json!("search"));
        changed = true;
    }
    if action.get("type").and_then(Value::as_str) != Some("search") {
        return changed;
    }

    if !action.get("query").is_some_and(Value::is_string) {
        let query = preferred_search_query(action.get("queries")).unwrap_or_default();
        action.insert("query".to_string(), Value::String(query));
        changed = true;
    }
    changed |= action.remove("queries").is_some();
    changed
}

fn preferred_search_query(queries: Option<&Value>) -> Option<String> {
    let queries = queries?.as_array()?;
    let nonempty = || {
        queries
            .iter()
            .filter_map(Value::as_str)
            .filter(|query| !query.trim().is_empty())
    };

    nonempty()
        .find(|query| !query.trim_start().starts_with("ws_call_id="))
        .or_else(|| nonempty().next())
        .map(ToString::to_string)
}

/// Wrap an OpenCode Go Responses SSE stream, replacing provider keepalives
/// with spec-compliant SSE comment lines and normalizing each JSON event
/// before it reaches the client.
pub(crate) fn create_opencode_go_responses_rectifier_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        if let Some(rectified) = rectify_sse_block(&block) {
                            yield Ok(rectified);
                        }
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            if let Some(rectified) = rectify_sse_block(&tail) {
                yield Ok(rectified);
            }
        }
    }
}

fn rectify_sse_block(block: &str) -> Option<Bytes> {
    let mut event_name = None;
    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.trim());
        }
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data);
        }
    }

    if event_name == Some("ping") {
        // Keepalives must not reach strict clients, but they must still count
        // as upstream activity for the streaming timeout layer. A comment line
        // is valid SSE that the client's parser ignores.
        return Some(Bytes::from(": keepalive\n\n"));
    }
    if data_parts.is_empty() {
        return Some(Bytes::from(format!("{block}\n\n")));
    }

    let data = data_parts.join("\n");
    if data.trim() == "[DONE]" {
        return Some(Bytes::from(format!("{block}\n\n")));
    }

    let mut event: Value = match serde_json::from_str(&data) {
        Ok(event) => event,
        Err(_) => return Some(Bytes::from(format!("{block}\n\n"))),
    };
    if event.get("type").and_then(Value::as_str) == Some("ping") {
        // Same keepalive handling as the named `event: ping` branch above.
        return Some(Bytes::from(": keepalive\n\n"));
    }
    if !rectify_opencode_go_sse_event(&mut event) {
        return Some(Bytes::from(format!("{block}\n\n")));
    }

    let rectified = serde_json::to_string(&event).unwrap_or(data);
    Some(replace_sse_data(block, &rectified))
}

fn replace_sse_data(block: &str, data: &str) -> Bytes {
    let mut output = String::new();
    let mut replaced = false;
    for line in block.lines() {
        if strip_sse_field(line, "data").is_some() {
            if !replaced {
                output.push_str("data: ");
                output.push_str(data);
                output.push('\n');
                replaced = true;
            }
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    Bytes::from(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    fn provider_with_base_url(base_url: &str) -> Provider {
        Provider::with_id(
            "provider-1".to_string(),
            "Provider".to_string(),
            json!({"base_url": base_url}),
            None,
        )
    }

    #[test]
    fn rectifier_gate_requires_grokbuild_and_official_opencode_go_path() {
        assert!(should_rectify_opencode_go_responses(
            &AppType::GrokBuild,
            &provider_with_base_url("https://opencode.ai/zen/go/v1")
        ));
        assert!(should_rectify_opencode_go_responses(
            &AppType::GrokBuild,
            &provider_with_base_url("https://OPENCODE.AI/zen/go/v1/responses")
        ));
        assert!(!should_rectify_opencode_go_responses(
            &AppType::Codex,
            &provider_with_base_url("https://opencode.ai/zen/go/v1")
        ));
        assert!(!should_rectify_opencode_go_responses(
            &AppType::GrokBuild,
            &provider_with_base_url("https://opencode.ai/v1")
        ));
        assert!(!should_rectify_opencode_go_responses(
            &AppType::GrokBuild,
            &provider_with_base_url("https://opencode.ai.attacker.example/zen/go/v1")
        ));
    }

    #[test]
    fn request_rectifier_converts_only_replayed_search_actions() {
        let mut body = json!({
            "tools": [{"type": "web_search"}],
            "input": [
                {
                    "type": "web_search_call",
                    "id": "ws_1",
                    "action": {"type": "search", "query": "OpenCode Go"}
                },
                {
                    "type": "web_search_call",
                    "id": "ws_2",
                    "action": {"type": "open_page", "query": "unchanged"}
                },
                {
                    "type": "function_call",
                    "arguments": {"query": "unchanged"}
                }
            ]
        });

        assert!(rectify_opencode_go_responses_request(&mut body));
        assert_eq!(body["tools"], json!([{"type": "web_search"}]));
        assert_eq!(
            body["input"][0]["action"],
            json!({"type": "search", "queries": ["OpenCode Go"]})
        );
        assert_eq!(body["input"][1]["action"]["query"], "unchanged");
        assert_eq!(body["input"][2]["arguments"]["query"], "unchanged");
        assert!(!rectify_opencode_go_responses_request(&mut body));
    }

    #[test]
    fn response_rectifier_normalizes_provider_extensions_and_search_calls() {
        let mut response = json!({
            "id": "resp_1",
            "cost": "0",
            "moderation": null,
            "output": [
                {
                    "id": "ws_pending",
                    "type": "web_search_call",
                    "status": "in_progress",
                    "action": null
                },
                {
                    "id": "ws_done",
                    "type": "web_search_call",
                    "status": "completed",
                    "action": {
                        "type": "search",
                        "queries": [
                            "opencode.ai",
                            "site:opencode.ai",
                            "ws_call_id=ws_done"
                        ]
                    }
                },
                {
                    "id": "msg_1",
                    "type": "message",
                    "phase": "final_answer",
                    "content": []
                }
            ]
        });

        assert!(rectify_opencode_go_response(&mut response));
        assert!(response.get("cost").is_none());
        assert!(response.get("moderation").is_none());
        assert_eq!(
            response["output"][0]["action"],
            json!({"type": "search", "query": ""})
        );
        assert_eq!(response["output"][1]["action"]["query"], "opencode.ai");
        assert!(response["output"][1]["action"].get("queries").is_none());
        assert!(response["output"][2].get("phase").is_none());
        assert!(!rectify_opencode_go_response(&mut response));
    }

    #[tokio::test]
    async fn sse_rectifier_replaces_ping_keepalives_and_handles_split_events() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from(concat!(
                "event: ping\n",
                "data: {\"type\":\"ping\",\"cost\":\"0\"}\n\n",
                "event: response.output_item.added\n",
                "id: event-1\n",
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"ws_1\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n"
            ))),
            Ok(Bytes::from(concat!(
                "\n",
                "event: response.output_item.done\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"ws_1\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"queries\":[\"Rust docs\",\"ws_call_id=ws_1\"]}}}\n\n"
            ))),
        ];

        let output = create_opencode_go_responses_rectifier_stream(stream::iter(chunks));
        tokio::pin!(output);
        let mut merged = Vec::new();
        while let Some(chunk) = output.next().await {
            merged.extend_from_slice(&chunk.unwrap());
        }
        let merged = String::from_utf8(merged).unwrap();

        // Ping keepalives never reach the client...
        assert!(!merged.contains("event: ping"));
        assert!(!merged.contains("\"type\":\"ping\""));
        // ...but are replaced by a comment line so the streaming timeout
        // layer still observes upstream activity.
        assert!(merged.contains(": keepalive"));
        assert!(merged.contains("id: event-1"));
        assert!(merged.contains("\"query\":\"\""));
        assert!(merged.contains("\"query\":\"Rust docs\""));
        assert!(!merged.contains("\"queries\""));
    }
}
