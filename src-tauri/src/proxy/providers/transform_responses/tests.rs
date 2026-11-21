use super::*;

#[test]
fn test_text_with_url_citations_preserves_unicode_character_ranges() {
    let text = "查看 Rust 文档和 Cargo。";
    let annotations = json!([
        {
            "type": "url_citation",
            "start_index": 3,
            "end_index": 7,
            "url": "https://www.rust-lang.org/",
            "title": "Rust"
        },
        {
            "type": "url_citation",
            "start_index": 12,
            "end_index": 17,
            "url": "https://doc.rust-lang.org/cargo/",
            "title": "Cargo"
        }
    ]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "查看 [Rust](https://www.rust-lang.org/) 文档和 [Cargo](https://doc.rust-lang.org/cargo/)。"
    );
}

#[test]
fn test_text_with_url_citations_preserves_non_space_whitespace_in_ranges() {
    let text = "Line one\nLine\ttwo.";
    let annotations = json!([{
        "type": "url_citation",
        "start_index": 0,
        "end_index": text.chars().count(),
        "url": "https://example.com/source",
        "title": "Source"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "Line one\nLine\ttwo.\n\nSources: [Source](https://example.com/source)"
    );
}

#[test]
fn test_text_with_url_citations_accepts_mixed_case_http_scheme() {
    let annotations = json!([{
        "type": "url_citation",
        "start_index": 0,
        "end_index": 6,
        "url": "HTTPS://example.com/source",
        "title": "Source"
    }]);

    assert_eq!(
        text_with_url_citations("Source", annotations.as_array().unwrap()),
        "[Source](HTTPS://example.com/source)"
    );
}

#[test]
fn test_text_with_url_citations_handles_many_unmatched_link_closers() {
    let text = "x](".repeat(4096);
    assert_eq!(text_with_url_citations(&text, &[]), text);

    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/source",
        "title": "Source"
    }]);
    let rendered = text_with_url_citations(&text, annotations.as_array().unwrap());
    assert!(rendered.starts_with(&text));
    assert!(rendered.ends_with("\n\nSources: [Source](https://example.com/source)"));
}

#[test]
fn test_text_with_url_citations_preserves_existing_link_to_same_url() {
    let existing_link = "([OpenAI docs](https://platform.openai.com/docs))";
    let text = format!("See {existing_link} for details.");
    let start_index = "See ".chars().count();
    let end_index = start_index + existing_link.chars().count();
    let annotations = json!([{
        "type": "url_citation",
        "start_index": start_index,
        "end_index": end_index,
        "url": "https://platform.openai.com/docs",
        "title": "OpenAI docs"
    }]);

    assert_eq!(
        text_with_url_citations(&text, annotations.as_array().unwrap()),
        text
    );
}

#[test]
fn test_markdown_link_detection_requires_a_complete_link() {
    let url = "https://example.com/docs";
    assert!(contains_markdown_link_to_url(
        "[Docs](https://example.com/docs)",
        url,
        url
    ));
    assert!(contains_markdown_link_to_url(
        "[Docs](https://example.com/docs \"Documentation\")",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "orphan ](https://example.com/docs)",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "[Docs](https://example.com/docs",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "[Docs](https://example.com/docs trailing",
        url,
        url
    ));
}

#[test]
fn test_markdown_link_detection_handles_autolinks_and_balanced_parentheses() {
    let parenthesized_url = "https://example.com/docs_(latest)";
    let rendered_url = markdown_link_destination(parenthesized_url).unwrap();
    assert!(contains_markdown_link_to_url(
        "[Docs](https://example.com/docs_(latest))",
        parenthesized_url,
        &rendered_url
    ));

    let url = "https://example.com/docs";
    assert!(contains_markdown_link_to_url(
        "<https://example.com/docs>",
        url,
        url
    ));
}

#[test]
fn test_markdown_link_detection_ignores_code_examples() {
    let url = "https://example.com/docs";
    assert!(!contains_markdown_link_to_url(
        "`[Docs](https://example.com/docs)`",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "```md\n[Docs](https://example.com/docs)\n```",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "`` `<https://example.com/docs>` ``",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        r"`[Docs](https://example.com/docs)\`",
        url,
        url
    ));
    assert!(contains_markdown_link_to_url(
        "```literal```\n[Docs](https://example.com/docs)",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "    [Docs](https://example.com/docs)",
        url,
        url
    ));
    assert!(!contains_markdown_link_to_url(
        "`[` orphan ](https://example.com/docs)",
        url,
        url
    ));
    for nested_fence in [
        "> ```md\n> [Docs](https://example.com/docs)\n> ```",
        "- ```md\n  [Docs](https://example.com/docs)\n  ```",
        "1. ```md\n   [Docs](https://example.com/docs)\n   ```",
        "> - ```md\n>   [Docs](https://example.com/docs)\n>   ```",
    ] {
        assert!(
            !contains_markdown_link_to_url(nested_fence, url, url),
            "link inside nested fence should remain masked: {nested_fence}"
        );
    }
}

#[test]
fn test_markdown_link_detection_ends_fence_with_its_container() {
    let url = "https://example.com/docs";
    for text in [
        "> ```md\n> code\n[Docs](https://example.com/docs)",
        "- ```md\n  code\n[Docs](https://example.com/docs)",
        "> - ```md\n>   code\n[Docs](https://example.com/docs)",
    ] {
        assert!(
            contains_markdown_link_to_url(text, url, url),
            "top-level link after nested fence should remain visible: {text}"
        );
    }
}

#[test]
fn test_markdown_link_detection_distinguishes_images_and_references() {
    let url = "https://example.com/docs";
    assert!(!contains_markdown_link_to_url(
        "![Docs](https://example.com/docs)",
        url,
        url
    ));
    assert!(contains_markdown_link_to_url(
        "[Docs][reference]\n\n[reference]: https://example.com/docs",
        url,
        url
    ));
    assert!(contains_markdown_link_to_url(
        "[reference][]\n\n[reference]: <https://example.com/docs>",
        url,
        url
    ));
}

#[test]
fn test_text_with_url_citations_escapes_untrusted_fallback_title() {
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "<img src=x onerror=alert(1)>\nDocs"
    }]);

    let rendered = text_with_url_citations("Answer.", annotations.as_array().unwrap());
    assert_eq!(
        rendered,
        "Answer.\n\nSources: [&lt;img src\\=x onerror\\=alert\\(1\\)&gt; Docs](https://example.com/docs)"
    );
}

#[test]
fn test_text_with_url_citations_deduplicates_ranged_urls() {
    let annotations = json!([
        {
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://example.com/docs",
            "title": "Docs"
        },
        {
            "type": "url_citation",
            "start_index": 9,
            "end_index": 14,
            "url": "https://example.com/docs",
            "title": "Duplicate"
        }
    ]);

    assert_eq!(
        text_with_url_citations("Rust and Cargo", annotations.as_array().unwrap()),
        "[Rust](https://example.com/docs) and Cargo"
    );
}

#[test]
fn test_text_with_url_citations_does_not_repeat_existing_body_link() {
    let text = "See [Docs](https://example.com/docs) for details.";
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "Docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        text
    );
}

#[test]
fn test_text_with_url_citations_keeps_fallback_after_code_example() {
    let text = "`[Docs](https://example.com/docs)`";
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "Docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "`[Docs](https://example.com/docs)`\n\nSources: [Docs](https://example.com/docs)"
    );
}

#[test]
fn test_text_with_url_citations_keeps_fallback_after_nested_fence() {
    let text = "> ```md\n> [Docs](https://example.com/docs)\n> ```";
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "Docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "> ```md\n> [Docs](https://example.com/docs)\n> ```\n\nSources: [Docs](https://example.com/docs)"
    );
}

#[test]
fn test_text_with_url_citations_does_not_open_fence_after_unclosed_nested_fence() {
    let text = "> ```md\n> code";
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "Docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "> ```md\n> code\n\nSources: [Docs](https://example.com/docs)"
    );
}

#[test]
fn test_text_with_url_citations_closes_truncated_fence_before_fallback() {
    let text = "```text\npartial output";
    let annotations = json!([{
        "type": "url_citation",
        "url": "https://example.com/docs",
        "title": "Docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "```text\npartial output\n```\n\nSources: [Docs](https://example.com/docs)"
    );
}

#[test]
fn test_text_with_url_citations_does_not_nest_inside_existing_link() {
    let text = "[Docs](https://example.com/old)";
    let annotations = json!([{
        "type": "url_citation",
        "start_index": 1,
        "end_index": 5,
        "url": "https://example.com/new",
        "title": "New docs"
    }]);

    assert_eq!(
        text_with_url_citations(text, annotations.as_array().unwrap()),
        "[Docs](https://example.com/old)\n\nSources: [New docs](https://example.com/new)"
    );
}

#[test]
fn test_text_with_url_citations_appends_safe_fallback_links() {
    let annotations = json!([
        {
            "type": "url_citation",
            "url": "https://example.com/docs_(latest)",
            "title": "Example [Docs]"
        },
        {
            "type": "url_citation",
            "url": "javascript:alert(1)",
            "title": "Unsafe"
        }
    ]);

    assert_eq!(
        text_with_url_citations("Answer.", annotations.as_array().unwrap()),
        "Answer.\n\nSources: [Example \\[Docs\\]](https://example.com/docs_%28latest%29)"
    );
}

#[test]
fn test_anthropic_to_responses_simple() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["model"], "gpt-4o");
    assert_eq!(result["max_output_tokens"], 1024);
    assert_eq!(result["input"][0]["role"], "user");
    assert_eq!(result["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(result["input"][0]["content"][0]["text"], "Hello");
    // stop_sequences should not appear
    assert!(result.get("stop_sequences").is_none());
}

#[test]
fn test_anthropic_to_responses_with_system_string() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["instructions"], "You are a helpful assistant.");
    // system should not appear in input
    assert_eq!(result["input"].as_array().unwrap().len(), 1);
}

#[test]
fn test_anthropic_to_responses_strips_leading_billing_header_from_system_string() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nYou are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["instructions"], "You are a helpful assistant.");
}

#[test]
fn test_anthropic_to_responses_strips_billing_header_with_crlf() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\r\n\r\nYou are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["instructions"], "You are a helpful assistant.");
}

#[test]
fn test_anthropic_to_responses_keeps_non_leading_billing_header_text() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": "Keep this literal:\nx-anthropic-billing-header: example",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(
        result["instructions"],
        "Keep this literal:\nx-anthropic-billing-header: example"
    );
}

#[test]
fn test_anthropic_to_responses_with_system_array() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": [
            {"type": "text", "text": "Part 1"},
            {"type": "text", "text": "Part 2"}
        ],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["instructions"], "Part 1\n\nPart 2");
}

#[test]
fn test_anthropic_to_responses_strips_billing_header_from_system_array_parts() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": [
            {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n"},
            {"type": "text", "text": "Stable prompt"}
        ],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["instructions"], "Stable prompt");
}

#[test]
fn test_anthropic_to_responses_preserves_prompt_after_billing_header_in_same_part() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "system": [
            {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nStable prompt part 1"},
            {"type": "text", "text": "Stable prompt part 2"}
        ],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(
        result["instructions"],
        "Stable prompt part 1\n\nStable prompt part 2"
    );
}

#[test]
fn test_anthropic_to_responses_with_tools() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tools": [{
            "name": "get_weather",
            "description": "Get weather info",
            "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["tools"][0]["type"], "function");
    // 省略 strict 会被 Responses 后端按 true 严格化，可选参数被强制输出（#7713）
    assert_eq!(result["tools"][0]["strict"], json!(false));
    assert_eq!(result["tools"][0]["name"], "get_weather");
    assert!(result["tools"][0].get("parameters").is_some());
    assert_eq!(result["tools"][0]["parameters"]["type"], json!("object"));
    assert_eq!(
        result["tools"][0]["parameters"]["properties"]["location"]["type"],
        json!("string")
    );
    // input_schema should not appear
    assert!(result["tools"][0].get("input_schema").is_none());
}

#[test]
fn test_anthropic_to_responses_marks_function_tools_not_strict() {
    // #7713：可选参数（required 是任意子集）必须保留可选语义。
    // 省略 strict 时 Responses 后端按 true 严格化，模型被迫输出 isolation。
    let input = json!({
        "model": "gpt-5.6-luna",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Call the agent"}],
        "tools": [{
            "name": "Agent",
            "description": "Launch a sub-agent",
            "input_schema": {
                "type": "object",
                "properties": {
                    "description": {"type": "string"},
                    "prompt": {"type": "string"},
                    "isolation": {"type": "string", "enum": ["worktree", "remote"]}
                },
                "required": ["description", "prompt"]
            }
        }]
    });

    let plain = anthropic_to_responses(input.clone(), None, false, false).unwrap();
    assert_eq!(plain["tools"][0]["strict"], json!(false));
    // required 保持为原有子集，strict 语义交由后端保留
    assert_eq!(
        plain["tools"][0]["parameters"]["required"],
        json!(["description", "prompt"])
    );

    // 同一转换分支也服务 Codex OAuth 后端
    let oauth = anthropic_to_responses(input, None, true, false).unwrap();
    assert_eq!(oauth["tools"][0]["strict"], json!(false));
    assert_eq!(
        oauth["tools"][0]["parameters"]["required"],
        json!(["description", "prompt"])
    );
}

#[test]
fn test_anthropic_to_responses_omits_missing_tool_description() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [
            {"name": "NoDesc", "input_schema": {"type": "object"}},
            {"name": "Described", "description": "Has one",
             "input_schema": {"type": "object"}}
        ]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    // 缺 description：省略字段而不是序列化成 null（严格上游会 400）
    assert!(tools[0].get("description").is_none());
    assert!(tools[0].get("parameters").is_some());
    assert_eq!(tools[1]["description"], json!("Has one"));
}

#[test]
fn test_anthropic_hosted_web_search_maps_newer_direct_version_to_codex_tool() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [{"role": "user", "content": "Search Rust docs"}],
        "tools": [{
            "type": "web_search_20260318",
            "name": "web_search_next",
            "allowed_callers": ["direct"],
            "max_uses": 8,
            "allowed_domains": ["rust-lang.org"],
            "blocked_domains": []
        }],
        "tool_choice": {"type": "tool", "name": "web_search_next"}
    });

    assert_eq!(
        anthropic_web_search_tool_name(&input),
        Some("web_search_next")
    );
    let result = anthropic_to_responses(input, None, true, false).unwrap();
    assert_eq!(
        result["tools"][0],
        json!({
            "type": "web_search",
            "external_web_access": true,
            "filters": {"allowed_domains": ["rust-lang.org"]}
        })
    );
    assert_eq!(result["tool_choice"], "required");
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("no more than 8 web search calls"));
    assert_eq!(
        result["include"],
        json!([
            "reasoning.encrypted_content",
            "web_search_call.action.sources"
        ])
    );
}

#[test]
fn test_anthropic_hosted_web_search_uses_responses_selector_for_api_key_backend() {
    let input = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{"type": "web_search_20250305", "name": "web_search"}],
        "tool_choice": {"type": "tool", "name": "web_search"}
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["tool_choice"], json!({"type": "web_search"}));
    assert_eq!(result["include"], json!(["web_search_call.action.sources"]));
    assert!(result["tools"][0].get("external_web_access").is_none());
}

#[test]
fn xai_hosted_web_search_choice_becomes_required_and_drops_other_tools() {
    let mut body = json!({
        "model": "grok-4.5",
        "tools": [
            {"type": "function", "name": "Bash"},
            {"type": "web_search"}
        ],
        "tool_choice": {"type": "web_search"}
    });

    assert!(rewrite_xai_hosted_web_search_tool_choice(&mut body));
    assert_eq!(body["tool_choice"], json!("required"));
    assert_eq!(body["tools"], json!([{"type": "web_search"}]));
    assert!(!rewrite_xai_hosted_web_search_tool_choice(&mut body));
}

#[test]
fn xai_rewrite_leaves_string_and_function_tool_choice() {
    let mut auto = json!({"tool_choice": "auto", "tools": [{"type": "function", "name": "Bash"}]});
    assert!(!rewrite_xai_hosted_web_search_tool_choice(&mut auto));
    assert_eq!(auto["tool_choice"], json!("auto"));

    let mut function = json!({
        "tool_choice": {"type": "function", "name": "Bash"},
        "tools": [{"type": "function", "name": "Bash"}]
    });
    assert!(!rewrite_xai_hosted_web_search_tool_choice(&mut function));
    assert_eq!(function["tool_choice"]["name"], json!("Bash"));
}

#[test]
fn xai_rewrite_drops_choice_when_no_web_search_tool_survives() {
    let mut body = json!({
        "tools": [{"type": "function", "name": "Bash"}],
        "tool_choice": {"type": "web_search"}
    });
    assert!(rewrite_xai_hosted_web_search_tool_choice(&mut body));
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("tools").is_none());
}

#[test]
fn test_api_key_hosted_web_search_maps_max_uses_to_max_tool_calls() {
    let input = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 3
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["max_tool_calls"], 3);
}

#[test]
fn test_codex_forced_hosted_web_search_max_uses_is_enforced_locally() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 3
        }],
        "tool_choice": {"type": "tool", "name": "web_search"}
    });

    let result = anthropic_to_responses(input, None, true, false).unwrap();
    assert!(result.get("max_tool_calls").is_none());
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("no more than 3 web search calls"));
}

#[test]
fn test_codex_auto_hosted_web_search_max_uses_enforced_via_instructions() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [
            {
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 3
            },
            {
                "name": "get_weather",
                "description": "A client-side function",
                "input_schema": {"type": "object"}
            }
        ]
    });

    // An unforced hosted search must coexist with client-side tools.
    let result = anthropic_to_responses(input, None, true, false).unwrap();
    assert!(result.get("max_tool_calls").is_none());
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("no more than 3 web search calls"));
    // Unrelated tools are preserved when the hosted tool is not forced.
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert!(tools
        .iter()
        .any(|tool| tool["type"] == "web_search" && tool["external_web_access"] == true));
    assert!(tools
        .iter()
        .any(|tool| tool["type"] == "function" && tool["name"] == "get_weather"));
}

#[test]
fn test_codex_forced_hosted_web_search_filters_unrelated_tools() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [
            {
                "type": "web_search_20250305",
                "name": "web_search_legacy"
            },
            {
                "type": "web_search_20260318",
                "name": "web_search_future",
                "allowed_callers": ["direct"]
            },
            {
                "name": "get_weather",
                "description": "A client-side function",
                "input_schema": {"type": "object"}
            }
        ],
        "tool_choice": {"type": "tool", "name": "web_search_future"}
    });

    assert_eq!(
        anthropic_web_search_tool_name(&input),
        Some("web_search_future")
    );
    let result = anthropic_to_responses(input, None, true, false).unwrap();
    assert_eq!(result["tool_choice"], "required");
    assert_eq!(result["tools"].as_array().unwrap().len(), 1);
    assert_eq!(result["tools"][0]["type"], "web_search");
    assert_eq!(result["tools"][0]["external_web_access"], true);
}

#[test]
fn test_anthropic_to_responses_replays_hosted_web_search_context() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "server_tool_use",
                        "id": "ws_previous",
                        "name": "web_search_next_version",
                        "input": {"queries": ["Rust ownership", "Rust borrowing"]},
                        "caller": {"type": "direct"}
                    },
                    {
                        "type": "web_search_tool_result",
                        "tool_use_id": "ws_previous",
                        "content": [
                            {
                                "type": "web_search_result",
                                "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
                                "title": "Understanding Ownership",
                                "encrypted_content": "",
                                "page_age": null
                            },
                            {
                                "type": "web_search_result",
                                "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
                                "title": "Duplicate",
                                "encrypted_content": "",
                                "page_age": null
                            }
                        ],
                        "caller": {"type": "direct"}
                    },
                    {"type": "text", "text": "Rust ownership is documented here."}
                ]
            },
            {
                "role": "user",
                "content": "What did that source say about borrowing?"
            }
        ],
        "tools": [{
            "type": "web_search_20260318",
            "name": "web_search_next_version",
            "allowed_callers": ["direct"]
        }]
    });

    let result = anthropic_to_responses(input, None, true, false).unwrap();
    let replayed = result["input"].as_array().unwrap();

    assert_eq!(
        replayed[0],
        json!({
            "type": "web_search_call",
            "id": "ws_previous",
            "status": "completed",
            "action": {
                "type": "search",
                "queries": ["Rust ownership", "Rust borrowing"],
                "sources": [{
                    "type": "url",
                    "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html"
                }]
            }
        })
    );
    assert_eq!(replayed[1]["role"], "assistant");
    assert_eq!(
        replayed[1]["content"][0],
        json!({
            "type": "output_text",
            "text": "Rust ownership is documented here."
        })
    );
    assert_eq!(replayed[2]["role"], "user");
    assert_eq!(
        replayed[2]["content"][0]["text"],
        "What did that source say about borrowing?"
    );
}

#[test]
fn test_custom_function_named_web_search_remains_a_function() {
    let input = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "Call my tool"}],
        "tools": [{
            "name": "web_search",
            "description": "A user-defined function",
            "input_schema": {"type": "object"}
        }],
        "tool_choice": {"type": "tool", "name": "web_search"}
    });

    let result = anthropic_to_responses(input, None, true, false).unwrap();
    assert_eq!(result["tools"][0]["type"], "function");
    assert_eq!(result["tools"][0]["name"], "web_search");
    assert_eq!(
        result["tool_choice"],
        json!({"type": "function", "name": "web_search"})
    );
}

#[test]
fn test_hosted_web_search_blocked_domains_maps_to_responses_filters() {
    let input = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{
            "type": "web_search_20260209",
            "name": "web_search",
            "allowed_callers": ["direct"],
            "blocked_domains": ["doc.rust-lang.org", "docs.rs", "github.com"]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(
        result["tools"][0],
        json!({
            "type": "web_search",
            "filters": {
                "blocked_domains": ["doc.rust-lang.org", "docs.rs", "github.com"]
            }
        })
    );
}

#[test]
fn test_hosted_web_search_blocked_domains_fails_closed_on_codex_oauth() {
    let input = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{
            "type": "web_search_20250305",
            "name": "web_search",
            "blocked_domains": ["example.com"]
        }]
    });

    let error = anthropic_to_responses(input, None, true, false).unwrap_err();
    assert!(error.to_string().contains("blocked_domains"));
}

#[test]
fn test_dynamic_filtering_web_search_requires_explicit_direct_caller() {
    for tool_type in ["web_search_20260209", "web_search_20260318"] {
        let input = json!({
            "model": "gpt-5.6",
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{"type": tool_type, "name": "web_search"}]
        });

        let error = anthropic_to_responses(input, None, true, false).unwrap_err();
        assert!(error.to_string().contains("defaults to code execution"));
        assert!(error.to_string().contains("[\"direct\"]"));
    }
}

#[test]
fn test_non_direct_web_search_callers_fail_closed() {
    for allowed_callers in [
        json!(["code_execution_20260120"]),
        json!(["direct", "code_execution_20260120"]),
        json!("direct"),
    ] {
        let input = json!({
            "model": "gpt-5.6",
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "allowed_callers": allowed_callers
            }]
        });

        let error = anthropic_to_responses(input, None, true, false).unwrap_err();
        assert!(error.to_string().contains("allowed_callers"));
        assert!(error.to_string().contains("exactly"));
    }
}

#[test]
fn test_web_search_response_inclusion_fails_closed() {
    for response_inclusion in ["full", "excluded"] {
        let input = json!({
            "model": "gpt-5.6",
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{
                "type": "web_search_20260318",
                "name": "web_search",
                "allowed_callers": ["direct"],
                "response_inclusion": response_inclusion
            }]
        });

        let error = anthropic_to_responses(input, None, true, false).unwrap_err();
        assert!(error.to_string().contains("response_inclusion"));
    }
}

#[test]
fn test_unknown_web_search_version_is_recognized_but_fails_closed() {
    let input = json!({
        "model": "gpt-5.6",
        "messages": [{"role": "user", "content": "Search"}],
        "tools": [{
            "type": "web_search_20991231",
            "name": "web_search_future",
            "allowed_callers": ["direct"]
        }]
    });

    assert_eq!(
        anthropic_web_search_tool_name(&input),
        Some("web_search_future")
    );
    let error = anthropic_to_responses(input, None, true, false).unwrap_err();
    assert!(error.to_string().contains("version 'web_search_20991231'"));
    assert!(error.to_string().contains("not supported"));
}

#[test]
fn test_anthropic_to_responses_defaults_missing_tool_schema_type() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tools": [{
            "name": "get_weather",
            "description": "Get weather info",
            "input_schema": {"properties": {"location": {"type": "string"}}}
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let parameters = &result["tools"][0]["parameters"];
    assert_eq!(parameters["type"], json!("object"));
    assert_eq!(
        parameters["properties"]["location"]["type"],
        json!("string")
    );
}

#[test]
fn test_anthropic_to_responses_defaults_empty_tool_schema() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Do work"}],
        "tools": [{"name": "do_work", "input_schema": {}}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let parameters = &result["tools"][0]["parameters"];
    assert_eq!(parameters, &json!({"type": "object", "properties": {}}));
}

#[test]
fn test_anthropic_to_responses_tool_choice_any_to_required() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tool_choice": {"type": "any"}
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["tool_choice"], "required");
}

#[test]
fn test_anthropic_to_responses_tool_choice_tool_to_function() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tool_choice": {"type": "tool", "name": "get_weather"}
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["tool_choice"]["type"], "function");
    assert_eq!(result["tool_choice"]["name"], "get_weather");
}

#[test]
fn test_anthropic_to_responses_tool_use_lifting() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check"},
                {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
            ]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let input_arr = result["input"].as_array().unwrap();

    // Should produce: assistant message (text) + function_call item
    assert_eq!(input_arr.len(), 2);

    // First: assistant message with output_text
    assert_eq!(input_arr[0]["role"], "assistant");
    assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
    assert_eq!(input_arr[0]["content"][0]["text"], "Let me check");

    // Second: function_call item (lifted from message)
    assert_eq!(input_arr[1]["type"], "function_call");
    assert_eq!(input_arr[1]["call_id"], "call_123");
    assert_eq!(input_arr[1]["name"], "get_weather");
}

#[test]
fn test_anthropic_to_responses_tool_result_lifting() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
            ]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let input_arr = result["input"].as_array().unwrap();

    // Should produce: function_call_output item (lifted)
    assert_eq!(input_arr.len(), 1);
    assert_eq!(input_arr[0]["type"], "function_call_output");
    assert_eq!(input_arr[0]["call_id"], "call_123");
    assert_eq!(input_arr[0]["output"], "Sunny, 25°C");
}

#[test]
fn test_anthropic_to_responses_tool_result_preserves_blocks_and_error() {
    let input = json!({
        "model":"gpt-5",
        "messages":[{"role":"user","content":[{
            "type":"tool_result",
            "tool_use_id":"call_1",
            "is_error":true,
            "content":[
                {"type":"text","text":"command failed"},
                {"type":"image","source":{"type":"url","url":"https://example.com/error.png"}},
                {"type":"document","title":"trace.pdf","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="}}
            ]
        }]}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let output = result["input"][0]["output"].as_array().unwrap();
    assert_eq!(output[0]["text"], TOOL_RESULT_ERROR_MARKER);
    assert_eq!(
        output[1],
        json!({"type":"input_text","text":"command failed"})
    );
    assert_eq!(output[2]["image_url"], "https://example.com/error.png");
    assert_eq!(output[3]["type"], "input_file");
    assert_eq!(output[3]["filename"], "trace.pdf");
}

#[test]
fn test_anthropic_to_responses_converts_mcp_tool_image() {
    let input = json!({
        "model":"gpt-5",
        "messages":[{"role":"user","content":[{
            "type":"tool_result",
            "tool_use_id":"call_1",
            "content":[{
                "type":"image",
                "mimeType":"image/webp",
                "data":"MCP_RESPONSES_IMAGE_SENTINEL"
            }]
        }]}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let output = result["input"][0]["output"].as_array().unwrap();

    assert_eq!(output[0]["type"], "input_text");
    assert!(!output[0]["text"]
        .as_str()
        .unwrap()
        .contains("MCP_RESPONSES_IMAGE_SENTINEL"));
    assert_eq!(output[1]["type"], "input_image");
    assert_eq!(
        output[1]["image_url"],
        "data:image/webp;base64,MCP_RESPONSES_IMAGE_SENTINEL"
    );
}

#[test]
fn test_anthropic_to_responses_converts_json_string_tool_image() {
    let residual_base64 = "A".repeat(20_000);
    let encoded = json!({
        "content":[
            {
                "type":"image_url",
                "image_url":{"url":"data:image/png;base64,STRING_RESPONSES_SENTINEL"}
            },
            {"type":"video","data":residual_base64}
        ]
    })
    .to_string();
    let input = json!({
        "model":"gpt-5",
        "messages":[{"role":"user","content":[{
            "type":"tool_result",
            "tool_use_id":"call_1",
            "content":encoded
        }]}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let output = result["input"][0]["output"].as_array().unwrap();
    let image = output
        .iter()
        .find(|part| part["type"] == "input_image")
        .expect("stringified image must stay a Responses image");

    assert_eq!(
        image["image_url"],
        "data:image/png;base64,STRING_RESPONSES_SENTINEL"
    );
    assert!(output
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .all(|text| !text.contains("STRING_RESPONSES_SENTINEL")));
    let serialized = result.to_string();
    assert!(serialized.contains("[cc-switch: omitted 20000 bytes]"));
    assert!(!serialized.contains(&"A".repeat(64)));
}

#[test]
fn test_anthropic_to_responses_thinking_discarded() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think..."},
                {"type": "text", "text": "The answer is 42"}
            ]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let input_arr = result["input"].as_array().unwrap();

    // thinking should be discarded, only text remains
    assert_eq!(input_arr.len(), 1);
    assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
    assert_eq!(input_arr[0]["content"][0]["text"], "The answer is 42");
}

#[test]
fn test_anthropic_to_responses_image() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
            ]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let content = result["input"][0]["content"].as_array().unwrap();

    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["image_url"], "data:image/png;base64,abc123");
}

#[test]
fn test_anthropic_to_responses_url_image_and_document() {
    let input = json!({
        "model":"gpt-5",
        "messages":[{"role":"user","content":[
            {"type":"image","source":{"type":"url","url":"https://example.com/a.png"}},
            {"type":"document","title":"manual.pdf","source":{"type":"url","url":"https://example.com/manual.pdf"}}
        ]}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    let content = result["input"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "input_image");
    assert_eq!(content[0]["image_url"], "https://example.com/a.png");
    assert_eq!(content[1]["type"], "input_file");
    assert_eq!(content[1]["file_url"], "https://example.com/manual.pdf");
    assert_eq!(content[1]["filename"], "manual.pdf");
}

#[test]
fn test_responses_to_anthropic_simple() {
    let input = json!({
        "id": "resp_123",
        "object": "response",
        "status": "completed",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "id": "msg_123",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Hello!"}]
        }],
        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["id"], "resp_123");
    assert_eq!(result["type"], "message");
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(result["content"][0]["text"], "Hello!");
    assert_eq!(result["stop_reason"], "end_turn");
    assert_eq!(result["usage"]["input_tokens"], 10);
    assert_eq!(result["usage"]["output_tokens"], 5);
}

#[test]
fn test_responses_to_anthropic_preserves_url_citation_positions() {
    let input = json!({
        "id": "resp_citation",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [{
            "type": "message",
            "id": "msg_citation",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "Rust docs explain ownership.",
                "annotations": [{
                    "type": "url_citation",
                    "start_index": 0,
                    "end_index": 9,
                    "url": "https://doc.rust-lang.org/book/",
                    "title": "The Rust Book"
                }]
            }]
        }]
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(
        result["content"][0]["text"],
        "[Rust docs](https://doc.rust-lang.org/book/) explain ownership."
    );
}

#[test]
fn test_responses_to_anthropic_hosted_web_search() {
    let input = json!({
        "id": "resp_search",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws_123",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "Rust official documentation",
                    "sources": [{
                        "type": "url",
                        "url": "https://doc.rust-lang.org/"
                    }]
                }
            },
            {
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "The Rust documentation is available online.",
                    "annotations": [
                        {
                            "type": "url_citation",
                            "url": "https://doc.rust-lang.org/",
                            "title": "The Rust Programming Language"
                        },
                        {
                            "type": "url_citation",
                            "url": "https://doc.rust-lang.org/",
                            "title": "Duplicate citation"
                        }
                    ]
                }]
            }
        ],
        "usage": {"input_tokens": 10, "output_tokens": 20}
    });

    let result =
        responses_to_anthropic_with_web_search_options(input, Some("web_search_next"), None)
            .unwrap();
    assert_eq!(result["content"][0]["type"], "server_tool_use");
    assert_eq!(result["content"][0]["id"], "ws_123");
    assert_eq!(result["content"][0]["name"], "web_search_next");
    assert_eq!(
        result["content"][0]["input"]["query"],
        "Rust official documentation"
    );
    assert_eq!(result["content"][1]["type"], "web_search_tool_result");
    assert_eq!(result["content"][1]["tool_use_id"], "ws_123");
    assert_eq!(result["content"][1]["content"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["content"][1]["content"][0]["url"],
        "https://doc.rust-lang.org/"
    );
    assert_eq!(
        result["content"][1]["content"][0]["title"],
        "The Rust Programming Language"
    );
    assert_eq!(result["content"][2]["type"], "text");
    assert_eq!(
        result["content"][2]["text"],
        "The Rust documentation is available online.\n\nSources: [The Rust Programming Language](https://doc.rust-lang.org/)"
    );
    assert_eq!(result["stop_reason"], "end_turn");
    assert_eq!(result["usage"]["server_tool_use"]["web_search_requests"], 1);
}

#[test]
fn test_responses_to_anthropic_preserves_failed_hosted_web_search() {
    let input = json!({
        "id": "resp_failed_search",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [{
            "type": "web_search_call",
            "id": "ws_failed",
            "status": "failed",
            "action": {"type": "search", "query": "latest docs"},
            "error": {
                "code": "rate_limit_exceeded",
                "message": "Too many requests"
            }
        }],
        "usage": {"input_tokens": 4, "output_tokens": 1}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "server_tool_use");
    assert_eq!(result["content"][0]["id"], "ws_failed");
    assert_eq!(result["content"][1]["type"], "web_search_tool_result");
    assert_eq!(result["content"][1]["tool_use_id"], "ws_failed");
    assert_eq!(
        result["content"][1]["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "too_many_requests"
        })
    );
}

#[test]
fn test_responses_to_anthropic_marks_unfinished_hosted_web_search_as_error() {
    let input = json!({
        "id": "resp_unfinished_search",
        "status": "incomplete",
        "incomplete_details": {"reason": "max_output_tokens"},
        "model": "gpt-5.6",
        "output": [{
            "type": "web_search_call",
            "id": "ws_unfinished",
            "status": "searching",
            "action": {"type": "search", "query": "latest docs"}
        }],
        "usage": {"input_tokens": 4, "output_tokens": 1}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "server_tool_use");
    assert_eq!(result["content"][1]["type"], "web_search_tool_result");
    assert_eq!(
        result["content"][1]["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "unavailable"
        })
    );
    assert_eq!(result["stop_reason"], "max_tokens");
}

#[test]
fn test_responses_to_anthropic_enforces_hosted_web_search_max_uses() {
    let input = json!({
        "id": "resp_search_limit",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws_allowed",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "allowed query",
                    "sources": [{
                        "type": "url",
                        "url": "https://example.com/allowed",
                        "title": "Allowed"
                    }]
                }
            },
            {
                "type": "web_search_call",
                "id": "ws_over_limit",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "disallowed query",
                    "sources": [{
                        "type": "url",
                        "url": "https://example.com/disallowed",
                        "title": "Disallowed"
                    }]
                }
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Answer based on the disallowed search."
                }]
            }
        ],
        "usage": {"input_tokens": 4, "output_tokens": 3}
    });

    let result =
        responses_to_anthropic_with_web_search_options(input, Some("web_search"), Some(1)).unwrap();
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 4);
    assert_eq!(content[0]["id"], "ws_allowed");
    assert_eq!(content[1]["content"][0]["title"], "Allowed");
    assert_eq!(content[2]["id"], "ws_over_limit");
    assert_eq!(
        content[3]["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "max_uses_exceeded"
        })
    );
    assert_eq!(result["usage"]["server_tool_use"]["web_search_requests"], 1);
}

#[test]
fn test_web_search_max_uses_cuts_off_later_function_calls() {
    let input = json!({
        "id": "resp_mixed_search_limit",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws_first",
                "status": "completed",
                "action": {"type": "search", "query": "first query"}
            },
            {
                "type": "function_call",
                "call_id": "call_before",
                "name": "get_weather",
                "arguments": "{\"location\":\"Tokyo\"}"
            },
            {
                "type": "web_search_call",
                "id": "ws_second",
                "status": "completed",
                "action": {"type": "search", "query": "second query"}
            },
            {
                "type": "function_call",
                "call_id": "call_after",
                "name": "get_weather",
                "arguments": "{\"location\":\"Paris\"}"
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Later answer."}]
            }
        ],
        "usage": {"input_tokens": 4, "output_tokens": 3}
    });

    let within_limit =
        responses_to_anthropic_with_web_search_options(input.clone(), Some("web_search"), Some(2))
            .unwrap();
    let content = within_limit["content"].as_array().unwrap();
    assert_eq!(content.len(), 7);
    assert_eq!(content[2]["id"], "call_before");
    assert_eq!(content[5]["id"], "call_after");
    assert_eq!(content[6]["text"], "Later answer.");
    assert_eq!(within_limit["stop_reason"], "tool_use");

    // Match stream cutoff: preserve completed calls before the excess
    // search, but do not return anything produced after that search.
    let over_limit =
        responses_to_anthropic_with_web_search_options(input, Some("web_search"), Some(1)).unwrap();
    let content = over_limit["content"].as_array().unwrap();
    assert_eq!(content.len(), 5);
    assert_eq!(content[2]["id"], "call_before");
    assert_eq!(content[2]["input"], json!({"location": "Tokyo"}));
    assert_eq!(content[3]["id"], "ws_second");
    assert_eq!(content[4]["content"]["error_code"], "max_uses_exceeded");
    assert_eq!(over_limit["stop_reason"], "tool_use");
    assert_eq!(
        over_limit["usage"]["server_tool_use"]["web_search_requests"],
        1
    );
}

#[test]
fn test_responses_to_anthropic_pairs_every_hosted_web_search_call() {
    let input = json!({
        "id": "resp_multi_search",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws_rust",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "Rust language",
                    "sources": [{
                        "type": "url",
                        "url": "https://www.rust-lang.org/",
                        "title": "Rust"
                    }]
                }
            },
            {
                "type": "web_search_call",
                "id": "ws_cargo",
                "status": "completed",
                "action": {
                    "type": "search",
                    "query": "Cargo documentation",
                    "sources": [{
                        "type": "url",
                        "url": "https://doc.rust-lang.org/cargo/",
                        "title": "Cargo"
                    }]
                }
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Rust and Cargo both have official documentation.",
                    "annotations": [
                        {
                            "type": "url_citation",
                            "url": "https://www.rust-lang.org/",
                            "title": "Rust"
                        },
                        {
                            "type": "url_citation",
                            "url": "https://doc.rust-lang.org/cargo/",
                            "title": "Cargo"
                        }
                    ]
                }]
            }
        ],
        "usage": {"input_tokens": 10, "output_tokens": 20}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 5);
    assert_eq!(content[0]["type"], "server_tool_use");
    assert_eq!(content[0]["id"], "ws_rust");
    assert_eq!(content[1]["type"], "web_search_tool_result");
    assert_eq!(content[1]["tool_use_id"], "ws_rust");
    assert_eq!(
        content[1]["content"][0]["url"],
        "https://www.rust-lang.org/"
    );
    assert_eq!(content[2]["type"], "server_tool_use");
    assert_eq!(content[2]["id"], "ws_cargo");
    assert_eq!(content[3]["type"], "web_search_tool_result");
    assert_eq!(content[3]["tool_use_id"], "ws_cargo");
    assert_eq!(
        content[3]["content"][0]["url"],
        "https://doc.rust-lang.org/cargo/"
    );
    assert_eq!(content[4]["type"], "text");
    assert_eq!(result["usage"]["server_tool_use"]["web_search_requests"], 2);
}

#[test]
fn test_responses_to_anthropic_pairs_calls_when_sources_are_unavailable() {
    let input = json!({
        "id": "resp_multi_search_without_sources",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [
            {
                "type": "web_search_call",
                "id": "ws_first",
                "status": "completed",
                "action": {"type": "search", "query": "first query"}
            },
            {
                "type": "web_search_call",
                "id": "ws_second",
                "status": "completed",
                "action": {"type": "search", "query": "second query"}
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Combined answer.",
                    "annotations": [{
                        "type": "url_citation",
                        "url": "https://example.com/result",
                        "title": "Combined result"
                    }]
                }]
            }
        ]
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    let content = result["content"].as_array().unwrap();
    assert_eq!(content[1]["type"], "web_search_tool_result");
    assert_eq!(content[1]["tool_use_id"], "ws_first");
    assert_eq!(content[1]["content"], json!([]));
    assert_eq!(content[3]["type"], "web_search_tool_result");
    assert_eq!(content[3]["tool_use_id"], "ws_second");
    assert_eq!(
        content[3]["content"][0]["url"],
        "https://example.com/result"
    );
}

#[test]
fn test_responses_to_anthropic_with_function_call() {
    let input = json!({
        "id": "resp_123",
        "object": "response",
        "status": "completed",
        "model": "gpt-4o",
        "output": [{
            "type": "function_call",
            "id": "fc_123",
            "call_id": "call_123",
            "name": "get_weather",
            "arguments": "{\"location\": \"Tokyo\"}",
            "status": "completed"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 15}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "tool_use");
    assert_eq!(result["content"][0]["id"], "call_123");
    assert_eq!(result["content"][0]["name"], "get_weather");
    assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
    assert_eq!(result["stop_reason"], "tool_use");
}

#[test]
fn test_completed_function_call_empty_arguments_normalizes_to_object() {
    let input = json!({
        "id": "resp_empty_args",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "ping",
            "arguments": ""
        }],
        "usage": {"input_tokens": 10, "output_tokens": 2}
    });
    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["input"], json!({}));
}

#[test]
fn test_incomplete_function_call_invalid_arguments_uses_empty_object() {
    let input = json!({
        "id": "resp_partial_args",
        "status": "incomplete",
        "incomplete_details": {"reason": "max_output_tokens"},
        "model": "gpt-5.6",
        "output": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "dangerous_tool",
            "arguments": "{\"path\":"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 2}
    });
    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "tool_use");
    assert_eq!(result["content"][0]["input"], json!({}));
    assert_eq!(result["stop_reason"], "max_tokens");
}

#[test]
fn test_completed_function_call_invalid_arguments_is_error() {
    let input = json!({
        "id": "resp_bad_args",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "broken_tool",
            "arguments": "{\"path\":"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 2}
    });
    assert!(matches!(
        responses_to_anthropic_with_web_search_options(input, None, None),
        Err(ProxyError::TransformError(_))
    ));
}

#[test]
fn test_responses_to_anthropic_read_drops_empty_pages() {
    let input = json!({
        "id": "resp_read",
        "object": "response",
        "status": "completed",
        "model": "gpt-5.5",
        "output": [{
            "type": "function_call",
            "id": "fc_read",
            "call_id": "call_read",
            "name": "Read",
            "arguments": "{\"file_path\":\"/tmp/demo.py\",\"limit\":2000,\"offset\":0,\"pages\":\"\"}",
            "status": "completed"
        }]
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    let tool_input = &result["content"][0]["input"];

    assert_eq!(result["content"][0]["type"], "tool_use");
    assert_eq!(result["content"][0]["name"], "Read");
    assert_eq!(tool_input["file_path"], "/tmp/demo.py");
    assert_eq!(tool_input["limit"], 2000);
    assert_eq!(tool_input["offset"], 0);
    assert!(tool_input.get("pages").is_none());
}

#[test]
fn test_responses_to_anthropic_preserves_empty_strings_for_other_tools() {
    let input = json!({
        "id": "resp_other",
        "object": "response",
        "status": "completed",
        "model": "gpt-5.5",
        "output": [{
            "type": "function_call",
            "id": "fc_other",
            "call_id": "call_other",
            "name": "search",
            "arguments": "{\"query\":\"\"}",
            "status": "completed"
        }]
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();

    assert_eq!(result["content"][0]["input"]["query"], "");
}

#[test]
fn test_responses_to_anthropic_with_refusal_block() {
    let input = json!({
        "id": "resp_123",
        "status": "completed",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "content": [{"type": "refusal", "refusal": "I can't help with that."}]
        }],
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(result["content"][0]["text"], "I can't help with that.");
    assert_eq!(result["stop_reason"], "end_turn");
}

#[test]
fn test_responses_to_anthropic_with_reasoning() {
    let input = json!({
        "id": "resp_123",
        "object": "response",
        "status": "completed",
        "model": "gpt-4o",
        "output": [
            {
                "type": "reasoning",
                "id": "rs_123",
                "summary": [
                    {"type": "summary_text", "text": "Thinking about the problem..."}
                ]
            },
            {
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "The answer is 42"}]
            }
        ],
        "usage": {"input_tokens": 10, "output_tokens": 20}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    // Should have thinking + text
    assert_eq!(result["content"][0]["type"], "thinking");
    assert_eq!(
        result["content"][0]["thinking"],
        "Thinking about the problem..."
    );
    assert_eq!(result["content"][1]["type"], "text");
    assert_eq!(result["content"][1]["text"], "The answer is 42");
}

#[test]
fn test_encrypted_reasoning_round_trips_through_anthropic_history() {
    let original = json!({
        "type": "reasoning",
        "id": "rs_123",
        "summary": [{"type": "summary_text", "text": "Need a tool."}],
        "encrypted_content": "opaque-ciphertext"
    });
    let response = json!({
        "id": "resp_123",
        "status": "completed",
        "model": "gpt-5.6",
        "output": [original.clone()],
        "usage": {"input_tokens": 10, "output_tokens": 2}
    });

    let anthropic = responses_to_anthropic_with_web_search_options(response, None, None).unwrap();
    let thinking = anthropic["content"][0].clone();
    assert_eq!(thinking["type"], "thinking");
    assert!(thinking["signature"]
        .as_str()
        .is_some_and(|value| value.starts_with("ccswitch-openai-reasoning-v1:")));

    let replay = anthropic_to_responses(
        json!({
            "model": "gpt-5.6",
            "messages": [{"role": "assistant", "content": [
                thinking,
                {"type": "tool_use", "id": "call_1", "name": "lookup", "input": {}}
            ]}]
        }),
        None,
        true,
        false,
    )
    .unwrap();
    assert_eq!(replay["input"][0], original);
    assert_eq!(replay["input"][1]["type"], "function_call");
}

#[test]
fn test_replayed_legacy_envelope_omits_output_only_fields() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // A pre-whitelist envelope keeps the backend output item verbatim,
    // including the `status` field the official Codex backend started
    // emitting on 2026-10-03 and rejects as an unknown input parameter.
    let legacy = json!({
        "id": "rs_legacy",
        "type": "reasoning",
        "summary": [{"type": "summary_text", "text": "prior turn"}],
        "encrypted_content": "opaque",
        "status": "completed"
    });
    let signature = format!(
        "ccswitch-openai-reasoning-v1:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&legacy).unwrap())
    );
    let replay = anthropic_to_responses(
        json!({
            "model": "gpt-5.6",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "prior turn", "signature": signature},
                    {"type": "tool_use", "id": "call_9", "name": "lookup", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_9", "content": "ok"}
                ]}
            ]
        }),
        None,
        true,
        false,
    )
    .unwrap();
    let reasoning_items: Vec<&Value> = replay["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .collect();
    assert_eq!(reasoning_items.len(), 1);
    assert_eq!(
        reasoning_items[0],
        &json!({
            "id": "rs_legacy",
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "prior turn"}],
            "encrypted_content": "opaque"
        })
    );
}

#[test]
fn test_reasoning_only_assistant_turn_is_not_replayed() {
    let item = json!({
        "type": "reasoning",
        "id": "rs_orphan",
        "summary": [],
        "encrypted_content": "opaque"
    });
    let block = anthropic_block_from_openai_reasoning_item(&item).unwrap();
    let replay = anthropic_to_responses(
        json!({
            "model": "gpt-5.6",
            "messages": [
                {"role": "assistant", "content": [block]},
                {"role": "user", "content": "continue"}
            ]
        }),
        None,
        true,
        false,
    )
    .unwrap();

    assert_eq!(replay["input"].as_array().unwrap().len(), 1);
    assert_eq!(replay["input"][0]["role"], "user");
}

#[test]
fn test_responses_failed_status_is_not_silent_empty_success() {
    let input = json!({
        "id": "resp_failed",
        "status": "failed",
        "error": {"type": "server_error", "message": "backend exploded"},
        "output": [],
        "usage": {"input_tokens": 10, "output_tokens": 0}
    });

    let error = responses_to_anthropic_with_web_search_options(input, None, None).unwrap_err();
    assert!(
        matches!(error, ProxyError::TransformError(message) if message.contains("backend exploded"))
    );
}

#[test]
fn test_responses_error_envelope_preserves_upstream_message() {
    let input = json!({
        "error": {"type": "rate_limit_error", "message": "too many requests"}
    });

    let error = responses_to_anthropic_with_web_search_options(input, None, None).unwrap_err();
    assert!(
        matches!(error, ProxyError::TransformError(message) if message.contains("too many requests"))
    );
}

#[test]
fn test_responses_cancelled_status_is_not_end_turn() {
    let input = json!({
        "id": "resp_cancelled",
        "status": "cancelled",
        "output": []
    });

    let error = responses_to_anthropic_with_web_search_options(input, None, None).unwrap_err();
    assert!(matches!(error, ProxyError::TransformError(message) if message.contains("cancelled")));
}

#[test]
fn test_responses_to_anthropic_incomplete_status() {
    let input = json!({
        "id": "resp_123",
        "status": "incomplete",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "content": [{"type": "output_text", "text": "Partial..."}]
        }],
        "usage": {"input_tokens": 10, "output_tokens": 4096}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["stop_reason"], "max_tokens");
}

#[test]
fn test_responses_to_anthropic_incomplete_non_token_reason() {
    let input = json!({
        "id": "resp_123",
        "status": "incomplete",
        "incomplete_details": {"reason": "content_filter"},
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "content": [{"type": "output_text", "text": "Blocked"}]
        }],
        "usage": {"input_tokens": 10, "output_tokens": 1}
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    assert_eq!(result["stop_reason"], "end_turn");
}

#[test]
fn test_model_passthrough() {
    let input = json!({
        "model": "o3-mini",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["model"], "o3-mini");
}

#[test]
fn test_anthropic_to_responses_with_cache_key() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, Some("my-provider-id"), false, false).unwrap();
    assert_eq!(result["prompt_cache_key"], "my-provider-id");
}

#[test]
fn test_anthropic_to_responses_strip_cache_control_on_tools() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tools": [{
            "name": "get_weather",
            "description": "Get weather",
            "input_schema": {"type": "object"},
            "cache_control": {"type": "ephemeral"}
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert!(result["tools"][0].get("cache_control").is_none());
}

#[test]
fn test_anthropic_to_responses_strip_cache_control_on_text() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
            ]
        }]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert!(result["input"][0]["content"][0]
        .get("cache_control")
        .is_none());
}

#[test]
fn test_responses_to_anthropic_with_cache_tokens() {
    let input = json!({
        "id": "resp_123",
        "status": "completed",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "content": [{"type": "output_text", "text": "Hello!"}]
        }],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "input_tokens_details": {
                "cached_tokens": 80
            }
        }
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    // input_tokens(100) 含 cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
    assert_eq!(result["usage"]["input_tokens"], 20);
    assert_eq!(result["usage"]["output_tokens"], 50);
    assert_eq!(result["usage"]["cache_read_input_tokens"], 80);
}

#[test]
fn test_responses_to_anthropic_with_direct_cache_fields() {
    let input = json!({
        "id": "resp_123",
        "status": "completed",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "content": [{"type": "output_text", "text": "Hello!"}]
        }],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 20
        }
    });

    let result = responses_to_anthropic_with_web_search_options(input, None, None).unwrap();
    // cache_read(60)+cache_creation(20) 均从 input(100) 扣除，fresh = 100 - 60 - 20 = 20
    // 守恒：input(20) + cache_read(60) + cache_creation(20) == 上游 input(100)
    assert_eq!(result["usage"]["input_tokens"], 20);
    assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
    assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
}

#[test]
fn test_anthropic_to_responses_o_series_uses_max_output_tokens() {
    // Responses API always uses max_output_tokens, even for o-series models
    let input = json!({
        "model": "o3-mini",
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["max_output_tokens"], 4096);
    assert!(result.get("max_completion_tokens").is_none());
}

#[test]
fn test_responses_output_config_max_sets_reasoning_xhigh() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": "max"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "xhigh");
}

#[test]
fn test_responses_max_capable_models_preserve_max() {
    for model in [
        "gpt-5.6",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-6-astra",
        "gpt-6-sol",
        "gpt-6-luna",
    ] {
        let input = json!({
            "model": model,
            "max_tokens": 1024,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "max"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "max", "model {model}");
    }
}

#[test]
fn test_responses_output_config_xhigh_sets_reasoning_xhigh() {
    // Claude Code's `/effort xhigh` sends output_config.effort="xhigh";
    // previously it fell into the unknown-value branch and was dropped.
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": "xhigh"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "xhigh");
}

#[test]
fn test_responses_grok_4_6_reasoning_effort_not_dropped() {
    // After model mapping, the Responses gate runs on the mapped name;
    // grok-4.6 / grok-4.6-* were missing from the whitelist (#7314).
    let input = json!({
        "model": "grok-4.6-build",
        "max_tokens": 1024,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": "xhigh"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "xhigh");
}

#[test]
fn test_responses_thinking_off_clamps_effort_to_low() {
    // Thinking turned off in Claude Code: no `thinking`, effort still "high".
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "output_config": {"effort": "high"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "low");
}

#[test]
fn test_responses_output_config_takes_priority_over_thinking() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "output_config": {"effort": "low"},
        "thinking": {"type": "adaptive"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "low");
}

#[test]
fn test_responses_thinking_enabled_small_budget_sets_reasoning_low() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "enabled", "budget_tokens": 2048},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "low");
}

#[test]
fn test_responses_thinking_enabled_medium_budget_sets_reasoning_medium() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "enabled", "budget_tokens": 8000},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "medium");
}

#[test]
fn test_responses_thinking_enabled_large_budget_sets_reasoning_high() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "enabled", "budget_tokens": 32000},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "high");
}

#[test]
fn test_responses_thinking_adaptive_sets_reasoning_xhigh() {
    let input = json!({
        "model": "gpt-5.4",
        "max_tokens": 1024,
        "thinking": {"type": "adaptive"},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert_eq!(result["reasoning"]["effort"], "xhigh");
}

#[test]
fn test_responses_non_reasoning_model_no_reasoning() {
    let input = json!({
        "model": "gpt-4o",
        "max_tokens": 1024,
        "thinking": {"type": "enabled", "budget_tokens": 2048},
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();
    assert!(result.get("reasoning").is_none());
}

// ==================== Codex OAuth (ChatGPT 反代) 协议约束 ====================

#[test]
fn test_anthropic_to_responses_codex_oauth_sets_store_and_include() {
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    // store 必须显式为 false（ChatGPT 后端拒绝 true）
    assert_eq!(result["store"], json!(false));
    assert_eq!(result["service_tier"], json!("priority"));

    // include 必须包含 reasoning.encrypted_content（无服务端状态下保持多轮 reasoning）
    assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
}

#[test]
fn test_anthropic_to_responses_non_codex_omits_store_and_include() {
    // 回归护栏：is_codex_oauth=false 时，行为必须与今日字节级一致
    // —— 不写 store、不写 include，OpenRouter / Azure / OpenAI 付费 API 路径不受影响
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();

    assert!(result.get("store").is_none());
    assert!(result.get("service_tier").is_none());
    assert!(result.get("include").is_none());
}

#[test]
fn test_anthropic_to_responses_codex_oauth_preserves_existing_include() {
    // 客户端预置了 include：union 保留原有项 + 添加 marker，不重复
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}],
        "include": ["something.else", "reasoning.encrypted_content"]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();
    let includes = result["include"]
        .as_array()
        .expect("include should be array");

    // 原有项必须保留
    assert!(includes
        .iter()
        .any(|v| v.as_str() == Some("something.else")));
    // marker 必须存在
    assert!(includes
        .iter()
        .any(|v| v.as_str() == Some("reasoning.encrypted_content")));
    // 不重复：marker 只出现一次
    let marker_count = includes
        .iter()
        .filter(|v| v.as_str() == Some("reasoning.encrypted_content"))
        .count();
    assert_eq!(marker_count, 1, "marker 不应被重复添加（idempotent 失败）");
}

#[test]
fn test_anthropic_to_responses_codex_oauth_fast_mode_can_be_disabled() {
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, false).unwrap();

    assert_eq!(result["store"], json!(false));
    assert!(result.get("service_tier").is_none());
    assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
}

#[test]
fn test_anthropic_to_responses_codex_oauth_strips_max_output_tokens() {
    // ChatGPT Plus/Pro 反代不接受 max_output_tokens（OpenAI 官方 codex-rs 的
    // ResponsesApiRequest 结构体里也没有这个字段），必须删除，否则服务端 400：
    // "Unsupported parameter: max_output_tokens"
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert!(
        result.get("max_output_tokens").is_none(),
        "Codex OAuth 路径必须删除 max_output_tokens"
    );
}

#[test]
fn test_anthropic_to_responses_non_codex_keeps_max_output_tokens() {
    // 回归护栏：非 Codex OAuth 路径必须保留 max_output_tokens
    // —— OpenAI 付费 Responses API / Azure 等仍然依赖这个字段
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();

    assert_eq!(result["max_output_tokens"], json!(1024));
}

#[test]
fn test_anthropic_to_responses_clamps_sub_floor_max_tokens() {
    // Responses API 拒绝 max_output_tokens < 16（"The number must be >= 16"）。
    // Claude Desktop 的模型可用性探测发 max_tokens=1，原样转发会让整个
    // 探测请求 400，客户端因此判定"配置的模型不可用"（#7103）。
    for budget in [1u64, 8, 15] {
        let input = json!({
            "model": "claude-haiku-4-5",
            "max_tokens": budget,
            "messages": [{"role": "user", "content": "ping"}]
        });
        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["max_output_tokens"], json!(16));
    }

    // 下限本身与正常预算保持原样
    for budget in [16u64, 1024] {
        let input = json!({
            "model": "claude-haiku-4-5",
            "max_tokens": budget,
            "messages": [{"role": "user", "content": "ping"}]
        });
        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["max_output_tokens"], json!(budget));
    }

    // 0 与非整数不抬升：保持既有透传语义，把非法值留给上游裁决
    for raw in [json!(0), json!("16"), json!(12.5)] {
        let input = json!({
            "model": "claude-haiku-4-5",
            "max_tokens": raw,
            "messages": [{"role": "user", "content": "ping"}]
        });
        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["max_output_tokens"], raw);
    }
}

// ==================== 第二轮：P0 + P1 字段对齐 ====================

#[test]
fn test_codex_oauth_strips_temperature() {
    // P0: ChatGPT 反代不接受 temperature
    // 依据：OpenAI 官方 codex-rs 的 ResponsesApiRequest 结构体根本没有这个字段
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "temperature": 0.7,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert!(
        result.get("temperature").is_none(),
        "Codex OAuth 路径必须删除 temperature"
    );
}

#[test]
fn test_codex_oauth_strips_top_p() {
    // P0: ChatGPT 反代不接受 top_p
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "top_p": 0.9,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert!(
        result.get("top_p").is_none(),
        "Codex OAuth 路径必须删除 top_p"
    );
}

#[test]
fn test_codex_oauth_defaults_required_fields_when_absent() {
    // P1: 极简输入（无 system / 无 tools / 无 stream），断言四个必填字段都被注入默认值
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert_eq!(
        result["instructions"],
        json!(""),
        "instructions 缺失时应兜底为空字符串"
    );
    assert_eq!(result["tools"], json!([]), "tools 缺失时应兜底为空数组");
    assert_eq!(
        result["parallel_tool_calls"],
        json!(true),
        "parallel_tool_calls 应默认允许并行工具调用"
    );
    assert_eq!(result["stream"], json!(true), "stream 应被强制设为 true");
}

#[test]
fn test_codex_oauth_maps_anthropic_parallel_tool_choice() {
    let enabled = anthropic_to_responses(
        json!({
            "model": "gpt-5.6-sol",
            "tools": [{
                "name": "read_file",
                "input_schema": {"type": "object"}
            }],
            "tool_choice": {
                "type": "auto",
                "disable_parallel_tool_use": false
            },
            "messages": [{"role": "user", "content": "Read both files"}]
        }),
        None,
        true,
        true,
    )
    .unwrap();
    assert_eq!(enabled["parallel_tool_calls"], json!(true));

    let disabled = anthropic_to_responses(
        json!({
            "model": "gpt-5.6-sol",
            "tools": [{
                "name": "read_file",
                "input_schema": {"type": "object"}
            }],
            "tool_choice": {
                "type": "auto",
                "disable_parallel_tool_use": true
            },
            "messages": [{"role": "user", "content": "Read one file"}]
        }),
        None,
        true,
        true,
    )
    .unwrap();
    assert_eq!(disabled["parallel_tool_calls"], json!(false));
}

#[test]
fn test_codex_oauth_preserves_existing_instructions_and_tools() {
    // P1: 客户端送了 system 和 tools，应保留原值，不被默认值覆盖
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "system": "You are a helpful assistant",
        "tools": [{
            "name": "get_weather",
            "description": "Get weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}}
            }
        }],
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert_eq!(
        result["instructions"],
        json!("You are a helpful assistant"),
        "client 已送的 instructions 必须保留"
    );

    let tools = result["tools"].as_array().expect("tools 应为数组");
    assert_eq!(tools.len(), 1, "client 已送的 tools 必须保留");
    assert_eq!(tools[0]["name"], json!("get_weather"));
}

#[test]
fn test_codex_oauth_forces_stream_true_even_when_client_sends_false() {
    // 即使客户端误传 stream:false，也要强制覆盖为 true
    // 依据：cc-switch SSE 解析层只支持流式响应
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "stream": false,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, true, true).unwrap();

    assert_eq!(
        result["stream"],
        json!(true),
        "Codex OAuth 路径下 stream 必须强制为 true"
    );
}

#[test]
fn test_non_codex_keeps_temperature_and_top_p() {
    // 回归护栏：非 Codex OAuth 路径必须保留 temperature/top_p
    // —— 防止 P0 删除逻辑误扩散到 OpenRouter / Azure / 付费 OpenAI 路径
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "temperature": 0.7,
        "top_p": 0.9,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();

    assert_eq!(result["temperature"], json!(0.7));
    assert_eq!(result["top_p"], json!(0.9));
}

#[test]
fn test_non_codex_does_not_inject_default_required_fields() {
    // 回归护栏：非 Codex OAuth 路径不应被 P1 默认值污染
    // —— OpenRouter / Azure / 付费 OpenAI 等保持原有"条件写入"语义
    let input = json!({
        "model": "gpt-5-codex",
        "max_tokens": 1024,
        "tool_choice": {
            "type": "auto",
            "disable_parallel_tool_use": false
        },
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let result = anthropic_to_responses(input, None, false, false).unwrap();

    assert!(
        result.get("parallel_tool_calls").is_none(),
        "非 Codex OAuth 路径不应注入 parallel_tool_calls"
    );
    assert!(
        result.get("stream").is_none(),
        "非 Codex OAuth 路径不应注入 stream"
    );
    // instructions 和 tools 因为客户端没送，所以不应出现
    assert!(
        result.get("instructions").is_none(),
        "非 Codex OAuth 路径下 instructions 在客户端未送时不应被注入"
    );
    assert!(
        result.get("tools").is_none(),
        "非 Codex OAuth 路径下 tools 在客户端未送时不应被注入"
    );
}

// ==================== Usage Field Robustness Tests ====================

#[test]
fn test_build_usage_from_null_parameter() {
    let result = build_anthropic_usage_from_responses(None);
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["output_tokens"], json!(0));
}

#[test]
fn test_build_usage_from_null_json_value() {
    let result = build_anthropic_usage_from_responses(Some(&json!(null)));
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["output_tokens"], json!(0));
}

#[test]
fn test_build_usage_from_empty_object() {
    let result = build_anthropic_usage_from_responses(Some(&json!({})));
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["output_tokens"], json!(0));
}

#[test]
fn test_build_usage_from_partial_input_only() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100
    })));
    assert_eq!(result["input_tokens"], json!(100));
    assert_eq!(result["output_tokens"], json!(0));
}

#[test]
fn test_build_usage_from_partial_output_only() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "output_tokens": 50
    })));
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["output_tokens"], json!(50));
}

#[test]
fn test_build_usage_with_openai_field_names() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "prompt_tokens": 120,
        "completion_tokens": 45
    })));
    assert_eq!(result["input_tokens"], json!(120));
    assert_eq!(result["output_tokens"], json!(45));
}

#[test]
fn test_build_usage_anthropic_names_precedence() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100,
        "prompt_tokens": 120,
        "output_tokens": 50,
        "completion_tokens": 45
    })));
    assert_eq!(result["input_tokens"], json!(100)); // Anthropic name takes precedence
    assert_eq!(result["output_tokens"], json!(50)); // Anthropic name takes precedence
}

#[test]
fn test_build_usage_cache_tokens_from_nested_details() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100,
        "output_tokens": 50,
        "input_tokens_details": {
            "cached_tokens": 80
        }
    })));
    // input_tokens(100) 含 nested cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
    assert_eq!(result["input_tokens"], json!(20));
    assert_eq!(result["output_tokens"], json!(50));
    assert_eq!(result["cache_read_input_tokens"], json!(80));
}

#[test]
fn test_build_usage_cache_write_tokens_from_nested_details() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100,
        "output_tokens": 10,
        "input_tokens_details": {
            "cached_tokens": 30,
            "cache_write_tokens": 20
        }
    })));
    assert_eq!(result["input_tokens"], json!(50));
    assert_eq!(result["cache_read_input_tokens"], json!(30));
    assert_eq!(result["cache_creation_input_tokens"], json!(20));
}

#[test]
fn test_build_usage_cache_tokens_direct_override() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100,
        "output_tokens": 50,
        "input_tokens_details": {
            "cached_tokens": 80
        },
        "cache_read_input_tokens": 100
    })));
    // 直传 cache_read(100) 优先于 nested(80)；input(100) - 100 = 0（fresh）
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["cache_read_input_tokens"], json!(100)); // Direct field overrides nested
}

#[test]
fn test_build_usage_clamps_input_when_cache_exceeds_input() {
    // input(100) < cache_read(60)+cache_creation(50)=110：saturating 钳到 0，防下溢。
    // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "input_tokens": 100,
        "output_tokens": 10,
        "cache_read_input_tokens": 60,
        "cache_creation_input_tokens": 50
    })));
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["cache_read_input_tokens"], json!(60));
    assert_eq!(result["cache_creation_input_tokens"], json!(50));
}

#[test]
fn test_build_usage_cache_tokens_without_input_output() {
    let result = build_anthropic_usage_from_responses(Some(&json!({
        "cache_read_input_tokens": 60,
        "cache_creation_input_tokens": 20
    })));
    assert_eq!(result["input_tokens"], json!(0));
    assert_eq!(result["output_tokens"], json!(0));
    assert_eq!(result["cache_read_input_tokens"], json!(60));
    assert_eq!(result["cache_creation_input_tokens"], json!(20));
}
