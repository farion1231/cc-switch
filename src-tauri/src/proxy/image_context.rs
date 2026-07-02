use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const IMAGE_ANALYSIS_PROMPT: &str = "You are an image content extractor. Do not answer the user's final question.\n\
Use the original order of the user's text and images to extract key details from each image that are relevant to the user's request.\n\
If there are multiple images, summarize their obvious relationships, differences, or comparable points.\n\
Output exactly in this structure:\n\
Image 1:\n\
<details for image 1>\n\n\
Image 2:\n\
<details for image 2>\n\n\
Cross-image relationship:\n\
<relationship between images; omit this section if there is only one image or no clear relationship>\n\n\
Only output image details and cross-image relationship context. Do not answer the user's final question.";

const IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAnalysis {
    pub images: BTreeMap<usize, String>,
    pub relation: Option<String>,
    pub raw_text: String,
}

pub fn image_model_from_provider(provider: &crate::provider::Provider) -> Option<String> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.image_model.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

pub fn contains_image_blocks(body: &Value) -> bool {
    count_image_blocks(body) > 0
}

pub fn count_image_blocks(body: &Value) -> usize {
    body.get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message.get("content"))
                .map(count_images_in_content)
                .sum()
        })
        .unwrap_or(0)
}

pub fn image_context_cache_key(
    body: &Value,
    provider_id: &str,
    image_model: &str,
) -> Option<String> {
    let mut image_blocks = Vec::new();
    collect_image_values(body.get("messages")?, &mut image_blocks);
    if image_blocks.is_empty() {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(provider_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(image_model.as_bytes());
    hasher.update(b"\0");
    for block in image_blocks {
        let bytes = serde_json::to_vec(block).ok()?;
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    Some(format!("{:x}", hasher.finalize()))
}

pub fn image_context_image_cache_keys(
    body: &Value,
    provider_id: &str,
    image_model: &str,
) -> Option<Vec<String>> {
    let mut image_blocks = Vec::new();
    collect_image_values(body.get("messages")?, &mut image_blocks);
    if image_blocks.is_empty() {
        return None;
    }

    let mut keys = Vec::with_capacity(image_blocks.len());
    for block in image_blocks {
        let normalized = normalize_image_block_for_analysis(block);
        let bytes = serde_json::to_vec(&normalized).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(provider_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(image_model.as_bytes());
        hasher.update(b"\0");
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
        keys.push(format!("{:x}", hasher.finalize()));
    }
    Some(keys)
}

fn collect_image_values<'a>(value: &'a Value, images: &mut Vec<&'a Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_image_values(item, images);
            }
        }
        Value::Object(map) => {
            if is_image_block(value) {
                images.push(value);
                return;
            }
            for item in map.values() {
                collect_image_values(item, images);
            }
        }
        _ => {}
    }
}

pub fn create_image_analysis_request(original_body: &Value, image_model: &str) -> Value {
    let mut request = copy_request_options(original_body);
    request.insert("model".to_string(), Value::String(image_model.to_string()));
    request.insert(
        "max_tokens".to_string(),
        json!(normalize_max_tokens(original_body.get("max_tokens"))),
    );
    request.insert(
        "messages".to_string(),
        json!([{
            "role": "user",
            "content": build_ordered_analysis_content(original_body.get("messages")),
        }]),
    );
    Value::Object(request)
}

pub fn parse_image_analysis_response(text: &str, image_count: usize) -> ImageAnalysis {
    let raw_text = text.trim().to_string();
    let mut images = BTreeMap::new();
    let mut relation = None;
    if raw_text.is_empty() {
        return ImageAnalysis {
            images,
            relation,
            raw_text,
        };
    }

    let header = Regex::new(
        r"(?i)^\s*((?:image|图片)\s*(\d+)|cross-image relationship|multi-image relationship|image relationship|多图关系)\s*[：:]\s*(.*)$",
    )
        .expect("valid image section regex");
    let mut current: Option<Section> = None;

    for line in raw_text.lines() {
        if let Some(captures) = header.captures(line) {
            store_section(current.take(), &mut images, &mut relation);
            let label = captures
                .get(1)
                .map(|m| {
                    m.as_str()
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                })
                .unwrap_or_default();
            let first_line = captures
                .get(3)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let label_lower = label.to_ascii_lowercase();
            let kind = if label == "多图关系" || label_lower.contains("relationship") {
                SectionKind::Relation
            } else {
                let index = captures
                    .get(2)
                    .and_then(|m| m.as_str().parse::<usize>().ok())
                    .unwrap_or(0);
                SectionKind::Image(index)
            };
            current = Some(Section {
                kind,
                lines: if first_line.is_empty() {
                    Vec::new()
                } else {
                    vec![first_line]
                },
            });
            continue;
        }

        if let Some(section) = current.as_mut() {
            section.lines.push(line.to_string());
        }
    }

    store_section(current.take(), &mut images, &mut relation);

    if images.is_empty() && image_count > 0 {
        for index in 1..=image_count {
            images.insert(index, raw_text.clone());
        }
    }

    ImageAnalysis {
        images,
        relation,
        raw_text,
    }
}

pub fn inject_image_context(messages: &Value, analysis: &ImageAnalysis) -> Value {
    let Some(message_items) = messages.as_array() else {
        return json!([]);
    };

    let mut output = Vec::with_capacity(message_items.len());
    let mut image_index = 0usize;
    let mut last_user_with_images = None;
    let mut last_user = None;

    for message in message_items {
        let mut next_message = message.clone();
        if next_message.get("role").and_then(Value::as_str) == Some("user") {
            last_user = Some(output.len());
        }

        let mut replaced = false;
        if let Some(content) = next_message.get_mut("content") {
            if content.is_array() {
                *content =
                    inject_content_blocks(content, analysis, &mut image_index, &mut replaced);
            }
        }

        if replaced && next_message.get("role").and_then(Value::as_str) == Some("user") {
            last_user_with_images = Some(output.len());
        }
        output.push(next_message);
    }

    if let Some(relation) = analysis
        .relation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let target_index = last_user_with_images.or(last_user);
        let relation_text = format_relation_context(relation);
        if let Some(index) = target_index {
            append_text_block(&mut output[index], relation_text);
        } else {
            output.push(json!({
                "role": "user",
                "content": relation_text,
            }));
        }
    }

    Value::Array(output)
}

pub fn extract_text_from_response(data: &Value) -> String {
    if let Some(content) = data.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }

    if let Some(content) = data
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|first| first.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        return content.to_string();
    }

    if let Some(output_text) = data.get("output_text").and_then(Value::as_str) {
        return output_text.to_string();
    }

    if let Some(output) = data.get("output").and_then(Value::as_array) {
        let text = output
            .iter()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flat_map(|content| content.iter())
            .filter_map(|block| {
                block
                    .get("text")
                    .or_else(|| block.get("output_text"))
                    .and_then(Value::as_str)
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }

    if let Some(candidates) = data.get("candidates").and_then(Value::as_array) {
        let text = candidates
            .iter()
            .filter_map(|candidate| {
                candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
            })
            .flat_map(|parts| parts.iter())
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }

    data.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn build_ordered_analysis_content(messages: Option<&Value>) -> Vec<Value> {
    let Some(messages) = messages.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut content = vec![json!({ "type": "text", "text": IMAGE_ANALYSIS_PROMPT })];
    let mut image_index = 0usize;

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match message.get("content") {
            Some(Value::String(text)) if !text.trim().is_empty() => {
                content.push(json!({
                    "type": "text",
                    "text": format!("[{role} text]\n{text}"),
                }));
            }
            Some(Value::Array(blocks)) => {
                build_ordered_blocks(blocks, role, &mut image_index, &mut content);
            }
            _ => {}
        }
    }

    content
}

fn build_ordered_blocks(
    blocks: &[Value],
    role: &str,
    image_index: &mut usize,
    content: &mut Vec<Value>,
) {
    for block in blocks {
        if is_image_block(block) {
            *image_index += 1;
            content.push(json!({ "type": "text", "text": format!("Image {}:", *image_index) }));
            content.push(normalize_image_block_for_analysis(block));
            continue;
        }

        if block.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    content.push(json!({
                        "type": "text",
                        "text": format!("[{role} text]\n{text}"),
                    }));
                }
            }
            continue;
        }

        if let Some(nested) = block.get("content").and_then(Value::as_array) {
            build_ordered_blocks(nested, role, image_index, content);
        }
    }
}

fn inject_content_blocks(
    content: &Value,
    analysis: &ImageAnalysis,
    image_index: &mut usize,
    replaced: &mut bool,
) -> Value {
    let Some(blocks) = content.as_array() else {
        return content.clone();
    };

    let mut output = Vec::with_capacity(blocks.len());
    for block in blocks {
        if is_image_block(block) {
            *image_index += 1;
            *replaced = true;
            let image_text = analysis
                .images
                .get(image_index)
                .map(String::as_str)
                .unwrap_or(analysis.raw_text.as_str())
                .trim();
            if !image_text.is_empty() {
                output.push(json!({
                    "type": "text",
                    "text": format_image_context(*image_index, image_text),
                }));
            }
            continue;
        }

        let mut next = block.clone();
        if let Some(nested) = next.get_mut("content") {
            if nested.is_array() {
                *nested = inject_content_blocks(nested, analysis, image_index, replaced);
            }
        }
        output.push(next);
    }

    Value::Array(output)
}

fn count_images_in_content(content: &Value) -> usize {
    let Some(blocks) = content.as_array() else {
        return 0;
    };

    blocks
        .iter()
        .map(|block| {
            if is_image_block(block) {
                1
            } else {
                block
                    .get("content")
                    .map(count_images_in_content)
                    .unwrap_or(0)
            }
        })
        .sum()
}

fn is_image_block(block: &Value) -> bool {
    if image_type_is_image_like(block.get("type").and_then(Value::as_str)) {
        return true;
    }

    if image_mime_from_value(block).is_some() || image_url_from_value(block).is_some() {
        return true;
    }

    ["source", "file", "image", "input_image"]
        .into_iter()
        .filter_map(|key| block.get(key))
        .any(|value| {
            image_mime_from_value(value).is_some() || image_url_from_value(value).is_some()
        })
}

fn normalize_image_block_for_analysis(block: &Value) -> Value {
    if block.get("type").and_then(Value::as_str) == Some("image") && block.get("source").is_some() {
        return block.clone();
    }

    if let Some(url) = image_url_from_value(block) {
        if let Some((media_type, data)) = parse_data_image_url(url) {
            return json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data,
                }
            });
        }
    }

    if let Some(source) = block.get("source") {
        if let (Some(media_type), Some(data)) = (
            image_mime_from_value(source).or_else(|| image_mime_from_value(block)),
            source.get("data").and_then(Value::as_str),
        ) {
            return json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data,
                }
            });
        }
    }

    block.clone()
}

fn image_type_is_image_like(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "image" | "input_image" | "image_url"
        )
    })
}

fn image_mime_from_value(value: &Value) -> Option<&str> {
    [
        "media_type",
        "mediaType",
        "mime_type",
        "mimeType",
        "mime",
        "type",
    ]
    .into_iter()
    .find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|mime| is_image_mime(mime))
    })
}

fn is_image_mime(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("image/") || IMAGE_MIME_TYPES.contains(&normalized.as_str())
}

fn image_url_from_value(value: &Value) -> Option<&str> {
    let url = value
        .get("image_url")
        .and_then(|image_url| {
            image_url
                .as_str()
                .or_else(|| image_url.get("url").and_then(Value::as_str))
        })
        .or_else(|| value.get("url").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("source")
                .and_then(|source| source.get("url"))
                .and_then(Value::as_str)
        })?;

    if url.trim_start().starts_with("data:image/") {
        Some(url)
    } else {
        None
    }
}

fn parse_data_image_url(url: &str) -> Option<(String, String)> {
    let rest = url.trim().strip_prefix("data:")?;
    let (metadata, data) = rest.split_once(',')?;
    let media_type = metadata.split(';').next().unwrap_or("").trim();
    if !metadata.to_ascii_lowercase().contains(";base64") || !is_image_mime(media_type) {
        return None;
    }
    Some((media_type.to_string(), data.to_string()))
}

fn copy_request_options(body: &Value) -> Map<String, Value> {
    let mut copy = Map::new();
    for key in ["system", "temperature", "top_p", "top_k", "metadata"] {
        if let Some(value) = body.get(key) {
            copy.insert(key.to_string(), value.clone());
        }
    }
    copy
}

fn normalize_max_tokens(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_u64)
        .map(|value| value.clamp(512, 4096))
        .unwrap_or(2048)
}

fn append_text_block(message: &mut Value, text: String) {
    match message.get_mut("content") {
        Some(Value::String(existing)) => {
            if existing.trim().is_empty() {
                *existing = text;
            } else {
                existing.push_str("\n\n");
                existing.push_str(&text);
            }
        }
        Some(Value::Array(blocks)) => {
            blocks.push(json!({ "type": "text", "text": text }));
        }
        _ => {
            if let Some(object) = message.as_object_mut() {
                object.insert("content".to_string(), Value::String(text));
            }
        }
    }
}

fn format_image_context(index: usize, description: &str) -> String {
    format!("[Image {index} analysis]\n{}", description.trim())
}

fn format_relation_context(relation: &str) -> String {
    format!("[Cross-image relationship]\n{}", relation.trim())
}

#[derive(Debug)]
struct Section {
    kind: SectionKind,
    lines: Vec<String>,
}

#[derive(Debug)]
enum SectionKind {
    Image(usize),
    Relation,
}

fn store_section(
    section: Option<Section>,
    images: &mut BTreeMap<usize, String>,
    relation: &mut Option<String>,
) {
    let Some(section) = section else {
        return;
    };
    let content = section.lines.join("\n").trim().to_string();
    if content.is_empty() {
        return;
    }
    match section.kind {
        SectionKind::Image(index) if index > 0 => {
            images.insert(index, content);
        }
        SectionKind::Relation => {
            *relation = Some(content);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_multi_image_results_in_original_order() {
        let messages = json!([
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "先看这里" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "a" } },
                    { "type": "text", "text": "再比较这个" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "b" } },
                    { "type": "text", "text": "区别是什么？" }
                ]
            }
        ]);
        let analysis = parse_image_analysis_response(
            "图片1：\n第一张是登录页\n\n图片2：\n第二张是错误页\n\n多图关系：\n第二张比第一张多了错误提示",
            2,
        );

        let injected = inject_image_context(&messages, &analysis);
        let content = injected[0]["content"].as_array().unwrap();

        assert_eq!(content[0]["text"], "先看这里");
        assert_eq!(content[1]["text"], "[Image 1 analysis]\n第一张是登录页");
        assert_eq!(content[2]["text"], "再比较这个");
        assert_eq!(content[3]["text"], "[Image 2 analysis]\n第二张是错误页");
        assert_eq!(content[4]["text"], "区别是什么？");
        assert_eq!(
            content[5]["text"],
            "[Cross-image relationship]\n第二张比第一张多了错误提示"
        );
        assert!(!serde_json::to_string(&injected)
            .unwrap()
            .contains(r#""type":"image""#));
    }

    #[test]
    fn unstructured_analysis_is_used_for_each_image_without_losing_text() {
        let messages = json!([
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "解释" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "a" } }
                ]
            }
        ]);
        let analysis = parse_image_analysis_response("这张图是一个设置页面", 1);
        let injected = inject_image_context(&messages, &analysis);
        let content = injected[0]["content"].as_array().unwrap();

        assert_eq!(content[0]["text"], "解释");
        assert_eq!(
            content[1]["text"],
            "[Image 1 analysis]\n这张图是一个设置页面"
        );
    }

    #[test]
    fn image_only_request_injects_result_without_empty_text_blocks() {
        let messages = json!([
            {
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "a" } }
                ]
            }
        ]);
        let analysis = parse_image_analysis_response("图片1：\n只有一张流程图", 1);
        let injected = inject_image_context(&messages, &analysis);
        let content = injected[0]["content"].as_array().unwrap();

        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"], "[Image 1 analysis]\n只有一张流程图");
    }

    #[test]
    fn creates_analysis_request_with_ordered_text_and_image_markers() {
        let body = json!({
            "model": "main",
            "max_tokens": 9999,
            "temperature": 0.2,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "看图" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "a" } }
                ]
            }]
        });

        let request = create_image_analysis_request(&body, "vision-model");
        let content = request["messages"][0]["content"].as_array().unwrap();

        assert_eq!(request["model"], "vision-model");
        assert_eq!(request["max_tokens"], 4096);
        assert_eq!(request["temperature"], 0.2);
        assert!(content[0]["text"]
            .as_str()
            .unwrap()
            .contains("Do not answer the user's final question"));
        assert_eq!(content[1]["text"], "[user text]\n看图");
        assert_eq!(content[2]["text"], "Image 1:");
        assert_eq!(content[3]["type"], "image");
    }

    #[test]
    fn handles_openai_style_input_image_data_urls() {
        let body = json!({
            "model": "main",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "这是什么" },
                    { "type": "input_image", "image_url": "data:image/png;base64,abc123" }
                ]
            }]
        });

        assert!(contains_image_blocks(&body));

        let request = create_image_analysis_request(&body, "vision-model");
        let content = request["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[3]["type"], "image");
        assert_eq!(content[3]["source"]["media_type"], "image/png");
        assert_eq!(content[3]["source"]["data"], "abc123");

        let analysis = parse_image_analysis_response("图片1：\n一个路由配置页面", 1);
        let injected = inject_image_context(&body["messages"], &analysis);
        let injected_content = injected[0]["content"].as_array().unwrap();
        assert_eq!(injected_content[0]["text"], "这是什么");
        assert_eq!(
            injected_content[1]["text"],
            "[Image 1 analysis]\n一个路由配置页面"
        );
        assert!(!serde_json::to_string(&injected)
            .unwrap()
            .contains("input_image"));
    }

    #[test]
    fn detects_image_blocks_by_nested_mime_type() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "file",
                        "source": {
                            "type": "base64",
                            "mimeType": "image/jpeg",
                            "data": "abc"
                        }
                    }
                ]
            }]
        });

        assert_eq!(count_image_blocks(&body), 1);
        let request = create_image_analysis_request(&body, "vision-model");
        let content = request["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[1]["text"], "Image 1:");
        assert_eq!(content[2]["type"], "image");
        assert_eq!(content[2]["source"]["media_type"], "image/jpeg");
    }

    #[test]
    fn image_cache_keys_are_stable_for_the_same_image_content() {
        let anthropic_body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "first question" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc123" } }
                ]
            }]
        });
        let openai_body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "second question" },
                    { "type": "input_image", "image_url": "data:image/png;base64,abc123" }
                ]
            }]
        });

        let first = image_context_image_cache_keys(&anthropic_body, "provider-a", "vision")
            .expect("image cache key");
        let second = image_context_image_cache_keys(&openai_body, "provider-a", "vision")
            .expect("image cache key");

        assert_eq!(first, second);
    }

    #[test]
    fn image_cache_keys_are_scoped_by_provider_and_image_model() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc123" } }
                ]
            }]
        });

        let base =
            image_context_image_cache_keys(&body, "provider-a", "vision").expect("image cache key");
        let other_provider =
            image_context_image_cache_keys(&body, "provider-b", "vision").expect("image cache key");
        let other_model = image_context_image_cache_keys(&body, "provider-a", "other-vision")
            .expect("image cache key");

        assert_ne!(base, other_provider);
        assert_ne!(base, other_model);
    }
}
