//! Copilot can reissue an opaque item ID between Responses lifecycle events.
//! Keep the first upstream-issued ID for each output index so Codex can merge
//! streamed text with its completed item. The original ID remains replayable.

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::{collections::HashMap, io};

#[derive(Default)]
struct ItemIds {
    by_output_index: HashMap<u64, String>,
}

impl ItemIds {
    fn stabilize_id(&mut self, output_index: u64, id: &mut Value) -> bool {
        let Some(current) = id.as_str().filter(|id| !id.is_empty()) else {
            return false;
        };
        let stable = self
            .by_output_index
            .entry(output_index)
            .or_insert_with(|| current.to_string());
        if current == stable {
            return false;
        }
        *id = Value::String(stable.clone());
        true
    }

    fn rewrite_event(&mut self, kind: &str, event: &mut Value) -> bool {
        if kind == "response.created" {
            self.by_output_index.clear();
        }

        let mut changed = false;
        if let Some(index) = event.get("output_index").and_then(Value::as_u64) {
            if matches!(
                kind,
                "response.output_item.added" | "response.output_item.done"
            ) {
                if let Some(id) = event.pointer_mut("/item/id") {
                    changed |= self.stabilize_id(index, id);
                }
            }
            if let Some(stable) = self.by_output_index.get(&index) {
                if let Some(Value::String(id)) = event.get_mut("item_id") {
                    if id != stable {
                        id.clone_from(stable);
                        changed = true;
                    }
                }
            }
        }

        if let Some(output) = event
            .pointer_mut("/response/output")
            .and_then(Value::as_array_mut)
        {
            for (index, item) in output.iter_mut().enumerate() {
                if let Some(id) = item.get_mut("id") {
                    changed |= self.stabilize_id(index as u64, id);
                }
            }
        }
        changed
    }

    fn rewrite_block(&mut self, raw: &str) -> Result<Bytes, io::Error> {
        let data = raw
            .lines()
            .filter_map(|line| strip_sse_field(line, "data"))
            .collect::<Vec<_>>()
            .join("\n");
        let unchanged = || Bytes::from(format!("{raw}\n\n"));
        if data.is_empty() || data.trim() == "[DONE]" {
            return Ok(unchanged());
        }
        let mut event: Value = match serde_json::from_str(&data) {
            Ok(event) => event,
            Err(error) => {
                log::warn!("[Copilot/Responses] Invalid SSE JSON: {error}");
                return Ok(unchanged());
            }
        };
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| raw.lines().find_map(|line| strip_sse_field(line, "event")))
            .unwrap_or_default()
            .to_string();
        if !self.rewrite_event(&kind, &mut event) {
            return Ok(unchanged());
        }

        let data = serde_json::to_string(&event).map_err(io::Error::other)?;
        let mut output = String::new();
        let mut wrote_data = false;
        for line in raw.lines() {
            if strip_sse_field(line, "data").is_some() {
                if !wrote_data {
                    output.push_str("data: ");
                    output.push_str(&data);
                    output.push('\n');
                    wrote_data = true;
                }
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }
        output.push('\n');
        Ok(Bytes::from(output))
    }
}

pub(crate) fn create_stable_item_id_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::try_stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut ids = ItemIds::default();
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|error| io::Error::other(error.to_string()))?;
            append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
            while let Some(block) = take_sse_block(&mut buffer) {
                if !block.trim().is_empty() {
                    yield ids.rewrite_block(&block)?;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        if !buffer.trim().is_empty() {
            yield ids.rewrite_block(&buffer)?;
        }
    }
}
