//! JSON → SQLite 数据迁移
//!
//! 将旧版 config.json (MultiAppConfig) 数据迁移到 SQLite 数据库。

use super::{lock_conn, to_json_string, Database};
use crate::app_config::MultiAppConfig;
use crate::error::AppError;
use rusqlite::{params, Connection};

impl Database {
    /// 从 MultiAppConfig 迁移数据到数据库
    pub fn migrate_from_json(&self, config: &MultiAppConfig) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Self::migrate_from_json_tx(&tx, config)?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("Commit migration failed: {e}")))?;
        Ok(())
    }

    /// 运行迁移的 dry-run 模式（在内存数据库中验证，不写入磁盘）
    ///
    /// 用于部署前验证迁移逻辑是否正确。
    pub fn migrate_from_json_dry_run(config: &MultiAppConfig) -> Result<(), AppError> {
        let mut conn =
            Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
        Self::create_tables_on_conn(&conn)?;
        Self::apply_schema_migrations_on_conn(&conn)?;

        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Self::migrate_from_json_tx(&tx, config)?;

        // 显式 drop transaction 而不提交（内存数据库会被丢弃）
        drop(tx);
        Ok(())
    }

    /// 在事务中执行迁移
    fn migrate_from_json_tx(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
        // 1. 迁移 Providers
        Self::migrate_providers(tx, config)?;

        // 2. 迁移 MCP Servers
        Self::migrate_mcp_servers(tx, config)?;

        // 3. 迁移 Prompts
        Self::migrate_prompts(tx, config)?;

        // 4. 迁移 Skills
        Self::migrate_skills(tx, config)?;

        // 5. 迁移 Common Config
        Self::migrate_common_config(tx, config)?;

        Ok(())
    }

    /// 迁移供应商数据
    fn migrate_providers(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
        for (app_key, manager) in &config.apps {
            let app_type = app_key;
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
                        to_json_string(&meta_clone)?,
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
        Ok(())
    }

    /// 迁移 MCP 服务器数据
    fn migrate_mcp_servers(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
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
        Ok(())
    }

    /// 迁移提示词数据
    fn migrate_prompts(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
        let migrate_app_prompts = |prompts_map: &std::collections::HashMap<
            String,
            crate::prompt::Prompt,
        >,
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

        migrate_app_prompts(&config.prompts.claude.prompts, "claude")?;
        migrate_app_prompts(&config.prompts.codex.prompts, "codex")?;
        migrate_app_prompts(&config.prompts.gemini.prompts, "gemini")?;

        Ok(())
    }

    /// 迁移 Skills 数据
    fn migrate_skills(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
        // v3.10.0+：Skills 的 SSOT 已迁移到文件系统（~/.cc-switch/skills/）+ 数据库统一结构。
        //
        // 旧版 config.json 里的 `skills.skills` 仅记录“安装状态”，但不包含完整元数据，
        // 且无法保证 SSOT 目录中一定存在对应的 skill 文件。
        //
        // 因此这里不再直接把旧的安装状态写入新 skills 表，避免产生“数据库显示已安装但文件缺失”的不一致。
        // 迁移后可通过：
        // - 前端「导入已有」(扫描各应用的 skills 目录并复制到 SSOT)
        // - 或后续启动时的自动扫描逻辑
        // 来重建已安装技能记录。

        for repo in &config.skills.repos {
            tx.execute(
                "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
                params![repo.owner, repo.name, repo.branch, repo.enabled],
            ).map_err(|e| AppError::Database(format!("Migrate skill repo failed: {e}")))?;
        }

        Ok(())
    }

    /// 迁移通用配置片段
    fn migrate_common_config(
        tx: &rusqlite::Transaction<'_>,
        config: &MultiAppConfig,
    ) -> Result<(), AppError> {
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

        Ok(())
    }
}
