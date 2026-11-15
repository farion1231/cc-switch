//! 合并重放历史里重复 JSON 键的工具参数。
//!
//! Codex 每轮把整段会话历史原样回放给上游，`function_call.arguments` 是模型当时产出的
//! **原始字符串**，Codex 自己不做规范化。轻量模型偶尔会把同一个键写两遍（实测
//! `_web_run` 的 `time` 出现两次，两次都是 `null`）：
//!
//! - Codex 和 `serde_json` 都按「同名键后值覆盖」解析，工具照常执行，端侧毫无异常；
//! - 火山方舟这类严格校验的 Responses 上游会解析 `arguments` 并拒绝重复键，整条请求
//!   400（`The parameter input[189].arguments ... duplicate field "time"`）。
//!
//! 这条历史每一轮都会重放，所以一次坏输出就让整条线程永久不可用。转发前只改写真正含
//! 重复键的条目，按「后值覆盖」合并——与 Codex 的解析语义一致——其余条目一个字节不改。

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

use crate::proxy::json_canonical::canonical_json_string;

/// 修复 `input` 中重复键的 `function_call.arguments`，返回被改写的条目数。
///
/// 只处理 `type == "function_call"` 且 `arguments` 为字符串的条目；参数不是合法 JSON
/// 时原样放行（那是另一类问题，替换成 `{}` 会丢掉真实调用参数）。
pub(crate) fn dedupe_duplicate_argument_keys(body: &mut Value) -> usize {
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };

    let mut repaired = 0;
    for (index, item) in items.iter_mut().enumerate() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        let Some((arguments, duplicate_keys)) = object
            .get("arguments")
            .and_then(Value::as_str)
            .and_then(dedupe_arguments)
        else {
            continue;
        };

        // 只记结构信息：参数内容可能包含用户代码或查询词。
        log::warn!(
            "[Codex] input[{index}] function_call '{}' repeated {duplicate_keys} JSON key(s) in its arguments; merged them with last-value-wins before forwarding",
            object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        object.insert("arguments".to_string(), Value::String(arguments));
        repaired += 1;
    }
    repaired
}

/// `arguments` 含重复键时返回合并后的字符串与被合并的键数；否则返回 `None`。
fn dedupe_arguments(arguments: &str) -> Option<(String, usize)> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return None;
    }
    let deduped = serde_json::from_str::<Deduped>(trimmed).ok()?;
    if deduped.duplicate_keys == 0 {
        return None;
    }
    Some((
        canonical_json_string(&deduped.value),
        deduped.duplicate_keys,
    ))
}

/// 一边反序列化一边数重复键：`serde_json::Value` 会静默丢弃重复键，数不出来。
struct Deduped {
    value: Value,
    duplicate_keys: usize,
}

impl<'de> Deserialize<'de> for Deduped {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DedupedVisitor)
    }
}

struct DedupedVisitor;

impl<'de> Visitor<'de> for DedupedVisitor {
    type Value = Deduped;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::Bool(value),
            duplicate_keys: 0,
        })
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::Number(Number::from(value)),
            duplicate_keys: 0,
        })
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::Number(Number::from(value)),
            duplicate_keys: 0,
        })
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Deduped {
            value: Number::from_f64(value).map_or(Value::Null, Value::Number),
            duplicate_keys: 0,
        })
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::String(value.to_string()),
            duplicate_keys: 0,
        })
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::String(value),
            duplicate_keys: 0,
        })
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::Null,
            duplicate_keys: 0,
        })
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Deduped {
            value: Value::Null,
            duplicate_keys: 0,
        })
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deduped::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        let mut duplicate_keys = 0;
        while let Some(item) = seq.next_element::<Deduped>()? {
            duplicate_keys += item.duplicate_keys;
            values.push(item.value);
        }
        Ok(Deduped {
            value: Value::Array(values),
            duplicate_keys,
        })
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        let mut duplicate_keys = 0;
        while let Some((key, item)) = map.next_entry::<String, Deduped>()? {
            duplicate_keys += item.duplicate_keys;
            // `preserve_order` 下重复键保留首次出现的位置、换成后到的值，
            // 与 Codex / serde_json 的解析结果一致。
            if object.insert(key, item.value).is_some() {
                duplicate_keys += 1;
            }
        }
        Ok(Deduped {
            value: Value::Object(object),
            duplicate_keys,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(arguments: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": "call_1",
            "name": "_web_run",
            "arguments": arguments
        })
    }

    #[test]
    fn merges_repeated_keys_with_last_value_wins() {
        let mut body = json!({
            "input": [
                call(r#"{"time":null,"open":null,"time":"later","response_length":"6000"}"#),
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        });

        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 1);
        assert_eq!(
            body["input"][0]["arguments"],
            r#"{"open":null,"response_length":"6000","time":"later"}"#
        );
        assert_eq!(body["input"][1]["output"], "ok");
    }

    #[test]
    fn merges_repeated_keys_nested_in_objects_and_arrays() {
        let mut body = json!({
            "input": [call(r#"{"items":[{"q":"a","q":"b"}],"meta":{"k":1,"k":2}}"#)]
        });

        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 1);
        assert_eq!(
            body["input"][0]["arguments"],
            r#"{"items":[{"q":"b"}],"meta":{"k":2}}"#
        );
    }

    /// 正常参数必须一个字节都不动：这里刻意保留空格与键序，一旦被 canonical 化就会失败。
    #[test]
    fn leaves_well_formed_arguments_untouched() {
        let original = r#"{ "command": ["ls", "-la"], "workdir": "/tmp" }"#;
        let mut body = json!({ "input": [call(original)] });

        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 0);
        assert_eq!(body["input"][0]["arguments"], original);
    }

    #[test]
    fn leaves_non_json_arguments_untouched() {
        for original in ["{broken", "plain text", ""] {
            let mut body = json!({ "input": [call(original)] });
            assert_eq!(dedupe_duplicate_argument_keys(&mut body), 0);
            assert_eq!(body["input"][0]["arguments"], original);
        }
    }

    #[test]
    fn ignores_other_item_types_and_string_input() {
        let mut body = json!({
            "input": [
                {"type": "custom_tool_call", "name": "apply_patch", "input": r#"{"a":1,"a":2}"#},
                {"type": "message", "role": "user", "content": r#"{"a":1,"a":2}"#}
            ]
        });
        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 0);

        let mut body = json!({ "input": "just a prompt" });
        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 0);

        let mut body = json!({ "input": [{"type": "function_call", "arguments": {"a": 1}}] });
        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 0);
    }

    #[test]
    fn preserves_every_json_value_kind() {
        let original =
            r#"{"s":"x","i":-3,"u":7,"f":1.5,"b":true,"n":null,"a":[1],"o":{},"dup":1,"dup":2}"#;
        let mut body = json!({ "input": [call(original)] });

        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 1);
        let arguments = body["input"][0]["arguments"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(arguments).unwrap();
        assert_eq!(
            parsed,
            json!({"s":"x","i":-3,"u":7,"f":1.5,"b":true,"n":null,"a":[1],"o":{},"dup":2})
        );
    }

    /// 实测样本：`_web_run` 顶层把 `time` 写了两次，两次都是 null。
    #[test]
    fn repairs_the_observed_web_run_arguments() {
        let arguments = concat!(
            r#"{"system2_search_query":[{"q":"a","domains":["x"],"recency":null}],"#,
            r#""time":null,"open":null,"find":null,"click":null,"image_query":null,"#,
            r#""screenshot":null,"time":null,"response_length":"6000"}"#
        );
        let mut body = json!({ "input": [call(arguments)] });

        assert_eq!(dedupe_duplicate_argument_keys(&mut body), 1);
        let repaired = body["input"][0]["arguments"].as_str().unwrap();
        assert_eq!(repaired.matches("\"time\"").count(), 1);
        let parsed: Value = serde_json::from_str(repaired).unwrap();
        assert_eq!(parsed["time"], Value::Null);
        assert_eq!(parsed["response_length"], "6000");
    }
}
