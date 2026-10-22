use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde_json::Value;

use crate::session_manager::model::SessionBlock;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, path_basename, truncate_summary, FileParseCache, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "deepseek-harness";
/// 新版 DeepSeek Harness Desktop（官方 Version 0.2.1-rc.2；窗口标题里的 2.0.17
/// 是 Electron 壳构建号）把新会话写为 `session.v4.jsonl.zstd`：事件结构与旧版
/// 兼容，usage 从 `data.message.usage` 移到 `data.usage`，解析层已兼容两者；
/// 旧版本写 `session.jsonl.zstd`。同一会话目录两者不会并存，扫描时两个都收。
const SESSION_FILE_NAME_V4: &str = "session.v4.jsonl.zstd";
const SESSION_FILE_NAME_LEGACY: &str = "session.jsonl.zstd";
/// Compressed size guard; decompressed content is bounded by the event limit.
const MAX_SESSION_BYTES: u64 = 64 * 1024 * 1024;
/// Decompressed size guard against zip-bomb-style payloads; bytes beyond the
/// limit are silently treated as EOF.
const MAX_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EVENTS: usize = 500_000;

fn session_file_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    (name == SESSION_FILE_NAME_V4 || name == SESSION_FILE_NAME_LEGACY).then_some(name)
}

/// 会话页每次打开都会全量扫描；没变过的文件直接复用上次的解析结果。
/// zstd 解压比 claude/codex 的明文解析更贵，缓存收益更大。
static PARSE_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);

pub fn session_roots() -> Vec<PathBuf> {
    vec![crate::deepseek_harness_config::get_dsh_home().join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = crate::deepseek_harness_config::get_dsh_home().join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for project in entries.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(session_dirs) = fs::read_dir(&project_path) else {
            continue;
        };
        for session_dir in session_dirs.flatten() {
            let dir = session_dir.path();
            // 同一目录优先 v4；旧版文件只在没有 v4 时收录，避免同一会话出现两条。
            let v4 = dir.join(SESSION_FILE_NAME_V4);
            if v4.is_file() {
                files.push(v4);
                continue;
            }
            let legacy = dir.join(SESSION_FILE_NAME_LEGACY);
            if legacy.is_file() {
                files.push(legacy);
            }
        }
    }
    PARSE_CACHE.scan(files, scan_session_file)
}

/// 适配 [`FileParseCache`] 的解析入口：解析失败按 io 错误上抛，
/// 让缓存跳过且不记住「没有会话」，下轮扫描重试。
fn scan_session_file(path: &Path) -> io::Result<Option<SessionMeta>> {
    match parse_session(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let events = read_events(path)?;
    let mut messages = Vec::new();
    for value in &events {
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    messages.push(SessionMessage::from_blocks(
                        "user",
                        event_time(value),
                        vec![SessionBlock::text(content)],
                    ));
                }
            }
            Some("assistant/message") => {
                let content = assistant_text(value);
                if !content.trim().is_empty() {
                    messages.push(SessionMessage::from_blocks(
                        "assistant",
                        event_time(value),
                        vec![SessionBlock::text(content)],
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if session_id.contains(['/', '\\']) || session_id == "." || session_id == ".." {
        return Err("Invalid DSH session ID".to_string());
    }
    if session_file_name(path).is_none() {
        return Err(format!(
            "DSH session source must be a {SESSION_FILE_NAME_V4} or {SESSION_FILE_NAME_LEGACY} file: {}",
            path.display()
        ));
    }
    let meta = parse_session(path)?;
    if meta.session_id != session_id {
        return Err(format!(
            "DSH session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve DSH sessions root {}: {error}",
            root.display()
        )
    })?;
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve DSH session {}: {error}", path.display()))?;
    let dir = source
        .parent()
        .ok_or_else(|| "DSH session has no parent directory".to_string())?;
    if !dir.starts_with(&root) {
        return Err("DSH session is outside the sessions root".to_string());
    }
    fs::remove_dir_all(dir)
        .map_err(|error| format!("Failed to delete DSH session {}: {error}", dir.display()))?;
    Ok(true)
}

/// 用量统计的扁平事件行：每条 `assistant/message` 的最终 usage 一行（与
/// `assistant/chunk` 里的 usage 逐 step 一致，只取 message 才不会双计），
/// 归因取文件顺序上最近一次 `request/header` 的 provider/model——模型切换
/// 会产生新 header，切换前后的消息各自归到当时的模型。解析留在 provider
/// 模块、入库在 session_usage 模块（与 agy 的 records 同款分层）。
pub(crate) fn usage_records(path: &Path) -> Result<Vec<Value>, String> {
    let events = read_events(path)?;
    let mut provider: Option<String> = None;
    let mut model: Option<String> = None;
    let mut rows = Vec::new();
    for value in &events {
        match value.get("type").and_then(Value::as_str) {
            Some("request/header") => {
                let config = value.pointer("/data/header/config");
                if let Some(found) = config
                    .and_then(|c| c.get("provider"))
                    .and_then(Value::as_str)
                {
                    provider = Some(found.to_string());
                }
                if let Some(found) = config.and_then(|c| c.get("model")).and_then(Value::as_str) {
                    model = Some(found.to_string());
                }
            }
            Some("assistant/message") => {
                let data = &value["data"];
                let Some(usage) = data.pointer("/message/usage").or_else(|| data.get("usage"))
                else {
                    continue;
                };
                let id = data
                    .pointer("/message/id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        let turn = data.get("turn").and_then(Value::as_i64).unwrap_or(0);
                        let step = data.get("step").and_then(Value::as_i64).unwrap_or(0);
                        let seq = value.get("seq").and_then(Value::as_i64).unwrap_or(0);
                        format!("t{turn}s{step}q{seq}")
                    });
                rows.push(serde_json::json!({
                    "id": id,
                    "time": value.get("time").and_then(Value::as_i64).unwrap_or(0),
                    "provider": provider,
                    "model": model,
                    "usage": usage,
                }));
            }
            _ => {}
        }
    }
    Ok(rows)
}

fn event_time(value: &Value) -> Option<i64> {
    value
        .get("time")
        .or_else(|| value.get("time0"))
        .and_then(Value::as_i64)
}

fn assistant_text(value: &Value) -> String {
    value
        .pointer("/data/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn read_events(path: &Path) -> Result<Vec<Value>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to stat DSH session {}: {error}", path.display()))?;
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(format!(
            "DSH session exceeds the {MAX_SESSION_BYTES}-byte safety limit"
        ));
    }
    let file = File::open(path).map_err(|error| format!("Failed to open DSH session: {error}"))?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .map_err(|error| format!("Failed to decompress DSH session: {error}"))?;
    let reader = BufReader::new(decoder.take(MAX_DECOMPRESSED_BYTES));
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read DSH session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if events.len() >= MAX_EVENTS {
            return Err(format!(
                "DSH session exceeds the {MAX_EVENTS}-event safety limit"
            ));
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            events.push(value);
        }
    }
    Ok(events)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve DSH session {}: {error}", path.display()))?;
    let events = read_events(&source)?;
    let header = events
        .iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session"))
        .ok_or_else(|| "DSH session has no header".to_string())?;
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH session header has no id".to_string())?
        .to_string();
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = header.get("createdAt").and_then(Value::as_i64);

    let mut title = events
        .iter()
        .rev()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session/title"))
        .and_then(|value| value.pointer("/data/title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut first_user: Option<String> = None;
    let mut last_text: Option<String> = None;
    let mut last_active_at = created_at;
    for value in &events {
        if let Some(time) = event_time(value) {
            last_active_at = Some(last_active_at.map_or(time, |current| current.max(time)));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if first_user.is_none() && !content.trim().is_empty() {
                    first_user = Some(content.clone());
                }
                if !content.trim().is_empty() {
                    last_text = Some(content);
                }
            }
            Some("assistant/message") => {
                let text = assistant_text(value);
                if !text.trim().is_empty() {
                    last_text = Some(text);
                }
            }
            _ => {}
        }
    }
    if title.is_none() {
        title = first_user
            .as_deref()
            .map(|message| truncate_summary(message, TITLE_MAX_CHARS))
            .filter(|message| !message.is_empty())
            .or_else(|| cwd.as_deref().and_then(path_basename));
    }
    let summary = last_text
        .as_deref()
        .map(|message| truncate_summary(message, 160))
        .filter(|message| !message.is_empty());
    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: id,
        title,
        summary,
        project_dir: cwd.filter(|value| !value.trim().is_empty()),
        created_at,
        last_active_at,
        source_path: source.to_str().map(str::to_string),
        resume_command: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn write_session(home: &Path, project: &str, id: &str, events: &[Value]) -> PathBuf {
        write_session_file(home, project, id, SESSION_FILE_NAME_LEGACY, events)
    }

    fn write_session_file(
        home: &Path,
        project: &str,
        id: &str,
        file_name: &str,
        events: &[Value],
    ) -> PathBuf {
        let dir = home.join("sessions").join(project).join(id);
        fs::create_dir_all(&dir).unwrap();
        let payload = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let compressed = zstd::stream::encode_all(payload.as_bytes(), 3).unwrap();
        let file = dir.join(file_name);
        fs::write(&file, compressed).unwrap();
        file
    }

    fn fixture_events() -> Vec<Value> {
        vec![
            serde_json::json!({"type":"session","version":0,"id":"session-abc","createdAt":1787104521333_i64,"cwd":"/Users/test/project"}),
            serde_json::json!({"type":"user/message","seq":8,"time":1787220655606_i64,"data":{"role":"user","content":[{"type":"text","text":"hello dsh"}]}}),
            serde_json::json!({"type":"session/title","seq":11,"time":1787220655607_i64,"data":{"title":"Greeting"}}),
            serde_json::json!({"type":"text-chunks","seq0":69,"time0":1787220662295_i64,"data":{"turn":1,"step":1,"index":1,"texts":["stream"," delta"]}}),
            serde_json::json!({"type":"assistant/message","seq":352,"time":1787220668317_i64,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"reasoning","text":"thinking"},{"type":"text","text":"hi there"}]}}}),
        ]
    }

    fn with_temp_home(test: impl FnOnce(&Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    #[test]
    #[serial]
    fn scan_reads_zstd_sessions_with_titles() {
        with_temp_home(|home| {
            let file = write_session(
                home,
                "--Users-test-project--",
                "session-abc",
                &fixture_events(),
            );
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
            let meta = &sessions[0];
            assert_eq!(meta.provider_id, "deepseek-harness");
            assert_eq!(meta.session_id, "session-abc");
            assert_eq!(meta.title.as_deref(), Some("Greeting"));
            assert_eq!(meta.project_dir.as_deref(), Some("/Users/test/project"));
            assert_eq!(meta.created_at, Some(1787104521333));
            assert_eq!(meta.last_active_at, Some(1787220668317));
            assert_eq!(
                meta.source_path.as_deref(),
                file.canonicalize().unwrap().to_str()
            );
        });
    }

    #[test]
    #[serial]
    fn load_messages_skips_stream_deltas_and_reasoning() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let messages = load_messages(&file).unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].role, "user");
            assert_eq!(messages[0].content, "hello dsh");
            assert_eq!(messages[0].ts, Some(1787220655606));
            assert_eq!(messages[1].role, "assistant");
            assert_eq!(messages[1].content, "hi there");
            let serialized = serde_json::to_value(&messages[1]).unwrap();
            assert_eq!(serialized["blocks"][0]["type"], "text");
            assert_eq!(serialized["blocks"][0]["text"], "hi there");
            assert!(serialized.get("content").is_none());
            let transcript =
                crate::session_manager::load_transcript(PROVIDER_ID, file.to_str().unwrap())
                    .unwrap();
            assert!(!transcript.cached);
        });
    }

    #[test]
    #[serial]
    fn corrupt_sessions_are_skipped_not_fatal() {
        with_temp_home(|home| {
            let dir = home.join("sessions").join("proj").join("session-bad");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(SESSION_FILE_NAME_LEGACY), b"not zstd at all").unwrap();
            write_session(home, "proj", "session-abc", &fixture_events());
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
        });
    }

    #[test]
    #[serial]
    fn rescan_reuses_the_cache_and_drops_deleted_sessions() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let first = scan_sessions();
            assert_eq!(first.len(), 1);
            // 第二次扫描命中解析缓存，结果保持一致
            let second = scan_sessions();
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].session_id, first[0].session_id);
            // 删除后按本轮文件列表重建缓存，不再返回已删会话
            delete_session(&home.join("sessions"), &file, "session-abc").unwrap();
            assert!(scan_sessions().is_empty());
        });
    }

    #[test]
    #[serial]
    fn v4_sessions_are_scanned_and_usage_read_from_data_root() {
        with_temp_home(|home| {
            // DeepSeek Harness 0.2.1-rc.2 的 v4 文件：文件名带 .v4，usage 挂在 data.usage（不在 message 里）
            let events = vec![
                serde_json::json!({"type":"session","version":4,"id":"session-v4","createdAt":1792483262099_i64,"cwd":"/Users/andy/Documents"}),
                serde_json::json!({"type":"request/header","seq":12,"time":1792640339967_i64,"data":{"header":{"config":{"provider":"yuzuvalley","model":"deepseek-flash"}}}}),
                serde_json::json!({"type":"assistant/message","seq":17,"time":1792640343104_i64,"data":{"turn":1,"step":1,"usage":{"inputTokens":567,"outputTokens":567,"totalTokens":8942,"cacheReadTokens":7808},"message":{"id":"16b568e1","role":"assistant","content":[]}}}),
            ];
            let file =
                write_session_file(home, "proj", "session-v4", SESSION_FILE_NAME_V4, &events);

            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1, "v4 文件必须被扫描到");
            assert_eq!(sessions[0].session_id, "session-v4");

            let rows = usage_records(&file).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["id"], "16b568e1");
            assert_eq!(rows[0]["model"], "deepseek-flash");
            assert_eq!(rows[0]["usage"]["inputTokens"], 567);

            // 删除路径同样接受 v4 文件名
            delete_session(&home.join("sessions"), &file, "session-v4").unwrap();
            assert!(scan_sessions().is_empty());
        });
    }

    #[test]
    #[serial]
    fn usage_records_attributes_messages_to_the_latest_header() {
        with_temp_home(|home| {
            let events = vec![
                serde_json::json!({"type":"request/header","seq":10,"time":1786945358957_i64,"data":{"header":{"config":{"provider":"deepseek-official","model":"k3"}}}}),
                serde_json::json!({"type":"assistant/message","seq":12,"time":1786945364644_i64,"data":{"turn":1,"step":1,"message":{"id":"msg-1","role":"assistant","content":[],"usage":{"inputTokens":3792,"outputTokens":107,"cacheReadTokens":6912,"reasoningTokens":59}}}}),
                // 同一文件里模型切换：新 header 之后的消息归到新 provider/model
                serde_json::json!({"type":"request/header","seq":30,"time":1786945400000_i64,"data":{"header":{"config":{"provider":"yuzuvalley","model":"glm-5.3"}}}}),
                serde_json::json!({"type":"assistant/message","seq":32,"time":1786945410000_i64,"data":{"turn":2,"step":1,"message":{"id":"msg-2","role":"assistant","content":[],"usage":{"inputTokens":20,"outputTokens":2,"cacheReadTokens":0}}}}),
                // 没有 message.id 的行回退到 turn/step/seq 组合键；零 usage 不产出行
                serde_json::json!({"type":"assistant/message","seq":40,"time":1786945420000_i64,"data":{"turn":3,"step":1,"message":{"role":"assistant","content":[]}}}),
            ];
            let file = write_session(home, "proj", "session-abc", &events);

            let rows = usage_records(&file).unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0]["id"], "msg-1");
            assert_eq!(rows[0]["provider"], "deepseek-official");
            assert_eq!(rows[0]["model"], "k3");
            assert_eq!(rows[0]["usage"]["inputTokens"], 3792);
            assert_eq!(rows[1]["id"], "msg-2");
            assert_eq!(rows[1]["provider"], "yuzuvalley");
            assert_eq!(rows[1]["model"], "glm-5.3");

            let orphan = write_session(
                home,
                "proj",
                "session-orphan",
                &[
                    serde_json::json!({"type":"assistant/message","seq":2,"time":5_i64,"data":{"turn":1,"step":1,"message":{"usage":{"inputTokens":9,"outputTokens":1,"cacheReadTokens":0}}}}),
                ],
            );
            let rows = usage_records(&orphan).unwrap();
            assert_eq!(rows.len(), 1);
            // 无 header 的旧会话：provider/model 为空，由用量模块回退到占位值
            assert_eq!(rows[0]["provider"], serde_json::Value::Null);
            // 无 message.id 时用 turn/step/seq 组合键
            assert_eq!(rows[0]["id"], "t1s1q2");
        });
    }

    #[test]
    #[serial]
    fn delete_removes_the_session_directory() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let root = home.join("sessions");
            let error = delete_session(&root, &file, "wrong-id").expect_err("id mismatch");
            assert!(error.contains("mismatch"), "unexpected error: {error}");
            assert!(file.exists());
            let deleted = delete_session(&root, &file, "session-abc").unwrap();
            assert!(deleted);
            assert!(!file.exists());
            assert!(!file.parent().unwrap().exists());
        });
    }

    #[test]
    #[serial]
    fn delete_accepts_an_unnormalized_root() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let root = home.join("sessions").join("proj").join("..");
            let deleted = delete_session(&root, &file, "session-abc").unwrap();
            assert!(deleted);
            assert!(!file.parent().unwrap().exists());
        });
    }
}
