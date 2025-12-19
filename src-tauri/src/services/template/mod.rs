use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod adapters;
pub mod index;
pub mod installer;
pub mod repo;

#[allow(unused_imports)]
pub use adapters::{create_adapter, AppAdapter};

/// 组件类型枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Agent,
    Command,
    Mcp,
    Setting,
    Hook,
    Skill,
}

impl ComponentType {
    pub fn as_str(&self) -> &str {
        match self {
            ComponentType::Agent => "agent",
            ComponentType::Command => "command",
            ComponentType::Mcp => "mcp",
            ComponentType::Setting => "setting",
            ComponentType::Hook => "hook",
            ComponentType::Skill => "skill",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "agent" => Some(ComponentType::Agent),
            "command" => Some(ComponentType::Command),
            "mcp" => Some(ComponentType::Mcp),
            "setting" => Some(ComponentType::Setting),
            "hook" => Some(ComponentType::Hook),
            "skill" => Some(ComponentType::Skill),
            _ => None,
        }
    }
}

/// 模板仓库
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateRepo {
    pub id: Option<i64>,
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub enabled: bool,
    #[serde(rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl TemplateRepo {
    pub fn new(owner: String, name: String, branch: String) -> Self {
        Self {
            id: None,
            owner,
            name,
            branch,
            enabled: true,
            created_at: None,
            updated_at: None,
        }
    }
}

/// 模板组件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateComponent {
    pub id: Option<i64>,
    #[serde(rename = "repoId")]
    pub repo_id: i64,
    #[serde(rename = "componentType")]
    pub component_type: ComponentType,
    pub category: Option<String>,
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    #[serde(rename = "contentHash")]
    pub content_hash: Option<String>,
    /// 是否已安装（前端展示用，需要在查询时填充）
    pub installed: bool,
}

/// 组件详情（含完整内容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDetail {
    #[serde(flatten)]
    pub component: TemplateComponent,
    /// 完整文件内容
    pub content: String,
    /// 仓库所有者
    #[serde(rename = "repoOwner")]
    pub repo_owner: String,
    /// 仓库名称
    #[serde(rename = "repoName")]
    pub repo_name: String,
    /// 仓库分支
    #[serde(rename = "repoBranch")]
    pub repo_branch: String,
    /// GitHub README URL
    #[serde(rename = "readmeUrl")]
    pub readme_url: String,
}

/// 组件元数据（从文件 front matter 解析）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ComponentMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Agent 专用 - 工具列表
    pub tools: Option<String>,
    /// Agent 专用 - 模型名称
    pub model: Option<String>,
}

/// 分页结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResult<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    #[serde(rename = "pageSize")]
    pub page_size: u32,
}

/// 批量安装结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchInstallResult {
    pub success: Vec<i64>,
    pub failed: Vec<(i64, String)>,
}

/// 已安装组件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledComponent {
    pub id: Option<i64>,
    #[serde(rename = "componentId")]
    pub component_id: Option<i64>,
    #[serde(rename = "componentType")]
    pub component_type: ComponentType,
    pub name: String,
    pub path: String,
    #[serde(rename = "appType")]
    pub app_type: String,
    #[serde(rename = "installedAt")]
    pub installed_at: DateTime<Utc>,
}

/// 市场组合项（plugin 中的单个组件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceBundleItem {
    pub name: String,
    pub path: String,
    #[serde(rename = "componentType")]
    pub component_type: String,
}

/// 市场组合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceBundle {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub components: Vec<MarketplaceBundleItem>,
}

/// Template 服务
pub struct TemplateService {
    http_client: Client,
}

impl TemplateService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http_client: Client::builder()
                .user_agent("cc-switch")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .context("创建 HTTP 客户端失败")?,
        })
    }

    /// 获取 HTTP 客户端
    pub fn client(&self) -> &Client {
        &self.http_client
    }

    /// 获取应用配置目录
    pub fn get_app_config_dir(app_type: &str) -> Result<PathBuf> {
        let home = dirs::home_dir().context("无法获取用户主目录")?;

        let dir = match app_type.to_lowercase().as_str() {
            "claude" => {
                // 检查是否有自定义 Claude 配置目录
                if let Some(custom) = crate::settings::get_claude_override_dir() {
                    custom
                } else {
                    home.join(".claude")
                }
            }
            "codex" => {
                // 检查是否有自定义 Codex 配置目录
                if let Some(custom) = crate::settings::get_codex_override_dir() {
                    custom
                } else {
                    home.join(".codex")
                }
            }
            "gemini" => {
                // 检查是否有自定义 Gemini 配置目录
                if let Some(custom) = crate::settings::get_gemini_override_dir() {
                    custom
                } else {
                    home.join(".gemini")
                }
            }
            _ => anyhow::bail!("不支持的应用类型: {app_type}"),
        };

        Ok(dir)
    }

    /// 从 components.json 获取市场组合
    pub async fn fetch_marketplace_bundles(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<MarketplaceBundle>> {
        // 获取启用的仓库
        let repos = self.list_enabled_repos(conn)?;
        if repos.is_empty() {
            return Ok(vec![]);
        }

        let mut bundles = Vec::new();

        for repo in repos {
            // 尝试多个可能的路径
            let urls = [
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/components.json",
                    repo.owner, repo.name, repo.branch
                ),
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/docs/components.json",
                    repo.owner, repo.name, repo.branch
                ),
            ];

            for url in urls {
                match self.http_client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            // 解析 marketplace.plugins（完整插件包）
                            if let Some(marketplace) = json.get("marketplace") {
                                if let Some(plugins) = marketplace.get("plugins") {
                                    if let Some(arr) = plugins.as_array() {
                                        for plugin in arr {
                                            let name = plugin
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown");
                                            let description = plugin
                                                .get("description")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            // 提取各类型组件路径
                                            let mut components = Vec::new();
                                            let component_types = [
                                                "agents", "commands", "mcps", "settings", "hooks",
                                                "skills",
                                            ];

                                            for comp_type in component_types {
                                                if let Some(paths) =
                                                    plugin.get(comp_type).and_then(|v| v.as_array())
                                                {
                                                    // 单数形式的类型名
                                                    let singular_type = match comp_type {
                                                        "agents" => "agent",
                                                        "commands" => "command",
                                                        "mcps" => "mcp",
                                                        "settings" => "setting",
                                                        "hooks" => "hook",
                                                        "skills" => "skill",
                                                        _ => comp_type,
                                                    };

                                                    for path_val in paths {
                                                        if let Some(path) = path_val.as_str() {
                                                            // 从路径提取组件名（文件名不含扩展名）
                                                            let comp_name =
                                                                std::path::Path::new(path)
                                                                    .file_stem()
                                                                    .and_then(|s| s.to_str())
                                                                    .unwrap_or("unknown")
                                                                    .to_string();

                                                            components.push(
                                                                MarketplaceBundleItem {
                                                                    name: comp_name,
                                                                    path: path.to_string(),
                                                                    component_type: singular_type
                                                                        .to_string(),
                                                                },
                                                            );
                                                        }
                                                    }
                                                }
                                            }

                                            if !components.is_empty() {
                                                bundles.push(MarketplaceBundle {
                                                    id: format!("{}-plugin-{}", repo.name, name),
                                                    name: name.to_string(),
                                                    description: description.to_string(),
                                                    category: "plugin".to_string(),
                                                    components,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        break; // 成功获取后跳出 URL 循环
                    }
                    _ => continue,
                }
            }
        }

        Ok(bundles)
    }
}

impl Default for TemplateService {
    fn default() -> Self {
        Self::new().expect("创建 TemplateService 失败")
    }
}
