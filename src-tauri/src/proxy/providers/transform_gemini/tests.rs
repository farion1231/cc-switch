use super::*;

#[test]
fn anthropic_to_gemini_maps_system_and_messages() {
    let input = json!({
        "model": "gemini-2.5-pro",
        "max_tokens": 128,
        "system": "You are helpful.",
        "messages": [
            { "role": "user", "content": "Hello" }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    assert_eq!(
        result["systemInstruction"]["parts"][0]["text"],
        "You are helpful."
    );
    assert_eq!(result["contents"][0]["role"], "user");
    assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
    assert_eq!(result["generationConfig"]["maxOutputTokens"], 128);
}

fn thinking_config_for(model: &str, thinking: Value) -> Option<Value> {
    let mut body = json!({
        "model": model,
        "max_tokens": 32000,
        "messages": [{ "role": "user", "content": "Hi" }]
    });
    for (key, value) in thinking.as_object().unwrap() {
        body[key] = value.clone();
    }
    anthropic_to_gemini_with_shadow(body, None, None, None).unwrap()["generationConfig"]
        .get("thinkingConfig")
        .cloned()
}

fn adaptive(effort: &str) -> Value {
    json!({
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": effort }
    })
}

#[test]
fn thinking_config_maps_effort_to_budget_for_gemini_2_5() {
    for model in [
        "gemini-2.5-pro",
        "gemini-2.5-flash",
        "gemini-2.5-flash-lite",
        "gemini-2.5-flash-preview-09-2025",
    ] {
        for (effort, budget) in [
            ("low", 1024),
            ("medium", 8192),
            ("high", 24_576),
            ("xhigh", 24_576),
            ("max", 24_576),
        ] {
            assert_eq!(
                thinking_config_for(model, adaptive(effort)),
                Some(json!({ "thinkingBudget": budget })),
                "model={model}, effort={effort}"
            );
        }
    }
}

#[test]
fn thinking_config_maps_effort_to_level_for_gemini_3_and_later() {
    for model in [
        "gemini-4-argon",
        "gemini-3.8-flash",
        "gemini-3.6-flash",
        "gemini-3.5-flash-lite",
        "gemini-3.1-pro-preview",
        "gemini-3.1-flash-lite",
        "gemini-3-flash-preview",
        "gemini-3.8-pro",
    ] {
        for (effort, level) in [
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("xhigh", "high"),
            ("max", "high"),
        ] {
            assert_eq!(
                thinking_config_for(model, adaptive(effort)),
                Some(json!({ "thinkingLevel": level })),
                "model={model}, effort={effort}"
            );
        }
    }
    // Gemini 3 Pro 只收 low / high。
    assert_eq!(
        thinking_config_for("gemini-3-pro-preview", adaptive("medium")),
        Some(json!({ "thinkingLevel": "high" }))
    );
    assert_eq!(
        thinking_config_for("gemini-3-pro-preview", adaptive("low")),
        Some(json!({ "thinkingLevel": "low" }))
    );
}

#[test]
fn thinking_config_disabled_sends_lowest_setting_each_model_accepts() {
    let cases = [
        ("gemini-2.5-pro", json!({ "thinkingBudget": 128 })),
        ("gemini-2.5-flash", json!({ "thinkingBudget": 0 })),
        ("gemini-2.5-flash-lite", json!({ "thinkingBudget": 0 })),
        (
            "gemini-3-flash-preview",
            json!({ "thinkingLevel": "minimal" }),
        ),
        ("gemini-3.6-flash", json!({ "thinkingLevel": "minimal" })),
        (
            "gemini-3.1-flash-lite",
            json!({ "thinkingLevel": "minimal" }),
        ),
        (
            "gemini-3.5-flash-lite",
            json!({ "thinkingLevel": "minimal" }),
        ),
        ("gemini-4-argon", json!({ "thinkingLevel": "low" })),
        ("gemini-3.8-flash", json!({ "thinkingLevel": "low" })),
        ("gemini-3.1-pro-preview", json!({ "thinkingLevel": "low" })),
        ("gemini-3-pro-preview", json!({ "thinkingLevel": "low" })),
    ];
    for (model, expected) in cases {
        assert_eq!(
            thinking_config_for(model, json!({ "thinking": { "type": "disabled" } })),
            Some(expected.clone()),
            "model={model}"
        );
        // Claude Code 关思考时照样带 effort：disabled 或省略 thinking 都算关。
        assert_eq!(
            thinking_config_for(
                model,
                json!({
                    "thinking": { "type": "disabled" },
                    "output_config": { "effort": "high" }
                })
            ),
            Some(expected.clone()),
            "model={model}"
        );
        assert_eq!(
            thinking_config_for(model, json!({ "output_config": { "effort": "high" } })),
            Some(expected),
            "model={model}"
        );
    }
}

#[test]
fn thinking_config_reads_budget_tokens_and_adaptive_without_effort() {
    let enabled = |budget: Option<u64>| {
        let mut thinking = json!({ "type": "enabled" });
        if let Some(budget) = budget {
            thinking["budget_tokens"] = json!(budget);
        }
        json!({ "thinking": thinking })
    };
    for (thinking, budget, level) in [
        (enabled(Some(2_048)), 1024, "low"),
        (enabled(Some(10_000)), 8192, "medium"),
        (enabled(Some(31_999)), 24_576, "high"),
        (enabled(None), 24_576, "high"),
        (
            json!({ "thinking": { "type": "adaptive" } }),
            24_576,
            "high",
        ),
    ] {
        assert_eq!(
            thinking_config_for("gemini-2.5-pro", thinking.clone()),
            Some(json!({ "thinkingBudget": budget })),
            "thinking={thinking}"
        );
        assert_eq!(
            thinking_config_for("gemini-4-argon", thinking.clone()),
            Some(json!({ "thinkingLevel": level })),
            "thinking={thinking}"
        );
    }
    // output_config.effort 优先于 budget_tokens。
    assert_eq!(
        thinking_config_for(
            "gemini-3.8-flash",
            json!({
                "thinking": { "type": "enabled", "budget_tokens": 31_999 },
                "output_config": { "effort": "low" }
            })
        ),
        Some(json!({ "thinkingLevel": "low" }))
    );
}

#[test]
fn thinking_config_matches_last_segment_of_prefixed_model_names() {
    assert_eq!(
        thinking_config_for("models/gemini-2.5-flash", adaptive("medium")),
        Some(json!({ "thinkingBudget": 8192 }))
    );
    assert_eq!(
        thinking_config_for("google/Gemini-3-Flash-Preview", adaptive("medium")),
        Some(json!({ "thinkingLevel": "medium" }))
    );
}

#[test]
fn thinking_config_omitted_for_unknown_models_or_missing_signal() {
    for model in [
        "gemini-2.0-flash",
        "gemini-1.5-pro",
        "gemini-flash-latest",
        "gemini-exp-1206",
        "gemini-3-pro-high",
        "gemini-4-argon-medium",
        "gemini-2.5-flash-image",
        "gemini-3.1-flash-lite-image",
        "gemini-3-pro-image",
        "gemini-3.8-flash-tts",
        "gemini-3.8-live",
        "gemini-2.5-flash-native-audio-preview-12-2025",
        "gemini-3.5-transcribe",
        "gemini-embedding-001",
        "my-relay-model",
        "claude-sonnet-5",
    ] {
        assert_eq!(
            thinking_config_for(model, adaptive("high")),
            None,
            "model={model}"
        );
        assert_eq!(
            thinking_config_for(model, json!({ "thinking": { "type": "disabled" } })),
            None,
            "model={model}"
        );
    }
    for thinking in [
        json!({}),
        json!({ "thinking": null }),
        json!({ "thinking": { "type": "something-new" } }),
        json!({ "thinking": { "type": "adaptive" }, "output_config": { "effort": "turbo" } }),
    ] {
        assert_eq!(
            thinking_config_for("gemini-2.5-flash", thinking.clone()),
            None,
            "thinking={thinking}"
        );
    }
}

#[test]
fn thinking_config_only_adds_thinking_config_to_generation_config() {
    let base = json!({
        "model": "gemini-3.8-flash",
        "max_tokens": 32000,
        "temperature": 1,
        "top_p": 0.9,
        "stop_sequences": ["END"],
        "system": "You are helpful.",
        "messages": [{ "role": "user", "content": "Hi" }]
    });
    let mut with_thinking = base.clone();
    with_thinking["thinking"] = json!({ "type": "adaptive" });
    with_thinking["output_config"] = json!({ "effort": "medium" });

    let mut plain = anthropic_to_gemini_with_shadow(base, None, None, None).unwrap();
    let mut converted = anthropic_to_gemini_with_shadow(with_thinking, None, None, None).unwrap();
    assert_eq!(
        converted["generationConfig"]
            .as_object_mut()
            .unwrap()
            .remove("thinkingConfig"),
        Some(json!({ "thinkingLevel": "medium" }))
    );
    assert_eq!(converted, plain);

    // 没有别的生成参数时，generationConfig 只放 thinkingConfig。
    plain = anthropic_to_gemini_with_shadow(
        json!({
            "model": "gemini-2.5-flash",
            "thinking": { "type": "disabled" },
            "messages": [{ "role": "user", "content": "Hi" }]
        }),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        plain["generationConfig"],
        json!({ "thinkingConfig": { "thinkingBudget": 0 } })
    );
}

#[test]
fn thinking_config_keeps_default_adaptive_of_5x_client_models() {
    // Claude Code 对 Opus 5.5 等「关不掉思考」的模型省略 thinking；转发前按客户端原模型
    // 补成 adaptive，否则映射到 Gemini 后会被当成关思考。
    let mut body = json!({
        "model": "gemini-3.8-flash",
        "output_config": { "effort": "high" },
        "messages": [{ "role": "user", "content": "Hi" }]
    });
    assert_eq!(
        thinking_config_for("gemini-3.8-flash", body.clone()),
        Some(json!({ "thinkingLevel": "low" }))
    );
    super::super::transform::make_default_adaptive_thinking_explicit(
        &mut body,
        Some("claude-opus-5-5"),
    );
    assert_eq!(
        anthropic_to_gemini_with_shadow(body, None, None, None).unwrap()["generationConfig"]
            ["thinkingConfig"],
        json!({ "thinkingLevel": "high" })
    );
}

#[test]
fn anthropic_to_gemini_merges_system_messages_into_system_instruction() {
    let input = json!({
        "model": "gemini-3-pro",
        "system": [{ "type": "text", "text": "Top level system." }],
        "messages": [
            { "role": "system", "content": "Message system." },
            {
                "role": "system",
                "content": [{ "type": "text", "text": "Block system." }]
            },
            { "role": "user", "content": "Hello" }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();

    assert_eq!(
        result["systemInstruction"]["parts"][0]["text"],
        "Top level system.\n\nMessage system.\n\nBlock system."
    );
    assert_eq!(result["contents"].as_array().unwrap().len(), 1);
    assert_eq!(result["contents"][0]["role"], "user");
    assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
}

#[test]
fn anthropic_to_gemini_keeps_mid_conversation_system_messages_in_place() {
    // Claude Code appends a `<total_tokens>` system message every turn;
    // hoisting it would change the instruction ahead of the whole history
    // and break the prefix cache, so it must stay where it appears.
    let first = json!({
        "model": "gemini-3-pro",
        "system": "Stable instructions",
        "messages": [
            { "role": "user", "content": "Existing history" },
            { "role": "system", "content": "<total_tokens>99900000 tokens left</total_tokens>\n\n" }
        ]
    });
    let mut second = first.clone();
    second["messages"].as_array_mut().unwrap().extend([
        json!({ "role": "assistant", "content": "Acknowledged" }),
        json!({ "role": "user", "content": "Continue" }),
        json!({
            "role": "system",
            "content": [{ "type": "text", "text": "<total_tokens>99899000 tokens left</total_tokens>\n\n" }]
        }),
    ]);

    let first = anthropic_to_gemini_with_shadow(first, None, None, None).unwrap();
    let second = anthropic_to_gemini_with_shadow(second, None, None, None).unwrap();

    assert_eq!(
        first["systemInstruction"]["parts"][0]["text"],
        "Stable instructions"
    );
    assert_eq!(first["systemInstruction"], second["systemInstruction"]);

    let first_contents = first["contents"].as_array().unwrap();
    let second_contents = second["contents"].as_array().unwrap();
    assert_eq!(first_contents.len(), 2);
    assert_eq!(second_contents.len(), 5);
    assert_eq!(&second_contents[..2], first_contents.as_slice());
    assert_eq!(
        first_contents[1],
        json!({
            "role": "user",
            "parts": [{ "text": "<total_tokens>99900000 tokens left</total_tokens>\n\n" }]
        })
    );
    assert_eq!(second_contents[2]["role"], "model");
    assert_eq!(
        second_contents[4],
        json!({
            "role": "user",
            "parts": [{ "text": "<total_tokens>99899000 tokens left</total_tokens>\n\n" }]
        })
    );
}

#[test]
fn anthropic_to_gemini_moves_system_message_after_function_response() {
    let input = json!({
        "model": "gemini-3-pro",
        "messages": [
            { "role": "user", "content": "Read the file" },
            {
                "role": "assistant",
                "content": [{ "type": "tool_use", "id": "toolu_1", "name": "Read", "input": {} }]
            },
            { "role": "system", "content": "<total_tokens>1 tokens left</total_tokens>" },
            {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "ok" }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let contents = result["contents"].as_array().unwrap();

    assert_eq!(contents.len(), 4);
    assert!(contents[1]["parts"][0].get("functionCall").is_some());
    assert!(contents[2]["parts"][0].get("functionResponse").is_some());
    assert_eq!(
        contents[3]["parts"][0]["text"],
        "<total_tokens>1 tokens left</total_tokens>"
    );
}

#[test]
fn anthropic_to_gemini_maps_tools_and_tool_results() {
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                ]
            }
        ],
        "tools": [
            {
                "name": "get_weather",
                "description": "Weather lookup",
                "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } }
            }
        ],
        "tool_choice": { "type": "tool", "name": "get_weather" }
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    assert_eq!(
        result["tools"][0]["functionDeclarations"][0]["name"],
        "get_weather"
    );
    assert!(result["tools"][0]["functionDeclarations"][0]
        .get("parameters")
        .is_some());
    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "get_weather"
    );
    assert_eq!(
        result["contents"][1]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
    assert_eq!(
        result["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "get_weather"
    );
}

/// Regression for #8988: Vertex Gemini rejects a final user turn shaped
/// `[functionResponse, text]` with "Requests ending with a model turn".
#[test]
fn anthropic_to_gemini_splits_text_after_tool_results_into_next_user_turn() {
    let input = json!({
        "messages": [
            { "role": "user", "content": "Check two cities" },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } },
                    { "type": "tool_use", "id": "call_2", "name": "get_weather", "input": { "city": "Paris" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" },
                    { "type": "tool_result", "tool_use_id": "call_2", "content": "Rain" },
                    { "type": "text", "text": "<system-reminder>keep going</system-reminder>" }
                ]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let contents = result["contents"].as_array().unwrap();

    assert_eq!(contents.len(), 4);
    assert_eq!(contents[0]["parts"][0]["text"], "Check two cities");
    assert_eq!(contents[1]["role"], "model");

    let responses = contents[2]["parts"].as_array().unwrap();
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["functionResponse"]["id"], "call_1");
    assert_eq!(responses[1]["functionResponse"]["id"], "call_2");

    assert_eq!(contents[3]["role"], "user");
    assert_eq!(
        contents[3]["parts"],
        json!([{ "text": "<system-reminder>keep going</system-reminder>" }])
    );
}

/// Regression for #9453: results of parallel tool calls spread over
/// consecutive user messages must land in one functionResponse turn,
/// otherwise Gemini rejects the call/response count mismatch.
#[test]
fn anthropic_to_gemini_merges_parallel_tool_results_split_across_user_messages() {
    let input = json!({
        "messages": [
            { "role": "user", "content": "Check two cities" },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } },
                    { "type": "tool_use", "id": "call_2", "name": "get_weather", "input": { "city": "Paris" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" },
                    { "type": "text", "text": "reminder 1" }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_2", "content": "Rain" },
                    { "type": "text", "text": "reminder 2" }
                ]
            },
            { "role": "assistant", "content": "Tokyo is sunny, Paris is rainy." },
            { "role": "user", "content": "Thanks" }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let contents = result["contents"].as_array().unwrap();

    assert_eq!(contents.len(), 6);
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[1]["parts"].as_array().unwrap().len(), 2);

    let responses = contents[2]["parts"].as_array().unwrap();
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["functionResponse"]["id"], "call_1");
    assert_eq!(responses[1]["functionResponse"]["id"], "call_2");

    assert_eq!(
        contents[3]["parts"],
        json!([{ "text": "reminder 1" }, { "text": "reminder 2" }])
    );
    assert_eq!(contents[4]["role"], "model");
    assert_eq!(contents[5]["parts"], json!([{ "text": "Thanks" }]));
}

/// Consecutive user messages with at most one tool_result carrier keep
/// their existing one-content-per-message conversion (prompt cache).
#[test]
fn anthropic_to_gemini_keeps_consecutive_user_messages_without_split_tool_results() {
    let input = json!({
        "messages": [
            { "role": "user", "content": "first" },
            { "role": "user", "content": [{ "type": "text", "text": "second" }] },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } }
                ]
            },
            {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }]
            },
            { "role": "user", "content": "follow-up" }
        ]
    });

    let messages = input["messages"].as_array().unwrap();
    assert!(matches!(
        coalesce_split_tool_result_messages(messages),
        std::borrow::Cow::Borrowed(_)
    ));

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let contents = result["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 5);
    assert_eq!(contents[0]["parts"], json!([{ "text": "first" }]));
    assert_eq!(contents[1]["parts"], json!([{ "text": "second" }]));
    assert_eq!(contents[3]["parts"][0]["functionResponse"]["id"], "call_1");
    assert_eq!(contents[4]["parts"], json!([{ "text": "follow-up" }]));
}

#[test]
fn anthropic_to_gemini_moves_mixed_tool_result_image_to_native_part() {
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_image",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_image",
                    "content": [
                        {"type": "text", "text": "caption"},
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "GEMINI_TOOL_IMAGE_SENTINEL"
                            }
                        }
                    ]
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let parts = result["contents"][1]["parts"].as_array().unwrap();
    let response = &parts[0]["functionResponse"]["response"]["content"];

    assert!(response.as_str().unwrap().contains("caption"));
    assert!(response
        .as_str()
        .unwrap()
        .contains("tool result media attached"));
    assert!(!response
        .as_str()
        .unwrap()
        .contains("GEMINI_TOOL_IMAGE_SENTINEL"));
    assert_eq!(parts.len(), 1);
    let media = result["contents"][2]["parts"].as_array().unwrap();
    assert_eq!(result["contents"][2]["role"], "user");
    assert_eq!(
        media[0]["text"],
        "[cc-switch: media output of tool call call_image]"
    );
    assert_eq!(media[1]["inlineData"]["mimeType"], "image/png");
    assert_eq!(media[1]["inlineData"]["data"], "GEMINI_TOOL_IMAGE_SENTINEL");
}

#[test]
fn anthropic_to_gemini_clamps_json_string_residual_base64_before_serializing() {
    let residual_base64 = "A".repeat(20_000);
    let encoded = json!({
        "content": [
            {
                "type": "image_url",
                "image_url": {
                    "url": "data:image/png;base64,GEMINI_STRING_IMAGE_SENTINEL"
                }
            },
            {"type": "video", "data": residual_base64}
        ]
    })
    .to_string();
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_string",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_string",
                    "content": encoded
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let serialized = result.to_string();

    assert!(serialized.contains("[cc-switch: omitted 20000 bytes]"));
    assert!(!serialized.contains(&"A".repeat(64)));
    assert_eq!(
        result["contents"][2]["parts"][1]["inlineData"]["data"],
        "GEMINI_STRING_IMAGE_SENTINEL"
    );
}

#[test]
fn anthropic_to_gemini_keeps_remote_tool_image_in_legacy_response() {
    let remote_image = json!({
        "type": "image",
        "source": {
            "type": "url",
            "url": "https://example.com/tool-image.png"
        }
    });
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_remote",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_remote",
                    "content": [remote_image.clone()]
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let parts = result["contents"][1]["parts"].as_array().unwrap();

    assert_eq!(parts.len(), 1);
    assert_eq!(
        parts[0]["functionResponse"]["response"]["content"][0],
        remote_image
    );
    assert!(!result.to_string().contains("fileData"));
    assert!(!result
        .to_string()
        .contains(TOOL_RESULT_MEDIA_ATTACHED_MARKER));
}

#[test]
fn anthropic_to_gemini_does_not_strip_unconvertible_tool_data_url() {
    let malformed_image = json!({
        "type": "image_url",
        "image_url": {
            "url": "data:image/png,NOT_BASE64"
        }
    });
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_malformed",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_malformed",
                    "content": [malformed_image.clone()]
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let parts = result["contents"][1]["parts"].as_array().unwrap();

    assert_eq!(parts.len(), 1);
    assert_eq!(
        parts[0]["functionResponse"]["response"]["content"][0],
        malformed_image
    );
    assert!(!result.to_string().contains("inlineData"));
    assert!(!result
        .to_string()
        .contains(TOOL_RESULT_MEDIA_ATTACHED_MARKER));
}

#[test]
fn anthropic_to_gemini_does_not_embed_image_only_tool_result_as_json() {
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_image",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_image",
                    "content": [{
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/webp",
                            "data": "IMAGE_ONLY_SENTINEL"
                        }
                    }]
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let parts = result["contents"][1]["parts"].as_array().unwrap();
    let response = &parts[0]["functionResponse"]["response"]["content"];

    assert!(response.is_string());
    assert!(!response.as_str().unwrap().contains("IMAGE_ONLY_SENTINEL"));
    let media = result["contents"][2]["parts"].as_array().unwrap();
    assert_eq!(media[1]["inlineData"]["mimeType"], "image/webp");
    assert_eq!(media[1]["inlineData"]["data"], "IMAGE_ONLY_SENTINEL");
}

#[test]
fn anthropic_to_gemini_3_uses_multimodal_function_response_parts() {
    let input = json!({
        "model": "gemini-3-pro-preview",
        "messages": [
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_image",
                    "name": "inspect",
                    "input": {}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_image",
                    "content": [{
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/jpeg",
                            "data": "GEMINI_3_IMAGE_SENTINEL"
                        }
                    }]
                }]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let parts = result["contents"][1]["parts"].as_array().unwrap();
    let function_response = &parts[0]["functionResponse"];

    assert_eq!(parts.len(), 1);
    assert_eq!(
        function_response["parts"][0]["inlineData"]["mimeType"],
        "image/jpeg"
    );
    assert_eq!(
        function_response["parts"][0]["inlineData"]["data"],
        "GEMINI_3_IMAGE_SENTINEL"
    );
    assert!(!function_response["response"]["content"]
        .as_str()
        .unwrap()
        .contains("GEMINI_3_IMAGE_SENTINEL"));
}

#[test]
fn anthropic_to_gemini_resolves_tool_result_name_from_shadow_content() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "provider-a",
        "session-1",
        json!({
            "parts": [{
                "functionCall": {
                    "id": "call_1",
                    "name": "get_weather",
                    "args": { "city": "Tokyo" }
                }
            }]
        }),
        vec![],
    );

    let input = json!({
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("provider-a"), Some("session-1"))
            .unwrap();

    assert_eq!(
        result["contents"][0]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
}

fn skip_signature_request(model: &str) -> Value {
    json!({
        "model": model,
        "messages": [
            { "role": "user", "content": "old question" },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_old", "name": "read", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_old", "content": "a" }
                ]
            },
            { "role": "assistant", "content": "done" },
            { "role": "user", "content": "new question" },
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "checking" },
                    { "type": "tool_use", "id": "call_1", "name": "read", "input": {} },
                    { "type": "tool_use", "id": "call_2", "name": "grep", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "b" },
                    { "type": "tool_result", "tool_use_id": "call_2", "content": "c" }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_3", "name": "read", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_3", "content": "d" }
                ]
            }
        ]
    })
}

#[test]
fn anthropic_to_gemini_fills_skip_signature_on_first_call_of_current_turn_steps() {
    let result = anthropic_to_gemini_with_shadow(
        skip_signature_request("gemini-3.8-flash"),
        None,
        None,
        None,
    )
    .unwrap();
    let contents = result["contents"].as_array().unwrap();
    let model_turns: Vec<&Value> = contents
        .iter()
        .filter(|content| content["role"] == "model")
        .collect();

    // Earlier turn (before "new question") is not validated by Gemini.
    assert!(model_turns[0]["parts"][0].get("thoughtSignature").is_none());

    // Current turn, first step: text part untouched, first call filled,
    // parallel sibling stays unsigned.
    let step_one = model_turns[2]["parts"].as_array().unwrap();
    assert!(step_one[0].get("thoughtSignature").is_none());
    assert_eq!(
        step_one[1]["thoughtSignature"],
        GEMINI_SKIP_THOUGHT_SIGNATURE
    );
    assert!(step_one[2].get("thoughtSignature").is_none());

    // Current turn, second step.
    assert_eq!(
        model_turns[3]["parts"][0]["thoughtSignature"],
        GEMINI_SKIP_THOUGHT_SIGNATURE
    );
}

#[test]
fn anthropic_to_gemini_keeps_real_signature_over_skip_placeholder() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "provider-a",
        "session-1",
        json!({}),
        vec![GeminiToolCallMeta::new(
            Some("call_3"),
            "read",
            json!({}),
            Some("sig-real"),
        )],
    );

    let result = anthropic_to_gemini_with_shadow(
        skip_signature_request("google/gemini-3.8-flash"),
        Some(&store),
        Some("provider-a"),
        Some("session-1"),
    )
    .unwrap();
    let contents = result["contents"].as_array().unwrap();
    let last_model = contents
        .iter()
        .rev()
        .find(|content| content["role"] == "model")
        .unwrap();
    assert_eq!(last_model["parts"][0]["thoughtSignature"], "sig-real");
}

#[test]
fn anthropic_to_gemini_skips_placeholder_for_pre_gemini_3_models() {
    let result = anthropic_to_gemini_with_shadow(
        skip_signature_request("gemini-2.5-flash"),
        None,
        None,
        None,
    )
    .unwrap();
    let has_placeholder = result["contents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|content| content["parts"].as_array().unwrap())
        .any(|part| part.get("thoughtSignature").is_some());
    assert!(!has_placeholder);
}

#[test]
fn anthropic_to_gemini_rebuilt_function_call_keeps_signature_on_part() {
    // The shadow turn holds the signature but no replayable parts, so the
    // assistant message is rebuilt from its tool_use blocks instead of
    // being replayed verbatim.
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "provider-a",
        "session-1",
        json!({}),
        vec![GeminiToolCallMeta::new(
            Some("call_1"),
            "get_weather",
            json!({ "city": "Tokyo" }),
            Some("sig-xyz"),
        )],
    );

    let input = json!({
        "messages": [
            { "role": "user", "content": "weather?" },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("provider-a"), Some("session-1"))
            .unwrap();

    let part = &result["contents"][1]["parts"][0];
    assert_eq!(part["functionCall"]["name"], "get_weather");
    assert_eq!(part["thoughtSignature"], "sig-xyz");
    assert!(part["functionCall"].get("thoughtSignature").is_none());
}

#[test]
fn anthropic_to_gemini_reattaches_thought_signature_as_part_sibling() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "provider-a",
        "session-1",
        json!({
            "parts": [{
                "functionCall": {
                    "id": "call_1",
                    "name": "get_weather",
                    "args": { "city": "Tokyo" }
                },
                "thoughtSignature": "sig-xyz"
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some("call_1"),
            "get_weather",
            json!({ "city": "Tokyo" }),
            Some("sig-xyz"),
        )],
    );

    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("provider-a"), Some("session-1"))
            .unwrap();

    let part = &result["contents"][0]["parts"][0];
    // `thoughtSignature` must be a sibling of `functionCall` on the Part,
    // NOT nested inside the functionCall object.
    assert_eq!(part["thoughtSignature"], "sig-xyz");
    assert!(
        part["functionCall"].get("thoughtSignature").is_none(),
        "thoughtSignature must not be nested inside functionCall"
    );
    assert_eq!(part["functionCall"]["name"], "get_weather");
}

#[test]
fn anthropic_to_gemini_rejects_tool_result_without_resolvable_name() {
    let input = json!({
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                ]
            }
        ]
    });

    let error = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap_err();
    assert!(error
        .to_string()
        .contains("Unable to resolve Gemini functionResponse.name"));
}

#[test]
fn anthropic_to_gemini_uses_parameters_json_schema_for_rich_tool_schema() {
    let input = json!({
        "tools": [
            {
                "name": "search",
                "description": "Search data",
                "input_schema": {
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let declaration = &result["tools"][0]["functionDeclarations"][0];

    assert!(declaration.get("parameters").is_none());
    assert!(declaration.get("parametersJsonSchema").is_some());
    assert!(declaration["parametersJsonSchema"].get("$schema").is_none());
    assert_eq!(
        declaration["parametersJsonSchema"]["additionalProperties"],
        false
    );
}

#[test]
fn gemini_to_anthropic_maps_text_and_usage() {
    let input = json!({
        "responseId": "resp_1",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{ "text": "Hello from Gemini" }]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 12,
            "totalTokenCount": 20,
            "cachedContentTokenCount": 3
        }
    });

    let result = gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, None).unwrap();
    assert_eq!(result["id"], "resp_1");
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(result["content"][0]["text"], "Hello from Gemini");
    assert_eq!(result["stop_reason"], "end_turn");
    // input_tokens = promptTokenCount(12) - cachedContentTokenCount(3) = 9（fresh input）。
    // Gemini 的 promptTokenCount 含缓存命中，但 Anthropic 语义要求 input 不含 cache、
    // cache_read 单列；二者相加(9+3)=总输入 12。扣减避免本路径以 app_type=claude
    // 记账时把缓存 token 双重计费。
    assert_eq!(result["usage"]["input_tokens"], 9);
    assert_eq!(result["usage"]["output_tokens"], 8);
    assert_eq!(result["usage"]["cache_read_input_tokens"], 3);
}

#[test]
fn gemini_to_anthropic_maps_function_calls_to_tool_use() {
    let input = json!({
        "responseId": "resp_2",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": {
                        "id": "call_1",
                        "name": "get_weather",
                        "args": { "city": "Tokyo" }
                    }
                }]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "totalTokenCount": 15
        }
    });

    let result = gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "tool_use");
    assert_eq!(result["content"][0]["id"], "call_1");
    assert_eq!(result["stop_reason"], "tool_use");
}

#[test]
fn gemini_to_anthropic_rectifies_tool_args_from_schema_hints() {
    let input = json!({
        "responseId": "resp_2",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": {
                        "id": "call_1",
                        "name": "Skill",
                        "args": {
                            "name": "git-commit",
                            "parameters": {
                                "args": ["详细分析内容 编写提交信息 分多次提交代码"]
                            }
                        }
                    }
                }]
            }
        }]
    });
    let hints = extract_anthropic_tool_schema_hints(&json!({
        "tools": [{
            "name": "Skill",
            "input_schema": {
                "type": "object",
                "properties": {
                    "skill": { "type": "string" },
                    "args": { "type": "string" }
                },
                "required": ["skill"]
            }
        }]
    }));

    let result =
        gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, Some(&hints)).unwrap();

    assert_eq!(result["content"][0]["input"]["skill"], "git-commit");
    assert_eq!(
        result["content"][0]["input"]["args"],
        "详细分析内容 编写提交信息 分多次提交代码"
    );
    assert!(result["content"][0]["input"].get("name").is_none());
    assert!(result["content"][0]["input"].get("parameters").is_none());
}

#[test]
fn gemini_to_anthropic_preserves_legitimate_parameters_arg() {
    let input = json!({
        "responseId": "resp_params",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": {
                        "id": "call_1",
                        "name": "ConfigTool",
                        "args": {
                            "parameters": {
                                "mode": "safe",
                                "retries": 2
                            }
                        }
                    }
                }]
            }
        }]
    });
    let hints = extract_anthropic_tool_schema_hints(&json!({
        "tools": [{
            "name": "ConfigTool",
            "input_schema": {
                "type": "object",
                "properties": {
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "mode": { "type": "string" },
                            "retries": { "type": "integer" }
                        }
                    }
                },
                "required": ["parameters"]
            }
        }]
    }));

    let result =
        gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, Some(&hints)).unwrap();

    assert_eq!(result["content"][0]["input"]["parameters"]["mode"], "safe");
    assert_eq!(result["content"][0]["input"]["parameters"]["retries"], 2);
}

#[test]
fn gemini_to_anthropic_maps_blocked_prompt_to_refusal() {
    let input = json!({
        "responseId": "resp_3",
        "modelVersion": "gemini-2.5-flash",
        "promptFeedback": { "blockReason": "SAFETY" },
        "usageMetadata": {
            "promptTokenCount": 4,
            "totalTokenCount": 4
        }
    });

    let result = gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, None).unwrap();
    assert_eq!(result["stop_reason"], "refusal");
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("SAFETY"));
}

#[test]
fn shadow_replay_aligns_to_latest_turns_after_client_truncation() {
    let store = GeminiShadowStore::with_limits(8, 4);
    // Record 3 shadow turns (assistant messages 0, 1, 2)
    for i in 0..3 {
        store.record_assistant_turn(
            "prov",
            "sess",
            json!({
                "parts": [{
                    "functionCall": {
                        "id": format!("call_{i}"),
                        "name": format!("tool_{i}"),
                        "args": {}
                    }
                }]
            }),
            vec![],
        );
    }

    // Client truncates history: only sends assistant messages 1 and 2
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "tool_1", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "ok" }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_2", "name": "tool_2", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_2", "content": "ok" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();

    // Shadow turns[1] (tool_1) should align with first assistant message,
    // shadow turns[2] (tool_2) with the second — not turns[0] and turns[1].
    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "tool_1"
    );
    assert_eq!(
        result["contents"][2]["parts"][0]["functionCall"]["name"],
        "tool_2"
    );
}

#[test]
fn shadow_replay_matches_tool_use_turn_by_id_when_position_drifts() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "id": "call_1",
                    "name": "Bash",
                    "args": { "command": "ls -R" }
                },
                "thoughtSignature": "sig-tool-1"
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some("call_1"),
            "Bash",
            json!({ "command": "ls -R" }),
            Some("sig-tool-1"),
        )],
    );

    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "call_1",
                        "name": "default_api:Bash",
                        "input": { "command": "ls -R" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_1", "content": "ok" }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "local-only assistant turn without Gemini shadow" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();

    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "Bash"
    );
    assert_eq!(
        result["contents"][0]["parts"][0]["thoughtSignature"],
        "sig-tool-1"
    );
}

/// Regression for P1: two shadow turns whose suffix-normalized names
/// collide (e.g. `server_a:search` / `server_b:search` both normalize to
/// `search`). When the incoming assistant tool_use carries a valid,
/// different id, exact-id matching must win over the normalized-name
/// clause — otherwise replay picks the wrong shadow turn and later
/// tool_result resolution mis-routes.
#[test]
fn shadow_replay_prefers_exact_id_match_over_normalized_name_collision() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "id": "call_a",
                    "name": "server_a:search",
                    "args": { "q": "alpha" }
                },
                "thoughtSignature": "sig-a"
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some("call_a"),
            "server_a:search",
            json!({ "q": "alpha" }),
            Some("sig-a"),
        )],
    );
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "id": "call_b",
                    "name": "server_b:search",
                    "args": { "q": "beta" }
                },
                "thoughtSignature": "sig-b"
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some("call_b"),
            "server_b:search",
            json!({ "q": "beta" }),
            Some("sig-b"),
        )],
    );

    // Two assistant turns: the first references call_b, the second
    // call_a. Positional fallback would align msg[0] to turn 0 (call_a)
    // and msg[1] to turn 1 (call_b) — both wrong. The old `||` chain
    // would also mis-match through the normalized "search" name.
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_b", "name": "server_b:search", "input": { "q": "beta" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_b", "content": "ok-b" }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_a", "name": "server_a:search", "input": { "q": "alpha" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_a", "content": "ok-a" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();

    // msg[0] replays shadow turn 1 (server_b:search) because id=call_b.
    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "server_b:search"
    );
    assert_eq!(
        result["contents"][0]["parts"][0]["thoughtSignature"],
        "sig-b"
    );
    // msg[2] replays shadow turn 0 (server_a:search) because id=call_a,
    // even though turn 1 was already consumed above.
    assert_eq!(
        result["contents"][2]["parts"][0]["functionCall"]["name"],
        "server_a:search"
    );
    assert_eq!(
        result["contents"][2]["parts"][0]["thoughtSignature"],
        "sig-a"
    );
}

/// When the incoming tool_use carries no id (or only empty-string ids),
/// the layered matcher must still fall back to name-based matching so
/// that shadow replay keeps working for providers that omit ids.
#[test]
fn shadow_replay_falls_back_to_name_when_ids_absent() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "name": "lookup",
                    "args": {}
                },
                "thoughtSignature": "sig-lookup"
            }]
        }),
        vec![GeminiToolCallMeta::new(
            None::<&str>,
            "lookup",
            json!({}),
            Some("sig-lookup"),
        )],
    );

    // id is an empty string; extract_assistant_tool_use_keys filters it
    // out, so tool_use_ids is empty and matching must go through names.
    // A trailing user text turn keeps the assistant turn well-formed
    // without feeding a tool_result back (which would require a real id).
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "", "name": "lookup", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": "ack"
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();

    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "lookup"
    );
    assert_eq!(
        result["contents"][0]["parts"][0]["thoughtSignature"],
        "sig-lookup"
    );
}

/// Regression for P1: Gemini 2.x may return parallel calls without ids.
/// Each Anthropic-visible tool_use must carry a unique id so the Claude
/// Code client can map tool_result responses back correctly.
#[test]
fn gemini_to_anthropic_synthesizes_unique_ids_for_missing_functioncall_ids() {
    let input = json!({
        "responseId": "r1",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [
                    { "functionCall": { "name": "foo", "args": {} } },
                    { "functionCall": { "name": "foo", "args": { "k": 1 } } }
                ]
            }
        }]
    });

    let result = gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, None).unwrap();
    let id0 = result["content"][0]["id"].as_str().unwrap();
    let id1 = result["content"][1]["id"].as_str().unwrap();
    assert!(is_synthesized_tool_call_id(id0));
    assert!(is_synthesized_tool_call_id(id1));
    assert_ne!(id0, id1, "synthesized ids must be unique per call");
}

/// Ensures the proxy does not leak synthesized ids back to Gemini when
/// Claude Code replies with a tool_result: the id must be stripped from
/// both `functionCall.id` and `functionResponse.id`.
#[test]
fn tool_result_with_synthesized_id_omits_id_in_gemini_request() {
    let synth = synthesize_tool_call_id();
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": &synth, "name": "get_weather", "input": { "city": "X" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": &synth, "content": "sunny" }
                ]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    let fc = &result["contents"][0]["parts"][0]["functionCall"];
    assert!(
        fc.get("id").is_none(),
        "synthesized id must not leak upstream in functionCall"
    );
    assert_eq!(fc["name"], "get_weather");
    let fr = &result["contents"][1]["parts"][0]["functionResponse"];
    assert!(
        fr.get("id").is_none(),
        "synthesized id must not leak upstream in functionResponse"
    );
    assert_eq!(fr["name"], "get_weather");
}

/// Genuine Gemini-assigned ids must round-trip unchanged so that Gemini
/// can correlate the tool result with its own prior functionCall entry.
#[test]
fn tool_result_with_genuine_gemini_id_round_trips() {
    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_real_1", "name": "get_weather", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_real_1", "content": "ok" }
                ]
            }
        ]
    });

    let result = anthropic_to_gemini_with_shadow(input, None, None, None).unwrap();
    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["id"],
        "call_real_1"
    );
    assert_eq!(
        result["contents"][1]["parts"][0]["functionResponse"]["id"],
        "call_real_1"
    );
}

/// Shadow replay must also strip synthesized ids when it reconstructs
/// the assistant's `functionCall` parts from a previously recorded turn.
#[test]
fn shadow_replay_strips_synthesized_id_from_function_call() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let synth = synthesize_tool_call_id();
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "id": &synth,
                    "name": "get_weather",
                    "args": { "city": "Tokyo" }
                }
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some(synth.clone()),
            "get_weather",
            json!({ "city": "Tokyo" }),
            None::<String>,
        )],
    );

    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": &synth, "name": "get_weather", "input": { "city": "Tokyo" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": &synth, "content": "sunny" }
                ]
            }
        ]
    });

    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();
    // The assistant message was replayed from shadow; its synthesized id
    // must be absent from the upstream functionCall representation.
    assert!(result["contents"][0]["parts"][0]["functionCall"]
        .get("id")
        .is_none());
    // And the tool_result round-trip must still resolve the name via the
    // shadow map even when the id is synthesized.
    assert_eq!(
        result["contents"][1]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
}

// ------------------------------------------------------------------
// Non-streaming shadow id coherence regressions.
//
// When Gemini returns a `functionCall` without an id (common in 2.x
// parallel calls) the proxy must synthesize a single id that is
// consistent across:
//   (a) the Anthropic `content[tool_use].id` sent to the client
//   (b) `shadow_content.parts[].functionCall.id` recorded in shadow
//   (c) `shadow_turn.tool_calls[].id` recorded in shadow
// Previously the non-streaming path generated independent UUIDs in (a)
// and (c), so the next round's `tool_result(tool_use_id=A)` would
// fail to resolve through `tool_name_by_id` (populated from (c)).
// ------------------------------------------------------------------

/// The id surfaced to the Anthropic client must equal the id recorded
/// in the shadow's `tool_calls` metadata and the shadow's serialized
/// `functionCall.id`. All three are read back as the same string.
#[test]
fn non_stream_shadow_id_matches_client_visible_id() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let body = json!({
        "responseId": "r-coherence",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": { "name": "get_weather", "args": { "city": "Tokyo" } }
                }]
            }
        }]
    });

    let response = gemini_to_anthropic_with_shadow_and_hints(
        body,
        Some(&store),
        Some("prov"),
        Some("sess"),
        None,
    )
    .unwrap();

    let client_id = response["content"][0]["id"].as_str().unwrap();
    assert!(
        is_synthesized_tool_call_id(client_id),
        "client-facing id must be synthesized for no-id Gemini responses"
    );

    let snapshot = store.get_session("prov", "sess").expect("shadow recorded");
    // (c) tool_calls metadata must agree with the client-visible id.
    let shadow_tool_call_id = snapshot.turns[0].tool_calls[0]
        .id
        .as_deref()
        .expect("tool_calls id populated");
    assert_eq!(
        shadow_tool_call_id, client_id,
        "shadow.tool_calls id must equal client-visible id"
    );
    // (b) assistant_content parts must agree too, so that
    // `merge_tool_names_from_parts` sees the same id on replay.
    let shadow_part_id = snapshot.turns[0].assistant_content["parts"][0]["functionCall"]["id"]
        .as_str()
        .expect("assistant_content functionCall id populated");
    assert_eq!(
        shadow_part_id, client_id,
        "shadow assistant_content functionCall.id must equal client-visible id"
    );
}

/// Scenario A: the client-side history was truncated so the next
/// request only contains `[tool_result(tool_use_id=A)]` without a
/// preceding assistant echo. The request must still resolve because
/// `build_tool_name_map_from_shadow_turns` now surfaces the same id
/// the client was given.
#[test]
fn non_stream_missing_id_scenario_a_truncated_history_resolves() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let turn1 = json!({
        "responseId": "r-truncated",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": { "name": "get_weather", "args": { "city": "Tokyo" } }
                }]
            }
        }]
    });
    let anthropic_response = gemini_to_anthropic_with_shadow_and_hints(
        turn1,
        Some(&store),
        Some("prov"),
        Some("sess"),
        None,
    )
    .unwrap();
    let client_id = anthropic_response["content"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Turn 2 — client replays ONLY the tool_result. No assistant echo.
    let turn2_input = json!({
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": &client_id, "content": "sunny" }
                ]
            }
        ]
    });
    let result =
        anthropic_to_gemini_with_shadow(turn2_input, Some(&store), Some("prov"), Some("sess"))
            .expect("scenario A must resolve tool name through shadow");
    assert_eq!(
        result["contents"][0]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
}

/// Scenario B: the client replays the full history. The proxy picks
/// the shadow-replay branch (not `convert_message_content_to_parts`),
/// which strips the synthesized id from the outgoing `functionCall`.
/// `tool_name_by_id` must still have been populated from the shadow
/// so the following `tool_result(A)` resolves.
#[test]
fn non_stream_missing_id_scenario_b_full_history_replay_resolves() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let turn1 = json!({
        "responseId": "r-full",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": { "name": "get_weather", "args": { "city": "Tokyo" } }
                }]
            }
        }]
    });
    let anthropic_response = gemini_to_anthropic_with_shadow_and_hints(
        turn1,
        Some(&store),
        Some("prov"),
        Some("sess"),
        None,
    )
    .unwrap();
    let client_id = anthropic_response["content"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Turn 2 — full history: assistant tool_use + tool_result.
    let turn2_input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": &client_id,
                        "name": "get_weather",
                        "input": { "city": "Tokyo" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": &client_id, "content": "sunny" }
                ]
            }
        ]
    });
    let result =
        anthropic_to_gemini_with_shadow(turn2_input, Some(&store), Some("prov"), Some("sess"))
            .expect("scenario B must resolve tool name through shadow replay");

    // Shadow-replay path: `functionCall.id` is stripped for the
    // assistant turn (the synthesized id must not leak upstream).
    assert!(
        result["contents"][0]["parts"][0]["functionCall"]
            .get("id")
            .is_none(),
        "synthesized id must not leak to Gemini in shadow replay"
    );
    assert_eq!(
        result["contents"][0]["parts"][0]["functionCall"]["name"],
        "get_weather"
    );
    // The tool_result round-trip resolves through the shadow map.
    assert_eq!(
        result["contents"][1]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
}

/// Regression: when Gemini returns an id, nothing is synthesized.
/// The original id is round-tripped in both the Anthropic response
/// and the shadow store, and it flows back to Gemini on the next
/// functionResponse.
#[test]
fn non_stream_preserves_original_gemini_id_when_present() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let body = json!({
        "responseId": "r-preserve",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{
            "finishReason": "STOP",
            "content": {
                "parts": [{
                    "functionCall": {
                        "id": "call_real_1",
                        "name": "get_weather",
                        "args": { "city": "Tokyo" }
                    }
                }]
            }
        }]
    });

    let response = gemini_to_anthropic_with_shadow_and_hints(
        body,
        Some(&store),
        Some("prov"),
        Some("sess"),
        None,
    )
    .unwrap();
    assert_eq!(response["content"][0]["id"], "call_real_1");
    let snapshot = store.get_session("prov", "sess").unwrap();
    assert_eq!(
        snapshot.turns[0].tool_calls[0].id.as_deref(),
        Some("call_real_1")
    );
    assert_eq!(
        snapshot.turns[0].assistant_content["parts"][0]["functionCall"]["id"],
        "call_real_1"
    );
}

/// Defensive: if a shadow turn somehow carries a synthesized
/// `functionCall.id` (e.g. recorded by this path), replaying it via
/// `anthropic_to_gemini_with_shadow` must strip the id before sending
/// upstream, so Gemini never sees the internal identifier.
#[test]
fn non_stream_synthesized_id_not_leaked_to_gemini_via_shadow_replay() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let synth = synthesize_tool_call_id();
    store.record_assistant_turn(
        "prov",
        "sess",
        json!({
            "parts": [{
                "functionCall": {
                    "id": &synth,
                    "name": "get_weather",
                    "args": { "city": "Tokyo" }
                }
            }]
        }),
        vec![GeminiToolCallMeta::new(
            Some(synth.clone()),
            "get_weather",
            json!({ "city": "Tokyo" }),
            None::<String>,
        )],
    );

    let input = json!({
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": &synth,
                        "name": "get_weather",
                        "input": { "city": "Tokyo" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": &synth, "content": "sunny" }
                ]
            }
        ]
    });
    let result =
        anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess")).unwrap();
    assert!(
        result["contents"][0]["parts"][0]["functionCall"]
            .get("id")
            .is_none(),
        "shadow replay must strip synthesized functionCall.id"
    );
    assert!(
        result["contents"][1]["parts"][0]["functionResponse"]
            .get("id")
            .is_none(),
        "functionResponse.id must also be omitted for synthesized ids"
    );
}
