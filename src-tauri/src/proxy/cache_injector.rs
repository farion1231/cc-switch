//! Cache 断点注入器
//!
//! 在请求转发前自动注入 cache_control 标记，启用 Bedrock Prompt Caching

use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 在请求体关键位置注入 cache_control 断点
pub fn inject(body: &mut Value, config: &OptimizerConfig) {
    if !config.cache_injection {
        return;
    }

    let existing = count_existing(body);

    // 升级已有断点的 TTL
    upgrade_existing_ttl(body, &config.cache_ttl);

    let mut budget = 4_usize.saturating_sub(existing);
    if budget == 0 {
        if existing > 0 {
            log::info!(
                "[OPT] cache: ttl-upgrade({existing}->{},existing={existing})",
                config.cache_ttl
            );
        } else {
            log::info!("[OPT] cache: no-op(existing={existing})");
        }
        return;
    }

    let mut injected = Vec::new();

    // (a) tools 末尾
    if budget > 0 {
        if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
            if let Some(last) = tools.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert(
                            "cache_control".to_string(),
                            make_cache_control(&config.cache_ttl),
                        );
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
        if body.get("system").and_then(|s| s.as_str()).is_some() {
            let text = body["system"].as_str().unwrap().to_string();
            body["system"] = json!([{"type": "text", "text": text}]);
        }

        if let Some(system) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
            if let Some(last) = system.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert(
                            "cache_control".to_string(),
                            make_cache_control(&config.cache_ttl),
                        );
                    }
                    budget -= 1;
                    injected.push("system");
                }
            }
        }
    }

    // (c) 最后一条 assistant 消息的最后一个非 thinking block
    if budget > 0 {
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            if let Some(assistant_msg) = messages
                .iter_mut()
                .rev()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
            {
                if let Some(content) = assistant_msg
                    .get_mut("content")
                    .and_then(|c| c.as_array_mut())
                {
                    // 逆序找最后一个非 thinking/redacted_thinking block
                    if let Some(block) = content.iter_mut().rev().find(|b| {
                        let bt = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        bt != "thinking" && bt != "redacted_thinking"
                    }) {
                        if block.get("cache_control").is_none() {
                            if let Some(o) = block.as_object_mut() {
                                o.insert(
                                    "cache_control".to_string(),
                                    make_cache_control(&config.cache_ttl),
                                );
                            }
                            injected.push("msgs");
                        }
                    }
                }
            }
        }
    }

    log::info!(
        "[OPT] cache: {}bp({},{},pre={existing})",
        injected.len(),
        injected.join("+"),
        config.cache_ttl,
    );
}

fn make_cache_control(ttl: &str) -> Value {
    if ttl == "5m" {
        json!({"type": "ephemeral"})
    } else {
        json!({"type": "ephemeral", "ttl": ttl})
    }
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

fn upgrade_existing_ttl(body: &mut Value, ttl: &str) {
    let upgrade = |val: &mut Value| {
        if let Some(cc) = val.get_mut("cache_control").and_then(|c| c.as_object_mut()) {
            if ttl == "5m" {
                cc.remove("ttl");
            } else {
                cc.insert("ttl".to_string(), json!(ttl));
            }
        }
    };

    if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools.iter_mut() {
            upgrade(tool);
        }
    }

    if let Some(system) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
        for block in system.iter_mut() {
            upgrade(block);
        }
    }

    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                for block in content.iter_mut() {
                    upgrade(block);
                }
            }
        }
    }
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
            cache_ttl: "1h".to_string(),
        }
    }

    #[test]
    fn test_empty_body_no_injection() {
        let mut body = json!({"model": "test", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
        let original = body.clone();
        inject(&mut body, &default_config());
        // No tools, no system, no assistant → no injection
        assert_eq!(body, original);
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
        assert_eq!(body["tools"][1]["cache_control"]["ttl"], "1h");
        // system last element
        assert!(body["system"][0].get("cache_control").is_some());
        // assistant last non-thinking block
        assert!(body["messages"][1]["content"][0]
            .get("cache_control")
            .is_some());
    }

    #[test]
    fn test_existing_four_breakpoints_only_upgrades_ttl() {
        let mut body = json!({
            "model": "test",
            "tools": [
                {"name": "t1", "cache_control": {"type": "ephemeral", "ttl": "5m"}},
                {"name": "t2", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
            ],
            "system": [
                {"type": "text", "text": "sys", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
            ],
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "ok", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ]}
            ]
        });

        inject(&mut body, &default_config());

        // All TTLs upgraded to 1h, no new breakpoints
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
    fn test_ttl_5m_no_ttl_field() {
        let config = OptimizerConfig {
            cache_ttl: "5m".to_string(),
            ..default_config()
        };
        let mut body = json!({
            "model": "test",
            "tools": [{"name": "tool1"}],
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });

        inject(&mut body, &config);

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
}
