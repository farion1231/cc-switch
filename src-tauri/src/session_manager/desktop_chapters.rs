//! Claude Code desktop 里用户手动 pin 的章节（只读）。
//!
//! 桌面端把它们存在 Chromium Local Storage（LevelDB）里，键 `epitaxy-chapters-v2`，
//! 值是一个 JSON：`state.bySession[<桌面会话 id>].userChapters[] = { id, afterId, title }`。
//! `afterId` 是 API 层的 id：`toolu_…`（tool_use 块）或 `msg_…-tN`（assistant 的
//! `message.id` + 第 N 个 text 块，只数 text）。这些 id 全局唯一，所以不需要知道
//! 桌面会话和 jsonl 的对应关系：在当前会话文件里找得到锚点的章节就属于它。
//!
//! 这是桌面端未公开的内部格式：任何一步读不到、解不开都当作「没有章节」，只记日志。
//! 永不写回，也不打开桌面端正在用的目录（它持有 LOCK）：复制一份到临时目录再读。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use rusty_leveldb::{Options, DB};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Local Storage 里的键：`_<origin>\0\x01<key>`（\x01 表示键名是 Latin-1 编码）
const STORAGE_KEY: &[u8] = b"_https://claude.ai\x00\x01epitaxy-chapters-v2";
/// 复制目录时的上限，防止异常大的目录拖慢阅读页
const MAX_COPY_BYTES: u64 = 256 * 1024 * 1024;

/// 章节锚在哪：一次工具调用，或一条 assistant 消息里的第 N 个 text 块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChapterAnchor {
    /// `ToolCall.id`
    #[serde(rename_all = "camelCase")]
    Tool { call_id: String },
    /// 同一个 `message.id` 会拆成多行，阅读页可能把它们并成一条消息，也可能不并：
    /// 这里给出所有行的 uuid（文档顺序），前端在这些消息里顺着数 text 块。
    #[serde(rename_all = "camelCase")]
    Text {
        message_ids: Vec<String>,
        text_index: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopChapter {
    pub id: String,
    pub title: String,
    pub anchor: ChapterAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredChapter {
    id: String,
    title: String,
    after_id: String,
}

#[derive(Deserialize)]
struct RawStore {
    state: RawState,
}

#[derive(Deserialize)]
struct RawState {
    #[serde(rename = "bySession", default)]
    by_session: HashMap<String, RawSession>,
}

#[derive(Deserialize)]
struct RawSession {
    #[serde(rename = "userChapters", default)]
    user_chapters: Vec<Value>,
}

/// 桌面端的 Local Storage 目录。目前只在 macOS 上实测过，其它平台不读。
fn default_storage_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().map(|dir| dir.join("Claude").join("Local Storage").join("leveldb"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Local Storage 的值：首字节 0 = UTF-16LE，1 = Latin-1。
fn decode_storage_value(value: &[u8]) -> Option<String> {
    let (prefix, body) = value.split_first()?;
    match prefix {
        0 => {
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).ok()
        }
        1 => Some(body.iter().map(|&byte| byte as char).collect()),
        _ => None,
    }
}

fn parse_store(json: &str) -> Result<Vec<StoredChapter>, String> {
    let store: RawStore = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut chapters = Vec::new();
    for session in store.state.by_session.into_values() {
        for raw in session.user_chapters {
            let field = |key: &str| {
                raw.get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            };
            // 字段不全的记录跳过，不影响其它章节
            if let (Some(id), Some(title), Some(after_id)) =
                (field("id"), field("title"), field("afterId"))
            {
                chapters.push(StoredChapter {
                    id,
                    title,
                    after_id,
                });
            }
        }
    }
    Ok(chapters)
}

/// 目录指纹：文件数、总大小、最新修改时间。桌面端一写入就会变。
fn dir_fingerprint(dir: &Path) -> std::io::Result<(usize, u64, Option<SystemTime>)> {
    let mut count = 0;
    let mut bytes = 0;
    let mut latest = None;
    for entry in fs::read_dir(dir)? {
        let meta = entry?.metadata()?;
        if !meta.is_file() {
            continue;
        }
        count += 1;
        bytes += meta.len();
        let modified = meta.modified().ok();
        if modified > latest {
            latest = modified;
        }
    }
    Ok((count, bytes, latest))
}

/// 把 LevelDB 目录复制到临时目录后只读打开，取出章节存储。
fn read_store(dir: &Path) -> Result<Vec<StoredChapter>, String> {
    let (_, bytes, _) = dir_fingerprint(dir).map_err(|e| e.to_string())?;
    if bytes > MAX_COPY_BYTES {
        return Err(format!("Local Storage directory too large ({bytes} bytes)"));
    }
    let copy = tempfile::tempdir().map_err(|e| e.to_string())?;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        // LOCK 归正在运行的桌面端所有，不复制；副本里由 rusty-leveldb 自己建
        if name == "LOCK" || !entry.path().is_file() {
            continue;
        }
        fs::copy(entry.path(), copy.path().join(&name)).map_err(|e| e.to_string())?;
    }
    let options = Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = DB::open(copy.path(), options).map_err(|e| e.to_string())?;
    let Some(value) = db.get(STORAGE_KEY) else {
        return Ok(Vec::new());
    };
    let json = decode_storage_value(&value).ok_or("unknown Local Storage value encoding")?;
    parse_store(&json)
}

type Fingerprint = (PathBuf, usize, u64, Option<SystemTime>);
type CachedStore = Option<(Fingerprint, Arc<Vec<StoredChapter>>)>;

fn store_cache() -> &'static Mutex<CachedStore> {
    static CACHE: OnceLock<Mutex<CachedStore>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// 读章节存储，按目录指纹缓存；桌面端正在写入时副本可能不完整，失败重试一次。
fn load_store(dir: &Path) -> Result<Arc<Vec<StoredChapter>>, String> {
    let (count, bytes, latest) = dir_fingerprint(dir).map_err(|e| e.to_string())?;
    let fingerprint = (dir.to_path_buf(), count, bytes, latest);
    if let Ok(cache) = store_cache().lock() {
        if let Some((cached, chapters)) = cache.as_ref() {
            if *cached == fingerprint {
                return Ok(chapters.clone());
            }
        }
    }
    let chapters = Arc::new(read_store(dir).or_else(|_| read_store(dir))?);
    if let Ok(mut cache) = store_cache().lock() {
        *cache = Some((fingerprint, chapters.clone()));
    }
    Ok(chapters)
}

/// `msg_…-tN` → (`msg_…`, N)
fn split_text_anchor(after_id: &str) -> Option<(&str, u32)> {
    let (message_id, index) = after_id.rsplit_once("-t")?;
    if message_id.is_empty() {
        return None;
    }
    Some((message_id, index.parse().ok()?))
}

/// 在会话 jsonl 里给章节找锚点；找不到锚点的章节不属于这个会话，丢掉。
fn resolve(chapters: &[StoredChapter], source: &Path) -> std::io::Result<Vec<DesktopChapter>> {
    // 要找的 tool_use id 与 message.id
    let mut wanted_tools: HashSet<&str> = HashSet::new();
    let mut wanted_messages: HashSet<&str> = HashSet::new();
    for chapter in chapters {
        match split_text_anchor(&chapter.after_id) {
            Some((message_id, _)) if !chapter.after_id.starts_with("toolu_") => {
                wanted_messages.insert(message_id);
            }
            _ => {
                wanted_tools.insert(chapter.after_id.as_str());
            }
        }
    }
    if wanted_tools.is_empty() && wanted_messages.is_empty() {
        return Ok(Vec::new());
    }

    let mut found_tools: HashSet<String> = HashSet::new();
    // message.id → (各行 uuid, text 块总数)
    let mut found_messages: HashMap<String, (Vec<String>, u32)> = HashMap::new();
    // 有的 jsonl 会把同一段行重复写一遍：同一个 uuid 只算一次
    let mut seen_rows: HashSet<String> = HashSet::new();

    for line in BufReader::new(fs::File::open(source)?).lines() {
        let line = line?;
        // 大文件里绝大多数行与章节无关：先做子串判断，命中才解析 JSON
        if !wanted_tools.iter().any(|id| line.contains(id))
            && !wanted_messages.iter().any(|id| line.contains(id))
        {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // 同样的 id 还会出现在 tool_result（user 行）等记录里，只认 assistant
        if record.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        let uuid = record
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !uuid.is_empty() && !seen_rows.insert(uuid.to_string()) {
            continue;
        }
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        fn block_type(block: &Value) -> Option<&str> {
            block.get("type").and_then(Value::as_str)
        }
        for block in blocks {
            if block_type(block) != Some("tool_use") {
                continue;
            }
            if let Some(id) = block.get("id").and_then(Value::as_str) {
                if wanted_tools.contains(id) {
                    found_tools.insert(id.to_string());
                }
            }
        }
        if let Some(message_id) = message.get("id").and_then(Value::as_str) {
            if wanted_messages.contains(message_id) {
                let entry = found_messages.entry(message_id.to_string()).or_default();
                if !uuid.is_empty() {
                    entry.0.push(uuid.to_string());
                }
                entry.1 += blocks
                    .iter()
                    .filter(|block| block_type(block) == Some("text"))
                    .count() as u32;
            }
        }
    }

    let mut resolved = Vec::new();
    let mut seen_chapters: HashSet<&str> = HashSet::new();
    for chapter in chapters {
        let anchor = if found_tools.contains(&chapter.after_id) {
            Some(ChapterAnchor::Tool {
                call_id: chapter.after_id.clone(),
            })
        } else {
            split_text_anchor(&chapter.after_id).and_then(|(message_id, text_index)| {
                let (message_ids, texts) = found_messages.get(message_id)?;
                (text_index < *texts && !message_ids.is_empty()).then(|| ChapterAnchor::Text {
                    message_ids: message_ids.clone(),
                    text_index,
                })
            })
        };
        if let Some(anchor) = anchor {
            // fork 出来的会话会带着父会话的章节记录，同一条章节只留一次
            if seen_chapters.insert(chapter.id.as_str()) {
                resolved.push(DesktopChapter {
                    id: chapter.id.clone(),
                    title: chapter.title.clone(),
                    anchor,
                });
            }
        }
    }
    Ok(resolved)
}

fn chapters_in(storage_dir: &Path, source: &Path) -> Vec<DesktopChapter> {
    if !storage_dir.is_dir() {
        return Vec::new();
    }
    let chapters = match load_store(storage_dir) {
        Ok(chapters) => chapters,
        Err(error) => {
            log::debug!("Skip Claude desktop chapters: {error}");
            return Vec::new();
        }
    };
    resolve(&chapters, source).unwrap_or_else(|error| {
        log::debug!("Skip Claude desktop chapters: {error}");
        Vec::new()
    })
}

/// 某个 Claude 会话文件里，用户在桌面端手动 pin 的章节。
/// 没装桌面端、格式变了、读失败：一律返回空。
pub fn chapters_for_source(source: &Path) -> Vec<DesktopChapter> {
    default_storage_dir()
        .map(|dir| chapters_in(&dir, source))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn utf16_value(text: &str) -> Vec<u8> {
        let mut value = vec![0u8];
        for unit in text.encode_utf16() {
            value.extend_from_slice(&unit.to_le_bytes());
        }
        value
    }

    fn latin1_value(text: &str) -> Vec<u8> {
        let mut value = vec![1u8];
        value.extend(text.bytes());
        value
    }

    fn write_storage(dir: &Path, value: &[u8]) {
        let options = Options {
            create_if_missing: true,
            ..Default::default()
        };
        let mut db = DB::open(dir, options).unwrap();
        db.put(STORAGE_KEY, value).unwrap();
        db.put(b"_https://claude.ai\x00\x01other-key", &latin1_value("x"))
            .unwrap();
        db.flush().unwrap();
    }

    fn store_json() -> String {
        json!({
            "state": { "bySession": {
                "local_a": { "renames": {}, "hidden": [], "userChapters": [
                    { "id": "uch-1", "afterId": "toolu_01A", "title": "配置恢复" },
                    { "id": "uch-2", "afterId": "msg_01B-t0", "title": "Order confirmed" },
                    { "id": "uch-3", "afterId": "msg_01B-t1", "title": "Second text" },
                    { "id": "uch-4", "afterId": "toolu_elsewhere", "title": "Other session" },
                    { "id": "uch-5", "afterId": "msg_01B-t9", "title": "Out of range" },
                    { "id": "uch-6", "title": "No anchor" }
                ] },
                // fork 带过来的同一条章节
                "local_b": { "userChapters": [
                    { "id": "uch-1", "afterId": "toolu_01A", "title": "配置恢复" }
                ] }
            } },
            "version": 0
        })
        .to_string()
    }

    fn write_session(path: &Path) {
        let assistant = |uuid: &str, id: &str, content: Value| {
            json!({ "type": "assistant", "uuid": uuid,
                "message": { "id": id, "role": "assistant", "content": content } })
        };
        let rows = [
            json!({ "type": "user", "uuid": "u1", "message": { "role": "user", "content": "hi" } }),
            assistant(
                "a1",
                "msg_01B",
                json!([{ "type": "thinking", "thinking": "…" }]),
            ),
            assistant(
                "a2",
                "msg_01B",
                json!([{ "type": "text", "text": "first" }]),
            ),
            assistant(
                "a3",
                "msg_01B",
                json!([
                    { "type": "text", "text": "second" },
                    { "type": "tool_use", "id": "toolu_01A", "name": "Bash", "input": {} }
                ]),
            ),
            // 同一行被重复写了一遍
            assistant(
                "a2",
                "msg_01B",
                json!([{ "type": "text", "text": "first" }]),
            ),
            // tool_result 里也有同一个 id，不能当锚点
            json!({ "type": "user", "uuid": "u2", "message": { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "toolu_elsewhere", "content": "ok" }
            ] } }),
        ];
        let mut file = fs::File::create(path).unwrap();
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
    }

    #[test]
    fn decodes_both_local_storage_encodings() {
        assert_eq!(
            decode_storage_value(&utf16_value("章节 a")).as_deref(),
            Some("章节 a")
        );
        assert_eq!(
            decode_storage_value(&latin1_value("plain")).as_deref(),
            Some("plain")
        );
        assert_eq!(decode_storage_value(&[7, 1, 2]), None);
        assert_eq!(decode_storage_value(&[]), None);
    }

    #[test]
    fn reads_chapters_from_a_copy_and_resolves_anchors() {
        let temp = tempfile::tempdir().unwrap();
        let storage = temp.path().join("leveldb");
        fs::create_dir(&storage).unwrap();
        write_storage(&storage, &utf16_value(&store_json()));
        // 桌面端运行时目录里有它持有的 LOCK
        fs::write(storage.join("LOCK"), b"").unwrap();
        let session = temp.path().join("session.jsonl");
        write_session(&session);

        let mut chapters = chapters_in(&storage, &session);
        chapters.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            chapters,
            vec![
                DesktopChapter {
                    id: "uch-1".into(),
                    title: "配置恢复".into(),
                    anchor: ChapterAnchor::Tool {
                        call_id: "toolu_01A".into()
                    },
                },
                DesktopChapter {
                    id: "uch-2".into(),
                    title: "Order confirmed".into(),
                    anchor: ChapterAnchor::Text {
                        message_ids: vec!["a1".into(), "a2".into(), "a3".into()],
                        text_index: 0,
                    },
                },
                DesktopChapter {
                    id: "uch-3".into(),
                    title: "Second text".into(),
                    anchor: ChapterAnchor::Text {
                        message_ids: vec!["a1".into(), "a2".into(), "a3".into()],
                        text_index: 1,
                    },
                },
            ]
        );
        assert_eq!(
            serde_json::to_value(&chapters[0]).unwrap(),
            json!({ "id": "uch-1", "title": "配置恢复",
                "anchor": { "kind": "tool", "callId": "toolu_01A" } })
        );
        assert_eq!(
            serde_json::to_value(&chapters[1].anchor).unwrap(),
            json!({ "kind": "text", "messageIds": ["a1", "a2", "a3"], "textIndex": 0 })
        );
    }

    #[test]
    fn latin1_store_is_read_too() {
        let temp = tempfile::tempdir().unwrap();
        let storage = temp.path().join("leveldb");
        fs::create_dir(&storage).unwrap();
        let json = json!({ "state": { "bySession": { "s": { "userChapters": [
            { "id": "uch-1", "afterId": "toolu_01A", "title": "ascii" }
        ] } } } })
        .to_string();
        write_storage(&storage, &latin1_value(&json));
        let session = temp.path().join("session.jsonl");
        write_session(&session);
        let chapters = chapters_in(&storage, &session);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "ascii");
    }

    #[test]
    fn missing_or_broken_storage_yields_no_chapters() {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("session.jsonl");
        write_session(&session);
        // 没装桌面端
        assert!(chapters_in(&temp.path().join("nope"), &session).is_empty());
        // 目录在，但不是 LevelDB
        let junk = temp.path().join("junk");
        fs::create_dir(&junk).unwrap();
        fs::write(junk.join("000003.log"), b"not leveldb").unwrap();
        assert!(chapters_in(&junk, &session).is_empty());
        // 键在，但内容不是预期的 JSON
        let odd = temp.path().join("odd");
        fs::create_dir(&odd).unwrap();
        write_storage(&odd, &utf16_value("{\"version\":3}"));
        assert!(chapters_in(&odd, &session).is_empty());
        // 库里没有这个键
        let empty = temp.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let options = Options {
            create_if_missing: true,
            ..Default::default()
        };
        DB::open(&empty, options).unwrap().flush().unwrap();
        assert!(chapters_in(&empty, &session).is_empty());
    }
}
