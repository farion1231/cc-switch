//! Narrow request-boundary helpers for Codex Multi-Agent V2 delivery.
//!
//! Third-party projection supports explicitly readable V2 agent items only.
//! Historical `encrypted_content` parts are opaque regardless of whether they
//! resemble plaintext; their provenance cannot be established from the request.

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// Remove Codex's private `encrypted` marker from non-reserved `agents.*`
/// collaboration function schemas.
///
/// Reserved `collaboration.*` schemas are never traversed or modified. The
/// caller is responsible for invoking this only under its trusted policy.
pub(crate) fn make_codex_v2_agents_messages_plaintext(body: &mut Value) -> usize {
    let Some(object) = body.as_object_mut() else {
        return 0;
    };

    let mut changed = object
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .map_or(0, |tools| make_agents_tool_messages_plaintext(tools, false));

    if let Some(input) = object.get_mut("input").and_then(Value::as_array_mut) {
        for item in input {
            if item.get("type").and_then(Value::as_str) != Some("additional_tools") {
                continue;
            }
            if let Some(tools) = item.get_mut("tools").and_then(Value::as_array_mut) {
                changed += make_agents_tool_messages_plaintext(tools, false);
            }
        }
    }

    changed
}

/// Return whether a request carries a non-reserved V2 agent delivery surface.
///
/// Detection follows the same namespace rules as marker removal. Reserved
/// `collaboration.*` schemas do not activate the policy; an `agent_message`
/// does, because it may contain a child task or reply regardless of tool schema.
pub(crate) fn has_codex_v2_agent_delivery(body: &Value) -> bool {
    if body
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("agent_message"))
        })
    {
        return true;
    }

    let top_level_tools = body.get("tools").and_then(Value::as_array);
    if top_level_tools.is_some_and(|tools| has_agents_tool_delivery(tools, false)) {
        return true;
    }

    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("additional_tools")
                    && item
                        .get("tools")
                        .and_then(Value::as_array)
                        .is_some_and(|tools| has_agents_tool_delivery(tools, false))
            })
        })
}

fn has_agents_tool_delivery(tools: &[Value], inside_agents: bool) -> bool {
    tools.iter().any(|tool| {
        if tool.get("type").and_then(Value::as_str) == Some("namespace") {
            let is_agents = tool
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("agents"));
            if !is_agents {
                return false;
            }
            let children_key = if tool.get("tools").is_some() {
                "tools"
            } else {
                "children"
            };
            return tool
                .get(children_key)
                .and_then(Value::as_array)
                .is_some_and(|children| has_agents_tool_delivery(children, true));
        }
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            return false;
        }
        let namespace = tool.get("namespace").and_then(Value::as_str);
        let is_agents = namespace
            .map(|value| value.eq_ignore_ascii_case("agents"))
            .unwrap_or(inside_agents);
        is_agents
            && tool
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| {
                    matches!(name, "spawn_agent" | "send_message" | "followup_task")
                })
    })
}

fn make_agents_tool_messages_plaintext(tools: &mut [Value], inside_agents: bool) -> usize {
    let mut changed = 0;
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) == Some("namespace") {
            let namespace_is_agents = tool
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("agents"));
            if namespace_is_agents {
                let children_key = if tool.get("tools").is_some() {
                    "tools"
                } else {
                    "children"
                };
                changed += tool
                    .get_mut(children_key)
                    .and_then(Value::as_array_mut)
                    .map_or(0, |children| {
                        make_agents_tool_messages_plaintext(children, true)
                    });
            }
            continue;
        }
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let namespace = tool.get("namespace").and_then(Value::as_str);
        let is_agents = namespace
            .map(|value| value.eq_ignore_ascii_case("agents"))
            .unwrap_or(inside_agents);
        if !is_agents {
            continue;
        }
        if !tool
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| matches!(name, "spawn_agent" | "send_message" | "followup_task"))
        {
            continue;
        }
        let encrypted = tool
            .pointer_mut("/parameters/properties/message")
            .and_then(Value::as_object_mut)
            .and_then(|message| message.remove("encrypted"));
        changed += usize::from(encrypted.is_some());
    }
    changed
}

/// Project Codex-private `agent_message` items to standard Responses user
/// messages, but only when the caller explicitly establishes the managed
/// plaintext policy and only when every content part is readable by this
/// projection. Any `encrypted_content` part is opaque and fails closed: a
/// request-level policy cannot establish the provenance of historical items.
pub(crate) fn project_codex_agent_messages_for_third_party(
    body: &mut Value,
    managed_plaintext_policy: bool,
) -> Result<usize, ProxyError> {
    if !managed_plaintext_policy {
        return Ok(0);
    }

    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return Ok(0);
    };

    // Build the full result before replacing input so an invalid item cannot
    // leave the request partially projected.
    let mut projected = items.clone();
    let mut changed = 0;
    for item in &mut projected {
        if item.get("type").and_then(Value::as_str) != Some("agent_message") {
            continue;
        }

        let Some(content) = item.get("content").and_then(Value::as_array) else {
            return Err(unreadable_agent_payload_error());
        };
        let mut projected_content = Vec::with_capacity(content.len());
        let mut has_readable_content = false;
        for part in content {
            match part.get("type").and_then(Value::as_str) {
                Some("encrypted_content") => {
                    return Err(opaque_agent_payload_error());
                }
                Some("output_text") => {
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        return Err(unreadable_agent_payload_error());
                    };
                    projected_content.push(json!({"type": "input_text", "text": text}));
                    has_readable_content |= !text.is_empty();
                }
                Some("input_text") => {
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        return Err(unreadable_agent_payload_error());
                    };
                    projected_content.push(part.clone());
                    has_readable_content |= !text.is_empty();
                }
                Some("input_image" | "input_file" | "input_audio") => {
                    projected_content.push(part.clone());
                    has_readable_content = true;
                }
                _ => return Err(unreadable_agent_payload_error()),
            }
        }
        if projected_content.is_empty() || !has_readable_content {
            return Err(unreadable_agent_payload_error());
        }

        *item = json!({
            "type": "message",
            "role": "user",
            "content": projected_content
        });
        changed += 1;
    }

    if changed > 0 {
        body["input"] = Value::Array(projected);
    }
    Ok(changed)
}

fn opaque_agent_payload_error() -> ProxyError {
    ProxyError::InvalidRequest(
        "third-party child cannot read opaque Codex agent payload".to_string(),
    )
}

fn unreadable_agent_payload_error() -> ProxyError {
    ProxyError::InvalidRequest(
        "third-party child received a Codex agent message without readable content".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use serde_json::json;

    #[test]
    fn strips_markers_from_agents_namespace_variants_and_additional_tools() {
        let mut body = json!({
            "tools": [
                {"type":"namespace","name":"agents","tools":[
                    {"type":"function","name":"spawn_agent","parameters":{"properties":{"message":{"type":"string","encrypted":true}}}},
                    {"type":"function","name":"send_message","parameters":{"properties":{"message":{"encrypted":true}}}},
                    {"type":"function","name":"followup_task","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]},
                {"type":"namespace","name":"agents","children":[
                    {"type":"function","name":"spawn_agent","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]},
                {"type":"function","namespace":"agents","name":"send_message","parameters":{"properties":{"message":{"encrypted":true}}}}
            ],
            "input": [{"type":"additional_tools","tools":[
                {"type":"namespace","name":"agents","tools":[
                    {"type":"function","name":"followup_task","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]}
            ]}]
        });

        assert_eq!(make_codex_v2_agents_messages_plaintext(&mut body), 6);
        assert_eq!(
            body["tools"][0]["tools"][0]["parameters"]["properties"]["message"]["type"],
            "string"
        );
        assert!(
            body["tools"][0]["tools"][0]["parameters"]["properties"]["message"]
                .get("encrypted")
                .is_none()
        );
        assert!(
            body["input"][0]["tools"][0]["tools"][0]["parameters"]["properties"]["message"]
                .get("encrypted")
                .is_none()
        );
    }

    #[test]
    fn reserved_collaboration_and_non_agent_schemas_are_byte_equivalent() {
        let mut body = json!({
            "tools": [
                {"type":"namespace","name":"collaboration","tools":[
                    {"type":"function","name":"spawn_agent","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]},
                {"type":"namespace","name":"agents","tools":[
                    {"type":"function","namespace":"collaboration","name":"send_message","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]},
                {"type":"function","namespace":"collaboration","name":"send_message","parameters":{"properties":{"message":{"encrypted":true}}}},
                {"type":"function","name":"spawn_agent","parameters":{"properties":{"message":{"encrypted":true}}}}
            ],
            "input": [{"type":"additional_tools","tools":[
                {"type":"namespace","name":"collaboration","tools":[
                    {"type":"function","name":"followup_task","parameters":{"properties":{"message":{"encrypted":true}}}}
                ]}
            ]}]
        });
        let reserved = json!([
            body["tools"][0].clone(),
            body["tools"][1]["tools"][0].clone(),
            body["tools"][2].clone(),
            body["input"][0]["tools"][0].clone()
        ]);

        assert_eq!(make_codex_v2_agents_messages_plaintext(&mut body), 0);
        assert_eq!(
            json!([
                body["tools"][0],
                body["tools"][1]["tools"][0],
                body["tools"][2],
                body["input"][0]["tools"][0]
            ]),
            reserved
        );
        assert_eq!(
            body["tools"][3]["parameters"]["properties"]["message"]["encrypted"],
            true
        );
    }

    #[test]
    fn delivery_detector_matches_agents_surfaces_but_not_reserved_only() {
        let reserved = json!({"tools":[{"type":"namespace","name":"collaboration","tools":[
            {"type":"function","name":"spawn_agent"}
        ]}]});
        assert!(!has_codex_v2_agent_delivery(&reserved));

        let tools = json!({"tools":[{"type":"namespace","name":"agents","tools":[
            {"type":"function","name":"spawn_agent"}
        ]}]});
        assert!(has_codex_v2_agent_delivery(&tools));

        let additional = json!({"input":[{"type":"additional_tools","tools":[
            {"type":"function","namespace":"agents","name":"followup_task"}
        ]}]});
        assert!(has_codex_v2_agent_delivery(&additional));

        let message_only = json!({"input":[{"type":"agent_message","content":[]}]});
        assert!(has_codex_v2_agent_delivery(&message_only));

        let explicitly_reserved_inside_agents = json!({"tools":[{"type":"namespace","name":"agents","tools":[
            {"type":"function","namespace":"collaboration","name":"send_message"}
        ]}]});
        assert!(!has_codex_v2_agent_delivery(
            &explicitly_reserved_inside_agents
        ));
    }

    #[test]
    fn policy_false_preserves_agent_items_exactly() {
        let mut body = json!({"input":[{
            "type":"agent_message","author":"/root/child",
            "content":[{"type":"input_text","text":"task"}]
        }]});
        let original = body.clone();

        assert_eq!(
            project_codex_agent_messages_for_third_party(&mut body, false).unwrap(),
            0
        );
        assert_eq!(body, original);
    }

    #[test]
    fn projects_task_and_reply_text_and_preserves_supported_multimodal_parts() {
        let task = "Message Type: NEW_TASK\nTask name: /root/child\nPayload:\nread this";
        let reply = "Message Type: FINAL_ANSWER\nPayload:\nresult";
        let whitespace = "  \n ";
        let image =
            json!({"type":"input_image","image_url":"data:image/png;base64,abc","detail":"high"});
        let mut body = json!({"input":[
            {"type":"message","role":"developer","content":[{"type":"input_text","text":"keep"}]},
            {"type":"agent_message","author":"/root","recipient":"/root/child","content":[
                {"type":"input_text","text":task}, image,
                {"type":"input_file","file_id":"file_1"},
                {"type":"input_audio","data":"audio-data"},
                {"type":"input_text","text":whitespace}
            ]},
            {"type":"agent_message","author":"/root/child","recipient":"/root","content":[
                {"type":"output_text","text":reply}
            ]}
        ]});

        assert_eq!(
            project_codex_agent_messages_for_third_party(&mut body, true).unwrap(),
            2
        );
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["input"][1]["content"][0]["text"], task);
        assert_eq!(body["input"][1]["content"][1], image);
        assert_eq!(body["input"][1]["content"][2]["file_id"], "file_1");
        assert_eq!(body["input"][1]["content"][3]["data"], "audio-data");
        assert_eq!(body["input"][1]["content"][4]["text"], whitespace);
        assert_eq!(body["input"][2]["content"][0]["text"], reply);
    }

    #[test]
    fn rejects_every_encrypted_content_shape_without_echo_or_partial_mutation() {
        let base64_plaintext = base64::engine::general_purpose::STANDARD.encode([42_u8; 48]);
        let mut bytes = vec![0_u8; 73];
        bytes[0] = 0x80;
        let structured_ciphertext = URL_SAFE_NO_PAD.encode(bytes);
        let samples = [
            "opaque_ciphertext_NON_FERNET_DO_NOT_ECHO",
            "This was accidentally marked encrypted but is plainly readable.",
            base64_plaintext.as_str(),
            structured_ciphertext.as_str(),
        ];

        for sample in samples {
            let mut body = json!({"input":[
                {"type":"agent_message","content":[{"type":"input_text","text":"valid earlier item"}]},
                {"type":"agent_message","content":[
                    {"type":"input_text","text":"mixed readable text"},
                    {"type":"encrypted_content","encrypted_content":sample}
                ]}
            ]});
            let original = body.clone();

            let error = project_codex_agent_messages_for_third_party(&mut body, true).unwrap_err();
            assert!(error
                .to_string()
                .contains("cannot read opaque Codex agent payload"));
            assert!(!error.to_string().contains(sample));
            assert_eq!(body, original);
        }

        let mut malformed_encrypted = json!({"input":[{"type":"agent_message","content":[
            {"type":"encrypted_content","encrypted_content":null}
        ]}]});
        let error = project_codex_agent_messages_for_third_party(&mut malformed_encrypted, true)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("cannot read opaque Codex agent payload"));
    }

    #[test]
    fn rejects_missing_empty_and_malformed_agent_content_without_payload_echo() {
        let mut missing = json!({"input":[{"type":"agent_message"}]});
        let error = project_codex_agent_messages_for_third_party(&mut missing, true).unwrap_err();
        assert!(error.to_string().contains("without readable content"));

        for content in [
            json!(null),
            json!([]),
            json!([{"type":"unknown","secret":"do-not-echo"}]),
        ] {
            let mut body = json!({"input":[{"type":"agent_message","content":content}]});
            let error = project_codex_agent_messages_for_third_party(&mut body, true).unwrap_err();
            assert!(error.to_string().contains("without readable content"));
            assert!(!error.to_string().contains("do-not-echo"));
        }

        let mut mixed_unknown = json!({"input":[{"type":"agent_message","content":[
            {"type":"input_text","text":"readable"},
            {"type":"future_private_part","value":"do-not-echo"}
        ]}]});
        let original = mixed_unknown.clone();
        let error =
            project_codex_agent_messages_for_third_party(&mut mixed_unknown, true).unwrap_err();
        assert!(error.to_string().contains("without readable content"));
        assert!(!error.to_string().contains("do-not-echo"));
        assert_eq!(mixed_unknown, original);
    }
}
