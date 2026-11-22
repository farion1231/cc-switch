//! OpenAI Responses API 格式转换模块
//!
//! 实现 Anthropic Messages ↔ OpenAI Responses API 格式转换。
//! Responses API 是 OpenAI 2025 年推出的新一代 API，采用扁平化的 input/output 结构。
//!
//! 与 Chat Completions 的主要差异：
//! - tool_use/tool_result 从 message content 中"提升"为顶层 input item
//! - system prompt 使用 `instructions` 字段而非 system role message
//! - usage 字段命名与 Anthropic 一致 (input_tokens/output_tokens)

use crate::proxy::{
    error::ProxyError,
    json_canonical::canonical_json_string,
    tool_media::{
        strip_and_clamp_media_from_tool_value, ToolMediaScope, TOOL_RESULT_MEDIA_ATTACHED_MARKER,
    },
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::reasoning_bridge::{
    anthropic_block_from_openai_reasoning_item, openai_reasoning_item_from_anthropic_block,
};

pub(crate) const TOOL_RESULT_ERROR_MARKER: &str = "[cc-switch:tool-result-error]";

/// OpenAI Responses API 拒绝 `max_output_tokens` 小于 16。Anthropic 侧的合法
/// 小预算探测请求（如 Claude Desktop 的模型可用性探测发 `max_tokens=1`）
/// 会被严格网关整体拒绝，转换时把低于下限的预算抬到最小值而不是让请求失败。
pub(crate) const RESPONSES_MIN_MAX_OUTPUT_TOKENS: u64 = 16;

fn has_http_url_scheme(value: &str) -> bool {
    value
        .get(.."http://".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        || value
            .get(.."https://".len())
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn anthropic_image_to_responses_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    match source.get("type").and_then(Value::as_str) {
        Some("url") => source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| has_http_url_scheme(url))
            .map(|url| json!({"type":"input_image","image_url":url})),
        Some("base64") | None => {
            let data = source.get("data").and_then(Value::as_str)?;
            if data.is_empty() {
                return None;
            }
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            Some(json!({
                "type":"input_image",
                "image_url":format!("data:{media_type};base64,{data}")
            }))
        }
        _ => None,
    }
}

fn anthropic_document_to_responses_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    let filename = block
        .get("title")
        .or_else(|| block.get("filename"))
        .and_then(Value::as_str)
        .unwrap_or("document.pdf");
    match source.get("type").and_then(Value::as_str) {
        Some("url") => source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| has_http_url_scheme(url))
            .map(|url| json!({"type":"input_file","file_url":url,"filename":filename})),
        Some("base64") => {
            let data = source.get("data").and_then(Value::as_str)?;
            if data.is_empty() {
                return None;
            }
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("application/pdf");
            Some(json!({
                "type":"input_file",
                "file_data":format!("data:{media_type};base64,{data}"),
                "filename":filename
            }))
        }
        _ => None,
    }
}

fn anthropic_tool_result_to_responses_output(block: &Value) -> Value {
    let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
    let content = block.get("content");

    if !is_error {
        if let Some(text @ Value::String(_)) = content {
            if let Some(output) = alternate_image_tool_result_to_responses(text) {
                return Value::Array(output);
            }
            return text.clone();
        }
    }

    let mut output = Vec::new();
    if is_error {
        output.push(json!({"type":"input_text","text":TOOL_RESULT_ERROR_MARKER}));
    }

    match content {
        Some(Value::String(text)) => {
            if let Some(mut alternate) =
                alternate_image_tool_result_to_responses(&Value::String(text.clone()))
            {
                output.append(&mut alternate);
            } else {
                output.push(json!({"type":"input_text","text":text}));
            }
        }
        Some(Value::Array(blocks)) => {
            for part in blocks {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            output.push(json!({"type":"input_text","text":text}));
                        }
                    }
                    Some("image") => {
                        if let Some(image) = anthropic_image_to_responses_part(part) {
                            output.push(image);
                        } else if let Some(mut alternate) =
                            alternate_image_tool_result_to_responses(part)
                        {
                            output.append(&mut alternate);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                    Some("document") => {
                        if let Some(file) = anthropic_document_to_responses_part(part) {
                            output.push(file);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                    _ => {
                        if let Some(mut alternate) = alternate_image_tool_result_to_responses(part)
                        {
                            output.append(&mut alternate);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                }
            }
        }
        Some(value) => {
            if let Some(mut alternate) = alternate_image_tool_result_to_responses(value) {
                output.append(&mut alternate);
            } else {
                output.push(json!({
                    "type":"input_text",
                    "text":canonical_json_string(value)
                }));
            }
        }
        None => {}
    }

    Value::Array(output)
}

fn alternate_image_tool_result_to_responses(value: &Value) -> Option<Vec<Value>> {
    let mut cleaned = value.clone();
    let replacement_block = json!({
        "type":"input_text",
        "text":TOOL_RESULT_MEDIA_ATTACHED_MARKER
    });
    let mut chat_media_parts = Vec::new();
    let replaced = strip_and_clamp_media_from_tool_value(
        &mut cleaned,
        &mut chat_media_parts,
        ToolMediaScope::ImagesOnly,
        &replacement_block,
        TOOL_RESULT_MEDIA_ATTACHED_MARKER,
    );
    if replaced == 0 {
        return None;
    }

    let mut output = Vec::new();
    append_sanitized_responses_tool_value(&cleaned, &mut output);
    output.extend(
        chat_media_parts
            .iter()
            .filter_map(responses_image_from_chat_media),
    );
    Some(output)
}

fn append_sanitized_responses_tool_value(value: &Value, output: &mut Vec<Value>) {
    match value {
        Value::String(text) if !text.is_empty() => {
            output.push(json!({"type":"input_text","text":text}));
        }
        Value::Array(parts) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            output.push(json!({"type":"input_text","text":text}));
                        }
                    }
                    _ => output.push(json!({
                        "type":"input_text",
                        "text":canonical_json_string(part)
                    })),
                }
            }
        }
        Value::Object(object)
            if matches!(
                object.get("type").and_then(Value::as_str),
                Some("input_text" | "output_text" | "text")
            ) =>
        {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                output.push(json!({"type":"input_text","text":text}));
            }
        }
        Value::Null | Value::String(_) => {}
        other => output.push(json!({
            "type":"input_text",
            "text":canonical_json_string(other)
        })),
    }
}

fn responses_image_from_chat_media(part: &Value) -> Option<Value> {
    let image_url = part
        .pointer("/image_url/url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())?;
    let mut image = json!({
        "type":"input_image",
        "image_url":image_url
    });
    if let Some(detail) = part.pointer("/image_url/detail") {
        image["detail"] = detail.clone();
    }
    Some(image)
}

pub(crate) fn sanitize_anthropic_tool_use_input(name: &str, input: Value) -> Value {
    if name != "Read" {
        return input;
    }

    match input {
        Value::Object(mut object) => {
            if matches!(object.get("pages"), Some(Value::String(value)) if value.is_empty()) {
                object.remove("pages");
            }
            Value::Object(object)
        }
        other => other,
    }
}

pub(crate) fn sanitize_anthropic_tool_use_input_json(name: &str, raw: &str) -> String {
    if name != "Read" || raw.is_empty() {
        return raw.to_string();
    }

    let Ok(input) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };

    serde_json::to_string(&sanitize_anthropic_tool_use_input(name, input))
        .unwrap_or_else(|_| raw.to_string())
}

/// Anthropic versions its hosted web-search tool in the `type` field
/// (`web_search_20250305`, and potentially newer date-suffixed variants).
/// Match the semantic tool family instead of pinning the bridge to one release.
fn is_anthropic_web_search_tool(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|tool_type| tool_type == "web_search" || tool_type.starts_with("web_search_"))
}

fn validate_anthropic_web_search_direct_mode(tool: &Value) -> Result<(), ProxyError> {
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("web_search");
    let defaults_to_direct = match tool_type {
        "web_search" | "web_search_20250305" => true,
        "web_search_20260209" | "web_search_20260318" => false,
        _ => {
            return Err(ProxyError::InvalidRequest(format!(
                "Anthropic WebSearch version '{tool_type}' is not supported by the Responses bridge"
            )))
        }
    };

    if tool.get("response_inclusion").is_some() {
        return Err(ProxyError::InvalidRequest(
            "Anthropic WebSearch response_inclusion cannot be represented by the Responses bridge"
                .to_string(),
        ));
    }

    match tool.get("allowed_callers") {
        None if defaults_to_direct => Ok(()),
        None => Err(ProxyError::InvalidRequest(format!(
            "Anthropic WebSearch version '{tool_type}' defaults to code execution; set allowed_callers to [\"direct\"] for the Responses bridge"
        ))),
        Some(Value::Array(callers))
            if callers.len() == 1 && callers[0].as_str() == Some("direct") =>
        {
            Ok(())
        }
        Some(_) => Err(ProxyError::InvalidRequest(
            "Anthropic WebSearch allowed_callers must be exactly [\"direct\"] for the Responses bridge"
                .to_string(),
        )),
    }
}

pub(crate) fn anthropic_web_search_tool_name(body: &Value) -> Option<&str> {
    let tools = body.get("tools").and_then(Value::as_array)?;
    let forced_name = body
        .get("tool_choice")
        .and_then(Value::as_object)
        .filter(|choice| choice.get("type").and_then(Value::as_str) == Some("tool"))
        .and_then(|choice| choice.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty());
    if let Some(forced_name) = forced_name.filter(|forced_name| {
        tools.iter().any(|tool| {
            is_anthropic_web_search_tool(tool)
                && tool.get("name").and_then(Value::as_str) == Some(*forced_name)
        })
    }) {
        return Some(forced_name);
    }

    tools
        .iter()
        .find(|tool| is_anthropic_web_search_tool(tool))
        .and_then(|tool| tool.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

fn anthropic_web_search_to_responses(
    tool: &Value,
    is_codex_oauth: bool,
) -> Result<(Value, Option<u64>), ProxyError> {
    validate_anthropic_web_search_direct_mode(tool)?;

    let blocked_domains = tool
        .get("blocked_domains")
        .and_then(Value::as_array)
        .filter(|domains| !domains.is_empty());
    if blocked_domains.is_some() && is_codex_oauth {
        // The Responses API documents `filters.blocked_domains`, but the
        // ChatGPT Codex backend does not. Failing closed avoids silently
        // searching domains the caller excluded if that backend ignores it.
        return Err(ProxyError::InvalidRequest(
            "Anthropic WebSearch blocked_domains is not supported by the Codex OAuth backend"
                .to_string(),
        ));
    }

    let max_uses = match tool.get("max_uses") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().filter(|limit| *limit > 0).ok_or_else(|| {
            ProxyError::InvalidRequest(
                "Anthropic WebSearch max_uses must be a positive integer".to_string(),
            )
        })?),
    };
    let mut response_tool = json!({"type": "web_search"});
    if is_codex_oauth {
        // `external_web_access` is a private ChatGPT Codex backend switch. It is
        // not part of the standard Responses web-search schema, and strict
        // API-key-compatible gateways reject the unknown member.
        response_tool["external_web_access"] = json!(true);
    }

    let mut filters = serde_json::Map::new();
    if let Some(allowed_domains) = tool
        .get("allowed_domains")
        .and_then(Value::as_array)
        .filter(|domains| !domains.is_empty())
    {
        filters.insert("allowed_domains".to_string(), json!(allowed_domains));
    }
    if let Some(blocked_domains) = blocked_domains {
        filters.insert("blocked_domains".to_string(), json!(blocked_domains));
    }
    if !filters.is_empty() {
        response_tool["filters"] = Value::Object(filters);
    }

    if let Some(user_location) = tool
        .get("user_location")
        .filter(|location| location.is_object())
    {
        response_tool["user_location"] = user_location.clone();
    }

    Ok((response_tool, max_uses))
}

pub(crate) fn anthropic_web_search_max_uses(body: &Value) -> Option<u64> {
    let tools = body.get("tools").and_then(Value::as_array)?;
    let forced_name = body
        .get("tool_choice")
        .and_then(Value::as_object)
        .filter(|choice| choice.get("type").and_then(Value::as_str) == Some("tool"))
        .and_then(|choice| choice.get("name"))
        .and_then(Value::as_str)
        .filter(|name| {
            tools.iter().any(|tool| {
                is_anthropic_web_search_tool(tool)
                    && tool.get("name").and_then(Value::as_str) == Some(*name)
            })
        });
    tools
        .iter()
        .filter(|tool| {
            is_anthropic_web_search_tool(tool)
                && forced_name
                    .is_none_or(|name| tool.get("name").and_then(Value::as_str) == Some(name))
        })
        .filter_map(|tool| tool.get("max_uses").and_then(Value::as_u64))
        .filter(|limit| *limit > 0)
        .min()
}

pub(crate) fn web_search_action_input(item: &Value) -> Value {
    let Some(action) = item.get("action").and_then(Value::as_object) else {
        return json!({});
    };

    let mut input = serde_json::Map::new();
    for key in ["query", "queries", "url", "pattern"] {
        if let Some(value) = action.get(key) {
            input.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(input)
}

fn markdown_link_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_whitespace() {
            escaped.push(' ');
        } else if character.is_control() {
            escaped.push('\u{fffd}');
        } else {
            match character {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                character if character.is_ascii_punctuation() => {
                    escaped.push('\\');
                    escaped.push(character);
                }
                character => escaped.push(character),
            }
        }
    }
    escaped
}

fn markdown_link_destination(value: &str) -> Option<String> {
    let value = value.trim();
    if !has_http_url_scheme(value) || value.chars().any(char::is_control) {
        return None;
    }
    Some(
        value
            .replace('\\', "%5C")
            .replace(' ', "%20")
            .replace('(', "%28")
            .replace(')', "%29")
            .replace('<', "%3C")
            .replace('>', "%3E"),
    )
}

fn char_index_to_byte_offset(text: &str, index: usize) -> Option<usize> {
    if index == text.chars().count() {
        Some(text.len())
    } else {
        text.char_indices()
            .nth(index)
            .map(|(byte_offset, _)| byte_offset)
    }
}

fn is_markdown_escaped(text: &str, byte_offset: usize) -> bool {
    text.as_bytes()[..byte_offset]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

struct MarkdownBracketPairs {
    opening_to_closing: Vec<Option<usize>>,
    closing_to_opening: Vec<Option<(usize, bool)>>,
}

fn markdown_bracket_pairs(text: &str, code_mask: &[bool]) -> MarkdownBracketPairs {
    let bytes = text.as_bytes();
    let mut opening_to_closing = vec![None; bytes.len()];
    let mut closing_to_opening = vec![None; bytes.len()];
    let mut openings = Vec::new();
    let mut preceding_backslashes = 0_usize;
    for (offset, byte) in bytes.iter().copied().enumerate() {
        if code_mask.get(offset).copied().unwrap_or_default() {
            preceding_backslashes = 0;
            continue;
        }
        if byte == b'\\' {
            preceding_backslashes += 1;
            continue;
        }
        let escaped = preceding_backslashes % 2 == 1;
        preceding_backslashes = 0;
        if escaped {
            continue;
        }
        match byte {
            b'[' => openings.push(offset),
            b']' => {
                let Some(opening) = openings.pop() else {
                    continue;
                };
                let is_image = opening > 0
                    && bytes[opening - 1] == b'!'
                    && !is_markdown_escaped(text, opening - 1);
                opening_to_closing[opening] = Some(offset);
                closing_to_opening[offset] = Some((opening, is_image));
            }
            _ => {}
        }
    }
    MarkdownBracketPairs {
        opening_to_closing,
        closing_to_opening,
    }
}

fn markdown_list_marker_end(line: &[u8], start: usize) -> Option<usize> {
    let marker_end = match *line.get(start)? {
        b'-' | b'+' | b'*' => start + 1,
        byte if byte.is_ascii_digit() => {
            let digit_count = line[start..]
                .iter()
                .take(9)
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let delimiter = start + digit_count;
            if digit_count == 0 || !matches!(line.get(delimiter), Some(b'.' | b')')) {
                return None;
            }
            delimiter + 1
        }
        _ => return None,
    };
    if !line
        .get(marker_end)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        return None;
    }

    // CommonMark list padding is one to four spaces. Five or more spaces use
    // one column of list padding and leave an indented-code prefix behind.
    if line[marker_end] == b'\t' {
        return Some(marker_end + 1);
    }
    let spaces = line[marker_end..]
        .iter()
        .take_while(|byte| **byte == b' ')
        .count();
    Some(marker_end + if spaces <= 4 { spaces } else { 1 })
}

#[derive(Clone)]
enum MarkdownContainerToken {
    BlockQuote,
    List { continuation_indent: usize },
}

#[derive(Default)]
struct MarkdownContainerPrefix {
    content_start: usize,
    tokens: Vec<MarkdownContainerToken>,
}

fn markdown_container_prefix(line: &[u8]) -> MarkdownContainerPrefix {
    let mut offset = 0;
    let mut tokens = Vec::new();
    loop {
        let indentation = line[offset..]
            .iter()
            .take_while(|byte| **byte == b' ')
            .count();
        if indentation > 3 {
            break;
        }
        let marker_start = offset + indentation;
        if line.get(marker_start) == Some(&b'>') {
            offset = marker_start + 1;
            if line
                .get(offset)
                .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
            {
                offset += 1;
            }
            tokens.push(MarkdownContainerToken::BlockQuote);
            continue;
        }
        if let Some(end) = markdown_list_marker_end(line, marker_start) {
            tokens.push(MarkdownContainerToken::List {
                continuation_indent: end - offset,
            });
            offset = end;
            continue;
        }
        break;
    }
    MarkdownContainerPrefix {
        content_start: if tokens.is_empty() { 0 } else { offset },
        tokens,
    }
}

fn markdown_container_continuation_start(
    line: &[u8],
    tokens: &[MarkdownContainerToken],
) -> Option<usize> {
    let mut offset = 0;
    for token in tokens {
        match token {
            MarkdownContainerToken::BlockQuote => {
                let indentation = line[offset..]
                    .iter()
                    .take_while(|byte| **byte == b' ')
                    .count();
                if indentation > 3 {
                    return None;
                }
                let marker_start = offset + indentation;
                if line.get(marker_start) != Some(&b'>') {
                    return None;
                }
                offset = marker_start + 1;
                if line
                    .get(offset)
                    .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
                {
                    offset += 1;
                }
            }
            MarkdownContainerToken::List {
                continuation_indent,
            } => {
                let mut columns = 0;
                let mut consumed = 0;
                for byte in &line[offset..] {
                    match *byte {
                        b' ' => columns += 1,
                        b'\t' => columns += 4 - (columns % 4),
                        _ => break,
                    }
                    consumed += 1;
                    if columns >= *continuation_indent {
                        break;
                    }
                }
                if columns < *continuation_indent {
                    return None;
                }
                offset += consumed;
            }
        }
    }
    Some(offset)
}

fn markdown_fence_at(line: &[u8], content_start: usize) -> Option<(u8, usize, usize)> {
    let indentation = line[content_start..]
        .iter()
        .take_while(|byte| **byte == b' ')
        .count();
    if indentation > 3 {
        return None;
    }
    let marker_start = content_start + indentation;
    let marker = *line.get(marker_start)?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let length = line[marker_start..]
        .iter()
        .take_while(|byte| **byte == marker)
        .count();
    (length >= 3).then_some((marker, length, marker_start + length))
}

#[derive(Clone)]
struct MarkdownFence {
    marker: u8,
    minimum_length: usize,
    containers: Vec<MarkdownContainerToken>,
}

fn markdown_fence(line: &[u8]) -> Option<(MarkdownFence, usize)> {
    let container = markdown_container_prefix(line);
    let (marker, minimum_length, suffix_start) = markdown_fence_at(line, container.content_start)?;
    Some((
        MarkdownFence {
            marker,
            minimum_length,
            containers: container.tokens,
        },
        suffix_start,
    ))
}

fn markdown_indentation_columns(line: &[u8]) -> usize {
    let mut columns = 0;
    for byte in line {
        match *byte {
            b' ' => columns += 1,
            b'\t' => columns += 4 - (columns % 4),
            _ => break,
        }
    }
    columns
}

struct MarkdownCodeScan {
    mask: Vec<bool>,
    unterminated_fence: Option<(u8, usize)>,
}

fn markdown_code_scan(text: &str) -> MarkdownCodeScan {
    let bytes = text.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut fence: Option<MarkdownFence> = None;
    let mut in_indented_code = false;
    let mut previous_line_blank = true;
    let mut line_start = 0;
    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset + 1);
        let line = &bytes[line_start..line_end];
        let line_blank = line.iter().all(|byte| byte.is_ascii_whitespace());
        let indentation = markdown_indentation_columns(line);
        let mut handled_by_fence = false;
        if let Some(open_fence) = fence.clone() {
            let content_start = if open_fence.containers.is_empty() {
                Some(0)
            } else {
                markdown_container_continuation_start(line, &open_fence.containers)
            };
            if let Some(content_start) = content_start {
                handled_by_fence = true;
                mask[line_start..line_end].fill(true);
                if let Some((candidate, length, suffix_start)) =
                    markdown_fence_at(line, content_start)
                {
                    if candidate == open_fence.marker
                        && length >= open_fence.minimum_length
                        && line[suffix_start..]
                            .iter()
                            .all(|byte| byte.is_ascii_whitespace())
                    {
                        fence = None;
                    }
                }
            } else {
                // A fenced block cannot outlive its blockquote/list container.
                // Re-process this line as top-level Markdown.
                fence = None;
            }
        }
        if !handled_by_fence {
            let mut handled_as_indented = false;
            if in_indented_code {
                if line_blank || indentation >= 4 {
                    mask[line_start..line_end].fill(true);
                    handled_as_indented = true;
                } else {
                    in_indented_code = false;
                }
            }
            if !handled_as_indented && indentation >= 4 && previous_line_blank {
                mask[line_start..line_end].fill(true);
                in_indented_code = true;
                handled_as_indented = true;
            }
            if !handled_as_indented {
                if let Some((new_fence, suffix_start)) = markdown_fence(line) {
                    // A backtick fence's info string cannot itself contain a backtick.
                    // Treat such a line as ordinary text instead of masking the rest
                    // of the response as an unterminated code block.
                    if new_fence.marker != b'`' || !line[suffix_start..].contains(&b'`') {
                        mask[line_start..line_end].fill(true);
                        fence = Some(new_fence);
                    }
                }
            }
        }
        previous_line_blank = line_blank;
        line_start = line_end;
    }

    let mut offset = 0;
    while offset < bytes.len() {
        if mask[offset] || bytes[offset] != b'`' || is_markdown_escaped(text, offset) {
            offset += 1;
            continue;
        }
        let delimiter_length = bytes[offset..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let mut candidate = offset + delimiter_length;
        let mut closing = None;
        while candidate < bytes.len() {
            if mask[candidate] {
                candidate += 1;
                continue;
            }
            // Backslashes have no escaping semantics inside a code span.
            if bytes[candidate] != b'`' {
                candidate += 1;
                continue;
            }
            let candidate_length = bytes[candidate..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if candidate_length == delimiter_length {
                closing = Some(candidate + candidate_length);
                break;
            }
            candidate += candidate_length;
        }
        if let Some(closing) = closing {
            mask[offset..closing].fill(true);
            offset = closing;
        } else {
            offset += delimiter_length;
        }
    }
    MarkdownCodeScan {
        mask,
        unterminated_fence: fence
            .filter(|fence| fence.containers.is_empty())
            .map(|fence| (fence.marker, fence.minimum_length)),
    }
}

fn markdown_link_suffix_end(rest: &str) -> Option<usize> {
    if rest.starts_with(')') {
        return Some(1);
    }
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }

    let trimmed = rest.trim_start_matches(char::is_whitespace);
    let leading_whitespace = rest.len() - trimmed.len();
    let rest = trimmed;
    if rest.starts_with(')') {
        return Some(leading_whitespace + 1);
    }
    let delimiter = rest
        .chars()
        .next()
        .filter(|ch| matches!(ch, '"' | '\'' | '('))?;
    let closing_delimiter = if delimiter == '(' { ')' } else { delimiter };
    let title = &rest[delimiter.len_utf8()..];
    let closing_offset = title
        .char_indices()
        .find(|(offset, ch)| *ch == closing_delimiter && !is_markdown_escaped(title, *offset))
        .map(|(offset, _)| offset)?;
    let after_title = &title[closing_offset + closing_delimiter.len_utf8()..];
    let trimmed_after_title = after_title.trim_start_matches(char::is_whitespace);
    trimmed_after_title.starts_with(')').then_some(
        leading_whitespace
            + delimiter.len_utf8()
            + closing_offset
            + closing_delimiter.len_utf8()
            + (after_title.len() - trimmed_after_title.len())
            + 1,
    )
}

fn markdown_link_suffix_is_closed(rest: &str) -> bool {
    markdown_link_suffix_end(rest).is_some()
}

fn markdown_inline_destination_end(destination: &str) -> Option<usize> {
    if let Some(angle_destination) = destination.strip_prefix('<') {
        let closing = angle_destination
            .char_indices()
            .find(|(offset, character)| {
                *character == '>' && !is_markdown_escaped(angle_destination, *offset)
            })
            .map(|(offset, _)| offset)?;
        let suffix_start = 1 + closing + 1;
        return markdown_link_suffix_end(&destination[suffix_start..])
            .map(|suffix_length| suffix_start + suffix_length);
    }

    let mut parentheses = 0_u32;
    let mut escaped = false;
    for (offset, character) in destination.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        match character {
            '<' | '>' if parentheses == 0 => return None,
            '(' => parentheses += 1,
            ')' if parentheses == 0 => return Some(offset + 1),
            ')' => parentheses -= 1,
            character if character.is_whitespace() && parentheses == 0 => {
                return markdown_link_suffix_end(&destination[offset..])
                    .map(|suffix_length| offset + suffix_length);
            }
            _ => {}
        }
    }
    None
}

fn is_valid_bare_markdown_destination(value: &str) -> bool {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || value.contains(['<', '>', '\\'])
    {
        return false;
    }

    let mut parentheses = 0_u32;
    for character in value.chars() {
        match character {
            '(' => parentheses += 1,
            ')' if parentheses == 0 => return false,
            ')' => parentheses -= 1,
            _ => {}
        }
    }
    parentheses == 0
}

fn is_valid_markdown_autolink_destination(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '<' | '>' | '\\')
        })
}

fn markdown_closing_bracket(text: &str, opening_bracket: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut nested = 0_u32;
    for (offset, byte) in bytes.iter().enumerate().skip(opening_bracket + 1) {
        if is_markdown_escaped(text, offset) {
            continue;
        }
        match byte {
            b'[' => nested += 1,
            b']' if nested == 0 => return Some(offset),
            b']' => nested -= 1,
            _ => {}
        }
    }
    None
}

fn normalize_markdown_reference_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Clone)]
struct MarkdownReferenceDefinition {
    label: String,
    destination: String,
    line_start: usize,
    line_end: usize,
}

fn markdown_reference_destination(value: &str) -> Option<&str> {
    let value = value.trim_start_matches(char::is_whitespace);
    if let Some(angle_destination) = value.strip_prefix('<') {
        let closing = angle_destination
            .char_indices()
            .find(|(offset, character)| {
                *character == '>' && !is_markdown_escaped(angle_destination, *offset)
            })
            .map(|(offset, _)| offset)?;
        return Some(&angle_destination[..closing]);
    }

    let mut parentheses = 0_u32;
    let mut escaped = false;
    for (offset, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        match character {
            '(' => parentheses += 1,
            ')' if parentheses == 0 => return None,
            ')' => parentheses -= 1,
            character if character.is_whitespace() && parentheses == 0 => {
                return (offset > 0).then_some(&value[..offset]);
            }
            _ => {}
        }
    }
    (!value.is_empty() && parentheses == 0).then_some(value)
}

fn markdown_reference_definitions(
    text: &str,
    code_mask: &[bool],
) -> Vec<MarkdownReferenceDefinition> {
    let bytes = text.as_bytes();
    let mut definitions = Vec::new();
    let mut line_start = 0;
    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset + 1);
        let line = &text[line_start..line_end];
        if code_mask.get(line_start).copied().unwrap_or_default() {
            line_start = line_end;
            continue;
        }
        let indentation = line
            .as_bytes()
            .iter()
            .take_while(|byte| **byte == b' ')
            .count();
        if indentation <= 3 && line.as_bytes().get(indentation) == Some(&b'[') {
            if let Some(closing) = markdown_closing_bracket(line, indentation) {
                if line.as_bytes().get(closing + 1) == Some(&b':') {
                    let label = normalize_markdown_reference_label(&line[indentation + 1..closing]);
                    if !label.is_empty() {
                        if let Some(destination) =
                            markdown_reference_destination(&line[closing + 2..])
                        {
                            definitions.push(MarkdownReferenceDefinition {
                                label,
                                destination: destination.to_string(),
                                line_start,
                                line_end,
                            });
                        }
                    }
                }
            }
        }
        line_start = line_end;
    }
    definitions
}

#[derive(Clone)]
struct MarkdownReferenceUse {
    label: String,
    start: usize,
    end: usize,
    is_image: bool,
}

fn markdown_reference_uses(
    text: &str,
    code_mask: &[bool],
    definitions: &[MarkdownReferenceDefinition],
    brackets: &MarkdownBracketPairs,
) -> Vec<MarkdownReferenceUse> {
    let bytes = text.as_bytes();
    let mut uses = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes[offset] != b'['
            || code_mask.get(offset).copied().unwrap_or_default()
            || is_markdown_escaped(text, offset)
            || definitions
                .iter()
                .any(|definition| (definition.line_start..definition.line_end).contains(&offset))
        {
            offset += 1;
            continue;
        }
        let Some(first_closing) = brackets.opening_to_closing.get(offset).copied().flatten() else {
            offset += 1;
            continue;
        };
        let first_label = normalize_markdown_reference_label(&text[offset + 1..first_closing]);
        let mut label = first_label.clone();
        let mut end = first_closing + 1;
        if bytes.get(end) == Some(&b'(') {
            offset = end;
            continue;
        }
        if bytes.get(end) == Some(&b'[') {
            let Some(second_closing) = brackets.opening_to_closing.get(end).copied().flatten()
            else {
                offset = end;
                continue;
            };
            let second_label = normalize_markdown_reference_label(&text[end + 1..second_closing]);
            if !second_label.is_empty() {
                label = second_label;
            }
            end = second_closing + 1;
        }
        if !label.is_empty()
            && definitions
                .iter()
                .any(|definition| definition.label == label)
        {
            let is_image =
                offset > 0 && bytes[offset - 1] == b'!' && !is_markdown_escaped(text, offset - 1);
            uses.push(MarkdownReferenceUse {
                label,
                start: if is_image { offset - 1 } else { offset },
                end,
                is_image,
            });
        }
        offset = end.max(offset + 1);
    }
    uses
}

fn markdown_link_syntax_mask(
    text: &str,
    code_mask: &[bool],
    definitions: &[MarkdownReferenceDefinition],
    reference_uses: &[MarkdownReferenceUse],
    brackets: &MarkdownBracketPairs,
) -> Vec<bool> {
    let mut mask = vec![false; text.len()];
    for (closing_label, _) in text.match_indices("](") {
        if code_mask.get(closing_label).copied().unwrap_or_default() {
            continue;
        }
        let Some((opening_label, is_image)) = brackets
            .closing_to_opening
            .get(closing_label)
            .copied()
            .flatten()
        else {
            continue;
        };
        let destination_start = closing_label + 2;
        if let Some(destination_length) =
            markdown_inline_destination_end(&text[destination_start..])
        {
            let start = if is_image {
                opening_label.saturating_sub(1)
            } else {
                opening_label
            };
            mask[start..destination_start + destination_length].fill(true);
        }
    }
    for reference in reference_uses {
        mask[reference.start..reference.end].fill(true);
    }
    for definition in definitions {
        mask[definition.line_start..definition.line_end].fill(true);
    }
    for (opening, _) in text.match_indices('<') {
        if code_mask.get(opening).copied().unwrap_or_default() || is_markdown_escaped(text, opening)
        {
            continue;
        }
        let Some(closing) = text[opening + 1..]
            .find('>')
            .map(|offset| opening + 1 + offset)
        else {
            continue;
        };
        let destination = &text[opening + 1..closing];
        if has_http_url_scheme(destination) && is_valid_markdown_autolink_destination(destination) {
            mask[opening..closing + 1].fill(true);
        }
    }
    mask
}

fn markdown_destination_matches_url(destination: &str, raw_url: &str, rendered_url: &str) -> bool {
    destination == raw_url
        || destination == rendered_url
        || markdown_link_destination(destination).as_deref() == Some(rendered_url)
}

fn contains_markdown_link_to_url_with_context(
    text: &str,
    raw_url: &str,
    rendered_url: &str,
    code_mask: &[bool],
    definitions: &[MarkdownReferenceDefinition],
    reference_uses: &[MarkdownReferenceUse],
    brackets: &MarkdownBracketPairs,
) -> bool {
    let has_inline_link = text.match_indices("](").any(|(offset, _)| {
        if code_mask.get(offset).copied().unwrap_or_default() {
            return false;
        }
        let Some((_, is_image)) = brackets.closing_to_opening.get(offset).copied().flatten() else {
            return false;
        };
        if is_image {
            return false;
        }
        let destination = &text[offset + 2..];
        destination
            .strip_prefix(rendered_url)
            .is_some_and(markdown_link_suffix_is_closed)
            || (is_valid_bare_markdown_destination(raw_url)
                && destination
                    .strip_prefix(raw_url)
                    .is_some_and(markdown_link_suffix_is_closed))
            || [raw_url, rendered_url].into_iter().any(|candidate| {
                destination
                    .strip_prefix('<')
                    .and_then(|rest| rest.strip_prefix(candidate))
                    .and_then(|rest| rest.strip_prefix('>'))
                    .is_some_and(markdown_link_suffix_is_closed)
            })
    });
    if has_inline_link {
        return true;
    }

    let has_reference_link = definitions.iter().any(|definition| {
        markdown_destination_matches_url(&definition.destination, raw_url, rendered_url)
            && reference_uses
                .iter()
                .any(|reference| !reference.is_image && reference.label == definition.label)
    });
    if has_reference_link {
        return true;
    }

    [raw_url, rendered_url].into_iter().any(|candidate| {
        if !is_valid_markdown_autolink_destination(candidate) {
            return false;
        }
        let autolink = format!("<{candidate}>");
        text.match_indices(&autolink).any(|(offset, _)| {
            !code_mask.get(offset).copied().unwrap_or_default()
                && !is_markdown_escaped(text, offset)
        })
    })
}

#[cfg(test)]
fn contains_markdown_link_to_url(text: &str, raw_url: &str, rendered_url: &str) -> bool {
    let code = markdown_code_scan(text);
    let definitions = markdown_reference_definitions(text, &code.mask);
    let brackets = markdown_bracket_pairs(text, &code.mask);
    let reference_uses = markdown_reference_uses(text, &code.mask, &definitions, &brackets);
    contains_markdown_link_to_url_with_context(
        text,
        raw_url,
        rendered_url,
        &code.mask,
        &definitions,
        &reference_uses,
        &brackets,
    )
}

pub(crate) fn text_with_url_citations(text: &str, annotations: &[Value]) -> String {
    struct Citation {
        start: Option<usize>,
        end: Option<usize>,
        raw_url: String,
        url: String,
        title: String,
    }

    let mut citations = Vec::new();
    for annotation in annotations {
        if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
            continue;
        }
        let Some(raw_url) = annotation.get("url").and_then(Value::as_str) else {
            continue;
        };
        let Some(url) = markdown_link_destination(raw_url) else {
            continue;
        };
        let title = annotation
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(raw_url)
            .to_string();
        let start = annotation
            .get("start_index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());
        let end = annotation
            .get("end_index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());
        citations.push(Citation {
            start,
            end,
            raw_url: raw_url.trim().to_string(),
            url,
            title,
        });
    }

    if citations.is_empty() {
        return text.to_string();
    }

    let code = markdown_code_scan(text);
    let definitions = markdown_reference_definitions(text, &code.mask);
    let brackets = markdown_bracket_pairs(text, &code.mask);
    let reference_uses = markdown_reference_uses(text, &code.mask, &definitions, &brackets);
    let link_syntax =
        markdown_link_syntax_mask(text, &code.mask, &definitions, &reference_uses, &brackets);
    let mut linked_urls = citations
        .iter()
        .filter(|citation| {
            contains_markdown_link_to_url_with_context(
                text,
                &citation.raw_url,
                &citation.url,
                &code.mask,
                &definitions,
                &reference_uses,
                &brackets,
            )
        })
        .map(|citation| citation.url.clone())
        .collect::<HashSet<_>>();
    let mut ranged = Vec::new();
    let mut fallback = Vec::new();
    for citation in citations {
        let Some((start, end)) = citation.start.zip(citation.end) else {
            fallback.push(citation);
            continue;
        };
        let Some(start) = char_index_to_byte_offset(text, start) else {
            fallback.push(citation);
            continue;
        };
        let Some(end) = char_index_to_byte_offset(text, end) else {
            fallback.push(citation);
            continue;
        };
        if start >= end
            || text[start..end].trim().is_empty()
            || text[start..end]
                .chars()
                .any(|character| character.is_whitespace() && character != ' ')
            || code.mask[start..end].iter().any(|masked| *masked)
            || (link_syntax[start..end].iter().any(|masked| *masked)
                && !linked_urls.contains(&citation.url))
        {
            fallback.push(citation);
            continue;
        }
        ranged.push((start, end, citation));
    }
    ranged.sort_by_key(|(start, end, _)| (*start, *end));

    let mut rendered = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, citation) in ranged {
        if start < cursor {
            fallback.push(citation);
            continue;
        }
        rendered.push_str(&text[cursor..start]);
        let cited_text = &text[start..end];
        if linked_urls.contains(&citation.url) {
            rendered.push_str(cited_text);
            cursor = end;
            continue;
        }
        rendered.push('[');
        rendered.push_str(&markdown_link_label(cited_text));
        rendered.push_str("](");
        rendered.push_str(&citation.url);
        rendered.push(')');
        cursor = end;
        linked_urls.insert(citation.url);
    }
    rendered.push_str(&text[cursor..]);

    let mut fallback_links = Vec::new();
    for citation in fallback {
        if !linked_urls.insert(citation.url.clone()) {
            continue;
        }
        fallback_links.push(format!(
            "[{}]({})",
            markdown_link_label(&citation.title),
            citation.url
        ));
    }
    if !fallback_links.is_empty() {
        if !rendered.is_empty() {
            if let Some((marker, length)) = code.unterminated_fence {
                if !rendered.ends_with('\n') {
                    rendered.push('\n');
                }
                rendered.extend(std::iter::repeat_n(char::from(marker), length));
                rendered.push_str("\n\n");
            } else {
                rendered.push_str("\n\n");
            }
        }
        rendered.push_str("Sources: ");
        rendered.push_str(&fallback_links.join(", "));
    }
    rendered
}

pub(crate) fn output_text_with_url_citations(block: &Value) -> Option<String> {
    let text = block
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())?;
    let annotations = block
        .get("annotations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Some(text_with_url_citations(text, annotations))
}

fn collect_web_search_results_from_content(content: &Value, results: &mut Vec<Value>) {
    let Some(blocks) = content.as_array() else {
        return;
    };

    for block in blocks {
        let Some(annotations) = block.get("annotations").and_then(Value::as_array) else {
            continue;
        };
        for annotation in annotations {
            if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
                continue;
            }
            let Some(url) = annotation
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let title = annotation
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .unwrap_or(url);
            results.push(json!({
                "type": "web_search_result",
                "url": url,
                "title": title,
                "encrypted_content": "",
                "page_age": null
            }));
        }
    }
}

pub(crate) fn web_search_results_from_output_item(item: &Value) -> Vec<Value> {
    let mut results = Vec::new();
    if item.get("type").and_then(Value::as_str) == Some("message") {
        if let Some(content) = item.get("content") {
            collect_web_search_results_from_content(content, &mut results);
        }
    } else if item.get("type").and_then(Value::as_str) == Some("output_text") {
        collect_web_search_results_from_content(&json!([item]), &mut results);
    }

    let mut seen = HashSet::new();
    results.retain(|result| {
        result
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| seen.insert(url.to_string()))
    });
    results
}

pub(crate) fn web_search_results_from_action(item: &Value) -> Vec<Value> {
    let mut results = Vec::new();
    let Some(sources) = item.pointer("/action/sources").and_then(Value::as_array) else {
        return results;
    };

    for source in sources {
        let Some(url) = source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
        else {
            continue;
        };
        let title = source
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.is_empty())
            .unwrap_or(url);
        let page_age = source
            .get("page_age")
            .filter(|page_age| page_age.is_string())
            .cloned()
            .unwrap_or(Value::Null);
        results.push(json!({
            "type": "web_search_result",
            "url": url,
            "title": title,
            "encrypted_content": "",
            "page_age": page_age
        }));
    }

    let mut seen = HashSet::new();
    results.retain(|result| {
        result
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| seen.insert(url.to_string()))
    });
    results
}

pub(crate) fn merge_web_search_result_metadata(target: &mut [Value], candidates: &[Value]) {
    for candidate in candidates {
        let Some(url) = candidate
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
        else {
            continue;
        };
        let Some(existing) = target
            .iter_mut()
            .find(|result| result.get("url").and_then(Value::as_str) == Some(url))
        else {
            continue;
        };
        let Some(existing_object) = existing.as_object_mut() else {
            continue;
        };

        if let Some(candidate_title) = candidate
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.is_empty() && *title != url)
        {
            let existing_title_is_placeholder = existing_object
                .get("title")
                .and_then(Value::as_str)
                .is_none_or(|title| title.is_empty() || title == url);
            if existing_title_is_placeholder {
                existing_object.insert("title".to_string(), json!(candidate_title));
            }
        }

        if let Some(candidate_page_age) = candidate
            .get("page_age")
            .and_then(Value::as_str)
            .filter(|page_age| !page_age.is_empty())
        {
            let existing_page_age_is_missing = existing_object
                .get("page_age")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty);
            if existing_page_age_is_missing {
                existing_object.insert("page_age".to_string(), json!(candidate_page_age));
            }
        }
    }
}

pub(crate) fn web_search_tool_result_error(item: &Value) -> Option<Value> {
    let status = item.get("status").and_then(Value::as_str);
    let has_error = item.get("error").is_some_and(|error| !error.is_null());
    if status.is_none_or(|status| status == "completed") && !has_error {
        return None;
    }

    let error_signals = [
        item.pointer("/error/code").and_then(Value::as_str),
        item.pointer("/error/type").and_then(Value::as_str),
        item.pointer("/error/message").and_then(Value::as_str),
        item.get("error").and_then(Value::as_str),
    ];
    let signal = error_signals
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let error_code = if signal.contains("max_uses") {
        "max_uses_exceeded"
    } else if signal.contains("too_many_requests")
        || signal.contains("rate_limit")
        || signal.contains("rate limit")
    {
        "too_many_requests"
    } else if signal.contains("query_too_long") || signal.contains("query too long") {
        "query_too_long"
    } else if signal.contains("request_too_large") || signal.contains("request too large") {
        "request_too_large"
    } else if signal.contains("invalid") {
        "invalid_tool_input"
    } else {
        "unavailable"
    };

    Some(json!({
        "type": "web_search_tool_result_error",
        "error_code": error_code
    }))
}

pub(crate) fn web_search_max_uses_exceeded_error() -> Value {
    json!({
        "type": "web_search_tool_result_error",
        "error_code": "max_uses_exceeded"
    })
}

fn responses_web_search_call_from_anthropic_blocks(
    tool_use: &Value,
    tool_result: &Value,
) -> Option<Value> {
    let id = tool_use
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let input = tool_use.get("input").and_then(Value::as_object);

    // Anthropic omits the Responses action discriminator from server_tool_use
    // input. Infer it from the action-specific fields while keeping search as
    // the safe default for current and future hosted WebSearch tool names.
    let action_type = if input.is_some_and(|input| {
        input.get("pattern").and_then(Value::as_str).is_some()
            && input.get("url").and_then(Value::as_str).is_some()
    }) {
        "find_in_page"
    } else if input.is_some_and(|input| {
        input.get("url").and_then(Value::as_str).is_some()
            && !input.contains_key("query")
            && !input.contains_key("queries")
    }) {
        "open_page"
    } else {
        "search"
    };

    let mut action = serde_json::Map::new();
    action.insert("type".to_string(), json!(action_type));
    if let Some(input) = input {
        let fields: &[&str] = match action_type {
            "find_in_page" => &["url", "pattern"],
            "open_page" => &["url"],
            _ => &["query", "queries"],
        };
        for field in fields {
            if let Some(value) = input.get(*field) {
                action.insert((*field).to_string(), value.clone());
            }
        }
    }

    if action_type == "search" {
        let mut seen_urls = HashSet::new();
        let sources: Vec<Value> = tool_result
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|result| {
                let url = result
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|url| !url.is_empty())?;
                seen_urls
                    .insert(url.to_string())
                    .then(|| json!({"type": "url", "url": url}))
            })
            .collect();
        if !sources.is_empty() {
            action.insert("sources".to_string(), Value::Array(sources));
        }
    }

    let failed = tool_result.get("is_error").and_then(Value::as_bool) == Some(true)
        || tool_result
            .pointer("/content/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.ends_with("_error"));

    Some(json!({
        "type": "web_search_call",
        "id": id,
        "status": if failed { "failed" } else { "completed" },
        "action": Value::Object(action)
    }))
}

/// Anthropic 请求 → OpenAI Responses 请求
///
/// `cache_key`: optional prompt_cache_key to inject for improved cache routing
/// `is_codex_oauth`: 当目标后端是 ChatGPT Plus/Pro 反代 (`chatgpt.com/backend-api/codex`) 时为 true。
/// 该后端强制要求 `store: false`，并要求 `include` 包含 `reasoning.encrypted_content`
/// 以便在无服务端状态下保持多轮 reasoning 上下文。
/// `codex_fast_mode`: 仅在 `is_codex_oauth` 为 true 时生效，控制是否注入
/// `service_tier = "priority"`。
pub fn anthropic_to_responses(
    body: Value,
    cache_key: Option<&str>,
    is_codex_oauth: bool,
    codex_fast_mode: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // system → instructions (Responses API 使用 instructions 字段)
    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            super::transform::strip_leading_anthropic_billing_header(text).to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
                .map(super::transform::strip_leading_anthropic_billing_header)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };
        if !instructions.is_empty() {
            result["instructions"] = json!(instructions);
        }
    }

    // messages → input
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        let input = convert_messages_to_input(msgs)?;
        result["input"] = json!(input);
    }

    // max_tokens → max_output_tokens (Responses API uses max_output_tokens for
    // all models). The Responses API rejects values below 16 ("The number must
    // be `>= 16`"), while Anthropic clients legitimately send tiny probe
    // budgets (Claude Desktop's model-availability probe uses max_tokens=1),
    // so sub-floor budgets are clamped up to the minimum instead of failing
    // the whole request. 0 and non-integer values keep their existing
    // pass-through semantics.
    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = match v.as_u64() {
            Some(n) if (1..RESPONSES_MIN_MAX_OUTPUT_TOKENS).contains(&n) => {
                json!(RESPONSES_MIN_MAX_OUTPUT_TOKENS)
            }
            _ => v.clone(),
        };
    }

    // 直接透传的参数
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // Map Anthropic thinking → OpenAI Responses reasoning.effort
    if let Some(model_name) = body.get("model").and_then(|m| m.as_str()) {
        if super::transform::supports_reasoning_effort(model_name) {
            if let Some(effort) = super::transform::resolve_reasoning_effort(&body) {
                result["reasoning"] = json!({ "effort": effort });
            }
        }
    }

    // stop_sequences → 丢弃 (Responses API 不支持)

    // 转换 tools (过滤 BatchTool)
    //
    // The Codex OAuth request contract accepts only a string `tool_choice`.
    // When Anthropic forces one hosted WebSearch tool, `"required"` is exact
    // only if no unrelated tools remain in the outbound list. Resolve the
    // selected tool by its declared name so future versioned names keep working.
    let forced_hosted_web_search_name = if is_codex_oauth {
        body.get("tool_choice")
            .and_then(Value::as_object)
            .filter(|choice| choice.get("type").and_then(Value::as_str) == Some("tool"))
            .and_then(|choice| choice.get("name"))
            .and_then(Value::as_str)
            .filter(|selected_name| {
                body.get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        tools.iter().any(|tool| {
                            is_anthropic_web_search_tool(tool)
                                && tool.get("name").and_then(Value::as_str) == Some(*selected_name)
                        })
                    })
            })
    } else {
        None
    };
    let mut hosted_web_search_names = HashSet::new();
    let mut hosted_web_search_max_uses: Option<u64> = None;
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let mut response_tools = Vec::new();
        for tool in tools
            .iter()
            .filter(|t| t.get("type").and_then(Value::as_str) != Some("BatchTool"))
        {
            if is_anthropic_web_search_tool(tool) {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("web_search");
                hosted_web_search_names.insert(name.to_string());
                if forced_hosted_web_search_name.is_some_and(|selected| selected != name) {
                    continue;
                }
                let (response_tool, max_uses) =
                    anthropic_web_search_to_responses(tool, is_codex_oauth)?;
                if let Some(max_uses) = max_uses {
                    hosted_web_search_max_uses = Some(
                        hosted_web_search_max_uses
                            .map_or(max_uses, |current| current.min(max_uses)),
                    );
                }
                response_tools.push(response_tool);
            } else {
                if forced_hosted_web_search_name.is_some() {
                    continue;
                }
                let mut response_tool = json!({
                    "type": "function",
                    // Anthropic 工具 schema 允许可选参数（required 是任意子集），而
                    // Responses 后端把省略的 `strict` 按 true 处理：可选属性会被
                    // 严格化进 required，模型被迫输出它本应能省略的字段（#7713 中
                    // Claude Code Agent 的可选 isolation 被强制输出即此形态）。
                    // 显式 false 保留客户端 schema 的可选语义。
                    "strict": false,
                    "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                });
                // 同 transform.rs：缺失的 description 省略而非输出 null，
                // 否则严格上游会拒绝整个请求。
                if let Some(description) = tool.get("description").filter(|d| !d.is_null()) {
                    response_tool["description"] = description.clone();
                }
                response_tool["parameters"] = super::transform::clean_schema(
                    tool.get("input_schema").cloned().unwrap_or(json!({})),
                );
                response_tools.push(response_tool);
            }
        }

        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if let Some(max_uses) = hosted_web_search_max_uses {
        if is_codex_oauth {
            // Codex rejects max_tool_calls. Add a soft instruction cap here;
            // downstream converters count web_search_call items and cut off the
            // response on excess calls, forced or not. This response-side bound
            // cannot guarantee that excess searches have not executed upstream.
            let existing = result
                .get("instructions")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cap_instruction = format!(
                "You must perform no more than {max_uses} web search calls in this response."
            );
            result["instructions"] = json!(if existing.is_empty() {
                cap_instruction
            } else {
                format!("{existing}\n\n{cap_instruction}")
            });
        } else {
            // Responses exposes one aggregate cap for built-in tool calls.
            // Hosted WebSearch is the only built-in tool produced by this
            // transform, so this exactly enforces Anthropic's request limit.
            result["max_tool_calls"] = json!(max_uses);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] =
            map_tool_choice_to_responses(v, &hosted_web_search_names, is_codex_oauth);
        if is_codex_oauth {
            if let Some(disable_parallel) =
                v.get("disable_parallel_tool_use").and_then(Value::as_bool)
            {
                result["parallel_tool_calls"] = json!(!disable_parallel);
            }
        }
    }

    const WEB_SEARCH_SOURCES_MARKER: &str = "web_search_call.action.sources";
    // OpenAI otherwise returns citations only on the final message, without a
    // search-call ID. Request per-call sources so multiple hosted searches can
    // be paired with the corresponding Anthropic result blocks.
    if !hosted_web_search_names.is_empty() && !is_codex_oauth {
        result["include"] = json!([WEB_SEARCH_SOURCES_MARKER]);
    }

    // Inject prompt_cache_key for improved cache routing on OpenAI-compatible endpoints
    if let Some(key) = cache_key {
        result["prompt_cache_key"] = json!(key);
    }

    // Codex OAuth (ChatGPT Plus/Pro 反代) 特殊协议约束：
    // 整体依据：OpenAI 官方 codex-rs 的 `ResponsesApiRequest` 结构体
    // (codex-rs/codex-api/src/common.rs) 是 ChatGPT 反代后端的协议契约。
    // 任何不在该结构体里的字段都可能被 ChatGPT 后端以
    // "Unsupported parameter: ..." 400 拒绝；任何在结构体里的必填字段
    // 都需要在请求体里出现。
    //
    // 字段处理：
    // - store: 必须显式为 false（ChatGPT 消费级后端不允许服务端持久化）
    // - include: 必须包含 "reasoning.encrypted_content"，
    //   否则多轮 reasoning 中间态会丢失（无服务端状态 + 无加密回传 = 上下文断链）
    // - max_output_tokens / temperature / top_p: 必须删除
    //   （codex-rs 结构体根本没有这三个字段，OpenAI 自己的客户端不发它们）
    // - instructions / tools / parallel_tool_calls: 必填字段，缺则兜底默认值
    //   （cc-switch 的 transform 当前是"条件写入"，可能产生缺失）
    // - service_tier: 仅在 FAST mode 开启时写入 "priority"
    //   （与 OpenAI 官方 codex-rs 当前请求结构保持一致）
    // - stream: 必须永远 true（codex-rs 硬编码 true，且 cc-switch 的
    //   SSE 解析层只处理流式响应，强制覆盖避免客户端误传 false）
    if is_codex_oauth {
        result["store"] = json!(false);
        if codex_fast_mode {
            result["service_tier"] = json!("priority");
        }

        const REASONING_MARKER: &str = "reasoning.encrypted_content";
        let mut includes: Vec<Value> = body
            .get("include")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if !includes
            .iter()
            .any(|v| v.as_str() == Some(REASONING_MARKER))
        {
            includes.push(json!(REASONING_MARKER));
        }
        if !hosted_web_search_names.is_empty()
            && !includes
                .iter()
                .any(|v| v.as_str() == Some(WEB_SEARCH_SOURCES_MARKER))
        {
            includes.push(json!(WEB_SEARCH_SOURCES_MARKER));
        }
        result["include"] = json!(includes);

        if let Some(obj) = result.as_object_mut() {
            // —— 删除 ChatGPT 反代不接受的字段 ——
            obj.remove("max_output_tokens");
            obj.remove("temperature");
            obj.remove("top_p");

            // —— 兜底必填字段（or_insert：客户端送了什么就保留，否则注入默认值）——
            obj.entry("instructions".to_string()).or_insert(json!(""));
            obj.entry("tools".to_string()).or_insert(json!([]));
            // Anthropic 默认允许并行工具调用。优先保留上方已转换的显式设置，
            // 否则允许支持该能力的 Codex 模型在同一响应中批量调用独立工具。
            obj.entry("parallel_tool_calls".to_string())
                .or_insert(json!(true));

            // —— 强制覆盖 stream = true ——
            // 即便客户端误传 stream:false 也要覆盖，因为 codex-rs 永远 true，
            // 且 cc-switch SSE 解析层只支持流式响应。
            obj.insert("stream".to_string(), json!(true));
        }
    }

    Ok(result)
}

fn map_tool_choice_to_responses(
    tool_choice: &Value,
    hosted_web_search_names: &HashSet<String>,
    is_codex_oauth: bool,
) -> Value {
    match tool_choice {
        Value::String(_) => tool_choice.clone(),
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            // Anthropic "any" means at least one tool call is required
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            // Anthropic forced tool -> Responses function tool selector
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if hosted_web_search_names.contains(name) {
                    // The ChatGPT Codex backend's canonical request schema accepts a
                    // string tool_choice. The request transform filters the outbound
                    // list to this dynamically named hosted tool, so `required`
                    // preserves the forced-tool behavior.
                    if is_codex_oauth {
                        json!("required")
                    } else {
                        json!({"type": "web_search"})
                    }
                } else {
                    json!({
                        "type": "function",
                        "name": name
                    })
                }
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

/// xAI Responses 的 `ModelToolChoice` 不接受 `{"type":"web_search"}`。
/// 强制托管搜索时改成 `"required"`，并去掉其它工具，否则 required 可能落到本地函数上。
/// 字符串选择和函数选择不动。没有存活的 web_search 工具时删掉这条 choice，避免空工具列表配 required。
pub(crate) fn rewrite_xai_hosted_web_search_tool_choice(body: &mut Value) -> bool {
    let is_hosted_choice = body
        .get("tool_choice")
        .and_then(Value::as_object)
        .and_then(|choice| choice.get("type"))
        .and_then(Value::as_str)
        == Some("web_search");
    if !is_hosted_choice {
        return false;
    }
    let Some(obj) = body.as_object_mut() else {
        return false;
    };
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        tools.retain(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search"));
    }
    let has_search = obj
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_search {
        obj.insert("tool_choice".to_string(), json!("required"));
    } else {
        obj.remove("tool_choice");
        obj.remove("tools");
    }
    true
}

pub(crate) fn map_responses_stop_reason(
    status: Option<&str>,
    has_tool_use: bool,
    incomplete_reason: Option<&str>,
) -> Option<&'static str> {
    status.map(|s| match s {
        "completed" if has_tool_use => "tool_use",
        "incomplete"
            if matches!(
                incomplete_reason,
                Some("max_output_tokens") | Some("max_tokens")
            ) || incomplete_reason.is_none() =>
        {
            "max_tokens"
        }
        "incomplete" => "end_turn",
        _ => "end_turn",
    })
}

fn responses_error_message(body: &Value, fallback: &str) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("error").and_then(Value::as_str))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn validate_responses_terminal_status(body: &Value) -> Result<(), ProxyError> {
    let status = body.get("status").and_then(Value::as_str);
    let has_error = body.get("error").is_some_and(|error| !error.is_null());

    match status {
        Some("failed") => Err(ProxyError::TransformError(format!(
            "Responses upstream failed: {}",
            responses_error_message(body, "response generation failed")
        ))),
        Some("cancelled") => Err(ProxyError::TransformError(format!(
            "Responses upstream cancelled the response: {}",
            responses_error_message(body, "response generation was cancelled")
        ))),
        _ if has_error => Err(ProxyError::TransformError(format!(
            "Responses upstream returned an error envelope: {}",
            responses_error_message(body, "unknown upstream error")
        ))),
        _ => Ok(()),
    }
}

/// Build Anthropic-style usage JSON from Responses API usage, including cache tokens.
///
/// **Robustness Features**:
/// - Handles null, missing, empty objects, and partial objects gracefully
/// - Supports OpenAI field name variants (prompt_tokens/completion_tokens) as fallbacks
/// - Always returns valid structure: {"input_tokens": N, "output_tokens": N}
/// - Preserves cache token fields even when input/output tokens are missing
///
/// **Field Name Resolution Priority**:
/// 1. input_tokens: Anthropic `input_tokens` → OpenAI `prompt_tokens` → default 0
/// 2. output_tokens: Anthropic `output_tokens` → OpenAI `completion_tokens` → default 0
/// 3. cache_read_input_tokens: Direct field → nested input_tokens_details.cached_tokens → prompt_tokens_details.cached_tokens
/// 4. cache_creation_input_tokens: Direct field → nested
///    input_tokens_details.cache_write_tokens → prompt_tokens_details.cache_write_tokens
///
/// **Cache Token Priority Order**:
/// 1. OpenAI nested details (`cached_tokens`, `cache_write_tokens`) as initial values
/// 2. Direct Anthropic-style fields (`cache_read_input_tokens`, `cache_creation_input_tokens`) override if present
///
/// **Logging**:
/// - Warns on empty objects {} or partial objects (only one field present)
/// - Debug logs when using OpenAI field name fallbacks
pub(crate) fn build_anthropic_usage_from_responses(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if !v.is_null() && v.is_object() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0
            })
        }
    };

    // Detect empty object {} and log warning
    if u.as_object().map(|obj| obj.is_empty()).unwrap_or(false) {
        log::warn!("[Responses] Empty usage object received, using defaults");
        return json!({
            "input_tokens": 0,
            "output_tokens": 0
        });
    }

    // Extract input_tokens with OpenAI field name fallback
    // Priority: input_tokens (Anthropic) → prompt_tokens (OpenAI) → 0
    let input = u
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            let prompt_tokens = u.get("prompt_tokens").and_then(|v| v.as_u64());
            if prompt_tokens.is_some() {
                log::debug!(
                    "[Responses] Using OpenAI field name fallback 'prompt_tokens' for input_tokens"
                );
            }
            prompt_tokens
        })
        .unwrap_or(0);

    // Extract output_tokens with OpenAI field name fallback
    // Priority: output_tokens (Anthropic) → completion_tokens (OpenAI) → 0
    let output = u.get("output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            let completion_tokens = u.get("completion_tokens").and_then(|v| v.as_u64());
            if completion_tokens.is_some() {
                log::debug!("[Responses] Using OpenAI field name fallback 'completion_tokens' for output_tokens");
            }
            completion_tokens
        })
        .unwrap_or(0);

    // Log if only one field present (partial object). Streaming chunks legitimately
    // arrive with partial usage, so this stays at debug level to avoid noise.
    if (input == 0 && output > 0) || (input > 0 && output == 0) {
        log::debug!("[Responses] Partial usage object: {:?}", u);
    }

    let mut result = json!({
        "input_tokens": input,
        "output_tokens": output
    });

    // Step 1: OpenAI nested details as fallback for cache tokens
    // OpenAI Responses API: input_tokens_details.cached_tokens
    if let Some(cached) = u
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
    {
        result["cache_read_input_tokens"] = json!(cached);
    }
    // OpenAI standard: prompt_tokens_details.cached_tokens
    if let Some(cached) = u
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
    {
        if result.get("cache_read_input_tokens").is_none() {
            result["cache_read_input_tokens"] = json!(cached);
        }
    }
    // GPT-5.6+ reports cache writes in the nested OpenAI token-details object.
    // Treat writes as Anthropic cache creation so the downstream client and
    // billing layer can distinguish them from fresh input.
    let nested_cache_write = u
        .pointer("/input_tokens_details/cache_write_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            u.pointer("/prompt_tokens_details/cache_write_tokens")
                .and_then(|v| v.as_u64())
        });
    if let Some(cache_write) = nested_cache_write {
        result["cache_creation_input_tokens"] = json!(cache_write);
    }

    // Step 2: Direct Anthropic-style fields override (authoritative if present)
    // These preserve cache tokens even if input/output_tokens are missing
    if let Some(v) = u.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = v.clone();
    }
    if let Some(v) = u.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = v.clone();
    }
    if let Some(v) = u.get("cache_creation") {
        result["cache_creation"] = v.clone();
    }

    // OpenAI/Responses 的 input(prompt_tokens/input_tokens)含缓存命中，Anthropic input_tokens 不含
    // → 减去 cache_read 与 cache_creation，使其成为 fresh input。本函数在计量意义上是 claude 专属
    // （Codex Responses 透传走 from_codex_response_*，不调用本函数），故可安全在此扣减。三桶互斥，
    // 恒等：input + cache_read + cache_creation == 上游 input(inclusive)。与 build_anthropic_usage_json
    // (#2774) 及 transform_gemini 的 saturating_sub 对称；一处同时覆盖非流式与流式(streaming_responses)。
    let cached = result
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = result
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if cached > 0 || cache_creation > 0 {
        result["input_tokens"] = json!(input.saturating_sub(cached).saturating_sub(cache_creation));
    }

    result
}

/// 将 Anthropic messages 数组转换为 Responses API input 数组
///
/// 核心转换逻辑：
/// - user/assistant 的 text 内容 → 对应 role 的 message item
/// - tool_use 从 assistant message 中"提升"为独立的 function_call item
/// - tool_result 从 user message 中"提升"为独立的 function_call_output item
/// - hosted WebSearch call/result blocks → restore one Responses web_search_call item
/// - bridge-owned thinking blocks → restore the original Responses reasoning item
/// - unrelated native thinking blocks → 丢弃
fn convert_messages_to_input(messages: &[Value]) -> Result<Vec<Value>, ProxyError> {
    let mut input = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = msg.get("content");
        let message_input_start = input.len();

        match content {
            // 字符串内容
            Some(Value::String(text)) => {
                let content_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                input.push(json!({
                    "role": role,
                    "content": [{ "type": content_type, "text": text }]
                }));
            }

            // 数组内容（多模态/工具调用）
            Some(Value::Array(blocks)) => {
                let mut message_content = Vec::new();
                let hosted_web_search_results: HashMap<&str, &Value> = blocks
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
                    })
                    .filter_map(|block| {
                        block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(|id| (id, block))
                    })
                    .collect();

                for block in blocks {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let content_type = if role == "assistant" {
                                    "output_text"
                                } else {
                                    "input_text"
                                };
                                // OpenAI Responses API does not accept Anthropic cache_control
                                // under input[].content[].
                                message_content.push(json!({ "type": content_type, "text": text }));
                            }
                        }

                        "image" => {
                            if let Some(image) = anthropic_image_to_responses_part(block) {
                                message_content.push(image);
                            } else {
                                log::warn!(
                                    "[Responses] Unsupported or invalid Anthropic image block"
                                );
                            }
                        }

                        "document" => {
                            if let Some(file) = anthropic_document_to_responses_part(block) {
                                message_content.push(file);
                            } else {
                                log::warn!(
                                    "[Responses] Unsupported or invalid Anthropic document block"
                                );
                            }
                        }

                        "tool_use" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call item
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let arguments = block.get("input").cloned().unwrap_or(json!({}));

                            input.push(json!({
                                "type": "function_call",
                                "call_id": id,
                                "name": name,
                                "arguments": canonical_json_string(&arguments)
                            }));
                        }

                        "tool_result" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call_output item
                            let call_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("");
                            let output = anthropic_tool_result_to_responses_output(block);

                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output
                            }));
                        }

                        "server_tool_use" => {
                            let tool_use_id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .filter(|id| !id.is_empty());
                            let web_search_call = tool_use_id
                                .and_then(|id| hosted_web_search_results.get(id))
                                .and_then(|result| {
                                    responses_web_search_call_from_anthropic_blocks(block, result)
                                });
                            if let Some(web_search_call) = web_search_call {
                                if !message_content.is_empty() {
                                    input.push(json!({
                                        "role": role,
                                        "content": message_content.clone()
                                    }));
                                    message_content.clear();
                                }
                                input.push(web_search_call);
                            }
                        }

                        // A Responses web_search_call embeds the hosted result
                        // sources in action.sources, so the paired result block
                        // is consumed together with server_tool_use above.
                        "web_search_tool_result" => {}

                        "thinking" | "redacted_thinking" => {
                            if let Some(reasoning_item) =
                                openai_reasoning_item_from_anthropic_block(block)
                            {
                                if !message_content.is_empty() {
                                    input.push(json!({
                                        "role": role,
                                        "content": message_content.clone()
                                    }));
                                    message_content.clear();
                                }
                                input.push(reasoning_item);
                            }
                        }

                        _ => {}
                    }
                }

                // 刷新剩余的消息内容
                if !message_content.is_empty() {
                    input.push(json!({
                        "role": role,
                        "content": message_content
                    }));
                }
            }

            _ => {
                // 无内容或 null
                input.push(json!({ "role": role }));
            }
        }

        // A replayed reasoning item is only valid when the same assistant
        // generation also contains a following message/function call item.
        // Reasoning-only incomplete turns otherwise brick the next request with
        // "reasoning item ... without its required following item".
        if role == "assistant" {
            let mut has_generated_follower = false;
            for index in (message_input_start..input.len()).rev() {
                let item_type = input[index].get("type").and_then(Value::as_str);
                let is_assistant_message =
                    input[index].get("role").and_then(Value::as_str) == Some("assistant");
                if item_type == Some("reasoning") {
                    if !has_generated_follower {
                        input.remove(index);
                    }
                } else if matches!(item_type, Some("function_call" | "web_search_call"))
                    || is_assistant_message
                {
                    has_generated_follower = true;
                }
            }
        }
    }

    Ok(input)
}

pub(crate) fn responses_to_anthropic_with_web_search_options(
    body: Value,
    hosted_web_search_name: Option<&str>,
    max_web_search_uses: Option<u64>,
) -> Result<Value, ProxyError> {
    // A Responses failure can arrive inside an HTTP 2xx response object. Reject it
    // before looking at `output`; otherwise `{status:"failed", output:[]}` becomes
    // a successful empty Anthropic `end_turn` and hides the upstream error.
    validate_responses_terminal_status(&body)?;

    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| ProxyError::TransformError("No output in response".to_string()))?;

    let mut content = Vec::new();
    let response_completed = body.get("status").and_then(Value::as_str) == Some("completed");
    let hosted_web_search_name = hosted_web_search_name
        .filter(|name| !name.is_empty())
        .unwrap_or("web_search");
    let all_web_search_indices: Vec<usize> = output
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            (item.get("type").and_then(Value::as_str) == Some("web_search_call")).then_some(index)
        })
        .collect();
    let max_web_search_uses =
        max_web_search_uses.map(|limit| usize::try_from(limit).unwrap_or(usize::MAX));
    let retained_web_search_count = max_web_search_uses
        .map(|limit| limit.saturating_add(1))
        .unwrap_or(usize::MAX);
    let web_search_indices: Vec<usize> = all_web_search_indices
        .iter()
        .copied()
        .take(retained_web_search_count)
        .collect();
    let web_search_ordinal_by_index: HashMap<usize, usize> = web_search_indices
        .iter()
        .enumerate()
        .map(|(ordinal, output_index)| (*output_index, ordinal))
        .collect();
    let web_search_limit_exceeded_index =
        max_web_search_uses.and_then(|limit| all_web_search_indices.get(limit).copied());
    let is_web_search_limit_error = |output_index: usize| {
        max_web_search_uses.is_some_and(|limit| {
            web_search_ordinal_by_index
                .get(&output_index)
                .is_some_and(|ordinal| *ordinal >= limit)
        })
    };
    let web_search_result_error = |output_index: usize| {
        if is_web_search_limit_error(output_index) {
            Some(web_search_max_uses_exceeded_error())
        } else {
            web_search_tool_result_error(&output[output_index])
        }
    };
    let last_web_search_index = web_search_indices
        .iter()
        .rev()
        .copied()
        .find(|index| web_search_result_error(*index).is_none());
    let mut web_search_results_by_index = HashMap::new();
    for &output_index in &web_search_indices {
        if web_search_result_error(output_index).is_some() {
            web_search_results_by_index.insert(output_index, Vec::new());
            continue;
        }
        let results = web_search_results_from_action(&output[output_index]);
        web_search_results_by_index.insert(output_index, results);
    }

    let mut terminal_web_search_results = Vec::new();
    let mut seen_web_search_urls = HashSet::new();
    for (output_index, item) in output.iter().enumerate() {
        if web_search_limit_exceeded_index.is_some_and(|limit_index| output_index > limit_index) {
            continue;
        }
        for result in web_search_results_from_output_item(item) {
            let Some(url) = result.get("url").and_then(Value::as_str) else {
                continue;
            };
            if seen_web_search_urls.insert(url.to_string()) {
                terminal_web_search_results.push(result);
            }
        }
    }
    // action.sources often contains only URLs, while final-message annotations
    // carry the human-readable titles. Enrich matching per-call results before
    // removing already-attributed citations.
    for (&output_index, results) in &mut web_search_results_by_index {
        if web_search_result_error(output_index).is_none() {
            merge_web_search_result_metadata(results, &terminal_web_search_results);
        }
    }
    let attributed_web_search_urls: HashSet<String> = web_search_results_by_index
        .iter()
        .filter(|(output_index, _)| web_search_result_error(**output_index).is_none())
        .flat_map(|(_, results)| results)
        .filter_map(|result| {
            result
                .get("url")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();
    let unassigned_web_search_results = terminal_web_search_results
        .into_iter()
        .filter(|result| {
            result
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| !attributed_web_search_urls.contains(url))
        })
        .collect::<Vec<_>>();
    // Compatible gateways may ignore the requested action.sources include.
    // Message annotations do not identify which call produced them, so keep
    // every call/result pair valid and attach only the unassigned citations to
    // the final call as a deterministic best-effort fallback.
    if let Some(last_web_search_index) = last_web_search_index {
        let results = web_search_results_by_index
            .entry(last_web_search_index)
            .or_insert_with(Vec::new);
        let mut seen_last_urls: HashSet<String> = results
            .iter()
            .filter_map(|result| result.get("url").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();
        for result in unassigned_web_search_results {
            let Some(url) = result.get("url").and_then(Value::as_str) else {
                continue;
            };
            if seen_last_urls.insert(url.to_string()) {
                results.push(result);
            }
        }
    }

    let mut has_tool_use = false;
    let mut web_search_count = 0_u64;
    for (output_index, item) in output.iter().enumerate() {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if web_search_limit_exceeded_index.is_some_and(|limit_index| output_index > limit_index) {
            continue;
        }

        match item_type {
            "message" => {
                if let Some(msg_content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in msg_content {
                        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if block_type == "output_text" {
                            if let Some(text) = output_text_with_url_citations(block) {
                                content.push(json!({"type": "text", "text": text}));
                            }
                        } else if block_type == "refusal" {
                            if let Some(refusal) = block.get("refusal").and_then(|t| t.as_str()) {
                                if !refusal.is_empty() {
                                    content.push(json!({"type": "text", "text": refusal}));
                                }
                            }
                        }
                    }
                }
            }

            "function_call" => {
                let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args_str = item
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let input: Value = if args_str.trim().is_empty() {
                    json!({})
                } else {
                    match serde_json::from_str(args_str) {
                        Ok(value) => value,
                        Err(error) if !response_completed => {
                            log::warn!(
                                "[Responses] Replacing incomplete function_call '{name}' arguments with an empty object: {error}"
                            );
                            json!({})
                        }
                        Err(error) => {
                            return Err(ProxyError::TransformError(format!(
                                "Invalid function_call arguments for '{name}': {error}"
                            )))
                        }
                    }
                };
                if !input.is_object() {
                    if !response_completed {
                        log::warn!(
                            "[Responses] Replacing incomplete function_call '{name}' non-object arguments with an empty object"
                        );
                        content.push(json!({
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": {}
                        }));
                        has_tool_use = true;
                        continue;
                    }
                    return Err(ProxyError::TransformError(format!(
                        "Function call arguments for '{name}' must be a JSON object"
                    )));
                }
                let input = sanitize_anthropic_tool_use_input(name, input);

                content.push(json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }

            "web_search_call" => {
                let Some(&web_search_ordinal) = web_search_ordinal_by_index.get(&output_index)
                else {
                    continue;
                };
                let limit_exceeded =
                    max_web_search_uses.is_some_and(|limit| web_search_ordinal >= limit);
                if !limit_exceeded {
                    web_search_count += 1;
                }
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("ws_{output_index}"));
                content.push(json!({
                    "type": "server_tool_use",
                    "id": id,
                    "name": hosted_web_search_name,
                    "input": web_search_action_input(item),
                    "caller": {"type": "direct"}
                }));

                let results = web_search_results_by_index
                    .remove(&output_index)
                    .unwrap_or_default();
                let result_content = if limit_exceeded {
                    web_search_max_uses_exceeded_error()
                } else {
                    web_search_tool_result_error(item).unwrap_or(Value::Array(results))
                };
                content.push(json!({
                    "type": "web_search_tool_result",
                    "tool_use_id": id,
                    "content": result_content,
                    "caller": {"type": "direct"}
                }));
            }

            "reasoning" => {
                if let Some(block) = anthropic_block_from_openai_reasoning_item(item) {
                    content.push(block);
                }
            }

            _ => {}
        }
    }

    // status → stop_reason
    let stop_reason = map_responses_stop_reason(
        body.get("status").and_then(|s| s.as_str()),
        has_tool_use,
        body.pointer("/incomplete_details/reason")
            .and_then(|r| r.as_str()),
    );

    let mut usage_json = build_anthropic_usage_from_responses(body.get("usage"));
    if web_search_count > 0 {
        usage_json["server_tool_use"] = json!({
            "web_search_requests": web_search_count
        });
    }

    let result = json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage_json
    });

    Ok(result)
}

#[cfg(test)]
mod tests;
