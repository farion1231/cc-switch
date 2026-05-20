use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde_json::{Map, Value};

/// Maximum number of characters for session titles (shared across providers).
pub const TITLE_MAX_CHARS: usize = 80;

/// Read the first `head_n` lines and last `tail_n` lines from a file.
/// For small files (< 16 KB), reads all lines once to avoid unnecessary seeking.
pub fn read_head_tail_lines(
    path: &Path,
    head_n: usize,
    tail_n: usize,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();

    // For small files, read all lines once and split
    if file_len < 16_384 {
        let reader = BufReader::new(file);
        let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let head = all.iter().take(head_n).cloned().collect();
        let skip = all.len().saturating_sub(tail_n);
        let tail = all.into_iter().skip(skip).collect();
        return Ok((head, tail));
    }

    // Read head lines from the beginning
    let reader = BufReader::new(file);
    let head: Vec<String> = reader.lines().take(head_n).map_while(Result::ok).collect();

    // Seek to last ~16 KB for tail lines
    let seek_pos = file_len.saturating_sub(16_384);
    let mut file2 = File::open(path)?;
    file2.seek(SeekFrom::Start(seek_pos))?;
    let tail_reader = BufReader::new(file2);
    let all_tail: Vec<String> = tail_reader.lines().map_while(Result::ok).collect();

    // Skip first partial line if we seeked into the middle of a line
    let skip_first = if seek_pos > 0 { 1 } else { 0 };
    let usable: Vec<String> = all_tail.into_iter().skip(skip_first).collect();
    let skip = usable.len().saturating_sub(tail_n);
    let tail = usable.into_iter().skip(skip).collect();

    Ok((head, tail))
}

pub fn parse_timestamp_to_ms(value: &Value) -> Option<i64> {
    // Integer: milliseconds (>1e12) or seconds
    if let Some(n) = value.as_i64() {
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    if let Some(n) = value.as_f64() {
        let n = n as i64;
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    // RFC3339 string
    let raw = value.as_str()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt: DateTime<FixedOffset>| dt.timestamp_millis())
}

pub fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .map(|text| text.to_string())
            .or_else(|| render_json_value(content))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");

    // tool_use: show tool name plus input parameters so history/search/export
    // preserve what was actually executed.
    if item_type == "tool_use" {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let call_id = item
            .get("id")
            .or_else(|| item.get("tool_use_id"))
            .and_then(Value::as_str);
        return Some(render_tool_call(name, item.get("input"), call_id));
    }

    // tool_result: extract nested content, but summarize non-text blocks so
    // image/audio payloads do not dump large base64 blobs into history views.
    if item_type == "tool_result" {
        if let Some(content) = item.get("content") {
            return extract_tool_result_content(content);
        }
        return None;
    }

    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("input_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(content) = item.get("content") {
        let text = extract_text(content);
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

fn extract_tool_result_content(content: &Value) -> Option<String> {
    let text = extract_text_without_json_fallback(content);
    if !text.trim().is_empty() {
        return Some(text);
    }

    summarize_tool_result_content(content).or_else(|| render_json_value(content))
}

fn extract_text_without_json_fallback(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => ["text", "input_text", "output_text"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(|text| text.to_string())
            .or_else(|| {
                map.get("content")
                    .map(extract_text_without_json_fallback)
                    .filter(|text| !text.trim().is_empty())
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn summarize_tool_result_content(content: &Value) -> Option<String> {
    match content {
        Value::Array(items) => {
            if items.is_empty() {
                return None;
            }

            let summaries = items
                .iter()
                .filter_map(summarize_tool_result_item)
                .collect::<Vec<_>>();

            if summaries.is_empty() {
                Some("[Tool result: non-text content]".to_string())
            } else {
                Some(summaries.join("\n"))
            }
        }
        Value::Object(_) => summarize_tool_result_item(content),
        _ => None,
    }
}

fn summarize_tool_result_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str)?;

    match item_type {
        "image" => {
            let media_type = item
                .get("source")
                .and_then(|source| source.get("media_type"))
                .or_else(|| item.get("media_type"))
                .and_then(Value::as_str);

            Some(match media_type {
                Some(media_type) if !media_type.trim().is_empty() => {
                    format!("[Image: {media_type}]")
                }
                _ => "[Image]".to_string(),
            })
        }
        "audio" => Some("[Audio]".to_string()),
        "file" => item
            .get("name")
            .or_else(|| item.get("filename"))
            .or_else(|| item.get("path"))
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(|name| format!("[File: {name}]"))
            .or_else(|| Some("[File]".to_string())),
        other if !other.trim().is_empty() => Some(format!("[Tool result: {other}]")),
        _ => None,
    }
}

pub fn render_tool_call(name: &str, input: Option<&Value>, call_id: Option<&str>) -> String {
    let mut lines = vec![format!("[Tool: {name}]")];

    if let Some(call_id) = call_id.filter(|id| !id.trim().is_empty()) {
        lines.push(format!("Call ID: {call_id}"));
    }

    let Some(input) = input else {
        return lines.join("\n");
    };

    match input {
        Value::Object(map) => append_tool_input_object(name, map, &mut lines),
        Value::Null => {}
        _ => {
            if let Some(rendered) = render_json_value(input) {
                lines.push(format!("Input:\n{rendered}"));
            }
        }
    }

    lines.join("\n")
}

fn append_tool_input_object(name: &str, input: &Map<String, Value>, lines: &mut Vec<String>) {
    if input.is_empty() {
        return;
    }

    let lower_name = name.to_ascii_lowercase();
    if lower_name == "todowrite" || lower_name == "todo_write" {
        if let Some(todos) = input.get("todos").and_then(Value::as_array) {
            if !todos.is_empty() {
                for todo in todos {
                    let content = todo
                        .get("content")
                        .or_else(|| todo.get("task"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let status = todo
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    if content.trim().is_empty() {
                        if let Some(rendered) = render_json_value(todo) {
                            lines.push(format!("- {status}: {rendered}"));
                        }
                    } else {
                        lines.push(format!("- {status}: {content}"));
                    }
                }
                append_remaining_input(input, &["todos"], lines);
                return;
            }
        }
    }

    let ordered_keys = [
        "command",
        "description",
        "file_path",
        "filePath",
        "path",
        "pattern",
        "url",
        "query",
        "prompt",
        "old_string",
        "new_string",
        "replace_all",
    ];
    let mut consumed = Vec::new();

    for key in ordered_keys {
        if let Some(value) = input.get(key) {
            if let Some(rendered) = render_json_value(value) {
                lines.push(format!("{}: {rendered}", tool_input_label(key)));
                consumed.push(key);
            }
        }
    }

    append_remaining_input(input, &consumed, lines);
}

fn append_remaining_input(input: &Map<String, Value>, consumed: &[&str], lines: &mut Vec<String>) {
    let mut remaining = Map::new();
    for (key, value) in input {
        if !consumed.iter().any(|consumed_key| consumed_key == key) {
            remaining.insert(key.clone(), value.clone());
        }
    }

    if remaining.is_empty() {
        return;
    }

    if let Some(rendered) = render_json_value(&Value::Object(remaining)) {
        lines.push(format!("Input:\n{rendered}"));
    }
}

fn tool_input_label(key: &str) -> &'static str {
    match key {
        "command" => "Command",
        "description" => "Description",
        "file_path" | "filePath" => "File",
        "path" => "Path",
        "pattern" => "Pattern",
        "url" => "URL",
        "query" => "Query",
        "prompt" => "Prompt",
        "old_string" => "Old",
        "new_string" => "New",
        "replace_all" => "Replace All",
        _ => "Input",
    }
}

pub fn render_json_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => (!text.trim().is_empty()).then(|| text.to_string()),
        Value::Array(items) if items.is_empty() => None,
        Value::Object(map) if map.is_empty() => None,
        _ => serde_json::to_string_pretty(value).ok(),
    }
}

pub fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut result = trimmed.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

pub fn path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches(['/', '\\']);
    let last = normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|segment| !segment.is_empty())?;
    Some(last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_timestamp_to_ms_supports_integers_and_rfc3339() {
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_033_i64)),
            Some(1_771_061_953_033)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_i64)),
            Some(1_771_061_953_000)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!("1970-01-01T00:00:01Z")),
            Some(1_000)
        );
    }

    #[test]
    fn extract_text_renders_tool_use_input() {
        let content = json!([{
            "type": "tool_use",
            "id": "toolu_1",
            "name": "Bash",
            "input": {
                "command": "ls -la",
                "description": "list files"
            }
        }]);

        let text = extract_text(&content);
        assert!(text.contains("[Tool: Bash]"));
        assert!(text.contains("Call ID: toolu_1"));
        assert!(text.contains("Command: ls -la"));
        assert!(text.contains("Description: list files"));
    }

    #[test]
    fn extract_text_summarizes_non_text_tool_result_blocks() {
        let content = json!([{
            "type": "tool_result",
            "tool_use_id": "toolu_1",
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "iVBORw0KGgoAAAANSUhEUgAA"
                }
            }]
        }]);

        let text = extract_text(&content);
        assert_eq!(text, "[Image: image/png]");
        assert!(!text.contains("iVBORw0KGgo"));
        assert!(!text.contains("\"data\""));
    }

    #[test]
    fn extract_text_preserves_structured_tool_result_without_typed_blocks() {
        let content = json!([{
            "type": "tool_result",
            "content": {
                "status": "ok",
                "count": 2
            }
        }]);

        let text = extract_text(&content);
        assert!(text.contains("\"status\": \"ok\""));
        assert!(text.contains("\"count\": 2"));
    }

    #[test]
    fn render_tool_call_preserves_unknown_input_json() {
        let text = render_tool_call(
            "mcp__demo__tool",
            Some(&json!({"custom": {"nested": true}})),
            None,
        );

        assert!(text.contains("[Tool: mcp__demo__tool]"));
        assert!(text.contains("Input:"));
        assert!(text.contains("\"nested\": true"));
    }

    #[test]
    fn render_tool_call_formats_todos() {
        let text = render_tool_call(
            "TodoWrite",
            Some(&json!({
                "todos": [
                    {"status": "pending", "content": "implement backend renderer"},
                    {"status": "completed", "content": "add plan"}
                ]
            })),
            None,
        );

        assert!(text.contains("- pending: implement backend renderer"));
        assert!(text.contains("- completed: add plan"));
    }
}
