//! Agents 服务层
//!
//! 文件级 Agent 管理架构（与 Rules 对称）：
//! - 每个 .md 文件是一条独立的 Agent
//! - SSOT（单一事实源）：`~/.cc-switch/agents/` 存放单个 .md 文件
//! - 安装时将 .md 文件下载到 SSOT，按需同步到各应用目录
//! - 数据库存储安装记录和启用状态
//! - 仓库发现：扫描 agents/**/*.md

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::timeout;

use crate::app_config::{AgentApps, AppType, InstalledAgent, UnmanagedAgent};
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::services::skill::SyncMethod;

// ========== 数据结构 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverableAgent {
    pub key: String,
    pub name: String,
    pub description: String,
    /// Agent 文件在仓库内的相对路径（如 "agents/common/planner.md"）
    pub directory: String,
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    #[serde(rename = "repoOwner")]
    pub repo_owner: String,
    #[serde(rename = "repoName")]
    pub repo_name: String,
    #[serde(rename = "repoBranch")]
    pub repo_branch: String,
}

/// Agent 对象（兼容旧 API，内部使用 DiscoverableAgent）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub key: String,
    pub name: String,
    pub description: String,
    pub directory: String,
    #[serde(rename = "readmeUrl")]
    pub readme_url: Option<String>,
    pub installed: bool,
    #[serde(rename = "repoOwner")]
    pub repo_owner: Option<String>,
    #[serde(rename = "repoName")]
    pub repo_name: Option<String>,
    #[serde(rename = "repoBranch")]
    pub repo_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRepo {
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUninstallResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBackupEntry {
    pub backup_id: String,
    pub backup_path: String,
    pub created_at: i64,
    pub agent: InstalledAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentBackupMetadata {
    agent: InstalledAgent,
    backup_created_at: i64,
    source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentMetadata {
    #[allow(dead_code)]
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAgentSelection {
    pub directory: String,
    #[serde(default)]
    pub apps: AgentApps,
}

const AGENT_BACKUP_RETAIN_COUNT: usize = 20;

/// 不作为 Agent 识别的文件名（大小写不敏感）
const SKIP_MD_FILES: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "LICENSE.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "AGENTS.md",
];

pub fn default_agent_repos() -> Vec<AgentRepo> {
    vec![]
}

// ========== AgentService ==========

pub struct AgentService;

impl Default for AgentService {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentService {
    pub fn new() -> Self {
        Self
    }

    fn build_agent_doc_url(owner: &str, repo: &str, branch: &str, file_path: &str) -> String {
        format!("https://github.com/{owner}/{repo}/blob/{branch}/{file_path}")
    }

    // ========== 路径管理 ==========

    /// SSOT 目录（~/.cc-switch/agents/），存放单个 .md 文件
    pub fn get_ssot_dir() -> Result<PathBuf> {
        let dir = get_app_config_dir().join("agents");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn get_backup_dir() -> Result<PathBuf> {
        let dir = get_app_config_dir().join("agent-backups");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 应用的 agents 目录（如 ~/.claude/agents/）
    pub fn get_app_agents_dir(app: &AppType) -> Result<PathBuf> {
        match app {
            AppType::Claude => {
                if let Some(custom) = crate::settings::get_claude_override_dir() {
                    return Ok(custom.join("agents"));
                }
            }
            AppType::Codex => {
                if let Some(custom) = crate::settings::get_codex_override_dir() {
                    return Ok(custom.join("agents"));
                }
            }
            AppType::Gemini => {
                if let Some(custom) = crate::settings::get_gemini_override_dir() {
                    return Ok(custom.join("agents"));
                }
            }
            AppType::OpenCode => {
                if let Some(custom) = crate::settings::get_opencode_override_dir() {
                    return Ok(custom.join("agents"));
                }
            }
            AppType::OpenClaw => {
                if let Some(custom) = crate::settings::get_openclaw_override_dir() {
                    return Ok(custom.join("agents"));
                }
            }
        }

        let home = dirs::home_dir().context("无法获取用户主目录")?;
        Ok(match app {
            AppType::Claude => home.join(".claude").join("agents"),
            AppType::Codex => home.join(".codex").join("agents"),
            AppType::Gemini => home.join(".gemini").join("agents"),
            AppType::OpenCode => home.join(".config").join("opencode").join("agents"),
            AppType::OpenClaw => home.join(".openclaw").join("agents"),
        })
    }

    // ========== 统一管理方法 ==========

    pub fn get_all_installed(db: &Arc<Database>) -> Result<Vec<InstalledAgent>> {
        let agents = db.get_all_installed_agents()?;
        Ok(agents.into_values().collect())
    }

    /// 安装 Agent：将单个 .md 文件从仓库下载到 SSOT 并同步到应用目录
    pub async fn install(
        &self,
        db: &Arc<Database>,
        agent: &DiscoverableAgent,
        current_app: &AppType,
    ) -> Result<InstalledAgent> {
        let ssot_dir = Self::get_ssot_dir()?;

        let install_name = Path::new(&agent.directory)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty() && n.ends_with(".md"))
            .ok_or_else(|| anyhow!("Invalid agent file path: {}", agent.directory))?;

        let existing_agents = db.get_all_installed_agents()?;
        for existing in existing_agents.values() {
            if existing.id == agent.key {
                let mut updated = existing.clone();
                updated.apps.set_enabled_for(current_app, true);
                db.save_agent(&updated)?;
                Self::sync_file_to_app(&updated.directory, current_app)?;
                return Ok(updated);
            }

            if existing.directory.eq_ignore_ascii_case(&install_name) {
                return Err(anyhow!(
                    "Agent file '{}' is already installed by '{}' and conflicts with '{}'",
                    install_name,
                    existing.id,
                    agent.key
                ));
            }
        }

        let dest = ssot_dir.join(&install_name);
        let mut repo_branch = agent.repo_branch.clone();

        if !dest.exists() {
            let skill_repo = crate::services::skill::SkillRepo {
                owner: agent.repo_owner.clone(),
                name: agent.repo_name.clone(),
                branch: agent.repo_branch.clone(),
                enabled: true,
            };

            let skill_svc = crate::services::skill::SkillService::new();
            let (temp_dir, used_branch) = timeout(
                std::time::Duration::from_secs(60),
                skill_svc.download_repo_pub(&skill_repo),
            )
            .await
            .map_err(|_| anyhow!("Download timeout"))??;
            repo_branch = used_branch;

            let source = temp_dir.join(&agent.directory);
            if !source.exists() || !source.is_file() {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(anyhow!("Agent file not found: {}", source.display()));
            }

            fs::copy(&source, &dest)?;
            let _ = fs::remove_dir_all(&temp_dir);
        }

        let readme_url = Some(Self::build_agent_doc_url(
            &agent.repo_owner,
            &agent.repo_name,
            &repo_branch,
            &agent.directory,
        ));

        let description = if agent.description.is_empty() {
            Self::extract_md_description(&dest)
        } else {
            Some(agent.description.clone())
        };

        let installed_agent = InstalledAgent {
            id: agent.key.clone(),
            name: agent.name.clone(),
            description,
            directory: install_name.clone(),
            repo_owner: Some(agent.repo_owner.clone()),
            repo_name: Some(agent.repo_name.clone()),
            repo_branch: Some(repo_branch),
            readme_url,
            apps: AgentApps::only(current_app),
            installed_at: Utc::now().timestamp(),
        };

        db.save_agent(&installed_agent)?;
        Self::sync_file_to_app(&install_name, current_app)?;

        log::info!("Agent {} 安装成功，已启用 {:?}", installed_agent.name, current_app);
        Ok(installed_agent)
    }

    pub fn uninstall(db: &Arc<Database>, id: &str) -> Result<AgentUninstallResult> {
        let agent = db
            .get_installed_agent(id)?
            .ok_or_else(|| anyhow!("Agent not found: {id}"))?;

        let backup_path =
            Self::create_uninstall_backup(&agent)?.map(|p| p.to_string_lossy().to_string());

        for app in AppType::all() {
            let _ = Self::remove_file_from_app(&agent.directory, &app);
        }

        let ssot_dir = Self::get_ssot_dir()?;
        let agent_path = ssot_dir.join(&agent.directory);
        if agent_path.exists() {
            fs::remove_file(&agent_path)?;
        }

        db.delete_agent(id)?;
        log::info!("Agent {} 卸载成功", agent.name);
        Ok(AgentUninstallResult { backup_path })
    }

    pub fn list_backups() -> Result<Vec<AgentBackupEntry>> {
        let backup_dir = Self::get_backup_dir()?;
        let mut entries = Vec::new();

        for entry in fs::read_dir(&backup_dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match Self::read_backup_metadata(&path) {
                Ok(metadata) => entries.push(AgentBackupEntry {
                    backup_id: entry.file_name().to_string_lossy().to_string(),
                    backup_path: path.to_string_lossy().to_string(),
                    created_at: metadata.backup_created_at,
                    agent: metadata.agent,
                }),
                Err(err) => {
                    log::warn!("解析 Agent 备份失败 {}: {err:#}", path.display());
                }
            }
        }

        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    pub fn delete_backup(backup_id: &str) -> Result<()> {
        let backup_path = Self::backup_path_for_id(backup_id)?;
        fs::remove_dir_all(&backup_path)
            .with_context(|| format!("failed to delete {}", backup_path.display()))?;
        Ok(())
    }

    pub fn restore_from_backup(
        db: &Arc<Database>,
        backup_id: &str,
        current_app: &AppType,
    ) -> Result<InstalledAgent> {
        let backup_path = Self::backup_path_for_id(backup_id)?;
        let metadata = Self::read_backup_metadata(&backup_path)?;
        let backup_file = backup_path.join(&metadata.agent.directory);
        if !backup_file.is_file() {
            return Err(anyhow!("Agent backup file not found: {}", backup_file.display()));
        }

        let ssot_dir = Self::get_ssot_dir()?;
        let restore_path = ssot_dir.join(&metadata.agent.directory);
        if restore_path.exists() {
            return Err(anyhow!("Restore target already exists: {}", restore_path.display()));
        }

        let mut restored = metadata.agent;
        restored.installed_at = Utc::now().timestamp();
        restored.apps = AgentApps::only(current_app);

        fs::copy(&backup_file, &restore_path)?;

        if let Err(err) = db.save_agent(&restored) {
            let _ = fs::remove_file(&restore_path);
            return Err(err.into());
        }

        if let Err(err) = Self::sync_file_to_app(&restored.directory, current_app) {
            let _ = db.delete_agent(&restored.id);
            let _ = fs::remove_file(&restore_path);
            return Err(err);
        }

        Ok(restored)
    }

    pub fn toggle_app(db: &Arc<Database>, id: &str, app: &AppType, enabled: bool) -> Result<()> {
        let mut agent = db
            .get_installed_agent(id)?
            .ok_or_else(|| anyhow!("Agent not found: {id}"))?;

        agent.apps.set_enabled_for(app, enabled);

        if enabled {
            Self::sync_file_to_app(&agent.directory, app)?;
        } else {
            Self::remove_file_from_app(&agent.directory, app)?;
        }

        db.update_agent_apps(id, &agent.apps)?;
        Ok(())
    }

    /// 扫描未管理的 Agent（应用 agents 目录下的 .md 文件）
    pub fn scan_unmanaged(db: &Arc<Database>) -> Result<Vec<UnmanagedAgent>> {
        let managed = db.get_all_installed_agents()?;
        let managed_files: HashSet<String> = managed.values().map(|a| a.directory.clone()).collect();

        let mut scan_sources: Vec<(PathBuf, String)> = Vec::new();
        for app in AppType::all() {
            if let Ok(d) = Self::get_app_agents_dir(&app) {
                scan_sources.push((d, app.as_str().to_string()));
            }
        }
        if let Ok(ssot_dir) = Self::get_ssot_dir() {
            scan_sources.push((ssot_dir, "cc-switch".to_string()));
        }

        let mut unmanaged: HashMap<String, UnmanagedAgent> = HashMap::new();

        for (scan_dir, label) in &scan_sources {
            let entries = match fs::read_dir(scan_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().to_string();
                if !file_name.ends_with(".md") || file_name.starts_with('.') {
                    continue;
                }
                if managed_files.contains(&file_name) {
                    continue;
                }
                if Self::is_skip_md(&file_name) {
                    continue;
                }

                let description = Self::extract_md_description(&path);
                let name = file_name.trim_end_matches(".md").to_string();

                unmanaged
                    .entry(file_name.clone())
                    .and_modify(|a| a.found_in.push(label.clone()))
                    .or_insert(UnmanagedAgent {
                        directory: file_name,
                        name,
                        description,
                        found_in: vec![label.clone()],
                        path: path.display().to_string(),
                    });
            }
        }

        Ok(unmanaged.into_values().collect())
    }

    pub fn import_from_apps(
        db: &Arc<Database>,
        imports: Vec<ImportAgentSelection>,
    ) -> Result<Vec<InstalledAgent>> {
        let ssot_dir = Self::get_ssot_dir()?;
        let mut imported = Vec::new();

        let mut search_sources: Vec<PathBuf> = Vec::new();
        for app in AppType::all() {
            if let Ok(d) = Self::get_app_agents_dir(&app) {
                search_sources.push(d);
            }
        }
        search_sources.push(ssot_dir.clone());

        for selection in imports {
            let file_name = selection.directory;
            let mut source_path: Option<PathBuf> = None;

            for base in &search_sources {
                let agent_path = base.join(&file_name);
                if agent_path.is_file() && source_path.is_none() {
                    source_path = Some(agent_path);
                }
            }

            let source = match source_path {
                Some(p) => p,
                None => continue,
            };

            let dest = ssot_dir.join(&file_name);
            if !dest.exists() {
                fs::copy(&source, &dest)?;
            }

            let name = file_name.trim_end_matches(".md").to_string();
            let description = Self::extract_md_description(&dest);

            let agent = InstalledAgent {
                id: format!("local:{file_name}"),
                name,
                description,
                directory: file_name,
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: selection.apps,
                installed_at: Utc::now().timestamp(),
            };

            db.save_agent(&agent)?;
            for app in AppType::all() {
                if agent.apps.is_enabled_for(&app) {
                    Self::sync_file_to_app(&agent.directory, &app)?;
                }
            }
            imported.push(agent);
        }

        Ok(imported)
    }

    // ========== 发现功能 ==========

    pub async fn discover_available(
        &self,
        repos: Vec<AgentRepo>,
    ) -> Result<Vec<DiscoverableAgent>> {
        let mut agents = Vec::new();
        let enabled_repos: Vec<AgentRepo> = repos.into_iter().filter(|r| r.enabled).collect();

        let fetch_tasks = enabled_repos.iter().map(|repo| self.fetch_repo_agents(repo));
        let results: Vec<Result<Vec<DiscoverableAgent>>> =
            futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results.into_iter()) {
            match result {
                Ok(repo_agents) => agents.extend(repo_agents),
                Err(e) => log::warn!("获取仓库 {}/{} Agent 失败: {}", repo.owner, repo.name, e),
            }
        }

        let mut seen = HashMap::new();
        agents.retain(|a| {
            let key = a.key.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(true);
                true
            } else {
                false
            }
        });
        agents.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(agents)
    }

    /// 列出所有 Agent（兼容旧 API）
    pub async fn list_agents(
        &self,
        repos: Vec<AgentRepo>,
        db: &Arc<Database>,
    ) -> Result<Vec<Agent>> {
        let discoverable = self.discover_available(repos).await?;

        let installed = db.get_all_installed_agents()?;
        let installed_ids: HashSet<String> = installed.keys().cloned().collect();

        let mut agents: Vec<Agent> = discoverable
            .into_iter()
            .map(|d| {
                Agent {
                    installed: installed_ids.contains(&d.key),
                    key: d.key,
                    name: d.name,
                    description: d.description,
                    directory: d.directory,
                    readme_url: d.readme_url,
                    repo_owner: Some(d.repo_owner),
                    repo_name: Some(d.repo_name),
                    repo_branch: Some(d.repo_branch),
                }
            })
            .collect();

        for inst in installed.values() {
            let already_in_list = agents.iter().any(|a| a.key == inst.id);

            if !already_in_list {
                agents.push(Agent {
                    key: inst.id.clone(),
                    name: inst.name.clone(),
                    description: inst.description.clone().unwrap_or_default(),
                    directory: inst.directory.clone(),
                    readme_url: inst.readme_url.clone(),
                    installed: true,
                    repo_owner: inst.repo_owner.clone(),
                    repo_name: inst.repo_name.clone(),
                    repo_branch: inst.repo_branch.clone(),
                });
            }
        }

        agents.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(agents)
    }

    async fn fetch_repo_agents(&self, repo: &AgentRepo) -> Result<Vec<DiscoverableAgent>> {
        let skill_repo = crate::services::skill::SkillRepo {
            owner: repo.owner.clone(),
            name: repo.name.clone(),
            branch: repo.branch.clone(),
            enabled: true,
        };

        let skill_svc = crate::services::skill::SkillService::new();
        let (temp_dir, resolved_branch) = timeout(
            std::time::Duration::from_secs(60),
            skill_svc.download_repo_pub(&skill_repo),
        )
        .await
        .map_err(|_| anyhow!("Download timeout"))??;

        let mut agents = Vec::new();
        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch;

        Self::scan_repo_for_agents(&temp_dir, &temp_dir, &resolved_repo, &mut agents)?;

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(agents)
    }

    /// 递归扫描仓库中的 .md Agent 文件
    ///
    /// 扫描策略：
    /// 1. 优先扫描 agents/ 目录下所有 .md 文件，每个文件是一条独立 Agent
    /// 2. 回退：如果根目录本身包含 .md 文件（不含 agents/ 子目录），逐文件扫描
    fn scan_repo_for_agents(
        current_dir: &Path,
        base_dir: &Path,
        repo: &AgentRepo,
        agents: &mut Vec<DiscoverableAgent>,
    ) -> Result<()> {
        let agents_dir = current_dir.join("agents");
        if agents_dir.is_dir() {
            Self::scan_md_files_recursive(&agents_dir, base_dir, repo, agents)?;
            return Ok(());
        }

        Self::scan_md_files_in_dir(current_dir, base_dir, repo, agents)?;

        Ok(())
    }

    /// 递归扫描目录下所有 .md 文件，每个文件作为一条 Agent
    fn scan_md_files_recursive(
        dir: &Path,
        base_dir: &Path,
        repo: &AgentRepo,
        agents: &mut Vec<DiscoverableAgent>,
    ) -> Result<()> {
        Self::scan_md_files_in_dir(dir, base_dir, repo, agents)?;

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    Self::scan_md_files_recursive(&path, base_dir, repo, agents)?;
                }
            }
        }
        Ok(())
    }

    /// 扫描单个目录下的 .md 文件
    fn scan_md_files_in_dir(
        dir: &Path,
        base_dir: &Path,
        repo: &AgentRepo,
        agents: &mut Vec<DiscoverableAgent>,
    ) -> Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.ends_with(".md") || file_name.starts_with('.') {
                continue;
            }
            if Self::is_skip_md(&file_name) {
                continue;
            }

            let rel_path = path
                .strip_prefix(base_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let name = file_name.trim_end_matches(".md").to_string();
            let description = Self::extract_md_description(&path).unwrap_or_default();

            let readme_url = Some(Self::build_agent_doc_url(
                &repo.owner,
                &repo.name,
                &repo.branch,
                &rel_path,
            ));

            agents.push(DiscoverableAgent {
                key: format!("{}/{}:{}", repo.owner, repo.name, rel_path),
                name,
                description,
                directory: rel_path,
                readme_url,
                repo_owner: repo.owner.clone(),
                repo_name: repo.name.clone(),
                repo_branch: repo.branch.clone(),
            });
        }
        Ok(())
    }

    /// 判断文件名是否应被跳过
    fn is_skip_md(file_name: &str) -> bool {
        SKIP_MD_FILES
            .iter()
            .any(|s| s.eq_ignore_ascii_case(file_name))
    }

    /// 从 .md 文件提取描述：取第一个非空非标题行（最多 200 字符）
    fn extract_md_description(path: &Path) -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        let content = content.trim_start_matches('\u{feff}');

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            if let Ok(meta) = serde_yaml::from_str::<AgentMetadata>(parts[1].trim()) {
                if let Some(desc) = meta.description {
                    if !desc.is_empty() {
                        return Some(Self::truncate(&desc, 200));
                    }
                }
            }
        }

        let body = if parts.len() >= 3 { parts[2] } else { content };
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            return Some(Self::truncate(trimmed, 200));
        }
        None
    }

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            let mut end = max;
            while !s.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &s[..end])
        }
    }

    // ========== 文件级同步 ==========

    fn get_sync_method() -> SyncMethod {
        crate::settings::get_agent_sync_method()
    }

    #[cfg(unix)]
    fn create_file_symlink(src: &Path, dest: &Path) -> Result<()> {
        std::os::unix::fs::symlink(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    #[cfg(windows)]
    fn create_file_symlink(src: &Path, dest: &Path) -> Result<()> {
        std::os::windows::fs::symlink_file(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    fn is_symlink(path: &Path) -> bool {
        path.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    /// 同步单个 .md 文件到应用 agents 目录
    pub fn sync_file_to_app(file_name: &str, app: &AppType) -> Result<()> {
        let ssot_dir = Self::get_ssot_dir()?;
        let source = ssot_dir.join(file_name);
        if !source.exists() {
            return Err(anyhow!("Agent 不存在于 SSOT: {file_name}"));
        }

        let app_dir = Self::get_app_agents_dir(app)?;
        fs::create_dir_all(&app_dir)?;
        let dest = app_dir.join(file_name);

        if dest.exists() || Self::is_symlink(&dest) {
            fs::remove_file(&dest).ok();
        }

        match Self::get_sync_method() {
            SyncMethod::Auto => {
                match Self::create_file_symlink(&source, &dest) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        log::warn!("Symlink 创建失败，回退到复制: {err:#}");
                    }
                }
                fs::copy(&source, &dest)?;
            }
            SyncMethod::Symlink => {
                Self::create_file_symlink(&source, &dest)?;
            }
            SyncMethod::Copy => {
                fs::copy(&source, &dest)?;
            }
        }
        Ok(())
    }

    /// 从应用 agents 目录删除单个 .md 文件
    pub fn remove_file_from_app(file_name: &str, app: &AppType) -> Result<()> {
        let app_dir = Self::get_app_agents_dir(app)?;
        let agent_path = app_dir.join(file_name);
        if agent_path.exists() || Self::is_symlink(&agent_path) {
            fs::remove_file(&agent_path)?;
        }
        Ok(())
    }

    // ========== 备份 ==========

    fn backup_path_for_id(backup_id: &str) -> Result<PathBuf> {
        if backup_id.contains("..") || backup_id.contains('/') || backup_id.contains('\\') || backup_id.trim().is_empty() {
            return Err(anyhow!("Invalid backup id: {backup_id}"));
        }
        Ok(Self::get_backup_dir()?.join(backup_id))
    }

    fn read_backup_metadata(backup_path: &Path) -> Result<AgentBackupMetadata> {
        let metadata_path = backup_path.join("meta.json");
        let content = fs::read_to_string(&metadata_path)
            .with_context(|| format!("failed to read {}", metadata_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", metadata_path.display()))
    }

    fn create_uninstall_backup(agent: &InstalledAgent) -> Result<Option<PathBuf>> {
        let ssot_path = Self::get_ssot_dir()?.join(&agent.directory);
        if !ssot_path.is_file() {
            return Ok(None);
        }

        let backup_root = Self::get_backup_dir()?;
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let slug: String = agent.directory.chars().map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        }).collect();
        let slug = slug.trim_matches('-');
        let slug = if slug.is_empty() { "agent" } else { slug };
        let mut backup_path = backup_root.join(format!("{timestamp}_{slug}"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_path = backup_root.join(format!("{timestamp}_{slug}_{counter}"));
            counter += 1;
        }

        fs::create_dir_all(&backup_path)?;
        fs::copy(&ssot_path, backup_path.join(&agent.directory))?;

        let metadata = AgentBackupMetadata {
            agent: agent.clone(),
            backup_created_at: Utc::now().timestamp(),
            source_path: ssot_path.to_string_lossy().to_string(),
        };
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(backup_path.join("meta.json"), metadata_json)?;

        let mut entries: Vec<_> = fs::read_dir(&backup_root)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_dir().then(|| (e.path(), m.modified().ok()))
            })
            .collect();
        if entries.len() > AGENT_BACKUP_RETAIN_COUNT {
            entries.sort_by_key(|(_, m)| *m);
            let remove = entries.len().saturating_sub(AGENT_BACKUP_RETAIN_COUNT);
            for (p, _) in entries.into_iter().take(remove) {
                let _ = fs::remove_dir_all(&p);
            }
        }

        Ok(Some(backup_path))
    }

    // ========== ZIP 安装 ==========

    pub fn install_from_zip(
        db: &Arc<Database>,
        zip_path: &Path,
        current_app: &AppType,
    ) -> Result<Vec<InstalledAgent>> {
        let temp_dir = crate::services::skill::SkillService::extract_local_zip_pub(zip_path)?;

        let md_files = Self::collect_md_files_recursive(&temp_dir);
        if md_files.is_empty() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(anyhow!("No .md agent files found in ZIP"));
        }

        let ssot_dir = Self::get_ssot_dir()?;
        let mut installed = Vec::new();
        let existing = db.get_all_installed_agents()?;
        let mut zip_file_names = HashSet::new();

        for md_path in &md_files {
            let file_name = md_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if file_name.is_empty() || Self::is_skip_md(&file_name) {
                continue;
            }

            let normalized = file_name.to_lowercase();
            if !zip_file_names.insert(normalized) {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(anyhow!(
                    "ZIP contains multiple agent files named '{}'",
                    file_name
                ));
            }
        }

        for md_path in md_files {
            let file_name = md_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if file_name.is_empty() || Self::is_skip_md(&file_name) {
                continue;
            }
            if let Some(existing_agent) = existing
                .values()
                .find(|a| a.directory.eq_ignore_ascii_case(&file_name))
            {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(anyhow!(
                    "Agent file '{}' is already installed by '{}'",
                    file_name,
                    existing_agent.id
                ));
            }

            let dest = ssot_dir.join(&file_name);
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            fs::copy(&md_path, &dest)?;

            let name = file_name.trim_end_matches(".md").to_string();
            let description = Self::extract_md_description(&dest);

            let agent = InstalledAgent {
                id: format!("local:{file_name}"),
                name,
                description,
                directory: file_name.clone(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: AgentApps::only(current_app),
                installed_at: Utc::now().timestamp(),
            };

            db.save_agent(&agent)?;
            Self::sync_file_to_app(&file_name, current_app)?;
            installed.push(agent);
        }

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(installed)
    }

    /// 递归收集目录下所有 .md 文件
    fn collect_md_files_recursive(dir: &Path) -> Vec<PathBuf> {
        let mut results = Vec::new();
        Self::collect_md_files_inner(dir, &mut results);
        results
    }

    fn collect_md_files_inner(dir: &Path, results: &mut Vec<PathBuf>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !name.starts_with('.') && !Self::is_skip_md(&name) {
                    results.push(path);
                }
            } else if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    Self::collect_md_files_inner(&path, results);
                }
            }
        }
    }
}
