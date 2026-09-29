//! Responses 密文整流器
//!
//! 同一个 Codex 线程可以在官方和第三方之间来回切，每家 Responses 后端只认自己签发的密文：
//! 推理条目的 `encrypted_content`、压缩条目、函数输出里的加密片段。发送前的清理
//! （`providers::codex_compaction`）只认得出 CC Switch 自己包装的内容；别家原生 Responses
//! 上游签发的密文看不出来源，只能等上游明确说"验不了"之后，去掉请求里的推理条目和加密
//! 片段，对同一家重发一次（先例：thinking 签名整流器）。
//!
//! 主要认上游自证的错误（错误码或固定措辞，取自 opencodex 的实测）。唯一的例外是 Codex 的
//! 原生 Responses 第三方：它们对不认识的压缩条目怎么报错没法枚举，请求里带着看不出来源的
//! 压缩条目时，400 / 422 也重试一次，只换掉压缩条目。

use super::error::ProxyError;
use super::providers::codex_compaction::{
    compaction_item_replay_text, is_compaction_item, is_unrecognized_compaction_item,
    user_message_item,
};
use super::types::RectifierConfig;
use serde_json::{json, Value};

/// ChatGPT 后端解不开函数输出里的加密片段时的原话（HTTP 502）。
const ENCRYPTED_FUNCTION_OUTPUT_REJECTION: &str =
    "Encrypted function output content could not be decrypted or decoded.";

/// 函数输出、agent_message 里去掉的加密片段换成这句。
const ENCRYPTED_PART_PLACEHOLDER: &str = "[encrypted content omitted]";

/// 上游拒绝了请求里的密文，重试前要去掉哪些状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpaqueStateRejection {
    /// 去掉推理条目，函数输出、agent_message 里的加密片段换成占位文字。
    pub reasoning: bool,
    /// 压缩条目换成文字（没有载荷的标记直接去掉）。压缩条目是整段早期对话的唯一载体：
    /// - 官方：只有错误明确点名压缩时才换。第三方回合的压缩由 CC Switch 包装、发送前已经
    ///   转成文字，到得了官方的压缩密文都是官方自己签发的。
    /// - 第三方：一被拒就换。它收到的压缩密文多半是官方签发的（Stack 模式下切换过来）。
    pub compaction: bool,
}

/// 整流结果
#[derive(Debug, Clone, Default)]
pub struct OpaqueStateRectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 去掉的推理条目数量
    pub removed_reasoning_items: usize,
    /// 换成文字或去掉的压缩条目数量
    pub replaced_compaction_items: usize,
    /// 换成占位文字的加密片段数量
    pub replaced_encrypted_parts: usize,
}

/// 上游是不是因为验不了请求里的密文而拒绝。受整流器总开关管辖。
///
/// `codex_third_party`：Codex 原样转发给非官方的原生 Responses 上游。
pub fn detect_opaque_state_rejection(
    error: &ProxyError,
    config: &RectifierConfig,
    request: &Value,
    codex_third_party: bool,
) -> Option<OpaqueStateRejection> {
    if !config.enabled {
        return None;
    }
    let ProxyError::UpstreamError { status, body } = error else {
        return None;
    };

    if let Some(body) = body {
        let payload = serde_json::from_str::<Value>(body).ok();
        let messages = payload.as_ref().map(error_messages).unwrap_or_default();
        let rejected = match *status {
            400..=499 => {
                is_encrypted_function_output_rejection(body, &messages)
                    || payload.as_ref().is_some_and(is_coded_rejection)
                    || messages.iter().any(|message| is_rejection_message(message))
            }
            // 函数输出里的加密片段解不开时 ChatGPT 后端回 502，只认这一种。
            502 => is_encrypted_function_output_rejection(body, &messages),
            _ => false,
        };
        if rejected {
            return Some(OpaqueStateRejection {
                reasoning: true,
                compaction: codex_third_party
                    || messages
                        .iter()
                        .any(|message| message.contains("compaction")),
            });
        }
    }

    // 非官方网关不认识官方的压缩条目时，报错措辞各家不同，也可能根本不是 JSON。请求里带着
    // 看不出来源的压缩条目才会走到这里（跨供应商切换并压缩过之后），每个请求最多多一次；
    // 只换压缩条目，推理条目照旧发，不白白丢掉这家自己的推理连续性。
    let unrecognized_compaction = request
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(is_unrecognized_compaction_item));
    (codex_third_party && matches!(*status, 400 | 422) && unrecognized_compaction).then_some(
        OpaqueStateRejection {
            reasoning: false,
            compaction: true,
        },
    )
}

/// 错误体里可能放错误原文的几个位置：`error.message`、平铺的 `message`、
/// 字符串形式的 `error`（xAI）、`detail`。
fn error_messages(payload: &Value) -> Vec<&str> {
    [
        payload.pointer("/error/message"),
        payload.get("message"),
        payload.get("error"),
        payload.get("detail"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .collect()
}

fn is_encrypted_function_output_rejection(body: &str, messages: &[&str]) -> bool {
    body.trim() == ENCRYPTED_FUNCTION_OUTPUT_REJECTION
        || messages.contains(&ENCRYPTED_FUNCTION_OUTPUT_REJECTION)
}

fn is_coded_rejection(payload: &Value) -> bool {
    // OpenAI：{"error":{"type":"invalid_request_error","code":"invalid_encrypted_content",...}}
    if payload.pointer("/error/code").and_then(Value::as_str) == Some("invalid_encrypted_content") {
        return true;
    }
    // xAI：{"code":"invalid-argument","error":"Could not decrypt the provided encrypted_content ..."}
    payload.get("code").and_then(Value::as_str) == Some("invalid-argument")
        && payload
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| {
                error.starts_with("Could not decode the compaction blob")
                    || error.starts_with("Could not decrypt the provided encrypted_content")
            })
}

fn is_rejection_message(message: &str) -> bool {
    // ChatGPT 后端不带错误码的原话："The encrypted content ... could not be verified. ..."
    (message.starts_with("The encrypted content") && message.contains("could not be verified"))
        // 推理密文由别的身份签发："reasoning `encrypted_content` was not issued to this caller"
        || (message.contains("was not issued to this caller")
            && (message.contains("encrypted_content") || message.contains("reasoning")))
        // store:false 下按 id 回查推理条目：
        // "Item with id 'rs_…' not found. Items are not persisted when `store` is set to false. ..."
        || (message.contains("not found") && message.contains("Items are not persisted when"))
}

/// 去掉请求里上游可能验不了的状态（按 `rejection` 的两个开关）：
/// - 推理条目整条去掉。它们只携带密文（或一个要回查的 id），被拒时分不清哪条是别家的；
///   去掉后同一段历史每次整流结果相同，重试之间的缓存前缀也稳定。
/// - 函数输出、agent_message 里的加密片段换成占位文字。
/// - 压缩条目换成文字（CC Switch 的摘要解回正文，别家的换成一句说明），没有载荷的
///   标记直接去掉。
///
/// 只动 `input` 里的条目，压缩触发等其他条目原样保留，交给转发时的常规处理。
pub fn rectify_opaque_state(
    body: &mut Value,
    rejection: OpaqueStateRejection,
) -> OpaqueStateRectifyResult {
    let mut result = OpaqueStateRectifyResult::default();
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return result;
    };

    let mut rectified = Vec::with_capacity(items.len());
    for mut item in std::mem::take(items) {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match item_type.as_str() {
            "reasoning" if rejection.reasoning => {
                result.removed_reasoning_items += 1;
                continue;
            }
            "function_call_output" | "custom_tool_call_output" if rejection.reasoning => {
                result.replaced_encrypted_parts += replace_encrypted_parts(item.get_mut("output"));
            }
            "agent_message" if rejection.reasoning => {
                result.replaced_encrypted_parts += replace_encrypted_parts(item.get_mut("content"));
            }
            _ if rejection.compaction && is_compaction_item(&item) => {
                result.replaced_compaction_items += 1;
                if let Some(text) = compaction_item_replay_text(&item) {
                    rectified.push(user_message_item(&text));
                }
                continue;
            }
            _ => {}
        }
        rectified.push(item);
    }
    *items = rectified;

    result.applied = result.removed_reasoning_items
        + result.replaced_compaction_items
        + result.replaced_encrypted_parts
        > 0;
    result
}

fn replace_encrypted_parts(parts: Option<&mut Value>) -> usize {
    let Some(parts) = parts.and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut replaced = 0;
    for part in parts {
        let encrypted = part.get("type").and_then(Value::as_str) == Some("encrypted_content")
            && part
                .get("encrypted_content")
                .and_then(Value::as_str)
                .is_some_and(|content| !content.is_empty());
        if encrypted {
            *part = json!({ "type": "input_text", "text": ENCRYPTED_PART_PLACEHOLDER });
            replaced += 1;
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::codex_compaction::{
        encode_compaction_summary, OPAQUE_COMPACTION_NOTE, SUMMARY_PREFIX,
    };

    const OFFICIAL: bool = false;
    const THIRD_PARTY: bool = true;

    const ALL_STATE: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: true,
        compaction: true,
    };
    const REASONING_ONLY: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: true,
        compaction: false,
    };
    const COMPACTION_ONLY: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: false,
        compaction: true,
    };

    fn upstream(status: u16, body: Value) -> ProxyError {
        ProxyError::UpstreamError {
            status,
            body: Some(body.to_string()),
        }
    }

    fn detect(error: &ProxyError) -> Option<OpaqueStateRejection> {
        detect_opaque_state_rejection(error, &RectifierConfig::default(), &json!({}), OFFICIAL)
    }

    fn detect_with(
        error: &ProxyError,
        request: &Value,
        codex_third_party: bool,
    ) -> Option<OpaqueStateRejection> {
        detect_opaque_state_rejection(
            error,
            &RectifierConfig::default(),
            request,
            codex_third_party,
        )
    }

    #[test]
    fn detects_self_identified_rejections() {
        let coded = upstream(
            400,
            json!({ "error": { "type": "invalid_request_error", "code": "invalid_encrypted_content",
                               "message": "Encrypted content is invalid." } }),
        );
        assert_eq!(detect(&coded), Some(REASONING_ONLY));

        let unverifiable = upstream(
            400,
            json!({ "error": { "type": "invalid_request_error", "code": null,
                               "message": "The encrypted content gAAA could not be verified. Reason: Encrypted content could not be decrypted or parsed." } }),
        );
        assert!(detect(&unverifiable).is_some());

        let caller = upstream(
            400,
            json!({ "error": { "type": "invalid_request_error",
                               "message": "reasoning `encrypted_content` was not issued to this caller" } }),
        );
        assert!(detect(&caller).is_some());

        let not_found = upstream(
            404,
            json!({ "error": { "type": "invalid_request_error", "param": "input",
                               "message": "Item with id 'rs_resp_1' not found. Items are not persisted when `store` is set to false. Try again with `store` to `true`, or remove this item from your input." } }),
        );
        assert!(detect(&not_found).is_some());

        let xai = upstream(
            400,
            json!({ "code": "invalid-argument", "error": "Could not decode the compaction blob: bad" }),
        );
        assert_eq!(detect(&xai), Some(ALL_STATE));

        let function_output = ProxyError::UpstreamError {
            status: 502,
            body: Some(ENCRYPTED_FUNCTION_OUTPUT_REJECTION.to_string()),
        };
        assert!(detect(&function_output).is_some());
    }

    #[test]
    fn ignores_unrelated_errors_and_respects_master_switch() {
        let unrelated = upstream(
            400,
            json!({ "error": { "type": "invalid_request_error", "message": "Invalid value for 'model'." } }),
        );
        assert_eq!(detect(&unrelated), None);

        // 5xx 只认函数输出那一种原话。
        let server = upstream(
            500,
            json!({ "error": { "code": "invalid_encrypted_content" } }),
        );
        assert_eq!(detect(&server), None);

        let coded = upstream(
            400,
            json!({ "error": { "code": "invalid_encrypted_content" } }),
        );
        let disabled = RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        };
        assert_eq!(
            detect_opaque_state_rejection(&coded, &disabled, &json!({}), THIRD_PARTY),
            None
        );
        assert_eq!(detect(&ProxyError::Timeout("slow".to_string())), None);
    }

    #[test]
    fn third_party_rejections_also_replace_compaction() {
        // 第三方收到的压缩密文多半是官方签发的：自证的拒绝不点名压缩也一起换。
        let coded = upstream(
            400,
            json!({ "error": { "code": "invalid_encrypted_content" } }),
        );
        assert_eq!(
            detect_with(&coded, &json!({}), THIRD_PARTY),
            Some(ALL_STATE)
        );
        assert_eq!(
            detect_with(&coded, &json!({}), OFFICIAL),
            Some(REASONING_ONLY)
        );
    }

    #[test]
    fn third_party_unrecognized_compaction_retries_on_any_bad_request() {
        let foreign = json!({ "input": [
            { "type": "message", "role": "user", "content": "hi" },
            { "type": "compaction", "encrypted_content": "gAAAA-openai" }
        ] });
        let marker = json!({ "input": [{ "type": "context_compaction" }] });
        let own = json!({ "input": [
            { "type": "compaction", "encrypted_content": encode_compaction_summary("done") }
        ] });
        let unknown_type = upstream(
            400,
            json!({ "error": { "message": "unsupported input item type: compaction" } }),
        );
        let plain_text = ProxyError::UpstreamError {
            status: 422,
            body: Some("bad input".to_string()),
        };

        assert_eq!(
            detect_with(&unknown_type, &foreign, THIRD_PARTY),
            Some(COMPACTION_ONLY)
        );
        assert_eq!(
            detect_with(&plain_text, &marker, THIRD_PARTY),
            Some(COMPACTION_ONLY)
        );
        // 官方、没有看不出来源的压缩条目、别的状态码：都不碰。
        assert_eq!(detect_with(&unknown_type, &foreign, OFFICIAL), None);
        assert_eq!(detect_with(&unknown_type, &own, THIRD_PARTY), None);
        assert_eq!(
            detect_with(&upstream(404, json!({})), &foreign, THIRD_PARTY),
            None
        );
        assert_eq!(
            detect_with(&upstream(500, json!({})), &foreign, THIRD_PARTY),
            None
        );
    }

    fn mixed_history() -> Value {
        json!({
            "model": "gpt-5.5",
            "store": false,
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "compaction", "id": "cmp_1", "encrypted_content": "gAAAA-openai" },
                { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-other-org" },
                { "type": "reasoning", "id": "rs_resp_chat", "summary": [{ "type": "summary_text", "text": "t" }] },
                { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "shell", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_1", "output": [
                    { "type": "input_text", "text": "ok" },
                    { "type": "encrypted_content", "encrypted_content": "enc_opaque" }
                ] },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "next" }] }
            ]
        })
    }

    #[test]
    fn rectify_drops_reasoning_and_keeps_official_compaction() {
        let mut body = mixed_history();
        let result = rectify_opaque_state(&mut body, REASONING_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_reasoning_items, 2);
        assert_eq!(result.replaced_encrypted_parts, 1);
        assert_eq!(result.replaced_compaction_items, 0);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 5);
        assert!(input.iter().all(|item| item["type"] != "reasoning"));
        assert_eq!(input[1]["encrypted_content"], "gAAAA-openai");
        assert_eq!(
            input[3]["output"][1],
            json!({ "type": "input_text", "text": ENCRYPTED_PART_PLACEHOLDER })
        );
        assert_eq!(input[3]["output"][0]["text"], "ok");
    }

    #[test]
    fn rectify_replaces_compaction_when_asked() {
        let mut body = mixed_history();
        body["input"].as_array_mut().unwrap().push(
            json!({ "type": "compaction", "encrypted_content": encode_compaction_summary("done") }),
        );
        let result = rectify_opaque_state(&mut body, ALL_STATE);
        assert_eq!(result.replaced_compaction_items, 2);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input[1]["content"][0]["text"], OPAQUE_COMPACTION_NOTE);
        assert_eq!(
            input.last().unwrap()["content"][0]["text"],
            format!("{SUMMARY_PREFIX}\ndone")
        );
    }

    #[test]
    fn compaction_only_rectify_keeps_reasoning_and_drops_markers() {
        let mut body = mixed_history();
        body["input"]
            .as_array_mut()
            .unwrap()
            .insert(2, json!({ "type": "context_compaction" }));
        let result = rectify_opaque_state(&mut body, COMPACTION_ONLY);
        assert!(result.applied);
        assert_eq!(result.replaced_compaction_items, 2);
        assert_eq!(result.removed_reasoning_items, 0);
        assert_eq!(result.replaced_encrypted_parts, 0);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 7);
        assert_eq!(input[1]["content"][0]["text"], OPAQUE_COMPACTION_NOTE);
        assert!(input
            .iter()
            .all(|item| item["type"] != "context_compaction"));
        assert_eq!(
            input
                .iter()
                .filter(|item| item["type"] == "reasoning")
                .count(),
            2
        );
        assert_eq!(input[5]["output"][1]["type"], "encrypted_content");
    }

    #[test]
    fn rectify_is_a_no_op_without_opaque_state() {
        let mut body = json!({
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "compaction_trigger" }
            ]
        });
        let before = body.clone();
        let result = rectify_opaque_state(&mut body, ALL_STATE);
        assert!(!result.applied);
        assert_eq!(body, before);
    }
}
