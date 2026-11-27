//! Replay evidence for Codex tool turns sent to Claude Fable 5.2.

use super::anthropic_tool_schema::ANTHROPIC_TOOL_ARGS_PROPERTY;
use super::transform_codex_anthropic::{
    anthropic_tool_uses_wrapped_arguments, unwrap_anthropic_tool_arguments,
};
use super::transform_codex_chat::CodexToolContext;
use super::transform_responses::sanitize_anthropic_tool_use_input;
use crate::provider::Provider;
use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::proxy::ProxyError;
use crate::sync_utils::MutexExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Fingerprint = [u8; 32];
const MAX_TURNS: usize = 512;
const TURN_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_BLOCKS: usize = 256;
const MAX_TRACKED_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MISSING_THINKING: &str = "Anthropic model requires thinking, but the tool history has no signed thinking block to replay";
pub(super) const UNVERIFIED_REPLAY: &str = "Cannot verify the Fable tool replay or its thinking history. Start a new conversation and run the tool again.";
const INVALID_TOOL_TURN: &str =
    "The upstream returned an incomplete or invalid Fable tool or thinking turn";

fn invalid_tool_turn() -> ProxyError {
    ProxyError::TransformError(INVALID_TOOL_TURN.to_string())
}

pub(crate) fn is_fable_5_2(model: &str) -> bool {
    strip_one_m_suffix_for_upstream(model)
        .trim()
        .to_ascii_lowercase()
        .replace(['.', '_'], "-")
        == "claude-fable-5-2"
}

#[derive(Clone, PartialEq, Eq)]
struct ToolTurn {
    call_ids: Vec<Fingerprint>,
    items: Vec<Fingerprint>,
    signed: bool,
}

impl ToolTurn {
    fn from_content(content: &[Value], tools: Option<&CodexToolContext>) -> Option<Self> {
        if content.len() > MAX_BLOCKS {
            return None;
        }
        let mut turn = Self {
            call_ids: Vec::new(),
            items: Vec::new(),
            signed: false,
        };
        for block in content {
            match block.get("type").and_then(Value::as_str)? {
                "tool_use" => {
                    let id = nonempty_string(block.get("id")?)?;
                    let name = nonempty_string(block.get("name")?)?;
                    let input = block.get("input")?.as_object()?;
                    turn.call_ids.push(digest(id.as_bytes()));
                    let mut tool = Sha256::new();
                    update_digest(&mut tool, b"tool_use");
                    update_digest(&mut tool, id.as_bytes());
                    update_digest(&mut tool, name.as_bytes());
                    let mut input = Value::Object(input.clone());
                    if let Some(tools) = tools {
                        let input_name =
                            tools.lookup_chat_name(name).map_or(name, |spec| &spec.name);
                        input = unwrap_anthropic_tool_arguments(name, input, tools).ok()?;
                        input = sanitize_anthropic_tool_use_input(input_name, input);
                        if anthropic_tool_uses_wrapped_arguments(name, tools) {
                            input = json!({ ANTHROPIC_TOOL_ARGS_PROPERTY: input });
                        }
                    }
                    let input = canonical_json_string(&input);
                    update_digest(&mut tool, input.as_bytes());
                    turn.items.push(tool.finalize().into());
                }
                "thinking" => {
                    block.get("thinking")?.as_str()?;
                    nonempty_string(block.get("signature")?)?;
                    turn.signed = true;
                    turn.items
                        .push(digest(canonical_json_string(block).as_bytes()));
                }
                "redacted_thinking" => {
                    nonempty_string(block.get("data")?)?;
                    turn.signed = true;
                    turn.items
                        .push(digest(canonical_json_string(block).as_bytes()));
                }
                "text" => {
                    let text = block.get("text")?.as_str()?;
                    if !text.trim().is_empty() {
                        let mut fingerprint = Sha256::new();
                        update_digest(&mut fingerprint, b"text");
                        update_digest(&mut fingerprint, text.as_bytes());
                        turn.items.push(fingerprint.finalize().into());
                    }
                }
                _ => return None,
            }
        }
        let call_count = turn.call_ids.len();
        turn.call_ids.sort_unstable();
        turn.call_ids.dedup();
        (call_count > 0 && call_count == turn.call_ids.len()).then_some(turn)
    }
}

fn nonempty_string(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.trim().is_empty())
}

fn digest(bytes: &[u8]) -> Fingerprint {
    Sha256::digest(bytes).into()
}

fn update_digest(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

struct StoredTurn {
    scope: Fingerprint,
    observed: Instant,
    call_ids: Vec<Fingerprint>,
    // None means a tool turn was observed but the transport or payload was incomplete.
    turn: Option<ToolTurn>,
}

#[derive(Default)]
pub(crate) struct FableReplayStore(Mutex<VecDeque<StoredTurn>>);

#[derive(Clone)]
pub(crate) struct FableReplay {
    store: Arc<FableReplayStore>,
    scope: Option<Fingerprint>,
}

impl FableReplay {
    pub(crate) fn new(
        store: Arc<FableReplayStore>,
        session: Option<&str>,
        provider: &Provider,
        url: &str,
        model: &str,
        headers: &http::HeaderMap,
    ) -> Self {
        let Some(session) = session else {
            return Self { store, scope: None };
        };
        let mut hash = Sha256::new();
        for component in [session, provider.id.as_str(), url, model] {
            update_digest(&mut hash, component.as_bytes());
        }
        update_digest(
            &mut hash,
            canonical_json_string(&provider.settings_config).as_bytes(),
        );
        // Use the final credentials. A provider row can be edited without changing its ID.
        let configured_headers = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.local_proxy_request_overrides.as_ref());
        let mut names = vec![
            "authorization",
            "x-api-key",
            "api-key",
            "chatgpt-account-id",
            "cookie",
        ];
        if let Some(overrides) = configured_headers {
            names.extend(overrides.headers.keys().map(String::as_str));
        }
        names.sort_unstable();
        names.dedup();
        for name in names {
            update_digest(&mut hash, name.as_bytes());
            for value in headers.get_all(name) {
                update_digest(&mut hash, value.as_bytes());
            }
        }
        Self {
            store,
            scope: Some(hash.finalize().into()),
        }
    }

    fn record(&self, call_ids: Vec<Fingerprint>, turn: Option<ToolTurn>) {
        let Some(scope) = self.scope else {
            return;
        };
        if call_ids.is_empty() {
            return;
        }
        let mut turns = self.store.0.lock_recover();
        turns.retain(|entry| entry.observed.elapsed() < TURN_TTL);
        if turns.len() >= MAX_TURNS {
            turns.pop_front();
        }
        turns.push_back(StoredTurn {
            scope,
            observed: Instant::now(),
            call_ids,
            turn,
        });
    }

    fn admits(&self, turn: &ToolTurn) -> bool {
        let Some(scope) = self.scope else {
            return turn.signed;
        };
        let turns = self.store.0.lock_recover();
        let mut observed = false;
        for entry in turns.iter().filter(|entry| {
            entry.scope == scope
                && entry.observed.elapsed() < TURN_TTL
                && entry.call_ids.iter().any(|id| turn.call_ids.contains(id))
        }) {
            if entry.turn.as_ref() != Some(turn) {
                return false;
            }
            observed = true;
        }
        observed || turn.signed
    }

    pub(crate) fn record_message(
        &self,
        body: &Value,
        wire_bytes: usize,
        tools: &CodexToolContext,
    ) -> Result<(), ProxyError> {
        let Some(content) = body.get("content").and_then(Value::as_array) else {
            return Ok(());
        };
        let valid_message = wire_bytes <= MAX_TRACKED_BYTES
            && valid_message_start(body)
            && body.get("role").and_then(Value::as_str) == Some("assistant")
            && body.get("error").is_none();
        let stop = body.get("stop_reason").and_then(Value::as_str);
        self.record_content(content, valid_message && stop == Some("tool_use"), tools)
    }

    fn record_content(
        &self,
        content: &[Value],
        complete: bool,
        tools: &CodexToolContext,
    ) -> Result<(), ProxyError> {
        let call_ids = content
            .iter()
            .filter_map(tool_id)
            .take(MAX_BLOCKS)
            .collect();
        let turn = complete
            .then(|| ToolTurn::from_content(content, Some(tools)))
            .flatten();
        let invalid = (turn.is_none() && content.iter().any(is_tool_use))
            || content.iter().any(invalid_thinking);
        self.record(call_ids, turn);
        if invalid {
            Err(invalid_tool_turn())
        } else {
            Ok(())
        }
    }
}

fn tool_id(block: &Value) -> Option<Fingerprint> {
    if !is_tool_use(block) {
        return None;
    }
    block
        .get("id")
        .and_then(nonempty_string)
        .map(|id| digest(id.as_bytes()))
}

fn is_tool_use(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("tool_use")
}

fn valid_message_start(message: &Value) -> bool {
    message.get("type").and_then(Value::as_str) == Some("message")
        && message.get("role").and_then(Value::as_str) == Some("assistant")
        && message.get("id").and_then(nonempty_string).is_some()
        && message
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(is_fable_5_2)
}

fn invalid_thinking(block: &Value) -> bool {
    match block.get("type").and_then(Value::as_str) {
        Some("thinking") => {
            block.get("thinking").and_then(Value::as_str).is_none()
                || block.get("signature").and_then(nonempty_string).is_none()
        }
        Some("redacted_thinking") => block.get("data").and_then(nonempty_string).is_none(),
        _ => false,
    }
}

pub(crate) fn validate_request(
    body: &Value,
    replay: Option<&FableReplay>,
) -> Result<(), ProxyError> {
    let invalid = || ProxyError::InvalidRequest(UNVERIFIED_REPLAY.to_string());
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    let Some(results) = messages
        .last()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    let mut result_ids = results
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|block| {
            block
                .get("tool_use_id")
                .and_then(nonempty_string)
                .map(|id| digest(id.as_bytes()))
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if result_ids.is_empty() {
        return Ok(());
    }
    result_ids.sort_unstable();
    let assistant = messages
        .iter()
        .rev()
        .nth(1)
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let turn = ToolTurn::from_content(assistant, None).ok_or_else(invalid)?;
    if turn.call_ids != result_ids
        || !replay.map_or(turn.signed, |scope| scope.admits(&turn))
        || (!turn.signed
            && body.pointer("/thinking/type").and_then(Value::as_str) != Some("adaptive"))
    {
        return Err(invalid());
    }
    Ok(())
}

struct ObservedBlock {
    value: Value,
    input_delta: Option<String>,
    closed: bool,
}

pub(crate) struct StreamObservation {
    replay: Option<FableReplay>,
    blocks: BTreeMap<u64, ObservedBlock>,
    seen_calls: Vec<Fingerprint>,
    saw_tool_use: bool,
    started: bool,
    tool_stop_reason: bool,
    ended: bool,
    damaged: bool,
    bytes: usize,
}

impl StreamObservation {
    pub(crate) fn new(replay: FableReplay) -> Self {
        Self {
            replay: Some(replay),
            blocks: BTreeMap::new(),
            seen_calls: Vec::new(),
            saw_tool_use: false,
            started: false,
            tool_stop_reason: false,
            ended: false,
            damaged: false,
            bytes: 0,
        }
    }

    pub(crate) fn mark_damaged(&mut self) {
        self.damaged = true;
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    pub(crate) fn observe_event(&mut self, event: Option<&str>, frame: &Value, wire_bytes: usize) {
        self.bytes = self.bytes.saturating_add(wire_bytes);
        let kind = frame.get("type").and_then(Value::as_str);
        if kind == Some("content_block_start") {
            let block = &frame["content_block"];
            self.saw_tool_use |= is_tool_use(block);
            if self.seen_calls.len() < MAX_BLOCKS {
                self.seen_calls.extend(tool_id(block));
            }
        }
        if self.bytes > MAX_TRACKED_BYTES
            || self.ended
            || event.is_some_and(|event| Some(event.trim()) != kind)
        {
            self.damaged = true;
            return;
        }
        self.process_event(kind, frame);
    }

    fn process_event(&mut self, kind: Option<&str>, data: &Value) {
        match kind {
            Some("ping") => {}
            Some("message_start") => {
                let message = &data["message"];
                if self.started
                    || !valid_message_start(message)
                    || !message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                {
                    self.damaged = true;
                }
                self.started = true;
            }
            Some("content_block_start") => {
                let value = &data["content_block"];
                let Some(index) = data.get("index").and_then(Value::as_u64) else {
                    self.damaged = true;
                    return;
                };
                if !self.started
                    || self.tool_stop_reason
                    || self.blocks.len() >= MAX_BLOCKS
                    || self.blocks.contains_key(&index)
                    || !value.is_object()
                {
                    self.damaged = true;
                    return;
                }
                self.blocks.insert(
                    index,
                    ObservedBlock {
                        value: value.clone(),
                        input_delta: None,
                        closed: false,
                    },
                );
            }
            Some("content_block_delta" | "content_block_stop") => {
                let Some(block) = data
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| self.blocks.get_mut(&index))
                    .filter(|block| !block.closed)
                else {
                    self.damaged = true;
                    return;
                };
                if self.tool_stop_reason {
                    self.damaged = true;
                }
                if kind == Some("content_block_stop") {
                    block.closed = true;
                    if let Some(input) = block.input_delta.take() {
                        match serde_json::from_str::<Value>(&input) {
                            Ok(input) if input.is_object() => block.value["input"] = input,
                            _ => self.damaged = true,
                        }
                    }
                    return;
                }
                let delta = &data["delta"];
                let block_kind = block.value.get("type").and_then(Value::as_str);
                let delta_kind = delta.get("type").and_then(Value::as_str);
                match (block_kind, delta_kind) {
                    (Some("tool_use"), Some("input_json_delta")) => {
                        if let Some(fragment) = delta.get("partial_json").and_then(Value::as_str) {
                            block
                                .input_delta
                                .get_or_insert_with(String::new)
                                .push_str(fragment);
                        } else {
                            self.damaged = true;
                        }
                    }
                    (Some("thinking"), Some("thinking_delta")) => {
                        if block
                            .value
                            .get("signature")
                            .and_then(nonempty_string)
                            .is_some()
                            || !append_fragment(&mut block.value, delta, "thinking")
                        {
                            self.damaged = true;
                        }
                    }
                    (Some("thinking"), Some("signature_delta")) => {
                        if let Some(signature) = delta
                            .get("signature")
                            .and_then(nonempty_string)
                            .filter(|_| {
                                block
                                    .value
                                    .get("signature")
                                    .and_then(nonempty_string)
                                    .is_none()
                            })
                        {
                            block.value["signature"] = Value::String(signature.to_owned());
                        } else {
                            self.damaged = true;
                        }
                    }
                    (Some("text"), Some("text_delta")) => {
                        self.damaged |= !append_fragment(&mut block.value, delta, "text");
                    }
                    _ => self.damaged = true,
                }
            }
            Some("message_delta") => {
                if !self.started || self.blocks.values().any(|block| !block.closed) {
                    self.damaged = true;
                }
                if let Some(reason) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    if self.tool_stop_reason {
                        self.damaged = true;
                    }
                    self.tool_stop_reason = reason == "tool_use";
                }
            }
            Some("message_stop") => self.ended = true,
            _ => self.damaged = true,
        }
    }

    pub(crate) fn finish(&mut self, tools: &CodexToolContext) -> Result<(), ProxyError> {
        let complete = !self.damaged
            && self.started
            && self.tool_stop_reason
            && self.ended
            && self
                .blocks
                .iter()
                .enumerate()
                .all(|(position, (index, block))| *index == position as u64 && block.closed);
        if let Some(replay) = self.replay.take() {
            if !complete && self.saw_tool_use {
                replay.record(std::mem::take(&mut self.seen_calls), None);
                return Err(invalid_tool_turn());
            }
            if self.blocks.values().any(|block| {
                invalid_thinking(&block.value)
                    || (!block.closed
                        && matches!(
                            block.value["type"].as_str(),
                            Some("thinking" | "redacted_thinking")
                        ))
            }) {
                replay.record(std::mem::take(&mut self.seen_calls), None);
                return Err(invalid_tool_turn());
            }
            let content = std::mem::take(&mut self.blocks)
                .into_values()
                .map(|block| block.value)
                .collect::<Vec<_>>();
            replay.record_content(&content, complete, tools)?;
        }
        Ok(())
    }
}

impl Drop for StreamObservation {
    fn drop(&mut self) {
        if let Some(replay) = self.replay.take() {
            replay.record(std::mem::take(&mut self.seen_calls), None);
        }
    }
}

fn append_fragment(block: &mut Value, delta: &Value, key: &str) -> bool {
    let (Some(fragment), Some(Value::String(value))) =
        (delta.get(key).and_then(Value::as_str), block.get_mut(key))
    else {
        return false;
    };
    value.push_str(fragment);
    true
}
