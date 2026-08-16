//! OpenAI Chat Completions SSE → OpenAI Responses SSE conversion.

use super::codex_responses_sse as sse;
use super::{
    codex_chat_common::{
        extract_reasoning_field_text, split_leading_think_block, strip_leading_think_open_tag,
    },
    transform_codex_chat::{
        chat_usage_to_responses_usage, custom_tool_input_from_chat_arguments,
        response_id_from_chat_id, response_status_from_finish_reason,
        response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
        CodexToolContext,
    },
};
use crate::proxy::json_canonical::canonicalize_tool_arguments_str;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct TextItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    added: bool,
    done: bool,
}

#[derive(Debug, Default)]
struct ReasoningItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    added: bool,
    done: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum InlineThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

#[derive(Debug, Default)]
struct InlineThinkState {
    mode: InlineThinkMode,
    buffer: String,
}

#[derive(Debug, Default)]
struct ToolCallState {
    output_index: Option<u32>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    reasoning_content: String,
    added: bool,
    done: bool,
}

#[derive(Debug)]
struct ChatToResponsesState {
    response_started: bool,
    completed: bool,
    response_id: String,
    model: String,
    created_at: u64,
    next_output_index: u32,
    text: TextItemState,
    reasoning: ReasoningItemState,
    inline_think: InlineThinkState,
    tools: BTreeMap<usize, ToolCallState>,
    /// Current owner of each raw upstream Chat tool-call index.
    ///
    /// Values are internal `tools` keys only; this map never drives output ordering.
    raw_index_to_key: BTreeMap<usize, usize>,
    next_tool_index_to_add: usize,
    output_items: Vec<(u32, Value)>,
    latest_usage: Option<Value>,
    finish_reason: Option<String>,
    tool_context: CodexToolContext,
    /// 本回合因缺少合法函数名而被丢弃的工具调用数（见 `finalize_tools`）。
    dropped_tool_calls: usize,
}

impl Default for ChatToResponsesState {
    fn default() -> Self {
        Self {
            response_started: false,
            completed: false,
            response_id: "resp_ccswitch".to_string(),
            model: String::new(),
            created_at: 0,
            next_output_index: 0,
            text: TextItemState::default(),
            reasoning: ReasoningItemState::default(),
            inline_think: InlineThinkState::default(),
            tools: BTreeMap::new(),
            raw_index_to_key: BTreeMap::new(),
            next_tool_index_to_add: 0,
            output_items: Vec::new(),
            latest_usage: None,
            finish_reason: None,
            tool_context: CodexToolContext::default(),
            dropped_tool_calls: 0,
        }
    }
}

impl ChatToResponsesState {
    fn with_tool_context(tool_context: CodexToolContext) -> Self {
        Self {
            tool_context,
            ..Self::default()
        }
    }

    fn handle_chat_chunk(&mut self, chunk: &Value) -> Vec<Bytes> {
        let mut events = Vec::new();

        if let Some(id) = chunk.get("id").and_then(|v| v.as_str()) {
            self.response_id = response_id_from_chat_id(Some(id));
        }
        if let Some(model) = chunk.get("model").and_then(|v| v.as_str()) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(created) = chunk.get("created").and_then(|v| v.as_u64()) {
            self.created_at = created;
        }

        events.extend(self.ensure_response_started());

        if let Some(usage) = chunk.get("usage").filter(|v| !v.is_null()) {
            self.latest_usage = Some(chat_usage_to_responses_usage(Some(usage)));
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|choices| choices.first())
        else {
            return events;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(reasoning) = chat_delta_reasoning_text(delta) {
                events.extend(self.push_reasoning_delta(&reasoning));
                self.append_reasoning_to_active_tools(&reasoning);
            }

            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    events.extend(self.push_content_delta(content));
                }
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                events.extend(self.flush_inline_think_at_boundary());
                let reasoning_for_tool_call = self.current_reasoning_text();
                events.extend(self.finalize_reasoning());
                for tool_call in tool_calls {
                    events.extend(
                        self.push_tool_call_delta(tool_call, reasoning_for_tool_call.as_deref()),
                    );
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = Some(finish_reason.to_string());
        }

        events
    }

    fn push_content_delta(&mut self, delta: &str) -> Vec<Bytes> {
        match self.inline_think.mode {
            InlineThinkMode::Text => {
                let mut events = self.finalize_reasoning();
                events.extend(self.push_text_delta(delta));
                events
            }
            InlineThinkMode::Detecting => {
                self.inline_think.buffer.push_str(delta);
                match leading_think_prefix_decision(&self.inline_think.buffer) {
                    ThinkPrefixDecision::NeedMore => Vec::new(),
                    ThinkPrefixDecision::Reasoning => {
                        self.inline_think.mode = InlineThinkMode::Reasoning;
                        self.drain_complete_inline_think()
                    }
                    ThinkPrefixDecision::Text => {
                        self.inline_think.mode = InlineThinkMode::Text;
                        let text = std::mem::take(&mut self.inline_think.buffer);
                        let mut events = self.finalize_reasoning();
                        events.extend(self.push_text_delta(&text));
                        events
                    }
                }
            }
            InlineThinkMode::Reasoning => {
                self.inline_think.buffer.push_str(delta);
                self.drain_complete_inline_think()
            }
        }
    }

    fn drain_complete_inline_think(&mut self) -> Vec<Bytes> {
        let Some((reasoning, answer)) = split_leading_think_block(&self.inline_think.buffer) else {
            return Vec::new();
        };

        self.inline_think.mode = InlineThinkMode::Text;
        self.inline_think.buffer.clear();

        let mut events = Vec::new();
        if !reasoning.is_empty() {
            events.extend(self.push_reasoning_delta(&reasoning));
            events.extend(self.finalize_reasoning());
        }
        if !answer.is_empty() {
            events.extend(self.push_text_delta(&answer));
        }

        events
    }

    fn flush_inline_think_at_boundary(&mut self) -> Vec<Bytes> {
        match self.inline_think.mode {
            InlineThinkMode::Text => Vec::new(),
            InlineThinkMode::Detecting => {
                self.inline_think.mode = InlineThinkMode::Text;
                let text = std::mem::take(&mut self.inline_think.buffer);
                if text.is_empty() {
                    Vec::new()
                } else {
                    let mut events = self.finalize_reasoning();
                    events.extend(self.push_text_delta(&text));
                    events
                }
            }
            InlineThinkMode::Reasoning => {
                let buffered = std::mem::take(&mut self.inline_think.buffer);
                self.inline_think.mode = InlineThinkMode::Text;
                if let Some((reasoning, answer)) = split_leading_think_block(&buffered) {
                    let mut events = Vec::new();
                    if !reasoning.is_empty() {
                        events.extend(self.push_reasoning_delta(&reasoning));
                        events.extend(self.finalize_reasoning());
                    }
                    if !answer.is_empty() {
                        events.extend(self.push_text_delta(&answer));
                    }
                    return events;
                }

                let reasoning = strip_leading_think_open_tag(&buffered).unwrap_or(buffered);
                if reasoning.is_empty() {
                    Vec::new()
                } else {
                    let mut events = self.push_reasoning_delta(&reasoning);
                    events.extend(self.finalize_reasoning());
                    events
                }
            }
        }
    }

    fn ensure_response_started(&mut self) -> Vec<Bytes> {
        if self.response_started {
            return Vec::new();
        }

        self.response_started = true;
        let response = self.base_response("in_progress", Vec::new());

        vec![
            sse::response_created(&response),
            sse::response_in_progress(&response),
        ]
    }

    fn push_reasoning_delta(&mut self, delta: &str) -> Vec<Bytes> {
        let mut events = Vec::new();

        if !self.reasoning.added {
            let output_index = self.next_output_index();
            let item_id = format!("rs_{}", self.response_id);
            self.reasoning.output_index = Some(output_index);
            self.reasoning.item_id = item_id.clone();
            self.reasoning.added = true;

            events.push(sse::reasoning_item_added(output_index, &item_id));
            events.push(sse::reasoning_summary_part_added(output_index, &item_id));
        }

        self.reasoning.text.push_str(delta);
        let output_index = self.reasoning.output_index.unwrap_or(0);
        events.push(sse::reasoning_summary_text_delta(
            output_index,
            &self.reasoning.item_id,
            delta,
        ));

        events
    }

    fn push_text_delta(&mut self, delta: &str) -> Vec<Bytes> {
        let mut events = Vec::new();

        if !self.text.added {
            let output_index = self.next_output_index();
            let item_id = format!("{}_msg", self.response_id);
            self.text.output_index = Some(output_index);
            self.text.item_id = item_id.clone();
            self.text.added = true;

            events.push(sse::message_item_added(output_index, &item_id));
            events.push(sse::message_content_part_added(output_index, &item_id));
        }

        self.text.text.push_str(delta);
        let output_index = self.text.output_index.unwrap_or(0);
        events.push(sse::output_text_delta(
            output_index,
            &self.text.item_id,
            delta,
        ));

        events
    }

    fn current_reasoning_text(&self) -> Option<String> {
        (!self.reasoning.text.trim().is_empty()).then(|| self.reasoning.text.trim().to_string())
    }

    /// 分配一个不会与现有调用冲突的内部 key。
    ///
    /// 正常情况下追加到最大 key 后面，以保持调用到达顺序。若畸形上游已经使用
    /// `usize::MAX`，则从 0 开始寻找第一个空洞，避免溢出、回绕或覆盖已有调用。
    fn next_available_tool_key(&self) -> Option<usize> {
        if let Some(next) = self
            .tools
            .keys()
            .next_back()
            .and_then(|key| key.checked_add(1))
        {
            return Some(next);
        }

        let mut candidate = 0usize;
        for key in self.tools.keys().copied() {
            if key > candidate {
                return Some(candidate);
            }
            if key == candidate {
                candidate = candidate.checked_add(1)?;
            }
        }
        Some(candidate)
    }

    /// 将上游的原始 `index` 与稳定的非空调用 ID 统一解析成内部 key。
    ///
    /// 非空 ID 是更强的身份信号，raw index 只能经显式 alias 访问内部状态。不同
    /// 非空 ID 复用 raw index 时会重绑该 alias，但旧状态仍可通过 ID 找回。
    fn resolve_tool_key(
        &mut self,
        explicit_index: Option<usize>,
        call_id: Option<&str>,
    ) -> Option<usize> {
        let call_id = call_id.filter(|id| !id.is_empty());

        if let Some(call_id) = call_id {
            let existing_key = self
                .tools
                .iter()
                .find_map(|(key, state)| (state.call_id == call_id).then_some(*key));
            if let Some(internal_key) = existing_key {
                if let Some(raw_index) = explicit_index {
                    self.raw_index_to_key.insert(raw_index, internal_key);
                }
                return Some(internal_key);
            }
        }

        let Some(raw_index) = explicit_index else {
            return match call_id {
                Some(_) => self.next_available_tool_key(),
                None => Some(self.tools.keys().next_back().copied().unwrap_or(0)),
            };
        };

        if let Some(internal_key) = self.raw_index_to_key.get(&raw_index).copied() {
            let can_reuse = match call_id {
                None => {
                    // Once a raw index is reused, ID-less frames are irreducibly ambiguous.
                    // The latest ID-bearing owner is the only deterministic continuation.
                    true
                }
                Some(call_id) => self
                    .tools
                    .get(&internal_key)
                    .map(|state| state.call_id.is_empty() || state.call_id == call_id)
                    .unwrap_or(true),
            };
            if can_reuse {
                return Some(internal_key);
            }

            let internal_key = self.next_available_tool_key()?;
            self.raw_index_to_key.insert(raw_index, internal_key);
            return Some(internal_key);
        }

        // Preserve late-identity compatibility only for an anonymous state that was created
        // without any raw-index alias. Numeric equality alone never establishes identity.
        let can_adopt_unaliased_anonymous = self
            .tools
            .get(&raw_index)
            .map(|state| state.call_id.is_empty())
            .unwrap_or(false)
            && !self
                .raw_index_to_key
                .values()
                .any(|internal_key| *internal_key == raw_index);
        let internal_key = if can_adopt_unaliased_anonymous || !self.tools.contains_key(&raw_index)
        {
            raw_index
        } else {
            self.next_available_tool_key()?
        };
        self.raw_index_to_key.insert(raw_index, internal_key);
        Some(internal_key)
    }

    fn push_tool_call_delta(&mut self, tool_call: &Value, reasoning: Option<&str>) -> Vec<Bytes> {
        let id_delta = tool_call
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let explicit_index = chat_tool_index(tool_call);
        let Some(internal_key) = self.resolve_tool_key(explicit_index, id_delta.as_deref()) else {
            // 所有 usize key 都被占用时无法再隔离身份；拒绝改写已有状态比混合调用安全。
            return Vec::new();
        };
        let function = tool_call.get("function").unwrap_or(&Value::Null);
        let name_delta = function
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let args_delta = function
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut output_index = None;
        let mut item_id = String::new();
        let current_name: String;

        {
            let state = self.tools.entry(internal_key).or_default();
            if let Some(ref id) = id_delta {
                if !id.is_empty() {
                    state.call_id.clone_from(id);
                }
            }
            if let Some(ref name) = name_delta {
                if !name.is_empty() {
                    state.name.clone_from(name);
                }
            }
            if !args_delta.is_empty() {
                state.arguments.push_str(&args_delta);
            }
            if state.reasoning_content.is_empty() {
                if let Some(reasoning) = reasoning.map(str::trim).filter(|value| !value.is_empty())
                {
                    state.reasoning_content = reasoning.to_string();
                }
            }

            if state.added {
                output_index = state.output_index;
                item_id = state.item_id.clone();
            }
            current_name = state.name.clone();
        }

        let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&current_name);
        let mut events = Vec::new();

        if !args_delta.is_empty() && !is_custom_tool {
            if let Some(output_index) = output_index {
                events.push(sse::function_call_arguments_delta(
                    output_index,
                    &item_id,
                    &args_delta,
                ));
            }
        }

        events.extend(self.flush_ready_tool_calls());

        events
    }

    fn flush_ready_tool_calls(&mut self) -> Vec<Bytes> {
        // Release consecutive internal keys so late identity fragments cannot reorder calls.
        let mut events = Vec::new();
        loop {
            let key = self.next_tool_index_to_add;
            let Some(state) = self.tools.get(&key) else {
                break;
            };
            if state.added || state.done {
                let Some(next_key) = self.next_tool_index_to_add.checked_add(1) else {
                    break;
                };
                self.next_tool_index_to_add = next_key;
                continue;
            }
            if state.call_id.is_empty() || state.name.is_empty() {
                break;
            }

            let assigned = self.next_output_index();
            let Some(state) = self.tools.get_mut(&key) else {
                continue;
            };
            state.added = true;
            state.output_index = Some(assigned);
            state.item_id = response_tool_call_item_id_from_chat_name(
                &state.call_id,
                &state.name,
                &self.tool_context,
            );

            let item = response_tool_call_item_from_chat_name(
                &state.item_id,
                "in_progress",
                &state.call_id,
                &state.name,
                "",
                Some(&state.reasoning_content),
                &self.tool_context,
            );

            events.push(sse::output_item_added(assigned, &item));

            if !state.arguments.is_empty()
                && !self.tool_context.is_custom_tool_chat_name(&state.name)
            {
                events.push(sse::function_call_arguments_delta(
                    assigned,
                    &state.item_id,
                    &state.arguments,
                ));
            }
            let Some(next_key) = self.next_tool_index_to_add.checked_add(1) else {
                break;
            };
            self.next_tool_index_to_add = next_key;
        }

        events
    }

    fn append_reasoning_to_active_tools(&mut self, delta: &str) {
        if delta.trim().is_empty() {
            return;
        }

        for state in self.tools.values_mut().filter(|state| !state.done) {
            if state.reasoning_content.is_empty() {
                state.reasoning_content = delta.trim_start().to_string();
            } else {
                state.reasoning_content.push_str(delta);
            }
        }
    }

    fn has_substantive_output(&self) -> bool {
        !self.text.text.trim().is_empty()
            || !self.reasoning.text.trim().is_empty()
            || !self.inline_think.buffer.trim().is_empty()
            || !self.output_items.is_empty()
            || self.tools.values().any(|state| {
                state.added
                    || !state.call_id.trim().is_empty()
                    || !state.name.trim().is_empty()
                    || !state.arguments.trim().is_empty()
                    || !state.reasoning_content.trim().is_empty()
            })
    }

    /// 本回合最终产出里是否至少有一个可被 Codex 识别的工具调用 item。
    fn has_emitted_tool_call(&self) -> bool {
        self.output_items.iter().any(|(_, item)| {
            matches!(
                item.get("type").and_then(|v| v.as_str()),
                Some("function_call" | "custom_tool_call" | "tool_search_call")
            )
        })
    }

    fn finalize(&mut self) -> Vec<Bytes> {
        if self.completed {
            return Vec::new();
        }

        let mut events = self.ensure_response_started();
        events.extend(self.flush_inline_think_at_boundary());
        events.extend(self.finalize_reasoning());
        events.extend(self.finalize_text());
        events.extend(self.finalize_tools());

        let status = response_status_from_finish_reason(self.finish_reason.as_deref());

        // 丢弃过工具调用、且最终一个工具调用都没剩下时，Codex 会收到一个
        // "status=completed 但 output 里没有任何工具调用" 的回合，agent loop 必然
        // 静默收尾——这正是 #4341「答一句就停、零报错」的形态。此时如实报错，
        // 而不是谎报成功。只要还剩下任何一个合法工具调用，Codex 本来就会继续，
        // 判据不成立，行为保持不变。
        //
        // 🔴 只对本应 `completed` 的回合生效：`finish_reason=length`（含流提前断开后
        // 合成的 length）有自己正当的终止解释，工具调用没拿到 name 是截断的后果而非
        // 上游发了畸形数据——报成 tool_call_dropped 会给出错误的归因，而本修复的全部
        // 意义就在于诊断信息的准确性。
        if status == "completed" && self.dropped_tool_calls > 0 && !self.has_emitted_tool_call() {
            let dropped = self.dropped_tool_calls;
            let message = format!(
                "Upstream returned {dropped} tool call(s) without a function name, \
                 leaving no usable tool call in this turn"
            );
            events.push(self.failed_event(message, Some("upstream_tool_call_dropped".to_string())));
            return events;
        }

        let mut response = self.base_response(status, self.completed_output_items());
        if status == "incomplete" {
            response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
        }

        events.push(sse::response_completed(&response));
        self.completed = true;
        events
    }

    fn finalize_reasoning(&mut self) -> Vec<Bytes> {
        if !self.reasoning.added || self.reasoning.done {
            return Vec::new();
        }

        let output_index = self.reasoning.output_index.unwrap_or(0);
        let item_id = self.reasoning.item_id.clone();
        let text = self.reasoning.text.clone();
        let (events, item) = sse::reasoning_close(output_index, &item_id, &text);
        self.output_items.push((output_index, item));
        self.reasoning.done = true;
        events
    }

    fn finalize_text(&mut self) -> Vec<Bytes> {
        if !self.text.added || self.text.done {
            return Vec::new();
        }

        let output_index = self.text.output_index.unwrap_or(0);
        let item_id = self.text.item_id.clone();
        let text = self.text.text.clone();
        let (events, item) = sse::message_close(output_index, &item_id, &text);
        self.output_items.push((output_index, item));
        self.text.done = true;
        events
    }

    fn finalize_tools(&mut self) -> Vec<Bytes> {
        let mut events = Vec::new();
        let keys: Vec<usize> = self.tools.keys().copied().collect();

        for key in keys {
            let mut add_event: Option<Bytes> = None;
            if self.tools.get(&key).map(|state| state.done).unwrap_or(true) {
                continue;
            }

            // Skip tool calls with missing names (defensive: some models generate
            // tool call deltas without providing a valid function name)
            // 纯空白名同样对应不到任何已发布工具，必须与空名同等对待——否则它会
            // 伪装成"本回合还有工具调用"，绕过下面 finalize 里的失败判据。
            let has_bad_name = self
                .tools
                .get(&key)
                .map(|state| state.name.trim().is_empty())
                .unwrap_or(true);
            if has_bad_name {
                let (call_id_empty, args_bytes) = self
                    .tools
                    .get(&key)
                    .map(|state| (state.call_id.is_empty(), state.arguments.len()))
                    .unwrap_or((true, 0));
                if let Some(state) = self.tools.get_mut(&key) {
                    state.done = true;
                }
                self.dropped_tool_calls += 1;
                // 只记结构信息：arguments 内容可能包含用户代码，且前端日志出口是
                // allowlist 脱敏，新字段不进白名单就不会被处理，因此只输出字节数。
                log::warn!(
                    "[Codex] dropped streaming tool call: model={} chat_index={} \
                     call_id_empty={} args_bytes={} finish_reason={} tools_total={}",
                    self.model,
                    key,
                    call_id_empty,
                    args_bytes,
                    self.finish_reason.as_deref().unwrap_or("<none>"),
                    self.tools.len()
                );
                continue;
            }

            if self
                .tools
                .get(&key)
                .map(|state| !state.added && !state.done)
                .unwrap_or(false)
            {
                let assigned = self.next_output_index();
                let Some(state) = self.tools.get_mut(&key) else {
                    continue;
                };
                state.added = true;
                if state.call_id.is_empty() {
                    state.call_id = format!("call_{key}");
                }
                state.output_index = Some(assigned);
                state.item_id = response_tool_call_item_id_from_chat_name(
                    &state.call_id,
                    &state.name,
                    &self.tool_context,
                );
                let item = response_tool_call_item_from_chat_name(
                    &state.item_id,
                    "in_progress",
                    &state.call_id,
                    &state.name,
                    "",
                    Some(&state.reasoning_content),
                    &self.tool_context,
                );
                add_event = Some(sse::output_item_added(assigned, &item));
            }

            if let Some(event) = add_event {
                events.push(event);
            }

            let Some(state) = self.tools.get_mut(&key) else {
                continue;
            };
            let output_index = state.output_index.unwrap_or(0);
            let arguments = canonicalize_tool_arguments_str(&state.arguments);
            let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&state.name);
            let item = response_tool_call_item_from_chat_name(
                &state.item_id,
                "completed",
                &state.call_id,
                &state.name,
                &arguments,
                Some(&state.reasoning_content),
                &self.tool_context,
            );
            state.done = true;
            self.output_items.push((output_index, item.clone()));

            if is_custom_tool {
                let input = custom_tool_input_from_chat_arguments(&arguments);
                if !input.is_empty() {
                    events.push(sse::custom_tool_call_input_delta(
                        output_index,
                        &state.item_id,
                        &input,
                    ));
                }
                events.push(sse::custom_tool_call_input_done(
                    output_index,
                    &state.item_id,
                    &input,
                ));
            } else {
                events.push(sse::function_call_arguments_done(
                    output_index,
                    &state.item_id,
                    &arguments,
                ));
            }
            events.push(sse::output_item_done(output_index, &item));
        }

        events
    }

    fn completed_output_items(&self) -> Vec<Value> {
        let mut output_items = self.output_items.clone();
        output_items.sort_by_key(|(output_index, _)| *output_index);
        output_items
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>()
    }

    fn base_response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": self
                .latest_usage
                .clone()
                .unwrap_or_else(|| chat_usage_to_responses_usage(None))
        })
    }

    fn next_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    fn failed_event(&mut self, message: String, error_type: Option<String>) -> Bytes {
        self.completed = true;
        let mut error = json!({ "message": message });
        if let Some(error_type) = error_type.filter(|value| !value.is_empty()) {
            error["type"] = json!(error_type);
        }

        let mut response = self.base_response("failed", self.completed_output_items());
        response["error"] = error;

        sse::response_failed(&response)
    }
}

fn chat_delta_reasoning_text(delta: &Value) -> Option<String> {
    extract_reasoning_field_text(delta)
}

/// Parse the optional wire index without allowing a narrowing conversion to wrap.
///
/// Chat JSON numbers are decoded as `u64` here because negative and fractional values
/// are not valid tool-call indices. `try_from` deliberately keeps an index that cannot
/// be represented by this host on the same conservative path as an omitted index.
fn chat_tool_index(tool_call: &Value) -> Option<usize> {
    tool_call
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
}

enum ThinkPrefixDecision {
    NeedMore,
    Reasoning,
    Text,
}

fn leading_think_prefix_decision(buffer: &str) -> ThinkPrefixDecision {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return ThinkPrefixDecision::NeedMore;
    }

    if trimmed.starts_with("<think>") {
        return ThinkPrefixDecision::Reasoning;
    }

    if "<think>".starts_with(trimmed) {
        return ThinkPrefixDecision::NeedMore;
    }

    ThinkPrefixDecision::Text
}

/// Create a stream that converts Chat Completions SSE chunks into Responses SSE events.
#[allow(dead_code)]
pub fn create_responses_sse_stream_from_chat<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_responses_sse_stream_from_chat_with_context(stream, CodexToolContext::default())
}

/// Create a stream that converts Chat Completions SSE chunks into Responses SSE
/// events while restoring Codex tool namespace/custom/tool_search metadata.
pub fn create_responses_sse_stream_from_chat_with_context<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: CodexToolContext,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = ChatToResponsesState::with_tool_context(tool_context);
        let mut stream_failed = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }

                        let mut event_name: Option<String> = None;
                        let mut data_parts: Vec<String> = Vec::new();
                        for line in block.lines() {
                            if let Some(event) = strip_sse_field(line, "event") {
                                event_name = Some(event.trim().to_string());
                            }
                            if let Some(data) = strip_sse_field(line, "data") {
                                data_parts.push(data.to_string());
                            }
                        }

                        if data_parts.is_empty() {
                            continue;
                        }

                        let data = data_parts.join("\n");
                        if data.trim() == "[DONE]" {
                            for event in state.finalize() {
                                yield Ok(event);
                            }
                            continue;
                        }

                        let chunk: Value = match serde_json::from_str(&data) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };

                        if event_name.as_deref() == Some("error") || chunk.get("error").is_some() {
                            let (message, error_type) = extract_chat_sse_error(&chunk);
                            yield Ok(state.failed_event(message, error_type));
                            stream_failed = true;
                            break;
                        }

                        for event in state.handle_chat_chunk(&chunk) {
                            yield Ok(event);
                        }
                    }

                    if stream_failed {
                        break;
                    }
                }
                Err(e) => {
                    yield Ok(state.failed_event(
                        format!("Stream error: {e}"),
                        Some("stream_error".to_string()),
                    ));
                    stream_failed = true;
                    break;
                }
            }
        }

        if !stream_failed {
            if state.completed || state.finish_reason.is_some() {
                for event in state.finalize() {
                    yield Ok(event);
                }
            } else if state.has_substantive_output() {
                state.finish_reason = Some("length".to_string());
                for event in state.finalize() {
                    yield Ok(event);
                }
            } else {
                yield Ok(state.failed_event(
                    "Upstream Chat Completions stream ended before sending finish_reason".to_string(),
                    Some("stream_truncated".to_string()),
                ));
            }
        }
    }
}

fn extract_chat_sse_error(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    (message, error_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    async fn collect(chunks: Vec<&str>) -> String {
        collect_with_context(chunks, CodexToolContext::default()).await
    }

    async fn collect_with_context(chunks: Vec<&str>, tool_context: CodexToolContext) -> String {
        let chunks: Vec<Result<Bytes, std::io::Error>> = chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk.as_bytes())))
            .collect();
        let upstream = stream::iter(chunks);
        let converted = create_responses_sse_stream_from_chat_with_context(upstream, tool_context);
        let bytes: Vec<Bytes> = converted.map(|item| item.unwrap()).collect().await;
        String::from_utf8(bytes.concat()).unwrap()
    }

    fn parse_sse_records(output: &str) -> Vec<(String, Value)> {
        output
            .split("\n\n")
            .filter(|block| !block.trim().is_empty())
            .map(|block| {
                let event = block
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .unwrap_or_else(|| panic!("SSE block has no event label: {block:?}"));
                let data = block
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .unwrap_or_else(|| panic!("SSE block has no data field: {block:?}"));
                let value: Value = serde_json::from_str(data)
                    .unwrap_or_else(|error| panic!("invalid SSE JSON ({error}): {data:?}"));
                assert_eq!(
                    value["type"].as_str(),
                    Some(event),
                    "SSE event label and payload type diverged"
                );
                (event.to_string(), value)
            })
            .collect()
    }

    fn parse_sse_events(output: &str) -> Vec<Value> {
        parse_sse_records(output)
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    fn completed_items(events: &[Value]) -> &[Value] {
        let created_pos = events
            .iter()
            .position(|event| event["type"] == "response.created")
            .expect("response.created is required");
        let in_progress_pos = events
            .iter()
            .position(|event| event["type"] == "response.in_progress")
            .expect("response.in_progress is required");
        let completed_pos = events
            .iter()
            .position(|event| event["type"] == "response.completed")
            .expect("response.completed is required");
        assert!(created_pos < in_progress_pos);
        assert!(in_progress_pos < completed_pos);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "response.completed")
                .count(),
            1,
            "expected exactly one response.completed event"
        );
        events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap()["response"]["output"]
            .as_array()
            .unwrap()
    }

    fn assert_function_item(
        item: &Value,
        item_id: &str,
        call_id: &str,
        name: &str,
        arguments: Value,
    ) {
        assert_eq!(item["type"], "function_call");
        assert_eq!(item["id"], item_id);
        assert_eq!(item["call_id"], call_id);
        assert_eq!(item["name"], name);
        assert_eq!(
            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
            arguments
        );
    }

    fn assert_tool_lifecycle(
        events: &[Value],
        item_id: &str,
        output_index: u64,
        delta_type: &str,
        done_type: &str,
    ) {
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "response.completed")
                .count(),
            1,
            "expected exactly one response.completed event"
        );
        let added_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.added" && event["item"]["id"] == item_id
            })
            .unwrap_or_else(|| panic!("missing output_item.added for {item_id}"));
        let delta_pos = events
            .iter()
            .position(|event| event["type"] == delta_type && event["item_id"] == item_id)
            .unwrap_or_else(|| panic!("missing {delta_type} for {item_id}"));
        let done_pos = events
            .iter()
            .position(|event| event["type"] == done_type && event["item_id"] == item_id)
            .unwrap_or_else(|| panic!("missing {done_type} for {item_id}"));
        let item_done_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.done" && event["item"]["id"] == item_id
            })
            .unwrap_or_else(|| panic!("missing output_item.done for {item_id}"));
        let completed_pos = events
            .iter()
            .position(|event| event["type"] == "response.completed")
            .unwrap();

        assert!(added_pos < delta_pos);
        assert!(delta_pos < done_pos);
        assert!(done_pos < item_done_pos);
        assert!(item_done_pos < completed_pos);
        assert_eq!(events[added_pos]["output_index"], output_index);
        assert_eq!(events[delta_pos]["output_index"], output_index);
        assert_eq!(events[done_pos]["output_index"], output_index);
        assert_eq!(events[item_done_pos]["output_index"], output_index);
        assert_eq!(events[added_pos]["item"]["status"], "in_progress");
        assert_eq!(events[item_done_pos]["item"]["status"], "completed");
    }

    fn assert_tool_search_lifecycle(
        events: &[Value],
        call_id: &str,
        item_id: &str,
        output_index: u64,
    ) {
        let added_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.added"
                    && event["item"]["type"] == "tool_search_call"
                    && event["item"]["call_id"] == call_id
            })
            .unwrap_or_else(|| panic!("missing tool-search output_item.added for {call_id}"));
        let delta_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.function_call_arguments.delta"
                    && event["item_id"] == item_id
            })
            .unwrap_or_else(|| panic!("missing tool-search argument delta for {call_id}"));
        let done_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.function_call_arguments.done"
                    && event["item_id"] == item_id
            })
            .unwrap_or_else(|| panic!("missing tool-search argument done for {call_id}"));
        let item_done_pos = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.done"
                    && event["item"]["type"] == "tool_search_call"
                    && event["item"]["call_id"] == call_id
            })
            .unwrap_or_else(|| panic!("missing tool-search output_item.done for {call_id}"));
        let completed_pos = events
            .iter()
            .position(|event| event["type"] == "response.completed")
            .unwrap();

        assert!(added_pos < delta_pos);
        assert!(delta_pos < done_pos);
        assert!(done_pos < item_done_pos);
        assert!(item_done_pos < completed_pos);
        assert_eq!(events[added_pos]["output_index"], output_index);
        assert_eq!(events[delta_pos]["output_index"], output_index);
        assert_eq!(events[done_pos]["output_index"], output_index);
        assert_eq!(events[item_done_pos]["output_index"], output_index);
        assert_eq!(events[added_pos]["item"]["status"], "in_progress");
        assert_eq!(events[item_done_pos]["item"]["status"], "completed");
    }

    #[tokio::test]
    async fn converts_text_chat_sse_to_responses_sse() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_1\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6,\"prompt_tokens_details\":{\"cached_tokens\":0},\"cache_read_input_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();

        assert!(output.contains("event: response.created"));
        assert!(output.contains("event: response.output_text.delta"));
        assert!(output.contains("\"text\":\"Hello\""));
        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"input_tokens\":4"));
        assert_eq!(
            completed["response"]["usage"]["input_tokens_details"]["cached_tokens"],
            2
        );
        assert_eq!(completed["response"]["usage"]["cache_read_input_tokens"], 2);
    }

    #[tokio::test]
    async fn converts_reasoning_content_chat_sse_to_responses_reasoning_events() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Need context. \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"reasoning\":\"Now answer. \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6,\"total_tokens\":10,\"completion_tokens_details\":{\"reasoning_tokens\":3}}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.reasoning_summary_part.added"));
        assert!(output.contains("event: response.reasoning_summary_text.delta"));
        assert!(output.contains("event: response.reasoning_summary_text.done"));
        assert!(output.contains("Need context. Now answer. "));
        assert!(output.contains("\"type\":\"reasoning\""));
        assert!(output.contains("\"text\":\"Done\""));
        assert!(output.contains("\"reasoning_tokens\":3"));

        let reasoning_pos = output.find("\"type\":\"reasoning\"").unwrap();
        let message_pos = output.find("\"type\":\"message\"").unwrap();
        assert!(reasoning_pos < message_pos);
    }

    #[tokio::test]
    async fn converts_inline_think_chat_sse_to_reasoning_without_leaking_tags() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"<think>\\nNeed\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[{\"delta\":{\"content\":\" context.</think>\\n\\npong\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6,\"total_tokens\":10,\"completion_tokens_details\":{\"reasoning_tokens\":3}}}\n\n",
        ])
        .await;

        assert!(output.contains("event: response.reasoning_summary_text.delta"));
        assert!(output.contains("Need context."));
        assert!(output.contains("\"text\":\"pong\""));
        assert!(output.contains("\"reasoning_tokens\":3"));
        assert!(!output.contains("<think>"));
        assert!(!output.contains("</think>"));
        assert!(output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn converts_tool_call_chat_sse_to_responses_sse() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.function_call_arguments.delta"));
        assert!(output.contains("event: response.function_call_arguments.done"));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"call_id\":\"call_1\""));
    }

    #[tokio::test]
    async fn preserves_tool_identity_across_empty_continuation_deltas() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_dashscope\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_dashscope\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_dashscope\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"function\",\"function\":{\"name\":\"\",\"arguments\":\"\\\"cmd\\\":\\\"date\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .find(|event| event["type"] == "response.output_item.done")
            .unwrap();
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();

        assert_eq!(added.len(), 1);
        for item in [&done["item"], &completed["response"]["output"][0]] {
            assert_eq!(item["type"], "function_call");
            assert_eq!(item["name"], "exec_command");
            assert_eq!(item["call_id"], "call_dashscope");
            assert_eq!(item["arguments"], r#"{"cmd":"date"}"#);
        }
        assert!(!output.contains(r#""name":"""#));
        assert!(!output.contains(r#""call_id":"""#));
    }

    #[tokio::test]
    async fn preserves_parallel_tool_order_when_earlier_name_arrives_late() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_parallel\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_first\",\"type\":\"function\",\"function\":{\"name\":\"\",\"arguments\":\"{\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_parallel\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_second\",\"type\":\"function\",\"function\":{\"name\":\"second_tool\",\"arguments\":\"{\\\"value\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_parallel\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"first_tool\",\"arguments\":\"\\\"value\\\":1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let items = completed_items(&events);

        assert_eq!(added.len(), 2);
        assert_eq!(added[0]["output_index"], 0);
        assert_eq!(added[0]["item"]["name"], "first_tool");
        assert_eq!(added[1]["output_index"], 1);
        assert_eq!(added[1]["item"]["name"], "second_tool");
        assert_eq!(items[0]["name"], "first_tool");
        assert_eq!(items[0]["call_id"], "call_first");
        assert_eq!(items[0]["arguments"], r#"{"value":1}"#);
        assert_eq!(items[1]["name"], "second_tool");
        assert_eq!(items[1]["call_id"], "call_second");
        assert_eq!(items[1]["arguments"], r#"{"value":2}"#);
    }

    #[tokio::test]
    async fn finalization_keeps_valid_call_after_unnamed_earlier_call() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_parallel_missing\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_missing\",\"type\":\"function\",\"function\":{\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_parallel_missing\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_valid\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"date\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "exec_command");
        assert_eq!(items[0]["call_id"], "call_valid");
        assert_eq!(items[0]["arguments"], r#"{"cmd":"date"}"#);
        assert!(!output.contains("call_missing"));
    }

    /// #4341：上游只给出畸形工具调用时，丢弃后本回合一个工具调用都不剩，
    /// Codex 会把它当成正常完成而静默收尾。此时必须如实报错。
    #[tokio::test]
    async fn dropped_only_tool_call_emits_failed_without_completed() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_drop\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"content\":\"让我继续处理这个文件\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_drop\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_bad\",\"type\":\"function\",\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("upstream_tool_call_dropped"));
        assert!(!output.contains("event: response.completed"));
        // 已经推给客户端的文本增量不受影响，用户仍能看到模型说了什么。
        assert!(output.contains("让我继续处理这个文件"));
    }

    /// `finish_reason=length`（token 截断）时工具调用往往只到一半就没了 name。
    /// 这不是"上游发了畸形数据"，而是截断——归因必须是 incomplete，不能报成
    /// tool_call_dropped，否则诊断信息本身就是错的。
    #[tokio::test]
    async fn truncated_turn_stays_incomplete_instead_of_failed() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_trunc\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"content\":\"我来看看\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_trunc\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_cut\",\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"pa\"}}]},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"status\":\"incomplete\""));
        assert!(!output.contains("event: response.failed"));
    }

    /// 纯空白函数名对应不到任何已发布工具，必须与空名同等对待，
    /// 否则它会伪装成"本回合还有工具调用"而绕过判据。
    #[tokio::test]
    async fn whitespace_only_tool_name_is_dropped() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_ws\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_ws\",\"type\":\"function\",\"function\":{\"name\":\"   \",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("upstream_tool_call_dropped"));
        assert!(!output.contains("event: response.completed"));
    }

    /// 纯文本回合（从未出现过工具调用增量）不受判据影响。
    #[tokio::test]
    async fn text_only_turn_still_completes() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_text\",\"model\":\"kimi-k3\",\"choices\":[{\"delta\":{\"content\":\"完成了\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);

        assert!(output.contains("event: response.completed"));
        assert!(!output.contains("event: response.failed"));
        for event_type in ["response.created", "response.completed"] {
            let event = events
                .iter()
                .find(|event| event["type"] == event_type)
                .unwrap();
            assert_eq!(
                event["response"]["usage"]["input_tokens_details"]["cached_tokens"],
                0
            );
        }
    }

    /// 上游省略 `index` 时，两个 id 不同的调用不得坍缩成一个。
    #[tokio::test]
    async fn missing_index_with_distinct_ids_keeps_calls_separate() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_noidx\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_noidx\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["call_id"], "call_a");
        assert_eq!(items[0]["name"], "read_file");
        assert_eq!(items[0]["arguments"], r#"{"path":"a.txt"}"#);
        assert_eq!(items[1]["call_id"], "call_b");
        assert_eq!(items[1]["name"], "exec_command");
        assert_eq!(items[1]["arguments"], r#"{"cmd":"ls"}"#);
    }

    /// #6449：不同非空 ID 即使复用了同一个显式 index，也必须保持独立身份与参数。
    #[tokio::test]
    async fn reused_explicit_index_with_distinct_ids_keeps_calls_separate() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_dupidx\",\"model\":\"minimax-m3\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_function_dup_1\",\"type\":\"function\",\"function\":{\"name\":\"shell_command\",\"arguments\":\"{\\\"command\\\":\\\"dir a\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_dupidx\",\"model\":\"minimax-m3\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_function_dup_2\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"command\\\":\\\"dir b\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();
        let expected = [
            (
                "fc_call_function_dup_1",
                "call_function_dup_1",
                "shell_command",
                "dir a",
            ),
            (
                "fc_call_function_dup_2",
                "call_function_dup_2",
                "exec_command",
                "dir b",
            ),
        ];

        assert_eq!(added.len(), 2);
        assert_eq!(done.len(), 2);
        assert_eq!(items.len(), 2);
        for (index, (item_id, call_id, name, command)) in expected.into_iter().enumerate() {
            let item = &items[index];
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], name);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], *item);
            assert_eq!(item["type"], "function_call");
            assert_eq!(item["id"], item_id);
            assert_eq!(item["call_id"], call_id);
            assert_eq!(item["name"], name);
            let arguments = item["arguments"].as_str().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(arguments).unwrap(),
                json!({ "command": command })
            );
        }
        assert!(!output.contains(r#"{"command":"dir a"}{"command":"dir b"}"#));
    }

    /// P1: a synthetic internal key must never become addressable as a later raw index.
    #[tokio::test]
    async fn p1_four_frame_real_index_alias_repro() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_c\",\"type\":\"function\",\"function\":{\"name\":\"tool_c\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"c\\\":3}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let items = completed_items(&events);
        let expected = [
            ("fc_call_a", "call_a", "tool_a", json!({ "a": 1 })),
            ("fc_call_b", "call_b", "tool_b", json!({ "b": 2 })),
            ("fc_call_c", "call_c", "tool_c", json!({ "c": 3 })),
        ];

        assert_eq!(added.len(), expected.len());
        assert_eq!(done.len(), expected.len());
        assert_eq!(items.len(), expected.len());
        let raw_arguments = items
            .iter()
            .map(|item| item["arguments"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            raw_arguments,
            vec![r#"{"a":1}"#, r#"{"b":2}"#, r#"{"c":3}"#]
        );
        for (index, (item_id, call_id, name, arguments)) in expected.into_iter().enumerate() {
            let item = &items[index];
            assert_tool_lifecycle(
                &events,
                item_id,
                index as u64,
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
            );
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], name);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], *item);
            assert_eq!(item["type"], "function_call");
            assert_eq!(item["id"], item_id);
            assert_eq!(item["call_id"], call_id);
            assert_eq!(item["name"], name);
            assert_eq!(
                serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
                arguments
            );
        }
    }

    #[tokio::test]
    async fn idless_continuation_follows_latest_reused_raw_index_owner() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_owner\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"owner\\\":\\\"a\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_owner\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"owner\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_owner\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"type\":\"function\",\"function\":{\"arguments\":\"\\\"b\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let items = completed_items(&events);

        assert_eq!(added.len(), 2);
        assert_eq!(done.len(), 2);
        assert_eq!(items.len(), 2);
        assert_eq!(added[0]["item"]["id"], "fc_call_a");
        assert_eq!(added[1]["item"]["id"], "fc_call_b");
        assert_eq!(done[0]["item"], items[0]);
        assert_eq!(done[1]["item"], items[1]);
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "owner": "a" }),
        );
        assert_function_item(
            &items[1],
            "fc_call_b",
            "call_b",
            "tool_b",
            json!({ "owner": "b" }),
        );
    }

    #[tokio::test]
    async fn known_id_moves_and_rebinds_raw_owner_back_to_itself() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"parts\\\":[\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":7,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"1\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":7,\"type\":\"function\",\"function\":{\"arguments\":\",2\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"owner\\\":\\\"b\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\",3\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"type\":\"function\",\"function\":{\"arguments\":\",4\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_move\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":7,\"type\":\"function\",\"function\":{\"arguments\":\"]}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 2);
        assert_eq!(done.len(), 2);
        assert_eq!(items.len(), 2);
        for (index, (item_id, call_id, name)) in [
            ("fc_call_a", "call_a", "tool_a"),
            ("fc_call_b", "call_b", "tool_b"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], name);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], items[index]);
        }
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "parts": [1, 2, 3, 4] }),
        );
        assert_function_item(
            &items[1],
            "fc_call_b",
            "call_b",
            "tool_b",
            json!({ "owner": "b" }),
        );
    }

    #[tokio::test]
    async fn same_batch_collisions_and_split_fragments_keep_arrival_order() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_batch\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":1}\"}},{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":\"}},{\"index\":1,\"id\":\"call_c\",\"type\":\"function\",\"function\":{\"name\":\"tool_c\",\"arguments\":\"{\\\"c\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_batch\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"3}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_batch\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"type\":\"function\",\"function\":{\"arguments\":\"2}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 3);
        assert_eq!(done.len(), 3);
        assert_eq!(items.len(), 3);
        for (index, (item_id, call_id, name)) in [
            ("fc_call_a", "call_a", "tool_a"),
            ("fc_call_b", "call_b", "tool_b"),
            ("fc_call_c", "call_c", "tool_c"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], name);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], items[index]);
        }
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "a": 1 }),
        );
        assert_function_item(
            &items[1],
            "fc_call_b",
            "call_b",
            "tool_b",
            json!({ "b": 2 }),
        );
        assert_function_item(
            &items[2],
            "fc_call_c",
            "call_c",
            "tool_c",
            json!({ "c": 3 }),
        );
    }

    #[tokio::test]
    async fn empty_ids_follow_alias_while_whitespace_ids_remain_identity() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_ids\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ids\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"function\",\"function\":{\"arguments\":\"1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ids\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\" \",\"type\":\"function\",\"function\":{\"name\":\"tool_space\",\"arguments\":\"{\\\"space\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_ids\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"true}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);

        assert_eq!(items.len(), 2);
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "a": 1 }),
        );
        assert_function_item(
            &items[1],
            "fc_ ",
            " ",
            "tool_space",
            json!({ "space": true }),
        );
    }

    #[tokio::test]
    async fn repeated_id_with_changed_explicit_index_stays_in_one_call() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_reindexed\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_stable\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reindexed\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":7,\"id\":\"call_stable\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "fc_call_stable");
        assert_eq!(items[0]["call_id"], "call_stable");
        assert_eq!(items[0]["name"], "read_file");
        assert_eq!(items[0]["arguments"], r#"{"path":"README.md"}"#);
    }

    #[test]
    fn explicit_index_collision_at_max_uses_first_gap_without_overflow() {
        let mut state = ChatToResponsesState::default();
        state.tools.insert(
            0,
            ToolCallState {
                call_id: "call_zero".to_string(),
                ..ToolCallState::default()
            },
        );
        state.tools.insert(
            usize::MAX,
            ToolCallState {
                call_id: "call_max".to_string(),
                ..ToolCallState::default()
            },
        );

        assert_eq!(
            state.resolve_tool_key(Some(usize::MAX), Some("call_new")),
            Some(1)
        );
        assert_eq!(state.raw_index_to_key.get(&usize::MAX), Some(&1));
    }

    #[test]
    fn max_internal_tool_key_flush_and_finalize_do_not_overflow_cursor() {
        let mut state = ChatToResponsesState {
            next_tool_index_to_add: usize::MAX,
            ..ChatToResponsesState::default()
        };
        state.tools.insert(
            usize::MAX,
            ToolCallState {
                call_id: "call_max".to_string(),
                name: "tool_max".to_string(),
                arguments: "{}".to_string(),
                ..ToolCallState::default()
            },
        );

        let events = state.flush_ready_tool_calls();
        assert!(!events.is_empty());
        assert_eq!(state.next_tool_index_to_add, usize::MAX);
        assert!(state.tools[&usize::MAX].added);

        // The already-added branch must also terminate safely at the boundary.
        assert!(state.flush_ready_tool_calls().is_empty());
        let final_events = state.finalize_tools();
        assert!(!final_events.is_empty());
        assert!(state.tools[&usize::MAX].done);
    }

    #[test]
    fn resolver_rebinds_raw_owner_between_known_ids() {
        let mut state = ChatToResponsesState::default();
        state.tools.insert(
            0,
            ToolCallState {
                call_id: "call_a".to_string(),
                ..ToolCallState::default()
            },
        );
        state.tools.insert(
            1,
            ToolCallState {
                call_id: "call_b".to_string(),
                ..ToolCallState::default()
            },
        );
        state.raw_index_to_key.insert(0, 0);

        assert_eq!(state.resolve_tool_key(Some(0), Some("call_b")), Some(1));
        assert_eq!(state.resolve_tool_key(Some(0), None), Some(1));
        assert_eq!(state.resolve_tool_key(Some(0), Some("call_a")), Some(0));
        assert_eq!(state.resolve_tool_key(Some(0), None), Some(0));
    }

    #[test]
    fn resolver_adopts_only_unaliased_anonymous_matching_state() {
        let mut unaliased = ChatToResponsesState::default();
        unaliased.tools.insert(0, ToolCallState::default());

        assert_eq!(
            unaliased.resolve_tool_key(Some(0), Some("call_late")),
            Some(0)
        );
        assert_eq!(unaliased.raw_index_to_key.get(&0), Some(&0));

        let mut already_aliased = ChatToResponsesState::default();
        already_aliased.tools.insert(1, ToolCallState::default());
        already_aliased.raw_index_to_key.insert(7, 1);

        assert_eq!(
            already_aliased.resolve_tool_key(Some(1), Some("call_new")),
            Some(2)
        );
        assert_eq!(already_aliased.raw_index_to_key.get(&1), Some(&2));
        assert_eq!(already_aliased.raw_index_to_key.get(&7), Some(&1));
    }

    #[test]
    fn resolver_preserves_empty_whitespace_and_missing_identity_contracts() {
        let mut state = ChatToResponsesState::default();
        state.tools.insert(
            2,
            ToolCallState {
                call_id: "call_a".to_string(),
                ..ToolCallState::default()
            },
        );
        state.tools.insert(
            9,
            ToolCallState {
                call_id: " ".to_string(),
                ..ToolCallState::default()
            },
        );
        state.tools.insert(
            12,
            ToolCallState {
                call_id: "call_greatest".to_string(),
                ..ToolCallState::default()
            },
        );
        state.raw_index_to_key.insert(2, 2);

        assert_eq!(state.resolve_tool_key(Some(2), Some("")), Some(2));
        assert_eq!(state.resolve_tool_key(Some(7), Some(" ")), Some(9));
        assert_eq!(state.raw_index_to_key.get(&7), Some(&9));
        assert_eq!(state.resolve_tool_key(Some(2), Some("call_a")), Some(2));
        // The most recently addressed key is 2, but the documented no-signal fallback
        // remains the greatest internal key rather than an inferred active owner.
        assert_eq!(state.resolve_tool_key(None, None), Some(12));
    }

    #[test]
    fn wire_tool_index_conversion_is_checked() {
        assert_eq!(chat_tool_index(&json!({ "index": 0 })), Some(0));
        assert_eq!(chat_tool_index(&json!({ "index": -1 })), None);

        #[cfg(target_pointer_width = "32")]
        assert_eq!(
            chat_tool_index(&json!({ "index": u64::from(u32::MAX) + 1 })),
            None
        );

        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            chat_tool_index(&json!({ "index": u64::MAX })),
            Some(usize::MAX)
        );
    }

    /// 上游省略 `index` 时，不带 id 的 arguments 续帧必须归入同一个调用，
    /// 不能被当成新调用炸成多个 item。
    #[tokio::test]
    async fn missing_index_argument_fragments_stay_in_one_call() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_frag\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_frag\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"type\":\"function\",\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["call_id"], "call_a");
        assert_eq!(items[0]["arguments"], r#"{"path":"a.txt"}"#);
    }

    #[tokio::test]
    async fn explicit_index_adopts_an_unaliased_anonymous_late_identity() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_late_identity\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"type\":\"function\",\"function\":{\"name\":\"tool_late\",\"arguments\":\"{\\\"value\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_late_identity\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_late\",\"type\":\"function\",\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);

        assert_eq!(items.len(), 1);
        assert_function_item(
            &items[0],
            "fc_call_late",
            "call_late",
            "tool_late",
            json!({ "value": 1 }),
        );
    }

    #[tokio::test]
    async fn mapped_anonymous_index_adopts_late_id_without_splitting_state() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_mapped_late\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":5,\"type\":\"function\",\"function\":{\"name\":\"tool_late\",\"arguments\":\"{\\\"value\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_mapped_late\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":5,\"id\":\"call_late\",\"type\":\"function\",\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);

        assert_eq!(items.len(), 1);
        assert_function_item(
            &items[0],
            "fc_call_late",
            "call_late",
            "tool_late",
            json!({ "value": 1 }),
        );
    }

    #[tokio::test]
    async fn identified_unaliased_state_is_not_adopted_by_numeric_index() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_nonanonymous\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_nonanonymous\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_nonanonymous\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"type\":\"function\",\"function\":{\"arguments\":\"2}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);

        assert_eq!(items.len(), 2);
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "a": 1 }),
        );
        assert_function_item(
            &items[1],
            "fc_call_b",
            "call_b",
            "tool_b",
            json!({ "b": 2 }),
        );
    }

    /// 上游省略 `index` 且重复下发同一个 id（部分网关每帧重复整个头部）时，
    /// 不得被判成新调用。
    #[tokio::test]
    async fn missing_index_repeated_same_id_stays_in_one_call() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_rep\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_rep\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\\\"a.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["call_id"], "call_a");
        assert_eq!(items[0]["arguments"], r#"{"path":"a.txt"}"#);
    }

    #[tokio::test]
    async fn oversized_wire_index_uses_missing_index_fallback_without_aliasing() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_bigidx\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_bigidx\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":18446744073709551616,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_bigidx\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"type\":\"function\",\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);

        assert_eq!(items.len(), 2);
        assert_function_item(
            &items[0],
            "fc_call_a",
            "call_a",
            "tool_a",
            json!({ "a": 1 }),
        );
        assert_function_item(
            &items[1],
            "fc_call_b",
            "call_b",
            "tool_b",
            json!({ "b": 2 }),
        );
    }

    #[tokio::test]
    async fn finalization_keeps_non_contiguous_tool_index() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_sparse\",\"model\":\"deepseek-v4-pro\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":2,\"id\":\"call_sparse\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        let items = completed["response"]["output"].as_array().unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "read_file");
        assert_eq!(items[0]["call_id"], "call_sparse");
        assert_eq!(items[0]["arguments"], r#"{"path":"README.md"}"#);
    }

    #[tokio::test]
    async fn restores_custom_tool_input_stream_events() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{ "type": "custom", "name": "exec" }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_custom\",\"type\":\"function\",\"function\":{\"name\":\"exec\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"input\\\":\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls -la\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("event: response.custom_tool_call_input.delta"));
        assert!(output.contains("event: response.custom_tool_call_input.done"));
        assert!(!output.contains("event: response.function_call_arguments.delta"));
        assert!(!output.contains("event: response.function_call_arguments.done"));
        assert!(output.contains("\"id\":\"ctc_call_custom\""));
        assert!(output.contains("\"type\":\"custom_tool_call\""));
        assert!(output.contains("\"name\":\"exec\""));
        assert!(output.contains("\"input\":\"ls -la\""));
    }

    #[tokio::test]
    async fn custom_tool_inputs_survive_raw_index_alias_collisions() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{ "type": "custom", "name": "exec" }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_custom_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"custom_a\",\"type\":\"function\",\"function\":{\"name\":\"exec\",\"arguments\":\"{\\\"input\\\":\\\"A\\\"}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"ordinary_b\",\"type\":\"function\",\"function\":{\"name\":\"normal_tool\",\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"custom_c\",\"type\":\"function\",\"function\":{\"name\":\"exec\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"input\\\":\\\"C\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let custom_deltas = events
            .iter()
            .filter(|event| event["type"] == "response.custom_tool_call_input.delta")
            .collect::<Vec<_>>();
        let custom_dones = events
            .iter()
            .filter(|event| event["type"] == "response.custom_tool_call_input.done")
            .collect::<Vec<_>>();
        let function_deltas = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.delta")
            .collect::<Vec<_>>();
        let function_dones = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.done")
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 3);
        assert_eq!(done.len(), 3);
        assert_eq!(custom_deltas.len(), 2);
        assert_eq!(custom_dones.len(), 2);
        assert_eq!(function_deltas.len(), 1);
        assert_eq!(function_dones.len(), 1);
        for (index, (item_id, call_id, name)) in [
            ("ctc_custom_a", "custom_a", "exec"),
            ("fc_ordinary_b", "ordinary_b", "normal_tool"),
            ("ctc_custom_c", "custom_c", "exec"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], name);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], items[index]);
        }
        for (event, item_id, output_index, delta) in [
            (&custom_deltas[0], "ctc_custom_a", 0, "A"),
            (&custom_deltas[1], "ctc_custom_c", 2, "C"),
        ] {
            assert_eq!(event["item_id"], item_id);
            assert_eq!(event["output_index"], output_index);
            assert_eq!(event["delta"], delta);
        }
        for (event, item_id, output_index, input) in [
            (&custom_dones[0], "ctc_custom_a", 0, "A"),
            (&custom_dones[1], "ctc_custom_c", 2, "C"),
        ] {
            assert_eq!(event["item_id"], item_id);
            assert_eq!(event["output_index"], output_index);
            assert_eq!(event["input"], input);
        }
        assert_eq!(function_deltas[0]["item_id"], "fc_ordinary_b");
        assert_eq!(function_deltas[0]["output_index"], 1);
        assert_eq!(function_deltas[0]["delta"], r#"{"b":2}"#);
        assert_eq!(function_dones[0]["item_id"], "fc_ordinary_b");
        assert_eq!(function_dones[0]["output_index"], 1);
        assert_eq!(function_dones[0]["arguments"], r#"{"b":2}"#);
        assert_tool_lifecycle(
            &events,
            "ctc_custom_a",
            0,
            "response.custom_tool_call_input.delta",
            "response.custom_tool_call_input.done",
        );
        assert_tool_lifecycle(
            &events,
            "fc_ordinary_b",
            1,
            "response.function_call_arguments.delta",
            "response.function_call_arguments.done",
        );
        assert_tool_lifecycle(
            &events,
            "ctc_custom_c",
            2,
            "response.custom_tool_call_input.delta",
            "response.custom_tool_call_input.done",
        );
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["id"], "ctc_custom_a");
        assert_eq!(items[0]["type"], "custom_tool_call");
        assert_eq!(items[0]["call_id"], "custom_a");
        assert_eq!(items[0]["input"], "A");
        assert_eq!(items[1]["id"], "fc_ordinary_b");
        assert_function_item(
            &items[1],
            "fc_ordinary_b",
            "ordinary_b",
            "normal_tool",
            json!({ "b": 2 }),
        );
        assert_eq!(items[2]["id"], "ctc_custom_c");
        assert_eq!(items[2]["type"], "custom_tool_call");
        assert_eq!(items[2]["call_id"], "custom_c");
        assert_eq!(items[2]["input"], "C");
    }

    #[tokio::test]
    async fn canonicalizes_streamed_tool_call_arguments_on_done_events() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{ \\\"b\\\": 2,\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" \\\"a\\\": 1 }\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains(r#""arguments":"{\"a\":1,\"b\":2}""#));
    }

    #[tokio::test]
    async fn preserves_reasoning_content_on_streamed_tool_call_items() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Need file.\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.output_item.done"));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"reasoning_content\":\"Need file.\""));
    }

    #[tokio::test]
    async fn preserves_late_reasoning_content_on_streamed_tool_call_items() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_tool_late_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_late_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_late_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Need file.\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_late_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.output_item.done"));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"reasoning_content\":\"Need file.\""));
    }

    #[tokio::test]
    async fn reasoning_metadata_survives_raw_index_alias_collisions() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Plan. \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Later.\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_c\",\"type\":\"function\",\"function\":{\"name\":\"tool_c\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"c\\\":3}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let reasoning_part_added = events
            .iter()
            .position(|event| event["type"] == "response.reasoning_summary_part.added")
            .unwrap();
        let reasoning_deltas = events
            .iter()
            .filter(|event| event["type"] == "response.reasoning_summary_text.delta")
            .collect::<Vec<_>>();
        let reasoning_text_done = events
            .iter()
            .position(|event| event["type"] == "response.reasoning_summary_text.done")
            .unwrap();
        let reasoning_part_done = events
            .iter()
            .position(|event| event["type"] == "response.reasoning_summary_part.done")
            .unwrap();
        let reasoning_item_done = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "reasoning"
            })
            .unwrap();
        let first_tool_added = events
            .iter()
            .position(|event| {
                event["type"] == "response.output_item.added"
                    && event["item"]["type"] == "function_call"
            })
            .unwrap();

        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["type"], "reasoning");
        assert_eq!(items[0]["summary"][0]["text"], "Plan. Later.");
        assert_eq!(reasoning_deltas.len(), 2);
        assert_eq!(reasoning_deltas[0]["delta"], "Plan. ");
        assert_eq!(reasoning_deltas[1]["delta"], "Later.");
        assert!(reasoning_part_added < reasoning_text_done);
        assert!(reasoning_text_done < reasoning_part_done);
        assert!(reasoning_part_done < reasoning_item_done);
        assert!(reasoning_item_done < first_tool_added);
        for (offset, (item_id, call_id, name, arguments)) in [
            ("fc_call_a", "call_a", "tool_a", json!({ "a": 1 })),
            ("fc_call_b", "call_b", "tool_b", json!({ "b": 2 })),
            ("fc_call_c", "call_c", "tool_c", json!({ "c": 3 })),
        ]
        .into_iter()
        .enumerate()
        {
            let item = &items[offset + 1];
            assert_function_item(item, item_id, call_id, name, arguments);
            assert_eq!(item["reasoning_content"], "Plan. Later.");
        }
    }

    #[tokio::test]
    async fn restores_namespace_on_streamed_tool_call_items() {
        let request = json!({
            "model": "gpt-5.4",
            "input": [{
                "type": "tool_search_output",
                "call_id": "call_tool_search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__codex_apps__gmail",
                    "tools": [{
                        "type": "function",
                        "name": "_search_emails",
                        "description": "Search Gmail.",
                        "parameters": {"type": "object"}
                    }]
                }]
            }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_gmail\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_gmail\",\"type\":\"function\",\"function\":{\"name\":\"mcp__codex_apps__gmail___search_emails\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_gmail\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\":\\\"in:inbox\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"namespace\":\"mcp__codex_apps__gmail\""));
        assert!(output.contains("\"name\":\"_search_emails\""));
        assert!(output.contains(r#""arguments":"{\"query\":\"in:inbox\"}""#));
    }

    #[tokio::test]
    async fn namespace_metadata_survives_raw_index_alias_collisions() {
        let request = json!({
            "model": "gpt-5.4",
            "input": [{
                "type": "tool_search_output",
                "call_id": "call_tool_search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__codex_apps__gmail",
                    "tools": [{
                        "type": "function",
                        "name": "_search_emails",
                        "description": "Search Gmail.",
                        "parameters": {"type": "object"}
                    }]
                }]
            }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_namespace_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"mcp__codex_apps__gmail___search_emails\",\"arguments\":\"{\\\"query\\\":\\\"a\\\"}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_namespace_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"mcp__codex_apps__gmail___search_emails\",\"arguments\":\"{\\\"query\\\":\\\"b\\\"}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_namespace_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_c\",\"type\":\"function\",\"function\":{\"name\":\"mcp__codex_apps__gmail___search_emails\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_namespace_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"query\\\":\\\"c\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let argument_deltas = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.delta")
            .collect::<Vec<_>>();
        let argument_dones = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.done")
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 3);
        assert_eq!(done.len(), 3);
        assert_eq!(argument_deltas.len(), 3);
        assert_eq!(argument_dones.len(), 3);
        assert_eq!(items.len(), 3);
        for (index, (item_id, call_id, query)) in [
            ("fc_call_a", "call_a", "a"),
            ("fc_call_b", "call_b", "b"),
            ("fc_call_c", "call_c", "c"),
        ]
        .into_iter()
        .enumerate()
        {
            let item = &items[index];
            assert_tool_lifecycle(
                &events,
                item_id,
                index as u64,
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
            );
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["id"], item_id);
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(added[index]["item"]["name"], "_search_emails");
            assert_eq!(added[index]["item"]["type"], "function_call");
            assert_eq!(added[index]["item"]["namespace"], "mcp__codex_apps__gmail");
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], *item);
            assert_eq!(argument_deltas[index]["item_id"], item_id);
            assert_eq!(argument_deltas[index]["output_index"], index as u64);
            assert_eq!(
                argument_deltas[index]["delta"],
                format!(r#"{{"query":"{query}"}}"#)
            );
            assert_eq!(argument_dones[index]["item_id"], item_id);
            assert_eq!(argument_dones[index]["output_index"], index as u64);
            assert_eq!(
                argument_dones[index]["arguments"],
                format!(r#"{{"query":"{query}"}}"#)
            );
            assert_eq!(item["id"], item_id);
            assert_eq!(item["type"], "function_call");
            assert_eq!(item["call_id"], call_id);
            assert_eq!(item["namespace"], "mcp__codex_apps__gmail");
            assert_eq!(item["name"], "_search_emails");
            assert_eq!(
                serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
                json!({ "query": query })
            );
        }
    }

    #[tokio::test]
    async fn restores_tool_search_on_streamed_tool_call_items() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": "Search for Gmail tools."
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_tool_search\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_tool_search_1\",\"type\":\"function\",\"function\":{\"name\":\"tool_search\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_tool_search\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\":\\\"Gmail search emails\\\",\\\"limit\\\":10}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("\"type\":\"tool_search_call\""));
        assert!(output.contains("\"execution\":\"client\""));
        assert!(output.contains("\"call_id\":\"call_tool_search_1\""));
        assert!(output.contains("\"query\":\"Gmail search emails\""));
    }

    #[tokio::test]
    async fn tool_search_metadata_survives_raw_index_alias_collisions() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": "Search for tools."
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_search_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"search_a\",\"type\":\"function\",\"function\":{\"name\":\"tool_search\",\"arguments\":\"{\\\"query\\\":\\\"a\\\"}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_search_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"search_b\",\"type\":\"function\",\"function\":{\"name\":\"tool_search\",\"arguments\":\"{\\\"query\\\":\\\"b\\\"}\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_search_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"search_c\",\"type\":\"function\",\"function\":{\"name\":\"tool_search\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_search_alias\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"query\\\":\\\"c\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;
        let events = parse_sse_events(&output);
        let items = completed_items(&events);
        let added = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect::<Vec<_>>();
        let done = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.done")
            .collect::<Vec<_>>();
        let argument_deltas = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.delta")
            .collect::<Vec<_>>();
        let argument_dones = events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.done")
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 3);
        assert_eq!(done.len(), 3);
        assert_eq!(argument_deltas.len(), 3);
        assert_eq!(argument_dones.len(), 3);
        assert_eq!(items.len(), 3);
        for (index, (call_id, query)) in [("search_a", "a"), ("search_b", "b"), ("search_c", "c")]
            .into_iter()
            .enumerate()
        {
            let item = &items[index];
            let item_id = format!("fc_{call_id}");
            let arguments = format!(r#"{{"query":"{query}"}}"#);
            assert_tool_search_lifecycle(&events, call_id, &item_id, index as u64);
            assert_eq!(added[index]["output_index"], index as u64);
            assert_eq!(added[index]["item"]["type"], "tool_search_call");
            assert!(added[index]["item"].get("id").is_none());
            assert_eq!(added[index]["item"]["execution"], "client");
            assert_eq!(added[index]["item"]["call_id"], call_id);
            assert_eq!(done[index]["output_index"], index as u64);
            assert_eq!(done[index]["item"], *item);
            assert_eq!(argument_deltas[index]["item_id"], item_id);
            assert_eq!(argument_deltas[index]["output_index"], index as u64);
            assert_eq!(argument_deltas[index]["delta"], arguments);
            assert_eq!(argument_dones[index]["item_id"], item_id);
            assert_eq!(argument_dones[index]["output_index"], index as u64);
            assert_eq!(argument_dones[index]["arguments"], arguments);
            assert_eq!(item["type"], "tool_search_call");
            assert_eq!(item["call_id"], call_id);
            assert_eq!(item["status"], "completed");
            assert_eq!(item["execution"], "client");
            assert_eq!(item["arguments"], json!({ "query": query }));
        }
    }

    #[tokio::test]
    async fn stream_error_emits_failed_without_completed() {
        let upstream = stream::iter(vec![Err::<Bytes, std::io::Error>(std::io::Error::other(
            "boom",
        ))]);
        let converted = create_responses_sse_stream_from_chat(upstream);
        let bytes: Vec<Bytes> = converted.map(|item| item.unwrap()).collect().await;
        let output = String::from_utf8(bytes.concat()).unwrap();

        assert!(output.contains("event: response.failed"));
        assert!(!output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn stream_end_with_output_without_finish_reason_emits_incomplete_without_failed() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_truncated\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ])
        .await;

        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"status\":\"incomplete\""));
        assert!(output.contains("\"incomplete_details\":{\"reason\":\"max_output_tokens\"}"));
        assert!(!output.contains("event: response.failed"));
    }

    #[tokio::test]
    async fn stream_end_without_output_or_finish_reason_emits_failed_without_completed() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_truncated\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{}}]}\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("stream_truncated"));
        assert!(!output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn chat_sse_error_event_emits_failed_without_completed() {
        let output = collect(vec![
            "event: error\ndata: {\"error\":{\"message\":\"bad request\",\"type\":\"invalid_request_error\"}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("bad request"));
        assert!(output.contains("invalid_request_error"));
        assert!(!output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn chat_sse_data_only_error_emits_failed_without_completed() {
        let output = collect(vec![
            "data: {\"error\":{\"message\":\"quota exceeded\",\"code\":\"rate_limit_exceeded\"}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("quota exceeded"));
        assert!(output.contains("rate_limit_exceeded"));
        assert!(!output.contains("event: response.completed"));
    }
}
