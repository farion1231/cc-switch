//! Skills 服务层
//!
//! v3.10.0+ 统一管理架构：
//! - SSOT（单一事实源）：`~/.cc-switch/skills/`
//! - 安装时下载到 SSOT，按需同步到各应用目录
//! - 数据库存储安装记录和启用状态

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

use crate::app_config::{AppType, InstalledSkill, SkillApps, UnmanagedSkill};
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::error::format_skill_error;

// ========== Skills state coordination ==========

/// Coordinates the database `skills` state with the filesystem SSOT.
///
/// Lock order is: global sync operation lock -> this lock -> database mutex.
/// Async install/update paths acquire the write guard only after downloads have
/// completed, so no non-Send std guard is held across an `.await`.
fn skill_state_lock() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

pub(crate) fn skill_state_read_guard() -> RwLockReadGuard<'static, ()> {
    skill_state_lock().read().unwrap_or_else(|poisoned| {
        log::warn!("Skills state read lock was poisoned; recovering the protected state");
        poisoned.into_inner()
    })
}

pub(crate) fn skill_state_write_guard() -> RwLockWriteGuard<'static, ()> {
    skill_state_lock().write().unwrap_or_else(|poisoned| {
        log::warn!("Skills state write lock was poisoned; recovering the protected state");
        poisoned.into_inner()
    })
}

// ========== 数据结构 ==========

/// Skill 同步方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncMethod {
    /// 自动选择：优先 symlink，失败时回退到 copy
    #[default]
    Auto,
    /// 符号链接（推荐，节省磁盘空间）
    Symlink,
    /// 文件复制（兼容模式）
    Copy,
}

/// Skill 存储位置（SSOT 目录选择）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillStorageLocation {
    /// CC Switch 管理目录 (~/.cc-switch/skills/)
    #[default]
    CcSwitch,
    /// Agent Skills 统一标准目录 (~/.agents/skills/)
    Unified,
}

/// 可发现的技能（来自仓库）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverableSkill {
    /// 唯一标识: "owner/name:directory"
    pub key: String,
    /// 显示名称 (从 SKILL.md 解析)
    pub name: String,
    /// 技能描述
    pub description: String,
    /// 目录名称 (安装路径的最后一段)
    pub directory: String,
    /// GitHub README URL
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    /// 仓库所有者
    #[serde(rename = "repoOwner")]
    pub repo_owner: String,
    /// 仓库名称
    #[serde(rename = "repoName")]
    pub repo_name: String,
    /// 分支名称
    #[serde(rename = "repoBranch")]
    pub repo_branch: String,
}

/// 技能对象（兼容旧 API，内部使用 DiscoverableSkill）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 唯一标识: "owner/name:directory" 或 "local:directory"
    pub key: String,
    /// 显示名称 (从 SKILL.md 解析)
    pub name: String,
    /// 技能描述
    pub description: String,
    /// 目录名称 (安装路径的最后一段)
    pub directory: String,
    /// GitHub README URL
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    /// 是否已安装
    pub installed: bool,
    /// 仓库所有者
    #[serde(rename = "repoOwner")]
    pub repo_owner: Option<String>,
    /// 仓库名称
    #[serde(rename = "repoName")]
    pub repo_name: Option<String>,
    /// 分支名称
    #[serde(rename = "repoBranch")]
    pub repo_branch: Option<String>,
}

/// 仓库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRepo {
    /// GitHub 用户/组织名
    pub owner: String,
    /// 仓库名称
    pub name: String,
    /// 分支 (默认 "main")
    pub branch: String,
    /// 是否启用
    pub enabled: bool,
}

/// 技能安装状态（旧版兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    /// 是否已安装
    pub installed: bool,
    /// 安装时间
    #[serde(rename = "installedAt")]
    pub installed_at: DateTime<Utc>,
}

/// 持久化存储结构（仓库配置）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    /// directory -> 安装状态（旧版兼容，新版不使用）
    pub skills: HashMap<String, SkillState>,
    /// 仓库列表
    pub repos: Vec<SkillRepo>,
}

impl Default for SkillStore {
    fn default() -> Self {
        SkillStore {
            skills: HashMap::new(),
            repos: vec![
                SkillRepo {
                    owner: "anthropics".to_string(),
                    name: "skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "ComposioHQ".to_string(),
                    name: "awesome-claude-skills".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "cexll".to_string(),
                    name: "myclaude".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    owner: "JimLiu".to_string(),
                    name: "baoyu-skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                },
            ],
        }
    }
}

/// Skill 卸载结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUninstallResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_pi_path: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pi_cleanup_incomplete: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Skill 更新检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateInfo {
    /// Skill ID
    pub id: String,
    /// Skill 名称
    pub name: String,
    /// 当前本地哈希
    pub current_hash: Option<String>,
    /// 远程最新哈希
    pub remote_hash: String,
}

/// Skill 存储位置迁移结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub migrated_count: usize,
    pub skipped_count: usize,
    pub errors: Vec<String>,
}

#[derive(Debug)]
enum PiSkillDeployment {
    Symlink { expected_target: PathBuf },
    Copy { expected_hash: String },
}

// ========== skills.sh API 类型 ==========

/// skills.sh API 原始响应
///
/// 注意：API 命名不一致（searchType 是 camelCase，duration_ms 是 snake_case），
/// 因此不能用 rename_all，需要逐字段指定。
#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiResponse {
    pub query: String,
    #[serde(rename = "searchType")]
    #[allow(dead_code)]
    pub search_type: String,
    pub skills: Vec<SkillsShApiSkill>,
    pub count: usize,
    #[allow(dead_code)]
    pub duration_ms: u64,
}

/// skills.sh API 原始技能条目
#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiSkill {
    pub id: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    pub installs: u64,
    pub source: String,
}

/// skills.sh 搜索结果（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShSearchResult {
    pub skills: Vec<SkillsShDiscoverableSkill>,
    pub total_count: usize,
    pub query: String,
}

/// skills.sh 可安装技能（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShDiscoverableSkill {
    pub key: String,
    pub name: String,
    pub directory: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub repo_branch: String,
    pub installs: u64,
    pub readme_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBackupEntry {
    pub backup_id: String,
    pub backup_path: String,
    pub created_at: i64,
    pub skill: InstalledSkill,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillBackupMetadata {
    skill: InstalledSkill,
    backup_created_at: i64,
    source_path: String,
}

const SKILL_BACKUP_RETAIN_COUNT: usize = 20;

/// 仓库归档解压上限：条目数与解压后总字节数。
///
/// 归档字节由第三方完全控制（仓库可经 deeplink 添加，且 branch 可把下载落点
/// 改写到攻击者自传的 release asset），没有上限时一个几 MB 的压缩炸弹就能塞满磁盘。
/// 取值对齐 `webdav_sync/archive.rs` 里同款保护的量级。
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// symlink 目标就是一条路径，几十字节就够；给到 4 KiB 是宽松上限。
/// 必须有这个上限：zip 2.4.2 的 `make_reader` 不按声明的 uncompressed_size
/// 截断读取，所以一个打了 symlink 标志、deflate 流却能膨胀到数 GB 的条目，
/// 会被 `read_to_string` 整个读进内存。
const MAX_SYMLINK_TARGET_BYTES: u64 = 4 * 1024;
/// 物化一个目录按一个目录块计费。空目录不写内容字节，但照样吃 inode 和磁盘块，
/// 不计费就等于允许无限量地造目录。
const DIRECTORY_BUDGET_COST: u64 = 4096;
/// 压缩体上限。解压预算只有在 ZipArchive 建起来之后才生效，而那时整个响应体
/// 已经在内存里了，所以下载这一步需要自己的上限。技能仓库是 Markdown，
/// 128 MiB 的压缩包已经远超正常规模。
const MAX_ARCHIVE_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;

/// 技能元数据 (从 SKILL.md 解析)
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// 导入已有 Skill 时，前端显式提交的启用应用选择
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkillSelection {
    pub directory: String,
    #[serde(default)]
    pub apps: SkillApps,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacySkillMigrationRow {
    directory: String,
    app_type: String,
}

// ========== ~/.agents/ lock 文件解析 ==========

/// `~/.agents/.skill-lock.json` 文件结构
#[derive(Deserialize)]
struct AgentsLockFile {
    skills: HashMap<String, AgentsLockSkill>,
}

/// lock 文件中单个 skill 的信息
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentsLockSkill {
    source: Option<String>,
    source_type: Option<String>,
    source_url: Option<String>,
    skill_path: Option<String>,
    branch: Option<String>,
    source_branch: Option<String>,
}

#[derive(Debug, Clone)]
struct LockRepoInfo {
    owner: String,
    repo: String,
    skill_path: Option<String>,
    branch: Option<String>,
}

fn normalize_optional_branch(branch: Option<String>) -> Option<String> {
    branch.and_then(|b| {
        let trimmed = b.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_branch_from_source_url(source_url: Option<&str>) -> Option<String> {
    let source_url = source_url?;
    let source_url = source_url.trim();
    if source_url.is_empty() {
        return None;
    }

    // 支持 https://github.com/owner/repo/tree/<branch>/...
    if let Some((_, after_tree)) = source_url.split_once("/tree/") {
        let branch = after_tree
            .split('/')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        return Some(branch.to_string());
    }

    // 支持 URL fragment: ...git#branch
    if let Some((_, fragment)) = source_url.split_once('#') {
        let branch = fragment
            .split('&')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        return Some(branch.to_string());
    }

    // 支持 query: ...?branch=xxx / ?ref=xxx
    if let Some((_, query)) = source_url.split_once('?') {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if matches!(key, "branch" | "ref") {
                let branch = value.trim();
                if !branch.is_empty() {
                    return Some(branch.to_string());
                }
            }
        }
    }

    None
}

/// 获取 `~/.agents/skills/` 目录（存在时返回）
fn get_agents_skills_dir() -> Option<PathBuf> {
    let dir = crate::config::get_home_dir().join(".agents").join("skills");
    dir.exists().then_some(dir)
}

/// 解析 `~/.agents/.skill-lock.json`，返回 skill_name -> 仓库信息
fn parse_agents_lock() -> HashMap<String, LockRepoInfo> {
    let path = crate::config::get_home_dir()
        .join(".agents")
        .join(".skill-lock.json");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                log::debug!("未找到 agents lock 文件: {}", path.display());
            } else {
                log::warn!("读取 agents lock 文件失败 ({}): {}", path.display(), e);
            }
            return HashMap::new();
        }
    };
    let lock: AgentsLockFile = match serde_json::from_str(&content) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("解析 agents lock 文件失败 ({}): {}", path.display(), e);
            return HashMap::new();
        }
    };
    let parsed: HashMap<String, LockRepoInfo> = lock
        .skills
        .into_iter()
        .filter_map(|(name, skill)| {
            let source = skill.source?;
            if skill.source_type.as_deref() != Some("github") {
                return None;
            }
            let (owner, repo) = source.split_once('/')?;
            let branch = normalize_optional_branch(skill.branch)
                .or_else(|| normalize_optional_branch(skill.source_branch))
                .or_else(|| parse_branch_from_source_url(skill.source_url.as_deref()));
            Some((
                name,
                LockRepoInfo {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    skill_path: skill.skill_path,
                    branch,
                },
            ))
        })
        .collect();
    log::info!(
        "agents lock 文件解析完成，共识别 {} 个 github skill",
        parsed.len()
    );
    parsed
}

// ========== SkillService ==========

pub struct SkillService;

impl Default for SkillService {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillService {
    pub fn new() -> Self {
        Self
    }

    /// 构建 Skill 文档 URL（指向仓库中的 SKILL.md 文件）
    ///
    /// 坐标不合法时返回 None：这个值会存进 `readme_url`，前端「查看文档」用
    /// `openExternal` 直接打开，恶意 branch 能把它指到 github.com 上的任意路径。
    fn build_skill_doc_url(
        owner: &str,
        repo: &str,
        branch: &str,
        doc_path: &str,
    ) -> Option<String> {
        if Self::validate_repo_ref(owner, repo, branch).is_err() {
            log::warn!("跳过非法仓库坐标的文档链接: {owner}/{repo}@{branch}");
            return None;
        }
        Some(format!(
            "https://github.com/{owner}/{repo}/blob/{branch}/{doc_path}"
        ))
    }

    /// 从旧 readme_url 中提取仓库内文档路径，兼容 `blob`/`tree` 两种格式
    fn extract_doc_path_from_url(url: &str) -> Option<String> {
        let marker = if url.contains("/blob/") {
            "/blob/"
        } else if url.contains("/tree/") {
            "/tree/"
        } else {
            return None;
        };

        let (_, tail) = url.split_once(marker)?;
        let (_, path) = tail.split_once('/')?;
        if path.is_empty() {
            return None;
        }
        Some(path.to_string())
    }

    // ========== 路径管理 ==========

    /// 获取 SSOT 目录（根据设置返回 ~/.cc-switch/skills/ 或 ~/.agents/skills/）
    pub fn get_ssot_dir() -> Result<PathBuf> {
        let location = crate::settings::get_skill_storage_location();
        let dir = match location {
            SkillStorageLocation::CcSwitch => get_app_config_dir().join("skills"),
            SkillStorageLocation::Unified => {
                crate::config::get_home_dir().join(".agents").join("skills")
            }
        };
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 获取 Skill 卸载备份目录（~/.cc-switch/skill-backups/）
    fn get_backup_dir() -> Result<PathBuf> {
        let dir = get_app_config_dir().join("skill-backups");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 获取应用的 skills 目录
    pub fn get_app_skills_dir(app: &AppType) -> Result<PathBuf> {
        // 目录覆盖：优先使用用户在 settings.json 中配置的 override 目录
        match app {
            AppType::Claude => {
                if let Some(custom) = crate::settings::get_claude_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::ClaudeDesktop => {}
            AppType::Codex => {
                if let Some(custom) = crate::settings::get_codex_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Gemini => {
                if let Some(custom) = crate::settings::get_gemini_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::GrokBuild => {
                if let Some(custom) = crate::settings::get_grok_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::OpenCode => {
                if let Some(custom) = crate::settings::get_opencode_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::OpenClaw => {
                if let Some(custom) = crate::settings::get_openclaw_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Hermes => {
                if let Some(custom) = crate::settings::get_hermes_override_dir() {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::Cursor => {
                if let Some(custom) = crate::settings::get_cursor_override_dir() {
                    return Ok(custom.join("rules"));
                }
            }
            AppType::Pi => {
                return Ok(crate::pi_config::get_pi_agent_dir()?.join("skills"));
            }
        }

        // 默认路径：回退到用户主目录下的标准位置。
        // 必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）：Windows 上 dirs::home_dir()
        // 走 Known Folder API，测试无法隔离真实用户目录。
        let home = crate::config::get_home_dir();

        Ok(match app {
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::ClaudeDesktop => home.join(".claude-desktop").join("skills"),
            AppType::Codex => home.join(".codex").join("skills"),
            AppType::Gemini => home.join(".gemini").join("skills"),
            AppType::GrokBuild => home.join(".grok").join("skills"),
            AppType::OpenCode => home.join(".config").join("opencode").join("skills"),
            AppType::OpenClaw => home.join(".openclaw").join("skills"),
            AppType::Hermes => crate::hermes_config::get_hermes_dir().join("skills"),
            AppType::Cursor => crate::cursor_config::get_cursor_dir().join("rules"),
            AppType::Pi => crate::pi_config::get_pi_agent_dir()?.join("skills"),
        })
    }

    fn paths_alias(left: &Path, right: &Path) -> bool {
        if left == right {
            return true;
        }

        matches!(
            (left.canonicalize(), right.canonicalize()),
            (Ok(left), Ok(right)) if left == right
        )
    }

    fn paths_overlap(left: &Path, right: &Path) -> bool {
        let overlaps = |left: &Path, right: &Path| {
            left == right || left.starts_with(right) || right.starts_with(left)
        };
        if overlaps(left, right) {
            return true;
        }

        if let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) {
            if overlaps(&left, &right) {
                return true;
            }
        }

        // canonicalize() follows the final component and therefore fails for a
        // dangling symlink. Resolve the parents separately so two applications
        // cannot delete the same directory entry through aliased roots.
        let canonical_entry =
            |path: &Path| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?));
        matches!(
            (canonical_entry(left), canonical_entry(right)),
            (Some(left), Some(right)) if overlaps(&left, &right)
        )
    }

    fn ensure_distinct_skill_roots(ssot_dir: &Path, app_dir: &Path, app: &AppType) -> Result<()> {
        if Self::paths_alias(ssot_dir, app_dir) {
            return Err(anyhow!(
                "Skill 存储目录不能与 {app:?} 的 Skills 目录相同: {}",
                ssot_dir.display()
            ));
        }
        Ok(())
    }

    fn get_distinct_app_skills_dir(ssot_dir: &Path, app: &AppType) -> Result<PathBuf> {
        let app_dir = Self::get_app_skills_dir(app)?;
        Self::ensure_distinct_skill_roots(ssot_dir, &app_dir, app)?;
        Ok(app_dir)
    }

    fn validate_skill_storage_destination(ssot_dir: &Path) -> Result<()> {
        for app in AppType::all() {
            if matches!(app, AppType::ClaudeDesktop) {
                continue;
            }
            let app_dir = Self::get_app_skills_dir(&app)?;
            Self::ensure_distinct_skill_roots(ssot_dir, &app_dir, &app)?;
        }
        Ok(())
    }

    // ========== 统一管理方法 ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed(db: &Arc<Database>) -> Result<Vec<InstalledSkill>> {
        let mut skills = db.get_all_installed_skills()?;
        for skill in skills.values_mut() {
            skill.apps.pi = Self::skill_exists_in_app(&skill.directory, &AppType::Pi);
        }
        Ok(skills.into_values().collect())
    }

    /// Reuse an existing installation or reject a directory owned by another repo.
    /// The caller must hold [`skill_state_write_guard`] because this can update the
    /// database and materialized app directory.
    fn reuse_existing_install(
        db: &Arc<Database>,
        skill: &DiscoverableSkill,
        install_name: &str,
        current_app: &AppType,
    ) -> Result<Option<InstalledSkill>> {
        let existing_skills = db.get_all_installed_skills()?;
        for existing in existing_skills.values() {
            if !existing.directory.eq_ignore_ascii_case(install_name) {
                continue;
            }

            let same_repo = existing.repo_owner.as_deref() == Some(&skill.repo_owner)
                && existing.repo_name.as_deref() == Some(&skill.repo_name);
            if same_repo {
                let mut updated = existing.clone();
                updated.apps.set_enabled_for(current_app, true);
                db.save_skill(&updated)?;
                Self::sync_to_app_dir(&updated.directory, current_app)?;
                log::info!(
                    "Skill {} 已存在，更新 {:?} 启用状态",
                    updated.name,
                    current_app
                );
                return Ok(Some(updated));
            }

            return Err(anyhow!(format_skill_error(
                "SKILL_DIRECTORY_CONFLICT",
                &[
                    ("directory", install_name),
                    (
                        "existing_repo",
                        &format!(
                            "{}/{}",
                            existing.repo_owner.as_deref().unwrap_or("unknown"),
                            existing.repo_name.as_deref().unwrap_or("unknown")
                        )
                    ),
                    (
                        "new_repo",
                        &format!("{}/{}", skill.repo_owner, skill.repo_name)
                    ),
                ],
                Some("uninstallFirst"),
            )));
        }

        Ok(None)
    }

    /// 安装 Skill
    ///
    /// 流程：
    /// 1. 下载到 SSOT 目录
    /// 2. 保存到数据库
    /// 3. 同步到启用的应用目录
    pub async fn install(
        &self,
        db: &Arc<Database>,
        skill: &DiscoverableSkill,
        current_app: &AppType,
    ) -> Result<InstalledSkill> {
        let ssot_dir = Self::get_ssot_dir()?;

        // 允许多级目录（如 a/b/c），但必须是安全的相对路径。
        let source_rel = Self::sanitize_skill_source_path(&skill.directory).ok_or_else(|| {
            anyhow!(format_skill_error(
                "INVALID_SKILL_DIRECTORY",
                &[("directory", &skill.directory)],
                Some("checkZipContent"),
            ))
        })?;
        // 安装目录名始终使用最后一段，避免在 SSOT 中创建多级目录。
        let install_name = source_rel
            .file_name()
            .and_then(|name| Self::sanitize_install_name(&name.to_string_lossy()))
            .ok_or_else(|| {
                anyhow!(format_skill_error(
                    "INVALID_SKILL_DIRECTORY",
                    &[("directory", &skill.directory)],
                    Some("checkZipContent"),
                ))
            })?;

        // Fast path for an existing installation. The write guard makes the DB
        // row and app projection indivisible from a concurrent cloud snapshot.
        {
            let _state_guard = skill_state_write_guard();
            if let Some(existing) =
                Self::reuse_existing_install(db, skill, &install_name, current_app)?
            {
                return Ok(existing);
            }
        }

        let dest = ssot_dir.join(&install_name);

        let mut repo_branch = skill.repo_branch.clone();
        // 真实解析出的源目录推导的文档路径（仅本次真正下载解析时可得）
        let mut resolved_doc_path: Option<String> = None;
        let mut downloaded_source: Option<(tempfile::TempDir, PathBuf)> = None;

        // 如果已存在则跳过下载
        if !dest.exists() {
            let repo = SkillRepo {
                owner: skill.repo_owner.clone(),
                name: skill.repo_name.clone(),
                branch: skill.repo_branch.clone(),
                enabled: true,
            };

            // 下载仓库
            let (temp_guard, used_branch) = timeout(
                std::time::Duration::from_secs(60),
                self.download_repo(&repo),
            )
            .await
            .map_err(|_| {
                anyhow!(format_skill_error(
                    "DOWNLOAD_TIMEOUT",
                    &[
                        ("owner", &repo.owner),
                        ("name", &repo.name),
                        ("timeout", "60")
                    ],
                    Some("checkNetwork"),
                ))
            })??;
            let temp_dir = temp_guard.path();
            repo_branch = used_branch;

            // 复制到 SSOT
            let source =
                Self::resolve_skill_source_dir(temp_dir, &skill.directory).ok_or_else(|| {
                    let missing = temp_dir.join(&source_rel).display().to_string();
                    anyhow!(format_skill_error(
                        "SKILL_DIR_NOT_FOUND",
                        &[("path", &missing)],
                        Some("checkRepoUrl"),
                    ))
                })?;

            let canonical_temp = temp_dir
                .canonicalize()
                .unwrap_or_else(|_| temp_dir.to_path_buf());
            let canonical_source = source.canonicalize().map_err(|_| {
                anyhow!(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &source.display().to_string())],
                    Some("checkRepoUrl"),
                ))
            })?;
            if !canonical_source.starts_with(&canonical_temp) || !canonical_source.is_dir() {
                return Err(anyhow!(format_skill_error(
                    "INVALID_SKILL_DIRECTORY",
                    &[("directory", &skill.directory)],
                    Some("checkZipContent"),
                )));
            }

            // 用真实解析出的源目录推导文档路径——skills.sh 的 directory 只是
            // skillId（末级目录名），嵌套目录场景直接拼接会丢路径、链接 404（#6111）
            resolved_doc_path = Self::doc_path_for_source(&canonical_temp, &canonical_source);

            downloaded_source = Some((temp_guard, canonical_source));

            // 使用实际下载成功的分支，避免 readme_url / repo_branch 与真实分支不一致。
            if repo_branch != skill.repo_branch {
                log::info!(
                    "Skill {}/{} 分支自动回退: {} -> {}",
                    skill.repo_owner,
                    skill.repo_name,
                    skill.repo_branch,
                    repo_branch
                );
            }
        }

        let doc_path = Self::choose_doc_path(
            resolved_doc_path,
            skill.readme_url.as_deref(),
            &skill.directory,
        );

        let readme_url =
            Self::build_skill_doc_url(&skill.repo_owner, &skill.repo_name, &repo_branch, &doc_path);

        // Re-check after the network download: another install/uninstall may have
        // completed while the lock was intentionally released around `.await`.
        let _state_guard = skill_state_write_guard();
        if let Some(existing) = Self::reuse_existing_install(db, skill, &install_name, current_app)?
        {
            return Ok(existing);
        }

        if !dest.exists() {
            let source = downloaded_source
                .as_ref()
                .map(|(_, source)| source)
                .ok_or_else(|| anyhow!("Skill directory changed during install; please retry"))?;
            Self::preflight_install_destination(source, &install_name, current_app)?;
            Self::copy_dir_recursive(source, &dest)?;
        }

        // 创建 InstalledSkill 记录
        // 计算内容哈希
        let content_hash = Self::compute_dir_hash(&dest).map(Some).unwrap_or_else(|e| {
            log::warn!("Failed to compute content hash for {}: {e}", install_name);
            None
        });

        let installed_skill = InstalledSkill {
            id: skill.key.clone(),
            name: skill.name.clone(),
            description: if skill.description.is_empty() {
                None
            } else {
                Some(skill.description.clone())
            },
            directory: install_name.clone(),
            repo_owner: Some(skill.repo_owner.clone()),
            repo_name: Some(skill.repo_name.clone()),
            repo_branch: Some(repo_branch),
            readme_url,
            apps: SkillApps::only(current_app),
            installed_at: chrono::Utc::now().timestamp(),
            content_hash,
            updated_at: 0,
        };

        Self::persist_and_sync_new_skill(db, &installed_skill, current_app)?;

        log::info!(
            "Skill {} 安装成功，已启用 {:?}",
            installed_skill.name,
            current_app
        );

        Ok(installed_skill)
    }

    /// 卸载 Skill
    ///
    /// 流程：
    /// 1. 从所有应用目录删除
    /// 2. 从 SSOT 删除
    /// 3. 从数据库删除
    pub fn uninstall(db: &Arc<Database>, id: &str) -> Result<SkillUninstallResult> {
        let _state_guard = skill_state_write_guard();

        // 获取 skill 信息
        let skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| anyhow!("Skill not found: {id}"))?;

        // DB 行可能被同步导入污染（远端快照 raw SQL 直接灌库，绕过安装期校验），
        // 也可能是 v3.11.0 引入 sanitize_install_name 之前留下的存量脏值
        // （当年扫描不过滤点开头目录，`.github/SKILL.md` 会存成 `.github`）。
        //
        // 守卫失败时**跳过全部文件系统操作、但仍删除 DB 行**：`db.delete_skill`
        // 全项目只有这一处调用且未暴露为命令，若在此直接返回 Err，用户就再也无法
        // 从界面删掉这条记录，只能手改 SQLite。安全目标是「不碰危险路径」，
        // 不是「把用户锁在坏状态里」。
        let (backup_path, preserved_pi_path, pi_cleanup_incomplete) =
            match Self::require_valid_directory(&skill.directory) {
                Ok(directory) => {
                    let ssot_dir = Self::get_ssot_dir()?;
                    let source = ssot_dir.join(&directory);
                    let mut preserved_pi_path: Option<PathBuf> = None;
                    let mut pi_cleanup_incomplete = false;
                    let mut pi_removal_path = None;

                    match Self::get_app_skills_dir(&AppType::Pi) {
                        Ok(pi_dir) => {
                            let destination = pi_dir.join(&directory);
                            if Self::paths_alias(&ssot_dir, &pi_dir) {
                                // Pi 直接使用 SSOT 时没有第二份副本；后续删除 SSOT 即完成清理。
                                log::debug!("Skill {id} 的 Pi Skills 目录与 SSOT 相同");
                            } else {
                                // 即使当前不存在也保留目标路径；备份期间可能有另一进程
                                // 同步该 Skill，删除 SSOT 前必须重新检查一次。
                                pi_removal_path = Some(destination.clone());
                                if destination.exists() || Self::is_symlink(&destination) {
                                    if let Err(err) = Self::inspect_pi_skill_destination(
                                        &source,
                                        &destination,
                                        &directory,
                                    ) {
                                        log::warn!(
                                            "Skill {id} 卸载时跳过 Pi 清理，保留 {}: {err}",
                                            destination.display()
                                        );
                                        preserved_pi_path = Some(destination);
                                        pi_removal_path = None;
                                        pi_cleanup_incomplete = true;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            log::warn!(
                                "Skill {id} 卸载时无法解析 Pi Skills 目录，跳过 Pi 清理: {err}"
                            );
                            pi_cleanup_incomplete = true;
                        }
                    }

                    let backup_path = Self::create_uninstall_backup_excluding(
                        &skill,
                        preserved_pi_path.as_deref(),
                    )?
                    .map(|path| path.to_string_lossy().to_string());

                    // Pi 目录可能包含用户自己维护的同名 Skill。删除 SSOT 前仅移除
                    // 能验证为 CC Switch 部署的副本；其余路径保留并返回警告。
                    if let Some(destination) = pi_removal_path {
                        let removal =
                            Self::remove_verified_pi_destination(&source, &destination, &directory);
                        if let Err(err) = removal {
                            log::warn!("Skill {id} 卸载时 Pi 清理失败，将继续卸载: {err}");
                            pi_cleanup_incomplete = true;
                            if destination.exists() || Self::is_symlink(&destination) {
                                preserved_pi_path = Some(destination);
                            }
                        }
                    }

                    // 其他应用沿用既有的逐项容错行为。
                    for app in AppType::all() {
                        if matches!(app, AppType::Pi) {
                            continue;
                        }
                        let _ = Self::remove_from_app_preserving(
                            &directory,
                            &app,
                            preserved_pi_path.as_deref(),
                        );
                    }

                    // 从 SSOT 删除
                    let skill_path = ssot_dir.join(&directory);
                    let overlaps_preserved_pi = preserved_pi_path
                        .as_deref()
                        .is_some_and(|path| Self::paths_overlap(&skill_path, path));
                    if overlaps_preserved_pi {
                        log::warn!("Skill {id} 的 SSOT 路径与保留的 Pi 副本重叠，跳过文件删除");
                    } else if skill_path.exists() {
                        fs::remove_dir_all(&skill_path)?;
                    }
                    (backup_path, preserved_pi_path, pi_cleanup_incomplete)
                }
                Err(err) => {
                    log::warn!(
                    "Skill {id} 的 directory 非法（{:?}），跳过文件清理，仅删除数据库记录: {err}",
                    skill.directory
                );
                    (None, None, true)
                }
            };

        // 从数据库删除
        db.delete_skill(id)?;

        log::info!(
            "Skill {} 卸载成功{}",
            skill.name,
            backup_path
                .as_deref()
                .map(|path| format!(", backup: {path}"))
                .unwrap_or_default()
        );

        Ok(SkillUninstallResult {
            backup_path,
            preserved_pi_path: preserved_pi_path.map(|path| path.to_string_lossy().to_string()),
            pi_cleanup_incomplete,
        })
    }

    // ========== 更新检测 ==========

    /// 计算目录内容的 SHA-256 哈希
    ///
    /// 递归遍历目录下所有非隐藏文件，按相对路径字典序排列，
    /// 将 "相对路径\0内容\0" 逐文件 feed 给同一个 hasher。
    pub fn compute_dir_hash(dir: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut files: Vec<PathBuf> = Vec::new();
        Self::collect_files_for_hash(dir, dir, &mut files)?;
        files.sort();

        let mut hasher = Sha256::new();
        for file_path in &files {
            let relative = file_path.strip_prefix(dir).unwrap_or(file_path);
            let rel_str = relative.to_string_lossy().replace('\\', "/");
            hasher.update(rel_str.as_bytes());
            hasher.update(b"\0");
            let content = fs::read(file_path)
                .with_context(|| format!("读取文件失败: {}", file_path.display()))?;
            hasher.update(&content);
            hasher.update(b"\0");
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// 递归收集目录下所有非隐藏文件
    #[allow(clippy::only_used_in_recursion)]
    fn collect_files_for_hash(base: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let entries = fs::read_dir(current)
            .with_context(|| format!("读取目录失败: {}", current.display()))?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                Self::collect_files_for_hash(base, &path, files)?;
            } else {
                files.push(path);
            }
        }
        Ok(())
    }

    /// Pi copy 部署的破坏性操作需要比较完整目录树，不能沿用更新检测哈希：
    /// 隐藏文件、空目录或文件类型变化都意味着用户已修改原生目录。
    fn compute_pi_deployment_hash(dir: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut entries = Vec::new();
        Self::collect_tree_entries(dir, &mut entries)?;
        entries.sort();

        let mut hasher = Sha256::new();
        for path in entries {
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
            hasher.update(b"\0");

            let metadata = fs::symlink_metadata(&path)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                hasher.update(b"link\0");
                hasher.update(fs::read_link(&path)?.to_string_lossy().as_bytes());
            } else if file_type.is_dir() {
                hasher.update(b"dir\0");
            } else if file_type.is_file() {
                hasher.update(b"file\0");
                hasher.update(fs::read(&path)?);
            } else {
                hasher.update(b"other\0");
            }
            hasher.update(b"\0");
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    fn collect_tree_entries(current: &Path, entries: &mut Vec<PathBuf>) -> Result<()> {
        for entry in
            fs::read_dir(current).with_context(|| format!("读取目录失败: {}", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            entries.push(path.clone());
            if file_type.is_dir() {
                Self::collect_tree_entries(&path, entries)?;
            }
        }
        Ok(())
    }

    /// 判定 check_updates 应使用的本地哈希。
    ///
    /// 次序关键：必须先确认 SSOT 目录存在，再信任数据库缓存的 content_hash。
    /// 换机恢复数据库备份后 Skill 文件不随库迁移，此时缓存哈希仍在而目录已
    /// 缺失；若先取缓存会把这类 Skill 误报成「无更新」，缺失状态被永久掩盖。
    /// 目录缺失 → 返回 None，与远端哈希必然不等，Skill 进入更新列表，
    /// update_skill 对缺失目录本就容忍（存在才删、随后整体复制），点更新即可重建。
    ///
    /// 返回 `(hash, freshly_computed)`；freshly_computed 表示哈希是现场算出的，
    /// 调用方应回填数据库缓存。
    fn local_hash_for_update_check(
        ssot_dir: &Path,
        raw_directory: &str,
        cached_hash: Option<&str>,
    ) -> Option<(String, bool)> {
        // 脏 directory 会让 compute_dir_hash 递归遍历任意目录，且哈希结果经
        // 「有无更新」的界面状态泄露少量信息；无法安全拼路径时也不能报
        // 「可更新」——update_skill 会在同一校验上硬报错，只能沿用缓存。
        let directory = match Self::require_valid_directory(raw_directory) {
            Ok(d) => d,
            Err(err) => {
                log::warn!("Skill directory 非法，跳过本地目录检查: {err}");
                return cached_hash.map(|h| (h.to_string(), false));
            }
        };

        let local_dir = ssot_dir.join(&directory);
        if !local_dir.exists() {
            return None;
        }

        if let Some(h) = cached_hash {
            return Some((h.to_string(), false));
        }

        match Self::compute_dir_hash(&local_dir) {
            Ok(h) => Some((h, true)),
            Err(_) => None,
        }
    }

    /// 检查所有已安装 Skill 的更新
    ///
    /// 仅检查有 repo_owner 的 Skill（本地 Skill 跳过），
    /// 按仓库分组下载，避免重复下载同一仓库。
    pub async fn check_updates(&self, db: &Arc<Database>) -> Result<Vec<SkillUpdateInfo>> {
        let skills = db.get_all_installed_skills()?;
        let mut updates = Vec::new();

        // 按 (owner, name, branch) 分组
        let mut repo_groups: HashMap<(String, String, String), Vec<InstalledSkill>> =
            HashMap::new();

        for skill in skills.into_values() {
            let (owner, name, branch) =
                match (&skill.repo_owner, &skill.repo_name, &skill.repo_branch) {
                    (Some(o), Some(n), Some(b)) => (o.clone(), n.clone(), b.clone()),
                    (Some(o), Some(n), None) => (o.clone(), n.clone(), "main".to_string()),
                    _ => continue,
                };
            repo_groups
                .entry((owner, name, branch))
                .or_default()
                .push(skill);
        }

        let ssot_dir = Self::get_ssot_dir()?;

        for ((owner, name, branch), group_skills) in &repo_groups {
            let repo = SkillRepo {
                owner: owner.clone(),
                name: name.clone(),
                branch: branch.clone(),
                enabled: true,
            };

            // 下载仓库 ZIP
            let (temp_guard, _used_branch) = match timeout(
                std::time::Duration::from_secs(60),
                self.download_repo(&repo),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => {
                    log::warn!("检查更新时下载 {}/{} 失败: {e}", owner, name);
                    continue;
                }
                Err(_) => {
                    log::warn!("检查更新时下载 {}/{} 超时", owner, name);
                    continue;
                }
            };
            let temp_dir = temp_guard.path();

            // 扫描仓库中的所有 Skill 目录
            let mut remote_skills: Vec<DiscoverableSkill> = Vec::new();
            let _ = self.scan_dir_recursive(temp_dir, temp_dir, &repo, &mut remote_skills);

            // Remote I/O is complete. Stabilize the local DB + SSOT while hashes
            // are read and any missing hash metadata is backfilled.
            let _state_guard = skill_state_read_guard();

            for skill in group_skills {
                // 在远程仓库中找到匹配的 Skill 目录
                let remote_match = remote_skills.iter().find(|rs| {
                    // 匹配方式：安装名称的最后一段
                    let remote_install_name =
                        rs.directory.rsplit('/').next().unwrap_or(&rs.directory);
                    remote_install_name.eq_ignore_ascii_case(&skill.directory)
                });

                let remote_skill_dir = match remote_match {
                    Some(rs) => match Self::resolve_skill_source_dir(temp_dir, &rs.directory) {
                        Some(path) => path,
                        None => continue,
                    },
                    None => continue,
                };

                let remote_hash = match Self::compute_dir_hash(&remote_skill_dir) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("计算远程哈希失败 {}: {e}", skill.id);
                        continue;
                    }
                };

                let local_hash = match Self::local_hash_for_update_check(
                    &ssot_dir,
                    &skill.directory,
                    skill.content_hash.as_deref(),
                ) {
                    Some((h, freshly_computed)) => {
                        if freshly_computed {
                            let _ = db.update_skill_hash(&skill.id, &h, 0);
                        }
                        Some(h)
                    }
                    None => None,
                };

                if local_hash.as_deref() != Some(&remote_hash) {
                    updates.push(SkillUpdateInfo {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                        current_hash: local_hash,
                        remote_hash,
                    });
                }
            }
        }

        Ok(updates)
    }

    /// 持久化更新后的 Skill 元数据，并重新读取数据库中的权威应用启用状态。
    ///
    /// 更新过程包含网络下载，期间用户可能切换启用状态或卸载 Skill。这里必须
    /// 使用只更新现有记录的 DAO，避免旧快照覆盖 `enabled_*`，也避免已卸载记录
    /// 被重新插入。
    fn persist_updated_skill_metadata(
        db: &Arc<Database>,
        updated_skill: &InstalledSkill,
    ) -> Result<InstalledSkill> {
        if !db.update_skill_metadata(updated_skill)? {
            return Err(anyhow!("Skill no longer installed: {}", updated_skill.id));
        }

        db.get_installed_skill(&updated_skill.id)?
            .ok_or_else(|| anyhow!("Skill no longer installed: {}", updated_skill.id))
    }

    /// 更新单个 Skill（重新下载并替换本地文件）
    pub async fn update_skill(&self, db: &Arc<Database>, skill_id: &str) -> Result<InstalledSkill> {
        let mut skill = db
            .get_installed_skill(skill_id)?
            .ok_or_else(|| anyhow!("Skill not found: {skill_id}"))?;
        skill.apps.pi = Self::skill_exists_in_app(&skill.directory, &AppType::Pi);

        // 本函数后续三种危险操作都用 directory 拼路径：备份源（把任意目录复制进
        // 备份区并在界面列出）、remove_dir_all（删任意目录）、copy_dir_recursive
        // （把远端仓库内容写到任意路径）。校验必须在这三者之前。
        Self::require_valid_directory(&skill.directory)?;

        let (owner, name, branch) = match (&skill.repo_owner, &skill.repo_name) {
            (Some(o), Some(n)) => (
                o.clone(),
                n.clone(),
                skill
                    .repo_branch
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            ),
            _ => return Err(anyhow!("Cannot update local skill: {skill_id}")),
        };

        let repo = SkillRepo {
            owner: owner.clone(),
            name: name.clone(),
            branch: branch.clone(),
            enabled: true,
        };

        let ssot_dir = Self::get_ssot_dir()?;
        if skill.apps.pi {
            Self::get_distinct_app_skills_dir(&ssot_dir, &AppType::Pi)?;
        }

        // 下载仓库
        let (temp_guard, used_branch) = timeout(
            std::time::Duration::from_secs(60),
            self.download_repo(&repo),
        )
        .await
        .map_err(|_| {
            anyhow!(format_skill_error(
                "DOWNLOAD_TIMEOUT",
                &[("owner", &owner), ("name", &name), ("timeout", "60")],
                Some("checkNetwork"),
            ))
        })??;
        let temp_dir = temp_guard.path();

        // 在解压的仓库中查找 Skill 源目录
        let mut remote_skills: Vec<DiscoverableSkill> = Vec::new();
        let _ = self.scan_dir_recursive(temp_dir, temp_dir, &repo, &mut remote_skills);

        let remote_match = remote_skills
            .iter()
            .find(|rs| {
                let remote_install_name = rs.directory.rsplit('/').next().unwrap_or(&rs.directory);
                remote_install_name.eq_ignore_ascii_case(&skill.directory)
            })
            .ok_or_else(|| {
                anyhow!(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &skill.directory)],
                    Some("checkRepoUrl"),
                ))
            })?;

        let source =
            Self::resolve_skill_source_dir(temp_dir, &remote_match.directory).ok_or_else(|| {
                let missing = temp_dir.join(&remote_match.directory).display().to_string();
                anyhow!(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &missing)],
                    Some("checkRepoUrl"),
                ))
            })?;

        // Downloads do not mutate local state, so acquire only now and hold the
        // guard through the SSOT replacement, DB metadata update, and app sync.
        let _state_guard = skill_state_write_guard();

        // 下载和扫描期间用户可能已经卸载了该 Skill。必须在任何备份、删除或
        // 复制之前重新确认记录仍存在；否则即使最终的 metadata UPDATE 能发现
        // 缺行，这里也会先把已卸载的 SSOT 目录重新创建出来。
        let mut current_skill = db
            .get_installed_skill(&skill.id)?
            .ok_or_else(|| anyhow!("Skill no longer installed: {}", skill.id))?;
        if current_skill.directory != skill.directory
            || current_skill.repo_owner != skill.repo_owner
            || current_skill.repo_name != skill.repo_name
            || current_skill.repo_branch != skill.repo_branch
            || current_skill.installed_at != skill.installed_at
        {
            return Err(anyhow!("Skill changed during update: {}", skill.id));
        }
        Self::require_valid_directory(&current_skill.directory)?;
        current_skill.apps.pi = Self::skill_exists_in_app(&current_skill.directory, &AppType::Pi);
        let skill = current_skill;

        let dest = ssot_dir.join(&skill.directory);
        let pi_deployment = if skill.apps.pi {
            let pi_dir = Self::get_distinct_app_skills_dir(&ssot_dir, &AppType::Pi)?;
            let pi_destination = pi_dir.join(&skill.directory);
            Self::inspect_pi_skill_destination(&dest, &pi_destination, &skill.directory)?
        } else {
            None
        };

        // 备份旧文件
        let _ = Self::create_uninstall_backup(&skill);

        // 删除旧 SSOT 目录并复制新文件
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        Self::copy_dir_recursive(&source, &dest)?;

        // 计算新哈希 + 解析新元数据
        let new_hash = Self::compute_dir_hash(&dest).ok();
        let skill_md = dest.join("SKILL.md");
        let (new_name, new_description) = Self::read_skill_name_desc(&skill_md, &skill.directory);

        // 更新 readme_url
        let doc_path = skill
            .readme_url
            .as_deref()
            .and_then(Self::extract_doc_path_from_url)
            .unwrap_or_else(|| format!("{}/SKILL.md", skill.directory.trim_end_matches('/')));
        let readme_url = Self::build_skill_doc_url(&owner, &name, &used_branch, &doc_path);

        let updated_metadata = InstalledSkill {
            id: skill.id.clone(),
            name: new_name,
            description: new_description,
            directory: skill.directory.clone(),
            repo_owner: skill.repo_owner.clone(),
            repo_name: skill.repo_name.clone(),
            repo_branch: Some(used_branch),
            readme_url,
            apps: skill.apps.clone(),
            installed_at: skill.installed_at,
            content_hash: new_hash,
            updated_at: chrono::Utc::now().timestamp(),
        };

        if let Some(deployment) = pi_deployment {
            let pi_destination =
                Self::get_app_skills_dir(&AppType::Pi)?.join(&updated_metadata.directory);
            Self::refresh_pi_skill_destination(
                &dest,
                &pi_destination,
                &updated_metadata.directory,
                &deployment,
            )?;
        }

        let mut updated_skill = Self::persist_updated_skill_metadata(db, &updated_metadata)?;
        updated_skill.apps.pi = Self::skill_exists_in_app(&updated_skill.directory, &AppType::Pi);

        // 同步到所有已启用的应用目录
        for app in updated_skill.apps.enabled_apps() {
            if matches!(app, AppType::Pi) {
                continue;
            }
            if let Err(e) = Self::sync_to_app_dir(&updated_skill.directory, &app) {
                log::warn!("同步更新后的 skill 到 {:?} 失败: {e}", app);
            }
        }

        log::info!("Skill {} 更新成功", updated_skill.name);
        Ok(updated_skill)
    }

    /// 为缺少 content_hash 的已安装 Skill 补算哈希
    pub fn backfill_content_hashes(db: &Arc<Database>) -> Result<usize> {
        let _state_guard = skill_state_write_guard();
        let skills = db.get_all_installed_skills()?;
        let ssot_dir = Self::get_ssot_dir()?;
        let mut count = 0;

        for skill in skills.values() {
            if skill.content_hash.is_some() {
                continue;
            }
            let Ok(directory) = Self::require_valid_directory(&skill.directory) else {
                log::warn!("跳过非法 directory 的哈希回填: {:?}", skill.directory);
                continue;
            };
            let skill_dir = ssot_dir.join(&directory);
            if !skill_dir.exists() {
                continue;
            }
            match Self::compute_dir_hash(&skill_dir) {
                Ok(hash) => {
                    let _ = db.update_skill_hash(&skill.id, &hash, 0);
                    count += 1;
                }
                Err(e) => {
                    log::warn!("补算哈希失败 {}: {e}", skill.id);
                }
            }
        }

        if count > 0 {
            log::info!("已为 {count} 个 Skill 补算内容哈希");
        }
        Ok(count)
    }

    /// 迁移 Skill 存储位置（在两个 SSOT 目录间移动文件）
    ///
    /// 安全策略：先移文件，后改设置。中途崩溃时设置仍指向旧目录。
    pub fn migrate_storage(
        db: &Arc<Database>,
        target: SkillStorageLocation,
    ) -> Result<MigrationResult> {
        let _state_guard = skill_state_write_guard();
        let current = crate::settings::get_skill_storage_location();
        if current == target {
            return Ok(MigrationResult {
                migrated_count: 0,
                skipped_count: 0,
                errors: vec![],
            });
        }

        // 1. 解析旧目录和新目录（不改设置）
        let old_dir = Self::get_ssot_dir()?;
        let new_dir = match target {
            SkillStorageLocation::CcSwitch => get_app_config_dir().join("skills"),
            SkillStorageLocation::Unified => {
                crate::config::get_home_dir().join(".agents").join("skills")
            }
        };
        fs::create_dir_all(&new_dir)?;
        Self::validate_skill_storage_destination(&new_dir)?;

        // 2. 逐个移动 skill 目录
        let skills = db.get_all_installed_skills()?;
        let pi_dir = Self::get_app_skills_dir(&AppType::Pi)?;
        let pi_uses_old_ssot = Self::paths_alias(&old_dir, &pi_dir);
        let mut pi_deployments = Vec::new();
        let mut pi_native_sources = Vec::new();
        let mut result = MigrationResult {
            migrated_count: 0,
            skipped_count: 0,
            errors: vec![],
        };

        for skill in skills.values() {
            // 下面是 rename 与 remove_dir_all，脏 directory 可把任意目录搬走或删掉。
            // 软失败：本函数已有 errors 收集通道，记一条继续处理其余 skill，
            // 不要整体中断——用户只是在切换存储位置。
            let directory = match Self::require_valid_directory(&skill.directory) {
                Ok(directory) => directory,
                Err(err) => {
                    result
                        .errors
                        .push(format!("{}: {err}", skill.directory.escape_debug()));
                    continue;
                }
            };
            let src = old_dir.join(&directory);
            let dst = new_dir.join(&directory);

            if !src.exists() {
                result.skipped_count += 1;
                continue;
            }
            if dst.exists() {
                result.skipped_count += 1;
                continue;
            }

            // 在移动 SSOT 前确认 Pi 目标确实由旧源管理。外部同名目录不参与迁移，
            // 也不会被覆盖；已确认的链接或副本则携带旧值进入受保护替换。
            let pi_deployment = if pi_uses_old_ssot {
                None
            } else {
                let pi_destination = pi_dir.join(&directory);
                Self::inspect_pi_skill_destination(&src, &pi_destination, &directory)
                    .ok()
                    .flatten()
            };

            // 优先 rename（同文件系统原子操作），失败则 copy+delete
            match fs::rename(&src, &dst) {
                Ok(()) => {
                    result.migrated_count += 1;
                    if pi_uses_old_ssot {
                        pi_native_sources.push(directory);
                    } else if let Some(deployment) = pi_deployment {
                        pi_deployments.push((directory, deployment));
                    }
                }
                Err(_) => match Self::copy_dir_recursive(&src, &dst) {
                    Ok(()) => {
                        let _ = fs::remove_dir_all(&src);
                        result.migrated_count += 1;
                        if pi_uses_old_ssot {
                            pi_native_sources.push(directory);
                        } else if let Some(deployment) = pi_deployment {
                            pi_deployments.push((directory, deployment));
                        }
                    }
                    Err(e) => {
                        result.errors.push(format!("{}: {e}", skill.directory));
                    }
                },
            }
        }

        // 3. 文件移动完成后才持久化设置
        crate::settings::set_skill_storage_location(target)?;

        // 4. 刷新所有应用目录的 symlink（指向新 SSOT）
        for app in AppType::all() {
            let _ = Self::sync_to_app_unlocked(db, &app);
        }
        for (directory, deployment) in pi_deployments {
            let source = new_dir.join(&directory);
            let destination = pi_dir.join(&directory);
            if let Err(err) =
                Self::refresh_pi_skill_destination(&source, &destination, &directory, &deployment)
            {
                result.errors.push(format!("{directory}: {err}"));
            }
        }
        for directory in pi_native_sources {
            if let Err(err) = Self::sync_to_app_dir(&directory, &AppType::Pi) {
                result.errors.push(format!("{directory}: {err}"));
            }
        }

        log::info!(
            "Skill 存储迁移完成: {} 迁移, {} 跳过, {} 错误",
            result.migrated_count,
            result.skipped_count,
            result.errors.len()
        );

        Ok(result)
    }

    pub fn list_backups() -> Result<Vec<SkillBackupEntry>> {
        let backup_dir = Self::get_backup_dir()?;
        let mut entries = Vec::new();

        for entry in fs::read_dir(&backup_dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    log::warn!("读取 Skill 备份目录项失败: {err}");
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            match Self::read_backup_metadata(&path) {
                Ok(metadata) => entries.push(SkillBackupEntry {
                    backup_id: entry.file_name().to_string_lossy().to_string(),
                    backup_path: path.to_string_lossy().to_string(),
                    created_at: metadata.backup_created_at,
                    skill: metadata.skill,
                }),
                Err(err) => {
                    log::warn!("解析 Skill 备份失败 {}: {err:#}", path.display());
                }
            }
        }

        entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
        Ok(entries)
    }

    pub fn delete_backup(backup_id: &str) -> Result<()> {
        let backup_path = Self::backup_path_for_id(backup_id)?;
        let metadata = fs::symlink_metadata(&backup_path)
            .with_context(|| format!("failed to access {}", backup_path.display()))?;

        if !metadata.is_dir() {
            return Err(anyhow!(
                "Skill backup is not a directory: {}",
                backup_path.display()
            ));
        }

        fs::remove_dir_all(&backup_path)
            .with_context(|| format!("failed to delete {}", backup_path.display()))?;

        log::info!("Skill 备份已删除: {}", backup_path.display());
        Ok(())
    }

    pub fn restore_from_backup(
        db: &Arc<Database>,
        backup_id: &str,
        current_app: &AppType,
    ) -> Result<InstalledSkill> {
        let _state_guard = skill_state_write_guard();
        let backup_path = Self::backup_path_for_id(backup_id)?;
        let metadata = Self::read_backup_metadata(&backup_path)?;
        let backup_skill_dir = backup_path.join("skill");
        if !backup_skill_dir.join("SKILL.md").exists() {
            return Err(anyhow!(
                "Skill backup is invalid or missing SKILL.md: {}",
                backup_path.display()
            ));
        }

        let existing_skills = db.get_all_installed_skills()?;
        if existing_skills.contains_key(&metadata.skill.id)
            || existing_skills.values().any(|skill| {
                skill
                    .directory
                    .eq_ignore_ascii_case(&metadata.skill.directory)
            })
        {
            return Err(anyhow!(
                "Skill already exists, please uninstall the current one first: {}",
                metadata.skill.directory
            ));
        }

        // meta.json 是文件内容（可能来自手工放置或不可信备份），directory 此前
        // 未经任何校验就直接 join——可穿越出 SSOT 目录写任意位置。必须先校验。
        let directory = Self::require_valid_directory(&metadata.skill.directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let restore_path = ssot_dir.join(&directory);
        if restore_path.exists() || Self::is_symlink(&restore_path) {
            return Err(anyhow!(
                "Restore target already exists: {}",
                restore_path.display()
            ));
        }

        let mut restored_skill = metadata.skill;
        restored_skill.directory = directory;
        restored_skill.installed_at = Utc::now().timestamp();
        restored_skill.apps = SkillApps::only(current_app);
        restored_skill.updated_at = 0;

        Self::copy_dir_recursive(&backup_skill_dir, &restore_path)?;

        // 重新计算内容哈希
        restored_skill.content_hash = Self::compute_dir_hash(&restore_path).ok();

        if let Err(err) = db.save_skill(&restored_skill) {
            let _ = fs::remove_dir_all(&restore_path);
            return Err(err.into());
        }

        if !restored_skill.apps.is_empty() {
            if let Err(err) = Self::sync_to_app_dir(&restored_skill.directory, current_app) {
                let _ = db.delete_skill(&restored_skill.id);
                let _ = fs::remove_dir_all(&restore_path);
                return Err(err);
            }
        }

        log::info!(
            "Skill {} 已从备份恢复到 {}",
            restored_skill.name,
            restore_path.display()
        );

        Ok(restored_skill)
    }

    /// 切换应用启用状态
    ///
    /// 启用：复制到应用目录
    /// 禁用：从应用目录删除
    pub fn toggle_app(db: &Arc<Database>, id: &str, app: &AppType, enabled: bool) -> Result<()> {
        let _state_guard = skill_state_write_guard();
        // 获取当前 skill
        let mut skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| anyhow!("Skill not found: {id}"))?;

        // 更新状态
        skill.apps.set_enabled_for(app, enabled);

        // 同步文件
        if enabled {
            Self::sync_to_app_dir(&skill.directory, app)?;
        } else {
            Self::remove_from_app(&skill.directory, app)?;
        }

        // Pi follows its native exists=active rule; other apps keep their
        // established persisted desired-state flags.
        if !matches!(app, AppType::Pi) {
            db.update_skill_apps(id, &skill.apps)?;
        }

        log::info!("Skill {} 的 {:?} 状态已更新为 {}", skill.name, app, enabled);

        Ok(())
    }

    /// 扫描未管理的 Skills
    ///
    /// 扫描各应用目录，找出未被 CC Switch 管理的 Skills
    pub fn scan_unmanaged(db: &Arc<Database>) -> Result<Vec<UnmanagedSkill>> {
        let _state_guard = skill_state_read_guard();
        let managed_skills = db.get_all_installed_skills()?;
        let managed_dirs: HashSet<String> = managed_skills
            .values()
            .map(|s| s.directory.clone())
            .collect();

        // 收集所有待扫描的目录及其来源标签
        let mut scan_sources: Vec<(PathBuf, String)> = Vec::new();
        for app in AppType::all() {
            if let Ok(d) = Self::get_app_skills_dir(&app) {
                scan_sources.push((d, app.as_str().to_string()));
            }
        }
        if let Some(agents_dir) = get_agents_skills_dir() {
            scan_sources.push((agents_dir, "agents".to_string()));
        }
        if let Ok(ssot_dir) = Self::get_ssot_dir() {
            scan_sources.push((ssot_dir, "cc-switch".to_string()));
        }

        let mut unmanaged: HashMap<String, UnmanagedSkill> = HashMap::new();

        for (scan_dir, label) in &scan_sources {
            let entries = match fs::read_dir(scan_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') || managed_dirs.contains(&dir_name) {
                    continue;
                }

                let skill_md = path.join("SKILL.md");
                if !skill_md.exists() {
                    continue;
                }
                let (name, description) = Self::read_skill_name_desc(&skill_md, &dir_name);

                unmanaged
                    .entry(dir_name.clone())
                    .and_modify(|s| s.found_in.push(label.clone()))
                    .or_insert(UnmanagedSkill {
                        directory: dir_name,
                        name,
                        description,
                        found_in: vec![label.clone()],
                        path: path.display().to_string(),
                    });
            }
        }

        Ok(unmanaged.into_values().collect())
    }

    /// 从应用目录导入 Skills
    ///
    /// 将未管理的 Skills 导入到 CC Switch 统一管理
    pub fn import_from_apps(
        db: &Arc<Database>,
        imports: Vec<ImportSkillSelection>,
    ) -> Result<Vec<InstalledSkill>> {
        let _state_guard = skill_state_write_guard();
        let ssot_dir = Self::get_ssot_dir()?;
        let agents_lock = parse_agents_lock();
        let mut imported = Vec::new();

        // 将 lock 文件中发现的仓库保存到 skill_repos
        save_repos_from_lock(
            db,
            &agents_lock,
            imports.iter().map(|selection| selection.directory.as_str()),
        );

        // 收集所有候选搜索目录
        let mut search_sources: Vec<(PathBuf, String)> = Vec::new();
        for app in AppType::all() {
            if let Ok(d) = Self::get_app_skills_dir(&app) {
                search_sources.push((d, app.as_str().to_string()));
            }
        }
        if let Some(agents_dir) = get_agents_skills_dir() {
            search_sources.push((agents_dir, "agents".to_string()));
        }
        search_sources.push((ssot_dir.clone(), "cc-switch".to_string()));

        for selection in imports {
            // selection.directory 由前端 IPC 直接传入、此前全程无校验，而它既被
            // 用来探测源目录、又作为 copy_dir_recursive 的目标、最后还原样入库。
            // 在入口处拒掉，同时切断「脏值 sink」和「脏值来源」两条线。
            let dir_name = match Self::require_valid_directory(&selection.directory) {
                Ok(dir_name) => dir_name,
                Err(err) => {
                    log::warn!("跳过导入：{err}");
                    continue;
                }
            };
            // 在所有候选目录中查找
            let mut source_path: Option<PathBuf> = None;

            for (base, label) in &search_sources {
                let skill_path = base.join(&dir_name);
                if skill_path.exists() {
                    if source_path.is_none() {
                        source_path = Some(skill_path);
                    }
                    log::debug!("Skill '{dir_name}' found in source '{label}'");
                }
            }

            let source = match source_path {
                Some(p) => p,
                None => continue,
            };
            if !source.join("SKILL.md").exists() {
                log::warn!(
                    "Skip importing '{}' because source '{}' has no SKILL.md",
                    dir_name,
                    source.display()
                );
                continue;
            }

            // 复制到 SSOT
            let dest = ssot_dir.join(&dir_name);
            if !dest.exists() {
                Self::copy_dir_recursive(&source, &dest)?;
            }

            // 解析元数据
            let skill_md = dest.join("SKILL.md");
            let (name, description) = Self::read_skill_name_desc(&skill_md, &dir_name);

            // 其他应用保存用户选择；Pi 的 exists=active 必须直接来自原生目录。
            let mut apps = selection.apps;
            apps.pi = Self::skill_exists_in_app(&dir_name, &AppType::Pi);

            // 从 lock 文件提取仓库信息
            let (id, repo_owner, repo_name, repo_branch, readme_url) =
                build_repo_info_from_lock(&agents_lock, &dir_name);

            // 计算内容哈希
            let ssot_skill_dir = ssot_dir.join(&dir_name);
            let content_hash = Self::compute_dir_hash(&ssot_skill_dir).ok();

            // 创建记录
            let skill = InstalledSkill {
                id,
                name,
                description,
                directory: dir_name,
                repo_owner,
                repo_name,
                repo_branch,
                readme_url,
                apps,
                installed_at: chrono::Utc::now().timestamp(),
                content_hash,
                updated_at: 0,
            };

            // 保存到数据库
            db.save_skill(&skill)?;

            imported.push(skill);
        }

        log::info!("成功导入 {} 个 Skills", imported.len());

        Ok(imported)
    }

    // ========== 文件同步方法 ==========

    /// 创建符号链接（跨平台）
    ///
    /// - Unix: 使用 std::os::unix::fs::symlink
    /// - Windows: 使用 std::os::windows::fs::symlink_dir
    #[cfg(unix)]
    fn create_symlink(src: &Path, dest: &Path) -> Result<()> {
        std::os::unix::fs::symlink(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    #[cfg(windows)]
    fn create_symlink(src: &Path, dest: &Path) -> Result<()> {
        std::os::windows::fs::symlink_dir(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    /// 检查路径是否为符号链接
    fn is_symlink(path: &Path) -> bool {
        path.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn skill_exists_in_app(directory: &str, app: &AppType) -> bool {
        let Ok(directory) = Self::require_valid_directory(directory) else {
            return false;
        };
        let Ok(app_dir) = Self::get_app_skills_dir(app) else {
            return false;
        };
        app_dir.join(directory).is_dir()
    }

    fn preflight_install_destination(source: &Path, directory: &str, app: &AppType) -> Result<()> {
        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        if !matches!(app, AppType::Pi) {
            return Ok(());
        }
        let destination = app_dir.join(directory);
        if destination.exists() || Self::is_symlink(&destination) {
            Self::ensure_pi_skill_destination_matches(source, &destination, directory)?;
        }
        Ok(())
    }

    fn persist_and_sync_new_skill(
        db: &Arc<Database>,
        skill: &InstalledSkill,
        app: &AppType,
    ) -> Result<()> {
        let source = Self::get_ssot_dir()?.join(&skill.directory);
        Self::preflight_install_destination(&source, &skill.directory, app)?;
        db.save_skill(skill)?;
        if let Err(error) = Self::sync_to_app_dir(&skill.directory, app) {
            if let Err(rollback_error) = db.delete_skill(&skill.id) {
                log::error!(
                    "Failed to roll back Skill {} after sync error: {rollback_error}",
                    skill.id
                );
            }
            return Err(error);
        }
        Ok(())
    }

    fn ensure_pi_skill_destination_matches(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<()> {
        Self::inspect_pi_skill_destination(source, destination, directory).map(|_| ())
    }

    fn inspect_pi_skill_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<Option<PiSkillDeployment>> {
        if !destination.exists() && !Self::is_symlink(destination) {
            return Ok(None);
        }

        if Self::is_symlink(destination) {
            let target = fs::read_link(destination)?;
            let resolved = if target.is_absolute() {
                target
            } else {
                destination
                    .parent()
                    .map(|parent| parent.join(&target))
                    .unwrap_or(target)
            };
            if matches!(
                (resolved.canonicalize(), source.canonicalize()),
                (Ok(resolved), Ok(source)) if resolved == source
            ) {
                return Ok(Some(PiSkillDeployment::Symlink {
                    expected_target: resolved,
                }));
            }
        } else if destination.is_dir() {
            if let (Ok(destination_hash), Ok(source_hash)) = (
                Self::compute_pi_deployment_hash(destination),
                Self::compute_pi_deployment_hash(source),
            ) {
                if destination_hash == source_hash {
                    return Ok(Some(PiSkillDeployment::Copy {
                        expected_hash: destination_hash,
                    }));
                }
            }
        }

        Err(anyhow!(
            "Pi 中已存在同名但内容不同的 Skill，拒绝覆盖或删除: {directory}"
        ))
    }

    fn remove_verified_pi_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<()> {
        match Self::inspect_pi_skill_destination(source, destination, directory)? {
            Some(_) => Self::remove_path(destination),
            None => Ok(()),
        }
    }

    fn refresh_pi_skill_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
        deployment: &PiSkillDeployment,
    ) -> Result<()> {
        Self::validate_sync_source_dir(source, directory)?;

        match deployment {
            PiSkillDeployment::Symlink { expected_target } => {
                if !Self::is_symlink(destination) {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                let target = fs::read_link(destination)?;
                let resolved = if target.is_absolute() {
                    target
                } else {
                    destination
                        .parent()
                        .map(|parent| parent.join(&target))
                        .unwrap_or(target)
                };
                if &resolved != expected_target {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                Self::remove_path(destination)?;
                Self::create_symlink(source, destination)?;
            }
            PiSkillDeployment::Copy { expected_hash } => {
                if Self::is_symlink(destination)
                    || !destination.is_dir()
                    || !matches!(
                        Self::compute_pi_deployment_hash(destination),
                        Ok(current_hash) if &current_hash == expected_hash
                    )
                {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                Self::replace_dest_with_copy(source, destination, directory)?;
            }
        }

        Ok(())
    }

    /// 获取当前同步方式配置
    fn get_sync_method() -> SyncMethod {
        crate::settings::get_skill_sync_method()
    }

    /// 同步 Skill 到应用目录（使用 symlink 或 copy）
    ///
    /// 根据配置和平台选择最佳同步方式：
    /// - Auto: 优先尝试 symlink，失败时回退到 copy
    /// - Symlink: 仅使用 symlink
    /// - Copy: 仅使用文件复制
    pub fn sync_to_app_dir(directory: &str, app: &AppType) -> Result<()> {
        if matches!(app, AppType::ClaudeDesktop) {
            return Ok(());
        }

        // directory 可能来自被污染的 DB 行（如同步导入的远端快照），join 前必须校验。
        let directory = Self::require_valid_directory(directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let source = ssot_dir.join(&directory);

        Self::validate_sync_source_dir(&source, &directory)?;

        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        fs::create_dir_all(&app_dir)?;

        let dest = app_dir.join(&directory);

        if matches!(app, AppType::Pi) && (dest.exists() || Self::is_symlink(&dest)) {
            Self::ensure_pi_skill_destination_matches(&source, &dest, &directory)?;
        }

        let sync_method = Self::get_sync_method();

        match sync_method {
            SyncMethod::Auto => {
                if dest.exists() && !Self::is_symlink(&dest) {
                    Self::replace_dest_with_copy(&source, &dest, &directory)?;
                    log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
                    return Ok(());
                }

                if Self::is_symlink(&dest) {
                    Self::remove_path(&dest)?;
                }

                // 优先尝试 symlink
                match Self::create_symlink(&source, &dest) {
                    Ok(()) => {
                        log::debug!("Skill {directory} 已通过 symlink 同步到 {app:?}");
                        return Ok(());
                    }
                    Err(err) => {
                        log::warn!(
                            "Symlink 创建失败，将回退到文件复制: {} -> {}. 错误: {err:#}",
                            source.display(),
                            dest.display()
                        );
                    }
                }
                // Fallback 到 copy
                Self::replace_dest_with_copy(&source, &dest, &directory)?;
                log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
            }
            SyncMethod::Symlink => {
                if dest.exists() || Self::is_symlink(&dest) {
                    Self::remove_path(&dest)?;
                }
                Self::create_symlink(&source, &dest)?;
                log::debug!("Skill {directory} 已通过 symlink 同步到 {app:?}");
            }
            SyncMethod::Copy => {
                Self::replace_dest_with_copy(&source, &dest, &directory)?;
                log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
            }
        }

        Ok(())
    }

    /// 复制 Skill 到应用目录（保留用于向后兼容）
    #[deprecated(note = "请使用 sync_to_app_dir() 代替")]
    pub fn copy_to_app(directory: &str, app: &AppType) -> Result<()> {
        Self::sync_to_app_dir(directory, app)
    }

    /// 删除路径（支持 symlink 和真实目录）
    fn remove_path(path: &Path) -> Result<()> {
        if Self::is_symlink(path) {
            // 符号链接：仅删除链接本身，不影响源文件
            #[cfg(unix)]
            fs::remove_file(path)?;
            #[cfg(windows)]
            fs::remove_dir(path)?; // Windows 的目录 symlink 需要用 remove_dir
        } else if path.is_dir() {
            // 真实目录：递归删除
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            // 普通文件
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn validate_sync_source_dir(source: &Path, directory: &str) -> Result<()> {
        if !source.is_dir() {
            return Err(anyhow!("Skill 不存在于 SSOT: {directory}"));
        }

        let manifest = source.join("SKILL.md");
        if !manifest.is_file() {
            return Err(anyhow!(
                "Skill 源目录缺少 SKILL.md，拒绝同步以避免覆盖目标目录: {}",
                source.display()
            ));
        }

        Ok(())
    }

    fn replace_dest_with_copy(source: &Path, dest: &Path, directory: &str) -> Result<()> {
        Self::validate_sync_source_dir(source, directory)?;

        let parent = dest
            .parent()
            .ok_or_else(|| anyhow!("Invalid skill destination: {}", dest.display()))?;
        fs::create_dir_all(parent)?;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_name = Self::sanitize_backup_segment(directory);
        let tmp = parent.join(format!(".{tmp_name}.tmp-{}-{nonce}", std::process::id()));

        if tmp.exists() || Self::is_symlink(&tmp) {
            Self::remove_path(&tmp)?;
        }

        let copy_result = Self::copy_dir_recursive(source, &tmp);
        if let Err(err) = copy_result {
            let _ = Self::remove_path(&tmp);
            return Err(err);
        }

        if dest.exists() || Self::is_symlink(dest) {
            Self::remove_path(dest)?;
        }

        fs::rename(&tmp, dest).with_context(|| {
            let _ = Self::remove_path(&tmp);
            format!(
                "替换 Skill 目录失败: {} -> {}",
                tmp.display(),
                dest.display()
            )
        })?;

        Ok(())
    }

    /// 判断路径是否为指向 SSOT 目录内的符号链接。
    fn is_symlink_to_ssot(path: &Path, ssot_dir: &Path) -> bool {
        if !Self::is_symlink(path) {
            return false;
        }

        let Ok(target) = fs::read_link(path) else {
            return false;
        };

        if target.is_absolute() && target.starts_with(ssot_dir) {
            return true;
        }

        let resolved = path
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target.clone());

        let canonical_ssot = ssot_dir
            .canonicalize()
            .unwrap_or_else(|_| ssot_dir.to_path_buf());
        let canonical_target = resolved.canonicalize().unwrap_or(resolved);

        canonical_target.starts_with(&canonical_ssot)
    }

    /// 从应用目录删除 Skill（支持 symlink 和真实目录）
    pub fn remove_from_app(directory: &str, app: &AppType) -> Result<()> {
        Self::remove_from_app_preserving(directory, app, None)
    }

    fn remove_from_app_preserving(
        directory: &str,
        app: &AppType,
        preserved_path: Option<&Path>,
    ) -> Result<()> {
        if matches!(app, AppType::ClaudeDesktop) {
            return Ok(());
        }

        // directory 可能来自被污染的 DB 行（如同步导入的远端快照），
        // 这里执行的是删除操作，join 前必须校验，防止任意目录删除。
        let directory = Self::require_valid_directory(directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        let skill_path = app_dir.join(&directory);

        if preserved_path.is_some_and(|path| Self::paths_overlap(&skill_path, path)) {
            log::debug!("Skill {directory} 的 {app:?} 路径与保留的 Pi 副本相同，跳过删除");
            return Ok(());
        }

        if skill_path.exists() || Self::is_symlink(&skill_path) {
            if matches!(app, AppType::Pi) {
                let source = ssot_dir.join(&directory);
                Self::ensure_pi_skill_destination_matches(&source, &skill_path, &directory)?;
            }
            Self::remove_path(&skill_path)?;
            log::debug!("Skill {directory} 已从 {app:?} 删除");
        }

        Ok(())
    }

    /// 同步所有已启用的 Skills 到指定应用
    pub fn sync_to_app(db: &Arc<Database>, app: &AppType) -> Result<()> {
        let _state_guard = skill_state_read_guard();
        Self::sync_to_app_unlocked(db, app)
    }

    /// Caller must hold either the Skills state read or write guard.
    fn sync_to_app_unlocked(db: &Arc<Database>, app: &AppType) -> Result<()> {
        if matches!(app, AppType::ClaudeDesktop | AppType::Pi) {
            return Ok(());
        }

        let skills = db.get_all_installed_skills()?;
        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;

        let indexed_skills: HashMap<String, &InstalledSkill> = skills
            .values()
            .map(|skill| (skill.directory.to_lowercase(), skill))
            .collect();

        if app_dir.exists() {
            for entry in fs::read_dir(&app_dir)? {
                let entry = entry?;
                let path = entry.path();
                let dir_name = entry.file_name().to_string_lossy().to_string();

                if dir_name.starts_with('.') {
                    continue;
                }

                if let Some(skill) = indexed_skills.get(&dir_name.to_lowercase()) {
                    if !skill.apps.is_enabled_for(app) {
                        Self::remove_path(&path)?;
                    }
                    continue;
                }

                if Self::is_symlink_to_ssot(&path, &ssot_dir) {
                    Self::remove_path(&path)?;
                }
            }
        }

        for skill in skills.values() {
            if skill.apps.is_enabled_for(app) {
                // 逐条容错而非 `?` 传播：本函数在切换供应商时被调用，一条脏
                // directory（存量点开头目录、或同步导入灌进来的行）不得让整个
                // 应用的 skill 同步全部失效。
                if let Err(err) = Self::sync_to_app_dir(&skill.directory, app) {
                    log::warn!(
                        "同步 skill {} 到 {app:?} 失败，跳过该条: {err}",
                        skill.directory
                    );
                }
            }
        }

        Ok(())
    }

    // ========== 发现功能（保留原有逻辑）==========

    /// 列出所有可发现的技能（从仓库获取）
    pub async fn discover_available(
        &self,
        repos: Vec<SkillRepo>,
    ) -> Result<Vec<DiscoverableSkill>> {
        let mut skills = Vec::new();

        // 仅使用启用的仓库
        let enabled_repos: Vec<SkillRepo> = repos.into_iter().filter(|repo| repo.enabled).collect();

        let fetch_tasks = enabled_repos
            .iter()
            .map(|repo| self.fetch_repo_skills(repo));

        let results: Vec<Result<Vec<DiscoverableSkill>>> =
            futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results) {
            match result {
                Ok(repo_skills) => skills.extend(repo_skills),
                Err(e) => log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e),
            }
        }

        // 去重并排序
        Self::deduplicate_discoverable_skills(&mut skills);
        skills.sort_by_key(|skill| skill.name.to_lowercase());

        Ok(skills)
    }

    /// 列出所有技能（兼容旧 API）
    pub async fn list_skills(
        &self,
        repos: Vec<SkillRepo>,
        db: &Arc<Database>,
    ) -> Result<Vec<Skill>> {
        // 获取可发现的技能
        let discoverable = self.discover_available(repos).await?;

        // 获取已安装的技能
        let installed = db.get_all_installed_skills()?;
        let installed_dirs: HashSet<String> =
            installed.values().map(|s| s.directory.clone()).collect();

        // 转换为 Skill 格式
        let mut skills: Vec<Skill> = discoverable
            .into_iter()
            .map(|d| {
                let install_name = Path::new(&d.directory)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| d.directory.clone());

                Skill {
                    key: d.key,
                    name: d.name,
                    description: d.description,
                    directory: d.directory,
                    readme_url: d.readme_url,
                    installed: installed_dirs.contains(&install_name),
                    repo_owner: Some(d.repo_owner),
                    repo_name: Some(d.repo_name),
                    repo_branch: Some(d.repo_branch),
                }
            })
            .collect();

        // 添加本地已安装但不在仓库中的技能
        for skill in installed.values() {
            let already_in_list = skills.iter().any(|s| {
                let s_install_name = Path::new(&s.directory)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| s.directory.clone());
                s_install_name == skill.directory
            });

            if !already_in_list {
                skills.push(Skill {
                    key: skill.id.clone(),
                    name: skill.name.clone(),
                    description: skill.description.clone().unwrap_or_default(),
                    directory: skill.directory.clone(),
                    readme_url: skill.readme_url.clone(),
                    installed: true,
                    repo_owner: skill.repo_owner.clone(),
                    repo_name: skill.repo_name.clone(),
                    repo_branch: skill.repo_branch.clone(),
                });
            }
        }

        skills.sort_by_key(|skill| skill.name.to_lowercase());

        Ok(skills)
    }

    /// 从仓库获取技能列表
    async fn fetch_repo_skills(&self, repo: &SkillRepo) -> Result<Vec<DiscoverableSkill>> {
        let (temp_guard, resolved_branch) =
            timeout(std::time::Duration::from_secs(60), self.download_repo(repo))
                .await
                .map_err(|_| {
                    anyhow!(format_skill_error(
                        "DOWNLOAD_TIMEOUT",
                        &[
                            ("owner", &repo.owner),
                            ("name", &repo.name),
                            ("timeout", "60")
                        ],
                        Some("checkNetwork"),
                    ))
                })??;

        let mut skills = Vec::new();
        let scan_dir = temp_guard.path();
        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch;
        self.scan_dir_recursive(scan_dir, scan_dir, &resolved_repo, &mut skills)?;

        Ok(skills)
    }

    /// 递归扫描目录查找 SKILL.md
    fn scan_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &SkillRepo,
        skills: &mut Vec<DiscoverableSkill>,
    ) -> Result<()> {
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            let directory = if current_dir == base_dir {
                repo.name.clone()
            } else {
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .replace('\\', "/")
            };

            let doc_path = skill_md
                .strip_prefix(base_dir)
                .unwrap_or(skill_md.as_path())
                .to_string_lossy()
                .replace('\\', "/");

            if let Ok(skill) =
                self.build_skill_from_metadata(&skill_md, &directory, &doc_path, repo)
            {
                skills.push(skill);
            }

            return Ok(());
        }

        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir_recursive(&path, base_dir, repo, skills)?;
            }
        }

        Ok(())
    }

    /// 从 SKILL.md 构建技能对象
    fn build_skill_from_metadata(
        &self,
        skill_md: &Path,
        directory: &str,
        doc_path: &str,
        repo: &SkillRepo,
    ) -> Result<DiscoverableSkill> {
        let meta = self.parse_skill_metadata(skill_md)?;

        Ok(DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Self::build_skill_doc_url(&repo.owner, &repo.name, &repo.branch, doc_path),
            repo_owner: repo.owner.clone(),
            repo_name: repo.name.clone(),
            repo_branch: repo.branch.clone(),
        })
    }

    /// 解析技能元数据
    fn parse_skill_metadata(&self, path: &Path) -> Result<SkillMetadata> {
        Self::parse_skill_metadata_static(path)
    }

    /// 静态方法：解析技能元数据
    fn parse_skill_metadata_static(path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(path)?;
        let content = content.trim_start_matches('\u{feff}');

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Ok(SkillMetadata {
                name: None,
                description: None,
            });
        }

        let front_matter = parts[1].trim();
        let meta: SkillMetadata = serde_yaml::from_str(front_matter).unwrap_or(SkillMetadata {
            name: None,
            description: None,
        });

        Ok(meta)
    }

    /// 从 SKILL.md 读取名称和描述，不存在则用目录名兜底
    fn read_skill_name_desc(skill_md: &Path, fallback_name: &str) -> (String, Option<String>) {
        if skill_md.exists() {
            match Self::parse_skill_metadata_static(skill_md) {
                Ok(meta) => (
                    meta.name.unwrap_or_else(|| fallback_name.to_string()),
                    meta.description,
                ),
                Err(_) => (fallback_name.to_string(), None),
            }
        } else {
            (fallback_name.to_string(), None)
        }
    }

    /// 校验并规范化技能源路径（允许多级目录），拒绝路径穿越和绝对路径
    fn sanitize_skill_source_path(raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut normalized = PathBuf::new();
        let mut has_component = false;

        for component in Path::new(trimmed).components() {
            match component {
                Component::Normal(name) => {
                    let segment = name.to_string_lossy().trim().to_string();
                    if segment.is_empty() || segment == "." || segment == ".." {
                        return None;
                    }
                    normalized.push(segment);
                    has_component = true;
                }
                Component::CurDir
                | Component::ParentDir
                | Component::RootDir
                | Component::Prefix(_) => {
                    return None;
                }
            }
        }

        has_component.then_some(normalized)
    }

    /// 校验并规范化安装目录名（最终落盘目录名，仅单段）
    fn sanitize_install_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 显式拒绝两种分隔符，不能依赖 components() 的平台语义：
        // `\` 在 Linux/macOS 上不是分隔符，会被当成合法单段名放行，
        // 但同一个值同步/还原到 Windows 上就变成了嵌套路径。
        if trimmed.contains('/') || trimmed.contains('\\') {
            return None;
        }

        let path = Path::new(trimmed);
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => {
                let normalized = name.to_string_lossy().trim().to_string();
                if normalized.is_empty()
                    || normalized == "."
                    || normalized == ".."
                    || normalized.starts_with('.')
                {
                    None
                } else {
                    Some(normalized)
                }
            }
            _ => None,
        }
    }

    /// 校验来自 DB 行 / 备份 meta.json 等外部来源的 directory 字段。
    ///
    /// 存储值按构造本应是单段安装名（见 sanitize_install_name），但有两个入口
    /// 会绕过安装期校验：同步导入的远端快照直接灌库（raw SQL），以及手工放置 /
    /// 不可信备份里的 meta.json。任何把它 join 进文件系统路径的使用点（尤其是
    /// remove_dir_all 这类删除操作）必须先过这道校验，拒绝路径穿越。
    ///
    /// 只校验、不归一化：`sanitize_install_name` 会 `trim()`，若拿它的返回值替换
    /// 原值，磁盘上真实带空格的目录名就再也 join 不中。所以这里要求归一化结果与
    /// 原值逐字相同，否则一律视为非法。
    fn require_valid_directory(directory: &str) -> Result<String> {
        match Self::sanitize_install_name(directory) {
            Some(normalized) if normalized == directory => Ok(normalized),
            _ => Err(anyhow!(
                "Invalid skill directory (possible path traversal): {directory:?}"
            )),
        }
    }

    /// GitHub 账号名（user / org login）。
    ///
    /// 只放行 ASCII 字母数字与 `-`。这比 GitHub 自身的规则更严，但该字段会被拼进
    /// 下载 URL，任何 `/`、`.`、`%`、`\` 都可能改写请求落点（见 validate_repo_ref）。
    fn is_valid_github_owner(owner: &str) -> bool {
        !owner.is_empty()
            && owner.len() <= 39
            && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// GitHub 仓库名。允许 `.` `-` `_`，但整体不能是 `.` 或 `..`。
    fn is_valid_github_repo_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 100
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    }

    /// git 分支名。
    ///
    /// 分支名合法含 `/`（`feature/x`），所以不能整体禁掉分隔符——按段做白名单。
    /// 逐段 `!starts_with('.')` 比整体 `contains("..")` 更稳：它同时挡掉 `a/./b`、
    /// `a/.../b` 这类变形。除 `git check-ref-format` 的规则外还额外禁掉 `#` 与 `%`：
    /// 前者会把 URL 后半截变成 fragment，后者可用百分号编码绕过字符检查。
    fn is_valid_git_branch(branch: &str) -> bool {
        // 空串和 "HEAD" 都是 `download_repo` 的哨兵，语义都是「用仓库默认分支」：
        // 分支候选表对两者一视同仁地跳过，改试 main / master，所以它们**永远不会
        // 被拼进 URL**，也就没有可校验的攻击面。空串必须放行——`skill_repos` 的
        // 存量行可以是空 branch（建表默认值是 'main'，但不禁止空串），前端两处
        // `repo.branch || "main"` 就是照着这个前提写的。把它当非法会让那些仓库
        // 在 download_repo 第一行就报 INVALID_REPO_REF，技能面板直接列不出来。
        if branch.is_empty() || branch.eq_ignore_ascii_case("HEAD") {
            return true;
        }
        if branch.len() > 255 {
            return false;
        }
        if branch.starts_with('/') || branch.ends_with('/') || branch.contains("//") {
            return false;
        }
        if branch.contains("@{") {
            return false;
        }
        // `is_ascii_control()` 的范围是 U+0000..=U+001F **加上** U+007F DELETE，
        // 所以不需要另外再点名 DEL。
        if branch
            .chars()
            .any(|c| c.is_ascii_control() || " ~^:?*[\\#%".contains(c))
        {
            return false;
        }
        branch.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && !segment.ends_with('.')
                && !segment.ends_with(".lock")
        })
    }

    /// 校验一组仓库坐标，用于任何会被拼进 github.com URL 的地方。
    ///
    /// 动机：`download_repo` 把 owner/name/branch 直接 format 进
    /// `https://github.com/{owner}/{name}/archive/refs/heads/{branch}.zip`，而 URL
    /// 解析会消解点段——branch 写成 `../../../releases/download/v1/evil` 时，落点变成
    /// 该仓库的 **release asset**，即攻击者可上传的任意字节。归档内容一旦可控，
    /// 解压路径校验就成了唯一防线，所以这一层必须堵死。
    pub(crate) fn validate_repo_ref(owner: &str, name: &str, branch: &str) -> Result<()> {
        if !Self::is_valid_github_owner(owner) || !Self::is_valid_github_repo_name(name) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        if !Self::is_valid_git_branch(branch) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name), ("branch", branch)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 出口断言：URL 拼好后再确认它确实指向预期的 github.com 路径。
    ///
    /// 这是纵深防御——即便上面的字符集校验将来漏了某种变形（百分号编码、新的
    /// 分隔符语义等），这里也能拦住落点被改写的请求。
    fn assert_github_archive_url(url: &str, owner: &str, name: &str) -> Result<()> {
        let parsed = url::Url::parse(url).map_err(|e| anyhow!("Invalid archive URL: {e}"))?;
        let expected_prefix = format!("/{owner}/{name}/archive/refs/heads/");
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || !parsed.path().starts_with(&expected_prefix)
        {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 在目录树中查找名称匹配且包含 SKILL.md 的子目录
    ///
    /// 用于 skills.sh 安装回退：API 只返回 skillId（如 "find-skills"），
    /// 但实际文件可能在仓库子目录中（如 "skills/find-skills"）。
    fn find_skill_dir_by_name(root: &Path, target_name: &str) -> Option<PathBuf> {
        fn walk(dir: &Path, target: &str, depth: usize) -> Option<PathBuf> {
            if depth > 3 {
                return None;
            }
            let entries = fs::read_dir(dir).ok()?;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue;
                }
                if name_str.eq_ignore_ascii_case(target) && path.join("SKILL.md").exists() {
                    return Some(path);
                }
                if let Some(found) = walk(&path, target, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        walk(root, target_name, 0)
    }

    /// 将 discoverable skill 的目录信息重新解析为解压目录中的真实源目录。
    ///
    /// **核心原则：返回的目录必定含 `SKILL.md`**（以 SKILL.md 为锚点）。解析顺序：
    /// 1. 直接相对路径命中（如 `skills/foo`），校验含 `SKILL.md`——明确路径优先；
    /// 2. 按安装名递归查找名字匹配 **且** 含 `SKILL.md` 的目录；
    /// 3. 兜底：仓库根本身含 `SKILL.md`。
    fn resolve_skill_source_dir(root: &Path, raw_directory: &str) -> Option<PathBuf> {
        let source_rel = Self::sanitize_skill_source_path(raw_directory)?;
        let install_name = source_rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())?;

        // 1. 直接相对路径命中（明确路径优先）——必须校验 SKILL.md，否则同名空壳目录
        //    （如 ast-grep/agent-skill 根下的 plugin 包目录 ast-grep/）会被误判为源目录。
        let direct = root.join(&source_rel);
        if direct.is_dir() && direct.join("SKILL.md").is_file() {
            return Some(direct);
        }

        // 2. 按名字递归查找（find_skill_dir_by_name 已校验 SKILL.md）
        if let Some(found) = Self::find_skill_dir_by_name(root, &install_name) {
            log::info!(
                "Skill directory '{}' not found at direct path, using fallback: {}",
                install_name,
                found.display()
            );
            return Some(found);
        }

        // 3. 兜底：仓库根本身是 skill
        if root.join("SKILL.md").is_file() {
            log::info!(
                "Skill directory '{}' not found, but SKILL.md exists at root, using repo root",
                install_name,
            );
            return Some(root.to_path_buf());
        }

        None
    }

    /// 由真实解析出的源目录推导 SKILL.md 在仓库内的相对文档路径（正斜杠）。
    /// 两个参数都应是已 canonicalize 的路径（安装流程已做包含性校验）。
    fn doc_path_for_source(repo_root: &Path, source: &Path) -> Option<String> {
        let rel = source.strip_prefix(repo_root).ok()?;
        let mut parts: Vec<String> = rel
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        parts.push("SKILL.md".to_string());
        Some(parts.join("/"))
    }

    /// 选择 readme_url 使用的仓库内文档路径：真实解析出的源目录优先，其次是
    /// 旧 readme_url 中保存的路径，最后才按 directory 拼接。skills.sh 的
    /// `directory` 只是 skillId（末级目录名），嵌套目录场景直接拼接会丢路径、
    /// 文档链接 404（#6111），所以真实源目录必须排第一优先级。
    fn choose_doc_path(
        resolved_source_doc_path: Option<String>,
        readme_url: Option<&str>,
        directory: &str,
    ) -> String {
        if let Some(path) = resolved_source_doc_path {
            return path;
        }
        if let Some(path) = readme_url.and_then(Self::extract_doc_path_from_url) {
            if path.ends_with("/SKILL.md") || path == "SKILL.md" {
                return path;
            }
            return format!("{}/SKILL.md", path.trim_end_matches('/'));
        }
        format!("{}/SKILL.md", directory.trim_end_matches('/'))
    }

    /// 去重技能列表（基于完整 key，不同仓库的同名 skill 分开显示）
    fn deduplicate_discoverable_skills(skills: &mut Vec<DiscoverableSkill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            // 使用完整 key（owner/repo:directory）作为唯一标识
            // 这样不同仓库的同名 skill 会分开显示
            let unique_key = skill.key.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(unique_key) {
                e.insert(true);
                true
            } else {
                false
            }
        });
    }

    /// 下载仓库
    ///
    /// 这里是仓库坐标进入 URL 的**唯一收敛点**——`fetch_repo_skills`、`install`、
    /// `check_updates`、`update_skill` 四条路径都经过它，而 `skill_repos` / `skills`
    /// 两张表都会被同步导入的远端快照整表覆盖，入库校验管不住它们。所以主防线放这里。
    async fn download_repo(&self, repo: &SkillRepo) -> Result<(tempfile::TempDir, String)> {
        Self::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;

        // 守卫全程持有，成功后连同目录一起交给调用方（见 `extract_local_zip` 的说明）。
        // 原来这里立刻 keep()，任何一步失败——下载超时、ARCHIVE_TOO_LARGE、解压出错
        // ——都会把半个解压目录永久留在磁盘上，反复触发即可持续填盘。
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();

        let mut branches = Vec::new();
        if !repo.branch.is_empty() && !repo.branch.eq_ignore_ascii_case("HEAD") {
            branches.push(repo.branch.as_str());
        }
        if !branches.contains(&"main") {
            branches.push("main");
        }
        if !branches.contains(&"master") {
            branches.push("master");
        }

        let mut last_error = None;
        for branch in branches {
            let url = format!(
                "https://github.com/{}/{}/archive/refs/heads/{}.zip",
                repo.owner, repo.name, branch
            );
            Self::assert_github_archive_url(&url, &repo.owner, &repo.name)?;

            match self.download_and_extract(&url, &temp_path).await {
                Ok(_) => return Ok((temp_dir, branch.to_string())),
                Err(e) => {
                    // 每个分支各自重算预算，所以失败后必须把上一轮的残留清掉——
                    // 否则 N 个候选分支等于 N 倍的落盘量堆在同一个目录里。
                    let _ = fs::remove_dir_all(&temp_path);
                    let _ = fs::create_dir_all(&temp_path);
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("所有分支下载失败")))
    }

    /// 下载并解压 ZIP
    async fn download_and_extract(&self, url: &str, dest: &Path) -> Result<()> {
        let client = crate::proxy::http_client::get();
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            let status = response.status().as_u16().to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "DOWNLOAD_FAILED",
                &[("status", &status)],
                match status.as_str() {
                    "403" => Some("http403"),
                    "404" => Some("http404"),
                    "429" => Some("http429"),
                    _ => Some("checkNetwork"),
                },
            )));
        }

        // 逐块读并卡住压缩体大小：`response.bytes()` 会先把攻击者控制的整个归档
        // 收进内存，之后才轮到 ZipArchive 和解压预算——那时候堆已经被吃光了。
        // 不能只信 Content-Length（可以撒谎或缺失），必须按实际收到的字节数算。
        let mut response = response;
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
                let limit_mb = (MAX_ARCHIVE_DOWNLOAD_BYTES / 1024 / 1024).to_string();
                return Err(anyhow::anyhow!(format_skill_error(
                    "ARCHIVE_TOO_LARGE",
                    &[("limit_mb", &limit_mb)],
                    Some("checkZipContent"),
                )));
            }
            body.extend_from_slice(&chunk);
        }

        let cursor = std::io::Cursor::new(body);
        let archive = zip::ZipArchive::new(cursor)?;
        Self::extract_repo_archive(archive, dest)
    }

    /// 按预算把单个归档条目写出，累计超限即中止。
    ///
    /// 逐块累加而非读取归档头里声明的 size：那个值由归档作者填写，压缩炸弹会撒谎。
    fn copy_entry_within_budget<R: std::io::Read, W: std::io::Write>(
        reader: &mut R,
        writer: &mut W,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            Self::charge_archive_budget(total_bytes, read as u64)?;
            writer.write_all(&buffer[..read])?;
        }
    }

    /// 读取 symlink 条目声明的目标路径。
    ///
    /// 这条分支曾是唯一一处不经预算的解压：`read_to_string` 直接把整条解压流吞进
    /// 内存，而 zip 2.4.2 的 `make_reader`（read.rs:437-449）只叠了 CRC 校验，
    /// **没有**按声明的 uncompressed_size 截断。于是一个打着 symlink 标志、
    /// deflate 后能膨胀到数 GB 的条目就是一颗内存炸弹，且预算读数全程为 0。
    ///
    /// 超长或非 UTF-8 一律返回 `None` 让调用方跳过：合法的 symlink 目标是一条
    /// 路径，这两种形状都不可能是真实数据。
    fn read_symlink_target<R: std::io::Read>(
        reader: &mut R,
        total_bytes: &mut u64,
    ) -> Result<Option<String>> {
        let mut raw = Vec::new();
        // 多读一个字节，用来区分"正好到上限"和"被截断"
        let mut limited = std::io::Read::take(reader, MAX_SYMLINK_TARGET_BYTES + 1);
        std::io::Read::read_to_end(&mut limited, &mut raw)?;
        if raw.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
            return Ok(None);
        }
        Self::charge_archive_budget(total_bytes, raw.len() as u64)?;
        Ok(String::from_utf8(raw)
            .ok()
            .map(|target| target.trim().to_string()))
    }

    /// 建目录并按**实际新建的层数**计费。
    ///
    /// `create_dir_all` 会一次性把缺失的父目录全建出来，所以一个条目名
    /// `a/a/…/a/f.txt` 可以隐式造出几百层目录。只在 symlink 物化那条路径上给目录
    /// 计费是不够的：常规解压这条路上，不到 10_000 个条目照样能造出数百万目录，
    /// 而内容字节几乎为零。
    fn create_dir_all_within_budget(path: &Path, total_bytes: &mut u64) -> Result<()> {
        let missing = path.ancestors().take_while(|p| !p.exists()).count() as u64;
        if missing > 0 {
            Self::charge_archive_budget(total_bytes, missing * DIRECTORY_BUDGET_COST)?;
        }
        fs::create_dir_all(path)?;
        Ok(())
    }

    /// 归档预算的唯一扣费点。
    ///
    /// 抽出来是因为「写文件内容」不是归档能消耗的唯一资源：symlink 物化出来的
    /// 目录一个字节都不写，但每一个都要占 inode 与一个目录块，而第二遍的
    /// symlink 解析可以让目录数量按层数指数增长。只按内容字节计费时，一个全是
    /// 空目录的归档能把预算读数一直停在 0。
    fn charge_archive_budget(total_bytes: &mut u64, amount: u64) -> Result<()> {
        if total_bytes.saturating_add(amount) > MAX_ARCHIVE_TOTAL_BYTES {
            let limit_mb = (MAX_ARCHIVE_TOTAL_BYTES / 1024 / 1024).to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_LARGE",
                &[("limit_mb", &limit_mb)],
                Some("checkZipContent"),
            )));
        }
        *total_bytes += amount;
        Ok(())
    }

    /// 把 GitHub 仓库归档解压到 `dest`（剥掉归档自带的一层根目录）。
    ///
    /// 与 `download_and_extract` 分离，使 zip-slip 防护可在不联网的情况下被单测覆盖。
    fn extract_repo_archive<R: std::io::Read + std::io::Seek>(
        mut archive: zip::ZipArchive<R>,
        dest: &Path,
    ) -> Result<()> {
        let root_name = if !archive.is_empty() {
            let first_file = archive.by_index(0)?;
            let name = first_file.name();
            name.split('/').next().unwrap_or("").to_string()
        } else {
            return Err(anyhow::anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkRepoUrl"),
            )));
        };

        // 归档字节完全由第三方控制（仓库可经 deeplink 添加），所以解压必须限量，
        // 否则一个几 MB 的压缩炸弹就能塞满磁盘。webdav_sync/archive.rs 早有同款
        // 双重上限，这条下载路径一直没有。
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }
        let mut total_bytes: u64 = 0;

        // 第一遍：解压普通文件和目录，收集 symlink 条目
        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            // 第一道：enclosed_name() 拒绝绝对路径、盘符前缀，以及净深度为负
            // （即逃出归档自身根目录）的条目。skill 仓库可由 deeplink 添加，
            // 压缩包内容属第三方可控输入。
            let Some(safe_path) = file.enclosed_name() else {
                log::warn!("跳过不安全的压缩包条目: {}", file.name());
                continue;
            };

            // GitHub 归档统一带一层 `<repo>-<branch>/` 根目录，需剥掉后再落盘。
            let Ok(relative_path) = safe_path.strip_prefix(&root_name) else {
                continue;
            };

            // 第二道：enclosed_name() 的保证是相对**归档根**的，且它不规范化路径
            // ——`..` 会原样留在返回值里。上面剥掉 root_name 等于花掉一级深度预算，
            // 于是 `repo-main/../evil` 这类条目仍能落到 dest 之外（Unix 逃一层；
            // Windows 上 root_name 可含反斜杠而被当作多段，逃逸深度随之放大）。
            // 因此 join 之前必须对**实际使用的相对路径**再验一次。
            if relative_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                log::warn!("跳过越界的压缩包条目: {}", file.name());
                continue;
            }

            if relative_path.as_os_str().is_empty() {
                continue;
            }

            let outpath = dest.join(relative_path);

            if file.is_symlink() {
                let Some(target) = Self::read_symlink_target(&mut file, &mut total_bytes)? else {
                    log::warn!("跳过目标不合法的 symlink 条目: {}", file.name());
                    continue;
                };
                symlinks.push((outpath, target));
            } else if file.is_dir() {
                Self::create_dir_all_within_budget(&outpath, &mut total_bytes)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    Self::create_dir_all_within_budget(parent, &mut total_bytes)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                // 按实际写入的字节累计，而不是信任归档头里声明的 size——
                // 压缩炸弹的声明值可以是假的。
                Self::copy_entry_within_budget(&mut file, &mut outfile, &mut total_bytes)?;
            }
        }

        // 第二遍：解析 symlink，将目标内容复制到 symlink 位置
        Self::resolve_symlinks_in_dir(dest, &symlinks, &mut total_bytes)?;

        Ok(())
    }

    /// 与 `copy_dir_recursive` 同语义，但把写出的字节计入归档总预算。
    /// 仅用于解压期间物化 symlink——常规的目录复制（安装、备份、迁移）不该受
    /// 归档预算约束，所以两个函数刻意不合并。
    fn copy_dir_within_budget(src: &Path, dest: &Path, total_bytes: &mut u64) -> Result<()> {
        Self::create_dir_all_within_budget(dest, total_bytes)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Self::copy_dir_within_budget(&path, &dest_path, total_bytes)?;
            } else {
                Self::copy_file_within_budget(&path, &dest_path, total_bytes)?;
            }
        }

        Ok(())
    }

    /// 复制单个文件并计入归档总预算，复用 `copy_entry_within_budget` 以保证
    /// 上限与报错文案只有一处定义。
    fn copy_file_within_budget(src: &Path, dest: &Path, total_bytes: &mut u64) -> Result<()> {
        let mut reader = fs::File::open(src)?;
        let mut writer = fs::File::create(dest)?;
        Self::copy_entry_within_budget(&mut reader, &mut writer, total_bytes)
    }

    /// 递归复制目录
    fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Self::copy_dir_recursive(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path)?;
            }
        }

        Ok(())
    }

    fn resolve_uninstall_backup_source_excluding(
        skill: &InstalledSkill,
        excluded_path: Option<&Path>,
    ) -> Result<Option<PathBuf>> {
        // 返回值会被整目录复制进 ~/.cc-switch/skill-backups/ 并由 get_skill_backups
        // 在界面上列出——脏 directory 在这里等于任意文件读取 + 外泄通道。
        let directory = Self::require_valid_directory(&skill.directory)?;

        let ssot_path = Self::get_ssot_dir()?.join(&directory);
        if ssot_path.is_dir()
            && !excluded_path.is_some_and(|path| Self::paths_overlap(&ssot_path, path))
        {
            return Ok(Some(ssot_path));
        }

        for app in AppType::all() {
            let app_dir = match Self::get_app_skills_dir(&app) {
                Ok(dir) => dir,
                Err(_) => continue,
            };
            let candidate = app_dir.join(&directory);
            if candidate.is_dir()
                && !excluded_path.is_some_and(|path| Self::paths_overlap(&candidate, path))
            {
                return Ok(Some(candidate));
            }
        }

        Ok(None)
    }

    fn sanitize_backup_segment(segment: &str) -> String {
        let sanitized = segment
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
                _ => '-',
            })
            .collect::<String>()
            .trim_matches('-')
            .to_string();

        if sanitized.is_empty() {
            "skill".to_string()
        } else {
            sanitized
        }
    }

    fn cleanup_old_skill_backups(dir: &Path) -> Result<()> {
        let mut entries = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                if !metadata.is_dir() {
                    return None;
                }
                Some((entry.path(), metadata.modified().ok()))
            })
            .collect::<Vec<_>>();

        if entries.len() <= SKILL_BACKUP_RETAIN_COUNT {
            return Ok(());
        }

        entries.sort_by_key(|(_, modified)| *modified);
        let remove_count = entries.len().saturating_sub(SKILL_BACKUP_RETAIN_COUNT);

        for (path, _) in entries.into_iter().take(remove_count) {
            fs::remove_dir_all(&path)?;
        }

        Ok(())
    }

    fn backup_path_for_id(backup_id: &str) -> Result<PathBuf> {
        if backup_id.contains("..")
            || backup_id.contains('/')
            || backup_id.contains('\\')
            || backup_id.trim().is_empty()
        {
            return Err(anyhow!("Invalid backup id: {backup_id}"));
        }

        Ok(Self::get_backup_dir()?.join(backup_id))
    }

    fn read_backup_metadata(backup_path: &Path) -> Result<SkillBackupMetadata> {
        let metadata_path = backup_path.join("meta.json");
        let content = fs::read_to_string(&metadata_path)
            .with_context(|| format!("failed to read {}", metadata_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", metadata_path.display()))
    }

    fn create_uninstall_backup(skill: &InstalledSkill) -> Result<Option<PathBuf>> {
        Self::create_uninstall_backup_excluding(skill, None)
    }

    fn create_uninstall_backup_excluding(
        skill: &InstalledSkill,
        excluded_path: Option<&Path>,
    ) -> Result<Option<PathBuf>> {
        let Some(source_path) =
            Self::resolve_uninstall_backup_source_excluding(skill, excluded_path)?
        else {
            log::warn!(
                "Skill {} 卸载前未找到可备份的目录，将跳过备份",
                skill.directory
            );
            return Ok(None);
        };

        let backup_root = Self::get_backup_dir()?;
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let slug = Self::sanitize_backup_segment(&skill.directory);
        let mut backup_path = backup_root.join(format!("{timestamp}_{slug}"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_path = backup_root.join(format!("{timestamp}_{slug}_{counter}"));
            counter += 1;
        }

        let write_backup = || -> Result<()> {
            let skill_backup_dir = backup_path.join("skill");
            Self::copy_dir_recursive(&source_path, &skill_backup_dir)?;

            let metadata = SkillBackupMetadata {
                skill: skill.clone(),
                backup_created_at: Utc::now().timestamp(),
                source_path: source_path.to_string_lossy().to_string(),
            };
            let metadata_path = backup_path.join("meta.json");
            let metadata_json = serde_json::to_string_pretty(&metadata)
                .context("failed to serialize skill backup metadata")?;
            fs::write(&metadata_path, metadata_json)
                .with_context(|| format!("failed to write {}", metadata_path.display()))?;
            Ok(())
        };

        if let Err(err) = write_backup() {
            let _ = fs::remove_dir_all(&backup_path);
            return Err(err);
        }

        if let Err(err) = Self::cleanup_old_skill_backups(&backup_root) {
            log::warn!("清理旧 Skill 备份失败: {err:#}");
        }

        log::info!(
            "Skill {} 已在卸载前备份到 {}",
            skill.name,
            backup_path.display()
        );

        Ok(Some(backup_path))
    }

    /// 解析 ZIP 中的符号链接：将目标内容复制到 symlink 位置
    ///
    /// GitHub ZIP 归档保留了 symlink 元数据，解压时可通过 `is_symlink()` 检测。
    /// 此方法将 symlink 解析为实际文件/目录内容（而非创建真实 symlink），
    /// 以确保跨平台兼容且 skill 内容自包含。
    fn resolve_symlinks_in_dir(
        base_dir: &Path,
        symlinks: &[(PathBuf, String)],
        total_bytes: &mut u64,
    ) -> Result<()> {
        // 规范化 base_dir（macOS 上 /tmp → /private/tmp，需保持一致）
        let canonical_base = base_dir
            .canonicalize()
            .unwrap_or_else(|_| base_dir.to_path_buf());

        for (link_path, target) in symlinks {
            // 计算 symlink 的父目录，然后拼接目标的相对路径
            let parent = link_path.parent().unwrap_or(base_dir);
            let resolved = parent.join(target);

            // 规范化路径（解析 .. 等）
            let resolved = match resolved.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    log::warn!(
                        "Symlink 目标不存在，跳过: {} -> {}",
                        link_path.display(),
                        target
                    );
                    continue;
                }
            };

            // 安全检查一：确保目标在 base_dir 内（防止路径穿越）
            if !resolved.starts_with(&canonical_base) {
                log::warn!(
                    "Symlink 目标超出仓库范围，跳过: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 安全检查二：目标不能包含 link 自身。上面那条防的是「跑出 base」，
            // 防不住「套进自己」——`dir/link -> ..` 解析后正是 base 本身，完全
            // 合规，随后递归复制会把归档根复制进自己的子目录；每递归一层都重新
            // 看到刚落盘的副本，目录树逐层膨胀直到 PATH_MAX 才失败。
            //
            // 比较必须在**规范形式**上做：`enclosed_name()` 不规范化路径，只保证
            // 净深度非负，所以 link_path 里可能带着未消解的 `..`（`e/../d/self`）。
            // 拿它按字面跟 canonicalize 过的 resolved 比组件，第一段就会错开
            // （`e` vs `d`），检查形同虚设。link_path 自身此刻尚未落盘，但它的父
            // 目录一定存在——`resolved` 能 canonicalize 成功就蕴含了这一点。
            let canonical_link = match parent.canonicalize() {
                Ok(canonical_parent) => match link_path.file_name() {
                    Some(name) => canonical_parent.join(name),
                    None => canonical_parent,
                },
                // 父目录都不存在时退回字面形式：此时 resolved 多半也解析不出来，
                // 上面就已经 continue 了；留着只是不让守卫在意外形状上 panic。
                Err(_) => match link_path.strip_prefix(base_dir) {
                    Ok(relative) => canonical_base.join(relative),
                    Err(_) => link_path.clone(),
                },
            };
            if canonical_link.starts_with(&resolved) {
                log::warn!(
                    "Symlink 目标包含链接自身，跳过（会导致递归自复制）: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 复制目标内容到 symlink 位置。必须与解压循环共用同一个字节预算：
            // 物化走的是这条独立路径，不计费的话「一个大文件 + N 个指向它的
            // symlink」能写下 N 倍字节，而 MAX_ARCHIVE_TOTAL_BYTES 全程显示合规。
            if resolved.is_dir() {
                Self::copy_dir_within_budget(&resolved, link_path, total_bytes)?;
            } else if resolved.is_file() {
                if let Some(parent) = link_path.parent() {
                    Self::create_dir_all_within_budget(parent, total_bytes)?;
                }
                Self::copy_file_within_budget(&resolved, link_path, total_bytes)?;
            }
        }
        Ok(())
    }

    // ========== 从 ZIP 文件安装 ==========

    /// 从本地 ZIP 文件安装 Skills
    ///
    /// 流程：
    /// 1. 解压 ZIP 到临时目录
    /// 2. 扫描目录查找包含 SKILL.md 的技能
    /// 3. 复制到 SSOT 并保存到数据库
    /// 4. 同步到当前应用目录
    pub fn install_from_zip(
        db: &Arc<Database>,
        zip_path: &Path,
        current_app: &AppType,
    ) -> Result<Vec<InstalledSkill>> {
        // 解压到临时目录
        let temp_guard = Self::extract_local_zip(zip_path)?;
        let temp_dir = temp_guard.path();

        // 扫描所有包含 SKILL.md 的目录
        let skill_dirs = Self::scan_skills_in_dir(temp_dir)?;

        if skill_dirs.is_empty() {
            return Err(anyhow!(format_skill_error(
                "NO_SKILLS_IN_ZIP",
                &[],
                Some("checkZipContent"),
            )));
        }

        let _state_guard = skill_state_write_guard();
        let ssot_dir = Self::get_ssot_dir()?;
        let mut installed = Vec::new();
        let existing_skills = db.get_all_installed_skills()?;
        let zip_stem = zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        for skill_dir in skill_dirs {
            // 解析元数据（提前解析，用于确定安装名）
            let skill_md = skill_dir.join("SKILL.md");
            let meta = if skill_md.exists() {
                Self::parse_skill_metadata_static(&skill_md).ok()
            } else {
                None
            };

            // 获取目录名称作为安装名
            // 当 SKILL.md 在 ZIP 根目录时，skill_dir == temp_dir，
            // file_name() 会返回临时目录名（如 .tmpDZKGpF），需要回退到其他来源
            let install_name = {
                let dir_name = skill_dir
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                if skill_dir.as_path() == temp_dir
                    || dir_name.is_empty()
                    || dir_name.starts_with('.')
                {
                    // SKILL.md 在根目录：优先用元数据 name，否则用 ZIP 文件名
                    meta.as_ref()
                        .and_then(|m| m.name.as_deref())
                        .and_then(Self::sanitize_install_name)
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                } else {
                    Self::sanitize_install_name(&dir_name)
                        .or_else(|| {
                            meta.as_ref()
                                .and_then(|m| m.name.as_deref())
                                .and_then(Self::sanitize_install_name)
                        })
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                }
            };
            let install_name = match install_name {
                Some(name) => name,
                None => {
                    return Err(anyhow!(format_skill_error(
                        "INVALID_SKILL_DIRECTORY",
                        &[("zip", &zip_path.display().to_string())],
                        Some("checkZipContent"),
                    )));
                }
            };

            // 检查是否已有同名 directory 的 skill
            let conflict = existing_skills
                .values()
                .find(|s| s.directory.eq_ignore_ascii_case(&install_name));

            if let Some(existing) = conflict {
                log::warn!(
                    "Skill directory '{}' already exists (from {}), skipping",
                    install_name,
                    existing.id
                );
                continue;
            }

            let (name, description) = match meta {
                Some(m) => (
                    m.name.unwrap_or_else(|| install_name.clone()),
                    m.description,
                ),
                None => (install_name.clone(), None),
            };

            Self::preflight_install_destination(&skill_dir, &install_name, current_app)?;

            // 复制到 SSOT
            let dest = ssot_dir.join(&install_name);
            if dest.exists() {
                let _ = fs::remove_dir_all(&dest);
            }
            Self::copy_dir_recursive(&skill_dir, &dest)?;

            // 计算内容哈希
            let content_hash = Self::compute_dir_hash(&dest).ok();

            // 创建 InstalledSkill 记录
            let skill = InstalledSkill {
                id: format!("local:{install_name}"),
                name,
                description,
                directory: install_name.clone(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::only(current_app),
                installed_at: chrono::Utc::now().timestamp(),
                content_hash,
                updated_at: 0,
            };

            Self::persist_and_sync_new_skill(db, &skill, current_app)?;

            log::info!(
                "Skill {} installed from ZIP, enabled for {:?}",
                skill.name,
                current_app
            );
            installed.push(skill);
        }

        Ok(installed)
    }

    /// 解压本地 ZIP 文件到临时目录
    ///
    /// 返回 `TempDir` 而不是 `PathBuf`：调用方在解压之后还有扫描、复制、写库、
    /// 同步等一长串 `?`，任何一处提前返回都会把最多 512 MiB 的临时内容永久留在
    /// 磁盘上。守卫交给调用方持有，清理就变成作用域结束时自动发生，不再依赖每条
    /// 出口都记得手写 `remove_dir_all`（实测漏了不止一条）。
    fn extract_local_zip(zip_path: &Path) -> Result<tempfile::TempDir> {
        Self::extract_local_zip_in(zip_path, &std::env::temp_dir())
    }

    /// 与 [`Self::extract_local_zip`] 相同，但临时目录的落点由调用方指定。
    /// 测试用它把解压根钉在私有目录里，而不是劫持进程级 `TMPDIR`——后者会把
    /// 并发测试的临时目录一起吸进被观测目录，"目录必须为空"的断言就会随机失败。
    fn extract_local_zip_in(zip_path: &Path, base_dir: &Path) -> Result<tempfile::TempDir> {
        let file = fs::File::open(zip_path)
            .with_context(|| format!("Failed to open ZIP file: {}", zip_path.display()))?;

        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Failed to read ZIP file: {}", zip_path.display()))?;

        if archive.is_empty() {
            return Err(anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkZipContent"),
            )));
        }

        // 与远端归档同一套上限。本地 ZIP 是用户自选文件（信任度更高），但"用户
        // 被诱导打开一个压缩炸弹"仍是常见路径，且两条解压路径共用同一个物化器。
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }

        // 守卫持有到解压全部成功为止：中途任何 `?` 都会让它清掉半成品目录。
        // 原来在这里就 keep()，超限或解压出错都会留下永久残留。
        let temp_dir = tempfile::tempdir_in(base_dir)?;
        let temp_path = temp_dir.path().to_path_buf();

        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();
        let mut total_bytes: u64 = 0;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => continue,
            };

            // `enclosed_name()` 只保证净深度非负，**不消解** `..`。这里没有
            // `strip_prefix` 吃掉深度预算，所以逃不出 temp_path；但留着未消解的
            // 路径会让后面的 symlink 自包含检查失去可比性（`e/../d/self` 与 `d`
            // 逐组件比在第一段就错开）。在入口就把它们挡掉，保证落进 symlinks
            // 表里的路径都是规范形状。
            if file_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                log::warn!("跳过越界的压缩包条目: {}", file.name());
                continue;
            }

            let outpath = temp_path.join(&file_path);

            if file.is_symlink() {
                let Some(target) = Self::read_symlink_target(&mut file, &mut total_bytes)? else {
                    log::warn!("跳过目标不合法的 symlink 条目: {}", file.name());
                    continue;
                };
                symlinks.push((outpath, target));
            } else if file.is_dir() {
                Self::create_dir_all_within_budget(&outpath, &mut total_bytes)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    Self::create_dir_all_within_budget(parent, &mut total_bytes)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                Self::copy_entry_within_budget(&mut file, &mut outfile, &mut total_bytes)?;
            }
        }

        // 解析 symlink
        Self::resolve_symlinks_in_dir(&temp_path, &symlinks, &mut total_bytes)?;

        Ok(temp_dir)
    }

    /// 递归扫描目录查找包含 SKILL.md 的技能目录
    fn scan_skills_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut skill_dirs = Vec::new();
        Self::scan_skills_recursive(dir, &mut skill_dirs)?;
        Ok(skill_dirs)
    }

    /// 递归扫描辅助函数
    fn scan_skills_recursive(current: &Path, results: &mut Vec<PathBuf>) -> Result<()> {
        // 检查当前目录是否包含 SKILL.md
        let skill_md = current.join("SKILL.md");
        if skill_md.exists() {
            results.push(current.to_path_buf());
            // 找到后不再递归子目录（一个 skill 目录）
            return Ok(());
        }

        // 递归子目录
        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // 跳过隐藏目录
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    if dir_name.starts_with('.') {
                        continue;
                    }
                    Self::scan_skills_recursive(&path, results)?;
                }
            }
        }

        Ok(())
    }

    // ========== 仓库管理（保留原有逻辑）==========

    /// 列出仓库
    pub fn list_repos(&self, store: &SkillStore) -> Vec<SkillRepo> {
        store.repos.clone()
    }

    /// 添加仓库
    pub fn add_repo(&self, store: &mut SkillStore, repo: SkillRepo) -> Result<()> {
        if let Some(pos) = store
            .repos
            .iter()
            .position(|r| r.owner == repo.owner && r.name == repo.name)
        {
            store.repos[pos] = repo;
        } else {
            store.repos.push(repo);
        }

        Ok(())
    }

    /// 删除仓库
    pub fn remove_repo(&self, store: &mut SkillStore, owner: String, name: String) -> Result<()> {
        store
            .repos
            .retain(|r| !(r.owner == owner && r.name == name));

        Ok(())
    }

    // ========== skills.sh 搜索 ==========

    /// 搜索 skills.sh 公共目录
    pub async fn search_skills_sh(
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SkillsShSearchResult> {
        let client = crate::proxy::http_client::get();

        let url = url::Url::parse_with_params(
            "https://skills.sh/api/search",
            &[
                ("q", query),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ],
        )?;

        let resp = client
            .get(url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json::<SkillsShApiResponse>()
            .await?;

        let skills = resp
            .skills
            .into_iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.source.splitn(2, '/').collect();
                if parts.len() != 2 {
                    return None;
                }
                let (owner, repo) = (parts[0].to_string(), parts[1].to_string());
                // 用与 download_repo 同一套坐标校验，而不是就地写启发式：下面这个
                // readme_url 最终交给 openExternal 打开，是和 build_skill_doc_url
                // 同一个 sink。原来的 `contains('.')` 既漏（`splitn(2, '/')` 允许
                // repo 里带 `/`，`owner/a/b` 能拼出三段路径），又误伤（GitHub 仓库
                // 名合法含点）。校验 owner 同时也保留了"过滤非 GitHub 来源"的效果
                // ——`skills.volces.com` 这类带点的 owner 本来就不是合法用户名。
                if Self::validate_repo_ref(&owner, &repo, "main").is_err() {
                    return None;
                }
                Some(SkillsShDiscoverableSkill {
                    key: s.id,
                    name: s.name,
                    directory: s.skill_id.clone(),
                    repo_owner: owner.clone(),
                    repo_name: repo.clone(),
                    repo_branch: "main".to_string(),
                    installs: s.installs,
                    readme_url: Some(format!("https://github.com/{}/{}", owner, repo)),
                })
            })
            .collect();

        Ok(SkillsShSearchResult {
            skills,
            total_count: resp.count,
            query: resp.query,
        })
    }
}

// ========== 迁移支持 ==========

/// 从 lock 文件信息构建 skill 的 ID、仓库字段和 readme URL
///
/// 返回 (id, repo_owner, repo_name, repo_branch, readme_url)
fn build_repo_info_from_lock(
    lock: &HashMap<String, LockRepoInfo>,
    dir_name: &str,
) -> (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    match lock.get(dir_name) {
        Some(info) => {
            let branch = info.branch.clone();
            let url_branch = branch.clone().unwrap_or_else(|| "HEAD".to_string());
            // 优先使用 lock 文件中的 skillPath，否则回退到 dir_name/SKILL.md
            let fallback = format!("{dir_name}/SKILL.md");
            let doc_path = info.skill_path.as_deref().unwrap_or(&fallback);
            let url =
                SkillService::build_skill_doc_url(&info.owner, &info.repo, &url_branch, doc_path);
            (
                format!("{}/{}:{dir_name}", info.owner, info.repo),
                Some(info.owner.clone()),
                Some(info.repo.clone()),
                branch,
                url,
            )
        }
        None => (format!("local:{dir_name}"), None, None, None, None),
    }
}

/// 将 lock 文件中发现的仓库保存到 skill_repos（去重）
fn save_repos_from_lock(
    db: &Arc<Database>,
    lock: &HashMap<String, LockRepoInfo>,
    directories: impl Iterator<Item = impl AsRef<str>>,
) {
    let existing_repos: HashSet<(String, String)> = db
        .get_skill_repos()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.owner, r.name))
        .collect();
    let mut added = HashSet::new();

    for dir_name in directories {
        if let Some(info) = lock.get(dir_name.as_ref()) {
            let key = (info.owner.clone(), info.repo.clone());
            if !existing_repos.contains(&key) && added.insert(key) {
                let skill_repo = SkillRepo {
                    owner: info.owner.clone(),
                    name: info.repo.clone(),
                    // 未知分支时使用 HEAD 语义，后续下载会回退到 main/master。
                    branch: info.branch.clone().unwrap_or_else(|| "HEAD".to_string()),
                    enabled: true,
                };
                // lock 文件由外部 agents CLI 写入，owner/repo/branch 均未经校验，
                // 且 branch 是从 `/tree/`、fragment、`?ref=` 里抠出来的裸串。
                if SkillService::validate_repo_ref(
                    &skill_repo.owner,
                    &skill_repo.name,
                    &skill_repo.branch,
                )
                .is_err()
                {
                    log::warn!(
                        "跳过 agents lock 中坐标非法的仓库: {}/{}@{}",
                        skill_repo.owner,
                        skill_repo.name,
                        skill_repo.branch
                    );
                    continue;
                }
                if let Err(e) = db.save_skill_repo(&skill_repo) {
                    log::warn!("保存 skill 仓库 {}/{} 失败: {}", info.owner, info.repo, e);
                } else {
                    log::info!(
                        "从 agents lock 文件发现并添加仓库: {}/{} ({})",
                        info.owner,
                        info.repo,
                        skill_repo.branch
                    );
                }
            }
        }
    }
}

/// 首次启动迁移：扫描应用目录，重建数据库
pub fn migrate_skills_to_ssot(db: &Arc<Database>) -> Result<usize> {
    let _state_guard = skill_state_write_guard();
    let ssot_dir = SkillService::get_ssot_dir()?;
    let agents_lock = parse_agents_lock();
    let snapshot: Vec<LegacySkillMigrationRow> =
        match db.get_setting("skills_ssot_migration_snapshot")? {
            Some(value) if !value.trim().is_empty() => match serde_json::from_str(&value) {
                Ok(rows) => rows,
                Err(err) => {
                    log::warn!("解析 skills 迁移快照失败，将回退到文件系统扫描: {err}");
                    Vec::new()
                }
            },
            _ => Vec::new(),
        };

    let has_snapshot = !snapshot.is_empty();
    let mut discovered: HashMap<String, SkillApps> = HashMap::new();

    if has_snapshot {
        for row in &snapshot {
            // snapshot 存在 settings 表里，而 settings 在同步范围内、可被远端快照
            // 覆盖。下面 discovered 的每个 key 都会被 join 成路径并写回 skills 表，
            // 所以脏值必须在进入 discovered 之前就滤掉。
            if SkillService::require_valid_directory(&row.directory).is_err() {
                log::warn!("跳过 SSOT 迁移快照中非法的 directory: {:?}", row.directory);
                continue;
            }
            if let Ok(app) = row.app_type.parse::<AppType>() {
                discovered
                    .entry(row.directory.clone())
                    .or_default()
                    .set_enabled_for(&app, true);
            }
        }
    }

    // 扫描各应用目录
    for app in AppType::all() {
        let app_dir = match SkillService::get_app_skills_dir(&app) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let entries = match fs::read_dir(&app_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();
            if dir_name.starts_with('.') {
                continue;
            }
            if !path.join("SKILL.md").exists() {
                continue;
            }
            if has_snapshot && !discovered.contains_key(&dir_name) {
                continue;
            }

            // 复制到 SSOT（如果不存在）
            let ssot_path = ssot_dir.join(&dir_name);
            if !ssot_path.exists() {
                SkillService::copy_dir_recursive(&path, &ssot_path)?;
            }

            if !has_snapshot {
                discovered
                    .entry(dir_name)
                    .or_default()
                    .set_enabled_for(&app, true);
            }
        }
    }

    // 重建数据库
    db.clear_skills()?;

    // 将 lock 文件中发现的仓库保存到 skill_repos
    save_repos_from_lock(db, &agents_lock, discovered.keys());

    let mut count = 0;
    for (directory, apps) in discovered {
        let ssot_path = ssot_dir.join(&directory);
        let skill_md = ssot_path.join("SKILL.md");

        let (name, description) = SkillService::read_skill_name_desc(&skill_md, &directory);

        let (id, repo_owner, repo_name, repo_branch, readme_url) =
            build_repo_info_from_lock(&agents_lock, &directory);

        let content_hash = SkillService::compute_dir_hash(&ssot_path).ok();

        let skill = InstalledSkill {
            id,
            name,
            description,
            directory,
            repo_owner,
            repo_name,
            repo_branch,
            readme_url,
            apps,
            installed_at: chrono::Utc::now().timestamp(),
            content_hash,
            updated_at: 0,
        };

        db.save_skill(&skill)?;
        count += 1;
    }

    let _ = db.set_setting("skills_ssot_migration_snapshot", "");

    log::info!("Skills 迁移完成，共 {count} 个");

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn skill_state_lock_allows_snapshots_but_excludes_writers() {
        let first_reader = skill_state_read_guard();
        let second_reader = skill_state_read_guard();
        assert!(
            skill_state_lock().try_write().is_err(),
            "a Skill mutation must wait for every snapshot reader"
        );

        drop(second_reader);
        drop(first_reader);
        assert!(skill_state_lock().try_write().is_ok());
    }

    /// 构造一个模拟 GitHub 归档的 ZIP：带一层 `repo-main/` 根目录，
    /// 其中掺入用 `../` 逃逸的恶意条目。
    ///
    /// 两个恶意条目走的是**不同**的拦截层，缺一不可：
    /// - 两级 `../../`：净深度为负，`enclosed_name()` 自己就会拒绝；
    /// - 一级 `../`：净深度非负，`enclosed_name()` **放行**且原样保留 `..`，
    ///   只有剥掉 root_name 之后的组件校验才能拦住。
    fn build_zip_with_traversal_entry() -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();

            // 合法条目：会被正常解压
            zip.start_file("repo-main/SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: ok\n---\n").unwrap();

            // 恶意条目 A：被 enclosed_name() 拒绝
            zip.start_file("repo-main/../../escaped.txt", opts).unwrap();
            zip.write_all(b"pwned").unwrap();

            // 恶意条目 B：能通过 enclosed_name()，靠组件校验拦截
            zip.start_file("repo-main/../escaped-one-level.txt", opts)
                .unwrap();
            zip.write_all(b"pwned").unwrap();

            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn validate_repo_ref_accepts_real_world_coordinates() {
        // 合法分支名允许 `/`，不能因为防穿越就把它们一起禁掉
        for branch in [
            "main",
            "master",
            "HEAD",
            "feature/new-thing",
            "release/v1.2.3",
            "fix-123",
            "user.name/topic",
        ] {
            assert!(
                SkillService::validate_repo_ref("farion1231", "cc-switch", branch).is_ok(),
                "must accept branch: {branch:?}"
            );
        }
        assert!(SkillService::validate_repo_ref("a", "b.c_d-e", "main").is_ok());
    }

    #[test]
    fn validate_repo_ref_accepts_the_empty_branch_sentinel() {
        // 空 branch 与 "HEAD" 在 download_repo 里是同一个哨兵：分支候选表跳过
        // 两者，改试 main / master，所以它们从不进 URL。校验若把空串当非法，
        // 存量 skill_repos 行（建表默认 'main'，但空串没被禁）会在 download_repo
        // 第一行就 INVALID_REPO_REF，整个技能面板列不出东西——前端两处
        // `repo.branch || "main"` 正是照着"空串可用"写的。
        assert!(
            SkillService::validate_repo_ref("farion1231", "cc-switch", "").is_ok(),
            "the empty-branch sentinel must stay usable"
        );
    }

    #[test]
    fn validate_repo_ref_rejects_url_hijacking_branches() {
        // 这是核心用例：branch 被拼进 archive URL，URL 解析会消解点段，
        // 落点会从 /archive/refs/heads/ 改写成攻击者可上传的 release asset。
        for branch in [
            "../../../releases/download/v1/evil",
            "..",
            "../x",
            "a/../../b",
            "a/./b",
            "..\\..\\releases\\download\\v1\\evil",
            "/leading",
            "trailing/",
            "double//slash",
            "with space",
            "frag#ment",
            "pct%2e%2e",
            "ref@{0}",
            "seg.lock",
            ".hidden/x",
        ] {
            assert!(
                SkillService::validate_repo_ref("owner", "repo", branch).is_err(),
                "must reject branch: {branch:?}"
            );
        }
        for (owner, name) in [
            ("..", "repo"),
            ("own/er", "repo"),
            ("owner", ".."),
            ("owner", "re/po"),
            ("owner", "re po"),
            ("", "repo"),
            ("owner", ""),
        ] {
            assert!(
                SkillService::validate_repo_ref(owner, name, "main").is_err(),
                "must reject coordinates: {owner:?}/{name:?}"
            );
        }
    }

    #[test]
    fn assert_github_archive_url_pins_host_and_path() {
        let ok = "https://github.com/owner/repo/archive/refs/heads/main.zip";
        assert!(SkillService::assert_github_archive_url(ok, "owner", "repo").is_ok());

        // 出口断言必须挡住落点被改写到 release asset 的情况
        for bad in [
            "https://github.com/owner/repo/releases/download/v1/evil.zip",
            "https://evil.example/owner/repo/archive/refs/heads/main.zip",
            "http://github.com/owner/repo/archive/refs/heads/main.zip",
            "https://github.com/other/repo/archive/refs/heads/main.zip",
        ] {
            assert!(
                SkillService::assert_github_archive_url(bad, "owner", "repo").is_err(),
                "must reject url: {bad}"
            );
        }
    }

    #[test]
    fn build_skill_doc_url_drops_illegal_coordinates() {
        assert_eq!(
            SkillService::build_skill_doc_url("owner", "repo", "main", "a/SKILL.md").as_deref(),
            Some("https://github.com/owner/repo/blob/main/a/SKILL.md")
        );
        // readme_url 会被前端 openExternal 直接打开，非法坐标不得产出链接
        assert!(
            SkillService::build_skill_doc_url("owner", "repo", "../../../issues", "x").is_none()
        );
    }

    #[test]
    fn copy_entry_within_budget_stops_before_exceeding_the_limit() {
        // 预算逐块累加，超限时中止且不再继续写——压缩炸弹声明的 size 不可信，
        // 所以判断只能基于实际读到的字节。
        let mut total = MAX_ARCHIVE_TOTAL_BYTES - 8;
        let mut reader = std::io::Cursor::new(vec![7u8; 64]);
        let mut writer: Vec<u8> = Vec::new();

        let err = SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
            .expect_err("must reject once the budget is exhausted");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
        assert!(
            writer.is_empty(),
            "nothing may be written once the chunk would exceed the budget"
        );

        // 预算充足时照常写完
        let mut total = 0u64;
        let mut reader = std::io::Cursor::new(vec![7u8; 64]);
        let mut writer: Vec<u8> = Vec::new();
        SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
            .expect("within budget");
        assert_eq!(writer.len(), 64);
        assert_eq!(total, 64);
    }

    #[test]
    fn extract_repo_archive_rejects_too_many_entries() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            for i in 0..(MAX_ARCHIVE_ENTRIES + 1) {
                zip.start_file(format!("repo-main/f{i}"), opts).unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");
        let err = SkillService::extract_repo_archive(archive, temp.path())
            .expect_err("entry count over the limit must be rejected");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_MANY_ENTRIES"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn extract_repo_archive_rejects_path_traversal_entries() {
        let temp = tempdir().expect("tempdir");
        // dest 放在深一层，这样逃逸一层/两层都落在 temp 内、可被检出
        let dest = temp.path().join("nested").join("dest");
        fs::create_dir_all(&dest).expect("create dest");

        let bytes = build_zip_with_traversal_entry();
        let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("archive parses");

        SkillService::extract_repo_archive(archive, &dest).expect("extract must not fail");

        // 合法条目正常落盘
        assert!(
            dest.join("SKILL.md").is_file(),
            "legitimate entry should be extracted"
        );
        // 两级逃逸：不得写到 dest 之外
        assert!(
            !temp.path().join("escaped.txt").exists(),
            "zip-slip entry must not escape dest (temp root)"
        );
        assert!(
            !temp.path().join("nested").join("escaped.txt").exists(),
            "zip-slip entry must not escape dest (parent dir)"
        );
        // 一级逃逸：enclosed_name() 放行的那一类，必须被组件校验拦住
        assert!(
            !temp
                .path()
                .join("nested")
                .join("escaped-one-level.txt")
                .exists(),
            "single-`..` entry must not escape dest (enclosed_name allows it)"
        );
    }

    #[test]
    fn extract_repo_archive_skips_a_symlink_that_contains_itself() {
        // `dir/link -> ..` 解析后正是归档根：它**通过**「目标必须在 base 内」的
        // 检查，因为目标就是 base 本身。没有第二道自包含检查时，
        // copy_dir_recursive(base, base/dir/link) 会把根复制进自己的子目录，
        // 每递归一层都重新看到刚落盘的副本，直到 PATH_MAX 才以 IO 错误收场。
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            zip.start_file("repo-main/SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: t\ndescription: d\n---\n")
                .unwrap();
            zip.add_directory("repo-main/dir/", opts).unwrap();
            zip.add_symlink("repo-main/dir/link", "..", opts).unwrap();
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let dest = temp.path().join("dest");
        fs::create_dir_all(&dest).expect("create dest");
        let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");

        SkillService::extract_repo_archive(archive, &dest)
            .expect("a self-containing symlink must be skipped, not blow up the extraction");

        assert!(
            dest.join("SKILL.md").is_file(),
            "legitimate entries must still be extracted"
        );
        assert!(
            !dest.join("dir").join("link").exists(),
            "a symlink whose target contains the link itself must not be materialized"
        );
    }

    #[test]
    fn symlink_materialization_is_charged_to_the_archive_budget() {
        // symlink 的物化走第二遍、与解压循环不同的代码路径。若它不计入同一个
        // 预算，「一个大文件 + N 个指向它的 symlink」就能写下 N 倍字节而上限
        // 全程显示合规。这里把预算预置到接近上限来验证物化确实在计费。
        let temp = tempdir().expect("tempdir");
        let base = temp.path().join("base");
        fs::create_dir_all(base.join("payload")).expect("create payload dir");
        fs::write(base.join("payload").join("big.bin"), vec![b'x'; 4096]).expect("write payload");

        let symlinks = vec![(base.join("copy"), "payload".to_string())];
        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 1024;

        let err = SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
            .expect_err("materializing 4 KiB with 1 KiB of budget left must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn symlink_guard_sees_through_unnormalized_link_paths() {
        // `enclosed_name()` 不消解 `..`，所以 link_path 可能长成 `e/../d/self`。
        // 守卫若拿它按字面跟规范化过的目标比组件，会在第一段（`e` vs `d`）就判定
        // "不包含"——而这个位置物理上就在 `d` 里面，把 `d` 复制进去正是递归自复制。
        let temp = tempdir().expect("tempdir");
        let base = temp.path().join("base");
        fs::create_dir_all(base.join("d").join("sub")).expect("create d");
        fs::create_dir_all(base.join("e")).expect("create e");

        let link_path = base.join("e").join("..").join("d").join("self");
        let symlinks = vec![(link_path, ".".to_string())];
        let mut total_bytes = 0u64;

        SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
            .expect("a self-containing symlink must be skipped, not blow up the extraction");

        assert!(
            !base.join("d").join("self").exists(),
            "a link that physically lives inside its own target must not be materialized"
        );
    }

    /// 记录实际被消耗了多少字节的 reader。
    ///
    /// 直接断言返回值是不够的：函数末尾本就有一道长度检查，把 `take` 的上限拆掉
    /// 之后它照样返回 `None`，断言仍然通过——而炸弹的危害全在读取过程里，不在
    /// 返回值。只有观测消耗量才能真正钉住"读取是有界的"。
    struct CountingReader {
        remaining: u64,
        consumed: u64,
    }

    impl std::io::Read for CountingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = buf.len().min(self.remaining as usize);
            buf[..n].fill(b'a');
            self.remaining -= n as u64;
            self.consumed += n as u64;
            Ok(n)
        }
    }

    #[test]
    fn read_symlink_target_is_bounded_and_charged() {
        // 一个打着 symlink 标志、解压流却极大的条目：zip 2.4.2 的 make_reader
        // 不按声明的 uncompressed_size 截断，没有上限就会被整条读进内存。
        let mut oversized = CountingReader {
            remaining: 8 * 1024 * 1024,
            consumed: 0,
        };
        let mut total_bytes = 0u64;

        let target = SkillService::read_symlink_target(&mut oversized, &mut total_bytes)
            .expect("an oversized target must be skipped, not raise");
        assert!(
            target.is_none(),
            "a target longer than a path can plausibly be must be rejected"
        );
        assert_eq!(total_bytes, 0, "a rejected target must not be charged");
        assert!(
            oversized.consumed <= MAX_SYMLINK_TARGET_BYTES + 1,
            "the read must stop at the cap instead of draining the stream, consumed {}",
            oversized.consumed
        );

        // 正常目标照常读出来并计费
        let mut normal = std::io::Cursor::new(b"../shared".to_vec());
        let target = SkillService::read_symlink_target(&mut normal, &mut total_bytes)
            .expect("a normal target must be read");
        assert_eq!(target.as_deref(), Some("../shared"));
        assert_eq!(total_bytes, 9);
    }

    #[test]
    fn directory_materialization_is_charged_to_the_archive_budget() {
        // 全是空目录的归档一个内容字节都不写。不给目录计费，第二遍的 symlink
        // 解析就能让目录数按层数指数增长，而预算读数一直停在 0。
        let temp = tempdir().expect("tempdir");
        let src = temp.path().join("src");
        fs::create_dir_all(src.join("a").join("b")).expect("create tree");

        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - DIRECTORY_BUDGET_COST;
        let err =
            SkillService::copy_dir_within_budget(&src, &temp.path().join("dest"), &mut total_bytes)
                .expect_err("materializing directories past the limit must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn create_dir_all_charges_every_directory_it_creates() {
        // `create_dir_all` 一次能把缺失的父目录全建出来，所以一个条目名
        // `a/a/…/a/f.txt` 可以隐式造出几百层。按调用次数计费会严重低估。
        let temp = tempdir().expect("tempdir");
        let deep = temp.path().join("a").join("b").join("c");

        // 预算只够两层，建三层必须被拦下
        let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 2 * DIRECTORY_BUDGET_COST;
        let err = SkillService::create_dir_all_within_budget(&deep, &mut total_bytes)
            .expect_err("creating more directories than the budget allows must fail");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_LARGE"),
            "unexpected error: {err}"
        );
        assert!(
            !deep.exists(),
            "nothing must be created once the budget is exceeded"
        );
    }

    #[test]
    fn extract_local_zip_leaves_no_partial_directory_when_it_fails() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        // scratch 只喂给这一次解压：并发测试的临时目录不会落进来，
        // 所以"必须为空"的断言观测到的恰好就是这次解压的残留
        let holder = tempdir().expect("tempdir");
        let scratch = tempdir().expect("tempdir");

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            // `x` 先落成文件，再要求把它当目录用 —— create_dir_all 必然失败，
            // 而这个失败发生在临时目录已经建好、且已经写进去东西之后
            zip.start_file("x", opts).unwrap();
            zip.write_all(b"i am a file").unwrap();
            zip.start_file("x/y", opts).unwrap();
            zip.write_all(b"and my parent is not a directory").unwrap();
            zip.finish().unwrap();
        }
        let zip_path = holder.path().join("collide.zip");
        fs::write(&zip_path, &buf).expect("write zip");

        let result = SkillService::extract_local_zip_in(&zip_path, scratch.path());

        assert!(
            result.is_err(),
            "the fixture must actually fail after the temp dir exists"
        );
        let leftovers: Vec<_> = fs::read_dir(scratch.path())
            .expect("read scratch")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        assert!(
            leftovers.is_empty(),
            "a failed extraction must not leave a partial directory behind: {leftovers:?}"
        );
    }

    #[test]
    fn extract_local_zip_hands_back_a_guard_that_owns_the_tree() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        // 解压成功之后调用方还要走扫描 / 取 SSOT / 复制 / 写库 / 同步一长串 `?`。
        // 之前返回裸 PathBuf，清理靠每条出口手写 remove_dir_all——实测漏了不止一条
        // （install_from_zip 的 copy_dir_recursive 与 save_skill、update_skill 的
        // copy_dir_recursive、fetch_repo_skills 的 scan_dir_recursive 都会漏）。
        let holder = tempdir().expect("tempdir");
        let scratch = tempdir().expect("tempdir");

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            zip.start_file("s/SKILL.md", opts).unwrap();
            zip.write_all(b"# skill").unwrap();
            zip.finish().unwrap();
        }
        let zip_path = holder.path().join("ok.zip");
        fs::write(&zip_path, &buf).expect("write zip");

        let extracted = SkillService::extract_local_zip_in(&zip_path, scratch.path())
            .expect("extract must succeed");
        assert!(
            extracted.path().join("s").join("SKILL.md").exists(),
            "the fixture must actually extract something worth cleaning up"
        );

        // 模拟调用方在下游任意一个 `?` 上提前返回
        drop(extracted);

        let leftovers: Vec<_> = fs::read_dir(scratch.path())
            .expect("read scratch")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        assert!(
            leftovers.is_empty(),
            "dropping the extraction result must take the whole tree with it: {leftovers:?}"
        );
    }

    #[test]
    fn extract_local_zip_rejects_dot_dot_entries() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            zip.add_directory("d/", opts).unwrap();
            zip.add_directory("e/", opts).unwrap();
            // 净深度非负，enclosed_name() 放行；未消解的 `..` 会把它落到 d 里面
            zip.start_file("e/../d/leaked.txt", opts).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("dots.zip");
        fs::write(&zip_path, &buf).expect("write zip");

        let extracted = SkillService::extract_local_zip(&zip_path).expect("extract must not fail");
        assert!(
            !extracted.path().join("d").join("leaked.txt").exists(),
            "an entry with an unresolved `..` must be skipped, not silently relocated"
        );
    }

    #[test]
    fn extract_local_zip_rejects_too_many_entries() {
        // 本地 ZIP 走的是另一个解压器，条目上限曾只加在远端归档那条路径上。
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            for i in 0..(MAX_ARCHIVE_ENTRIES + 1) {
                zip.start_file(format!("f{i}"), opts).unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
        }

        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("bomb.zip");
        fs::write(&zip_path, &buf).expect("write zip");

        let err = SkillService::extract_local_zip(&zip_path)
            .expect_err("entry count over the limit must be rejected for local ZIPs too");
        assert!(
            err.to_string().contains("ARCHIVE_TOO_MANY_ENTRIES"),
            "unexpected error: {err}"
        );
    }

    fn write_skill(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("create skill dir");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\n"),
        )
        .expect("write SKILL.md");
    }

    #[test]
    #[serial_test::serial]
    fn pi_skill_state_follows_native_directory_presence() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
        skill.apps.pi = true;
        db.save_skill(&skill).expect("save skill");

        let installed = SkillService::get_all_installed(&db).expect("read skills");
        assert!(!installed[0].apps.pi, "a DB flag must not activate Pi");

        write_skill(
            &SkillService::get_ssot_dir().unwrap().join("test-skill"),
            "test",
        );
        SkillService::toggle_app(&db, &skill.id, &AppType::Pi, true).expect("enable Pi skill");
        assert!(SkillService::get_all_installed(&db).unwrap()[0].apps.pi);

        SkillService::toggle_app(&db, &skill.id, &AppType::Pi, false).expect("disable Pi skill");
        assert!(!SkillService::get_all_installed(&db).unwrap()[0].apps.pi);
    }

    #[test]
    #[serial_test::serial]
    fn pi_skill_toggle_preserves_a_same_name_external_directory() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        write_skill(
            &SkillService::get_ssot_dir().unwrap().join("test-skill"),
            "managed",
        );
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        write_skill(&pi_skill, "external");

        let enable = SkillService::toggle_app(&db, &skill.id, &AppType::Pi, true);
        assert!(
            enable.is_err(),
            "enable must not overwrite external content"
        );
        assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
            .unwrap()
            .contains("external"));

        let disable = SkillService::toggle_app(&db, &skill.id, &AppType::Pi, false);
        assert!(disable.is_err(), "disable must not delete external content");
        assert!(pi_skill.join("SKILL.md").exists());
    }

    #[test]
    #[serial_test::serial]
    fn pi_install_conflict_is_rejected_before_persisting() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        write_skill(
            &SkillService::get_ssot_dir().unwrap().join("test-skill"),
            "managed",
        );
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        write_skill(&pi_skill, "external");

        let result = SkillService::persist_and_sync_new_skill(&db, &skill, &AppType::Pi);

        assert!(result.is_err());
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("read skill")
            .is_none());
        assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
            .expect("read external skill")
            .contains("external"));
    }

    #[test]
    fn managed_pi_copy_refreshes_from_its_expected_old_content() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("pi-skill");
        write_skill(&source, "old");
        SkillService::copy_dir_recursive(&source, &destination).expect("seed managed copy");

        let deployment =
            SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
                .expect("inspect managed copy")
                .expect("managed deployment");

        fs::remove_dir_all(&source).expect("replace old source");
        write_skill(&source, "new");
        SkillService::refresh_pi_skill_destination(
            &source,
            &destination,
            "test-skill",
            &deployment,
        )
        .expect("refresh managed copy");

        assert!(fs::read_to_string(destination.join("SKILL.md"))
            .expect("read refreshed copy")
            .contains("name: new"));
    }

    #[test]
    fn managed_pi_copy_rejects_hidden_native_changes() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("pi-skill");
        write_skill(&source, "old");
        SkillService::copy_dir_recursive(&source, &destination).expect("seed managed copy");
        let deployment =
            SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
                .expect("inspect managed copy")
                .expect("managed deployment");

        fs::write(destination.join(".env"), "user-owned").expect("add hidden native file");
        fs::remove_dir_all(&source).expect("replace old source");
        write_skill(&source, "new");

        let result = SkillService::refresh_pi_skill_destination(
            &source,
            &destination,
            "test-skill",
            &deployment,
        );
        assert!(
            result.is_err(),
            "hidden native changes must block replacement"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".env")).expect("read hidden native file"),
            "user-owned"
        );
    }

    #[test]
    #[serial_test::serial]
    fn importing_a_native_pi_skill_returns_its_derived_active_state() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("native-skill");
        write_skill(&pi_skill, "native");

        let imported = SkillService::import_from_apps(
            &db,
            vec![ImportSkillSelection {
                directory: "native-skill".to_string(),
                apps: SkillApps::default(),
            }],
        )
        .expect("import native Pi skill");

        assert_eq!(imported.len(), 1);
        assert!(imported[0].apps.pi);
        assert!(SkillService::get_all_installed(&db).unwrap()[0].apps.pi);
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_preserves_an_external_pi_skill_and_removes_the_managed_record() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        write_skill(&pi_skill, "external");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

        assert!(!source.exists(), "managed source must be removed");
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
        assert_eq!(
            result.preserved_pi_path,
            Some(pi_skill.to_string_lossy().to_string())
        );
        assert!(result.pi_cleanup_incomplete);
        assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
            .expect("read external skill")
            .contains("name: external"));
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_does_not_remove_a_preserved_pi_skill_through_another_app() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".claude"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");
        let shared_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        assert_eq!(
            shared_skill,
            SkillService::get_app_skills_dir(&AppType::Claude)
                .unwrap()
                .join("test-skill")
        );
        write_skill(&shared_skill, "external");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

        assert!(!source.exists(), "managed source must be removed");
        assert_eq!(
            result.preserved_pi_path,
            Some(shared_skill.to_string_lossy().to_string())
        );
        assert!(result.pi_cleanup_incomplete);
        assert!(fs::read_to_string(shared_skill.join("SKILL.md"))
            .expect("read shared external skill")
            .contains("name: external"));
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn uninstall_preserves_a_dangling_pi_link_through_an_aliased_app_root() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let shared_agent = temp.path().join("shared-agent");
        fs::create_dir_all(shared_agent.join("skills")).expect("create shared skills root");
        symlink(&shared_agent, temp.path().join(".claude")).expect("alias Claude root");
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&shared_agent);

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");
        let pi_skill = shared_agent.join("skills").join("test-skill");
        symlink(temp.path().join("missing-target"), &pi_skill).expect("dangling Pi link");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

        assert_eq!(
            result.preserved_pi_path,
            Some(pi_skill.to_string_lossy().to_string())
        );
        assert!(result.pi_cleanup_incomplete);
        assert!(
            SkillService::is_symlink(&pi_skill),
            "the aliased application cleanup must not remove the preserved link"
        );
        assert!(!source.exists());
    }

    #[test]
    fn pi_uninstall_rechecks_a_destination_created_after_preflight() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("pi-skill");
        write_skill(&source, "managed");
        assert!(
            SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
                .expect("inspect absent destination")
                .is_none()
        );

        SkillService::copy_dir_recursive(&source, &destination)
            .expect("simulate concurrent Pi sync");
        SkillService::remove_verified_pi_destination(&source, &destination, "test-skill")
            .expect("remove destination created after preflight");

        assert!(!destination.exists());
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_keeps_ssot_when_it_contains_a_preserved_pi_skill() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&source.join("pi-agent"));
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        write_skill(&pi_skill, "external");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

        assert!(result.backup_path.is_none());
        assert_eq!(
            result.preserved_pi_path,
            Some(pi_skill.to_string_lossy().to_string())
        );
        assert!(result.pi_cleanup_incomplete);
        assert!(source.exists(), "the preserved path is nested under SSOT");
        assert!(pi_skill.exists(), "the external Pi skill must remain");
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_with_a_missing_source_does_not_backup_or_delete_the_pi_directory() {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        write_skill(&pi_skill, "external");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall missing source");

        assert!(result.backup_path.is_none());
        assert_eq!(
            result.preserved_pi_path,
            Some(pi_skill.to_string_lossy().to_string())
        );
        assert!(result.pi_cleanup_incomplete);
        assert!(pi_skill.join("SKILL.md").exists());
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_treats_an_aliased_pi_root_as_the_ssot() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::Unified);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");

        let result = SkillService::uninstall(&db, &skill.id).expect("uninstall aliased skill");

        assert!(!source.exists());
        assert!(result.preserved_pi_path.is_none());
        assert!(!result.pi_cleanup_incomplete);
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_warns_when_the_pi_root_cannot_be_resolved() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::CcSwitch);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(Path::new("relative/pi-agent"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&source, "managed");

        let result =
            SkillService::uninstall(&db, &skill.id).expect("uninstall with invalid Pi root");

        assert!(!source.exists());
        assert!(result.preserved_pi_path.is_none());
        assert!(result.pi_cleanup_incomplete);
        assert!(db
            .get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none());
    }

    /// CC_SWITCH_TEST_HOME 隔离守卫（serial 测试间互斥由 #[serial] 保证，
    /// 守卫只负责在测试结束后恢复原值）。
    struct TestHomeGuard(Option<std::ffi::OsString>);
    impl TestHomeGuard {
        fn set(home: &Path) -> Self {
            let guard = Self(std::env::var_os("CC_SWITCH_TEST_HOME"));
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            guard
        }
    }
    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    struct StorageLocationGuard(SkillStorageLocation);
    impl StorageLocationGuard {
        fn set(location: SkillStorageLocation) -> Self {
            let previous = crate::settings::get_skill_storage_location();
            crate::settings::set_skill_storage_location(location)
                .expect("set test skill storage location");
            Self(previous)
        }
    }
    impl Drop for StorageLocationGuard {
        fn drop(&mut self) {
            let _ = crate::settings::set_skill_storage_location(self.0);
        }
    }

    fn poisoned_skill(id: &str, directory: &str) -> InstalledSkill {
        InstalledSkill {
            id: id.to_string(),
            name: "poisoned".to_string(),
            description: None,
            directory: directory.to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps::default(),
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        }
    }

    #[test]
    fn persist_updated_skill_metadata_uses_database_apps() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let mut installed = poisoned_skill("owner/repo:skill", "skill");
        installed.name = "old name".to_string();
        installed.apps = SkillApps::only(&AppType::Claude);
        db.save_skill(&installed).expect("seed skill");

        // 模拟下载期间用户将 Skill 从 Claude 切换到 Codex。待写入的 metadata
        // 仍携带下载开始时的旧 apps 快照。
        let authoritative_apps = SkillApps::only(&AppType::Codex);
        db.update_skill_apps(&installed.id, &authoritative_apps)
            .expect("toggle apps");

        let mut updated_metadata = installed.clone();
        updated_metadata.name = "new name".to_string();
        updated_metadata.content_hash = Some("new hash".to_string());
        updated_metadata.updated_at = 42;

        let persisted = SkillService::persist_updated_skill_metadata(&db, &updated_metadata)
            .expect("persist metadata");

        assert_eq!(persisted.name, "new name");
        assert_eq!(persisted.content_hash.as_deref(), Some("new hash"));
        assert_eq!(persisted.updated_at, 42);
        assert_eq!(persisted.apps, authoritative_apps);
        assert_eq!(
            db.get_installed_skill(&installed.id)
                .expect("query skill")
                .expect("skill remains installed")
                .apps,
            authoritative_apps
        );
    }

    #[test]
    fn persist_updated_skill_metadata_does_not_restore_uninstalled_skill() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let installed = poisoned_skill("owner/repo:skill", "skill");
        db.save_skill(&installed).expect("seed skill");

        // 模拟下载期间卸载完成，随后旧的更新任务才尝试落库。
        assert!(db.delete_skill(&installed.id).expect("uninstall skill"));

        let mut updated_metadata = installed.clone();
        updated_metadata.name = "downloaded update".to_string();
        let err = SkillService::persist_updated_skill_metadata(&db, &updated_metadata)
            .expect_err("an uninstalled skill must not be restored");

        assert!(
            err.to_string().contains("Skill no longer installed"),
            "unexpected error: {err}"
        );
        assert!(db
            .get_installed_skill(&installed.id)
            .expect("query skill")
            .is_none());
    }

    #[test]
    fn require_valid_directory_accepts_single_segment_names_only() {
        assert_eq!(
            SkillService::require_valid_directory("my-skill").expect("valid name"),
            "my-skill"
        );
        for bad in [
            "..",
            "../..",
            "../../etc",
            "a/b",
            "a\\b",
            "",
            ".hidden",
            "C:\\evil",
            "/etc",
        ] {
            assert!(
                SkillService::require_valid_directory(bad).is_err(),
                "must reject: {bad:?}"
            );
        }
    }

    #[test]
    fn local_hash_for_update_check_ignores_cached_hash_when_dir_missing() {
        // 换机恢复数据库备份的现场：content_hash 还在库里，SSOT 目录已不在。
        // 必须无视缓存返回 None，让 Skill 进入更新列表以便重建文件；
        // 若信任缓存则界面显示「无更新」，缺失状态被永久掩盖。
        let ssot = tempdir().expect("tempdir");
        assert_eq!(
            SkillService::local_hash_for_update_check(ssot.path(), "my-skill", Some("cached")),
            None
        );
    }

    #[test]
    fn local_hash_for_update_check_uses_cache_when_dir_exists() {
        let ssot = tempdir().expect("tempdir");
        fs::create_dir(ssot.path().join("my-skill")).expect("create skill dir");
        assert_eq!(
            SkillService::local_hash_for_update_check(ssot.path(), "my-skill", Some("cached")),
            Some(("cached".to_string(), false))
        );
    }

    #[test]
    fn local_hash_for_update_check_computes_and_backfills_when_cache_empty() {
        let ssot = tempdir().expect("tempdir");
        let dir = ssot.path().join("my-skill");
        fs::create_dir(&dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");

        let expected = SkillService::compute_dir_hash(&dir).expect("hash");
        assert_eq!(
            SkillService::local_hash_for_update_check(ssot.path(), "my-skill", None),
            Some((expected, true))
        );
    }

    #[test]
    fn local_hash_for_update_check_keeps_cache_for_invalid_directory() {
        // 非法 directory 无法安全拼路径，做不了存在性检查：有缓存沿用缓存
        // （维持修复前行为），无缓存返回 None。不能因非法值报「可更新」，
        // 否则用户点更新会在 update_skill 的同一校验上硬报错。
        let ssot = tempdir().expect("tempdir");
        assert_eq!(
            SkillService::local_hash_for_update_check(ssot.path(), "../evil", Some("cached")),
            Some(("cached".to_string(), false))
        );
        assert_eq!(
            SkillService::local_hash_for_update_check(ssot.path(), "../evil", None),
            None
        );
    }

    #[test]
    #[serial_test::serial]
    fn restore_from_backup_rejects_traversal_directory_in_metadata() {
        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 手工放置一个备份：meta.json 里的 directory 指向 SSOT 之外。
        // SSOT 位于 {home}/.cc-switch/skills，"../../pwned-restore" 若生效会写到 {home}/pwned-restore。
        let backup_id = "20260727_120000_evil";
        let backup_dir = SkillService::get_backup_dir()
            .expect("backup dir")
            .join(backup_id);
        write_skill(&backup_dir.join("skill"), "evil");
        let metadata = SkillBackupMetadata {
            skill: poisoned_skill("owner/repo:evil", "../../pwned-restore"),
            backup_created_at: 0,
            source_path: "x".to_string(),
        };
        fs::write(
            backup_dir.join("meta.json"),
            serde_json::to_string_pretty(&metadata).expect("serialize metadata"),
        )
        .expect("write meta.json");

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let result = SkillService::restore_from_backup(&db, backup_id, &AppType::Claude);

        assert!(
            result.is_err(),
            "restore must reject a traversal directory from meta.json"
        );
        assert!(
            !temp.path().join("pwned-restore").exists(),
            "restore must not write outside the SSOT dir"
        );
    }

    #[test]
    #[serial_test::serial]
    fn remove_from_app_rejects_traversal_directory() {
        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 受害目录与 app skills 目录都先建好，保证未修复时代码真的能删到它：
        // app_dir = {home}/.claude/skills，"../../victim-remove" 解析为 {home}/victim-remove。
        let victim = temp.path().join("victim-remove");
        fs::create_dir_all(&victim).expect("create victim dir");
        fs::create_dir_all(temp.path().join(".claude").join("skills")).expect("create app dir");

        let result = SkillService::remove_from_app("../../victim-remove", &AppType::Claude);

        assert!(result.is_err(), "remove_from_app must reject traversal");
        assert!(victim.exists(), "victim directory must not be deleted");
    }

    #[test]
    #[serial_test::serial]
    fn aliased_skill_roots_reject_sync_and_remove_without_deleting_the_source() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::Unified);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

        let source = SkillService::get_ssot_dir()
            .expect("SSOT")
            .join("test-skill");
        write_skill(&source, "managed");

        let sync = SkillService::sync_to_app_dir("test-skill", &AppType::Pi);
        assert!(sync.is_err(), "an aliased deployment root must be rejected");
        assert!(source.join("SKILL.md").exists());

        let remove = SkillService::remove_from_app("test-skill", &AppType::Pi);
        assert!(
            remove.is_err(),
            "an aliased deployment root must be rejected"
        );
        assert!(
            source.join("SKILL.md").exists(),
            "rejecting the operation must leave the SSOT untouched"
        );
    }

    #[test]
    #[serial_test::serial]
    fn migrate_storage_rejects_an_aliased_destination_before_moving_skills() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::CcSwitch);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let source = SkillService::get_ssot_dir()
            .expect("SSOT")
            .join("test-skill");
        write_skill(&source, "managed");

        let migration = SkillService::migrate_storage(&db, SkillStorageLocation::Unified);

        assert!(
            migration.is_err(),
            "migration must reject a target that aliases an app deployment root"
        );
        assert_eq!(
            crate::settings::get_skill_storage_location(),
            SkillStorageLocation::CcSwitch
        );
        assert!(
            source.join("SKILL.md").exists(),
            "validation must happen before any source is moved"
        );
    }

    #[test]
    #[serial_test::serial]
    fn migrate_storage_safely_leaves_an_existing_pi_ssot_alias() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::Unified);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let old_source = SkillService::get_ssot_dir()
            .expect("SSOT")
            .join("test-skill");
        write_skill(&old_source, "managed");

        let result = SkillService::migrate_storage(&db, SkillStorageLocation::CcSwitch)
            .expect("migrate away from alias");
        let new_source = temp
            .path()
            .join(".cc-switch")
            .join("skills")
            .join("test-skill");
        let pi_skill = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("test-skill");

        assert_eq!(result.migrated_count, 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(new_source.join("SKILL.md").exists());
        assert!(
            pi_skill.join("SKILL.md").exists(),
            "the previously native Pi skill must stay active after migration"
        );
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_rejects_traversal_directory_from_db_row() {
        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 模拟同步导入灌进来的脏数据：directory 含路径穿越（save_skill 不校验，
        // 与 import_sql_string_for_sync 的效果一致）。SSOT = {home}/.cc-switch/skills，
        // "../../victim-uninstall" 解析为 {home}/victim-uninstall。
        let victim = temp.path().join("victim-uninstall");
        fs::create_dir_all(&victim).expect("create victim dir");

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:evil", "../../victim-uninstall");
        db.save_skill(&skill).expect("seed poisoned row");

        let result = SkillService::uninstall(&db, &skill.id)
            .expect("uninstall must remove the poisoned database row");

        // 危险的文件系统操作必须被跳过……
        assert!(victim.exists(), "victim directory must not be deleted");
        // ……但记录本身必须能删掉。db.delete_skill 全项目只有 uninstall 一处调用
        // 且未暴露为命令，若这里返回 Err，脏行就永远无法从界面清除。
        assert!(
            result.pi_cleanup_incomplete,
            "skipped filesystem cleanup must be reported"
        );
        assert!(
            db.get_installed_skill(&skill.id)
                .expect("query skill")
                .is_none(),
            "poisoned row must be deleted from the database"
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn migrate_storage_retargets_a_managed_pi_symlink() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::CcSwitch);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let old_source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&old_source, "managed");
        SkillService::sync_to_app_dir("test-skill", &AppType::Pi).expect("enable Pi skill");
        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        assert!(SkillService::is_symlink(&pi_skill));

        let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
            .expect("migrate storage");
        let new_source = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("test-skill");

        assert_eq!(result.migrated_count, 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(!old_source.exists());
        assert!(new_source.exists());
        assert_eq!(
            pi_skill.canonicalize().expect("resolve Pi symlink"),
            new_source.canonicalize().expect("resolve new source")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn migrate_storage_retargets_an_equivalent_relative_pi_symlink() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::CcSwitch);
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _pi_dir =
            crate::pi_config::test_support::TestAgentDir::at(&temp.path().join("pi-agent"));

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:skill", "test-skill");
        db.save_skill(&skill).expect("save skill");
        let old_source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&old_source, "managed");

        let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
            .unwrap()
            .join("test-skill");
        fs::create_dir_all(pi_skill.parent().expect("Pi skills directory"))
            .expect("create Pi skills directory");
        std::os::unix::fs::symlink(Path::new("../../.cc-switch/skills/test-skill"), &pi_skill)
            .expect("create relative Pi symlink");

        let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
            .expect("migrate storage");
        let new_source = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("test-skill");

        assert_eq!(result.migrated_count, 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            pi_skill.canonicalize().expect("resolve Pi symlink"),
            new_source.canonicalize().expect("resolve new source")
        );
    }

    #[test]
    #[serial_test::serial]
    fn migrate_storage_skips_bad_rows_without_moving_foreign_dirs() {
        let _location = StorageLocationGuard::set(SkillStorageLocation::CcSwitch);

        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // migrate_storage 会 fs::rename / remove_dir_all，脏 directory 能把
        // SSOT 之外的任意目录搬走或删掉。
        let victim = temp.path().join("victim-migrate");
        fs::create_dir_all(&victim).expect("create victim dir");

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let skill = poisoned_skill("owner/repo:evil", "../../victim-migrate");
        db.save_skill(&skill).expect("seed poisoned row");

        // 必须迁到与当前不同的位置，否则函数在 current == target 处直接短路返回
        let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
            .expect("migration must not abort");

        assert!(victim.exists(), "foreign directory must not be moved away");
        assert_eq!(
            result.migrated_count, 0,
            "poisoned row must not count as migrated"
        );
        assert!(
            !result.errors.is_empty(),
            "the skipped row must be reported through the errors channel"
        );
    }

    #[test]
    #[serial_test::serial]
    fn uninstall_backup_source_rejects_traversal_directory() {
        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 备份源会被整目录复制进 skill-backups 并在界面列出 → 任意文件外泄面。
        let secrets = temp.path().join("secrets");
        fs::create_dir_all(&secrets).expect("create secrets dir");
        fs::write(secrets.join("id_rsa"), b"PRIVATE").expect("write secret");

        let skill = poisoned_skill("owner/repo:evil", "../../secrets");
        let result = SkillService::resolve_uninstall_backup_source_excluding(&skill, None);

        assert!(
            result.is_err(),
            "backup source must reject a traversal directory"
        );
    }

    #[test]
    #[serial_test::serial]
    fn sync_to_app_skips_bad_rows_instead_of_aborting_the_whole_app() {
        let temp = tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        let ssot_dir = SkillService::get_ssot_dir().expect("ssot dir");
        write_skill(&ssot_dir.join("good-skill"), "good");

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        // 一条脏行 + 一条正常行。脏行来自同步导入/存量数据，不得连累正常行——
        // sync_to_app 在切换供应商时触发，整体中断会让所有 skill 一起失效。
        //
        // 名字刻意让脏行排前面：查询是 `ORDER BY name ASC`，脏行必须先被处理，
        // 否则「未修复时会中断」这个前提不成立，测试就成了摆设。
        let mut bad = poisoned_skill("owner/repo:bad", "../../escape-sync");
        bad.name = "a-poisoned".to_string();
        bad.apps = SkillApps::only(&AppType::Claude);
        db.save_skill(&bad).expect("seed poisoned row");

        let mut good = poisoned_skill("owner/repo:good", "good-skill");
        good.name = "z-healthy".to_string();
        good.apps = SkillApps::only(&AppType::Claude);
        db.save_skill(&good).expect("seed good row");

        SkillService::sync_to_app(&db, &AppType::Claude).expect("sync must not abort");

        let app_dir = SkillService::get_app_skills_dir(&AppType::Claude).expect("app dir");
        assert!(
            app_dir.join("good-skill").exists(),
            "the healthy skill must still be synced despite the poisoned row"
        );
    }

    #[test]
    // serial：与 backup/s3_sync/deeplink 等同样读写进程级 CC_SWITCH_TEST_HOME 的测试互斥，
    // EnvGuard 只负责恢复不提供互斥。
    #[serial_test::serial]
    fn get_app_skills_dir_honors_test_home_override() {
        // 回归：曾直呼 dirs::home_dir() 绕过 CC_SWITCH_TEST_HOME——Unix 上碰巧跟 $HOME
        // 一致所以测试能过，Windows 上 dirs 走 Known Folder API，测试隔离整体失效
        // （tests/skill_sync.rs 扫到 runner 真实用户目录）。
        struct EnvGuard(Option<std::ffi::OsString>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                    None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
                }
            }
        }
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard(std::env::var_os("CC_SWITCH_TEST_HOME"));
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let dir =
            SkillService::get_app_skills_dir(&AppType::Claude).expect("resolve claude skills dir");
        assert!(
            dir.starts_with(temp.path()),
            "skills dir must live under the overridden test home, got {}",
            dir.display()
        );
    }

    #[test]
    fn resolve_skill_source_dir_returns_repo_root_for_root_level_skill() {
        let temp = tempdir().expect("tempdir");
        write_skill(temp.path(), "Root Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "last30days-skill-cn")
            .expect("root-level skill should resolve to the extracted repo root");

        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn resolve_skill_source_dir_returns_direct_nested_directory_when_present() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("skills").join("nested-skill");
        write_skill(&nested, "Nested Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "skills/nested-skill")
            .expect("nested skill should resolve from its relative source path");

        assert_eq!(resolved, nested);
    }

    #[test]
    fn resolve_skill_source_dir_falls_back_to_matching_install_name() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("skills").join("nested-skill");
        write_skill(&nested, "Nested Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "nested-skill")
            .expect("install name should fall back to the matching discovered skill directory");

        assert_eq!(resolved, nested);
    }

    #[test]
    fn replace_dest_with_copy_rejects_empty_source_without_touching_existing_dest() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source-skill");
        let dest = temp.path().join("app-skills").join("source-skill");
        fs::create_dir_all(&source).expect("create empty source");
        write_skill(&dest, "Existing Skill");

        let err = SkillService::replace_dest_with_copy(&source, &dest, "source-skill")
            .expect_err("empty source should not replace existing app skill");

        assert!(
            err.to_string().contains("SKILL.md"),
            "unexpected error: {err:#}"
        );
        assert!(
            dest.join("SKILL.md").is_file(),
            "existing destination skill should be preserved"
        );
    }

    #[test]
    fn resolve_skill_source_dir_rejects_same_name_wrapper_without_skill_md() {
        // 复刻 issue #4141：ast-grep/agent-skill 结构。仓库根下有同名目录 ast-grep/
        // （plugin 包，无 SKILL.md），真正的 skill 在 ast-grep/skills/ast-grep/SKILL.md。
        let temp = tempdir().expect("tempdir");
        let wrapper = temp.path().join("ast-grep");
        fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
        fs::write(
            wrapper.join(".claude-plugin").join("plugin.json"),
            "{\"name\":\"ast-grep\"}",
        )
        .expect("write plugin.json");
        let real_skill = wrapper.join("skills").join("ast-grep");
        write_skill(&real_skill, "ast-grep");

        // directory 只给了 skill 名 "ast-grep"（skills.sh API 的语义），不能命中空壳 wrapper。
        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep")
            .expect("should resolve to the inner skill dir, not the same-name wrapper");

        assert_eq!(resolved, real_skill);
        assert!(resolved.join("SKILL.md").is_file());
    }

    #[test]
    fn resolve_skill_source_dir_finds_two_level_catalog_skill() {
        // catalog layout：skills/category/foo/SKILL.md（depth 3，find_skill_dir_by_name 可达）。
        let temp = tempdir().expect("tempdir");
        let catalog_skill = temp.path().join("skills").join("category").join("foo");
        write_skill(&catalog_skill, "Foo Skill");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo")
            .expect("should resolve the two-level catalog skill by name");

        assert_eq!(resolved, catalog_skill);
    }

    #[test]
    fn resolve_skill_source_dir_returns_none_for_wrapper_without_inner_skill() {
        // 同名 wrapper 存在、无 SKILL.md，且无 inner skill / root SKILL.md 可兜底时，
        // 必须返回 None——守住 #4141 这个 bug class 的负例（不能把空壳目录当源目录）。
        let temp = tempdir().expect("tempdir");
        let wrapper = temp.path().join("ast-grep");
        fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
        fs::write(
            wrapper.join(".claude-plugin").join("plugin.json"),
            "{\"name\":\"ast-grep\"}",
        )
        .expect("write plugin.json");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep");
        assert!(
            resolved.is_none(),
            "wrapper dir without SKILL.md and no inner skill must resolve to None, got {:?}",
            resolved
        );
    }

    #[test]
    fn resolve_skill_source_dir_returns_none_when_no_skill_md_anywhere() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills").join("foo")).expect("create empty skill dir");
        fs::write(temp.path().join("README.md"), "no skills here").expect("write README");

        let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo");
        assert!(
            resolved.is_none(),
            "no SKILL.md anywhere must resolve to None"
        );
    }

    #[test]
    fn choose_doc_path_prefers_resolved_source_over_stale_readme_url() {
        // #6111 现场还原：skills.sh 安装嵌套目录 skill——directory 是 skillId
        // （末级目录名），readme_url 是仓库根 URL，真实嵌套路径只能来自
        // 解析出的源目录。若退回"readme_url 提取优先、directory 拼接兜底"，
        // 本断言即失败（旧逻辑产出 alibabacloud-cli-guidance/SKILL.md → 404）。
        let doc_path = SkillService::choose_doc_path(
            Some("skills/developertools/solutions/alibabacloud-cli-guidance/SKILL.md".to_string()),
            Some("https://github.com/aliyun/alibabacloud-aiops-skills"),
            "alibabacloud-cli-guidance",
        );
        assert_eq!(
            doc_path,
            "skills/developertools/solutions/alibabacloud-cli-guidance/SKILL.md"
        );
    }

    #[test]
    fn choose_doc_path_falls_back_to_readme_url_path_then_directory() {
        // 无解析结果（SSOT 目录已存在、未重新下载）时沿用旧 readme_url 的仓库内
        // 路径，兼容 blob/tree 两种格式并补 SKILL.md 后缀
        let doc_path = SkillService::choose_doc_path(
            None,
            Some("https://github.com/o/r/tree/main/skills/foo"),
            "foo",
        );
        assert_eq!(doc_path, "skills/foo/SKILL.md");

        // 旧 readme_url 已是完整文档路径时原样保留
        let doc_path = SkillService::choose_doc_path(
            None,
            Some("https://github.com/o/r/blob/main/skills/foo/SKILL.md"),
            "foo",
        );
        assert_eq!(doc_path, "skills/foo/SKILL.md");

        // 两者都没有时按 directory 拼接
        let doc_path = SkillService::choose_doc_path(None, None, "skills/foo");
        assert_eq!(doc_path, "skills/foo/SKILL.md");
    }

    #[test]
    fn doc_path_for_source_returns_repo_relative_skill_md_path() {
        let temp = tempdir().expect("tempdir");
        let nested = temp
            .path()
            .join("skills")
            .join("developertools")
            .join("solutions")
            .join("foo");
        fs::create_dir_all(&nested).expect("create nested dirs");

        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), &nested),
            Some("skills/developertools/solutions/foo/SKILL.md".to_string())
        );
        // 源目录即仓库根：文档路径就是根下的 SKILL.md
        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), temp.path()),
            Some("SKILL.md".to_string())
        );
        // 仓库根之外：None（调用方已做包含性校验，防御性兜底）
        assert_eq!(
            SkillService::doc_path_for_source(temp.path(), std::path::Path::new("/elsewhere")),
            None
        );
    }
}
