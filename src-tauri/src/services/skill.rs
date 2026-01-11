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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::timeout;

use crate::app_config::{AppType, InstalledSkill, SkillApps, UnmanagedSkill};
use crate::config::get_app_config_dir;
use crate::database::Database;
use crate::error::format_skill_error;

// ========== 数据结构 ==========

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
            ],
        }
    }
}

/// 技能元数据 (从 SKILL.md 解析)
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
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

    // ========== 路径管理 ==========

    /// 获取 SSOT 目录（~/.cc-switch/skills/）
    pub fn get_ssot_dir() -> Result<PathBuf> {
        let dir = get_app_config_dir().join("skills");
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
        }

        // 默认路径：回退到用户主目录下的标准位置
        let home = dirs::home_dir().context(format_skill_error(
            "GET_HOME_DIR_FAILED",
            &[],
            Some("checkPermission"),
        ))?;

        Ok(match app {
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::Codex => home.join(".codex").join("skills"),
            AppType::Gemini => home.join(".gemini").join("skills"),
        })
    }

    // ========== 统一管理方法 ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed(db: &Arc<Database>) -> Result<Vec<InstalledSkill>> {
        let skills = db.get_all_installed_skills()?;
        Ok(skills.into_values().collect())
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

        // 使用目录最后一段作为安装名
        let install_name = Path::new(&skill.directory)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| skill.directory.clone());

        let dest = ssot_dir.join(&install_name);

        // 如果已存在则跳过下载
        if !dest.exists() {
            let repo = SkillRepo {
                owner: skill.repo_owner.clone(),
                name: skill.repo_name.clone(),
                branch: skill.repo_branch.clone(),
                enabled: true,
            };

            // 下载仓库
            let temp_dir = timeout(
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

            // 复制到 SSOT
            let source = temp_dir.join(&skill.directory);
            if !source.exists() {
                let _ = fs::remove_dir_all(&temp_dir);
                return Err(anyhow!(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &source.display().to_string())],
                    Some("checkRepoUrl"),
                )));
            }

            Self::copy_dir_recursive(&source, &dest)?;
            let _ = fs::remove_dir_all(&temp_dir);
        }

        // 创建 InstalledSkill 记录
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
            repo_branch: Some(skill.repo_branch.clone()),
            readme_url: skill.readme_url.clone(),
            apps: SkillApps::only(current_app),
            installed_at: chrono::Utc::now().timestamp(),
        };

        // 保存到数据库
        db.save_skill(&installed_skill)?;

        // 同步到当前应用目录
        Self::copy_to_app(&install_name, current_app)?;

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
    pub fn uninstall(db: &Arc<Database>, id: &str) -> Result<()> {
        // 获取 skill 信息
        let skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| anyhow!("Skill not found: {id}"))?;

        // 从所有应用目录删除
        for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            let _ = Self::remove_from_app(&skill.directory, &app);
        }

        // 从 SSOT 删除
        let ssot_dir = Self::get_ssot_dir()?;
        let skill_path = ssot_dir.join(&skill.directory);
        if skill_path.exists() {
            fs::remove_dir_all(&skill_path)?;
        }

        // 从数据库删除
        db.delete_skill(id)?;

        log::info!("Skill {} 卸载成功", skill.name);

        Ok(())
    }

    /// 切换应用启用状态
    ///
    /// 启用：复制到应用目录
    /// 禁用：从应用目录删除
    pub fn toggle_app(db: &Arc<Database>, id: &str, app: &AppType, enabled: bool) -> Result<()> {
        // 获取当前 skill
        let mut skill = db
            .get_installed_skill(id)?
            .ok_or_else(|| anyhow!("Skill not found: {id}"))?;

        // 更新状态
        skill.apps.set_enabled_for(app, enabled);

        // 同步文件
        if enabled {
            Self::copy_to_app(&skill.directory, app)?;
        } else {
            Self::remove_from_app(&skill.directory, app)?;
        }

        // 更新数据库
        db.update_skill_apps(id, &skill.apps)?;

        log::info!("Skill {} 的 {:?} 状态已更新为 {}", skill.name, app, enabled);

        Ok(())
    }

    /// 扫描未管理的 Skills
    ///
    /// 扫描各应用目录，找出未被 CC Switch 管理的 Skills
    pub fn scan_unmanaged(db: &Arc<Database>) -> Result<Vec<UnmanagedSkill>> {
        let managed_skills = db.get_all_installed_skills()?;
        let managed_dirs: HashSet<String> = managed_skills
            .values()
            .map(|s| s.directory.clone())
            .collect();

        let mut unmanaged: HashMap<String, UnmanagedSkill> = HashMap::new();

        for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            let app_dir = match Self::get_app_skills_dir(&app) {
                Ok(d) => d,
                Err(_) => continue,
            };

            if !app_dir.exists() {
                continue;
            }

            for entry in fs::read_dir(&app_dir)? {
                let entry = entry?;
                let path = entry.path();

                if !path.is_dir() {
                    continue;
                }

                let dir_name = entry.file_name().to_string_lossy().to_string();

                // 跳过隐藏目录（以 . 开头，如 .system）
                if dir_name.starts_with('.') {
                    continue;
                }

                // 跳过已管理的
                if managed_dirs.contains(&dir_name) {
                    continue;
                }

                // 检查是否有 SKILL.md
                let skill_md = path.join("SKILL.md");
                let (name, description) = if skill_md.exists() {
                    match Self::parse_skill_metadata_static(&skill_md) {
                        Ok(meta) => (
                            meta.name.unwrap_or_else(|| dir_name.clone()),
                            meta.description,
                        ),
                        Err(_) => (dir_name.clone(), None),
                    }
                } else {
                    (dir_name.clone(), None)
                };

                // 添加或更新
                let app_str = match app {
                    AppType::Claude => "claude",
                    AppType::Codex => "codex",
                    AppType::Gemini => "gemini",
                };

                unmanaged
                    .entry(dir_name.clone())
                    .and_modify(|s| s.found_in.push(app_str.to_string()))
                    .or_insert(UnmanagedSkill {
                        directory: dir_name,
                        name,
                        description,
                        found_in: vec![app_str.to_string()],
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
        directories: Vec<String>,
    ) -> Result<Vec<InstalledSkill>> {
        let ssot_dir = Self::get_ssot_dir()?;
        let mut imported = Vec::new();

        for dir_name in directories {
            // 找到源目录（从任一应用目录复制）
            let mut source_path: Option<PathBuf> = None;
            let mut found_in: Vec<String> = Vec::new();

            for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
                if let Ok(app_dir) = Self::get_app_skills_dir(&app) {
                    let skill_path = app_dir.join(&dir_name);
                    if skill_path.exists() {
                        if source_path.is_none() {
                            source_path = Some(skill_path);
                        }
                        let app_str = match app {
                            AppType::Claude => "claude",
                            AppType::Codex => "codex",
                            AppType::Gemini => "gemini",
                        };
                        found_in.push(app_str.to_string());
                    }
                }
            }

            let source = match source_path {
                Some(p) => p,
                None => continue,
            };

            // 复制到 SSOT
            let dest = ssot_dir.join(&dir_name);
            if !dest.exists() {
                Self::copy_dir_recursive(&source, &dest)?;
            }

            // 解析元数据
            let skill_md = dest.join("SKILL.md");
            let (name, description) = if skill_md.exists() {
                match Self::parse_skill_metadata_static(&skill_md) {
                    Ok(meta) => (
                        meta.name.unwrap_or_else(|| dir_name.clone()),
                        meta.description,
                    ),
                    Err(_) => (dir_name.clone(), None),
                }
            } else {
                (dir_name.clone(), None)
            };

            // 构建启用状态
            let mut apps = SkillApps::default();
            for app_str in &found_in {
                match app_str.as_str() {
                    "claude" => apps.claude = true,
                    "codex" => apps.codex = true,
                    "gemini" => apps.gemini = true,
                    _ => {}
                }
            }

            // 创建记录
            let skill = InstalledSkill {
                id: format!("local:{dir_name}"),
                name,
                description,
                directory: dir_name,
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps,
                installed_at: chrono::Utc::now().timestamp(),
            };

            // 保存到数据库
            db.save_skill(&skill)?;
            imported.push(skill);
        }

        log::info!("成功导入 {} 个 Skills", imported.len());

        Ok(imported)
    }

    // ========== 文件同步方法 ==========

    /// 复制 Skill 到应用目录
    pub fn copy_to_app(directory: &str, app: &AppType) -> Result<()> {
        let ssot_dir = Self::get_ssot_dir()?;
        let source = ssot_dir.join(directory);

        if !source.exists() {
            return Err(anyhow!("Skill 不存在于 SSOT: {directory}"));
        }

        let app_dir = Self::get_app_skills_dir(app)?;
        fs::create_dir_all(&app_dir)?;

        let dest = app_dir.join(directory);

        // 如果已存在则先删除
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }

        Self::copy_dir_recursive(&source, &dest)?;

        log::debug!("Skill {directory} 已复制到 {app:?}");

        Ok(())
    }

    /// 从应用目录删除 Skill
    pub fn remove_from_app(directory: &str, app: &AppType) -> Result<()> {
        let app_dir = Self::get_app_skills_dir(app)?;
        let skill_path = app_dir.join(directory);

        if skill_path.exists() {
            fs::remove_dir_all(&skill_path)?;
            log::debug!("Skill {directory} 已从 {app:?} 删除");
        }

        Ok(())
    }

    /// 同步所有已启用的 Skills 到指定应用
    pub fn sync_to_app(db: &Arc<Database>, app: &AppType) -> Result<()> {
        let skills = db.get_all_installed_skills()?;

        for skill in skills.values() {
            if skill.apps.is_enabled_for(app) {
                Self::copy_to_app(&skill.directory, app)?;
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

        for (repo, result) in enabled_repos.into_iter().zip(results.into_iter()) {
            match result {
                Ok(repo_skills) => skills.extend(repo_skills),
                Err(e) => log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e),
            }
        }

        // 去重并排序
        Self::deduplicate_discoverable_skills(&mut skills);
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

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

        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(skills)
    }

    /// 从仓库获取技能列表
    async fn fetch_repo_skills(&self, repo: &SkillRepo) -> Result<Vec<DiscoverableSkill>> {
        let temp_dir = timeout(std::time::Duration::from_secs(60), self.download_repo(repo))
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
        let scan_dir = temp_dir.clone();

        self.scan_dir_recursive(&scan_dir, &scan_dir, repo, &mut skills)?;

        let _ = fs::remove_dir_all(&temp_dir);

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
                    .to_string()
            };

            if let Ok(skill) = self.build_skill_from_metadata(&skill_md, &directory, repo) {
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
        repo: &SkillRepo,
    ) -> Result<DiscoverableSkill> {
        let meta = self.parse_skill_metadata(skill_md)?;

        Ok(DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Some(format!(
                "https://github.com/{}/{}/tree/{}/{}",
                repo.owner, repo.name, repo.branch, directory
            )),
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

    /// 去重技能列表
    fn deduplicate_discoverable_skills(skills: &mut Vec<DiscoverableSkill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
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
    async fn download_repo(&self, repo: &SkillRepo) -> Result<PathBuf> {
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();
        let _ = temp_dir.keep();

        let branches = if repo.branch.is_empty() {
            vec!["main", "master"]
        } else {
            vec![repo.branch.as_str(), "main", "master"]
        };

        let mut last_error = None;
        for branch in branches {
            let url = format!(
                "https://github.com/{}/{}/archive/refs/heads/{}.zip",
                repo.owner, repo.name, branch
            );

            match self.download_and_extract(&url, &temp_path).await {
                Ok(_) => {
                    return Ok(temp_path);
                }
                Err(e) => {
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

        let bytes = response.bytes().await?;
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;

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

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = file.name();

            let relative_path =
                if let Some(stripped) = file_path.strip_prefix(&format!("{root_name}/")) {
                    stripped
                } else {
                    continue;
                };

            if relative_path.is_empty() {
                continue;
            }

            let outpath = dest.join(relative_path);

            if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Ok(())
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
}

// ========== 迁移支持 ==========

/// 首次启动迁移：扫描应用目录，重建数据库
pub fn migrate_skills_to_ssot(db: &Arc<Database>) -> Result<usize> {
    let ssot_dir = SkillService::get_ssot_dir()?;
    let mut discovered: HashMap<String, SkillApps> = HashMap::new();

    // 扫描各应用目录
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let app_dir = match SkillService::get_app_skills_dir(&app) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if !app_dir.exists() {
            continue;
        }

        for entry in fs::read_dir(&app_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();

            // 跳过隐藏目录（以 . 开头，如 .system）
            if dir_name.starts_with('.') {
                continue;
            }

            // 复制到 SSOT（如果不存在）
            let ssot_path = ssot_dir.join(&dir_name);
            if !ssot_path.exists() {
                SkillService::copy_dir_recursive(&path, &ssot_path)?;
            }

            // 记录启用状态
            discovered
                .entry(dir_name)
                .or_default()
                .set_enabled_for(&app, true);
        }
    }

    // 重建数据库
    db.clear_skills()?;

    let mut count = 0;
    for (directory, apps) in discovered {
        let ssot_path = ssot_dir.join(&directory);
        let skill_md = ssot_path.join("SKILL.md");

        let (name, description) = if skill_md.exists() {
            match SkillService::parse_skill_metadata_static(&skill_md) {
                Ok(meta) => (
                    meta.name.unwrap_or_else(|| directory.clone()),
                    meta.description,
                ),
                Err(_) => (directory.clone(), None),
            }
        } else {
            (directory.clone(), None)
        };

        let skill = InstalledSkill {
            id: format!("local:{directory}"),
            name,
            description,
            directory,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps,
            installed_at: chrono::Utc::now().timestamp(),
        };

        db.save_skill(&skill)?;
        count += 1;
    }

    log::info!("Skills 迁移完成，共 {count} 个");

    Ok(count)
}
