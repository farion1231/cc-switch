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

pub(crate) fn add_delete_ops(messages: &[serde_json::Value], plan: &mut ToolRepairPlan) {
    let mut all_expected_ids: HashSet<String> = HashSet::new();
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                            all_expected_ids.insert(id.to_string());
                        }
                    }
                }
            }
        }
    }

    for (user_idx, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let Some(content) = msg.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        let accepted_in_place = plan.accepted_in_place_by_user.get(&user_idx);
        let extracted = plan.extracted_by_user.get(&user_idx);

        for (block_idx, block) in content.iter().enumerate() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let is_accepted_in_place =
                accepted_in_place.map(|s| s.contains(&block_idx)).unwrap_or(false);
            let is_extracted = extracted.map(|s| s.contains(&block_idx)).unwrap_or(false);
            let is_marker = block
                .get("_dsk_accepted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_accepted_in_place || is_extracted || is_marker {
                continue;
            }
            plan.ops
                .push(RepairOp::DeleteBlock { user_idx, block_idx });
            plan.deleted_by_user
                .entry(user_idx)
                .or_default()
                .insert(block_idx);
        }
    }
}

#[cfg(test)]
mod tests_add_delete_ops {
    use super::*;
    use serde_json::json;

    fn tool_result(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok"})
    }

    fn tool_result_accepted(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok", "_dsk_accepted": true})
    }

    fn tool_use(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "bash", "input": {}})
    }

    #[test]
    fn test_orphan_tool_result_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("ORPHAN")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        let deletes: Vec<_> = plan
            .ops
            .iter()
            .filter(|op| {
                matches!(op, RepairOp::DeleteBlock { user_idx: 1, block_idx: 1 })
            })
            .collect();
        assert_eq!(deletes.len(), 1, "orphan ORPHAN should be deleted");
    }

    #[test]
    fn test_duplicate_tool_result_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result_accepted("A"), tool_result("A")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        let deletes: Vec<_> = plan
            .ops
            .iter()
            .filter(|op| {
                matches!(op, RepairOp::DeleteBlock { user_idx: 1, block_idx: 1 })
            })
            .collect();
        assert_eq!(deletes.len(), 1, "duplicate A at block 1 should be deleted");
    }

    #[test]
    fn test_late_legitimate_tool_result_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "irrelevant"}]}),
            json!({"role": "user", "content": [tool_result("A")]}),
        ];
        let mut plan = ToolRepairPlan::default();

        add_delete_ops(&messages, &mut plan);

        let has_delete = plan.ops.iter().any(|op| {
            matches!(op, RepairOp::DeleteBlock { user_idx: 2, block_idx: 0 })
        });
        assert!(has_delete, "unapproved legitimate id should be deleted");
    }

    #[test]
    fn test_accepted_block_not_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result_accepted("A")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        assert!(plan.ops.is_empty(), "accepted block should not be deleted");
    }

    #[test]
    fn test_deleted_by_user_populated() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("ORPHAN")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        add_delete_ops(&messages, &mut plan);

        assert!(plan.deleted_by_user.contains_key(&1));
        assert!(plan.deleted_by_user[&1].contains(&0));
    }
}

pub(crate) fn aggregate_paired_remaining(messages: &[serde_json::Value], plan: &mut ToolRepairPlan) {
    let mut user_will_be_removed: HashSet<usize> = HashSet::new();
    for op in &plan.ops {
        match op {
            RepairOp::SplitAndPromote { source_user_idx, .. } => {
                if !plan
                    .extracted_by_user
                    .get(source_user_idx)
                    .map(|s| s.is_empty())
                    .unwrap_or(true)
                {
                    user_will_be_removed.insert(*source_user_idx);
                }
            }
            RepairOp::SynthesizePlaceholder {
                candidate_source_user_idx: Some(u),
                ..
            } => {
                user_will_be_removed.insert(*u);
            }
            _ => {}
        }
    }

    let conflict_users: Vec<usize> = plan
        .accepted_in_place_by_user
        .keys()
        .filter(|u| user_will_be_removed.contains(u))
        .copied()
        .collect();
    for u in conflict_users {
        if let Some(accepted) = plan.accepted_in_place_by_user.remove(&u) {
            let deleted = plan.deleted_by_user.entry(u).or_default();
            for bi in &accepted {
                deleted.insert(*bi);
                plan.ops.push(RepairOp::DeleteBlock {
                    user_idx: u,
                    block_idx: *bi,
                });
            }
        }
    }

    let mut final_remaining: HashMap<usize, Vec<serde_json::Value>> = HashMap::new();
    for (&user_idx, original) in &plan.original_content {
        let extracted = plan.extracted_by_user.get(&user_idx);
        let deleted = plan.deleted_by_user.get(&user_idx);
        let accepted = plan.accepted_in_place_by_user.get(&user_idx);

        let remaining: Vec<serde_json::Value> = original
            .iter()
            .enumerate()
            .filter_map(|(bi, block)| {
                let in_extracted = extracted.map(|s| s.contains(&bi)).unwrap_or(false);
                let in_deleted = deleted.map(|s| s.contains(&bi)).unwrap_or(false);
                let in_accepted = accepted.map(|s| s.contains(&bi)).unwrap_or(false);
                if in_extracted || in_deleted || in_accepted {
                    None
                } else {
                    Some(block.clone())
                }
            })
            .collect();

        if user_will_be_removed.contains(&user_idx) && !remaining.is_empty() {
            final_remaining.insert(user_idx, remaining);
        }
    }

    let mut source_to_op_indices: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    for (op_idx, op) in plan.ops.iter().enumerate() {
        match op {
            RepairOp::SplitAndPromote {
                source_user_idx,
                insert_after_assistant_idx,
                ..
            } => {
                source_to_op_indices
                    .entry(*source_user_idx)
                    .or_default()
                    .push((op_idx, *insert_after_assistant_idx));
            }
            RepairOp::SynthesizePlaceholder {
                candidate_source_user_idx: Some(u),
                insert_after_assistant_idx,
                ..
            } => {
                source_to_op_indices
                    .entry(*u)
                    .or_default()
                    .push((op_idx, *insert_after_assistant_idx));
            }
            _ => {}
        }
    }

    for (source_user, mut op_indices) in source_to_op_indices {
        op_indices.sort_by_key(|&(_, ia)| ia);
        let (last_op_idx, _) = *op_indices.last().unwrap();
        let remaining = final_remaining.remove(&source_user).unwrap_or_default();
        match &mut plan.ops[last_op_idx] {
            RepairOp::SplitAndPromote {
                ref mut paired_remaining_blocks,
                ref mut paired_source_user_idx,
                ..
            } => {
                *paired_remaining_blocks = remaining;
                *paired_source_user_idx = Some(source_user);
            }
            RepairOp::SynthesizePlaceholder {
                ref mut paired_remaining_blocks,
                ref mut paired_source_user_idx,
                ..
            } => {
                *paired_remaining_blocks = remaining;
                *paired_source_user_idx = Some(source_user);
            }
            _ => {}
        }
    }

    for (&user_idx, deleted_set) in &plan.deleted_by_user {
        if user_will_be_removed.contains(&user_idx) {
            continue;
        }
        let original_len = plan
            .original_content
            .get(&user_idx)
            .map(|v| v.len())
            .unwrap_or(0);
        let total_removed = deleted_set.len()
            + plan
                .accepted_in_place_by_user
                .get(&user_idx)
                .map(|s| s.len())
                .unwrap_or(0);
        if total_removed >= original_len {
            plan.ops.push(RepairOp::RemoveEmptyUser { user_idx });
        }
    }
}

#[cfg(test)]
mod tests_aggregate {
    use super::*;
    use serde_json::json;

    fn make_split_op(source: usize, assistant: usize) -> RepairOp {
        RepairOp::SplitAndPromote {
            source_user_idx: source,
            blocks_to_extract: vec![1],
            synthetic_blocks: vec![
                json!({"type": "tool_result", "tool_use_id": "A", "_dsk_accepted": true}),
            ],
            insert_after_assistant_idx: assistant,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        }
    }

    #[test]
    fn test_remaining_text_bound_to_last_op() {
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "before"},
                {"type": "tool_result", "tool_use_id": "A", "content": "ok", "_dsk_accepted": true},
                {"type": "text", "text": "after"}
            ]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content
            .insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.extracted_by_user.entry(1).or_default().insert(1);
        plan.ops.push(make_split_op(1, 0));

        aggregate_paired_remaining(&messages, &mut plan);

        if let RepairOp::SplitAndPromote {
            ref paired_remaining_blocks,
            ref paired_source_user_idx,
            ..
        } = plan.ops[0]
        {
            assert_eq!(*paired_source_user_idx, Some(1));
            assert_eq!(paired_remaining_blocks.len(), 2);
        } else {
            panic!("expected SplitAndPromote");
        }
    }

    #[test]
    fn test_two_ops_same_source_last_gets_remaining() {
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "A", "content": "ok", "_dsk_accepted": true},
                {"type": "tool_result", "tool_use_id": "B", "content": "ok", "_dsk_accepted": true},
                {"type": "text", "text": "remaining"}
            ]}),
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "B", "name": "b", "input": {}}]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content
            .insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.extracted_by_user.entry(1).or_default().extend([0, 1]);
        plan.ops.push(RepairOp::SplitAndPromote {
            source_user_idx: 1,
            blocks_to_extract: vec![0],
            synthetic_blocks: vec![json!({"type": "tool_result", "tool_use_id": "A"})],
            insert_after_assistant_idx: 0,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        });
        plan.ops.push(RepairOp::SplitAndPromote {
            source_user_idx: 1,
            blocks_to_extract: vec![1],
            synthetic_blocks: vec![json!({"type": "tool_result", "tool_use_id": "B"})],
            insert_after_assistant_idx: 2,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        });

        aggregate_paired_remaining(&messages, &mut plan);

        if let RepairOp::SplitAndPromote {
            insert_after_assistant_idx: 2,
            ref paired_remaining_blocks,
            ref paired_source_user_idx,
            ..
        } = plan.ops[1]
        {
            assert_eq!(*paired_source_user_idx, Some(1));
            assert_eq!(paired_remaining_blocks.len(), 1);
            assert_eq!(paired_remaining_blocks[0]["text"], "remaining");
        } else {
            panic!("expected last op to carry remaining");
        }

        if let RepairOp::SplitAndPromote {
            insert_after_assistant_idx: 0,
            ref paired_source_user_idx,
            ..
        } = plan.ops[0]
        {
            assert_eq!(*paired_source_user_idx, None);
        } else {
            panic!("first op should not carry remaining");
        }
    }

    #[test]
    fn test_remove_empty_user_added_when_all_blocks_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "ORPHAN", "content": "ok"}
            ]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content
            .insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.deleted_by_user.entry(1).or_default().insert(0);
        plan.ops.push(RepairOp::DeleteBlock {
            user_idx: 1,
            block_idx: 0,
        });

        aggregate_paired_remaining(&messages, &mut plan);

        let has_remove = plan
            .ops
            .iter()
            .any(|op| matches!(op, RepairOp::RemoveEmptyUser { user_idx: 1 }));
        assert!(has_remove, "fully-cleared user should get RemoveEmptyUser");
    }
}
