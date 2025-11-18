use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::time::timeout;

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
    /// 技能所在的子目录路径 (可选, 如 "skills", "my-skills/subdir")
    #[serde(rename = "skillsPath")]
    pub skills_path: Option<String>,
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
                    skills_path: None, // 扫描根目录
                },
                SkillRepo {
                    owner: "anthropics".to_string(),
                    name: "skills".to_string(),
                    branch: "main".to_string(),
                    enabled: true,
                    skills_path: None, // 扫描根目录
                },
                SkillRepo {
                    owner: "cexll".to_string(),
                    name: "myclaude".to_string(),
                    branch: "master".to_string(),
                    enabled: true,
                    skills_path: Some("skills".to_string()), // 扫描 skills 子目录
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
}

impl SkillService {
    pub fn new() -> Result<Self> {
        let install_dir = Self::get_install_dir()?;

        // 确保目录存在
        fs::create_dir_all(&install_dir)?;

        Ok(Self {
            http_client: Client::builder()
                .user_agent("cc-switch")
                // 将单次请求超时时间控制在 10 秒以内，避免无效链接导致长时间卡住
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            install_dir,
        })
    }

    fn get_install_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("无法获取用户主目录")?;
        Ok(home.join(".claude").join("skills"))
    }
}

// 核心方法实现
impl SkillService {
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
        // 为单个仓库加载增加整体超时，避免无效链接长时间阻塞
        let temp_dir = timeout(std::time::Duration::from_secs(15), self.download_repo(repo))
            .await
            .map_err(|_| anyhow!("下载仓库 {}/{} 超时", repo.owner, repo.name))??;
        let mut skills = Vec::new();

        // 确定要扫描的目录路径
        let scan_dir = if let Some(ref skills_path) = repo.skills_path {
            // 如果指定了 skillsPath，则扫描该子目录
            let subdir = temp_dir.join(skills_path.trim_matches('/'));
            if !subdir.exists() {
                log::warn!(
                    "仓库 {}/{} 中指定的技能路径 '{}' 不存在",
                    repo.owner,
                    repo.name,
                    skills_path
                );
                let _ = fs::remove_dir_all(&temp_dir);
                return Ok(skills);
            }
            subdir
        } else {
            // 否则扫描仓库根目录
            temp_dir.clone()
        };

        // 遍历目标目录
        for entry in fs::read_dir(&scan_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            // 解析技能元数据
            match self.parse_skill_metadata(&skill_md) {
                Ok(meta) => {
                    let directory = path.file_name().unwrap().to_string_lossy().to_string();

                    // 构建 README URL（考虑 skillsPath）
                    let readme_path = if let Some(ref skills_path) = repo.skills_path {
                        format!("{}/{}", skills_path.trim_matches('/'), directory)
                    } else {
                        directory.clone()
                    };

                    skills.push(Skill {
                        key: format!("{}/{}:{}", repo.owner, repo.name, directory),
                        name: meta.name.unwrap_or_else(|| directory.clone()),
                        description: meta.description.unwrap_or_default(),
                        directory,
                        readme_url: Some(format!(
                            "https://github.com/{}/{}/tree/{}/{}",
                            repo.owner, repo.name, repo.branch, readme_path
                        )),
                        installed: false,
                        repo_owner: Some(repo.owner.clone()),
                        repo_name: Some(repo.name.clone()),
                        repo_branch: Some(repo.branch.clone()),
                    });
                }
                Err(e) => log::warn!("解析 {} 元数据失败: {}", skill_md.display(), e),
            }
        }

        // 清理临时目录
        let _ = fs::remove_dir_all(&temp_dir);

        Ok(skills)
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

        for entry in fs::read_dir(&self.install_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let directory = path.file_name().unwrap().to_string_lossy().to_string();

            // 更新已安装状态
            let mut found = false;
            for skill in skills.iter_mut() {
                if skill.directory.eq_ignore_ascii_case(&directory) {
                    skill.installed = true;
                    found = true;
                    break;
                }
            }

            // 添加本地独有的技能（仅当在仓库中未找到时）
            if !found {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
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
                }
            }
        }

        Ok(())
    }

    /// 去重技能列表
    fn deduplicate_skills(skills: &mut Vec<Skill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            let key = skill.directory.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
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
        let _ = temp_dir.keep(); // 保持临时目录，稍后手动清理

        // 尝试多个分支
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
        // 下载 ZIP
        let response = self.http_client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("下载失败: {}", response.status()));
        }

        let bytes = response.bytes().await?;

        // 解压
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;

        // 获取根目录名称 (GitHub 的 zip 会有一个根目录)
        let root_name = if !archive.is_empty() {
            let first_file = archive.by_index(0)?;
            let name = first_file.name();
            name.split('/').next().unwrap_or("").to_string()
        } else {
            return Err(anyhow::anyhow!("空的压缩包"));
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
        let dest = self.install_dir.join(&directory);

        // 若目标目录已存在，则视为已安装，避免重复下载
        if dest.exists() {
            return Ok(());
        }

        // 下载仓库时增加总超时，防止无效链接导致长时间卡住安装过程
        let temp_dir = timeout(
            std::time::Duration::from_secs(15),
            self.download_repo(&repo),
        )
        .await
        .map_err(|_| anyhow!("下载仓库 {}/{} 超时", repo.owner, repo.name))??;

        // 复制到安装目录
        let source = temp_dir.join(&directory);

        if !source.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(anyhow::anyhow!("技能目录不存在"));
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
        let dest = self.install_dir.join(&directory);

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
}
