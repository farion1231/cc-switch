use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub(crate) enum RepairOp {
    SplitAndPromote {
        source_user_idx: usize,
        blocks_to_extract: Vec<usize>,
        synthetic_blocks: Vec<Value>,
        insert_after_assistant_idx: usize,
        paired_remaining_blocks: Vec<Value>,
        paired_source_user_idx: Option<usize>,
    },
    SynthesizePlaceholder {
        insert_after_assistant_idx: usize,
        synthetic_blocks: Vec<Value>,
        candidate_source_user_idx: Option<usize>,
        paired_remaining_blocks: Vec<Value>,
        paired_source_user_idx: Option<usize>,
    },
    DeleteBlock {
        user_idx: usize,
        block_idx: usize,
    },
    RemoveEmptyUser {
        user_idx: usize,
    },
}

#[derive(Debug, Default)]
pub(crate) struct ToolRepairPlan {
    pub ops: Vec<RepairOp>,
    pub extracted_by_user: HashMap<usize, HashSet<usize>>,
    pub deleted_by_user: HashMap<usize, HashSet<usize>>,
    pub accepted_in_place_by_user: HashMap<usize, HashSet<usize>>,
    pub original_content: HashMap<usize, Vec<Value>>,
}

pub(crate) fn build_plan(messages: &mut Vec<Value>) -> ToolRepairPlan {
    let mut plan = ToolRepairPlan::default();

    for (idx, msg) in messages.iter().enumerate() {
        if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
            plan.original_content.insert(idx, content.clone());
        }
    }

    let mut assistant_tool_uses: Vec<(usize, Vec<(usize, String)>)> = Vec::new();
    let mut user_tool_results: Vec<(usize, usize, String)> = Vec::new();

    for (msg_idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
            if role == "assistant" {
                let uses: Vec<(usize, String)> = content
                    .iter()
                    .enumerate()
                    .filter_map(|(bi, b)| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            b.get("id").and_then(|i| i.as_str()).map(|id| (bi, id.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !uses.is_empty() {
                    assistant_tool_uses.push((msg_idx, uses));
                }
            } else if role == "user" {
                for (bi, block) in content.iter().enumerate() {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str()) {
                            user_tool_results.push((msg_idx, bi, id.to_string()));
                        }
                    }
                }
            }
        }
    }

    for (assistant_idx, expected_ids_with_blocks) in &assistant_tool_uses {
        let expected_ids: Vec<String> =
            expected_ids_with_blocks.iter().map(|(_, id)| id.clone()).collect();
        if expected_ids.is_empty() {
            continue;
        }

        let next_user_idx = (assistant_idx + 1..messages.len()).find(|&i| {
            messages[i].get("role").and_then(|r| r.as_str()) == Some("user")
        });

        let is_case_a = if let Some(nu) = next_user_idx {
            if let Some(content) = messages[nu].get("content").and_then(|v| v.as_array()) {
                let n = expected_ids.len();
                content.len() >= n
                    && content[..n].iter().zip(expected_ids.iter()).all(|(block, eid)| {
                        block.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                            && block.get("tool_use_id").and_then(|i| i.as_str())
                                == Some(eid.as_str())
                    })
            } else {
                false
            }
        } else {
            false
        };

        if is_case_a {
            let nu = next_user_idx.unwrap();
            let n = expected_ids.len();
            if let Some(content) =
                messages[nu].get_mut("content").and_then(|v| v.as_array_mut())
            {
                for block in content[..n].iter_mut() {
                    if let Some(obj) = block.as_object_mut() {
                        obj.insert("_dsk_accepted".into(), Value::Bool(true));
                    }
                }
            }
            let set = plan.accepted_in_place_by_user.entry(nu).or_default();
            for bi in 0..n {
                set.insert(bi);
            }
            continue;
        }

        let mut user_matches: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (user_idx, block_idx, tool_use_id) in &user_tool_results {
            if *user_idx <= *assistant_idx {
                continue;
            }
            if let Some(eid_pos) = expected_ids.iter().position(|e| e == tool_use_id) {
                user_matches
                    .entry(*user_idx)
                    .or_default()
                    .push((*block_idx, eid_pos));
            }
        }

        if user_matches.is_empty() {
            let synthetic_blocks: Vec<Value> = expected_ids
                .iter()
                .map(|id| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": "[no result]",
                        "is_error": false,
                        "_dsk_accepted": true
                    })
                })
                .collect();
            plan.ops.push(RepairOp::SynthesizePlaceholder {
                insert_after_assistant_idx: *assistant_idx,
                synthetic_blocks,
                candidate_source_user_idx: next_user_idx,
                paired_remaining_blocks: Vec::new(),
                paired_source_user_idx: None,
            });
        } else {
            let source_user_idx = user_matches
                .iter()
                .max_by_key(|(&uidx, matches)| (matches.len(), usize::MAX - uidx))
                .map(|(&uidx, _)| uidx)
                .unwrap();

            let source_matches = user_matches[&source_user_idx].clone();
            let mut blocks_to_extract: Vec<usize> = Vec::new();
            let synthetic_blocks: Vec<Value> = expected_ids
                .iter()
                .enumerate()
                .map(|(ei, id)| {
                    if let Some(&(block_idx, _)) = source_matches.iter().find(|(_, e)| *e == ei) {
                        let block = plan.original_content[&source_user_idx][block_idx].clone();
                        blocks_to_extract.push(block_idx);
                        if let Some(content) = messages[source_user_idx]
                            .get_mut("content")
                            .and_then(|v| v.as_array_mut())
                        {
                            if let Some(orig) = content.get_mut(block_idx) {
                                if let Some(obj) = orig.as_object_mut() {
                                    obj.insert("_dsk_accepted".into(), Value::Bool(true));
                                }
                            }
                        }
                        let mut b = block;
                        if let Some(obj) = b.as_object_mut() {
                            obj.insert("_dsk_accepted".into(), Value::Bool(true));
                        }
                        b
                    } else {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": "[no result]",
                            "is_error": false,
                            "_dsk_accepted": true
                        })
                    }
                })
                .collect();

            plan.extracted_by_user
                .entry(source_user_idx)
                .or_default()
                .extend(blocks_to_extract.iter().copied());

            plan.ops.push(RepairOp::SplitAndPromote {
                source_user_idx,
                blocks_to_extract,
                synthetic_blocks,
                insert_after_assistant_idx: *assistant_idx,
                paired_remaining_blocks: Vec::new(),
                paired_source_user_idx: None,
            });
        }
    }

    plan
}

#[cfg(feature = "test-hooks")]
pub fn inspect_plan(messages: &mut Vec<Value>) -> Vec<String> {
    let plan = build_plan(messages);
    plan.ops.iter().map(|op| format!("{:?}", op)).collect()
}

pub fn repair_tool_order(_messages: &mut Vec<Value>) {
    // implemented in T09-T11
    todo!()
}

#[cfg(test)]
mod tests_build_plan {
    use super::*;
    use serde_json::json;

    fn tool_use(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "bash", "input": {}})
    }

    fn tool_result(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok"})
    }

    #[test]
    fn test_case_a_no_op() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("B")]}),
        ];
        let plan = build_plan(&mut messages);
        assert!(plan.ops.is_empty(), "case(a) should produce no ops");
        assert_eq!(messages[1]["content"][0]["_dsk_accepted"], true);
        assert_eq!(messages[1]["content"][1]["_dsk_accepted"], true);
    }

    #[test]
    fn test_case_a_order_mismatch_becomes_b() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("A")]}),
        ];
        let plan = build_plan(&mut messages);
        let has_split = plan
            .ops
            .iter()
            .any(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(has_split, "order mismatch should produce SplitAndPromote");
    }

    #[test]
    fn test_case_b_tool_result_in_text_mixed_user() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "before"},
                tool_result("A"),
                {"type": "text", "text": "after"}
            ]}),
        ];
        let plan = build_plan(&mut messages);
        let split = plan
            .ops
            .iter()
            .find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some(), "should have SplitAndPromote");
        if let Some(RepairOp::SplitAndPromote {
            blocks_to_extract,
            source_user_idx,
            ..
        }) = split
        {
            assert_eq!(*source_user_idx, 1);
            assert!(blocks_to_extract.contains(&1));
        }
    }

    #[test]
    fn test_case_c_full_missing_produces_synthesize_placeholder() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "something else"}]}),
        ];
        let plan = build_plan(&mut messages);
        let synth = plan
            .ops
            .iter()
            .find(|op| matches!(op, RepairOp::SynthesizePlaceholder { .. }));
        assert!(synth.is_some(), "all missing → SynthesizePlaceholder");
        if let Some(RepairOp::SynthesizePlaceholder {
            synthetic_blocks,
            insert_after_assistant_idx,
            ..
        }) = synth
        {
            assert_eq!(*insert_after_assistant_idx, 0);
            assert_eq!(synthetic_blocks.len(), 2);
            assert_eq!(synthetic_blocks[0]["tool_use_id"], "A");
            assert_eq!(synthetic_blocks[1]["tool_use_id"], "B");
        }
    }

    #[test]
    fn test_case_b_partial_hit_placeholder_inline() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B"), tool_use("C")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("C")]}),
        ];
        let plan = build_plan(&mut messages);
        assert!(!plan
            .ops
            .iter()
            .any(|op| matches!(op, RepairOp::SynthesizePlaceholder { .. })));
        let split = plan
            .ops
            .iter()
            .find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some());
        if let Some(RepairOp::SplitAndPromote {
            synthetic_blocks, ..
        }) = split
        {
            assert_eq!(synthetic_blocks.len(), 3);
            assert_eq!(synthetic_blocks[0]["tool_use_id"], "A");
            assert_eq!(synthetic_blocks[1]["tool_use_id"], "B");
            assert_eq!(synthetic_blocks[1]["content"], "[no result]");
            assert_eq!(synthetic_blocks[2]["tool_use_id"], "C");
        }
    }

    #[test]
    fn test_single_source_user_selection() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B"), tool_use("C")]}),
            json!({"role": "user", "content": [tool_result("A")]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "ok"}]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("C")]}),
        ];
        let plan = build_plan(&mut messages);
        let split = plan
            .ops
            .iter()
            .find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some());
        if let Some(RepairOp::SplitAndPromote {
            source_user_idx, ..
        }) = split
        {
            assert_eq!(*source_user_idx, 3, "should select user with most matches");
        }
    }

    #[test]
    fn test_no_tool_uses_no_ops() {
        let mut messages = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "hi"}]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "hello"}]}),
        ];
        let plan = build_plan(&mut messages);
        assert!(plan.ops.is_empty());
    }
}
