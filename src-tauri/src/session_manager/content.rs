//! 会话源校验与按需取内容。
//!
//! 前端回传的 `sourcePath`、[`ContentRef`]、[`ImageRef`] 都不可信：
//!
//! - [`validate_source`]：`sourcePath` 必须落在对应 provider 的会话根目录下（规范化后比较，
//!   符号链接解析到真实位置再判断），SQLite 源只认 provider 自己的数据库
//! - [`resolve_content_ref`]：按 kind 校验后读取全文，单次 ≤ 32MB
//! - [`load_image`]：内联图片走 ContentRef 校验并解码，本地图片只允许会话相关目录，≤ 20MB

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use base64::Engine;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

use super::model::{ContentRef, ImageRef, ImageSource};
use super::providers::{hermes, opencode};
use super::{canonicalize_existing_path, provider_roots};

/// 单次读取的全文上限
pub const MAX_TEXT_BYTES: u64 = 32 * 1024 * 1024;
/// 图片（解码后）上限
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// `get_session_block_content` 单页默认 / 最大字符数（512K 字符 ≤ 2MB UTF-8）
pub const MAX_PAGE_CHARS: u32 = 512 * 1024;

/// JSONL 区间与当前文件对不上时的统一文案
const STALE: &str = "会话已更新，请重新读取";
const NOT_TEXT: &str = "引用的内容不是文本";
const OUTSIDE: &str = "引用的路径不在会话目录内";
const TOO_LARGE_TEXT: &str = "内容超过 32MB，请直接打开源文件查看";
const TOO_LARGE_IMAGE: &str = "图片超过 20MB，无法预览";

/// 本地图片允许的扩展名（SVG 单独拒绝：可能含脚本，前端只显示路径）
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// 校验通过的会话源。
#[derive(Debug, Clone)]
pub struct ValidatedSource {
    pub provider_id: String,
    /// 前端传入的原始 sourcePath（缓存键、`session_manager::load_messages` 分发用）
    pub raw: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLocation {
    /// 文件或目录（OpenCode 旧版存储是消息目录）；`path` 与 `root` 均已规范化
    Path { path: PathBuf, root: PathBuf },
    /// SQLite 中的一个会话；`db` 已规范化且等于 provider 自己的数据库
    Sqlite { db: PathBuf, session_id: String },
}

impl ValidatedSource {
    /// 交给解析器的路径：文件源用规范化后的路径，确保读到的就是校验过的那个文件
    pub fn load_path(&self) -> String {
        match &self.location {
            SourceLocation::Path { path, .. } => path.to_string_lossy().into_owned(),
            SourceLocation::Sqlite { .. } => self.raw.clone(),
        }
    }

    /// 工具落盘输出所在目录：`<sourcePath 去扩展名>/`（Claude `<sessionId>/tool-results/`）
    fn sidecar_base(&self) -> Option<PathBuf> {
        match &self.location {
            SourceLocation::Path { path, .. } if path.is_file() => Some(path.with_extension("")),
            SourceLocation::Path { path, .. } => Some(path.clone()),
            SourceLocation::Sqlite { .. } => None,
        }
    }
}

/// 校验 `sourcePath` 归属，见模块文档。
pub fn validate_source(provider_id: &str, source_path: &str) -> Result<ValidatedSource, String> {
    let location = match provider_id {
        "mcode" => {
            let id = source_path
                .strip_prefix("mcode:")
                .filter(|id| !id.is_empty())
                .ok_or("Invalid MCode session source")?;
            SourceLocation::Sqlite {
                db: canonicalize_existing_path(
                    &super::providers::mcode::database_path(),
                    "MCode database",
                )?,
                session_id: id.to_string(),
            }
        }
        "opencode" if source_path.starts_with("sqlite:") => validate_sqlite_source(
            source_path,
            opencode::parse_sqlite_source(source_path),
            &crate::opencode_config::get_opencode_db_path(),
        )?,
        "hermes" if source_path.starts_with("sqlite:") => validate_sqlite_source(
            source_path,
            hermes::parse_sqlite_source(source_path),
            &crate::hermes_config::get_hermes_dir().join("state.db"),
        )?,
        _ => {
            let roots = provider_roots(provider_id)?;
            let (root, path) = resolve_under_roots(provider_id, Path::new(source_path), &roots)?;
            SourceLocation::Path { path, root }
        }
    };
    Ok(ValidatedSource {
        provider_id: provider_id.to_string(),
        raw: source_path.to_string(),
        location,
    })
}

/// 要求 `source` 规范化后位于某个（存在的）root 之下，返回 `(root, source)` 的规范化路径。
/// 删除会话与读取会话共用这条校验。
pub(super) fn resolve_under_roots(
    provider_id: &str,
    source: &Path,
    roots: &[PathBuf],
) -> Result<(PathBuf, PathBuf), String> {
    let validated_source = canonicalize_existing_path(source, "session source")?;

    let mut saw_existing_root = false;
    for root in roots {
        if !root.exists() {
            continue;
        }
        saw_existing_root = true;
        let validated_root = canonicalize_existing_path(root, "session root")?;
        if validated_source.starts_with(&validated_root) {
            return Ok((validated_root, validated_source));
        }
    }

    if !saw_existing_root {
        return Err(format!(
            "Session root not found for provider {provider_id}: {}",
            roots
                .first()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    Err(format!(
        "Session source path is outside provider roots: {}",
        source.display()
    ))
}

fn validate_sqlite_source(
    raw: &str,
    parsed: Option<(PathBuf, String)>,
    expected_db: &Path,
) -> Result<SourceLocation, String> {
    let (db, session_id) = parsed.ok_or_else(|| format!("Invalid SQLite session source: {raw}"))?;
    let db = canonicalize_existing_path(&db, "session database")?;
    let expected = canonicalize_existing_path(expected_db, "session database")?;
    if db != expected {
        return Err("Session database is not the provider's own database".to_string());
    }
    Ok(SourceLocation::Sqlite { db, session_id })
}

// ── ContentRef ──────────────────────────────────────────────────────

/// 分页后的全文（§5.1 `BlockContent`）。`offset` / `limit` / `total_len` 均按字符计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockContent {
    pub text: String,
    pub total_len: u32,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
}

/// 从全文里截取 `[offset, offset + limit)` 字符；`limit` 默认且最大为 [`MAX_PAGE_CHARS`]。
pub fn paginate(full: &str, offset: Option<u32>, limit: Option<u32>) -> BlockContent {
    let offset = offset.unwrap_or(0) as usize;
    let limit = limit.unwrap_or(MAX_PAGE_CHARS).clamp(1, MAX_PAGE_CHARS) as usize;
    let total = full.chars().count();
    let text: String = full.chars().skip(offset).take(limit).collect();
    let end = offset.saturating_add(limit);
    let truncated = end < total;
    BlockContent {
        text,
        total_len: total.min(u32::MAX as usize) as u32,
        truncated,
        next_offset: truncated.then(|| end.min(u32::MAX as usize) as u32),
    }
}

/// 按 kind 校验并读取 [`ContentRef`] 指向的全文。
pub fn resolve_content_ref(
    source: &ValidatedSource,
    content: &ContentRef,
) -> Result<String, String> {
    match content {
        ContentRef::Jsonl {
            offset,
            len,
            pointer,
        } => {
            let path = jsonl_path(source)?;
            let line = read_jsonl_line(&path, *offset, *len)?;
            let value: Value = serde_json::from_slice(&line).map_err(|_| STALE.to_string())?;
            extract_text(&value, pointer)
        }
        ContentRef::Sqlite {
            table,
            id,
            column,
            pointer,
        } => {
            let SourceLocation::Sqlite { db, session_id } = &source.location else {
                return Err(STALE.to_string());
            };
            let raw = read_sqlite_cell(&source.provider_id, db, session_id, table, id, column)?;
            if pointer.is_empty() {
                return Ok(raw);
            }
            let value: Value = serde_json::from_str(&raw).map_err(|_| STALE.to_string())?;
            extract_text(&value, pointer)
        }
        ContentRef::File { rel_path, pointer } => {
            let SourceLocation::Path { root, .. } = &source.location else {
                return Err(OUTSIDE.to_string());
            };
            let file = safe_join(root, rel_path)?;
            let bytes = read_limited(&file, MAX_TEXT_BYTES, TOO_LARGE_TEXT)?;
            let value: Value = if source.provider_id == "gemini" {
                // 指针按解析器还原后的 `/messages/<i>` 编号，JSONL 也要同样回放
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(super::providers::gemini::parse_session_document)
                    .ok_or_else(|| STALE.to_string())?
            } else {
                serde_json::from_slice(&bytes).map_err(|_| STALE.to_string())?
            };
            // Gemini 多条思考合并成一个块：按解析器同一口径格式化，而不是返回 JSON
            if source.provider_id == "gemini" && pointer.ends_with("/thoughts") {
                if let Some(thoughts) = value
                    .pointer(pointer)
                    .and_then(super::providers::gemini::format_thoughts)
                {
                    return Ok(thoughts.text);
                }
            }
            extract_text(&value, pointer)
        }
        ContentRef::Sidecar { rel_path } => {
            let base = source.sidecar_base().ok_or_else(|| OUTSIDE.to_string())?;
            let SourceLocation::Path { root, .. } = &source.location else {
                return Err(OUTSIDE.to_string());
            };
            let base = canonicalize_existing_path(&base, "sidecar directory")
                .map_err(|_| "附属输出目录不存在".to_string())?;
            if !base.starts_with(root) {
                return Err(OUTSIDE.to_string());
            }
            let file = safe_join(&base, rel_path)?;
            let bytes = read_limited(&file, MAX_TEXT_BYTES, TOO_LARGE_TEXT)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// Grok Build 的会话正文文件名：sourcePath 是同目录的 `summary.json`
pub(crate) const GROK_CHAT_HISTORY: &str = "chat_history.jsonl";

/// `ContentRef::Jsonl` 指向的文件。一般就是会话源本身；Grok Build 的源是 `summary.json`，
/// 正文在同目录固定名的 `chat_history.jsonl`——只认这一个文件名，规范化后仍须在会话根内。
fn jsonl_path(source: &ValidatedSource) -> Result<PathBuf, String> {
    let SourceLocation::Path { path, root } = &source.location else {
        return Err(STALE.to_string());
    };
    if source.provider_id != "grokbuild" {
        return Ok(path.clone());
    }
    if path.file_name().and_then(|name| name.to_str()) != Some("summary.json") {
        return Err(OUTSIDE.to_string());
    }
    let chat = path
        .with_file_name(GROK_CHAT_HISTORY)
        .canonicalize()
        .map_err(|_| "会话正文文件不存在".to_string())?;
    if !chat.starts_with(root)
        || chat.file_name().and_then(|n| n.to_str()) != Some(GROK_CHAT_HISTORY)
    {
        return Err(OUTSIDE.to_string());
    }
    Ok(chat)
}

/// 读取 JSONL 的一行：区间必须在文件内、前一字节是换行（或文件开头）、
/// 以换行或文件末尾结束且中间不含换行。`len` 是否包含结尾的 `\n` 都接受。
fn read_jsonl_line(path: &Path, offset: u64, len: u32) -> Result<Vec<u8>, String> {
    if len == 0 {
        return Err(STALE.to_string());
    }
    if u64::from(len) > MAX_TEXT_BYTES {
        return Err(TOO_LARGE_TEXT.to_string());
    }
    let mut file = File::open(path).map_err(|_| "无法读取会话文件".to_string())?;
    let file_len = file
        .metadata()
        .map_err(|_| "无法读取会话文件".to_string())?
        .len();
    let end = offset
        .checked_add(u64::from(len))
        .filter(|end| *end <= file_len)
        .ok_or_else(|| STALE.to_string())?;

    // 多读前后各一个字节，用来判断行边界
    let start = offset.saturating_sub(1);
    let read_end = (end + 1).min(file_len);
    let mut buf = vec![0u8; (read_end - start) as usize];
    file.seek(SeekFrom::Start(start))
        .and_then(|_| file.read_exact(&mut buf))
        .map_err(|_| STALE.to_string())?;

    let line_start = (offset - start) as usize;
    if offset > 0 && buf[0] != b'\n' {
        return Err(STALE.to_string());
    }
    let mut line = &buf[line_start..line_start + len as usize];
    if line.last() == Some(&b'\n') {
        line = &line[..line.len() - 1];
    } else if end < file_len {
        let next = buf[buf.len() - 1];
        if next != b'\n' && next != b'\r' {
            return Err(STALE.to_string());
        }
    }
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    if line.is_empty() || line.contains(&b'\n') {
        return Err(STALE.to_string());
    }
    Ok(line.to_vec())
}

/// 按 JSON Pointer 取文本：
/// - 字符串 → 原文
/// - 数组 → 字符串项或 `{text}` 项按行拼接（Claude tool_result 的 content 数组）
/// - 对象 / 无文本项的数组 → 格式化 JSON（工具参数 `input_full`）
/// - 数字 / 布尔 / null → 拒绝
fn extract_text(value: &Value, pointer: &str) -> Result<String, String> {
    let target = if pointer.is_empty() {
        value
    } else {
        value.pointer(pointer).ok_or_else(|| STALE.to_string())?
    };
    match target {
        Value::String(text) => Ok(text.clone()),
        Value::Array(items) => {
            let texts: Vec<&str> = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.as_str()),
                    Value::Object(map) => map.get("text").and_then(Value::as_str),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                pretty_json(target)
            } else {
                Ok(texts.join("\n"))
            }
        }
        Value::Object(_) => pretty_json(target),
        _ => Err(NOT_TEXT.to_string()),
    }
}

fn pretty_json(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|_| NOT_TEXT.to_string())
}

/// 各 provider 允许回取的 (表, 列)；id 与 session_id 用参数绑定，确保只能读本会话的行
fn sqlite_allowed(provider_id: &str, table: &str, column: &str) -> bool {
    let (tables, columns): (&[&str], &[&str]) = match provider_id {
        "opencode" => (
            &["part", "message", "session_message"],
            &["data", "content"],
        ),
        // Hermes：正文、推理全文、tool_calls JSON（参数按 pointer 取）
        "hermes" => (&["messages"], &["content", "reasoning", "tool_calls"]),
        _ => (&[], &[]),
    };
    tables.contains(&table) && columns.contains(&column)
}

fn read_sqlite_cell(
    provider_id: &str,
    db: &Path,
    session_id: &str,
    table: &str,
    id: &str,
    column: &str,
) -> Result<String, String> {
    if !sqlite_allowed(provider_id, table, column) {
        return Err("不支持的内容引用".to_string());
    }
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "无法打开会话数据库".to_string())?;
    // 表名、列名来自上面的白名单，可以安全拼接
    let sql = format!(
        "SELECT length(CAST({column} AS BLOB)), {column} FROM {table} WHERE id = ?1 AND session_id = ?2"
    );
    let (size, cell): (Option<i64>, rusqlite::types::Value) = conn
        .query_row(&sql, rusqlite::params![id, session_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| STALE.to_string())?;
    if size.unwrap_or(0) as u64 > MAX_TEXT_BYTES {
        return Err(TOO_LARGE_TEXT.to_string());
    }
    match cell {
        rusqlite::types::Value::Text(text) => Ok(text),
        rusqlite::types::Value::Blob(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        _ => Err(NOT_TEXT.to_string()),
    }
}

/// 把相对路径拼到 `base` 下：只允许普通路径段，拼好后规范化（解析符号链接）再确认仍在 `base` 内。
fn safe_join(base: &Path, rel_path: &str) -> Result<PathBuf, String> {
    let rel = Path::new(rel_path);
    if rel_path.is_empty()
        || !rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(OUTSIDE.to_string());
    }
    let base = base
        .canonicalize()
        .map_err(|_| "会话目录不存在".to_string())?;
    let joined = base
        .join(rel)
        .canonicalize()
        .map_err(|_| "引用的文件不存在".to_string())?;
    if !joined.starts_with(&base) {
        return Err(OUTSIDE.to_string());
    }
    Ok(joined)
}

/// 读取整个文件，超过 `max` 字节直接拒绝（先看元数据，读时再限一次以防文件在读的过程中变大）
fn read_limited(path: &Path, max: u64, too_large: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| "无法读取文件".to_string())?;
    let meta = file.metadata().map_err(|_| "无法读取文件".to_string())?;
    if !meta.is_file() {
        return Err("引用的不是文件".to_string());
    }
    if meta.len() > max {
        return Err(too_large.to_string());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取文件".to_string())?;
    if bytes.len() as u64 > max {
        return Err(too_large.to_string());
    }
    Ok(bytes)
}

// ── 图片 ────────────────────────────────────────────────────────────

/// 读取图片原始字节。`Inline` 走 ContentRef 校验后 base64 解码；`LocalFile` 见 [`local_image_dirs`]。
pub fn load_image(source: &ValidatedSource, image: &ImageRef) -> Result<Vec<u8>, String> {
    match &image.source {
        ImageSource::Inline { content } => {
            let encoded = resolve_content_ref(source, content)?;
            decode_inline_image(&encoded, MAX_IMAGE_BYTES)
        }
        ImageSource::LocalFile { path } => {
            let canonical = canonical_local_image(path)?;
            let mut allowed = local_image_dirs(source);
            if !allowed.iter().any(|dir| canonical.starts_with(dir)) {
                // 最后才查会话的 project_dir（需要扫描该 provider 的会话列表）
                if let Some(project) = project_dir_for(source) {
                    allowed.push(project);
                }
            }
            read_local_image(&canonical, &allowed, MAX_IMAGE_BYTES)
        }
    }
}

/// 去掉 `data:<mime>;base64,` 前缀后解码；编码长度先粗检，解码后再精确检查上限。
fn decode_inline_image(encoded: &str, max: u64) -> Result<Vec<u8>, String> {
    let data = match encoded.strip_prefix("data:") {
        Some(rest) => rest
            .split_once(',')
            .map(|(_, data)| data)
            .ok_or("图片数据格式无效")?,
        None => encoded,
    };
    if data.len() as u64 > max / 3 * 4 + 8 {
        return Err(TOO_LARGE_IMAGE.to_string());
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine.decode(data.trim()).or_else(|_| {
        let compact: String = data.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        engine.decode(compact)
    });
    let bytes = bytes.map_err(|_| "图片数据格式无效".to_string())?;
    if bytes.len() as u64 > max {
        return Err(TOO_LARGE_IMAGE.to_string());
    }
    Ok(bytes)
}

/// 规范化本地图片路径：接受绝对路径或 `file://` URL，只允许位图扩展名
fn canonical_local_image(path: &str) -> Result<PathBuf, String> {
    let path = if path.starts_with("file://") {
        url::Url::parse(path)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or("图片路径无效")?
    } else {
        PathBuf::from(path)
    };
    if !path.is_absolute() {
        return Err("图片路径无效".to_string());
    }
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "svg" {
        return Err("SVG 图片不直接显示".to_string());
    }
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err("不支持的图片格式".to_string());
    }
    path.canonicalize()
        .map_err(|_| "图片文件不存在".to_string())
}

/// 本地图片允许的目录（均已规范化）：会话根、会话附属目录、provider 自有目录
fn local_image_dirs(source: &ValidatedSource) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let SourceLocation::Path { root, .. } = &source.location {
        dirs.push(root.clone());
    }
    if let Some(base) = source.sidecar_base() {
        dirs.push(base);
    }
    if source.provider_id == "codex" {
        dirs.push(crate::codex_config::get_codex_config_dir().join("visualizations"));
    }
    dirs.into_iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect()
}

fn read_local_image(canonical: &Path, allowed: &[PathBuf], max: u64) -> Result<Vec<u8>, String> {
    if !allowed.iter().any(|dir| canonical.starts_with(dir)) {
        return Err("图片不在会话项目或工具目录内，无法预览".to_string());
    }
    read_limited(canonical, max, TOO_LARGE_IMAGE)
}

/// 会话的项目目录：从该 provider 的会话列表里按 sourcePath 查（扫描有文件级缓存），
/// 结果按 (provider, sourcePath) 记住。项目目录为文件系统根时不采用。
fn project_dir_for(source: &ValidatedSource) -> Option<PathBuf> {
    static PROJECT_DIRS: LazyLock<Mutex<std::collections::HashMap<(String, String), PathBuf>>> =
        LazyLock::new(Default::default);

    let key = (source.provider_id.clone(), source.raw.clone());
    if let Some(dir) = PROJECT_DIRS.lock().ok()?.get(&key) {
        return Some(dir.clone());
    }

    use super::providers::{
        claude, codex, gemini, grokbuild, hermes, mcode, openclaw, opencode, pi,
    };
    let sessions = match source.provider_id.as_str() {
        "codex" => codex::scan_sessions(),
        "claude" => claude::scan_sessions(),
        "opencode" => opencode::scan_sessions(),
        "openclaw" => openclaw::scan_sessions(),
        "gemini" => gemini::scan_sessions(),
        "grokbuild" => grokbuild::scan_sessions(),
        "hermes" => hermes::scan_sessions(),
        "pi" => pi::scan_sessions(),
        "mcode" => mcode::scan_sessions(),
        _ => return None,
    };
    let project = sessions
        .into_iter()
        .find(|meta| meta.source_path.as_deref() == Some(source.raw.as_str()))?
        .project_dir?;
    let dir = Path::new(&project).canonicalize().ok()?;
    // 文件系统根没有父目录：不把整个磁盘当作项目目录
    dir.parent()?;
    if let Ok(mut map) = PROJECT_DIRS.lock() {
        map.insert(key, dir.clone());
    }
    Some(dir)
}

/// `reveal_session_path` 的路径校验：绝对路径或 `file://` URL，且必须已存在。
/// 只在文件管理器里定位，不打开文件，因此不限制目录。
pub fn resolve_reveal_path(path: &str) -> Result<PathBuf, String> {
    let path = if path.starts_with("file://") {
        url::Url::parse(path)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or("路径无效")?
    } else {
        PathBuf::from(path)
    };
    if !path.is_absolute() {
        return Err("路径无效".to_string());
    }
    path.canonicalize().map_err(|_| "路径不存在".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn file_source(root: &Path, path: &Path) -> ValidatedSource {
        let roots = vec![root.to_path_buf()];
        let (root, path) = resolve_under_roots("claude", path, &roots).expect("valid source");
        ValidatedSource {
            provider_id: "claude".into(),
            raw: path.to_string_lossy().into_owned(),
            location: SourceLocation::Path { path, root },
        }
    }

    fn jsonl(offset: u64, len: u32, pointer: &str) -> ContentRef {
        ContentRef::Jsonl {
            offset,
            len,
            pointer: pointer.into(),
        }
    }

    /// 两行 JSONL，返回 (源, 第二行的偏移, 第二行长度含换行)
    fn two_line_session() -> (tempfile::TempDir, ValidatedSource, u64, u32) {
        let dir = tempdir().unwrap();
        let line1 = "{\"a\":1}\n";
        let line2 = "{\"message\":{\"content\":[{\"text\":\"x\"},{\"text\":\"y\"}],\"n\":3,\"s\":\"full\"}}\n";
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, format!("{line1}{line2}")).unwrap();
        let source = file_source(dir.path(), &path);
        (dir, source, line1.len() as u64, line2.len() as u32)
    }

    #[test]
    fn source_outside_roots_or_via_symlink_is_rejected() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let secret = outside.path().join("secret.jsonl");
        std::fs::write(&secret, "{}").unwrap();
        let roots = vec![root.path().to_path_buf()];

        let err = resolve_under_roots("claude", &secret, &roots).unwrap_err();
        assert!(err.contains("outside provider roots"));

        let traversal = root.path().join("..").join(
            outside
                .path()
                .strip_prefix(outside.path().parent().unwrap())
                .unwrap(),
        );
        let err =
            resolve_under_roots("claude", &traversal.join("secret.jsonl"), &roots).unwrap_err();
        assert!(err.contains("outside provider roots"));

        #[cfg(unix)]
        {
            let link = root.path().join("link.jsonl");
            std::os::unix::fs::symlink(&secret, &link).unwrap();
            let err = resolve_under_roots("claude", &link, &roots).unwrap_err();
            assert!(err.contains("outside provider roots"));
        }
    }

    #[test]
    fn sqlite_source_must_be_provider_database() {
        let dir = tempdir().unwrap();
        let own = dir.path().join("opencode.db");
        let other = dir.path().join("other.db");
        std::fs::write(&own, "").unwrap();
        std::fs::write(&other, "").unwrap();

        let raw = format!("sqlite:{}:ses_1", own.display());
        let ok = validate_sqlite_source(&raw, opencode::parse_sqlite_source(&raw), &own).unwrap();
        assert!(
            matches!(ok, SourceLocation::Sqlite { ref session_id, .. } if session_id == "ses_1")
        );

        let raw = format!("sqlite:{}:ses_1", other.display());
        assert!(validate_sqlite_source(&raw, opencode::parse_sqlite_source(&raw), &own).is_err());
        assert!(hermes::parse_sqlite_source("sqlite:/x/state.db#").is_none());
    }

    /// 超长注入文本与压缩摘要只下发预览：`full` 必须能取回原文（Claude 与 Codex 解析器）
    #[test]
    fn oversized_injected_text_and_summary_refs_resolve_to_original() {
        use super::super::model::{SessionBlock, SessionMessage};
        use super::super::providers::{claude, codex};
        use serde_json::json;

        fn check(source: &ValidatedSource, messages: &[SessionMessage], originals: &[&str]) {
            let mut resolved = Vec::new();
            for block in messages.iter().flat_map(|m| &m.blocks) {
                let (preview, full) = match block {
                    SessionBlock::Text {
                        text,
                        full: Some(full),
                    } => (text.clone(), full),
                    SessionBlock::Event {
                        text: Some(text),
                        full: Some(full),
                        ..
                    } => (text.clone(), full),
                    _ => continue,
                };
                let text = resolve_content_ref(source, full).unwrap();
                assert!(text.starts_with(preview.trim_end()), "预览应是全文开头");
                assert!(text.chars().count() > preview.chars().count());
                resolved.push(text);
            }
            let mut expected: Vec<&str> = originals.to_vec();
            expected.sort_unstable();
            let mut resolved: Vec<&str> = resolved.iter().map(String::as_str).collect();
            resolved.sort_unstable();
            assert_eq!(resolved, expected);
        }

        let dir = tempdir().unwrap();
        let notification = format!(
            "<task-notification>{}</task-notification>",
            "n".repeat(9000)
        );
        let summary = format!("This session is being continued. {}", "s".repeat(800));
        let lines = [
            json!({ "type": "user", "uuid": "u1", "message": { "role": "user", "content": notification } }),
            json!({ "type": "user", "uuid": "u2", "message": { "role": "user", "content": "short question" } }),
            json!({ "type": "user", "isCompactSummary": true, "uuid": "u3", "message": { "role": "user", "content": summary } }),
        ];
        let path = dir.path().join("claude.jsonl");
        std::fs::write(&path, lines.map(|l| format!("{l}\n")).concat()).unwrap();
        let messages = claude::load_messages(&path).unwrap();
        check(
            &file_source(dir.path(), &path),
            &messages,
            &[&notification, &summary],
        );
        // 短文本照常完整下发
        assert!(messages.iter().flat_map(|m| &m.blocks).any(
            |b| matches!(b, SessionBlock::Text { text, full: None } if text == "short question")
        ));

        let developer = format!("<permissions instructions>{}", "d".repeat(9000));
        let agents = format!("# AGENTS.md instructions for /repo\n\n{}", "a".repeat(9000));
        let compacted = format!("Summary: {}", "c".repeat(800));
        let ts = "2026-03-06T21:50:12Z";
        let lines = [
            json!({ "timestamp": ts, "type": "response_item", "payload": { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": developer }] } }),
            json!({ "timestamp": ts, "type": "response_item", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": agents }, { "type": "input_text", "text": "real question" }] } }),
            json!({ "timestamp": ts, "type": "compacted", "payload": { "message": compacted } }),
        ];
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, lines.map(|l| format!("{l}\n")).concat()).unwrap();
        let messages = codex::load_messages(&path).unwrap();
        check(
            &file_source(dir.path(), &path),
            &messages,
            &[&developer, &agents, &compacted],
        );
    }

    /// Grok Build：sourcePath 是 summary.json，Jsonl 引用读同目录的 chat_history.jsonl
    #[test]
    fn grokbuild_jsonl_refs_read_chat_history_next_to_summary() {
        use super::super::model::SessionBlock;
        use super::super::providers::grokbuild;
        use serde_json::json;

        let root = tempdir().unwrap();
        let dir = root.path().join("s1");
        std::fs::create_dir(&dir).unwrap();
        let summary = dir.join("summary.json");
        std::fs::write(&summary, "{}").unwrap();
        let output = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
        let lines = [
            json!({ "type": "user", "content": "run it" }),
            json!({ "type": "assistant", "content": "", "tool_calls": [{ "id": "c1", "function": { "name": "bash", "arguments": "{\"command\":\"ls\"}" } }] }),
            json!({ "type": "tool", "tool_call_id": "c1", "content": output }),
        ];
        std::fs::write(
            dir.join(GROK_CHAT_HISTORY),
            lines.map(|l| format!("{l}\n")).concat(),
        )
        .unwrap();

        let messages = grokbuild::load_messages(&summary).unwrap();
        let full = messages
            .iter()
            .flat_map(|m| &m.blocks)
            .find_map(|b| match b {
                SessionBlock::ToolResult { full, .. } => full.clone(),
                _ => None,
            })
            .expect("长输出应带引用");
        let mut source = file_source(root.path(), &summary);
        source.provider_id = "grokbuild".into();
        assert_eq!(resolve_content_ref(&source, &full).unwrap(), output);

        // 源不是 summary.json 时不改读别的文件
        let mut other = file_source(root.path(), &dir.join(GROK_CHAT_HISTORY));
        other.provider_id = "grokbuild".into();
        assert_eq!(resolve_content_ref(&other, &full).unwrap_err(), OUTSIDE);

        // chat_history.jsonl 是指向会话根外的符号链接时拒绝
        #[cfg(unix)]
        {
            let outside = tempdir().unwrap();
            let target = outside.path().join(GROK_CHAT_HISTORY);
            std::fs::rename(dir.join(GROK_CHAT_HISTORY), &target).unwrap();
            std::os::unix::fs::symlink(&target, dir.join(GROK_CHAT_HISTORY)).unwrap();
            assert_eq!(resolve_content_ref(&source, &full).unwrap_err(), OUTSIDE);
        }
    }

    /// Hermes：推理全文与 tool_calls 参数按白名单列回取，仍限定本会话的行
    #[test]
    fn hermes_sqlite_refs_cover_reasoning_and_tool_calls() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        use super::super::model::SessionBlock;
        use super::super::providers::hermes;

        let dir = tempdir().unwrap();
        let db = dir.path().join("state.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                 role TEXT NOT NULL, content TEXT, tool_call_id TEXT, tool_calls TEXT,
                 tool_name TEXT, timestamp REAL NOT NULL, reasoning TEXT);",
        )
        .unwrap();
        let reasoning = "r".repeat(1000);
        let command = "c".repeat(1000);
        let calls = serde_json::json!([
            { "id": "c1", "function": { "name": "terminal", "arguments": serde_json::json!({ "command": command }).to_string() } },
            { "id": "c2", "name": "terminal", "arguments": serde_json::json!({ "command": command }).to_string() },
        ]);
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls, timestamp, reasoning)
             VALUES ('s1', 'assistant', 'ok', ?1, 1.0, ?2)",
            rusqlite::params![calls.to_string(), reasoning],
        )
        .unwrap();
        drop(conn);

        let messages =
            hermes::load_messages_sqlite(&format!("sqlite:{}#s1", db.display())).unwrap();
        let source = ValidatedSource {
            provider_id: "hermes".into(),
            raw: String::new(),
            location: SourceLocation::Sqlite {
                db: db.canonicalize().unwrap(),
                session_id: "s1".into(),
            },
        };
        let mut seen = Vec::new();
        for block in messages.iter().flat_map(|m| &m.blocks) {
            let full = match block {
                SessionBlock::Thinking { full: Some(f), .. } => f,
                SessionBlock::ToolCall {
                    input_full: Some(f),
                    ..
                } => f,
                _ => continue,
            };
            seen.push(resolve_content_ref(&source, full).unwrap());
        }
        let args = serde_json::json!({ "command": command }).to_string();
        assert_eq!(seen, vec![reasoning, args.clone(), args]);

        // 别的会话读不到这一行；白名单外的列被拒
        let other = ValidatedSource {
            location: SourceLocation::Sqlite {
                db: db.canonicalize().unwrap(),
                session_id: "s2".into(),
            },
            ..source.clone()
        };
        let cell = |column: &str| ContentRef::Sqlite {
            table: "messages".into(),
            id: "1".into(),
            column: column.into(),
            pointer: String::new(),
        };
        assert_eq!(
            resolve_content_ref(&other, &cell("reasoning")).unwrap_err(),
            STALE
        );
        assert!(resolve_content_ref(&source, &cell("role")).is_err());
    }

    /// Gemini：多条思考合并成一个块，全文按同一口径格式化（不是 JSON）
    #[test]
    fn gemini_merged_thoughts_resolve_to_formatted_text() {
        use super::super::model::SessionBlock;
        use super::super::providers::gemini;
        use serde_json::json;

        let root = tempdir().unwrap();
        let chats = root.path().join("hash").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let path = chats.join("session-1.json");
        let long = "d".repeat(500);
        let session = json!({
            "sessionId": "s1",
            "messages": [
                { "id": "1", "type": "user", "content": "hi" },
                { "id": "2", "type": "gemini", "content": "done", "thoughts": [
                    { "subject": "Plan", "description": long, "timestamp": "x" },
                    { "subject": "Check", "description": "ok" }
                ] }
            ]
        });
        std::fs::write(&path, session.to_string()).unwrap();

        let messages = gemini::load_messages(&path).unwrap();
        let (preview, full) = messages
            .iter()
            .flat_map(|m| &m.blocks)
            .find_map(|b| match b {
                SessionBlock::Thinking {
                    text,
                    full: Some(full),
                    ..
                } => Some((text.clone(), full.clone())),
                _ => None,
            })
            .expect("长思考应带引用");
        let mut source = file_source(root.path(), &path);
        source.provider_id = "gemini".into();
        let text = resolve_content_ref(&source, &full).unwrap();
        assert_eq!(text, format!("**Plan**\n\n{long}\n\n**Check**\n\nok"));
        assert!(text.starts_with(&preview));
    }

    /// Gemini JSONL：`/messages/<i>` 指针按回放后的消息编号解析
    #[test]
    fn gemini_jsonl_tool_output_resolves_by_replayed_index() {
        use super::super::model::SessionBlock;
        use super::super::providers::gemini;
        use serde_json::json;

        let root = tempdir().unwrap();
        let chats = root.path().join("hash").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let path = chats.join("session-1.jsonl");
        let long: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let lines = [
            json!({ "sessionId": "s1", "projectHash": "h" }),
            json!({ "id": "1", "type": "user", "content": [{ "text": "hi" }] }),
            json!({ "id": "2", "type": "gemini", "content": "", "toolCalls": [
                { "id": "c1", "name": "run_shell_command", "args": { "command": "ls" },
                  "status": "success", "resultDisplay": long }
            ] }),
        ];
        let data: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, data).unwrap();

        let messages = gemini::load_messages(&path).unwrap();
        let full = messages
            .iter()
            .flat_map(|m| &m.blocks)
            .find_map(|b| match b {
                SessionBlock::ToolResult {
                    full: Some(full), ..
                } => Some(full.clone()),
                _ => None,
            })
            .expect("长输出应带引用");
        let mut source = file_source(root.path(), &path);
        source.provider_id = "gemini".into();
        assert_eq!(resolve_content_ref(&source, &full).unwrap(), long);
    }

    #[test]
    fn jsonl_ref_reads_line_and_pointer() {
        let (_dir, source, offset, len) = two_line_session();
        // len 含换行与不含换行都接受
        for len in [len, len - 1] {
            let text = resolve_content_ref(&source, &jsonl(offset, len, "/message/s")).unwrap();
            assert_eq!(text, "full");
        }
        let joined = resolve_content_ref(&source, &jsonl(offset, len, "/message/content")).unwrap();
        assert_eq!(joined, "x\ny");
        let first = resolve_content_ref(&source, &jsonl(0, 8, "")).unwrap();
        assert!(first.contains("\"a\": 1"));
    }

    #[test]
    fn jsonl_ref_out_of_bounds_is_rejected() {
        let (_dir, source, offset, len) = two_line_session();
        let err = resolve_content_ref(&source, &jsonl(offset, len + 1, "/message/s")).unwrap_err();
        assert_eq!(err, STALE);
        let err = resolve_content_ref(&source, &jsonl(u64::MAX, 1, "")).unwrap_err();
        assert_eq!(err, STALE);
        let err = resolve_content_ref(&source, &jsonl(0, 0, "")).unwrap_err();
        assert_eq!(err, STALE);
    }

    #[test]
    fn jsonl_ref_partial_line_is_rejected() {
        let (_dir, source, offset, len) = two_line_session();
        // 起点不在行首
        let err = resolve_content_ref(&source, &jsonl(offset + 1, len - 1, "")).unwrap_err();
        assert_eq!(err, STALE);
        // 终点不在行尾
        let err = resolve_content_ref(&source, &jsonl(offset, len - 5, "")).unwrap_err();
        assert_eq!(err, STALE);
        // 跨两行
        let err = resolve_content_ref(&source, &jsonl(0, 8 + len, "")).unwrap_err();
        assert_eq!(err, STALE);
    }

    #[test]
    fn jsonl_ref_pointer_must_resolve_to_text() {
        let (_dir, source, offset, len) = two_line_session();
        let err = resolve_content_ref(&source, &jsonl(offset, len, "/message/n")).unwrap_err();
        assert_eq!(err, NOT_TEXT);
        let err =
            resolve_content_ref(&source, &jsonl(offset, len, "/message/missing")).unwrap_err();
        assert_eq!(err, STALE);
    }

    #[test]
    fn sidecar_and_file_refs_reject_traversal() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        let session = root.path().join("abc.jsonl");
        std::fs::write(&session, "{}\n").unwrap();
        std::fs::create_dir_all(root.path().join("abc/tool-results")).unwrap();
        std::fs::write(root.path().join("abc/tool-results/out.txt"), "full output").unwrap();
        std::fs::write(
            root.path().join("part.json"),
            "{\"state\":{\"output\":\"ok\"}}",
        )
        .unwrap();
        let source = file_source(root.path(), &session);

        let ok = resolve_content_ref(
            &source,
            &ContentRef::Sidecar {
                rel_path: "tool-results/out.txt".into(),
            },
        )
        .unwrap();
        assert_eq!(ok, "full output");
        let ok = resolve_content_ref(
            &source,
            &ContentRef::File {
                rel_path: "part.json".into(),
                pointer: "/state/output".into(),
            },
        )
        .unwrap();
        assert_eq!(ok, "ok");

        let outside_abs = outside
            .path()
            .join("secret.txt")
            .to_string_lossy()
            .into_owned();
        for rel in [
            "../../etc/passwd",
            "tool-results/../../x",
            outside_abs.as_str(),
            "",
        ] {
            let err = resolve_content_ref(
                &source,
                &ContentRef::Sidecar {
                    rel_path: rel.into(),
                },
            )
            .unwrap_err();
            assert_eq!(err, OUTSIDE, "sidecar {rel}");
            let err = resolve_content_ref(
                &source,
                &ContentRef::File {
                    rel_path: rel.into(),
                    pointer: String::new(),
                },
            )
            .unwrap_err();
            assert_eq!(err, OUTSIDE, "file {rel}");
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("secret.txt"),
                root.path().join("abc/tool-results/link.txt"),
            )
            .unwrap();
            let err = resolve_content_ref(
                &source,
                &ContentRef::Sidecar {
                    rel_path: "tool-results/link.txt".into(),
                },
            )
            .unwrap_err();
            assert_eq!(err, OUTSIDE);
        }
    }

    #[test]
    fn sqlite_ref_is_scoped_to_whitelist_and_session() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE part (id TEXT, session_id TEXT, data TEXT);
             INSERT INTO part VALUES ('p1', 'ses_1', '{\"state\":{\"output\":\"hello\"}}');
             INSERT INTO part VALUES ('p2', 'ses_2', '{\"state\":{\"output\":\"other\"}}');",
        )
        .unwrap();
        drop(conn);
        let source = ValidatedSource {
            provider_id: "opencode".into(),
            raw: String::new(),
            location: SourceLocation::Sqlite {
                db,
                session_id: "ses_1".into(),
            },
        };
        let sqlite = |table: &str, id: &str, column: &str| ContentRef::Sqlite {
            table: table.into(),
            id: id.into(),
            column: column.into(),
            pointer: "/state/output".into(),
        };
        assert_eq!(
            resolve_content_ref(&source, &sqlite("part", "p1", "data")).unwrap(),
            "hello"
        );
        // 其他会话的行读不到
        assert_eq!(
            resolve_content_ref(&source, &sqlite("part", "p2", "data")).unwrap_err(),
            STALE
        );
        // 表 / 列不在白名单
        assert!(resolve_content_ref(&source, &sqlite("sqlite_master", "p1", "sql")).is_err());
        assert!(
            resolve_content_ref(&source, &sqlite("part; DROP TABLE part", "p1", "data")).is_err()
        );
    }

    #[test]
    fn reveal_path_requires_existing_absolute_path() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(resolve_reveal_path(&file.to_string_lossy()).is_ok());
        assert!(resolve_reveal_path("relative/a.txt").is_err());
        assert!(resolve_reveal_path(&dir.path().join("missing").to_string_lossy()).is_err());
    }

    #[test]
    fn paginate_counts_chars() {
        let page = paginate("你好世界abc", Some(2), Some(3));
        assert_eq!(page.text, "世界a");
        assert_eq!(page.total_len, 7);
        assert!(page.truncated);
        assert_eq!(page.next_offset, Some(5));

        let last = paginate("你好世界abc", Some(5), None);
        assert_eq!(last.text, "bc");
        assert!(!last.truncated);
        assert_eq!(last.next_offset, None);
    }

    #[test]
    fn inline_image_decodes_and_enforces_limit() {
        let engine = base64::engine::general_purpose::STANDARD;
        let encoded = engine.encode([1u8, 2, 3, 4]);
        assert_eq!(
            decode_inline_image(&format!("data:image/png;base64,{encoded}"), 1024).unwrap(),
            vec![1, 2, 3, 4]
        );
        let big = engine.encode(vec![0u8; 64]);
        assert_eq!(decode_inline_image(&big, 32).unwrap_err(), TOO_LARGE_IMAGE);
        assert!(decode_inline_image("not base64!!", 1024).is_err());
    }

    #[test]
    fn local_image_checks_extension_dir_and_size() {
        let allowed = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let ok = allowed.path().join("a.png");
        std::fs::write(&ok, vec![0u8; 16]).unwrap();
        let other = outside.path().join("b.png");
        std::fs::write(&other, vec![0u8; 16]).unwrap();
        let dirs = vec![allowed.path().canonicalize().unwrap()];

        let canonical = canonical_local_image(&ok.to_string_lossy()).unwrap();
        assert_eq!(read_local_image(&canonical, &dirs, 1024).unwrap().len(), 16);
        assert_eq!(
            read_local_image(&canonical, &dirs, 8).unwrap_err(),
            TOO_LARGE_IMAGE
        );

        let canonical = canonical_local_image(&other.to_string_lossy()).unwrap();
        assert!(read_local_image(&canonical, &dirs, 1024).is_err());

        let file_url = url::Url::from_file_path(&ok).unwrap().to_string();
        assert!(canonical_local_image(&file_url).is_ok());
        assert!(canonical_local_image("relative.png").is_err());
        assert!(canonical_local_image("/tmp/x.svg").is_err());
        assert!(canonical_local_image("/etc/passwd").is_err());
    }
}
