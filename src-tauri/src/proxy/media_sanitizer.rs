#[cfg(test)]
use crate::model_capabilities::is_confirmed_text_only_model as confirmed_text_only_model;
use crate::model_capabilities::{image_input_capability_from_settings, ImageInputCapability};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::tool_media::{
    strip_media_from_tool_value, tool_output_contains_media, ToolMediaScope,
};
use serde_json::{json, Value};

pub const UNSUPPORTED_IMAGE_MARKER: &str = "[Unsupported Image]";

/// 图片预算：按文档顺序保留前 N 张，其余的标记替换。
///
/// 用于「请求整流：图片数量上限」——部分上游（如阶跃星辰）对单次请求的图片
/// 张数有硬上限，超限直接 400，而长会话里累积的截图很容易越过上限。
///
/// [`ImageBudget::keep`] 只在**遇到**图片时调用（而非替换时），因此计数与
/// 「这段对话里出现过多少张图」一致，与遍历路径无关。
struct ImageBudget {
    remaining: usize,
}

impl ImageBudget {
    /// 无上限（能力降级等既有路径用）。
    fn unlimited() -> Self {
        Self {
            remaining: usize::MAX,
        }
    }

    /// 消耗一次额度：仍有余量返回 true（保留该图片），否则 false（应替换）。
    fn keep(&mut self) -> bool {
        if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }
}

/// 剥离「前 `keep` 张以外」的图片：保留前 `keep` 张，把第 `keep + 1` 张及之后
/// 的图片块替换为 [`UNSUPPORTED_IMAGE_MARKER`]，返回剥离张数。
///
/// 覆盖 Anthropic `messages`、OpenAI Responses `input`、Gemini `contents`
/// 三处会话图片（含 tool_result 内的图片）。`keep == 0` 表示全部剥离。
///
/// 注意：tool output 里经 `strip_media_from_tool_value` 的附件化媒体不在
/// 本函数的遍历内，那批图片不进此预算——见 issue（StepFun 图片上限）。
pub fn strip_images_beyond_first(body: &mut Value, keep: usize) -> usize {
    replace_images_in_body_with_budget(body, &mut ImageBudget { remaining: keep })
}

/// Replace image blocks before sending when the routed model is text-only.
///
/// Two paths, both reached only when the caller's media-fallback switch is on:
/// - explicit capability from the provider config (modelCatalog / modalities) is
///   always trusted — it is declaration-driven, never a guess;
/// - the confirmed text-only registry is used for proactive replacement only
///   when `allow_heuristic` is true. This switch controls silent request-body
///   mutation, not the capability truth advertised by the Codex model catalog.
pub fn replace_images_for_text_only_model(
    body: &mut Value,
    provider: &Provider,
    allow_heuristic: bool,
) -> usize {
    if !contains_image_blocks(body) {
        return 0;
    }

    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");

    if image_input_capability_from_settings(&provider.settings_config, model, allow_heuristic)
        != ImageInputCapability::Unsupported
    {
        return 0;
    }

    replace_images_in_body(body)
}

pub fn contains_image_blocks(body: &Value) -> bool {
    messages_have_image_blocks(body)
        || responses_input_has_image_blocks(body.get("input"))
        || gemini_contents_have_image_blocks(body)
}

pub fn replace_image_blocks_with_marker(body: &mut Value) -> usize {
    replace_images_in_body(body)
}

pub fn is_unsupported_image_error(error: &ProxyError) -> bool {
    let ProxyError::UpstreamError { status, body } = error else {
        return false;
    };

    if !matches!(*status, 400 | 415 | 422 | 501) {
        return false;
    }

    let Some(body) = body.as_deref() else {
        return false;
    };

    let message = extract_error_text(body);
    let message = message.to_ascii_lowercase();

    // 自证性表述：这类短语本身就断言了"仅接受文本"，属于模态拒绝，无需再要求
    // 错误提到 image/media 等字样——火山方舟等网关的报错是
    // "Model only support text input"，全程不出现 image（issue #5025）。
    // 国产网关的英文常缺三单 s，因此带 s / 不带 s 两种形式都要列。
    const TEXT_ONLY_SELF_EVIDENT_HINTS: &[&str] = &["only support text", "only supports text"];
    if TEXT_ONLY_SELF_EVIDENT_HINTS
        .iter()
        .any(|hint| message.contains(hint))
    {
        return true;
    }

    let mentions_image = message.contains("image")
        || message.contains("vision")
        || message.contains("multimodal")
        || message.contains("multi-modal")
        || message.contains("modality")
        || message.contains("modalities")
        || message.contains("media")
        || message.contains("attachment");

    if !mentions_image {
        return false;
    }

    const UNSUPPORTED_HINTS: &[&str] = &[
        "unsupported",
        "not supported",
        "does not support",
        "doesn't support",
        "do not support",
        "don't support",
        "text only",
        "text-only",
        "invalid content type",
        "invalid message content",
        "unknown variant",
        "unknown content type",
        "unrecognized content type",
        "cannot process",
        "cannot handle",
        "can't process",
        "can't handle",
        "unable to process",
    ];

    UNSUPPORTED_HINTS.iter().any(|hint| message.contains(hint))
}

fn content_has_image_blocks(content: &Value) -> bool {
    let Some(blocks) = content.as_array() else {
        return false;
    };

    blocks.iter().any(|block| {
        is_image_block_type(block.get("type").and_then(Value::as_str))
            || block.get("content").is_some_and(|nested| {
                content_has_image_blocks(nested)
                    || (block.get("type").and_then(Value::as_str) == Some("tool_result")
                        && tool_output_contains_media(nested, ToolMediaScope::ImagesOnly))
            })
    })
}

fn replace_images_in_body(body: &mut Value) -> usize {
    replace_images_in_body_with_budget(body, &mut ImageBudget::unlimited())
}

fn replace_images_in_body_with_budget(body: &mut Value, budget: &mut ImageBudget) -> usize {
    let message_replacements = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .map(|messages| {
            messages
                .iter_mut()
                .map(|message| replace_images_in_message(message, budget))
                .sum()
        })
        .unwrap_or(0);

    message_replacements
        + body
            .get_mut("input")
            .map(|input| replace_images_in_responses_input(input, budget))
            .unwrap_or(0)
        + replace_images_in_gemini_contents(body, budget)
}

fn replace_images_in_message(message: &mut Value, budget: &mut ImageBudget) -> usize {
    let is_tool_message = message.get("role").and_then(Value::as_str) == Some("tool");
    let Some(content) = message.get_mut("content") else {
        return 0;
    };

    if is_tool_message {
        // Preserve the legacy typed-image replacement semantics first,
        // including Anthropic cache_control on the replacement text block.
        // The shared traversal then handles JSON strings, MCP wrappers, and
        // loose data-URL shapes that the legacy recursion does not recognize.
        let mut replaced = replace_images_in_content(content, budget);
        let replacement_block = json!({
            "type":"text",
            "text":UNSUPPORTED_IMAGE_MARKER
        });
        let mut discarded_media = Vec::new();
        replaced += strip_media_from_tool_value(
            content,
            &mut discarded_media,
            ToolMediaScope::ImagesOnly,
            &replacement_block,
            UNSUPPORTED_IMAGE_MARKER,
        );
        replaced
    } else {
        replace_images_in_content(content, budget)
    }
}

fn replace_images_in_content(content: &mut Value, budget: &mut ImageBudget) -> usize {
    replace_images_in_content_with_text_type(content, "text", budget)
}

fn replace_images_in_content_with_text_type(
    content: &mut Value,
    text_type: &str,
    budget: &mut ImageBudget,
) -> usize {
    let Some(blocks) = content.as_array_mut() else {
        return 0;
    };

    let mut replaced = 0usize;
    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str);
        if is_image_block_type(block_type) && !budget.keep() {
            replace_image_block_with_text_marker(block, text_type);
            replaced += 1;
            continue;
        }

        let is_tool_result = block_type == Some("tool_result");
        if let Some(nested_content) = block.get_mut("content") {
            if is_tool_result {
                // Run the legacy typed-block replacement before the shared
                // payload-aware traversal. This makes replacement a superset
                // of detection and preserves cache_control on Anthropic image
                // blocks, while the second pass covers alternate tool shapes.
                replaced +=
                    replace_images_in_content_with_text_type(nested_content, text_type, budget);
                let replacement_block = json!({
                    "type":text_type,
                    "text":UNSUPPORTED_IMAGE_MARKER
                });
                let mut discarded_media = Vec::new();
                replaced += strip_media_from_tool_value(
                    nested_content,
                    &mut discarded_media,
                    ToolMediaScope::ImagesOnly,
                    &replacement_block,
                    UNSUPPORTED_IMAGE_MARKER,
                );
            } else {
                replaced +=
                    replace_images_in_content_with_text_type(nested_content, text_type, budget);
            }
        }
    }

    replaced
}

fn messages_have_image_blocks(body: &Value) -> bool {
    body.get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                let Some(content) = message.get("content") else {
                    return false;
                };
                content_has_image_blocks(content)
                    || (message.get("role").and_then(Value::as_str) == Some("tool")
                        && tool_output_contains_media(content, ToolMediaScope::ImagesOnly))
            })
        })
}

fn gemini_contents_have_image_blocks(body: &Value) -> bool {
    body.get("contents")
        .and_then(Value::as_array)
        .is_some_and(|contents| {
            contents.iter().any(|content| {
                content
                    .get("parts")
                    .and_then(Value::as_array)
                    .is_some_and(|parts| parts.iter().any(gemini_part_has_image))
            })
        })
}

fn gemini_part_has_image(part: &Value) -> bool {
    gemini_media_payload_is_image(part.get("inlineData").or_else(|| part.get("inline_data")))
        || gemini_media_payload_is_image(part.get("fileData").or_else(|| part.get("file_data")))
        || part
            .get("functionResponse")
            .or_else(|| part.get("function_response"))
            .and_then(|response| response.get("parts"))
            .and_then(Value::as_array)
            .is_some_and(|parts| parts.iter().any(gemini_part_has_image))
}

fn gemini_media_payload_is_image(payload: Option<&Value>) -> bool {
    payload
        .and_then(|payload| payload.get("mimeType").or_else(|| payload.get("mime_type")))
        .and_then(Value::as_str)
        .is_some_and(|mime_type| {
            mime_type
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
        })
}

fn replace_images_in_gemini_contents(body: &mut Value, budget: &mut ImageBudget) -> usize {
    body.get_mut("contents")
        .and_then(Value::as_array_mut)
        .map(|contents| {
            contents
                .iter_mut()
                .filter_map(|content| content.get_mut("parts").and_then(Value::as_array_mut))
                .map(|parts| {
                    parts
                        .iter_mut()
                        .map(|part| replace_images_in_gemini_part(part, budget))
                        .sum::<usize>()
                })
                .sum()
        })
        .unwrap_or(0)
}

fn replace_images_in_gemini_part(part: &mut Value, budget: &mut ImageBudget) -> usize {
    if (gemini_media_payload_is_image(part.get("inlineData").or_else(|| part.get("inline_data")))
        || gemini_media_payload_is_image(part.get("fileData").or_else(|| part.get("file_data"))))
        && !budget.keep()
    {
        *part = json!({"text":UNSUPPORTED_IMAGE_MARKER});
        return 1;
    }

    let response_key = if part.get("functionResponse").is_some() {
        "functionResponse"
    } else {
        "function_response"
    };
    let Some(function_response) = part.get_mut(response_key) else {
        return 0;
    };
    let Some(media_parts) = function_response
        .get_mut("parts")
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };

    let before = media_parts.len();
    media_parts.retain(|media_part| !gemini_part_has_image(media_part));
    let replaced = before.saturating_sub(media_parts.len());
    if replaced > 0 {
        if let Some(response) = function_response
            .get_mut("response")
            .and_then(Value::as_object_mut)
        {
            response.insert(
                "cc_switch_media".to_string(),
                Value::String(UNSUPPORTED_IMAGE_MARKER.to_string()),
            );
        }
    }
    replaced
}

fn responses_input_has_image_blocks(input: Option<&Value>) -> bool {
    match input {
        Some(Value::Array(items)) => items.iter().any(responses_input_item_has_image_blocks),
        Some(item @ Value::Object(_)) => responses_input_item_has_image_blocks(item),
        _ => false,
    }
}

fn responses_input_item_has_image_blocks(item: &Value) -> bool {
    if item.get("type").and_then(Value::as_str) == Some("input_image") {
        return true;
    }

    item.get("content").is_some_and(content_has_image_blocks)
        || item
            .get("output")
            .is_some_and(|output| tool_output_contains_media(output, ToolMediaScope::ImagesOnly))
}

fn replace_images_in_responses_input(input: &mut Value, budget: &mut ImageBudget) -> usize {
    match input {
        Value::Array(items) => items
            .iter_mut()
            .map(|item| replace_images_in_responses_input_item(item, budget))
            .sum(),
        Value::Object(_) => replace_images_in_responses_input_item(input, budget),
        _ => 0,
    }
}

fn replace_images_in_responses_input_item(item: &mut Value, budget: &mut ImageBudget) -> usize {
    let mut replaced = 0usize;

    if item.get("type").and_then(Value::as_str) == Some("input_image") && !budget.keep() {
        replace_image_block_with_text_marker(item, "input_text");
        replaced += 1;
    }

    if let Some(content) = item.get_mut("content") {
        replaced += replace_images_in_content_with_text_type(content, "input_text", budget);
    }

    if let Some(output) = item.get_mut("output") {
        // The image-capability fallback deliberately strips images only.
        // Tool-output files/audio remain a known unsupported-modality gap.
        let replacement_block = json!({
            "type": "input_text",
            "text": UNSUPPORTED_IMAGE_MARKER
        });
        let mut discarded_media = Vec::new();
        replaced += strip_media_from_tool_value(
            output,
            &mut discarded_media,
            ToolMediaScope::ImagesOnly,
            &replacement_block,
            UNSUPPORTED_IMAGE_MARKER,
        );
    }

    replaced
}

fn is_image_block_type(block_type: Option<&str>) -> bool {
    matches!(block_type, Some("image" | "image_url" | "input_image"))
}

fn replace_image_block_with_text_marker(block: &mut Value, text_type: &str) {
    let cache_control = block.get("cache_control").cloned();
    *block = json!({
        "type": text_type,
        "text": UNSUPPORTED_IMAGE_MARKER
    });
    if let (Some(cache_control), Some(object)) = (cache_control, block.as_object_mut()) {
        object.insert("cache_control".to_string(), cache_control);
    }
}

fn extract_error_text(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let candidates = [
            value.pointer("/error/message"),
            value.pointer("/message"),
            value.pointer("/detail"),
            value.pointer("/error"),
        ];
        if let Some(message) = candidates
            .into_iter()
            .flatten()
            .find_map(|value| value.as_str())
        {
            return message.to_string();
        }

        if let Ok(compact) = serde_json::to_string(&value) {
            return compact;
        }
    }

    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider(settings_config: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn large_tool_data_url() -> String {
        format!(
            "data:image/png;base64,{}",
            "SANITIZER_TOOL_MEDIA_SENTINEL".repeat(400)
        )
    }

    fn image_block(tag: &str) -> Value {
        json!({"type": "image", "source": {"tag": tag}})
    }

    fn remaining_image_tags(body: &Value) -> Vec<String> {
        body.get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .filter_map(|message| message.get("content").and_then(Value::as_array))
                    .flatten()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("image"))
                    .filter_map(|block| {
                        block
                            .pointer("/source/tag")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn budget_keeps_first_n_images_strips_the_rest() {
        let mut body = json!({
            "model": "step-3",
            "messages": [{
                "role": "user",
                "content": [image_block("a"), image_block("b"), image_block("c"), image_block("d")]
            }]
        });

        let stripped = strip_images_beyond_first(&mut body, 2);

        assert_eq!(stripped, 2);
        assert_eq!(remaining_image_tags(&body), vec!["a", "b"]);
        // 被剥离的位置替换成统一标记，文本内容不变
        let blocks = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[2]["text"], UNSUPPORTED_IMAGE_MARKER);
        assert_eq!(blocks[3]["text"], UNSUPPORTED_IMAGE_MARKER);
    }

    #[test]
    fn budget_zero_strips_all_images() {
        let mut body = json!({
            "model": "step-3",
            "messages": [{
                "role": "user",
                "content": [image_block("a"), image_block("b")]
            }]
        });

        let stripped = strip_images_beyond_first(&mut body, 0);

        assert_eq!(stripped, 2);
        assert!(remaining_image_tags(&body).is_empty());
    }

    #[test]
    fn budget_larger_than_available_is_a_noop() {
        let original = json!({
            "model": "step-3",
            "messages": [{
                "role": "user",
                "content": [image_block("a"), image_block("b")]
            }]
        });
        let mut body = original.clone();

        let stripped = strip_images_beyond_first(&mut body, 70);

        assert_eq!(stripped, 0);
        assert_eq!(body, original);
    }

    #[test]
    fn budget_counts_across_messages_in_document_order() {
        // 通常要守住的是「当前轮次」的图片：跨消息按文档顺序累计，
        // 保留最前面的若干张。
        let mut body = json!({
            "model": "step-3",
            "messages": [
                {"role": "user", "content": [image_block("first")]},
                {"role": "assistant", "content": [{"type": "text", "text": "ok"}]},
                {"role": "user", "content": [image_block("second"), image_block("third")]}
            ]
        });

        let stripped = strip_images_beyond_first(&mut body, 1);

        assert_eq!(stripped, 2);
        assert_eq!(remaining_image_tags(&body), vec!["first"]);
    }

    #[test]
    fn budget_applies_to_responses_input_and_gemini_parts() {
        let mut responses = json!({
            "model": "step-3",
            "input": [
                {"type": "input_image", "tag": "r1"},
                {"type": "input_image", "tag": "r2"},
                {"type": "input_image", "tag": "r3"}
            ]
        });
        assert_eq!(strip_images_beyond_first(&mut responses, 1), 2);

        let mut gemini = json!({
            "model": "step-3",
            "contents": [{
                "parts": [
                    {"inlineData": {"mimeType": "image/png", "data": "a"}},
                    {"inlineData": {"mimeType": "image/png", "data": "b"}}
                ]
            }]
        });
        assert_eq!(strip_images_beyond_first(&mut gemini, 1), 1);
        assert_eq!(gemini["contents"][0]["parts"][0]["inlineData"]["data"], "a");
        assert_eq!(
            gemini["contents"][0]["parts"][1]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn unlimited_budget_replaces_every_image_like_before() {
        // 既有路径（能力降级）必须保持原语义：全部替换。
        let mut body = json!({
            "model": "step-3",
            "messages": [{
                "role": "user",
                "content": [image_block("a"), image_block("b"), image_block("c")]
            }]
        });

        let count = replace_image_blocks_with_marker(&mut body);

        assert_eq!(count, 3);
        assert!(remaining_image_tags(&body).is_empty());
    }

    #[test]
    fn keeps_images_when_model_capability_is_unknown() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "unknown-model",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "look" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 0);
        assert_eq!(body["messages"][0]["content"][1]["type"], "image");
    }

    #[test]
    fn confirmed_text_only_models_replace_images_before_send() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "qwen/qwen3-coder-plus",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn confirmed_text_only_models_replace_chat_image_url_before_send() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "qwen3-coder-plus",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "look" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
        assert_eq!(
            body["messages"][0]["content"][1]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn confirmed_text_only_models_replace_codex_input_image_before_send() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "qwen3-coder-plus",
            "input": [{
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "look" },
                    { "type": "input_image", "image_url": "data:image/png;base64,abc" }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(body["input"][0]["content"][1]["type"], "input_text");
        assert_eq!(
            body["input"][0]["content"][1]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn longcat_models_are_classified_text_only() {
        // LongCat-2.0 (like the retired Flash Chat) is a text-only model; the
        // preset ships it in mixed case, so the classifier must normalize first.
        assert!(confirmed_text_only_model("LongCat-2.0"));
        assert!(confirmed_text_only_model("longcat/LongCat-2.0"));
        assert!(confirmed_text_only_model("LongCat-Flash-Chat"));
    }

    #[test]
    fn explicit_text_modalities_replace_images_before_send() {
        let provider = provider(json!({
            "models": [
                { "id": "deepseek-v4-pro", "input": ["text"] }
            ]
        }));
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "look" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(body["messages"][0]["content"][0]["text"], "look");
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
        assert_eq!(
            body["messages"][0]["content"][1]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn preserves_images_without_explicit_capability_even_for_unknown_models() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "unknown-model",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn explicit_text_modalities_can_override_visual_model_ids() {
        let provider = provider(json!({
            "models": [
                { "id": "gpt-4o", "input": ["text"] }
            ]
        }));
        let mut body = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn explicit_image_modalities_preserve_model_images() {
        let provider = provider(json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-pro", "modalities": { "input": ["text", "image"] } }
                ]
            }
        }));
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn known_mimo_pro_replaces_but_mimo_multimodal_preserves() {
        let provider = provider(json!({}));
        let mut pro_body = json!({
            "model": "xiaomi-mimo-token-plan/mimo-v2.5-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });
        let mut multimodal_body = json!({
            "model": "xiaomi-mimo-token-plan/mimo-v2.5",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let pro_count = replace_images_for_text_only_model(&mut pro_body, &provider, true);
        let multimodal_count =
            replace_images_for_text_only_model(&mut multimodal_body, &provider, true);

        assert_eq!(pro_count, 1);
        assert_eq!(multimodal_count, 0);
        assert_eq!(
            multimodal_body["messages"][0]["content"][0]["type"],
            "image"
        );
    }

    #[test]
    fn multimodal_kimi_model_is_not_on_text_only_list() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "kimi/kimi-k2.6",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn confirmed_text_only_variant_replaces_images_before_send() {
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "therouter/qwen/qwen3-coder-480b",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn unconditional_marker_replacement_handles_retry_path() {
        let mut body = json!({
            "model": "xiaomi-mimo-token-plan/mimo-v2.5-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        assert!(contains_image_blocks(&body));
        let count = replace_image_blocks_with_marker(&mut body);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn replaces_nested_tool_result_image_blocks() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_1",
                    "content": [
                        { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                    ]
                }]
            }]
        });

        let count = replace_image_blocks_with_marker(&mut body);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }

    #[test]
    fn replaces_file_backed_tool_result_image_and_preserves_cache_control() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_file",
                    "content": [{
                        "type": "image",
                        "source": {
                            "type": "file",
                            "file_id": "file_123"
                        },
                        "cache_control": {"type": "ephemeral"}
                    }]
                }]
            }]
        });

        assert!(contains_image_blocks(&body));
        let count = replace_image_blocks_with_marker(&mut body);
        let replacement = &body["messages"][0]["content"][0]["content"][0];

        assert_eq!(count, 1);
        assert_eq!(replacement["type"], "text");
        assert_eq!(replacement["text"], UNSUPPORTED_IMAGE_MARKER);
        assert_eq!(replacement["cache_control"]["type"], "ephemeral");
        assert!(!body.to_string().contains("file_123"));
    }

    #[test]
    fn replaces_stringified_anthropic_tool_result_image_blocks() {
        let content = json!({
            "content": [{
                "type": "image",
                "mimeType": "image/png",
                "data": "ANTHROPIC_STRING_TOOL_SENTINEL"
            }]
        })
        .to_string();
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_1",
                    "content": content
                }]
            }]
        });

        assert!(contains_image_blocks(&body));
        let count = replace_image_blocks_with_marker(&mut body);
        let rewritten = body["messages"][0]["content"][0]["content"]
            .as_str()
            .unwrap();

        assert_eq!(count, 1);
        assert!(rewritten.contains(UNSUPPORTED_IMAGE_MARKER));
        assert!(!rewritten.contains("ANTHROPIC_STRING_TOOL_SENTINEL"));
    }

    #[test]
    fn detects_and_replaces_responses_function_output_images() {
        let data_url = large_tool_data_url();
        let mut body = json!({
            "model": "text-only",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": {
                    "content": [
                        {"type": "input_text", "text": "caption"},
                        {"type": "input_image", "image_url": data_url.clone()},
                        {"type": "image", "mimeType": "image/webp", "data": "MCP_SENTINEL"}
                    ]
                }
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 2);
        assert_eq!(
            body["input"][0]["output"]["content"][1],
            json!({"type": "input_text", "text": UNSUPPORTED_IMAGE_MARKER})
        );
        assert_eq!(
            body["input"][0]["output"]["content"][2],
            json!({"type": "input_text", "text": UNSUPPORTED_IMAGE_MARKER})
        );
        assert!(!body.to_string().contains(&data_url));
        assert!(!body.to_string().contains("MCP_SENTINEL"));
    }

    #[test]
    fn proactive_text_only_sanitizer_covers_responses_tool_outputs() {
        let provider = provider(json!({
            "models": [{"id": "text-model", "input": ["text"]}]
        }));
        let mut body = json!({
            "model": "text-model",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,PROACTIVE_SENTINEL"
                }]
            }]
        });

        let replaced = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(replaced, 1);
        assert_eq!(body["input"][0]["output"][0]["type"], "input_text");
        assert!(!body.to_string().contains("PROACTIVE_SENTINEL"));
    }

    #[test]
    fn detects_and_replaces_json_string_tool_output_symmetrically() {
        let data_url = large_tool_data_url();
        let output = json!({
            "content": [{
                "type": "input_image",
                "image_url": data_url.clone()
            }]
        })
        .to_string();
        let mut body = json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "call_string",
                "output": output
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 1);
        let rewritten = body["input"][0]["output"].as_str().unwrap();
        assert!(rewritten.contains(UNSUPPORTED_IMAGE_MARKER));
        assert!(!rewritten.contains(&data_url));
        let parsed: Value = serde_json::from_str(rewritten).unwrap();
        assert_eq!(parsed["content"][0]["type"], "input_text");
    }

    #[test]
    fn detects_and_replaces_whole_string_tool_image_data_url() {
        let data_url = large_tool_data_url();
        let mut body = json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "call_raw",
                "output": data_url.clone()
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 1);
        assert_eq!(
            body["input"][0]["output"],
            Value::String(UNSUPPORTED_IMAGE_MARKER.to_string())
        );
        assert!(!body.to_string().contains(&data_url));
    }

    #[test]
    fn detects_and_replaces_custom_tool_output_images() {
        let mut body = json!({
            "input": [{
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "status": "completed",
                "output": [{
                    "type": "image_url",
                    "image_url": {"url": "https://example.com/render.png"}
                }]
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 1);
        assert_eq!(body["input"][0]["status"], "completed");
        assert_eq!(body["input"][0]["output"][0]["type"], "input_text");
    }

    #[test]
    fn ignores_no_media_and_untyped_remote_tool_outputs() {
        let mut body = json!({
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_text",
                    "output": {"content": [{"type": "text", "text": "ordinary result"}]}
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "output": {
                        "image_url": {"url": "https://example.com/search-thumbnail.png"}
                    }
                }
            ]
        });
        let original = body.clone();

        assert!(!contains_image_blocks(&body));
        assert_eq!(replace_image_blocks_with_marker(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn image_retry_scope_intentionally_ignores_tool_files_and_audio() {
        let mut body = json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "call_modalities",
                "output": {
                    "content": [
                        {"type": "input_file", "file_id": "file_1"},
                        {
                            "type": "input_audio",
                            "input_audio": {"data": "AUDIO", "format": "wav"}
                        }
                    ]
                }
            }]
        });
        let original = body.clone();

        assert!(!contains_image_blocks(&body));
        assert_eq!(replace_image_blocks_with_marker(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn replaces_synthetic_user_and_tool_role_chat_image_parts() {
        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "tool media"},
                        {
                            "type": "image_url",
                            "image_url": {"url": "data:image/png;base64,USER_SENTINEL"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": [{
                        "type": "image_url",
                        "image_url": {"url": "https://example.com/tool.png"}
                    }]
                }
            ]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 2);
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
        assert_eq!(body["messages"][1]["content"][0]["type"], "text");
    }

    #[test]
    fn detects_and_replaces_stringified_chat_tool_image() {
        let content = json!({
            "content": [{
                "type": "image",
                "mimeType": "image/png",
                "data": "STRINGIFIED_CHAT_TOOL_SENTINEL"
            }]
        })
        .to_string();
        let mut body = json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_1",
                "content": content
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);
        let rewritten = body["messages"][0]["content"].as_str().unwrap();

        assert_eq!(replaced, 1);
        assert!(rewritten.contains(UNSUPPORTED_IMAGE_MARKER));
        assert!(!rewritten.contains("STRINGIFIED_CHAT_TOOL_SENTINEL"));
    }

    #[test]
    fn detects_and_replaces_gemini_native_image_parts() {
        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {
                        "functionResponse": {
                            "name": "inspect",
                            "response": {"content": "done"}
                        }
                    },
                    {
                        "inlineData": {
                            "mimeType": "image/png",
                            "data": "GEMINI_INLINE_SENTINEL"
                        }
                    }
                ]
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 1);
        assert_eq!(
            body["contents"][0]["parts"][1]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
        assert!(!body.to_string().contains("GEMINI_INLINE_SENTINEL"));
    }

    #[test]
    fn detects_and_removes_nested_gemini_function_response_media() {
        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": "inspect",
                        "response": {"content": "done"},
                        "parts": [{
                            "inlineData": {
                                "mimeType": "image/webp",
                                "data": "GEMINI_FUNCTION_SENTINEL"
                            }
                        }]
                    }
                }]
            }]
        });

        assert!(contains_image_blocks(&body));
        let replaced = replace_image_blocks_with_marker(&mut body);

        assert_eq!(replaced, 1);
        assert!(body["contents"][0]["parts"][0]["functionResponse"]["parts"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            body["contents"][0]["parts"][0]["functionResponse"]["response"]["cc_switch_media"],
            UNSUPPORTED_IMAGE_MARKER
        );
        assert!(!body.to_string().contains("GEMINI_FUNCTION_SENTINEL"));
    }

    #[test]
    fn detects_unsupported_image_errors() {
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"This model does not support image input"}}"#.to_string(),
            ),
        };

        assert!(is_unsupported_image_error(&error));
    }

    #[test]
    fn detects_text_only_errors_without_image_mention() {
        // 火山方舟真实报错（issue #5025）：不含 image/media 等字样，且英文缺
        // 三单 s——旧逻辑的 mentions_image 门与 "only supports text" 提示都拦不住。
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"Model only support text input Request id: 021783"}}"#
                    .to_string(),
            ),
        };

        assert!(is_unsupported_image_error(&error));
    }

    #[test]
    fn glm_52_is_classified_text_only() {
        // issue #5025：火山 Coding Plan 的 GLM 5.2 是纯文本端点，
        // 映射链 glm-5.2[1M] 归一化后尾部为 glm-5.2。
        assert!(confirmed_text_only_model("glm-5.2"));
        assert!(confirmed_text_only_model("GLM-5.2[1M]"));
        assert!(confirmed_text_only_model("zai-org/GLM-5.2"));
        // 未来视觉版（智谱 4v/5v 命名惯例）不能被误判为纯文本。
        assert!(!confirmed_text_only_model("glm-5.2v"));
    }

    #[test]
    fn ignores_non_image_errors() {
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(r#"{"error":{"message":"Invalid API key"}}"#.to_string()),
        };

        assert!(!is_unsupported_image_error(&error));
    }

    #[test]
    fn preserves_cache_control_when_replacing_image() {
        // image block 可能承载 prompt cache 断点；替换成标记时必须把
        // cache_control 迁移到新的 text block，否则会断掉缓存命中。
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": "abc" },
                    "cache_control": { "type": "ephemeral" }
                }]
            }]
        });

        let count = replace_image_blocks_with_marker(&mut body);

        assert_eq!(count, 1);
        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], UNSUPPORTED_IMAGE_MARKER);
        assert_eq!(block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn detects_media_and_attachment_error_phrasings() {
        let media_error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"This model cannot process media inputs"}}"#.to_string(),
            ),
        };
        assert!(is_unsupported_image_error(&media_error));

        let attachment_error = ProxyError::UpstreamError {
            status: 422,
            body: Some(r#"{"message":"attachments are not supported by this model"}"#.to_string()),
        };
        assert!(is_unsupported_image_error(&attachment_error));
    }

    #[test]
    fn detects_chat_content_unknown_variant_image_url_errors() {
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"}}"#
                    .to_string(),
            ),
        };

        assert!(is_unsupported_image_error(&error));
    }

    #[test]
    fn heuristic_disabled_keeps_images_for_listed_text_only_models() {
        // allow_heuristic = false：内置列表不再预测性剥图，避免误判多模态模型时静默丢图。
        let provider = provider(json!({}));
        let mut body = json!({
            "model": "deepseek/deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, false);

        assert_eq!(count, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn explicit_text_capability_replaces_even_when_heuristic_disabled() {
        // 显式声明 text-only 是声明驱动、零误判，即使关掉启发式也应生效。
        let provider = provider(json!({
            "models": [
                { "id": "deepseek-v4-pro", "input": ["text"] }
            ]
        }));
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });

        let count = replace_images_for_text_only_model(&mut body, &provider, false);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
    }
}
