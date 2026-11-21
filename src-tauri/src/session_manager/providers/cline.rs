//! Read Cline CLI sessions: index from `sessions.db`, history from `<id>.messages.json`.
use super::{blocks, utils};
use crate::cline_session::{self, strip_user_input, valid_session_id, ClineSession};
use crate::session_manager::model::{ContentRef, MessageMeta, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Env vars that move Cline's data; a resumed CLI must see the same values.
const LOCATION_ENV: &[&str] = &[
    "CLINE_DIR",
    "CLINE_DATA_DIR",
    "CLINE_DB_DATA_DIR",
    "CLINE_SESSION_DATA_DIR",
];

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![crate::cline_config::sessions_dir()]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    to_metas(cline_session::sessions())
}

fn to_metas(sessions: Vec<ClineSession>) -> Vec<SessionMeta> {
    sessions
        .into_iter()
        .filter(|session| !session.is_subagent && session.messages_path.is_file())
        .map(|session| {
            let prompt = session
                .prompt
                .as_deref()
                .map(strip_user_input)
                .filter(|text| !text.is_empty());
            SessionMeta {
                provider_id: "cline".into(),
                title: session
                    .title
                    .clone()
                    .or_else(|| prompt.map(|text| utils::truncate_summary(text, 80))),
                summary: prompt.map(|text| utils::truncate_summary(text, 200)),
                project_dir: session.cwd.clone(),
                created_at: session.started_at,
                last_active_at: session.updated_at.or(session.started_at),
                source_path: Some(session.messages_path.to_string_lossy().into_owned()),
                resume_command: resume_command(&session.id),
                session_id: session.id,
            }
        })
        .collect()
}

fn resume_command(id: &str) -> Option<String> {
    if !valid_session_id(id) {
        return None;
    }
    let env: Vec<(&str, String)> = LOCATION_ENV
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|value| (*key, value))
        })
        .collect();
    #[cfg(windows)]
    let command = env
        .iter()
        .map(|(key, value)| format!("$env:{key} = '{}'; ", value.replace('\'', "''")))
        .collect::<String>()
        + &format!("cline --id {id}");
    #[cfg(not(windows))]
    let command = if env.is_empty() {
        format!("cline --id {id}")
    } else {
        let assignments = env
            .iter()
            .map(|(key, value)| {
                format!(
                    "{key}={}",
                    crate::session_manager::terminal::shell_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("env {assignments} cline --id {id}")
    };
    Some(command)
}

/// `rel_path` for [`ContentRef::File`] is relative to the sessions root.
fn relative_to_sessions_root(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read session: {e}"))?;
    let document: Value =
        serde_json::from_slice(&data).map_err(|_| "Failed to parse session JSON".to_string())?;
    let raw_messages = document
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;
    let rel_path = relative_to_sessions_root(path);
    let file_ref = |pointer: String| -> Option<ContentRef> {
        Some(ContentRef::File {
            rel_path: rel_path.clone(),
            pointer,
        })
    };

    let mut messages = Vec::new();
    for (index, raw) in raw_messages.iter().enumerate() {
        let Some(role @ ("user" | "assistant")) = raw["role"].as_str() else {
            continue;
        };
        let base = format!("/messages/{index}/content");
        let mut texts = Vec::new();
        let mut message_blocks = Vec::new();
        match &raw["content"] {
            Value::String(text) => texts.push(text.as_str()),
            Value::Array(parts) => {
                for (i, part) in parts.iter().enumerate() {
                    match part["type"].as_str() {
                        Some("text") => {
                            if let Some(text) = part["text"].as_str() {
                                texts.push(text);
                            }
                        }
                        Some("thinking") => message_blocks.push(blocks::thinking_block(
                            part["thinking"].as_str().unwrap_or_default(),
                            None,
                            None,
                            || file_ref(format!("{base}/{i}/thinking")),
                        )),
                        Some("tool_use") => message_blocks.push(blocks::tool_call_block(
                            blocks::ToolSource::Generic,
                            part["id"].as_str().unwrap_or_default(),
                            part["name"].as_str().unwrap_or_default(),
                            &blocks::parse_arguments(&part["input"]),
                            || file_ref(format!("{base}/{i}/input")),
                        )),
                        Some("tool_result") => {
                            let status = if part["is_error"].as_bool() == Some(true) {
                                ToolStatus::Error
                            } else {
                                ToolStatus::Success
                            };
                            message_blocks.push(blocks::tool_result_block(
                                part["tool_use_id"].as_str().unwrap_or_default(),
                                status,
                                &utils::extract_text(&part["content"]),
                                || file_ref(format!("{base}/{i}/content")),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let only_results = role == "user" && texts.is_empty() && !message_blocks.is_empty();
        for text in texts {
            let text = if role == "user" {
                strip_user_input(text)
            } else {
                text.trim()
            };
            if !text.is_empty() {
                message_blocks.push(SessionBlock::text(text));
            }
        }
        if message_blocks.is_empty() {
            continue;
        }
        let ts = raw.get("ts").and_then(cline_session::timestamp_ms);
        let mut message = SessionMessage::from_blocks(
            if only_results { "tool" } else { role },
            ts,
            message_blocks,
        );
        message.id = raw["id"].as_str().map(str::to_owned);
        if role == "assistant" {
            message.meta = assistant_meta(raw);
        }
        messages.push(message);
    }
    blocks::assign_turn_ids(&mut messages);
    Ok(messages)
}

/// Cline's `inputTokens` includes cache reads and writes; show fresh input.
fn assistant_meta(raw: &Value) -> Option<MessageMeta> {
    let metrics = raw.get("metrics").filter(|m| m.is_object());
    let model = raw
        .pointer("/modelInfo/id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned);
    if metrics.is_none() && model.is_none() {
        return None;
    }
    let field = |key: &str| metrics.and_then(|m| m[key].as_u64());
    let cache_read = field("cacheReadTokens");
    let cache_write = field("cacheWriteTokens");
    Some(MessageMeta {
        model,
        input_tokens: field("inputTokens").map(|input| {
            input
                .saturating_sub(cache_read.unwrap_or(0))
                .saturating_sub(cache_write.unwrap_or(0))
        }),
        output_tokens: field("outputTokens"),
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        cost_usd: metrics
            .and_then(|m| m["cost"].as_f64())
            .filter(|cost| cost.is_finite() && *cost > 0.0),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cline_session::tests::write_fixture;
    use serde_json::json;

    fn conversation() -> Value {
        json!([
            {"id":"u1","role":"user","content":[{"type":"text","text":"<user_input mode=\"act\">Call the echo tool</user_input>"}],"ts":1792687453901i64},
            {"id":"msg_a","role":"assistant","content":[
                {"type":"thinking","thinking":"Let me call it."},
                {"type":"tool_use","id":"call_1","name":"ccs-stdio__ccs_echo","input":{"t":"acceptance"}}
             ],"ts":1792687455708i64,"modelInfo":{"id":"deepseek-v4-flash","provider":"ccs-relay-chat"},
             "metrics":{"inputTokens":5691,"outputTokens":53,"cacheReadTokens":5376,"cacheWriteTokens":0}},
            {"id":"msg_r","role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"ECHO:acceptance"}],"ts":1792687455715i64},
            {"id":"msg_b","role":"assistant","content":[{"type":"text","text":"ECHO:acceptance"}],"ts":1792687457019i64,
             "modelInfo":{"id":"claude-sonnet-4-5","provider":"anthropic"},
             "metrics":{"inputTokens":130,"outputTokens":7,"cacheReadTokens":20,"cacheWriteTokens":10,"cost":0.0004485}}
        ])
    }

    #[test]
    fn cline_sessions_list_cli_roots_and_read_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db/sessions.db");
        let sessions_dir = dir.path().join("sessions");
        write_fixture(
            &db,
            &sessions_dir,
            &[
                ("s_cli", "cli", false, conversation()),
                ("s_vscode", "vscode", false, conversation()),
                ("s_sub", "cli", true, conversation()),
            ],
        );
        let metas = to_metas(cline_session::read_index(&db, &sessions_dir));
        assert_eq!(metas.len(), 1);
        let meta = &metas[0];
        assert_eq!(meta.session_id, "s_cli");
        assert_eq!(meta.title.as_deref(), Some("Fixture title"));
        assert_eq!(meta.summary.as_deref(), Some("Reply with one word please."));
        assert_eq!(meta.project_dir.as_deref(), Some("/work/project"));
        assert!(meta
            .resume_command
            .as_deref()
            .unwrap()
            .ends_with("cline --id s_cli"));

        let messages = load_messages(Path::new(meta.source_path.as_deref().unwrap())).unwrap();
        assert_eq!(
            messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            vec!["user", "assistant", "tool", "assistant"]
        );
        assert_eq!(messages[0].content, "Call the echo tool");
        assert!(matches!(
            messages[1].blocks[..],
            [SessionBlock::Thinking { .. }, SessionBlock::ToolCall { .. }]
        ));
        let first = messages[1].meta.as_ref().unwrap();
        assert_eq!(first.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(
            (first.input_tokens, first.cache_read_tokens, first.cost_usd),
            (Some(315), Some(5376), None)
        );
        let second = messages[3].meta.as_ref().unwrap();
        assert_eq!(
            (second.input_tokens, second.cache_write_tokens),
            (Some(100), Some(10))
        );
        assert_eq!(second.cost_usd, Some(0.0004485));
        assert_eq!(messages[3].id.as_deref(), Some("msg_b"));
    }

    #[test]
    fn cline_resume_command_rejects_unsafe_ids() {
        assert_eq!(resume_command("../x"), None);
        assert_eq!(resume_command("a b"), None);
        assert_eq!(
            relative_to_sessions_root(Path::new("/root/sessions/s1/s1.messages.json")),
            "s1/s1.messages.json"
        );
    }
}
