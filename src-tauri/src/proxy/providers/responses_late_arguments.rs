//! 原生 Responses 透传（官方以外的上游）：补齐函数调用的身份与迟到的参数。
//!
//! 上游会把流式 `function_call` 的字段吐坏，实测有两类：
//!
//! - MiniMax 的 `/v1/responses` 在长历史下（整段参数一次吐出时）会乱序：先发
//!   `function_call_arguments.done` 和 `output_item.done`，`arguments` 都是空串，再发唯一一个
//!   带完整参数的 `function_call_arguments.delta`，最后 `response.completed` 里的参数是对的
//!   （实测 2026-10-03）。Codex 在 `output_item.done` 就定下调用，拿到空串，工具全部报
//!   `failed to parse function arguments`。
//! - Xiaomi MiMo 的 `/v1/responses` 把 `name` 和 `call_id` 在 `output_item.done` 与
//!   `response.completed` 里吐成空串，只有先到的 `output_item.added` 是好的（#7671，参数完整，
//!   所以跟上一类无关）。Codex 拿到空 `name` 后报 `unsupported call: `，整轮任务静默失败。
//!   同一请求 `stream: false` 时三个字段全对，且失效按时间窗整段出现 ⇒ 是上游侧抖动，
//!   只能无条件防御，不能按厂商名开关。
//!
//! 这里只动响应：`output_item.added` 原样放行，顺带按条目记下 `name` / `call_id`；参数为空的
//! 那两个结束事件先扣住，等之后第一个不是参数增量的事件到来时，用累积的增量（或
//! `response.completed` 里的同 id 条目）补上参数再发。所有放行走同一个出口，出口顺手把身份
//! 回填到 `output_item.done`、`function_call_arguments.done` 的 `name`，以及
//! `response.completed` 的 `response.output[]`。三条铁律：已有非空值一律不覆盖；没有依据就不改，
//! 绝不凭空造值；真的改过字段才重新序列化，否则原样发出上游字节。顺序正常的流原样放行。
//! 请求一个字节不改。

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{Map, Value};

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

/// 一个 SSE 块，解析出的事件（不是 JSON 的块为 None）。
struct Block {
    raw: String,
    event_name: Option<String>,
    event: Option<Value>,
    /// `event` 被改写过才重新序列化，没改过就仍发 `raw`。
    patched: bool,
}

impl Block {
    fn parse(raw: &str) -> Self {
        let mut event_name = None;
        let mut data_parts = Vec::new();
        for line in raw.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                event_name = Some(event.trim().to_string());
            }
            if let Some(data) = strip_sse_field(line, "data") {
                data_parts.push(data);
            }
        }
        let event = (!data_parts.is_empty())
            .then(|| serde_json::from_str::<Value>(&data_parts.join("\n")).ok())
            .flatten();
        Self {
            raw: raw.to_string(),
            event_name,
            event,
            patched: false,
        }
    }

    fn event_type(&self) -> Option<&str> {
        self.event.as_ref()?.get("type")?.as_str()
    }

    /// 参数为空的函数调用结束事件，返回条目 id。
    fn empty_function_call_end(&self) -> Option<String> {
        let event = self.event.as_ref()?;
        let (item_id, arguments) = match self.event_type()? {
            "response.function_call_arguments.done" => {
                (event.get("item_id")?, event.get("arguments"))
            }
            "response.output_item.done" => {
                let item = event.get("item")?;
                if item.get("type")?.as_str()? != "function_call" {
                    return None;
                }
                (item.get("id")?, item.get("arguments"))
            }
            _ => return None,
        };
        let empty = arguments
            .and_then(Value::as_str)
            .is_none_or(|arguments| arguments.is_empty());
        empty
            .then(|| item_id.as_str().map(str::to_string))
            .flatten()
    }

    fn arguments_delta(&self) -> Option<(String, &str)> {
        if self.event_type()? != "response.function_call_arguments.delta" {
            return None;
        }
        let event = self.event.as_ref()?;
        Some((
            event.get("item_id")?.as_str()?.to_string(),
            event.get("delta")?.as_str()?,
        ))
    }

    /// `response.output_item.added` 里函数调用的身份，按条目 id 返回。
    /// `name` 和 `call_id` 都是空的条目没有可记的，返回 None。
    fn function_call_identity(&self) -> Option<(String, FunctionCallIdentity)> {
        if self.event_type()? != "response.output_item.added" {
            return None;
        }
        let item = self.event.as_ref()?.get("item")?;
        if item.get("type")?.as_str()? != "function_call" {
            return None;
        }
        let text = |key: &str| {
            item.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let identity = FunctionCallIdentity {
            name: text("name"),
            call_id: text("call_id"),
        };
        let item_id = item.get("id")?.as_str()?.to_string();
        (!identity.name.is_empty() || !identity.call_id.is_empty()).then_some((item_id, identity))
    }

    /// 把参数写进结束事件。
    fn set_arguments(&mut self, arguments: &str) {
        let Some(event) = self.event.as_mut() else {
            return;
        };
        let target =
            if event.get("type").and_then(Value::as_str) == Some("response.output_item.done") {
                event.get_mut("item")
            } else {
                Some(&mut *event)
            };
        let Some(target) = target.and_then(Value::as_object_mut) else {
            return;
        };
        target.insert(
            "arguments".to_string(),
            Value::String(arguments.to_string()),
        );
        self.patched = true;
    }

    fn into_bytes(self) -> Bytes {
        let Some(event) = self.event.as_ref().filter(|_| self.patched) else {
            return Bytes::from(format!("{}\n\n", self.raw));
        };
        let mut out = String::new();
        if let Some(name) = &self.event_name {
            out.push_str("event: ");
            out.push_str(name);
            out.push('\n');
        }
        out.push_str("data: ");
        out.push_str(&serde_json::to_string(event).unwrap_or_default());
        out.push_str("\n\n");
        Bytes::from(out)
    }
}

/// 一个函数调用条目的身份，从 `output_item.added` 记下，用来回填结束事件里丢掉的同名字段。
/// 只覆盖 `function_call`：`custom_tool_call` / `tool_search_call` 按规范同样带 `call_id`，
/// 但 #7671 的证据只到 `function_call`，等实测到再一起纳进来。
#[derive(Clone, Default)]
struct FunctionCallIdentity {
    name: String,
    call_id: String,
}

#[derive(Default)]
struct Repair {
    /// 每个条目累积的参数增量。
    arguments: HashMap<String, String>,
    /// 每个条目在 `output_item.added` 里报出的身份。
    identity: HashMap<String, FunctionCallIdentity>,
    /// 扣住的结束事件，保持原来的顺序。
    held: Vec<(String, Block)>,
}

impl Repair {
    fn push(&mut self, raw: &str) -> Vec<Bytes> {
        let block = Block::parse(raw);
        let mut out = Vec::new();

        // 身份要在分流之前记：`added` 一定排在带它的结束事件前面，记下就照原样放行。
        if let Some((item_id, identity)) = block.function_call_identity() {
            self.identity.entry(item_id).or_insert(identity);
        }

        if let Some((item_id, delta)) = block.arguments_delta() {
            self.arguments
                .entry(item_id.clone())
                .or_default()
                .push_str(delta);
            // 增量一律照发，不触发放行：并行调用时几个条目的结束事件可能都先到，各自的增量
            // （乃至别的调用的增量）随后交错着来，见到任何增量就放行会让还没等到参数的条目空着发出。
            out.push(block.into_bytes());
            return out;
        }

        if let Some(item_id) = block.empty_function_call_end() {
            self.held.push((item_id, block));
            return out;
        }

        let completed = block
            .event
            .as_ref()
            .filter(|_| block.event_type() == Some("response.completed"));
        out.extend(self.flush(completed));
        out.push(self.emit(block, None));
        out
    }

    fn flush(&mut self, completed: Option<&Value>) -> Vec<Bytes> {
        std::mem::take(&mut self.held)
            .into_iter()
            .map(|(item_id, block)| {
                let arguments = self
                    .arguments
                    .get(&item_id)
                    .filter(|arguments| !arguments.is_empty())
                    .cloned()
                    .or_else(|| completed_arguments(completed?, &item_id));
                self.emit(block, arguments.as_deref())
            })
            .collect()
    }

    /// 唯一的放行出口：按需补参数、按需补身份，一个字段都没改就仍发上游原始字节。
    /// 参数完整的结束事件不会进 `held`，改写只能收在这一个出口上，否则 #7671 那类缺陷修不到。
    fn emit(&self, mut block: Block, arguments: Option<&str>) -> Bytes {
        if let Some(arguments) = arguments {
            block.set_arguments(arguments);
        }
        self.fill_identities(&mut block);
        block.into_bytes()
    }

    /// 把记下的身份补进事件里空缺的 `name` / `call_id`。
    fn fill_identities(&self, block: &mut Block) {
        if self.identity.is_empty() {
            return;
        }
        let arguments_done = block.event_type() == Some("response.function_call_arguments.done");
        let Some(event) = block.event.as_mut() else {
            return;
        };
        let mut changed = fill_function_call_identities(event, &self.identity);
        if arguments_done {
            changed |= fill_arguments_done_name(event, &self.identity);
        }
        block.patched |= changed;
    }
}

/// 递归找出 `type == "function_call"` 的条目（`output_item.done` 的 `item`、
/// `response.completed` 的 `response.output[]`），按 id 补上丢掉的身份。
fn fill_function_call_identities(
    value: &mut Value,
    identity: &HashMap<String, FunctionCallIdentity>,
) -> bool {
    match value {
        Value::Array(items) => {
            // 不能用 Iterator::any：短路会让后面的条目根本走不到。
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= fill_function_call_identities(item, identity);
            }
            changed
        }
        Value::Object(obj) if obj.get("type").and_then(Value::as_str) == Some("function_call") => {
            fill_one_identity(obj, identity)
        }
        Value::Object(obj) => {
            let mut changed = false;
            for child in obj.values_mut() {
                changed |= fill_function_call_identities(child, identity);
            }
            changed
        }
        _ => false,
    }
}

/// 补一个 `function_call` 条目，按它的 `id` 找身份。
fn fill_one_identity(
    obj: &mut Map<String, Value>,
    identity: &HashMap<String, FunctionCallIdentity>,
) -> bool {
    let Some(record) = obj
        .get("id")
        .and_then(Value::as_str)
        .and_then(|item_id| identity.get(item_id))
    else {
        return false;
    };
    let mut changed = false;
    changed |= fill_text_field(obj, "name", &record.name);
    changed |= fill_text_field(obj, "call_id", &record.call_id);
    changed
}

/// `response.function_call_arguments.done` 的条目 id 在顶层 `item_id`，带 `name` 不带 `call_id`，
/// 所以只补名，不往这个事件里塞 `call_id`。
fn fill_arguments_done_name(
    event: &mut Value,
    identity: &HashMap<String, FunctionCallIdentity>,
) -> bool {
    let Some(name) = event
        .get("item_id")
        .and_then(Value::as_str)
        .and_then(|item_id| identity.get(item_id))
        .map(|record| record.name.clone())
    else {
        return false;
    };
    let Some(obj) = event.as_object_mut() else {
        return false;
    };
    fill_text_field(obj, "name", &name)
}

/// 只补空缺：既有非空值一律不动（可能是上游有意改名，猜错比不猜好），依据本身为空也不动，
/// 绝不凭空造一个值出来。
fn fill_text_field(obj: &mut Map<String, Value>, key: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let occupied = obj
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|current| !current.is_empty());
    if occupied {
        return false;
    }
    obj.insert(key.to_string(), Value::String(value.to_string()));
    true
}

/// `response.completed` 里同 id 函数调用的参数。
fn completed_arguments(completed: &Value, item_id: &str) -> Option<String> {
    completed
        .pointer("/response/output")?
        .as_array()?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(item_id))?
        .get("arguments")?
        .as_str()
        .filter(|arguments| !arguments.is_empty())
        .map(str::to_string)
}

/// 包一层原生 Responses SSE 流，补齐函数调用的身份与迟到的参数。
pub(crate) fn create_late_arguments_repair_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut repair = Repair::default();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        for out in repair.push(&block) {
                            yield Ok(out);
                        }
                    }
                }
                Err(e) => {
                    for out in repair.flush(None) {
                        yield Ok(out);
                    }
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
            for out in repair.push(&tail) {
                yield Ok(out);
            }
        }
        for out in repair.flush(None) {
            yield Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sse(event: Value) -> String {
        format!(
            "event: {}\ndata: {}",
            event["type"].as_str().unwrap(),
            event
        )
    }

    fn run(blocks: Vec<Value>) -> Vec<Value> {
        let mut repair = Repair::default();
        let mut out: Vec<Bytes> = blocks
            .into_iter()
            .flat_map(|event| repair.push(&sse(event)))
            .collect();
        out.extend(repair.flush(None));
        out.iter()
            .map(|bytes| {
                let text = std::str::from_utf8(bytes).unwrap();
                let data = text
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .unwrap();
                serde_json::from_str(data).unwrap()
            })
            .collect()
    }

    fn types(events: &[Value]) -> Vec<&str> {
        events.iter().map(|e| e["type"].as_str().unwrap()).collect()
    }

    fn call_item(arguments: &str) -> Value {
        call_item_with(arguments, "exec_command", "call_1")
    }

    /// 身份（`name` / `call_id`）可指定的函数调用条目，用来表达上游吐空串的情形。
    fn call_item_with(arguments: &str, name: &str, call_id: &str) -> Value {
        json!({ "type": "function_call", "id": "x_fc_0", "call_id": call_id,
                "name": name, "arguments": arguments })
    }

    /// 实测的 MiniMax 乱序：结束事件先到、参数为空，唯一的增量排在后面。
    #[test]
    fn fills_arguments_that_arrive_after_the_end_events() {
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 3, "item": call_item("") }),
            json!({ "type": "response.function_call_arguments.done", "output_index": 3,
                    "item_id": "x_fc_0", "name": "exec_command", "arguments": "" }),
            json!({ "type": "response.output_item.done", "output_index": 3, "item": call_item("") }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 3,
                    "item_id": "x_fc_0", "delta": "{\"cmd\":\"ls\"}" }),
            json!({ "type": "response.completed", "response": { "output": [call_item("{\"cmd\":\"ls\"}")] } }),
        ]);
        assert_eq!(
            types(&events),
            vec![
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        assert_eq!(events[2]["arguments"], "{\"cmd\":\"ls\"}");
        assert_eq!(events[3]["item"]["arguments"], "{\"cmd\":\"ls\"}");
    }

    /// 并行调用：两个条目的结束事件都先到，各自的增量随后才来，两个都要补上。
    #[test]
    fn fills_parallel_calls_whose_arguments_arrive_after_all_end_events() {
        let item = |id: &str, arguments: &str| {
            json!({ "type": "function_call", "id": id, "call_id": format!("call_{id}"),
                    "name": "exec_command", "arguments": arguments })
        };
        let events = run(vec![
            json!({ "type": "response.output_item.done", "output_index": 0, "item": item("a", "") }),
            json!({ "type": "response.output_item.done", "output_index": 1, "item": item("b", "") }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 0,
                    "item_id": "a", "delta": "{\"cmd\":\"ls\"}" }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 1,
                    "item_id": "b", "delta": "{\"cmd\":\"pwd\"}" }),
            json!({ "type": "response.completed", "response": { "output": [] } }),
        ]);
        assert_eq!(
            types(&events),
            vec![
                "response.function_call_arguments.delta",
                "response.function_call_arguments.delta",
                "response.output_item.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        assert_eq!(events[2]["item"]["arguments"], "{\"cmd\":\"ls\"}");
        assert_eq!(events[3]["item"]["arguments"], "{\"cmd\":\"pwd\"}");
    }

    /// 别的调用的增量插在中间（A → C → B）：扣住的 A、B 不能被 C 的增量提前放走。
    #[test]
    fn unrelated_deltas_do_not_release_held_calls() {
        let item = |id: &str| {
            json!({ "type": "function_call", "id": id, "call_id": format!("call_{id}"),
                    "name": "exec_command", "arguments": "" })
        };
        let delta = |id: &str, delta: &str| json!({ "type": "response.function_call_arguments.delta", "item_id": id, "delta": delta });
        let events = run(vec![
            json!({ "type": "response.output_item.done", "output_index": 0, "item": item("a") }),
            json!({ "type": "response.output_item.done", "output_index": 1, "item": item("b") }),
            delta("a", "{\"cmd\":\"ls\"}"),
            delta("c", "{\"cmd\":"),
            delta("b", "{\"cmd\":\"pwd\"}"),
            json!({ "type": "response.completed", "response": { "output": [] } }),
        ]);
        let done: Vec<(&str, &str)> = events
            .iter()
            .filter(|e| e["type"] == "response.output_item.done")
            .map(|e| {
                (
                    e["item"]["id"].as_str().unwrap(),
                    e["item"]["arguments"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            done,
            vec![("a", "{\"cmd\":\"ls\"}"), ("b", "{\"cmd\":\"pwd\"}")]
        );
    }

    /// 没有增量时，从 `response.completed` 里同 id 的条目取参数。
    #[test]
    fn falls_back_to_the_completed_output() {
        let events = run(vec![
            json!({ "type": "response.output_item.done", "output_index": 0, "item": call_item("") }),
            json!({ "type": "response.completed", "response": { "output": [call_item("{\"a\":1}")] } }),
        ]);
        assert_eq!(events[0]["item"]["arguments"], "{\"a\":1}");
        assert_eq!(events[1]["type"], "response.completed");
    }

    /// 顺序正常的流原样放行，字节不变。
    #[test]
    fn leaves_well_ordered_streams_untouched() {
        let blocks = vec![
            json!({ "type": "response.function_call_arguments.delta", "item_id": "x_fc_0", "delta": "{\"cmd\":" }),
            json!({ "type": "response.function_call_arguments.delta", "item_id": "x_fc_0", "delta": "\"ls\"}" }),
            json!({ "type": "response.function_call_arguments.done", "item_id": "x_fc_0", "arguments": "{\"cmd\":\"ls\"}" }),
            json!({ "type": "response.output_item.done", "item": call_item("{\"cmd\":\"ls\"}") }),
            json!({ "type": "response.completed", "response": { "output": [] } }),
        ];
        let mut repair = Repair::default();
        for event in blocks {
            let raw = sse(event);
            let out = repair.push(&raw);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0], Bytes::from(format!("{raw}\n\n")));
        }
    }

    /// 真没有参数的调用：扣住的事件在下一个事件前原样补发，不丢。
    #[test]
    fn releases_held_events_without_arguments_unchanged() {
        let events = run(vec![
            json!({ "type": "response.output_item.done", "output_index": 0, "item": call_item("") }),
            json!({ "type": "response.output_item.added", "output_index": 1,
                    "item": { "type": "message", "id": "m1" } }),
        ]);
        assert_eq!(
            types(&events),
            vec!["response.output_item.done", "response.output_item.added"]
        );
        assert_eq!(events[0]["item"]["arguments"], "");

        // 流在扣住时结束，也照样补发。
        let events = run(vec![
            json!({ "type": "response.output_item.done", "item": call_item("") }),
        ]);
        assert_eq!(events.len(), 1);
    }

    /// #7671 实测（MiMo `mimo-v2.5-pro`）：`output_item.added` 里 `name` / `call_id` 是好的，
    /// 到 `output_item.done` 与 `response.completed` 就被吐成空串，参数却是完整的。
    #[test]
    fn carries_name_and_call_id_into_output_item_done() {
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": call_item_with("", "shell", "call_a08838cd5e844ac7a2b5a8e7") }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 0,
                    "item_id": "x_fc_0", "delta": "{\"command\":\"ls\"}" }),
            json!({ "type": "response.function_call_arguments.done", "output_index": 0,
                    "item_id": "x_fc_0", "name": "", "arguments": "{\"command\":\"ls\"}" }),
            json!({ "type": "response.output_item.done", "output_index": 0,
                    "item": call_item_with("{\"command\":\"ls\"}", "", "") }),
            json!({ "type": "response.completed", "output_index": 0,
                    "response": { "output": [call_item_with("{\"command\":\"ls\"}", "", "")] } }),
        ]);
        assert_eq!(
            types(&events),
            vec![
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        assert_eq!(events[2]["name"], "shell");
        assert_eq!(events[3]["item"]["name"], "shell");
        assert_eq!(
            events[3]["item"]["call_id"],
            "call_a08838cd5e844ac7a2b5a8e7"
        );
        assert_eq!(events[4]["response"]["output"][0]["name"], "shell");
        assert_eq!(
            events[4]["response"]["output"][0]["call_id"],
            "call_a08838cd5e844ac7a2b5a8e7"
        );
        // 补齐身份不能把已有的参数弄坏。
        assert_eq!(events[3]["item"]["arguments"], "{\"command\":\"ls\"}");
    }

    /// 身份要补进 `response.completed` 的 `response.output[]`，那是 Codex 收单时最后读的一份。
    #[test]
    fn carries_identity_into_response_completed_output() {
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": call_item_with("{\"a\":1}", "shell", "call_9") }),
            json!({ "type": "response.completed",
                    "response": { "output": [call_item_with("{\"a\":1}", "", "")] } }),
        ]);
        assert_eq!(
            types(&events),
            vec!["response.output_item.added", "response.completed"]
        );
        assert_eq!(events[1]["response"]["output"][0]["name"], "shell");
        assert_eq!(events[1]["response"]["output"][0]["call_id"], "call_9");
    }

    /// C1：上游给了非空的 `name` / `call_id` 就一个字都不碰，哪怕与 `added` 里的不一致。
    #[test]
    fn never_overwrites_a_non_empty_name_from_upstream() {
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": call_item_with("", "shell", "call_9") }),
            json!({ "type": "response.output_item.done", "output_index": 0,
                    "item": call_item_with("{\"a\":1}", "exec_command", "call_other") }),
        ]);
        assert_eq!(events[1]["item"]["name"], "exec_command");
        assert_eq!(events[1]["item"]["call_id"], "call_other");
    }

    /// C2/C3：没有可依据的身份就整块原样放行，字节都不许变。
    #[test]
    fn leaves_events_alone_without_a_matching_identity() {
        // 没有 added，身份无从记起。
        let mut repair = Repair::default();
        for event in [
            json!({ "type": "response.output_item.done", "item": call_item_with("{\"a\":1}", "", "") }),
            json!({ "type": "response.completed",
                    "response": { "output": [call_item_with("{\"a\":1}", "", "")] } }),
        ] {
            let raw = sse(event);
            let out = repair.push(&raw);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0], Bytes::from(format!("{raw}\n\n")));
        }

        // added 来了，但它自己报的身份也是空的 ⇒ 不记、也就不补。
        let mut repair = Repair::default();
        for event in [
            json!({ "type": "response.output_item.added", "item": call_item_with("", "", "") }),
            json!({ "type": "response.output_item.done", "item": call_item_with("{\"a\":1}", "", "") }),
        ] {
            let raw = sse(event);
            let out = repair.push(&raw);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0], Bytes::from(format!("{raw}\n\n")));
        }
    }

    /// MiniMax 乱序 + 身份也丢（参数与身份同缺）：扣住的那个出口两个都要补上。
    /// 顺带把「结束事件用 `item.id`、参数事件用 `item_id`，两者同值」这一既有假设固化成断言。
    #[test]
    fn fills_identity_on_the_held_event_too() {
        let item = |arguments: &str, name: &str, call_id: &str| {
            json!({ "type": "function_call", "id": "x_fc_0", "call_id": call_id,
                    "name": name, "arguments": arguments })
        };
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": item("", "shell", "call_7") }),
            json!({ "type": "response.function_call_arguments.done", "output_index": 0,
                    "item_id": "x_fc_0", "name": "", "arguments": "" }),
            json!({ "type": "response.output_item.done", "output_index": 0,
                    "item": item("", "", "") }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 0,
                    "item_id": "x_fc_0", "delta": "{\"cmd\":\"ls\"}" }),
            json!({ "type": "response.completed",
                    "response": { "output": [item("{\"cmd\":\"ls\"}", "", "")] } }),
        ]);
        assert_eq!(
            types(&events),
            vec![
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        assert_eq!(events[2]["name"], "shell");
        assert_eq!(events[2]["arguments"], "{\"cmd\":\"ls\"}");
        assert_eq!(events[3]["item"]["name"], "shell");
        assert_eq!(events[3]["item"]["call_id"], "call_7");
        assert_eq!(events[3]["item"]["arguments"], "{\"cmd\":\"ls\"}");
        assert_eq!(events[4]["response"]["output"][0]["call_id"], "call_7");
    }

    /// `response.function_call_arguments.done` 也带 `name`，按 `item_id` 补；
    /// 但它没有 `call_id` 这个字段，不能凭空塞一个进去。
    #[test]
    fn patches_function_call_arguments_done_name() {
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": call_item("{\"cmd\":\"ls\"}") }),
            json!({ "type": "response.function_call_arguments.done", "item_id": "x_fc_0",
                    "name": "", "arguments": "{\"cmd\":\"ls\"}" }),
        ]);
        assert_eq!(events[1]["name"], "exec_command");
        assert!(events[1].get("call_id").is_none());
    }

    /// 只认 `function_call`：message 之类的条目既不记身份，也不被改写。id 故意与函数调用
    /// 用例同名，防止实现按 id 而不是按 type 认条目。
    #[test]
    fn ignores_non_function_call_output_items() {
        let blocks = vec![
            json!({ "type": "response.output_item.added",
                    "item": { "type": "message", "id": "x_fc_0" } }),
            json!({ "type": "response.output_item.done",
                    "item": { "type": "message", "id": "x_fc_0", "name": "", "call_id": "" } }),
            json!({ "type": "response.completed", "response": { "output": [
                { "type": "message", "id": "x_fc_0", "name": "", "call_id": "" } ] } }),
        ];
        let mut repair = Repair::default();
        for event in blocks {
            let raw = sse(event);
            let out = repair.push(&raw);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0], Bytes::from(format!("{raw}\n\n")));
        }
    }

    /// 并行调用交错：身份按 id 分桶，谁都不串到谁身上。
    #[test]
    fn keeps_identity_buckets_apart_across_parallel_calls() {
        let item = |id: &str, name: &str, call_id: &str| {
            json!({ "type": "function_call", "id": id, "call_id": call_id,
                    "name": name, "arguments": "{\"cmd\":\"ls\"}" })
        };
        let events = run(vec![
            json!({ "type": "response.output_item.added", "output_index": 0,
                    "item": item("a", "shell", "call_a") }),
            json!({ "type": "response.output_item.added", "output_index": 1,
                    "item": item("b", "read_file", "call_b") }),
            json!({ "type": "response.output_item.done", "output_index": 1,
                    "item": item("b", "", "") }),
            json!({ "type": "response.output_item.done", "output_index": 0,
                    "item": item("a", "", "") }),
        ]);
        assert_eq!(
            types(&events),
            vec![
                "response.output_item.added",
                "response.output_item.added",
                "response.output_item.done",
                "response.output_item.done",
            ]
        );
        assert_eq!(events[2]["item"]["id"], "b");
        assert_eq!(events[2]["item"]["name"], "read_file");
        assert_eq!(events[2]["item"]["call_id"], "call_b");
        assert_eq!(events[3]["item"]["id"], "a");
        assert_eq!(events[3]["item"]["name"], "shell");
        assert_eq!(events[3]["item"]["call_id"], "call_a");
    }
}
