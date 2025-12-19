use anyhow::Result;
use rusqlite::{params, Connection};
use std::fs;

use super::{
    BatchInstallResult, ComponentDetail, ComponentType, InstalledComponent, TemplateComponent,
    TemplateService,
};

impl TemplateService {
    /// 获取组件详情（含完整内容）
    pub async fn get_component(&self, conn: &Connection, id: i64) -> Result<ComponentDetail> {
        // 查询组件基本信息
        let component: TemplateComponent = conn.query_row(
            "SELECT id, repo_id, component_type, category, name, path, description, content_hash
             FROM template_components
             WHERE id = ?1",
            params![id],
            |row| {
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
            },
        )?;

        // 查询仓库信息
        let (repo_owner, repo_name, branch): (String, String, String) = conn.query_row(
            "SELECT owner, name, branch FROM template_repos WHERE id = ?1",
            params![component.repo_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        // 构建 README URL
        let readme_url = format!(
            "https://github.com/{}/{}/tree/{}/{}",
            repo_owner, repo_name, branch, component.path
        );

        // 下载并读取组件内容
        let content = self
            .download_component_content(&repo_owner, &repo_name, &branch, &component.path)
            .await?;

        Ok(ComponentDetail {
            component,
            content,
            repo_owner,
            repo_name,
            repo_branch: branch,
            readme_url,
        })
    }

    /// 下载组件内容
    async fn download_component_content(
        &self,
        owner: &str,
        name: &str,
        branch: &str,
        path: &str,
    ) -> Result<String> {
        let url = format!("https://raw.githubusercontent.com/{owner}/{name}/{branch}/{path}");

        let response = self.client().get(&url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("下载组件内容失败: HTTP {}", response.status());
        }

        let content = response.text().await?;
        Ok(content)
    }

    /// 安装组件到指定应用
    pub async fn install_component(
        &self,
        conn: &Connection,
        id: i64,
        app_type: &str,
    ) -> Result<()> {
        // 获取组件详情
        let detail = self.get_component(conn, id).await?;

        // 根据组件类型执行不同的安装逻辑
        match detail.component.component_type {
            ComponentType::Agent => {
                self.install_agent(&detail, app_type).await?;
            }
            ComponentType::Command => {
                self.install_command(&detail, app_type).await?;
            }
            ComponentType::Mcp => {
                self.install_mcp(&detail, app_type).await?;
            }
            ComponentType::Setting => {
                self.install_setting(&detail, app_type).await?;
            }
            ComponentType::Hook => {
                self.install_hook(&detail, app_type).await?;
            }
            ComponentType::Skill => {
                self.install_skill(&detail, app_type).await?;
            }
        }

        // 记录安装状态
        self.record_installation(conn, &detail.component, app_type)?;

        Ok(())
    }

    /// 安装 Agent
    async fn install_agent(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let agents_dir = config_dir.join("agents");
        fs::create_dir_all(&agents_dir)?;

        let file_name = format!("{}.md", detail.component.name);
        let dest_path = agents_dir.join(&file_name);

        fs::write(&dest_path, &detail.content)?;
        log::info!("Agent 已安装: {}", dest_path.display());

        Ok(())
    }

    /// 安装 Command
    async fn install_command(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let commands_dir = config_dir.join("commands");
        fs::create_dir_all(&commands_dir)?;

        let file_name = format!("{}.md", detail.component.name);
        let dest_path = commands_dir.join(&file_name);

        fs::write(&dest_path, &detail.content)?;
        log::info!("Command 已安装: {}", dest_path.display());

        Ok(())
    }

    /// 安装 MCP 服务器
    /// MCP 配置保存为独立 JSON 文件到 mcps/ 目录，不会修改原有 .mcp.json
    async fn install_mcp(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let mcps_dir = config_dir.join("mcps");
        fs::create_dir_all(&mcps_dir)?;

        // 保存为独立的 JSON 文件（保留原始格式，包含 mcpServers 结构）
        let file_name = format!("{}.json", detail.component.name);
        let dest_path = mcps_dir.join(&file_name);

        fs::write(&dest_path, &detail.content)?;
        log::info!(
            "MCP 配置已保存: {} (可手动合并到 .mcp.json)",
            dest_path.display()
        );

        Ok(())
    }

    /// 安装 Setting
    /// Setting 配置保存为独立 JSON 文件到 settings/ 目录，不会修改原有 settings.json
    /// 原始格式包含 permissions 等配置，可手动合并
    async fn install_setting(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let settings_dir = config_dir.join("settings");
        fs::create_dir_all(&settings_dir)?;

        // 保存为独立的 JSON 文件（保留原始格式，包含 permissions 等结构）
        let file_name = format!("{}.json", detail.component.name);
        let dest_path = settings_dir.join(&file_name);

        fs::write(&dest_path, &detail.content)?;
        log::info!(
            "Setting 配置已保存: {} (可手动合并到 settings.json)",
            dest_path.display()
        );

        Ok(())
    }

    /// 安装 Hook
    /// Hook 配置保存为独立 JSON 文件到 hooks/ 目录，不会修改原有 settings.json
    /// 原始格式包含 hooks 对象（如 PostToolUse 等），可手动合并
    async fn install_hook(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let hooks_dir = config_dir.join("hooks");
        fs::create_dir_all(&hooks_dir)?;

        // 保存为独立的 JSON 文件（保留原始格式，包含 hooks 结构）
        let file_name = format!("{}.json", detail.component.name);
        let dest_path = hooks_dir.join(&file_name);

        fs::write(&dest_path, &detail.content)?;
        log::info!(
            "Hook 配置已保存: {} (可手动合并到 settings.json 的 hooks 字段)",
            dest_path.display()
        );

        Ok(())
    }

    /// 安装 Skill
    /// Skill 是一个目录结构，包含 SKILL.md 和可能的子目录（如 reference/, scripts/）
    /// 使用 GitHub API 递归下载整个目录
    async fn install_skill(&self, detail: &ComponentDetail, app_type: &str) -> Result<()> {
        let config_dir = Self::get_app_config_dir(app_type)?;
        let skills_dir = config_dir.join("skills");
        fs::create_dir_all(&skills_dir)?;

        let skill_dir = skills_dir.join(&detail.component.name);
        fs::create_dir_all(&skill_dir)?;

        // 首先保存 SKILL.md（已下载的内容）
        let skill_md = skill_dir.join("SKILL.md");
        fs::write(&skill_md, &detail.content)?;

        // 尝试下载整个 skill 目录的其他文件
        // 构建 GitHub API URL 来获取目录内容
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            detail.repo_owner,
            detail.repo_name,
            detail.component.path.trim_end_matches("/SKILL.md")
        );

        // 递归下载目录内容
        if let Err(e) = self
            .download_skill_directory(&api_url, &skill_dir, &detail.repo_branch)
            .await
        {
            log::warn!("下载 Skill 附加文件失败: {e}，仅安装 SKILL.md");
        }

        log::info!("Skill 已安装: {}", skill_dir.display());
        Ok(())
    }

    /// 递归下载 Skill 目录内容
    async fn download_skill_directory(
        &self,
        api_url: &str,
        target_dir: &std::path::Path,
        branch: &str,
    ) -> Result<()> {
        let response = self
            .client()
            .get(api_url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "cc-switch")
            .query(&[("ref", branch)])
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("GitHub API 请求失败: {}", response.status());
        }

        let contents: Vec<serde_json::Value> = response.json().await?;

        for item in contents {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let item_name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");

            // 跳过 SKILL.md（已经下载）
            if item_name == "SKILL.md" {
                continue;
            }

            if item_type == "file" {
                // 下载文件
                if let Some(download_url) = item.get("download_url").and_then(|v| v.as_str()) {
                    let file_response = self.client().get(download_url).send().await?;
                    if file_response.status().is_success() {
                        let content = file_response.text().await?;
                        let file_path = target_dir.join(item_name);
                        fs::write(&file_path, &content)?;
                        log::debug!("下载文件: {}", file_path.display());
                    }
                }
            } else if item_type == "dir" {
                // 递归下载子目录
                if let Some(sub_url) = item.get("url").and_then(|v| v.as_str()) {
                    let sub_dir = target_dir.join(item_name);
                    fs::create_dir_all(&sub_dir)?;
                    // 递归调用，使用 Box::pin 处理异步递归
                    Box::pin(self.download_skill_directory(sub_url, &sub_dir, branch)).await?;
                }
            }
        }

        Ok(())
    }

    /// 记录安装状态
    fn record_installation(
        &self,
        conn: &Connection,
        component: &TemplateComponent,
        app_type: &str,
    ) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO installed_components (component_id, component_type, name, path, app_type)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                component.id,
                component.component_type.as_str(),
                &component.name,
                &component.path,
                app_type
            ],
        )?;

        Ok(())
    }

    /// 卸载组件
    pub fn uninstall_component(&self, conn: &Connection, id: i64, app_type: &str) -> Result<()> {
        // 查询组件信息
        let component: TemplateComponent = conn.query_row(
            "SELECT id, repo_id, component_type, category, name, path, description, content_hash
             FROM template_components
             WHERE id = ?1",
            params![id],
            |row| {
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
            },
        )?;

        // 删除文件
        match component.component_type {
            ComponentType::Agent => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let file_path = config_dir
                    .join("agents")
                    .join(format!("{}.md", component.name));
                if file_path.exists() {
                    fs::remove_file(&file_path)?;
                }
            }
            ComponentType::Command => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let file_path = config_dir
                    .join("commands")
                    .join(format!("{}.md", component.name));
                if file_path.exists() {
                    fs::remove_file(&file_path)?;
                }
            }
            ComponentType::Skill => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let skill_dir = config_dir.join("skills").join(&component.name);
                if skill_dir.exists() {
                    fs::remove_dir_all(&skill_dir)?;
                }
            }
            ComponentType::Mcp => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let file_path = config_dir
                    .join("mcps")
                    .join(format!("{}.json", component.name));
                if file_path.exists() {
                    fs::remove_file(&file_path)?;
                }
            }
            ComponentType::Setting => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let file_path = config_dir
                    .join("settings")
                    .join(format!("{}.json", component.name));
                if file_path.exists() {
                    fs::remove_file(&file_path)?;
                }
            }
            ComponentType::Hook => {
                let config_dir = Self::get_app_config_dir(app_type)?;
                let file_path = config_dir
                    .join("hooks")
                    .join(format!("{}.json", component.name));
                if file_path.exists() {
                    fs::remove_file(&file_path)?;
                }
            }
        }

        // 删除安装记录
        conn.execute(
            "DELETE FROM installed_components
             WHERE component_id = ?1 AND app_type = ?2",
            params![id, app_type],
        )?;

        log::info!("组件已卸载: {}", component.name);
        Ok(())
    }

    /// 批量安装组件
    pub async fn batch_install(
        &self,
        conn: &Connection,
        ids: Vec<i64>,
        app_type: &str,
    ) -> Result<BatchInstallResult> {
        let mut success = Vec::new();
        let mut failed = Vec::new();

        for id in ids {
            match self.install_component(conn, id, app_type).await {
                Ok(_) => success.push(id),
                Err(e) => failed.push((id, e.to_string())),
            }
        }

        Ok(BatchInstallResult { success, failed })
    }

    /// 列出已安装的组件
    #[allow(dead_code)]
    pub fn list_installed(
        &self,
        conn: &Connection,
        app_type: Option<&str>,
    ) -> Result<Vec<InstalledComponent>> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(at) = app_type {
            (
                "SELECT id, component_id, component_type, name, path, app_type, installed_at
                 FROM installed_components
                 WHERE app_type = ?
                 ORDER BY installed_at DESC"
                    .to_string(),
                vec![Box::new(at.to_string())],
            )
        } else {
            (
                "SELECT id, component_id, component_type, name, path, app_type, installed_at
                 FROM installed_components
                 ORDER BY installed_at DESC"
                    .to_string(),
                vec![],
            )
        };

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let components = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(InstalledComponent {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    component_type: ComponentType::from_str(&row.get::<_, String>(2)?)
                        .unwrap_or(ComponentType::Agent),
                    name: row.get(3)?,
                    path: row.get(4)?,
                    app_type: row.get(5)?,
                    installed_at: row
                        .get::<_, String>(6)?
                        .parse()
                        .unwrap_or_else(|_| chrono::Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(components)
    }

    /// 预览组件内容（仅获取内容，不进行安装）
    pub async fn preview_content(&self, conn: &Connection, id: i64) -> Result<String> {
        // 查询组件基本信息
        let component: TemplateComponent = conn.query_row(
            "SELECT id, repo_id, component_type, category, name, path, description, content_hash
             FROM template_components
             WHERE id = ?1",
            params![id],
            |row| {
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
            },
        )?;

        // 查询仓库信息
        let (repo_owner, repo_name, branch): (String, String, String) = conn.query_row(
            "SELECT owner, name, branch FROM template_repos WHERE id = ?1",
            params![component.repo_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        // 下载并读取组件内容
        let content = self
            .download_component_content(&repo_owner, &repo_name, &branch, &component.path)
            .await?;

        Ok(content)
    }
}
