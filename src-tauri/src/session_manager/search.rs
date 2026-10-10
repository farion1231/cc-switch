//! 会话正文搜索索引（issue #7420）。
//!
//! 会话页的元数据搜索（标题、目录、首末消息）在前端用 FlexSearch 做；消息正文太大，
//! 放不进前端内存，这里在 Rust 侧维护一份 SQLite FTS5 索引：
//!
//! - 独立文件 `~/.cc-switch/session-index.db`，不进主库，不参与备份与云同步。它只是
//!   缓存，删掉后下次打开会话页会重建；关掉「搜索消息正文」时整个文件一起删除
//! - 每条消息一行，只收用户与 Agent 的正文（Text 块）；思考、工具参数与输出、
//!   注入内容（AGENTS.md、system-reminder…）不收
//! - trigram 分词：中文、代码标识符都能按子串命中；不足 3 个字符的查询 trigram
//!   用不上，退回 `LIKE` 扫描
//! - 增量：按会话源的 `(mtime, len)` 指纹判断，没变的会话不再解析
//!
//! FTS 行的 rowid 由「源 id × 2^24 + 消息序号」组成，重建一个会话时按 rowid 区间删除，
//! 不必扫全表。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::cache::{self, Fingerprint};
use super::content;
use super::model::{SessionBlock, SessionMessage};

const INDEX_FILE: &str = "session-index.db";
/// 索引格式版本；表结构或收录规则变了就加一，旧索引整体重建
const INDEX_VERSION: i64 = 1;
/// rowid 里留给消息序号的位数（单个会话最多 1600 万条消息，超出部分不收）
const ROW_SHIFT: u32 = 24;
const MAX_MESSAGES_PER_SOURCE: usize = 1 << ROW_SHIFT;
/// 单条消息收录的最大字符数
const MAX_ROW_CHARS: usize = 20_000;
/// 一次查询最多取的命中行数（按会话聚合前）
const MAX_HIT_ROWS: i64 = 20_000;
/// 一次查询最多返回的会话数
pub const DEFAULT_LIMIT: usize = 200;
/// 每个会话最多给几条摘录
const MAX_SNIPPETS_PER_SESSION: usize = 3;
/// trigram 能用上索引的最短查询（字符数）
const MIN_MATCH_CHARS: usize = 3;
/// 摘录里命中词前后保留的字符数
const SNIPPET_BEFORE: usize = 30;
const SNIPPET_AFTER: usize = 90;
/// 同步时每处理多少个会话上报一次进度
const PROGRESS_EVERY: usize = 25;
/// 一次同步重建了这么多会话后整理索引（合并 FTS 段、VACUUM）；
/// 实测 1600 多个会话首次建库后，整理能把文件从约 170MB 缩到约 100MB
const COMPACT_AFTER_CHANGES: usize = 50;

/// 一条命中消息的摘录
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSnippet {
    /// 摘录所在消息在会话里的序号（与 `load_messages` 的下标一致）
    pub message_index: usize,
    /// 命中词附近的一段正文（纯文本，空白已压缩）
    pub text: String,
}

/// 一个会话的命中：最相关的几条消息的摘录（第一条最相关）与命中消息数
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    pub provider_id: String,
    pub source_path: String,
    pub snippets: Vec<ContentSnippet>,
    /// 命中的消息条数
    pub match_count: usize,
}

/// 索引进度；`running` 为 false 时 `processed == total` 表示索引已是最新
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub running: bool,
    pub processed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSearchResult {
    pub hits: Vec<ContentHit>,
    pub status: IndexStatus,
}

/// 存进索引的指纹：mtime（纳秒，取不到为 None）+ 长度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredFingerprint {
    pub modified_ns: Option<i64>,
    pub len: i64,
}

impl From<Fingerprint> for StoredFingerprint {
    fn from(fp: Fingerprint) -> Self {
        Self {
            modified_ns: fp
                .modified
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos().min(i64::MAX as u128) as i64),
            len: fp.len.min(i64::MAX as u64) as i64,
        }
    }
}

/// 一条消息的收录文本：用户与 Agent 的 Text 块，跳过注入内容
pub fn message_rows(messages: &[SessionMessage]) -> Vec<(usize, String)> {
    messages
        .iter()
        .enumerate()
        .take(MAX_MESSAGES_PER_SOURCE)
        .filter(|(_, message)| {
            !message.injected && matches!(message.role.as_str(), "user" | "assistant")
        })
        .filter_map(|(index, message)| {
            let text = if message.blocks.is_empty() {
                message.content.clone()
            } else {
                message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        SessionBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            Some((index, truncate_chars(text, MAX_ROW_CHARS)))
        })
        .collect()
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    }
}

/// 不区分大小写地比较字符（只取小写形式的第一个字符，保证下标一一对应）
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// 逐字符折叠大小写后的字符串（与 [`fold`] 一致，长度不变）
fn fold_str(text: &str) -> String {
    text.chars().map(fold).collect()
}

/// 查询里有没有 ASCII 以外、区分大小写的字符（É、Ж、Σ…）。
/// SQLite 的 `LIKE` 只折叠 ASCII 大小写，这类查询不能交给它
fn has_non_ascii_case(query: &str) -> bool {
    query
        .chars()
        .any(|c| !c.is_ascii() && c.to_lowercase().ne(c.to_uppercase()))
}

/// 取命中词附近的一段文字；找不到（大小写折叠规则不同）时从开头截
pub fn make_snippet(text: &str, query: &str) -> String {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query.chars().map(fold).collect();
    let found = if needle.is_empty() || needle.len() > hay.len() {
        None
    } else {
        (0..=hay.len() - needle.len()).find(|&start| {
            hay[start..start + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(a, b)| fold(*a) == *b)
        })
    };
    let (start, end) = match found {
        Some(at) => (
            at.saturating_sub(SNIPPET_BEFORE),
            (at + needle.len() + SNIPPET_AFTER).min(hay.len()),
        ),
        None => (0, (SNIPPET_BEFORE + SNIPPET_AFTER).min(hay.len())),
    };

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    // 连续空白压成一个空格；开头的空白直接丢掉
    let mut pending_space = false;
    let mut started = false;
    for &c in &hay[start..end] {
        if c.is_whitespace() {
            pending_space = started;
            continue;
        }
        if pending_space {
            snippet.push(' ');
            pending_space = false;
        }
        snippet.push(c);
        started = true;
    }
    if end < hay.len() {
        snippet.push('…');
    }
    snippet
}

/// FTS5 短语查询：整段查询按子串匹配
fn match_expression(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

/// `LIKE` 模式，转义 `%` `_` `\`
fn like_pattern(query: &str) -> String {
    let mut pattern = String::from("%");
    for c in query.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

pub struct SessionIndex {
    conn: Connection,
}

impl SessionIndex {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != INDEX_VERSION {
            conn.execute_batch(
                "DROP TABLE IF EXISTS message_text;
                 DROP TABLE IF EXISTS indexed_sources;",
            )?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS indexed_sources (
                 id INTEGER PRIMARY KEY,
                 provider_id TEXT NOT NULL,
                 source_path TEXT NOT NULL,
                 modified_ns INTEGER,
                 len INTEGER NOT NULL,
                 UNIQUE (provider_id, source_path)
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS message_text
                 USING fts5(text, tokenize = 'trigram');",
        )?;
        conn.pragma_update(None, "user_version", INDEX_VERSION)?;
        Ok(Self { conn })
    }

    pub fn stored_fingerprint(
        &self,
        provider_id: &str,
        source_path: &str,
    ) -> rusqlite::Result<Option<StoredFingerprint>> {
        self.conn
            .query_row(
                "SELECT modified_ns, len FROM indexed_sources
                 WHERE provider_id = ?1 AND source_path = ?2",
                params![provider_id, source_path],
                |row| {
                    Ok(StoredFingerprint {
                        modified_ns: row.get(0)?,
                        len: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    /// 用新解析的正文替换一个会话的全部索引行
    pub fn replace_source(
        &mut self,
        provider_id: &str,
        source_path: &str,
        fingerprint: StoredFingerprint,
        rows: &[(usize, String)],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM indexed_sources WHERE provider_id = ?1 AND source_path = ?2",
                params![provider_id, source_path],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => {
                delete_rows(&tx, id)?;
                tx.execute(
                    "UPDATE indexed_sources SET modified_ns = ?1, len = ?2 WHERE id = ?3",
                    params![fingerprint.modified_ns, fingerprint.len, id],
                )?;
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO indexed_sources (provider_id, source_path, modified_ns, len)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        provider_id,
                        source_path,
                        fingerprint.modified_ns,
                        fingerprint.len
                    ],
                )?;
                tx.last_insert_rowid()
            }
        };
        {
            let mut insert =
                tx.prepare("INSERT INTO message_text (rowid, text) VALUES (?1, ?2)")?;
            for (index, text) in rows {
                insert.execute(params![(id << ROW_SHIFT) + *index as i64, text])?;
            }
        }
        tx.commit()
    }

    /// 合并 FTS 段并 VACUUM（会持锁数秒，只在大批量重建后调用）
    pub fn compact(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO message_text (message_text) VALUES ('optimize')",
            [],
        )?;
        self.conn.execute_batch("VACUUM;")?;
        self.checkpoint()
    }

    /// 把 WAL 写回主库并截断，免得 `-wal` 文件留在磁盘上占空间
    pub fn checkpoint(&self) -> rusqlite::Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
    }

    pub fn remove_source(&mut self, provider_id: &str, source_path: &str) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM indexed_sources WHERE provider_id = ?1 AND source_path = ?2",
                params![provider_id, source_path],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            delete_rows(&tx, id)?;
            tx.execute("DELETE FROM indexed_sources WHERE id = ?1", params![id])?;
        }
        tx.commit()
    }

    /// 删掉已经不在会话列表里的源（会话在外部被删、移走）
    pub fn retain_sources(&mut self, keep: &HashSet<(String, String)>) -> rusqlite::Result<()> {
        let stale: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT provider_id, source_path FROM indexed_sources")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.filter_map(Result::ok)
                .filter(|key| !keep.contains(key))
                .collect()
        };
        for (provider_id, source_path) in stale {
            self.remove_source(&provider_id, &source_path)?;
        }
        Ok(())
    }

    /// 按会话聚合的正文命中。`providers` 为 None 时不按应用过滤。
    pub fn search(
        &self,
        query: &str,
        providers: Option<&HashSet<String>>,
        limit: usize,
    ) -> rusqlite::Result<Vec<ContentHit>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let sources: HashMap<i64, (String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, provider_id, source_path FROM indexed_sources")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // 应用过滤要在截断（MAX_HIT_ROWS）之前生效，否则其他应用的大量命中会把选中应用挤掉
        let allowed: Option<Vec<i64>> = providers.map(|set| {
            sources
                .iter()
                .filter(|(_, (provider_id, _))| set.contains(provider_id))
                .map(|(id, _)| *id)
                .collect()
        });
        if allowed.as_ref().is_some_and(Vec::is_empty) {
            return Ok(Vec::new());
        }
        // 传给 SQL 的源 id 列表（JSON 数组）；None 表示不过滤
        let allowed_json = allowed
            .as_ref()
            .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()));
        let source_filter =
            format!("(?3 IS NULL OR (rowid >> {ROW_SHIFT}) IN (SELECT value FROM json_each(?3)))");

        let rowids: Vec<i64> = if query.chars().count() >= MIN_MATCH_CHARS {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT rowid FROM message_text WHERE message_text MATCH ?1 AND {source_filter}
                 ORDER BY rank LIMIT ?2"
            ))?;
            let rows = stmt.query_map(
                params![match_expression(query), MAX_HIT_ROWS, allowed_json],
                |row| row.get(0),
            )?;
            rows.collect::<rusqlite::Result<_>>()?
        } else if has_non_ascii_case(query) {
            // 不足 3 个字符且含 É 之类的字母：LIKE 会漏掉大小写不同的写法，在 Rust 里逐行折叠比较
            let needle = fold_str(query);
            let allowed: Option<HashSet<i64>> = allowed.map(|ids| ids.into_iter().collect());
            let mut stmt = self
                .conn
                .prepare("SELECT rowid, text FROM message_text ORDER BY rowid DESC")?;
            let mut rows = stmt.query([])?;
            let mut found = Vec::new();
            while let Some(row) = rows.next()? {
                let rowid: i64 = row.get(0)?;
                if allowed
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(&(rowid >> ROW_SHIFT)))
                {
                    continue;
                }
                let text: String = row.get(1)?;
                if fold_str(&text).contains(&needle) {
                    found.push(rowid);
                    if found.len() as i64 >= MAX_HIT_ROWS {
                        break;
                    }
                }
            }
            found
        } else {
            // 不足 3 个字符：trigram 用不上，LIKE 扫描（ASCII 与中日文等无大小写的字符都正确）
            let mut stmt = self.conn.prepare(&format!(
                "SELECT rowid FROM message_text WHERE text LIKE ?1 ESCAPE '\\' AND {source_filter}
                 ORDER BY rowid DESC LIMIT ?2"
            ))?;
            let rows = stmt.query_map(
                params![like_pattern(query), MAX_HIT_ROWS, allowed_json],
                |row| row.get(0),
            )?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if rowids.is_empty() {
            return Ok(Vec::new());
        }

        // 按会话聚合：保留最先出现（最相关）的几行，累计命中条数
        let mut order: Vec<i64> = Vec::new();
        let mut best: HashMap<i64, (Vec<i64>, usize)> = HashMap::new();
        for rowid in rowids {
            let source_id = rowid >> ROW_SHIFT;
            if !sources.contains_key(&source_id) {
                continue;
            }
            let (top, count) = best.entry(source_id).or_insert_with(|| {
                order.push(source_id);
                (Vec::new(), 0)
            });
            *count += 1;
            if top.len() < MAX_SNIPPETS_PER_SESSION {
                top.push(rowid);
            }
        }

        let mut text_stmt = self
            .conn
            .prepare("SELECT text FROM message_text WHERE rowid = ?1")?;
        let mut hits = Vec::new();
        for source_id in order.into_iter().take(limit) {
            let (top, match_count) = &best[&source_id];
            let (provider_id, source_path) = &sources[&source_id];
            let snippets = top
                .iter()
                .map(|&rowid| {
                    let text: String = text_stmt.query_row(params![rowid], |row| row.get(0))?;
                    Ok(ContentSnippet {
                        message_index: (rowid & ((1 << ROW_SHIFT) - 1)) as usize,
                        text: make_snippet(&text, query),
                    })
                })
                .collect::<rusqlite::Result<_>>()?;
            hits.push(ContentHit {
                provider_id: provider_id.clone(),
                source_path: source_path.clone(),
                snippets,
                match_count: *match_count,
            });
        }
        Ok(hits)
    }
}

fn delete_rows(conn: &Connection, source_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM message_text WHERE rowid >= ?1 AND rowid < ?2",
        params![source_id << ROW_SHIFT, (source_id + 1) << ROW_SHIFT],
    )?;
    Ok(())
}

// ── 全局索引与后台同步 ───────────────────────────────────────────────

static INDEX: LazyLock<Mutex<Option<SessionIndex>>> = LazyLock::new(|| Mutex::new(None));
static STATUS: LazyLock<Mutex<IndexStatus>> = LazyLock::new(|| Mutex::new(IndexStatus::default()));
static RUNNING: AtomicBool = AtomicBool::new(false);
static RERUN: AtomicBool = AtomicBool::new(false);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn index_path() -> PathBuf {
    crate::config::get_app_config_dir().join(INDEX_FILE)
}

pub fn enabled() -> bool {
    crate::settings::get_settings().session_content_search_enabled
}

pub fn status() -> IndexStatus {
    *lock(&STATUS)
}

/// 打开（必要时创建）索引后执行 `f`；功能关闭时不碰磁盘
fn with_index<T>(f: impl FnOnce(&mut SessionIndex) -> rusqlite::Result<T>) -> Result<T, String> {
    with_index_in(&INDEX, &index_path(), enabled, f)
}

/// [`with_index`] 的实现，索引槽、路径、开关可替换（测试用）。
///
/// 开关必须在持有锁之后检查：关闭功能时 [`clear`] 在同一把锁里删除索引文件，
/// 锁外检查的话，已经通过检查、正在等锁的后台写入会在清理之后把文件重新建出来
fn with_index_in<T>(
    slot: &Mutex<Option<SessionIndex>>,
    path: &Path,
    is_enabled: impl Fn() -> bool,
    f: impl FnOnce(&mut SessionIndex) -> rusqlite::Result<T>,
) -> Result<T, String> {
    let mut guard = lock(slot);
    if !is_enabled() {
        return Err("Session content search is disabled".to_string());
    }
    if guard.is_none() {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let index = SessionIndex::open(path).or_else(|first| {
            // 索引只是缓存：打不开（损坏、格式不认识）就删掉重建
            log::warn!("会话正文索引无法打开，重建: {first}");
            remove_index_files(path);
            SessionIndex::open(path)
        });
        *guard = Some(index.map_err(|e| format!("Failed to open session index: {e}"))?);
    }
    let index = guard.as_mut().expect("index opened above");
    f(index).map_err(|e| format!("Session index error: {e}"))
}

fn remove_index_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let _ = fs::remove_file(PathBuf::from(file));
    }
}

/// 删除整个索引文件（关闭功能时调用）
pub fn clear() -> Result<(), String> {
    let mut guard = lock(&INDEX);
    *guard = None;
    remove_index_files(&index_path());
    *lock(&STATUS) = IndexStatus::default();
    Ok(())
}

/// 会话删除后同步移除索引行；索引没开或功能关闭时什么也不做
pub fn forget_source(provider_id: &str, source_path: &str) {
    if !enabled() {
        return;
    }
    if let Err(e) = with_index(|index| index.remove_source(provider_id, source_path)) {
        log::warn!("移除会话正文索引失败: {e}");
    }
}

pub fn search(query: &str, providers: Option<Vec<String>>, limit: usize) -> ContentSearchResult {
    let status = status();
    if !enabled() || query.trim().is_empty() {
        return ContentSearchResult {
            hits: Vec::new(),
            status,
        };
    }
    let providers: Option<HashSet<String>> = providers.map(|list| list.into_iter().collect());
    let hits =
        with_index(|index| index.search(query, providers.as_ref(), limit)).unwrap_or_else(|e| {
            log::warn!("会话正文搜索失败: {e}");
            Vec::new()
        });
    ContentSearchResult { hits, status }
}

/// 在后台线程里把索引同步到当前的会话列表；已经在跑时只记一次「跑完再来一遍」。
/// `on_status` 在开始、每处理一批、结束时各调用一次。
pub fn start_sync<F>(on_status: F) -> IndexStatus
where
    F: Fn(IndexStatus) + Send + 'static,
{
    if !enabled() {
        return status();
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        RERUN.store(true, Ordering::SeqCst);
        return status();
    }
    std::thread::spawn(move || loop {
        RERUN.store(false, Ordering::SeqCst);
        run_sync(&on_status);
        if RERUN.load(Ordering::SeqCst) {
            continue;
        }
        RUNNING.store(false, Ordering::SeqCst);
        // 收尾的这一刻又有人请求同步：自己接着跑，别把请求丢掉
        if RERUN.load(Ordering::SeqCst) && !RUNNING.swap(true, Ordering::SeqCst) {
            continue;
        }
        break;
    });
    status()
}

fn set_status(status: IndexStatus, on_status: &dyn Fn(IndexStatus)) {
    *lock(&STATUS) = status;
    on_status(status);
}

fn run_sync(on_status: &dyn Fn(IndexStatus)) {
    let mut seen = HashSet::new();
    let sources: Vec<(String, String)> = super::scan_sessions()
        .into_iter()
        .filter_map(|session| Some((session.provider_id, session.source_path?)))
        .filter(|key| seen.insert(key.clone()))
        .collect();
    let total = sources.len();
    let mut status = IndexStatus {
        running: true,
        processed: 0,
        total,
    };
    set_status(status, on_status);

    if let Err(e) = with_index(|index| index.retain_sources(&seen)) {
        log::warn!("清理会话正文索引失败: {e}");
    }

    let mut changed = 0;
    for (provider_id, source_path) in &sources {
        if !enabled() {
            break;
        }
        if index_source(provider_id, source_path) {
            changed += 1;
        }
        status.processed += 1;
        if status.processed % PROGRESS_EVERY == 0 {
            set_status(status, on_status);
        }
    }

    let tidy = if changed >= COMPACT_AFTER_CHANGES {
        with_index(|index| index.compact())
    } else {
        with_index(|index| index.checkpoint())
    };
    if let Err(e) = tidy {
        log::warn!("整理会话正文索引失败: {e}");
    }

    status.running = false;
    set_status(status, on_status);
}

/// 指纹没变就跳过；变了（或从没收录过）就重新解析并替换。返回是否重建过
fn index_source(provider_id: &str, source_path: &str) -> bool {
    let Ok(source) = content::validate_source(provider_id, source_path) else {
        return false;
    };
    let Ok(fingerprint) = cache::source_fingerprint(&source) else {
        return false;
    };
    let fingerprint = StoredFingerprint::from(fingerprint);
    let stored = with_index(|index| index.stored_fingerprint(provider_id, source_path))
        .ok()
        .flatten();
    if stored == Some(fingerprint) {
        return false;
    }
    // 解析失败也记下指纹（零行），免得每次同步都重试一个坏文件
    let rows = super::load_messages(&source.provider_id, &source.load_path())
        .map(|messages| message_rows(&messages))
        .unwrap_or_default();
    match with_index(|index| index.replace_source(provider_id, source_path, fingerprint, &rows)) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("写入会话正文索引失败: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::EventKind;

    fn fp(len: i64) -> StoredFingerprint {
        StoredFingerprint {
            modified_ns: Some(1),
            len,
        }
    }

    fn rows(texts: &[&str]) -> Vec<(usize, String)> {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| (i, text.to_string()))
            .collect()
    }

    #[test]
    fn message_rows_keeps_user_and_assistant_text_only() {
        let mut injected =
            SessionMessage::from_blocks("user", None, vec![SessionBlock::text("AGENTS.md 内容")]);
        injected.injected = true;
        let messages = vec![
            SessionMessage::from_blocks("user", None, vec![SessionBlock::text("问一下跑马灯")]),
            injected,
            SessionMessage::from_blocks(
                "assistant",
                None,
                vec![
                    SessionBlock::Thinking {
                        text: "内部思考".into(),
                        summary: None,
                        redacted: false,
                        duration_ms: None,
                        full: None,
                    },
                    SessionBlock::text("第一段"),
                    SessionBlock::event(EventKind::Info, Some("事件".into()), None),
                    SessionBlock::text("第二段"),
                ],
            ),
            SessionMessage {
                role: "tool".into(),
                content: "工具输出".into(),
                ..SessionMessage::default()
            },
            SessionMessage {
                role: "assistant".into(),
                content: "旧格式正文".into(),
                ..SessionMessage::default()
            },
            SessionMessage::from_blocks("user", None, vec![SessionBlock::text("   ")]),
        ];

        assert_eq!(
            message_rows(&messages),
            vec![
                (0, "问一下跑马灯".to_string()),
                (2, "第一段\n第二段".to_string()),
                (4, "旧格式正文".to_string()),
            ]
        );
    }

    #[test]
    fn message_rows_truncates_long_text() {
        let long = "字".repeat(MAX_ROW_CHARS + 10);
        let messages = vec![SessionMessage::from_blocks(
            "user",
            None,
            vec![SessionBlock::text(long)],
        )];
        assert_eq!(message_rows(&messages)[0].1.chars().count(), MAX_ROW_CHARS);
    }

    #[test]
    fn search_matches_chinese_substrings_including_two_char_queries() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        index
            .replace_source(
                "codex",
                "/a.jsonl",
                fp(1),
                &rows(&["开头", "中间讨论了跑马灯的滚动速度", "结尾"]),
            )
            .unwrap();
        index
            .replace_source(
                "claude",
                "/b.jsonl",
                fp(1),
                &rows(&["排版时注意正交的布局"]),
            )
            .unwrap();

        let hits = index.search("跑马灯", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].provider_id, "codex");
        assert_eq!(hits[0].source_path, "/a.jsonl");
        assert_eq!(hits[0].snippets.len(), 1);
        assert_eq!(hits[0].snippets[0].message_index, 1);
        assert!(hits[0].snippets[0].text.contains("跑马灯"));

        // 两个字：trigram 用不上，走 LIKE
        let hits = index.search("正交", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_path, "/b.jsonl");

        assert!(index.search("不存在的词", None, 10).unwrap().is_empty());
    }

    #[test]
    fn search_is_case_insensitive_and_escapes_special_characters() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        index
            .replace_source(
                "claude",
                "/a.jsonl",
                fp(1),
                &rows(&[
                    "call useSessionSearch() here",
                    "100% done",
                    "a \"quoted\" word",
                ]),
            )
            .unwrap();

        assert_eq!(index.search("USESESSIONSEARCH", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("0%", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("%", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("_", None, 10).unwrap().len(), 0);
        assert_eq!(index.search("\"quoted\"", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("AND OR NOT", None, 10).unwrap().len(), 0);
    }

    #[test]
    fn search_folds_non_ascii_case_for_long_and_short_queries() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        index
            .replace_source(
                "claude",
                "/a.jsonl",
                fp(1),
                &rows(&["Ordered an Éclair", "Привет мир"]),
            )
            .unwrap();

        // trigram 路径
        assert_eq!(index.search("éclair", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("ÉCLAIR", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("ПРИВЕТ", None, 10).unwrap().len(), 1);
        // 不足 3 个字符：LIKE 只折叠 ASCII，这里要走 Rust 逐行比较
        let hits = index.search("éc", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippets[0].text.contains("Éclair"));
        assert_eq!(index.search("ПР", None, 10).unwrap().len(), 1);
        assert_eq!(index.search("ÉX", None, 10).unwrap().len(), 0);
    }

    #[test]
    fn search_aggregates_per_session_and_filters_providers() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        index
            .replace_source(
                "codex",
                "/a.jsonl",
                fp(1),
                &rows(&["deploy one", "nothing", "deploy two"]),
            )
            .unwrap();
        index
            .replace_source("claude", "/b.jsonl", fp(1), &rows(&["deploy three"]))
            .unwrap();

        let hits = index.search("deploy", None, 10).unwrap();
        assert_eq!(hits.len(), 2);
        let codex = hits.iter().find(|hit| hit.provider_id == "codex").unwrap();
        assert_eq!(codex.match_count, 2);
        let mut indexes: Vec<usize> = codex.snippets.iter().map(|s| s.message_index).collect();
        indexes.sort_unstable();
        assert_eq!(indexes, vec![0, 2]);

        let only_claude: HashSet<String> = ["claude".to_string()].into_iter().collect();
        let hits = index.search("deploy", Some(&only_claude), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].provider_id, "claude");

        assert_eq!(index.search("deploy", None, 1).unwrap().len(), 1);
    }

    #[test]
    fn search_caps_snippets_per_session_but_counts_every_match() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        let texts: Vec<String> = (0..5).map(|i| format!("deploy step {i}")).collect();
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        index
            .replace_source("codex", "/a.jsonl", fp(1), &rows(&texts))
            .unwrap();

        let hits = index.search("deploy", None, 10).unwrap();
        assert_eq!(hits[0].match_count, 5);
        assert_eq!(hits[0].snippets.len(), MAX_SNIPPETS_PER_SESSION);
    }

    #[test]
    fn provider_filter_applies_before_the_hit_row_cap() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        // 先插入选中应用的唯一命中，再让其他应用占满候选上限（LIKE 路径按 rowid 倒序）
        index
            .replace_source(
                "codex",
                "/codex.jsonl",
                fp(1),
                &rows(&["needle xy Éclair target"]),
            )
            .unwrap();
        let noise: Vec<(usize, String)> = (0..MAX_HIT_ROWS as usize + 5)
            .map(|i| (i, "needle xy Éclair".to_string()))
            .collect();
        index
            .replace_source("claude", "/claude.jsonl", fp(1), &noise)
            .unwrap();

        let only_codex: HashSet<String> = ["codex".to_string()].into_iter().collect();
        for query in ["needle", "xy", "éc"] {
            let hits = index.search(query, Some(&only_codex), 10).unwrap();
            assert_eq!(hits.len(), 1, "query {query}");
            assert_eq!(hits[0].provider_id, "codex", "query {query}");
        }
        let nobody: HashSet<String> = ["gemini".to_string()].into_iter().collect();
        assert!(index
            .search("needle", Some(&nobody), 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn disabling_while_a_writer_waits_for_the_lock_keeps_the_index_deleted() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INDEX_FILE);
        let slot: Arc<Mutex<Option<SessionIndex>>> = Arc::new(Mutex::new(None));
        let enabled = Arc::new(AtomicBool::new(true));

        // 模拟 clear：持锁期间关闭开关、删文件；写入线程此时已经在等锁
        let guard = lock(&slot);
        let writer = {
            let slot = Arc::clone(&slot);
            let enabled = Arc::clone(&enabled);
            let path = path.clone();
            std::thread::spawn(move || {
                with_index_in(
                    &slot,
                    &path,
                    || enabled.load(Ordering::SeqCst),
                    |index| index.replace_source("codex", "/a.jsonl", fp(1), &rows(&["secret"])),
                )
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        enabled.store(false, Ordering::SeqCst);
        remove_index_files(&path);
        drop(guard);

        assert!(writer.join().unwrap().is_err());
        assert!(!path.exists());
    }

    #[test]
    fn replace_and_remove_only_touch_one_source() {
        let mut index = SessionIndex::open_in_memory().unwrap();
        index
            .replace_source("codex", "/a.jsonl", fp(1), &rows(&["old words"]))
            .unwrap();
        index
            .replace_source("codex", "/b.jsonl", fp(1), &rows(&["old words too"]))
            .unwrap();

        index
            .replace_source("codex", "/a.jsonl", fp(2), &rows(&["new words"]))
            .unwrap();
        assert_eq!(
            index.stored_fingerprint("codex", "/a.jsonl").unwrap(),
            Some(fp(2))
        );
        let hits = index.search("old words", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_path, "/b.jsonl");

        index.remove_source("codex", "/b.jsonl").unwrap();
        assert!(index.search("old words", None, 10).unwrap().is_empty());
        assert_eq!(index.stored_fingerprint("codex", "/b.jsonl").unwrap(), None);

        let keep: HashSet<(String, String)> = HashSet::new();
        index.retain_sources(&keep).unwrap();
        assert!(index.search("new words", None, 10).unwrap().is_empty());
    }

    #[test]
    fn reopening_with_another_version_rebuilds_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INDEX_FILE);
        {
            let mut index = SessionIndex::open(&path).unwrap();
            index
                .replace_source("codex", "/a.jsonl", fp(1), &rows(&["persisted text"]))
                .unwrap();
            index.compact().unwrap();
        }
        {
            let index = SessionIndex::open(&path).unwrap();
            assert_eq!(index.search("persisted", None, 10).unwrap().len(), 1);
            index
                .conn
                .pragma_update(None, "user_version", INDEX_VERSION + 1)
                .unwrap();
        }
        let index = SessionIndex::open(&path).unwrap();
        assert!(index.search("persisted", None, 10).unwrap().is_empty());
        assert_eq!(index.stored_fingerprint("codex", "/a.jsonl").unwrap(), None);
    }

    #[test]
    fn snippet_centers_on_the_match_and_collapses_whitespace() {
        let text = format!(
            "{}\n\n关键词 出现在这里{}",
            "前".repeat(50),
            "后".repeat(200)
        );
        let snippet = make_snippet(&text, "关键词");
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
        assert!(snippet.contains(" 关键词 出现在这里"));
        assert!(!snippet.contains('\n'));

        assert_eq!(make_snippet("Hello World", "world"), "Hello World");
        assert_eq!(make_snippet("short", "missing"), "short");
    }
}
