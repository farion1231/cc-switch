//! 原生 Responses 透传（官方以外的上游）：补齐迟到的函数调用参数。
//!
//! MiniMax 的 `/v1/responses` 在长历史下（整段参数一次吐出时）会乱序：先发
//! `function_call_arguments.done` 和 `output_item.done`，`arguments` 都是空串，再发唯一一个
//! 带完整参数的 `function_call_arguments.delta`，最后 `response.completed` 里的参数是对的
//! （实测 2026-10-03）。Codex 在 `output_item.done` 就定下调用，拿到空串，工具全部报
//! `failed to parse function arguments`。
//!
//! 这里只动响应：参数为空的那两个结束事件先扣住，等这个条目之后第一个不是它的参数增量的事件
//! 到来时，用累积的增量（或 `response.completed` 里的同 id 条目）补上参数再发。顺序正常的流
//! 里结束事件本来就带参数，原样放行；真没有参数的调用，扣住的事件原样补发。请求一个字节不改。

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

/// 一个 SSE 块，解析出的事件（不是 JSON 的块为 None）。
struct Block {
    raw: String,
    event_name: Option<String>,
    event: Option<Value>,
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

    /// 把参数写进结束事件，重新序列化。
    fn with_arguments(mut self, arguments: &str) -> Bytes {
        let Some(event) = self.event.as_mut() else {
            return self.into_bytes();
        };
        let target =
            if event.get("type").and_then(Value::as_str) == Some("response.output_item.done") {
                event.get_mut("item")
            } else {
                Some(&mut *event)
            };
        let Some(target) = target.and_then(Value::as_object_mut) else {
            return self.into_bytes();
        };
        target.insert(
            "arguments".to_string(),
            Value::String(arguments.to_string()),
        );
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

    fn into_bytes(self) -> Bytes {
        Bytes::from(format!("{}\n\n", self.raw))
    }
}

#[derive(Default)]
struct Repair {
    /// 每个条目累积的参数增量。
    arguments: HashMap<String, String>,
    /// 扣住的结束事件，保持原来的顺序。
    held: Vec<(String, Block)>,
}

impl Repair {
    fn push(&mut self, raw: &str) -> Vec<Bytes> {
        let block = Block::parse(raw);
        let mut out = Vec::new();

        if let Some((item_id, delta)) = block.arguments_delta() {
            self.arguments
                .entry(item_id.clone())
                .or_default()
                .push_str(delta);
            // 扣住的就是这个条目：增量照发，结束事件继续等。
            if self.held.iter().all(|(held_id, _)| *held_id == item_id) {
                out.push(block.into_bytes());
                return out;
            }
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
        out.push(block.into_bytes());
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
                match arguments {
                    Some(arguments) => block.with_arguments(&arguments),
                    None => block.into_bytes(),
                }
            })
            .collect()
    }
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

/// 包一层原生 Responses SSE 流，补齐迟到的函数调用参数。
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
        json!({ "type": "function_call", "id": "x_fc_0", "call_id": "call_1",
                "name": "exec_command", "arguments": arguments })
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
}
