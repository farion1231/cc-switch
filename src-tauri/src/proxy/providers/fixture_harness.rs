use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::{
    transform::anthropic_to_openai,
    transform_codex_chat::responses_to_chat_completions,
    transform_gemini::{anthropic_to_gemini, gemini_to_anthropic},
    transform_reverse::{anthropic_response_to_openai_chat, openai_chat_request_to_anthropic},
};

const MATRIX: &str =
    include_str!("../../../tests/fixtures/protocol_matrix/protocol_capabilities.json");

const FIXTURES: &[(&str, &str)] = &[
    (
        "anthropic_to_openai_chat.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/anthropic_to_openai_chat.basic.json"),
    ),
    (
        "openai_chat_to_anthropic.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/openai_chat_to_anthropic.basic.json"),
    ),
    (
        "anthropic_to_gemini.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/anthropic_to_gemini.basic.json"),
    ),
    (
        "responses_to_chat.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/responses_to_chat.basic.json"),
    ),
    (
        "responses_to_anthropic.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/responses_to_anthropic.basic.json"),
    ),
    (
        "responses_to_gemini.basic.json",
        include_str!("../../../tests/fixtures/protocol_matrix/responses_to_gemini.basic.json"),
    ),
    (
        "anthropic_response_to_openai_chat.basic.json",
        include_str!(
            "../../../tests/fixtures/protocol_matrix/anthropic_response_to_openai_chat.basic.json"
        ),
    ),
    (
        "gemini_response_to_openai_chat.basic.json",
        include_str!(
            "../../../tests/fixtures/protocol_matrix/gemini_response_to_openai_chat.basic.json"
        ),
    ),
];

#[derive(Debug, Deserialize)]
struct CapabilityMatrix {
    version: u32,
    protocols: HashMap<String, ProtocolCapabilities>,
    bridges: Vec<BridgeFixture>,
}

#[derive(Debug, Deserialize)]
struct ProtocolCapabilities {
    request_format: String,
    streaming: String,
    system_field: String,
    tool_calls: String,
    tool_results: String,
    image_input: bool,
    native_oauth: bool,
    required_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeFixture {
    from: String,
    to: String,
    fixture: String,
    direction: String,
}

#[derive(Debug, Deserialize)]
struct GoldenFixture {
    name: String,
    transform: String,
    input: Value,
    expected: Value,
}

#[test]
fn protocol_capability_matrix_is_complete_and_references_existing_fixtures() {
    let matrix: CapabilityMatrix = serde_json::from_str(MATRIX).expect("parse capability matrix");
    assert_eq!(matrix.version, 1);

    for protocol in [
        "anthropic_messages",
        "openai_chat_completions",
        "openai_responses",
        "gemini_generate_content",
    ] {
        let capabilities = matrix
            .protocols
            .get(protocol)
            .unwrap_or_else(|| panic!("missing protocol {protocol}"));
        assert!(!capabilities.request_format.is_empty());
        assert_eq!(capabilities.streaming, "sse");
        assert!(!capabilities.system_field.is_empty());
        assert!(!capabilities.tool_calls.is_empty());
        assert!(!capabilities.tool_results.is_empty());
        assert!(
            !capabilities.required_fields.is_empty(),
            "{protocol} should declare required fields"
        );
        let _ = capabilities.image_input;
        let _ = capabilities.native_oauth;
    }

    let fixture_names: HashSet<&str> = FIXTURES.iter().map(|(name, _)| *name).collect();
    assert_eq!(matrix.bridges.len(), FIXTURES.len());

    for bridge in matrix.bridges {
        assert!(
            matrix.protocols.contains_key(&bridge.from),
            "unknown source protocol {}",
            bridge.from
        );
        assert!(
            matrix.protocols.contains_key(&bridge.to),
            "unknown target protocol {}",
            bridge.to
        );
        assert!(
            fixture_names.contains(bridge.fixture.as_str()),
            "missing fixture {}",
            bridge.fixture
        );
        assert!(!bridge.direction.is_empty());
    }
}

#[test]
fn golden_protocol_fixtures_match_current_transforms() {
    for (filename, raw) in FIXTURES {
        let fixture: GoldenFixture =
            serde_json::from_str(raw).unwrap_or_else(|e| panic!("{filename}: {e}"));
        let actual = match fixture.transform.as_str() {
            "anthropic_to_openai" => anthropic_to_openai(fixture.input),
            "openai_chat_request_to_anthropic" => openai_chat_request_to_anthropic(fixture.input),
            "anthropic_to_gemini" => anthropic_to_gemini(fixture.input),
            "responses_to_chat_completions" => responses_to_chat_completions(fixture.input),
            "responses_to_anthropic_messages" => responses_to_chat_completions(fixture.input)
                .and_then(openai_chat_request_to_anthropic),
            "responses_to_gemini_generate_content" => responses_to_chat_completions(fixture.input)
                .and_then(openai_chat_request_to_anthropic)
                .and_then(anthropic_to_gemini),
            "anthropic_response_to_openai_chat" => anthropic_response_to_openai_chat(fixture.input),
            "gemini_response_to_openai_chat" => {
                gemini_to_anthropic(fixture.input).and_then(anthropic_response_to_openai_chat)
            }
            other => panic!("{filename}: unknown transform {other}"),
        }
        .unwrap_or_else(|e| panic!("{} ({}) failed: {e}", filename, fixture.name));

        let actual = normalize_dynamic_fields(fixture.transform.as_str(), actual);
        assert_eq!(actual, fixture.expected, "{filename}: {}", fixture.name);
    }
}

fn normalize_dynamic_fields(transform: &str, mut value: Value) -> Value {
    if matches!(
        transform,
        "anthropic_response_to_openai_chat" | "gemini_response_to_openai_chat"
    ) {
        value["created"] = Value::Number(0.into());
    }
    value
}
