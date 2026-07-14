use codex_plus_core::models::{ExportResult, ExportStatus, SessionRef};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn export_markdown_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
) -> ExportResult {
    let thread_id = normalize_session_id(&session.session_id);
    let mut result = failed(&thread_id, "未找到对应会话");
    let mut saw_candidate = false;
    for db_path in db_paths {
        if !db_path.exists() {
            continue;
        }
        saw_candidate = true;
        let candidate = MarkdownExportService::new(Some(db_path)).export(session);
        if matches!(candidate.status, ExportStatus::Exported) {
            return candidate;
        }
        if result.message == "未找到对应会话" || candidate.message != "未找到对应会话"
        {
            result = candidate;
        }
    }
    if saw_candidate {
        result
    } else {
        failed(&thread_id, "未配置本地 Codex 数据库")
    }
}

#[derive(Debug, Clone)]
pub struct MarkdownExportService {
    db_path: Option<PathBuf>,
}

impl MarkdownExportService {
    pub fn new(db_path: Option<impl Into<PathBuf>>) -> Self {
        Self {
            db_path: db_path.map(Into::into),
        }
    }

    pub fn export(&self, session: &SessionRef) -> ExportResult {
        let Some(db_path) = &self.db_path else {
            return failed(&session.session_id, "未配置本地 Codex 数据库");
        };
        if !db_path.exists() {
            return failed(
                &session.session_id,
                format!("数据库不存在：{}", db_path.to_string_lossy()),
            );
        }
        let thread_id = normalize_session_id(&session.session_id);
        let result = (|| -> anyhow::Result<ExportResult> {
            let db = Connection::open(db_path)?;
            let record = match lookup_thread_record(&db, db_path, &thread_id)? {
                ThreadLookup::Found(record) => record,
                ThreadLookup::Missing => return Ok(failed(&thread_id, "未找到对应会话")),
                ThreadLookup::Unsupported => {
                    return Ok(failed(&thread_id, "不支持当前本地存储结构"));
                }
            };
            let title = display_title(record.title.as_deref().unwrap_or(&session.title));
            let Some(rollout_path) = record
                .rollout_path
                .filter(|path| !path.as_os_str().is_empty())
            else {
                return Ok(failed(&thread_id, "会话缺少 rollout 文件路径"));
            };
            if !rollout_path.is_file() {
                return Ok(failed(
                    &thread_id,
                    format!("rollout 文件不存在：{}", rollout_path.to_string_lossy()),
                ));
            }
            let messages = load_messages(&rollout_path)?;
            if messages.is_empty() {
                return Ok(failed(&thread_id, "未找到可导出的用户或助手消息"));
            }
            let filename = build_filename(&title, &thread_id);
            let markdown = render_markdown(&title, &messages);
            Ok(ExportResult {
                status: ExportStatus::Exported,
                session_id: thread_id.clone(),
                message: format!("已导出为 Markdown：{filename}"),
                filename: Some(filename),
                markdown: Some(markdown),
            })
        })();
        result.unwrap_or_else(|err| failed(&thread_id, format!("读取 rollout 失败：{err}")))
    }
}

#[derive(Debug)]
struct ThreadRecord {
    title: Option<String>,
    rollout_path: Option<PathBuf>,
}

#[derive(Debug)]
enum ThreadLookup {
    Found(ThreadRecord),
    Missing,
    Unsupported,
}

#[derive(Debug)]
struct Message {
    speaker: &'static str,
    timestamp: Option<String>,
    body: String,
}

fn failed(session_id: &str, message: impl Into<String>) -> ExportResult {
    ExportResult {
        status: ExportStatus::Failed,
        session_id: session_id.to_string(),
        message: message.into(),
        filename: None,
        markdown: None,
    }
}

fn lookup_thread_record(
    db: &Connection,
    db_path: &Path,
    thread_id: &str,
) -> anyhow::Result<ThreadLookup> {
    if has_columns(db, "threads", &["id", "title", "rollout_path"])? {
        let row = db.query_row(
            "SELECT title, rollout_path FROM threads WHERE id = ?1",
            [thread_id],
            |row| {
                Ok(ThreadRecord {
                    title: row.get::<_, Option<String>>(0)?,
                    rollout_path: row
                        .get::<_, Option<String>>(1)?
                        .filter(|path| !path.trim().is_empty())
                        .map(PathBuf::from),
                })
            },
        );
        return match row {
            Ok(row) => Ok(ThreadLookup::Found(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ThreadLookup::Missing),
            Err(err) => Err(err.into()),
        };
    }

    if has_columns(db, "automation_runs", &["thread_id"])? {
        let columns = table_columns(db, "automation_runs")?;
        let title_expr = if columns.iter().any(|column| column == "thread_title") {
            "thread_title"
        } else if columns.iter().any(|column| column == "title") {
            "title"
        } else {
            "''"
        };
        let sql = format!("SELECT {title_expr} FROM automation_runs WHERE thread_id = ?1");
        let row = db.query_row(
            &sql,
            [thread_id],
            |row| Ok(row.get::<_, Option<String>>(0)?),
        );
        return match row {
            Ok(title) => Ok(ThreadLookup::Found(ThreadRecord {
                title,
                rollout_path: discover_rollout_path(db_path, thread_id)?,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ThreadLookup::Missing),
            Err(err) => Err(err.into()),
        };
    }

    Ok(ThreadLookup::Unsupported)
}

fn discover_rollout_path(db_path: &Path, thread_id: &str) -> anyhow::Result<Option<PathBuf>> {
    for home in codex_home_candidates(db_path) {
        let mut candidates = Vec::new();
        collect_jsonl_files(&home.join("sessions"), &mut candidates)?;
        collect_jsonl_files(&home.join("archived_sessions"), &mut candidates)?;
        candidates.sort_by_key(|path| {
            std::cmp::Reverse(
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok(),
            )
        });
        for path in candidates {
            if rollout_matches_thread(&path, thread_id)? {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

fn codex_home_candidates(db_path: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for ancestor in db_path.ancestors().skip(1) {
        if ancestor.join("sessions").is_dir() || ancestor.join("archived_sessions").is_dir() {
            homes.push(ancestor.to_path_buf());
        }
    }
    if homes.is_empty() {
        if let Some(parent) = db_path.parent().and_then(Path::parent) {
            homes.push(parent.to_path_buf());
        }
    }
    homes
}

fn collect_jsonl_files(dir: &Path, output: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_jsonl_files(&path, output)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            output.push(path);
        }
    }
    Ok(())
}

fn rollout_matches_thread(path: &Path, thread_id: &str) -> anyhow::Result<bool> {
    for raw in fs::read_to_string(path)?.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if event.get("type") != Some(&Value::String("session_meta".to_string())) {
            continue;
        }
        let id = event
            .get("payload")
            .and_then(|payload| payload.get("id"))
            .or_else(|| event.get("id"))
            .and_then(Value::as_str)
            .map(normalize_session_id)
            .unwrap_or_default();
        if id == thread_id {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_columns(db: &Connection, table: &str, columns: &[&str]) -> anyhow::Result<bool> {
    let existing = table_columns(db, table)?;
    if existing.is_empty() {
        return Ok(false);
    }
    Ok(columns
        .iter()
        .all(|column| existing.iter().any(|existing| existing == column)))
}

fn table_columns(db: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    if !db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .is_ok()
    {
        return Ok(Vec::new());
    }
    let mut stmt = db.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table.replace('"', "\"\"")
    ))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(columns.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_messages(path: &Path) -> anyhow::Result<Vec<Message>> {
    let mut messages = Vec::new();
    for raw in fs::read_to_string(path)?.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(raw)?;
        if event.get("type") != Some(&Value::String("response_item".to_string())) {
            continue;
        }
        let payload = &event["payload"];
        if payload.get("type") != Some(&Value::String("message".to_string())) {
            continue;
        }
        let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
        let speaker = match role {
            "user" => "User",
            "assistant" => "Assistant",
            _ => continue,
        };
        let body = serialize_message_content(&payload["content"]);
        if body.is_empty() {
            continue;
        }
        messages.push(Message {
            speaker,
            timestamp: format_timestamp(event.get("timestamp")),
            body,
        });
    }
    Ok(messages)
}

fn serialize_message_content(content: &Value) -> String {
    let Some(items) = content.as_array() else {
        return String::new();
    };
    items
        .iter()
        .filter_map(|block| {
            let block_type = block.get("type").and_then(Value::as_str)?;
            match block_type {
                "input_text" | "output_text" => {
                    let text =
                        normalize_newlines(block.get("text").and_then(Value::as_str).unwrap_or(""))
                            .trim_matches('\n')
                            .to_string();
                    (!text.trim().is_empty()).then_some(text)
                }
                "input_image" => {
                    let image_url = block
                        .get("image_url")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if image_url.is_empty() || image_url.starts_with("data:") {
                        Some("> Image attachment".to_string())
                    } else {
                        Some(format!("> Image attachment\n[Image link](<{image_url}>)"))
                    }
                }
                _ => None,
            }
        })
        .filter(|block| !block.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn format_timestamp(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw
        .strip_suffix('Z')
        .map_or_else(|| raw.to_string(), |prefix| format!("{prefix}+00:00"));
    let parsed = chrono::DateTime::parse_from_rfc3339(&normalized).ok()?;
    Some(
        parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}

fn display_title(value: &str) -> String {
    let normalized = normalize_newlines(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        "Untitled session".to_string()
    } else {
        normalized
    }
}

fn build_filename(title: &str, thread_id: &str) -> String {
    let cleaned = collapse_whitespace(&replace_windows_filename_chars(title, " "))
        .trim_matches([' ', '.'])
        .to_string();
    let mut safe_title = cleaned
        .chars()
        .take(80)
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string();
    if safe_title.is_empty() {
        safe_title = "Untitled session".to_string();
    }
    let safe_thread_id = replace_windows_filename_chars(thread_id, "-");
    format!("{safe_title}-{}.md", safe_thread_id.trim())
}

fn render_markdown(title: &str, messages: &[Message]) -> String {
    let mut lines = vec![format!("# {title}"), String::new()];
    for message in messages {
        lines.push(format!("### {}", message.speaker));
        if let Some(timestamp) = &message.timestamp {
            lines.push(format!("_{timestamp}_"));
        }
        lines.push(String::new());
        lines.push(message.body.trim_end().to_string());
        lines.push(String::new());
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn normalize_session_id(session_id: &str) -> String {
    session_id
        .strip_prefix("local:")
        .unwrap_or(session_id)
        .to_string()
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn replace_windows_filename_chars(value: &str, replacement: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control() {
            output.push_str(replacement);
        } else {
            output.push(ch);
        }
    }
    output
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
