//! Shared media handling for tool outputs.
//!
//! Responses and Anthropic tool outputs may carry structured media blocks.
//! Chat Completions tool messages are text-only, so protocol bridges extract
//! those blocks and re-emit them in a synthetic user message. The media
//! sanitizer reuses the same recognition and traversal rules when it needs to
//! remove images for a text-only upstream.

use crate::proxy::json_canonical::canonical_json_string;
use serde_json::{json, Map, Value};

pub(crate) const WHOLE_DATA_URL_MIN_BYTES: usize = 8 * 1024;
pub(crate) const TOOL_RESULT_MEDIA_MOVED_MARKER: &str =
    "[cc-switch: tool result media moved to the following user message]";
pub(crate) const TOOL_RESULT_MEDIA_ATTACHED_MARKER: &str =
    "[cc-switch: tool result media attached as native media]";
const BASE64ISH_MIN_BYTES: usize = 16 * 1024;
const MAX_MEDIA_TRAVERSAL_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolMediaScope {
    /// Used by the existing image-capability sanitizer and its retry path.
    ImagesOnly,
    /// Used by Gemini Native `generateContent`, whose existing bridge only
    /// promises inline base64 image input. Remote URLs and malformed data URLs
    /// must stay in the legacy tool-result representation.
    InlineImagesOnly,
    /// Used by Chat conversion bridges, where user messages can carry all
    /// currently mapped Chat input modalities.
    AllSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolMediaKind {
    Image,
    File,
    Audio,
}

pub(crate) struct ChatToolOutputMediaPlan {
    pub(crate) tool_content: String,
    pub(crate) media_parts: Vec<Value>,
}

impl ToolMediaScope {
    fn allows(self, kind: ToolMediaKind) -> bool {
        matches!(kind, ToolMediaKind::Image) || matches!(self, Self::AllSupported)
    }

    fn accepts_chat_part(self, part: &Value) -> bool {
        !matches!(self, Self::InlineImagesOnly) || chat_image_part_has_inline_data(part)
    }
}

/// Build a Chat-compatible tool-output plan without changing no-media output.
///
/// Scalar strings remain scalar strings after replacement. This matters for a
/// raw image data URL and for JSON encoded inside a tool-output string: adding
/// another layer of JSON string quotes would change what the model sees.
pub(crate) fn plan_chat_tool_output_media(mut output: Value) -> Option<ChatToolOutputMediaPlan> {
    let output_was_string = output.is_string();
    let replacement_block = json!({
        "type": "text",
        "text": TOOL_RESULT_MEDIA_MOVED_MARKER
    });
    let mut media_parts = Vec::new();
    let replaced = strip_and_clamp_media_from_tool_value(
        &mut output,
        &mut media_parts,
        ToolMediaScope::AllSupported,
        &replacement_block,
        TOOL_RESULT_MEDIA_MOVED_MARKER,
    );
    if replaced == 0 {
        return None;
    }

    let tool_content = if output_was_string {
        output.as_str().unwrap_or_default().to_string()
    } else {
        canonical_json_string(&output)
    };

    Some(ChatToolOutputMediaPlan {
        tool_content,
        media_parts,
    })
}

pub(crate) fn queue_chat_tool_output_media(
    pending_media: &mut Vec<Value>,
    call_id: &str,
    media_parts: Vec<Value>,
) {
    if media_parts.is_empty() {
        return;
    }

    pending_media.push(json!({
        "type": "text",
        "text": format!("[cc-switch: media output of tool call {call_id}]")
    }));
    pending_media.extend(media_parts);
}

pub(crate) fn flush_pending_chat_tool_media(
    messages: &mut Vec<Value>,
    pending_media: &mut Vec<Value>,
) {
    if pending_media.is_empty() {
        return;
    }

    messages.push(json!({
        "role": "user",
        "content": std::mem::take(pending_media)
    }));
}

/// Convert one recognized tool media block to a Chat user content part. This
/// is the single shape-recognition entry point used by extraction.
pub(crate) fn chat_media_part_from_tool_part(part: &Value, scope: ToolMediaScope) -> Option<Value> {
    let kind = tool_media_kind(part)?;
    if !scope.allows(kind) {
        return None;
    }

    let chat_part = match kind {
        ToolMediaKind::Image => chat_image_part(part),
        ToolMediaKind::File => chat_file_from_input_file(part).map(|file| {
            json!({
                "type": "file",
                "file": file
            })
        }),
        ToolMediaKind::Audio => part.get("input_audio").map(|input_audio| {
            json!({
                "type": "input_audio",
                "input_audio": input_audio.clone()
            })
        }),
    }?;

    scope.accepts_chat_part(&chat_part).then_some(chat_part)
}

/// Map a Responses `input_file` block to the Chat file payload. Kept here so
/// top-level content and tool-output extraction share the exact same rules.
pub(crate) fn chat_file_from_input_file(part: &Value) -> Option<Value> {
    let mut file = Map::new();
    let has_supported_file_ref = part.get("file_id").is_some() || part.get("file_data").is_some();
    if !has_supported_file_ref {
        return None;
    }

    for key in ["file_id", "file_data", "filename"] {
        if let Some(value) = part.get(key) {
            file.insert(key.to_string(), value.clone());
        }
    }
    Some(Value::Object(file))
}

/// Recognize a complete image data URL stored as a scalar string.
///
/// Only whole-string matches are accepted. Embedded data URLs in HTML/CSS/SVG
/// source are deliberately left alone. Small values remain text as well, which
/// preserves workflows that intentionally inspect tiny inline icons.
pub(crate) fn whole_string_image_data_url(value: &str) -> Option<Value> {
    let trimmed = value.trim();
    if trimmed.len() < WHOLE_DATA_URL_MIN_BYTES || !is_image_base64_data_url(trimmed) {
        return None;
    }

    Some(json!({
        "type": "image_url",
        "image_url": {
            "url": trimmed
        }
    }))
}

/// Read-only media detection using the same shape classifier and recursive
/// boundaries as [`strip_media_from_tool_value`].
pub(crate) fn tool_output_contains_media(value: &Value, scope: ToolMediaScope) -> bool {
    tool_output_contains_media_at_depth(value, scope, 0)
}

/// Extract recognized media blocks and replace them in-place.
///
/// `replacement_block` is used for structured array/object parts. A scalar
/// string that is itself a complete image data URL uses `replacement_text`, so
/// an originally plain string stays a plain string. Parseable JSON strings are
/// recursively transformed and canonicalized back into a string only when a
/// replacement actually occurred.
pub(crate) fn strip_media_from_tool_value(
    value: &mut Value,
    media_parts: &mut Vec<Value>,
    scope: ToolMediaScope,
    replacement_block: &Value,
    replacement_text: &str,
) -> usize {
    strip_media_from_tool_value_at_depth(
        value,
        media_parts,
        scope,
        replacement_block,
        replacement_text,
        false,
        0,
    )
}

/// Extract media and clamp residual large data/base64 scalars on media-bearing
/// outputs. Parseable JSON strings are clamped while still represented as a
/// JSON tree, before they are canonicalized back into their original string
/// container.
pub(crate) fn strip_and_clamp_media_from_tool_value(
    value: &mut Value,
    media_parts: &mut Vec<Value>,
    scope: ToolMediaScope,
    replacement_block: &Value,
    replacement_text: &str,
) -> usize {
    let replaced = strip_media_from_tool_value_at_depth(
        value,
        media_parts,
        scope,
        replacement_block,
        replacement_text,
        true,
        0,
    );
    if replaced > 0 {
        clamp_base64ish_strings(value);
    }
    replaced
}

/// Remove residual data/base64 payloads only after a tool output has already
/// been positively identified as media-bearing. Ordinary long text is kept.
pub(crate) fn clamp_base64ish_strings(value: &mut Value) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            let should_omit = (trimmed.len() >= WHOLE_DATA_URL_MIN_BYTES
                && trimmed
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:")))
                || looks_like_base64_payload(trimmed);
            if should_omit {
                let byte_len = text.len();
                *text = format!("[cc-switch: omitted {byte_len} bytes]");
            }
        }
        Value::Array(items) => {
            for item in items {
                clamp_base64ish_strings(item);
            }
        }
        Value::Object(object) => {
            for nested in object.values_mut() {
                clamp_base64ish_strings(nested);
            }
        }
        _ => {}
    }
}

fn tool_output_contains_media_at_depth(value: &Value, scope: ToolMediaScope, depth: usize) -> bool {
    if depth > MAX_MEDIA_TRAVERSAL_DEPTH {
        return false;
    }

    match value {
        Value::String(text) => {
            if scope.allows(ToolMediaKind::Image) && whole_string_image_data_url(text).is_some() {
                return true;
            }

            let trimmed = text.trim();
            if trimmed.is_empty() {
                return false;
            }
            serde_json::from_str::<Value>(trimmed)
                .ok()
                .is_some_and(|parsed| {
                    tool_output_contains_media_at_depth(&parsed, scope, depth + 1)
                })
        }
        Value::Array(items) => items
            .iter()
            .any(|item| tool_output_contains_media_at_depth(item, scope, depth + 1)),
        Value::Object(object) => {
            if chat_media_part_from_tool_part(value, scope).is_some() {
                return true;
            }

            object.get("content").is_some_and(|content| {
                tool_output_contains_media_at_depth(content, scope, depth + 1)
            })
        }
        _ => false,
    }
}

fn strip_media_from_tool_value_at_depth(
    value: &mut Value,
    media_parts: &mut Vec<Value>,
    scope: ToolMediaScope,
    replacement_block: &Value,
    replacement_text: &str,
    clamp_parsed_strings: bool,
    depth: usize,
) -> usize {
    if depth > MAX_MEDIA_TRAVERSAL_DEPTH {
        return 0;
    }

    match value {
        Value::String(text) => {
            if scope.allows(ToolMediaKind::Image) {
                if let Some(media_part) = whole_string_image_data_url(text) {
                    media_parts.push(media_part);
                    *text = replacement_text.to_string();
                    return 1;
                }
            }

            let trimmed = text.trim();
            if trimmed.is_empty() {
                return 0;
            }
            let Ok(mut parsed) = serde_json::from_str::<Value>(trimmed) else {
                return 0;
            };
            let replaced = strip_media_from_tool_value_at_depth(
                &mut parsed,
                media_parts,
                scope,
                replacement_block,
                replacement_text,
                clamp_parsed_strings,
                depth + 1,
            );
            if replaced > 0 {
                if clamp_parsed_strings {
                    clamp_base64ish_strings(&mut parsed);
                }
                *text = canonical_json_string(&parsed);
            }
            replaced
        }
        Value::Array(items) => items
            .iter_mut()
            .map(|item| {
                strip_media_from_tool_value_at_depth(
                    item,
                    media_parts,
                    scope,
                    replacement_block,
                    replacement_text,
                    clamp_parsed_strings,
                    depth + 1,
                )
            })
            .sum(),
        Value::Object(_) => {
            if let Some(media_part) = chat_media_part_from_tool_part(value, scope) {
                media_parts.push(media_part);
                *value = replacement_block.clone();
                return 1;
            }

            value
                .as_object_mut()
                .expect("object match arm must remain an object")
                .get_mut("content")
                .map(|content| {
                    strip_media_from_tool_value_at_depth(
                        content,
                        media_parts,
                        scope,
                        replacement_block,
                        replacement_text,
                        clamp_parsed_strings,
                        depth + 1,
                    )
                })
                .unwrap_or(0)
        }
        _ => 0,
    }
}

fn tool_media_kind(part: &Value) -> Option<ToolMediaKind> {
    let object = part.as_object()?;
    let part_type = object.get("type").and_then(Value::as_str);

    match part_type {
        Some("input_image" | "image_url") if normalized_image_url(part).is_some() => {
            Some(ToolMediaKind::Image)
        }
        Some("input_file") if part.get("file_id").is_some() || part.get("file_data").is_some() => {
            Some(ToolMediaKind::File)
        }
        Some("input_audio") if part.get("input_audio").is_some_and(Value::is_object) => {
            Some(ToolMediaKind::Audio)
        }
        Some("image") if typed_image_has_payload(part) => Some(ToolMediaKind::Image),
        None if loose_data_image_url(part).is_some() => Some(ToolMediaKind::Image),
        _ => None,
    }
}

fn chat_image_part(part: &Value) -> Option<Value> {
    match part.get("type").and_then(Value::as_str) {
        Some("input_image" | "image_url") => normalized_image_url(part).map(image_url_content_part),
        Some("image") => typed_image_url(part).map(image_url_content_part),
        None => loose_data_image_url(part).map(image_url_content_part),
        _ => None,
    }
}

fn normalized_image_url(part: &Value) -> Option<Value> {
    let image_url = part.get("image_url")?;
    let mut object = match image_url {
        Value::String(url) if !url.trim().is_empty() => {
            let mut object = Map::new();
            object.insert("url".to_string(), Value::String(url.clone()));
            object
        }
        Value::Object(object)
            if object
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| !url.trim().is_empty()) =>
        {
            object.clone()
        }
        _ => return None,
    };
    merge_top_level_detail(part, &mut object);
    Some(Value::Object(object))
}

fn loose_data_image_url(part: &Value) -> Option<Value> {
    if part.get("type").is_some() {
        return None;
    }
    let normalized = normalized_image_url(part)?;
    let url = normalized.get("url").and_then(Value::as_str)?;
    if !url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return None;
    }
    Some(normalized)
}

fn typed_image_has_payload(part: &Value) -> bool {
    let Some(object) = part.as_object() else {
        return false;
    };

    if let Some(source) = object.get("source").and_then(Value::as_object) {
        if source_media_type_is_image(source) {
            let has_url = source
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| !url.trim().is_empty());
            let has_data = source
                .get("data")
                .and_then(Value::as_str)
                .is_some_and(|data| !data.is_empty());
            if has_url || has_data {
                return true;
            }
        }
    }

    object
        .get("data")
        .and_then(Value::as_str)
        .is_some_and(|data| !data.is_empty())
        && object
            .get("mimeType")
            .or_else(|| object.get("mime_type"))
            .and_then(Value::as_str)
            .is_some_and(is_image_mime_type)
}

fn typed_image_url(part: &Value) -> Option<Value> {
    let object = part.as_object()?;

    if let Some(source) = object.get("source").and_then(Value::as_object) {
        if !source_media_type_is_image(source) {
            return None;
        }

        if let Some(url) = source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
        {
            let mut image_url = Map::new();
            image_url.insert("url".to_string(), Value::String(url.to_string()));
            merge_top_level_detail(part, &mut image_url);
            return Some(Value::Object(image_url));
        }

        if let Some(data) = source
            .get("data")
            .and_then(Value::as_str)
            .filter(|data| !data.is_empty())
        {
            let media_type = source
                .get("media_type")
                .or_else(|| source.get("mime_type"))
                .or_else(|| source.get("mimeType"))
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            let url = if data
                .get(..11)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
            {
                data.to_string()
            } else {
                format!("data:{media_type};base64,{data}")
            };
            let mut image_url = Map::new();
            image_url.insert("url".to_string(), Value::String(url));
            merge_top_level_detail(part, &mut image_url);
            return Some(Value::Object(image_url));
        }
    }

    let data = object
        .get("data")
        .and_then(Value::as_str)
        .filter(|data| !data.is_empty())?;
    let media_type = object
        .get("mimeType")
        .or_else(|| object.get("mime_type"))
        .and_then(Value::as_str)
        .filter(|media_type| is_image_mime_type(media_type))?;
    let mut image_url = Map::new();
    image_url.insert(
        "url".to_string(),
        Value::String(format!("data:{media_type};base64,{data}")),
    );
    merge_top_level_detail(part, &mut image_url);
    Some(Value::Object(image_url))
}

fn image_url_content_part(image_url: Value) -> Value {
    let mut content_part = Map::new();
    content_part.insert("type".to_string(), Value::String("image_url".to_string()));
    content_part.insert("image_url".to_string(), image_url);
    Value::Object(content_part)
}

fn chat_image_part_has_inline_data(part: &Value) -> bool {
    part.pointer("/image_url/url")
        .and_then(Value::as_str)
        .is_some_and(|url| {
            let trimmed = url.trim();
            let Some(comma_index) = trimmed.find(',') else {
                return false;
            };
            comma_index + 1 < trimmed.len() && is_image_base64_data_url(trimmed)
        })
}

fn merge_top_level_detail(part: &Value, image_url: &mut Map<String, Value>) {
    if image_url.get("detail").is_none() {
        if let Some(detail) = part.get("detail") {
            image_url.insert("detail".to_string(), detail.clone());
        }
    }
}

fn source_media_type_is_image(source: &Map<String, Value>) -> bool {
    source
        .get("media_type")
        .or_else(|| source.get("mime_type"))
        .or_else(|| source.get("mimeType"))
        .and_then(Value::as_str)
        .is_none_or(is_image_mime_type)
}

fn is_image_mime_type(value: &str) -> bool {
    value
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
}

fn is_image_base64_data_url(value: &str) -> bool {
    let Some(comma_index) = value.find(',') else {
        return false;
    };
    let header = &value[..comma_index];
    let header = header.to_ascii_lowercase();
    header.starts_with("data:image/") && header.ends_with(";base64")
}

fn looks_like_base64_payload(value: &str) -> bool {
    if value.len() < BASE64ISH_MIN_BYTES {
        return false;
    }

    value
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'/' | b'='))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    fn large_image_data_url() -> String {
        format!(
            "data:image/png;base64,{}",
            "iVBORw0KGgoAAAANSUhEUgAAAAE".repeat(400)
        )
    }

    #[test]
    fn maps_input_image_and_merges_top_level_detail() {
        let part = json!({
            "type": "input_image",
            "image_url": "https://example.com/image.png",
            "detail": "high"
        });

        let mapped = chat_media_part_from_tool_part(&part, ToolMediaScope::AllSupported).unwrap();

        assert_eq!(mapped["type"], "image_url");
        assert_eq!(mapped["image_url"]["url"], "https://example.com/image.png");
        assert_eq!(mapped["image_url"]["detail"], "high");
    }

    #[test]
    fn maps_already_chat_shaped_image_url() {
        let part = json!({
            "type": "image_url",
            "image_url": {
                "url": "https://example.com/image.png",
                "detail": "low"
            },
            "cache_control": {"type": "ephemeral"},
            "prompt_cache_breakpoint": true
        });

        let mapped = chat_media_part_from_tool_part(&part, ToolMediaScope::AllSupported).unwrap();

        assert_eq!(
            mapped,
            json!({
                "type": "image_url",
                "image_url": {
                    "url": "https://example.com/image.png",
                    "detail": "low"
                }
            })
        );
    }

    #[test]
    fn maps_anthropic_and_mcp_image_shapes() {
        let anthropic = json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/jpeg",
                "data": "YWJj"
            }
        });
        let mcp = json!({
            "type": "image",
            "mimeType": "image/webp",
            "data": "ZGVm"
        });
        let anthropic_url = json!({
            "type": "image",
            "source": {
                "url": "https://example.com/anthropic.png"
            }
        });

        let anthropic =
            chat_media_part_from_tool_part(&anthropic, ToolMediaScope::AllSupported).unwrap();
        let mcp = chat_media_part_from_tool_part(&mcp, ToolMediaScope::AllSupported).unwrap();
        let anthropic_url =
            chat_media_part_from_tool_part(&anthropic_url, ToolMediaScope::AllSupported).unwrap();

        assert_eq!(anthropic["image_url"]["url"], "data:image/jpeg;base64,YWJj");
        assert_eq!(mcp["image_url"]["url"], "data:image/webp;base64,ZGVm");
        assert_eq!(
            anthropic_url["image_url"]["url"],
            "https://example.com/anthropic.png"
        );
    }

    #[test]
    fn maps_anthropic_source_data_when_optional_url_is_empty() {
        let part = json!({
            "type": "image",
            "source": {
                "url": "",
                "media_type": "image/png",
                "data": "YWJj"
            }
        });

        let mapped = chat_media_part_from_tool_part(&part, ToolMediaScope::AllSupported).unwrap();

        assert_eq!(mapped["image_url"]["url"], "data:image/png;base64,YWJj");
    }

    #[test]
    fn rejects_image_metadata_and_non_image_mcp_payloads() {
        let metadata = json!({"type": "image", "name": "cover"});
        let non_image = json!({
            "type": "image",
            "mimeType": "text/plain",
            "data": "aGVsbG8="
        });

        assert!(chat_media_part_from_tool_part(&metadata, ToolMediaScope::AllSupported).is_none());
        assert!(chat_media_part_from_tool_part(&non_image, ToolMediaScope::AllSupported).is_none());
    }

    #[test]
    fn loose_data_image_url_is_media_but_loose_remote_url_is_not() {
        let data = json!({
            "image_url": {
                "url": "data:application/octet-stream;base64,YWJj"
            }
        });
        let remote = json!({
            "image_url": {
                "url": "https://example.com/search-thumbnail.png"
            }
        });

        assert!(tool_output_contains_media(
            &data,
            ToolMediaScope::ImagesOnly
        ));
        assert!(!tool_output_contains_media(
            &remote,
            ToolMediaScope::ImagesOnly
        ));
    }

    #[test]
    fn inline_image_scope_rejects_remote_and_malformed_data_urls() {
        let inline = json!({
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64,YWJj"}
        });
        let remote = json!({
            "type": "image_url",
            "image_url": {"url": "https://example.com/image.png"}
        });
        let missing_base64 = json!({
            "type": "image_url",
            "image_url": {"url": "data:image/png,YWJj"}
        });
        let empty_data = json!({
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64,"}
        });

        assert!(tool_output_contains_media(
            &inline,
            ToolMediaScope::InlineImagesOnly
        ));
        assert!(!tool_output_contains_media(
            &remote,
            ToolMediaScope::InlineImagesOnly
        ));
        assert!(!tool_output_contains_media(
            &missing_base64,
            ToolMediaScope::InlineImagesOnly
        ));
        assert!(!tool_output_contains_media(
            &empty_data,
            ToolMediaScope::InlineImagesOnly
        ));
    }

    #[test]
    fn does_not_scan_embedded_data_urls_inside_plain_text() {
        let data_url = large_image_data_url();
        let mut value = json!(format!("<html><img src=\"{data_url}\"></html>"));
        let original = value.clone();
        let replacement = json!({"type": "text", "text": "moved"});
        let mut media = Vec::new();

        assert!(!tool_output_contains_media(
            &value,
            ToolMediaScope::AllSupported
        ));
        assert_eq!(
            strip_media_from_tool_value(
                &mut value,
                &mut media,
                ToolMediaScope::AllSupported,
                &replacement,
                "moved",
            ),
            0
        );
        assert!(media.is_empty());
        assert_eq!(value, original);
    }

    #[test]
    fn whole_string_data_url_respects_threshold() {
        let large = large_image_data_url();
        let small = "data:image/png;base64,YWJj";

        assert!(whole_string_image_data_url(&large).is_some());
        assert!(whole_string_image_data_url(small).is_none());
    }

    #[test]
    fn strips_media_from_json_string_and_nested_content() {
        let data_url = large_image_data_url();
        let mut value = Value::String(
            json!({
                "content": [
                    {"type": "input_text", "text": "caption"},
                    {"type": "input_image", "image_url": data_url}
                ]
            })
            .to_string(),
        );
        let replacement = json!({
            "type": "text",
            "text": "moved"
        });
        let mut media = Vec::new();

        let replaced = strip_media_from_tool_value(
            &mut value,
            &mut media,
            ToolMediaScope::AllSupported,
            &replacement,
            "moved",
        );

        assert_eq!(replaced, 1);
        assert_eq!(media.len(), 1);
        let serialized = value.as_str().unwrap();
        assert!(serialized.contains("\"text\":\"moved\""));
        assert!(!serialized.contains("iVBORw0KGgo"));
    }

    #[test]
    fn chat_plan_keeps_scalar_tool_strings_unquoted() {
        let raw_data_url = large_image_data_url();
        let raw_plan = plan_chat_tool_output_media(Value::String(raw_data_url.clone())).unwrap();
        assert_eq!(raw_plan.tool_content, TOOL_RESULT_MEDIA_MOVED_MARKER);
        assert_eq!(raw_plan.media_parts[0]["image_url"]["url"], raw_data_url);

        let encoded = json!({
            "content": [{
                "type": "input_image",
                "image_url": raw_data_url
            }]
        })
        .to_string();
        let encoded_plan = plan_chat_tool_output_media(Value::String(encoded)).unwrap();
        assert!(encoded_plan.tool_content.starts_with('{'));
        assert!(encoded_plan
            .tool_content
            .contains(TOOL_RESULT_MEDIA_MOVED_MARKER));
        assert!(!encoded_plan.tool_content.starts_with('"'));
    }

    #[test]
    fn chat_plan_clamps_residual_base64_inside_json_string_before_serializing() {
        let residual_base64 = "A".repeat(20_000);
        let encoded = json!({
            "content": [
                {
                    "type": "input_image",
                    "image_url": "data:image/png;base64,IMAGE_SENTINEL"
                },
                {
                    "type": "video",
                    "data": residual_base64
                }
            ]
        })
        .to_string();

        let plan = plan_chat_tool_output_media(Value::String(encoded)).unwrap();

        assert!(plan
            .tool_content
            .contains("[cc-switch: omitted 20000 bytes]"));
        assert!(!plan.tool_content.contains(&"A".repeat(64)));
        assert!(!plan.tool_content.contains("IMAGE_SENTINEL"));
        assert_eq!(plan.media_parts.len(), 1);
    }

    #[test]
    fn image_only_scope_ignores_file_and_audio() {
        let file = json!({"type": "input_file", "file_id": "file_1"});
        let audio = json!({
            "type": "input_audio",
            "input_audio": {"data": "YWJj", "format": "wav"}
        });

        assert!(!tool_output_contains_media(
            &file,
            ToolMediaScope::ImagesOnly
        ));
        assert!(!tool_output_contains_media(
            &audio,
            ToolMediaScope::ImagesOnly
        ));
        assert!(tool_output_contains_media(
            &file,
            ToolMediaScope::AllSupported
        ));
        assert!(tool_output_contains_media(
            &audio,
            ToolMediaScope::AllSupported
        ));
    }

    #[test]
    fn clamp_preserves_long_text_but_removes_data_and_base64_payloads() {
        let long_text = format!(
            "{} with spaces and punctuation!",
            "ordinary text ".repeat(9000)
        );
        let data_url = large_image_data_url();
        let bytes = (0_u8..=255).cycle().take(18_000).collect::<Vec<_>>();
        let base64 = STANDARD.encode(bytes);
        let mut value = json!({
            "text": long_text,
            "data_url": data_url,
            "raw": base64
        });

        clamp_base64ish_strings(&mut value);

        assert_eq!(value["text"], long_text);
        assert!(value["data_url"]
            .as_str()
            .unwrap()
            .starts_with("[cc-switch: omitted "));
        assert!(value["raw"]
            .as_str()
            .unwrap()
            .starts_with("[cc-switch: omitted "));
    }

    #[test]
    fn no_media_strip_is_byte_stable() {
        let mut value = json!({
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "image", "name": "business metadata"}
            ]
        });
        let before = canonical_json_string(&value);
        let replacement = json!({"type": "text", "text": "moved"});
        let mut media = Vec::new();

        let replaced = strip_media_from_tool_value(
            &mut value,
            &mut media,
            ToolMediaScope::AllSupported,
            &replacement,
            "moved",
        );

        assert_eq!(replaced, 0);
        assert!(media.is_empty());
        assert_eq!(canonical_json_string(&value), before);
    }
}
