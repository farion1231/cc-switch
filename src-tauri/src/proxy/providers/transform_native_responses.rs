//! Compatibility fills for native Responses payloads.
//!
//! Native upstreams are normally passed through unchanged, but some compatible
//! gateways return nodes that omit fields strict clients deserialize as
//! required — grok CLI fails the whole turn with `serialization error: missing
//! field ...` (#8387). Keep the fills narrow: only add a field when it is
//! missing, never overwrite what the upstream sent.

use serde_json::{json, Value};

/// 补齐 grok CLI 反序列化必填、上游节点可能漏掉的字段：
/// - `type: "output_text"` 内容块缺 `annotations` → `[]`
/// - `type: "function_call"` 条目（任意 status）缺 `arguments` → `""`
/// - `type: "response.function_call_arguments.done"` 事件缺 `arguments` → `""`
///   （报告者的「工具调用轮次」报错落在条目还是事件上无法区分，两边都兜住）
///
/// 递归处理，事件里嵌的条目（`output_item.done` 的 `item`、`response.completed`
/// 的 `output`）一并覆盖。返回是否有改动。
pub(crate) fn ensure_grokbuild_required_fields(value: &mut Value) -> bool {
    let mut changed = false;
    match value {
        Value::Array(items) => {
            for item in items {
                changed |= ensure_grokbuild_required_fields(item);
            }
        }
        Value::Object(object) => {
            match object.get("type").and_then(Value::as_str) {
                Some("output_text") if !object.contains_key("annotations") => {
                    object.insert("annotations".to_string(), json!([]));
                    changed = true;
                }
                Some("function_call" | "response.function_call_arguments.done")
                    if !object.contains_key("arguments") =>
                {
                    object.insert("arguments".to_string(), Value::String(String::new()));
                    changed = true;
                }
                _ => {}
            }
            for child in object.values_mut() {
                changed |= ensure_grokbuild_required_fields(child);
            }
        }
        _ => {}
    }
    changed
}

/// SSE 事件的 `sequence_number` 计数器。官方流逐事件递增，grok CLI 按必填反序列化；
/// 上游带了序号的用上游的（计数器跟随，保证注入的值不回退、不撞车），缺的注入下一个值。
pub(crate) struct SseSequenceNumbers {
    next: u64,
}

impl SseSequenceNumbers {
    pub(crate) fn new() -> Self {
        Self { next: 0 }
    }

    pub(crate) fn ensure(&mut self, event: &mut Value) -> bool {
        let Value::Object(object) = event else {
            return false;
        };
        match object.get("sequence_number") {
            // 上游自己带了（且是数字）：原样保留，计数器走到它后面。
            Some(n) if n.is_u64() => {
                self.next = self
                    .next
                    .max(n.as_u64().unwrap_or_default().saturating_add(1));
                false
            }
            Some(_) => false,
            None => {
                object.insert("sequence_number".to_string(), json!(self.next));
                self.next += 1;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_missing_required_fields_in_a_response_body() {
        let mut body = json!({
            "type": "response.completed",
            "response": {
                "output": [
                    {"type": "function_call", "name": "shell", "status": "completed"},
                    {"type": "message", "content": [{"type": "output_text", "text": "hi"}]}
                ]
            }
        });

        assert!(ensure_grokbuild_required_fields(&mut body));
        assert_eq!(body["response"]["output"][0]["arguments"], json!(""));
        assert_eq!(
            body["response"]["output"][1]["content"][0]["annotations"],
            json!([])
        );

        let mut done_event =
            json!({"type": "response.function_call_arguments.done", "item_id": "fc_1"});
        assert!(ensure_grokbuild_required_fields(&mut done_event));
        assert_eq!(done_event["arguments"], json!(""));
    }

    #[test]
    fn keeps_upstream_values_and_is_idempotent() {
        let citations = json!([{"type": "url_citation", "url": "https://example.com"}]);
        let mut body = json!({
            "output": [
                {"type": "function_call", "arguments": "{\"x\":1}"},
                {"type": "output_text", "text": "cited", "annotations": citations},
                {"type": "response.output_text.delta", "delta": "x"}
            ]
        });

        assert!(!ensure_grokbuild_required_fields(&mut body));
        assert_eq!(body["output"][0]["arguments"], json!("{\"x\":1}"));
        assert_eq!(body["output"][1]["annotations"], citations);
    }

    #[test]
    fn injects_sequence_numbers_and_follows_upstream_ones() {
        let mut sequence = SseSequenceNumbers::new();
        let mut first = json!({"type": "response.created"});
        let mut numbered = json!({"type": "response.output_item.added", "sequence_number": 7});
        let mut after = json!({"type": "response.output_item.done"});

        assert!(sequence.ensure(&mut first));
        assert_eq!(first["sequence_number"], json!(0));
        assert!(!sequence.ensure(&mut numbered));
        assert_eq!(numbered["sequence_number"], json!(7));
        assert!(sequence.ensure(&mut after));
        assert_eq!(after["sequence_number"], json!(8));
    }
}
