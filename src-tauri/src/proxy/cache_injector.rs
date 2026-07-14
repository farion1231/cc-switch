//! Cache 断点注入器
//!
//! 在请求转发前自动注入 cache_control 标记，启用 Bedrock Prompt Caching

use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 在请求体关键位置注入 cache_control 断点
pub fn inject(body: &mut Value, config: &OptimizerConfig) {
    if !config.enabled || !config.cache_injection {
        return;
    }

    let existing = count_existing(body);

    if existing > 4 {
        // Existing markers are caller-owned. Do not silently delete or reorder
        // them; surface the invalid/unsupported total and leave validation to
        // the upstream provider.
        log::warn!(
            "[OPT] cache: existing breakpoint count {existing} exceeds the supported total of 4; preserving caller input"
        );
    }

    let mut budget = 4_usize.saturating_sub(existing);
    if budget == 0 {
        log::info!("[OPT] cache: no-op(existing={existing})");
        return;
    }

    let mut injected = Vec::new();

    // (a) tools 末尾
    if budget > 0 {
        if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
            if let Some(last) = tools.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert("cache_control".to_string(), make_cache_control());
                    }
                    budget -= 1;
                    injected.push("tools");
                }
            }
        }
    }

    // (b) system 末尾
    if budget > 0 {
        // 字符串 system → 转为数组
        if let Some(text) = body
            .get("system")
            .and_then(|s| s.as_str())
            .map(str::to_string)
        {
            body["system"] = json!([{"type": "text", "text": text}]);
        }

        if let Some(system) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
            if let Some(last) = system.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert("cache_control".to_string(), make_cache_control());
                    }
                    budget -= 1;
                    injected.push("system");
                }
            }
        }
    }

    // (c) 最后一条可缓存消息的最后一个非 thinking block。工具循环通常以
    // user/tool_result 结束；只标 assistant 会让最新稳定前缀无法命中缓存。
    if budget > 0 {
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for message in messages.iter_mut().rev() {
                if inject_message_breakpoint(message) {
                    budget -= 1;
                    injected.push("msgs-latest");
                    break;
                }
            }

            // (d) A second, older user anchor helps long tool-result turns where
            // the stable prefix falls outside Anthropic's 20-block lookback from
            // the newest breakpoint. Keep this best-effort and inside the 4-BP cap.
            if budget > 0 && messages.len() >= 4 {
                let mut user_count = 0;
                for message in messages.iter_mut().rev() {
                    if message.get("role").and_then(Value::as_str) != Some("user") {
                        continue;
                    }
                    user_count += 1;
                    if user_count == 2 {
                        if inject_message_breakpoint(message) {
                            injected.push("msgs-prior-user");
                        }
                        break;
                    }
                }
            }
        }
    }

    log::info!(
        "[OPT] cache: {}bp({},{},pre={existing})",
        injected.len(),
        injected.join("+"),
        "5m",
    );
}

fn inject_message_breakpoint(message: &mut Value) -> bool {
    let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
        return false;
    };
    let Some(block) = content.iter_mut().rev().find(|block| {
        !matches!(
            block.get("type").and_then(Value::as_str),
            Some("thinking" | "redacted_thinking")
        )
    }) else {
        return false;
    };
    if block.get("cache_control").is_some() {
        return false;
    }
    let Some(object) = block.as_object_mut() else {
        return false;
    };
    object.insert("cache_control".to_string(), make_cache_control());
    true
}

fn make_cache_control() -> Value {
    json!({"type": "ephemeral"})
}

fn count_existing(body: &Value) -> usize {
    let mut count = 0;

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        count += tools
            .iter()
            .filter(|t| t.get("cache_control").is_some())
            .count();
    }

    if let Some(system) = body.get("system").and_then(|s| s.as_array()) {
        count += system
            .iter()
            .filter(|b| b.get("cache_control").is_some())
            .count();
    }

    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                count += content
                    .iter()
                    .filter(|b| b.get("cache_control").is_some())
                    .count();
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: true,
            cache_injection: true,
        }
    }

    #[test]
    fn test_empty_body_no_injection() {
        let mut body = json!({"model": "test", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
        inject(&mut body, &default_config());
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_inject_three_breakpoints() {
        let mut body = json!({
            "model": "test",
            "tools": [{"name": "tool1"}, {"name": "tool2"}],
            "system": [{"type": "text", "text": "sys prompt"}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "hello"}
                ]}
            ]
        });

        inject(&mut body, &default_config());

        // tools last element
        assert!(body["tools"][1].get("cache_control").is_some());
        assert!(body["tools"][1]["cache_control"].get("ttl").is_none());
        // system last element
        assert!(body["system"][0].get("cache_control").is_some());
        // assistant last non-thinking block
        assert!(body["messages"][1]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_long_history_uses_fourth_prior_user_breakpoint() {
        let mut body = json!({
            "model":"test",
            "tools":[{"name":"tool1"}],
            "system":[{"type":"text","text":"sys"}],
            "messages":[
                {"role":"user","content":[{"type":"text","text":"first"}]},
                {"role":"assistant","content":[{"type":"text","text":"answer"}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"c1","content":"result"}]},
                {"role":"assistant","content":[{"type":"text","text":"latest"}]}
            ]
        });

        inject(&mut body, &default_config());
        assert_eq!(count_existing(&body), 4);
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_some());
        assert!(body["messages"][3]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_existing_four_breakpoints_preserve_caller_ttl() {
        let mut body = json!({
            "model": "test",
            "tools": [
                {"name": "t1", "cache_control": {"type": "ephemeral", "ttl": "1h"}},
                {"name": "t2", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
            ],
            "system": [
                {"type": "text", "text": "sys", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
            ],
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "ok", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
                ]}
            ]
        });

        inject(&mut body, &default_config());

        // Existing markers are caller-owned; only newly injected markers are fixed to 5m.
        assert_eq!(body["tools"][0]["cache_control"]["ttl"], "1h");
        assert_eq!(body["tools"][1]["cache_control"]["ttl"], "1h");
        assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["ttl"],
            "1h"
        );
    }

    #[test]
    fn test_existing_two_injects_two_more() {
        let mut body = json!({
            "model": "test",
            "tools": [
                {"name": "t1", "cache_control": {"type": "ephemeral"}},
                {"name": "t2", "cache_control": {"type": "ephemeral"}}
            ],
            "system": [{"type": "text", "text": "sys"}],
            "messages": [
                {"role": "assistant", "content": [{"type": "text", "text": "ok"}]}
            ]
        });

        inject(&mut body, &default_config());

        // budget = 4 - 2 = 2, inject system + msgs
        assert!(body["system"][0].get("cache_control").is_some());
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_more_than_four_existing_breakpoints_are_preserved() {
        let mut body = json!({
            "model": "test",
            "tools": [
                {"name": "t1", "cache_control": {"type": "ephemeral"}},
                {"name": "t2", "cache_control": {"type": "ephemeral"}}
            ],
            "system": [
                {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "s2", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "m1", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "m2"}
            ]}]
        });

        inject(&mut body, &default_config());

        assert_eq!(count_existing(&body), 5);
        assert!(body["messages"][0]["content"][1]
            .get("cache_control")
            .is_none());
    }

    #[test]
    fn test_system_string_converted_to_array() {
        let mut body = json!({
            "model": "test",
            "system": "You are a helpful assistant",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });

        inject(&mut body, &default_config());

        assert!(body["system"].is_array());
        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0]["type"], "text");
        assert_eq!(sys[0]["text"], "You are a helpful assistant");
        assert!(sys[0].get("cache_control").is_some());
    }

    #[test]
    fn test_standard_five_minute_cache_control_omits_ttl() {
        let mut body = json!({
            "model": "test",
            "tools": [{"name": "tool1"}],
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });

        inject(&mut body, &default_config());

        let cc = &body["tools"][0]["cache_control"];
        assert_eq!(cc["type"], "ephemeral");
        assert!(cc.get("ttl").is_none() || cc["ttl"].is_null());
    }

    #[test]
    fn test_disabled_no_change() {
        let config = OptimizerConfig {
            cache_injection: false,
            ..default_config()
        };
        let mut body = json!({
            "model": "test",
            "tools": [{"name": "tool1"}],
            "system": [{"type": "text", "text": "sys"}],
            "messages": [{"role": "assistant", "content": [{"type": "text", "text": "ok"}]}]
        });
        let original = body.clone();

        inject(&mut body, &config);

        assert_eq!(body, original);
    }

    #[test]
    fn test_optimizer_disabled_no_change() {
        let config = OptimizerConfig {
            enabled: false,
            cache_injection: true,
            ..default_config()
        };
        let mut body = json!({
            "model":"test",
            "tools":[{"name":"tool1"}],
            "messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]
        });
        let original = body.clone();

        inject(&mut body, &config);
        assert_eq!(body, original);
    }

    #[test]
    fn test_skip_thinking_blocks_in_assistant() {
        let mut body = json!({
            "model": "test",
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "result"},
                    {"type": "redacted_thinking", "data": "xxx"}
                ]}
            ]
        });

        inject(&mut body, &default_config());

        // Should inject on "text" block (last non-thinking), not on thinking/redacted_thinking
        assert!(body["messages"][0]["content"][1]
            .get("cache_control")
            .is_some());
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert!(body["messages"][0]["content"][2]
            .get("cache_control")
            .is_none());
    }

    #[test]
    fn test_injects_latest_tool_result_instead_of_older_assistant() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "Read", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "done"}]}
            ]
        });

        inject(&mut body, &default_config());

        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert!(body["messages"][1]["content"][0]
            .get("cache_control")
            .is_some());
    }
}
