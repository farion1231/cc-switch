//! Skill service with SSOT file syncing and import helpers.

use anyhow::{anyhow, Context, Result as AnyResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::time::timeout;

use crate::app_config::{AppType, InstalledSkill, SkillApps, UnmanagedSkill};
use crate::config::{config_dir, get_home_dir};
use crate::database::Database;
use crate::error::{format_skill_error, AppError};
use crate::settings::SyncMethod;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverableSkill {
    pub key: String,
    pub name: String,
    pub description: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
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
pub struct SkillRepo {
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    pub installed: bool,
    #[serde(rename = "installedAt")]
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    pub skills: HashMap<String, SkillState>,
    pub repos: Vec<SkillRepo>,
}

impl Default for SkillStore {
    fn default() -> Self {
        Self {
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

#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
struct AgentsLockFile {
    skills: HashMap<String, AgentsLockSkill>,
}

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
    branch.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_branch_from_source_url(source_url: Option<&str>) -> Option<String> {
    let source_url = source_url?.trim();
    if source_url.is_empty() {
        return None;
    }

    if let Some((_, after_tree)) = source_url.split_once("/tree/") {
        return after_tree
            .split('/')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }

    if let Some((_, fragment)) = source_url.split_once('#') {
        return fragment
            .split('&')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }

    if let Some((_, query)) = source_url.split_once('?') {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if matches!(key, "branch" | "ref") {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

fn get_agents_skills_dir() -> Option<PathBuf> {
    let path = get_home_dir().join(".agents").join("skills");
    path.exists().then_some(path)
}

fn parse_agents_lock() -> HashMap<String, LockRepoInfo> {
    let path = get_home_dir().join(".agents").join(".skill-lock.json");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("读取 agents lock 文件失败 ({}): {}", path.display(), err);
            }
            return HashMap::new();
        }
    };

    let lock: AgentsLockFile = match serde_json::from_str(&content) {
        Ok(lock) => lock,
        Err(err) => {
            log::warn!("解析 agents lock 文件失败 ({}): {}", path.display(), err);
            return HashMap::new();
        }
    };

    lock.skills
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
        .collect()
}

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

    pub fn get_ssot_dir() -> Result<PathBuf, AppError> {
        let dir = config_dir().join("skills");
        fs::create_dir_all(&dir).map_err(|err| AppError::io(&dir, err))?;
        Ok(dir)
    }

    pub fn get_app_skills_dir(app: &AppType) -> Result<PathBuf, AppError> {
        let custom = match app {
            AppType::Claude => crate::settings::get_claude_override_dir(),
            AppType::Codex => crate::settings::get_codex_override_dir(),
            AppType::Gemini => crate::settings::get_gemini_override_dir(),
            AppType::OpenCode => crate::settings::get_opencode_override_dir(),
            AppType::OpenClaw => crate::settings::get_openclaw_override_dir(),
        };

        if let Some(custom) = custom {
            return Ok(custom.join("skills"));
        }

        let home = get_home_dir();
        Ok(match app {
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::Codex => home.join(".codex").join("skills"),
            AppType::Gemini => home.join(".gemini").join("skills"),
            AppType::OpenCode => home.join(".config").join("opencode").join("skills"),
            AppType::OpenClaw => home.join(".openclaw").join("skills"),
        })
    }

    pub fn get_all_installed(db: &Arc<Database>) -> Result<Vec<InstalledSkill>, AppError> {
        let skills = db.get_all_installed_skills()?;
        Ok(skills.into_values().collect())
    }

    pub fn get_skill(db: &Arc<Database>, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        db.get_installed_skill(id)
    }

    pub async fn install(
        &self,
        db: &Arc<Database>,
        skill: &DiscoverableSkill,
        current_app: &AppType,
    ) -> Result<InstalledSkill, AppError> {
        let ssot_dir = Self::get_ssot_dir()?;
        let source_rel = Self::sanitize_skill_source_path(&skill.directory).ok_or_else(|| {
            AppError::Message(format_skill_error(
                "INVALID_SKILL_DIRECTORY",
                &[("directory", &skill.directory)],
                Some("checkZipContent"),
            ))
        })?;
        let install_name = source_rel
            .file_name()
            .and_then(|name| Self::sanitize_install_name(&name.to_string_lossy()))
            .ok_or_else(|| {
                AppError::Message(format_skill_error(
                    "INVALID_SKILL_DIRECTORY",
                    &[("directory", &skill.directory)],
                    Some("checkZipContent"),
                ))
            })?;

        let existing_skills = db.get_all_installed_skills()?;
        for existing in existing_skills.values() {
            if !existing.directory.eq_ignore_ascii_case(&install_name) {
                continue;
            }

            let same_repo = existing.repo_owner.as_deref() == Some(&skill.repo_owner)
                && existing.repo_name.as_deref() == Some(&skill.repo_name);
            if same_repo {
                let mut updated = existing.clone();
                updated.apps.set_enabled_for(current_app, true);
                db.save_skill(&updated)?;
                Self::sync_to_app_dir(&updated.directory, current_app)?;
                return Ok(updated);
            }

            return Err(AppError::Message(format_skill_error(
                "SKILL_DIRECTORY_CONFLICT",
                &[
                    ("directory", &install_name),
                    (
                        "existing_repo",
                        &format!(
                            "{}/{}",
                            existing.repo_owner.as_deref().unwrap_or("unknown"),
                            existing.repo_name.as_deref().unwrap_or("unknown")
                        ),
                    ),
                    (
                        "new_repo",
                        &format!("{}/{}", skill.repo_owner, skill.repo_name),
                    ),
                ],
                Some("uninstallFirst"),
            )));
        }

        let dest = ssot_dir.join(&install_name);
        let mut repo_branch = skill.repo_branch.clone();

        if !dest.exists() {
            let repo = SkillRepo {
                owner: skill.repo_owner.clone(),
                name: skill.repo_name.clone(),
                branch: skill.repo_branch.clone(),
                enabled: true,
            };

            let (temp_dir, used_branch) = timeout(
                std::time::Duration::from_secs(60),
                self.download_repo(&repo),
            )
            .await
            .map_err(|_| {
                AppError::Message(format_skill_error(
                    "DOWNLOAD_TIMEOUT",
                    &[
                        ("owner", &repo.owner),
                        ("name", &repo.name),
                        ("timeout", "60"),
                    ],
                    Some("checkNetwork"),
                ))
            })??;
            repo_branch = used_branch;

            let source = temp_dir.join(&source_rel);
            if !source.exists() {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(AppError::Message(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &source.display().to_string())],
                    Some("checkRepoUrl"),
                )));
            }

            let canonical_temp = temp_dir.canonicalize().unwrap_or_else(|_| temp_dir.clone());
            let canonical_source = source.canonicalize().map_err(|_| {
                AppError::Message(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &source.display().to_string())],
                    Some("checkRepoUrl"),
                ))
            })?;

            if !canonical_source.starts_with(&canonical_temp) || !canonical_source.is_dir() {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(AppError::Message(format_skill_error(
                    "INVALID_SKILL_DIRECTORY",
                    &[("directory", &skill.directory)],
                    Some("checkZipContent"),
                )));
            }

            Self::copy_dir_recursive(&canonical_source, &dest)?;
            let _ = fs::remove_dir_all(&temp_dir);
        }

        let doc_path = skill
            .readme_url
            .as_deref()
            .and_then(Self::extract_doc_path_from_url)
            .map(|path| {
                if path.ends_with("/SKILL.md") || path == "SKILL.md" {
                    path
                } else {
                    format!("{}/SKILL.md", path.trim_end_matches('/'))
                }
            })
            .unwrap_or_else(|| format!("{}/SKILL.md", skill.directory.trim_end_matches('/')));

        let installed_skill = InstalledSkill {
            id: skill.key.clone(),
            name: skill.name.clone(),
            description: (!skill.description.is_empty()).then_some(skill.description.clone()),
            directory: install_name.clone(),
            repo_owner: Some(skill.repo_owner.clone()),
            repo_name: Some(skill.repo_name.clone()),
            repo_branch: Some(repo_branch.clone()),
            readme_url: Some(Self::build_skill_doc_url(
                &skill.repo_owner,
                &skill.repo_name,
                &repo_branch,
                &doc_path,
            )),
            apps: SkillApps::only(current_app),
            installed_at: chrono::Utc::now().timestamp(),
        };

        db.save_skill(&installed_skill)?;
        Self::sync_to_app_dir(&install_name, current_app)?;
        Ok(installed_skill)
    }

    pub fn save_installed(
        state: &crate::store::AppState,
        skill: &InstalledSkill,
    ) -> Result<(), AppError> {
        state.db.save_skill(skill)
    }

    pub fn uninstall(db: &Arc<Database>, id: &str) -> Result<(), AppError> {
        let skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| AppError::Message(format!("Skill not found: {id}")))?;

        for app in AppType::all() {
            let _ = Self::remove_from_app(&skill.directory, &app);
        }

        let ssot_dir = Self::get_ssot_dir()?;
        let skill_path = ssot_dir.join(&skill.directory);
        if skill_path.exists() {
            fs::remove_dir_all(&skill_path).map_err(|err| AppError::io(&skill_path, err))?;
        }

        db.delete_skill(id)?;
        Ok(())
    }

    pub fn toggle_app(
        db: &Arc<Database>,
        id: &str,
        app: &AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| AppError::Message(format!("Skill not found: {id}")))?;

        skill.apps.set_enabled_for(app, enabled);
        if enabled {
            Self::sync_to_app_dir(&skill.directory, app)?;
        } else {
            Self::remove_from_app(&skill.directory, app)?;
        }

        db.update_skill_apps(id, &skill.apps)?;
        Ok(())
    }

    pub fn scan_unmanaged(db: &Arc<Database>) -> Result<Vec<UnmanagedSkill>, AppError> {
        let managed_skills = db.get_all_installed_skills()?;
        let managed_dirs: HashSet<String> = managed_skills
            .values()
            .map(|skill| skill.directory.clone())
            .collect();

        let mut scan_sources = Vec::new();
        for app in AppType::all() {
            if let Ok(dir) = Self::get_app_skills_dir(&app) {
                scan_sources.push((dir, app.as_str().to_string()));
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
                Ok(entries) => entries,
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
                let (name, description) = Self::read_skill_name_desc(&skill_md, &dir_name);
                unmanaged
                    .entry(dir_name.clone())
                    .and_modify(|skill| skill.found_in.push(label.clone()))
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

    pub fn import_from_apps(
        db: &Arc<Database>,
        directories: Vec<String>,
    ) -> Result<Vec<InstalledSkill>, AppError> {
        let ssot_dir = Self::get_ssot_dir()?;
        let agents_lock = parse_agents_lock();
        save_repos_from_lock(
            db,
            &agents_lock,
            directories.iter().map(|item| item.as_str()),
        );

        let mut search_sources = Vec::new();
        for app in AppType::all() {
            if let Ok(dir) = Self::get_app_skills_dir(&app) {
                search_sources.push((dir, app.as_str().to_string()));
            }
        }
        if let Some(agents_dir) = get_agents_skills_dir() {
            search_sources.push((agents_dir, "agents".to_string()));
        }
        search_sources.push((ssot_dir.clone(), "cc-switch".to_string()));

        let mut imported = Vec::new();
        for dir_name in directories {
            let mut source_path = None;
            let mut found_in = Vec::new();

            for (base, label) in &search_sources {
                let skill_path = base.join(&dir_name);
                if skill_path.exists() {
                    if source_path.is_none() {
                        source_path = Some(skill_path);
                    }
                    found_in.push(label.clone());
                }
            }

            let Some(source) = source_path else {
                continue;
            };

            let dest = ssot_dir.join(&dir_name);
            if !dest.exists() {
                Self::copy_dir_recursive(&source, &dest)?;
            }

            let skill_md = dest.join("SKILL.md");
            let (name, description) = Self::read_skill_name_desc(&skill_md, &dir_name);
            let apps = SkillApps::from_labels(&found_in);
            let (id, repo_owner, repo_name, repo_branch, readme_url) =
                build_repo_info_from_lock(&agents_lock, &dir_name);

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
            };

            db.save_skill(&skill)?;
            imported.push(skill);
        }

        Ok(imported)
    }

    #[cfg(unix)]
    fn create_symlink(src: &Path, dest: &Path) -> AnyResult<()> {
        std::os::unix::fs::symlink(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    #[cfg(windows)]
    fn create_symlink(src: &Path, dest: &Path) -> AnyResult<()> {
        std::os::windows::fs::symlink_dir(src, dest)
            .with_context(|| format!("创建符号链接失败: {} -> {}", src.display(), dest.display()))
    }

    fn is_symlink(path: &Path) -> bool {
        path.symlink_metadata()
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn get_sync_method() -> SyncMethod {
        crate::settings::get_skill_sync_method()
    }

    pub fn sync_to_app_dir(directory: &str, app: &AppType) -> Result<(), AppError> {
        let ssot_dir = Self::get_ssot_dir()?;
        let source = ssot_dir.join(directory);
        if !source.exists() {
            return Err(AppError::Message(format!(
                "Skill 不存在于 SSOT: {directory}"
            )));
        }

        let app_dir = Self::get_app_skills_dir(app)?;
        fs::create_dir_all(&app_dir).map_err(|err| AppError::io(&app_dir, err))?;

        let dest = app_dir.join(directory);
        if dest.exists() || Self::is_symlink(&dest) {
            Self::remove_path(&dest)?;
        }

        match Self::get_sync_method() {
            SyncMethod::Auto => {
                if let Err(err) = Self::create_symlink(&source, &dest) {
                    log::warn!(
                        "Skill symlink 失败，回退到复制: {} -> {}: {err:#}",
                        source.display(),
                        dest.display()
                    );
                    Self::copy_dir_recursive(&source, &dest)?;
                }
            }
            SyncMethod::Symlink => {
                Self::create_symlink(&source, &dest)?;
            }
            SyncMethod::Copy => {
                Self::copy_dir_recursive(&source, &dest)?;
            }
        }

        Ok(())
    }

    pub fn remove_from_app(directory: &str, app: &AppType) -> Result<(), AppError> {
        let app_dir = Self::get_app_skills_dir(app)?;
        let skill_path = app_dir.join(directory);
        if skill_path.exists() || Self::is_symlink(&skill_path) {
            Self::remove_path(&skill_path)?;
        }
        Ok(())
    }

    pub fn sync_to_app(db: &Arc<Database>, app: &AppType) -> Result<(), AppError> {
        let skills = db.get_all_installed_skills()?;
        for skill in skills.values() {
            if skill.apps.is_enabled_for(app) {
                Self::sync_to_app_dir(&skill.directory, app)?;
            }
        }
        Ok(())
    }

    pub async fn discover_available(
        &self,
        repos: Vec<SkillRepo>,
    ) -> Result<Vec<DiscoverableSkill>, AppError> {
        let enabled_repos: Vec<_> = repos.into_iter().filter(|repo| repo.enabled).collect();
        let tasks = enabled_repos
            .iter()
            .map(|repo| self.fetch_repo_skills(repo));
        let results = futures::future::join_all(tasks).await;

        let mut skills = Vec::new();
        for (repo, result) in enabled_repos.into_iter().zip(results.into_iter()) {
            match result {
                Ok(found) => skills.extend(found),
                Err(err) => log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, err),
            }
        }

        Self::deduplicate_discoverable_skills(&mut skills);
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(skills)
    }

    pub async fn list_skills(
        &self,
        repos: Vec<SkillRepo>,
        db: &Arc<Database>,
    ) -> Result<Vec<Skill>, AppError> {
        let discoverable = self.discover_available(repos).await?;
        let installed = db.get_all_installed_skills()?;
        let installed_dirs: HashSet<String> = installed
            .values()
            .map(|skill| skill.directory.clone())
            .collect();

        let mut skills: Vec<Skill> = discoverable
            .into_iter()
            .map(|skill| {
                let install_name = Path::new(&skill.directory)
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| skill.directory.clone());

                Skill {
                    key: skill.key,
                    name: skill.name,
                    description: skill.description,
                    directory: skill.directory,
                    readme_url: skill.readme_url,
                    installed: installed_dirs.contains(&install_name),
                    repo_owner: Some(skill.repo_owner),
                    repo_name: Some(skill.repo_name),
                    repo_branch: Some(skill.repo_branch),
                }
            })
            .collect();

        for skill in installed.values() {
            let already_listed = skills.iter().any(|item| {
                let install_name = Path::new(&item.directory)
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| item.directory.clone());
                install_name == skill.directory
            });
            if already_listed {
                continue;
            }

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

        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(skills)
    }

    fn build_skill_doc_url(owner: &str, repo: &str, branch: &str, doc_path: &str) -> String {
        format!("https://github.com/{owner}/{repo}/blob/{branch}/{doc_path}")
    }

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
        (!path.is_empty()).then_some(path.to_string())
    }

    async fn fetch_repo_skills(&self, repo: &SkillRepo) -> AnyResult<Vec<DiscoverableSkill>> {
        let (temp_dir, resolved_branch) =
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

        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch;
        let mut skills = Vec::new();
        self.scan_dir_recursive(&temp_dir, &temp_dir, &resolved_repo, &mut skills)?;
        let _ = fs::remove_dir_all(&temp_dir);
        Ok(skills)
    }

    fn scan_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &SkillRepo,
        skills: &mut Vec<DiscoverableSkill>,
    ) -> AnyResult<()> {
        let skill_md = current_dir.join("SKILL.md");
        if skill_md.exists() {
            let directory = if current_dir == base_dir {
                repo.name.clone()
            } else {
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .to_string()
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

    fn build_skill_from_metadata(
        &self,
        skill_md: &Path,
        directory: &str,
        doc_path: &str,
        repo: &SkillRepo,
    ) -> AnyResult<DiscoverableSkill> {
        let meta = self.parse_skill_metadata(skill_md)?;
        Ok(DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Some(Self::build_skill_doc_url(
                &repo.owner,
                &repo.name,
                &repo.branch,
                doc_path,
            )),
            repo_owner: repo.owner.clone(),
            repo_name: repo.name.clone(),
            repo_branch: repo.branch.clone(),
        })
    }

    fn parse_skill_metadata(&self, path: &Path) -> AnyResult<SkillMetadata> {
        Self::parse_skill_metadata_static(path)
    }

    fn parse_skill_metadata_static(path: &Path) -> AnyResult<SkillMetadata> {
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
        Ok(serde_yaml::from_str(front_matter).unwrap_or(SkillMetadata {
            name: None,
            description: None,
        }))
    }

    fn read_skill_name_desc(skill_md: &Path, fallback_name: &str) -> (String, Option<String>) {
        if !skill_md.exists() {
            return (fallback_name.to_string(), None);
        }

        match Self::parse_skill_metadata_static(skill_md) {
            Ok(meta) => (
                meta.name.unwrap_or_else(|| fallback_name.to_string()),
                meta.description,
            ),
            Err(_) => (fallback_name.to_string(), None),
        }
    }

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
                | Component::Prefix(_) => return None,
            }
        }

        has_component.then_some(normalized)
    }

    fn sanitize_install_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut components = Path::new(trimmed).components();
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

    fn deduplicate_discoverable_skills(skills: &mut Vec<DiscoverableSkill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            let unique_key = skill.key.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(unique_key) {
                entry.insert(true);
                true
            } else {
                false
            }
        });
    }

    async fn download_repo(&self, repo: &SkillRepo) -> AnyResult<(PathBuf, String)> {
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();
        let _ = temp_dir.keep();

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

            match self.download_and_extract(&url, &temp_path).await {
                Ok(()) => return Ok((temp_path, branch.to_string())),
                Err(err) => last_error = Some(err),
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("所有分支下载失败")))
    }

    async fn download_and_extract(&self, url: &str, dest: &Path) -> AnyResult<()> {
        let client = reqwest::Client::new();
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            let status = response.status().as_u16().to_string();
            return Err(anyhow!(format_skill_error(
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

        let bytes = response.bytes().await?;
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        if archive.is_empty() {
            return Err(anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkRepoUrl"),
            )));
        }

        let root_name = {
            let first_file = archive.by_index(0)?;
            first_file
                .name()
                .split('/')
                .next()
                .unwrap_or("")
                .to_string()
        };

        let mut symlinks = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = file.name().to_string();
            let Some(relative_path) = file_path.strip_prefix(&format!("{root_name}/")) else {
                continue;
            };
            if relative_path.is_empty() {
                continue;
            }

            let outpath = dest.join(relative_path);
            if file.is_symlink() {
                let mut target = String::new();
                std::io::Read::read_to_string(&mut file, &mut target)?;
                symlinks.push((outpath, target.trim().to_string()));
            } else if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Self::resolve_symlinks_in_dir(dest, &symlinks)?;
        Ok(())
    }

    fn copy_dir_recursive(src: &Path, dest: &Path) -> AnyResult<()> {
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

    fn resolve_symlinks_in_dir(base_dir: &Path, symlinks: &[(PathBuf, String)]) -> AnyResult<()> {
        let canonical_base = base_dir
            .canonicalize()
            .unwrap_or_else(|_| base_dir.to_path_buf());

        for (link_path, target) in symlinks {
            let parent = link_path.parent().unwrap_or(base_dir);
            let resolved = parent.join(target);
            let resolved = match resolved.canonicalize() {
                Ok(path) => path,
                Err(_) => {
                    log::warn!(
                        "Skill symlink 目标不存在，跳过: {} -> {}",
                        link_path.display(),
                        target
                    );
                    continue;
                }
            };

            if !resolved.starts_with(&canonical_base) {
                log::warn!(
                    "Skill symlink 目标超出仓库范围，跳过: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            if resolved.is_dir() {
                Self::copy_dir_recursive(&resolved, link_path)?;
            } else if resolved.is_file() {
                if let Some(parent) = link_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&resolved, link_path)?;
            }
        }

        Ok(())
    }

    pub fn install_from_zip(
        db: &Arc<Database>,
        zip_path: &Path,
        current_app: &AppType,
    ) -> Result<Vec<InstalledSkill>, AppError> {
        let temp_dir = Self::extract_local_zip(zip_path)?;
        let skill_dirs = Self::scan_skills_in_dir(&temp_dir)?;
        if skill_dirs.is_empty() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(AppError::Message(format_skill_error(
                "NO_SKILLS_IN_ZIP",
                &[],
                Some("checkZipContent"),
            )));
        }

        let ssot_dir = Self::get_ssot_dir()?;
        let existing_skills = db.get_all_installed_skills()?;
        let zip_stem = zip_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string);

        let mut installed = Vec::new();
        for skill_dir in skill_dirs {
            let skill_md = skill_dir.join("SKILL.md");
            let meta = if skill_md.exists() {
                Self::parse_skill_metadata_static(&skill_md).ok()
            } else {
                None
            };

            let install_name = {
                let dir_name = skill_dir
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_default();

                if skill_dir == temp_dir || dir_name.is_empty() || dir_name.starts_with('.') {
                    meta.as_ref()
                        .and_then(|value| value.name.as_deref())
                        .and_then(Self::sanitize_install_name)
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                } else {
                    Self::sanitize_install_name(&dir_name)
                        .or_else(|| {
                            meta.as_ref()
                                .and_then(|value| value.name.as_deref())
                                .and_then(Self::sanitize_install_name)
                        })
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                }
            }
            .ok_or_else(|| {
                AppError::Message(format_skill_error(
                    "INVALID_SKILL_DIRECTORY",
                    &[("zip", &zip_path.display().to_string())],
                    Some("checkZipContent"),
                ))
            })?;

            if let Some(existing) = existing_skills
                .values()
                .find(|value| value.directory.eq_ignore_ascii_case(&install_name))
            {
                log::warn!(
                    "Skill directory '{}' already exists (from {}), skipping",
                    install_name,
                    existing.id
                );
                continue;
            }

            let (name, description) = match meta {
                Some(meta) => (
                    meta.name.unwrap_or_else(|| install_name.clone()),
                    meta.description,
                ),
                None => (install_name.clone(), None),
            };

            let dest = ssot_dir.join(&install_name);
            if dest.exists() {
                fs::remove_dir_all(&dest).map_err(|err| AppError::io(&dest, err))?;
            }
            Self::copy_dir_recursive(&skill_dir, &dest)?;

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
            };

            db.save_skill(&skill)?;
            Self::sync_to_app_dir(&install_name, current_app)?;
            installed.push(skill);
        }

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(installed)
    }

    fn extract_local_zip(zip_path: &Path) -> AnyResult<PathBuf> {
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

        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();
        let _ = temp_dir.keep();

        let mut symlinks = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let Some(file_path) = file.enclosed_name().map(|value| value.to_owned()) else {
                continue;
            };
            let outpath = temp_path.join(&file_path);

            if file.is_symlink() {
                let mut target = String::new();
                std::io::Read::read_to_string(&mut file, &mut target)?;
                symlinks.push((outpath, target.trim().to_string()));
            } else if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Self::resolve_symlinks_in_dir(&temp_path, &symlinks)?;
        Ok(temp_path)
    }

    fn scan_skills_in_dir(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
        let mut skill_dirs = Vec::new();
        Self::scan_skills_recursive(dir, &mut skill_dirs)?;
        Ok(skill_dirs)
    }

    fn scan_skills_recursive(current: &Path, results: &mut Vec<PathBuf>) -> Result<(), AppError> {
        if current.join("SKILL.md").exists() {
            results.push(current.to_path_buf());
            return Ok(());
        }

        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') {
                    continue;
                }
                Self::scan_skills_recursive(&path, results)?;
            }
        }

        Ok(())
    }

    fn remove_path(path: &Path) -> AnyResult<()> {
        if Self::is_symlink(path) {
            #[cfg(unix)]
            fs::remove_file(path)?;
            #[cfg(windows)]
            fs::remove_dir(path)?;
        } else if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn list_repos(&self, store: &SkillStore) -> Vec<SkillRepo> {
        store.repos.clone()
    }

    pub fn add_repo(&self, store: &mut SkillStore, repo: SkillRepo) -> Result<(), AppError> {
        if let Some(position) = store
            .repos
            .iter()
            .position(|item| item.owner == repo.owner && item.name == repo.name)
        {
            store.repos[position] = repo;
        } else {
            store.repos.push(repo);
        }
        Ok(())
    }

    pub fn remove_repo(
        &self,
        store: &mut SkillStore,
        owner: String,
        name: String,
    ) -> Result<(), AppError> {
        store
            .repos
            .retain(|repo| !(repo.owner == owner && repo.name == name));
        Ok(())
    }
}

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
            let fallback = format!("{dir_name}/SKILL.md");
            let doc_path = info.skill_path.as_deref().unwrap_or(&fallback);
            (
                format!("{}/{}:{dir_name}", info.owner, info.repo),
                Some(info.owner.clone()),
                Some(info.repo.clone()),
                branch,
                Some(SkillService::build_skill_doc_url(
                    &info.owner,
                    &info.repo,
                    &url_branch,
                    doc_path,
                )),
            )
        }
        None => (format!("local:{dir_name}"), None, None, None, None),
    }
}

fn save_repos_from_lock(
    db: &Arc<Database>,
    lock: &HashMap<String, LockRepoInfo>,
    directories: impl Iterator<Item = impl AsRef<str>>,
) {
    let existing_repos: HashSet<(String, String)> = db
        .get_skill_repos()
        .unwrap_or_default()
        .into_iter()
        .map(|repo| (repo.owner, repo.name))
        .collect();
    let mut added = HashSet::new();

    for dir_name in directories {
        if let Some(info) = lock.get(dir_name.as_ref()) {
            let key = (info.owner.clone(), info.repo.clone());
            if existing_repos.contains(&key) || !added.insert(key) {
                continue;
            }

            let repo = SkillRepo {
                owner: info.owner.clone(),
                name: info.repo.clone(),
                branch: info.branch.clone().unwrap_or_else(|| "HEAD".to_string()),
                enabled: true,
            };
            if let Err(err) = db.save_skill_repo(&repo) {
                log::warn!("保存 skill 仓库 {}/{} 失败: {}", info.owner, info.repo, err);
            }
        }
    }
}

pub fn migrate_skills_to_ssot(db: &Arc<Database>) -> Result<usize, AppError> {
    let ssot_dir = SkillService::get_ssot_dir()?;
    let agents_lock = parse_agents_lock();
    let mut discovered: HashMap<String, SkillApps> = HashMap::new();

    for app in AppType::all() {
        let app_dir = match SkillService::get_app_skills_dir(&app) {
            Ok(dir) => dir,
            Err(_) => continue,
        };

        let entries = match fs::read_dir(&app_dir) {
            Ok(entries) => entries,
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

            let ssot_path = ssot_dir.join(&dir_name);
            if !ssot_path.exists() {
                SkillService::copy_dir_recursive(&path, &ssot_path)?;
            }

            discovered
                .entry(dir_name)
                .or_default()
                .set_enabled_for(&app, true);
        }
    }

    db.clear_skills()?;
    save_repos_from_lock(db, &agents_lock, discovered.keys());

    let mut count = 0;
    for (directory, apps) in discovered {
        let ssot_path = ssot_dir.join(&directory);
        let skill_md = ssot_path.join("SKILL.md");
        let (name, description) = SkillService::read_skill_name_desc(&skill_md, &directory);
        let (id, repo_owner, repo_name, repo_branch, readme_url) =
            build_repo_info_from_lock(&agents_lock, &directory);

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
        };

        db.save_skill(&skill)?;
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::database::Database;
    use crate::settings::AppSettings;

    fn exists_or_symlink(path: &Path) -> bool {
        path.exists()
            || path
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink())
    }

    fn write_skill(dir: &Path, name: &str, description: &str) -> Result<(), AppError> {
        fs::create_dir_all(dir).map_err(|err| AppError::io(dir, err))?;
        let skill_md = dir.join("SKILL.md");
        let content = format!("---\nname: {name}\ndescription: {description}\n---\n");
        fs::write(&skill_md, content).map_err(|err| AppError::io(&skill_md, err))
    }

    #[test]
    #[serial]
    fn toggle_app_syncs_filesystem_state() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let db = Arc::new(Database::memory()?);
        let ssot_dir = SkillService::get_ssot_dir()?;
        write_skill(&ssot_dir.join("demo-skill"), "Demo Skill", "demo")?;

        db.save_skill(&InstalledSkill {
            id: "local:demo-skill".to_string(),
            name: "Demo Skill".to_string(),
            description: Some("demo".to_string()),
            directory: "demo-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps::default(),
            installed_at: 1,
        })?;

        SkillService::toggle_app(&db, "local:demo-skill", &AppType::Claude, true)?;
        let app_path = SkillService::get_app_skills_dir(&AppType::Claude)?.join("demo-skill");
        assert!(exists_or_symlink(&app_path));

        SkillService::toggle_app(&db, "local:demo-skill", &AppType::Claude, false)?;
        assert!(!exists_or_symlink(&app_path));

        Ok(())
    }

    #[test]
    #[serial]
    fn scan_unmanaged_finds_live_directories() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let db = Arc::new(Database::memory()?);
        let managed_dir = SkillService::get_app_skills_dir(&AppType::Claude)?.join("managed");
        write_skill(&managed_dir, "Managed", "managed")?;
        let unmanaged_dir = SkillService::get_app_skills_dir(&AppType::Claude)?.join("unmanaged");
        write_skill(&unmanaged_dir, "Unmanaged", "unmanaged")?;
        let agents_dir = get_home_dir()
            .join(".agents")
            .join("skills")
            .join("agent-skill");
        write_skill(&agents_dir, "Agent Skill", "agent")?;

        db.save_skill(&InstalledSkill {
            id: "local:managed".to_string(),
            name: "Managed".to_string(),
            description: Some("managed".to_string()),
            directory: "managed".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps::only(&AppType::Claude),
            installed_at: 1,
        })?;

        let mut unmanaged = SkillService::scan_unmanaged(&db)?;
        unmanaged.sort_by(|a, b| a.directory.cmp(&b.directory));

        assert_eq!(unmanaged.len(), 2);
        assert_eq!(unmanaged[0].directory, "agent-skill");
        assert_eq!(unmanaged[1].directory, "unmanaged");
        assert!(unmanaged[1].found_in.contains(&"claude".to_string()));

        Ok(())
    }

    #[test]
    #[serial]
    fn import_from_apps_copies_into_ssot() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let db = Arc::new(Database::memory()?);
        let source_dir = SkillService::get_app_skills_dir(&AppType::Claude)?.join("imported");
        write_skill(&source_dir, "Imported", "from app")?;

        let imported = SkillService::import_from_apps(&db, vec!["imported".to_string()])?;
        assert_eq!(imported.len(), 1);
        assert!(imported[0].apps.claude);

        let ssot_skill = SkillService::get_ssot_dir()?
            .join("imported")
            .join("SKILL.md");
        assert!(ssot_skill.exists());
        assert!(db.get_installed_skill("local:imported")?.is_some());

        Ok(())
    }

    #[test]
    #[serial]
    fn install_from_zip_imports_root_skill() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let zip_path = temp.path().join("skill.zip");
        let zip_file = fs::File::create(&zip_path).map_err(|err| AppError::io(&zip_path, err))?;
        let mut writer = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("SKILL.md", options)
            .map_err(|err| AppError::Message(err.to_string()))?;
        writer
            .write_all(b"---\nname: Zip Skill\ndescription: from zip\n---\n")
            .map_err(|err| AppError::Message(err.to_string()))?;
        writer
            .start_file("README.txt", options)
            .map_err(|err| AppError::Message(err.to_string()))?;
        writer
            .write_all(b"zip body")
            .map_err(|err| AppError::Message(err.to_string()))?;
        writer
            .finish()
            .map_err(|err| AppError::Message(err.to_string()))?;

        let db = Arc::new(Database::memory()?);
        let installed = SkillService::install_from_zip(&db, &zip_path, &AppType::Claude)?;
        assert_eq!(installed.len(), 1);

        let install_name = &installed[0].directory;
        let ssot_dir = SkillService::get_ssot_dir()?.join(install_name);
        assert!(ssot_dir.join("SKILL.md").exists());
        let app_dir = SkillService::get_app_skills_dir(&AppType::Claude)?.join(install_name);
        assert!(exists_or_symlink(&app_dir));

        Ok(())
    }
}
