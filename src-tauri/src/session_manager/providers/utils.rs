use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use crate::session_manager::model::ContentRef;
use crate::session_manager::SessionMeta;

/// Maximum number of characters for session titles (shared across providers).
pub const TITLE_MAX_CHARS: usize = 80;

/// 会话文件解析结果的缓存：按文件的修改时间和大小判断有没有变，没变就复用上次
/// 解析出的 [`SessionMeta`]，只重新解析新增或改过的文件。
///
/// 只存摘要（标题、路径、时间等），不存会话正文；每次扫描都会用本轮看到的文件
/// 重建整张表，已删除文件的条目随之丢弃，内存占用和会话数量成正比（约每千个
/// 会话 1 MB）。解析结果必须只取决于文件本身——依赖外部数据的部分（如 Codex 的
/// 线程标题）要在拿到缓存结果后再叠加。
pub struct FileParseCache {
    entries: Mutex<HashMap<PathBuf, CachedParse>>,
}

struct CachedParse {
    modified: Option<SystemTime>,
    len: u64,
    meta: Option<SessionMeta>,
}

impl FileParseCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 按顺序解析 `files`，没变过的文件直接用缓存。返回能解析出会话的那些。
    pub fn scan<F>(&self, files: Vec<PathBuf>, parse: F) -> Vec<SessionMeta>
    where
        F: Fn(&Path) -> io::Result<Option<SessionMeta>>,
    {
        // 锁中毒（上次扫描 panic）时丢掉旧缓存重来，不影响本次结果
        let mut previous = match self.entries.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                HashMap::new()
            }
        };

        let mut next = HashMap::with_capacity(files.len());
        let mut sessions = Vec::new();
        for path in files {
            let (modified, len) = match std::fs::metadata(&path) {
                Ok(meta) => (meta.modified().ok(), meta.len()),
                Err(_) => continue,
            };
            let meta = match previous.remove(&path) {
                Some(entry) if entry.modified == modified && entry.len == len => entry.meta,
                _ => match parse(&path) {
                    Ok(meta) => meta,
                    // 读不了（权限、被占用等）不缓存：否则恢复读取后文件没变，
                    // 会一直命中「没有会话」，直到文件改动或重启
                    Err(err) => {
                        log::debug!("会话文件暂时读取失败，下轮重试 {}: {err}", path.display());
                        continue;
                    }
                },
            };
            if let Some(meta) = &meta {
                sessions.push(meta.clone());
            }
            next.insert(
                path,
                CachedParse {
                    modified,
                    len,
                    meta,
                },
            );
        }

        if let Ok(mut guard) = self.entries.lock() {
            *guard = next;
        }
        sessions
    }
}

impl Default for FileParseCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the first `head_n` lines and last `tail_n` lines from a file.
/// For small files (< 16 KB), reads all lines once to avoid unnecessary seeking.
pub fn read_head_tail_lines(
    path: &Path,
    head_n: usize,
    tail_n: usize,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();

    // For small files, read all lines once and split
    if file_len < 16_384 {
        let reader = BufReader::new(file);
        let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let head = all.iter().take(head_n).cloned().collect();
        let skip = all.len().saturating_sub(tail_n);
        let tail = all.into_iter().skip(skip).collect();
        return Ok((head, tail));
    }

    // Read head lines from the beginning
    let reader = BufReader::new(file);
    let head: Vec<String> = reader.lines().take(head_n).map_while(Result::ok).collect();

    // Seek to last ~16 KB for tail lines
    let seek_pos = file_len.saturating_sub(16_384);
    let mut file2 = File::open(path)?;
    file2.seek(SeekFrom::Start(seek_pos))?;
    let tail_reader = BufReader::new(file2);
    let all_tail: Vec<String> = tail_reader.lines().map_while(Result::ok).collect();

    // Skip first partial line if we seeked into the middle of a line
    let skip_first = if seek_pos > 0 { 1 } else { 0 };
    let usable: Vec<String> = all_tail.into_iter().skip(skip_first).collect();
    let skip = usable.len().saturating_sub(tail_n);
    let tail = usable.into_iter().skip(skip).collect();

    Ok((head, tail))
}

pub fn parse_timestamp_to_ms(value: &Value) -> Option<i64> {
    // Integer: milliseconds (>1e12) or seconds
    if let Some(n) = value.as_i64() {
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    if let Some(n) = value.as_f64() {
        let n = n as i64;
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    // RFC3339 string
    let raw = value.as_str()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt: DateTime<FixedOffset>| dt.timestamp_millis())
}

pub fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");

    // Anthropic uses tool_use; Pi's assistant messages use toolCall.
    if matches!(item_type, "tool_use" | "toolCall") {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Some(format!("[Tool: {name}]"));
    }

    // tool_result: extract nested content
    if item_type == "tool_result" {
        if let Some(content) = item.get("content") {
            let text = extract_text(content);
            if !text.is_empty() {
                return Some(text);
            }
        }
        return None;
    }

    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("input_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(content) = item.get("content") {
        let text = extract_text(content);
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

pub fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut result = trimmed.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

/// 一行 JSONL 在文件里的字节区间（含行尾 `\n`），生成 [`ContentRef::Jsonl`] 用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JsonlSpan {
    /// 行首的字节偏移
    pub offset: u64,
    /// 整行字节数，含行尾 `\n`（文件最后一行可能没有）
    pub len: u32,
}

impl JsonlSpan {
    /// 指向本行内 `pointer`（RFC 6901）处内容的引用
    pub fn content_ref(&self, pointer: impl Into<String>) -> ContentRef {
        ContentRef::Jsonl {
            offset: self.offset,
            len: self.len,
            pointer: pointer.into(),
        }
    }
}

/// JSONL 的一行及其字节区间。
pub struct LineSpan<'a> {
    pub span: JsonlSpan,
    /// 行内容，已去掉行尾 `\r\n` / `\n`
    pub bytes: &'a [u8],
}

/// 用 `read_until(b'\n')` 逐行读取并累计字节偏移，供需要回溯原文的解析器使用
/// （所有 JSONL 解析器共用这一份，`ContentRef::Jsonl` 的区间口径因此一致）。
///
/// 复用同一块缓冲区，不做 UTF-8 校验（交给 `serde_json::from_slice`），
/// 所以比 `BufRead::lines()` 少一次分配和一次校验。
pub struct LineSpans<R> {
    reader: R,
    buf: Vec<u8>,
    offset: u64,
}

impl<R: BufRead> LineSpans<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: Vec::with_capacity(64 * 1024),
            offset: 0,
        }
    }

    /// 读取下一行；到文件末尾返回 `Ok(None)`。
    pub fn next_line(&mut self) -> io::Result<Option<LineSpan<'_>>> {
        self.buf.clear();
        let read = self.reader.read_until(b'\n', &mut self.buf)?;
        if read == 0 {
            return Ok(None);
        }
        let offset = self.offset;
        self.offset += read as u64;

        let mut end = self.buf.len();
        if end > 0 && self.buf[end - 1] == b'\n' {
            end -= 1;
            if end > 0 && self.buf[end - 1] == b'\r' {
                end -= 1;
            }
        }
        Ok(Some(LineSpan {
            span: JsonlSpan {
                offset,
                len: u32::try_from(read).unwrap_or(u32::MAX),
            },
            bytes: &self.buf[..end],
        }))
    }
}

/// 逐行读取 JSONL，跳过空行和无法解析的行，回调拿到 (字节区间, JSON)。
/// 回调返回 `Err` 时立即停止并向上传递。
pub fn for_each_jsonl_value(
    path: &Path,
    mut f: impl FnMut(JsonlSpan, Value) -> Result<(), String>,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let mut lines = LineSpans::new(BufReader::new(file));
    while let Some(line) = lines
        .next_line()
        .map_err(|e| format!("Failed to read session file: {e}"))?
    {
        let bytes = line.bytes.trim_ascii();
        if bytes.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            continue;
        };
        f(line.span, value)?;
    }
    Ok(())
}

pub fn path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches(['/', '\\']);
    let last = normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|segment| !segment.is_empty())?;
    Some(last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta_from_file(path: &Path) -> Option<SessionMeta> {
        let text = std::fs::read_to_string(path).ok()?;
        if text.trim() == "skip" {
            return None;
        }
        Some(SessionMeta {
            provider_id: "test".to_string(),
            session_id: text.trim().to_string(),
            title: None,
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: Some(path.to_string_lossy().to_string()),
            resume_command: None,
        })
    }

    /// 读取失败不能缓存成「没有会话」：恢复读取后，文件没变也要重新解析出来
    #[test]
    fn file_parse_cache_retries_files_that_failed_to_read() {
        use std::cell::Cell;

        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        std::fs::write(&a, "alpha").unwrap();

        let cache = FileParseCache::new();
        let fail = Cell::new(true);
        let calls = Cell::new(0);
        let parse = |path: &Path| {
            calls.set(calls.get() + 1);
            if fail.get() {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            } else {
                Ok(meta_from_file(path))
            }
        };

        assert!(cache.scan(vec![a.clone()], parse).is_empty());
        assert!(cache.scan(vec![a.clone()], parse).is_empty());
        assert_eq!(calls.get(), 2, "失败的文件每轮都要重试，不能命中缓存");

        fail.set(false);
        let sessions = cache.scan(vec![a.clone()], parse);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "alpha");
        assert_eq!(calls.get(), 3);

        // 成功后才进缓存：再扫不重新解析
        assert_eq!(cache.scan(vec![a], parse).len(), 1);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn file_parse_cache_reparses_only_changed_files() {
        use std::cell::Cell;

        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        let c = dir.path().join("c.jsonl");
        std::fs::write(&a, "alpha").unwrap();
        std::fs::write(&b, "beta").unwrap();
        std::fs::write(&c, "skip").unwrap();

        let cache = FileParseCache::new();
        let calls = Cell::new(0);
        let parse = |path: &Path| {
            calls.set(calls.get() + 1);
            Ok(meta_from_file(path))
        };
        let ids = |sessions: Vec<SessionMeta>| -> Vec<String> {
            sessions.into_iter().map(|m| m.session_id).collect()
        };

        // 第一次全部解析；解析不出会话的文件也记下来，下次不重复解析
        let files = vec![a.clone(), b.clone(), c.clone()];
        assert_eq!(ids(cache.scan(files.clone(), parse)), ["alpha", "beta"]);
        assert_eq!(calls.get(), 3);

        // 没有改动：一个都不重新解析
        assert_eq!(ids(cache.scan(files.clone(), parse)), ["alpha", "beta"]);
        assert_eq!(calls.get(), 3);

        // 改了内容（长度变了）：只重新解析这一个
        std::fs::write(&b, "beta-2").unwrap();
        assert_eq!(ids(cache.scan(files.clone(), parse)), ["alpha", "beta-2"]);
        assert_eq!(calls.get(), 4);

        // 文件删掉：不再出现，条目也被丢弃；重新出现时要重新解析
        std::fs::remove_file(&a).unwrap();
        assert_eq!(ids(cache.scan(files.clone(), parse)), ["beta-2"]);
        assert_eq!(calls.get(), 4);
        std::fs::write(&a, "alpha").unwrap();
        assert_eq!(ids(cache.scan(files, parse)), ["alpha", "beta-2"]);
        assert_eq!(calls.get(), 5);
    }

    #[test]
    fn parse_timestamp_to_ms_supports_integers_and_rfc3339() {
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_033_i64)),
            Some(1_771_061_953_033)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_i64)),
            Some(1_771_061_953_000)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!("1970-01-01T00:00:01Z")),
            Some(1_000)
        );
    }

    #[test]
    fn line_spans_report_byte_offsets() {
        let data = "{\"a\":1}\r\n\n{\"b\":\"字\"}\n{\"c\":3}";
        let mut spans = LineSpans::new(io::Cursor::new(data.as_bytes()));
        let mut got = Vec::new();
        while let Some(span) = spans.next_line().unwrap() {
            got.push((
                span.span.offset,
                span.span.len,
                String::from_utf8(span.bytes.to_vec()).unwrap(),
            ));
        }
        assert_eq!(
            got,
            [
                (0, 9, "{\"a\":1}".to_string()),
                (9, 1, String::new()),
                (10, 12, "{\"b\":\"字\"}".to_string()),
                (22, 7, "{\"c\":3}".to_string()),
            ]
        );
        // 偏移 + 长度能切回原行
        assert_eq!(&data.as_bytes()[10..10 + 12], "{\"b\":\"字\"}\n".as_bytes());
    }

    #[test]
    fn jsonl_reader_reports_byte_spans() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "{\"a\":1}\n\n{bad\n{\"b\":\"字\"}").unwrap();
        let mut seen = Vec::new();
        for_each_jsonl_value(&path, |span, value| {
            seen.push((span, value));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, JsonlSpan { offset: 0, len: 8 });
        // 第二条：前面有 8 + 1 + 5 字节，末行无换行
        assert_eq!(
            seen[1].0,
            JsonlSpan {
                offset: 14,
                len: 11
            }
        );
        assert_eq!(seen[1].1, json!({"b":"字"}));
    }

    #[test]
    fn extract_text_supports_pi_tool_calls() {
        assert_eq!(
            extract_text(&json!([{ "type": "toolCall", "name": "read" }])),
            "[Tool: read]"
        );
    }
}
