//! Kimi For Coding / Moonshot 原生 Responses：把 Kimi 自己产出、却又拒收的历史排列理顺。
//!
//! Kimi 有时在一次回复里先给出工具调用，再给出一条（多半是空文字的）助手消息。
//! Codex 按收到的顺序记进历史，下一轮原样带回去，这条消息就夹在工具调用和它的结果
//! 之间。Kimi 在内部转成 Chat 消息后校验，报「an assistant message with 'tool_calls'
//! must be followed by tool messages」，整个线程从此每轮都失败；空文字的助手消息放在
//! 别处也会被拒（「text content is empty」）。
//!
//! 只改这两种排列：夹在调用与结果之间的助手消息，有文字的挪到这组调用前面（先说话
//! 再调工具），空文字的去掉；不在调用组里的空文字助手消息也去掉。改写是确定的，同一
//! 段历史每轮得到同样的字节，缓存前缀不受影响；请求里没有这些排列时逐字节不变。

use serde_json::Value;
use std::collections::HashSet;

const CALL_TYPES: &[&str] = &["function_call", "custom_tool_call"];
const OUTPUT_TYPES: &[&str] = &["function_call_output", "custom_tool_call_output"];

/// 上游是不是 Kimi / Moonshot（与 Chat 转换的 `$ref` 改写共用同一份域名名单）。
pub fn upstream_is_kimi(base_url: &str) -> bool {
    super::transform_codex_chat_moonshot_schema::upstream_requires_ref_sibling_all_of(base_url)
}

/// 理顺 `input` 里 Kimi 拒收的助手消息排列。返回是否改动过请求体。
pub fn repair_tool_history(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    if !input.iter().any(is_assistant_message) {
        return false;
    }

    let mut changed = false;
    let mut output: Vec<Value> = Vec::with_capacity(input.len());
    let mut pending: HashSet<String> = HashSet::new();
    // 当前这组调用里，挪上来的消息该插的位置（组里第一个调用之前，按原顺序往后排）。
    let mut hoist_at = 0;

    for item in std::mem::take(input) {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if is_assistant_message(&item) {
            if is_empty_text_message(&item) {
                changed = true;
                continue;
            }
            if !pending.is_empty() {
                output.insert(hoist_at, item);
                hoist_at += 1;
                changed = true;
                continue;
            }
        } else if CALL_TYPES.contains(&item_type) {
            if pending.is_empty() {
                hoist_at = output.len();
            }
            if let Some(call_id) = call_id(&item) {
                pending.insert(call_id.to_string());
            }
        } else if OUTPUT_TYPES.contains(&item_type) {
            if let Some(call_id) = call_id(&item) {
                pending.remove(call_id);
            }
        }
        output.push(item);
    }

    *input = output;
    changed
}

fn is_assistant_message(item: &Value) -> bool {
    item.get("role").and_then(Value::as_str) == Some("assistant")
        && item
            .get("type")
            .and_then(Value::as_str)
            .is_none_or(|item_type| item_type == "message")
}

/// 内容非空、且每一段都是空文字的助手消息。内容是空数组时 Kimi 照收，不动。
fn is_empty_text_message(item: &Value) -> bool {
    match item.get("content") {
        Some(Value::String(text)) => text.is_empty(),
        Some(Value::Array(parts)) => {
            !parts.is_empty()
                && parts.iter().all(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("output_text" | "input_text" | "text")
                    ) && part
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(str::is_empty)
                })
        }
        _ => false,
    }
}

fn call_id(item: &Value) -> Option<&str> {
    item.get("call_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user() -> Value {
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"run"}]})
    }
    fn reasoning() -> Value {
        json!({"type":"reasoning","summary":[{"type":"summary_text","text":"Need run."}]})
    }
    fn say(text: &str) -> Value {
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]})
    }
    fn call(id: &str) -> Value {
        json!({"type":"function_call","name":"exec_command","arguments":"{}","call_id":id})
    }
    fn result(id: &str) -> Value {
        json!({"type":"function_call_output","call_id":id,"output":"a\n"})
    }
    fn repaired(input: Vec<Value>) -> (bool, Vec<Value>) {
        let mut body = json!({ "model": "k3", "input": input });
        let changed = repair_tool_history(&mut body);
        (changed, body["input"].as_array().unwrap().clone())
    }

    #[test]
    fn empty_message_between_call_and_result_is_dropped() {
        let (changed, input) = repaired(vec![
            user(),
            reasoning(),
            call("exec_command:0"),
            say(""),
            result("exec_command:0"),
        ]);
        assert!(changed);
        assert_eq!(
            input,
            vec![
                user(),
                reasoning(),
                call("exec_command:0"),
                result("exec_command:0")
            ]
        );
    }

    #[test]
    fn spoken_message_inside_a_call_group_moves_before_the_calls_in_order() {
        let (changed, input) = repaired(vec![
            user(),
            reasoning(),
            call("a"),
            say("first"),
            call("b"),
            say("second"),
            result("a"),
            result("b"),
            say("done"),
        ]);
        assert!(changed);
        assert_eq!(
            input,
            vec![
                user(),
                reasoning(),
                say("first"),
                say("second"),
                call("a"),
                call("b"),
                result("a"),
                result("b"),
                say("done"),
            ]
        );
    }

    #[test]
    fn empty_message_outside_a_call_group_is_dropped_but_empty_arrays_are_kept() {
        let empty_array = json!({"type":"message","role":"assistant","content":[]});
        let (changed, input) = repaired(vec![
            user(),
            say(""),
            empty_array.clone(),
            call("a"),
            result("a"),
        ]);
        assert!(changed);
        assert_eq!(input, vec![user(), empty_array, call("a"), result("a")]);
    }

    #[test]
    fn well_formed_history_is_left_byte_identical() {
        let input = vec![
            user(),
            reasoning(),
            say("let me check"),
            call("a"),
            result("a"),
            say("a"),
        ];
        let mut body = json!({ "model": "k3", "input": input });
        let before = serde_json::to_vec(&body).unwrap();
        assert!(!repair_tool_history(&mut body));
        assert_eq!(serde_json::to_vec(&body).unwrap(), before);
    }

    #[test]
    fn only_kimi_and_moonshot_hosts_are_gated_in() {
        assert!(upstream_is_kimi("https://api.kimi.com/coding/v1"));
        assert!(upstream_is_kimi("https://api.moonshot.cn/v1"));
        assert!(!upstream_is_kimi("https://api.deepseek.com/v1"));
        assert!(!upstream_is_kimi("https://api.kimi.com.evil.net/v1"));
    }
}
