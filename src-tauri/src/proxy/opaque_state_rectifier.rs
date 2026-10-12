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
//!
//! 子 agent 的任务是例外：Codex 新版协作工具把主 agent 派的任务交给主 agent 那家后端加密，
//! 放在请求末尾的 `agent_message` 里。去掉它子 agent 就没了任务，所以这一条不动；上游解不开时
//! 返回一个说明原因的错误（[`unreadable_agent_task_error`]），不让子 agent 拿空任务干活。

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

/// 子 agent 读不到任务时返回给 Codex 的错误码（与 opencodex 的同名错误一致）。
const UNREADABLE_AGENT_TASK_CODE: &str = "unreadable_encrypted_agent_task";

/// 子 agent 读不到任务时返回给 Codex 的说明。
const UNREADABLE_AGENT_TASK_MESSAGE: &str = "子 agent 的任务由主 agent 那家供应商加密，当前供应商解不开（主、子 agent 不在同一家）。可以在 CC Switch 设置 → 应用配置 → Codex 里打开「聚合模式下子 agent 用经典工具」后重启 Codex，或让子 agent 用和主 agent 同一家的模型。 (The sub-agent task was encrypted by the main agent's provider and this provider cannot read it. Turn on \"Classic sub-agent tools in aggregation\" under CC Switch Settings > App config > Codex and restart Codex, or give the sub-agent a model from the main agent's provider.)";

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
            400..=499 => is_self_identified_rejection(body, payload.as_ref(), &messages, request),
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

/// 上游的错误体是不是自证验不了请求里的密文（错误码或固定措辞）。
fn is_self_identified_rejection(
    body: &str,
    payload: Option<&Value>,
    messages: &[&str],
    request: &Value,
) -> bool {
    is_encrypted_function_output_rejection(body, messages)
        || payload.is_some_and(is_coded_rejection)
        || messages.iter().any(|message| is_rejection_message(message))
        || (messages
            .iter()
            .any(|message| is_content_array_rejection(message))
            && carries_plaintext_reasoning(request))
}

/// 流里还没有输出就到了的失败事件，它的 `error` 对象是不是自证验不了请求里的密文。是的话返回
/// 当作 HTTP 400 的上游错误，交给 [`detect_opaque_state_rejection`] 走和 HTTP 报错一样的整流；
/// 不是返回 `None`，流照常交给客户端。
pub fn in_stream_rejection(error: &Value, request: &Value) -> Option<ProxyError> {
    let payload = json!({ "error": error });
    let body = payload.to_string();
    let messages = error_messages(&payload);
    is_self_identified_rejection(&body, Some(&payload), &messages, request).then(|| {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(body.clone()),
        }
    })
}

/// 请求末尾的条目：Codex 把这一轮的新输入放在最后，后面只可能跟着压缩触发、工具清单这类
/// 附加条目（与 opencodex `findEnvelope` 同一规则）。
fn current_input_index(items: &[Value]) -> Option<usize> {
    items.iter().rposition(|item| {
        !matches!(
            item.get("type").and_then(Value::as_str),
            Some("compaction_trigger" | "additional_tools")
        )
    })
}

/// 这一轮的新输入是不是一条带密文的 `agent_message`：Codex 新版协作工具派给子 agent 的
/// 任务，只有主 agent 那家后端解得开。
pub fn carries_encrypted_agent_task(request: &Value) -> bool {
    let Some(items) = request.get("input").and_then(Value::as_array) else {
        return false;
    };
    current_input_index(items).is_some_and(|index| {
        let item = &items[index];
        item.get("type").and_then(Value::as_str) == Some("agent_message")
            && item
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| parts.iter().any(is_encrypted_part))
    })
}

/// 子 agent 读不到任务：上游解不开请求末尾那条任务的密文。HTTP 400，Codex 不会重试。
pub fn unreadable_agent_task_error() -> ProxyError {
    ProxyError::UpstreamError {
        status: 400,
        body: Some(
            json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": UNREADABLE_AGENT_TASK_CODE,
                    "message": UNREADABLE_AGENT_TASK_MESSAGE,
                }
            })
            .to_string(),
        ),
    }
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

/// 去掉请求里上游可能验不了的状态（按 `rejection` 的两个开关）：
/// - 推理条目整条去掉。它们只携带密文（或一个要回查的 id），被拒时分不清哪条是别家的；
///   去掉后同一段历史每次整流结果相同，重试之间的缓存前缀也稳定。
/// - 函数输出、agent_message 里的加密片段换成占位文字；请求末尾那条 agent_message（这一轮
///   派给子 agent 的任务）不动。
/// - 消息、函数调用的 id 不是 OpenAI 格式的（别家签发的）去掉，和推理条目同一个开关：两者都
///   来自别家回合，被拒时一次处理掉，一次重试就够。
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

    let current = current_input_index(items);
    let mut rectified = Vec::with_capacity(items.len());
    for (index, mut item) in std::mem::take(items).into_iter().enumerate() {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match item_type.as_str() {
            // 这一轮派给子 agent 的任务：换掉密文它就没了任务，原样保留（见模块说明）。
            "agent_message" if Some(index) == current => {}
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
        > 0;
    result
}

fn is_encrypted_part(part: &Value) -> bool {
    part.get("type").and_then(Value::as_str) == Some("encrypted_content")
        && part
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty())
}

fn replace_encrypted_parts(parts: Option<&mut Value>) -> usize {
    let Some(parts) = parts.and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut replaced = 0;
    for part in parts {
        if is_encrypted_part(part) {
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
            Some(ALL_STATE)
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

    fn agent_message(ciphertext: &str) -> Value {
        json!({ "type": "agent_message", "author": "/root", "recipient": "/root/worker", "content": [
            { "type": "input_text", "text": "Message Type: NEW_TASK\nPayload:\n" },
            { "type": "encrypted_content", "encrypted_content": ciphertext }
        ] })
    }

    /// 末尾那条 agent_message 是这一轮派给子 agent 的任务，原样保留；更早的照旧换成占位文字。
    /// 末尾之后的压缩触发、工具清单不算这一轮的输入。
    #[test]
    fn rectify_keeps_the_current_sub_agent_task() {
        for trailer in [None, Some("compaction_trigger"), Some("additional_tools")] {
            let mut input = vec![
                agent_message("gAAAA-older-task"),
                json!({ "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA" }),
                agent_message("gAAAA-current-task"),
            ];
            if let Some(trailer) = trailer {
                input.push(json!({ "type": trailer }));
            }
            let mut body = json!({ "input": input });
            assert!(carries_encrypted_agent_task(&body), "{trailer:?}");

            let result = rectify_opaque_state(&mut body, REASONING_ONLY);
            assert_eq!(result.removed_reasoning_items, 1);
            assert_eq!(result.replaced_encrypted_parts, 1);
            let input = body["input"].as_array().unwrap();
            assert_eq!(
                input[0]["content"][1],
                json!({ "type": "input_text", "text": ENCRYPTED_PART_PLACEHOLDER })
            );
            assert_eq!(
                input[1]["content"][1]["encrypted_content"],
                "gAAAA-current-task"
            );
        }
    }

    #[test]
    fn only_a_trailing_encrypted_agent_message_is_a_sub_agent_task() {
        let user = json!({ "role": "user", "content": [{ "type": "input_text", "text": "next" }] });
        let plaintext = json!({ "type": "agent_message", "content": [
            { "type": "input_text", "text": "do it" }
        ] });
        for input in [
            json!([agent_message("gAAAA"), user]),
            json!([plaintext]),
            json!([agent_message("")]),
            json!([]),
        ] {
            assert!(
                !carries_encrypted_agent_task(&json!({ "input": input })),
                "{input}"
            );
        }
        assert!(!carries_encrypted_agent_task(&json!({})));
    }

    #[test]
    fn in_stream_failures_follow_the_same_rejection_rules() {
        let request = json!({ "input": [agent_message("gAAAA")] });
        let rejection = in_stream_rejection(
            &json!({ "code": "server_error", "message": ENCRYPTED_FUNCTION_OUTPUT_REJECTION }),
            &request,
        )
        .expect("rejection");
        assert_eq!(
            detect_with(&rejection, &request, THIRD_PARTY),
            Some(ALL_STATE)
        );
        assert!(in_stream_rejection(
            &json!({ "code": "invalid_encrypted_content", "message": "x" }),
            &request
        )
        .is_some());
        assert!(in_stream_rejection(
            &json!({ "code": "server_error", "message": "backend exploded" }),
            &request
        )
        .is_none());
    }

    #[test]
    fn the_unreadable_task_error_is_a_non_retryable_explanation() {
        let ProxyError::UpstreamError {
            status,
            body: Some(body),
        } = unreadable_agent_task_error()
        else {
            panic!("upstream error");
        };
        assert_eq!(status, 400);
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["error"]["code"], UNREADABLE_AGENT_TASK_CODE);
        assert_eq!(body["error"]["message"], UNREADABLE_AGENT_TASK_MESSAGE);
        // 自己的说明不能又被当成别家密文的拒绝，引出一轮整流重试。
        let error = unreadable_agent_task_error();
        let request = json!({ "input": [agent_message("gAAAA")] });
        assert_eq!(detect_with(&error, &request, THIRD_PARTY), None);
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
