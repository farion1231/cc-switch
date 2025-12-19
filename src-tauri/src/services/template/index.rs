use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::time::timeout;

use super::{ComponentMetadata, ComponentType, TemplateComponent, TemplateRepo, TemplateService};

impl TemplateService {
    /// 刷新所有启用仓库的组件索引
    pub async fn refresh_index(&self, conn: &Connection) -> Result<()> {
        // 获取所有启用的仓库
        let repos = self.list_enabled_repos(conn)?;

        if repos.is_empty() {
            log::info!("没有启用的模板仓库");
            return Ok(());
        }

        log::info!("开始刷新 {} 个模板仓库", repos.len());

        // 并行扫描所有仓库
        let scan_tasks = repos.iter().map(|repo| self.scan_repo(repo));
        let results: Vec<Result<Vec<TemplateComponent>>> =
            futures::future::join_all(scan_tasks).await;

        // 处理扫描结果
        let mut total_components = 0;
        for (repo, result) in repos.iter().zip(results.into_iter()) {
            match result {
                Ok(components) => {
                    log::info!(
                        "仓库 {}/{} 扫描到 {} 个组件",
                        repo.owner,
                        repo.name,
                        components.len()
                    );

                    // 保存到数据库
                    if let Err(e) = self.save_components(conn, &components) {
                        log::error!("保存组件到数据库失败: {e}");
                    } else {
                        total_components += components.len();
                    }
                }
                Err(e) => {
                    log::warn!("扫描仓库 {}/{} 失败: {}", repo.owner, repo.name, e);
                }
            }
        }

        log::info!("刷新完成，共索引 {total_components} 个组件");
        Ok(())
    }

    /// 扫描单个仓库
    pub async fn scan_repo(&self, repo: &TemplateRepo) -> Result<Vec<TemplateComponent>> {
        log::info!("开始扫描仓库: {}/{}", repo.owner, repo.name);

        // 下载仓库（增加超时控制）
        let temp_dir = timeout(
            std::time::Duration::from_secs(120),
            self.download_repo(repo),
        )
        .await
        .map_err(|_| anyhow!("下载仓库超时: {}/{}", repo.owner, repo.name))??;

        let mut components = Vec::new();

        // 扫描不同类型的组件
        self.scan_agents(&temp_dir, repo, &mut components)?;
        self.scan_commands(&temp_dir, repo, &mut components)?;
        self.scan_mcps(&temp_dir, repo, &mut components)?;
        self.scan_settings(&temp_dir, repo, &mut components)?;
        self.scan_hooks(&temp_dir, repo, &mut components)?;
        self.scan_skills(&temp_dir, repo, &mut components)?;

        // 清理临时目录
        let _ = fs::remove_dir_all(&temp_dir);

        log::info!(
            "仓库 {}/{} 扫描完成，找到 {} 个组件",
            repo.owner,
            repo.name,
            components.len()
        );

        Ok(components)
    }

    /// 下载仓库 ZIP
    async fn download_repo(&self, repo: &TemplateRepo) -> Result<PathBuf> {
        let temp_dir = tempfile::tempdir().context("创建临时目录失败")?;
        let temp_path = temp_dir.path().to_path_buf();
        let _ = temp_dir.keep();

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

            log::debug!("尝试下载: {url}");
            match self.download_and_extract(&url, &temp_path).await {
                Ok(_) => {
                    log::info!("成功下载仓库: {}/{} ({})", repo.owner, repo.name, branch);
                    return Ok(temp_path);
                }
                Err(e) => {
                    log::debug!("下载分支 {branch} 失败: {e}");
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("所有分支下载失败")))
    }

    /// 下载并解压 ZIP
    async fn download_and_extract(&self, url: &str, dest: &Path) -> Result<()> {
        // 下载 ZIP
        let response = self.client().get(url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("下载失败: HTTP {}", response.status());
        }

        let bytes = response.bytes().await?;

        // 解压
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;

        // 获取根目录名称
        let root_name = if !archive.is_empty() {
            let first_file = archive.by_index(0)?;
            let name = first_file.name();
            name.split('/').next().unwrap_or("").to_string()
        } else {
            return Err(anyhow!("空的压缩包"));
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

    /// 扫描 Agents
    fn scan_agents(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        // 尝试多个可能的路径
        let paths = [
            base_dir.join("cli-tool").join("components").join("agents"),
            base_dir.join("src").join("agents"),
            base_dir.join("components").join("agents"),
        ];
        for agents_dir in paths {
            if agents_dir.exists() {
                self.scan_markdown_components(
                    &agents_dir,
                    base_dir,
                    ComponentType::Agent,
                    repo,
                    components,
                )?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 Commands
    fn scan_commands(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let paths = [
            base_dir
                .join("cli-tool")
                .join("components")
                .join("commands"),
            base_dir.join("src").join("commands"),
            base_dir.join("components").join("commands"),
        ];
        for commands_dir in paths {
            if commands_dir.exists() {
                self.scan_markdown_components(
                    &commands_dir,
                    base_dir,
                    ComponentType::Command,
                    repo,
                    components,
                )?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 MCPs
    fn scan_mcps(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let paths = [
            base_dir.join("cli-tool").join("components").join("mcps"),
            base_dir.join("src").join("mcp"),
            base_dir.join("components").join("mcps"),
        ];
        for mcps_dir in paths {
            if mcps_dir.exists() {
                self.scan_json_components(
                    &mcps_dir,
                    base_dir,
                    ComponentType::Mcp,
                    repo,
                    components,
                )?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 Settings
    fn scan_settings(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let paths = [
            base_dir
                .join("cli-tool")
                .join("components")
                .join("settings"),
            base_dir.join("src").join("settings"),
            base_dir.join("components").join("settings"),
        ];
        for settings_dir in paths {
            if settings_dir.exists() {
                self.scan_json_components(
                    &settings_dir,
                    base_dir,
                    ComponentType::Setting,
                    repo,
                    components,
                )?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 Hooks
    fn scan_hooks(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let paths = [
            base_dir.join("cli-tool").join("components").join("hooks"),
            base_dir.join("src").join("hooks"),
            base_dir.join("components").join("hooks"),
        ];
        for hooks_dir in paths {
            if hooks_dir.exists() {
                self.scan_json_components(
                    &hooks_dir,
                    base_dir,
                    ComponentType::Hook,
                    repo,
                    components,
                )?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 Skills
    fn scan_skills(
        &self,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let paths = [
            base_dir.join("cli-tool").join("components").join("skills"),
            base_dir.join("src").join("skills"),
            base_dir.join("components").join("skills"),
        ];
        for skills_dir in paths {
            if skills_dir.exists() {
                self.scan_skills_recursive(&skills_dir, base_dir, repo, components)?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// 扫描 Markdown 组件（Agent/Command）
    fn scan_markdown_components(
        &self,
        dir: &Path,
        base_dir: &Path,
        component_type: ComponentType,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Ok(component) =
                    self.parse_markdown_component(&path, base_dir, component_type.clone(), repo)
                {
                    components.push(component);
                }
            } else if path.is_dir() {
                // 递归扫描子目录（用于分类）
                self.scan_markdown_components(
                    &path,
                    base_dir,
                    component_type.clone(),
                    repo,
                    components,
                )?;
            }
        }

        Ok(())
    }

    /// 扫描 JSON 组件（MCP/Setting/Hook）
    fn scan_json_components(
        &self,
        dir: &Path,
        base_dir: &Path,
        component_type: ComponentType,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(component) =
                    self.parse_json_component(&path, base_dir, component_type.clone(), repo)
                {
                    components.push(component);
                }
            } else if path.is_dir() {
                // 递归扫描子目录（用于分类）
                self.scan_json_components(
                    &path,
                    base_dir,
                    component_type.clone(),
                    repo,
                    components,
                )?;
            }
        }

        Ok(())
    }

    /// 递归扫描技能目录
    fn scan_skills_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &TemplateRepo,
        components: &mut Vec<TemplateComponent>,
    ) -> Result<()> {
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            // 发现技能
            if let Ok(component) = self.parse_skill_component(&skill_md, base_dir, repo) {
                components.push(component);
            }
            return Ok(());
        }

        // 继续递归扫描子目录
        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.scan_skills_recursive(&path, base_dir, repo, components)?;
            }
        }

        Ok(())
    }

    /// 解析 Markdown 组件元数据
    fn parse_markdown_component(
        &self,
        path: &Path,
        base_dir: &Path,
        component_type: ComponentType,
        repo: &TemplateRepo,
    ) -> Result<TemplateComponent> {
        let content = fs::read_to_string(path)?;
        let meta = self.parse_component_metadata(&content)?;

        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // 提取分类（从目录结构）
        let category = self.extract_category(path, &format!("src/{}", component_type.as_str()));

        // 计算相对于仓库根目录的路径
        let relative_path = path
            .strip_prefix(base_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        Ok(TemplateComponent {
            id: None,
            repo_id: repo.id.unwrap_or(0),
            component_type,
            category,
            name: meta.name.unwrap_or_else(|| file_name.to_string()),
            path: relative_path,
            description: meta.description,
            content_hash: Some(Self::calculate_hash(&content)),
            installed: false,
        })
    }

    /// 解析 JSON 组件元数据
    fn parse_json_component(
        &self,
        path: &Path,
        base_dir: &Path,
        component_type: ComponentType,
        repo: &TemplateRepo,
    ) -> Result<TemplateComponent> {
        let content = fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(file_name)
            .to_string();

        let description = json
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let category = self.extract_category(path, &format!("src/{}", component_type.as_str()));

        // 计算相对于仓库根目录的路径
        let relative_path = path
            .strip_prefix(base_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        Ok(TemplateComponent {
            id: None,
            repo_id: repo.id.unwrap_or(0),
            component_type,
            category,
            name,
            path: relative_path,
            description,
            content_hash: Some(Self::calculate_hash(&content)),
            installed: false,
        })
    }

    /// 解析技能组件
    fn parse_skill_component(
        &self,
        skill_md: &Path,
        base_dir: &Path,
        repo: &TemplateRepo,
    ) -> Result<TemplateComponent> {
        let content = fs::read_to_string(skill_md)?;
        let meta = self.parse_component_metadata(&content)?;

        let skill_dir = skill_md.parent().unwrap();
        let directory = skill_dir
            .strip_prefix(base_dir)
            .unwrap_or(skill_dir)
            .to_string_lossy()
            .to_string();

        Ok(TemplateComponent {
            id: None,
            repo_id: repo.id.unwrap_or(0),
            component_type: ComponentType::Skill,
            category: None,
            name: meta.name.unwrap_or_else(|| directory.clone()),
            path: directory,
            description: meta.description,
            content_hash: Some(Self::calculate_hash(&content)),
            installed: false,
        })
    }

    /// 解析组件元数据（从 front matter）
    pub fn parse_component_metadata(&self, content: &str) -> Result<ComponentMetadata> {
        // 移除 BOM
        let content = content.trim_start_matches('\u{feff}');

        // 提取 YAML front matter
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Ok(ComponentMetadata {
                name: None,
                description: None,
                tools: None,
                model: None,
            });
        }

        let front_matter = parts[1].trim();
        let meta: ComponentMetadata =
            serde_yaml::from_str(front_matter).unwrap_or(ComponentMetadata {
                name: None,
                description: None,
                tools: None,
                model: None,
            });

        Ok(meta)
    }

    /// 提取分类（从路径）
    fn extract_category(&self, path: &Path, base: &str) -> Option<String> {
        let path_str = path.to_string_lossy();
        if let Some(pos) = path_str.find(base) {
            let after_base = &path_str[pos + base.len()..];
            let parts: Vec<&str> = after_base.split('/').filter(|s| !s.is_empty()).collect();
            if parts.len() > 1 {
                return Some(parts[0].to_string());
            }
        }
        None
    }

    /// 计算内容哈希
    fn calculate_hash(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 保存组件到数据库
    fn save_components(&self, conn: &Connection, components: &[TemplateComponent]) -> Result<()> {
        for component in components {
            // 检查是否已存在（通过 repo_id + component_type + path）
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM template_components
                     WHERE repo_id = ?1 AND component_type = ?2 AND path = ?3",
                    params![
                        component.repo_id,
                        component.component_type.as_str(),
                        &component.path
                    ],
                    |row| row.get(0),
                )
                .ok();

            if let Some(id) = existing {
                // 更新现有组件
                conn.execute(
                    "UPDATE template_components
                     SET category = ?1, name = ?2, description = ?3, content_hash = ?4, updated_at = CURRENT_TIMESTAMP
                     WHERE id = ?5",
                    params![
                        &component.category,
                        &component.name,
                        &component.description,
                        &component.content_hash,
                        id
                    ],
                )?;
            } else {
                // 插入新组件
                conn.execute(
                    "INSERT INTO template_components (repo_id, component_type, category, name, path, description, content_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        component.repo_id,
                        component.component_type.as_str(),
                        &component.category,
                        &component.name,
                        &component.path,
                        &component.description,
                        &component.content_hash
                    ],
                )?;
            }
        }

        Ok(())
    }

    /// 列出组件（支持过滤和分页）
    #[allow(dead_code)]
    pub fn list_components(
        &self,
        conn: &Connection,
        component_type: Option<ComponentType>,
        category: Option<String>,
        search: Option<String>,
        page: u32,
        page_size: u32,
    ) -> Result<super::PaginatedResult<TemplateComponent>> {
        let mut where_clauses = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ct) = &component_type {
            where_clauses.push("component_type = ?");
            params.push(Box::new(ct.as_str().to_string()));
        }

        if let Some(cat) = &category {
            where_clauses.push("category = ?");
            params.push(Box::new(cat.clone()));
        }

        if let Some(s) = &search {
            where_clauses.push("(name LIKE ? OR description LIKE ?)");
            let search_pattern = format!("%{s}%");
            params.push(Box::new(search_pattern.clone()));
            params.push(Box::new(search_pattern));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        // 获取总数
        let count_sql = format!("SELECT COUNT(*) FROM template_components {where_sql}");
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let total: i64 = conn.query_row(&count_sql, param_refs.as_slice(), |row| row.get(0))?;

        // 获取分页数据
        let offset = (page - 1) * page_size;
        let query_sql = format!(
            "SELECT id, repo_id, component_type, category, name, path, description, content_hash
             FROM template_components
             {where_sql}
             ORDER BY name
             LIMIT ? OFFSET ?"
        );

        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&query_sql)?;
        let components = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(TemplateComponent {
                    id: Some(row.get(0)?),
                    repo_id: row.get(1)?,
                    component_type: ComponentType::from_str(&row.get::<_, String>(2)?)
                        .unwrap_or(ComponentType::Agent),
                    category: row.get(3)?,
                    name: row.get(4)?,
                    path: row.get(5)?,
                    description: row.get(6)?,
                    content_hash: row.get(7)?,
                    installed: false,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(super::PaginatedResult {
            items: components,
            total,
            page,
            page_size,
        })
    }
}
