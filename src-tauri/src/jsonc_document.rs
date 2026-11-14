//! Format-preserving JSONC/JSON5 editing shared by config business layers.
//! File selection, locking, snapshots and rollback belong to the caller.
use crate::error::AppError;
use json_five::rt::parser::{
    ArrayValueContext as RtArrayValueContext, JSONArrayContext as RtJSONArrayContext,
    JSONArrayValue as RtJSONArrayValue, JSONKeyValuePair as RtJSONKeyValuePair,
    JSONObjectContext as RtJSONObjectContext, JSONText as RtJSONText, JSONValue as RtJSONValue,
    KeyValuePairContext as RtKeyValuePairContext,
};
use json_five::tokenize::{tokenize_rt_str, TokType};
use serde_json::Value;

pub(crate) struct JsoncDocument {
    original_source: String,
    original_value: Value,
    semantic: Value,
    text: RtJSONText,
}

impl JsoncDocument {
    pub(crate) fn parse(source: &str) -> Result<Self, AppError> {
        let semantic: Value = json5::from_str(source)
            .map_err(|e| AppError::Config(format!("Failed to parse JSONC config: {e}")))?;
        if !semantic.is_object() {
            return Err(AppError::Config(
                "JSONC config root must be a JSON object".into(),
            ));
        }
        let text = parse_round_trip(source)?;
        Ok(Self {
            original_source: source.to_string(),
            original_value: semantic.clone(),
            semantic,
            text,
        })
    }

    pub(crate) fn value(&self) -> &Value {
        &self.semantic
    }

    pub(crate) fn original_source(&self) -> &str {
        &self.original_source
    }

    pub(crate) fn root_key_count(&self, name: &str) -> Result<usize, AppError> {
        let RtJSONValue::JSONObject {
            key_value_pairs, ..
        } = &self.text.value
        else {
            return Err(AppError::Config(
                "JSONC config root must be a JSON object".into(),
            ));
        };
        let mut count = 0;
        for pair in key_value_pairs {
            if json5_key_name(&pair.key)? == name {
                count += 1;
            }
        }
        Ok(count)
    }

    pub(crate) fn apply(&mut self, desired: &Value) -> Result<bool, AppError> {
        if !desired.is_object() {
            return Err(AppError::Config(
                "JSONC config root must be a JSON object".into(),
            ));
        }
        let line_ending = if self.original_source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let changed = merge_rt_value(
            &mut self.text.value,
            &self.semantic,
            desired,
            "",
            line_ending,
        )?;
        self.semantic = desired.clone();
        Ok(changed)
    }

    pub(crate) fn validated_source(&self) -> Result<String, AppError> {
        let source = self.text.to_string();
        parse_round_trip(&source).map_err(|e| {
            AppError::Config(format!(
                "Refusing to write invalid JSONC config after round-trip serialization: {e}"
            ))
        })?;
        let reparsed: Value = json5::from_str(&source).map_err(|e| {
            AppError::Config(format!(
                "Refusing to write invalid JSONC config after round-trip serialization: {e}"
            ))
        })?;
        if reparsed != self.semantic {
            return Err(AppError::Config(
                "Refusing to write JSONC config: serialized output does not match the intended state".into()
            ));
        }
        if self.semantic == self.original_value {
            Ok(self.original_source.clone())
        } else {
            Ok(source)
        }
    }

    #[cfg(test)]
    pub(crate) fn corrupt_output_for_test(&mut self, parseable: bool) {
        self.text.value = if parseable {
            RtJSONValue::Null
        } else {
            RtJSONValue::Identifier("{".into())
        };
    }
}

fn parse_round_trip(source: &str) -> Result<RtJSONText, AppError> {
    let mut tokens = tokenize_rt_str(source)
        .map_err(|e| AppError::Config(format!("Failed to tokenize JSONC config: {e}")))?;
    // json-five 0.3.1 consumes the closing slash of a block comment but leaves
    // its exclusive end offset on that slash. Line comments ending at EOF or
    // U+2028/U+2029 can also end inside the last UTF-8 character. Repair only
    // these token boundaries; leave the source and already valid spans intact.
    for (start, kind, end) in &mut tokens.tok_spans {
        if *kind == TokType::BlockComment
            && source.as_bytes().get(*end) == Some(&b'/')
            && source
                .get(*start..*end)
                .is_some_and(|text| text.ends_with('*'))
        {
            *end += 1;
        }
        if *kind == TokType::LineComment {
            while *end < source.len() && !source.is_char_boundary(*end) {
                *end += 1;
            }
        }
    }
    json_five::rt::parser::from_tokens(&tokens).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse round-trip JSONC config: {}",
            e.message
        ))
    })
}

fn json5_key_name(key: &RtJSONValue) -> Result<String, AppError> {
    // RT strings/identifiers contain source escapes, not decoded key names.
    // Let json5 decode all three key spellings, including Unicode escapes.
    let object: serde_json::Map<String, Value> = json5::from_str(&format!("{{{key}:null}}"))
        .map_err(|e| AppError::Config(format!("Failed to decode JSONC key: {e}")))?;
    object
        .into_iter()
        .next()
        .map(|(key, _)| key)
        .ok_or_else(|| AppError::Config("Missing JSONC key".into()))
}

fn extract_trailing_indent(separator_ws: &str) -> String {
    separator_ws
        .rsplit_once('\n')
        .map(|(_, tail)| {
            tail.chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect()
        })
        .unwrap_or_default()
}

fn ensure_object_context(context: &mut Option<RtJSONObjectContext>) -> &mut RtJSONObjectContext {
    context.get_or_insert_with(|| RtJSONObjectContext {
        wsc: (String::new(),),
    })
}

fn ensure_kvp_context(
    pair: &mut json_five::rt::parser::JSONKeyValuePair,
) -> &mut RtKeyValuePairContext {
    pair.context.get_or_insert_with(|| RtKeyValuePairContext {
        wsc: (String::new(), String::new(), String::new(), None),
    })
}

fn ensure_array_context(context: &mut Option<RtJSONArrayContext>) -> &mut RtJSONArrayContext {
    context.get_or_insert_with(|| RtJSONArrayContext {
        wsc: (String::new(),),
    })
}

fn ensure_array_value_context(value: &mut RtJSONArrayValue) -> &mut RtArrayValueContext {
    value.context.get_or_insert_with(|| RtArrayValueContext {
        wsc: (String::new(), None),
    })
}

fn object_layout(
    pairs: &[RtJSONKeyValuePair],
    context: &Option<RtJSONObjectContext>,
    parent_indent: &str,
) -> (bool, String) {
    let mut is_multiline = false;
    let mut child_indent = None;

    if let Some(context) = context {
        if context.wsc.0.contains('\n') {
            is_multiline = true;
            child_indent = Some(extract_trailing_indent(&context.wsc.0));
        }
    }
    for pair in pairs {
        if let Some(context) = &pair.context {
            is_multiline |= context.wsc.0.contains('\n')
                || context.wsc.1.contains('\n')
                || context.wsc.2.contains('\n')
                || context
                    .wsc
                    .3
                    .as_ref()
                    .is_some_and(|whitespace| whitespace.contains('\n'));
            if child_indent.is_none() {
                child_indent = context
                    .wsc
                    .3
                    .as_ref()
                    .filter(|whitespace| whitespace.contains('\n'))
                    .map(|whitespace| extract_trailing_indent(whitespace));
            }
        }
    }

    let child_indent = child_indent
        .filter(|indent| !indent.is_empty())
        .unwrap_or_else(|| format!("{parent_indent}  "));
    (is_multiline, child_indent)
}

fn array_layout(
    values: &[RtJSONArrayValue],
    context: &Option<RtJSONArrayContext>,
    parent_indent: &str,
) -> (bool, String) {
    let mut is_multiline = false;
    let mut child_indent = None;

    if let Some(context) = context {
        if context.wsc.0.contains('\n') {
            is_multiline = true;
            child_indent = Some(extract_trailing_indent(&context.wsc.0));
        }
    }
    for value in values {
        if let Some(context) = &value.context {
            is_multiline |= context.wsc.0.contains('\n')
                || context
                    .wsc
                    .1
                    .as_ref()
                    .is_some_and(|whitespace| whitespace.contains('\n'));
            if child_indent.is_none() {
                child_indent = context
                    .wsc
                    .1
                    .as_ref()
                    .filter(|whitespace| whitespace.contains('\n'))
                    .map(|whitespace| extract_trailing_indent(whitespace));
            }
        }
    }

    let child_indent = child_indent
        .filter(|indent| !indent.is_empty())
        .unwrap_or_else(|| format!("{parent_indent}  "));
    (is_multiline, child_indent)
}

fn remove_object_pair_at(
    pairs: &mut Vec<RtJSONKeyValuePair>,
    context: &mut Option<RtJSONObjectContext>,
    index: usize,
) {
    let removed = pairs.remove(index);
    let Some(removed_context) = removed.context else {
        return;
    };

    if index < pairs.len() {
        let after_comma = removed_context.wsc.3.unwrap_or_default();
        if index == 0 {
            ensure_object_context(context).wsc.0.push_str(&after_comma);
        } else {
            let previous = ensure_kvp_context(&mut pairs[index - 1]);
            let separator = previous.wsc.3.take().unwrap_or_default();
            previous.wsc.3 = Some(format!("{separator}{after_comma}"));
        }
    } else if index == 0 {
        let object_context = ensure_object_context(context);
        object_context.wsc.0.push_str(&removed_context.wsc.2);
        if let Some(after_comma) = removed_context.wsc.3 {
            object_context.wsc.0.push_str(&after_comma);
        }
    } else {
        let previous = ensure_kvp_context(&mut pairs[index - 1]);
        let separator = previous.wsc.3.take().unwrap_or_default();
        if let Some(after_comma) = removed_context.wsc.3 {
            previous.wsc.3 = Some(format!("{separator}{after_comma}"));
        } else {
            previous.wsc.2.push_str(&separator);
            previous.wsc.2.push_str(&removed_context.wsc.2);
        }
    }
}

fn remove_array_value_at(
    values: &mut Vec<RtJSONArrayValue>,
    context: &mut Option<RtJSONArrayContext>,
    index: usize,
) {
    let removed = values.remove(index);
    let Some(removed_context) = removed.context else {
        return;
    };

    if index < values.len() {
        let after_comma = removed_context.wsc.1.unwrap_or_default();
        if index == 0 {
            ensure_array_context(context).wsc.0.push_str(&after_comma);
        } else {
            let previous = ensure_array_value_context(&mut values[index - 1]);
            let separator = previous.wsc.1.take().unwrap_or_default();
            previous.wsc.1 = Some(format!("{separator}{after_comma}"));
        }
    } else if index == 0 {
        let array_context = ensure_array_context(context);
        array_context.wsc.0.push_str(&removed_context.wsc.0);
        if let Some(after_comma) = removed_context.wsc.1 {
            array_context.wsc.0.push_str(&after_comma);
        }
    } else {
        let previous = ensure_array_value_context(&mut values[index - 1]);
        let separator = previous.wsc.1.take().unwrap_or_default();
        if let Some(after_comma) = removed_context.wsc.1 {
            previous.wsc.1 = Some(format!("{separator}{after_comma}"));
        } else {
            previous.wsc.0.push_str(&separator);
            previous.wsc.0.push_str(&removed_context.wsc.0);
        }
    }
}

fn append_object_pair(
    pairs: &mut Vec<RtJSONKeyValuePair>,
    context: &mut Option<RtJSONObjectContext>,
    key: &str,
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<(), AppError> {
    let (is_multiline, child_indent) = object_layout(pairs, context, parent_indent);
    let separator = if is_multiline {
        format!("{line_ending}{child_indent}")
    } else {
        String::new()
    };
    let mut pair = RtJSONKeyValuePair {
        key: value_to_rt_value(&Value::String(key.to_string()), "", line_ending)?,
        value: value_to_rt_value(value, &child_indent, line_ending)?,
        context: Some(RtKeyValuePairContext {
            wsc: (String::new(), " ".to_string(), String::new(), None),
        }),
    };

    if let Some(previous) = pairs.last_mut() {
        let previous_context = ensure_kvp_context(previous);
        let trailing_comma = previous_context.wsc.3.is_some();
        let closing_ws = previous_context
            .wsc
            .3
            .take()
            .unwrap_or_else(|| std::mem::take(&mut previous_context.wsc.2));
        previous_context.wsc.3 = Some(separator);
        if trailing_comma {
            ensure_kvp_context(&mut pair).wsc.3 = Some(closing_ws);
        } else {
            ensure_kvp_context(&mut pair).wsc.2 = closing_ws;
        }
    } else {
        let object_context = ensure_object_context(context);
        let closing_ws = std::mem::take(&mut object_context.wsc.0);
        object_context.wsc.0 = separator;
        ensure_kvp_context(&mut pair).wsc.2 = closing_ws;
    }

    pairs.push(pair);
    Ok(())
}

fn append_array_value(
    values: &mut Vec<RtJSONArrayValue>,
    context: &mut Option<RtJSONArrayContext>,
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<(), AppError> {
    let (is_multiline, child_indent) = array_layout(values, context, parent_indent);
    let separator = if is_multiline {
        format!("{line_ending}{child_indent}")
    } else {
        String::new()
    };
    let mut array_value = RtJSONArrayValue {
        value: value_to_rt_value(value, &child_indent, line_ending)?,
        context: Some(RtArrayValueContext {
            wsc: (String::new(), None),
        }),
    };

    if let Some(previous) = values.last_mut() {
        let previous_context = ensure_array_value_context(previous);
        let trailing_comma = previous_context.wsc.1.is_some();
        let closing_ws = previous_context
            .wsc
            .1
            .take()
            .unwrap_or_else(|| std::mem::take(&mut previous_context.wsc.0));
        previous_context.wsc.1 = Some(separator);
        if trailing_comma {
            ensure_array_value_context(&mut array_value).wsc.1 = Some(closing_ws);
        } else {
            ensure_array_value_context(&mut array_value).wsc.0 = closing_ws;
        }
    } else {
        let array_context = ensure_array_context(context);
        let closing_ws = std::mem::take(&mut array_context.wsc.0);
        array_context.wsc.0 = separator;
        ensure_array_value_context(&mut array_value).wsc.0 = closing_ws;
    }

    values.push(array_value);
    Ok(())
}

fn merge_rt_value(
    round_trip: &mut RtJSONValue,
    current: &Value,
    desired: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<bool, AppError> {
    if current == desired {
        return Ok(false);
    }

    match (round_trip, current, desired) {
        (
            RtJSONValue::JSONObject {
                key_value_pairs,
                context,
            },
            Value::Object(current),
            Value::Object(desired),
        ) => {
            let (_, child_indent) = object_layout(key_value_pairs, context, parent_indent);
            let mut changed = false;
            let mut seen = std::collections::HashSet::new();
            let mut index = 0;
            while index < key_value_pairs.len() {
                let key = json5_key_name(&key_value_pairs[index].key)?;
                let Some(desired_value) = desired.get(&key) else {
                    remove_object_pair_at(key_value_pairs, context, index);
                    changed = true;
                    continue;
                };
                if !seen.insert(key.clone()) {
                    if current.get(&key) == Some(desired_value) {
                        index += 1;
                        continue;
                    }
                    return Err(AppError::Config(format!(
                        "Cannot edit duplicate JSONC key: {key}"
                    )));
                }

                let current_value = current.get(&key).unwrap_or(&Value::Null);
                changed |= merge_rt_value(
                    &mut key_value_pairs[index].value,
                    current_value,
                    desired_value,
                    &child_indent,
                    line_ending,
                )?;
                index += 1;
            }

            for (key, desired_value) in desired {
                if seen.contains(key) {
                    continue;
                }
                append_object_pair(
                    key_value_pairs,
                    context,
                    key,
                    desired_value,
                    parent_indent,
                    line_ending,
                )?;
                changed = true;
            }
            Ok(changed)
        }
        (
            RtJSONValue::JSONArray { values, context },
            Value::Array(current),
            Value::Array(desired),
        ) => {
            let (_, child_indent) = array_layout(values, context, parent_indent);
            let common_len = values.len().min(current.len()).min(desired.len());
            let mut changed = false;
            for index in 0..common_len {
                changed |= merge_rt_value(
                    &mut values[index].value,
                    &current[index],
                    &desired[index],
                    &child_indent,
                    line_ending,
                )?;
            }
            while values.len() > desired.len() {
                let index = values.len() - 1;
                remove_array_value_at(values, context, index);
                changed = true;
            }
            while values.len() < desired.len() {
                let index = values.len();
                append_array_value(values, context, &desired[index], parent_indent, line_ending)?;
                changed = true;
            }
            Ok(changed)
        }
        (round_trip, _, desired) => {
            *round_trip = value_to_rt_value(desired, parent_indent, line_ending)?;
            Ok(true)
        }
    }
}

fn value_to_rt_value(
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<RtJSONValue, AppError> {
    let source = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::Config(format!("Failed to serialize JSONC value: {e}")))?;
    let adjusted = reindent_json5_block(&source, parent_indent, line_ending);
    let text = parse_round_trip(&adjusted)?;
    Ok(text.value)
}

fn reindent_json5_block(source: &str, parent_indent: &str, line_ending: &str) -> String {
    if !source.contains('\n') {
        return source.to_string();
    }

    let mut lines = source.lines();
    let Some(first_line) = lines.next() else {
        return String::new();
    };

    let mut result = String::from(first_line);
    for line in lines {
        result.push_str(line_ending);
        result.push_str(parent_indent);
        result.push_str(line);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_comments_and_unchanged_nodes_with_both_line_endings() {
        let original = r#"/* 顶部 */{
	// 模型说明
	"model": "keep /* literal */ // text",
	"provider": { /* 供应商 */ "demo": {"options": {"apiKey": "old", "keep": 1,},},},
	"nested": [/* 数组 */ {"unchanged": true},],
}/* 尾部 */
"#;
        for source in [original.to_string(), original.replace("\n", "\r\n")] {
            let mut doc = JsoncDocument::parse(&source).unwrap();
            assert!(!doc.apply(&doc.value().clone()).unwrap());
            assert_eq!(doc.validated_source().unwrap(), source);
            let mut desired = doc.value().clone();
            desired["provider"]["demo"]["options"]["apiKey"] = json!("new");
            assert!(doc.apply(&desired).unwrap());
            assert_eq!(
                doc.validated_source().unwrap(),
                source.replace("old", "new")
            );
        }
    }

    #[test]
    fn block_comment_boundaries_do_not_swallow_other_fields() {
        for source in [
            r#"{/*中文*/"keep":1,// tail */
"edit":0}/*end*/"#,
            r#"/**/{/*one*//*two*/"keep":1,"edit":0}/**/"#,
        ] {
            let mut doc = JsoncDocument::parse(source).unwrap();
            doc.apply(&json!({"keep":1,"edit":2})).unwrap();
            assert_eq!(doc.validated_source().unwrap(), source.replace(":0", ":2"));
        }
    }

    #[test]
    fn unicode_line_comments_preserve_bytes_before_and_after_editing() {
        for suffix in [
            "// 中文",
            "// 😀",
            "/* 中文 */ // 尾",
            "// ab\u{2028}",
            "// 中文\u{2028}",
            "// ab\u{2029}",
            "// 中文\u{2029}",
            "// 😀\u{2028}",
            "// 😀\u{2029}",
            "// 中文\n",
            "// 中文\r\n",
            "// ASCII",
        ] {
            let source = format!(r#"{{"provider":{{"demo":{{"name":"old"}}}}}} {suffix}"#);
            let mut doc = JsoncDocument::parse(&source).unwrap();
            assert!(!doc.apply(&doc.value().clone()).unwrap());
            assert_eq!(
                doc.validated_source().unwrap().as_bytes(),
                source.as_bytes()
            );

            let desired = json!({"provider": {"demo": {"name": "new"}}});
            assert!(doc.apply(&desired).unwrap());
            let output = doc.validated_source().unwrap();
            assert_eq!(output.as_bytes(), source.replace("old", "new").as_bytes());
            assert_eq!(json5::from_str::<Value>(&output).unwrap(), desired);
        }
    }

    #[test]
    fn unicode_line_separators_keep_the_following_field_outside_the_comment() {
        for separator in ['\u{2028}', '\u{2029}'] {
            for comment in ["ASCII", "中文", "😀"] {
                let source = format!("{{\"keep\":true,// {comment}{separator}\"edit\":0}}");
                let mut doc = JsoncDocument::parse(&source).unwrap();
                assert_eq!(doc.validated_source().unwrap(), source);
                let desired = json!({"keep":true,"edit":1});
                doc.apply(&desired).unwrap();
                let output = doc.validated_source().unwrap();
                assert_eq!(output, source.replace(":0", ":1"));
                assert_eq!(json5::from_str::<Value>(&output).unwrap(), desired);
            }
        }
    }

    #[test]
    fn escaped_keys_are_decoded_and_new_keys_are_escaped() {
        let source = r#"{"prov\u0069der":{"quote\"key":1,'single\'key':2,unquoted:3},"keep":true}"#;
        let mut doc = JsoncDocument::parse(source).unwrap();
        let mut desired = doc.value().clone();
        desired["provider"]["quote\"key"] = json!(4);
        desired["provider"]
            .as_object_mut()
            .unwrap()
            .remove("single'key");
        for key in ["new\"key", "back\\slash", "line\nkey", "中文键", "\u{0000}"] {
            desired["provider"][key] = json!({"nested": true});
        }
        doc.apply(&desired).unwrap();
        let output = doc.validated_source().unwrap();
        assert!(output.contains(r#""prov\u0069der""#));
        assert!(output.contains("unquoted:3"));
        assert_eq!(json5::from_str::<Value>(&output).unwrap(), desired);
    }

    #[test]
    fn edits_nested_arrays_and_adds_and_removes_object_members() {
        for source in [
            "{\n  /* keep */ \"items\": [1,2,3,], \"remove\": {},\n}",
            "{\r\n\t/* keep */ \"items\": [1,2,3], \"remove\": {}\r\n}",
        ] {
            let mut doc = JsoncDocument::parse(source).unwrap();
            for desired in [
                json!({"items":[1,4],"added":{"x":[1,2]}}),
                json!({"items":[]}),
                json!({}),
                json!({"after":1}),
            ] {
                doc.apply(&desired).unwrap();
                let output = doc.validated_source().unwrap();
                assert!(output.contains("/* keep */"));
                assert_eq!(json5::from_str::<Value>(&output).unwrap(), desired);
            }
        }
    }

    #[test]
    fn appending_keeps_trailing_commas_and_does_not_copy_comments_as_indentation() {
        let mut doc = JsoncDocument::parse("{\n\t/*keep*/ \"items\": [1,],\n}\n").unwrap();
        doc.apply(&json!({"items":[1,2],"new":true})).unwrap();
        assert_eq!(
            doc.validated_source().unwrap(),
            "{\n\t/*keep*/ \"items\": [1,2,],\n\t\"new\": true,\n}\n"
        );
    }

    #[test]
    fn rejects_invalid_roots_and_corrupted_output() {
        for source in ["{broken", "[]", "null", "42", "\"text\""] {
            assert!(JsoncDocument::parse(source).is_err());
        }
        for parseable in [false, true] {
            let mut doc = JsoncDocument::parse("{}").unwrap();
            doc.apply(&json!({"new":true})).unwrap();
            doc.corrupt_output_for_test(parseable);
            assert!(doc.validated_source().is_err());
        }
    }
}
