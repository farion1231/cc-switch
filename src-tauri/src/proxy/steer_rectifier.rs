//! Opt-in Claude Code human-steer compatibility for Chat Completions upstreams.
//!
//! For upstreams with cache misses on mid-conversation system steer messages,
//! convert only Claude Code's exact human-steer envelope before format conversion,
//! leaving its text and position intact.
//! Apply on every request, including replayed history; no per-process cache is needed.

use super::types::RectifierConfig;
use serde_json::Value;

// Exact human-steer wrapper from Claude Code 2.1.261. Unknown wrappers are left alone.
const HUMAN_STEER_PREFIX: &str = "The user sent a new message while you were working:\n";
const HUMAN_STEER_SUFFIX: &str = "\n\nThis is how Claude Code surfaces messages the user sends mid-turn — within the running turn, often alongside the next tool result, rather than as a separate conversation turn. Address the message above as you continue this turn.";

fn is_human_steer(content: &Value) -> bool {
    let text = match content {
        Value::String(text) => Some(text.as_str()),
        Value::Array(blocks) if blocks.len() == 1 => {
            let block = &blocks[0];
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        }
        _ => None,
    };
    text.and_then(|text| text.strip_prefix(HUMAN_STEER_PREFIX))
        .and_then(|text| text.strip_suffix(HUMAN_STEER_SUFFIX))
        .is_some_and(|prompt| !prompt.trim().is_empty())
}

/// Returns the number of role changes. Never reads or alters top-level `system`.
pub(crate) fn rectify_steer_messages(
    body: &mut Value,
    config: &RectifierConfig,
    api_format: &str,
) -> usize {
    if !config.enabled || !config.request_steer_user_role || api_format != "openai_chat" {
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
            Some("system") if seen_user && message.get("content").is_some_and(is_human_steer) => {
                message["role"] = Value::String("user".into());
                changed += 1;
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

    fn steer(prompt: &str) -> String {
        format!("{HUMAN_STEER_PREFIX}{prompt}{HUMAN_STEER_SUFFIX}")
    }
    fn config() -> RectifierConfig {
        RectifierConfig {
            request_steer_user_role: true,
            ..Default::default()
        }
    }
    fn request(content: Value) -> Value {
        json!({"model":"deepseek-v4-flash", "system":"Real system instruction", "messages":[
            {"role":"user","content":"Start"},
            {"role":"assistant","content":[{"type":"tool_use","id":"call-1","name":"Read","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":"result"}]},
            {"role":"system","content":content}
        ]})
    }

    #[test]
    fn changes_only_role_for_string_and_cached_text_block() {
        for content in [
            json!(steer("Keep it concise")),
            json!([{"type":"text","text":steer("保留当前工作"),"cache_control":{"type":"ephemeral"}}]),
        ] {
            let mut body = request(content);
            let mut expected = body.clone();
            expected["messages"][3]["role"] = json!("user");
            assert_eq!(
                rectify_steer_messages(&mut body, &config(), "openai_chat"),
                1
            );
            assert_eq!(body, expected);
            assert_eq!(
                rectify_steer_messages(&mut body, &config(), "openai_chat"),
                0
            );
        }
    }

    #[test]
    fn rejects_other_sources_partial_envelopes_and_mixed_blocks() {
        for content in [
            json!("Actual system policy"),
            json!(format!("{HUMAN_STEER_PREFIX}incomplete")),
            json!(steer(" ")),
            json!(steer("test").replace("The user sent", "The plugin sent")),
            json!(steer("test").replace(
                "The user sent a new message",
                "A message arrived in the bound thread"
            )),
            json!(format!("Unrelated instruction\n{}", steer("test"))),
            json!([{"type":"text","text":steer("test")},{"type":"text","text":"Other system instruction"}]),
            json!([{"type":"image","text":steer("test")}]),
        ] {
            let mut body = request(content);
            let original = body.clone();
            assert_eq!(
                rectify_steer_messages(&mut body, &config(), "openai_chat"),
                0
            );
            assert_eq!(body, original);
        }
        let mut body = json!({"messages":[{"role":"system","content":steer("Bootstrap system")}]});
        assert_eq!(
            rectify_steer_messages(&mut body, &config(), "openai_chat"),
            0
        );
    }

    #[test]
    fn respects_master_switch_opt_in_and_protocol() {
        for (config, format) in [
            (RectifierConfig::default(), "openai_chat"),
            (
                RectifierConfig {
                    enabled: false,
                    ..config()
                },
                "openai_chat",
            ),
            (config(), "anthropic"),
            (config(), "openai_responses"),
            (config(), "gemini_native"),
        ] {
            let mut body = request(json!(steer("test")));
            let original = body.clone();
            assert_eq!(rectify_steer_messages(&mut body, &config, format), 0);
            assert_eq!(body, original);
        }
        let old: RectifierConfig = serde_json::from_value(json!({"enabled":true})).unwrap();
        assert!(!old.request_steer_user_role);
        let saved = serde_json::to_value(config()).unwrap();
        assert_eq!(saved["requestSteerUserRole"], true);
        assert!(
            serde_json::from_value::<RectifierConfig>(saved)
                .unwrap()
                .request_steer_user_role
        );
    }

    #[test]
    fn converted_history_stays_a_prefix_across_replay_and_next_steer() {
        let mut before = request(
            json!([{"type":"text","text":steer("first"),"cache_control":{"type":"ephemeral"}}]),
        );
        let mut after = before.clone();
        // Claude Code drops cache_control and collapses a single text block on replay.
        after["messages"][3]["content"] = json!(steer("first"));
        after["messages"].as_array_mut().unwrap().extend([
            json!({"role":"assistant","content":"Continuing"}),
            json!({"role":"system","content":[{"type":"text","text":steer("second")}]}),
        ]);
        assert_eq!(
            rectify_steer_messages(&mut before, &config(), "openai_chat"),
            1
        );
        assert_eq!(
            rectify_steer_messages(&mut after, &config(), "openai_chat"),
            2
        );
        let before = crate::proxy::providers::transform::anthropic_to_openai(before).unwrap();
        let after = crate::proxy::providers::transform::anthropic_to_openai(after).unwrap();
        let prefix = before["messages"].as_array().unwrap();
        assert_eq!(
            prefix.as_slice(),
            &after["messages"].as_array().unwrap()[..prefix.len()]
        );
        assert_eq!(before["messages"][0], after["messages"][0]);
        assert_eq!(
            after["messages"].as_array().unwrap().last().unwrap()["role"],
            "user"
        );
    }
}
