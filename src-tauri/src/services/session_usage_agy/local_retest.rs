//! Opt-in audit of copied local transcripts and read-only native database snapshots.
use super::*;
use crate::session_manager::{
    content::{resolve_content_ref, SourceLocation, ValidatedSource},
    model::SessionBlock,
};
use serde_json::json;
use std::{collections::HashSet, fs, path::PathBuf};

#[test]
#[ignore = "requires CC_SWITCH_AGY_RETEST_ROOT containing copied local data"]
fn local_snapshot_reader_and_sql_usage_are_consistent() {
    let root = PathBuf::from(std::env::var("CC_SWITCH_AGY_RETEST_ROOT").expect("snapshot root"));
    let report_path =
        PathBuf::from(std::env::var("CC_SWITCH_AGY_RETEST_REPORT").expect("report path"));
    let db = Database::memory().unwrap();
    let mut sessions = HashSet::new();
    let (mut files, mut messages_count, mut calls_count, mut results_count, mut refs_count) =
        (0, 0, 0, 0, 0);
    let mut errors = Vec::new();
    let mut profiles = Vec::new();
    let (mut legacy_file_calls, mut incomplete_edit_calls) = (0, 0);
    for name in [
        "antigravity-cli",
        "antigravity",
        "antigravity-ide",
        "antigravity-acp",
    ] {
        let profile = root.join(name);
        let mut paths: Vec<_> = fs::read_dir(profile.join("brain"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| {
                entry
                    .path()
                    .join(".system_generated/logs/transcript_full.jsonl")
            })
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        let mut profile_files = 0;
        for path in paths {
            let session = path
                .ancestors()
                .nth(3)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            files += 1;
            profile_files += 1;
            let original = fs::read(&path).unwrap();
            let rows = agy::records(&path).unwrap();
            let messages = match agy::load_messages(&path) {
                Ok(messages) => messages,
                Err(_) => {
                    errors.push(json!({"profile":name,"session":session,"category":"parse"}));
                    continue;
                }
            };
            messages_count += messages.len();
            let ids: HashSet<_> = messages.iter().filter_map(|m| m.id.as_ref()).collect();
            if ids.len() != messages.len() {
                errors.push(
                    json!({"profile":name,"session":session,"category":"duplicate_message_ids"}),
                );
            }
            let source = ValidatedSource {
                provider_id: "agy".into(),
                raw: path.to_string_lossy().into_owned(),
                location: SourceLocation::Path {
                    path: path.clone(),
                    root: profile.join("brain"),
                },
            };
            let mut call_ids = HashSet::new();
            for block in messages.iter().flat_map(|m| &m.blocks) {
                if let SessionBlock::ToolCall {
                    id,
                    raw_name,
                    title,
                    diff,
                    input_preview,
                    input_full,
                    ..
                } = block
                {
                    calls_count += 1;
                    if !call_ids.insert(id) {
                        errors.push(json!({"profile":name,"session":session,"category":"duplicate_call_ids"}));
                    }
                    if matches!(raw_name.as_str(), "read_file" | "write_file" | "edit_file") {
                        legacy_file_calls += 1;
                        if title.is_empty() || (raw_name != "read_file" && diff.is_none()) {
                            errors.push(json!({"profile":name,"session":session,"category":"legacy_file_shape"}));
                        }
                    }
                    if raw_name == "multi_replace_file_content" {
                        let input = match input_full {
                            Some(reference) => {
                                resolve_content_ref(&source, reference).unwrap_or_default()
                            }
                            None => input_preview.clone(),
                        };
                        let args: Value = serde_json::from_str(&input).unwrap_or_default();
                        let complete = args["edits"].as_array().is_some_and(|edits| {
                            !edits.is_empty()
                                && edits.iter().all(|edit| {
                                    edit["old_string"].is_string() && edit["new_string"].is_string()
                                })
                        });
                        if !complete {
                            incomplete_edit_calls += 1;
                            if diff.is_some() {
                                errors.push(json!({"profile":name,"session":session,"category":"partial_diff_summary"}));
                            }
                        }
                    }
                }
            }
            for block in messages.iter().flat_map(|m| &m.blocks) {
                let mut refs = Vec::new();
                match block {
                    SessionBlock::Thinking { full, .. }
                    | SessionBlock::Text { full, .. }
                    | SessionBlock::Event { full, .. } => refs.extend(full),
                    SessionBlock::ToolCall {
                        input_full, diff, ..
                    } => {
                        refs.extend(input_full);
                        if let Some(diff) = diff {
                            refs.extend(&diff.full);
                        }
                    }
                    SessionBlock::ToolResult { call_id, full, .. } => {
                        results_count += 1;
                        if !call_ids.contains(call_id) {
                            errors.push(json!({"profile":name,"session":session,"category":"orphan_result"}));
                        }
                        refs.extend(full);
                    }
                    _ => {}
                }
                for reference in refs {
                    refs_count += 1;
                    if resolve_content_ref(&source, reference).is_err() {
                        errors.push(json!({"profile":name,"session":session,"category":"full_content_ref","reference":reference}));
                    }
                }
            }
            serde_json::to_vec(&messages).unwrap();
            if sessions.insert(session.clone()) {
                match import_records(&db,&session,&rows,Some(&path)) {
                    Ok(_) => match import_records(&db,&session,&rows,Some(&path)) {
                        Ok((0,_)) => {},
                        _ => errors.push(json!({"profile":name,"session":session,"category":"repeat_usage_import"})),
                    },
                    Err(_) => errors.push(json!({"profile":name,"session":session,"category":"usage_import"})),
                }
            }
            if fs::read(&path).unwrap() != original || agy::records(&path).unwrap() != rows {
                errors.push(
                    json!({"profile":name,"session":session,"category":"transcript_mutated"}),
                );
            }
        }
        profiles.push(json!({"profile":name,"files":profile_files}));
    }
    let conn = db.conn.lock().unwrap();
    let (usage_rows, unknown_models): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(model='unknown'),0) FROM proxy_request_logs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let report = json!({"files":files,"unique_sessions":sessions.len(),"messages":messages_count,
        "tool_calls":calls_count,"tool_results":results_count,"full_content_refs":refs_count,
        "usage_rows":usage_rows,"unknown_models":unknown_models,"profiles":profiles,"errors":errors,
        "legacy_file_calls":legacy_file_calls,"incomplete_edit_calls":incomplete_edit_calls});
    fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("local snapshot: {files} files, {messages_count} messages, {refs_count} full-content refs, {usage_rows} usage rows, {} errors",errors.len());
    assert!(files > 0, "snapshot must contain transcripts");
    assert!(
        errors.is_empty(),
        "see aggregate report for error categories"
    );
}
