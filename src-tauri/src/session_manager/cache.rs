//! 会话解析结果缓存 `TranscriptCache`。
//!
//! 打开同一个会话（切换回来、流式读取后再取全文、复制整段）时不必重新解析。
//! 命中条件与 `FileParseCache` 一致：源的 `(mtime, len)` 没变；容量按 LRU 控制在
//! 8 个会话、合计约 96MB（以序列化长度估算）以内。

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::SystemTime;

use super::content::{SourceLocation, ValidatedSource, GROK_CHAT_HISTORY};
use super::model::{EventKind, SessionBlock, SessionMessage, ToolStatus, TurnIndex};

/// 最多缓存的会话数
pub const MAX_ENTRIES: usize = 8;
/// 缓存内所有会话序列化后的合计字节上限
pub const MAX_TOTAL_BYTES: usize = 96 * 1024 * 1024;
/// 提问目录预览的最大字符数
const QUESTION_PREVIEW_CHARS: usize = 80;

/// 一次完整解析的结果。
#[derive(Debug, Default)]
pub struct Transcript {
    pub messages: Vec<SessionMessage>,
    pub turns: Vec<TurnIndex>,
    /// 全部消息序列化后的字节数（缓存容量估算与 `Done.payload_bytes` 用）
    pub approx_bytes: usize,
    /// 每条消息序列化后的字节数，流式分块时按它切包，避免重复序列化
    pub message_bytes: Vec<usize>,
}

impl Transcript {
    pub fn new(messages: Vec<SessionMessage>) -> Self {
        let message_bytes: Vec<usize> = messages
            .iter()
            .map(|message| serde_json::to_vec(message).map(|v| v.len()).unwrap_or(0))
            .collect();
        let approx_bytes = message_bytes.iter().sum();
        let turns = build_turns(&messages);
        Self {
            messages,
            turns,
            approx_bytes,
            message_bytes,
        }
    }
}

/// 源的指纹：修改时间 + 长度。目录 / SQLite 源取多个文件的合成值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

impl Fingerprint {
    fn of_metadata(meta: &fs::Metadata) -> Self {
        Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        }
    }

    /// 合并另一个文件：取较新的 mtime，长度累加
    fn merge(&mut self, other: Fingerprint) {
        self.modified = match (self.modified, other.modified) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.len = self.len.wrapping_add(other.len);
    }
}

/// 计算源的指纹。
///
/// - 普通文件：自身的 mtime / len
/// - 目录（OpenCode 旧版消息目录）：目录本身 + 一级子项；子项增删或改写都会改变指纹
/// - SQLite：数据库文件 + `-wal` 文件（WAL 模式下新写入先落在 wal 里，主库 mtime 不变）
pub fn fingerprint(location: &SourceLocation) -> io::Result<Fingerprint> {
    match location {
        SourceLocation::Path { path, .. } => {
            let meta = fs::metadata(path)?;
            let mut fp = Fingerprint::of_metadata(&meta);
            if meta.is_dir() {
                for entry in fs::read_dir(path)?.flatten() {
                    if let Ok(child) = entry.metadata() {
                        fp.merge(Fingerprint::of_metadata(&child));
                        // 子项个数也计入，避免「删一个、加一个同样大小」时指纹不变
                        fp.len = fp.len.wrapping_add(1);
                    }
                }
            }
            Ok(fp)
        }
        SourceLocation::Sqlite { db, .. } => {
            let mut fp = Fingerprint::of_metadata(&fs::metadata(db)?);
            if let Some(wal) = sqlite_wal_path(db) {
                if let Ok(meta) = fs::metadata(wal) {
                    fp.merge(Fingerprint::of_metadata(&meta));
                }
            }
            Ok(fp)
        }
    }
}

/// 会话源的指纹：在 [`fingerprint`] 的基础上补上「正文不在 sourcePath 里」的情况。
///
/// Grok Build 的 sourcePath 是 `summary.json`，正文在同目录的 `chat_history.jsonl`，
/// 而 `summary.json` 只在一轮结束时改写——只看它的话，一轮进行中刷新会命中旧缓存。
pub fn source_fingerprint(source: &ValidatedSource) -> io::Result<Fingerprint> {
    let mut fp = fingerprint(&source.location)?;
    if source.provider_id == "grokbuild" {
        if let SourceLocation::Path { path, .. } = &source.location {
            let history = path.with_file_name(GROK_CHAT_HISTORY);
            if let Ok(meta) = fs::metadata(&history) {
                fp.merge(Fingerprint::of_metadata(&meta));
            }
        }
    }
    Ok(fp)
}

fn sqlite_wal_path(db: &Path) -> Option<std::path::PathBuf> {
    let name = db.file_name()?.to_string_lossy().into_owned();
    Some(db.with_file_name(format!("{name}-wal")))
}

/// 缓存键：(provider_id, sourcePath)。sourcePath 用前端传来的原值，与删除时一致。
pub type CacheKey = (String, String);

struct Entry {
    key: CacheKey,
    fingerprint: Fingerprint,
    transcript: Arc<Transcript>,
}

/// 按 `(mtime, len)` 判定新鲜度的 LRU 缓存；队尾为最近使用。
pub struct TranscriptCache {
    entries: Mutex<VecDeque<Entry>>,
    max_entries: usize,
    max_bytes: usize,
}

impl TranscriptCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries,
            max_bytes,
        }
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<Entry>> {
        // 锁中毒（上次持锁时 panic）时丢掉旧缓存重来
        self.entries.lock().unwrap_or_else(|poisoned| {
            let mut guard = poisoned.into_inner();
            guard.clear();
            guard
        })
    }

    /// 指纹一致时返回缓存并把它移到队尾；指纹不一致时顺手移除过期项。
    pub fn get(&self, key: &CacheKey, fingerprint: Fingerprint) -> Option<Arc<Transcript>> {
        let mut entries = self.lock();
        let index = entries.iter().position(|entry| &entry.key == key)?;
        let entry = entries.remove(index)?;
        if entry.fingerprint != fingerprint {
            return None;
        }
        let transcript = Arc::clone(&entry.transcript);
        entries.push_back(entry);
        Some(transcript)
    }

    /// 写入（覆盖同键旧值），再按条数与字节上限从队头淘汰。
    /// 单个会话本身就超过字节上限时不缓存。
    pub fn insert(&self, key: CacheKey, fingerprint: Fingerprint, transcript: Arc<Transcript>) {
        let mut entries = self.lock();
        entries.retain(|entry| entry.key != key);
        if transcript.approx_bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        entries.push_back(Entry {
            key,
            fingerprint,
            transcript,
        });
        let mut total: usize = entries.iter().map(|e| e.transcript.approx_bytes).sum();
        while entries.len() > self.max_entries || total > self.max_bytes {
            match entries.pop_front() {
                Some(evicted) => total -= evicted.transcript.approx_bytes,
                None => break,
            }
        }
    }

    /// 命中返回 `(缓存, true)`；否则调用 `load` 解析并写入，返回 `(新值, false)`。
    /// 解析在锁外进行，同一会话并发打开时可能重复解析一次，但不会互相阻塞。
    pub fn get_or_load<F>(
        &self,
        key: CacheKey,
        fingerprint: Fingerprint,
        load: F,
    ) -> Result<(Arc<Transcript>, bool), String>
    where
        F: FnOnce() -> Result<Transcript, String>,
    {
        if let Some(hit) = self.get(&key, fingerprint) {
            return Ok((hit, true));
        }
        let transcript = Arc::new(load()?);
        self.insert(key, fingerprint, Arc::clone(&transcript));
        Ok((transcript, false))
    }

    pub fn remove(&self, provider_id: &str, source_path: &str) {
        self.lock()
            .retain(|entry| !(entry.key.0 == provider_id && entry.key.1 == source_path));
    }

    #[cfg(test)]
    fn keys(&self) -> Vec<String> {
        self.lock().iter().map(|e| e.key.1.clone()).collect()
    }
}

static GLOBAL: LazyLock<TranscriptCache> =
    LazyLock::new(|| TranscriptCache::new(MAX_ENTRIES, MAX_TOTAL_BYTES));

/// 进程级共享缓存
pub fn global() -> &'static TranscriptCache {
    &GLOBAL
}

// ── 流式分块（§5.1）──

/// 单个 `Messages` 包的序列化字节上限
pub const CHUNK_MAX_BYTES: usize = 256 * 1024;
/// 单个 `Messages` 包的消息条数上限
pub const CHUNK_MAX_MESSAGES: usize = 150;

/// 按 ≤ 256KB 或 ≤ 150 条切包，返回每包的 `[start, end)`。单条超过字节上限时独占一包。
pub fn chunk_ranges(message_bytes: &[usize]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (i, size) in message_bytes.iter().enumerate() {
        let count = i - start;
        if count > 0 && (count >= CHUNK_MAX_MESSAGES || bytes + size > CHUNK_MAX_BYTES) {
            ranges.push((start, i));
            start = i;
            bytes = 0;
        }
        bytes += size;
    }
    if start < message_bytes.len() {
        ranges.push((start, message_bytes.len()));
    }
    ranges
}

/// 由消息列表生成提问目录（§3.4 / §6.2）。
///
/// - 轮次键：消息自带 `turn_id` 时直接用；否则每条非注入、有内容的 user 消息开启新一轮
///   `t{n}`，首条提问之前的内容归 `t0`
/// - `question_preview`：本轮第一条非注入 user 消息，空白折叠后 ≤ 80 字符
/// - 只含注入内容的轮次不进目录
pub fn build_turns(messages: &[SessionMessage]) -> Vec<TurnIndex> {
    struct Building {
        index: TurnIndex,
        has_visible: bool,
        reply_pending: bool,
    }

    let mut turns: Vec<Building> = Vec::new();
    let mut generated = 0usize;
    let mut current_key: Option<String> = None;

    for (i, message) in messages.iter().enumerate() {
        let is_question = message.role == "user" && !message.injected && !message.is_empty();
        let key = match &message.turn_id {
            Some(turn_id) => turn_id.clone(),
            None if is_question => {
                generated += 1;
                format!("t{generated}")
            }
            None => current_key.clone().unwrap_or_else(|| "t0".to_string()),
        };

        if current_key.as_deref() != Some(key.as_str()) || turns.is_empty() {
            turns.push(Building {
                index: TurnIndex {
                    turn_id: key.clone(),
                    first_message_index: i,
                    last_message_index: i,
                    question_preview: String::new(),
                    ts: None,
                    step_count: 0,
                    error_count: 0,
                    has_final_reply: false,
                    aborted: false,
                },
                has_visible: false,
                reply_pending: false,
            });
            current_key = Some(key);
        }

        let Some(turn) = turns.last_mut() else {
            continue;
        };
        turn.index.last_message_index = i;
        if !message.injected {
            turn.has_visible = true;
        }
        if is_question && turn.index.question_preview.is_empty() {
            turn.index.question_preview = question_preview(message);
            turn.index.ts = message.ts;
        }

        for block in &message.blocks {
            match block {
                SessionBlock::ToolCall { .. } => {
                    turn.index.step_count += 1;
                    turn.reply_pending = false;
                }
                SessionBlock::ToolResult { status, .. } if *status == ToolStatus::Error => {
                    turn.index.error_count += 1;
                }
                SessionBlock::Text { text, .. }
                    if message.role == "assistant" && !text.trim().is_empty() =>
                {
                    turn.reply_pending = true;
                }
                SessionBlock::Event {
                    kind: EventKind::Aborted,
                    ..
                } => turn.index.aborted = true,
                _ => {}
            }
        }
        // 旧解析器（无 blocks）：assistant 的非工具文本视为回复
        if message.blocks.is_empty() && message.role == "assistant" {
            let content = message.content.trim();
            if content.starts_with("[Tool:") {
                turn.reply_pending = false;
            } else if !content.is_empty() {
                turn.reply_pending = true;
            }
        }
        turn.index.has_final_reply = turn.reply_pending;
    }

    turns
        .into_iter()
        .filter(|turn| turn.has_visible)
        .map(|turn| turn.index)
        .collect()
}

fn question_preview(message: &SessionMessage) -> String {
    let text_blocks: Vec<&str> = message
        .blocks
        .iter()
        .filter_map(|block| match block {
            SessionBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let source = if text_blocks.is_empty() {
        message.content.clone()
    } else {
        text_blocks.join(" ")
    };
    let collapsed = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= QUESTION_PREVIEW_CHARS {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(QUESTION_PREVIEW_CHARS - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::{ToolKind, ToolStatus};
    use std::time::Duration;
    use tempfile::tempdir;

    fn transcript_of_bytes(bytes: usize) -> Transcript {
        Transcript {
            approx_bytes: bytes,
            ..Transcript::default()
        }
    }

    fn key(name: &str) -> CacheKey {
        ("claude".to_string(), name.to_string())
    }

    fn fp(len: u64) -> Fingerprint {
        Fingerprint {
            modified: Some(SystemTime::UNIX_EPOCH),
            len,
        }
    }

    #[test]
    fn chunk_ranges_split_by_count_and_bytes() {
        assert!(chunk_ranges(&[]).is_empty());

        let ranges = chunk_ranges(&vec![10; 320]);
        assert_eq!(ranges, vec![(0, 150), (150, 300), (300, 320)]);

        let big = CHUNK_MAX_BYTES;
        let ranges = chunk_ranges(&[100, big, 100, big / 2, big / 2, 1]);
        assert_eq!(ranges, vec![(0, 1), (1, 2), (2, 4), (4, 6)]);
    }

    /// 审查 #7825：Grok 一轮进行中只往 chat_history.jsonl 追加，summary.json 不变，
    /// 指纹也必须变；其它 provider 不受影响
    #[test]
    fn grok_fingerprint_follows_chat_history() {
        use std::io::Write;

        let dir = tempdir().unwrap();
        let summary = dir.path().join("summary.json");
        let history = dir.path().join(GROK_CHAT_HISTORY);
        std::fs::write(&summary, "{}").unwrap();
        std::fs::write(&history, "{\"role\":\"user\"}\n").unwrap();
        let source = |provider: &str| ValidatedSource {
            provider_id: provider.to_string(),
            raw: summary.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: summary.clone(),
                root: dir.path().to_path_buf(),
            },
        };

        let grok_before = source_fingerprint(&source("grokbuild")).unwrap();
        let other_before = source_fingerprint(&source("claude")).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&history)
            .unwrap()
            .write_all(b"{\"role\":\"assistant\"}\n")
            .unwrap();

        assert_ne!(
            source_fingerprint(&source("grokbuild")).unwrap(),
            grok_before
        );
        assert_eq!(source_fingerprint(&source("claude")).unwrap(), other_before);
    }

    #[test]
    fn cache_hits_until_file_mtime_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let location = SourceLocation::Path {
            path: path.clone(),
            root: dir.path().to_path_buf(),
        };
        let cache = TranscriptCache::new(MAX_ENTRIES, MAX_TOTAL_BYTES);
        let mut loads = 0;

        let first = fingerprint(&location).unwrap();
        let (_, cached) = cache
            .get_or_load(key("s"), first, || {
                loads += 1;
                Ok(Transcript::default())
            })
            .unwrap();
        assert!(!cached);

        let (_, cached) = cache
            .get_or_load(key("s"), fingerprint(&location).unwrap(), || {
                loads += 1;
                Ok(Transcript::default())
            })
            .unwrap();
        assert!(cached, "未变化的文件应命中缓存");

        // 长度不变、只改 mtime 也要失效
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        drop(file);
        let changed = fingerprint(&location).unwrap();
        assert_ne!(changed, first);
        let (_, cached) = cache
            .get_or_load(key("s"), changed, || {
                loads += 1;
                Ok(Transcript::default())
            })
            .unwrap();
        assert!(!cached);
        assert_eq!(loads, 2);
    }

    #[test]
    fn load_error_is_not_cached() {
        let cache = TranscriptCache::new(MAX_ENTRIES, MAX_TOTAL_BYTES);
        let err = cache
            .get_or_load(key("bad"), fp(1), || Err("boom".to_string()))
            .unwrap_err();
        assert_eq!(err, "boom");
        assert!(cache.keys().is_empty());
    }

    #[test]
    fn lru_evicts_least_recently_used_by_count() {
        let cache = TranscriptCache::new(2, MAX_TOTAL_BYTES);
        cache.insert(key("a"), fp(1), Arc::new(transcript_of_bytes(1)));
        cache.insert(key("b"), fp(1), Arc::new(transcript_of_bytes(1)));
        // 访问 a，使 b 成为最久未用
        assert!(cache.get(&key("a"), fp(1)).is_some());
        cache.insert(key("c"), fp(1), Arc::new(transcript_of_bytes(1)));
        assert_eq!(cache.keys(), vec!["a", "c"]);
    }

    #[test]
    fn lru_evicts_by_total_bytes_and_skips_oversized() {
        let cache = TranscriptCache::new(8, 100);
        cache.insert(key("a"), fp(1), Arc::new(transcript_of_bytes(60)));
        cache.insert(key("b"), fp(1), Arc::new(transcript_of_bytes(30)));
        cache.insert(key("c"), fp(1), Arc::new(transcript_of_bytes(30)));
        assert_eq!(cache.keys(), vec!["b", "c"]);

        cache.insert(key("huge"), fp(1), Arc::new(transcript_of_bytes(101)));
        assert_eq!(cache.keys(), vec!["b", "c"]);
    }

    #[test]
    fn remove_drops_entry() {
        let cache = TranscriptCache::new(8, 100);
        cache.insert(key("a"), fp(1), Arc::new(transcript_of_bytes(1)));
        cache.remove("claude", "a");
        assert!(cache.get(&key("a"), fp(1)).is_none());
    }

    #[test]
    fn sqlite_fingerprint_includes_wal() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("state.db");
        std::fs::write(&db, "x").unwrap();
        let location = SourceLocation::Sqlite {
            db: db.clone(),
            session_id: "s".into(),
        };
        let before = fingerprint(&location).unwrap();
        std::fs::write(dir.path().join("state.db-wal"), "wal").unwrap();
        assert_ne!(fingerprint(&location).unwrap(), before);
    }

    fn text(role: &str, text: &str) -> SessionMessage {
        SessionMessage::from_blocks(role, None, vec![SessionBlock::text(text.to_string())])
    }

    fn tool_call(id: &str) -> SessionBlock {
        SessionBlock::ToolCall {
            id: id.to_string(),
            raw_name: "Bash".into(),
            kind: ToolKind::Shell,
            title: "ls".into(),
            detail: None,
            server: None,
            input_preview: String::new(),
            input_total_len: 0,
            input_full: None,
            diff: None,
            by_user: false,
        }
    }

    fn tool_result(id: &str, status: ToolStatus) -> SessionBlock {
        SessionBlock::ToolResult {
            call_id: id.to_string(),
            status,
            preview: "out".into(),
            total_len: 3,
            line_count: 1,
            truncated: false,
            full: None,
            exit_code: None,
            duration_ms: None,
            images: vec![],
            saved_path: None,
        }
    }

    #[test]
    fn build_turns_groups_by_question_and_counts_steps() {
        let mut injected = text("user", "# AGENTS.md instructions");
        injected.injected = true;
        let messages = vec![
            injected,
            text("user", "  first\n question  "),
            SessionMessage::from_blocks("assistant", None, vec![tool_call("c1")]),
            SessionMessage::from_blocks("tool", None, vec![tool_result("c1", ToolStatus::Error)]),
            text("assistant", "done"),
            text("user", &"x".repeat(200)),
            SessionMessage::from_blocks("assistant", None, vec![tool_call("c2")]),
            SessionMessage::from_blocks(
                "assistant",
                None,
                vec![SessionBlock::event(EventKind::Aborted, None, None)],
            ),
        ];

        let turns = build_turns(&messages);
        // 只有注入内容的 t0 不进目录
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn_id, "t1");
        assert_eq!(turns[0].question_preview, "first question");
        assert_eq!(
            (turns[0].first_message_index, turns[0].last_message_index),
            (1, 4)
        );
        assert_eq!(turns[0].step_count, 1);
        assert_eq!(turns[0].error_count, 1);
        assert!(turns[0].has_final_reply);
        assert!(!turns[0].aborted);

        assert_eq!(turns[1].question_preview.chars().count(), 80);
        assert!(turns[1].question_preview.ends_with('…'));
        assert!(!turns[1].has_final_reply);
        assert!(turns[1].aborted);
    }

    #[test]
    fn build_turns_prefers_native_turn_ids() {
        let mut q = text("user", "hi");
        q.turn_id = Some("turn-a".into());
        let mut a = text("assistant", "hello");
        a.turn_id = Some("turn-a".into());
        let mut q2 = text("user", "again");
        q2.turn_id = Some("turn-b".into());
        let turns = build_turns(&[q, a, q2]);
        assert_eq!(
            turns.iter().map(|t| t.turn_id.as_str()).collect::<Vec<_>>(),
            vec!["turn-a", "turn-b"]
        );
        assert!(turns[0].has_final_reply);
    }

    #[test]
    fn transcript_records_serialized_sizes() {
        let messages = vec![text("user", "hi"), text("assistant", "hello")];
        let expected: usize = messages
            .iter()
            .map(|m| serde_json::to_vec(m).unwrap().len())
            .sum();
        let transcript = Transcript::new(messages);
        assert_eq!(transcript.approx_bytes, expected);
        assert_eq!(transcript.message_bytes.len(), 2);
    }
}
