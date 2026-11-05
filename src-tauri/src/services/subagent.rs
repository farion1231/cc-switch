//! Claude Code 用户级子代理（`~/.claude/agents/*.md`）管理
//!
//! 模式与 Prompts 相同：数据库存完整原始 markdown（唯一事实源），启用时投影成
//! 磁盘文件。和 Prompts 不同的是这个目录 Claude Code 自己（`/agents`）和用户也会
//! 直接改，所以每次写文件都记下 sha256（`written_hash`）：
//! - 磁盘哈希 == written_hash：文件仍是我们写的那份，可以放心覆盖/删除；
//! - 不相等：文件被外部改过（偏离）或根本不是我们写的，不加 `force` 一律拒绝动它。
//!
//! 后端只解析 frontmatter 做校验和取 name/description，绝不重新序列化回写——
//! frontmatter 字段在 Claude Code 里不断新增，任何“解析后再生成”都会丢字段。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::write_text_file;
use crate::database::{Database, SubagentRecord};
use crate::error::AppError;

/// 单个子代理文件的大小上限。正常文件只有几 KB；扫描时防止把误放进目录的大文件
/// 整个读进内存再塞进数据库。
const MAX_SUBAGENT_BYTES: usize = 1024 * 1024;

/// 文件读写 + 数据库更新必须作为一个整体串行执行，否则两个并发操作可能都通过了
/// 哈希检查，再先后写文件，written_hash 就和磁盘对不上了。
static SUBAGENT_LOCK: Mutex<()> = Mutex::new(());

// ─── 对外类型（serde camelCase，与前端契约一致）─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Synced,
    Drifted,
    Missing,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentDriftStrategy {
    AdoptDisk,
    OverwriteDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    pub name: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub content: String,
    pub enabled: bool,
    pub source: String,
    pub status: SubagentStatus,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub content: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmanagedSubagent {
    pub file_name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSubagent {
    pub plugin_name: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentsDirInfo {
    pub path: String,
    pub display_path: String,
    pub exists: bool,
}

// ─── 错误（前端按前缀识别冲突类型，前缀不能改）──────────────────────────────

fn err_drifted(detail: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("SUBAGENT_DRIFTED: {detail}"))
}

fn err_file_exists(detail: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("SUBAGENT_FILE_EXISTS: {detail}"))
}

fn err_invalid(detail: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("SUBAGENT_INVALID: {detail}"))
}

fn err_name_taken(detail: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("SUBAGENT_NAME_TAKEN: {detail}"))
}

fn err_not_found(detail: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("SUBAGENT_NOT_FOUND: {detail}"))
}

// ─── frontmatter 解析 ─────────────────────────────────────────────────────

/// 从 frontmatter 取出的、后端关心的少数几个字段
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubagentMeta {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
}

/// 拆出 `---` 包围的 frontmatter 文本。只认文件开头（允许 UTF-8 BOM）的 `---` 行。
fn split_frontmatter(content: &str) -> Option<&str> {
    let text = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Some(&text[start..offset]);
        }
        offset += line.len();
    }
    None
}

/// 严格 YAML 解析失败时的宽松兜底：按顶层 `key: value` 行切分。
///
/// 为什么需要：Claude Code `/agents` 生成的文件，description 常写成一长串带
/// `Context: ...` / `<example>` 的普通标量，里面的 `: ` 让严格 YAML 直接报错，
/// 但 Claude Code 自己能正常加载这些文件。这里只用于取 name/description 做校验
/// 和展示，内容本身原样保存，所以宽松解析不会造成任何数据改写。
fn parse_frontmatter_loose(yaml: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for raw in yaml.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                if !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    fields.push((key.to_string(), value.trim().to_string()));
                    continue;
                }
            }
        }
        // 缩进行或无法识别的行视为上一个字段的续行
        if let Some((_, value)) = fields.last_mut() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line.trim());
        }
    }
    for (_, value) in fields.iter_mut() {
        let trimmed = value.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                trimmed
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .unwrap_or(trimmed);
        *value = unquoted.to_string();
    }
    fields
}

fn yaml_scalar_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 解析子代理 markdown，返回 name/description/model；不合法时返回人类可读原因
/// （不带错误前缀，由调用方决定包装方式）。
pub(crate) fn parse_subagent_meta(content: &str) -> Result<SubagentMeta, String> {
    let meta = extract_meta(content)?;
    if !is_valid_identifier(&meta.name) {
        return Err(format!(
            "name {:?} 不合法：只能包含小写字母、数字和连字符，以字母或数字开头，最长 64 个字符",
            meta.name
        ));
    }
    Ok(meta)
}

/// 只取字段、检查必填，不校验 name 格式（插件子代理不归我们管，只做展示）
fn extract_meta(content: &str) -> Result<SubagentMeta, String> {
    let yaml = split_frontmatter(content)
        .ok_or_else(|| "缺少以 --- 包围的 YAML frontmatter".to_string())?;

    let (name, description, model) = match serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        Ok(serde_yaml::Value::Mapping(map)) => {
            let get = |key: &str| {
                map.get(serde_yaml::Value::String(key.to_string()))
                    .and_then(yaml_scalar_to_string)
            };
            (get("name"), get("description"), get("model"))
        }
        Ok(serde_yaml::Value::Null) => (None, None, None),
        Ok(_) => return Err("frontmatter 必须是 key: value 映射".to_string()),
        Err(_) => {
            let fields = parse_frontmatter_loose(yaml);
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            (get("name"), get("description"), get("model"))
        }
    };

    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| "frontmatter 缺少必填字段 name".to_string())?;
    let description = description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .ok_or_else(|| "frontmatter 缺少必填字段 description".to_string())?;
    let model = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    Ok(SubagentMeta {
        name,
        description,
        model,
    })
}

/// `^[a-z0-9][a-z0-9-]{0,63}$`。手写而不是正则：规则简单，而且这条规则同时就是
/// 文件名的路径穿越防线（不允许 `.`、`/`、`\`），写成显式字符判断更一目了然。
fn is_valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn validate_file_name(file_name: &str) -> Result<(), AppError> {
    if !is_valid_identifier(file_name) {
        return Err(err_invalid(format!(
            "文件名 {file_name:?} 不合法：只能包含小写字母、数字和连字符，以字母或数字开头，最长 64 个字符"
        )));
    }
    Ok(())
}

fn validate_content(content: &str) -> Result<SubagentMeta, AppError> {
    if content.len() > MAX_SUBAGENT_BYTES {
        return Err(err_invalid(format!(
            "内容超过 {} KB 上限",
            MAX_SUBAGENT_BYTES / 1024
        )));
    }
    parse_subagent_meta(content).map_err(err_invalid)
}

// ─── 文件辅助 ─────────────────────────────────────────────────────────────

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 文件路径。file_name 在入库前已校验，这里再校验一次：同步恢复的数据库可能
/// 含任意值，不能让它被 join 成目录外的路径。
fn file_path_for(dir: &Path, file_name: &str) -> Result<PathBuf, AppError> {
    validate_file_name(file_name)?;
    Ok(dir.join(format!("{file_name}.md")))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

fn disk_hash(path: &Path) -> Result<Option<String>, AppError> {
    Ok(read_optional(path)?.map(|bytes| sha256_hex(&bytes)))
}

fn remove_if_exists(path: &Path) -> Result<(), AppError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::io(path, e)),
    }
}

/// 数据库写失败时把文件恢复到操作前的样子（尽力而为）
fn restore_file(path: &Path, previous: Option<&[u8]>) {
    let result = match previous {
        Some(bytes) => crate::config::atomic_write(path, bytes),
        None => remove_if_exists(path),
    };
    if let Err(e) = result {
        log::warn!("回滚子代理文件失败 {}: {e}", path.display());
    }
}

/// 写入前检查：目标位置没有文件，或文件哈希等于 `owned_hash`（还是我们写的那份）。
///
/// `owned_hash` 为 `None` 表示这个路径当前不归本记录管（新建、改名目标、停用后
/// 重新启用），此时任何已有文件都是外来文件。
fn ensure_writable(
    path: &Path,
    owned_hash: Option<&str>,
    force: bool,
) -> Result<Option<Vec<u8>>, AppError> {
    let previous = read_optional(path)?;
    if let Some(bytes) = previous.as_ref() {
        let matches = owned_hash.is_some_and(|h| h == sha256_hex(bytes));
        if !matches && !force {
            return Err(if owned_hash.is_some() {
                err_drifted(format!(
                    "{} 已在 CC Switch 之外被修改，覆盖会丢失这些修改",
                    path.display()
                ))
            } else {
                err_file_exists(format!("{} 已存在且不由 CC Switch 管理", path.display()))
            });
        }
    }
    Ok(previous)
}

/// 删除前检查：文件不存在（无需删除）或哈希等于 `owned_hash`。
/// 返回删除前的文件内容，供数据库失败时回滚。
fn ensure_removable(
    path: &Path,
    owned_hash: Option<&str>,
    force: bool,
) -> Result<Option<Vec<u8>>, AppError> {
    let previous = read_optional(path)?;
    if let Some(bytes) = previous.as_ref() {
        let matches = owned_hash.is_some_and(|h| h == sha256_hex(bytes));
        if !matches && !force {
            return Err(err_drifted(format!(
                "{} 已在 CC Switch 之外被修改，删除会丢失这些修改",
                path.display()
            )));
        }
    }
    Ok(previous)
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    // 锁里只保护文件与数据库的一致性，panic 中毒后状态仍可由哈希重新判断，继续用即可
    SUBAGENT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ─── 服务 ─────────────────────────────────────────────────────────────────

pub struct SubagentService;

impl SubagentService {
    /// 用户级子代理目录：有 Claude 覆盖目录时用 `<override>/agents`，否则
    /// `~/.claude/agents`（走 get_home_dir，测试可用 CC_SWITCH_TEST_HOME 隔离）。
    pub fn agents_dir() -> PathBuf {
        if let Some(custom) = crate::settings::get_claude_override_dir() {
            return custom.join("agents");
        }
        crate::config::get_home_dir().join(".claude").join("agents")
    }

    /// Claude 配置根目录（插件清单在 `<root>/plugins` 下）
    pub fn claude_root_dir() -> PathBuf {
        crate::settings::get_claude_override_dir()
            .unwrap_or_else(|| crate::config::get_home_dir().join(".claude"))
    }

    pub fn dir_info(dir: &Path) -> SubagentsDirInfo {
        SubagentsDirInfo {
            path: dir.to_string_lossy().to_string(),
            display_path: crate::prompt_files::display_path(dir),
            exists: dir.is_dir(),
        }
    }

    fn status_of(record: &SubagentRecord, path: &Path) -> SubagentStatus {
        if !record.enabled {
            return SubagentStatus::Disabled;
        }
        match disk_hash(path) {
            Ok(None) => SubagentStatus::Missing,
            Ok(Some(hash)) if record.written_hash.as_deref() == Some(hash.as_str()) => {
                SubagentStatus::Synced
            }
            Ok(Some(_)) => SubagentStatus::Drifted,
            Err(e) => {
                // 读不了（权限、同名目录等）也说明磁盘状态不是我们写的那样
                log::warn!("读取子代理文件失败 {}: {e}", path.display());
                SubagentStatus::Drifted
            }
        }
    }

    fn to_dto(dir: &Path, record: SubagentRecord) -> Subagent {
        // file_name 非法（只可能来自外部恢复的数据库）时仍要能列出来让用户处理，
        // 路径只用于展示，状态按“缺失”算，绝不去碰那个路径
        let (path, status) = match file_path_for(dir, &record.file_name) {
            Ok(path) => {
                let status = Self::status_of(&record, &path);
                (path, status)
            }
            Err(_) => (
                dir.join(format!("{}.md", record.file_name)),
                if record.enabled {
                    SubagentStatus::Missing
                } else {
                    SubagentStatus::Disabled
                },
            ),
        };
        Subagent {
            id: record.id,
            name: record.name,
            file_name: record.file_name,
            description: record.description,
            content: record.content,
            enabled: record.enabled,
            source: record.source,
            status,
            file_path: path.to_string_lossy().to_string(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }

    fn require(db: &Database, id: &str) -> Result<SubagentRecord, AppError> {
        db.get_subagent(id)?
            .ok_or_else(|| err_not_found(format!("子代理 {id} 不存在")))
    }

    pub fn list(db: &Database, dir: &Path) -> Result<Vec<Subagent>, AppError> {
        Ok(db
            .list_subagents()?
            .into_iter()
            .map(|record| Self::to_dto(dir, record))
            .collect())
    }

    pub fn upsert(
        db: &Database,
        dir: &Path,
        input: SubagentInput,
        force: bool,
    ) -> Result<Subagent, AppError> {
        let _guard = lock();
        let meta = validate_content(&input.content)?;
        match input.id.as_deref().filter(|id| !id.is_empty()) {
            None => Self::create(db, dir, input, meta, force),
            Some(id) => {
                let existing = Self::require(db, id)?;
                Self::update(db, dir, existing, input, meta, force)
            }
        }
    }

    fn create(
        db: &Database,
        dir: &Path,
        input: SubagentInput,
        meta: SubagentMeta,
        force: bool,
    ) -> Result<Subagent, AppError> {
        let file_name = input
            .file_name
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| meta.name.clone());
        let path = file_path_for(dir, &file_name)?;
        if db.get_subagent_by_file_name(&file_name)?.is_some() {
            return Err(err_name_taken(format!(
                "文件名 {file_name} 已被另一个子代理使用"
            )));
        }

        let mut previous = None;
        let written_hash = if input.enabled {
            previous = Some(ensure_writable(&path, None, force)?);
            write_text_file(&path, &input.content)?;
            Some(sha256_hex(input.content.as_bytes()))
        } else {
            None
        };

        let now = now_ts();
        let record = SubagentRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: meta.name,
            file_name,
            description: Some(meta.description),
            content: input.content,
            enabled: input.enabled,
            source: "local".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_path: None,
            written_hash,
            created_at: Some(now),
            updated_at: Some(now),
        };
        if let Err(e) = db.save_subagent(&record) {
            if let Some(previous) = previous {
                restore_file(&path, previous.as_deref());
            }
            return Err(e);
        }
        Ok(Self::to_dto(dir, record))
    }

    fn update(
        db: &Database,
        dir: &Path,
        existing: SubagentRecord,
        input: SubagentInput,
        meta: SubagentMeta,
        force: bool,
    ) -> Result<Subagent, AppError> {
        let new_file_name = input
            .file_name
            .clone()
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| existing.file_name.clone());
        let new_path = file_path_for(dir, &new_file_name)?;
        let renamed = new_file_name != existing.file_name;
        if renamed {
            if let Some(other) = db.get_subagent_by_file_name(&new_file_name)? {
                if other.id != existing.id {
                    return Err(err_name_taken(format!(
                        "文件名 {new_file_name} 已被另一个子代理使用"
                    )));
                }
            }
        }
        // 只有记录处于启用状态时，旧路径上的文件才可能是我们写的
        let owned_hash = existing
            .written_hash
            .as_deref()
            .filter(|_| existing.enabled);
        // 旧文件需要删除的情况：改名后旧位置作废，或本次停用
        let remove_old = existing.enabled && (renamed || !input.enabled);

        // 先完成所有检查，再动任何文件，避免检查到一半失败时已经写了一部分
        let old_path = if remove_old {
            let old_path = file_path_for(dir, &existing.file_name)?;
            let old_previous = ensure_removable(&old_path, owned_hash, force)?;
            Some((old_path, old_previous))
        } else {
            None
        };
        let new_previous = if input.enabled {
            let target_owned = if renamed { None } else { owned_hash };
            Some(ensure_writable(&new_path, target_owned, force)?)
        } else {
            None
        };

        let written_hash = if input.enabled {
            write_text_file(&new_path, &input.content)?;
            Some(sha256_hex(input.content.as_bytes()))
        } else {
            None
        };

        let record = SubagentRecord {
            name: meta.name,
            file_name: new_file_name,
            description: Some(meta.description),
            content: input.content,
            enabled: input.enabled,
            written_hash,
            updated_at: Some(now_ts()),
            ..existing
        };
        if let Err(e) = db.save_subagent(&record) {
            if let Some(previous) = new_previous {
                restore_file(&new_path, previous.as_deref());
            }
            return Err(e);
        }

        if let Some((old_path, old_previous)) = old_path {
            if old_previous.is_some() {
                if let Err(e) = remove_if_exists(&old_path) {
                    // 记录已更新，旧文件残留只会变成“未管理”文件，不影响数据一致性
                    log::warn!("删除旧子代理文件失败 {}: {e}", old_path.display());
                }
            }
        }
        Ok(Self::to_dto(dir, record))
    }

    pub fn delete(db: &Database, dir: &Path, id: &str, force: bool) -> Result<(), AppError> {
        let _guard = lock();
        let record = Self::require(db, id)?;
        // 停用的记录没有磁盘文件；同路径若有文件，那是别人的，不能删
        if !record.enabled {
            return db.delete_subagent(id);
        }
        let path = file_path_for(dir, &record.file_name)?;
        let previous = ensure_removable(&path, record.written_hash.as_deref(), force)?;
        if previous.is_some() {
            remove_if_exists(&path)?;
        }
        if let Err(e) = db.delete_subagent(id) {
            if let Some(bytes) = previous.as_deref() {
                restore_file(&path, Some(bytes));
            }
            return Err(e);
        }
        Ok(())
    }

    pub fn toggle(
        db: &Database,
        dir: &Path,
        id: &str,
        enabled: bool,
        force: bool,
    ) -> Result<Subagent, AppError> {
        let _guard = lock();
        let record = Self::require(db, id)?;
        let path = file_path_for(dir, &record.file_name)?;
        let owned_hash = record.written_hash.as_deref().filter(|_| record.enabled);

        if enabled {
            // 已启用时再次启用 = 重新投影（例如文件缺失时补写）；偏离照样要 force
            let previous = ensure_writable(&path, owned_hash, force)?;
            write_text_file(&path, &record.content)?;
            let updated = SubagentRecord {
                enabled: true,
                written_hash: Some(sha256_hex(record.content.as_bytes())),
                updated_at: Some(now_ts()),
                ..record
            };
            if let Err(e) = db.save_subagent(&updated) {
                restore_file(&path, previous.as_deref());
                return Err(e);
            }
            return Ok(Self::to_dto(dir, updated));
        }

        if !record.enabled {
            return Ok(Self::to_dto(dir, record));
        }
        let previous = ensure_removable(&path, owned_hash, force)?;
        if previous.is_some() {
            remove_if_exists(&path)?;
        }
        let updated = SubagentRecord {
            enabled: false,
            written_hash: None,
            updated_at: Some(now_ts()),
            ..record
        };
        if let Err(e) = db.save_subagent(&updated) {
            if let Some(bytes) = previous.as_deref() {
                restore_file(&path, Some(bytes));
            }
            return Err(e);
        }
        Ok(Self::to_dto(dir, updated))
    }

    /// 目录顶层 `*.md` 中不属于任何记录的文件。解析失败的也列出来（带
    /// parseError），让用户知道为什么不能导入，而不是悄悄消失。
    pub fn scan_unmanaged(db: &Database, dir: &Path) -> Result<Vec<UnmanagedSubagent>, AppError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(AppError::io(dir, e)),
        };
        let managed: HashSet<String> = db
            .list_subagents()?
            .into_iter()
            .map(|record| record.file_name)
            .collect();

        let mut result = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") || !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if managed.contains(file_name) {
                continue;
            }
            result.push(Self::inspect_file(&path, file_name));
        }
        result.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        Ok(result)
    }

    fn inspect_file(path: &Path, file_name: &str) -> UnmanagedSubagent {
        let mut item = UnmanagedSubagent {
            file_name: file_name.to_string(),
            path: path.to_string_lossy().to_string(),
            name: None,
            description: None,
            content: String::new(),
            parse_error: None,
        };
        match Self::read_subagent_file(path) {
            Ok((content, _)) => {
                match parse_subagent_meta(&content) {
                    Ok(meta) => {
                        item.name = Some(meta.name);
                        item.description = Some(meta.description);
                        if !is_valid_identifier(file_name) {
                            item.parse_error = Some(format!(
                                "文件名 {file_name:?} 不合法：只能包含小写字母、数字和连字符"
                            ));
                        }
                    }
                    Err(reason) => item.parse_error = Some(reason),
                }
                item.content = content;
            }
            Err(reason) => item.parse_error = Some(reason),
        }
        item
    }

    /// 读取并做基本检查，返回 (内容, 磁盘哈希)
    fn read_subagent_file(path: &Path) -> Result<(String, String), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
        if bytes.len() > MAX_SUBAGENT_BYTES {
            return Err(format!("文件超过 {} KB 上限", MAX_SUBAGENT_BYTES / 1024));
        }
        let hash = sha256_hex(&bytes);
        let content =
            String::from_utf8(bytes).map_err(|_| "文件不是有效的 UTF-8 文本".to_string())?;
        Ok((content, hash))
    }

    /// 把未管理文件纳入管理：内容原样入库，written_hash 取磁盘哈希，不重写文件。
    ///
    /// 全部先校验再统一落库：任何一个文件不合法就整体报 SUBAGENT_INVALID 并列出
    /// 所有原因，一个都不导入。返回值只能表达“导入成功的列表”，部分成功时调用方
    /// 无从得知哪些被跳过，不如让用户取消勾选有问题的文件后重试。
    pub fn import(
        db: &Database,
        dir: &Path,
        file_names: Vec<String>,
    ) -> Result<Vec<Subagent>, AppError> {
        let _guard = lock();
        let mut seen = HashSet::new();
        let mut records = Vec::new();
        let mut failures = Vec::new();
        let now = now_ts();

        for file_name in file_names {
            if !seen.insert(file_name.clone()) {
                continue;
            }
            if !is_valid_identifier(&file_name) {
                failures.push(format!("{file_name}: 文件名不合法"));
                continue;
            }
            let path = dir.join(format!("{file_name}.md"));
            if db.get_subagent_by_file_name(&file_name)?.is_some() {
                failures.push(format!("{file_name}: 已由 CC Switch 管理"));
                continue;
            }
            let (content, hash) = match Self::read_subagent_file(&path) {
                Ok(read) => read,
                Err(reason) => {
                    failures.push(format!("{file_name}: {reason}"));
                    continue;
                }
            };
            let meta = match parse_subagent_meta(&content) {
                Ok(meta) => meta,
                Err(reason) => {
                    failures.push(format!("{file_name}: {reason}"));
                    continue;
                }
            };
            records.push(SubagentRecord {
                id: uuid::Uuid::new_v4().to_string(),
                name: meta.name,
                file_name,
                description: Some(meta.description),
                content,
                enabled: true,
                source: "imported".to_string(),
                repo_owner: None,
                repo_name: None,
                repo_path: None,
                written_hash: Some(hash),
                created_at: Some(now),
                updated_at: Some(now),
            });
        }

        if !failures.is_empty() {
            return Err(err_invalid(format!(
                "以下文件无法导入：{}",
                failures.join("; ")
            )));
        }
        db.save_subagents(&records)?;
        let mut imported: Vec<Subagent> = records
            .into_iter()
            .map(|record| Self::to_dto(dir, record))
            .collect();
        imported.sort_by(|a, b| a.name.cmp(&b.name).then(a.file_name.cmp(&b.file_name)));
        Ok(imported)
    }

    pub fn resolve_drift(
        db: &Database,
        dir: &Path,
        id: &str,
        strategy: SubagentDriftStrategy,
    ) -> Result<Subagent, AppError> {
        let _guard = lock();
        let record = Self::require(db, id)?;
        if !record.enabled {
            // 停用记录的路径上即使有文件也不是我们的，谈不上“偏离”
            return Err(err_invalid(format!(
                "子代理 {} 已停用，没有可处理的偏离",
                record.file_name
            )));
        }
        let path = file_path_for(dir, &record.file_name)?;

        match strategy {
            SubagentDriftStrategy::AdoptDisk => {
                if !path.exists() {
                    return Err(err_not_found(format!("{} 不存在", path.display())));
                }
                let (content, hash) = Self::read_subagent_file(&path).map_err(err_invalid)?;
                let meta = parse_subagent_meta(&content).map_err(err_invalid)?;
                let updated = SubagentRecord {
                    name: meta.name,
                    description: Some(meta.description),
                    content,
                    written_hash: Some(hash),
                    updated_at: Some(now_ts()),
                    ..record
                };
                db.save_subagent(&updated)?;
                Ok(Self::to_dto(dir, updated))
            }
            SubagentDriftStrategy::OverwriteDisk => {
                // 用户明确选择覆盖，不再做哈希检查
                let previous = read_optional(&path)?;
                write_text_file(&path, &record.content)?;
                let updated = SubagentRecord {
                    written_hash: Some(sha256_hex(record.content.as_bytes())),
                    updated_at: Some(now_ts()),
                    ..record
                };
                if let Err(e) = db.save_subagent(&updated) {
                    restore_file(&path, previous.as_deref());
                    return Err(e);
                }
                Ok(Self::to_dto(dir, updated))
            }
        }
    }

    pub fn disk_content(db: &Database, dir: &Path, id: &str) -> Result<Option<String>, AppError> {
        let record = Self::require(db, id)?;
        let path = file_path_for(dir, &record.file_name)?;
        match read_optional(&path)? {
            None => Ok(None),
            Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        }
    }

    /// 数据库被同步/备份恢复替换后，把启用记录投影回磁盘。
    ///
    /// - 文件缺失：写入（恢复的意义就在于此）；
    /// - 文件存在且哈希不同：不覆盖，留给用户在界面上处理偏离——本地文件不在同步
    ///   负载里，覆盖它就是在用远端数据销毁本地独有的修改。
    ///
    /// 正常启动时不调用：用户可能用 Claude Code 的 /agents 主动删了文件。
    pub fn sync_after_restore(db: &Database, dir: &Path) -> Result<(), AppError> {
        let _guard = lock();
        let mut failures = Vec::new();
        for record in db.list_subagents()? {
            if !record.enabled {
                continue;
            }
            if let Err(e) = Self::project_if_missing(db, dir, record.clone()) {
                log::warn!("恢复后投影子代理 {} 失败: {e}", record.file_name);
                failures.push(format!("{}: {e}", record.file_name));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分子代理投影失败: {}",
                failures.join("; ")
            )))
        }
    }

    fn project_if_missing(
        db: &Database,
        dir: &Path,
        record: SubagentRecord,
    ) -> Result<(), AppError> {
        let path = file_path_for(dir, &record.file_name)?;
        if path.exists() {
            return Ok(());
        }
        // 远端记录同样必须是合法的子代理文件，否则不写
        validate_content(&record.content)?;
        write_text_file(&path, &record.content)?;
        let hash = sha256_hex(record.content.as_bytes());
        // 只有哈希真的变了才写库，避免恢复后无意义的数据库变更再次触发自动上传
        if record.written_hash.as_deref() != Some(hash.as_str()) {
            db.save_subagent(&SubagentRecord {
                written_hash: Some(hash),
                ..record
            })?;
        }
        Ok(())
    }

    /// 已安装 Claude Code 插件自带的子代理（只读、尽力而为）。
    ///
    /// 读 `<claude>/plugins/installed_plugins.json`：`plugins` 是
    /// `"插件名@市场" -> 安装记录` 的映射，v2 里值是安装记录数组（按 scope 区分），
    /// v1 里是单个对象；每条记录的 `installPath` 下的 `agents/*.md` 即插件子代理。
    /// 插件可在 plugin.json 里自定义 agents 路径，这里不处理；任何格式不符都只是
    /// 跳过，不报错。
    pub fn list_plugin_subagents(claude_root: &Path) -> Vec<PluginSubagent> {
        let manifest = claude_root.join("plugins").join("installed_plugins.json");
        let Ok(raw) = std::fs::read_to_string(&manifest) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            log::debug!("无法解析 {}", manifest.display());
            return Vec::new();
        };
        let Some(plugins) = value.get("plugins").and_then(|p| p.as_object()) else {
            return Vec::new();
        };

        let mut seen_paths = HashSet::new();
        let mut result = Vec::new();
        for (key, entry) in plugins {
            let plugin_name = key.split('@').next().unwrap_or(key).to_string();
            let installs: Vec<&serde_json::Value> = match entry {
                serde_json::Value::Array(items) => items.iter().collect(),
                serde_json::Value::Object(_) => vec![entry],
                _ => continue,
            };
            for install in installs {
                let Some(install_path) = install.get("installPath").and_then(|p| p.as_str()) else {
                    continue;
                };
                let agents_dir = PathBuf::from(install_path).join("agents");
                if !seen_paths.insert(agents_dir.clone()) {
                    continue;
                }
                Self::collect_plugin_agents(&plugin_name, &agents_dir, &mut result);
            }
        }
        result.sort_by(|a, b| a.plugin_name.cmp(&b.plugin_name).then(a.name.cmp(&b.name)));
        result
    }

    fn collect_plugin_agents(plugin_name: &str, agents_dir: &Path, out: &mut Vec<PluginSubagent>) {
        let Ok(entries) = std::fs::read_dir(agents_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") || !path.is_file() {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let parsed = Self::read_subagent_file(&path)
                .ok()
                .and_then(|(content, _)| extract_meta(&content).ok());
            // 插件不归我们校验，解析不了就用文件名兜底展示
            let (name, description, model) = match parsed {
                Some(meta) => (meta.name, Some(meta.description), meta.model),
                None => (stem, None, None),
            };
            out.push(PluginSubagent {
                plugin_name: plugin_name.to_string(),
                name,
                description,
                model,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
}

#[cfg(test)]
#[path = "subagent_tests.rs"]
mod tests;
