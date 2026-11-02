use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session_manager::model::ContentRef;
use crate::session_manager::SessionMeta;

/// Maximum number of characters for session titles (shared across providers).
pub const TITLE_MAX_CHARS: usize = 80;

static SCAN_IN_FLIGHT: Mutex<usize> = Mutex::new(0);
static SCAN_SLOT_FREED: Condvar = Condvar::new();

/// 一个读取会话文件的名额。各应用的扫描同时进行，这里按进程计数，
/// 设置里的并行数才是真正的总上限，而不是每个应用各开那么多。
struct ScanSlot;

impl ScanSlot {
    fn acquire(limit: usize) -> Self {
        let mut in_flight = SCAN_IN_FLIGHT
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while *in_flight >= limit {
            in_flight = SCAN_SLOT_FREED
                .wait(in_flight)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *in_flight += 1;
        Self
    }
}

impl Drop for ScanSlot {
    fn drop(&mut self) {
        let mut in_flight = SCAN_IN_FLIGHT
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *in_flight -= 1;
        // 各轮扫描的上限可能不同，唤醒全部，由各自重新判断
        SCAN_SLOT_FREED.notify_all();
    }
}

/// 用 `limit` 个线程对 `files` 逐个调用 `visit`，结果顺序与 `files` 一致。
fn map_files_parallel<T, F>(files: &[PathBuf], limit: usize, visit: &F) -> Vec<T>
where
    T: Send,
    F: Fn(&Path) -> Option<T> + Sync,
{
    if files.is_empty() {
        return Vec::new();
    }
    if limit <= 1 || files.len() == 1 {
        return files.iter().filter_map(|path| visit(path)).collect();
    }
    // 文件大小悬殊，按「谁空闲谁取下一个」分发，不预先切块
    let next = AtomicUsize::new(0);
    let worker = || {
        let mut found = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(index) else {
                break;
            };
            if let Some(value) = visit(path) {
                found.push((index, value));
            }
        }
        found
    };
    let found: Vec<Vec<(usize, T)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..limit.clamp(1, files.len()))
            .map(|_| scope.spawn(worker))
            .collect();
        workers
            .into_iter()
            .map(|handle| handle.join().unwrap_or_default())
            .collect()
    });
    // 文件下标已经是稠密的 0..N，直接归位即可，不必再做 O(N log N) 排序。
    let mut ordered: Vec<Option<T>> = (0..files.len()).map(|_| None).collect();
    for (index, value) in found.into_iter().flatten() {
        ordered[index] = Some(value);
    }
    ordered.into_iter().flatten().collect()
}

/// 并行解析 `files`，并行数取自设置。返回能解析出会话的那些，顺序与 `files` 一致；
/// 读不了的文件返回 `Err`，本轮跳过。
#[cfg(test)]
pub fn scan_files<F>(files: Vec<PathBuf>, parse: F) -> Vec<SessionMeta>
where
    F: Fn(&Path) -> io::Result<Option<SessionMeta>> + Sync,
{
    let limit = crate::settings::effective_session_scan_concurrency();
    map_files_parallel(&files, limit, &|path: &Path| {
        let _slot = ScanSlot::acquire(limit);
        match parse(path) {
            Ok(meta) => meta,
            Err(err) => {
                log::debug!("会话文件暂时读取失败 {}: {err}", path.display());
                None
            }
        }
    })
}

/// 会话文件解析结果的缓存：按文件的修改时间和大小判断有没有变，没变就复用上次
/// 解析出的 [`SessionMeta`]，只重新解析新增或改过的文件。
///
/// 只存摘要（标题、路径、时间等），不存会话正文；每次扫描都会用本轮看到的文件
/// 重建整张表，已删除文件的条目随之丢弃，内存占用和会话数量成正比（约每千个
/// 会话 1 MB）。解析结果必须只取决于文件本身——依赖外部数据的部分（如 Codex 的
/// 线程标题）要在拿到缓存结果后再叠加。
///
/// 用 [`FileParseCache::persistent`] 建的缓存还会存到磁盘，重启后第一次扫描也只解析
/// 改过的文件。磁盘缓存按应用版本区分，换版本（解析规则可能变了）时整份作废重建；
/// 开发构建不落盘，免得改了解析代码却读到旧结果。
pub struct FileParseCache {
    entries: Mutex<HashMap<PathBuf, CachedParse>>,
    /// 磁盘缓存文件；`None` 时只在内存里缓存
    store: Option<PathBuf>,
    /// 磁盘缓存是否已读进内存（第一次扫描时读）
    loaded: AtomicBool,
}

struct CachedParse {
    modified: Option<SystemTime>,
    len: u64,
    meta: Option<SessionMeta>,
}

/// 磁盘缓存的版本：应用版本 + 存储格式。任何一项不同，整份缓存作废。
const STORE_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "/1");

#[derive(Serialize, Deserialize)]
struct StoredCache {
    version: String,
    entries: Vec<StoredEntry>,
}

#[derive(Serialize, Deserialize)]
struct StoredEntry {
    path: String,
    /// 修改时间：UNIX 纪元以来的 (秒, 纳秒)
    modified: Option<(u64, u32)>,
    len: u64,
    meta: Option<SessionMeta>,
}

fn encode_time(time: SystemTime) -> Option<(u64, u32)> {
    let since = time.duration_since(UNIX_EPOCH).ok()?;
    Some((since.as_secs(), since.subsec_nanos()))
}

fn decode_time((secs, nanos): (u64, u32)) -> Option<SystemTime> {
    UNIX_EPOCH.checked_add(Duration::new(secs, nanos))
}

impl FileParseCache {
    pub fn new() -> Self {
        Self::with_store(None)
    }

    /// 存到磁盘的缓存（应用配置目录下的 `cache/session-list/{name}.json`）。开发构建只用内存。
    pub fn persistent(name: &str) -> Self {
        let store = (!cfg!(debug_assertions)).then(|| {
            crate::config::get_app_config_dir()
                .join("cache")
                .join("session-list")
                .join(format!("{name}.json"))
        });
        Self::with_store(store)
    }

    pub(crate) fn with_store(store: Option<PathBuf>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            store,
            loaded: AtomicBool::new(false),
        }
    }

    fn load_store(store: &Path) -> HashMap<PathBuf, CachedParse> {
        let Ok(bytes) = std::fs::read(store) else {
            return HashMap::new();
        };
        let stored = match serde_json::from_slice::<StoredCache>(&bytes) {
            Ok(stored) if stored.version == STORE_VERSION => stored,
            Ok(_) => return HashMap::new(),
            Err(err) => {
                log::debug!("会话列表缓存读取失败，重建 {}: {err}", store.display());
                return HashMap::new();
            }
        };
        stored
            .entries
            .into_iter()
            .filter_map(|entry| {
                let modified = match entry.modified {
                    Some(time) => Some(decode_time(time)?),
                    None => None,
                };
                Some((
                    PathBuf::from(entry.path),
                    CachedParse {
                        modified,
                        len: entry.len,
                        meta: entry.meta,
                    },
                ))
            })
            .collect()
    }

    fn save_store(store: &Path, entries: &HashMap<PathBuf, CachedParse>) {
        let stored = StoredCache {
            version: STORE_VERSION.to_string(),
            entries: entries
                .iter()
                .filter_map(|(path, entry)| {
                    let modified = match entry.modified {
                        Some(time) => Some(encode_time(time)?),
                        None => None,
                    };
                    Some(StoredEntry {
                        // 非 UTF-8 路径存不进 JSON，只留在内存里
                        path: path.to_str()?.to_string(),
                        modified,
                        len: entry.len,
                        meta: entry.meta.clone(),
                    })
                })
                .collect(),
        };
        let result = serde_json::to_vec(&stored)
            .map_err(|err| err.to_string())
            .and_then(|bytes| {
                if let Some(parent) = store.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
                // 缓存里有会话标题、首条提问，按私有文件写
                crate::config::atomic_write_private(store, &bytes).map_err(|err| err.to_string())
            });
        if let Err(err) = result {
            log::debug!("会话列表缓存写入失败 {}: {err}", store.display());
        }
    }

    /// 解析 `files`，没变过的文件直接用缓存，其余并行解析（并行数取自设置）。
    /// 返回能解析出会话的那些，顺序与 `files` 一致。
    pub fn scan<F>(&self, files: Vec<PathBuf>, parse: F) -> Vec<SessionMeta>
    where
        F: Fn(&Path) -> io::Result<Option<SessionMeta>> + Sync,
    {
        self.scan_with_limit(
            files,
            crate::settings::effective_session_scan_concurrency(),
            parse,
        )
    }

    fn scan_with_limit<F>(&self, files: Vec<PathBuf>, limit: usize, parse: F) -> Vec<SessionMeta>
    where
        F: Fn(&Path) -> io::Result<Option<SessionMeta>> + Sync,
    {
        // 锁中毒（上次扫描 panic）时丢掉旧缓存重来，不影响本次结果
        // 同一缓存的重叠刷新串行复用结果，避免后一次扫描拿到空缓存重复解析。
        // 不同 provider 的缓存仍可并行扫描。
        let mut guard = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                self.entries.clear_poison();
                guard
            }
        };
        if let Some(store) = &self.store {
            if !self.loaded.swap(true, Ordering::AcqRel) {
                *guard = Self::load_store(store);
            }
        }
        let mut previous = std::mem::take(&mut *guard);

        let scanned = map_files_parallel(&files, limit, &|path: &Path| {
            let file_meta = std::fs::metadata(path).ok()?;
            let (modified, len) = (file_meta.modified().ok(), file_meta.len());
            let meta = match previous.get(path) {
                Some(entry) if entry.modified == modified && entry.len == len => {
                    // 命中项在归并时直接移入下一轮缓存，避免把所有字符串复制两次。
                    return Some((path.to_path_buf(), None));
                }
                _ => {
                    let _slot = ScanSlot::acquire(limit);
                    match parse(path) {
                        Ok(meta) => meta,
                        // 读不了（权限、被占用等）不缓存：否则恢复读取后文件没变，
                        // 会一直命中「没有会话」，直到文件改动或重启
                        Err(err) => {
                            log::debug!("会话文件暂时读取失败，下轮重试 {}: {err}", path.display());
                            return None;
                        }
                    }
                }
            };
            Some((
                path.to_path_buf(),
                Some(CachedParse {
                    modified,
                    len,
                    meta,
                }),
            ))
        });

        let mut next = HashMap::with_capacity(scanned.len());
        let mut sessions = Vec::new();
        let mut changed = false;
        for (path, entry) in scanned {
            changed |= entry.is_some();
            let entry = entry.unwrap_or_else(|| {
                previous
                    .remove(&path)
                    .or_else(|| next.remove(&path))
                    .expect("cached file")
            });
            if let Some(meta) = &entry.meta {
                sessions.push(meta.clone());
            }
            next.insert(path, entry);
        }

        // 剩下的是本轮没再出现的文件（已删除）
        changed |= !previous.is_empty();
        if changed {
            if let Some(store) = &self.store {
                Self::save_store(store, &next);
            }
        }
        *guard = next;
        sessions
    }
}

impl Default for FileParseCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 递归收集 `root` 下 `keep` 认可的文件。类型直接取自目录枚举，不为每个条目再查一次；
/// 符号链接（含 Windows 目录联接）枚举不出目标类型，才单独查。
pub fn collect_files(root: &Path, files: &mut Vec<PathBuf>, keep: &dyn Fn(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() || (file_type.is_symlink() && path.is_dir()) {
            collect_files(&path, files, keep);
        } else if keep(&path) {
            files.push(path);
        }
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

    fn session_ids(sessions: Vec<SessionMeta>) -> Vec<String> {
        sessions.into_iter().map(|meta| meta.session_id).collect()
    }

    fn numbered_files(dir: &Path, count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let path = dir.join(format!("{i}.jsonl"));
                std::fs::write(&path, format!("session-{i}")).unwrap();
                path
            })
            .collect()
    }

    /// 用磁盘缓存 `store` 扫描一遍，返回 (会话 ID, 实际解析的文件数)
    fn scan_stored(store: &Path, files: &[PathBuf]) -> (Vec<String>, usize) {
        let parsed = AtomicUsize::new(0);
        let cache = FileParseCache::with_store(Some(store.to_path_buf()));
        let sessions = cache.scan_with_limit(files.to_vec(), 2, |path: &Path| {
            parsed.fetch_add(1, Ordering::Relaxed);
            if path.extension().and_then(|ext| ext.to_str()) == Some("locked") {
                return Err(io::Error::other("locked"));
            }
            Ok(meta_from_file(path))
        });
        (session_ids(sessions), parsed.into_inner())
    }

    #[test]
    fn persistent_cache_survives_a_restart_and_only_reparses_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("cache").join("list.json");
        let mut files = numbered_files(dir.path(), 8);

        let (first, parsed) = scan_stored(&store, &files);
        assert_eq!(parsed, 8);
        // 新实例相当于重启：全部从磁盘命中，结果不变
        assert_eq!(scan_stored(&store, &files), (first.clone(), 0));

        // 改一个、删一个、加一个：只解析改过和新增的
        std::fs::write(&files[3], "session-3-edited").unwrap();
        let removed = files.remove(5);
        std::fs::remove_file(&removed).unwrap();
        let added = dir.path().join("added.jsonl");
        std::fs::write(&added, "session-added").unwrap();
        files.push(added);
        let (ids, parsed) = scan_stored(&store, &files);
        assert_eq!(parsed, 2);
        assert!(ids.contains(&"session-3-edited".to_string()));
        assert!(!ids.contains(&"session-5".to_string()));
        assert_eq!(scan_stored(&store, &files), (ids, 0));
    }

    #[test]
    fn persistent_cache_retries_unreadable_files_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("list.json");
        let locked = dir.path().join("busy.locked");
        std::fs::write(&locked, "session-busy").unwrap();
        let files = vec![locked];
        assert_eq!(scan_stored(&store, &files).1, 1);
        assert_eq!(
            scan_stored(&store, &files).1,
            1,
            "errors must not be stored"
        );
    }

    #[test]
    fn persistent_cache_ignores_other_versions_and_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("list.json");
        let files = numbered_files(dir.path(), 3);
        scan_stored(&store, &files);

        let mut stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
        stored["version"] = serde_json::json!("0.0.0/1");
        std::fs::write(&store, serde_json::to_vec(&stored).unwrap()).unwrap();
        assert_eq!(scan_stored(&store, &files).1, 3);

        std::fs::write(&store, b"{not json").unwrap();
        assert_eq!(scan_stored(&store, &files).1, 3);
        assert_eq!(scan_stored(&store, &files).1, 0, "rewritten after rebuild");
    }

    #[test]
    fn persistent_cache_is_not_rewritten_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("list.json");
        let files = numbered_files(dir.path(), 3);
        let cache = FileParseCache::with_store(Some(store.clone()));
        cache.scan_with_limit(files.clone(), 2, |path: &Path| Ok(meta_from_file(path)));
        std::fs::write(&store, b"sentinel").unwrap();
        cache.scan_with_limit(files, 2, |path: &Path| Ok(meta_from_file(path)));
        assert_eq!(std::fs::read(&store).unwrap(), b"sentinel");
    }

    #[test]
    fn scan_keeps_input_order_and_skips_non_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = numbered_files(dir.path(), 64);
        let skipped = dir.path().join("skip.jsonl");
        std::fs::write(&skipped, "skip").unwrap();
        files.insert(10, skipped);
        let expected: Vec<String> = (0..64).map(|i| format!("session-{i}")).collect();

        for limit in [1, 3, 8, 200] {
            let cache = FileParseCache::new();
            let sessions =
                cache.scan_with_limit(files.clone(), limit, |path: &Path| Ok(meta_from_file(path)));
            assert_eq!(session_ids(sessions), expected, "limit={limit}");
        }
    }

    #[test]
    fn scan_never_exceeds_the_configured_concurrency() {
        let dir = tempfile::tempdir().unwrap();
        let files = numbered_files(dir.path(), 32);
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        // 两个缓存同时扫描共用一个上限，模拟多个应用同时扫描
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    FileParseCache::new().scan_with_limit(files.clone(), 2, |path: &Path| {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        running.fetch_sub(1, Ordering::SeqCst);
                        Ok(meta_from_file(path))
                    });
                });
            }
        });

        assert!(peak.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn overlapping_scans_share_the_completed_cache() {
        let dir = tempfile::tempdir().unwrap();
        let files = numbered_files(dir.path(), 32);
        let cache = FileParseCache::new();
        let calls = AtomicUsize::new(0);
        let start = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        cache.scan_with_limit(files.clone(), 4, |path| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(1));
                            Ok(meta_from_file(path))
                        })
                    })
                })
                .collect();
            for handle in handles {
                assert_eq!(handle.join().unwrap().len(), files.len());
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), files.len());
    }

    #[test]
    fn warm_scan_preserves_duplicate_input_paths() {
        let dir = tempfile::tempdir().unwrap();
        let files = numbered_files(dir.path(), 2);
        let cache = FileParseCache::new();
        cache.scan_with_limit(files.clone(), 2, |path| Ok(meta_from_file(path)));
        let repeated = vec![files[0].clone(), files[1].clone(), files[0].clone()];
        let sessions = cache.scan_with_limit(repeated, 2, |_| panic!("all files are cached"));
        assert_eq!(
            session_ids(sessions),
            ["session-0", "session-1", "session-0"]
        );
    }

    #[test]
    fn cache_can_be_reused_after_a_serial_parser_panics() {
        let dir = tempfile::tempdir().unwrap();
        let files = numbered_files(dir.path(), 1);
        let cache = FileParseCache::new();
        assert!(std::panic::catch_unwind(|| {
            cache.scan_with_limit(files.clone(), 1, |_| panic!("parser failed"));
        })
        .is_err());
        assert_eq!(
            cache
                .scan_with_limit(files.clone(), 1, |path| Ok(meta_from_file(path)))
                .len(),
            1
        );
        assert_eq!(
            cache
                .scan_with_limit(files, 1, |_| panic!("recovered entry should be cached"))
                .len(),
            1
        );
    }

    /// 读取失败不能缓存成「没有会话」：恢复读取后，文件没变也要重新解析出来
    #[test]
    fn file_parse_cache_retries_files_that_failed_to_read() {
        use std::sync::atomic::AtomicBool;

        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        std::fs::write(&a, "alpha").unwrap();

        let cache = FileParseCache::new();
        let fail = AtomicBool::new(true);
        let calls = AtomicUsize::new(0);
        let parse = |path: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            if fail.load(Ordering::SeqCst) {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            } else {
                Ok(meta_from_file(path))
            }
        };

        assert!(cache.scan_with_limit(vec![a.clone()], 2, parse).is_empty());
        assert!(cache.scan_with_limit(vec![a.clone()], 2, parse).is_empty());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "失败的文件每轮都要重试，不能命中缓存"
        );

        fail.store(false, Ordering::SeqCst);
        let sessions = cache.scan_with_limit(vec![a.clone()], 2, parse);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "alpha");
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // 成功后才进缓存：再扫不重新解析
        assert_eq!(cache.scan_with_limit(vec![a], 2, parse).len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn file_parse_cache_reparses_only_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        let c = dir.path().join("c.jsonl");
        std::fs::write(&a, "alpha").unwrap();
        std::fs::write(&b, "beta").unwrap();
        std::fs::write(&c, "skip").unwrap();

        let cache = FileParseCache::new();
        let calls = AtomicUsize::new(0);
        let parse = |path: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(meta_from_file(path))
        };
        let scan = |files: &[PathBuf]| session_ids(cache.scan_with_limit(files.to_vec(), 4, parse));
        let count = || calls.load(Ordering::SeqCst);

        // 第一次全部解析；解析不出会话的文件也记下来，下次不重复解析
        let files = vec![a.clone(), b.clone(), c.clone()];
        assert_eq!(scan(&files), ["alpha", "beta"]);
        assert_eq!(count(), 3);

        // 没有改动：一个都不重新解析
        assert_eq!(scan(&files), ["alpha", "beta"]);
        assert_eq!(count(), 3);

        // 改了内容（长度变了）：只重新解析这一个
        std::fs::write(&b, "beta-2").unwrap();
        assert_eq!(scan(&files), ["alpha", "beta-2"]);
        assert_eq!(count(), 4);

        // 文件删掉：不再出现，条目也被丢弃；重新出现时要重新解析
        std::fs::remove_file(&a).unwrap();
        assert_eq!(scan(&files), ["beta-2"]);
        assert_eq!(count(), 4);
        std::fs::write(&a, "alpha").unwrap();
        assert_eq!(scan(&files), ["alpha", "beta-2"]);
        assert_eq!(count(), 5);
    }

    #[test]
    fn scan_files_skips_unreadable_files_and_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let files = numbered_files(dir.path(), 8);
        let denied = files[3].clone();

        let sessions = scan_files(files, |path: &Path| {
            if path == denied {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
            }
            Ok(meta_from_file(path))
        });

        let expected: Vec<String> = [0, 1, 2, 4, 5, 6, 7]
            .iter()
            .map(|i| format!("session-{i}"))
            .collect();
        assert_eq!(session_ids(sessions), expected);
    }

    #[test]
    fn collect_files_recurses_and_applies_the_filter() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("top.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("note.txt"), "").unwrap();
        std::fs::write(nested.join("deep.jsonl"), "").unwrap();

        let mut files = Vec::new();
        collect_files(dir.path(), &mut files, &|path| {
            path.extension().is_some_and(|ext| ext == "jsonl")
        });
        files.sort();

        assert_eq!(
            files,
            [nested.join("deep.jsonl"), dir.path().join("top.jsonl")]
        );
    }

    #[test]
    fn collect_files_ignores_a_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        collect_files(&dir.path().join("missing"), &mut files, &|_| true);
        assert!(files.is_empty());
    }

    /// 符号链接的项目目录要继续往里走，否则链接过去的会话会从列表消失
    #[cfg(unix)]
    #[test]
    fn collect_files_follows_directory_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(target.path().join("linked.jsonl"), "").unwrap();
        std::os::unix::fs::symlink(target.path(), dir.path().join("project")).unwrap();

        let mut files = Vec::new();
        collect_files(dir.path(), &mut files, &|_| true);

        assert_eq!(files, [dir.path().join("project").join("linked.jsonl")]);
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
