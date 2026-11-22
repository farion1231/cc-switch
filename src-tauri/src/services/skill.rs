//! Skills 服务层
//!
//! v3.10.0+ 统一管理架构：
//! - SSOT（单一事实源）：`~/.cc-switch/skills/`
//! - 安装时下载到 SSOT，按需同步到各应用目录
//! - 数据库存储安装记录和启用状态

use crate::sync_utils::MutexExt;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

use crate::app_config::{AppType, InstalledSkill, SkillApps, UnmanagedSkill};
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::error::format_skill_error;
use crate::services::skill_repo_tokens::{RepoTokenList, RepoTokenStore};

mod backup;
mod discovery;
mod download;
mod repo;
mod skills_sh;
mod sync;
mod update;
mod zip_install;

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
    /// 仓库里 SKILL.md 的网页地址
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    /// 仓库所在主机：`github.com`，或 GitLab / Gitea 实例的主机名（可带端口）
    #[serde(rename = "repoHost", default = "default_repo_host")]
    pub repo_host: String,
    /// 仓库所有者（GitLab 是分组路径，可含 `/`）
    #[serde(rename = "repoOwner")]
    pub repo_owner: String,
    /// 仓库名称
    #[serde(rename = "repoName")]
    pub repo_name: String,
    /// 分支名称
    #[serde(rename = "repoBranch")]
    pub repo_branch: String,
}

/// 仓库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRepo {
    /// 仓库所在主机：`github.com` 走 GitHub，其余是 GitLab 或 Gitea / Forgejo 实例
    /// （可带端口），具体是哪种下载前探测一次，见 [`RepoPlatform`]。
    ///
    /// 只是仓库的一个属性，不参与身份：`skill_repos` 的主键仍是 (owner, name)，
    /// 所以同一个 owner/name 不能同时存在于两个主机上，后加的会替换先加的。
    #[serde(default = "default_repo_host")]
    pub host: String,
    /// GitHub 用户/组织名；GitLab 是分组路径，可含 `/`（`group/subgroup`）
    pub owner: String,
    /// 仓库名称
    pub name: String,
    /// 分支 (默认 "main")
    pub branch: String,
    /// 是否启用
    pub enabled: bool,
}

/// GitHub 的主机名，也是 `host` 缺省时的取值
pub const GITHUB_HOST: &str = "github.com";

pub(crate) fn default_repo_host() -> String {
    GITHUB_HOST.to_string()
}

impl SkillRepo {
    pub fn is_github(&self) -> bool {
        self.host == GITHUB_HOST
    }
}

/// 仓库主机跑的是哪种代码托管平台，决定归档下载地址和文档链接的格式。
///
/// 不入库：它是主机的属性，不是仓库的属性，下载前向主机探测一次、按主机记在内存里
/// （[`SkillService::resolve_platform`]）。Forgejo（Codeberg）与 Gitea 接口相同，归 `Gitea`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoPlatform {
    GitHub,
    GitLab,
    Gitea,
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
                    host: default_repo_host(),
                    owner: "anthropics".to_string(),
                    name: "skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    host: default_repo_host(),
                    owner: "ComposioHQ".to_string(),
                    name: "awesome-claude-skills".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    host: default_repo_host(),
                    owner: "cexll".to_string(),
                    name: "myclaude".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                },
                SkillRepo {
                    host: default_repo_host(),
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

/// 某个仓库没有读到（发现、检查更新时逐仓库报告，不再静默吞掉）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRepoFailure {
    pub owner: String,
    pub name: String,
    pub branch: String,
    /// 原始错误；下载类错误是 `format_skill_error` 的结构化 JSON
    pub error: String,
}

/// 发现结果：读到的 Skill + 没读到的仓库
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDiscoveryResult {
    pub skills: Vec<DiscoverableSkill>,
    pub failures: Vec<SkillRepoFailure>,
}

/// 检查更新结果：可更新的 Skill + 没读到的仓库
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateCheckResult {
    pub updates: Vec<SkillUpdateInfo>,
    pub failures: Vec<SkillRepoFailure>,
}

/// ZIP 里因为目录名已被占用而跳过的 Skill（#3749）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipSkippedSkill {
    /// ZIP 里这个 Skill 要用的目录名
    pub directory: String,
    /// 占用这个目录名的已安装 Skill
    pub existing_id: String,
    pub existing_name: String,
}

/// 从 ZIP 安装的结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipInstallResult {
    pub installed: Vec<InstalledSkill>,
    pub skipped: Vec<ZipSkippedSkill>,
}

/// 重新同步时某个 Skill 没同步成功
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncFailure {
    pub directory: String,
    pub error: String,
}

/// 「立即重新同步」里单个应用的结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAppSyncOutcome {
    pub app: String,
    pub ok: bool,
    /// 整个应用没同步（如读不了应用目录）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 应用同步了，但其中这些 Skill 失败
    pub failed_skills: Vec<SkillSyncFailure>,
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

/// skills.sh API 原始响应（只取用到的字段，其余由 serde 忽略）
#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiResponse {
    pub query: String,
    pub skills: Vec<SkillsShApiSkill>,
    pub count: usize,
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
///
/// 条目数只是解压前的快速失败：真正兜底磁盘与 inode 消耗的是字节预算——每个
/// 文件、目录都至少按一个磁盘块计费。上限曾是 10_000，但下载的是整个仓库归档
/// 而非单个技能目录，真实技能仓库（如 hugohe3/ppt-master，13k+ 条目）会被误拒
/// （#7475）；30_000 给这类仓库留出余量，同时仍远低于字节预算能物化的条目数。
const MAX_ARCHIVE_ENTRIES: usize = 30_000;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// symlink 目标就是一条路径，几十字节就够；给到 4 KiB 是宽松上限。
/// 必须有这个上限：zip 2.4.2 的 `make_reader` 不按声明的 uncompressed_size
/// 截断读取，所以一个打了 symlink 标志、deflate 流却能膨胀到数 GB 的条目，
/// 会被 `read_to_string` 整个读进内存。
const MAX_SYMLINK_TARGET_BYTES: u64 = 4 * 1024;
/// 物化一个目录按一个目录块计费。空目录不写内容字节，但照样吃 inode 和磁盘块，
/// 不计费就等于允许无限量地造目录。
const DIRECTORY_BUDGET_COST: u64 = 4096;
/// 文件同理：空文件与不足一块的小文件照样占 inode 和一个磁盘块。只按内容字节
/// 计费时，一个全是空文件的归档能让预算读数一直停在 0，条目上限就成了唯一防线。
const FILE_ENTRY_BUDGET_COST: u64 = DIRECTORY_BUDGET_COST;
/// 压缩体上限。解压预算只有在 ZipArchive 建起来之后才生效，所以下载这一步需要
/// 自己的上限。下载的是整个仓库归档，`docs/` 里的图片、GIF 常常比技能本身大
/// 一个数量级（#8421：scientific-agent-skills 压缩包 235 MiB，技能只有 20 MiB），
/// 曾经的 128 MiB 会把这类仓库挡在门外。响应体流式落到临时文件而不进内存，
/// 上限对齐解压预算：压缩体超过解压后能容纳的总量，解压那一步也必然失败。
const MAX_ARCHIVE_DOWNLOAD_BYTES: u64 = MAX_ARCHIVE_TOTAL_BYTES;

/// 技能归档下载请求的 User-Agent。
const SKILL_DOWNLOAD_USER_AGENT: &str = concat!("CC-Switch/", env!("CARGO_PKG_VERSION"));

/// 下载停滞判定：连接、等响应头、相邻两块数据之间，任一处等这么久都没动静即放弃。
/// 下载的是整个仓库归档，大仓库（emqx/MQTTX 这类整套客户端源码）在慢的跨境链路上
/// 要好几分钟，按总时长掐断会把一直在传的下载判成超时；按停滞判定，只要数据还在来
/// 就让它下完，链路真断了照样很快报错。
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// 一次仓库下载（平台探测 + 分支回落 + 下载 + 解压）的总上限，只兜底不该无限挂着的
/// 情况，日常靠上面的停滞判定。与共享 HTTP 客户端单个请求的 600 秒上限对齐。
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(600);

/// 平台探测的超时：它排在下载前面，占的是调用方那份下载总时长
const PLATFORM_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 平台探测最多读这么多响应体。Gitea 的回答只有几十个字节
const PLATFORM_PROBE_MAX_BODY_BYTES: usize = 4 * 1024;

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

/// 本地 Skill 元数据刷新时扫描到的候选变更。
///
/// 扫描（目录哈希）在全局写锁外完成，写回前用这里记录的安装代次、目录和
/// 旧哈希重新核对，避免覆盖扫描期间发生的并发更新。
struct LocalSkillMetadataCandidate {
    id: String,
    directory: String,
    installed_at: i64,
    previous_hash: Option<String>,
    name: String,
    description: Option<String>,
    new_hash: String,
}

pub struct SkillService {
    #[cfg(test)]
    repo_fixture: Option<PathBuf>,
    /// 发现结果按仓库缓存（键是 owner/name/branch），只存读成功的仓库。
    /// 发现要把每个仓库的归档整包下下来，添加、启用仓库或导入后的重拉不该把
    /// 没变的仓库再下一遍；手动刷新会绕过它（`force_refresh`）。
    discovery_cache: Mutex<HashMap<RepoCacheKey, (Instant, Vec<DiscoverableSkill>)>>,
    /// 主机 → 平台的探测结果，进程内有效。只记探明的，见 `resolve_platform`。
    platform_cache: Mutex<HashMap<String, RepoPlatform>>,
    /// 访问令牌的代次，任何主机的令牌增删都加一。下载开始时记下它，探测结果回写缓存前
    /// 比对：令牌在探测途中变了，这次的结论是按旧身份得出的，不能记下来。
    credential_epoch: AtomicU64,
    /// 按主机存的访问令牌（#9666），读私有仓库用。读钥匙串会起子进程，经 spawn_blocking 调。
    repo_tokens: Arc<RepoTokenStore>,
}

/// host / owner / name / branch（branch 是配置值，不是解析后的分支）
type RepoCacheKey = (String, String, String, String);

/// 发现缓存的有效期：GitHub 归档本身也有几分钟的 CDN 缓存，再短意义不大
const DISCOVERY_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

impl Default for SkillService {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillService {
    pub fn new() -> Self {
        Self {
            #[cfg(test)]
            repo_fixture: None,
            discovery_cache: Mutex::new(HashMap::new()),
            platform_cache: Mutex::new(HashMap::new()),
            credential_epoch: AtomicU64::new(0),
            repo_tokens: Arc::new(RepoTokenStore::system()),
        }
    }

    /// 构建 Skill 文档 URL（指向仓库中的 SKILL.md 文件）
    ///
    /// 坐标不合法时返回 None：这个值会存进 `readme_url`，前端「查看文档」用
    /// `openExternal` 直接打开，恶意 branch 能把它指到该主机上的任意路径。
    fn build_skill_doc_url(
        platform: RepoPlatform,
        host: &str,
        owner: &str,
        repo: &str,
        branch: &str,
        doc_path: &str,
    ) -> Option<String> {
        if Self::validate_repo_coords(host, owner, repo, branch).is_err() {
            log::warn!("跳过非法仓库坐标的文档链接: {host}/{owner}/{repo}@{branch}");
            return None;
        }
        // 主机才是准绳：`github.com` 不管传进来什么平台都按 GitHub 拼
        Some(if host == GITHUB_HOST {
            format!("https://github.com/{owner}/{repo}/blob/{branch}/{doc_path}")
        } else if platform == RepoPlatform::Gitea {
            format!("https://{host}/{owner}/{repo}/src/branch/{branch}/{doc_path}")
        } else {
            format!("https://{host}/{owner}/{repo}/-/blob/{branch}/{doc_path}")
        })
    }

    /// 从旧 readme_url 中提取仓库内文档路径，兼容 GitHub 的 `blob` / `tree`、
    /// GitLab 的 `-/blob`、Gitea 的 `src/branch`
    fn extract_doc_path_from_url(url: &str) -> Option<String> {
        let (_, path) = Self::doc_url_ref_and_path(url)?.split_once('/')?;
        if path.is_empty() {
            return None;
        }
        Some(path.to_string())
    }

    /// 文档链接里平台标记之后的部分：`<分支>/<仓库内路径>`。
    fn doc_url_ref_and_path(url: &str) -> Option<&str> {
        // GitLab 的地址是 `…/-/blob/…`，先认它：分组可以嵌套、分组名本身可以叫
        // blob / tree，只有 `/-/` 能说明项目路径到哪里结束。
        for marker in ["/-/blob/", "/-/tree/"] {
            if let Some((_, tail)) = url.split_once(marker) {
                return Some(tail);
            }
        }
        // GitHub 与 Gitea 的仓库固定是 `主机/owner/repo` 三段，标记紧跟其后。按位置认，
        // 不在整条地址里搜：仓库可以叫 `tree`，仓库里也可以有 `blob/`、`src/branch/` 目录。
        let anchored = url.split_once("://").and_then(|(_, rest)| {
            let after_repo = rest.splitn(4, '/').nth(3)?;
            ["src/branch/", "blob/", "tree/"]
                .into_iter()
                .find_map(|marker| after_repo.strip_prefix(marker))
        });
        // 都对不上的旧地址（比如不带 `/-/` 的老式 GitLab 链接）沿用原来的搜法
        anchored.or_else(|| {
            ["/blob/", "/tree/"]
                .into_iter()
                .find_map(|marker| url.split_once(marker).map(|(_, tail)| tail))
        })
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

    /// 已安装 Skill 在 SSOT 目录下的主副本路径（目录名先过路径穿越校验）
    pub fn get_installed_skill_dir(directory: &str) -> Result<PathBuf> {
        let directory = Self::require_valid_directory(directory)?;
        Ok(Self::get_ssot_dir()?.join(directory))
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
            AppType::Claude
            | AppType::Codex
            | AppType::Gemini
            | AppType::GrokBuild
            | AppType::OpenCode
            | AppType::OpenClaw
            | AppType::Hermes => {
                if let Some(custom) = crate::settings::get_override_dir(app) {
                    return Ok(custom.join("skills"));
                }
            }
            AppType::ClaudeDesktop
            | AppType::Mcode
            | AppType::Kimi
            | AppType::Dsh
            | AppType::Copilot
            | AppType::WorkBuddy
            | AppType::Codebuddy => {}
            AppType::Pi => {
                return Ok(crate::pi_config::get_pi_agent_dir()?.join("skills"));
            }
            AppType::Omp => {
                return Ok(crate::omp_config::get_omp_agent_dir()?.join("skills"));
            }
            AppType::Agy => {
                return Ok(crate::config::get_home_dir()
                    .join(".gemini")
                    .join("config")
                    .join("skills"));
            }
        }

        // 默认路径：回退到用户主目录下的标准位置。
        // 必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）：Windows 上 dirs::home_dir()
        // 走 Known Folder API，测试无法隔离真实用户目录。
        let home = crate::config::get_home_dir();

        Ok(match app {
            AppType::Mcode => crate::mcode_config::data_dir().join("skills"),
            AppType::Dsh => crate::dsh_config::data_dir().join("skills"),
            AppType::Kimi => crate::kimi_config::data_dir().join("skills"),
            AppType::Copilot => crate::copilot_config::skills_dir(),
            AppType::WorkBuddy => crate::workbuddy_config::data_dir().join("skills"),
            AppType::Codebuddy => crate::codebuddy_config::data_dir().join("skills"),
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::ClaudeDesktop => home.join(".claude-desktop").join("skills"),
            AppType::Codex => home.join(".codex").join("skills"),
            AppType::Gemini => home.join(".gemini").join("skills"),
            AppType::GrokBuild => home.join(".grok").join("skills"),
            AppType::OpenCode => home.join(".config").join("opencode").join("skills"),
            AppType::OpenClaw => home.join(".openclaw").join("skills"),
            AppType::Hermes => crate::hermes_config::get_hermes_dir().join("skills"),
            AppType::Pi => crate::pi_config::get_pi_agent_dir()?.join("skills"),
            AppType::Omp => crate::omp_config::get_omp_agent_dir()?.join("skills"),
            AppType::Agy => home.join(".gemini").join("config").join("skills"),
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
            skill.apps.omp = Self::skill_exists_in_app(&skill.directory, &AppType::Omp);
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
                && existing.repo_name.as_deref() == Some(&skill.repo_name)
                && existing.repo_host.as_deref().unwrap_or(GITHUB_HOST) == skill.repo_host;
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
                host: skill.repo_host.clone(),
                owner: skill.repo_owner.clone(),
                name: skill.repo_name.clone(),
                branch: skill.repo_branch.clone(),
                enabled: true,
            };

            // 下载仓库
            let (temp_guard, used_branch) = self.download_repo_bounded(&repo).await?;
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

        // 目标目录已存在时上面没下载，这个主机可能还没探测过（重启后界面先显示
        // 上次的发现结果），所以平台要参考发现时给出的链接。
        let readme_url = Self::build_skill_doc_url(
            self.platform_for_doc_url(skill),
            &skill.repo_host,
            &skill.repo_owner,
            &skill.repo_name,
            &repo_branch,
            &doc_path,
        );

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
            repo_host: Some(skill.repo_host.clone()),
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
                    let mut managed_destinations = Vec::new();
                    for app in [
                        AppType::Mcode,
                        AppType::Agy,
                        AppType::Kimi,
                        AppType::Dsh,
                        AppType::Copilot,
                        AppType::WorkBuddy,
                        AppType::Codebuddy,
                    ] {
                        if skill.apps.is_enabled_for(&app) {
                            let destination = Self::get_distinct_app_skills_dir(&ssot_dir, &app)?
                                .join(&directory);
                            Self::inspect_pi_skill_destination(&source, &destination, &directory)
                                .with_context(|| {
                                format!(
                                    "{} Skill could not be verified; uninstall cancelled",
                                    app.as_str()
                                )
                            })?;
                            managed_destinations.push(destination);
                        }
                    }
                    let mut preserved_paths = Vec::new();
                    let mut pi_cleanup_incomplete = false;
                    let mut removal_paths = Vec::new();

                    for app in [AppType::Pi, AppType::Omp] {
                        match Self::get_app_skills_dir(&app) {
                            Ok(app_dir) => {
                                if Self::paths_alias(&ssot_dir, &app_dir) {
                                    // 原生目录直接使用 SSOT 时没有第二份副本；后续删除 SSOT 即完成清理。
                                    log::debug!("Skill {id} 的 {app:?} Skills 目录与 SSOT 相同");
                                    continue;
                                }
                                // 即使当前不存在也保留目标路径；备份期间可能有另一进程
                                // 同步该 Skill，删除 SSOT 前必须重新检查一次。
                                let destination = app_dir.join(&directory);
                                if (destination.exists() || Self::is_symlink(&destination))
                                    && Self::inspect_pi_skill_destination(
                                        &source,
                                        &destination,
                                        &directory,
                                    )
                                    .inspect_err(|err| {
                                        log::warn!(
                                            "Skill {id} 卸载时跳过 {app:?} 清理，保留 {}: {err}",
                                            destination.display()
                                        )
                                    })
                                    .is_err()
                                {
                                    preserved_paths.push(destination);
                                    pi_cleanup_incomplete = true;
                                } else {
                                    removal_paths.push((app, destination));
                                }
                            }
                            Err(err) => {
                                log::warn!(
                                    "Skill {id} 卸载时无法解析 {app:?} Skills 目录，跳过清理: {err}"
                                );
                                pi_cleanup_incomplete = true;
                            }
                        }
                    }

                    let backup_path =
                        Self::create_uninstall_backup_excluding(&skill, &preserved_paths)?
                            .map(|path| path.to_string_lossy().to_string());

                    for destination in managed_destinations {
                        Self::remove_verified_pi_destination(&source, &destination, &directory)
                            .context("Managed Skill could not be removed; uninstall cancelled")?;
                    }

                    // Pi、Omp 目录可能包含用户自己维护的同名 Skill。删除 SSOT 前仅移除
                    // 能验证为 CC Switch 部署的副本；其余路径保留并返回警告。
                    for (app, destination) in removal_paths {
                        if let Err(err) =
                            Self::remove_verified_pi_destination(&source, &destination, &directory)
                        {
                            log::warn!("Skill {id} 卸载时 {app:?} 清理失败，将继续卸载: {err}");
                            pi_cleanup_incomplete = true;
                            if destination.exists() || Self::is_symlink(&destination) {
                                preserved_paths.push(destination);
                            }
                        }
                    }

                    // 其他应用沿用既有的逐项容错行为。
                    for app in AppType::all() {
                        if matches!(
                            app,
                            AppType::Pi
                                | AppType::WorkBuddy
                                | AppType::Codebuddy
                                | AppType::Mcode
                                | AppType::Agy
                                | AppType::Kimi
                                | AppType::Dsh
                                | AppType::Omp
                                | AppType::Copilot
                        ) {
                            continue;
                        }
                        let _ =
                            Self::remove_from_app_preserving(&directory, &app, &preserved_paths);
                    }

                    // 从 SSOT 删除
                    let skill_path = ssot_dir.join(&directory);
                    if preserved_paths
                        .iter()
                        .any(|path| Self::paths_overlap(&skill_path, path))
                    {
                        log::warn!("Skill {id} 的 SSOT 路径与保留的原生副本重叠，跳过文件删除");
                    } else if skill_path.exists() {
                        fs::remove_dir_all(&skill_path)?;
                    }
                    (
                        backup_path,
                        preserved_paths.into_iter().next(),
                        pi_cleanup_incomplete,
                    )
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
        let mut mcode_deployments = Vec::new();
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
            for app in [
                AppType::Mcode,
                AppType::Agy,
                AppType::Kimi,
                AppType::Dsh,
                AppType::Copilot,
                AppType::WorkBuddy,
                AppType::Codebuddy,
            ] {
                if skill.apps.is_enabled_for(&app) {
                    let destination = Self::get_app_skills_dir(&app)?.join(&directory);
                    if let Some(deployment) =
                        Self::inspect_pi_skill_destination(&src, &destination, &directory)
                            .ok()
                            .flatten()
                    {
                        mcode_deployments.push((directory.clone(), destination, deployment));
                    }
                }
            }
            // Omp 没有开关列，是否部署只看原生目录里有没有 CC Switch 的副本。
            let omp_destination = Self::get_app_skills_dir(&AppType::Omp)?.join(&directory);
            if let Some(deployment) =
                Self::inspect_pi_skill_destination(&src, &omp_destination, &directory)
                    .ok()
                    .flatten()
            {
                mcode_deployments.push((directory.clone(), omp_destination, deployment));
            }
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

        for (directory, destination, deployment) in mcode_deployments {
            let source = new_dir.join(&directory);
            if let Err(err) =
                Self::refresh_pi_skill_destination(&source, &destination, &directory, &deployment)
            {
                result.errors.push(format!("{directory}: {err}"));
            }
        }

        // 4. 刷新所有应用目录的 symlink（指向新 SSOT）
        // 旧目录已经搬走：哪个应用没重链成功，它的 Skill 就指向不存在的路径，必须报出来。
        for app in AppType::all() {
            match Self::sync_to_app_unlocked(db, &app) {
                Ok(failed) => result.errors.extend(failed.into_iter().map(|failure| {
                    format!("{}/{}: {}", app.as_str(), failure.directory, failure.error)
                })),
                Err(err) => result.errors.push(format!("{}: {err}", app.as_str())),
            }
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

        if matches!(
            app,
            AppType::Agy
                | AppType::Kimi
                | AppType::Dsh
                | AppType::Copilot
                | AppType::WorkBuddy
                | AppType::Codebuddy
        ) {
            if !enabled && !skill.apps.is_enabled_for(app) {
                return Ok(());
            }
            let directory = Self::require_valid_directory(&skill.directory)?;
            let ssot = Self::get_ssot_dir()?;
            let source = ssot.join(&directory);
            let native_dir = Self::get_distinct_app_skills_dir(&ssot, app)?;
            let native = native_dir.join(&directory);
            Self::ensure_pi_skill_destination_matches(&source, &native, &directory)?;
            fs::create_dir_all(&native_dir)?;
            let staging = tempfile::Builder::new()
                .prefix(".cc-switch-toggle-")
                .tempdir_in(&native_dir)?;
            let previous = staging.path().join("old");
            let existed = native.exists() || Self::is_symlink(&native);
            if existed {
                fs::rename(&native, &previous)?;
            }
            skill.apps.set_enabled_for(app, enabled);
            let result = (|| -> Result<()> {
                if enabled {
                    Self::sync_to_app_dir(&directory, app)?;
                }
                if !db.update_skill_apps(id, &skill.apps)? {
                    return Err(anyhow!("Skill no longer installed: {id}"));
                }
                Ok(())
            })();
            if let Err(error) = result {
                if native.exists() || Self::is_symlink(&native) {
                    Self::remove_path(&native)?;
                }
                if existed {
                    fs::rename(previous, native)?;
                }
                return Err(error);
            }
            return Ok(());
        }

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
        if !matches!(app, AppType::Pi | AppType::Omp) {
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
        let mut skipped_mcode = Vec::new();
        let mut not_enabled = Vec::new();

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

        'imports: for selection in imports {
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
            apps.omp = Self::skill_exists_in_app(&dir_name, &AppType::Omp);

            // An explicitly imported MCode link must point at the managed copy
            // so subsequent toggles and updates can verify its ownership.
            for app in [
                AppType::Mcode,
                AppType::Agy,
                AppType::Kimi,
                AppType::Dsh,
                AppType::Copilot,
                AppType::WorkBuddy,
                AppType::Codebuddy,
            ] {
                if apps.is_enabled_for(&app) {
                    let projection = (|| -> Result<()> {
                        let native =
                            Self::get_distinct_app_skills_dir(&ssot_dir, &app)?.join(&dir_name);
                        if Self::is_symlink(&native) && !Self::paths_alias(&native, &dest) {
                            if Self::compute_pi_deployment_hash(&native)?
                                != Self::compute_pi_deployment_hash(&dest)?
                            {
                                return Err(anyhow!(
                                    "Skill 与托管副本内容不同，拒绝替换链接: {dir_name}"
                                ));
                            }
                            if let Some(deployment) =
                                Self::inspect_pi_skill_destination(&native, &native, &dir_name)?
                            {
                                Self::refresh_pi_skill_destination(
                                    &dest,
                                    &native,
                                    &dir_name,
                                    &deployment,
                                )?;
                            }
                        } else {
                            Self::preflight_install_destination(&dest, &dir_name, &app)?;
                        }
                        if !native.exists() {
                            Self::sync_to_app_dir(&dir_name, &app)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = projection {
                        skipped_mcode.push(format!("{dir_name}: {error}"));
                        continue 'imports;
                    }
                }
            }
            // 勾选的应用里还没有这个 Skill（它是在别处发现的）：导入时就投影过去。
            // 只存标志的话，界面显示已启用、应用却读不到，要等下次切换供应商才补上。
            // 已经在应用目录里的保持原样；投影失败就不记这个应用的启用，
            // 让库里的标志始终等于磁盘上的事实。
            for app in AppType::all() {
                if matches!(
                    app,
                    AppType::Pi
                        | AppType::Mcode
                        | AppType::Agy
                        | AppType::Kimi
                        | AppType::Dsh
                        | AppType::Codebuddy
                        | AppType::Omp
                        | AppType::Copilot
                        | AppType::WorkBuddy
                ) || !apps.is_enabled_for(&app)
                    || Self::skill_exists_in_app(&dir_name, &app)
                {
                    continue;
                }
                if let Err(error) = Self::sync_to_app_dir(&dir_name, &app) {
                    log::warn!("导入 Skill {dir_name} 时同步到 {app:?} 失败: {error:#}");
                    apps.set_enabled_for(&app, false);
                    not_enabled.push(format!("{dir_name} ({}): {error:#}", app.as_str()));
                }
            }

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
                repo_host: None,
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

        let mut problems = Vec::new();
        if !skipped_mcode.is_empty() {
            problems.push(format!(
                "skipped MiniMax Code entries: {}",
                skipped_mcode.join("; ")
            ));
        }
        if !not_enabled.is_empty() {
            problems.push(format!("could not enable: {}", not_enabled.join("; ")));
        }

        if problems.is_empty() {
            Ok(imported)
        } else {
            Err(anyhow!(
                "Imported {} Skills; {}",
                imported.len(),
                problems.join("; ")
            ))
        }
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
            let url = SkillService::build_skill_doc_url(
                RepoPlatform::GitHub,
                GITHUB_HOST,
                &info.owner,
                &info.repo,
                &url_branch,
                doc_path,
            );
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
                    host: default_repo_host(),
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
            repo_host: None,
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
mod tests;
