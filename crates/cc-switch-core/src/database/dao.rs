//! Database DAO (Data Access Object) methods

use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

use crate::app_config::{InstalledSkill, McpApps, McpServer, SkillApps};
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::services::proxy::{
    FailoverQueueItem, LiveBackup, ProxyConfig, ProxyTakeoverStatus, RequestLog, UsageSummary,
};
use crate::services::skill::SkillRepo;
use crate::services::usage::{
    PaginatedUsageLogs, UsageLogDetail, UsageLogFilters, UsageModelStat, UsageProviderStat,
    UsageTrendPoint,
};
use crate::settings::AppSettings;

use super::{lock_conn, to_json_string, Database};

impl Database {
    // ========== Provider Methods ==========

    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE app_type = ? ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let providers = stmt
            .query_map([app_type], |row| {
                Ok(Provider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    settings_config: parse_json(row.get(2)?),
                    website_url: row.get(3)?,
                    category: row.get(4)?,
                    created_at: row.get(5)?,
                    sort_index: row.get(6)?,
                    notes: row.get(7)?,
                    icon: row.get(8)?,
                    icon_color: row.get(9)?,
                    meta: Some(parse_json(row.get::<_, String>(10)?)),
                    in_failover_queue: row.get(11)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = IndexMap::new();
        for mut provider in providers {
            let mut stmt_endpoints = conn.prepare(
                "SELECT url, added_at FROM provider_endpoints
                 WHERE provider_id = ?1 AND app_type = ?2
                 ORDER BY added_at ASC, url ASC",
            )?;

            let endpoints = stmt_endpoints
                .query_map(params![&provider.id, app_type], |row| {
                    Ok(crate::settings::CustomEndpoint {
                        url: row.get(0)?,
                        added_at: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        last_used: None,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut meta = provider.meta.take().unwrap_or_default();
            meta.custom_endpoints = endpoints
                .into_iter()
                .map(|endpoint| (endpoint.url.clone(), endpoint))
                .collect();
            provider.meta = Some(meta);
            map.insert(provider.id.clone(), provider);
        }
        Ok(map)
    }

    pub fn get_current_provider(&self, app_type: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id FROM providers WHERE app_type = ? AND is_current = 1",
                [app_type],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn set_current_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE providers SET is_current = 1 WHERE app_type = ? AND id = ?",
            [app_type, id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn save_provider(&self, app_type: &str, provider: &Provider) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let config_json = to_json_string(&provider.settings_config)?;
        let mut meta = provider.meta.clone().unwrap_or_default();
        let endpoints = std::mem::take(&mut meta.custom_endpoints);
        let meta_json = to_json_string(&meta)?;

        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "INSERT OR REPLACE INTO providers (
                id, app_type, name, settings_config, website_url, category, created_at,
                sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
            )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                COALESCE((SELECT is_current FROM providers WHERE id = ?1 AND app_type = ?2), 0),
                ?13)",
            params![
                provider.id,
                app_type,
                provider.name,
                config_json,
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                meta_json,
                provider.in_failover_queue,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

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
        Ok(true)
    }

    pub fn delete_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM providers WHERE id = ? AND app_type = ?",
            [id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_provider_by_id(
        &self,
        id: &str,
        app_type: &str,
    ) -> Result<Option<Provider>, AppError> {
        Ok(self.get_all_providers(app_type)?.shift_remove(id))
    }

    pub fn update_provider_settings_config(
        &self,
        app_type: &str,
        provider_id: &str,
        settings_config: &serde_json::Value,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
            params![
                serde_json::to_string(settings_config).map_err(|e| AppError::Database(format!(
                    "Failed to serialize settings_config: {e}"
                )))?,
                provider_id,
                app_type
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
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
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![provider_id, app_type, url, added_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
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

    pub fn set_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
            params![app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        let opposite = match category {
            "omo" => Some("omo-slim"),
            "omo-slim" => Some("omo"),
            _ => None,
        };
        if let Some(opposite) = opposite {
            tx.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
                params![app_type, opposite],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        let updated = tx
            .execute(
                "UPDATE providers
                 SET is_current = 1
                 WHERE id = ?1 AND app_type = ?2 AND category = ?3",
                params![provider_id, app_type, category],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        if updated != 1 {
            return Err(AppError::Database(format!(
                "Failed to set {category} provider current: provider '{provider_id}' not found in app '{app_type}'"
            )));
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn is_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        match conn.query_row(
            "SELECT is_current FROM providers
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
            |row| row.get(0),
        ) {
            Ok(is_current) => Ok(is_current),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn clear_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET is_current = 0
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_current_omo_provider(
        &self,
        app_type: &str,
        category: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let row_data = conn.query_row(
            "SELECT id, name, settings_config, category, created_at, sort_index, notes, meta
             FROM providers
             WHERE app_type = ?1 AND category = ?2 AND is_current = 1
             LIMIT 1",
            params![app_type, category],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<usize>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        );

        let (
            id,
            name,
            settings_config_str,
            category_value,
            created_at,
            sort_index,
            notes,
            meta_str,
        ) = match row_data {
            Ok(value) => value,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(AppError::Database(e.to_string())),
        };

        let settings_config = serde_json::from_str(&settings_config_str).map_err(|e| {
            AppError::Database(format!(
                "Failed to parse {category} provider settings_config (provider_id={id}): {e}"
            ))
        })?;
        let meta = if meta_str.trim().is_empty() {
            crate::provider::ProviderMeta::default()
        } else {
            serde_json::from_str(&meta_str).map_err(|e| {
                AppError::Database(format!(
                    "Failed to parse {category} provider meta (provider_id={id}): {e}"
                ))
            })?
        };

        Ok(Some(Provider {
            id,
            name,
            settings_config,
            website_url: None,
            category: category_value,
            created_at,
            sort_index,
            notes,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }))
    }

    // ========== Universal Provider Methods ==========

    pub fn get_all_universal_providers(
        &self,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, apps, base_url, api_key, models, website_url, notes, icon, icon_color, meta, created_at, sort_index
             FROM universal_providers ORDER BY sort_index, created_at"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let providers = stmt
            .query_map([], |row| {
                Ok(UniversalProvider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_type: row.get(2)?,
                    apps: parse_json(row.get(3)?),
                    base_url: row.get(4)?,
                    api_key: row.get(5)?,
                    models: parse_json(row.get(6)?),
                    website_url: row.get(7)?,
                    notes: row.get(8)?,
                    icon: row.get(9)?,
                    icon_color: row.get(10)?,
                    meta: parse_json_opt(row.get(11)?),
                    created_at: row.get(12)?,
                    sort_index: row.get(13)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = HashMap::new();
        for p in providers {
            map.insert(p.id.clone(), p);
        }
        Ok(map)
    }

    pub fn get_universal_provider(&self, id: &str) -> Result<Option<UniversalProvider>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id, name, provider_type, apps, base_url, api_key, models, website_url, notes, icon, icon_color, meta, created_at, sort_index
                 FROM universal_providers WHERE id = ?",
                [id],
                |row| {
                    Ok(UniversalProvider {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        provider_type: row.get(2)?,
                        apps: parse_json(row.get(3)?),
                        base_url: row.get(4)?,
                        api_key: row.get(5)?,
                        models: parse_json(row.get(6)?),
                        website_url: row.get(7)?,
                        notes: row.get(8)?,
                        icon: row.get(9)?,
                        icon_color: row.get(10)?,
                        meta: parse_json_opt(row.get(11)?),
                        created_at: row.get(12)?,
                        sort_index: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn save_universal_provider(&self, provider: &UniversalProvider) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let apps_json = to_json_string(&provider.apps)?;
        let models_json = to_json_string(&provider.models)?;
        let meta_json = to_json_string(&provider.meta)?;

        conn.execute(
            "INSERT OR REPLACE INTO universal_providers (id, name, provider_type, apps, base_url, api_key, models, website_url, notes, icon, icon_color, meta, created_at, sort_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                provider.id,
                provider.name,
                provider.provider_type,
                apps_json,
                provider.base_url,
                provider.api_key,
                models_json,
                provider.website_url,
                provider.notes,
                provider.icon,
                provider.icon_color,
                meta_json,
                provider.created_at,
                provider.sort_index,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(true)
    }

    pub fn delete_universal_provider(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM universal_providers WHERE id = ?", [id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // ========== MCP Server Methods ==========

    pub fn get_all_mcp_servers(&self) -> Result<IndexMap<String, McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw
             FROM mcp_servers ORDER BY id"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let servers = stmt
            .query_map([], |row| {
                Ok(McpServer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    server: parse_json(row.get(2)?),
                    description: row.get(3)?,
                    homepage: row.get(4)?,
                    docs: row.get(5)?,
                    tags: parse_json(row.get(6)?),
                    apps: McpApps {
                        claude: row.get(7)?,
                        codex: row.get(8)?,
                        gemini: row.get(9)?,
                        opencode: row.get(10)?,
                        openclaw: row.get(11)?,
                    },
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = IndexMap::new();
        for s in servers {
            map.insert(s.id.clone(), s);
        }
        Ok(map)
    }

    pub fn get_mcp_server(&self, id: &str) -> Result<Option<McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw
                 FROM mcp_servers WHERE id = ?",
                [id],
                |row| {
                    Ok(McpServer {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        server: parse_json(row.get(2)?),
                        description: row.get(3)?,
                        homepage: row.get(4)?,
                        docs: row.get(5)?,
                        tags: parse_json(row.get(6)?),
                        apps: McpApps {
                            claude: row.get(7)?,
                            codex: row.get(8)?,
                            gemini: row.get(9)?,
                            opencode: row.get(10)?,
                            openclaw: row.get(11)?,
                        },
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn save_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let config_json = to_json_string(&server.server)?;
        let tags_json = to_json_string(&server.tags)?;

        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                server.id,
                server.name,
                config_json,
                server.description,
                server.homepage,
                server.docs,
                tags_json,
                server.apps.claude,
                server.apps.codex,
                server.apps.gemini,
                server.apps.opencode,
                server.apps.openclaw,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM mcp_servers WHERE id = ?", [id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // ========== Prompt Methods ==========

    pub fn get_all_prompts(&self, app_type: &str) -> Result<IndexMap<String, Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE app_type = ? ORDER BY id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let prompts = stmt
            .query_map([app_type], |row| {
                Ok(Prompt {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    description: row.get(3)?,
                    enabled: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = IndexMap::new();
        for p in prompts {
            map.insert(p.id.clone(), p);
        }
        Ok(map)
    }

    pub fn get_prompt(&self, app_type: &str, id: &str) -> Result<Option<Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id, name, content, description, enabled, created_at, updated_at
                 FROM prompts WHERE app_type = ? AND id = ?",
                [app_type, id],
                |row| {
                    Ok(Prompt {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        content: row.get(2)?,
                        description: row.get(3)?,
                        enabled: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn save_prompt(&self, app_type: &str, prompt: &Prompt) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO prompts (id, app_type, name, content, description, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
            "DELETE FROM prompts WHERE app_type = ? AND id = ?",
            [app_type, id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // ========== Skill Methods ==========

    pub fn get_all_skills(&self) -> Result<Vec<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw, installed_at
             FROM skills ORDER BY installed_at DESC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let skills = stmt
            .query_map([], |row| {
                Ok(InstalledSkill {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    directory: row.get(3)?,
                    repo_owner: row.get(4)?,
                    repo_name: row.get(5)?,
                    repo_branch: row.get(6)?,
                    readme_url: row.get(7)?,
                    apps: SkillApps {
                        claude: row.get(8)?,
                        codex: row.get(9)?,
                        gemini: row.get(10)?,
                        opencode: row.get(11)?,
                        openclaw: row.get(12)?,
                    },
                    installed_at: row.get(13)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(skills)
    }

    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let skills = self.get_all_skills()?;
        Ok(skills
            .into_iter()
            .map(|skill| (skill.id.clone(), skill))
            .collect())
    }

    pub fn get_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw, installed_at
                 FROM skills WHERE id = ?",
                [id],
                |row| {
                    Ok(InstalledSkill {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        directory: row.get(3)?,
                        repo_owner: row.get(4)?,
                        repo_name: row.get(5)?,
                        repo_branch: row.get(6)?,
                        readme_url: row.get(7)?,
                        apps: SkillApps {
                            claude: row.get(8)?,
                            codex: row.get(9)?,
                            gemini: row.get(10)?,
                            opencode: row.get(11)?,
                            openclaw: row.get(12)?,
                        },
                        installed_at: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn get_installed_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        self.get_skill(id)
    }

    pub fn save_skill(&self, skill: &InstalledSkill) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills (id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_openclaw, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.directory,
                skill.repo_owner,
                skill.repo_name,
                skill.repo_branch,
                skill.readme_url,
                skill.apps.claude,
                skill.apps.codex,
                skill.apps.gemini,
                skill.apps.opencode,
                skill.apps.openclaw,
                skill.installed_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_skill(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM skills WHERE id = ?", [id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn clear_skills(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM skills", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_skill_apps(&self, id: &str, apps: &SkillApps) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE skills
             SET enabled_claude = ?1,
                 enabled_codex = ?2,
                 enabled_gemini = ?3,
                 enabled_opencode = ?4,
                 enabled_openclaw = ?5
             WHERE id = ?6",
            params![
                apps.claude,
                apps.codex,
                apps.gemini,
                apps.opencode,
                apps.openclaw,
                id
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_skill_repos(&self) -> Result<Vec<SkillRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled
                 FROM skill_repos
                 ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let repos = stmt
            .query_map([], |row| {
                Ok(SkillRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(repos)
    }

    pub fn save_skill_repo(&self, repo: &SkillRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled)
             VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
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

    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        let existing = self.get_skill_repos()?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|repo| (repo.owner.clone(), repo.name.clone()))
            .collect();

        let default_store = crate::services::skill::SkillStore::default();
        let mut added = 0;
        for repo in &default_store.repos {
            let key = (repo.owner.clone(), repo.name.clone());
            if existing_keys.contains(&key) {
                continue;
            }

            self.save_skill_repo(repo)?;
            added += 1;
        }

        Ok(added)
    }

    // ========== Settings Methods ==========

    pub fn get_settings(&self) -> Result<AppSettings, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let pairs = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<(String, String)>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut settings = AppSettings::default();
        for (key, value) in pairs {
            match key.as_str() {
                "language" => settings.language = Some(value),
                "claudeConfigDir" => settings.claude_config_dir = Some(value),
                "codexConfigDir" => settings.codex_config_dir = Some(value),
                "geminiConfigDir" => settings.gemini_config_dir = Some(value),
                "opencodeConfigDir" => settings.opencode_config_dir = Some(value),
                "openclawConfigDir" => settings.openclaw_config_dir = Some(value),
                "currentProviderClaude" => settings.current_provider_claude = Some(value),
                "currentProviderCodex" => settings.current_provider_codex = Some(value),
                "currentProviderGemini" => settings.current_provider_gemini = Some(value),
                "currentProviderOpenCode" => settings.current_provider_opencode = Some(value),
                "currentProviderOpenClaw" => settings.current_provider_openclaw = Some(value),
                "skillSyncMethod" => settings.skill_sync_method = value.parse().unwrap_or_default(),
                "preferredTerminal" => settings.preferred_terminal = Some(value),
                _ => {}
            }
        }

        Ok(settings)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row("SELECT value FROM settings WHERE key = ?", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [key, value],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        if let Some(v) = &settings.language {
            self.set_setting("language", v)?;
        }
        if let Some(v) = &settings.claude_config_dir {
            self.set_setting("claudeConfigDir", v)?;
        }
        if let Some(v) = &settings.codex_config_dir {
            self.set_setting("codexConfigDir", v)?;
        }
        if let Some(v) = &settings.gemini_config_dir {
            self.set_setting("geminiConfigDir", v)?;
        }
        if let Some(v) = &settings.opencode_config_dir {
            self.set_setting("opencodeConfigDir", v)?;
        }
        if let Some(v) = &settings.openclaw_config_dir {
            self.set_setting("openclawConfigDir", v)?;
        }
        if let Some(v) = &settings.current_provider_claude {
            self.set_setting("currentProviderClaude", v)?;
        }
        if let Some(v) = &settings.current_provider_codex {
            self.set_setting("currentProviderCodex", v)?;
        }
        if let Some(v) = &settings.current_provider_gemini {
            self.set_setting("currentProviderGemini", v)?;
        }
        if let Some(v) = &settings.current_provider_opencode {
            self.set_setting("currentProviderOpenCode", v)?;
        }
        if let Some(v) = &settings.current_provider_openclaw {
            self.set_setting("currentProviderOpenClaw", v)?;
        }
        self.set_setting("skillSyncMethod", &settings.skill_sync_method.to_string())?;
        if let Some(v) = &settings.preferred_terminal {
            self.set_setting("preferredTerminal", v)?;
        }
        Ok(())
    }

    // ========== Proxy Methods ==========

    pub fn get_proxy_config(&self, app_type: &str) -> Result<ProxyConfig, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT listen_port, listen_address, enable_logging FROM proxy_config WHERE app_type = ?",
                [app_type],
                |row| {
                    Ok(ProxyConfig {
                        port: row.get(0)?,
                        host: row.get(1)?,
                        log_enabled: row.get(2)?,
                    })
                },
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn set_proxy_takeover(&self, app: &str, enabled: bool) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET proxy_enabled = ?1, enabled = ?1 WHERE app_type = ?2",
            params![enabled, app],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_proxy_takeover_status(&self) -> Result<ProxyTakeoverStatus, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT app_type, enabled FROM proxy_config")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let pairs = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<(String, bool)>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut apps = HashMap::new();
        for (app, enabled) in pairs {
            apps.insert(app, enabled);
        }

        Ok(ProxyTakeoverStatus { apps })
    }

    pub fn switch_proxy_target(&self, app: &str, provider_id: &str) -> Result<(), AppError> {
        let exists = self.get_provider_by_id(provider_id, app)?.is_some();
        if !exists {
            return Err(AppError::Message(format!(
                "Provider '{provider_id}' not found for app '{app}'"
            )));
        }

        self.set_current_provider(app, provider_id)
    }

    pub fn get_failover_queue(&self, app_type: &str) -> Result<Vec<FailoverQueueItem>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(sort_index, 999999), id, name
                 FROM providers
                 WHERE app_type = ?1 AND in_failover_queue = 1
                 ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([app_type], |row| {
                Ok(FailoverQueueItem {
                    priority: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_name: row.get(2)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn add_to_failover_queue(&self, app_type: &str, provider_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET in_failover_queue = 1 WHERE id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_from_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET in_failover_queue = 0 WHERE id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM provider_health WHERE provider_id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn clear_failover_queue(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET in_failover_queue = 0 WHERE app_type = ?1",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM provider_health WHERE app_type = ?1",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn is_in_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT in_failover_queue FROM providers WHERE id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn reset_provider_health(&self, provider_id: &str, app: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE provider_health SET is_healthy = 1, consecutive_failures = 0 WHERE provider_id = ? AND app_type = ?",
            [provider_id, app],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn clear_provider_health_for_app(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM provider_health WHERE app_type = ?", [app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn save_live_backup(&self, app_type: &str, config_json: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO proxy_live_backup (app_type, original_config, backed_up_at)
             VALUES (?1, ?2, ?3)",
            params![app_type, config_json, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn has_any_live_backup(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_live_backup", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    pub fn get_live_backup(&self, app_type: &str) -> Result<Option<LiveBackup>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT app_type, original_config, backed_up_at
             FROM proxy_live_backup
             WHERE app_type = ?1",
            [app_type],
            |row| {
                Ok(LiveBackup {
                    app_type: row.get(0)?,
                    original_config: row.get(1)?,
                    backed_up_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn delete_live_backup(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM proxy_live_backup WHERE app_type = ?",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_all_live_backups(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM proxy_live_backup", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_usage_summary(&self, app: &str, days: u32) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let since = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let since_ts = since.timestamp_millis();

        let (where_clause, params) = build_usage_filters(app, Some(since_ts), None);
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let summary_sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(SUM(input_tokens + output_tokens), 0),
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs
             {where_clause}"
        );

        let (total_requests, total_tokens, total_cost) = conn
            .query_row(&summary_sql, params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let by_model_sql = format!(
            "SELECT model, COUNT(*)
             FROM proxy_request_logs
             {where_clause}
             GROUP BY model
             ORDER BY COUNT(*) DESC, model ASC"
        );
        let mut stmt = conn
            .prepare(&by_model_sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut requests_by_model = HashMap::new();
        for row in rows {
            let (model, count) = row.map_err(|e| AppError::Database(e.to_string()))?;
            requests_by_model.insert(model, count);
        }

        Ok(UsageSummary {
            total_requests,
            total_tokens,
            total_cost,
            requests_by_model,
        })
    }

    pub fn get_request_logs(
        &self,
        app: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<RequestLog>, AppError> {
        let start_ts = from.map(|date| parse_usage_date(date, false)).transpose()?;
        let end_ts = to.map(|date| parse_usage_date(date, true)).transpose()?;

        if let (Some(start_ts), Some(end_ts)) = (start_ts, end_ts) {
            if start_ts > end_ts {
                return Err(AppError::InvalidInput(
                    "The 'from' date must be earlier than or equal to the 'to' date".to_string(),
                ));
            }
        }

        let conn = lock_conn!(self.conn);
        let (where_clause, params) = build_usage_filters(app, start_ts, end_ts);
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let sql = format!(
            "SELECT created_at, model, input_tokens + output_tokens, CAST(total_cost_usd AS REAL)
             FROM proxy_request_logs
             {where_clause}
             ORDER BY created_at DESC"
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(RequestLog {
                    timestamp: format_usage_timestamp(row.get(0)?),
                    model: row.get(1)?,
                    total_tokens: row.get::<_, u64>(2)?,
                    cost: row.get::<_, f64>(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_usage_trends(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<Vec<UsageTrendPoint>, AppError> {
        use chrono::{Local, TimeZone};

        let conn = lock_conn!(self.conn);
        let end_ts = end_date.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let mut start_ts =
            start_date.unwrap_or_else(|| end_ts - chrono::Duration::hours(24).num_milliseconds());

        if start_ts >= end_ts {
            start_ts = end_ts - chrono::Duration::hours(24).num_milliseconds();
        }

        let duration = end_ts - start_ts;
        let bucket_ms: i64 = if duration <= chrono::Duration::hours(24).num_milliseconds() {
            chrono::Duration::hours(1).num_milliseconds()
        } else {
            chrono::Duration::days(1).num_milliseconds()
        };
        let mut bucket_count = if duration <= 0 {
            1
        } else {
            ((duration as f64) / bucket_ms as f64).ceil() as i64
        };
        if bucket_ms == chrono::Duration::hours(1).num_milliseconds() {
            bucket_count = 24;
        }
        if bucket_count < 1 {
            bucket_count = 1;
        }

        let sql = "
            SELECT
                CAST((created_at - ?1) / ?3 AS INTEGER) as bucket_idx,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as total_cache_read_tokens
            FROM proxy_request_logs
            WHERE created_at >= ?1 AND created_at <= ?2
            GROUP BY bucket_idx
            ORDER BY bucket_idx ASC";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![start_ts, end_ts, bucket_ms], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    UsageTrendPoint {
                        date: String::new(),
                        request_count: row.get::<_, i64>(1)? as u64,
                        total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                        total_tokens: row.get::<_, i64>(3)? as u64,
                        total_input_tokens: row.get::<_, i64>(4)? as u64,
                        total_output_tokens: row.get::<_, i64>(5)? as u64,
                        total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                        total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = HashMap::new();
        for row in rows {
            let (mut bucket_idx, point) = row.map_err(|e| AppError::Database(e.to_string()))?;
            if bucket_idx < 0 {
                continue;
            }
            if bucket_idx >= bucket_count {
                bucket_idx = bucket_count - 1;
            }
            map.insert(bucket_idx, point);
        }

        let mut points = Vec::with_capacity(bucket_count as usize);
        for index in 0..bucket_count {
            let bucket_start_ms = start_ts + index * bucket_ms;
            let bucket_start = Local
                .timestamp_millis_opt(bucket_start_ms)
                .single()
                .unwrap_or_else(Local::now);
            let date = bucket_start.to_rfc3339();

            if let Some(mut point) = map.remove(&index) {
                point.date = date;
                points.push(point);
            } else {
                points.push(UsageTrendPoint {
                    date,
                    request_count: 0,
                    total_cost: "0.000000".to_string(),
                    total_tokens: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cache_creation_tokens: 0,
                    total_cache_read_tokens: 0,
                });
            }
        }

        Ok(points)
    }

    pub fn get_usage_provider_stats(&self) -> Result<Vec<UsageProviderStat>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = "SELECT
                l.provider_id,
                COALESCE(p.name, 'Unknown') as provider_name,
                COUNT(*) as request_count,
                COALESCE(SUM(l.input_tokens + l.output_tokens), 0) as total_tokens,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                COALESCE(AVG(l.latency_ms), 0) as avg_latency
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             GROUP BY l.provider_id, l.app_type
             ORDER BY total_cost DESC";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let request_count: i64 = row.get(2)?;
                let success_count: i64 = row.get(5)?;
                let success_rate = if request_count > 0 {
                    (success_count as f32 / request_count as f32) * 100.0
                } else {
                    0.0
                };

                Ok(UsageProviderStat {
                    provider_id: row.get(0)?,
                    provider_name: row.get(1)?,
                    request_count: request_count as u64,
                    total_tokens: row.get::<_, i64>(3)? as u64,
                    total_cost: format!("{:.6}", row.get::<_, f64>(4)?),
                    success_rate,
                    avg_latency_ms: row.get::<_, f64>(6)? as u64,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_usage_model_stats(&self) -> Result<Vec<UsageModelStat>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = "SELECT
                model,
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost
             FROM proxy_request_logs
             GROUP BY model
             ORDER BY total_cost DESC";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let request_count: i64 = row.get(1)?;
                let total_cost: f64 = row.get(3)?;
                let avg_cost = if request_count > 0 {
                    total_cost / request_count as f64
                } else {
                    0.0
                };
                Ok(UsageModelStat {
                    model: row.get(0)?,
                    request_count: request_count as u64,
                    total_tokens: row.get::<_, i64>(2)? as u64,
                    total_cost: format!("{total_cost:.6}"),
                    avg_cost_per_request: format!("{avg_cost:.6}"),
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_usage_log_details(
        &self,
        filters: &UsageLogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedUsageLogs, AppError> {
        let conn = lock_conn!(self.conn);
        let (where_clause, mut params) = build_usage_detail_filters(filters);

        let count_sql = format!(
            "SELECT COUNT(*) FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}"
        );
        let count_params: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|item| item.as_ref()).collect();
        let total = conn
            .query_row(&count_sql, count_params.as_slice(), |row| {
                row.get::<_, i64>(0).map(|value| value as u32)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let offset = page.saturating_mul(page_size);
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|item| item.as_ref()).collect();

        let sql = format!(
            "SELECT l.request_id, l.provider_id, p.name, l.app_type, l.model, l.request_model,
                    COALESCE(l.cost_multiplier, '1'),
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}
             ORDER BY l.created_at DESC
             LIMIT ? OFFSET ?"
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(UsageLogDetail {
                    request_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_name: row.get(2)?,
                    app_type: row.get(3)?,
                    model: row.get(4)?,
                    request_model: row.get(5)?,
                    cost_multiplier: row.get(6)?,
                    input_tokens: row.get::<_, i64>(7)? as u32,
                    output_tokens: row.get::<_, i64>(8)? as u32,
                    cache_read_tokens: row.get::<_, i64>(9)? as u32,
                    cache_creation_tokens: row.get::<_, i64>(10)? as u32,
                    input_cost_usd: row.get(11)?,
                    output_cost_usd: row.get(12)?,
                    cache_read_cost_usd: row.get(13)?,
                    cache_creation_cost_usd: row.get(14)?,
                    total_cost_usd: row.get(15)?,
                    is_streaming: row.get::<_, i64>(16)? != 0,
                    latency_ms: row.get::<_, i64>(17)? as u64,
                    first_token_ms: row.get::<_, Option<i64>>(18)?.map(|value| value as u64),
                    duration_ms: row.get::<_, Option<i64>>(19)?.map(|value| value as u64),
                    status_code: row.get::<_, i64>(20)? as u16,
                    error_message: row.get(21)?,
                    created_at: row.get(22)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let data = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(PaginatedUsageLogs {
            data,
            total,
            page,
            page_size,
        })
    }

    pub fn get_usage_request_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<UsageLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = "SELECT l.request_id, l.provider_id, p.name, l.app_type, l.model, l.request_model,
                    COALESCE(l.cost_multiplier, '1'),
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?1
             LIMIT 1";

        conn.query_row(sql, [request_id], |row| {
            Ok(UsageLogDetail {
                request_id: row.get(0)?,
                provider_id: row.get(1)?,
                provider_name: row.get(2)?,
                app_type: row.get(3)?,
                model: row.get(4)?,
                request_model: row.get(5)?,
                cost_multiplier: row.get(6)?,
                input_tokens: row.get::<_, i64>(7)? as u32,
                output_tokens: row.get::<_, i64>(8)? as u32,
                cache_read_tokens: row.get::<_, i64>(9)? as u32,
                cache_creation_tokens: row.get::<_, i64>(10)? as u32,
                input_cost_usd: row.get(11)?,
                output_cost_usd: row.get(12)?,
                cache_read_cost_usd: row.get(13)?,
                cache_creation_cost_usd: row.get(14)?,
                total_cost_usd: row.get(15)?,
                is_streaming: row.get::<_, i64>(16)? != 0,
                latency_ms: row.get::<_, i64>(17)? as u64,
                first_token_ms: row.get::<_, Option<i64>>(18)?.map(|value| value as u64),
                duration_ms: row.get::<_, Option<i64>>(19)?.map(|value| value as u64),
                status_code: row.get::<_, i64>(20)? as u16,
                error_message: row.get(21)?,
                created_at: row.get(22)?,
            })
        })
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    // ========== Export Methods ==========

    pub fn export_all_providers(&self) -> Result<serde_json::Value, AppError> {
        let mut result = serde_json::Map::new();
        for app in ["claude", "codex", "gemini", "opencode", "openclaw"] {
            let providers = self.get_all_providers(app)?;
            result.insert(
                app.to_string(),
                serde_json::to_value(providers).unwrap_or_default(),
            );
        }
        Ok(serde_json::Value::Object(result))
    }

    pub fn export_all_mcp_servers(&self) -> Result<serde_json::Value, AppError> {
        let servers = self.get_all_mcp_servers()?;
        Ok(serde_json::to_value(servers).unwrap_or_default())
    }

    pub fn export_all_prompts(&self) -> Result<serde_json::Value, AppError> {
        let mut result = serde_json::Map::new();
        for app in ["claude", "codex", "gemini", "opencode", "openclaw"] {
            let prompts = self.get_all_prompts(app)?;
            result.insert(
                app.to_string(),
                serde_json::to_value(prompts).unwrap_or_default(),
            );
        }
        Ok(serde_json::Value::Object(result))
    }

    pub fn export_all_skills(&self) -> Result<serde_json::Value, AppError> {
        let skills = self.get_all_skills()?;
        Ok(serde_json::to_value(skills).unwrap_or_default())
    }

    pub fn clear_all_data(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM providers", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM universal_providers", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM mcp_servers", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM prompts", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM skills", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM settings", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}

fn parse_json<T: DeserializeOwned>(json: String) -> T {
    serde_json::from_str(&json).unwrap_or_else(|_| panic!("Failed to parse JSON: {}", json))
}

fn parse_json_opt<T: DeserializeOwned>(json: Option<String>) -> Option<T> {
    json.and_then(|s| serde_json::from_str(&s).ok())
}

fn build_usage_filters(
    app: &str,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut conditions = vec!["app_type = ?".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(app.to_string())];

    if let Some(start_ts) = start_ts {
        conditions.push("created_at >= ?".to_string());
        params.push(Box::new(start_ts));
    }

    if let Some(end_ts) = end_ts {
        conditions.push("created_at <= ?".to_string());
        params.push(Box::new(end_ts));
    }

    (format!("WHERE {}", conditions.join(" AND ")), params)
}

fn build_usage_detail_filters(
    filters: &UsageLogFilters,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(app_type) = &filters.app_type {
        conditions.push("l.app_type = ?".to_string());
        params.push(Box::new(app_type.clone()));
    }
    if let Some(provider_name) = &filters.provider_name {
        conditions.push("p.name LIKE ?".to_string());
        params.push(Box::new(format!("%{provider_name}%")));
    }
    if let Some(model) = &filters.model {
        conditions.push("l.model LIKE ?".to_string());
        params.push(Box::new(format!("%{model}%")));
    }
    if let Some(status_code) = filters.status_code {
        conditions.push("l.status_code = ?".to_string());
        params.push(Box::new(status_code as i64));
    }
    if let Some(start_date) = filters.start_date {
        conditions.push("l.created_at >= ?".to_string());
        params.push(Box::new(start_date));
    }
    if let Some(end_date) = filters.end_date {
        conditions.push("l.created_at <= ?".to_string());
        params.push(Box::new(end_date));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    (where_clause, params)
}

fn parse_usage_date(date: &str, end_of_day: bool) -> Result<i64, AppError> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
        AppError::InvalidInput(format!(
            "Invalid date '{date}'. Expected format: YYYY-MM-DD"
        ))
    })?;

    let datetime = if end_of_day {
        parsed.and_hms_milli_opt(23, 59, 59, 999)
    } else {
        parsed.and_hms_opt(0, 0, 0)
    }
    .ok_or_else(|| AppError::InvalidInput(format!("Invalid date '{date}'")))?;

    Ok(datetime.and_utc().timestamp_millis())
}

fn format_usage_timestamp(created_at: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(created_at)
        .map(|datetime| datetime.to_rfc3339())
        .unwrap_or_else(|| created_at.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, UniversalProvider};
    use rusqlite::params;
    use serde_json::json;

    #[test]
    fn universal_providers_round_trip_through_database() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut provider = UniversalProvider::new(
            "universal-openrouter".to_string(),
            "OpenRouter".to_string(),
            "openai-compatible".to_string(),
            "https://openrouter.ai/api/v1".to_string(),
            "test-key".to_string(),
        );
        provider.apps.claude = true;
        provider.apps.codex = true;

        db.save_universal_provider(&provider)?;

        let providers = db.get_all_universal_providers()?;
        let saved = providers
            .get("universal-openrouter")
            .expect("saved universal provider should exist");

        assert_eq!(saved.name, "OpenRouter");
        assert!(saved.apps.claude);
        assert!(saved.apps.codex);
        assert!(!saved.apps.gemini);

        Ok(())
    }

    #[test]
    fn usage_summary_filters_by_app_and_groups_models() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp_millis();
        let stale = now - chrono::Duration::days(10).num_milliseconds();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-claude-1",
                    "p1",
                    "claude",
                    "claude-sonnet",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    now
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-claude-2",
                    "p1",
                    "claude",
                    "claude-haiku",
                    40,
                    10,
                    "0.005",
                    90,
                    200,
                    now
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-codex-1",
                    "p2",
                    "codex",
                    "gpt-5",
                    200,
                    100,
                    "0.02",
                    120,
                    200,
                    now
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-claude-stale",
                    "p1",
                    "claude",
                    "claude-sonnet",
                    999,
                    1,
                    "9.99",
                    80,
                    200,
                    stale
                ],
            )?;
        }

        let summary = db.get_usage_summary("claude", 7)?;

        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.total_tokens, 200);
        assert!((summary.total_cost - 0.015).abs() < f64::EPSILON);
        assert_eq!(summary.requests_by_model.get("claude-sonnet"), Some(&1));
        assert_eq!(summary.requests_by_model.get("claude-haiku"), Some(&1));
        assert!(!summary.requests_by_model.contains_key("gpt-5"));

        Ok(())
    }

    #[test]
    fn request_logs_filter_by_app_and_date_range() -> Result<(), AppError> {
        let db = Database::memory()?;
        let march_4 = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .expect("valid date")
            .and_hms_opt(12, 0, 0)
            .expect("valid time")
            .and_utc()
            .timestamp_millis();
        let march_6 = chrono::NaiveDate::from_ymd_opt(2026, 3, 6)
            .expect("valid date")
            .and_hms_opt(9, 30, 0)
            .expect("valid time")
            .and_utc()
            .timestamp_millis();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-claude-older",
                    "p1",
                    "claude",
                    "claude-sonnet",
                    10,
                    5,
                    "0.001",
                    100,
                    200,
                    march_4
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-codex-same-day",
                    "p2",
                    "codex",
                    "gpt-5",
                    20,
                    10,
                    "0.002",
                    100,
                    200,
                    march_6
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-claude-latest",
                    "p1",
                    "claude",
                    "claude-haiku",
                    30,
                    15,
                    "0.003",
                    100,
                    200,
                    march_6
                ],
            )?;
        }

        let logs = db.get_request_logs("claude", Some("2026-03-05"), Some("2026-03-06"))?;

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].model, "claude-haiku");
        assert_eq!(logs[0].total_tokens, 45);
        assert!((logs[0].cost - 0.003).abs() < f64::EPSILON);
        assert!(logs[0].timestamp.starts_with("2026-03-06T09:30:00"));

        Ok(())
    }

    #[test]
    fn usage_provider_and_model_stats_are_aggregated() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["p1", "claude", "Claude Main", "{}", "{}"],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-1",
                    "p1",
                    "claude",
                    "claude-sonnet",
                    10,
                    5,
                    "0.010000",
                    100,
                    200,
                    1_741_000_000_000i64
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-2",
                    "p1",
                    "claude",
                    "claude-sonnet",
                    20,
                    10,
                    "0.020000",
                    200,
                    500,
                    1_741_000_100_000i64
                ],
            )?;
        }

        let provider_stats = db.get_usage_provider_stats()?;
        assert_eq!(provider_stats.len(), 1);
        assert_eq!(provider_stats[0].provider_name, "Claude Main");
        assert_eq!(provider_stats[0].request_count, 2);
        assert_eq!(provider_stats[0].total_tokens, 45);

        let model_stats = db.get_usage_model_stats()?;
        assert_eq!(model_stats.len(), 1);
        assert_eq!(model_stats[0].model, "claude-sonnet");
        assert_eq!(model_stats[0].request_count, 2);
        assert_eq!(model_stats[0].avg_cost_per_request, "0.015000");

        Ok(())
    }

    #[test]
    fn usage_log_details_support_filters_and_detail_lookup() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["p9", "gemini", "Gemini Alpha", "{}", "{}"],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, first_token_ms, duration_ms, status_code, error_message, is_streaming, cost_multiplier, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-detail",
                    "p9",
                    "gemini",
                    "gemini-2.5-pro",
                    "gemini-2.5-pro",
                    111,
                    222,
                    3,
                    4,
                    "0.010000",
                    "0.020000",
                    "0.001000",
                    "0.002000",
                    "0.033000",
                    321,
                    111,
                    654,
                    200,
                    Option::<String>::None,
                    1,
                    "1.0",
                    1_741_000_200_000i64
                ],
            )?;
        }

        let filters = UsageLogFilters {
            app_type: Some("gemini".into()),
            provider_name: Some("Alpha".into()),
            ..Default::default()
        };
        let page = db.get_usage_log_details(&filters, 0, 20)?;
        assert_eq!(page.total, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].request_id, "req-detail");

        let detail = db
            .get_usage_request_detail("req-detail")?
            .expect("detail should exist");
        assert_eq!(detail.provider_name.as_deref(), Some("Gemini Alpha"));
        assert!(detail.is_streaming);
        assert_eq!(detail.total_cost_usd, "0.033000");

        Ok(())
    }

    #[test]
    fn failover_queue_round_trip_through_provider_flags() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_provider(
            "claude",
            &Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None),
        )?;
        db.save_provider(
            "claude",
            &Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None),
        )?;

        db.add_to_failover_queue("claude", "b")?;
        db.add_to_failover_queue("claude", "a")?;

        let queue = db.get_failover_queue("claude")?;
        assert_eq!(queue.len(), 2);
        assert!(db.is_in_failover_queue("claude", "a")?);
        assert!(db.is_in_failover_queue("claude", "b")?);

        db.remove_from_failover_queue("claude", "a")?;
        assert!(!db.is_in_failover_queue("claude", "a")?);
        assert!(db.is_in_failover_queue("claude", "b")?);

        db.clear_failover_queue("claude")?;
        assert!(db.get_failover_queue("claude")?.is_empty());

        Ok(())
    }

    #[test]
    fn live_backup_round_trip_through_database() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert!(!db.has_any_live_backup()?);

        db.save_live_backup("claude", "{\"env\":{\"ANTHROPIC_AUTH_TOKEN\":\"abc\"}}")?;
        assert!(db.has_any_live_backup()?);

        let backup = db.get_live_backup("claude")?.expect("backup should exist");
        assert_eq!(backup.app_type, "claude");
        assert!(backup.original_config.contains("ANTHROPIC_AUTH_TOKEN"));

        db.delete_live_backup("claude")?;
        assert!(db.get_live_backup("claude")?.is_none());

        Ok(())
    }
}
