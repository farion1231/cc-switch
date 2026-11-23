//! Read-only DeepSeek Harness session browser; native Web UI owns resume/delete.
use super::{blocks, utils};
use crate::session_manager::{
    model::{ContentRef, MessageMeta, SessionBlock, ToolStatus},
    SessionMessage, SessionMeta,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 128 * 1024 * 1024;

pub fn session_roots() -> Vec<PathBuf> {
    vec![crate::dsh_config::data_dir().join("sessions")]
}

fn generation(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix("session.v")?
        .strip_suffix(".jsonl.zstd")
        .or_else(|| name.strip_prefix("session.v")?.strip_suffix(".jsonl"))?
        .parse()
        .ok()
}

/// Decompress complete independent frames; a partially appended final frame waits for the next scan.
pub(crate) fn read_session(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_BYTES {
        return Err("DeepSeek Harness session is too large".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if path.extension().is_none_or(|ext| ext != "zstd") {
        return Ok(bytes);
    }
    let mut output = vec![];
    let mut offset = 0;
    while offset < bytes.len() {
        let frame = &bytes[offset..];
        let size = match zstd::zstd_safe::find_frame_compressed_size(frame) {
            Ok(size) => size,
            Err(_) if frame.len() < 4 || frame.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) => break,
            Err(_) => return Err("Invalid DeepSeek Harness zstd session".into()),
        };
        let decoder =
            zstd::stream::read::Decoder::new(&frame[..size]).map_err(|error| error.to_string())?;
        decoder
            .take(MAX_BYTES + 1 - output.len() as u64)
            .read_to_end(&mut output)
            .map_err(|error| error.to_string())?;
        if output.len() as u64 > MAX_BYTES {
            return Err("DeepSeek Harness session is too large".into());
        }
        offset += size;
    }
    Ok(output)
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut result = vec![];
    // Native layout is <project>/<session>/session.vN.jsonl[.zstd]. Do not follow directory links.
    for project in fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .take(100_000)
    {
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        for session in fs::read_dir(project.path())
            .into_iter()
            .flatten()
            .flatten()
            .take(100_000)
        {
            if !session.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let selected = fs::read_dir(session.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|file| file.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|file| file.path())
                .filter_map(|path| generation(&path).map(|version| (version, path)))
                .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            if let Some((_, path)) = selected {
                result.push(path);
            }
        }
    }
    result
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan(&session_roots()[0])
}

fn scan(root: &Path) -> Vec<SessionMeta> {
    files(root)
        .into_iter()
        .filter_map(|path| {
            let data = read_session(&path).ok()?;
            let mut lines = data.split(|byte| *byte == b'\n');
            let header: Value = serde_json::from_slice(lines.next()?).ok()?;
            if !matches!(header["type"].as_str(), Some("session" | "header")) {
                return None;
            }
            let id = header["id"]
                .as_str()
                .filter(|id| !id.is_empty())?
                .to_string();
            let messages = parse_messages(&data).ok()?;
            let title = messages
                .iter()
                .find(|message| message.role == "user" && !message.injected)
                .map(|message| utils::truncate_summary(&message.content, utils::TITLE_MAX_CHARS));
            Some(SessionMeta {
                provider_id: "dsh".into(),
                session_id: id,
                title,
                summary: None,
                project_dir: header["cwd"].as_str().map(str::to_string),
                created_at: header["createdAt"].as_i64(),
                last_active_at: messages.iter().filter_map(|message| message.ts).max(),
                source_path: Some(path.to_string_lossy().into_owned()),
                resume_command: None,
            })
        })
        .collect()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    parse_messages(&read_session(path)?)
}

fn parse_messages(data: &[u8]) -> Result<Vec<SessionMessage>, String> {
    let mut messages = vec![];
    let mut seen = HashSet::new();
    let mut offset = 0;
    for line in data.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset += line.len();
        // An incomplete JSONL tail is still being written.
        if !line.ends_with(b"\n") {
            break;
        }
        let record: Value =
            serde_json::from_slice(line).map_err(|_| "Invalid DeepSeek Harness session JSONL")?;
        // Replacement copies only alter the model-visible surface. The human
        // transcript retains append-origin messages and full tool outputs.
        if record["surfaceOp"]["op"] == "replace" {
            continue;
        }
        let kind = record["type"].as_str().unwrap_or_default();
        let (raw, pointer, role) = match kind {
            "user/message" => (&record["data"], "/data", "user"),
            "assistant/message" => (&record["data"]["message"], "/data/message", "assistant"),
            "tool/result" => (&record["data"]["message"], "/data/message", "tool"),
            _ => continue,
        };
        if let Some(id) = raw["id"].as_str() {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        let mut message_blocks = vec![];
        let reference = |pointer: String| {
            Some(ContentRef::Jsonl {
                offset: start as u64,
                len: line.len() as u32,
                pointer,
            })
        };
        if role == "tool" {
            let text = utils::extract_text(&raw["content"]);
            message_blocks.push(blocks::tool_result_block(
                raw["toolCallId"].as_str().unwrap_or_default(),
                if raw["isError"] == true {
                    ToolStatus::Error
                } else {
                    ToolStatus::Success
                },
                &text,
                || reference(format!("{pointer}/content")),
            ));
        } else {
            for (index, part) in raw["content"].as_array().into_iter().flatten().enumerate() {
                match part["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = part["text"].as_str() {
                            message_blocks.push(SessionBlock::text(text));
                        }
                    }
                    Some("thinking" | "reasoning") => {
                        let field = if part.get("text").is_some() {
                            "text"
                        } else {
                            "thinking"
                        };
                        message_blocks.push(blocks::thinking_block(
                            part[field].as_str().unwrap_or_default(),
                            None,
                            None,
                            || reference(format!("{pointer}/content/{index}/{field}")),
                        ));
                    }
                    Some("tool-call") => {
                        let arguments = blocks::parse_arguments(&part["arguments"]);
                        message_blocks.push(blocks::tool_call_block(
                            blocks::ToolSource::Generic,
                            part["id"].as_str().unwrap_or_default(),
                            part["name"].as_str().unwrap_or_default(),
                            &arguments,
                            || reference(format!("{pointer}/content/{index}/arguments")),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if message_blocks.is_empty() {
            continue;
        }
        let mut message =
            SessionMessage::from_blocks(role, record["time"].as_i64(), message_blocks);
        message.injected = role == "user"
            && raw["source"]["kind"]
                .as_str()
                .is_some_and(|kind| kind != "user");
        let usage = &record["data"]["usage"];
        if role == "assistant" {
            message.meta = Some(MessageMeta {
                model: raw["source"]["model"].as_str().map(str::to_string),
                input_tokens: usage["inputTokens"].as_u64(),
                output_tokens: usage["outputTokens"].as_u64(),
                cache_read_tokens: usage["cacheReadTokens"].as_u64(),
                cache_write_tokens: usage["cacheWriteTokens"].as_u64(),
                ..Default::default()
            });
        }
        messages.push(message);
    }
    blocks::assign_turn_ids(&mut messages);
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Vec<u8> {
        [json!({"type":"session","version":4,"id":"test","cwd":"/project","createdAt":1000}),
         json!({"type":"user/message","seq":1,"surfaceOp":"append","time":1001,"data":{"id":"u","role":"user","source":{"kind":"user"},"content":[{"type":"text","text":"Fix this"}]}}),
         json!({"type":"assistant/message","seq":2,"surfaceOp":"append","time":2000,"data":{"message":{"id":"a","source":{"model":"route:model"},"content":[{"type":"text","text":"Fixed"},{"type":"tool-call","id":"call","name":"read","arguments":"{\"path\":\"test.rs\"}"}]},"usage":{"inputTokens":10,"outputTokens":20,"cacheReadTokens":30}}}),
         json!({"type":"tool/result","seq":3,"surfaceOp":"append","time":2001,"data":{"message":{"id":"result","role":"tool","toolCallId":"call","isError":false,"content":[{"type":"text","text":"contents"}]}}})]
            .iter().map(|line| format!("{line}\n")).collect::<String>().into_bytes()
    }

    #[test]
    fn browser_reads_concatenated_frames_and_ignores_incomplete_tail() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project/test");
        fs::create_dir_all(&root).unwrap();
        let bytes = fixture();
        let boundary = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        let mut compressed = zstd::encode_all(&bytes[..boundary], 1).unwrap();
        compressed.extend(zstd::encode_all(&bytes[boundary..], 1).unwrap());
        let tail = zstd::encode_all(b"{\"pending\":true}\n".as_slice(), 1).unwrap();
        compressed.extend(&tail[..tail.len() / 2]);
        let path = root.join("session.v4.jsonl.zstd");
        fs::write(&path, compressed).unwrap();
        assert_eq!(read_session(&path).unwrap(), bytes);
        let sessions = scan(temp.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Fix this"));
        let messages = load_messages(&path).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[1].meta.as_ref().unwrap().cache_read_tokens,
            Some(30)
        );
        assert!(matches!(
            messages[1].blocks[1],
            SessionBlock::ToolCall { .. }
        ));
        assert!(matches!(
            messages[2].blocks[0],
            SessionBlock::ToolResult { ref call_id, status: ToolStatus::Success, ref preview, .. }
                if call_id == "call" && preview == "contents"
        ));
    }

    #[test]
    fn duplicate_messages_and_model_only_summaries_are_not_shown() {
        let mut bytes = fixture();
        let original = parse_messages(&bytes).unwrap();
        bytes.extend(format!("{}\n",json!({"type":"assistant/message","seq":4,"data":{"message":{"id":"a","content":[{"type":"text","text":"Duplicate"}]}}})).as_bytes());
        bytes.extend(format!("{}\n",json!({"type":"user/message","seq":5,"surfaceOp":{"op":"replace","startSeq":1,"endSeq":3},"data":{"id":"summary","content":[{"type":"text","text":"Summary"}]}})).as_bytes());
        let parsed = parse_messages(&bytes).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed
                .iter()
                .map(|message| &message.content)
                .collect::<Vec<_>>(),
            original
                .iter()
                .map(|message| &message.content)
                .collect::<Vec<_>>()
        );
        bytes.extend(b"{partial");
        assert_eq!(parse_messages(&bytes).unwrap().len(), 3);
    }

    #[test]
    fn native_tool_results_preserve_errors_empty_results_and_full_content() {
        let output = "failed output\n".repeat(20);
        for content in [json!([]), json!([{"type":"text","text":output}])] {
            let mut bytes = fixture();
            bytes.extend(format!("{}\n", json!({"type":"tool/result","seq":4,"data":{"message":{"id":"failure","role":"tool","toolCallId":"failed-call","isError":true,"content":content}}})).as_bytes());
            let messages = parse_messages(&bytes).unwrap();
            assert_eq!(messages.len(), 4);
            assert_eq!(messages[3].blocks.len(), 1);
            let SessionBlock::ToolResult {
                call_id,
                status: ToolStatus::Error,
                total_len,
                full,
                ..
            } = &messages[3].blocks[0]
            else {
                panic!("expected a native tool error result");
            };
            assert_eq!(call_id, "failed-call");
            let expected = utils::extract_text(&content);
            assert_eq!(*total_len as usize, expected.chars().count());
            if !expected.is_empty() {
                let ContentRef::Jsonl {
                    offset,
                    len,
                    pointer,
                } = full.as_ref().unwrap()
                else {
                    panic!("expected a JSONL content reference");
                };
                let record: Value = serde_json::from_slice(
                    &bytes[*offset as usize..*offset as usize + *len as usize],
                )
                .unwrap();
                assert_eq!(pointer, "/data/message/content");
                assert_eq!(
                    utils::extract_text(record.pointer(pointer).unwrap()),
                    expected
                );
            }
        }
    }

    #[test]
    fn surface_replacements_preserve_original_transcript_and_do_not_claim_message_ids() {
        let mut bytes = fixture();
        let original = parse_messages(&bytes).unwrap();
        let mut expected: Vec<_> = original
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        expected.extend(["Keep this", "After compaction"]);
        for record in [
            json!({"type":"user/message","seq":4,"surfaceOp":"append","data":{"id":"later","content":[{"type":"text","text":"Keep this"}]}}),
            json!({"type":"user/message","seq":5,"surfaceOp":{"op":"replace","startSeq":1,"endSeq":2},"data":{"id":"summary","content":[{"type":"text","text":"Summary"}]}}),
            json!({"type":"user/message","seq":6,"surfaceOp":{"op":"replace","startSeq":5,"endSeq":3},"data":{"id":"summary-2","content":[{"type":"text","text":"New summary"}]}}),
            json!({"type":"tool/result","seq":7,"surfaceOp":{"op":"replace","startSeq":3,"endSeq":3},"data":{"message":{"id":"result","toolCallId":"call","content":[{"type":"text","text":"Trimmed output"}]}}}),
            json!({"type":"assistant/message","seq":8,"surfaceOp":{"op":"replace","startSeq":2,"endSeq":2},"data":{"message":{"id":"after","content":[{"type":"text","text":"Model-only copy"}]}}}),
            json!({"type":"assistant/message","seq":9,"surfaceOp":"append","data":{"message":{"id":"after","content":[{"type":"text","text":"After compaction"}]}}}),
        ] {
            bytes.extend(format!("{record}\n").as_bytes());
        }
        let messages = parse_messages(&bytes).unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(matches!(
            messages[2].blocks[0],
            SessionBlock::ToolResult { ref call_id, ref preview, .. }
                if call_id == "call" && preview == "contents"
        ));
    }
}
