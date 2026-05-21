use std::path::Path;

use serde_json::{Map, Value};

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, render_json_value, render_tool_call, truncate_summary};

const PROVIDER_ID: &str = "gemini";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Iterate over project directories: tmp/<project_name>/chats/session-*.json
    let project_dirs = match std::fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in project_dirs.flatten() {
        let chats_dir = entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let chat_files = match std::fs::read_dir(&chats_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let project_root_file = entry.path().join(".project_root");
        let project_dir = std::fs::read_to_string(project_root_file).ok();

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(meta) = parse_session(&path) {
                sessions.push(SessionMeta {
                    project_dir: project_dir.clone(),
                    ..meta
                });
            }
        }
    }

    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    let value: Value =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse session JSON: {e}"))?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;

    let mut result = Vec::new();
    for msg in messages {
        let role = match msg.get("type").and_then(Value::as_str) {
            Some("gemini") => "assistant",
            Some("user") => "user",
            Some("info") | Some("error") => continue,
            Some(_) | None => continue,
        };

        // Gemini content may be a plain string or an array of {text: ...} objects
        let mut content = match msg.get("content") {
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };

        // Append tool calls from the optional toolCalls array, including args so
        // history/search/export do not lose what was invoked.
        if let Some(Value::Array(calls)) = msg.get("toolCalls") {
            for call in calls {
                if let Some(name) = call.get("name").and_then(Value::as_str) {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    let args = call.get("args").or_else(|| call.get("arguments"));
                    let call_id = call.get("id").and_then(Value::as_str);
                    content.push_str(&render_tool_call(name, args, call_id));
                    if let Some(result) = call.get("result") {
                        if let Some(rendered_result) = render_gemini_tool_result(result) {
                            content.push('\n');
                            content.push_str(&rendered_result);
                        }
                    }
                }
            }
        }

        if content.trim().is_empty() {
            continue;
        }

        let ts = msg.get("timestamp").and_then(parse_timestamp_to_ms);

        result.push(SessionMessage {
            role: role.to_string(),
            content,
            ts,
        });
    }

    Ok(result)
}

fn render_gemini_tool_result(result: &Value) -> Option<String> {
    let mut lines = Vec::new();
    collect_gemini_tool_result(result, &mut lines);

    if !lines.is_empty() {
        return Some(format!("Result:\n{}", lines.join("\n")));
    }

    let sanitized = sanitize_gemini_result_value(result);
    render_json_value(&sanitized).map(|rendered| format!("Result:\n{rendered}"))
}

fn collect_gemini_tool_result(value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_gemini_tool_result(item, lines);
            }
        }
        Value::Object(obj) => {
            if let Some(function_response) = obj.get("functionResponse") {
                collect_gemini_function_response(function_response, lines);
                return;
            }

            if let Some(output) = obj
                .get("response")
                .and_then(|response| response.get("output"))
            {
                let sanitized = sanitize_gemini_result_value(output);
                if let Some(rendered) = render_json_value(&sanitized) {
                    lines.push(format!("Output:\n{rendered}"));
                }
            } else if let Some(output) = obj.get("output") {
                let sanitized = sanitize_gemini_result_value(output);
                if let Some(rendered) = render_json_value(&sanitized) {
                    lines.push(format!("Output:\n{rendered}"));
                }
            }

            append_gemini_part_summaries(obj.get("parts"), lines);
        }
        Value::String(text) if !text.trim().is_empty() => {
            lines.push(format!("Output:\n{text}"));
        }
        _ => {}
    }
}

fn collect_gemini_function_response(value: &Value, lines: &mut Vec<String>) {
    let Some(obj) = value.as_object() else {
        collect_gemini_tool_result(value, lines);
        return;
    };

    let before_len = lines.len();
    if let Some(output) = obj
        .get("response")
        .and_then(|response| response.get("output"))
    {
        let sanitized = sanitize_gemini_result_value(output);
        if let Some(rendered) = render_json_value(&sanitized) {
            lines.push(format!("Output:\n{rendered}"));
        }
    } else if let Some(response) = obj.get("response") {
        let sanitized = sanitize_gemini_result_value(response);
        if let Some(rendered) = render_json_value(&sanitized) {
            lines.push(format!("Response:\n{rendered}"));
        }
    }

    append_gemini_part_summaries(obj.get("parts"), lines);

    if lines.len() == before_len {
        let sanitized = sanitize_gemini_result_value(value);
        if let Some(rendered) = render_json_value(&sanitized) {
            lines.push(rendered);
        }
    }
}

fn append_gemini_part_summaries(parts: Option<&Value>, lines: &mut Vec<String>) {
    let Some(Value::Array(parts)) = parts else {
        return;
    };

    let mut summaries = Vec::new();
    for part in parts {
        if let Some(summary) = summarize_gemini_part(part) {
            summaries.push(summary);
        }
    }

    if !summaries.is_empty() {
        lines.push(format!("Attachments:\n{}", summaries.join("\n")));
    }
}

fn summarize_gemini_part(part: &Value) -> Option<String> {
    let inline_data = part.get("inlineData").or_else(|| part.get("inline_data"));
    if let Some(inline_data) = inline_data.and_then(Value::as_object) {
        let mime = inline_data
            .get("mimeType")
            .or_else(|| inline_data.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("inline data");
        let byte_count = inline_data
            .get("data")
            .and_then(Value::as_str)
            .map(estimated_base64_bytes)
            .unwrap_or(0);
        if byte_count > 0 {
            return Some(format!(
                "- inlineData: {mime}, ~{byte_count} bytes (base64 omitted)"
            ));
        }
        return Some(format!("- inlineData: {mime} (base64 omitted)"));
    }

    if let Some(file_data) = part.get("fileData").or_else(|| part.get("file_data")) {
        let sanitized = sanitize_gemini_result_value(file_data);
        return render_json_value(&sanitized).map(|rendered| format!("- fileData: {rendered}"));
    }

    None
}

fn sanitize_gemini_result_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(sanitize_gemini_result_value).collect())
        }
        Value::Object(obj) => {
            let mut sanitized = Map::new();
            for (key, value) in obj {
                if key == "data" {
                    if let Some(data) = value.as_str() {
                        if data.len() > 512 {
                            sanitized.insert(
                                key.clone(),
                                Value::String(format!(
                                    "<base64 omitted: ~{} bytes>",
                                    estimated_base64_bytes(data)
                                )),
                            );
                            continue;
                        }
                    }
                }
                sanitized.insert(key.clone(), sanitize_gemini_result_value(value));
            }
            Value::Object(sanitized)
        }
        _ => value.clone(),
    }
}

fn estimated_base64_bytes(data: &str) -> usize {
    let trimmed = data.trim_end_matches('=');
    trimmed.len() * 3 / 4
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Gemini session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Gemini session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Gemini session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("sessionId").and_then(Value::as_str)?.to_string();

    let created_at = value.get("startTime").and_then(parse_timestamp_to_ms);
    let last_active_at = value.get("lastUpdated").and_then(parse_timestamp_to_ms);

    // Derive title from first user message
    let title = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|msgs| {
            msgs.iter()
                .find(|m| m.get("type").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("content").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate_summary(s, 160))
        });

    let source_path = path.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None, // (optionally) populated later
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(source_path),
        resume_command: Some(format!("gemini --resume {session_id}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_json_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-03-06T10-17-test.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "gemini-session-123",
              "startTime": "2026-03-06T10:17:58.000Z",
              "lastUpdated": "2026-03-06T10:20:00.000Z",
              "messages": [
                {
                  "id": "msg-1",
                  "timestamp": "2026-03-06T10:17:58.000Z",
                  "type": "user",
                  "content": "hello"
                }
              ]
            }"#,
        )
        .expect("write session");

        delete_session(temp.path(), &path, "gemini-session-123").expect("delete session");

        assert!(!path.exists());
    }

    #[test]
    fn load_messages_handles_array_content() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-06T10:00:00Z","type":"user","content":[{"text":"hello"}]},
                {"id":"2","timestamp":"2026-03-06T10:00:01Z","type":"gemini","content":"world"},
                {"id":"3","timestamp":"2026-03-06T10:00:02Z","type":"info","content":"system info"},
                {"id":"4","timestamp":"2026-03-06T10:00:03Z","type":"error","content":"MCP ERROR"}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "world");
    }

    #[test]
    fn load_messages_includes_tool_calls() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"","toolCalls":[{"id":"call_1","name":"web_search","args":{"query":"test"},"result":[{"functionResponse":{"id":"call_1","name":"web_search","response":{"output":"result text"}}}]}]},
                {"id":"2","timestamp":"2026-03-10T08:25:00Z","type":"gemini","content":"Here are the results.","toolCalls":[{"id":"call_2","name":"web_fetch","args":{"url":"http://example.com"},"result":[{"functionResponse":{"id":"call_2","name":"web_fetch","response":{"output":"Binary content provided (1 item(s))."},"parts":[{"inlineData":{"mimeType":"application/pdf","data":"QUJDRA=="}}]}}]}]}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: web_search]"));
        assert!(msgs[0].content.contains("Call ID: call_1"));
        assert!(msgs[0].content.contains("Query: test"));
        assert!(msgs[0].content.contains("Result:\nOutput:\nresult text"));
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("Here are the results."));
        assert!(msgs[1].content.contains("[Tool: web_fetch]"));
        assert!(msgs[1].content.contains("URL: http://example.com"));
        assert!(msgs[1].content.contains("Output:\nBinary content provided"));
        assert!(msgs[1]
            .content
            .contains("inlineData: application/pdf, ~4 bytes"));
        assert!(!msgs[1].content.contains("QUJDRA=="));
    }

    #[test]
    fn load_messages_sanitizes_function_response_output_data() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        let large_data = "QUJD".repeat(200);
        let session = format!(
            r#"{{
              "sessionId": "test",
              "messages": [
                {{"id":"1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"","toolCalls":[{{"id":"call_1","name":"read_image","args":{{"path":"image.png"}},"result":[{{"functionResponse":{{"id":"call_1","name":"read_image","response":{{"output":{{"mimeType":"image/png","data":"{large_data}"}}}}}}}}]}}]}}
              ]
            }}"#
        );
        std::fs::write(&path, session).expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content.contains("Output:"));
        assert!(msgs[0].content.contains("<base64 omitted: ~600 bytes>"));
        assert!(!msgs[0].content.contains(&large_data));
    }
}
