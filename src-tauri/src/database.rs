use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, ProviderMeta};
use crate::services::skill::{SkillRepo, SkillState};
use indexmap::IndexMap;
use rusqlite::{params, Connection, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// 安全地序列化 JSON，避免 unwrap panic
fn to_json_string<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))
}

/// 安全地获取 Mutex 锁，避免 unwrap panic
macro_rules! lock_conn {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {}", e)))?
    };
}

pub struct Database {
    // 使用 Mutex 包装 Connection 以支持在多线程环境（如 Tauri State）中共享
    // rusqlite::Connection 本身不是 Sync 的
    conn: Mutex<Connection>,
}

impl Database {
    /// 初始化数据库连接并创建表
    pub fn init() -> Result<Self, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let conn = Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;

        Ok(db)
    }

    fn create_tables(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 1. Providers 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT,
                is_current BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 2. Provider Endpoints 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                url TEXT NOT NULL,
                added_at INTEGER,
                FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
            )",
            [],
        ).map_err(|e| AppError::Database(e.to_string()))?;

        // 3. MCP Servers 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                server_config TEXT NOT NULL,
                description TEXT,
                homepage TEXT,
                docs TEXT,
                tags TEXT,
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 4. Prompts 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS prompts (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                description TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                PRIMARY KEY (id, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 5. Skills 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
                key TEXT PRIMARY KEY,
                installed BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 6. Skill Repos 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_repos (
                owner TEXT NOT NULL,
                name TEXT NOT NULL,
                branch TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                skills_path TEXT,
                PRIMARY KEY (owner, name)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 7. Settings 表 (通用配置)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 从 MultiAppConfig 迁移数据
    pub fn migrate_from_json(&self, config: &MultiAppConfig) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        // 1. 迁移 Providers
        for (app_key, manager) in &config.apps {
            let app_type = app_key; // "claude", "codex", "gemini"
            let current_id = &manager.current;

            for (id, provider) in &manager.providers {
                let is_current = if id == current_id { 1 } else { 0 };

                // 处理 meta 和 endpoints
                let mut meta_clone = provider.meta.clone().unwrap_or_default();
                let endpoints = std::mem::take(&mut meta_clone.custom_endpoints);

                tx.execute(
                    "INSERT OR REPLACE INTO providers (
                        id, app_type, name, settings_config, website_url, category, 
                        created_at, sort_index, notes, icon, icon_color, meta, is_current
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        id,
                        app_type,
                        provider.name,
                        to_json_string(&provider.settings_config)?,
                        provider.website_url,
                        provider.category,
                        provider.created_at,
                        provider.sort_index,
                        provider.notes,
                        provider.icon,
                        provider.icon_color,
                        to_json_string(&meta_clone)?, // 不含 endpoints 的 meta
                        is_current,
                    ],
                )
                .map_err(|e| AppError::Database(format!("Migrate provider failed: {e}")))?;

                // 迁移 Endpoints
                for (url, endpoint) in endpoints {
                    tx.execute(
                        "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![id, app_type, url, endpoint.added_at],
                    )
                    .map_err(|e| AppError::Database(format!("Migrate endpoint failed: {e}")))?;
                }
            }
        }

        // 2. 迁移 MCP Servers
        if let Some(servers) = &config.mcp.servers {
            for (id, server) in servers {
                tx.execute(
                    "INSERT OR REPLACE INTO mcp_servers (
                        id, name, server_config, description, homepage, docs, tags,
                        enabled_claude, enabled_codex, enabled_gemini
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        id,
                        server.name,
                        to_json_string(&server.server)?,
                        server.description,
                        server.homepage,
                        server.docs,
                        to_json_string(&server.tags)?,
                        server.apps.claude,
                        server.apps.codex,
                        server.apps.gemini,
                    ],
                )
                .map_err(|e| AppError::Database(format!("Migrate mcp server failed: {e}")))?;
            }
        }

        // 3. 迁移 Prompts
        let migrate_prompts =
            |prompts_map: &std::collections::HashMap<String, crate::prompt::Prompt>,
             app_type: &str|
             -> Result<(), AppError> {
                for (id, prompt) in prompts_map {
                    tx.execute(
                        "INSERT OR REPLACE INTO prompts (
                        id, app_type, name, content, description, enabled, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            id,
                            app_type,
                            prompt.name,
                            prompt.content,
                            prompt.description,
                            prompt.enabled,
                            prompt.created_at,
                            prompt.updated_at,
                        ],
                    )
                    .map_err(|e| AppError::Database(format!("Migrate prompt failed: {e}")))?;
                }
                Ok(())
            };

        migrate_prompts(&config.prompts.claude.prompts, "claude")?;
        migrate_prompts(&config.prompts.codex.prompts, "codex")?;
        migrate_prompts(&config.prompts.gemini.prompts, "gemini")?;

        // 4. 迁移 Skills
        for (key, state) in &config.skills.skills {
            tx.execute(
                "INSERT OR REPLACE INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
                params![key, state.installed, state.installed_at.timestamp()],
            )
            .map_err(|e| AppError::Database(format!("Migrate skill failed: {e}")))?;
        }

        for repo in &config.skills.repos {
            tx.execute(
                "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled, skills_path) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![repo.owner, repo.name, repo.branch, repo.enabled, repo.skills_path],
            ).map_err(|e| AppError::Database(format!("Migrate skill repo failed: {e}")))?;
        }

        // 5. 迁移 Common Config
        if let Some(snippet) = &config.common_config_snippets.claude {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params!["common_config_claude", snippet],
            )
            .map_err(|e| AppError::Database(format!("Migrate settings failed: {e}")))?;
        }
        if let Some(snippet) = &config.common_config_snippets.codex {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params!["common_config_codex", snippet],
            )
            .map_err(|e| AppError::Database(format!("Migrate settings failed: {e}")))?;
        }
        if let Some(snippet) = &config.common_config_snippets.gemini {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params!["common_config_gemini", snippet],
            )
            .map_err(|e| AppError::Database(format!("Migrate settings failed: {e}")))?;
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("Commit migration failed: {e}")))?;
        Ok(())
    }

    /// 检查数据库是否为空（需要首次导入）
    /// 通过检查是否有任何 MCP 服务器、提示词、Skills 仓库或供应商来判断
    pub fn is_empty_for_first_import(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);

        // 检查是否有 MCP 服务器
        let mcp_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        // 检查是否有提示词
        let prompt_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        // 检查是否有 Skills 仓库
        let skill_repo_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skill_repos", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        // 检查是否有供应商
        let provider_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        // 如果四者都为 0，说明是空数据库
        Ok(mcp_count == 0 && prompt_count == 0 && skill_repo_count == 0 && provider_count == 0)
    }

    // --- Providers DAO ---

    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta
             FROM providers WHERE app_type = ?1
             ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let provider_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let settings_config_str: String = row.get(2)?;
                let website_url: Option<String> = row.get(3)?;
                let category: Option<String> = row.get(4)?;
                let created_at: Option<i64> = row.get(5)?;
                let sort_index: Option<usize> = row.get(6)?;
                let notes: Option<String> = row.get(7)?;
                let icon: Option<String> = row.get(8)?;
                let icon_color: Option<String> = row.get(9)?;
                let meta_str: String = row.get(10)?;

                let settings_config =
                    serde_json::from_str(&settings_config_str).unwrap_or(serde_json::Value::Null);
                let meta: ProviderMeta = serde_json::from_str(&meta_str).unwrap_or_default();

                Ok((
                    id,
                    Provider {
                        id: "".to_string(), // Placeholder, set below
                        name,
                        settings_config,
                        website_url,
                        category,
                        created_at,
                        sort_index,
                        notes,
                        meta: Some(meta),
                        icon,
                        icon_color,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = IndexMap::new();
        for provider_res in provider_iter {
            let (id, mut provider) = provider_res.map_err(|e| AppError::Database(e.to_string()))?;
            provider.id = id.clone();

            // Load endpoints
            let mut stmt_endpoints = conn.prepare(
                "SELECT url, added_at FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 ORDER BY added_at ASC, url ASC"
            ).map_err(|e| AppError::Database(e.to_string()))?;

            let endpoints_iter = stmt_endpoints
                .query_map(params![id, app_type], |row| {
                    let url: String = row.get(0)?;
                    let added_at: Option<i64> = row.get(1)?;
                    Ok((
                        url,
                        crate::settings::CustomEndpoint {
                            url: "".to_string(),
                            added_at: added_at.unwrap_or(0),
                            last_used: None,
                        },
                    ))
                })
                .map_err(|e| AppError::Database(e.to_string()))?;

            let mut custom_endpoints = HashMap::new();
            for ep_res in endpoints_iter {
                let (url, mut ep) = ep_res.map_err(|e| AppError::Database(e.to_string()))?;
                ep.url = url.clone();
                custom_endpoints.insert(url, ep);
            }

            if let Some(meta) = &mut provider.meta {
                meta.custom_endpoints = custom_endpoints;
            }

            providers.insert(id, provider);
        }

        Ok(providers)
    }

    pub fn get_current_provider(&self, app_type: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn save_provider(&self, app_type: &str, provider: &Provider) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        // Handle meta and endpoints
        let mut meta_clone = provider.meta.clone().unwrap_or_default();
        let endpoints = std::mem::take(&mut meta_clone.custom_endpoints);

        // Check if it exists to preserve is_current
        let is_current: bool = tx
            .query_row(
                "SELECT is_current FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider.id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(false);

        tx.execute(
            "INSERT OR REPLACE INTO providers (
                id, app_type, name, settings_config, website_url, category, 
                created_at, sort_index, notes, icon, icon_color, meta, is_current
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                provider.id,
                app_type,
                provider.name,
                serde_json::to_string(&provider.settings_config).unwrap(),
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                serde_json::to_string(&meta_clone).unwrap(),
                is_current,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Sync endpoints: Delete all and re-insert
        tx.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
            params![provider.id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        for (url, endpoint) in endpoints {
            tx.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![provider.id, app_type, url, endpoint.added_at],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn set_current_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        // Reset all to 0
        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Set new current
        tx.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn add_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let added_at = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![provider_id, app_type, url, added_at],
        ).map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 AND url = ?3",
            params![provider_id, app_type, url],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // --- MCP Servers DAO ---

    pub fn get_all_mcp_servers(&self) -> Result<IndexMap<String, McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini
             FROM mcp_servers
             ORDER BY name ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let server_iter = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let server_config_str: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let homepage: Option<String> = row.get(4)?;
                let docs: Option<String> = row.get(5)?;
                let tags_str: String = row.get(6)?;
                let enabled_claude: bool = row.get(7)?;
                let enabled_codex: bool = row.get(8)?;
                let enabled_gemini: bool = row.get(9)?;

                let server = serde_json::from_str(&server_config_str).unwrap_or_default();
                let tags = serde_json::from_str(&tags_str).unwrap_or_default();

                Ok((
                    id.clone(),
                    McpServer {
                        id,
                        name,
                        server,
                        apps: McpApps {
                            claude: enabled_claude,
                            codex: enabled_codex,
                            gemini: enabled_gemini,
                        },
                        description,
                        homepage,
                        docs,
                        tags,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut servers = IndexMap::new();
        for server_res in server_iter {
            let (id, server) = server_res.map_err(|e| AppError::Database(e.to_string()))?;
            servers.insert(id, server);
        }
        Ok(servers)
    }

    pub fn save_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags,
                enabled_claude, enabled_codex, enabled_gemini
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                server.id,
                server.name,
                serde_json::to_string(&server.server).unwrap(),
                server.description,
                server.homepage,
                server.docs,
                serde_json::to_string(&server.tags).unwrap(),
                server.apps.claude,
                server.apps.codex,
                server.apps.gemini,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // --- Prompts DAO ---

    pub fn get_prompts(&self, app_type: &str) -> Result<IndexMap<String, Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE app_type = ?1
             ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let prompt_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let content: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let enabled: bool = row.get(4)?;
                let created_at: Option<i64> = row.get(5)?;
                let updated_at: Option<i64> = row.get(6)?;

                Ok((
                    id.clone(),
                    Prompt {
                        id,
                        name,
                        content,
                        description,
                        enabled,
                        created_at,
                        updated_at,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut prompts = IndexMap::new();
        for prompt_res in prompt_iter {
            let (id, prompt) = prompt_res.map_err(|e| AppError::Database(e.to_string()))?;
            prompts.insert(id, prompt);
        }
        Ok(prompts)
    }

    pub fn save_prompt(&self, app_type: &str, prompt: &Prompt) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO prompts (
                id, app_type, name, content, description, enabled, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                prompt.id,
                app_type,
                prompt.name,
                prompt.content,
                prompt.description,
                prompt.enabled,
                prompt.created_at,
                prompt.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_prompt(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM prompts WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // --- Skills DAO ---

    pub fn get_skills(&self) -> Result<IndexMap<String, SkillState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT key, installed, installed_at FROM skills ORDER BY key ASC")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let skill_iter = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let installed: bool = row.get(1)?;
                let installed_at_ts: i64 = row.get(2)?;

                let installed_at =
                    chrono::DateTime::from_timestamp(installed_at_ts, 0).unwrap_or_default();

                Ok((
                    key,
                    SkillState {
                        installed,
                        installed_at,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut skills = IndexMap::new();
        for skill_res in skill_iter {
            let (key, skill) = skill_res.map_err(|e| AppError::Database(e.to_string()))?;
            skills.insert(key, skill);
        }
        Ok(skills)
    }

    pub fn update_skill_state(&self, key: &str, state: &SkillState) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
            params![key, state.installed, state.installed_at.timestamp()],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_skill_repos(&self) -> Result<Vec<SkillRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT owner, name, branch, enabled, skills_path FROM skill_repos ORDER BY owner ASC, name ASC")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let repo_iter = stmt
            .query_map([], |row| {
                Ok(SkillRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                    skills_path: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut repos = Vec::new();
        for repo_res in repo_iter {
            repos.push(repo_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(repos)
    }

    pub fn save_skill_repo(&self, repo: &SkillRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled, skills_path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo.owner, repo.name, repo.branch, repo.enabled, repo.skills_path],
        ).map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_skill_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM skill_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 初始化默认的 Skill 仓库（首次启动时调用）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        // 检查是否已有仓库
        let existing = self.get_skill_repos()?;
        if !existing.is_empty() {
            return Ok(0);
        }

        // 获取默认仓库列表
        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        for repo in &default_store.repos {
            self.save_skill_repo(repo)?;
            count += 1;
        }

        log::info!("初始化默认 Skill 仓库完成，共 {count} 个");
        Ok(count)
    }

    // --- Settings DAO ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![key])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // --- Config Snippets Helper Methods ---

    pub fn get_config_snippet(&self, app_type: &str) -> Result<Option<String>, AppError> {
        self.get_setting(&format!("common_config_{app_type}"))
    }

    pub fn set_config_snippet(
        &self,
        app_type: &str,
        snippet: Option<String>,
    ) -> Result<(), AppError> {
        let key = format!("common_config_{app_type}");
        if let Some(value) = snippet {
            self.set_setting(&key, &value)
        } else {
            // Delete if None
            let conn = lock_conn!(self.conn);
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
                .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        }
    }
}
