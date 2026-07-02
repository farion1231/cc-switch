//! Kiro Request Transformation Module
//!
//! Converts Anthropic Messages API format into Kiro's JSON request format.

use crate::proxy::error::ProxyError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use uuid::Uuid;

/// 模型能力（是否支持 thinking / output_config.effort）。
/// 来自 ListAvailableModels 返回的 additionalModelRequestFieldsSchema。
#[derive(Debug, Clone, Copy, Default)]
pub struct KiroModelCaps {
    /// 模型是否支持 thinking 字段（预留；cc-switch 目前仅用 effort 门控）。
    #[allow(dead_code)]
    pub supports_thinking: bool,
    pub supports_effort: bool,
}

/// 全局模型能力缓存（kiro_model_id -> caps）。模型能力是全局的，
/// 与账号无关，所以以 Kiro 侧 modelId 为键。get_kiro_models 拉取时写入，
/// anthropic_to_kiro 转换时同步读取。
fn caps_cache() -> &'static RwLock<HashMap<String, KiroModelCaps>> {
    static CACHE: OnceLock<RwLock<HashMap<String, KiroModelCaps>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 写入/更新某个 Kiro 模型的能力（由 get_kiro_models 调用）。
pub fn set_model_caps(kiro_model_id: &str, caps: KiroModelCaps) {
    if let Ok(mut map) = caps_cache().write() {
        map.insert(kiro_model_id.to_string(), caps);
    }
}

/// 读取某个 Kiro 模型的能力；未命中（冷启动/未拉取过）返回 None。
pub fn get_model_caps(kiro_model_id: &str) -> Option<KiroModelCaps> {
    caps_cache()
        .read()
        .ok()
        .and_then(|m| m.get(kiro_model_id).copied())
}
use crate::provider::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroImage {
    pub format: String,
    pub source: KiroImageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroImageSource {
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroToolUse {
    pub name: String,
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroToolResult {
    pub content: Vec<KiroToolResultContent>,
    pub status: String, // "success" or "error"
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KiroToolResultContent {
    Text { text: String },
    Json { json: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroToolSpec {
    #[serde(rename = "toolSpecification")]
    pub tool_specification: KiroToolSpecification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroToolSpecification {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: KiroInputSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroInputSchema {
    pub json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroUserInputMessage {
    pub content: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub origin: String, // "KIRO_CLI"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<KiroImage>>,
    #[serde(
        rename = "userInputMessageContext",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_input_message_context: Option<KiroUserInputMessageContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroUserInputMessageContext {
    #[serde(rename = "toolResults", skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<KiroToolResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<KiroToolSpec>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroAssistantResponseMessage {
    pub content: String,
    #[serde(rename = "toolUses", skip_serializing_if = "Option::is_none")]
    pub tool_uses: Option<Vec<KiroToolUse>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroHistoryEntry {
    #[serde(rename = "userInputMessage", skip_serializing_if = "Option::is_none")]
    pub user_input_message: Option<KiroUserInputMessage>,
    #[serde(
        rename = "assistantResponseMessage",
        skip_serializing_if = "Option::is_none"
    )]
    pub assistant_response_message: Option<KiroAssistantResponseMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroRequest {
    #[serde(rename = "conversationState")]
    pub conversation_state: KiroConversationState,
    #[serde(rename = "profileArn", skip_serializing_if = "Option::is_none")]
    pub profile_arn: Option<String>,
    #[serde(rename = "agentMode")]
    pub agent_mode: String, // "vibe"
    #[serde(
        rename = "additionalModelRequestFields",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_model_request_fields: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroConversationState {
    #[serde(rename = "chatTriggerType")]
    pub chat_trigger_type: String, // "MANUAL"
    #[serde(rename = "agentTaskType")]
    pub agent_task_type: String, // "vibe"
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    #[serde(rename = "currentMessage")]
    pub current_message: KiroCurrentMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<KiroHistoryEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroCurrentMessage {
    #[serde(rename = "userInputMessage")]
    pub user_input_message: KiroUserInputMessage,
}

/// Map Anthropic model to Kiro model ID
///
/// 仅把数字-数字边界（版本号，如 `4-5`）转换为点号（`4.5`），保留品牌名中的连字符。
/// 例如 `claude-haiku-4-5` -> `claude-haiku-4.5`（不是 `claude.haiku.4.5`）。
/// 这与参考实现 resolveKiroModel 一致；多余的全连字符替换会让 Kiro 返回
/// REQUEST_BODY_INVALID。
pub fn map_model_to_kiro(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower == "auto" {
        return "auto".to_string();
    }
    // 只替换 (digit)-(digit) 为 (digit).(digit)，保留其余连字符
    let re = regex::Regex::new(r"(\d)-(\d)").unwrap();
    re.replace_all(&lower, "$1.$2").into_owned()
}

fn sanitize_surrogates(text: &str) -> String {
    // Rust strings are valid UTF-8, but we may want to filter out unpaired surrogates
    // if the original text was parsed from JSON containing raw escape sequences.
    // Generally, standard Rust strings don't contain unpaired surrogates, but we can do a best effort.
    text.to_string()
}

fn parse_tool_result_content(text: &str) -> Vec<KiroToolResultContent> {
    let trimmed = text.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            return vec![KiroToolResultContent::Json { json: parsed }];
        }
    }
    vec![KiroToolResultContent::Text {
        text: text.to_string(),
    }]
}

fn convert_tools(tools: &[Value]) -> Vec<KiroToolSpec> {
    tools
        .iter()
        .map(|t| KiroToolSpec {
            tool_specification: KiroToolSpecification {
                name: t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: KiroInputSchema {
                    json: t.get("input_schema").cloned().unwrap_or(json!({})),
                },
            },
        })
        .collect()
}

/// Anthropic Request -> Kiro Request
pub fn anthropic_to_kiro(
    body: Value,
    _provider: &Provider,
    session_id: Option<&str>,
    profile_arn: Option<String>,
) -> Result<Value, ProxyError> {
    let original_model = body.get("model").and_then(|m| m.as_str()).unwrap_or("auto");
    let kiro_model_id = map_model_to_kiro(original_model);

    // Build history and current message
    let mut history: Vec<KiroHistoryEntry> = Vec::new();
    let mut current_content = String::new();
    let mut current_images: Option<Vec<KiroImage>> = None;
    let mut current_tool_results: Option<Vec<KiroToolResult>> = None;

    // Extract system prompt
    let mut system_prompt = String::new();
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            system_prompt = text.to_string();
        } else if let Some(arr) = system.as_array() {
            let parts: Vec<&str> = arr
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect();
            system_prompt = parts.join("\n\n");
        }
    }

    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        let len = messages.len();
        if len > 0 {
            // Process history messages (all except the last one)
            for (i, msg) in messages.iter().take(len - 1).enumerate() {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = msg.get("content");

                if role == "user" {
                    let mut text_parts = Vec::new();
                    let mut images = Vec::new();
                    let mut tool_results = Vec::new();

                    if let Some(text) = content.and_then(|c| c.as_str()) {
                        text_parts.push(text.to_string());
                    } else if let Some(arr) = content.and_then(|c| c.as_array()) {
                        for block in arr {
                            let block_type =
                                block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                            if block_type == "text" {
                                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            } else if block_type == "image" {
                                if let Some(source) = block.get("source") {
                                    let media_type = source
                                        .get("media_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("image/png");
                                    let data =
                                        source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                    let format =
                                        media_type.split('/').nth(1).unwrap_or("png").to_string();
                                    images.push(KiroImage {
                                        format,
                                        source: KiroImageSource {
                                            bytes: data.to_string(),
                                        },
                                    });
                                }
                            } else if block_type == "tool_result" {
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let is_error = block
                                    .get("is_error")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let tr_content = block.get("content");
                                let tr_text = if let Some(t) = tr_content.and_then(|v| v.as_str()) {
                                    t.to_string()
                                } else if let Some(arr) = tr_content.and_then(|v| v.as_array()) {
                                    let parts: Vec<&str> = arr
                                        .iter()
                                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                        .collect();
                                    parts.join("\n")
                                } else {
                                    "".to_string()
                                };
                                tool_results.push(KiroToolResult {
                                    content: parse_tool_result_content(&tr_text),
                                    status: if is_error {
                                        "error".to_string()
                                    } else {
                                        "success".to_string()
                                    },
                                    tool_use_id,
                                });
                            }
                        }
                    }

                    let mut content_str = text_parts.join("\n\n");
                    if i == 0 && !system_prompt.is_empty() {
                        content_str = format!("{}\n\n{}", system_prompt, content_str);
                    }

                    let uim = KiroUserInputMessage {
                        content: sanitize_surrogates(&content_str),
                        model_id: kiro_model_id.clone(),
                        origin: "KIRO_CLI".to_string(),
                        images: if images.is_empty() {
                            None
                        } else {
                            Some(images)
                        },
                        user_input_message_context: if tool_results.is_empty() {
                            None
                        } else {
                            Some(KiroUserInputMessageContext {
                                tool_results: Some(tool_results),
                                tools: None,
                            })
                        },
                    };

                    // Merge sequential user inputs if needed
                    if let Some(last_entry) = history.last_mut() {
                        if let Some(ref mut prev_uim) = last_entry.user_input_message {
                            prev_uim.content.push_str(&format!("\n\n{}", uim.content));
                            if let Some(imgs) = uim.images {
                                let existing = prev_uim.images.get_or_insert_with(Vec::new);
                                existing.extend(imgs);
                            }
                            continue;
                        }
                    }

                    history.push(KiroHistoryEntry {
                        user_input_message: Some(uim),
                        assistant_response_message: None,
                    });
                } else if role == "assistant" {
                    let mut arm_content = String::new();
                    let mut tool_uses = Vec::new();

                    if let Some(text) = content.and_then(|c| c.as_str()) {
                        arm_content = text.to_string();
                    } else if let Some(arr) = content.and_then(|c| c.as_array()) {
                        for block in arr {
                            let block_type =
                                block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                            if block_type == "text" {
                                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                    arm_content.push_str(t);
                                }
                            } else if block_type == "thinking" {
                                // 丢弃 thinking 块以防止消耗 Credit，不拼入文本正文
                            } else if block_type == "tool_use" {
                                let name = block
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let tool_use_id = block
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let input = block.get("input").cloned().unwrap_or(json!({}));
                                tool_uses.push(KiroToolUse {
                                    name,
                                    tool_use_id,
                                    input,
                                });
                            }
                        }
                    }

                    history.push(KiroHistoryEntry {
                        user_input_message: None,
                        assistant_response_message: Some(KiroAssistantResponseMessage {
                            content: arm_content,
                            tool_uses: if tool_uses.is_empty() {
                                None
                            } else {
                                Some(tool_uses)
                            },
                        }),
                    });
                }
            }

            // Process the last message (current turn)
            let last_msg = &messages[len - 1];
            let last_content = last_msg.get("content");
            let mut text_parts = Vec::new();
            let mut images = Vec::new();
            let mut tool_results = Vec::new();

            if let Some(text) = last_content.and_then(|c| c.as_str()) {
                text_parts.push(text.to_string());
            } else if let Some(arr) = last_content.and_then(|c| c.as_array()) {
                for block in arr {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                    if block_type == "text" {
                        if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    } else if block_type == "image" {
                        if let Some(source) = block.get("source") {
                            let media_type = source
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/png");
                            let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                            let format = media_type.split('/').nth(1).unwrap_or("png").to_string();
                            images.push(KiroImage {
                                format,
                                source: KiroImageSource {
                                    bytes: data.to_string(),
                                },
                            });
                        }
                    } else if block_type == "tool_result" {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let tr_content = block.get("content");
                        let tr_text = if let Some(t) = tr_content.and_then(|v| v.as_str()) {
                            t.to_string()
                        } else if let Some(arr) = tr_content.and_then(|v| v.as_array()) {
                            let parts: Vec<&str> = arr
                                .iter()
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .collect();
                            parts.join("\n")
                        } else {
                            "".to_string()
                        };
                        tool_results.push(KiroToolResult {
                            content: parse_tool_result_content(&tr_text),
                            status: if is_error {
                                "error".to_string()
                            } else {
                                "success".to_string()
                            },
                            tool_use_id,
                        });
                    }
                }
            }

            current_content = text_parts.join("\n\n");
            if len == 1 && !system_prompt.is_empty() {
                current_content = format!("{}\n\n{}", system_prompt, current_content);
            }
            if !images.is_empty() {
                current_images = Some(images);
            }
            if !tool_results.is_empty() {
                current_tool_results = Some(tool_results);
            }
        }
    }

    // Extract tools at root
    let mut kt_tools = None;
    if let Some(tools_arr) = body.get("tools").and_then(|t| t.as_array()) {
        if !tools_arr.is_empty() {
            kt_tools = Some(convert_tools(tools_arr));
        }
    }

    let uimc = if current_tool_results.is_some() || kt_tools.is_some() {
        Some(KiroUserInputMessageContext {
            tool_results: current_tool_results,
            tools: kt_tools,
        })
    } else {
        None
    };

    // 能力驱动：在 kiro_model_id 被 move 进消息体前先查询缓存。
    let model_caps = get_model_caps(&kiro_model_id);

    let user_message = KiroUserInputMessage {
        content: sanitize_surrogates(&current_content),
        model_id: kiro_model_id,
        origin: "KIRO_CLI".to_string(),
        images: current_images,
        user_input_message_context: uimc,
    };

    // Build additionalModelRequestFields
    let mut additional_fields = json!({});

    // Map Anthropic thinking to Kiro effort.
    // 能力驱动：仅在模型明确支持 effort 时才加；能力未知（冷启动/未拉取过
    // 模型列表）则保守性加上，由 forwarder 的 400 重试兑底。
    if let Some(thinking) = body.get("thinking") {
        if thinking.get("type").and_then(|t| t.as_str()) == Some("enabled") {
            let allow_effort = model_caps.map(|c| c.supports_effort).unwrap_or(true);
            if allow_effort {
                let effort = body
                    .pointer("/output_config/effort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium");
                additional_fields["output_config"] = json!({
                    "effort": effort
                });
            }
        }
    }

    let additional_fields_opt = if additional_fields.as_object().unwrap().is_empty() {
        None
    } else {
        Some(additional_fields)
    };

    let conversation_id = match session_id {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => Uuid::new_v4().to_string(),
    };

    let request = KiroRequest {
        conversation_state: KiroConversationState {
            chat_trigger_type: "MANUAL".to_string(),
            agent_task_type: "vibe".to_string(),
            conversation_id,
            current_message: KiroCurrentMessage {
                user_input_message: user_message,
            },
            history: if history.is_empty() {
                None
            } else {
                Some(history)
            },
        },
        profile_arn,
        agent_mode: "vibe".to_string(),
        additional_model_request_fields: additional_fields_opt,
    };

    serde_json::to_value(request).map_err(|e| ProxyError::TransformError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_model_preserves_brand_hyphens() {
        // 只转换版本号的数字-数字边界，保留品牌名连字符
        assert_eq!(map_model_to_kiro("claude-haiku-4-5"), "claude-haiku-4.5");
        assert_eq!(map_model_to_kiro("claude-sonnet-4-6"), "claude-sonnet-4.6");
        assert_eq!(map_model_to_kiro("claude-opus-4-8"), "claude-opus-4.8");
        assert_eq!(map_model_to_kiro("auto"), "auto");
        // 大小写归一
        assert_eq!(map_model_to_kiro("Claude-Sonnet-4-5"), "claude-sonnet-4.5");
    }

    #[test]
    fn conversation_id_falls_back_to_uuid_when_session_missing() {
        let provider = Provider {
            id: "test".to_string(),
            name: "Test Kiro".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: Some("third_party".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        let body = json!({
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let out = anthropic_to_kiro(body, &provider, None, None).unwrap();
        let cid = out
            .pointer("/conversationState/conversationId")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(!cid.is_empty());
        // 模型 ID 正确传递
        let model_id = out
            .pointer("/conversationState/currentMessage/userInputMessage/modelId")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(model_id, "claude-sonnet-4.5");
    }

    fn kiro_provider() -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Kiro".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: Some("third_party".to_string()),
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
    fn effort_gated_by_model_caps() {
        let provider = kiro_provider();
        let make_body = |model: &str| {
            json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "thinking": {"type": "enabled"}
            })
        };
        let has_output_config = |out: &Value| {
            out.pointer("/additionalModelRequestFields/output_config")
                .is_some()
        };

        // 未知能力（缓存未命中）：保守性加上 output_config，由 400 重试兑底
        let out = anthropic_to_kiro(make_body("unknown-model-x"), &provider, None, None).unwrap();
        assert!(has_output_config(&out));

        // 明确不支持 effort：不加 output_config
        set_model_caps(
            "claude-haiku-4.5",
            KiroModelCaps {
                supports_thinking: false,
                supports_effort: false,
            },
        );
        let out = anthropic_to_kiro(make_body("claude-haiku-4-5"), &provider, None, None).unwrap();
        assert!(!has_output_config(&out));

        // 明确支持 effort：加 output_config
        set_model_caps(
            "claude-sonnet-4.5",
            KiroModelCaps {
                supports_thinking: true,
                supports_effort: true,
            },
        );
        let out = anthropic_to_kiro(make_body("claude-sonnet-4-5"), &provider, None, None).unwrap();
        assert!(has_output_config(&out));
    }
}
