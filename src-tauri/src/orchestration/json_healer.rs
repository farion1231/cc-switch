//! Progressive JSON self-repair heuristic.
//!
//! Ported from MiroFish `simulation_config_generator.py` lines 483-533:
//!   - `_fix_truncated_json`  -> Level 2: close unclosed brackets
//!   - `_try_fix_config_json` -> Level 3: extract JSON block
//!   - Control-char cleanup   -> Level 4: remove control characters
//!
//! The public entry point [`heal_json`] tries four progressively more
//! aggressive repair strategies, returning as soon as one succeeds.

use serde_json::Value;

/// Attempt to parse and progressively repair a raw JSON string.
///
/// Repair levels (tried in order; returns immediately on success):
///
/// | Level | Strategy |
/// |-------|----------|
/// | 1     | Direct `serde_json::from_str` |
/// | 2     | Close unclosed `{` / `[` brackets |
/// | 3     | Extract JSON block between first opener and last matching closer |
/// | 4     | Strip control characters (< 0x20 except `\\n \\r \\t`) + re-normalise whitespace |
///
/// If all four levels fail, a descriptive error string is returned.
pub fn heal_json(raw: &str) -> Result<Value, String> {
    // Level 1 — direct parse
    if let Ok(val) = serde_json::from_str::<Value>(raw) {
        return Ok(val);
    }

    // Level 2 — close unclosed brackets
    let level2 = close_unclosed_brackets(raw);
    if let Ok(val) = serde_json::from_str::<Value>(&level2) {
        return Ok(val);
    }

    // Level 3 — extract JSON block
    let level3 = extract_json_block(raw);
    if level3 != raw {
        if let Ok(val) = serde_json::from_str::<Value>(&level3) {
            return Ok(val);
        }
        // Also try closing brackets on the extracted block
        let level3_fixed = close_unclosed_brackets(&level3);
        if let Ok(val) = serde_json::from_str::<Value>(&level3_fixed) {
            return Ok(val);
        }
    }

    // Level 4 — remove control characters + re-normalise whitespace
    let level4 = clean_control_chars(raw);
    // Try both the cleaned raw and the cleaned + bracket-closed version
    if let Ok(val) = serde_json::from_str::<Value>(&level4) {
        return Ok(val);
    }
    let level4_fixed = close_unclosed_brackets(&level4);
    if let Ok(val) = serde_json::from_str::<Value>(&level4_fixed) {
        return Ok(val);
    }

    Err(format!(
        "JSON repair failed after 4 levels. Input length: {} bytes, preview: {:?}",
        raw.len(),
        &raw[..raw.len().min(120)]
    ))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Count net unclosed `{` and `[` and append the missing closers.
///
/// Mirrors MiroFish `_fix_truncated_json`.
fn close_unclosed_brackets(raw: &str) -> String {
    let mut open_braces: i32 = 0; // {
    let mut open_brackets: i32 = 0; // [
    let mut in_string = false;
    let mut escape_next = false;

    for ch in raw.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => open_braces += 1,
            '}' => open_braces -= 1,
            '[' => open_brackets += 1,
            ']' => open_brackets -= 1,
            _ => {}
        }
    }

    let mut result = raw.to_string();

    // If the string ends mid-value, try to close it
    if !result.is_empty() {
        let last = result.chars().last().unwrap();
        if last != '"' && last != '}' && last != ']' && last != ',' && last != ':' {
            result.push('"');
        }
    }

    // Close brackets in reverse order (brackets first, then braces)
    for _ in 0..open_brackets.max(0) {
        result.push(']');
    }
    for _ in 0..open_braces.max(0) {
        result.push('}');
    }

    result
}

/// Extract a JSON block from surrounding text.
///
/// Finds the first `{` or `[`, then locates the matching closer.
/// Mirrors MiroFish `_try_fix_config_json` regex extraction.
fn extract_json_block(raw: &str) -> String {
    // Find first { or [
    let first_open = raw.chars().position(|c| c == '{' || c == '[');
    let first_open = match first_open {
        Some(pos) => pos,
        None => return raw.to_string(),
    };

    let opener = raw.chars().nth(first_open).unwrap();

    // Find the matching closer
    if let Some(end) = find_matching_brace(raw, first_open) {
        return raw[first_open..=end].to_string();
    }

    // Fallback: if we can't find matching brace, try regex-style extraction
    // (first { to last } or first [ to last ])
    let closer = if opener == '{' { '}' } else { ']' };
    if let Some(last_close) = raw.rfind(closer) {
        if last_close > first_open {
            return raw[first_open..=last_close].to_string();
        }
    }

    raw.to_string()
}

/// Find the position of the bracket/brace that matches the one at `start`.
fn find_matching_brace(raw: &str, start: usize) -> Option<usize> {
    let chars: Vec<char> = raw.chars().collect();
    if start >= chars.len() {
        return None;
    }

    let opener = chars[start];
    let closer = match opener {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for i in start..chars.len() {
        let ch = chars[i];

        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }

        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 && ch == closer {
                    return Some(i);
                }
            }
            _ => {}
        }
    }

    None
}

/// Remove control characters (bytes < 0x20 except `\\n`, `\\r`, `\\t`)
/// and collapse runs of whitespace into single spaces.
///
/// Mirrors MiroFish control-char cleanup:
/// ```python
/// re.sub(r'[\x00-\x1f\x7f-\x9f]', ' ', json_str)
/// re.sub(r'\s+', ' ', json_str)
/// ```
fn clean_control_chars(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            let code = c as u32;
            if code < 0x20 && c != '\n' && c != '\r' && c != '\t' {
                ' '
            } else if code >= 0x7f && code <= 0x9f {
                ' '
            } else {
                c
            }
        })
        .collect();

    // Collapse whitespace runs, but preserve newlines inside string values
    // by processing character-by-character.
    let mut result = String::with_capacity(cleaned.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut prev_was_space = false;

    for ch in cleaned.chars() {
        if escape_next {
            escape_next = false;
            result.push(ch);
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            result.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            prev_was_space = false;
            result.push(ch);
            continue;
        }

        if in_string {
            result.push(ch);
            continue;
        }

        // Outside strings: collapse whitespace
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            prev_was_space = false;
            result.push(ch);
        }
    }

    result.trim().to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Level 1: Valid JSON passes through ---

    #[test]
    fn valid_json_object_passes_level1() {
        let input = r#"{"name": "test", "value": 42}"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn valid_json_array_passes_level1() {
        let input = r#"[1, 2, 3]"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    #[test]
    fn nested_valid_json_passes_level1() {
        let input = r#"{"outer": {"inner": [1, 2, {"deep": true}]}}"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["outer"]["inner"][2]["deep"], true);
    }

    // --- Level 2: Missing closing brace ---

    #[test]
    fn missing_closing_brace_repaired_by_level2() {
        let input = r#"{"name": "test", "value": 42"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn missing_multiple_brackets_fixed() {
        let input = r#"{"items": [1, 2, 3"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn missing_closing_bracket_only() {
        let input = r#"[1, 2, 3"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    // --- Level 3: JSON embedded in markdown/text ---

    #[test]
    fn json_in_markdown_code_block_extracted_by_level3() {
        let input = r#"Here is the result:
```json
{"status": "ok", "count": 5}
```
That's it."#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["count"], 5);
    }

    #[test]
    fn json_embedded_in_plain_text() {
        let input = r#"The response is {"key": "value"} and then some more text."#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn json_array_extraction() {
        let input = r#"Some prefix [1, 2, 3] some suffix"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    // --- Level 4: Control characters ---

    #[test]
    fn control_chars_cleaned_by_level4() {
        let input = "{\x00\"name\":\x01 \"test\"\x02}";
        let result = heal_json(input).unwrap();
        assert_eq!(result["name"], "test");
    }

    #[test]
    fn newlines_preserved_in_strings() {
        // Newlines inside strings should be preserved as-is for serde to handle
        let input = "{\"name\": \"line1\nline2\"}";
        let result = heal_json(input).unwrap();
        assert!(result["name"].as_str().unwrap().contains('\n'));
    }

    // --- Error case ---

    #[test]
    fn completely_invalid_input_returns_error() {
        let input = "this is not json at all, no brackets or anything parseable";
        let result = heal_json(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JSON repair failed"));
    }

    // --- Nested objects ---

    #[test]
    fn deeply_nested_objects_work() {
        let input = r#"{"a": {"b": {"c": {"d": "deep"}}}}"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["a"]["b"]["c"]["d"], "deep");
    }

    #[test]
    fn nested_with_missing_braces() {
        let input = r#"{"a": {"b": [1, 2]"#;
        let result = heal_json(input).unwrap();
        assert_eq!(result["a"]["b"].as_array().unwrap().len(), 2);
    }

    // --- Unit tests for helpers ---

    #[test]
    fn close_unclosed_brackets_basic() {
        assert_eq!(close_unclosed_brackets("{"), "{}");
        assert_eq!(close_unclosed_brackets("["), "[]");
        assert_eq!(close_unclosed_brackets("{["), "{[]}");
    }

    #[test]
    fn close_unclosed_does_not_overclose() {
        assert_eq!(close_unclosed_brackets("{}"), "{}");
        assert_eq!(close_unclosed_brackets("[]"), "[]");
        assert_eq!(close_unclosed_brackets("{\"a\":[]}"), "{\"a\":[]}");
    }

    #[test]
    fn extract_json_block_finds_first_object() {
        let input = "prefix {\"x\": 1} suffix";
        assert_eq!(extract_json_block(input), "{\"x\": 1}");
    }

    #[test]
    fn extract_json_block_finds_first_array() {
        let input = "prefix [1,2,3] suffix";
        assert_eq!(extract_json_block(input), "[1,2,3]");
    }

    #[test]
    fn clean_control_chars_strips_low_bytes() {
        let input = "hello\x00world\x01test";
        assert_eq!(clean_control_chars(input), "hello world test");
    }

    #[test]
    fn clean_control_chars_preserves_newlines_in_strings() {
        // Outside strings, whitespace is collapsed
        let input = "{  \"a\"  :  1  }";
        assert_eq!(clean_control_chars(input), "{ \"a\" : 1 }");
    }
}
