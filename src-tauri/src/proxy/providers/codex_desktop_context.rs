//! Desktop page snapshots are context, not results of model-issued tool calls.

use serde_json::Value;

fn is_page_context(item: &Value) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("function_call_output")
        || item.get("namespace").and_then(Value::as_str) != Some("agent")
        || item
            .get("call_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
    {
        return false;
    }
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return false;
    };
    if name == "codex_apps_open_page" {
        return true;
    }
    name.strip_prefix("codex_apps_selected_page_part_")
        .and_then(|suffix| suffix.split_once("_of_"))
        .is_some_and(|(part, total)| {
            [part, total]
                .iter()
                .all(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

/// Preserve synthetic page snapshots without inventing a tool-call pairing.
pub(crate) fn normalize_page_context(body: &mut Value) -> usize {
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut rewritten = 0;
    for item in items {
        if !is_page_context(item) {
            continue;
        }
        let Some(output) = item.get("output").and_then(Value::as_str) else {
            continue;
        };
        let text = format!(
            "[Codex Desktop context: {}]\n{output}",
            item["name"].as_str().expect("checked page context name")
        );
        *item = super::codex_compaction::user_message_item(&text);
        rewritten += 1;
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot(name: &str, output: Value) -> Value {
        json!({
            "type": "function_call_output",
            "id": "fco_desktop_context",
            "namespace": "agent",
            "name": name,
            "output": output
        })
    }

    #[test]
    fn desktop_page_snapshots_become_context_without_changing_real_tool_pairs() {
        let call = json!({
            "type": "function_call", "call_id": "call_status",
            "name": "status", "arguments": "{}"
        });
        let result = json!({
            "type": "function_call_output", "call_id": "call_status", "output": "OK"
        });
        let user = json!({"role": "user", "content": "continue"});
        let mut body = json!({"model": "example-model", "previous_response_id": "resp_previous", "input": [
            snapshot("codex_apps_open_page", json!("{\"page_id\":null}")),
            call.clone(), result.clone(),
            snapshot("codex_apps_selected_page_part_1_of_1", json!("Companion Page snapshot part 1 of 1:\n{\"page_id\":null,\"document_view\":null}")),
            user.clone()
        ]});

        assert_eq!(normalize_page_context(&mut body), 2);
        assert_eq!(
            body["input"][0],
            super::super::codex_compaction::user_message_item(
                "[Codex Desktop context: codex_apps_open_page]\n{\"page_id\":null}"
            )
        );
        assert_eq!(body["input"][3]["role"], "user");
        assert!(body["input"][3]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Companion Page snapshot part 1 of 1:\n"));
        assert!(body["input"][3].get("id").is_none());
        assert!(body["input"][3].get("call_id").is_none());
        assert_eq!(body["input"][1], call);
        assert_eq!(body["input"][2], result);
        assert_eq!(body["input"][4], user);
        assert_eq!(body["model"], "example-model");
        assert_eq!(body["previous_response_id"], "resp_previous");
        let once = body.clone();
        assert_eq!(normalize_page_context(&mut body), 0);
        assert_eq!(body, once);
    }

    #[test]
    fn desktop_page_context_preserves_populated_multipart_snapshots() {
        let output = json!({"page_id": "page-1", "document_view": {"text": "selected text"}});
        let mut body = json!({"input": [
            snapshot("codex_apps_selected_page_part_2_of_3", json!(output.to_string()))
        ]});
        assert_eq!(normalize_page_context(&mut body), 1);
        let text = body["input"][0]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("[Codex Desktop context: codex_apps_selected_page_part_2_of_3]\n"));
        assert_eq!(
            serde_json::from_str::<Value>(text.split_once('\n').unwrap().1).unwrap(),
            output
        );
    }

    #[test]
    fn desktop_page_context_handles_null_and_empty_call_ids() {
        for id in [Value::Null, json!(""), json!(" \t\n")] {
            let mut item = snapshot("codex_apps_open_page", json!("context"));
            item["call_id"] = id;
            let mut body = json!({"input": [item]});
            assert_eq!(normalize_page_context(&mut body), 1);
        }
    }

    #[test]
    fn desktop_page_context_leaves_other_outputs_unchanged() {
        let mut real = snapshot("codex_apps_open_page", json!("real result"));
        real["call_id"] = json!("call_real");
        let mut other_namespace = snapshot("codex_apps_open_page", json!("other namespace"));
        other_namespace["namespace"] = json!("other");
        let mut missing_output = snapshot("codex_apps_open_page", Value::Null);
        missing_output.as_object_mut().unwrap().remove("output");
        let mut body = json!({"input": [real, other_namespace, missing_output,
            {"type": "function_call_output", "output": "unknown orphan"},
            {"type": "custom_tool_call_output", "call_id": "call_custom", "output": "OK"},
            snapshot("codex_apps_other_context", json!("other")),
            snapshot("codex_apps_open_page", json!([
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
            ])),
            snapshot("codex_apps_selected_page_part__of_1", json!("malformed")),
            snapshot("codex_apps_selected_page_part_1_of_1_extra", json!("malformed"))
        ]});
        let before = body.clone();
        assert_eq!(normalize_page_context(&mut body), 0);
        assert_eq!(body, before);
        for mut body in [json!({}), json!({"input": "hello"}), json!({"input": null})] {
            let before = body.clone();
            assert_eq!(normalize_page_context(&mut body), 0);
            assert_eq!(body, before);
        }
    }
}
