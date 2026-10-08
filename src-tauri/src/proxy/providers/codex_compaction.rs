//! Codex 远程压缩（Responses compaction V2）与第三方模型之间的桥接。
//!
//! Codex 按 provider 名称判断能否远程压缩：名为 "OpenAI" 的 provider 走 V2。Stack 模式下
//! 官方卡的镜像表就叫 "OpenAI"，于是挂在它下面的每个第三方模型也会收到 V2 压缩请求：
//! 一个普通的 `/responses` 流式请求，input 末尾是 `{"type":"compaction_trigger"}`。
//! codex-rs 的 `collect_compaction_output` 要求返回里**恰好一个**
//! `{"type":"compaction","encrypted_content":...}` 条目，否则直接 Fatal。
//!
//! 第三方模型产不出 OpenAI 的密文，所以这里把它当普通摘要模型用：去掉工具、在末尾追加
//! Codex 自己的压缩提示词，再把摘要正文包成 `ccswitch-compaction-v1:` + base64 放进
//! `encrypted_content`。以后 Codex 在历史里回放这个条目时：
//! - 发往 Chat / Anthropic 转换：解回摘要正文，作为用户消息；解不开的（别家的密文）
//!   转换后的格式装不下，换成一句说明，不再静默丢弃。
//! - 发往原生 Responses（官方和第三方）：CC Switch 包装的摘要转成用户消息；别的密文看不出
//!   来源（可能正是这家自己签发的），原样发出，上游明确拒绝后再由 `opaque_state_rectifier`
//!   换掉。

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

pub(crate) const CCSWITCH_COMPACTION_PREFIX: &str = "ccswitch-compaction-v1:";

/// CC Switch 自己包装进 `encrypted_content` 的内容都以它开头，官方后端一律解不开。
const CCSWITCH_ENVELOPE_PREFIX: &str = "ccswitch-";

/// 与 codex-rs `prompts/templates/compact/prompt.md` 一致（Codex 本地压缩用的提示词）。
pub(crate) const COMPACT_PROMPT: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that will resume the task.

Include:
- Current progress and key decisions made
- Important context, constraints, or user preferences
- What remains to be done (clear next steps)
- Any critical data, examples, or references needed to continue

Be concise, structured, and focused on helping the next LLM seamlessly continue the work.
";

/// 与 codex-rs `prompts/templates/compact/summary_prefix.md` 一致；Codex 本地压缩把摘要
/// 写成 `{SUMMARY_PREFIX}\n{summary}` 的用户消息，这里保持同一形态。
pub(crate) const SUMMARY_PREFIX: &str = "Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:";

/// 历史里的压缩条目是别家的密文、当前模型读不了时，用这句话占位。
pub(crate) const OPAQUE_COMPACTION_NOTE: &str =
    "[earlier conversation was compacted; the summary is stored in a format this model cannot read]";

/// Codex 在历史里回放的压缩条目类型（`compaction_summary` 是 `compaction` 的别名）。
pub(crate) fn is_compaction_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("compaction" | "compaction_summary" | "context_compaction")
    )
}

pub(crate) fn is_compaction_trigger(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("compaction_trigger")
}

/// 这个 Responses 请求是不是一次 V2 压缩。
pub(crate) fn is_compaction_request(body: &Value) -> bool {
    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(is_compaction_trigger))
}

pub(crate) fn encode_compaction_summary(summary: &str) -> String {
    format!(
        "{CCSWITCH_COMPACTION_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(summary.as_bytes())
    )
}

/// 解开 CC Switch 包装的摘要；别家密文或损坏的内容返回 None。
pub(crate) fn decode_compaction_summary(encrypted_content: &str) -> Option<String> {
    let encoded = encrypted_content.strip_prefix(CCSWITCH_COMPACTION_PREFIX)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

fn is_ccswitch_envelope(encrypted_content: &str) -> bool {
    encrypted_content.starts_with(CCSWITCH_ENVELOPE_PREFIX)
}

fn compaction_encrypted_content(item: &Value) -> Option<&str> {
    item.get("encrypted_content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// CC Switch 自己包装的压缩条目，解回摘要正文；别的条目返回 None。
fn own_compaction_summary(item: &Value) -> Option<String> {
    if !is_compaction_item(item) {
        return None;
    }
    compaction_encrypted_content(item).and_then(decode_compaction_summary)
}

/// 看不出来源的压缩条目：别家签发的密文，或没有载荷的标记。
pub(crate) fn is_unrecognized_compaction_item(item: &Value) -> bool {
    is_compaction_item(item) && own_compaction_summary(item).is_none()
}

fn own_compaction_message(summary: &str) -> Value {
    user_message_item(&format!("{SUMMARY_PREFIX}\n{summary}"))
}

/// 压缩条目回放给第三方模型时的文字。没有载荷的 `context_compaction` 只是个标记，返回 None。
pub(crate) fn compaction_item_replay_text(item: &Value) -> Option<String> {
    let encrypted_content = compaction_encrypted_content(item)?;
    Some(match decode_compaction_summary(encrypted_content) {
        Some(summary) => format!("{SUMMARY_PREFIX}\n{summary}"),
        None => OPAQUE_COMPACTION_NOTE.to_string(),
    })
}

pub(crate) fn user_message_item(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "user",
        "content": [{ "type": "input_text", "text": text }]
    })
}

/// 压缩请求里 `compaction_trigger` 所在位置换成的那条用户消息。
pub(crate) fn compaction_prompt_item() -> Value {
    user_message_item(COMPACT_PROMPT)
}

/// 压缩回合要交回 Codex 的唯一一个 compaction 条目。
pub(crate) fn compaction_output_item(summary: &str) -> Value {
    json!({
        "type": "compaction",
        "id": format!("cmp_{}", uuid::Uuid::new_v4().simple()),
        "encrypted_content": encode_compaction_summary(summary)
    })
}

/// 从本回合的输出条目里取摘要正文：所有 assistant message 的文字按顺序拼起来。
pub(crate) fn summary_from_output_items<'a>(items: impl IntoIterator<Item = &'a Value>) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            if part.get("type").and_then(Value::as_str) != Some("output_text") {
                continue;
            }
            if let Some(text) = part
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                parts.push(text);
            }
        }
    }
    parts.join("\n\n")
}

/// 转换来的压缩回合（Chat 的 `finish_reason`、Anthropic 的 `stop_reason`）能不能交回
/// 压缩条目：返回 None 表示摘要写完了，否则是 `response.incomplete` 要带的原因。
///
/// 普通回合把内容过滤等非正常结束当作完成，至多少一段回答；压缩回合一旦算完成，Codex
/// 就拿这段摘要覆盖整段历史，所以只认正常停下的几种写法，其余一律报未完成（Codex 会重试）。
/// 没有结束原因时按完成处理，和普通回合一致（流提前断开由各自的流处理合成截断）。
pub(crate) fn compaction_incomplete_reason(finish_reason: Option<&str>) -> Option<String> {
    let reason = finish_reason.map(str::trim).filter(|r| !r.is_empty())?;
    match reason.to_ascii_lowercase().as_str() {
        "stop" | "end_turn" | "stop_sequence" => None,
        "length" | "max_tokens" | "model_context_window_exceeded" => {
            Some("max_output_tokens".to_string())
        }
        // OpenAI 的 content_filter、智谱的 sensitive、Anthropic 的 refusal。
        "content_filter" | "sensitive" | "refusal" => Some("content_filter".to_string()),
        _ => Some(reason.to_string()),
    }
}

/// 原生 Responses 格式的第三方上游：压缩触发是 Codex 私有协议，普通 Responses 网关收到
/// 只会回一条普通消息（Codex 照样 Fatal），这里改成和 Chat / Anthropic 转换一样的摘要回合；
/// CC Switch 包装的压缩摘要谁都解不开，转成用户消息。
///
/// 别的压缩条目看不出来源，可能正是这家自己签发、能正常解开的，原样发出；上游明确拒绝后
/// 由 `opaque_state_rectifier` 换成文字重试。请求里没有前两类条目时一个字节都不动。
pub(crate) fn prepare_native_third_party_request(body: &mut Value) -> bool {
    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return false;
    };
    if !items
        .iter()
        .any(|item| is_compaction_trigger(item) || own_compaction_summary(item).is_some())
    {
        return false;
    }

    let mut compaction = false;
    let rewritten: Vec<Value> = items
        .iter()
        .map(|item| {
            if is_compaction_trigger(item) {
                compaction = true;
                compaction_prompt_item()
            } else if let Some(summary) = own_compaction_summary(item) {
                own_compaction_message(&summary)
            } else {
                item.clone()
            }
        })
        .collect();
    body["input"] = Value::Array(rewritten);

    if compaction {
        strip_tools_for_summary_turn(body);
    }
    true
}

/// 压缩回合只要一段摘要：去掉工具和结构化输出，和 Codex 本地压缩请求的形态一致
/// （codex-rs `compact.rs` 的 Prompt 不带 tools）。
fn strip_tools_for_summary_turn(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.remove("tools");
    obj.remove("tool_choice");
    obj.remove("parallel_tool_calls");
    let text_is_empty = obj
        .get_mut("text")
        .and_then(Value::as_object_mut)
        .map(|text| {
            text.remove("format");
            text.is_empty()
        })
        .unwrap_or(false);
    if text_is_empty {
        obj.remove("text");
    }
}

/// codex-rs `ResponseItem::id_prefix` 与 OpenAI 实测校验一致的类型 → id 前缀。
fn expected_item_id_prefix(item_type: &str) -> Option<&'static str> {
    Some(match item_type {
        "message" => "msg_",
        "agent_message" => "amsg_",
        "reasoning" => "rs_",
        "function_call" => "fc_",
        "custom_tool_call" => "ctc_",
        "tool_search_call" => "tsc_",
        "web_search_call" => "ws_",
        _ => return None,
    })
}

/// id 前缀和条目类型对不上（转换器产出的、别家签发的）就去掉 id，返回是否去掉了。
/// id 在输入里可省略，调用和结果靠 `call_id` 对应。
pub(crate) fn strip_mismatched_item_id(item: &mut Value, item_type: &str) -> bool {
    let Some(prefix) = expected_item_id_prefix(item_type) else {
        return false;
    };
    let bad_id = item
        .get("id")
        .is_some_and(|id| !id.as_str().is_some_and(|id| id.starts_with(prefix)));
    bad_id
        && item
            .as_object_mut()
            .is_some_and(|obj| obj.remove("id").is_some())
}

/// 发往官方（ChatGPT 登录直通）前，清理同一线程里第三方回合留下、官方一定会拒的东西：
/// - CC Switch 包装的压缩摘要：转成用户消息（官方验不了这段"密文"）；
/// - 推理条目带着 CC Switch 包装的内容（Anthropic thinking 签名），或根本没有
///   encrypted_content（Chat 转换产出的）：`store:false` 下官方只能靠密文还原推理，
///   这类条目要么验签失败、要么按 id 查无此条目（404），整条去掉；
/// - id 前缀和条目类型对不上（转换器产出的 `resp_…_msg` 等）：去掉 id。
///
/// 纯官方的线程里这些情况都不会出现，请求一个字节都不变（prompt cache 前缀不受影响）；
/// 同一段混合历史每轮清理结果相同，缓存前缀也稳定。
pub(crate) fn scrub_ccswitch_state_for_official(body: &mut Value) -> bool {
    let stored = body.get("store").and_then(Value::as_bool) == Some(true);
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut changed = false;
    let mut scrubbed = Vec::with_capacity(items.len());
    for mut item in std::mem::take(items) {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        if let Some(summary) = own_compaction_summary(&item) {
            changed = true;
            scrubbed.push(own_compaction_message(&summary));
            continue;
        }

        if item_type == "reasoning" {
            let foreign = match compaction_encrypted_content(&item) {
                Some(encrypted_content) => is_ccswitch_envelope(encrypted_content),
                None => !stored,
            };
            if foreign {
                changed = true;
                continue;
            }
        }

        changed |= strip_mismatched_item_id(&mut item, &item_type);

        scrubbed.push(item);
    }
    *items = scrubbed;
    changed
}

/// 原生 Responses 第三方上游的压缩回合：上游只回了普通消息，在 `response.completed`
/// 之前补上唯一一个 compaction 条目。上游截断（incomplete）时如实报 `response.incomplete`，
/// Codex 会按可重试错误处理，而不是装上半截摘要。
pub(crate) fn create_native_compaction_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = NativeCompactionState::default();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        for event in state.handle_block(&block) {
                            yield Ok(event);
                        }
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    return;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            for event in state.handle_block(&tail) {
                yield Ok(event);
            }
        }
    }
}

#[derive(Default)]
struct NativeCompactionState {
    done_items: Vec<Value>,
    next_output_index: u64,
}

impl NativeCompactionState {
    fn handle_block(&mut self, block: &str) -> Vec<Bytes> {
        let passthrough = || vec![Bytes::from(format!("{block}\n\n"))];
        let data = block
            .lines()
            .filter_map(|line| strip_sse_field(line, "data"))
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(event) = serde_json::from_str::<Value>(&data) else {
            return passthrough();
        };

        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(index) = event.get("output_index").and_then(Value::as_u64) {
                    self.next_output_index = self.next_output_index.max(index + 1);
                }
                if let Some(item) = event.get("item") {
                    self.done_items.push(item.clone());
                }
                passthrough()
            }
            Some("response.completed") => self.finish(event),
            _ => passthrough(),
        }
    }

    fn finish(&mut self, event: Value) -> Vec<Bytes> {
        let mut response = event.get("response").cloned().unwrap_or_else(|| json!({}));
        let status = response
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed");
        if status == "incomplete" {
            return vec![response_event("response.incomplete", response)];
        }

        let mut summary = summary_from_output_items(&self.done_items);
        if summary.is_empty() {
            if let Some(output) = response.get("output").and_then(Value::as_array) {
                summary = summary_from_output_items(output);
            }
        }
        if summary.is_empty() {
            response["status"] = json!("failed");
            response["error"] = json!({
                "type": "compaction_summary_empty",
                "message": "Upstream returned no summary text for the compaction turn"
            });
            return vec![response_event("response.failed", response)];
        }

        let item = compaction_output_item(&summary);
        let output_index = response
            .get("output")
            .and_then(Value::as_array)
            .map(|output| output.len() as u64)
            .unwrap_or(0)
            .max(self.next_output_index);
        let item_done = response_event_with(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item
            }),
        );
        match response.get_mut("output").and_then(Value::as_array_mut) {
            Some(output) => output.push(item),
            None => response["output"] = json!([item]),
        }
        vec![item_done, response_event("response.completed", response)]
    }
}

fn response_event(event: &str, response: Value) -> Bytes {
    response_event_with(event, json!({ "type": event, "response": response }))
}

fn response_event_with(event: &str, data: Value) -> Bytes {
    Bytes::from(format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_events(output: &str) -> Vec<Value> {
        output
            .split("\n\n")
            .filter_map(|block| {
                block
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .and_then(|data| serde_json::from_str(data).ok())
            })
            .collect()
    }

    async fn run_native(input: &str) -> Vec<Value> {
        let upstream = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.to_string(),
        ))]);
        let chunks: Vec<_> = create_native_compaction_sse_stream(upstream)
            .collect()
            .await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
            .collect::<String>();
        parse_events(&merged)
    }

    #[test]
    fn summary_envelope_round_trips_and_rejects_foreign_blobs() {
        let encoded = encode_compaction_summary("进度：修好了压缩\nnext: test");
        assert!(encoded.starts_with(CCSWITCH_COMPACTION_PREFIX));
        assert_eq!(
            decode_compaction_summary(&encoded).as_deref(),
            Some("进度：修好了压缩\nnext: test")
        );
        assert_eq!(decode_compaction_summary("gAAAAABopenai-blob"), None);
    }

    #[test]
    fn replay_text_decodes_own_summary_and_notes_foreign_blob() {
        let own = json!({
            "type": "compaction",
            "encrypted_content": encode_compaction_summary("the summary")
        });
        assert_eq!(
            compaction_item_replay_text(&own).as_deref(),
            Some(format!("{SUMMARY_PREFIX}\nthe summary").as_str())
        );
        let foreign = json!({ "type": "compaction_summary", "encrypted_content": "gAAAA" });
        assert_eq!(
            compaction_item_replay_text(&foreign).as_deref(),
            Some(OPAQUE_COMPACTION_NOTE)
        );
        let marker = json!({ "type": "context_compaction" });
        assert_eq!(compaction_item_replay_text(&marker), None);
    }

    #[test]
    fn native_request_without_compaction_items_is_untouched() {
        let mut body = json!({
            "model": "kimi-k3",
            "input": [{ "type": "message", "role": "user", "content": "hi" }],
            "tools": [{ "type": "function", "name": "shell" }]
        });
        let before = body.clone();
        assert!(!prepare_native_third_party_request(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn native_request_keeps_unrecognized_compaction_blobs_byte_identical() {
        // 同一家原生 Responses 上游自己签发的压缩密文：看不出来源，原样发出，
        // 被拒后再由 opaque_state_rectifier 处理。
        let mut body = json!({
            "model": "gpt-5.5",
            "input": [
                { "type": "message", "role": "user", "content": "hi" },
                { "type": "compaction", "id": "cmp_1", "encrypted_content": "gAAAA-issued-here" },
                { "type": "context_compaction" },
                { "type": "message", "role": "user", "content": "go on" }
            ],
            "tools": [{ "type": "function", "name": "shell" }]
        });
        let before = body.clone();
        assert!(!prepare_native_third_party_request(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn native_compaction_request_becomes_summary_turn() {
        let mut body = json!({
            "model": "kimi-k3",
            "input": [
                { "type": "message", "role": "user", "content": "hi" },
                { "type": "compaction", "encrypted_content": "gAAAA-openai" },
                { "type": "context_compaction" },
                { "type": "compaction", "encrypted_content": encode_compaction_summary("prior work") },
                { "type": "compaction_trigger" }
            ],
            "tools": [{ "type": "function", "name": "shell" }],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "text": { "format": { "type": "json_schema" }, "verbosity": "low" }
        });
        assert!(prepare_native_third_party_request(&mut body));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 5);
        assert_eq!(
            input[1],
            json!({ "type": "compaction", "encrypted_content": "gAAAA-openai" })
        );
        assert_eq!(input[2], json!({ "type": "context_compaction" }));
        assert_eq!(
            input[3]["content"][0]["text"],
            format!("{SUMMARY_PREFIX}\nprior work")
        );
        assert_eq!(input[4]["content"][0]["text"], COMPACT_PROMPT);
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
        assert_eq!(body["text"], json!({ "verbosity": "low" }));
    }

    #[test]
    fn official_scrub_leaves_native_history_byte_identical() {
        let mut body = json!({
            "store": false,
            "input": [
                { "type": "message", "id": "msg_1", "role": "assistant", "content": [] },
                { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAA" },
                { "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "shell", "arguments": "{}" },
                { "type": "compaction", "id": "cmp_1", "encrypted_content": "gAAAA-openai" },
                { "type": "message", "role": "user", "content": "go" }
            ]
        });
        let before = body.clone();
        assert!(!scrub_ccswitch_state_for_official(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn official_scrub_removes_third_party_state() {
        let mut body = json!({
            "store": false,
            "input": [
                { "type": "reasoning", "id": "rs_resp_chat", "summary": [{ "type": "summary_text", "text": "t" }] },
                { "type": "reasoning", "id": "rs_resp_msg_0", "summary": [],
                  "encrypted_content": "ccswitch-anthropic-thinking-v1:abc" },
                { "type": "message", "id": "resp_msg_01_msg_0", "role": "assistant",
                  "content": [{ "type": "output_text", "text": "hi" }] },
                { "type": "compaction", "id": "cmp_2", "encrypted_content": encode_compaction_summary("done so far") },
                { "type": "message", "role": "user", "content": "next" }
            ]
        });
        assert!(scrub_ccswitch_state_for_official(&mut body));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert!(input[0].get("id").is_none());
        assert_eq!(input[0]["content"][0]["text"], "hi");
        assert_eq!(
            input[1]["content"][0]["text"],
            format!("{SUMMARY_PREFIX}\ndone so far")
        );
        assert_eq!(input[2]["content"], "next");
    }

    #[test]
    fn compaction_turn_only_completes_on_a_normal_stop() {
        for normal in [
            None,
            Some("stop"),
            Some("end_turn"),
            Some("stop_sequence"),
            Some("STOP"),
        ] {
            assert_eq!(compaction_incomplete_reason(normal), None, "{normal:?}");
        }
        for (reason, expected) in [
            ("length", "max_output_tokens"),
            ("max_tokens", "max_output_tokens"),
            ("content_filter", "content_filter"),
            ("sensitive", "content_filter"),
            ("refusal", "content_filter"),
            (
                "insufficient_system_resource",
                "insufficient_system_resource",
            ),
            ("tool_calls", "tool_calls"),
        ] {
            assert_eq!(
                compaction_incomplete_reason(Some(reason)).as_deref(),
                Some(expected),
                "{reason}"
            );
        }
    }

    #[tokio::test]
    async fn native_stream_appends_single_compaction_item_before_completed() {
        let events = run_native(concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"sum\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"summary text\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[{\"type\":\"message\"}]}}\n\n"
        ))
        .await;
        let types: Vec<&str> = events
            .iter()
            .map(|event| event["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            [
                "response.output_text.delta",
                "response.output_item.done",
                "response.output_item.done",
                "response.completed"
            ]
        );
        let item = &events[2]["item"];
        assert_eq!(item["type"], "compaction");
        assert_eq!(events[2]["output_index"], 1);
        assert_eq!(
            decode_compaction_summary(item["encrypted_content"].as_str().unwrap()).as_deref(),
            Some("summary text")
        );
        let output = events[3]["response"]["output"].as_array().unwrap();
        assert_eq!(
            output
                .iter()
                .filter(|item| item["type"] == "compaction")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn native_stream_reports_truncated_or_empty_summary_as_retryable() {
        let incomplete = run_native(
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"incomplete\",\"output\":[]}}\n\n",
        )
        .await;
        assert_eq!(incomplete.last().unwrap()["type"], "response.incomplete");

        let empty = run_native(
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n",
        )
        .await;
        assert_eq!(empty.last().unwrap()["type"], "response.failed");
    }
}
