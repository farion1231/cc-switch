use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

fn large_test_image_data_url() -> String {
    let bytes = b"CC_SWITCH_TOOL_MEDIA_SENTINEL".repeat(400);
    format!("data:image/png;base64,{}", STANDARD.encode(bytes))
}

fn message_roles(result: &Value) -> Vec<&str> {
    result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message.get("role").and_then(Value::as_str))
        .collect()
}

fn test_function_call(call_id: &str) -> Value {
    json!({
        "type": "function_call",
        "call_id": call_id,
        "name": "view_image",
        "arguments": "{}"
    })
}

fn test_function_output(call_id: &str, output: Value) -> Value {
    json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output
    })
}

fn convert_test_input(items: Vec<Value>) -> Value {
    responses_to_chat_completions_with_reasoning(
        json!({
            "model": "kimi-k3",
            "input": items
        }),
        None,
        true,
    )
    .unwrap()
}

fn result_messages(result: &Value) -> &[Value] {
    result["messages"].as_array().unwrap()
}

fn convert_deepseek_test_input(items: Vec<Value>) -> Value {
    responses_to_chat_completions_with_reasoning(
        json!({
            "model": "deepseek/deepseek-v4-flash",
            "input": items
        }),
        None,
        true,
    )
    .unwrap()
}

#[test]
fn responses_request_to_chat_limits_tool_replay_repair_to_deepseek() {
    for (model, repair) in [
        ("deepseek-v4-flash", true),
        ("deepseek/deepseek-v4-flash", true),
        ("deepseek-ai/DeepSeek-V3.2", true),
        ("Pro/deepseek-ai/DeepSeek-R1", true),
        ("DEEPSEEK-CHAT", true),
        ("deepseek", true),
        ("gpt-5.4", false),
        ("kimi-k3", false),
        ("glm-5.2", false),
        ("MiniMax-M2.7", false),
        ("qwen3.6-plus", false),
        ("claude-sonnet-4-6", false),
        ("custom-alias", false),
        ("deepseek/gpt-5.4", false),
        ("not-deepseek-v4-flash", false),
    ] {
        let result = responses_to_chat_completions_with_reasoning(
            json!({
                "model": model,
                "input": [
                    test_function_call("call_1"),
                    {"type": "message", "role": "assistant", "content": "Replay text.",
                        "reasoning_content": "replay thought"},
                    test_function_output("call_1", json!("ok"))
                ]
            }),
            None,
            true,
        )
        .unwrap();

        if repair {
            assert_complete_tool_turns(&result);
            assert_eq!(message_roles(&result), vec!["assistant", "tool"], "{model}");
            assert_eq!(result["messages"][0]["content"], "Replay text.");
            assert_eq!(result["messages"][0]["reasoning_content"], "replay thought");
        } else {
            assert_eq!(
                result["messages"],
                json!([
                    {"role": "assistant", "content": null, "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {
                            "name": "view_image", "arguments": "{}"
                        }}
                    ], "reasoning_content": "tool call"},
                    {"role": "assistant", "content": "Replay text.", "reasoning_content": "replay thought"},
                    {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
                ]),
                "{model}"
            );
        }
    }
}

fn assert_complete_tool_turns(result: &Value) {
    let mut pending = HashSet::new();
    for message in result_messages(result) {
        let role = message["role"].as_str().unwrap();
        if !pending.is_empty() {
            assert_eq!(role, "tool", "interrupted tool turn: {result}");
        }
        if role == "tool" {
            let call_id = message["tool_call_id"].as_str().unwrap();
            assert!(pending.remove(call_id), "unexpected tool result: {result}");
        }
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                assert!(pending.insert(call["id"].as_str().unwrap()));
            }
        }
    }
    assert!(pending.is_empty(), "missing tool results: {result}");
}

#[test]
fn responses_request_to_chat_keeps_valid_deepseek_tool_history_bytes_stable() {
    let items = vec![
        json!({"type": "message", "role": "user", "content": "Inspect the image."}),
        json!({"type": "message", "role": "assistant", "content": "Checking."}),
        test_function_call("call_1"),
        test_function_call("call_2"),
        test_function_output("call_2", json!("")),
        test_function_output(
            "call_1",
            json!([
                {"type": "text", "text": "image result"},
                {"type": "input_image", "image_url": "https://example.com/image.png"}
            ]),
        ),
        json!({"type": "message", "role": "assistant", "content": "Image inspected.",
            "reasoning_content": "final answer thought"}),
        json!({"type": "message", "role": "user", "content": "Continue."}),
        test_function_call("call_next"),
        test_function_output("call_next", json!("ok")),
    ];
    let original = convert_test_input(items.clone());
    let deepseek = convert_deepseek_test_input(items);

    assert_complete_tool_turns(&deepseek);
    assert_eq!(
        serde_json::to_vec(&deepseek["messages"]).unwrap(),
        serde_json::to_vec(&original["messages"]).unwrap(),
        "well-formed history must keep its cache-sensitive serialization"
    );
}

#[test]
fn responses_request_to_chat_keeps_empty_assistant_replay_inside_tool_turn() {
    let empty_messages = [
        json!({"type": "message", "role": "assistant", "content": null}),
        json!({"type": "message", "role": "assistant", "content": null,
            "reasoning_content": "tool-only thought"}),
        json!({"type": "message", "role": "assistant", "content": ""}),
        json!({"type": "message", "role": "assistant", "content": []}),
        json!({"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": ""}
        ]}),
        json!({"type": "message", "role": "assistant"}),
        json!({"role": "assistant", "content": ""}),
        json!({"type": "future_message", "role": "assistant", "content": ""}),
        json!({"type": "input_text", "role": "assistant", "text": ""}),
    ];

    for empty_message in empty_messages {
        let reasoning = empty_message.get("reasoning_content").cloned();
        let result = convert_deepseek_test_input(vec![
            test_function_call("call_1"),
            empty_message,
            test_function_output("call_1", json!("")),
        ]);

        assert_complete_tool_turns(&result);
        assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
        assert_eq!(result["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(result["messages"][1]["content"], "");
        if let Some(reasoning) = reasoning {
            assert_eq!(result["messages"][0]["reasoning_content"], reasoning);
        }
    }
}

#[test]
fn responses_request_to_chat_coalesces_custom_and_search_tool_replay() {
    for (call, output) in [
        (
            json!({"type": "custom_tool_call", "call_id": "call_1",
                "name": "apply_patch", "input": "patch"}),
            json!({"type": "custom_tool_call_output", "call_id": "call_1", "output": "ok"}),
        ),
        (
            json!({"type": "tool_search_call", "call_id": "call_1", "arguments": {"query": "shell"}}),
            json!({"type": "tool_search_output", "call_id": "call_1", "output": []}),
        ),
    ] {
        let expected_output = canonical_json_string(&output);
        let result = convert_deepseek_test_input(vec![
            call,
            json!({"type": "message", "role": "assistant", "content": ""}),
            output,
        ]);
        assert_complete_tool_turns(&result);
        assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
        assert_eq!(result["messages"][1]["content"], expected_output);
    }
}

#[test]
fn responses_request_to_chat_preserves_replayed_tool_turn_text_and_reasoning() {
    let mut first_call = test_function_call("call_1");
    first_call["reasoning_content"] = json!("first thought");
    let mut second_call = test_function_call("call_2");
    second_call["reasoning_content"] = json!("third thought");
    let result = convert_deepseek_test_input(vec![
        first_call,
        json!({
            "type": "message", "role": "assistant", "content": "First update.",
            "reasoning_content": "second thought"
        }),
        second_call,
        json!({"type": "message", "role": "assistant", "content": "Second update."}),
        test_function_output("call_1", json!("first result")),
        test_function_output("call_2", json!("second result")),
    ]);

    assert_complete_tool_turns(&result);
    assert_eq!(message_roles(&result), vec!["assistant", "tool", "tool"]);
    let assistant = &result["messages"][0];
    assert_eq!(assistant["content"], "First update.\nSecond update.");
    assert_eq!(
        assistant["reasoning_content"],
        "first thought\n\nsecond thought\n\nthird thought"
    );
    assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
    assert_eq!(assistant["tool_calls"][1]["id"], "call_2");
}

#[test]
fn responses_request_to_chat_coalesces_assistant_replay_between_parallel_results() {
    let result = convert_deepseek_test_input(vec![
        test_function_call("call_1"),
        test_function_call("call_2"),
        test_function_output("call_2", json!("second result")),
        json!({
            "type": "message", "role": "assistant", "content": "Still working.",
            "reasoning_content": "waiting for the first result"
        }),
        test_function_output("call_1", json!("first result")),
        json!({"type": "message", "role": "assistant", "content": "Done."}),
    ]);

    assert_complete_tool_turns(&result);
    assert_eq!(
        message_roles(&result),
        vec!["assistant", "tool", "tool", "assistant"]
    );
    assert_eq!(result["messages"][0]["content"], "Still working.");
    assert_eq!(
        result["messages"][0]["reasoning_content"],
        "waiting for the first result"
    );
    assert_eq!(result["messages"][1]["tool_call_id"], "call_2");
    assert_eq!(result["messages"][2]["tool_call_id"], "call_1");
    assert_eq!(result["messages"][3]["content"], "Done.");
}

#[test]
fn responses_request_to_chat_preserves_multimodal_assistant_replay() {
    let result = convert_deepseek_test_input(vec![
        json!({"type": "message", "role": "assistant", "content": "Opening the image."}),
        test_function_call("call_1"),
        json!({"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": "Image context."},
            {"type": "refusal", "refusal": "Cannot identify a person."},
            {"type": "input_image", "image_url": "https://example.com/image.png"}
        ]}),
        test_function_output("call_1", json!("ok")),
    ]);

    assert_complete_tool_turns(&result);
    assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
    assert_eq!(
        result["messages"][0]["content"],
        json!([
            {"type": "text", "text": "Opening the image."},
            {"type": "text", "text": "Image context."},
            {"type": "text", "text": "Cannot identify a person."},
            {"type": "image_url", "image_url": {"url": "https://example.com/image.png"}}
        ])
    );
}

#[test]
fn responses_request_to_chat_keeps_user_boundaries_and_missing_results() {
    for role in ["user", "system"] {
        let result = convert_deepseek_test_input(vec![
            test_function_call("call_missing"),
            json!({"type": "message", "role": role, "content": "New context."}),
            json!({"type": "message", "role": "assistant", "content": "Next turn."}),
            test_function_call("call_next"),
            test_function_output("call_next", json!("ok")),
        ]);
        let messages = result_messages(&result);
        let assistants: Vec<_> = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .collect();
        assert_eq!(assistants.len(), 2);
        assert_eq!(assistants[0]["tool_calls"][0]["id"], "call_missing");
        assert_eq!(assistants[1]["tool_calls"][0]["id"], "call_next");
        assert_eq!(assistants[1]["content"], "Next turn.");
        let outputs: Vec<_> = messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .collect();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["tool_call_id"], "call_next");
    }
}

/// 回归：没有配对 function_call、call_id 为空的工具结果（Codex 桌面端
/// 新建会话时注入的 create_thread 回执就是这个形态）不能序列化成 role=tool，
/// 否则严格上游会以「Messages with role 'tool' must be a response to a
/// preceding message with 'tool_calls'」400 拒掉整个请求。
#[test]
fn responses_request_to_chat_rewrites_orphan_tool_output_without_call_id() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        json!({"type": "function_call_output", "output": "delegation-result"}),
    ]);

    assert!(
        result_messages(&result)
            .iter()
            .all(|message| message.get("role").and_then(Value::as_str) != Some("tool")),
        "orphan tool output must not stay as role=tool: {result}"
    );
    let last = result_messages(&result).last().unwrap();
    assert_eq!(last["role"], "user");
    let content = last["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("delegation-result"),
        "content lost: {content}"
    );
}

/// 回归：call_id 非空但没有对应 tool_call 的孤儿结果同样要降级为 user 消息。
#[test]
fn responses_request_to_chat_rewrites_orphan_tool_output_with_unknown_call_id() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        json!({"type": "function_call_output", "call_id": "call_missing", "output": "stale-result"}),
    ]);

    assert!(result_messages(&result)
        .iter()
        .all(|message| message.get("role").and_then(Value::as_str) != Some("tool")));
    let last = result_messages(&result).last().unwrap();
    assert_eq!(last["role"], "user");
    assert!(last["content"]
        .as_str()
        .unwrap_or_default()
        .contains("stale-result"));
    assert!(last["content"]
        .as_str()
        .unwrap_or_default()
        .contains("call_missing"));
}

/// 正常配对不受影响：function_call -> function_call_output 必须保持 role=tool。
#[test]
fn responses_request_to_chat_keeps_paired_tool_output() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        test_function_call("call_pair_1"),
        test_function_output("call_pair_1", json!("tool-result")),
    ]);

    let messages = result_messages(&result);
    let tool_message = messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .expect("paired output must stay as role=tool");
    assert_eq!(tool_message["tool_call_id"], "call_pair_1");
    assert_eq!(tool_message["content"], "tool-result");
}

/// 并行工具调用只回灌部分结果：已配对的那条保持 role=tool，
/// 没有配对记录的多余结果降级为 user，整体仍然是合法序列。
#[test]
fn responses_request_to_chat_rewrites_extra_tool_output_without_call() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        test_function_call("call_a"),
        test_function_output("call_a", json!("ok-a")),
        test_function_output("call_b", json!("orphan-b")),
    ]);

    let messages = result_messages(&result);
    let tools: Vec<&Value> = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .collect();
    assert_eq!(
        tools.len(),
        1,
        "only the paired output stays as tool: {result}"
    );
    assert_eq!(tools[0]["tool_call_id"], "call_a");
    assert!(messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("user")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("orphan-b"))
    }));
}

/// 复刻 Codex 桌面端新建会话的首轮历史：孤立的 create_thread 回执
/// 出现在 user 环境消息之后、且没有任何 function_call。
#[test]
fn responses_request_to_chat_rewrites_desktop_delegation_receipt() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "environment-context"}]}),
        json!({
            "type": "function_call_output",
            "name": "create_thread",
            "namespace": "codex_app",
            "call_id": null,
            "output": "<codex_delegation>...</codex_delegation>"
        }),
    ]);

    let messages = result_messages(&result);
    assert!(
        messages
            .iter()
            .all(|message| message.get("role").and_then(Value::as_str) != Some("tool")),
        "desktop receipt must not surface as role=tool: {result}"
    );
    assert!(messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| content.contains("codex_delegation"))
    }));
}

/// P2 回归：孤儿工具结果里的媒体不能退化成 base64 纯文本。
/// 图片要复用配对路径的提取逻辑，作为同一条 user 消息的 content parts
/// 输出；否则模型看不到图，base64 还会瞬间撑爆上下文与费用。
#[test]
fn responses_request_to_chat_orphan_tool_output_keeps_media_parts() {
    let data_url = large_test_image_data_url();
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        test_function_output(
            "",
            json!([
                {"type": "input_text", "text": "screenshot follows"},
                {"type": "input_image", "image_url": data_url.clone()}
            ]),
        ),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["user", "user"]);
    let parts = messages[1]["content"]
        .as_array()
        .expect("orphan media output must use content parts");
    let text = parts[0]["text"].as_str().unwrap();
    assert!(text.starts_with(ORPHAN_TOOL_OUTPUT_MARKER), "{text}");
    assert!(text.contains("screenshot follows"), "{text}");
    assert!(
        text.contains(TOOL_RESULT_MEDIA_ATTACHED_MARKER),
        "inline media placeholder must match where the media lands: {text}"
    );
    assert!(
        !text.contains("following user message"),
        "media travels in this same message: {text}"
    );
    assert!(
        !text.contains(&data_url),
        "base64 image must not stay in the text body: {text}"
    );
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(parts[1]["image_url"]["url"], data_url);
}

/// P3 回归：孤儿结果夹在并行工具结果中间时必须先暂存、等回合边界再
/// 降级输出。立即输出会形成 `tool, user, tool`，后一条 tool 前面隔着
/// user，仍会被严格上游以「tool must follow tool_calls」拒掉。
#[test]
fn responses_request_to_chat_defers_orphan_between_parallel_tool_outputs() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        test_function_call("call_a"),
        test_function_call("call_b"),
        test_function_output("call_a", json!("ok-a")),
        test_function_output("call_orphan", json!("orphan-in-middle")),
        test_function_output("call_b", json!("ok-b")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["user", "assistant", "tool", "tool", "user"]
    );
    assert_eq!(messages[2]["tool_call_id"], "call_a");
    assert_eq!(messages[3]["tool_call_id"], "call_b");
    assert!(messages[4]["content"]
        .as_str()
        .unwrap()
        .contains("orphan-in-middle"));
}

/// P3 回归：孤儿结果前尾随的 reasoning 不能丢。`assistant 消息,
/// reasoning, 孤儿, user` 序列里 reasoning 要照常回溯附挂到上一条
/// assistant——降级输出孤儿时清掉 assistant 索引会让紧随其后的 user
/// 边界再也找不到附挂目标。
#[test]
fn responses_request_to_chat_keeps_reasoning_before_boundary_orphan() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "commentary"}]
        }),
        json!({
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "trailing-before-orphan"}]
        }),
        json!({"type": "function_call_output", "output": "orphan-payload"}),
        json!({"role": "user", "content": [{"type": "input_text", "text": "next"}]}),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["user", "assistant", "user", "user"]
    );
    assert_eq!(messages[1]["reasoning_content"], "trailing-before-orphan");
    assert!(messages[2]["content"]
        .as_str()
        .unwrap()
        .contains("orphan-payload"));
    assert_eq!(messages[3]["content"], "next");
}

/// 孤儿降级输出不应改变 reasoning 的归属：`assistant 消息, reasoning,
/// 孤儿, function_call` 里 reasoning 仍要前向附挂到新的 tool-call 批次，
/// 而不是被提前回溯到旧 assistant 或被吞掉。
#[test]
fn responses_request_to_chat_orphan_does_not_divert_forward_reasoning() {
    let result = convert_test_input(vec![
        json!({"role": "user", "content": [{"type": "input_text", "text": "hi"}]}),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "commentary"}]
        }),
        json!({
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "think-before-call"}]
        }),
        json!({"type": "function_call_output", "output": "orphan-payload"}),
        test_function_call("call_next"),
        test_function_output("call_next", json!("next-result")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["user", "assistant", "user", "assistant", "tool"]
    );
    assert_eq!(messages[1].get("reasoning_content"), None);
    assert_eq!(messages[3]["reasoning_content"], "think-before-call");
    assert_eq!(messages[3]["tool_calls"][0]["id"], "call_next");
}

#[test]
fn map_reasoning_effort_deepseek_keeps_official_tiers() {
    // DeepSeek 官方档位 low / high / max，其余照官方服务端的换算折。
    for (requested, expected) in [
        ("minimal", "low"),
        ("low", "low"),
        ("medium", "high"),
        ("high", "high"),
        ("xhigh", "high"),
        ("max", "max"),
        ("ultra", "max"),
    ] {
        assert_eq!(
            map_reasoning_effort(requested, Some("deepseek"), None),
            Some(expected),
            "{requested}"
        );
    }
}

#[test]
fn map_reasoning_effort_handles_ultra_per_mode() {
    // passthrough 面向枚举未知的通用上游：ultra 与 max/xhigh 一样原值透传，
    // 让声明了该档位的上游能收到用户选择。
    assert_eq!(map_reasoning_effort("ultra", None, None), Some("ultra"));
    // 已知枚举的专用模式钳到自身最高合法档，而不是走 None 被静默丢弃。
    assert_eq!(
        map_reasoning_effort("ultra", Some("deepseek"), None),
        Some("max")
    );
    assert_eq!(
        map_reasoning_effort("ultra", Some("low_high"), None),
        Some("high")
    );
    assert_eq!(
        map_reasoning_effort("ultra", Some("openrouter"), None),
        Some("xhigh")
    );
    // 真正的未知值仍然丢弃，防上游 400。
    assert_eq!(map_reasoning_effort("turbo", None, None), None);
}

#[test]
fn responses_request_with_stream_injects_include_usage() {
    let input = json!({
        "model": "kimi-k2.6",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
        "stream": true
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(result["stream"], true);
    assert_eq!(result["stream_options"]["include_usage"], true);
}

#[test]
fn responses_request_without_stream_omits_stream_options() {
    let input = json!({
        "model": "kimi-k2.6",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(result.get("stream_options").is_none());
}

#[test]
fn responses_request_merges_include_usage_into_existing_stream_options() {
    let input = json!({
        "model": "kimi-k2.6",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
        "stream": true,
        "stream_options": {"continuous_usage_stats": true}
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    // 既补上 include_usage，又保留客户端原有的 stream_options 字段。
    assert_eq!(result["stream_options"]["include_usage"], true);
    assert_eq!(result["stream_options"]["continuous_usage_stats"], true);
}

#[test]
fn responses_request_maps_input_file_content_parts() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [{
            "role": "user",
            "content": [
                {"type": "input_text", "text": "Summarize this."},
                {
                    "type": "input_file",
                    "file_id": "file_123",
                    "file_url": "https://example.com/spec.pdf",
                    "filename": "spec.pdf"
                },
                {
                    "type": "input_audio",
                    "input_audio": {
                        "data": "UklGRg==",
                        "format": "wav"
                    }
                }
            ]
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let content = result["messages"][0]["content"].as_array().unwrap();

    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "file");
    assert_eq!(content[1]["file"]["file_id"], "file_123");
    assert!(content[1]["file"].get("file_url").is_none());
    assert_eq!(content[1]["file"]["filename"], "spec.pdf");
    assert_eq!(content[2]["type"], "input_audio");
    assert_eq!(content[2]["input_audio"]["format"], "wav");
}

#[test]
fn responses_request_does_not_emit_chat_file_for_url_only_input_file() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [{
            "role": "user",
            "content": [
                {"type": "input_text", "text": "Summarize this URL file."},
                {
                    "type": "input_file",
                    "file_url": "https://example.com/spec.pdf"
                }
            ]
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(result["messages"][0]["content"], "Summarize this URL file.");
}

#[test]
fn responses_request_maps_top_level_input_file_item() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "input_file",
                "file_id": "file_top",
                "filename": "top.pdf"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let content = result["messages"][0]["content"].as_array().unwrap();

    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(content[0]["type"], "file");
    assert_eq!(content[0]["file"]["file_id"], "file_top");
    assert_eq!(content[0]["file"]["filename"], "top.pdf");
}

#[test]
fn top_level_user_content_part_clears_pending_reasoning() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "reasoning",
                "summary": [{"text": "stale reasoning"}]
            },
            {
                "type": "input_text",
                "text": "Please run the tool."
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{}"
            }
        ],
        "tools": [{
            "type": "function",
            "name": "lookup",
            "parameters": {"type": "object"}
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Please run the tool.");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["reasoning_content"], "tool call");
}

#[test]
fn responses_request_to_chat_maps_messages_tools_and_limits() {
    let input = json!({
        "model": "gpt-5.4",
        "instructions": "You are concise.",
        "input": [
            {
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Weather?"},
                    {"type": "input_image", "image_url": "data:image/png;base64,abc"},
                    {"type": "input_text", "text": "Use Celsius."}
                ]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Tokyo\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Sunny"
            }
        ],
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get weather",
            "parameters": {"type": "object"},
            "strict": true
        }],
        "tool_choice": {"type": "function", "name": "get_weather"},
        "max_output_tokens": 100,
        "reasoning": {"effort": "high"},
        "stream": true
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(result["model"], "gpt-5.4");
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][1]["role"], "user");
    assert_eq!(result["messages"][1]["content"][0]["type"], "text");
    assert_eq!(result["messages"][1]["content"][1]["type"], "image_url");
    assert_eq!(result["messages"][1]["content"][2]["type"], "text");
    assert_eq!(result["messages"][1]["content"][2]["text"], "Use Celsius.");
    assert_eq!(result["messages"][2]["tool_calls"][0]["id"], "call_1");
    assert_eq!(result["messages"][3]["role"], "tool");
    assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
    assert_eq!(result["tools"][0]["function"]["strict"], true);
    assert_eq!(result["tool_choice"]["function"]["name"], "get_weather");
    assert_eq!(result["max_tokens"], 100);
    assert_eq!(result["reasoning_effort"], "high");
}

#[test]
fn responses_request_to_chat_omits_missing_tool_description() {
    // A missing description must be omitted, not serialized as null —
    // strict OpenAI-compatible upstreams reject the whole request on a
    // null value (400 "expected string, received null").
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ],
        "tools": [
            {"type": "function", "name": "no_desc", "parameters": {"type": "object"}},
            {"type": "function", "name": "with_desc", "description": "Has one", "parameters": {"type": "object"}}
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert!(tools[0]["function"].get("description").is_none());
    assert_eq!(tools[1]["function"]["description"], "Has one");
}

#[test]
fn responses_request_to_chat_defaults_null_tool_parameters() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "codex_app__automation_update",
            "description": "Update an automation.",
            "parameters": null
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(
        result["tools"][0]["function"]["parameters"],
        json!({"type": "object", "properties": {}})
    );
}

#[test]
fn responses_request_to_chat_defaults_nested_null_tool_parameters() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "function": {
                "name": "codex_app__automation_update",
                "description": "Update an automation.",
                "parameters": null
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(
        result["tools"][0]["function"]["parameters"],
        json!({"type": "object", "properties": {}})
    );
}

#[test]
fn responses_request_to_chat_defaults_nested_missing_tool_parameters() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "function": {
                "name": "codex_app__automation_update",
                "description": "Update an automation."
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(
        result["tools"][0]["function"]["parameters"],
        json!({"type": "object", "properties": {}})
    );
}

#[test]
fn responses_request_to_chat_normalizes_explicit_null_tool_parameter_type() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "search",
            "parameters": {
                "type": null,
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let parameters = &result["tools"][0]["function"]["parameters"];

    assert_eq!(parameters["type"], "object");
    assert_eq!(parameters["properties"]["query"]["type"], "string");
    assert_eq!(parameters["required"], json!(["query"]));
}

#[test]
fn responses_request_to_chat_normalizes_null_required_and_preserves_closed_schema() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "search",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": null,
                "additionalProperties": false
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let parameters = &result["tools"][0]["function"]["parameters"];

    assert_eq!(parameters["required"], json!([]));
    assert_eq!(parameters["additionalProperties"], json!(false));
    assert_eq!(parameters["properties"]["query"]["type"], json!("string"));
}

#[test]
fn responses_request_to_chat_adds_required_for_closed_nested_schemas() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "inspect",
            "parameters": {
                "type": "object",
                "properties": {
                    "options": {
                        "type": "object",
                        "properties": {"verbose": {"type": "boolean"}},
                        "additionalProperties": false
                    },
                    "entries": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {"id": {"type": "string"}},
                            "required": null,
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["options", "entries"]
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let parameters = &result["tools"][0]["function"]["parameters"];

    assert_eq!(parameters["properties"]["options"]["required"], json!([]));
    assert_eq!(
        parameters["properties"]["options"]["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        parameters["properties"]["entries"]["items"]["required"],
        json!([])
    );
    assert_eq!(
        parameters["properties"]["entries"]["items"]["additionalProperties"],
        json!(false)
    );
}

#[test]
fn responses_request_to_chat_sanitizes_schema_combinators_and_defs() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "choose",
            "parameters": {
                "type": "object",
                "oneOf": [{
                    "type": "object",
                    "required": null,
                    "additionalProperties": false
                }],
                "$defs": {
                    "payload": {
                        "type": "object",
                        "required": null,
                        "additionalProperties": false
                    }
                }
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let parameters = &result["tools"][0]["function"]["parameters"];

    assert_eq!(parameters["oneOf"][0]["required"], json!([]));
    assert_eq!(parameters["oneOf"][0]["additionalProperties"], json!(false));
    assert_eq!(parameters["$defs"]["payload"]["required"], json!([]));
    assert_eq!(
        parameters["$defs"]["payload"]["additionalProperties"],
        json!(false)
    );
}

#[test]
fn responses_request_to_chat_preserves_valid_and_unconstrained_schemas() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "lookup",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "default": {"required": null}
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }, {
            "type": "function",
            "name": "open_lookup",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    assert_eq!(
        result["tools"][0]["function"]["parameters"],
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "default": {"required": null}
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    );
    assert_eq!(
        result["tools"][1]["function"]["parameters"],
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}}
        })
    );
}

#[test]
fn responses_request_to_chat_defaults_top_level_one_of_tool_parameters_to_object() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "lookup",
            "parameters": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {"id": {"type": "string"}}
                    },
                    {
                        "type": "object",
                        "properties": {"slug": {"type": "string"}}
                    }
                ]
            }
        }],
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let parameters = &result["tools"][0]["function"]["parameters"];

    assert_eq!(parameters["type"], "object");
    assert_eq!(
        parameters["oneOf"],
        json!([
            {
                "type": "object",
                "properties": {"id": {"type": "string"}}
            },
            {
                "type": "object",
                "properties": {"slug": {"type": "string"}}
            }
        ])
    );
}

#[test]
fn responses_request_to_chat_exposes_tool_search_and_loaded_namespace_tools() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{"type": "tool_search"}],
        "input": [
            {
                "type": "tool_search_call",
                "call_id": "call_tool_search_1",
                "status": "completed",
                "execution": "client",
                "arguments": {"query": "Gmail search emails", "limit": 5}
            },
            {
                "type": "tool_search_output",
                "call_id": "call_tool_search_1",
                "status": "completed",
                "execution": "client",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__codex_apps__gmail",
                    "description": "Find and reference emails from your inbox.",
                    "tools": [{
                        "type": "function",
                        "name": "_search_emails",
                        "description": "Search Gmail for emails matching a query.",
                        "strict": false,
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "query": {"type": "string"},
                                "max_results": {"type": "integer"}
                            },
                            "required": ["query"]
                        }
                    }]
                }]
            },
            {
                "type": "message",
                "role": "user",
                "content": "Search unread inbox mail."
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let tools = result["tools"].as_array().unwrap();
    let tool_names = tools
        .iter()
        .filter_map(|tool| tool.pointer("/function/name").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();

    assert!(tool_names.contains(&"tool_search"));
    assert!(tool_names.contains(&"mcp__codex_apps__gmail___search_emails"));
    assert_eq!(
        result["messages"][0]["tool_calls"][0]["function"]["name"],
        "tool_search"
    );
    assert_eq!(result["messages"][1]["role"], "tool");
    assert_eq!(result["messages"][1]["tool_call_id"], "call_tool_search_1");
    assert!(result["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("mcp__codex_apps__gmail"));
}

#[test]
fn responses_request_to_chat_maps_custom_tool_and_choice() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply a patch to files."
        }],
        "tool_choice": {"type": "custom", "name": "apply_patch"},
        "input": [{
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_patch",
            "name": "apply_patch",
            "input": "*** Begin Patch\n*** End Patch"
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(result["tools"][0]["function"]["name"], "apply_patch");
    assert_eq!(
        result["tools"][0]["function"]["parameters"]["required"][0],
        "input"
    );
    assert_eq!(result["tool_choice"]["function"]["name"], "apply_patch");
    assert_eq!(
        result["messages"][0]["tool_calls"][0]["function"]["arguments"],
        r#"{"input":"*** Begin Patch\n*** End Patch"}"#
    );
}

#[test]
fn responses_request_to_chat_preserves_custom_tool_metadata_in_description() {
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "custom",
            "name": "apply_patch",
            "description": "Use the `apply_patch` tool to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: begin_patch hunk+ end_patch"
            }
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let description = result["tools"][0]["function"]["description"]
        .as_str()
        .unwrap();

    assert!(description.starts_with("Original tool definition:"));
    assert!(!description.contains("Original Codex tool definition"));
    assert!(description.contains("\"type\":\"custom\""));
    assert!(description.contains("\"format\":"));
    assert!(description.contains("\"syntax\":\"lark\""));
}

#[test]
fn responses_request_to_chat_drops_responses_only_stream_options() {
    // 聚合模式下 Codex 以为在跟内置 openai 说话，会带上 Responses 专有的
    // reasoning_summary_delivery；Chat 没有这个字段，Mistral 会 422（#9507）。
    let input = json!({
        "model": "deepseek-v4-1-flash-1017",
        "input": "hello",
        "stream": true,
        "stream_options": {"reasoning_summary_delivery": "sequential_cutoff"},
        "tools": [{"type": "web_search"}]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(result["stream_options"], json!({"include_usage": true}));
    // 托管 web_search 在 Chat 路径上本来就丢掉。
    assert!(result.get("tools").is_none());

    let non_stream = responses_to_chat_completions_with_reasoning(
        json!({
            "model": "m",
            "input": "hello",
            "stream_options": {"reasoning_summary_delivery": "sequential_cutoff", "other": 1}
        }),
        None,
        true,
    )
    .unwrap();
    assert_eq!(non_stream["stream_options"], json!({"other": 1}));

    let only_private = responses_to_chat_completions_with_reasoning(
        json!({
            "model": "m",
            "input": "hello",
            "stream_options": {"reasoning_summary_delivery": "sequential_cutoff"}
        }),
        None,
        true,
    )
    .unwrap();
    assert!(only_private.get("stream_options").is_none());
}

#[test]
fn responses_request_to_chat_uses_provider_reasoning_effort_for_deepseek_model() {
    let input = json!({
        "model": "deepseek-v4-pro",
        "input": "hello",
        "reasoning": {"effort": "xhigh"}
    });
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("thinking".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("deepseek".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert_eq!(result["thinking"]["type"], "enabled");
    // DeepSeek 官方服务端把 xhigh 当 high。
    assert_eq!(result["reasoning_effort"], "high");
}

/// 严格上游（Mistral）拒收 assistant 历史里的 `reasoning_content`：转换出口按
/// `preserve_reasoning_content` 剥离，真实思考回填与 `"tool call"` 占位两种来源
/// 都要覆盖。回归锁：issue #9507。
#[test]
fn responses_request_to_chat_drops_reasoning_content_for_strict_upstream() {
    let input = json!({
        "model": "deepseek-v4-1-flash-1017",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "list files"}]
            },
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "need ls"}]
            },
            {
                "type": "function_call",
                "call_id": "c1",
                "name": "shell",
                "arguments": "{\"cmd\":\"ls\"}"
            },
            {"type": "function_call_output", "call_id": "c1", "output": "a.txt"},
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "thanks"}]
            },
            // 第二轮没有思考内容：只会拿到 "tool call" 占位。
            {
                "type": "function_call",
                "call_id": "c2",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}"
            },
            {"type": "function_call_output", "call_id": "c2", "output": "/tmp"}
        ],
        "reasoning": {"effort": "high"}
    });
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(false),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("deepseek".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    // 保持原行为：第三方网关仍要这个字段，两种来源都在。
    let preserved =
        responses_to_chat_completions_with_reasoning(input.clone(), Some(&config), true).unwrap();
    let preserved_reasoning: Vec<&str> = result_messages(&preserved)
        .iter()
        .filter_map(|message| message.get("reasoning_content")?.as_str())
        .collect();
    assert!(
        preserved_reasoning.contains(&"need ls"),
        "preserve=true 时应保留真实思考: {preserved}"
    );
    assert!(
        preserved_reasoning.contains(&"tool call"),
        "preserve=true 时应保留占位: {preserved}"
    );

    // 严格上游：assistant 消息上不得残留该字段，其余内容不变。
    let stripped =
        responses_to_chat_completions_with_reasoning(input, Some(&config), false).unwrap();
    let stripped_has_reasoning = result_messages(&stripped)
        .iter()
        .any(|message| message.get("reasoning_content").is_some());
    assert!(
        !stripped_has_reasoning,
        "preserve=false 时应剥离 reasoning_content: {stripped}"
    );
    let stripped_has_tool_calls = result_messages(&stripped)
        .iter()
        .any(|message| message.get("tool_calls").is_some());
    assert!(
        stripped_has_tool_calls,
        "工具调用本身不能被剥离: {stripped}"
    );
    // 非 assistant 消息不受影响。
    assert_eq!(result_messages(&stripped)[0]["role"], "user");
}

#[test]
fn chat_usage_to_responses_usage_maps_deepseek_cache_hit_tokens() {
    // DeepSeek Chat 的文档化缓存命中字段也要进 Responses 的
    // input_tokens_details（issue #6073 关联）：当上游只发该字段、不镜像
    // prompt_tokens_details.cached_tokens 时（如部分中转），少了这个兜底，
    // 路由模式下合成的 response.completed 里 cached_tokens 为 0，
    // Codex 侧会话记录与本地日志都拿不到缓存命中。
    let usage = json!({
        "prompt_tokens": 1000,
        "completion_tokens": 100,
        "total_tokens": 1100,
        "prompt_cache_hit_tokens": 600,
        "prompt_cache_miss_tokens": 400
    });

    let result = chat_usage_to_responses_usage(Some(&usage));
    assert_eq!(result["input_tokens"], 1000);
    assert_eq!(result["output_tokens"], 100);
    assert_eq!(result["input_tokens_details"]["cached_tokens"], 600);
    assert_eq!(result["input_tokens_details"]["cache_write_tokens"], 0);
}

#[test]
fn chat_usage_to_responses_usage_stub_zero_cache_read_falls_through_to_deepseek_hit_tokens() {
    // #8041 报告者 payload：桩 0 不能短路候选链——合成 response.completed 后
    // from_codex_response_auto 只能看到这个重建对象，真值必须经
    // input_tokens_details.cached_tokens 存活到落库/计费（Responses 形态的
    // input_tokens 保持含缓存的总量，缓存命中进 details）。
    let usage = json!({
        "prompt_tokens": 6650,
        "completion_tokens": 16,
        "total_tokens": 6666,
        "cache_read_input_tokens": 0,
        "prompt_cache_hit_tokens": 6400
    });

    let result = chat_usage_to_responses_usage(Some(&usage));
    assert_eq!(result["input_tokens"], 6650);
    assert_eq!(result["input_tokens_details"]["cached_tokens"], 6400);
}

#[test]
fn responses_request_to_chat_maps_openrouter_to_native_reasoning_object() {
    // OpenRouter 平台形态：原生 reasoning:{effort} 对象 + "openrouter" 值映射
    // （与 infer_aggregator_platform_config 推断出的配置保持一致）。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(false),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning.effort".to_string()),
        effort_value_mode: Some("openrouter".to_string()),
        output_format: Some("auto".to_string()),
        effort_levels: None,
    };

    // max 不在 OpenRouter 枚举内（见 openclaw#77350），必须钳成 xhigh，
    // 且写进原生 reasoning 对象，而非顶层 reasoning_effort 别名。
    let input = json!({
        "model": "deepseek/deepseek-chat-v3.1",
        "input": "hello",
        "reasoning": {"effort": "max"}
    });
    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert_eq!(result["reasoning"]["effort"], "xhigh");
    assert!(result.get("reasoning_effort").is_none());
    // thinking_param=none：即使 supports_effort 把 supports_thinking 带成 true，
    // 也不写任何 thinking 字段（OpenRouter 不认 thinking:{type}）。
    assert!(result.get("thinking").is_none());

    // 合法档位原样透传。
    let input_high = json!({
        "model": "deepseek/deepseek-chat-v3.1",
        "input": "hello",
        "reasoning": {"effort": "high"}
    });
    let result_high =
        responses_to_chat_completions_with_reasoning(input_high, Some(&config), true).unwrap();
    assert_eq!(result_high["reasoning"]["effort"], "high");
    assert!(result_high.get("reasoning_effort").is_none());
}

#[test]
fn responses_request_to_chat_passes_explicit_none_through_for_openrouter() {
    // OpenRouter 原生 reasoning 对象支持显式关闭：effort=none 应忠实转发为
    // {"reasoning":{"effort":"none"}}，而非被吞掉——否则默认开思考的模型无法关闭，
    // 带来行为与成本偏差。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(false),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning.effort".to_string()),
        effort_value_mode: Some("openrouter".to_string()),
        output_format: Some("auto".to_string()),
        effort_levels: None,
    };

    let input = json!({
        "model": "openai/gpt-5",
        "input": "hello",
        "reasoning": {"effort": "none"}
    });
    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert_eq!(result["reasoning"]["effort"], "none");
    // none 不是 OpenAI 顶层 reasoning_effort 的合法枚举，不写顶层别名；也不写 thinking。
    assert!(result.get("reasoning_effort").is_none());
    assert!(result.get("thinking").is_none());
}

#[test]
fn responses_request_to_chat_drops_explicit_none_for_top_level_effort_provider() {
    // 对照：顶层 reasoning_effort 平台（DeepSeek/OpenAI 风格）的 effort 枚举不含 none，
    // 显式 none 不应透传成 reasoning_effort:"none"（会被上游拒），仅走 thinking 关闭路径。
    // 锁定「none 透传仅限 reasoning.effort 形态」的边界，防止回归。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("thinking".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("deepseek".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    let input = json!({
        "model": "deepseek-v4-pro",
        "input": "hello",
        "reasoning": {"effort": "none"}
    });
    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    // thinking 关闭信号照发；但不写 reasoning_effort，也不写原生 reasoning 对象。
    assert_eq!(result["thinking"]["type"], "disabled");
    assert!(result.get("reasoning_effort").is_none());
    assert!(result.get("reasoning").is_none());
}

#[test]
fn responses_request_to_chat_clamps_zen_effort_to_model_declared_levels() {
    // OpenCode Zen 平台形态 + 逐模型档位钳制（表数据镜像 models.dev：glm-5.2
    // 仅声明 high|max）。锁定：统一并集映射会把 Codex 默认的 medium 发给只声明
    // high|max 的 glm-5.2（恰好是预设默认模型），严格校验的网关会报错；
    // 低档一律上钳到最近合法档，超出最高档（含 Codex 扩展档 ultra）取最高合法档。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("zen".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: Some(vec!["high".to_string(), "max".to_string()]),
    };

    for (input_effort, expected) in [
        ("minimal", "high"),
        ("low", "high"),
        ("medium", "high"),
        ("high", "high"),
        ("xhigh", "max"),
        ("max", "max"),
        ("ultra", "max"),
    ] {
        let input = json!({
            "model": "glm-5.2",
            "input": "hello",
            "reasoning": {"effort": input_effort}
        });
        let result =
            responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();
        assert_eq!(
            result["reasoning_effort"], expected,
            "effort={input_effort}"
        );
        // 写成顶层 reasoning_effort；不发任何 thinking 字段
        // （网关只认平台归一参数，不认厂商 thinking 形状）。
        assert!(result.get("thinking").is_none());
    }
}

#[test]
fn responses_request_to_chat_zen_preserves_low_when_model_declares_it() {
    // deepseek-v4-flash 声明 low|high|max：low 合法原样透传，medium 上钳 high，
    // xhigh 上钳 max——回归锁：旧 deepseek 厂商分支把非 max 一律归 high，
    // 逐模型钳制不得比那更差，也不得发出声明外的值。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("zen".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: Some(vec![
            "low".to_string(),
            "high".to_string(),
            "max".to_string(),
        ]),
    };

    for (input_effort, expected) in [
        ("minimal", "low"),
        ("low", "low"),
        ("medium", "high"),
        ("xhigh", "max"),
    ] {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": "hello",
            "reasoning": {"effort": input_effort}
        });
        let result =
            responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();
        assert_eq!(
            result["reasoning_effort"], expected,
            "effort={input_effort}"
        );
    }
}

#[test]
fn responses_request_to_chat_zen_single_level_model_clamps_everything_to_max() {
    // kimi-k3 仅声明 max（全模型交集不存在，统一映射覆盖不了它——必须逐模型）：
    // 任何请求档都钳到 max。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("zen".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: Some(vec!["max".to_string()]),
    };

    for input_effort in ["minimal", "medium", "high", "max"] {
        let input = json!({
            "model": "kimi-k3",
            "input": "hello",
            "reasoning": {"effort": input_effort}
        });
        let result =
            responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();
        assert_eq!(result["reasoning_effort"], "max", "effort={input_effort}");
    }
}

#[test]
fn responses_request_to_chat_omits_zen_effort_without_model_levels() {
    // 模型未在目录声明 effort（toggle/budget 型如 glm-5.1、qwen 系，或目录未收录）
    // → 完全不发 reasoning_effort，避免给严格校验的网关送无法核实的值。
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(true),
        thinking_param: Some("none".to_string()),
        effort_param: Some("reasoning_effort".to_string()),
        effort_value_mode: Some("zen".to_string()),
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    let input = json!({
        "model": "glm-5.1",
        "input": "hello",
        "reasoning": {"effort": "medium"}
    });
    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert!(result.get("reasoning_effort").is_none());
    assert!(result.get("thinking").is_none());
}

#[test]
fn responses_request_to_chat_maps_thinking_only_provider_without_effort() {
    let input = json!({
        "model": "kimi-k2.6",
        "input": "hello",
        "reasoning": {"effort": "high"}
    });
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some("thinking".to_string()),
        effort_param: Some("none".to_string()),
        effort_value_mode: None,
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert_eq!(result["thinking"]["type"], "enabled");
    assert!(result.get("reasoning_effort").is_none());
}

#[test]
fn responses_request_to_chat_maps_enable_thinking_provider() {
    let input = json!({
        "model": "qwen3-max",
        "input": "hello",
        "reasoning": {"effort": "medium"}
    });
    let config = CodexChatReasoningConfig {
        supports_thinking: Some(true),
        supports_effort: Some(false),
        thinking_param: Some("enable_thinking".to_string()),
        effort_param: Some("none".to_string()),
        effort_value_mode: None,
        output_format: Some("reasoning_content".to_string()),
        effort_levels: None,
    };

    let result = responses_to_chat_completions_with_reasoning(input, Some(&config), true).unwrap();

    assert_eq!(result["enable_thinking"], true);
    assert!(result.get("reasoning_effort").is_none());
}

#[test]
fn chat_response_to_responses_extracts_reasoning_details() {
    let input = json!({
        "id": "chatcmpl_minimax",
        "object": "chat.completion",
        "created": 123,
        "model": "MiniMax-M2.7",
        "choices": [{
            "message": {
                "role": "assistant",
                "reasoning_details": [
                    {"type": "reasoning_text", "text": "Need to inspect the code."}
                ],
                "content": "Done"
            },
            "finish_reason": "stop"
        }]
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["output"][0]["type"], "reasoning");
    assert_eq!(
        result["output"][0]["summary"][0]["text"],
        "Need to inspect the code."
    );
    assert_eq!(result["output"][1]["content"][0]["text"], "Done");
}

#[test]
fn chat_completion_to_response_reads_mistral_thinking_chunks() {
    // #9902：Mistral 开思考后 content 是 thinking / text 分段数组
    let input = json!({
        "id": "chatcmpl_mistral",
        "created": 123,
        "model": "mistral-large-4",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": [{"type": "text", "text": "We "}, {"type": "text", "text": "add"}]},
                    {"type": "text", "text": "391"}
                ]
            },
            "finish_reason": "stop"
        }]
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["output"][0]["type"], "reasoning");
    assert_eq!(result["output"][0]["summary"][0]["text"], "We add");
    assert_eq!(result["output"][1]["content"][0]["text"], "391");
    assert_eq!(result["output"].as_array().unwrap().len(), 2);
}

#[test]
fn chat_completion_to_response_unwraps_data_envelope() {
    // #5658：ClinePass 把非流式 Chat 响应包在 data 里
    let input = json!({
        "success": true,
        "data": {
            "id": "chatcmpl-wrapped",
            "model": "cline-pass/deepseek-v4.1-flash",
            "created": 1_700_000_000,
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "OK"}
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 1, "total_tokens": 13}
        }
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["model"], "cline-pass/deepseek-v4.1-flash");
    assert_eq!(result["created_at"], 1_700_000_000);
    assert_eq!(result["status"], "completed");
    assert_eq!(result["output"][0]["content"][0]["text"], "OK");
    assert_eq!(result["usage"]["input_tokens"], 12);
    assert_eq!(result["usage"]["output_tokens"], 1);
}

#[test]
fn responses_request_to_chat_normalizes_codex_internal_roles() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "message",
                "role": "developer",
                "content": [
                    {"type": "input_text", "text": "Follow project instructions."}
                ]
            },
            {
                "type": "message",
                "role": "latest_reminder",
                "content": "Keep the reply brief."
            },
            {
                "type": "message",
                "role": "unknown_codex_role",
                "content": "Fallback content."
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "Follow project instructions.");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Keep the reply brief.");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"], "Fallback content.");
}

#[test]
fn responses_request_to_chat_keeps_only_head_system_for_minimax() {
    let input = json!({
        "model": "MiniMax-M2.7",
        "instructions": "You are Codex.",
        "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Permissions block"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "AGENTS.md"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]},
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Collaboration Mode: Default"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]}
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    for (idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap();
        if idx == 0 {
            assert_eq!(role, "system", "first message must be system");
        } else {
            assert_ne!(
                role, "system",
                "no system role allowed past index 0 (got at {idx})"
            );
        }
    }

    let head_content = messages[0]["content"].as_str().unwrap();
    assert!(head_content.contains("You are Codex."));
    assert!(head_content.contains("Permissions block"));
    // A developer message opening a later turn stays in place as user, so
    // the head (and the prefix already sent) does not change.
    assert!(!head_content.contains("Collaboration Mode: Default"));
    assert_eq!(messages[3]["role"], "user");
    assert_eq!(messages[3]["content"], "Collaboration Mode: Default");
}

#[test]
fn turn_start_developer_message_keeps_previous_request_as_prefix() {
    let first_turn = vec![
        json!({"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Permissions: read-only"}]}),
        json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "list files"}]}),
        json!({"type": "function_call", "call_id": "call_0", "name": "exec_command", "arguments": "{}"}),
        json!({"type": "function_call_output", "call_id": "call_0", "output": "a.rs"}),
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}),
    ];
    let mut second_turn = first_turn.clone();
    second_turn.extend([
        json!({"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Permissions: workspace-write"}]}),
        json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "edit a.rs"}]}),
    ]);

    let convert = |input: Vec<Value>| {
        let body = json!({"model": "k3", "instructions": "You are Codex.", "input": input});
        responses_to_chat_completions_with_reasoning(body, None, true).unwrap()["messages"]
            .as_array()
            .unwrap()
            .clone()
    };
    let first = convert(first_turn);
    let second = convert(second_turn);

    assert_eq!(&second[..first.len()], &first[..]);
    assert_eq!(second[first.len()]["role"], "user");
    assert_eq!(
        second[first.len()]["content"],
        "Permissions: workspace-write"
    );
}

#[test]
fn additional_tools_carrier_is_lifted_not_converted_to_a_message() {
    // Shape from #7451: Codex 0.154+ ships extra tools in an
    // `additional_tools` input carrier (role `developer`, no `content`).
    let input = json!({
        "model": "agnes-3.0-flash",
        "input": [
            {
                "type": "additional_tools",
                "id": "at_a8d5b9f5",
                "role": "developer",
                "tools": [
                    {
                        "type": "namespace",
                        "name": "functions",
                        "description": "",
                        "tools": [
                            {
                                "type": "function",
                                "name": "exec_command",
                                "description": "Run a shell command.",
                                "strict": false,
                                "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}, "required": ["cmd"]}
                            }
                        ]
                    },
                    {
                        "type": "function",
                        "name": "wait",
                        "description": "Waits on a yielded exec cell.",
                        "strict": false,
                        "parameters": {"type": "object", "properties": {"cell_id": {"type": "string"}}, "required": ["cell_id"]}
                    }
                ]
            },
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "You are Codex, a coding agent."}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]}
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    // The carrier is not a message: every emitted message must carry
    // real content, and the developer instruction must remain the only
    // system head.
    for (idx, message) in messages.iter().enumerate() {
        assert!(
            !matches!(message.get("content"), None | Some(Value::Null)),
            "messages[{idx}] lost its content: {message}"
        );
    }
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are Codex, a coding agent.");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "你好");

    // The carried tools survive as flattened chat tools.
    let tool_names: Vec<&str> = result["tools"]
        .as_array()
        .expect("carried tools must be lifted into top-level tools")
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(
        tool_names.contains(&"functions__exec_command"),
        "{tool_names:?}"
    );
    assert!(tool_names.contains(&"wait"), "{tool_names:?}");
}

#[test]
fn additional_tools_carrier_tools_dedup_against_top_level_tools() {
    let input = json!({
        "model": "m",
        "tools": [
            {"type": "function", "name": "wait", "description": "Top-level wait.", "parameters": {"type": "object", "properties": {}}}
        ],
        "input": [
            {
                "type": "additional_tools",
                "role": "developer",
                "tools": [
                    {"type": "function", "name": "wait", "description": "Carried wait.", "parameters": {"type": "object", "properties": {}}}
                ]
            },
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "duplicate carried tool must be dropped");
    assert_eq!(tools[0]["function"]["name"], "wait");
    assert_eq!(tools[0]["function"]["description"], "Top-level wait.");
}

#[test]
fn plain_developer_messages_convert_without_additional_tools_carrier() {
    // No carrier present: developer messages keep mapping to system and
    // no tools array is fabricated.
    let input = json!({
        "model": "m",
        "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Instructions."}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "Instructions.");
    assert_eq!(messages[1]["role"], "user");
    assert!(result.get("tools").is_none());
}

#[test]
fn empty_user_messages_from_compacted_media_only_input_are_dropped() {
    // Codex 压缩后把只有图片的用户消息换成空文本，转成 Chat 不能留下 content: ""。
    let input = json!({
        "model": "agnes-2.5-flash",
        "instructions": "Instructions.",
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": ""}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Fix the bug."}]},
            {"type": "message", "role": "user", "content": []},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Summary."}]}
        ]
    });
    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["content"], "Fix the bug.");
    assert_eq!(messages[2]["content"], "Summary.");
}

#[test]
fn dropping_empty_user_keeps_mid_conversation_developer_in_place() {
    let input = json!({
        "model": "agnes-2.5-flash",
        "instructions": "Instructions.",
        "input": [
            {"type": "message", "role": "user", "content": "U1"},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "A1"}]},
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "S2"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": ""}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "A2"}]}
        ]
    });
    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    let roles: Vec<&str> = messages.iter().filter_map(|m| m["role"].as_str()).collect();
    assert_eq!(
        roles,
        vec!["system", "user", "assistant", "user", "assistant"]
    );
    assert_eq!(messages[0]["content"], "Instructions.");
    assert_eq!(messages[3]["content"], "S2");
}

#[test]
fn non_empty_user_messages_are_kept_verbatim() {
    let input = json!({
        "model": "agnes-2.5-flash",
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": " "}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": ""}]},
            {"type": "message", "role": "user", "content": "Next."}
        ]
    });
    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["content"], " ");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["content"], "Next.");
}

#[test]
fn collapse_system_messages_preserves_non_system_order() {
    let input = vec![
        json!({"role": "system", "content": "S1"}),
        json!({"role": "user", "content": "U1"}),
        json!({"role": "assistant", "content": "A1"}),
        json!({"role": "system", "content": "S2"}),
        json!({"role": "user", "content": "U2"}),
    ];
    let out = collapse_system_messages_to_head(input);

    assert_eq!(out.len(), 5);
    assert_eq!(out[0]["role"], "system");
    assert_eq!(out[0]["content"], "S1");
    assert_eq!(out[1]["content"], "U1");
    assert_eq!(out[2]["content"], "A1");
    assert_eq!(out[3]["role"], "user");
    assert_eq!(out[3]["content"], "S2");
    assert_eq!(out[4]["content"], "U2");
}

#[test]
fn collapse_system_messages_hoists_mid_system_not_followed_by_user() {
    let input = vec![
        json!({"role": "system", "content": "S1"}),
        json!({"role": "user", "content": "U1"}),
        json!({"role": "assistant", "content": null, "tool_calls": [{"id": "c0", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}),
        json!({"role": "system", "content": "Notice"}),
        json!({"role": "tool", "tool_call_id": "c0", "content": "ok"}),
    ];
    let out = collapse_system_messages_to_head(input);

    let roles: Vec<&str> = out.iter().filter_map(|m| m["role"].as_str()).collect();
    assert_eq!(roles, vec!["system", "user", "assistant", "tool"]);
    assert_eq!(out[0]["content"], "S1\n\nNotice");
}

#[test]
fn responses_request_to_chat_passes_reasoning_content_back_to_assistant_message() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "reasoning",
                "summary": [
                    {"type": "summary_text", "text": "Need to inspect the repo."}
                ]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": "I will check the files."}
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "I will check the files.");
    assert_eq!(
        messages[0]["reasoning_content"],
        "Need to inspect the repo."
    );
    assert_eq!(messages[1]["role"], "user");
    assert!(messages[1].get("reasoning_content").is_none());
}

#[test]
fn responses_request_to_chat_attaches_trailing_reasoning_to_previous_assistant() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": "I checked the files."
            },
            {
                "type": "reasoning",
                "summary": [
                    {"type": "summary_text", "text": "The answer came from README."}
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "I checked the files.");
    assert_eq!(
        messages[0]["reasoning_content"],
        "The answer came from README."
    );
    assert_eq!(messages[1]["role"], "user");
    assert!(messages[1].get("reasoning_content").is_none());
}

#[test]
fn responses_request_to_chat_keeps_embedded_assistant_reasoning() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "reasoning_content": "I need to preserve thinking history.",
                "content": "Done."
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "Done.");
    assert_eq!(
        messages[0]["reasoning_content"],
        "I need to preserve thinking history."
    );
}

#[test]
fn responses_request_to_chat_preserves_trailing_reasoning_after_embedded_reasoning() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "reasoning_content": "Embedded thought.",
                "content": "Done."
            },
            {
                "type": "reasoning",
                "summary": [
                    {"type": "summary_text", "text": "Trailing thought."}
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "Done.");
    assert_eq!(
        messages[0]["reasoning_content"],
        "Embedded thought.\n\nTrailing thought."
    );
    assert_eq!(messages[1]["role"], "user");
    assert!(messages[1].get("reasoning_content").is_none());
}

#[test]
fn responses_request_to_chat_attaches_reasoning_to_tool_call_message() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "reasoning",
                "summary": "Need to read a file."
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[1]["role"], "tool");
}

#[test]
fn responses_request_to_chat_recovers_reasoning_from_function_call_item() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}",
                "reasoning_content": "Need to read a file."
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
    assert_eq!(messages[1]["role"], "tool");
}

#[test]
fn responses_request_to_chat_injects_placeholder_reasoning_for_bare_tool_call() {
    // 历史恢复 miss 时，带 tool_calls 的 assistant 消息没有任何可用 reasoning，
    // 必须补占位，否则 kimi/Moonshot thinking 模型会拒绝整个请求。
    let input = json!({
        "model": "kimi-k2-thinking",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "tool call");
    assert_eq!(messages[1]["role"], "tool");
}

#[test]
fn responses_request_to_chat_attaches_trailing_reasoning_to_tool_call_message() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            },
            {
                "type": "reasoning",
                "summary": "Need to read a file."
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
    assert_eq!(messages[1]["role"], "tool");
}

#[test]
fn responses_request_to_chat_attaches_reasoning_forward_to_following_assistant() {
    // 回归：reasoning 必须前向附挂到其后的 assistant 消息，不得回溯拼进
    // 上一条 assistant。此前多轮序列 [r1, m1, r2, m2] 中 r2 会被拼到 m1
    // 尾部、 m2 丢失 reasoning_content，思考型模型（kimi 等）因此中途"断片"。
    let input = json!({
        "model": "kimi-k2-thinking",
        "input": [
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "first thought"}]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": "First answer."
            },
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "second thought"}]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": "Second answer."
            },
            {
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "First answer.");
    assert_eq!(messages[0]["reasoning_content"], "first thought");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Second answer.");
    assert_eq!(messages[1]["reasoning_content"], "second thought");
    assert_eq!(messages[2]["role"], "user");
    assert!(messages[2].get("reasoning_content").is_none());
}

#[test]
fn responses_request_to_chat_keeps_reasoning_on_final_answer_after_tool_call() {
    // 回归（Kimi 契约）：[reasoning, function_call, output, reasoning, message]
    // 最后一个纯文本 assistant 必须保留自己的 reasoning_content，且该 reasoning
    // 不得被回溯拼进前面的 tool-call 消息（否则上游历史里 tool-call 消息的思考
    // 被污染、最终答复消息反而没有 reasoning_content）。
    let input = json!({
        "model": "kimi-k2-thinking",
        "input": [
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "need to read a file"}]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            },
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "now I can answer"}]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": "The file says hello."
            },
            {
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "need to read a file");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(messages[2]["content"], "The file says hello.");
    assert_eq!(messages[2]["reasoning_content"], "now I can answer");
    assert_eq!(messages[3]["role"], "user");
    assert!(messages[3].get("reasoning_content").is_none());
}

#[test]
fn responses_request_to_chat_keeps_multiple_tool_calls_adjacent_to_outputs() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call",
                "call_id": "call_2",
                "name": "list_files",
                "arguments": "{\"path\":\"src\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "Readme content"
            },
            {
                "type": "function_call_output",
                "call_id": "call_2",
                "output": ["main.rs", "lib.rs"]
            },
            {
                "role": "user",
                "content": "Continue"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["tool_calls"][1]["id"], "call_2");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_1");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_2");
    assert_eq!(messages[2]["content"], "[\"main.rs\",\"lib.rs\"]");
    assert_eq!(messages[3]["role"], "user");
}

#[test]
fn responses_request_to_chat_canonicalizes_json_string_tool_payloads() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{ \"b\": 2, \"a\": 1 }"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "{ \"z\": true, \"a\": [2, 1] }"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(
        messages[0]["tool_calls"][0]["function"]["arguments"],
        r#"{"a":1,"b":2}"#
    );
    assert_eq!(messages[1]["content"], r#"{"a":[2,1],"z":true}"#);
}

#[test]
fn responses_request_to_chat_preserves_plain_text_tool_output() {
    let input = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "plain text result"
            }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(messages[0]["tool_calls"][0]["function"]["arguments"], "{}");
    assert_eq!(messages[1]["content"], "plain text result");
}

// Synthetic history: a model stopped in the middle of a JSON string and
// the client retained the failed call before asking the model to continue.
fn malformed_history_request(arguments: Value) -> Value {
    json!({
        "model": "test-model",
        "input": [
            {"role": "assistant", "content": "Looking up the result."},
            {"type": "function_call", "call_id": "call_broken", "name": "lookup",
             "arguments": arguments, "reasoning_content": "Existing reasoning."},
            {"type": "function_call_output", "call_id": "call_broken",
             "output": "failed to parse function arguments: unexpected end of input"},
            {"role": "user", "content": "continue"}
        ],
        "tools": [{"type": "function", "name": "lookup", "parameters": {
            "type": "object", "properties": {"query": {"type": "string"}}
        }}]
    })
}

#[test]
fn malformed_history_arguments_preserve_attempt_and_error_as_text() {
    let raw = r#"{"query":"unfinished"#;
    let result = responses_to_chat_completions_with_reasoning(
        malformed_history_request(json!(raw)),
        None,
        true,
    )
    .unwrap();
    let messages = result_messages(&result);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "assistant");
    assert!(messages[0].get("tool_calls").is_none());
    let text = messages[0]["content"].as_str().unwrap();
    for preserved in [
        "Looking up the result.",
        "lookup",
        raw,
        "failed to parse function arguments",
    ] {
        assert!(text.contains(preserved), "missing context: {preserved}");
    }
    assert!(messages[0]["reasoning_content"]
        .as_str()
        .unwrap()
        .contains("Existing reasoning."));
    assert_eq!(messages[1], json!({"role": "user", "content": "continue"}));
    assert_eq!(result["tools"][0]["function"]["name"], "lookup");
}

#[test]
fn malformed_history_arguments_recover_non_object_json_without_guessing() {
    for arguments in ["not json", "[]", "42", "true", "null", "\"text\""] {
        let result = responses_to_chat_completions_with_reasoning(
            malformed_history_request(json!(arguments)),
            None,
            true,
        )
        .unwrap();
        let messages = result_messages(&result);
        assert!(messages
            .iter()
            .all(|message| message.get("tool_calls").is_none()));
        assert!(messages.iter().all(|message| message["role"] != "tool"));
        assert!(messages[0]["content"].as_str().unwrap().contains(arguments));
    }
}

#[test]
fn malformed_history_arguments_keep_valid_parallel_calls_paired() {
    for bad_result_first in [true, false] {
        let mut body = malformed_history_request(json!("{broken"));
        let input = body["input"].as_array_mut().unwrap();
        input.insert(
            2,
            json!({"type": "function_call", "call_id": "call_ok", "name": "lookup",
            "arguments": "{\"query\":\"weather\"}"}),
        );
        input.insert(
            if bad_result_first { 4 } else { 3 },
            json!({
                "type": "function_call_output", "call_id": "call_ok", "output": "sunny"
            }),
        );
        let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
        let messages = result_messages(&result);
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0]["tool_calls"],
            json!([{
                "id": "call_ok", "type": "function", "function": {
                    "name": "lookup", "arguments": "{\"query\":\"weather\"}"
                }
            }])
        );
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("failed to parse function arguments"));
        assert_eq!(
            messages[1],
            json!({"role": "tool", "tool_call_id": "call_ok", "content": "sunny"})
        );
        assert_eq!(messages[2]["role"], "user");
    }
}

#[test]
fn malformed_history_arguments_do_not_affect_valid_objects_or_no_arg_calls() {
    for arguments in [
        json!("{\"query\":\"weather\"}"),
        json!({"query": "weather"}),
        json!("{}"),
        json!(""),
    ] {
        let result = responses_to_chat_completions_with_reasoning(
            malformed_history_request(arguments),
            None,
            true,
        )
        .unwrap();
        let messages = result_messages(&result);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_broken");
        let args = messages[0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(serde_json::from_str::<Value>(args).unwrap().is_object());
        assert_eq!(messages[1]["role"], "tool");
    }
}

#[test]
fn malformed_history_arguments_preserve_tool_result_media() {
    let mut body = malformed_history_request(json!("{broken"));
    let image_url = "data:image/png;base64,aGVsbG8=";
    body["input"][2]["output"] = json!([
        {"type": "input_text", "text": "failed to parse function arguments"},
        {"type": "input_image", "image_url": image_url}
    ]);
    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    let messages = result_messages(&result);
    assert!(messages
        .iter()
        .all(|message| message.get("tool_calls").is_none()));
    assert!(messages.iter().all(|message| message["role"] != "tool"));
    assert!(messages.iter().any(|message| message["role"] == "user"
        && message["content"].as_array().is_some_and(|parts| parts
            .iter()
            .any(
                |part| part.pointer("/image_url/url").and_then(Value::as_str) == Some(image_url)
            ))));
}

#[test]
fn malformed_history_arguments_without_a_result_remain_visible_as_context() {
    let mut body = malformed_history_request(json!("{broken"));
    body["input"].as_array_mut().unwrap().remove(2);
    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    let messages = result_messages(&result);
    assert!(messages
        .iter()
        .all(|message| message.get("tool_calls").is_none()));
    assert!(messages[0]["content"].as_str().unwrap().contains("{broken"));
    assert_eq!(messages.last().unwrap()["content"], "continue");
}

#[test]
fn responses_request_to_chat_moves_tool_image_to_synthetic_user_message() {
    let data_url = large_test_image_data_url();
    let result = convert_test_input(vec![
        test_function_call("call_image"),
        test_function_output(
            "call_image",
            json!([
                {"type": "input_text", "text": "screenshot follows"},
                {"type": "input_image", "image_url": data_url.clone()}
            ]),
        ),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool", "user"]);
    assert!(messages[1]["content"].is_string());
    let tool_content: Value =
        serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(tool_content[0]["text"], "screenshot follows");
    assert_eq!(tool_content[1]["type"], "text");
    assert_eq!(tool_content[1]["text"], TOOL_RESULT_MEDIA_MOVED_MARKER);
    assert!(!messages[1]["content"].as_str().unwrap().contains(&data_url));

    assert_eq!(
        messages[2]["content"][0]["text"],
        "[cc-switch: media output of tool call call_image]"
    );
    assert_eq!(messages[2]["content"][1]["type"], "image_url");
    assert_eq!(messages[2]["content"][1]["image_url"]["url"], data_url);
}

#[test]
fn responses_request_to_chat_groups_parallel_media_after_all_tool_outputs() {
    let first_url = large_test_image_data_url();
    let second_payload = "MCP_TOOL_MEDIA_SENTINEL";
    let result = convert_test_input(vec![
        test_function_call("call_1"),
        test_function_call("call_2"),
        test_function_output(
            "call_1",
            json!({"type": "input_image", "image_url": first_url.clone()}),
        ),
        json!({
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "keep outputs adjacent"}]
        }),
        test_function_output(
            "call_2",
            json!({
                "type": "image",
                "mimeType": "image/webp",
                "data": second_payload
            }),
        ),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["assistant", "tool", "tool", "user"]
    );
    assert_eq!(messages[0]["reasoning_content"], "keep outputs adjacent");
    assert_ne!(messages[0]["reasoning_content"], "tool call");
    assert_eq!(messages[1]["tool_call_id"], "call_1");
    assert_eq!(messages[2]["tool_call_id"], "call_2");
    assert!(messages[1]["content"].is_string());
    assert!(messages[2]["content"].is_string());
    assert!(!messages[1]["content"]
        .as_str()
        .unwrap()
        .contains(&first_url));
    assert!(!messages[2]["content"]
        .as_str()
        .unwrap()
        .contains(second_payload));
    assert_eq!(messages[3]["content"].as_array().unwrap().len(), 4);
    assert_eq!(
        messages[3]["content"][3]["image_url"]["url"],
        format!("data:image/webp;base64,{second_payload}")
    );
}

#[test]
fn responses_request_to_chat_flushes_media_before_next_tool_call_batch() {
    let result = convert_test_input(vec![
        test_function_call("call_1"),
        test_function_output(
            "call_1",
            json!({
                "type": "input_image",
                "image_url": large_test_image_data_url()
            }),
        ),
        test_function_call("call_2"),
        test_function_output("call_2", json!("second result")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["assistant", "tool", "user", "assistant", "tool"]
    );
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[3]["tool_calls"][0]["id"], "call_2");
    assert_eq!(messages[4]["tool_call_id"], "call_2");
}

#[test]
fn responses_request_to_chat_flushes_media_before_real_user_messages() {
    let boundaries = [
        json!({"type": "input_text", "text": "continue"}),
        json!({"type": "future_message", "role": "user", "content": "continue"}),
    ];

    for boundary in boundaries {
        let result = convert_test_input(vec![
            test_function_call("call_1"),
            test_function_output(
                "call_1",
                json!({
                    "type": "input_image",
                    "image_url": large_test_image_data_url()
                }),
            ),
            boundary,
        ]);
        let messages = result_messages(&result);

        assert_eq!(
            message_roles(&result),
            vec!["assistant", "tool", "user", "user"]
        );
        assert!(messages[2]["content"].is_array());
        assert_eq!(messages[3]["role"], "user");
    }
}

#[test]
fn responses_request_to_chat_handles_raw_data_url_thresholds() {
    let large = large_test_image_data_url();
    let large_result = convert_test_input(vec![
        test_function_call("call_large"),
        test_function_output("call_large", Value::String(large.clone())),
    ]);
    let large_messages = result_messages(&large_result);

    assert_eq!(
        message_roles(&large_result),
        vec!["assistant", "tool", "user"]
    );
    assert_eq!(large_messages[1]["content"], TOOL_RESULT_MEDIA_MOVED_MARKER);
    assert_eq!(large_messages[2]["content"][1]["image_url"]["url"], large);

    let small = "data:image/png;base64,YWJj";
    let small_result = convert_test_input(vec![
        test_function_call("call_small"),
        test_function_output("call_small", json!(small)),
    ]);
    assert_eq!(message_roles(&small_result), vec!["assistant", "tool"]);
    assert_eq!(small_result["messages"][1]["content"], small);
}

#[test]
fn responses_request_to_chat_maps_supported_structured_image_shapes() {
    let cases = vec![
        (
            json!({
                "type": "input_image",
                "image_url": {"url": "https://example.com/input.png"},
                "detail": "high"
            }),
            "https://example.com/input.png",
            Some("high"),
        ),
        (
            json!({
                "type": "image_url",
                "image_url": "https://example.com/chat-string.png"
            }),
            "https://example.com/chat-string.png",
            None,
        ),
        (
            json!({
                "type": "image_url",
                "image_url": {
                    "url": "https://example.com/chat-object.png",
                    "detail": "low"
                }
            }),
            "https://example.com/chat-object.png",
            Some("low"),
        ),
        (
            json!({"image_url": "data:image/gif;base64,LOOSE_SENTINEL"}),
            "data:image/gif;base64,LOOSE_SENTINEL",
            None,
        ),
        (
            json!({
                "type": "image",
                "source": {
                    "media_type": "image/jpeg",
                    "data": "ANTHROPIC_SENTINEL"
                }
            }),
            "data:image/jpeg;base64,ANTHROPIC_SENTINEL",
            None,
        ),
        (
            json!({
                "type": "image",
                "mimeType": "image/webp",
                "data": "MCP_SENTINEL"
            }),
            "data:image/webp;base64,MCP_SENTINEL",
            None,
        ),
    ];

    for (index, (output, expected_url, expected_detail)) in cases.into_iter().enumerate() {
        let call_id = format!("call_shape_{index}");
        let result = convert_test_input(vec![
            test_function_call(&call_id),
            test_function_output(&call_id, output),
        ]);
        let image = &result["messages"][2]["content"][1];

        assert_eq!(message_roles(&result), vec!["assistant", "tool", "user"]);
        assert_eq!(image["type"], "image_url");
        assert_eq!(image["image_url"]["url"], expected_url);
        match expected_detail {
            Some(detail) => assert_eq!(image["image_url"]["detail"], detail),
            None => assert!(image["image_url"].get("detail").is_none()),
        }
    }
}

#[test]
fn responses_request_to_chat_extracts_media_from_json_string_and_content_wrapper() {
    let output = json!({
        "content": [
            {"type": "input_text", "text": "MCP response"},
            {
                "type": "image",
                "mimeType": "image/png",
                "data": "STRING_MCP_SENTINEL"
            }
        ]
    })
    .to_string();
    let result = convert_test_input(vec![
        test_function_call("call_string"),
        test_function_output("call_string", Value::String(output)),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool", "user"]);
    let tool_content: Value =
        serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(tool_content["content"][0]["text"], "MCP response");
    assert_eq!(
        tool_content["content"][1]["text"],
        TOOL_RESULT_MEDIA_MOVED_MARKER
    );
    assert!(!messages[1]["content"]
        .as_str()
        .unwrap()
        .contains("STRING_MCP_SENTINEL"));
    assert_eq!(
        messages[2]["content"][1]["image_url"]["url"],
        "data:image/png;base64,STRING_MCP_SENTINEL"
    );
}

#[test]
fn responses_request_to_chat_extracts_tool_files_and_audio() {
    let result = convert_test_input(vec![
        test_function_call("call_media"),
        test_function_output(
            "call_media",
            json!({
                "content": [
                    {
                        "type": "input_file",
                        "file_id": "file_123",
                        "filename": "report.pdf"
                    },
                    {
                        "type": "input_audio",
                        "input_audio": {"data": "AUDIO_SENTINEL", "format": "wav"}
                    }
                ]
            }),
        ),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool", "user"]);
    assert_eq!(messages[2]["content"][1]["type"], "file");
    assert_eq!(messages[2]["content"][1]["file"]["file_id"], "file_123");
    assert_eq!(messages[2]["content"][2]["type"], "input_audio");
    assert_eq!(
        messages[2]["content"][2]["input_audio"]["data"],
        "AUDIO_SENTINEL"
    );
}

#[test]
fn responses_request_to_chat_extracts_custom_and_tool_search_output_media() {
    let cases = [
        (
            json!({
                "type": "custom_tool_call",
                "call_id": "call_custom",
                "name": "render",
                "input": "draw"
            }),
            json!({
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "status": "completed",
                "output": {
                    "content": [{
                        "type": "input_image",
                        "image_url": "data:image/png;base64,CUSTOM_SENTINEL"
                    }]
                }
            }),
        ),
        (
            json!({
                "type": "tool_search_call",
                "call_id": "call_search",
                "arguments": {"query": "image tool"}
            }),
            json!({
                "type": "tool_search_output",
                "call_id": "call_search",
                "status": "completed",
                "output": {
                    "content": [{
                        "type": "image",
                        "mimeType": "image/png",
                        "data": "SEARCH_SENTINEL"
                    }]
                }
            }),
        ),
    ];

    for (call, output) in cases {
        let expected_type = output["type"].as_str().unwrap().to_string();
        let result = convert_test_input(vec![call, output]);
        let messages = result_messages(&result);
        let tool_content: Value =
            serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();

        assert_eq!(message_roles(&result), vec!["assistant", "tool", "user"]);
        assert_eq!(tool_content["type"], expected_type);
        assert_eq!(tool_content["status"], "completed");
        assert_eq!(
            tool_content["output"]["content"][0]["text"],
            TOOL_RESULT_MEDIA_MOVED_MARKER
        );
    }
}

#[test]
fn responses_request_to_chat_clamps_stringified_custom_output_residual_base64() {
    let encoded_output = json!({
        "content": [
            {
                "type": "input_image",
                "image_url": "data:image/png;base64,CUSTOM_STRING_IMAGE_SENTINEL"
            },
            {
                "type": "video",
                "data": "A".repeat(20_000)
            }
        ]
    })
    .to_string();
    let result = convert_test_input(vec![
        json!({
            "type": "custom_tool_call",
            "call_id": "call_custom_string",
            "name": "render",
            "input": "draw"
        }),
        json!({
            "type": "custom_tool_call_output",
            "call_id": "call_custom_string",
            "status": "completed",
            "output": encoded_output
        }),
    ]);
    let messages = result_messages(&result);
    let tool_item: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
    let rewritten = tool_item["output"].as_str().unwrap();

    assert!(rewritten.contains("[cc-switch: omitted 20000 bytes]"));
    assert!(!rewritten.contains(&"A".repeat(64)));
    assert!(!rewritten.contains("CUSTOM_STRING_IMAGE_SENTINEL"));
    assert_eq!(messages[2]["content"][1]["type"], "image_url");
}

#[test]
fn responses_request_to_chat_rejects_false_positive_media_shapes() {
    let outputs = [
        json!({"type": "image", "name": "business metadata"}),
        json!({
            "type": "image",
            "mimeType": "text/plain",
            "data": "NOT_AN_IMAGE"
        }),
        json!({
            "image_url": {
                "url": "https://example.com/search-thumbnail.png"
            }
        }),
    ];

    for (index, output) in outputs.into_iter().enumerate() {
        let call_id = format!("call_false_positive_{index}");
        let expected = canonical_json_string(&output);
        let result = convert_test_input(vec![
            test_function_call(&call_id),
            test_function_output(&call_id, output),
        ]);

        assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
        assert_eq!(result["messages"][1]["content"], expected);
    }
}

#[test]
fn responses_request_to_chat_keeps_no_media_tool_output_bytes_stable() {
    let cases = [
        (Some(json!("plain text")), "plain text".to_string()),
        (
            Some(json!("{ \"z\": true, \"a\": [2, 1] }")),
            r#"{"a":[2,1],"z":true}"#.to_string(),
        ),
        (
            Some(json!(["main.rs", "lib.rs"])),
            r#"["main.rs","lib.rs"]"#.to_string(),
        ),
        (
            Some(json!({"z": true, "a": [2, 1]})),
            r#"{"a":[2,1],"z":true}"#.to_string(),
        ),
        (Some(json!([])), "[]".to_string()),
        (None, String::new()),
    ];

    for (index, (output, expected)) in cases.into_iter().enumerate() {
        let call_id = format!("call_stable_{index}");
        let mut item = json!({
            "type": "function_call_output",
            "call_id": call_id
        });
        if let Some(output) = output {
            item["output"] = output;
        }
        let result = convert_test_input(vec![test_function_call(&call_id), item]);

        assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
        assert_eq!(result["messages"][1]["content"], expected);
    }

    // 输出必须与前面的 call 配对，否则会被降级成 user 消息（孤儿清洗）。
    for (call, item) in [
        (
            json!({
                "type": "custom_tool_call",
                "call_id": "call_custom",
                "name": "render",
                "input": "draw"
            }),
            json!({
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "status": "completed",
                "output": {"text": "unchanged"}
            }),
        ),
        (
            json!({
                "type": "tool_search_call",
                "call_id": "call_search",
                "arguments": {"query": "noop"}
            }),
            json!({
                "type": "tool_search_output",
                "call_id": "call_search",
                "status": "completed",
                "output": []
            }),
        ),
    ] {
        let expected = canonical_json_string(&item);
        let result = convert_test_input(vec![call, item]);
        let messages = result_messages(&result);
        assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
        assert_eq!(messages[1]["content"], expected);
    }
}

#[test]
fn responses_request_to_chat_preserves_legacy_unknown_item_batch_boundary_without_media() {
    let result = convert_test_input(vec![
        test_function_call("call_1"),
        json!({"type": "future_metadata", "value": 1}),
        json!({
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "second batch reasoning"}]
        }),
        test_function_call("call_2"),
        test_function_output("call_1", json!("first result")),
        test_function_output("call_2", json!("second result")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["assistant", "assistant", "tool", "tool"]
    );
    assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "tool call");
    assert_eq!(messages[1]["tool_calls"].as_array().unwrap().len(), 1);
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_2");
    assert_eq!(messages[1]["reasoning_content"], "second batch reasoning");
}

#[test]
fn responses_request_to_chat_coalesces_adjacent_commentary_with_tool_calls() {
    let mut call = test_function_call("call_1");
    call["reasoning_content"] = json!("need to update the file");

    let result = convert_test_input(vec![
        json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "need to update the file"}
            ]
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Part 1 written. Appending sections 4-5."}
            ]
        }),
        call,
        test_function_output("call_1", json!("Success")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
    assert_eq!(
        messages[0]["content"],
        "Part 1 written. Appending sections 4-5."
    );
    let calls = messages[0]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "need to update the file");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_1");
}

#[test]
fn responses_request_to_chat_deduplicates_repeated_call_reasoning_segments() {
    let mut call_1 = test_function_call("call_1");
    call_1["reasoning_content"] = json!("need to update the file");
    let mut call_2 = test_function_call("call_2");
    call_2["reasoning_content"] = json!("second section planning");

    let result = convert_test_input(vec![
        json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "need to update the file"}
            ]
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Part 1 written. Appending sections 4-5."}
            ]
        }),
        call_1,
        call_2,
        test_function_output("call_1", json!("Success 1")),
        test_function_output("call_2", json!("Success 2")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool", "tool"]);
    assert_eq!(
        messages[0]["reasoning_content"],
        "need to update the file\n\nsecond section planning"
    );
}

#[test]
fn responses_request_to_chat_keeps_user_boundary_before_tool_call_turn() {
    let result = convert_test_input(vec![
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Done for now."}
            ]
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": "Continue with the next file."
        }),
        test_function_call("call_next"),
        test_function_output("call_next", json!("Next result")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["assistant", "user", "assistant", "tool"]
    );
    assert!(messages[0].get("tool_calls").is_none());
    assert_eq!(messages[0]["content"], "Done for now.");
    assert_eq!(messages[2]["content"], Value::Null);
    assert_eq!(messages[2]["tool_calls"][0]["id"], "call_next");
}

#[test]
fn responses_request_to_chat_coalesces_when_reasoning_sits_between_commentary_and_call() {
    let mut call = test_function_call("call_1");
    call["reasoning_content"] = json!("need to update the file");

    let result = convert_test_input(vec![
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Part 1 written. Appending sections 4-5."}
            ]
        }),
        json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "need to update the file"}
            ]
        }),
        call,
        test_function_output("call_1", json!("Success")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
    assert_eq!(
        messages[0]["content"],
        "Part 1 written. Appending sections 4-5."
    );
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[0]["reasoning_content"], "need to update the file");
}

#[test]
fn responses_request_to_chat_backfills_reasoning_placeholder_for_coalesced_call() {
    let result = convert_test_input(vec![
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Running the build now."}
            ]
        }),
        test_function_call("call_build"),
        test_function_output("call_build", json!("Build succeeded")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
    assert_eq!(messages[0]["content"], "Running the build now.");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_build");
    assert_eq!(messages[0]["reasoning_content"], "tool call");
}

#[test]
fn responses_request_to_chat_coalesces_custom_tool_call_with_commentary() {
    let input = json!({
        "model": "kimi-k3",
        "tools": [{
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply a patch to files."
        }],
        "input": [
            json!({
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": "Applying the next patch."}
                ]
            }),
            json!({
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch"
            }),
            json!({
                "type": "custom_tool_call_output",
                "call_id": "call_patch",
                "output": {"text": "Success"}
            })
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();

    assert_eq!(message_roles(&result), vec!["assistant", "tool"]);
    assert_eq!(messages[0]["content"], "Applying the next patch.");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_patch");
    assert_eq!(messages[1]["tool_call_id"], "call_patch");
}

#[test]
fn responses_request_to_chat_keeps_media_before_coalesced_commentary_tool_call() {
    let data_url = large_test_image_data_url();
    let result = convert_test_input(vec![
        test_function_call("call_media"),
        test_function_output(
            "call_media",
            json!({
                "content": [{
                    "type": "input_image",
                    "image_url": data_url
                }]
            }),
        ),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Viewing the image now."}
            ]
        }),
        test_function_call("call_next"),
        test_function_output("call_next", json!("Next result")),
    ]);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec!["assistant", "tool", "user", "assistant", "tool"]
    );
    assert_eq!(messages[0]["tool_calls"][0]["id"], "call_media");
    let media_content = messages[2]["content"].as_array().unwrap();
    assert!(media_content.iter().any(|part| part
        .get("image_url")
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .is_some_and(|url| url == data_url)));
    assert_eq!(messages[3]["content"], "Viewing the image now.");
    assert_eq!(messages[3]["tool_calls"][0]["id"], "call_next");
    assert_eq!(messages[4]["tool_call_id"], "call_next");
}

#[test]
fn responses_request_to_chat_multi_round_history_has_no_text_only_assistant_turns() {
    let mut items = vec![json!({
        "type": "message",
        "role": "user",
        "content": "Fix the failing build."
    })];
    for index in 1..=3 {
        let call_id = format!("call_round_{index}");
        items.push(json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": format!("round {index} reasoning")}
            ]
        }));
        items.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": format!("Step {index}: checking the logs.")}
            ]
        }));
        let mut call = test_function_call(&call_id);
        call["reasoning_content"] = json!(format!("round {index} reasoning"));
        items.push(call);
        items.push(test_function_output(
            &call_id,
            json!(format!("round {index} result")),
        ));
    }
    items.push(json!({
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "output_text", "text": "Build fixed."}
        ]
    }));

    let result = convert_test_input(items);
    let messages = result_messages(&result);

    assert_eq!(
        message_roles(&result),
        vec![
            "user",
            "assistant",
            "tool",
            "assistant",
            "tool",
            "assistant",
            "tool",
            "assistant"
        ]
    );
    for (index, message) in messages.iter().enumerate() {
        if message["role"] == "assistant" && index + 1 < messages.len() {
            assert!(
                message["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| !calls.is_empty()),
                "assistant at index {index} must carry tool calls"
            );
        }
    }
    assert_eq!(messages[1]["content"], "Step 1: checking the logs.");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_round_1");
    assert_eq!(messages[1]["reasoning_content"], "round 1 reasoning");
    assert_eq!(messages[7]["content"], "Build fixed.");
    assert!(messages[7].get("tool_calls").is_none());
}

#[test]
fn responses_request_to_chat_clamps_only_residual_base64ish_strings() {
    let data_url = large_test_image_data_url();
    let long_text = format!("{}end", "ordinary OCR text with spaces. ".repeat(3_500));
    let residual_base64 = "A".repeat(20_000);
    let encoded_output = json!([
        {"type": "input_image", "image_url": data_url.clone()},
        {"type": "text", "text": long_text.clone()},
        {"type": "video", "data": residual_base64}
    ])
    .to_string();
    let result = convert_test_input(vec![
        test_function_call("call_clamp"),
        test_function_output("call_clamp", json!(encoded_output)),
    ]);
    let tool_content_text = result["messages"][1]["content"].as_str().unwrap();
    let tool_content: Value = serde_json::from_str(tool_content_text).unwrap();

    assert_eq!(tool_content[1]["text"], long_text);
    assert!(tool_content[2]["data"]
        .as_str()
        .unwrap()
        .starts_with("[cc-switch: omitted 20000 bytes]"));
    assert!(!tool_content_text.contains(&data_url));
    assert!(!tool_content_text.contains("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
}

#[test]
fn responses_request_to_chat_media_conversion_is_deterministic() {
    let input = json!({
        "model": "kimi-k3",
        "input": [
            test_function_call("call_repeat"),
            test_function_output(
                "call_repeat",
                json!({
                    "content": [{
                        "type": "input_image",
                        "image_url": large_test_image_data_url()
                    }]
                })
            )
        ]
    });

    let first = responses_to_chat_completions_with_reasoning(input.clone(), None, true).unwrap();
    let second = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert_eq!(first, second);
}

#[test]
fn chat_usage_to_responses_includes_required_input_token_details() {
    let usage = json!({
        "prompt_tokens": 13,
        "completion_tokens": 245,
        "total_tokens": 258,
        "prompt_tokens_details": { "cached_tokens": 0 }
    });

    let converted = chat_usage_to_responses_usage(Some(&usage));
    assert_eq!(
        converted["input_tokens_details"],
        json!({ "cached_tokens": 0 })
    );

    let fallback = chat_usage_to_responses_usage(None);
    assert_eq!(
        fallback["input_tokens_details"],
        json!({ "cached_tokens": 0 })
    );
}

#[test]
fn chat_usage_to_responses_resolves_cache_read_precedence() {
    let direct_cache_usage = json!({
        "prompt_tokens": 100,
        "completion_tokens": 5,
        "prompt_tokens_details": { "cached_tokens": 0 },
        "cache_read_input_tokens": 40
    });
    let converted = chat_usage_to_responses_usage(Some(&direct_cache_usage));
    assert_eq!(converted["input_tokens_details"]["cached_tokens"], 40);
    assert_eq!(converted["cache_read_input_tokens"], 40);

    // #8041 行为反转：显式 0 按「未上报」处理让位给嵌套标准字段（与
    // usage/parser.rs、流式 extract_cache_read_tokens 同规则）；重建对象
    // 只镜像非 0 直传字段，桩 0 不再上抛。
    let direct_zero = json!({
        "prompt_tokens_details": { "cached_tokens": 12 },
        "cache_read_input_tokens": 0
    });
    let converted = chat_usage_to_responses_usage(Some(&direct_zero));
    assert_eq!(converted["input_tokens_details"]["cached_tokens"], 12);
    assert!(converted.get("cache_read_input_tokens").is_none());

    let invalid_direct = json!({
        "prompt_tokens_details": { "cached_tokens": 12 },
        "cache_read_input_tokens": "invalid"
    });
    let converted = chat_usage_to_responses_usage(Some(&invalid_direct));
    assert_eq!(converted["input_tokens_details"]["cached_tokens"], 12);
    assert!(converted.get("cache_read_input_tokens").is_none());

    let invalid_prompt_details = json!({
        "prompt_tokens_details": { "cached_tokens": "invalid" },
        "input_tokens_details": { "cached_tokens": 7 }
    });
    let converted = chat_usage_to_responses_usage(Some(&invalid_prompt_details));
    assert_eq!(converted["input_tokens_details"]["cached_tokens"], 7);
}

#[test]
fn chat_response_to_responses_maps_text_tool_calls_and_usage() {
    let input = json!({
        "id": "chatcmpl_1",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "message": {
                "role": "assistant",
                "reasoning_content": "I should check the weather before answering.",
                "content": "Let me check.",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"city\":\"Tokyo\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "prompt_tokens_details": {"cached_tokens": 3, "cache_write_tokens": 2}
        }
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["id"], "resp_chatcmpl_1");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["output"][0]["type"], "reasoning");
    assert_eq!(
        result["output"][0]["summary"][0]["text"],
        "I should check the weather before answering."
    );
    assert_eq!(result["output"][1]["type"], "message");
    assert_eq!(result["output"][1]["id"], "msg_chatcmpl_1");
    assert_eq!(result["output"][1]["content"][0]["text"], "Let me check.");
    assert_eq!(result["output"][2]["type"], "function_call");
    assert_eq!(result["output"][2]["call_id"], "call_1");
    assert_eq!(
        result["output"][2]["reasoning_content"],
        "I should check the weather before answering."
    );
    assert_eq!(result["usage"]["input_tokens"], 10);
    assert_eq!(result["usage"]["output_tokens"], 5);
    assert_eq!(result["usage"]["input_tokens_details"]["cached_tokens"], 3);
    assert_eq!(
        result["usage"]["input_tokens_details"]["cache_write_tokens"],
        2
    );
}

#[test]
fn message_item_ids_use_msg_prefix() {
    // Regression test for #9054: deriving the message item ID as
    // `{response_id}_msg` yields `resp_..._msg`, which strict Responses
    // upstreams reject with `invalid_id_prefix` when the stored thread is
    // replayed under an official/native provider.
    assert_eq!(
        response_message_item_id("resp_chatcmpl_1"),
        "msg_chatcmpl_1"
    );
    assert_eq!(
        response_message_item_id("resp_8dd313c3703bd26593ad5912844d12f8"),
        "msg_8dd313c3703bd26593ad5912844d12f8"
    );
    assert_eq!(
        response_message_item_id_indexed("resp_msg_1", 0),
        "msg_msg_1_0"
    );
    // IDs without a `resp_` prefix still gain the required `msg_` prefix.
    assert_eq!(response_message_item_id("resp_ccswitch"), "msg_ccswitch");
    assert_eq!(response_message_item_id("ccswitch"), "msg_ccswitch");
}

#[test]
fn chat_response_to_responses_restores_loaded_namespace_tool_call() {
    let request = json!({
        "model": "gpt-5.4",
        "tools": [{"type": "tool_search"}],
        "input": [{
            "type": "tool_search_output",
            "call_id": "call_tool_search_1",
            "status": "completed",
            "execution": "client",
            "tools": [{
                "type": "namespace",
                "name": "mcp__codex_apps__gmail",
                "description": "Find and reference emails from your inbox.",
                "tools": [{
                    "type": "function",
                    "name": "_search_emails",
                    "description": "Search Gmail for emails matching a query.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"},
                            "label_ids": {"type": "array", "items": {"type": "string"}},
                            "max_results": {"type": "integer"}
                        }
                    }
                }]
            }]
        }]
    });
    let context = build_codex_tool_context_from_request(&request);
    let chat = json!({
        "id": "chatcmpl_gmail",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_gmail",
                    "type": "function",
                    "function": {
                        "name": "mcp__codex_apps__gmail___search_emails",
                        "arguments": "{\"query\":\"-in:spam -in:trash\",\"label_ids\":[\"UNREAD\"],\"max_results\":5}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let result = chat_completion_to_response_with_context(chat, &context).unwrap();

    assert_eq!(result["output"][0]["type"], "function_call");
    assert_eq!(result["output"][0]["call_id"], "call_gmail");
    assert_eq!(result["output"][0]["namespace"], "mcp__codex_apps__gmail");
    assert_eq!(result["output"][0]["name"], "_search_emails");
    assert_eq!(
        result["output"][0]["arguments"],
        r#"{"label_ids":["UNREAD"],"max_results":5,"query":"-in:spam -in:trash"}"#
    );
}

#[test]
fn chat_response_to_responses_restores_tool_search_call() {
    let request = json!({
        "model": "gpt-5.4",
        "tools": [{"type": "tool_search"}],
        "input": "Find tools."
    });
    let context = build_codex_tool_context_from_request(&request);
    let chat = json!({
        "id": "chatcmpl_tool_search",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_tool_search_1",
                    "type": "function",
                    "function": {
                        "name": "tool_search",
                        "arguments": "{\"query\":\"Gmail search emails\",\"limit\":10}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let result = chat_completion_to_response_with_context(chat, &context).unwrap();

    assert_eq!(result["output"][0]["type"], "tool_search_call");
    assert_eq!(result["output"][0]["call_id"], "call_tool_search_1");
    assert_eq!(result["output"][0]["execution"], "client");
    assert_eq!(
        result["output"][0]["arguments"]["query"],
        "Gmail search emails"
    );
    assert_eq!(result["output"][0]["arguments"]["limit"], 10);
}

#[test]
fn chat_response_to_responses_restores_custom_tool_call() {
    let request = json!({
        "model": "gpt-5.4",
        "tools": [{"type": "custom", "name": "apply_patch"}],
        "input": "Patch it."
    });
    let context = build_codex_tool_context_from_request(&request);
    let chat = json!({
        "id": "chatcmpl_custom",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_patch",
                    "type": "function",
                    "function": {
                        "name": "apply_patch",
                        "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let result = chat_completion_to_response_with_context(chat, &context).unwrap();

    assert_eq!(result["output"][0]["type"], "custom_tool_call");
    assert_eq!(result["output"][0]["id"], "ctc_call_patch");
    assert_eq!(result["output"][0]["call_id"], "call_patch");
    assert_eq!(result["output"][0]["name"], "apply_patch");
    assert_eq!(
        result["output"][0]["input"],
        "*** Begin Patch\n*** End Patch"
    );
}

/// #4341（非流式路径）：丢弃后一个工具调用都不剩时，必须如实报错，
/// 而不是返回一个 Codex 会当成正常完成的空壳回合。
#[test]
fn chat_response_with_only_unnamed_tool_call_is_an_error() {
    let chat = json!({
        "id": "chatcmpl_drop",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "让我继续处理这个文件",
                "tool_calls": [{
                    "id": "call_bad",
                    "type": "function",
                    "function": {"arguments": "{}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let err =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap_err();
    assert!(matches!(err, ProxyError::TransformError(_)));
    assert!(err.to_string().contains("without a function name"));
}

/// 只要还剩下一个合法工具调用，Codex 本来就会继续，行为保持不变。
#[test]
fn chat_response_keeps_valid_tool_call_beside_unnamed_one() {
    let chat = json!({
        "id": "chatcmpl_mixed",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {"id": "call_bad", "type": "function", "function": {"arguments": "{}"}},
                    {
                        "id": "call_good",
                        "type": "function",
                        "function": {"name": "exec_command", "arguments": "{\"cmd\":\"ls\"}"}
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let result =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap();
    let output = result["output"].as_array().unwrap();

    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["name"], "exec_command");
    assert_eq!(output[0]["call_id"], "call_good");
    assert_eq!(result["status"], "completed");
}

/// legacy `function_call` 形态同样受判据保护。
#[test]
fn chat_response_with_unnamed_legacy_function_call_is_an_error() {
    let chat = json!({
        "id": "chatcmpl_legacy",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {
                "role": "assistant",
                "function_call": {"id": "call_legacy", "arguments": "{}"}
            },
            "finish_reason": "function_call"
        }]
    });

    let err =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap_err();
    assert!(matches!(err, ProxyError::TransformError(_)));
}

/// `finish_reason=length` 是截断，不是"上游发了畸形数据"——归因必须保持
/// incomplete，不能报成 tool_call_dropped。
#[test]
fn chat_response_truncated_stays_incomplete_instead_of_error() {
    let chat = json!({
        "id": "chatcmpl_trunc",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "我来看看",
                "tool_calls": [{
                    "id": "call_cut",
                    "type": "function",
                    "function": {"arguments": "{\"pa"}
                }]
            },
            "finish_reason": "length"
        }]
    });

    let result =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap();
    assert_eq!(result["status"], "incomplete");
    assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
}

/// 纯空白函数名必须与空名同等对待，否则会伪装成"本回合还有工具调用"。
#[test]
fn chat_response_whitespace_only_tool_name_is_an_error() {
    let chat = json!({
        "id": "chatcmpl_ws",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_ws",
                    "type": "function",
                    "function": {"name": "   ", "arguments": "{}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let err =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap_err();
    assert!(matches!(err, ProxyError::TransformError(_)));
}

/// 纯文本回合（从未出现工具调用）不受判据影响。
#[test]
fn chat_response_text_only_still_completes() {
    let chat = json!({
        "id": "chatcmpl_text",
        "object": "chat.completion",
        "created": 123,
        "model": "kimi-k3",
        "choices": [{
            "message": {"role": "assistant", "content": "完成了"},
            "finish_reason": "stop"
        }]
    });

    let result =
        chat_completion_to_response_with_context(chat, &CodexToolContext::default()).unwrap();
    assert_eq!(result["status"], "completed");
}

#[test]
fn chat_response_to_responses_canonicalizes_json_string_tool_arguments() {
    let input = json!({
        "id": "chatcmpl_args",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "arguments": "{ \"b\": 2, \"a\": 1 }"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["output"][0]["type"], "function_call");
    assert_eq!(result["output"][0]["arguments"], r#"{"a":1,"b":2}"#);
}

#[test]
fn chat_response_to_responses_splits_inline_think_content() {
    let input = json!({
        "id": "chatcmpl_think",
        "object": "chat.completion",
        "created": 123,
        "model": "MiniMax-M2.7",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "<think>\nI should answer with pong.\n</think>\n\npong"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30,
            "completion_tokens_details": {"reasoning_tokens": 18}
        }
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["output"][0]["type"], "reasoning");
    assert_eq!(
        result["output"][0]["summary"][0]["text"],
        "I should answer with pong."
    );
    assert_eq!(result["output"][1]["type"], "message");
    assert_eq!(result["output"][1]["content"][0]["text"], "pong");
    assert_eq!(
        result["usage"]["output_tokens_details"]["reasoning_tokens"],
        18
    );
}

#[test]
fn chat_response_length_maps_to_incomplete_response() {
    let input = json!({
        "id": "chatcmpl_2",
        "model": "gpt-5.4",
        "choices": [{
            "message": {"role": "assistant", "content": "partial"},
            "finish_reason": "length"
        }]
    });

    let result =
        chat_completion_to_response_with_context(input, &CodexToolContext::default()).unwrap();

    assert_eq!(result["status"], "incomplete");
    assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
}

#[test]
fn chat_error_to_response_error_normalizes_standard_openai_shape() {
    let input = json!({
        "error": {
            "message": "Invalid API key",
            "type": "invalid_request_error",
            "code": "invalid_api_key",
            "param": "api_key"
        }
    });

    let result = chat_error_to_response_error(Some(&input));

    assert_eq!(result["error"]["message"], "Invalid API key");
    assert_eq!(result["error"]["type"], "invalid_request_error");
    assert_eq!(result["error"]["code"], "invalid_api_key");
    assert_eq!(result["error"]["param"], "api_key");
}

#[test]
fn gemini_input_overflow_errors() {
    let message = "The input token count exceeds the maximum number of tokens allowed 1048576.";
    for input in [
        json!({ "error": { "code": 400, "status": "INVALID_ARGUMENT", "message": message } }),
        json!({ "error": { "type": "invalid_request_error", "message": message } }),
        json!({ "detail": message }),
        json!(message),
    ] {
        let result = chat_error_to_response_error(Some(&input));
        assert_eq!(result["error"]["code"], "context_length_exceeded");
        assert_eq!(result["error"]["message"], message);
    }
    let ordinary_error = json!({ "error": {
        "code": 400, "status": "INVALID_ARGUMENT",
        "message": "max_output_tokens exceeds the maximum number of tokens allowed 65536."
    } });
    assert_eq!(
        chat_error_to_response_error(Some(&ordinary_error))["error"]["code"],
        400
    );
}

#[test]
fn chat_error_to_response_error_normalizes_minimax_base_resp() {
    // MiniMax 把错误塞在 base_resp 里，code 是数字而不是字符串
    let input = json!({
        "base_resp": {
            "status_code": 2013,
            "status_msg": "invalid params, chat content has invalid message role: system"
        }
    });

    let result = chat_error_to_response_error(Some(&input));

    assert_eq!(
        result["error"]["message"],
        "invalid params, chat content has invalid message role: system"
    );
    assert_eq!(result["error"]["code"], 2013);
    // type 没有显式给出，应该回落到 upstream_error
    assert_eq!(result["error"]["type"], "upstream_error");
}

#[test]
fn chat_error_to_response_error_handles_plain_text_body() {
    let input = json!("Upstream timeout");

    let result = chat_error_to_response_error(Some(&input));

    assert_eq!(result["error"]["message"], "Upstream timeout");
    assert_eq!(result["error"]["type"], "upstream_error");
    assert!(result["error"]["code"].is_null());
    assert!(result["error"]["param"].is_null());
}

#[test]
fn chat_error_to_response_error_handles_missing_body() {
    let result = chat_error_to_response_error(None);

    assert_eq!(
        result["error"]["message"],
        "Upstream returned an empty error response"
    );
    assert_eq!(result["error"]["type"], "upstream_error");
}

#[test]
fn chat_error_to_response_error_falls_back_to_detail_field() {
    // 部分中转把错误塞在顶层 detail 字段（OpenAI 兼容层常见）
    let input = json!({
        "detail": "rate limit exceeded"
    });

    let result = chat_error_to_response_error(Some(&input));

    assert_eq!(result["error"]["message"], "rate limit exceeded");
    assert_eq!(result["error"]["type"], "upstream_error");
}
// Regression tests for tool_choice without tools guard
// https://github.com/farion1231/cc-switch/issues/3557

#[test]
fn responses_request_to_chat_drops_tool_choice_when_no_tools() {
    // When tools is absent from the request, tool_choice must be dropped
    // to avoid 503/400 from strict OpenAI-compatible upstreams.
    let input = json!({
        "model": "qwen3-7-max",
        "tool_choice": "auto",
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice should be dropped when tools is absent"
    );
    assert!(result.get("tools").is_none(), "tools should be absent");
    assert_eq!(result["model"], "qwen3-7-max");
}

#[test]
fn responses_request_to_chat_drops_tool_choice_when_tools_empty_array() {
    // When tools is an empty array, tool_choice must be dropped.
    let input = json!({
        "model": "gpt-5.4",
        "tools": [],
        "tool_choice": "auto",
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice should be dropped when tools is empty"
    );
    assert!(
        result.get("tools").is_none(),
        "tools should be absent when input tools was empty"
    );
}

#[test]
fn responses_request_to_chat_drops_parallel_tool_calls_when_no_tools() {
    // parallel_tool_calls must also be dropped when tools is absent,
    // as it is part of EXTRA_CHAT_PASSTHROUGH_FIELDS.
    let input = json!({
        "model": "gpt-5.4",
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice should be dropped"
    );
    assert!(
        result.get("parallel_tool_calls").is_none(),
        "parallel_tool_calls should be dropped"
    );
    assert!(result.get("tools").is_none(), "tools should be absent");
}

#[test]
fn responses_request_to_chat_drops_tool_choice_when_all_tools_filtered() {
    // When all tools are filtered out (e.g., missing name), tool_choice must be dropped.
    let input = json!({
        "model": "gpt-5.4",
        "tools": [
            {"type": "function"}
        ],
        "tool_choice": "auto",
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice should be dropped when all tools filtered"
    );
    assert!(
        result.get("tools").is_none(),
        "tools should be absent when all filtered"
    );
}

#[test]
fn responses_request_to_chat_keeps_tool_choice_when_tools_present() {
    // When tools is present and non-empty, tool_choice must be preserved.
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get weather",
            "parameters": {"type": "object"}
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_some(),
        "tool_choice should be kept when tools present"
    );
    assert_eq!(result["tool_choice"], "auto");
    assert!(
        result.get("parallel_tool_calls").is_some(),
        "parallel_tool_calls should be kept"
    );
    assert_eq!(result["parallel_tool_calls"], true);
    assert!(
        result
            .get("tools")
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty())),
        "tools should be present"
    );
    assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
}

#[test]
fn responses_request_to_chat_keeps_tool_choice_function_when_tools_present() {
    // When tools is present, function-type tool_choice must be preserved and mapped.
    let input = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get weather",
            "parameters": {"type": "object"}
        }],
        "tool_choice": {"type": "function", "name": "get_weather"},
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_some(),
        "tool_choice should be kept"
    );
    assert_eq!(result["tool_choice"]["type"], "function");
    assert_eq!(result["tool_choice"]["function"]["name"], "get_weather");
}

#[test]
fn responses_request_to_chat_no_tool_choice_no_tools_stays_clean() {
    // When neither tool_choice nor tools are present, the output should be clean.
    let input = json!({
        "model": "gpt-5.4",
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice should be absent"
    );
    assert!(result.get("tools").is_none(), "tools should be absent");
    assert!(
        result.get("parallel_tool_calls").is_none(),
        "parallel_tool_calls should be absent"
    );
}

#[test]
fn responses_request_to_chat_tool_choice_none_dropped_when_no_tools() {
    // Even tool_choice: "none" should be dropped when tools is absent,
    // because strict upstreams reject the combination regardless of value.
    let input = json!({
        "model": "gpt-5.4",
        "tool_choice": "none",
        "input": "hi"
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_none(),
        "tool_choice 'none' should be dropped when no tools"
    );
}

#[test]
fn responses_request_to_chat_tool_search_output_provides_tools_keeps_tool_choice() {
    // When tool_search_output in input provides tools, tool_choice should be kept.
    let input = json!({
        "model": "gpt-5.4",
        "tool_choice": "auto",
        "input": [{
            "type": "tool_search_output",
            "call_id": "call_ts_1",
            "status": "completed",
            "execution": "client",
            "tools": [{
                "type": "function",
                "name": "search_docs",
                "description": "Search documentation.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }
            }]
        }]
    });

    let result = responses_to_chat_completions_with_reasoning(input, None, true).unwrap();

    assert!(
        result.get("tool_choice").is_some(),
        "tool_choice should be kept when tool_search_output provides tools"
    );
    assert_eq!(result["tool_choice"], "auto");
    assert!(
        result
            .get("tools")
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty())),
        "tools should be present from tool_search_output"
    );
    assert_eq!(result["tools"][0]["function"]["name"], "search_docs");
}

#[test]
fn compaction_request_becomes_tool_free_summary_turn() {
    let own_summary = codex_compaction::encode_compaction_summary("earlier progress");
    let body = json!({
        "model": "kimi-k3",
        "stream": true,
        "input": [
            { "type": "compaction", "id": "cmp_1", "encrypted_content": own_summary },
            { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "fix bug" }] },
            { "type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{}" },
            { "type": "function_call_output", "call_id": "call_1", "output": "ok" },
            { "type": "compaction_trigger" }
        ],
        "tools": [{ "type": "function", "name": "shell", "parameters": { "type": "object" } }],
        "tool_choice": "auto",
        "parallel_tool_calls": true
    });
    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    assert!(result.get("tools").is_none());
    assert!(result.get("tool_choice").is_none());
    assert!(result.get("parallel_tool_calls").is_none());

    let messages = result["messages"].as_array().unwrap();
    let first = serde_json::to_string(&messages[0]["content"]).unwrap();
    assert!(first.contains("earlier progress"));
    assert!(first.contains("Another language model started to solve this problem"));
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "user");
    assert!(serde_json::to_string(&last["content"])
        .unwrap()
        .contains("CONTEXT CHECKPOINT COMPACTION"));
    // 工具调用历史照常保留（与 Codex 本地压缩请求同形）。
    assert!(messages.iter().any(|message| message["role"] == "tool"));
}

#[test]
fn foreign_compaction_blob_becomes_readable_note_instead_of_vanishing() {
    let body = json!({
        "model": "kimi-k3",
        "input": [
            { "type": "compaction", "encrypted_content": "gAAAAB-openai-blob" },
            { "type": "context_compaction" },
            { "type": "message", "role": "user", "content": "continue" }
        ]
    });
    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    let rendered = serde_json::to_string(&result["messages"]).unwrap();
    assert!(rendered.contains(codex_compaction::OPAQUE_COMPACTION_NOTE));
    assert!(!rendered.contains("gAAAAB-openai-blob"));
}

#[test]
fn developer_resize_notice_between_parallel_image_outputs_keeps_tool_messages_contiguous() {
    let png_data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let body = json!({
        "model": "deepseek-flash",
        "input": [
            { "type": "message", "role": "user", "content": "inspect screenshots" },
            { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Checking images." }] },
            { "type": "function_call", "call_id": "call_0", "name": "view_image", "arguments": "{\"path\":\"a.png\"}" },
            { "type": "function_call", "call_id": "call_1", "name": "view_image", "arguments": "{\"path\":\"b.png\"}" },
            { "type": "function_call", "call_id": "call_2", "name": "view_image", "arguments": "{\"path\":\"c.png\"}" },
            { "type": "function_call_output", "call_id": "call_0", "output": [{ "type": "input_image", "image_url": png_data_url }] },
            { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "<image_resize_notice>\nImage 1 of 1 in the preceding tool output was resized.\n</image_resize_notice>" }] },
            { "type": "function_call_output", "call_id": "call_1", "output": [{ "type": "input_image", "image_url": png_data_url }] },
            { "type": "function_call_output", "call_id": "call_2", "output": [{ "type": "input_image", "image_url": png_data_url }] }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();
    let roles: Vec<&str> = messages
        .iter()
        .filter_map(|msg| msg.get("role").and_then(Value::as_str))
        .collect();
    assert_eq!(
        roles,
        vec![
            "system",
            "user",
            "assistant",
            "tool",
            "tool",
            "tool",
            "user"
        ]
    );
    assert_eq!(messages[2]["tool_calls"].as_array().unwrap().len(), 3);
    assert_eq!(messages[3]["tool_call_id"], "call_0");
    assert_eq!(messages[4]["tool_call_id"], "call_1");
    assert_eq!(messages[5]["tool_call_id"], "call_2");
    assert!(messages[0]["content"]
        .as_str()
        .unwrap()
        .contains("image_resize_notice"));
}

#[test]
fn developer_resize_notice_between_call_and_output_preserves_commentary_merge() {
    let body = json!({
        "model": "deepseek-flash",
        "input": [
            { "type": "message", "role": "user", "content": "look" },
            { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Checking the image." }] },
            { "type": "function_call", "call_id": "call_0", "name": "view_image", "arguments": "{\"path\":\"a.png\"}" },
            { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "<image_resize_notice>resized</image_resize_notice>" }] },
            { "type": "function_call_output", "call_id": "call_0", "output": "ok" }
        ]
    });

    let result = responses_to_chat_completions_with_reasoning(body, None, true).unwrap();
    let messages = result["messages"].as_array().unwrap();
    let roles: Vec<&str> = messages
        .iter()
        .filter_map(|msg| msg.get("role").and_then(Value::as_str))
        .collect();
    assert_eq!(roles, vec!["system", "user", "assistant", "tool"]);
    assert_eq!(messages[2]["content"], "Checking the image.");
    assert_eq!(messages[2]["tool_calls"].as_array().unwrap().len(), 1);
    assert_eq!(messages[2]["tool_calls"][0]["id"], "call_0");
    assert_eq!(messages[3]["tool_call_id"], "call_0");
}

#[tokio::test]
async fn colliding_namespace_boundaries_preserve_both_tools_and_round_trip_everywhere() {
    use bytes::Bytes;
    use futures::{stream, StreamExt};

    let request = json!({
        "model": "gpt-5.4",
        "tools": [
            {
                "type": "namespace",
                "name": "a__b",
                "tools": [{
                    "type": "function",
                    "name": "c",
                    "description": "Tool from namespace a__b.",
                    "parameters": {
                        "type": "object",
                        "properties": { "from_ab": { "type": "string" } },
                        "required": ["from_ab"]
                    }
                }]
            },
            {
                "type": "namespace",
                "name": "a",
                "tools": [{
                    "type": "function",
                    "name": "b__c",
                    "description": "Tool from namespace a.",
                    "parameters": {
                        "type": "object",
                        "properties": { "from_a": { "type": "integer" } },
                        "required": ["from_a"]
                    }
                }]
            }
        ],
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "Start."
            },
            {
                "type": "function_call",
                "call_id": "hist_ab",
                "namespace": "a__b",
                "name": "c",
                "arguments": "{\"from_ab\":\"hello\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "hist_ab",
                "output": "ok_ab"
            },
            {
                "type": "function_call",
                "call_id": "hist_a",
                "namespace": "a",
                "name": "b__c",
                "arguments": "{\"from_a\":42}"
            },
            {
                "type": "function_call_output",
                "call_id": "hist_a",
                "output": "ok_a"
            },
            {
                "type": "message",
                "role": "user",
                "content": "Invoke both tools."
            }
        ]
    });

    let tool_context = build_codex_tool_context_from_request(&request);
    let chat_req =
        responses_to_chat_completions_with_reasoning(request.clone(), None, true).unwrap();
    let tools = chat_req["tools"].as_array().expect("tools array");
    assert_eq!(
        tools.len(),
        2,
        "neither colliding namespace tool may be dropped"
    );

    let chat_name_ab = tools[0]["function"]["name"].as_str().unwrap();
    let chat_name_a = tools[1]["function"]["name"].as_str().unwrap();
    assert_ne!(chat_name_ab, chat_name_a);
    for name in [chat_name_ab, chat_name_a] {
        assert!(!name.is_empty() && name.len() <= CHAT_TOOL_NAME_MAX_LEN);
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
            "invalid chat tool name: {name}"
        );
    }

    // Distinct schemas & descriptions preserved
    assert_eq!(
        tools[0]["function"]["description"],
        "Tool from namespace a__b."
    );
    assert!(tools[0]["function"]["parameters"]["properties"]
        .get("from_ab")
        .is_some());
    assert_eq!(
        tools[1]["function"]["description"],
        "Tool from namespace a."
    );
    assert!(tools[1]["function"]["parameters"]["properties"]
        .get("from_a")
        .is_some());

    // Historical assistant function_call items map to each tool's disambiguated chat_name
    assert_eq!(
        chat_req["messages"][1]["tool_calls"][0]["function"]["name"],
        chat_name_ab
    );
    assert_eq!(
        chat_req["messages"][3]["tool_calls"][0]["function"]["name"],
        chat_name_a
    );

    // Non-streaming Chat response round-trip
    let chat_resp = json!({
        "id": "chatcmpl_ns_collide",
        "created": 1700000000,
        "model": "gpt-5.4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": "call_out_ab",
                        "type": "function",
                        "function": {
                            "name": chat_name_ab,
                            "arguments": "{\"from_ab\":\"world\"}"
                        }
                    },
                    {
                        "id": "call_out_a",
                        "type": "function",
                        "function": {
                            "name": chat_name_a,
                            "arguments": "{\"from_a\":7}"
                        }
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let restored = chat_completion_to_response_with_context(chat_resp, &tool_context).unwrap();
    assert_eq!(restored["output"][0]["type"], "function_call");
    assert_eq!(restored["output"][0]["namespace"], "a__b");
    assert_eq!(restored["output"][0]["name"], "c");
    assert_eq!(restored["output"][1]["type"], "function_call");
    assert_eq!(restored["output"][1]["namespace"], "a");
    assert_eq!(restored["output"][1]["name"], "b__c");

    // Anthropic request + non-streaming response round-trip
    let anth_req =
        super::super::transform_codex_anthropic::responses_request_to_anthropic(request, 4096)
            .unwrap();
    let anth_tools = anth_req["tools"].as_array().unwrap();
    assert_eq!(anth_tools.len(), 2);
    assert_eq!(anth_tools[0]["name"], chat_name_ab);
    assert_eq!(anth_tools[1]["name"], chat_name_a);
    assert_eq!(anth_req["messages"][1]["content"][0]["name"], chat_name_ab);
    assert_eq!(anth_req["messages"][3]["content"][0]["name"], chat_name_a);
    let anth_resp = json!({
        "id": "msg_ns_collide",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "stop_reason": "tool_use",
        "content": [
            {
                "type": "tool_use",
                "id": "toolu_ab",
                "name": chat_name_ab,
                "input": { "from_ab": "anth" }
            },
            {
                "type": "tool_use",
                "id": "toolu_a",
                "name": chat_name_a,
                "input": { "from_a": 99 }
            }
        ],
        "usage": { "input_tokens": 10, "output_tokens": 5 }
    });
    let anth_restored =
        super::super::transform_codex_anthropic::anthropic_response_to_responses_with_context(
            anth_resp,
            &tool_context,
        )
        .unwrap();
    assert_eq!(anth_restored["output"][0]["namespace"], "a__b");
    assert_eq!(anth_restored["output"][0]["name"], "c");
    assert_eq!(anth_restored["output"][1]["namespace"], "a");
    assert_eq!(anth_restored["output"][1]["name"], "b__c");

    // Streaming Chat SSE round-trip
    let chunk_1 = format!(
        "data: {}\n\n",
        json!({
            "id": "chatcmpl_stream_ns",
            "created": 1700000000,
            "model": "gpt-5.4",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call_s_ab",
                            "type": "function",
                            "function": { "name": chat_name_ab, "arguments": "{\"from_ab\":\"s\"}" }
                        },
                        {
                            "index": 1,
                            "id": "call_s_a",
                            "type": "function",
                            "function": { "name": chat_name_a, "arguments": "{\"from_a\":1}" }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }]
        })
    );
    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from(chunk_1)),
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
    ];
    let sse_stream =
        super::super::streaming_codex_chat::create_responses_sse_stream_from_chat_with_context(
            stream::iter(chunks),
            tool_context,
        );
    let raw_bytes: Vec<Bytes> = sse_stream.map(|frame| frame.unwrap()).collect().await;
    let sse_text = String::from_utf8(raw_bytes.concat()).unwrap();
    let events: Vec<Value> = sse_text
        .split("\n\n")
        .filter_map(|block| {
            let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
            serde_json::from_str(data).ok()
        })
        .collect();
    let completed = events
        .iter()
        .find(|ev| ev["type"] == "response.completed")
        .expect("response.completed event");
    assert_eq!(completed["response"]["output"][0]["namespace"], "a__b");
    assert_eq!(completed["response"]["output"][0]["name"], "c");
    assert_eq!(completed["response"]["output"][1]["namespace"], "a");
    assert_eq!(completed["response"]["output"][1]["name"], "b__c");
}

#[test]
fn top_level_and_namespaced_tool_collision_round_trips_in_both_declaration_orders() {
    let top_level_tool = json!({
        "type": "function",
        "name": "files__read",
        "description": "Top-level files__read.",
        "parameters": {
            "type": "object",
            "properties": { "top_path": { "type": "string" } }
        }
    });
    let namespaced_tool = json!({
        "type": "namespace",
        "name": "files",
        "tools": [{
            "type": "function",
            "name": "read",
            "description": "Namespaced files/read.",
            "parameters": {
                "type": "object",
                "properties": { "ns_path": { "type": "string" } }
            }
        }]
    });

    for tools_array in [
        vec![top_level_tool.clone(), namespaced_tool.clone()],
        vec![namespaced_tool, top_level_tool],
    ] {
        let request = json!({
            "model": "gpt-5.4",
            "tools": tools_array,
            "tool_choice": {
                "type": "function",
                "namespace": "files",
                "name": "read"
            },
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_top_hist",
                    "name": "files__read",
                    "arguments": "{\"top_path\":\"/a\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_top_hist",
                    "output": "top_ok"
                },
                {
                    "type": "function_call",
                    "call_id": "call_ns_hist",
                    "namespace": "files",
                    "name": "read",
                    "arguments": "{\"ns_path\":\"/b\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_ns_hist",
                    "output": "ns_ok"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "Run both."
                }
            ]
        });

        let tool_context = build_codex_tool_context_from_request(&request);
        let chat_req = responses_to_chat_completions_with_reasoning(request, None, true).unwrap();
        let emitted_tools = chat_req["tools"].as_array().unwrap();
        assert_eq!(emitted_tools.len(), 2, "must emit both tools in any order");

        let top_chat_name = tool_context.chat_name_for_response_function("files__read", None);
        let ns_chat_name = tool_context.chat_name_for_response_function("read", Some("files"));
        assert_eq!(top_chat_name, "files__read");
        assert_ne!(top_chat_name, ns_chat_name);
        assert!(ns_chat_name.len() <= CHAT_TOOL_NAME_MAX_LEN);

        assert_eq!(chat_req["tool_choice"]["function"]["name"], ns_chat_name);
        assert_eq!(
            chat_req["messages"][0]["tool_calls"][0]["function"]["name"],
            top_chat_name
        );
        assert_eq!(
            chat_req["messages"][2]["tool_calls"][0]["function"]["name"],
            ns_chat_name
        );

        let chat_resp = json!({
            "id": "chatcmpl_order",
            "created": 1700000001,
            "model": "gpt-5.4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "c_top",
                            "type": "function",
                            "function": { "name": top_chat_name, "arguments": "{\"top_path\":\"/x\"}" }
                        },
                        {
                            "id": "c_ns",
                            "type": "function",
                            "function": { "name": ns_chat_name, "arguments": "{\"ns_path\":\"/y\"}" }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let restored = chat_completion_to_response_with_context(chat_resp, &tool_context).unwrap();
        assert_eq!(restored["output"][0]["name"], "files__read");
        assert!(restored["output"][0].get("namespace").is_none());
        assert_eq!(restored["output"][1]["name"], "read");
        assert_eq!(restored["output"][1]["namespace"], "files");
    }
}

#[test]
fn non_colliding_long_namespace_tool_preserves_legacy_flattened_name() {
    let ns = "mcp__codex_apps__github";
    let name = "_list_pull_request_review_comments_for_repo";
    let expected = "mcp__codex_apps__github___list_pull_request_re__b9df71aaf1654d40";

    assert_eq!(flatten_namespace_tool_name(ns, name), expected);

    let request = json!({
        "model": "gpt-5.4",
        "tools": [{
            "type": "namespace",
            "name": ns,
            "tools": [{
                "type": "function",
                "name": name,
                "parameters": { "type": "object", "properties": {} }
            }]
        }],
        "tool_choice": {
            "type": "function",
            "namespace": ns,
            "name": name
        },
        "input": [
            {
                "type": "function_call",
                "call_id": "call_mcp_1",
                "namespace": ns,
                "name": name,
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_mcp_1",
                "output": "[]"
            },
            {
                "type": "message",
                "role": "user",
                "content": "List review comments."
            }
        ]
    });

    let tool_context = build_codex_tool_context_from_request(&request);
    let chat_req =
        responses_to_chat_completions_with_reasoning(request.clone(), None, true).unwrap();
    assert_eq!(chat_req["tools"][0]["function"]["name"], expected);
    assert_eq!(chat_req["tool_choice"]["function"]["name"], expected);
    assert_eq!(
        chat_req["messages"][0]["tool_calls"][0]["function"]["name"],
        expected
    );

    let anth_req =
        super::super::transform_codex_anthropic::responses_request_to_anthropic(request, 4096)
            .unwrap();
    assert_eq!(anth_req["tools"][0]["name"], expected);

    let chat_resp = json!({
        "id": "chatcmpl_mcp_long",
        "created": 1700000003,
        "model": "gpt-5.4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_mcp_out",
                    "type": "function",
                    "function": { "name": expected, "arguments": "{}" }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let restored = chat_completion_to_response_with_context(chat_resp, &tool_context).unwrap();
    assert_eq!(restored["output"][0]["namespace"], ns);
    assert_eq!(restored["output"][0]["name"], name);
}

#[test]
fn long_colliding_tool_names_produce_distinct_valid_hashes_and_round_trip() {
    let tail = "c".repeat(70);
    let name_for_a = format!("b__{tail}");
    let long_top_name = format!("top__{}", "z".repeat(75));

    let preferred_ab = flatten_namespace_tool_name("a__b", &tail);
    let preferred_a = flatten_namespace_tool_name("a", &name_for_a);
    assert_eq!(preferred_ab, preferred_a);
    assert!(preferred_ab.len() <= CHAT_TOOL_NAME_MAX_LEN);

    let request = json!({
        "model": "gpt-5.4",
        "tools": [
            {
                "type": "namespace",
                "name": "a__b",
                "tools": [{
                    "type": "function",
                    "name": tail,
                    "parameters": { "type": "object", "properties": { "k1": { "type": "string" } } }
                }]
            },
            {
                "type": "namespace",
                "name": "a",
                "tools": [{
                    "type": "function",
                    "name": name_for_a,
                    "parameters": { "type": "object", "properties": { "k2": { "type": "string" } } }
                }]
            },
            {
                "type": "function",
                "name": long_top_name,
                "parameters": { "type": "object", "properties": { "k3": { "type": "string" } } }
            }
        ],
        "input": [
            {
                "type": "function_call",
                "call_id": "call_l1",
                "namespace": "a__b",
                "name": tail,
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_l1",
                "output": "ok1"
            },
            {
                "type": "function_call",
                "call_id": "call_l3",
                "name": long_top_name,
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_l3",
                "output": "ok3"
            },
            {
                "type": "message",
                "role": "user",
                "content": "go"
            }
        ]
    });

    let tool_context = build_codex_tool_context_from_request(&request);
    let chat_req = responses_to_chat_completions_with_reasoning(request, None, true).unwrap();
    let tools = chat_req["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);

    let n0 = tools[0]["function"]["name"].as_str().unwrap();
    let n1 = tools[1]["function"]["name"].as_str().unwrap();
    let n2 = tools[2]["function"]["name"].as_str().unwrap();
    assert_ne!(n0, n1);
    assert_ne!(n0, n2);
    assert_ne!(n1, n2);
    for n in [n0, n1, n2] {
        assert!(n.len() <= CHAT_TOOL_NAME_MAX_LEN, "len={} for {n}", n.len());
        assert!(n
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'));
    }
    assert_eq!(
        chat_req["messages"][0]["tool_calls"][0]["function"]["name"],
        n0
    );
    assert_eq!(
        chat_req["messages"][2]["tool_calls"][0]["function"]["name"],
        n2
    );

    let chat_resp = json!({
        "id": "chatcmpl_long",
        "created": 1700000002,
        "model": "gpt-5.4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    { "id": "r0", "type": "function", "function": { "name": n0, "arguments": "{}" } },
                    { "id": "r1", "type": "function", "function": { "name": n1, "arguments": "{}" } },
                    { "id": "r2", "type": "function", "function": { "name": n2, "arguments": "{}" } }
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let restored = chat_completion_to_response_with_context(chat_resp, &tool_context).unwrap();
    assert_eq!(restored["output"][0]["namespace"], "a__b");
    assert_eq!(restored["output"][0]["name"], tail);
    assert_eq!(restored["output"][1]["namespace"], "a");
    assert_eq!(restored["output"][1]["name"], name_for_a);
    assert!(restored["output"][2].get("namespace").is_none());
    assert_eq!(restored["output"][2]["name"], long_top_name);
}

#[test]
fn duplicate_exact_tool_identity_is_idempotent_without_panic() {
    let request = json!({
        "model": "gpt-5.4",
        "tools": [
            {
                "type": "namespace",
                "name": "files",
                "tools": [
                    {
                        "type": "function",
                        "name": "read",
                        "description": "First namespaced read.",
                        "parameters": { "type": "object", "properties": {} }
                    },
                    {
                        "type": "function",
                        "name": "read",
                        "description": "Duplicate namespaced read.",
                        "parameters": { "type": "object", "properties": {} }
                    }
                ]
            },
            {
                "type": "function",
                "name": "shell",
                "description": "First shell.",
                "parameters": { "type": "object", "properties": {} }
            },
            {
                "type": "function",
                "name": "shell",
                "description": "Duplicate shell.",
                "parameters": { "type": "object", "properties": {} }
            }
        ],
        "input": "hello"
    });

    let chat_req = responses_to_chat_completions_with_reasoning(request, None, true).unwrap();
    let tools = chat_req["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["function"]["name"], "files__read");
    assert_eq!(
        tools[0]["function"]["description"],
        "First namespaced read."
    );
    assert_eq!(tools[1]["function"]["name"], "shell");
    assert_eq!(tools[1]["function"]["description"], "First shell.");
}
