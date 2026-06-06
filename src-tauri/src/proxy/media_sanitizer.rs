use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

pub const UNSUPPORTED_IMAGE_MARKER: &str = "[Unsupported Image]";

/// Replace image blocks before sending when the routed model is text-only.
///
/// Two paths, both reached only when the caller's media-fallback switch is on:
/// - explicit capability from the provider config (modelCatalog / modalities) is
///   always trusted — it is declaration-driven, never a guess;
/// - the curated `known_text_only_model` list is a heuristic *prediction* and only
///   runs when `allow_heuristic` is true, so a mislabeled multimodal model cannot
///   have its images silently stripped when the user opts out.
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

    match explicit_model_image_support(provider, model) {
        Some(true) => return 0,
        Some(false) => return replace_images_in_body(body),
        None => {}
    }

    if !allow_heuristic || !known_text_only_model(model) {
        return 0;
    }

    replace_images_in_body(body)
}

pub fn contains_image_blocks(body: &Value) -> bool {
    body.get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages
                .iter()
                .filter_map(|message| message.get("content"))
                .any(content_has_image_blocks)
        })
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
        "only supports text",
        "text only",
        "text-only",
        "invalid content type",
        "invalid message content",
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
        block.get("type").and_then(Value::as_str) == Some("image")
            || block.get("content").is_some_and(content_has_image_blocks)
    })
}

fn replace_images_in_body(body: &mut Value) -> usize {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return 0;
    };

    messages
        .iter_mut()
        .filter_map(|message| message.get_mut("content"))
        .map(replace_images_in_content)
        .sum()
}

fn replace_images_in_content(content: &mut Value) -> usize {
    let Some(blocks) = content.as_array_mut() else {
        return 0;
    };

    let mut replaced = 0usize;
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("image") {
            let cache_control = block.get("cache_control").cloned();
            *block = json!({
                "type": "text",
                "text": UNSUPPORTED_IMAGE_MARKER
            });
            if let (Some(cache_control), Some(object)) = (cache_control, block.as_object_mut()) {
                object.insert("cache_control".to_string(), cache_control);
            }
            replaced += 1;
            continue;
        }

        if let Some(nested_content) = block.get_mut("content") {
            replaced += replace_images_in_content(nested_content);
        }
    }

    replaced
}

fn explicit_model_image_support(provider: &Provider, model: &str) -> Option<bool> {
    let settings = &provider.settings_config;
    [
        settings
            .get("modelCatalog")
            .and_then(|catalog| catalog.get("models")),
        settings.get("modelCatalog"),
        settings.get("models"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| explicit_model_image_support_in_value(value, model))
}

fn known_text_only_model(model: &str) -> bool {
    let normalized = normalize_model_id(model);
    let tail = normalized.rsplit('/').next().unwrap_or(normalized.as_str());

    const EXACT_TAILS: &[&str] = &[
        "ark-code-latest",
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-v4-flash",
        "deepseek-v4-pro",
        "glm-5.1",
        "kat-coder",
        "kat-coder-pro",
        "kat-coder-pro v1",
        "kat-coder-pro v2",
        "kat-coder-pro-v1",
        "kat-coder-pro-v2",
        "ling-2.5-1t",
        "longcat-flash-chat",
        "mimo-v2.5-pro",
        "us.deepseek.r1-v1",
    ];

    const TAIL_PREFIXES: &[&str] = &["minimax-m2.7", "qwen3-coder", "step-3.5-flash"];

    EXACT_TAILS.contains(&tail) || TAIL_PREFIXES.iter().any(|prefix| tail.starts_with(prefix))
}

fn explicit_model_image_support_in_value(value: &Value, model: &str) -> Option<bool> {
    if let Some(models) = value.as_array() {
        return models.iter().find_map(|entry| {
            model_entry_matches(entry, None, model).then(|| explicit_image_support(entry))?
        });
    }

    let object = value.as_object()?;
    object.iter().find_map(|(key, entry)| {
        model_entry_matches(entry, Some(key), model).then(|| explicit_image_support(entry))?
    })
}

fn explicit_image_support(entry: &Value) -> Option<bool> {
    if let Some(value) = entry
        .get("supportsImage")
        .or_else(|| entry.get("supports_image"))
        .or_else(|| entry.get("vision"))
        .and_then(Value::as_bool)
    {
        return Some(value);
    }

    [
        entry.get("input"),
        entry.pointer("/modalities/input"),
        entry.get("input_modalities"),
        entry.get("inputModalities"),
    ]
    .into_iter()
    .flatten()
    .find_map(input_modalities_support_image)
}

fn input_modalities_support_image(value: &Value) -> Option<bool> {
    let modalities = value.as_array()?;
    Some(modalities.iter().any(|item| {
        item.as_str()
            .map(str::trim)
            .is_some_and(|item| item.eq_ignore_ascii_case("image"))
    }))
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

fn model_entry_matches(entry: &Value, key: Option<&str>, model: &str) -> bool {
    key.is_some_and(|key| model_ids_match(key, model))
        || ["model", "id", "name"]
            .into_iter()
            .filter_map(|field| entry.get(field).and_then(Value::as_str))
            .any(|candidate| model_ids_match(candidate, model))
}

fn model_ids_match(candidate: &str, model: &str) -> bool {
    let candidate = normalize_model_id(candidate);
    let model = normalize_model_id(model);
    if candidate.is_empty() || model.is_empty() {
        return false;
    }
    if candidate == model {
        return true;
    }

    let candidate_tail = candidate.rsplit('/').next().unwrap_or(candidate.as_str());
    let model_tail = model.rsplit('/').next().unwrap_or(model.as_str());
    candidate_tail == model_tail || candidate == model_tail || candidate_tail == model
}

fn normalize_model_id(value: &str) -> String {
    let mut normalized = value
        .trim()
        .trim_start_matches("models/")
        .trim()
        .to_ascii_lowercase();
    if let Some(stripped) =
        normalized.strip_suffix(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
    {
        normalized = stripped.trim().to_string();
    }
    normalized
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
    fn known_text_only_models_replace_images_before_send() {
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

        let count = replace_images_for_text_only_model(&mut body, &provider, true);

        assert_eq!(count, 1);
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            UNSUPPORTED_IMAGE_MARKER
        );
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
    fn known_text_only_prefixes_replace_images_before_send() {
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
