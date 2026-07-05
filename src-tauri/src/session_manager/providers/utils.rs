use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

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
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");

    // tool_use: show tool name
    if item_type == "tool_use" {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Some(format!("[Tool: {name}]"));
    }

    // tool_result: extract nested content
    if item_type == "tool_result" {
        if let Some(content) = item.get("content") {
            let text = extract_text(content);
            if !text.is_empty() {
                return Some(text);
            }
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

/// Maximum number of characters in a search snippet (context around a match).
pub const SNIPPET_MAX_CHARS: usize = 160;

/// Build a search snippet around the first occurrence of `needle` in `haystack`.
/// Returns up to `SNIPPET_MAX_CHARS` chars, centered on the match when possible.
/// Case-insensitive matching for ASCII; exact for non-ASCII (CJK etc.).
pub fn build_snippet(haystack: &str, needle: &str) -> Option<String> {
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    let needle_len = needle_chars.len();
    if needle_len == 0 || chars.len() < needle_len {
        return None;
    }
    let lower_needle = needle.to_lowercase();
    // Find the first window (case-insensitive) where needle matches.
    let start = (0..=chars.len().saturating_sub(needle_len)).find(|&i| {
        let window: String = chars[i..i + needle_len].iter().collect();
        window.to_lowercase() == lower_needle
    })?;
    let match_end = (start + needle_len).min(chars.len());
    // Context: try to center the match in a window of SNIPPET_MAX_CHARS.
    let half = SNIPPET_MAX_CHARS.saturating_sub(needle_len) / 2;
    let ctx_start = start.saturating_sub(half);
    let ctx_end = (ctx_start + SNIPPET_MAX_CHARS).min(chars.len());
    let mut snippet: String = chars[ctx_start..ctx_end].iter().collect();
    snippet = snippet.trim().to_string();
    if ctx_start > 0 {
        snippet.insert(0, '…');
    }
    if ctx_end < chars.len() {
        snippet.push('…');
    }
    // Sanity: never return a snippet that doesn't actually contain the needle.
    if !snippet.to_lowercase().contains(&lower_needle) {
        return None;
    }
    let _ = match_end; // kept for potential future use (exact match bounds)
    Some(snippet)
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
    fn build_snippet_finds_ascii_substring_case_insensitively() {
        let haystack = "The quick BROWN fox jumps over the lazy dog";
        let s = build_snippet(haystack, "brown").expect("should find 'brown'");
        assert!(
            s.to_lowercase().contains("brown"),
            "snippet should contain the match: {s}"
        );
    }

    #[test]
    fn build_snippet_finds_cjk_substring() {
        let haystack =
            "这是一段关于浙江移动的对话内容，后面还有很多其他文字用于测试截断逻辑是否正确工作。";
        let s = build_snippet(haystack, "浙江移动").expect("should find '浙江移动'");
        assert!(
            s.contains("浙江移动"),
            "snippet should contain the CJK match: {s}"
        );
    }

    #[test]
    fn build_snippet_truncates_long_context() {
        let prefix: String = "a".repeat(300);
        let haystack = format!("{prefix}NEEDLE here and some suffix text");
        let s = build_snippet(&haystack, "needle").expect("should find 'needle'");
        // Snippet should be roughly SNIPPET_MAX_CHARS + ellipsis overhead
        assert!(s.chars().count() < 200, "snippet should be truncated: {s}");
        assert!(s.contains("NEEDLE"), "snippet should contain match: {s}");
        assert!(s.starts_with('…'), "should be prefixed with ellipsis");
    }

    #[test]
    fn build_snippet_returns_none_for_missing_needle() {
        assert!(build_snippet("hello world", "missing").is_none());
    }

    #[test]
    fn build_snippet_returns_none_for_empty_needle() {
        assert!(build_snippet("hello", "").is_none());
    }
}
