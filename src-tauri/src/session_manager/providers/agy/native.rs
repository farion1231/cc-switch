//! Native steps are full Protobuf messages, not UTF-8 transcript exports.
//! Decode only verified fields; do not interpret context or opaque bytes as chat text.
use super::*;
use prost::Message;
use rusqlite::OptionalExtension;
use serde_json::json;

#[derive(Clone, PartialEq, Message)]
struct Stamp {
    #[prost(int64, tag = "1")]
    seconds: i64,
    #[prost(int32, tag = "2")]
    nanos: i32,
}
#[derive(Clone, PartialEq, Message)]
struct Metadata {
    #[prost(message, optional, tag = "1")]
    created: Option<Stamp>,
}
#[derive(Clone, PartialEq, Message)]
struct User {
    #[prost(string, tag = "2")]
    text: String,
    #[prost(message, repeated, tag = "3")]
    parts: Vec<Text>,
}
#[derive(Clone, PartialEq, Message)]
struct Call {
    #[prost(string, tag = "2")]
    name: String,
    #[prost(string, tag = "3")]
    args: String,
}
#[derive(Clone, PartialEq, Message)]
struct Planner {
    #[prost(string, tag = "1")]
    text: String,
    #[prost(string, tag = "3")]
    thinking: String,
    #[prost(message, repeated, tag = "7")]
    calls: Vec<Call>,
}
#[derive(Clone, PartialEq, Message)]
struct Text {
    #[prost(string, tag = "1")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct FileView {
    #[prost(string, tag = "4")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct Command {
    #[prost(message, optional, tag = "21")]
    output: Option<Text>,
}
#[derive(Clone, PartialEq, Message)]
struct Generic {
    #[prost(message, optional, tag = "2")]
    output: Option<Text>,
}
#[derive(Clone, PartialEq, Message)]
struct Error {
    #[prost(message, optional, tag = "3")]
    details: Option<ErrorDetails>,
}
#[derive(Clone, PartialEq, Message)]
struct ErrorDetails {
    #[prost(string, tag = "9")]
    text: String,
    #[prost(string, tag = "2")]
    fallback: String,
}
#[derive(Clone, PartialEq, Message)]
struct CodeAction {
    #[prost(string, tag = "26")]
    summary: String,
}
#[derive(Clone, PartialEq, Message)]
struct Search {
    #[prost(string, tag = "3")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct Directory {
    #[prost(message, repeated, tag = "3")]
    entries: Vec<Text>,
}
#[derive(Clone, PartialEq, Message)]
struct WebSearch {
    #[prost(string, tag = "5")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct CommandStatus {
    #[prost(string, tag = "3")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct UrlContent {
    #[prost(message, optional, tag = "2")]
    page: Option<Page>,
}
#[derive(Clone, PartialEq, Message)]
struct Page {
    #[prost(message, optional, tag = "6")]
    body: Option<PageBody>,
}
#[derive(Clone, PartialEq, Message)]
struct PageBody {
    #[prost(message, optional, tag = "3")]
    content: Option<UrlText>,
}
#[derive(Clone, PartialEq, Message)]
struct UrlText {
    #[prost(string, tag = "2")]
    text: String,
}
#[derive(Clone, PartialEq, Message)]
struct Step {
    #[prost(message, optional, tag = "31")]
    command_status: Option<CommandStatus>,
    #[prost(message, optional, tag = "40")]
    url: Option<UrlContent>,
    #[prost(message, optional, tag = "10")]
    code: Option<CodeAction>,
    #[prost(message, optional, tag = "13")]
    search: Option<Search>,
    #[prost(message, optional, tag = "15")]
    directory: Option<Directory>,
    #[prost(message, optional, tag = "42")]
    web: Option<WebSearch>,
    #[prost(message, optional, tag = "5")]
    metadata: Option<Metadata>,
    #[prost(message, optional, tag = "19")]
    user: Option<User>,
    #[prost(message, optional, tag = "20")]
    planner: Option<Planner>,
    #[prost(message, optional, tag = "14")]
    file: Option<FileView>,
    #[prost(message, optional, tag = "28")]
    command: Option<Command>,
    #[prost(message, optional, tag = "140")]
    generic: Option<Generic>,
    #[prost(message, optional, tag = "24")]
    error: Option<Error>,
    #[prost(message, optional, tag = "103")]
    ephemeral: Option<Text>,
    #[prost(message, optional, tag = "114")]
    system: Option<Text>,
}

fn decode(index: i64, kind: i64, status: i64, format: i64, bytes: &[u8]) -> Result<Value, String> {
    if format != 0 {
        return Err(format!(
            "Unsupported Antigravity step format {format} at step {index}"
        ));
    }
    let step =
        Step::decode(bytes).map_err(|e| format!("Cannot decode Antigravity step {index}: {e}"))?;
    let mut value = json!({"step_index":index,"source":"MODEL","type":"GENERIC",
        "status":match status { 2=>"RUNNING",3=>"DONE",4=>"ERROR",5=>"CANCELLED",_=>"UNKNOWN" }});
    // Internal history/checkpoint steps must not become assistant responses or tool results.
    if !matches!(
        kind,
        5 | 7 | 8 | 9 | 14 | 15 | 17 | 21 | 28 | 31 | 33 | 85 | 127 | 132 | 138
    ) {
        value["source"] = json!("SYSTEM");
        value["type"] = json!(format!("NATIVE_STEP_{kind}"));
    }
    if let Some(time) = step.metadata.and_then(|m| m.created) {
        if let Some(time) = chrono::DateTime::from_timestamp(time.seconds, time.nanos as u32) {
            value["created_at"] = json!(time.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
        }
    }
    let content = match kind {
        14 => {
            value["type"] = json!("USER_INPUT");
            value["source"] = json!("USER");
            step.user
                .map(|u| {
                    let text = if u.text.is_empty() {
                        u.parts.into_iter().map(|p| p.text).collect::<String>()
                    } else {
                        u.text
                    };
                    text.trim().to_string()
                })
                .unwrap_or_default()
        }
        15 => {
            value["type"] = json!("PLANNER_RESPONSE");
            let planner = step.planner.unwrap_or_default();
            value["thinking"] = json!(planner.thinking);
            value["tool_calls"] = json!(planner
                .calls
                .into_iter()
                .map(|c| {
                    let args =
                        serde_json::from_str::<Value>(&c.args).unwrap_or(Value::String(c.args));
                    json!({"name":c.name,"args":args})
                })
                .collect::<Vec<_>>());
            planner.text
        }
        5 => step.code.map(|c| c.summary).unwrap_or_default(),
        7 => step.search.map(|s| s.text).unwrap_or_default(),
        9 => step
            .directory
            .map(|d| {
                d.entries
                    .into_iter()
                    .map(|e| e.text)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        33 => step.web.map(|w| w.text).unwrap_or_default(),
        28 => step.command_status.map(|c| c.text).unwrap_or_default(),
        31 => step
            .url
            .and_then(|u| u.page)
            .and_then(|p| p.body)
            .and_then(|b| b.content)
            .map(|c| c.text)
            .unwrap_or_default(),
        8 => step.file.map(|f| f.text).unwrap_or_default(),
        21 => step
            .command
            .and_then(|c| c.output)
            .map(|t| t.text)
            .unwrap_or_default(),
        132 => step
            .generic
            .and_then(|g| g.output)
            .map(|t| t.text)
            .unwrap_or_default(),
        17 => {
            value["type"] = json!("ERROR_MESSAGE");
            value["source"] = json!("SYSTEM");
            step.error
                .and_then(|e| e.details)
                .map(|e| {
                    if e.text.is_empty() {
                        e.fallback
                    } else {
                        e.text
                    }
                })
                .unwrap_or_default()
        }
        90 => {
            value["source"] = json!("SYSTEM");
            step.ephemeral.map(|t| t.text).unwrap_or_default()
        }
        101 => {
            value["source"] = json!("SYSTEM");
            step.system.map(|t| t.text).unwrap_or_default()
        }
        _ => String::new(),
    };
    value["content"] = json!(if content.is_empty() && kind != 15 {
        format!("此 Antigravity 步骤没有可显示的文本（类型 {kind}）。")
    } else {
        content
    });
    Ok(value)
}

/// Read a single prompt for unindexed sessions; empty databases are not new conversations.
pub(super) fn preview(path: &Path) -> Option<(String, Option<i64>)> {
    let conn = crate::agy_conversation::open(path).ok()?;
    let row=conn.query_row("SELECT idx,step_type,status,step_format,step_payload FROM steps WHERE step_type IN (14,15) ORDER BY idx LIMIT 1",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,Vec<u8>>(4)?))).optional().ok()??;
    let value = decode(row.0, row.1, row.2, row.3, &row.4).ok()?;
    let text = if row.1 == 14 {
        extract_antigravity_content(&value).trim().to_string()
    } else {
        String::new()
    };
    Some((text, reader_timestamp(&value)))
}

pub(super) fn load(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut conn = crate::agy_conversation::open(path).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut query = tx
        .prepare("SELECT idx,step_type,status,step_format,step_payload FROM steps ORDER BY idx")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<Vec<u8>>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut steps = Vec::new();
    for row in rows {
        let (idx, kind, status, format, bytes) = row.map_err(|e| e.to_string())?;
        let Some(bytes) = bytes else { continue };
        steps.push(ReaderStep {
            value: decode(idx, kind, status, format, &bytes)?,
            fields: HashMap::new(),
            native_index: Some(idx),
        });
    }
    let indices = steps
        .iter()
        .filter(|s| s.value["type"] == "PLANNER_RESPONSE")
        .filter_map(|s| s.value["step_index"].as_u64())
        .collect::<Vec<_>>();
    let models = crate::agy_conversation::models::models_in(&tx, &indices).unwrap_or_default();
    for step in &mut steps {
        if let Some(model) = step.value["step_index"]
            .as_u64()
            .and_then(|i| models.get(&i))
        {
            step.value["model"] = json!(model);
        }
    }
    messages_from_steps(steps)
}

pub(super) fn resolve(path: &Path, id: &str, pointer: &str) -> Result<String, String> {
    let index = id
        .parse::<i64>()
        .map_err(|_| "Invalid Antigravity step reference")?;
    let conn = crate::agy_conversation::open(path).map_err(|e| e.to_string())?;
    let row = conn
        .query_row(
            "SELECT step_type,status,step_format,step_payload FROM steps WHERE idx=?1",
            [index],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("Antigravity step no longer exists")?;
    if row.3.len() as u64 > crate::session_manager::content::MAX_TEXT_BYTES {
        return Err("Content exceeds 32MB".into());
    }
    let value = decode(index, row.0, row.1, row.2, &row.3)?;
    if pointer.starts_with(ANTIGRAVITY_READER_REF_PREFIX) {
        return resolve_antigravity_reader_content(&value, pointer);
    }
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or("Invalid Antigravity content reference".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::content::{resolve_content_ref, SourceLocation, ValidatedSource};

    fn database(path: &Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE steps(idx INTEGER PRIMARY KEY,step_type INTEGER,status INTEGER,step_format INTEGER,step_payload BLOB);").unwrap();
        conn
    }
    fn insert(conn: &rusqlite::Connection, index: i64, kind: i64, step: Step) {
        conn.execute(
            "INSERT INTO steps VALUES (?1,?2,3,0,?3)",
            rusqlite::params![index, kind, step.encode_to_vec()],
        )
        .unwrap();
    }
    #[test]
    fn native_details_reuse_reader_blocks_and_full_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let conn = database(&path);
        insert(
            &conn,
            0,
            14,
            Step {
                user: Some(User {
                    text: String::new(),
                    parts: vec![
                        Text {
                            text: "old ".into(),
                        },
                        Text {
                            text: "<USER_REQUEST>request</USER_REQUEST>".into(),
                        },
                    ],
                }),
                ..Default::default()
            },
        );
        let thought = "思考🦀".repeat(3000);
        let output = "旧文件内容\n".repeat(3000);
        insert(
            &conn,
            1,
            15,
            Step {
                planner: Some(Planner {
                    text: "answer".into(),
                    thinking: thought.clone(),
                    calls: vec![Call {
                        name: "read_file".into(),
                        args: json!({"Path":"/tmp/example","note":"参数".repeat(4000)}).to_string(),
                    }],
                }),
                ..Default::default()
            },
        );
        insert(
            &conn,
            2,
            132,
            Step {
                generic: Some(Generic {
                    output: Some(Text {
                        text: output.clone(),
                    }),
                }),
                ..Default::default()
            },
        );
        let webpage = "网页🦀正文\n".repeat(3000);
        insert(
            &conn,
            3,
            15,
            Step {
                planner: Some(Planner {
                    text: String::new(),
                    thinking: String::new(),
                    calls: vec![Call {
                        name: "read_url_content".into(),
                        args: "{}".into(),
                    }],
                }),
                ..Default::default()
            },
        );
        insert(
            &conn,
            4,
            31,
            Step {
                url: Some(UrlContent {
                    page: Some(Page {
                        body: Some(PageBody {
                            content: Some(UrlText {
                                text: webpage.clone(),
                            }),
                        }),
                    }),
                }),
                ..Default::default()
            },
        );
        let source = ValidatedSource {
            provider_id: "agy".into(),
            raw: String::new(),
            location: SourceLocation::Sqlite {
                db: path.clone(),
                session_id: "old".into(),
            },
        };
        let messages = load(&path).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].role, "user");
        assert_eq!(
            messages[0].content,
            "old <USER_REQUEST>request</USER_REQUEST>"
        );
        assert_eq!(messages[2].role, "tool");
        for (message, expected) in [
            (&messages[1], thought.as_str()),
            (&messages[2], output.as_str()),
            (&messages[4], webpage.as_str()),
        ] {
            let reference = message
                .blocks
                .iter()
                .find_map(|b| match b {
                    SessionBlock::Thinking { full, .. } | SessionBlock::ToolResult { full, .. } => {
                        full.as_ref()
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(resolve_content_ref(&source, reference).unwrap(), expected);
        }
        let input = messages[1]
            .blocks
            .iter()
            .find_map(|b| match b {
                SessionBlock::ToolCall { input_full, .. } => input_full.as_ref(),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&resolve_content_ref(&source, input).unwrap()).unwrap()
                ["path"],
            "/tmp/example"
        );
    }
    #[test]
    fn native_scan_and_empty_transcript_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let conversations = dir.path().join("conversations");
        fs::create_dir_all(&conversations).unwrap();
        let path = conversations.join("old-id.db");
        let conn = database(&path);
        insert(
            &conn,
            0,
            14,
            Step {
                user: Some(User {
                    text: "real old request".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        let sessions = scan_root(&dir.path().join("brain"), None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            parse_sqlite_source(sessions[0].source_path.as_deref().unwrap()).unwrap(),
            (path.clone(), "old-id".into())
        );
        let logs = dir.path().join("brain/old-id/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let transcript = logs.join("transcript_full.jsonl");
        fs::write(&transcript, "").unwrap();
        assert_eq!(
            super::super::load_messages(&transcript).unwrap()[0].content,
            "real old request"
        );
        let source = ValidatedSource {
            provider_id: "agy".into(),
            raw: String::new(),
            location: SourceLocation::Path {
                path: transcript,
                root: dir.path().join("brain"),
            },
        };
        let reference = ContentRef::Sqlite {
            table: "other".into(),
            id: "0".into(),
            column: "step_payload".into(),
            pointer: "/content".into(),
        };
        assert!(resolve_content_ref(&source, &reference).is_err());
    }
    #[test]
    fn malformed_and_future_formats_are_explicit_errors() {
        assert!(decode(0, 14, 3, 1, &[]).is_err());
        assert!(decode(0, 14, 3, 0, &[255]).is_err());
    }

    #[test]
    #[ignore = "requires copied native AGY databases; reports metadata only"]
    fn local_native_databases() {
        let root = PathBuf::from(std::env::var("CC_SWITCH_AGY_NATIVE_ROOT").unwrap());
        let mut listed = 0;
        let mut compared = 0;
        let mut changed_user_fields = 0;
        let mut exported_databases = 0;
        let mut databases = 0;
        let mut messages = 0;
        let mut user = 0;
        let mut assistant = 0;
        let mut tools = 0;
        let mut references = 0;
        for profile in fs::read_dir(&root).unwrap().flatten() {
            listed += scan_root(&profile.path().join("brain"), None)
                .iter()
                .filter(|s| s.source_path.is_some())
                .count();
            let Ok(entries) = fs::read_dir(profile.path().join("conversations")) else {
                continue;
            };
            for entry in entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|s| s == "db"))
            {
                let path = entry.path();
                let parsed = load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                databases += 1;
                messages += parsed.len();
                let transcript = profile
                    .path()
                    .join("brain")
                    .join(path.file_stem().unwrap())
                    .join(".system_generated/logs/transcript_full.jsonl");
                if let Ok(exported) = records(&transcript) {
                    exported_databases += 1;
                    let exported = exported
                        .into_iter()
                        .filter_map(|v| v["step_index"].as_i64().map(|i| (i, v)))
                        .collect::<HashMap<_, _>>();
                    let conn = crate::agy_conversation::open(&path).unwrap();
                    let mut query=conn.prepare("SELECT idx,step_type,status,step_format,step_payload FROM steps WHERE step_type IN (14,15)").unwrap();
                    let rows = query
                        .query_map([], |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, i64>(2)?,
                                r.get::<_, i64>(3)?,
                                r.get::<_, Vec<u8>>(4)?,
                            ))
                        })
                        .unwrap();
                    for row in rows {
                        let (idx, kind, status, format, bytes) = row.unwrap();
                        let native = decode(idx, kind, status, format, &bytes).unwrap();
                        let Some(exported) = exported.get(&idx) else {
                            continue;
                        };
                        // Exports truncate timestamps to seconds; the native store keeps nanos.
                        if !reader_timestamp(&native)
                            .zip(reader_timestamp(exported))
                            .is_some_and(|(a, b)| a.div_euclid(1000) == b.div_euclid(1000))
                        {
                            continue;
                        }
                        for key in ["content", "thinking"] {
                            if let (Some(a), Some(b)) =
                                (native[key].as_str(), exported[key].as_str())
                            {
                                // Exported user requests carry CLI context wrappers.
                                let (a, b) = if kind == 14 {
                                    (a.trim(), user_request(b).trim())
                                } else {
                                    (a, b)
                                };
                                // Some native generations omit text retained by the export.
                                if a.is_empty() || b.is_empty() {
                                    continue;
                                }
                                // Native stored user text can differ from the captured export; display its actual value.
                                if kind == 14 && a != b {
                                    changed_user_fields += 1;
                                    continue;
                                }
                                assert!(a == b, "Native/exported {key} differ at step {idx}");
                                compared += 1;
                            }
                        }
                    }
                }
                let source = ValidatedSource {
                    provider_id: "agy".into(),
                    raw: String::new(),
                    location: SourceLocation::Sqlite {
                        db: path,
                        session_id: String::new(),
                    },
                };
                for m in parsed {
                    match m.role.as_str() {
                        "user" => user += 1,
                        "assistant" => assistant += 1,
                        "tool" => tools += 1,
                        _ => (),
                    };
                    for block in m.blocks {
                        let refs = match block {
                            SessionBlock::Thinking { full, .. }
                            | SessionBlock::ToolResult { full, .. } => vec![full],
                            SessionBlock::ToolCall {
                                input_full, diff, ..
                            } => vec![input_full, diff.and_then(|d| d.full)],
                            _ => vec![],
                        };
                        for reference in refs.into_iter().flatten() {
                            resolve_content_ref(&source, &reference).unwrap();
                            references += 1;
                        }
                    }
                }
            }
        }
        assert!(databases > 0);
        assert!(exported_databases == 0 || compared > 0);
        println!(
            "{}",
            json!({"databases":databases,"listed":listed,"compared_fields":compared,"changed_user_fields":changed_user_fields,"messages":messages,"user":user,"assistant":assistant,"tools":tools,"references":references})
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::session_manager::{
        cache::source_fingerprint,
        content::{SourceLocation, ValidatedSource},
    };

    #[test]
    fn sql_source_rejects_foreign_database_and_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("antigravity");
        fs::create_dir_all(root.join("conversations")).unwrap();
        let db = root.join("conversations/old.db");
        rusqlite::Connection::open(&db).unwrap();
        let sources = vec![(root.join("brain"), None)];
        let source = format!("sqlite:{}#old", db.display());
        assert!(validate_sqlite_in(&source, &sources).is_ok());
        assert!(validate_sqlite_in(&format!("sqlite:{}#other", db.display()), &sources).is_err());
        let foreign = dir.path().join("foreign.db");
        rusqlite::Connection::open(&foreign).unwrap();
        assert!(
            validate_sqlite_in(&format!("sqlite:{}#old", foreign.display()), &sources).is_err()
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&foreign, root.join("conversations/escape.db")).unwrap();
            assert!(validate_sqlite_in(
                &format!("sqlite:{}/conversations/escape.db#escape", root.display()),
                &sources
            )
            .is_err());
        }
    }
    #[test]
    fn native_reads_committed_wal_and_invalidates_both_source_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("conversations/old.db");
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let writer = rusqlite::Connection::open(&db).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE steps(idx INTEGER PRIMARY KEY,step_type INTEGER,status INTEGER,step_format INTEGER,step_payload BLOB);").unwrap();
        let logs = dir.path().join("brain/old/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let transcript = logs.join("transcript_full.jsonl");
        fs::write(&transcript, "").unwrap();
        let sql = ValidatedSource {
            provider_id: "agy".into(),
            raw: String::new(),
            location: SourceLocation::Sqlite {
                db: db.clone(),
                session_id: "old".into(),
            },
        };
        let jsonl = ValidatedSource {
            provider_id: "agy".into(),
            raw: String::new(),
            location: SourceLocation::Path {
                path: transcript.clone(),
                root: dir.path().join("brain"),
            },
        };
        let before = [
            source_fingerprint(&sql).unwrap(),
            source_fingerprint(&jsonl).unwrap(),
        ];
        writer
            .execute(
                "INSERT INTO steps VALUES (0,14,3,0,?1)",
                [Step {
                    user: Some(User {
                        text: "committed WAL request".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .encode_to_vec()],
            )
            .unwrap();
        assert_ne!(source_fingerprint(&sql).unwrap(), before[0]);
        assert_ne!(source_fingerprint(&jsonl).unwrap(), before[1]);
        assert_eq!(load(&db).unwrap()[0].content, "committed WAL request");
        assert_eq!(
            super::super::load_messages(&transcript).unwrap()[0].content,
            "committed WAL request"
        );
    }
}
