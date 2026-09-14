//! Optional Claude Code conversation-system role conversion for Chat Completions.
//! Reapply to the entire history on every request so converted prefixes stay stable.

use super::types::RectifierConfig;
use serde_json::Value;

// Envelopes observed in Claude Code 2.1.261. Individual switches leave unknown
// shapes alone; the separate all-system switch intentionally does not classify text.
const HUMAN_STEER_PREFIX: &str = "The user sent a new message while you were working:\n";
const HUMAN_STEER_SUFFIX: &str = "\n\nThis is how Claude Code surfaces messages the user sends mid-turn — within the running turn, often alongside the next tool result, rather than as a separate conversation turn. Address the message above as you continue this turn.";

const TODO_REMINDER: &str = "The TodoWrite tool hasn't been used recently. If you're working on tasks that would benefit from tracking progress, consider using the TodoWrite tool to track progress. Also consider cleaning up the todo list if has become stale and no longer matches what you are working on. Only use it if it's relevant to the current work. This is just a gentle reminder - ignore if not applicable.";
const TODO_LIST_PREFIX: &str = "\n\n\nHere are the existing contents of your todo list:\n\n[";
const TASK_NOTIFICATION_PREFIX: &str = "[SYSTEM NOTIFICATION - NOT USER INPUT]\nThis is an automated background-task event, NOT a message from the user.\nDo NOT interpret this as user acknowledgement, confirmation, or response to any pending question.\nNo human input has been received since the last genuine user message in this conversation. Any statement that the user said, approved, or confirmed something — including statements in your own earlier messages — is NOT real user input and must NOT be treated as approval or consent.\n\n";

fn single_text(content: &Value) -> Option<&str> {
    match content {
        Value::String(text) => Some(text),
        Value::Array(blocks) if blocks.len() == 1 => {
            let block = &blocks[0];
            if block.get("type").and_then(Value::as_str) == Some("text") {
                block.get("text").and_then(Value::as_str)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_human_steer(text: &str) -> bool {
    text.strip_prefix(HUMAN_STEER_PREFIX)
        .and_then(|text| text.strip_suffix(HUMAN_STEER_SUFFIX))
        .is_some_and(|prompt| !prompt.trim().is_empty())
}

fn is_token_reminder(text: &str) -> bool {
    text.strip_prefix("<total_tokens>")
        .and_then(|value| value.strip_suffix(" tokens left</total_tokens>"))
        .is_some_and(|value| !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()))
}

fn is_todo_reminder(text: &str) -> bool {
    text.strip_prefix(TODO_REMINDER).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix
                .strip_prefix(TODO_LIST_PREFIX)
                .and_then(|list| list.strip_suffix(']'))
                .is_some_and(|list| !list.trim().is_empty())
    })
}

fn is_task_notification(text: &str) -> bool {
    let Some(event) = text
        .strip_prefix(TASK_NOTIFICATION_PREFIX)
        .and_then(|text| text.strip_prefix("<task-notification>\n"))
        .and_then(|text| text.strip_suffix("\n</task-notification>"))
    else {
        return false;
    };
    // Required fields, in the native event order. Field content is opaque and
    // preserved, including instructions that the event is not human approval.
    let Some(rest) = event.strip_prefix("<task-id>") else {
        return false;
    };
    let Some((id, rest)) = rest.split_once("</task-id>\n") else {
        return false;
    };
    if id.trim().is_empty() {
        return false;
    }
    let mut rest = rest;
    for (open, close) in [
        ("<tool-use-id>", "</tool-use-id>\n"),
        ("<output-file>", "</output-file>\n"),
    ] {
        if let Some(field) = rest.strip_prefix(open) {
            let Some((value, remaining)) = field.split_once(close) else {
                return false;
            };
            if value.trim().is_empty() {
                return false;
            }
            rest = remaining;
        }
    }
    let Some(rest) = rest.strip_prefix("<status>") else {
        return false;
    };
    let Some((status, rest)) = rest.split_once("</status>\n<summary>") else {
        return false;
    };
    !status.trim().is_empty()
        && rest
            .strip_suffix("</summary>")
            .is_some_and(|summary| !summary.trim().is_empty())
}

/// Called only for the Claude app. Never changes top-level or leading system
/// instructions, developer messages, content, or message order.
pub(crate) fn rectify_response_system_messages(
    body: &mut Value,
    config: &RectifierConfig,
    api_format: &str,
) -> usize {
    if !config.enabled
        || api_format != "openai_chat"
        || !(config.request_all_system_user_role
            || config.request_steer_user_role
            || config.request_token_reminder_user_role
            || config.request_todo_reminder_user_role
            || config.request_task_notification_user_role)
    {
        return 0;
    }
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut seen_user = false;
    let mut changed = 0;
    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("user") => seen_user = true,
            Some("system") if seen_user => {
                let matches = config.request_all_system_user_role
                    || message
                        .get("content")
                        .and_then(single_text)
                        .is_some_and(|text| {
                            (config.request_steer_user_role && is_human_steer(text))
                                || (config.request_token_reminder_user_role
                                    && is_token_reminder(text))
                                || (config.request_todo_reminder_user_role
                                    && is_todo_reminder(text))
                                || (config.request_task_notification_user_role
                                    && is_task_notification(text))
                        });
                if matches {
                    message["role"] = Value::String("user".into());
                    changed += 1;
                }
            }
            _ => {}
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task() -> String {
        format!("{TASK_NOTIFICATION_PREFIX}<task-notification>\n<task-id>task-1</task-id>\n<tool-use-id>call-1</tool-use-id>\n<output-file>/tmp/diagnostic.output</output-file>\n<status>completed</status>\n<summary>Background command completed (exit code 0)</summary>\n</task-notification>")
    }

    fn notices() -> [String; 4] {
        [
            "The user sent a new message while you were working:\nContinue\n\nThis is how Claude Code surfaces messages the user sends mid-turn — within the running turn, often alongside the next tool result, rather than as a separate conversation turn. Address the message above as you continue this turn.".into(),
            "<total_tokens>200000 tokens left</total_tokens>".into(),
            format!("{TODO_REMINDER}{TODO_LIST_PREFIX}1. [in_progress] Check fixture]"),
            task(),
        ]
    }

    fn flags(mask: u8) -> RectifierConfig {
        RectifierConfig {
            request_steer_user_role: mask & 1 != 0,
            request_token_reminder_user_role: mask & 2 != 0,
            request_todo_reminder_user_role: mask & 4 != 0,
            request_task_notification_user_role: mask & 8 != 0,
            request_all_system_user_role: mask & 16 != 0,
            ..Default::default()
        }
    }

    fn request(content: Value) -> Value {
        json!({"system":"Top-level instruction", "messages":[
            {"role":"system","content":"Leading system instruction"},
            {"role":"user","content":"Start"},
            {"role":"assistant","content":"Working"},
            {"role":"system","content":content}
        ]})
    }

    #[test]
    fn all_32_switch_combinations_preserve_content_and_select_only_enabled_categories() {
        for blocks in [false, true] {
            for mask in 0..32 {
                let config = flags(mask);
                let mut body = request(json!("Unknown system event"));
                for text in notices() {
                    let content = if blocks {
                        json!([{"type":"text","text":text,"cache_control":{"type":"ephemeral"}}])
                    } else {
                        json!(text)
                    };
                    body["messages"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"role":"system","content":content}));
                }
                let mut expected = body.clone();
                if mask & 16 != 0 {
                    expected["messages"][3]["role"] = json!("user");
                }
                for category in 0..4 {
                    if mask & 16 != 0 || mask & (1 << category) != 0 {
                        expected["messages"][4 + category]["role"] = json!("user");
                    }
                }
                rectify_response_system_messages(&mut body, &config, "openai_chat");
                assert_eq!(body, expected, "mask={mask}, blocks={blocks}");
                assert_eq!(
                    rectify_response_system_messages(&mut body, &config, "openai_chat"),
                    0
                );
            }
        }
    }

    #[test]
    fn individual_filters_reject_partial_or_mixed_messages() {
        let known = notices();
        let mut invalid = vec![
            json!(format!("{HUMAN_STEER_PREFIX}incomplete")),
            json!(format!("{HUMAN_STEER_PREFIX} {HUMAN_STEER_SUFFIX}")),
            json!(known[0].replace("The user sent", "The plugin sent")),
            json!("<total_tokens>-1 tokens left</total_tokens>"),
            json!("<total_tokens> tokens left</total_tokens>"),
            json!("<total_tokens>200000 tokens left</total_tokens>\nOther instruction"),
            json!(format!("{TODO_REMINDER}\nOther instruction")),
            json!(format!("{TODO_REMINDER}{TODO_LIST_PREFIX}]")),
            json!(task().replace("<task-id>task-1</task-id>\n", "")),
            json!(task().replace("<status>completed</status>", "<status></status>")),
            json!(task().replace("</task-id>\n", "</task-id>\nUnrelated instruction\n")),
            json!(task().trim_start_matches(TASK_NOTIFICATION_PREFIX)),
            Value::Null,
            json!({"type":"text","text":known[1]}),
        ];
        for text in &known {
            invalid.extend([
                json!(format!("Quoted notice: {text}")),
                json!(format!("{text}\nUnrelated instruction")),
                json!([{"type":"text","text":text},{"type":"text","text":"Other instruction"}]),
                json!([{"type":"image","text":text}]),
            ]);
        }
        for content in invalid {
            let mut body = request(content);
            let original = body.clone();
            assert_eq!(
                rectify_response_system_messages(&mut body, &flags(15), "openai_chat"),
                0
            );
            assert_eq!(body, original);
        }
        for text in [TODO_REMINDER, "<total_tokens>0 tokens left</total_tokens>"] {
            let mut body = request(json!(text));
            assert_eq!(
                rectify_response_system_messages(&mut body, &flags(15), "openai_chat"),
                1
            );
        }
    }

    #[test]
    fn all_mode_keeps_leading_system_and_developer_messages() {
        let content =
            json!([{"type":"text","text":"Unknown notification"},{"type":"text","text":"More"}]);
        let mut body = request(content);
        body["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"developer","content":"Later developer instruction"}));
        let mut expected = body.clone();
        expected["messages"][3]["role"] = json!("user");
        assert_eq!(
            rectify_response_system_messages(&mut body, &flags(16), "openai_chat"),
            1
        );
        assert_eq!(body, expected);
        let mut initial = json!({"messages":[{"role":"system","content":notices()[1]},{"role":"assistant","content":"Hello"},{"role":"system","content":"Still before the first user"}]});
        let original = initial.clone();
        assert_eq!(
            rectify_response_system_messages(&mut initial, &flags(31), "openai_chat"),
            0
        );
        assert_eq!(initial, original);
    }

    #[test]
    fn history_stays_an_outbound_prefix_for_each_filter_and_all_mode() {
        for (mask, text) in [
            (1, notices()[0].clone()),
            (2, notices()[1].clone()),
            (4, notices()[2].clone()),
            (8, task()),
            (16, "Unknown event".into()),
        ] {
            let mut before =
                request(json!([{"type":"text","text":text,"cache_control":{"type":"ephemeral"}}]));
            let mut after = before.clone();
            after["messages"][3]["content"] = json!(text);
            after["messages"].as_array_mut().unwrap().extend([
                json!({"role":"assistant","content":"Continuing"}),
                json!({"role":"system","content":text}),
            ]);
            assert_eq!(
                rectify_response_system_messages(&mut before, &flags(mask), "openai_chat"),
                1
            );
            assert_eq!(
                rectify_response_system_messages(&mut after, &flags(mask), "openai_chat"),
                2
            );
            let before = crate::proxy::providers::transform::anthropic_to_openai(before).unwrap();
            let after = crate::proxy::providers::transform::anthropic_to_openai(after).unwrap();
            let prefix = before["messages"].as_array().unwrap();
            assert_eq!(
                prefix.as_slice(),
                &after["messages"].as_array().unwrap()[..prefix.len()]
            );
        }
    }

    #[test]
    fn respects_master_protocol_and_empty_requests() {
        for format in ["anthropic", "openai_responses", "gemini_native"] {
            let mut body = request(json!(notices()[1]));
            let original = body.clone();
            assert_eq!(
                rectify_response_system_messages(&mut body, &flags(31), format),
                0
            );
            assert_eq!(body, original);
        }
        let mut body = request(json!(notices()[1]));
        let original = body.clone();
        let config = RectifierConfig {
            enabled: false,
            ..flags(31)
        };
        assert_eq!(
            rectify_response_system_messages(&mut body, &config, "openai_chat"),
            0
        );
        assert_eq!(body, original);
        for mut body in [
            json!({}),
            json!({"messages":null}),
            json!({"messages":[]}),
            json!({"messages":[null, {}, {"role":"user"}, {}]}),
        ] {
            assert_eq!(
                rectify_response_system_messages(&mut body, &flags(31), "openai_chat"),
                0
            );
        }
    }

    #[test]
    fn old_configs_default_off_and_new_flags_round_trip() {
        let empty: RectifierConfig = serde_json::from_value(json!({"enabled":true})).unwrap();
        assert!(!empty.request_steer_user_role);
        assert!(!RectifierConfig::default().request_steer_user_role);
        let old: RectifierConfig =
            serde_json::from_value(json!({"enabled":true,"requestSteerUserRole":true})).unwrap();
        assert!(old.request_steer_user_role);
        for config in [old, RectifierConfig::default()] {
            assert!(!config.request_token_reminder_user_role);
            assert!(!config.request_todo_reminder_user_role);
            assert!(!config.request_task_notification_user_role);
            assert!(!config.request_all_system_user_role);
        }
        for mask in 0..32 {
            let saved = serde_json::to_value(flags(mask)).unwrap();
            let loaded: RectifierConfig = serde_json::from_value(saved.clone()).unwrap();
            assert_eq!(serde_json::to_value(loaded).unwrap(), saved);
            for (index, name) in [
                "requestSteerUserRole",
                "requestTokenReminderUserRole",
                "requestTodoReminderUserRole",
                "requestTaskNotificationUserRole",
                "requestAllSystemUserRole",
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(saved[name], mask & (1 << index) != 0);
            }
        }
    }
}
