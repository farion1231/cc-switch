//! 流式响应转换模块
//!
//! 实现 OpenAI SSE → Anthropic SSE 格式转换

use super::inline_think::InlineThinkSplitter;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// OpenAI 流式响应数据结构
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    // OpenRouter/Kimi/其它 使用 reasoning，DeepSeek 使用 reasoning_content
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// OpenAI 流式响应的 usage 信息（完整版）
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// Some compatible servers return Anthropic-style cache fields directly
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

/// Nested token details from OpenAI format
#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
}

#[derive(Debug, Clone)]
struct ToolBlockState {
    anthropic_index: u32,
    id: String,
    name: String,
    started: bool,
    pending_args: String,
    /// 连续空白字符计数 — 用于检测 Copilot 无限换行 bug
    /// 当 function call 参数中的连续空白字符达到阈值时，强制终止流
    consecutive_whitespace: usize,
    /// 是否已因无限空白 bug 被中止
    aborted: bool,
}

/// 无限空白 bug 的连续空白字符阈值
const INFINITE_WHITESPACE_THRESHOLD: usize = 500;

fn build_anthropic_usage_json(usage: &Usage) -> Value {
    // OpenAI prompt_tokens 含缓存，Anthropic input_tokens 不含，需减去 cache_read 与 cache_creation
    // （三桶互斥，恒等 input + cache_read + cache_creation == prompt_tokens）。
    let cached = extract_cache_read_tokens(usage).unwrap_or(0);
    let cache_creation = extract_cache_write_tokens(usage).unwrap_or(0);
    let input_tokens = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": usage.completion_tokens
    });
    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
    }
    usage_json
}

fn default_anthropic_usage_json() -> Value {
    json!({
        "input_tokens": 0,
        "output_tokens": 0
    })
}

fn build_message_delta_event(stop_reason: Option<String>, usage_json: Option<Value>) -> Value {
    let usage = usage_json
        .filter(|usage| usage.is_object())
        .unwrap_or_else(default_anthropic_usage_json);

    json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": stop_reason,
            "stop_sequence": null
        },
        "usage": usage
    })
}

fn sse_event_string(event: Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        event.get("type").and_then(|v| v.as_str()).unwrap_or(""),
        serde_json::to_string(&event).unwrap_or_default()
    )
}

/// 生成一个 thinking/text 增量所需的 Anthropic SSE 事件序列
/// （必要时先关闭当前块：content_block_stop? + content_block_start + content_block_delta）。
fn non_tool_block_events(
    block_type: &'static str,
    delta: &str,
    next_content_index: &mut u32,
    current_type: &mut Option<&'static str>,
    current_index: &mut Option<u32>,
) -> Vec<String> {
    let mut events = Vec::new();

    if *current_type != Some(block_type) {
        if let Some(index) = current_index.take() {
            events.push(sse_event_string(json!({
                "type": "content_block_stop",
                "index": index
            })));
        }
        let index = *next_content_index;
        *next_content_index += 1;
        let content_block = if block_type == "thinking" {
            json!({"type": "thinking", "thinking": ""})
        } else {
            json!({"type": "text", "text": ""})
        };
        events.push(sse_event_string(json!({
            "type": "content_block_start",
            "index": index,
            "content_block": content_block
        })));
        *current_type = Some(block_type);
        *current_index = Some(index);
    }

    if let Some(index) = *current_index {
        let delta_payload = if block_type == "thinking" {
            json!({"type": "thinking_delta", "thinking": delta})
        } else {
            json!({"type": "text_delta", "text": delta})
        };
        events.push(sse_event_string(json!({
            "type": "content_block_delta",
            "index": index,
            "delta": delta_payload
        })));
    }

    events
}

/// 终止边界（[DONE] / 流末尾发终止事件前）为仍开着的块生成配对的
/// content_block_stop：当前非工具块一个 + 所有仍开的工具块（按 index 排序）。
/// finish_reason 正常到达时这些块已在该路径关闭，此处为空操作；
/// finish 缺失或其后又来迟到增量时，靠这里补齐 Anthropic 协议要求的
/// 「每个 content_block_start 都有配对 content_block_stop」不变量。
fn open_block_stop_events(
    current_non_tool_block_index: &mut Option<u32>,
    current_non_tool_block_type: &mut Option<&'static str>,
    open_tool_block_indices: &mut HashSet<u32>,
) -> Vec<String> {
    let mut events = Vec::new();
    if let Some(index) = current_non_tool_block_index.take() {
        events.push(sse_event_string(json!({
            "type": "content_block_stop",
            "index": index
        })));
    }
    *current_non_tool_block_type = None;
    let mut tool_indices: Vec<u32> = open_tool_block_indices.iter().copied().collect();
    tool_indices.sort_unstable();
    for index in tool_indices {
        events.push(sse_event_string(json!({
            "type": "content_block_stop",
            "index": index
        })));
    }
    open_tool_block_indices.clear();
    events
}

/// 创建 Anthropic SSE 流
pub fn create_anthropic_sse_stream<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index: u32 = 0;
        let mut has_sent_message_start = false;
        // 某些上游 provider（如 OpenRouter 的 kimi-k2.6）会在 tool_use 后发送多个
        // 带 finish_reason 的 SSE chunk。Anthropic 协议要求每个消息流只能有一个
        // message_delta，重复会导致 Claude Code abort 连接。因此需要：
        // 1) has_emitted_message_delta: 去重，只处理第一个 finish_reason
        // 2) pending_message_delta: 缓存延迟到 [DONE] 发送，确保 usage 完整
        let mut has_emitted_message_delta = false;
        let mut pending_message_delta: Option<(Option<String>, Option<Value>)> = None;
        let mut has_sent_message_stop = false;
        let mut stream_ended_with_error = false;
        let mut latest_usage: Option<Value> = None;
        let mut current_non_tool_block_type: Option<&'static str> = None;
        let mut current_non_tool_block_index: Option<u32> = None;
        let mut inline_think = InlineThinkSplitter::default();
        let mut tool_blocks_by_index: HashMap<usize, ToolBlockState> = HashMap::new();
        let mut open_tool_block_indices: HashSet<u32> = HashSet::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(line) = take_sse_block(&mut buffer) {
                        if line.trim().is_empty() {
                            continue;
                        }

                        for l in line.lines() {
                            if let Some(data) = strip_sse_field(l, "data") {
                                if data.trim() == "[DONE]" {
                                    log::debug!("[Claude/OpenRouter] <<< OpenAI SSE: [DONE]");

                                    // 流结束边界：冲刷 inline think 残留（幂等，通常已在前面的
                                    // finish_reason 边界冲刷过）。
                                    let (thinking, text) = inline_think.flush();
                                    for (block_type, delta) in
                                        [("thinking", thinking), ("text", text)]
                                    {
                                        if let Some(delta) = &delta {
                                            for sse_data in non_tool_block_events(
                                                block_type,
                                                delta,
                                                &mut next_content_index,
                                                &mut current_non_tool_block_type,
                                                &mut current_non_tool_block_index,
                                            ) {
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }
                                    }

                                    // [DONE] 边界：补关仍开着的块。finish_reason 缺失
                                    //（部分上游只发 [DONE]）或其后又来迟到增量重开了块时，
                                    // 终止事件前必须有配对的 content_block_stop。
                                    for sse_data in open_block_stop_events(
                                        &mut current_non_tool_block_index,
                                        &mut current_non_tool_block_type,
                                        &mut open_tool_block_indices,
                                    ) {
                                        yield Ok(Bytes::from(sse_data));
                                    }

                                    // 流正常结束，发出缓存的 message_delta（含完整 usage）。
                                    if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                                        let event = build_message_delta_event(stop_reason, usage_json);
                                        let sse_data = format!("event: message_delta\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (from pending)");
                                        yield Ok(Bytes::from(sse_data));
                                    }

                                    let event = json!({"type": "message_stop"});
                                    let sse_data = format!("event: message_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop");
                                    yield Ok(Bytes::from(sse_data));
                                    has_sent_message_stop = true;
                                    continue;
                                }

                                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                                    log::debug!("[Claude/OpenRouter] <<< SSE chunk received");

                                    if message_id.is_none() && !chunk.id.is_empty() {
                                        message_id = Some(chunk.id.clone());
                                    }
                                    if current_model.is_none() && !chunk.model.is_empty() {
                                        current_model = Some(chunk.model.clone());
                                    }

                                    let chunk_usage_json =
                                        chunk.usage.as_ref().map(build_anthropic_usage_json);
                                    if let Some(usage_json) = &chunk_usage_json {
                                        latest_usage = Some(usage_json.clone());
                                        if let Some((_, pending_usage)) = pending_message_delta.as_mut() {
                                            *pending_usage = Some(usage_json.clone());
                                        }
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        if !has_sent_message_start {
                                            // Build usage with cache tokens if available from first chunk
                                            let mut start_usage = json!({
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            });
                                            if let Some(u) = &chunk.usage {
                                                let cached = extract_cache_read_tokens(u).unwrap_or(0);
                                                let cache_creation =
                                                    extract_cache_write_tokens(u).unwrap_or(0);
                                                let input = u
                                                    .prompt_tokens
                                                    .saturating_sub(cached)
                                                    .saturating_sub(cache_creation);
                                                start_usage["input_tokens"] = json!(input);
                                                if cached > 0 {
                                                    start_usage["cache_read_input_tokens"] = json!(cached);
                                                }
                                                if cache_creation > 0 {
                                                    start_usage["cache_creation_input_tokens"] =
                                                        json!(cache_creation);
                                                }
                                            }

                                            let event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": start_usage
                                                }
                                            });
                                            let sse_data = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                            has_sent_message_start = true;
                                        }

                                        // 处理 reasoning（thinking）
                                        // 某些上游始终填充 reasoning_content，输出正文时也带上空字符串。空值会匹配
                                        // Some("") 进入本分支，与下方 content 分支轮流改写 current_non_tool_block_type，
                                        // 导致单个 chunk 内 text/thinking 块被反复关闭重开，正文碎片化。
                                        // 非流式路径 openai_to_anthropic 已对 reasoning_content 做同样的空值过滤，
                                        // 此处与之保持一致（也与下方 content 分支的 !content.is_empty() 对称）。
                                        if let Some(reasoning) = choice
                                            .delta
                                            .reasoning
                                            .as_ref()
                                            .filter(|r| !r.is_empty())
                                        {
                                            for sse_data in non_tool_block_events(
                                                "thinking",
                                                reasoning,
                                                &mut next_content_index,
                                                &mut current_non_tool_block_type,
                                                &mut current_non_tool_block_index,
                                            ) {
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }

                                        // 处理文本内容。content 里流首内联的 <think>/<thinking>
                                        // 块（DeepSeek 系、MiniMax M3 等 Chat 兼容上游）先剥离为
                                        // thinking，剩余正文按原逻辑下发。
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                let (thinking, text) = inline_think.push(content);
                                                for (block_type, delta) in
                                                    [("thinking", thinking), ("text", text)]
                                                {
                                                    if let Some(delta) = &delta {
                                                        for sse_data in non_tool_block_events(
                                                            block_type,
                                                            delta,
                                                            &mut next_content_index,
                                                            &mut current_non_tool_block_type,
                                                            &mut current_non_tool_block_index,
                                                        ) {
                                                            yield Ok(Bytes::from(sse_data));
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // 处理工具调用
                                        if let Some(tool_calls) = &choice.delta.tool_calls {
                                            if !tool_calls.is_empty() {
                                                // 工具调用边界：先冲刷 inline think 残留再关块。
                                                // 拖到 finish_reason 才下发的话，这些内容会在
                                                // 工具块打开之后才开块，顺序颠倒且块交叠。
                                                let (thinking, text) = inline_think.flush();
                                                for (block_type, delta) in
                                                    [("thinking", thinking), ("text", text)]
                                                {
                                                    if let Some(delta) = &delta {
                                                        for sse_data in non_tool_block_events(
                                                            block_type,
                                                            delta,
                                                            &mut next_content_index,
                                                            &mut current_non_tool_block_type,
                                                            &mut current_non_tool_block_index,
                                                        ) {
                                                            yield Ok(Bytes::from(sse_data));
                                                        }
                                                    }
                                                }

                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                current_non_tool_block_type = None;

                                                for tool_call in tool_calls {
                                                    let (
                                                        anthropic_index,
                                                        id,
                                                        name,
                                                        should_start,
                                                        pending_after_start,
                                                        immediate_delta,
                                                    ) = {
                                                        let state = tool_blocks_by_index
                                                            .entry(tool_call.index)
                                                            .or_insert_with(|| {
                                                                let index = next_content_index;
                                                                next_content_index += 1;
                                                                ToolBlockState {
                                                                    anthropic_index: index,
                                                                    id: String::new(),
                                                                    name: String::new(),
                                                                    started: false,
                                                                    pending_args: String::new(),
                                                                    consecutive_whitespace: 0,
                                                                    aborted: false,
                                                                }
                                                            });

                                                        // 如果此 tool call 已被中止（无限空白 bug），跳过后续处理
                                                        if state.aborted {
                                                            continue;
                                                        }

                                                        if let Some(id) = &tool_call.id {
                                                            state.id = id.clone();
                                                        }
                                                        if let Some(function) = &tool_call.function {
                                                            if let Some(name) = &function.name {
                                                                state.name = name.clone();
                                                            }
                                                        }

                                                        let should_start =
                                                            !state.started
                                                                && !state.id.is_empty()
                                                                && !state.name.is_empty();
                                                        if should_start {
                                                            state.started = true;
                                                        }
                                                        let pending_after_start = if should_start
                                                            && !state.pending_args.is_empty()
                                                        {
                                                            Some(std::mem::take(&mut state.pending_args))
                                                        } else {
                                                            None
                                                        };
                                                        let args_delta = tool_call
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.arguments.clone());
                                                        let immediate_delta = if let Some(args) = args_delta {
                                                            // 无限空白 bug 检测：跟踪连续空白字符
                                                            for ch in args.chars() {
                                                                if ch.is_whitespace() {
                                                                    state.consecutive_whitespace += 1;
                                                                } else {
                                                                    state.consecutive_whitespace = 0;
                                                                }
                                                            }
                                                            if state.consecutive_whitespace >= INFINITE_WHITESPACE_THRESHOLD {
                                                                log::warn!(
                                                                    "[Copilot] 检测到无限空白 bug (tool: {}), 中止此 tool call 流",
                                                                    state.name
                                                                );
                                                                state.aborted = true;
                                                                None
                                                            } else if state.started {
                                                                Some(args)
                                                            } else {
                                                                state.pending_args.push_str(&args);
                                                                None
                                                            }
                                                        } else {
                                                            None
                                                        };
                                                        (
                                                            state.anthropic_index,
                                                            state.id.clone(),
                                                            state.name.clone(),
                                                            should_start,
                                                            pending_after_start,
                                                            immediate_delta,
                                                        )
                                                    };

                                                    if should_start {
                                                        let event = json!({
                                                            "type": "content_block_start",
                                                            "index": anthropic_index,
                                                            "content_block": {
                                                                "type": "tool_use",
                                                                "id": id,
                                                                "name": name
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                        open_tool_block_indices.insert(anthropic_index);
                                                    }

                                                    if let Some(args) = pending_after_start {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": args
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }

                                                    if let Some(args) = immediate_delta {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": args
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }
                                            }
                                        }

                                        // 处理 finish_reason。
                                        // 注意：OpenRouter 某些 provider 会发送多个带 finish_reason 的 chunk
                                        // （第一个 usage 为 null，后续才补全）。此处只做缓存，不立即发送，
                                        // 等到 [DONE] 或流末尾再统一发出，确保 usage 完整且只发一次。
                                        if let Some(finish_reason) = &choice.finish_reason {
                                            let stop_reason = map_stop_reason(Some(finish_reason));
                                            let usage_json =
                                                chunk_usage_json.clone().or_else(|| latest_usage.clone());

                                            if has_emitted_message_delta {
                                                // 更新缓存的 message_delta usage（如果有更完整的 usage）
                                                if let (Some((_, ref mut usage)), Some(uj)) = (&mut pending_message_delta, usage_json) {
                                                    *usage = Some(uj);
                                                }
                                                continue;
                                            }
                                            has_emitted_message_delta = true;

                                            // finish_reason 边界：冲刷 inline think 残留
                                            // （未闭合的 think 块按思考内容下发），再统一关块。
                                            let (thinking, text) = inline_think.flush();
                                            for (block_type, delta) in
                                                [("thinking", thinking), ("text", text)]
                                            {
                                                if let Some(delta) = &delta {
                                                    for sse_data in non_tool_block_events(
                                                        block_type,
                                                        delta,
                                                        &mut next_content_index,
                                                        &mut current_non_tool_block_type,
                                                        &mut current_non_tool_block_index,
                                                    ) {
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }
                                            }

                                            if let Some(index) = current_non_tool_block_index.take() {
                                                let event = json!({
                                                    "type": "content_block_stop",
                                                    "index": index
                                                });
                                                let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                            current_non_tool_block_type = None;

                                            // Late start for blocks that accumulated args before id/name arrived.
                                            let mut late_tool_starts: Vec<(u32, String, String, String)> =
                                                Vec::new();
                                            for (tool_idx, state) in tool_blocks_by_index.iter_mut() {
                                                if state.started {
                                                    continue;
                                                }
                                                let has_payload = !state.pending_args.is_empty()
                                                    || !state.id.is_empty()
                                                    || !state.name.is_empty();
                                                if !has_payload {
                                                    continue;
                                                }
                                                let fallback_id = if state.id.is_empty() {
                                                    format!("tool_call_{tool_idx}")
                                                } else {
                                                    state.id.clone()
                                                };
                                                let fallback_name = if state.name.is_empty() {
                                                    "unknown_tool".to_string()
                                                } else {
                                                    state.name.clone()
                                                };
                                                state.started = true;
                                                let pending = std::mem::take(&mut state.pending_args);
                                                late_tool_starts.push((
                                                    state.anthropic_index,
                                                    fallback_id,
                                                    fallback_name,
                                                    pending,
                                                ));
                                            }
                                            late_tool_starts.sort_unstable_by_key(|(index, _, _, _)| *index);
                                            for (index, id, name, pending) in late_tool_starts {
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {
                                                        "type": "tool_use",
                                                        "id": id,
                                                        "name": name
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                                open_tool_block_indices.insert(index);
                                                if !pending.is_empty() {
                                                    let delta_event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "input_json_delta",
                                                            "partial_json": pending
                                                        }
                                                    });
                                                    let delta_sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&delta_event).unwrap_or_default());
                                                    yield Ok(Bytes::from(delta_sse));
                                                }
                                            }

                                            if !open_tool_block_indices.is_empty() {
                                                let mut tool_indices: Vec<u32> =
                                                    open_tool_block_indices.iter().copied().collect();
                                                tool_indices.sort_unstable();
                                                for index in tool_indices {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                open_tool_block_indices.clear();
                                            }

                                            // 缓存 message_delta，等到 [DONE] 时发送（以便收集完整的 usage）
                                            pending_message_delta = Some((stop_reason, usage_json));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    stream_ended_with_error = true;
                    // 错误路径同样冲刷 inline think 残留：先下发已收到的载荷，
                    // 再以 error 事件收尾，不伪造成功终止事件。
                    let (thinking, text) = inline_think.flush();
                    for (block_type, delta) in [("thinking", thinking), ("text", text)] {
                        if let Some(delta) = &delta {
                            for sse_data in non_tool_block_events(
                                block_type,
                                delta,
                                &mut next_content_index,
                                &mut current_non_tool_block_type,
                                &mut current_non_tool_block_index,
                            ) {
                                yield Ok(Bytes::from(sse_data));
                            }
                        }
                    }
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse_data = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse_data));
                    break;
                }
            }
        }

        // 流自然结束（含上游截断、无 finish_reason/[DONE]）也冲刷 inline think
        // 残留：已收到的内容必须下发，不得整段留在缓冲区丢弃。
        let (thinking, text) = inline_think.flush();
        for (block_type, delta) in [("thinking", thinking), ("text", text)] {
            if let Some(delta) = &delta {
                for sse_data in non_tool_block_events(
                    block_type,
                    delta,
                    &mut next_content_index,
                    &mut current_non_tool_block_type,
                    &mut current_non_tool_block_index,
                ) {
                    yield Ok(Bytes::from(sse_data));
                }
            }
        }

        // 流自然结束但未收到 [DONE] 时，确保发送缓存的 message_delta 和 message_stop。
        // 若上游已显式报错，则只保留 error 事件，避免把失败伪装成成功完成。
        // 无 finish_reason 的纯截断不发终止事件（既有方向），也不在此补关块。
        if !stream_ended_with_error {
            let emitted_pending_message_delta = if let Some((stop_reason, usage_json)) =
                pending_message_delta.take()
            {
                // 发终止事件前补关仍开着的块（finish 处理完后又来迟到增量重开块的场景）。
                for sse_data in open_block_stop_events(
                    &mut current_non_tool_block_index,
                    &mut current_non_tool_block_type,
                    &mut open_tool_block_indices,
                ) {
                    yield Ok(Bytes::from(sse_data));
                }
                let event = build_message_delta_event(stop_reason, usage_json);
                let sse_data = format!("event: message_delta\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (at stream end)");
                yield Ok(Bytes::from(sse_data));
                true
            } else {
                false
            };

            if emitted_pending_message_delta && !has_sent_message_stop {
                let event = json!({"type": "message_stop"});
                let sse_data = format!("event: message_stop\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop (at stream end)");
                yield Ok(Bytes::from(sse_data));
            }
        }
    }
}

/// Extract cache_read tokens from Usage, checking both direct field and nested details
fn extract_cache_read_tokens(usage: &Usage) -> Option<u32> {
    // Direct field takes priority (compatible servers)
    if let Some(v) = usage.cache_read_input_tokens {
        return Some(v);
    }
    // OpenAI standard: prompt_tokens_details.cached_tokens
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .filter(|&v| v > 0)
}

/// Extract cache-write tokens from direct compatibility fields or OpenAI details.
fn extract_cache_write_tokens(usage: &Usage) -> Option<u32> {
    if let Some(value) = usage.cache_creation_input_tokens {
        return Some(value);
    }
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|details| details.cache_write_tokens)
        .filter(|value| *value > 0)
}

/// 映射停止原因
fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| {
        match r {
            "tool_calls" | "function_call" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            "content_filter" => "end_turn",
            other => {
                log::warn!("[Claude/OpenRouter] Unknown finish_reason in streaming: {other}");
                "end_turn"
            }
        }
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;
    use serde_json::Value;
    use std::collections::HashMap;

    async fn collect_anthropic_events(input: &str) -> Vec<Value> {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect()
    }

    fn event_type(event: &Value) -> Option<&str> {
        event.get("type").and_then(|v| v.as_str())
    }

    /// 收集某一类 content_block_delta 的文本字段并拼接，用于断言流式增量的最终内容。
    fn collect_delta_text(events: &[Value], delta_type: &str, field: &str) -> String {
        events
            .iter()
            .filter(|event| {
                event_type(event) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str()) == Some(delta_type)
            })
            .map(|event| {
                event
                    .pointer(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    #[tokio::test]
    async fn test_tool_call_with_mixed_sse_line_endings() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_mixed\",\"model\":\"test-model\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"README.md\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        for delimiter in ["\n\r\n", "\r\n\n"] {
            let input = input.replace("\n\n", delimiter);
            for parts in [
                vec![input.clone()],
                input.chars().map(|c| c.to_string()).collect(),
            ] {
                let events = collect_events(parts).await;
                let tool_start = events.iter().find(|event| {
                    event_type(event) == Some("content_block_start")
                        && event["content_block"]["type"] == "tool_use"
                });
                let tool_start = tool_start.expect("mixed delimiters must preserve the tool call");
                assert_eq!(tool_start["content_block"]["id"], "call_0");
                assert_eq!(tool_start["content_block"]["name"], "read_file");
                let arguments =
                    collect_delta_text(&events, "input_json_delta", "/delta/partial_json");
                assert_eq!(
                    serde_json::from_str::<Value>(&arguments).unwrap(),
                    json!({"path": "README.md"})
                );
                assert!(events.iter().any(|event| {
                    event_type(event) == Some("message_delta")
                        && event["delta"]["stop_reason"] == "tool_use"
                }));
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| event_type(event) == Some("message_stop"))
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn test_map_stop_reason_legacy_and_filtered_values() {
        assert_eq!(
            map_stop_reason(Some("function_call")),
            Some("tool_use".to_string())
        );
        assert_eq!(
            map_stop_reason(Some("content_filter")),
            Some("end_turn".to_string())
        );
    }

    #[tokio::test]
    async fn test_streaming_tool_calls_routed_by_index() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"second_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let mut tool_index_by_call: HashMap<String, u64> = HashMap::new();
        for event in &events {
            if event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                && event
                    .pointer("/content_block/type")
                    .and_then(|v| v.as_str())
                    == Some("tool_use")
            {
                if let (Some(call_id), Some(index)) = (
                    event.pointer("/content_block/id").and_then(|v| v.as_str()),
                    event.get("index").and_then(|v| v.as_u64()),
                ) {
                    tool_index_by_call.insert(call_id.to_string(), index);
                }
            }
        }

        assert_eq!(tool_index_by_call.len(), 2);
        assert_ne!(
            tool_index_by_call.get("call_0"),
            tool_index_by_call.get("call_1")
        );

        let deltas: Vec<(u64, String)> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                let index = event.get("index").and_then(|v| v.as_u64())?;
                let partial_json = event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())?
                    .to_string();
                Some((index, partial_json))
            })
            .collect();

        assert_eq!(deltas.len(), 2);
        let second_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"b\":2}").then_some(*index))
            .unwrap();
        let first_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"a\":1}").then_some(*index))
            .unwrap();

        assert_eq!(second_idx, *tool_index_by_call.get("call_1").unwrap());
        assert_eq!(first_idx, *tool_index_by_call.get("call_0").unwrap());

        assert!(events.iter().any(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("tool_use")
        }));
    }

    #[tokio::test]
    async fn test_streaming_delays_tool_start_until_id_and_name_ready() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let starts: Vec<&Value> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("tool_use")
            })
            .collect();
        assert_eq!(starts.len(), 1);
        assert_eq!(
            starts[0]
                .pointer("/content_block/id")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "call_0"
        );
        assert_eq!(
            starts[0]
                .pointer("/content_block/name")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "first_tool"
        );

        let deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())
            })
            .collect();
        assert!(deltas.contains(&"{\"a\":"));
        assert!(deltas.contains(&"1}"));
    }

    #[tokio::test]
    async fn test_streaming_chinese_split_across_chunks_no_replacement_chars() {
        // "你好" split across two TCP chunks inside a streaming text delta.
        // Before the fix, from_utf8_lossy would produce U+FFFD for each half.
        let full = concat!(
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let bytes = full.as_bytes();

        // Find "你" in the byte stream and split inside it
        let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let split_point = ni_start + 1; // split after first byte of "你"

        let chunk1 = Bytes::from(bytes[..split_point].to_vec());
        let chunk2 = Bytes::from(bytes[split_point..].to_vec());

        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(chunk1),
            Ok::<_, std::io::Error>(chunk2),
        ]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        // Must contain the original Chinese characters, not replacement chars
        assert!(
            merged.contains("你好"),
            "expected '你好' in output, got replacement chars (U+FFFD)"
        );
        assert!(
            !merged.contains('\u{FFFD}'),
            "output must not contain U+FFFD replacement characters"
        );
    }

    #[tokio::test]
    async fn test_duplicate_finish_reason_emits_only_one_message_delta() {
        // Simulates OpenRouter behavior where two chunks carry finish_reason:
        // first with null usage, second with populated usage.
        let input = concat!(
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta"))
            .collect();

        assert_eq!(
            message_deltas.len(),
            1,
            "duplicate finish_reason chunks must produce exactly one message_delta, got {}: {:?}",
            message_deltas.len(),
            message_deltas
        );

        assert_eq!(message_deltas[0]["usage"]["input_tokens"], 10);
        assert_eq!(message_deltas[0]["usage"]["output_tokens"], 5);

        let message_stops = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop"))
            .count();
        assert_eq!(message_stops, 1, "message_stop must only be emitted once");
    }

    #[tokio::test]
    async fn test_usage_only_chunk_after_finish_reason_updates_message_delta_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-0924\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":13312,\"completion_tokens\":79,\"prompt_tokens_details\":{\"cached_tokens\":100}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();
        let message_stops = events
            .iter()
            .filter(|event| event_type(event) == Some("message_stop"))
            .count();

        assert_eq!(message_deltas.len(), 1);
        assert_eq!(message_stops, 1);

        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(13212)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(79)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_subtracts_cache_read_and_creation_from_input() {
        // prompt_tokens(1000) 含 cache_read(600) 与 cache_creation(300)；转 Anthropic 后
        // input 应为 fresh，守恒：input(100) + cache_read(600) + cache_creation(300) == prompt(1000)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":600,\"cache_write_tokens\":300}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        // fresh input = 1000 - 600 - 300 = 100
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(600)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(300)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_clamps_input_to_zero_when_cache_exceeds_prompt() {
        // prompt(100) < cache_read(80)+cache_creation(50)=130：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":80},\"cache_creation_input_tokens\":50}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(80)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(50)
        );
    }

    #[tokio::test]
    async fn test_message_delta_includes_zero_usage_when_stream_has_no_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"get_time\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();

        assert_eq!(message_deltas.len(), 1);
        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn test_streaming_finalizes_after_finish_when_done_is_missing() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert!(events.iter().any(|event| {
            event_type(event) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("end_turn")
        }));
        assert_eq!(
            events.last().and_then(|event| event_type(event)),
            Some("message_stop")
        );
    }

    #[tokio::test]
    async fn test_stream_end_without_finish_reason_does_not_emit_success_terminal_events() {
        let input = "data: {\"id\":\"chatcmpl_truncated\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";

        let events = collect_anthropic_events(input).await;

        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    #[tokio::test]
    async fn test_stream_error_does_not_emit_success_terminal_events() {
        let upstream = stream::iter(vec![Err::<Bytes, _>(std::io::Error::other(
            "upstream disconnected",
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        assert!(events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("error")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop")));
    }

    #[tokio::test]
    async fn test_empty_reasoning_alongside_content_does_not_fragment_blocks() {
        // 回归：某些上游输出正文时仍会填充 reasoning_content 字段（值为空字符串）。
        // 修复前 reasoning 分支无空值保护，空字符串会进入分支并与 content 分支轮流
        // 改写 current_non_tool_block_type，使 text/thinking 块被反复关闭重开，正文
        // 被切成多个碎片且夹带空 thinking 块。修复后应只有一个 text 块，且不产生
        // 任何 thinking 块。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_frag\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"某个历史\",\"reasoning_content\":\"\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_frag\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"？\",\"reasoning_content\":\"\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_frag\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        // 锁死块的 index、类型与顺序：整条流只应有一个 text 块，位于 index 0。
        let block_starts: Vec<(Option<u64>, &str)> = events
            .iter()
            .filter(|event| event_type(event) == Some("content_block_start"))
            .map(|event| {
                (
                    event.get("index").and_then(|v| v.as_u64()),
                    event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            block_starts,
            vec![(Some(0), "text")],
            "empty reasoning_content must not open any thinking block, and all content \
             chunks must append to a single text block"
        );

        // 不应产生任何 thinking_delta（包括空的）
        let thinking: String = collect_delta_text(&events, "thinking_delta", "/delta/thinking");
        assert!(
            thinking.is_empty(),
            "no thinking_delta should be emitted, got: {:?}",
            thinking
        );

        // 所有 text_delta 应落在同一个块上，拼接后是完整句子
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "某个历史？"
        );
    }

    #[tokio::test]
    async fn test_non_empty_reasoning_still_creates_thinking_block() {
        // 保护性测试：确保上面的修复没有误伤——真正带内容的 reasoning 仍应产出
        // thinking 块。断言同时锁死块的 index、类型、顺序与 thinking 内容。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_think\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"reasoning_content\":\"让我想想\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_think\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"content\":\"答案是 42\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_think\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        let block_starts: Vec<(Option<u64>, &str)> = events
            .iter()
            .filter(|event| event_type(event) == Some("content_block_start"))
            .map(|event| {
                (
                    event.get("index").and_then(|v| v.as_u64()),
                    event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            block_starts,
            vec![(Some(0), "thinking"), (Some(1), "text")],
            "real reasoning must yield exactly one thinking block followed by one text block"
        );

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "让我想想"
        );
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "答案是 42"
        );
    }

    #[tokio::test]
    async fn test_streaming_inline_thinking_split_across_chunks() {
        // 回归（#7722）：DeepSeek 系 openai_chat 上游把思考内容以 <thinking>...</thinking>
        // 内联进 content，且开闭标签会被 SSE chunk 边界任意切开。修复前整段连同标签
        // 一起当正文（text 块）泄漏；修复后思考进 thinking 块，正文进 text 块。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_ith\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ith\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"nking>tool call</thin\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ith\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"king>\\n日志很大。我先看当前配置。\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ith\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "tool call"
        );
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "日志很大。我先看当前配置。"
        );

        // 块顺序：thinking 块在前、text 块在后，各恰好一个
        let block_types: Vec<&str> = events
            .iter()
            .filter(|event| event_type(event) == Some("content_block_start"))
            .filter_map(|event| {
                event
                    .pointer("/content_block/type")
                    .and_then(|v| v.as_str())
            })
            .collect();
        assert_eq!(block_types, vec!["thinking", "text"]);
    }

    #[tokio::test]
    async fn test_streaming_inline_think_block_single_chunk() {
        // <think> 变体（MiniMax M3 等）在单个 content 增量内完整出现时同样剥离。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_ith1\",\"model\":\"minimax-m3\",\"choices\":[{\"delta\":{\"content\":\"<think>the ball costs 0.05</think>The ball is $0.05\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ith1\",\"model\":\"minimax-m3\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "the ball costs 0.05"
        );
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "The ball is $0.05"
        );
    }

    #[tokio::test]
    async fn test_streaming_unclosed_thinking_flushed_as_thinking() {
        // 流结束时 <thinking> 未闭合：缓冲内容按思考内容下发，不再泄漏标签本体。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_ithu\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"<thinking>partial reasoning\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ithu\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "partial reasoning"
        );
        assert_eq!(collect_delta_text(&events, "text_delta", "/delta/text"), "");
    }

    #[tokio::test]
    async fn test_streaming_prose_containing_thinking_tag_not_stripped() {
        // 流首不是 <thinking> 的正文里出现 "<thinking>" 字样，保持原样不剥离
        //（只识别流首块，与 Codex Chat 路径语义一致）。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_ithp\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"a <thinking> looks like \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ithp\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"a tag in prose\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ithp\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "a <thinking> looks like a tag in prose"
        );
        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            ""
        );
    }

    #[tokio::test]
    async fn test_streaming_mismatched_think_tag_pairs_still_split() {
        // 上游偶尔用不配对的标签收尾（<think> 开、</thinking> 闭）。
        // 只认配对闭合会让整段连正文一起被当推理吞掉；取最早出现的任一闭合标签。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_ithm\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"<think>checking</thinking>Done\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ithm\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "checking"
        );
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "Done"
        );
    }

    fn content_chunk(id: &str, content: &str) -> String {
        format!(
            "data: {{\"id\":\"{id}\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(content).unwrap()
        )
    }

    /// 转换流的单个 yield 即一条完整 SSE 事件，解析出 data JSON。
    fn parse_event(chunk: &Bytes) -> Value {
        let text = String::from_utf8_lossy(chunk.as_ref());
        let data = text
            .lines()
            .find_map(|line| strip_sse_field(line, "data"))
            .unwrap_or_default();
        serde_json::from_str(data).expect("valid SSE data JSON")
    }

    async fn collect_events(parts: Vec<String>) -> Vec<Value> {
        let upstream = stream::iter(
            parts
                .into_iter()
                .map(|part| Ok::<_, std::io::Error>(Bytes::from(part))),
        );
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        chunks
            .into_iter()
            .map(|chunk| parse_event(&chunk.unwrap()))
            .collect()
    }

    #[tokio::test]
    async fn review_progress_during_continuous_reasoning() {
        // P1 回归：Reasoning 态必须逐增量下发思考内容。闭合标签迟迟未到时，
        // 上游持续推送的每一条增量都应让转换流立即产生输出；修复前整段缓冲，
        // 外层 failover 对转换后的 next() 计时会误判空闲并中断活跃流。
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
        let upstream = async_stream::stream! {
            while let Some(item) = rx.recv().await {
                yield item;
            }
        };
        let converted = create_anthropic_sse_stream(upstream);
        tokio::pin!(converted);

        // 流首开标签：确立 Reasoning 态，本身不产生输出。
        tx.send(Ok(Bytes::from(content_chunk("progress", "<think>"))))
            .await
            .unwrap();

        // 上游只发推理增量，闭合标签始终不到（模拟远超空闲阈值的长推理）。
        for i in 0..20u32 {
            let expected = format!("step {i} ");
            tx.send(Ok(Bytes::from(content_chunk("progress", &expected))))
                .await
                .unwrap();

            // 每个增量都必须在宽裕时限内产生 thinking 输出：只验证"有进展"，
            // 不测量时延，避免 Windows 定时器粒度导致的抖动。
            loop {
                let item =
                    tokio::time::timeout(std::time::Duration::from_secs(2), converted.next())
                        .await
                        .unwrap_or_else(|_| {
                            panic!("上游持续推送时转换流在第 {i} 条增量处停摆（假空闲）")
                        })
                        .expect("converted stream ended unexpectedly");
                let event = parse_event(&item.unwrap());
                if event_type(&event) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("thinking_delta")
                {
                    assert_eq!(
                        event.pointer("/delta/thinking").and_then(|v| v.as_str()),
                        Some(expected.as_str())
                    );
                    break;
                }
            }
        }

        // 上游此后不再发送（闭合标签仍未来），转换流不得伪造成功终止事件。
        drop(tx);
        let chunks: Vec<_> = converted.collect().await;
        let events: Vec<Value> = chunks
            .into_iter()
            .map(|chunk| parse_event(&chunk.unwrap()))
            .collect();
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    #[tokio::test]
    async fn review_answer_independent_of_content_chunk_partition() {
        // P2 回归：同一逻辑响应无论 SSE 怎么分块，正文输出必须逐字节一致。
        // 修复前闭合标签与正文同增量时 trim_start_matches 会剥掉正文缩进，
        // 闭合标签在增量末尾时却原样保留，答案随分块方式变化。
        let thinking_expect = "reason";
        let answer_expect = "    return 42\n";
        let partitions: Vec<Vec<&str>> = vec![
            vec!["<think>reason</think>    return 42\n"], // 旧实现在此剥掉缩进
            vec!["<think>reason</think>", "    return 42\n"], // 旧实现在此保留缩进
            vec!["<think>", "reason", "</th", "ink>", "    return 42\n"], // 闭合标签跨增量
            vec!["<think>reason</think>    ", "return 42\n"],
        ];

        for parts in partitions {
            let events = collect_events(
                parts
                    .iter()
                    .map(|content| content_chunk("partition", content))
                    .collect(),
            )
            .await;
            assert_eq!(
                collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
                thinking_expect,
                "thinking differs under partition {parts:?}"
            );
            assert_eq!(
                collect_delta_text(&events, "text_delta", "/delta/text"),
                answer_expect,
                "answer whitespace must not depend on chunk partition {parts:?}"
            );
        }
    }

    #[tokio::test]
    async fn review_eof_keeps_received_partial_payload() {
        // P1 回归：未闭合推理在上游截断（无 finish_reason、无 [DONE]）直接 EOF 时，
        // 已收到的内容必须下发。修复前缓冲整段丢弃，客户端只剩 message_start。
        let events = collect_events(vec![content_chunk("eof", "<think>partial reas</th")]).await;

        // 已收到的载荷全部下发（含扣住待定的闭合标签短尾），分类为思考内容。
        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "partial reas</th"
        );
        assert!(events
            .iter()
            .any(|event| event_type(event) == Some("message_start")));
        // 截断流不得伪造成功终止事件。
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    #[tokio::test]
    async fn test_stream_error_keeps_received_partial_payload() {
        // 错误路径同样冲刷残留：先下发已收到的载荷，再以 error 事件收尾，
        // 不伪造成功终止事件。
        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from(content_chunk("err", "<think>partial reas</th"))),
            Err(std::io::Error::other("upstream disconnected")),
        ]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let events: Vec<Value> = chunks
            .into_iter()
            .map(|chunk| parse_event(&chunk.unwrap()))
            .collect();

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "partial reas</th"
        );
        assert!(events
            .iter()
            .any(|event| event_type(event) == Some("error")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    /// content_block_start / content_block_stop 的先后顺序，形如 "start:0:text"。
    fn block_boundaries(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| {
                let index = event.get("index").and_then(|v| v.as_u64())?;
                match event_type(event)? {
                    "content_block_start" => Some(format!(
                        "start:{index}:{}",
                        event
                            .pointer("/content_block/type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                    )),
                    "content_block_stop" => Some(format!("stop:{index}")),
                    _ => None,
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn test_tool_call_flushes_held_content_before_the_tool_block_opens() {
        // 状态机扣住的内容（纯空白、半截开标签、闭合标签短尾）必须在工具块打开之前
        // 下发并关块。拖到 finish_reason 才冲刷的话，文本块会在工具块未关时开出，
        // 而且排到工具块之后。
        let tool_call = "data: {\"id\":\"tool\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{}\"}}]}}]}\n\n";
        let finish = "data: {\"id\":\"tool\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";

        let cases = [
            ("\n\n", "text_delta", "/delta/text", "\n\n", "text"),
            ("<thi", "text_delta", "/delta/text", "<thi", "text"),
            (
                "<thinking>plan</thin",
                "thinking_delta",
                "/delta/thinking",
                "plan</thin",
                "thinking",
            ),
        ];

        for (content, delta_type, field, expected, block_type) in cases {
            let events = collect_events(vec![
                content_chunk("tool", content),
                tool_call.to_string(),
                finish.to_string(),
            ])
            .await;

            assert_eq!(
                collect_delta_text(&events, delta_type, field),
                expected,
                "content {content:?}"
            );
            assert_eq!(
                block_boundaries(&events),
                vec![
                    format!("start:0:{block_type}"),
                    "stop:0".to_string(),
                    "start:1:tool_use".to_string(),
                    "stop:1".to_string(),
                ],
                "content {content:?}"
            );
        }
    }

    /// 断言第一个 index 为 N 的 content_block_stop 出现在第一个指定类型事件之前。
    fn assert_block_stop_precedes_event(events: &[Value], index: u64, later_event: &str) {
        let stop_position = events
            .iter()
            .position(|event| {
                event_type(event) == Some("content_block_stop")
                    && event.get("index").and_then(|v| v.as_u64()) == Some(index)
            })
            .unwrap_or_else(|| panic!("content_block_stop for index {index} must be emitted"));
        let later_position = events
            .iter()
            .position(|event| event_type(event) == Some(later_event))
            .unwrap_or_else(|| panic!("{later_event} must be emitted"));
        assert!(
            stop_position < later_position,
            "content_block_stop for index {index} must precede {later_event}"
        );
    }

    #[tokio::test]
    async fn test_done_without_finish_reason_still_closes_open_text_block() {
        // 上游不发 finish_reason、只发 [DONE] 时，finish 路径的关块逻辑不会执行，
        // 仍开着的 text 块必须在 [DONE] 边界补配对的 content_block_stop——
        // Anthropic 协议要求每个 start 都有 stop，否则客户端 SDK 丢弃未闭合块。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_nofin\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking hard\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_nofin\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "thinking_delta", "/delta/thinking"),
            "thinking hard"
        );
        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "hello"
        );
        assert_eq!(
            block_boundaries(&events),
            vec![
                "start:0:thinking".to_string(),
                "stop:0".to_string(),
                "start:1:text".to_string(),
                "stop:1".to_string(),
            ]
        );
        assert!(events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    #[tokio::test]
    async fn test_late_content_after_finish_reason_reopened_block_closed_at_done() {
        // finish_reason 处理完之后上游又补了一条正文增量（部分中转的收尾形状）：
        // 迟到增量会重开一个 text 块，[DONE] 边界必须把它关上，且先于终止事件。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_late\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"plan\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_late\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"main text\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_late\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_late\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\" trailing\"}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            collect_delta_text(&events, "text_delta", "/delta/text"),
            "main text trailing"
        );
        assert_eq!(
            block_boundaries(&events),
            vec![
                "start:0:thinking".to_string(),
                "stop:0".to_string(),
                "start:1:text".to_string(),
                "stop:1".to_string(),
                "start:2:text".to_string(),
                "stop:2".to_string(),
            ]
        );
        assert_block_stop_precedes_event(&events, 2, "message_delta");
        assert_block_stop_precedes_event(&events, 2, "message_stop");
    }

    #[tokio::test]
    async fn test_reasoning_then_text_with_finish_pairs_all_block_stops() {
        // 常规形状（推理→正文→finish→[DONE]）本就双停；锁住配对不变量，
        // 同时验证 [DONE] 边界补关不会在 finish 路径已关块后重复发 stop。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_pair\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"plan\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_pair\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_pair\",\"model\":\"deepseek-v4.1-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            block_boundaries(&events),
            vec![
                "start:0:thinking".to_string(),
                "stop:0".to_string(),
                "start:1:text".to_string(),
                "stop:1".to_string(),
            ]
        );
        assert_block_stop_precedes_event(&events, 1, "message_delta");
        assert_block_stop_precedes_event(&events, 1, "message_stop");
    }

    #[tokio::test]
    async fn test_done_without_finish_reason_still_closes_open_tool_block() {
        // 工具块同理：finish_reason 缺失时工具块也只能靠 [DONE] 边界补关。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_tool\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert_eq!(
            block_boundaries(&events),
            vec!["start:0:tool_use".to_string(), "stop:0".to_string()]
        );
    }
}
