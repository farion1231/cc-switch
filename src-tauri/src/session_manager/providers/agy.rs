//! Read Antigravity's exported transcript; never open its binary conversation store for writing.
use super::{
    blocks::{
        assign_turn_ids, normalize_tool, parse_antigravity_args, thinking_block, tool_call_block,
        tool_result_block, ToolSource,
    },
    utils::{parse_timestamp_to_ms, truncate_summary},
};
use crate::session_manager::model::{ContentRef, MessageMeta, SessionBlock, ToolKind, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Antigravity CLI keeps sessions in `~/.gemini/antigravity-cli/brain`, while the IDE writes the
/// same layout to `~/.gemini/antigravity/brain`. API-key providers run agy with a private
/// `--gemini_dir` (`~/.cc-switch/agy/<provider>/`), so their sessions live under that directory's
/// `antigravity-cli/brain`. All of them are scanned.
pub fn session_roots() -> Vec<PathBuf> {
    session_sources()
        .into_iter()
        .map(|(root, _)| root)
        .collect()
}

/// Each root, with the private `--gemini_dir` its sessions must be resumed with.
fn session_sources() -> Vec<(PathBuf, Option<PathBuf>)> {
    let cli = crate::agy_config::agy_dir();
    let ide = cli.with_file_name("antigravity");
    let mut sources = vec![(cli.join("brain"), None), (ide.join("brain"), None)];
    let mut profiles: Vec<PathBuf> = fs::read_dir(crate::agy_config::key_profiles_root())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect();
    profiles.sort();
    for dir in profiles {
        sources.push((dir.join("antigravity-cli").join("brain"), Some(dir)));
    }
    sources
}

pub(crate) fn records(path: &Path) -> Result<Vec<Value>, String> {
    let file =
        fs::File::open(path).map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
    // The export can rewrite steps as they finish. The latest record for a step wins.
    let mut steps = BTreeMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(index) = value["step_index"].as_u64() {
            steps.insert(index, value);
        }
    }
    Ok(steps.into_values().collect())
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan_roots(&session_sources())
}

/// Missing roots are tolerated; a session id found in several roots is reported once (first wins).
fn scan_roots(sources: &[(PathBuf, Option<PathBuf>)]) -> Vec<SessionMeta> {
    let mut seen = std::collections::HashSet::new();
    let mut sessions = Vec::new();
    for (root, gemini_dir) in sources {
        for meta in scan_root(root, gemini_dir.as_deref()) {
            if seen.insert(meta.session_id.clone()) {
                sessions.push(meta);
            }
        }
    }
    sessions
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn scan_root(root: &Path, gemini_dir: Option<&Path>) -> Vec<SessionMeta> {
    let mut summaries = read_summaries(
        &root
            .parent()
            .unwrap_or(root)
            .join("conversation_summaries.db"),
        gemini_dir,
    );
    let canonical_root = root.canonicalize().ok();
    let mut sessions = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(id) = entry
                .file_name()
                .to_str()
                .map(str::to_string)
                .filter(|id| valid_session_id(id))
            else {
                continue;
            };
            let path = entry
                .path()
                .join(".system_generated/logs/transcript_full.jsonl");
            if !path.is_file()
                || !path
                    .canonicalize()
                    .ok()
                    .zip(canonical_root.as_ref())
                    .is_some_and(|(path, root)| path.starts_with(root))
            {
                continue;
            }
            let summary = summaries.remove(&id);
            // Avoid parsing every transcript on list refresh when agy's own index is available.
            let mut meta = if let Some(meta) = summary {
                meta
            } else {
                let Ok(rows) = records(&path) else { continue };
                let first = rows
                    .iter()
                    .find(|v| v["type"] == "USER_INPUT")
                    .and_then(|v| v["content"].as_str())
                    .map(user_request);
                SessionMeta {
                    provider_id: "agy".into(),
                    session_id: id.clone(),
                    title: first.map(|s| truncate_summary(s, 80)),
                    summary: first.map(|s| truncate_summary(s, 160)),
                    project_dir: None,
                    created_at: rows.iter().find_map(timestamp),
                    last_active_at: rows.iter().filter_map(timestamp).max(),
                    source_path: None,
                    resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
                }
            };
            meta.source_path = Some(path.to_string_lossy().into_owned());
            sessions.push(meta);
        }
    }
    // A binary-only conversation can still be found and resumed, but has no readable transcript.
    sessions.extend(summaries.into_values());
    sessions
}

fn read_summaries(path: &Path, gemini_dir: Option<&Path>) -> BTreeMap<String, SessionMeta> {
    let mut result = BTreeMap::new();
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return result;
    };
    let Ok(mut query) = conn.prepare("SELECT conversation_id, title, workspace_uris, preview, last_modified_time FROM conversation_summaries") else { return result };
    let Ok(rows) = query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    }) else {
        return result;
    };
    for (id, title, uris, preview, modified) in rows.flatten() {
        if !valid_session_id(&id) {
            continue;
        }
        let project_dir = serde_json::from_str::<Vec<String>>(&uris)
            .ok()
            .and_then(|v| v.into_iter().next())
            .and_then(|uri| url::Url::parse(&uri).ok()?.to_file_path().ok())
            .map(|p| p.to_string_lossy().into_owned());
        let last_active_at =
            parse_timestamp_to_ms(&Value::String(modified.clone())).or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&modified, "%Y-%m-%d %H:%M:%S%.f")
                    .ok()
                    .map(|t| t.and_utc().timestamp_millis())
            });
        result.insert(
            id.clone(),
            SessionMeta {
                provider_id: "agy".into(),
                session_id: id.clone(),
                title: (!title.is_empty()).then_some(title),
                summary: (!preview.is_empty()).then(|| truncate_summary(&preview, 160)),
                project_dir,
                created_at: None,
                last_active_at,
                source_path: None,
                resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
            },
        );
    }
    result
}

/// agy stores each prompt wrapped in `<USER_REQUEST>`, followed by context it adds itself
/// (`<ADDITIONAL_METADATA>`, `<USER_SETTINGS_CHANGE>`, ...). Only the part the user typed is shown.
fn user_request(content: &str) -> &str {
    content
        .split_once("<USER_REQUEST>")
        .and_then(|(_, rest)| rest.split_once("</USER_REQUEST>"))
        .map(|(inner, _)| inner.trim())
        .filter(|inner| !inner.is_empty())
        .unwrap_or(content)
}

pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    value
        .get("ts")
        .and_then(parse_timestamp_to_ms)
        .or_else(|| parse_timestamp_to_ms(&value["created_at"]))
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data =
        fs::read_to_string(path).map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
    // Keep physical byte ranges so previews can retrieve the original JSONL text.
    let mut offset = 0u64;
    let mut steps: Vec<_> = data
        .split_inclusive('\n')
        .filter_map(|line| {
            let start = offset;
            offset += line.len() as u64;
            let value = serde_json::from_str::<Value>(line).ok()?;
            Some((value, start, u32::try_from(line.len()).ok()))
        })
        .collect();
    let mut latest: Vec<(Value, u64, Option<u32>)> = Vec::new();
    for step in steps.drain(..) {
        if latest.last().is_some_and(|previous| {
            step.0["step_index"].as_i64().is_some()
                && previous.0["step_index"] == step.0["step_index"]
                && previous.0["type"] == step.0["type"]
        }) {
            latest.pop();
        }
        latest.push(step);
    }
    let steps = latest;
    let mut planner_counts = HashMap::new();
    for (step, ..) in &steps {
        if is_antigravity_planner(step) {
            if let Some(index) = step.get("step_index").and_then(Value::as_i64) {
                *planner_counts.entry(index).or_insert(0usize) += 1;
            }
        }
    }
    // Rewound/replayed logs can reuse step indices. Only those planners need a
    // physical-row suffix; ordinary logs retain their existing deterministic IDs.
    let step_ids: Vec<_> = steps
        .iter()
        .enumerate()
        .map(
            |(row, (step, ..))| match step.get("step_index").and_then(Value::as_i64) {
                Some(index) if planner_counts.get(&index).copied().unwrap_or(0) > 1 => {
                    format!("{index}:row-{row}")
                }
                Some(index) => index.to_string(),
                None => format!("row-{row}"),
            },
        )
        .collect();
    // Resolve forwards within one planner's execution segment. A new request,
    // planner, or index rewind ends the segment, so later records cannot replace
    // an earlier result or finish a call from another turn.
    let mut pending_calls = HashMap::new();
    let mut result_calls = HashMap::new();
    let mut previous_index = None;
    for (row, (step, ..)) in steps.iter().enumerate() {
        let index = step.get("step_index").and_then(Value::as_i64);
        let planner = is_antigravity_planner(step);
        let user_input = step.get("type").and_then(Value::as_str) == Some("USER_INPUT");
        let rewound = index
            .zip(previous_index)
            .is_some_and(|(current, previous)| current <= previous);
        if planner || user_input || rewound {
            pending_calls.clear();
        }
        if let Some(index) = index {
            previous_index = Some(index);
            if planner {
                for (tool_index, call) in antigravity_calls(step).enumerate() {
                    let Some(name) = call
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    else {
                        continue;
                    };
                    let Some(result_index) = index
                        .checked_add(1)
                        .and_then(|n| n.checked_add(i64::try_from(tool_index).ok()?))
                    else {
                        continue;
                    };
                    pending_calls.insert(
                        result_index,
                        (format!("agy:{}:{tool_index}", step_ids[row]), name),
                    );
                }
            } else if let Some((call_id, name)) = pending_calls.remove(&index) {
                if is_antigravity_tool_result(step, name) {
                    result_calls.insert(row, (call_id, name));
                }
            }
        }
    }
    let mut result = Vec::new();
    for (row, (value, offset, len)) in steps.iter().enumerate() {
        let ts = timestamp(value);
        let step_index = value.get("step_index").and_then(Value::as_i64);
        let full = |pointer: String| {
            len.map(|len| ContentRef::Jsonl {
                offset: *offset,
                len,
                pointer,
            })
        };
        let (role, blocks) = if let Some((call_id, name)) = result_calls.get(&row) {
            (
                "tool",
                vec![antigravity_tool_result(value, call_id, name, &full)],
            )
        } else if value.get("type").and_then(Value::as_str) == Some("USER_INPUT") {
            let content = user_request(&extract_antigravity_content(value)).to_string();
            if content.is_empty() {
                continue;
            }
            ("user", vec![SessionBlock::text(content)])
        } else if is_antigravity_planner(value) {
            (
                "assistant",
                antigravity_model_blocks(value, &step_ids[row], &full),
            )
        } else {
            // Keep future event types visible, as the upstream reader does.
            let content = extract_antigravity_content(value);
            let content = if content.is_empty() {
                serde_json::to_string_pretty(value).unwrap_or_default()
            } else {
                content
            };
            let role = if value["source"] == "MODEL" {
                "assistant"
            } else {
                "system"
            };
            (role, vec![SessionBlock::text(content)])
        };
        if blocks.is_empty() {
            continue;
        }
        let mut message = SessionMessage::from_blocks(role, ts, blocks);
        message.id = step_index.map(|n| n.to_string());

        message.meta = Some(MessageMeta {
            model: value["model"].as_str().map(String::from),
            input_tokens: value["input_tokens"].as_u64(),
            output_tokens: value["output_tokens"].as_u64(),
            cache_read_tokens: value["cache_read_tokens"].as_u64(),
            cache_write_tokens: value["cache_write_tokens"].as_u64(),
            reasoning_tokens: value["reasoning_tokens"].as_u64(),
            duration_ms: value["duration_ms"]
                .as_u64()
                .filter(|d| *d > 0 && *d <= 3_600_000),
            ..Default::default()
        });

        result.push(message);
    }

    assign_turn_ids(&mut result);
    Ok(result)
}

fn is_antigravity_planner(step: &Value) -> bool {
    step.get("type").and_then(Value::as_str) == Some("PLANNER_RESPONSE")
}

fn antigravity_calls(step: &Value) -> impl Iterator<Item = &Value> {
    step.get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn is_antigravity_tool_result(step: &Value, name: &str) -> bool {
    let source = step.get("source").and_then(Value::as_str);
    let kind = step.get("type").and_then(Value::as_str);
    if source == Some("SYSTEM") && kind == Some("ERROR_MESSAGE") {
        return true;
    }
    if source != Some("MODEL") {
        return false;
    }
    let expected = match name {
        "replace_file_content" | "multi_replace_file_content" | "write_to_file" => {
            "CODE_ACTION".to_string()
        }
        "list_dir" => "LIST_DIRECTORY".to_string(),
        _ => name.to_ascii_uppercase(),
    };
    kind == Some("GENERIC") || kind == Some(expected.as_str())
}

// Derived content keeps the existing reader contract and physical JSONL source validation.
pub(crate) const ANTIGRAVITY_READER_REF_PREFIX: &str = "/_cc_switch/antigravity/tool_calls/";

fn antigravity_reader_args(raw: &Value) -> Value {
    fn rename_fields(value: &mut Value) {
        let Some(map) = value.as_object_mut() else {
            return;
        };
        for (alias, key) in [
            ("CommandLine", "command"),
            ("AbsolutePath", "path"),
            ("DirectoryPath", "path"),
            ("TargetFile", "path"),
            ("SearchPath", "path"),
            ("SearchDirectory", "path"),
            ("TargetContent", "old_string"),
            ("ReplacementContent", "new_string"),
            ("ReplacementChunks", "edits"),
            ("CodeContent", "content"),
            ("Url", "url"),
            ("Pattern", "pattern"),
            ("StartLine", "start_line"),
            ("EndLine", "end_line"),
        ] {
            if !map.contains_key(key) {
                if let Some(value) = map.remove(alias) {
                    map.insert(key.into(), value);
                }
            }
        }
        if let Some(edits) = map.get_mut("edits").and_then(Value::as_array_mut) {
            for edit in edits {
                rename_fields(edit);
            }
        }
    }
    // Decode exactly once: textual values such as "true" must not become booleans on replay.
    let mut args = parse_antigravity_args(raw);
    rename_fields(&mut args);
    args
}

fn antigravity_diff_pairs(args: &Value, kind: ToolKind) -> Option<Vec<(&str, &str)>> {
    match kind {
        ToolKind::Write => Some(vec![("", args.get("content")?.as_str()?)]),
        ToolKind::Edit => {
            fn pair(item: &Value) -> Option<(&str, &str)> {
                Some((
                    item.get("old_string")?.as_str()?,
                    item.get("new_string")?.as_str()?,
                ))
            }
            match args.get("edits") {
                Some(edits) => {
                    let edits = edits.as_array()?;
                    if edits.is_empty() {
                        return None;
                    }
                    // An incomplete chunk list cannot represent a complete diff.
                    edits.iter().map(pair).collect()
                }
                None => Some(vec![pair(args)?]),
            }
        }
        _ => None,
    }
}

pub(crate) fn resolve_antigravity_reader_content(
    step: &Value,
    pointer: &str,
) -> Result<String, String> {
    let stale = || "会话已更新，请重新读取".to_string();
    if step.get("source").and_then(Value::as_str) != Some("MODEL")
        || step.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE")
    {
        return Err(stale());
    }
    let (index, field) = pointer
        .strip_prefix(ANTIGRAVITY_READER_REF_PREFIX)
        .and_then(|suffix| suffix.split_once('/'))
        .ok_or_else(stale)?;
    let index = index.parse::<usize>().map_err(|_| stale())?;
    let call = antigravity_calls(step).nth(index).ok_or_else(stale)?;
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(stale)?;
    let args = antigravity_reader_args(call.get("args").unwrap_or(&Value::Null));
    match field {
        "input" => serde_json::to_string_pretty(&args).map_err(|_| stale()),
        "diff" => {
            let kind = normalize_tool(ToolSource::Gemini, name, None).kind;
            let pairs = antigravity_diff_pairs(&args, kind).ok_or_else(stale)?;
            let mut diff = String::new();
            for (old, new) in pairs {
                diff.push_str("@@\n");
                for (prefix, text) in [('-', old), ('+', new)] {
                    let normalized = text.replace("\r\n", "\n");
                    let text = normalized.strip_suffix('\n').unwrap_or(&normalized);
                    if !normalized.is_empty() {
                        // Keep content resembling paired ---/+++ file headers visible in the reader.
                        if prefix == '+'
                            && text.starts_with("++ ")
                            && diff
                                .lines()
                                .last()
                                .is_some_and(|line| line.starts_with("--- "))
                        {
                            diff.push_str("@@\n");
                        }
                        for line in text.split('\n') {
                            diff.push(prefix);
                            diff.push_str(line);
                            diff.push('\n');
                        }
                    }
                }
            }
            Ok(diff)
        }
        _ => Err(stale()),
    }
}

fn antigravity_model_blocks(
    step: &Value,
    step_id: &str,
    full: &dyn Fn(String) -> Option<ContentRef>,
) -> Vec<SessionBlock> {
    let mut blocks = Vec::new();
    if let Some(thinking) = step.get("thinking").and_then(Value::as_str) {
        if !thinking.trim().is_empty() {
            blocks.push(thinking_block(thinking.trim(), None, None, || {
                full("/thinking".into())
            }));
        }
    }
    let content = extract_antigravity_content(step);
    if !content.is_empty() {
        blocks.push(SessionBlock::text(content));
    }
    for (tool_index, call) in antigravity_calls(step).enumerate() {
        let Some(name) = call
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let args = antigravity_reader_args(call.get("args").unwrap_or(&Value::Null));
        let mut block = tool_call_block(
            ToolSource::Gemini,
            format!("agy:{step_id}:{tool_index}"),
            name,
            &args,
            || full(format!("{ANTIGRAVITY_READER_REF_PREFIX}{tool_index}/input")),
        );
        if let SessionBlock::ToolCall {
            input_total_len, ..
        } = &mut block
        {
            // The existing reader compares JavaScript string.length (UTF-16 code units).
            let input = match &args {
                Value::Null => String::new(),
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            *input_total_len = input.encode_utf16().count().min(u32::MAX as usize) as u32;
        }
        if let SessionBlock::ToolCall {
            kind,
            diff: Some(diff),
            ..
        } = &mut block
        {
            if antigravity_diff_pairs(&args, *kind).is_some() {
                diff.full = full(format!("{ANTIGRAVITY_READER_REF_PREFIX}{tool_index}/diff"));
            }
        }
        blocks.push(block);
    }
    blocks
}

fn antigravity_tool_result(
    step: &Value,
    call_id: &str,
    tool_name: &str,
    full: &dyn Fn(String) -> Option<ContentRef>,
) -> SessionBlock {
    // Tool output is literal text: user-request/metadata markers are not wrappers here.
    let content = extract_antigravity_content(step);
    let (output, pointer) = if content.trim().is_empty() {
        (
            step.get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            "/error",
        )
    } else {
        let pointer = if step.get("content").is_some_and(Value::is_object) {
            "/content/text"
        } else {
            "/content"
        };
        (content, pointer)
    };
    let exit_code = if matches!(tool_name, "run_command" | "run_shell_command") {
        antigravity_command_exit_code(step, &output)
    } else {
        None
    };
    let is_system_error = step.get("type").and_then(Value::as_str) == Some("ERROR_MESSAGE");
    let status = if is_system_error {
        ToolStatus::Error
    } else {
        match step.get("status").and_then(Value::as_str) {
            Some("DONE") if exit_code.is_some_and(|code| code != 0) => ToolStatus::Error,
            Some("DONE") => ToolStatus::Success,
            Some("ERROR") => ToolStatus::Error,
            Some("RUNNING") => ToolStatus::Pending,
            Some("CANCELLED" | "CANCELED" | "INTERRUPTED") => ToolStatus::Interrupted,
            _ => ToolStatus::Unknown,
        }
    };
    let mut block = tool_result_block(call_id, status, &output, || full(pointer.into()));
    if let SessionBlock::ToolResult {
        exit_code: code, ..
    } = &mut block
    {
        *code = exit_code;
    }
    block
}

/// DONE describes the tool step, not the process's exit status. Only inspect
/// shell metadata before Output:, never arbitrary stdout or file contents.
fn antigravity_command_exit_code(step: &Value, output: &str) -> Option<i32> {
    if let Some(code) = step
        .get("exit_code")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok())
    {
        return Some(code);
    }
    if !output.trim_start().starts_with("Created At:") {
        return None;
    }
    for line in output.lines().take(12).map(str::trim) {
        if line == "Output:" || line == "Command Output:" {
            break;
        }
        if let Some(code) = line.strip_prefix("The command failed with exit code:") {
            return code.trim().parse().ok();
        }
        if line == "The command completed successfully." {
            return Some(0);
        }
    }
    None
}

fn extract_antigravity_content(value: &Value) -> String {
    match value.get("content") {
        Some(Value::String(text)) => text.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(map)) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_antigravity_timestamp_prefers_ts_over_created_at() {
        let value = serde_json::json!({
            "ts": 1771061953123i64,
            "created_at": "2026-07-10T13:22:44Z"
        });
        assert_eq!(timestamp(&value), Some(1_771_061_953_123));
    }

    #[test]
    fn parse_antigravity_timestamp_normalizes_seconds_and_iso_to_millis() {
        let val_sec = serde_json::json!({ "ts": 1771061953i64 });
        assert_eq!(timestamp(&val_sec), Some(1_771_061_953_000));

        let val_iso = serde_json::json!({ "created_at": "2026-07-10T13:22:44Z" });
        assert_eq!(timestamp(&val_iso), Some(1_783_689_764_000));
    }

    fn antigravity_message_fixture() -> (tempfile::TempDir, PathBuf, ()) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp
            .path()
            .join("brain/agy-reader/.system_generated/logs/transcript_full.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        (temp, path, ())
    }
    #[test]
    fn antigravity_structured_calls_preserve_order_and_results() {
        use crate::session_manager::model::ToolKind;
        let (_temp, path, _conn) = antigravity_message_fixture();
        let rows = [
            serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT",
                "content":"<USER_REQUEST>检查文件</USER_REQUEST><ADDITIONAL_METADATA>hidden"}),
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "thinking":"先检查状态", "content":"正在检查", "tool_calls":[
                    {"name":"run_command", "args":{"CommandLine":"\"git status\"",
                        "toolSummary":"\"Check git status\"", "toolAction":"\"Running command\""}},
                    {"name":"view_file", "args":{"AbsolutePath":"\"/tmp/package.json\"",
                        "StartLine":"1", "EndLine":"95"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND",
                "status":"DONE", "created_at":"2026-10-10T10:00:01Z",
                "content":"<USER_REQUEST>literal tool output</USER_REQUEST>"}),
            serde_json::json!({"step_index":3,"source":"MODEL","type":"GENERIC",
                "status":"ERROR","content":"permission denied"}),
            serde_json::json!({"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE",
                "content":"检查完成"}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_messages(&path).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].content, "检查文件");
        assert!(messages.iter().all(|m| m.turn_id.as_deref() == Some("t1")));
        assert_eq!(messages[1].id.as_deref(), Some("1"));
        let blocks = &messages[1].blocks;
        assert_eq!(blocks.len(), 4);
        assert!(matches!(&blocks[0], SessionBlock::Thinking { text, .. } if text == "先检查状态"));
        assert!(matches!(&blocks[1], SessionBlock::Text { text, .. } if text == "正在检查"));
        assert!(
            matches!(&blocks[2], SessionBlock::ToolCall { id, kind: ToolKind::Shell, title, detail, .. }
            if id == "agy:1:0" && title == "Check git status" && detail.as_deref() == Some("Running command"))
        );
        assert!(
            matches!(&blocks[3], SessionBlock::ToolCall { id, kind: ToolKind::Read, title, detail, .. }
            if id == "agy:1:1" && title == "/tmp/package.json" && detail.as_deref() == Some(":1-95"))
        );
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].ts, Some(1_791_626_401_000));
        assert!(
            matches!(&messages[2].blocks[0], SessionBlock::ToolResult { call_id, status: ToolStatus::Success, preview, .. }
            if call_id == "agy:1:0" && preview == "<USER_REQUEST>literal tool output</USER_REQUEST>")
        );
        assert!(
            matches!(&messages[3].blocks[0], SessionBlock::ToolResult { call_id, status: ToolStatus::Error, .. }
            if call_id == "agy:1:1")
        );
    }

    #[test]
    fn antigravity_tool_only_and_thinking_only_steps_survive_without_false_pairing() {
        let (_temp, path, _conn) = antigravity_message_fixture();
        let rows = [
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE",
                "thinking":"工具尚未完成"}),
            serde_json::json!({"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"停止"}),
            serde_json::json!({"step_index":4,"source":"MODEL","type":"GENERIC","status":"DONE","content":"unpaired"}),
            serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"view_file","args":{}}]}),
            serde_json::json!({"step_index":i64::MAX,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{}}]}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_messages(&path).unwrap();
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].meta.as_ref().unwrap().output_tokens, None);
        assert!(
            matches!(&messages[0].blocks[0], SessionBlock::ToolCall { id, .. } if id == "agy:1:0")
        );
        assert!(matches!(
            &messages[1].blocks[0],
            SessionBlock::Thinking { .. }
        ));
        assert!(messages
            .iter()
            .flat_map(|m| &m.blocks)
            .all(|b| !matches!(b, SessionBlock::ToolResult { .. })));
        assert_eq!(messages[2].turn_id.as_deref(), Some("t1"));
        assert!(
            matches!(&messages[4].blocks[0], SessionBlock::ToolCall { id, .. } if id == "agy:row-4:0")
        );
    }

    #[test]
    fn antigravity_reused_indices_keep_each_execution_and_unique_call_ids() {
        for new_turn in [true, false] {
            let (_temp, path, _conn) = antigravity_message_fixture();
            let planner = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]});
            let boundary = if new_turn {
                serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"再试一次"})
            } else {
                serde_json::json!({"step_index":0,"source":"SYSTEM","type":"CHECKPOINT"})
            };
            let rows = [
                planner.clone(),
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"original output"}),
                boundary,
                planner,
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"ERROR","content":"replayed output"}),
            ];
            std::fs::write(
                &path,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let messages = load_messages(&path).unwrap();
            let calls: Vec<_> = messages
                .iter()
                .flat_map(|m| &m.blocks)
                .filter_map(|b| {
                    if let SessionBlock::ToolCall { id, .. } = b {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            let results: Vec<_> = messages
                .iter()
                .filter_map(|m| {
                    if let SessionBlock::ToolResult {
                        call_id,
                        preview,
                        status,
                        ..
                    } = &m.blocks[0]
                    {
                        Some((call_id, preview.as_str(), *status, m.turn_id.as_deref()))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(calls.len(), 2);
            assert_ne!(calls[0], calls[1]);
            assert_eq!(results.len(), 2);
            assert_eq!(
                results[0],
                (calls[0], "original output", ToolStatus::Success, Some("t0"))
            );
            assert_eq!(
                results[1],
                (
                    calls[1],
                    "replayed output",
                    ToolStatus::Error,
                    Some(if new_turn { "t1" } else { "t0" })
                )
            );
        }
    }

    #[test]
    fn antigravity_pending_calls_do_not_cross_execution_boundaries() {
        let boundaries = [
            serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"new request"}),
            serde_json::json!({"step_index":0,"source":"SYSTEM","type":"CHECKPOINT"}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","content":"another planner"}),
        ];
        for boundary in boundaries {
            let (_temp, path, _conn) = antigravity_message_fixture();
            let rows = [
                serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                    "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]}),
                boundary,
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"unrelated result"}),
            ];
            std::fs::write(
                &path,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let messages = load_messages(&path).unwrap();
            assert!(matches!(
                &messages[0].blocks[0],
                SessionBlock::ToolCall { .. }
            ));
            assert!(messages
                .iter()
                .flat_map(|m| &m.blocks)
                .all(|b| !matches!(b, SessionBlock::ToolResult { .. })));
        }
    }

    #[test]
    fn antigravity_done_shell_failure_and_file_output_keep_distinct_statuses() {
        let (_temp, path, _conn) = antigravity_message_fixture();
        let failure = "Created At: 2026-10-11T00:00:00Z\nCompleted At: 2026-10-11T00:00:01Z\n    The command failed with exit code: 1\n    Output:\nerror details";
        let rows = [
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"false"}},
                    {"name":"view_file","args":{"AbsolutePath":"/tmp/log"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND","status":"DONE","content":failure}),
            serde_json::json!({"step_index":3,"source":"MODEL","type":"VIEW_FILE","status":"DONE","content":failure}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_messages(&path).unwrap();
        assert!(matches!(&messages[1].blocks[0], SessionBlock::ToolResult {
            status: ToolStatus::Error, exit_code: Some(1), call_id, .. } if call_id == "agy:1:0"));
        assert!(matches!(&messages[2].blocks[0], SessionBlock::ToolResult {
            status: ToolStatus::Success, exit_code: None, call_id, .. } if call_id == "agy:1:1"));
    }

    #[test]
    fn antigravity_shell_exit_codes_respect_metadata_and_lifecycle() {
        let failure = "Created At: now\nThe command failed with exit code: -1\nOutput:\ndetails";
        let cases = [
            ("DONE", "Created At: now\nThe command completed successfully.\nOutput:\nThe command failed with exit code: 2", ToolStatus::Success, Some(0)),
            ("DONE", "Created At: now\nOutput:\nThe command failed with exit code: 2", ToolStatus::Success, None),
            ("DONE", "The command failed with exit code: 2", ToolStatus::Success, None),
            ("DONE", failure, ToolStatus::Error, Some(-1)),
            ("RUNNING", failure, ToolStatus::Pending, Some(-1)),
            ("CANCELLED", failure, ToolStatus::Interrupted, Some(-1)),
        ];
        for (state, content, expected, code) in cases {
            let step = serde_json::json!({"type":"GENERIC","status":state,"content":content});
            assert!(
                matches!(antigravity_tool_result(&step, "c1", "run_command", &|_| None),
                SessionBlock::ToolResult { status, exit_code, .. } if status == expected && exit_code == code)
            );
        }
        let explicit = serde_json::json!({"type":"GENERIC","status":"DONE","exit_code":124,
            "content":"Created At: now\nThe command completed successfully.\nOutput:\n"});
        assert!(matches!(
            antigravity_tool_result(&explicit, "c2", "run_command", &|_| None),
            SessionBlock::ToolResult {
                status: ToolStatus::Error,
                exit_code: Some(124),
                ..
            }
        ));
        let error = serde_json::json!({"type":"GENERIC","status":"ERROR","exit_code":0});
        assert!(matches!(
            antigravity_tool_result(&error, "c3", "run_shell_command", &|_| None),
            SessionBlock::ToolResult {
                status: ToolStatus::Error,
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn antigravity_result_statuses_and_type_guards() {
        for (status, expected) in [
            ("DONE", ToolStatus::Success),
            ("ERROR", ToolStatus::Error),
            ("RUNNING", ToolStatus::Pending),
            ("CANCELLED", ToolStatus::Interrupted),
            ("INVALID", ToolStatus::Unknown),
        ] {
            let step = serde_json::json!({"source":"MODEL","type":"GENERIC","status":status,"content":"output"});
            assert!(is_antigravity_tool_result(&step, "run_command"));
            assert!(
                matches!(antigravity_tool_result(&step, "c1", "run_command", &|_| None),
                SessionBlock::ToolResult { status, .. } if status == expected)
            );
        }
        let error = serde_json::json!({"source":"SYSTEM","type":"ERROR_MESSAGE","status":"DONE","error":"invalid args"});
        assert!(is_antigravity_tool_result(&error, "view_file"));
        assert!(
            matches!(antigravity_tool_result(&error, "c2", "view_file", &|_| None),
            SessionBlock::ToolResult { status: ToolStatus::Error, preview, .. } if preview == "invalid args")
        );
        for (name, kind) in [
            ("write_to_file", "CODE_ACTION"),
            ("replace_file_content", "CODE_ACTION"),
            ("multi_replace_file_content", "CODE_ACTION"),
            ("list_dir", "LIST_DIRECTORY"),
        ] {
            assert!(is_antigravity_tool_result(
                &serde_json::json!({"source":"MODEL","type":kind}),
                name
            ));
        }
        for step in [
            serde_json::json!({"source":"USER_EXPLICIT","type":"USER_INPUT"}),
            serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE"}),
            serde_json::json!({"source":"MODEL","type":"SEARCH_WEB"}),
            serde_json::json!({"source":"SYSTEM","type":"EPHEMERAL_MESSAGE"}),
        ] {
            assert!(!is_antigravity_tool_result(&step, "run_command"));
        }
    }

    #[test]
    fn antigravity_reader_maps_args_without_decoding_text_twice() {
        let args = serde_json::json!({
            "CommandLine": "\"true\"", "toolSummary": "Run check",
            "ReplacementChunks": "[{\"TargetContent\":\"true\",\"ReplacementContent\":\"null\"}]",
            "Url": "\"https://example.com\"", "Overwrite": "true"
        });
        let normalized = antigravity_reader_args(&args);
        assert_eq!(normalized["command"], "true");
        assert_eq!(normalized["edits"][0]["old_string"], "true");
        assert_eq!(normalized["edits"][0]["new_string"], "null");
        assert_eq!(normalized["url"], "https://example.com");
        assert_eq!(normalized["Overwrite"], true);
        assert!(normalized.get("CommandLine").is_none());
        let conflict =
            antigravity_reader_args(&serde_json::json!({"command":"pwd", "CommandLine":"\"ls\""}));
        assert_eq!(conflict["command"], "pwd");
        assert_eq!(conflict["CommandLine"], "ls");
    }

    #[test]
    fn antigravity_reader_refs_serve_normalized_inputs_and_complete_diffs() {
        use crate::session_manager::content::{
            resolve_content_ref, SourceLocation, ValidatedSource,
        };
        let (temp, path, _conn) = antigravity_message_fixture();
        let long_old = "old line\n".repeat(100);
        let step = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
        "tool_calls":[
            {"name":"run_command", "args":{"CommandLine":"\"true\"", "toolSummary":"Run check"}},
            {"name":"multi_replace_file_content", "args":{"TargetFile":"\"/tmp/test\"", "ReplacementChunks":serde_json::to_string(&serde_json::json!([
                {"TargetContent":long_old,"ReplacementContent":"new\r\n"},
                {"TargetContent":"","ReplacementContent":"\n"}
            ])).unwrap()}},
            {"name":"write_to_file", "args":{"TargetFile":"\"/tmp/new\"", "CodeContent":"\"hello\\n\""}}
        ]});
        let original = format!("{step}\n");
        std::fs::write(&path, &original).unwrap();
        let source = ValidatedSource {
            provider_id: "agy".into(),
            raw: path.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: path.clone(),
                root: temp.path().to_path_buf(),
            },
        };
        let messages = load_messages(&path).unwrap();
        let SessionBlock::ToolCall {
            input_preview,
            input_full,
            title,
            ..
        } = &messages[0].blocks[0]
        else {
            panic!("call")
        };
        assert_eq!(title, "Run check");
        assert!(input_full.is_none());
        assert_eq!(
            serde_json::from_str::<Value>(input_preview).unwrap()["command"],
            "true"
        );
        let SessionBlock::ToolCall {
            input_full: Some(input),
            diff: Some(diff),
            ..
        } = &messages[0].blocks[1]
        else {
            panic!("long edit")
        };
        let normalized: Value =
            serde_json::from_str(&resolve_content_ref(&source, input).unwrap()).unwrap();
        assert_eq!(normalized["edits"][0]["old_string"], long_old);
        let body = resolve_content_ref(&source, diff.full.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            format!("@@\n{}+new\n@@\n+\n", "-old line\n".repeat(100))
        );
        let SessionBlock::ToolCall {
            diff: Some(write), ..
        } = &messages[0].blocks[2]
        else {
            panic!("write")
        };
        assert_eq!(
            resolve_content_ref(&source, write.full.as_ref().unwrap()).unwrap(),
            "@@\n+hello\n"
        );
        for suffix in ["999/input", "0/diff", "1/input/extra", "../input"] {
            let bad = ContentRef::Jsonl {
                offset: 0,
                len: original.len() as u32,
                pointer: format!("{ANTIGRAVITY_READER_REF_PREFIX}{suffix}"),
            };
            assert!(resolve_content_ref(&source, &bad).is_err());
        }
        let other = ValidatedSource {
            provider_id: "codex".into(),
            ..source
        };
        assert!(resolve_content_ref(&other, input).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn antigravity_diff_preserves_content_that_looks_like_file_headers() {
        let step = serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE", "tool_calls":[
            {"name":"replace_file_content", "args":{"TargetFile":"/tmp/file", "TargetContent":"\"-- a\"", "ReplacementContent":"\"++ b\""}}
        ]});
        let body = resolve_antigravity_reader_content(
            &step,
            &format!("{ANTIGRAVITY_READER_REF_PREFIX}0/diff"),
        )
        .unwrap();
        assert_eq!(body, "@@\n--- a\n@@\n+++ b\n");
    }

    #[test]
    fn antigravity_emoji_inputs_keep_full_params_accessible_to_existing_reader() {
        let command = "echo 🦀".to_string() + &"🦀".repeat(420);
        let step = serde_json::json!({"tool_calls":[{"name":"run_command", "args":{"CommandLine":serde_json::to_string(&command).unwrap()}}]});
        let blocks = antigravity_model_blocks(&step, "1", &|pointer| {
            Some(ContentRef::Jsonl {
                offset: 0,
                len: 1,
                pointer,
            })
        });
        let SessionBlock::ToolCall {
            input_preview,
            input_total_len,
            input_full,
            ..
        } = &blocks[0]
        else {
            panic!("call")
        };
        assert!(input_full.is_some());
        assert!(input_preview.encode_utf16().count() < *input_total_len as usize);
        assert_eq!(
            *input_total_len as usize,
            antigravity_reader_args(&step["tool_calls"][0]["args"])
                .to_string()
                .encode_utf16()
                .count()
        );
    }

    #[test]
    fn antigravity_reader_rejects_incomplete_diff_chunks_and_non_planner_refs() {
        let incomplete = serde_json::json!({"edits":[{"old_string":"a","new_string":"b"},{"old_string":"missing"}]});
        assert!(antigravity_diff_pairs(&incomplete, ToolKind::Edit).is_none());
        let truncated =
            antigravity_reader_args(&serde_json::json!({"ReplacementChunks":"[{<truncated>"}));
        assert!(antigravity_diff_pairs(&truncated, ToolKind::Edit).is_none());
        let result = serde_json::json!({"source":"MODEL","type":"GENERIC","tool_calls":[{"name":"run_command","args":{}}]});
        assert!(resolve_antigravity_reader_content(
            &result,
            &format!("{ANTIGRAVITY_READER_REF_PREFIX}0/input")
        )
        .is_err());
    }

    #[test]
    fn antigravity_long_content_refs_resolve_physical_jsonl_lines() {
        use crate::session_manager::content::{
            resolve_content_ref, SourceLocation, ValidatedSource,
        };
        let (temp, path, _conn) = antigravity_message_fixture();
        let thought = "思考🦀".repeat(200);
        let output = (0..30)
            .map(|i| format!("第{i}行输出"))
            .collect::<Vec<_>>()
            .join("\n");
        let args = serde_json::json!({"CommandLine": format!("\"{}\"", "echo hello ".repeat(60))});
        let planner = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
            "thinking":thought,"tool_calls":[{"name":"run_command","args":args}]});
        let execution = serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND",
            "status":"DONE","content":[{"text":output}]});
        // Invalid/blank lines, CRLF, indentation and no trailing newline all affect byte ranges.
        std::fs::write(&path, format!("invalid\r\n\r\n  {planner}\r\n{execution}")).unwrap();
        let messages = load_messages(&path).unwrap();
        let source = ValidatedSource {
            provider_id: "agy".into(),
            raw: path.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: path.clone(),
                root: temp.path().to_path_buf(),
            },
        };
        let SessionBlock::Thinking {
            full: Some(thinking_ref),
            ..
        } = &messages[0].blocks[0]
        else {
            panic!("missing thinking ref")
        };
        assert_eq!(resolve_content_ref(&source, thinking_ref).unwrap(), thought);
        let SessionBlock::ToolCall {
            input_full: Some(args_ref),
            ..
        } = &messages[0].blocks[1]
        else {
            panic!("missing args ref")
        };
        assert_eq!(
            serde_json::from_str::<Value>(&resolve_content_ref(&source, args_ref).unwrap())
                .unwrap(),
            antigravity_reader_args(&args)
        );
        let SessionBlock::ToolResult {
            full: Some(result_ref),
            truncated: true,
            total_len,
            ..
        } = &messages[1].blocks[0]
        else {
            panic!("missing result ref")
        };
        assert_eq!(*total_len, output.chars().count() as u32);
        assert_eq!(resolve_content_ref(&source, result_ref).unwrap(), output);
    }

    #[test]
    fn agy_binary_only_sessions_keep_index_metadata_without_fabricated_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project space");
        let workspace_uris =
            serde_json::to_string(&[url::Url::from_file_path(&project).unwrap().to_string()])
                .unwrap();
        let db = rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        db.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT);").unwrap();
        db.execute(
            "INSERT INTO conversation_summaries VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                "abc-123",
                "Test title",
                workspace_uris,
                "Preview",
                "2026-10-13T17:26:12Z"
            ],
        )
        .unwrap();
        let sessions = scan_root(&dir.path().join("brain"), None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Test title"));
        assert_eq!(
            sessions[0].project_dir.as_deref(),
            Some(project.to_string_lossy().as_ref())
        );
        assert!(sessions[0].source_path.is_none());
        assert!(sessions[0].last_active_at.is_some());
    }

    #[test]
    fn agy_scans_cli_and_ide_roots_deduplicating_and_tolerating_missing() {
        let home = tempfile::tempdir().unwrap();
        let write = |root: &str, id: &str| {
            let logs = home
                .path()
                .join(root)
                .join("brain")
                .join(id)
                .join(".system_generated/logs");
            fs::create_dir_all(&logs).unwrap();
            fs::write(
                logs.join("transcript_full.jsonl"),
                r#"{"step_index":0,"type":"USER_INPUT","content":"hi"}"#,
            )
            .unwrap();
        };
        write("antigravity-cli", "cli-1");
        write("antigravity", "ide-1");
        write("antigravity", "cli-1");
        let roots = vec![
            (home.path().join("antigravity-cli/brain"), None),
            (home.path().join("antigravity/brain"), None),
            (home.path().join("missing/brain"), None),
        ];
        let mut ids: Vec<_> = scan_roots(&roots)
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["cli-1", "ide-1"]);
    }

    #[test]
    #[cfg(unix)]
    fn agy_discovery_does_not_follow_transcripts_outside_its_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let session = root.path().join("abc-123");
        fs::create_dir_all(&session).unwrap();
        let logs = outside.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"type":"USER_INPUT","content":"outside"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), session.join(".system_generated")).unwrap();
        assert!(scan_root(root.path(), None).is_empty());
    }

    #[test]
    fn agy_shows_only_the_typed_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("abc-123/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let file = logs.join("transcript_full.jsonl");
        fs::write(&file, concat!(
            "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"<USER_REQUEST>\\nfix the build\\n</USER_REQUEST>\\n<ADDITIONAL_METADATA>\\nThe current local time is: 2026-10-21T08:40:35+08:00.\\n</ADDITIONAL_METADATA>\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"<USER_REQUEST> stays in replies\"}\n",
            "{\"step_index\":2,\"type\":\"USER_INPUT\",\"content\":\"plain prompt\"}\n",
        )).unwrap();
        assert_eq!(
            scan_root(dir.path(), None)[0].title.as_deref(),
            Some("fix the build")
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(messages[0].content, "fix the build");
        assert_eq!(messages[1].content, "<USER_REQUEST> stays in replies");
        assert_eq!(messages[2].content, "plain prompt");
    }

    #[test]
    fn agy_reads_tokens_rewritten_steps_and_partial_tail() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("abc-123/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let file = logs.join("transcript_full.jsonl");
        fs::write(&file, concat!(
            "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"hello\",\"created_at\":\"2026-10-13T17:26:12Z\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"draft\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"done\",\"input_tokens\":100,\"output_tokens\":12,\"cache_read_tokens\":20}\n{"
        )).unwrap();
        let sessions = scan_root(dir.path(), None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some("agy --conversation=abc-123")
        );
        let private = Path::new("/home/me/.cc-switch/agy/relay");
        assert_eq!(
            scan_root(dir.path(), Some(private))[0]
                .resume_command
                .as_deref(),
            Some("agy --gemini_dir=/home/me/.cc-switch/agy/relay --conversation=abc-123")
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "done");
        assert_eq!(messages[1].meta.as_ref().unwrap().input_tokens, Some(100));
        assert_eq!(
            messages[1].meta.as_ref().unwrap().cache_read_tokens,
            Some(20)
        );
    }
}
