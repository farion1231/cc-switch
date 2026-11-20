use serde_json::Value;

pub(super) const CLASSIFIER_LOOP_MESSAGE: &str = "CC Switch stopped this request after three consecutive tool-result turns reported an unavailable auto-mode safety classifier. No upstream request was sent. Fix the classifier model mapping or balance, then send a new user message to resume. Tool permission checks remain required.";

pub(super) fn classifier_retry_loop(body: &Value) -> bool {
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_none_or(|tools| tools.is_empty())
    {
        return false;
    }
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return false;
    };
    let mut failed_turns = 0;
    let mut has_failed_results = false;
    for message in messages.iter().rev() {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            if has_failed_results {
                failed_turns += 1;
                if failed_turns == 3 {
                    return true;
                }
                has_failed_results = false;
            }
            continue;
        }
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return false;
        };
        if blocks.is_empty() || !blocks.iter().all(unavailable_classifier_result) {
            return false;
        }
        has_failed_results = true;
    }
    false
}

fn unavailable_classifier_result(block: &Value) -> bool {
    if block.get("type").and_then(Value::as_str) != Some("tool_result")
        || block.get("is_error").and_then(Value::as_bool) != Some(true)
    {
        return false;
    }
    fn matches_error(text: &str) -> bool {
        text.contains("is temporarily unavailable, so auto mode cannot determine the safety of ")
            || text.contains("The auto mode classifier is temporarily unavailable, so ")
    }
    match block.get("content") {
        Some(Value::String(text)) => matches_error(text),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block.get("type").and_then(Value::as_str) == Some("text")
                && block
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(matches_error)
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn failed_turn() -> Value {
        json!({"role":"user", "content":[{
            "type":"tool_result", "tool_use_id":"call_1", "is_error":true,
            "content":"claude-opus-5[1m] is temporarily unavailable, so auto mode cannot determine the safety of Bash right now."
        }]})
    }

    fn request(turns: usize) -> Value {
        let mut messages = vec![json!({"role":"user", "content":"Check the connection"})];
        for _ in 0..turns {
            messages.push(json!({"role":"assistant", "content":[{"type":"tool_use", "id":"call_1", "name":"Bash", "input":{}}]}));
            messages.push(failed_turn());
        }
        json!({"tools":[{"name":"Bash", "input_schema":{"type":"object"}}],"messages":messages})
    }

    #[test]
    fn stops_after_three_failed_turns() {
        assert!(!classifier_retry_loop(&request(2)));
        assert!(classifier_retry_loop(&request(3)));
        assert!(classifier_retry_loop(&request(50)));
    }

    #[test]
    fn a_new_user_message_or_success_resets_the_streak() {
        for content in [
            json!("I fixed the mapping. Try again."),
            json!([{"type":"text","text":"Try again"}]),
            json!([{"type":"tool_result","is_error":false,"content":"ok"}]),
        ] {
            let mut body = request(3);
            body["messages"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"user","content":content}));
            assert!(!classifier_retry_loop(&body));
        }
    }

    #[test]
    fn classifier_calls_and_other_tool_errors_are_not_blocked() {
        let mut body = request(3);
        body["tools"] = json!([]);
        assert!(!classifier_retry_loop(&body));
        let mut body = request(3);
        body["messages"][6]["content"][0]["content"] = json!("Permission denied");
        assert!(!classifier_retry_loop(&body));
        body["messages"][6]["content"][0] = failed_turn()["content"][0].clone();
        body["messages"][6]["content"][0]["is_error"] = json!(false);
        assert!(!classifier_retry_loop(&body));
    }

    #[test]
    fn parallel_tool_failures_count_as_one_turn() {
        let mut body = request(1);
        body["messages"][2]["content"] = json!([
            failed_turn()["content"][0],
            failed_turn()["content"][0],
            failed_turn()["content"][0]
        ]);
        assert!(!classifier_retry_loop(&body));
        body["messages"]
            .as_array_mut()
            .unwrap()
            .extend([failed_turn(), failed_turn()]);
        assert!(!classifier_retry_loop(&body));
    }

    #[test]
    fn recognizes_text_blocks_but_not_successful_or_mixed_results() {
        let mut body = request(3);
        body["messages"][6]["content"][0]["content"] = json!([{"type":"text","text":"The auto mode classifier is temporarily unavailable, so Bash cannot be used right now."}]);
        assert!(classifier_retry_loop(&body));
        body["messages"][6]["content"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"tool_result","content":"ok"}));
        assert!(!classifier_retry_loop(&body));
    }
}
