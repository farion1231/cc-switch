//! Responses 密文整流器
//!
//! 同一个 Codex 线程可以在官方和第三方之间来回切，每家 Responses 后端只认自己签发的密文：
//! 推理条目的 `encrypted_content`、压缩条目、函数输出里的加密片段。发送前的清理
//! （`providers::codex_compaction`）只认得出 CC Switch 自己包装的内容；别家原生 Responses
//! 上游签发的密文看不出来源，只能等上游明确说"验不了"之后，去掉请求里的推理条目和加密
//! 片段，对同一家重发一次（先例：thinking 签名整流器）。
//!
//! 推理条目也可能不带密文、直接把思考原文放在 `content` 里（MiniMax），OpenAI 系上游同样不收，
//! 按同一套规则去掉推理条目重发。
//!
//! 主要认上游自证的错误（错误码或固定措辞，取自 opencodex 的实测）。唯一的例外是 Codex 的
//! 原生 Responses 第三方：它们对不认识的压缩条目怎么报错没法枚举，请求里带着看不出来源的
//! 压缩条目时，400 / 422 也重试一次，只换掉压缩条目。

use super::error::ProxyError;
use super::providers::codex_compaction::{
    compaction_item_replay_text, is_compaction_item, is_unrecognized_compaction_item,
    strip_mismatched_item_id, user_message_item,
};
use super::types::RectifierConfig;
use serde_json::{json, Value};

/// ChatGPT 后端解不开函数输出里的加密片段时的原话（HTTP 502）。
const ENCRYPTED_FUNCTION_OUTPUT_REJECTION: &str =
    "Encrypted function output content could not be decrypted or decoded.";

/// 函数输出、agent_message 里去掉的加密片段换成这句。
const ENCRYPTED_PART_PLACEHOLDER: &str = "[encrypted content omitted]";

/// 游标"查不到"类措辞：上游回查自家签发的历史找不到这个 id 的具体表现。整流只认这种
/// 正向证据——错误信息仅仅点名游标字段还不够，用法 / 能力协商错误（和 `conversation` 同时使用、
/// endpoint 不支持 continuation）同样会点名游标，但不该靠改写请求来绕过。
const CURSOR_LOOKUP_FAILURE_TERMS: &[&str] = &["not found", "does not exist", "unknown response"];

/// 上游拒绝了请求里的密文，重试前要去掉哪些状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpaqueStateRejection {
    /// 清掉别家回合留下的状态：去掉推理条目，函数输出、agent_message 里的加密片段换成
    /// 占位文字，前缀和条目类型对不上的 id 去掉。
    pub reasoning: bool,
    /// 压缩条目换成文字（没有载荷的标记直接去掉）。压缩条目是整段早期对话的唯一载体：
    /// - 官方：只有错误明确点名压缩时才换。第三方回合的压缩由 CC Switch 包装、发送前已经
    ///   转成文字，到得了官方的压缩密文都是官方自己签发的。
    /// - 第三方：一被拒就换。它收到的压缩密文多半是官方签发的（Stack 模式下切换过来）。
    pub compaction: bool,
    /// 去掉请求顶层的 `previous_response_id`。它是续聊指针而不是密文，语义独立成一个维度：
    /// 上游只是不认这个游标时，这家自己签发的推理条目多半还查得到，不该跟着被清；反过来
    /// 密文类被拒时，指向这家历史的游标也不该被殃及。这里只负责识别该不该删；删掉之后
    /// 请求是否仍有完整历史，由转发层拿压缩回合的见证记录（`CompactionReplayStore`）核对，
    /// 核对不上就保留游标、原样返回查找错误，不制造一个可能被上游照收却丢上下文的请求。
    pub previous_response_id: bool,
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
    /// 去掉的别家格式条目 id 数量
    pub removed_foreign_ids: usize,
    /// 去掉的响应游标数量（`previous_response_id`）
    pub removed_previous_response_id: usize,
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
                    || (messages
                        .iter()
                        .any(|message| is_content_array_rejection(message))
                        && carries_plaintext_reasoning(request))
            }
            // 函数输出里的加密片段解不开时 ChatGPT 后端回 502，只认这一种。
            502 => is_encrypted_function_output_rejection(body, &messages),
            _ => false,
        };
        // 5xx 是上游自己的故障，按常规重试 / 故障转移处理；删续聊指针只允许在 4xx 的
        // 明确拒绝上进行，故障状态码即便错误文本点名游标也不动请求。这里只是纯识别；
        // 删掉之后请求是否仍有完整历史，由转发层拿压缩回合的见证记录核对后再决定。
        let cursor_rejected = matches!(*status, 400..=499)
            && payload.as_ref().is_some_and(|payload| {
                is_previous_response_id_rejection(payload, &messages, request)
            });
        if rejected || cursor_rejected {
            return Some(OpaqueStateRejection {
                reasoning: rejected,
                compaction: rejected
                    && (codex_third_party
                        || messages
                            .iter()
                            .any(|message| message.contains("compaction"))),
                previous_response_id: cursor_rejected,
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
            previous_response_id: false,
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
        // 别家条目 id 的格式不对："Invalid 'input[19].id': '…_msg_35'. Expected an ID that begins with 'msg'."
        || (message.contains("Invalid 'input[") && message.contains("Expected an ID that begins with"))
        // store:false 下按 id 回查推理条目：
        // "Item with id 'rs_…' not found. Items are not persisted when `store` is set to false. ..."
        || (message.contains("not found") && message.contains("Items are not persisted when"))
}

/// 上游不认请求里的 `previous_response_id`：错误要同时给出两样证据——字段定位（结构化
/// `error.param` 指向游标，或消息里点名 `previous_response_id`）和"查不到"的原因
/// （`CURSOR_LOOKUP_FAILURE_TERMS`），且请求里真的带着非空游标。只点名字段、原因却是
/// 用法 / 能力协商（和 `conversation` 同时使用、endpoint 不支持 continuation）的不算：
/// 那类错误不该靠删掉游标来绕过。
///
/// 可达性：游标维度的输入前提是请求自带非空游标。当前公开 Codex 的 HTTP 请求体
/// （codex-api `ResponsesApiRequest`）不含 `previous_response_id`——该字段只随 Responses
/// WebSocket 的 `response.create`（`ResponseCreateWsRequest`）发送，而本地代理对 WS 升级
/// 返回 426、Codex 随即改走 HTTP。因此标准 Codex HTTP 路径在这里必然 fail-closed；本维度
/// 实际覆盖的是任何携带游标的兼容客户端。
fn is_previous_response_id_rejection(payload: &Value, messages: &[&str], request: &Value) -> bool {
    let carries_cursor = request
        .get("previous_response_id")
        .and_then(Value::as_str)
        .is_some_and(|cursor| !cursor.is_empty());
    if !carries_cursor {
        return false;
    }
    let lookup_failure = |message: &&str| {
        let lower = message.to_ascii_lowercase();
        CURSOR_LOOKUP_FAILURE_TERMS
            .iter()
            .any(|term| lower.contains(term))
    };
    if payload.pointer("/error/param").and_then(Value::as_str) == Some("previous_response_id") {
        return messages.iter().any(lookup_failure);
    }
    messages.iter().any(|message| {
        let lower = message.to_ascii_lowercase();
        lower.contains("previous_response_id") && lookup_failure(message)
    })
}

/// OpenAI 不收带内容的推理条目："Invalid 'input[N].content': array too long. Expected an array
/// with maximum length 0, ..."（错误码 `array_above_max_length`）。
fn is_content_array_rejection(message: &str) -> bool {
    message.contains("Invalid 'input[")
        && message.contains(".content': array too long")
        && message.contains("maximum length 0")
}

/// 请求里有把思考原文放在 `content` 里的推理条目（MiniMax 原生 Responses 这样签发）。
fn carries_plaintext_reasoning(request: &Value) -> bool {
    request
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("reasoning")
                    && item
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|content| !content.is_empty())
            })
        })
}

/// 去掉请求里上游可能验不了的状态（按 `rejection` 的三个开关）：
/// - `previous_response_id` 是续聊指针：上游按 id 回查自家签发的历史，跨供应商时一定查不到。
///   它在请求顶层，先于 `input` 处理，不依赖 `input` 存不存在、是不是数组。
/// - 推理条目整条去掉。它们只携带密文（或一个要回查的 id），被拒时分不清哪条是别家的；
///   去掉后同一段历史每次整流结果相同，重试之间的缓存前缀也稳定。
/// - 函数输出、agent_message 里的加密片段换成占位文字。
/// - 消息、函数调用的 id 不是 OpenAI 格式的（别家签发的）去掉，和推理条目同一个开关：两者都
///   来自别家回合，被拒时一次处理掉，一次重试就够。
/// - 压缩条目换成文字（CC Switch 的摘要解回正文，别家的换成一句说明），没有载荷的
///   标记直接去掉。
///
/// `input` 里只动上面几类条目，压缩触发等其他条目原样保留，交给转发时的常规处理。
pub fn rectify_opaque_state(
    body: &mut Value,
    rejection: OpaqueStateRejection,
) -> OpaqueStateRectifyResult {
    let mut result = OpaqueStateRectifyResult::default();

    // 游标在顶层，和 input 无关：没有 input 数组的请求也要能删掉它。null 之类的无效值
    // 不算一次整流。
    if rejection.previous_response_id {
        if let Some(object) = body.as_object_mut() {
            let present = object
                .get("previous_response_id")
                .and_then(Value::as_str)
                .is_some_and(|cursor| !cursor.is_empty());
            if present && object.remove("previous_response_id").is_some() {
                result.removed_previous_response_id += 1;
            }
        }
    }

    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        result.applied = result.removed_previous_response_id > 0;
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
        // 别家签发的 id（MiniMax 的 `<hex>_msg_N`、`<hex>_fc_N`）不合 OpenAI 的前缀校验
        if rejection.reasoning && strip_mismatched_item_id(&mut item, &item_type) {
            result.removed_foreign_ids += 1;
        }
        rectified.push(item);
    }
    *items = rectified;

    result.applied = result.removed_reasoning_items
        + result.replaced_compaction_items
        + result.replaced_encrypted_parts
        + result.removed_foreign_ids
        + result.removed_previous_response_id
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

    /// 一并清掉密文类状态（推理条目、加密片段、别家 id）和压缩条目——自证的拒绝在第三方
    /// 上游下的形态（`detect` 输出），不含游标维度。
    const REASONING_AND_COMPACTION: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: true,
        compaction: true,
        previous_response_id: false,
    };
    /// 三个维度全开，给整流函数本身的测试用。
    const ALL_STATE: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: true,
        compaction: true,
        previous_response_id: true,
    };
    const REASONING_ONLY: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: true,
        compaction: false,
        previous_response_id: false,
    };
    const COMPACTION_ONLY: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: false,
        compaction: true,
        previous_response_id: false,
    };
    /// 上游只是不认请求里的游标：清它，别的不碰。
    const CURSOR_ONLY: OpaqueStateRejection = OpaqueStateRejection {
        reasoning: false,
        compaction: false,
        previous_response_id: true,
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
        assert_eq!(detect(&xai), Some(REASONING_AND_COMPACTION));

        let function_output = ProxyError::UpstreamError {
            status: 502,
            body: Some(ENCRYPTED_FUNCTION_OUTPUT_REJECTION.to_string()),
        };
        assert!(detect(&function_output).is_some());
    }

    /// MiniMax 的推理条目把思考原文放在 `content` 里，OpenAI 系上游不收（实测：packycode 转
    /// gpt-6-astra，2026-10-03）。只有请求里真带着这种条目才认，同一句话碰上别的 content 不算。
    #[test]
    fn detects_plaintext_reasoning_content_rejection() {
        let rejection = upstream(
            400,
            json!({ "error": { "type": "packy_invalid_request_error", "code": "invalid_request_error", "param": "",
                               "message": "[ArrayParam] [input[2].content] [array_above_max_length] Invalid 'input[2].content': array too long. Expected an array with maximum length 0, but got an array with length 1 instead. (request id: 01M40DR9R08MZWH7F7XEF2PWHY)" } }),
        );
        let plaintext = json!({ "input": [
            { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "ok" }] },
            { "type": "reasoning", "id": "070fee_rs", "summary": [],
              "content": [{ "type": "reasoning_text", "text": "thinking" }] }
        ] });
        assert_eq!(
            detect_with(&rejection, &plaintext, OFFICIAL),
            Some(REASONING_ONLY)
        );
        assert_eq!(
            detect_with(&rejection, &plaintext, THIRD_PARTY),
            Some(REASONING_AND_COMPACTION)
        );

        let encrypted_only = json!({ "input": [
            { "type": "reasoning", "id": "rs_1", "summary": [], "content": null, "encrypted_content": "gAAAA" },
            { "type": "reasoning", "id": "rs_2", "summary": [], "content": [] }
        ] });
        assert_eq!(detect_with(&rejection, &encrypted_only, THIRD_PARTY), None);

        let foreign_id = upstream(
            400,
            json!({ "error": { "code": "invalid_request_error",
                               "message": "[ApiIdParam] [input[19].id] [invalid_id_prefix] Invalid 'input[19].id': '070ff2d8f785aadf67bc4cd4c344154b_msg_35'. Expected an ID that begins with 'msg'." } }),
        );
        assert_eq!(detect(&foreign_id), Some(REASONING_ONLY));
    }

    /// 一次重试要同时清掉 MiniMax 回合留下的两样东西：带原文的推理条目、非 OpenAI 格式的 id。
    #[test]
    fn rectify_clears_minimax_turns_in_one_pass() {
        let mut body = json!({ "input": [
            { "type": "message", "id": "msg_01a1", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "reasoning", "id": "070ff2_rs_1", "summary": [],
              "content": [{ "type": "reasoning_text", "text": "thinking" }] },
            { "type": "function_call", "id": "070ff2_fc_2", "call_id": "call_9", "name": "exec_command", "arguments": "{}" },
            { "type": "function_call_output", "id": "fco_01a1", "call_id": "call_9", "output": "ok" },
            { "type": "message", "id": "070ff2_msg_3", "role": "assistant", "content": [{ "type": "output_text", "text": "done" }] },
            { "type": "function_call", "id": "fc_grok_0", "call_id": "call_1", "name": "exec_command", "arguments": "{}" }
        ] });
        let result = rectify_opaque_state(&mut body, REASONING_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_reasoning_items, 1);
        assert_eq!(result.removed_foreign_ids, 2);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 5);
        assert_eq!(input[0]["id"], "msg_01a1");
        assert!(input[1].get("id").is_none());
        assert_eq!(input[1]["call_id"], "call_9");
        assert_eq!(input[2]["id"], "fco_01a1");
        assert!(input[3].get("id").is_none());
        assert_eq!(input[4]["id"], "fc_grok_0");

        // 只换压缩条目的重试不碰 id。
        let mut body = json!({ "input": [
            { "type": "message", "id": "070ff2_msg_3", "role": "assistant", "content": [] }
        ] });
        assert!(!rectify_opaque_state(&mut body, COMPACTION_ONLY).applied);
        assert_eq!(body["input"][0]["id"], "070ff2_msg_3");
    }

    /// 游标拒绝单独成维：识别要求请求里真带着非空游标，错误同时给出字段定位和"查不到"
    /// 证据；用法 / 能力协商错误不认；整流只删游标、保推理条目。
    #[test]
    fn detects_previous_response_id_rejection_and_removes_cursor_only() {
        let cursor_request = json!({
            "previous_response_id": "resp_foreign",
            "input": [
                { "type": "message", "id": "msg_01a1", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-other" }
            ]
        });
        let cases = [
            r#"{"error":{"message":"Invalid 'previous_response_id': 'resp_68a1b2c3'. Previous response not found."}}"#,
            // 结构化字段路径：param 点名游标，消息再给出"查不到"证据才认。
            r#"{"error":{"param":"previous_response_id","message":"Previous response not found."}}"#,
            r#"{"error":{"param":"previous_response_id","message":"Unknown response id 'resp_68a1b2c3'."}}"#,
        ];
        for body in cases {
            let rejection = upstream(400, serde_json::from_str(body).unwrap());
            assert_eq!(
                detect_with(&rejection, &cursor_request, OFFICIAL),
                Some(CURSOR_ONLY),
                "body={body}"
            );
        }

        // 请求里没有游标可删时不触发：错误文本提到游标也不认。
        let error = upstream(
            400,
            json!({ "error": { "message": "previous_response_id is not found for this key" } }),
        );
        assert_eq!(detect_with(&error, &json!({ "input": [] }), OFFICIAL), None);

        // 用法 / 能力协商错误（和 conversation 同时用、endpoint 不支持这种 continuation）：
        // 只是点名了游标字段，没有"查不到"证据，不触发整流。
        for message in [
            "previous_response_id cannot be used with conversation",
            "previous_response_id is not supported by this endpoint",
            "previous_response_id is invalid for this endpoint",
        ] {
            let error = upstream(
                400,
                json!({ "error": { "param": "previous_response_id", "message": message } }),
            );
            assert_eq!(
                detect_with(&error, &cursor_request, OFFICIAL),
                None,
                "message={message}"
            );
        }

        // 只有字面出现、没有任何拒绝措辞的场合不算。
        let unrelated = upstream(
            400,
            json!({ "error": { "message": "previous_response_id was echoed back in metadata" } }),
        );
        assert_eq!(detect_with(&unrelated, &cursor_request, OFFICIAL), None);

        // 游标被拒：只删游标，这家自己签发的推理条目原样保留。
        let mut body = cursor_request.clone();
        let result = rectify_opaque_state(&mut body, CURSOR_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_previous_response_id, 1);
        assert_eq!(result.removed_reasoning_items, 0);
        assert_eq!(result.replaced_encrypted_parts, 0);
        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().unwrap().len(), 2);

        // 反过来：密文类被拒时，请求里的游标必须留下。
        let mut body = cursor_request.clone();
        let result = rectify_opaque_state(&mut body, REASONING_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_reasoning_items, 1);
        assert_eq!(result.removed_previous_response_id, 0);
        assert_eq!(body["previous_response_id"], "resp_foreign");
    }

    /// 5xx 是上游自己的故障：删续聊指针只允许在 4xx 的明确拒绝上进行，故障类状态码
    /// 即便错误文本点名游标也按常规错误处理（重试 / 故障转移），不改请求。
    #[test]
    fn does_not_touch_cursor_on_5xx() {
        let cursor_request = json!({ "previous_response_id": "resp_own" });
        for status in [500u16, 502, 503] {
            let error = upstream(
                status,
                json!({ "error": { "param": "previous_response_id",
                                   "message": "previous_response_id not found" } }),
            );
            assert_eq!(
                detect_with(&error, &cursor_request, OFFICIAL),
                None,
                "status={status}"
            );
        }

        // 502 的既有例外不受影响：函数输出密文解不开时照旧整流（只清密文类状态）。
        let function_output = ProxyError::UpstreamError {
            status: 502,
            body: Some(ENCRYPTED_FUNCTION_OUTPUT_REJECTION.to_string()),
        };
        assert_eq!(
            detect_with(&function_output, &cursor_request, OFFICIAL),
            Some(REASONING_ONLY)
        );
    }

    /// 游标拒绝的识别只看错误证据和"请求里真带着游标"，与 `input` 形态无关——删游标
    /// 之后历史是否完整不在这层证明：转发层会拿压缩回合的见证记录
    /// （`CompactionReplayStore`）核对，核对不上就 fail-closed、原样返回查找错误。
    #[test]
    fn cursor_rejection_identification_is_independent_of_input_shape() {
        let lookup_error = || {
            upstream(
                400,
                json!({ "error": { "message":
                    "Invalid 'previous_response_id': 'resp_x'. Previous response not found." } }),
            )
        };

        let shapes = [
            // 增量普通 user 回合。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "message", "role": "user",
                  "content": [{ "type": "input_text", "text": "continue from the previous answer" }] }
            ] }),
            // 孤儿工具输出。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "function_call_output", "call_id": "call_123", "output": "done" }
            ] }),
            // 最近一组完整工具对。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "ok" }
            ] }),
            // 局部重放（几条消息 + 推理条目）。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA-other" },
                { "type": "message", "id": "msg_1", "role": "assistant",
                  "content": [{ "type": "output_text", "text": "a" }] },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
            ] }),
            // 没有 input。
            json!({ "previous_response_id": "resp_x" }),
            // 别家的压缩密文。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "compaction", "encrypted_content": "gAAAA-openai" },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
            ] }),
            // CC Switch 包装的完整摘要。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "compaction", "encrypted_content": encode_compaction_summary("prior work") },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
            ] }),
            // 摘要已经换成文字（上一轮整流换掉过）。
            json!({ "previous_response_id": "resp_x", "input": [
                { "type": "message", "role": "user",
                  "content": [{ "type": "input_text", "text": format!("{SUMMARY_PREFIX}\nprior work") }] },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "again" }] }
            ] }),
        ];
        for (index, shape) in shapes.iter().enumerate() {
            assert_eq!(
                detect_with(&lookup_error(), shape, OFFICIAL),
                Some(CURSOR_ONLY),
                "shape={index}"
            );
        }
    }

    /// 游标证据齐备、但请求本身不带游标：不得识别为游标拒绝——没有可删的东西。标准
    /// Codex HTTP 请求体就是这个形态：codex-api `ResponsesApiRequest` 的字段集不含
    /// `previous_response_id`（游标只随 Responses WebSocket 的 `response.create` 发送；
    /// 本地代理对 WS 升级返回 426，Codex 改走 HTTP）。
    #[test]
    fn cursor_evidence_without_a_cursor_in_the_request_is_not_a_cursor_rejection() {
        let error = upstream(
            400,
            json!({ "error": { "message":
                "Invalid 'previous_response_id': 'resp_x'. Previous response not found." } }),
        );

        let requests = [
            // 生产形状：完整字段集里没有游标。
            json!({
                "model": "gpt-5-codex",
                "stream": true,
                "input": [{ "type": "message", "role": "user",
                            "content": [{ "type": "input_text", "text": "hi" }] }],
                "tools": [],
                "tool_choice": "auto",
                "parallel_tool_calls": false,
                "reasoning": { "effort": "medium" },
                "store": false,
                "include": [],
                "prompt_cache_key": "8b1f0b5e-6d3a-4c2f-9f0d-2a7c4e6b8d10"
            }),
            // null / 空串同样不算携带游标。
            json!({ "previous_response_id": null, "input": [] }),
            json!({ "previous_response_id": "", "input": [] }),
        ];
        for (index, request) in requests.iter().enumerate() {
            for third_party in [OFFICIAL, THIRD_PARTY] {
                assert_eq!(
                    detect_with(&error, request, third_party),
                    None,
                    "request={index} third_party={third_party}"
                );
            }
        }
    }

    /// 游标删除不依赖 `input` 的形态：没有 input、空数组、非数组都要能删掉；null 之类的
    /// 无效值不算一次整流。压缩类重试不碰游标。
    #[test]
    fn cursor_removal_ignores_input_shape() {
        let mut body = json!({ "previous_response_id": "resp_only" });
        let result = rectify_opaque_state(&mut body, CURSOR_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_previous_response_id, 1);
        assert!(body.get("previous_response_id").is_none());

        let mut body = json!({ "previous_response_id": "resp_empty", "input": [] });
        assert!(rectify_opaque_state(&mut body, CURSOR_ONLY).applied);
        assert!(body.get("previous_response_id").is_none());

        let mut body = json!({ "previous_response_id": "resp_non_array", "input": "x" });
        assert!(rectify_opaque_state(&mut body, CURSOR_ONLY).applied);
        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"], "x");

        // null 游标：删不删都不算整流（没有可用的东西被去掉）。
        let mut body = json!({ "previous_response_id": null, "input": [] });
        assert!(!rectify_opaque_state(&mut body, CURSOR_ONLY).applied);

        // 只换压缩条目的重试不碰游标。
        let mut body = json!({ "previous_response_id": "resp_keep", "input": [] });
        assert!(!rectify_opaque_state(&mut body, COMPACTION_ONLY).applied);
        assert_eq!(body["previous_response_id"], "resp_keep");
    }

    /// 游标 + 增量函数输出（携带游标的续聊形态，见 providers::codex_chat_history）：
    /// 整流只删游标，不尝试重建只有游标才查得到的历史；剩下的 input 原样交给上游判断。
    #[test]
    fn cursor_removal_keeps_incremental_function_call_output() {
        let mut body = json!({
            "previous_response_id": "resp_provider_a",
            "input": [
                { "type": "function_call_output", "call_id": "call_123", "output": "done" }
            ]
        });
        let result = rectify_opaque_state(&mut body, CURSOR_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_previous_response_id, 1);
        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert_eq!(body["input"][0]["call_id"], "call_123");
        assert_eq!(body["input"][0]["output"], "done");
    }

    /// 别家 id 被拒：同一开关下顺带清 id，游标留着。
    #[test]
    fn foreign_id_rejection_keeps_cursor() {
        let mut body = json!({
            "previous_response_id": "resp_own",
            "input": [
                { "type": "message", "id": "070ff2d8f785aadf67bc4cd4c344154b_msg_35", "role": "assistant", "content": [] }
            ]
        });
        let result = rectify_opaque_state(&mut body, REASONING_ONLY);
        assert!(result.applied);
        assert_eq!(result.removed_foreign_ids, 1);
        assert_eq!(result.removed_previous_response_id, 0);
        assert_eq!(body["previous_response_id"], "resp_own");
        assert!(body["input"][0].get("id").is_none());
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
            Some(REASONING_AND_COMPACTION)
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
