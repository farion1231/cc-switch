use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::time::timeout;

use crate::app_config::AppType;
use crate::error::format_skill_error;

/// 技能对象
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
    
    // 私有仓库字段（可选）
    /// 私有仓库的基础 URL（如 https://gitlab.company.com）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 访问令牌
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// 认证头名称（连通测试时自动探测，如 "Authorization" 或 "PRIVATE-TOKEN"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
}

/// 技能安装状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    /// 是否已安装
    pub installed: bool,
    /// 安装时间
    #[serde(rename = "installedAt")]
    pub installed_at: DateTime<Utc>,
}

/// 持久化存储结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    /// directory -> 安装状态
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
                    owner: "ComposioHQ".to_string(),
                    name: "awesome-claude-skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                    base_url: None,
                    access_token: None,
                    auth_header: None,
                },
                SkillRepo {
                    owner: "anthropics".to_string(),
                    name: "skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                    base_url: None,
                    access_token: None,
                    auth_header: None,
                },
                SkillRepo {
                    owner: "cexll".to_string(),
                    name: "myclaude".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                    base_url: None,
                    access_token: None,
                    auth_header: None,
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

pub struct SkillService {
    http_client: Client,
    install_dir: PathBuf,
    app_type: AppType,
}

impl SkillService {
    pub fn new() -> Result<Self> {
        Self::new_for_app(AppType::Claude)
    }

    pub fn new_for_app(app_type: AppType) -> Result<Self> {
        let install_dir = Self::get_install_dir_for_app(&app_type)?;

        // 确保目录存在
        fs::create_dir_all(&install_dir)?;

        Ok(Self {
            http_client: Client::builder()
                .user_agent("cc-switch")
                // 将单次请求超时时间控制在 10 秒以内，避免无效链接导致长时间卡住
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            install_dir,
            app_type,
        })
    }

    fn get_install_dir_for_app(app_type: &AppType) -> Result<PathBuf> {
        let home = dirs::home_dir().context(format_skill_error(
            "GET_HOME_DIR_FAILED",
            &[],
            Some("checkPermission"),
        ))?;

        let dir = match app_type {
            AppType::Claude => home.join(".claude").join("skills"),
            AppType::Codex => {
                // 检查是否有自定义 Codex 配置目录
                if let Some(custom) = crate::settings::get_codex_override_dir() {
                    custom.join("skills")
                } else {
                    home.join(".codex").join("skills")
                }
            }
            AppType::Gemini => {
                // 为 Gemini 预留，暂时使用默认路径
                home.join(".gemini").join("skills")
            }
        };

        Ok(dir)
    }

    pub fn app_type(&self) -> &AppType {
        &self.app_type
    }
}

// 核心方法实现
impl SkillService {
    /// 获取单个仓库的技能列表
    /// 
    /// 根据 owner 和 name 查找对应的仓库配置，然后加载该仓库的技能。
    /// 只更新已安装状态，不添加本地独有的技能。
    pub async fn list_skills_for_repo(
        &self,
        repos: &[SkillRepo],
        repo_owner: &str,
        repo_name: &str,
    ) -> Result<Vec<Skill>> {
        // 查找匹配的仓库配置
        let repo = repos
            .iter()
            .find(|r| r.owner == repo_owner && r.name == repo_name && r.enabled)
            .ok_or_else(|| {
                anyhow!(format_skill_error(
                    "REPO_NOT_FOUND",
                    &[("owner", repo_owner), ("name", repo_name)],
                    Some("checkRepoConfig"),
                ))
            })?;

        // 获取该仓库的技能
        let mut skills = self.fetch_repo_skills(repo).await?;

        // 只更新已安装状态，不添加本地独有的技能
        self.update_installed_status(&mut skills)?;

        // 去重并排序
        Self::deduplicate_skills(&mut skills);
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(skills)
    }

    /// 列出所有技能
    pub async fn list_skills(&self, repos: Vec<SkillRepo>) -> Result<Vec<Skill>> {
        let mut skills = Vec::new();

        // 仅使用启用的仓库，并行获取技能列表，避免单个无效仓库拖慢整体刷新
        let enabled_repos: Vec<SkillRepo> = repos.into_iter().filter(|repo| repo.enabled).collect();

        let fetch_tasks = enabled_repos
            .iter()
            .map(|repo| self.fetch_repo_skills(repo));

        let results: Vec<Result<Vec<Skill>>> = futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results.into_iter()) {
            match result {
                Ok(repo_skills) => skills.extend(repo_skills),
                Err(e) => log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e),
            }
        }

        // 合并本地技能
        self.merge_local_skills(&mut skills)?;

        // 去重并排序
        Self::deduplicate_skills(&mut skills);
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Ok(skills)
    }

    /// 从仓库获取技能列表
    async fn fetch_repo_skills(&self, repo: &SkillRepo) -> Result<Vec<Skill>> {
        log::info!("开始获取仓库技能: {}/{}, branch: {}, is_private: {}", 
            repo.owner, repo.name, repo.branch, repo.access_token.is_some());
        
        // 为单个仓库加载增加整体超时，避免无效链接长时间阻塞
        let temp_dir = timeout(std::time::Duration::from_secs(60), self.download_repo(repo))
            .await
            .map_err(|_| {
                log::error!("下载仓库超时: {}/{}", repo.owner, repo.name);
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
        
        log::info!("仓库下载成功: {}/{}, 临时目录: {:?}", repo.owner, repo.name, temp_dir);
        
        let mut skills = Vec::new();

        // 扫描仓库根目录（支持全仓库递归扫描）
        let scan_dir = temp_dir.clone();

        // 递归扫描目录查找所有技能
        self.scan_dir_recursive(&scan_dir, &scan_dir, repo, &mut skills)?;
        
        log::info!("仓库 {}/{} 扫描完成，发现 {} 个技能", repo.owner, repo.name, skills.len());

        // 清理临时目录
        let _ = fs::remove_dir_all(&temp_dir);

        Ok(skills)
    }

    /// 递归扫描目录查找 SKILL.md
    ///
    /// 规则：
    /// 1. 如果当前目录存在 SKILL.md，则识别为技能，停止扫描其子目录（子目录视为功能文件夹）
    /// 2. 如果当前目录不存在 SKILL.md，则递归扫描所有子目录
    fn scan_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &SkillRepo,
        skills: &mut Vec<Skill>,
    ) -> Result<()> {
        // 检查当前目录是否包含 SKILL.md
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            log::debug!("发现 SKILL.md: {:?}", skill_md);
            
            // 发现技能！获取相对路径作为目录名
            let directory = if current_dir == base_dir {
                // 根目录的 SKILL.md，使用仓库名
                repo.name.clone()
            } else {
                // 子目录的 SKILL.md，使用相对路径
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .to_string()
            };

            if let Ok(skill) = self.build_skill_from_metadata(&skill_md, &directory, repo) {
                log::info!("解析技能成功: {} ({})", skill.name, skill.directory);
                skills.push(skill);
            } else {
                log::warn!("解析技能元数据失败: {:?}", skill_md);
            }

            // 停止扫描此目录的子目录（同级目录都是功能文件夹）
            return Ok(());
        }

        // 未发现 SKILL.md，继续递归扫描所有子目录
        let entries: Vec<_> = fs::read_dir(current_dir)?
            .filter_map(|e| e.ok())
            .collect();
        
        log::debug!("扫描目录 {:?}, 子项数量: {}", current_dir, entries.len());
        
        for entry in entries {
            let path = entry.path();

            // 只处理目录
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
    ) -> Result<Skill> {
        let meta = self.parse_skill_metadata(skill_md)?;

        // 构建 README URL
        let readme_path = directory.to_string();

        Ok(Skill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Some(format!(
                "https://github.com/{}/{}/tree/{}/{}",
                repo.owner, repo.name, repo.branch, readme_path
            )),
            installed: false,
            repo_owner: Some(repo.owner.clone()),
            repo_name: Some(repo.name.clone()),
            repo_branch: Some(repo.branch.clone()),
        })
    }

    /// 解析技能元数据
    fn parse_skill_metadata(&self, path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(path)?;

        // 移除 BOM
        let content = content.trim_start_matches('\u{feff}');

        // 提取 YAML front matter
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

    /// 合并本地技能
    fn merge_local_skills(&self, skills: &mut Vec<Skill>) -> Result<()> {
        if !self.install_dir.exists() {
            return Ok(());
        }

        // 收集所有本地技能
        let mut local_skills = Vec::new();
        self.scan_local_dir_recursive(&self.install_dir, &self.install_dir, &mut local_skills)?;

        // 处理找到的本地技能
        for local_skill in local_skills {
            let directory = &local_skill.directory;

            // 更新已安装状态（匹配远程技能）
            // 使用目录最后一段进行比较，因为安装时只使用最后一段作为目录名
            let mut found = false;
            let local_install_name = Path::new(directory)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| directory.clone());

            for skill in skills.iter_mut() {
                let remote_install_name = Path::new(&skill.directory)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| skill.directory.clone());

                if remote_install_name.eq_ignore_ascii_case(&local_install_name) {
                    skill.installed = true;
                    found = true;
                    break;
                }
            }

            // 添加本地独有的技能（仅当在仓库中未找到时）
            if !found {
                skills.push(local_skill);
            }
        }

        Ok(())
    }

    /// 只更新已安装状态，不添加本地独有的技能
    /// 用于单个仓库加载时，避免本地技能被重复添加到每个仓库
    fn update_installed_status(&self, skills: &mut Vec<Skill>) -> Result<()> {
        if !self.install_dir.exists() {
            return Ok(());
        }

        // 收集所有本地技能
        let mut local_skills = Vec::new();
        self.scan_local_dir_recursive(&self.install_dir, &self.install_dir, &mut local_skills)?;

        // 只更新已安装状态，不添加本地独有的技能
        for local_skill in local_skills {
            let directory = &local_skill.directory;

            // 使用目录最后一段进行比较，因为安装时只使用最后一段作为目录名
            let local_install_name = Path::new(directory)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| directory.clone());

            for skill in skills.iter_mut() {
                let remote_install_name = Path::new(&skill.directory)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| skill.directory.clone());

                if remote_install_name.eq_ignore_ascii_case(&local_install_name) {
                    skill.installed = true;
                    break;
                }
            }
        }

        Ok(())
    }

    /// 获取本地独有的技能列表
    /// 
    /// 返回所有本地安装的技能中，不属于任何远程仓库的技能。
    /// 用于渐进式加载时单独显示本地技能。
    pub fn list_local_skills(&self, remote_skills: &[Skill]) -> Result<Vec<Skill>> {
        if !self.install_dir.exists() {
            return Ok(Vec::new());
        }

        // 收集所有本地技能
        let mut local_skills = Vec::new();
        self.scan_local_dir_recursive(&self.install_dir, &self.install_dir, &mut local_skills)?;

        // 过滤出本地独有的技能（不在远程仓库中的）
        let local_only_skills: Vec<Skill> = local_skills
            .into_iter()
            .filter(|local_skill| {
                let local_install_name = Path::new(&local_skill.directory)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| local_skill.directory.clone());

                // 检查是否在远程技能中存在
                !remote_skills.iter().any(|remote_skill| {
                    let remote_install_name = Path::new(&remote_skill.directory)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| remote_skill.directory.clone());

                    remote_install_name.eq_ignore_ascii_case(&local_install_name)
                })
            })
            .collect();

        Ok(local_only_skills)
    }

    /// 递归扫描本地目录查找 SKILL.md
    fn scan_local_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        skills: &mut Vec<Skill>,
    ) -> Result<()> {
        // 检查当前目录是否包含 SKILL.md
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            // 发现技能！获取相对路径作为目录名
            let directory = if current_dir == base_dir {
                // 如果是 install_dir 本身，使用最后一段路径名
                current_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            } else {
                // 使用相对于 install_dir 的路径
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .to_string()
            };

            // 解析元数据并创建本地技能对象
            if let Ok(meta) = self.parse_skill_metadata(&skill_md) {
                skills.push(Skill {
                    key: format!("local:{directory}"),
                    name: meta.name.unwrap_or_else(|| directory.clone()),
                    description: meta.description.unwrap_or_default(),
                    directory: directory.clone(),
                    readme_url: None,
                    installed: true,
                    repo_owner: None,
                    repo_name: None,
                    repo_branch: None,
                });
            }

            // 停止扫描此目录的子目录（同级目录都是功能文件夹）
            return Ok(());
        }

        // 未发现 SKILL.md，继续递归扫描所有子目录
        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            // 只处理目录
            if path.is_dir() {
                self.scan_local_dir_recursive(&path, base_dir, skills)?;
            }
        }

        Ok(())
    }

    /// 去重技能列表
    /// 使用完整的 key (owner/name:directory) 来区分不同仓库的同名技能
    fn deduplicate_skills(skills: &mut Vec<Skill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            // 使用完整 key 而非仅 directory，允许不同仓库的同名技能共存
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
    /// 根据 access_token 是否存在选择下载策略：
    /// - 公共仓库（access_token 为空）：无认证，直接下载
    /// - 私有仓库（access_token 有值）：尝试多种认证头下载
    async fn download_repo(&self, repo: &SkillRepo) -> Result<PathBuf> {
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_path_buf();
        let _ = temp_dir.keep(); // 保持临时目录，稍后手动清理

        // 尝试多个分支
        let branches = if repo.branch.is_empty() {
            vec!["main", "master"]
        } else {
            vec![repo.branch.as_str(), "main", "master"]
        };

        // 根据 access_token 是否存在判断仓库类型
        let is_private = repo.access_token.is_some();
        
        log::info!("下载仓库 {}/{}, is_private: {}, base_url: {:?}", 
            repo.owner, repo.name, is_private, repo.base_url);

        // 构建认证头列表（仅私有仓库需要）
        let auth_headers = if is_private {
            self.build_auth_headers(repo)
        } else {
            vec![]
        };

        let mut last_error = None;
        for branch in &branches {
            // 构建下载 URL 列表（主 URL + 备用 URL）
            let urls = if is_private {
                let base_url = repo.base_url.as_deref().unwrap_or("https://github.com");
                let primary = self.build_private_repo_download_url(base_url, &repo.owner, &repo.name, branch);
                let fallbacks = self.build_fallback_download_urls(base_url, &repo.owner, &repo.name, branch);
                std::iter::once(primary).chain(fallbacks).collect::<Vec<_>>()
            } else {
                // 公共仓库：使用 GitHub URL
                vec![format!(
                    "https://github.com/{}/{}/archive/refs/heads/{}.zip",
                    repo.owner, repo.name, branch
                )]
            };

            // 尝试所有 URL 格式
            for url in &urls {
                // 对于私有仓库，尝试所有认证头
                if is_private {
                    for (header_name, header_value) in &auth_headers {
                        log::info!("尝试下载: {} (branch: {}, auth: {})", url, branch, header_name);

                        match self.download_and_extract(url, &temp_path, Some(&(header_name.clone(), header_value.clone()))).await {
                            Ok(_) => {
                                log::info!("下载成功: {} (auth: {})", url, header_name);
                                return Ok(temp_path);
                            }
                            Err(e) => {
                                log::warn!("下载失败: {} (auth: {}) - {}", url, header_name, e);
                                last_error = Some(e);
                                continue;
                            }
                        }
                    }
                } else {
                    // 公共仓库，无需认证
                    log::info!("尝试下载: {} (branch: {})", url, branch);

                    match self.download_and_extract(url, &temp_path, None).await {
                        Ok(_) => {
                            log::info!("下载成功: {}", url);
                            return Ok(temp_path);
                        }
                        Err(e) => {
                            log::warn!("下载失败: {} - {}", url, e);
                            last_error = Some(e);
                            continue;
                        }
                    }
                }
            }
        }

        log::error!("所有分支和格式下载失败: {}/{}, 尝试的分支: {:?}", repo.owner, repo.name, branches);
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("所有分支下载失败")))
    }

    /// 构建私有仓库下载 URL
    /// 支持 GitHub、GitLab、Gitea 等平台
    /// 
    /// GitLab 需要使用 API 方式下载，Web URL 不支持 Token 认证
    fn build_private_repo_download_url(&self, base_url: &str, owner: &str, name: &str, branch: &str) -> String {
        let base_url = base_url.trim_end_matches('/');
        
        // 只有明确是 github.com 时使用 GitHub 格式
        if base_url.contains("github.com") {
            // GitHub 格式: {base_url}/{owner}/{name}/archive/refs/heads/{branch}.zip
            format!("{}/{}/{}/archive/refs/heads/{}.zip", base_url, owner, name, branch)
        } else {
            // GitLab API 格式（支持 Token 认证）
            // 格式: {base_url}/api/v4/projects/{owner}%2F{name}/repository/archive.zip?sha={branch}
            let encoded_path = format!("{}%2F{}", owner, name);
            format!("{}/api/v4/projects/{}/repository/archive.zip?sha={}", base_url, encoded_path, branch)
        }
    }
    
    /// 构建备用下载 URL（当主 URL 失败时尝试）
    /// 返回其他平台格式的 URL 列表
    fn build_fallback_download_urls(&self, base_url: &str, owner: &str, name: &str, branch: &str) -> Vec<String> {
        let base_url = base_url.trim_end_matches('/');
        
        if base_url.contains("github.com") {
            // GitHub 没有备用格式
            vec![]
        } else {
            // 私有仓库的备用格式：Gitea 格式
            vec![
                // Gitea 格式: {base_url}/{owner}/{name}/archive/{branch}.zip
                format!("{}/{}/{}/archive/{}.zip", base_url, owner, name, branch),
            ]
        }
    }

    /// 构建认证头
    /// 根据存储的 auth_header 和 access_token 构建完整的认证头
    /// 返回多个可能的认证头，按优先级排序
    fn build_auth_headers(&self, repo: &SkillRepo) -> Vec<(String, String)> {
        let token = match repo.access_token.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        
        let base_url = repo.base_url.as_deref().unwrap_or("");
        
        // 根据平台返回不同的认证头列表
        if base_url.contains("github.com") {
            // GitHub 只需要 Authorization
            vec![
                ("Authorization".to_string(), format!("token {}", token)),
            ]
        } else {
            // 其他平台（GitLab、Gitea 等）尝试多种认证头
            // GitLab 下载 ZIP 需要 PRIVATE-TOKEN
            vec![
                ("PRIVATE-TOKEN".to_string(), token.clone()),           // GitLab
                ("Authorization".to_string(), format!("token {}", token)), // Gitea/GitHub
                ("Authorization".to_string(), format!("Bearer {}", token)), // 通用
            ]
        }
    }

    /// 下载并解压 ZIP
    /// auth_header: 可选的认证头，格式为 (header_name, header_value)
    async fn download_and_extract(&self, url: &str, dest: &Path, auth_header: Option<&(String, String)>) -> Result<()> {
        // 构建请求
        let mut request = self.http_client.get(url);
        
        // 如果提供了认证头，添加到请求中
        if let Some((header_name, header_value)) = auth_header {
            request = request.header(header_name.as_str(), header_value.as_str());
        }
        
        // 下载 ZIP
        let response = request.send().await?;
        let status = response.status();
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        
        log::info!("响应状态: {}, Content-Type: {}", status, content_type);
        
        if !status.is_success() {
            let status_code = status.as_u16().to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "DOWNLOAD_FAILED",
                &[("status", &status_code)],
                match status_code.as_str() {
                    "403" => Some("http403"),
                    "404" => Some("http404"),
                    "429" => Some("http429"),
                    _ => Some("checkNetwork"),
                },
            )));
        }

        let bytes = response.bytes().await?;
        
        // 检查响应内容
        log::info!("响应大小: {} bytes", bytes.len());
        if bytes.len() < 100 {
            log::warn!("响应内容过小，可能不是有效的 ZIP 文件");
        }
        
        // 检查是否是 HTML（可能是登录页面或错误页面）
        if bytes.starts_with(b"<!DOCTYPE") || bytes.starts_with(b"<html") || bytes.starts_with(b"<HTML") {
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(500)]);
            log::error!("收到 HTML 响应而非 ZIP 文件: {}", preview);
            return Err(anyhow::anyhow!("服务器返回 HTML 页面而非 ZIP 文件，可能需要重新认证"));
        }

        // 解压
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;

        // 获取根目录名称 (GitHub 的 zip 会有一个根目录)
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

        // 解压所有文件
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = file.name();

            // 跳过根目录，直接提取内容
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

    /// 安装技能（仅负责下载和文件操作，状态更新由上层负责）
    pub async fn install_skill(&self, directory: String, repo: SkillRepo) -> Result<()> {
        // 使用技能目录的最后一段作为安装目录名，避免嵌套路径问题
        // 例如: "skills/codex" -> "codex"
        let install_name = Path::new(&directory)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| directory.clone());

        let dest = self.install_dir.join(&install_name);

        // 若目标目录已存在，则视为已安装，避免重复下载
        if dest.exists() {
            return Ok(());
        }

        // 下载仓库时增加总超时，防止无效链接导致长时间卡住安装过程
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

        // 确定源目录路径（技能相对于仓库根目录的路径）
        let source = temp_dir.join(&directory);

        if !source.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(anyhow::anyhow!(format_skill_error(
                "SKILL_DIR_NOT_FOUND",
                &[("path", &source.display().to_string())],
                Some("checkRepoUrl"),
            )));
        }

        // 删除旧版本
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }

        // 递归复制
        Self::copy_dir_recursive(&source, &dest)?;

        // 清理临时目录
        let _ = fs::remove_dir_all(&temp_dir);

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

    /// 卸载技能（仅负责文件操作，状态更新由上层负责）
    pub fn uninstall_skill(&self, directory: String) -> Result<()> {
        // 使用技能目录的最后一段作为安装目录名，与 install_skill 保持一致
        let install_name = Path::new(&directory)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| directory.clone());

        let dest = self.install_dir.join(&install_name);

        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }

        Ok(())
    }

    /// 列出仓库
    pub fn list_repos(&self, store: &SkillStore) -> Vec<SkillRepo> {
        store.repos.clone()
    }

    /// 添加仓库
    pub fn add_repo(&self, store: &mut SkillStore, repo: SkillRepo) -> Result<()> {
        // 检查重复
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

    /// 切换仓库的启用状态
    pub fn toggle_repo_enabled(
        &self,
        store: &mut SkillStore,
        owner: String,
        name: String,
        enabled: bool,
    ) -> Result<()> {
        if let Some(repo) = store
            .repos
            .iter_mut()
            .find(|r| r.owner == owner && r.name == name)
        {
            repo.enabled = enabled;
            Ok(())
        } else {
            Err(anyhow!(format_skill_error(
                "REPO_NOT_FOUND",
                &[("owner", &owner), ("name", &name)],
                Some("checkRepoConfig"),
            )))
        }
    }
}
