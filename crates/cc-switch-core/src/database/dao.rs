//! Database DAO (Data Access Object) methods

use indexmap::IndexMap;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

use crate::app_config::{InstalledSkill, McpApps, McpServer, SkillApps};
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::proxy::types::{
    AppProxyConfig, FailoverQueueItem, GlobalProxyConfig, LiveBackup, LogConfig, ProviderHealth,
    ProxyConfig, ProxyTakeoverStatus, RectifierConfig,
};
use crate::proxy::CircuitBreakerConfig;
use crate::services::skill::SkillRepo;
use crate::services::stream_check::{StreamCheckConfig, StreamCheckResult};
use crate::services::usage::{
    DetailedUsageSummary, ModelPricingInfo, PaginatedUsageLogs, ProviderLimitStatus, RequestLog,
    UsageLogDetail, UsageLogFilters, UsageModelStat, UsageProviderStat, UsageSummary,
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
            let endpoints = load_provider_endpoints(&conn, &provider.id, app_type)?;

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
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at, last_used)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    provider.id,
                    app_type,
                    url,
                    endpoint.added_at,
                    endpoint.last_used
                ],
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

    pub fn delete_setting(&self, key: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_config_snippet(&self, app_type: &str) -> Result<Option<String>, AppError> {
        self.get_setting(&format!("common_config_{app_type}"))
    }

    pub fn set_config_snippet(
        &self,
        app_type: &str,
        snippet: Option<String>,
    ) -> Result<(), AppError> {
        let key = format!("common_config_{app_type}");
        match snippet {
            Some(value) if !value.trim().is_empty() => self.set_setting(&key, &value),
            _ => self.delete_setting(&key),
        }
    }

    const GLOBAL_PROXY_URL_KEY: &'static str = "global_proxy_url";

    pub fn get_global_proxy_url(&self) -> Result<Option<String>, AppError> {
        self.get_setting(Self::GLOBAL_PROXY_URL_KEY)
    }

    pub fn set_global_proxy_url(&self, url: Option<&str>) -> Result<(), AppError> {
        match url {
            Some(value) if !value.trim().is_empty() => {
                self.set_setting(Self::GLOBAL_PROXY_URL_KEY, value.trim())
            }
            _ => self.delete_setting(Self::GLOBAL_PROXY_URL_KEY),
        }
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

    fn ensure_proxy_config_row_exists(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let (retries, fb_timeout, idle_timeout, cb_fail, cb_succ, cb_timeout, cb_rate, cb_min) =
            match app_type {
                "claude" => (6, 90, 180, 8, 3, 90, 0.7, 15),
                "codex" => (3, 60, 120, 4, 2, 60, 0.6, 10),
                "gemini" => (5, 60, 120, 4, 2, 60, 0.6, 10),
                _ => (3, 60, 120, 4, 2, 60, 0.6, 10),
            };

        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES (?1, ?2, ?3, ?4, 600, ?5, ?6, ?7, ?8, ?9)",
            params![
                app_type,
                retries,
                fb_timeout,
                idle_timeout,
                cb_fail,
                cb_succ,
                cb_timeout,
                cb_rate,
                cb_min
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    fn ensure_runtime_proxy_config_rows_initialized(&self) -> Result<(), AppError> {
        self.ensure_proxy_config_row_exists("claude")?;
        self.ensure_proxy_config_row_exists("codex")?;
        self.ensure_proxy_config_row_exists("gemini")?;
        Ok(())
    }

    pub fn get_global_proxy_config(&self) -> Result<GlobalProxyConfig, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT proxy_enabled, listen_address, listen_port, enable_logging
             FROM proxy_config WHERE app_type = 'claude'",
            [],
            |row| {
                Ok(GlobalProxyConfig {
                    proxy_enabled: row.get::<_, i32>(0)? != 0,
                    listen_address: row.get(1)?,
                    listen_port: row.get::<_, i32>(2)? as u16,
                    enable_logging: row.get::<_, i32>(3)? != 0,
                })
            },
        );

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                drop(conn);
                self.ensure_runtime_proxy_config_rows_initialized()?;
                Ok(GlobalProxyConfig {
                    proxy_enabled: false,
                    listen_address: "127.0.0.1".to_string(),
                    listen_port: 15721,
                    enable_logging: true,
                })
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn update_global_proxy_config(&self, config: GlobalProxyConfig) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                proxy_enabled = ?1,
                listen_address = ?2,
                listen_port = ?3,
                enable_logging = ?4,
                updated_at = datetime('now')",
            params![
                if config.proxy_enabled { 1 } else { 0 },
                config.listen_address,
                config.listen_port as i32,
                if config.enable_logging { 1 } else { 0 },
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_proxy_config_for_app(&self, app_type: &str) -> Result<AppProxyConfig, AppError> {
        let app_type_owned = app_type.to_string();
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT app_type, enabled, auto_failover_enabled,
                    max_retries, streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                    circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                    circuit_error_rate_threshold, circuit_min_requests
             FROM proxy_config WHERE app_type = ?1",
            [app_type],
            |row| {
                Ok(AppProxyConfig {
                    app_type: row.get(0)?,
                    enabled: row.get::<_, i32>(1)? != 0,
                    auto_failover_enabled: row.get::<_, i32>(2)? != 0,
                    max_retries: row.get::<_, i32>(3)? as u32,
                    streaming_first_byte_timeout: row.get::<_, i32>(4)? as u32,
                    streaming_idle_timeout: row.get::<_, i32>(5)? as u32,
                    non_streaming_timeout: row.get::<_, i32>(6)? as u32,
                    circuit_failure_threshold: row.get::<_, i32>(7)? as u32,
                    circuit_success_threshold: row.get::<_, i32>(8)? as u32,
                    circuit_timeout_seconds: row.get::<_, i32>(9)? as u32,
                    circuit_error_rate_threshold: row.get(10)?,
                    circuit_min_requests: row.get::<_, i32>(11)? as u32,
                })
            },
        );

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                drop(conn);
                self.ensure_runtime_proxy_config_rows_initialized()?;
                let (max_retries, first_byte_timeout, idle_timeout, circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds, circuit_error_rate_threshold, circuit_min_requests) =
                    match app_type {
                        "claude" => (6, 90, 180, 8, 3, 90, 0.7, 15),
                        "codex" => (3, 60, 120, 4, 2, 60, 0.6, 10),
                        "gemini" => (5, 60, 120, 4, 2, 60, 0.6, 10),
                        _ => (3, 60, 120, 4, 2, 60, 0.6, 10),
                    };
                Ok(AppProxyConfig {
                    app_type: app_type_owned,
                    enabled: false,
                    auto_failover_enabled: false,
                    max_retries,
                    streaming_first_byte_timeout: first_byte_timeout,
                    streaming_idle_timeout: idle_timeout,
                    non_streaming_timeout: 600,
                    circuit_failure_threshold,
                    circuit_success_threshold,
                    circuit_timeout_seconds,
                    circuit_error_rate_threshold,
                    circuit_min_requests,
                })
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn update_proxy_config_for_app(&self, config: AppProxyConfig) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                enabled = ?2,
                auto_failover_enabled = ?3,
                max_retries = ?4,
                streaming_first_byte_timeout = ?5,
                streaming_idle_timeout = ?6,
                non_streaming_timeout = ?7,
                circuit_failure_threshold = ?8,
                circuit_success_threshold = ?9,
                circuit_timeout_seconds = ?10,
                circuit_error_rate_threshold = ?11,
                circuit_min_requests = ?12,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            params![
                config.app_type,
                if config.enabled { 1 } else { 0 },
                if config.auto_failover_enabled { 1 } else { 0 },
                config.max_retries as i32,
                config.streaming_first_byte_timeout as i32,
                config.streaming_idle_timeout as i32,
                config.non_streaming_timeout as i32,
                config.circuit_failure_threshold as i32,
                config.circuit_success_threshold as i32,
                config.circuit_timeout_seconds as i32,
                config.circuit_error_rate_threshold,
                config.circuit_min_requests as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_proxy_config(&self, app_type: &str) -> Result<ProxyConfig, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT listen_address, listen_port, max_retries, enable_logging,
                        streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout
                 FROM proxy_config WHERE app_type = ?",
                [app_type],
                |row| {
                    Ok(ProxyConfig {
                        listen_address: row.get(0)?,
                        listen_port: row.get::<_, i32>(1)? as u16,
                        max_retries: row.get::<_, i32>(2)? as u8,
                        request_timeout: 600,
                        enable_logging: row.get::<_, i32>(3)? != 0,
                        live_takeover_active: false,
                        streaming_first_byte_timeout: row.get::<_, i32>(4).unwrap_or(60) as u64,
                        streaming_idle_timeout: row.get::<_, i32>(5).unwrap_or(120) as u64,
                        non_streaming_timeout: row.get::<_, i32>(6).unwrap_or(600) as u64,
                    })
                },
            )
            .map_err(|e| AppError::Database(e.to_string()));

        match result {
            Ok(config) => Ok(config),
            Err(AppError::Database(message)) if message.contains("Query returned no rows") => {
                self.ensure_runtime_proxy_config_rows_initialized()?;
                Ok(ProxyConfig::default())
            }
            Err(err) => Err(err),
        }
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

    pub fn update_proxy_config(&self, config: ProxyConfig) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                listen_address = ?1,
                listen_port = ?2,
                max_retries = ?3,
                enable_logging = ?4,
                streaming_first_byte_timeout = ?5,
                streaming_idle_timeout = ?6,
                non_streaming_timeout = ?7,
                updated_at = datetime('now')",
            params![
                config.listen_address,
                config.listen_port as i32,
                config.max_retries as i32,
                if config.enable_logging { 1 } else { 0 },
                config.streaming_first_byte_timeout as i32,
                config.streaming_idle_timeout as i32,
                config.non_streaming_timeout as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn set_live_takeover_active(&self, _active: bool) -> Result<(), AppError> {
        Ok(())
    }

    pub fn is_live_takeover_active(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_config WHERE enabled = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count > 0)
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

        let mut status = ProxyTakeoverStatus::default();
        for (app, enabled) in pairs {
            match app.as_str() {
                "claude" => status.claude = enabled,
                "codex" => status.codex = enabled,
                "gemini" => status.gemini = enabled,
                "opencode" => status.opencode = enabled,
                "openclaw" => status.openclaw = enabled,
                _ => {}
            }
        }

        Ok(status)
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

    pub fn get_available_providers_for_failover(
        &self,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let all_providers = self.get_all_providers(app_type)?;
        Ok(all_providers
            .into_values()
            .filter(|provider| !provider.in_failover_queue)
            .collect())
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

    pub fn get_provider_health(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderHealth, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT provider_id, app_type, is_healthy, consecutive_failures,
                    last_success_at, last_failure_at, last_error, updated_at
             FROM provider_health
             WHERE provider_id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
            |row| {
                Ok(ProviderHealth {
                    provider_id: row.get(0)?,
                    app_type: row.get(1)?,
                    is_healthy: row.get::<_, i64>(2)? != 0,
                    consecutive_failures: row.get::<_, i64>(3)? as u32,
                    last_success_at: row.get(4)?,
                    last_failure_at: row.get(5)?,
                    last_error: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        );

        match result {
            Ok(health) => Ok(health),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ProviderHealth {
                provider_id: provider_id.to_string(),
                app_type: app_type.to_string(),
                is_healthy: true,
                consecutive_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                updated_at: chrono::Utc::now().to_rfc3339(),
            }),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn update_provider_health_with_threshold(
        &self,
        provider_id: &str,
        app_type: &str,
        success: bool,
        error_msg: Option<String>,
        failure_threshold: u32,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = chrono::Utc::now().to_rfc3339();
        let current = conn.query_row(
            "SELECT consecutive_failures FROM provider_health
             WHERE provider_id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
            |row| Ok(row.get::<_, i64>(0)? as u32),
        );

        let (is_healthy, consecutive_failures) = if success {
            (1, 0)
        } else {
            let failures = current.unwrap_or(0) + 1;
            let healthy = if failures >= failure_threshold { 0 } else { 1 };
            (healthy, failures)
        };

        let (last_success_at, last_failure_at) = if success {
            (Some(now.clone()), None)
        } else {
            (None, Some(now.clone()))
        };

        conn.execute(
            "INSERT OR REPLACE INTO provider_health
             (provider_id, app_type, is_healthy, consecutive_failures,
              last_success_at, last_failure_at, last_error, updated_at)
             VALUES (?1, ?2, ?3, ?4,
                     COALESCE(?5, (SELECT last_success_at FROM provider_health
                                   WHERE provider_id = ?1 AND app_type = ?2)),
                     COALESCE(?6, (SELECT last_failure_at FROM provider_health
                                   WHERE provider_id = ?1 AND app_type = ?2)),
                     ?7, ?8)",
            params![
                provider_id,
                app_type,
                is_healthy,
                consecutive_failures as i64,
                last_success_at,
                last_failure_at,
                error_msg,
                &now,
            ],
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

    pub fn clear_all_provider_health(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM provider_health", [])
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

    pub fn get_circuit_breaker_config(&self) -> Result<CircuitBreakerConfig, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                    circuit_error_rate_threshold, circuit_min_requests
             FROM proxy_config WHERE app_type = 'claude'",
            [],
            |row| {
                Ok(CircuitBreakerConfig {
                    failure_threshold: row.get::<_, i32>(0)? as u32,
                    success_threshold: row.get::<_, i32>(1)? as u32,
                    timeout_seconds: row.get::<_, i64>(2)? as u64,
                    error_rate_threshold: row.get(3)?,
                    min_requests: row.get::<_, i32>(4)? as u32,
                })
            },
        );

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                drop(conn);
                self.ensure_runtime_proxy_config_rows_initialized()?;
                Ok(CircuitBreakerConfig::default())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn update_circuit_breaker_config(
        &self,
        config: &CircuitBreakerConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                circuit_failure_threshold = ?1,
                circuit_success_threshold = ?2,
                circuit_timeout_seconds = ?3,
                circuit_error_rate_threshold = ?4,
                circuit_min_requests = ?5,
                updated_at = datetime('now')",
            params![
                config.failure_threshold as i32,
                config.success_threshold as i32,
                config.timeout_seconds as i64,
                config.error_rate_threshold,
                config.min_requests as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_default_cost_multiplier(&self, app_type: &str) -> Result<String, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT default_cost_multiplier FROM proxy_config WHERE app_type = ?1",
            [app_type],
            |row| row.get(0),
        );
        match result {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                drop(conn);
                self.ensure_runtime_proxy_config_rows_initialized()?;
                Ok("1".to_string())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn set_default_cost_multiplier(&self, app_type: &str, value: &str) -> Result<(), AppError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AppError::localized(
                "error.multiplierEmpty",
                "倍率不能为空",
                "Multiplier cannot be empty",
            ));
        }
        trimmed.parse::<rust_decimal::Decimal>().map_err(|e| {
            AppError::localized(
                "error.invalidMultiplier",
                format!("无效倍率: {value} - {e}"),
                format!("Invalid multiplier: {value} - {e}"),
            )
        })?;

        self.ensure_proxy_config_row_exists(app_type)?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                default_cost_multiplier = ?2,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            params![app_type, trimmed],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn get_pricing_model_source(&self, app_type: &str) -> Result<String, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT pricing_model_source FROM proxy_config WHERE app_type = ?1",
            [app_type],
            |row| row.get(0),
        );
        match result {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                drop(conn);
                self.ensure_runtime_proxy_config_rows_initialized()?;
                Ok("response".to_string())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn set_pricing_model_source(&self, app_type: &str, value: &str) -> Result<(), AppError> {
        let trimmed = value.trim();
        if !matches!(trimmed, "response" | "request") {
            return Err(AppError::localized(
                "error.invalidPricingMode",
                format!("无效计费模式: {value}"),
                format!("Invalid pricing mode: {value}"),
            ));
        }

        self.ensure_proxy_config_row_exists(app_type)?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                pricing_model_source = ?2,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            params![app_type, trimmed],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn get_rectifier_config(&self) -> Result<RectifierConfig, AppError> {
        match self.get_setting("rectifier_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Database(format!("解析整流器配置失败: {e}"))),
            None => Ok(RectifierConfig::default()),
        }
    }

    pub fn set_rectifier_config(&self, config: &RectifierConfig) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Database(format!("序列化整流器配置失败: {e}")))?;
        self.set_setting("rectifier_config", &json)
    }

    pub fn get_log_config(&self) -> Result<LogConfig, AppError> {
        match self.get_setting("log_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Database(format!("解析日志配置失败: {e}"))),
            None => Ok(LogConfig::default()),
        }
    }

    pub fn set_log_config(&self, config: &LogConfig) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Database(format!("序列化日志配置失败: {e}")))?;
        self.set_setting("log_config", &json)
    }

    pub fn get_usage_summary(&self, app: &str, days: u32) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let since = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let since_ts = since.timestamp_millis();

        self.query_usage_summary(app, None, Some(since_ts), None, &conn)
    }

    pub fn get_usage_summary_all(&self, app: &str) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        self.query_usage_summary(app, None, None, None, &conn)
    }

    pub fn get_usage_detailed_summary(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<DetailedUsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let created_at_expr = normalized_usage_timestamp_sql("created_at");
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start_date) = start_date {
            conditions.push(format!("{created_at_expr} >= ?"));
            params.push(Box::new(start_date));
        }

        if let Some(end_date) = end_date {
            conditions.push(format!("{created_at_expr} <= ?"));
            params.push(Box::new(end_date));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|item| item.as_ref()).collect();

        let sql = format!(
            "SELECT
                COUNT(*) as total_requests,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as total_cache_read_tokens,
                COALESCE(SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
             FROM proxy_request_logs
             {where_clause}"
        );

        conn.query_row(&sql, params_refs.as_slice(), |row| {
            let total_requests = row.get::<_, i64>(0)? as u64;
            let total_cost = row.get::<_, f64>(1)?;
            let total_input_tokens = row.get::<_, i64>(2)? as u64;
            let total_output_tokens = row.get::<_, i64>(3)? as u64;
            let total_cache_creation_tokens = row.get::<_, i64>(4)? as u64;
            let total_cache_read_tokens = row.get::<_, i64>(5)? as u64;
            let success_count = row.get::<_, i64>(6)? as u64;
            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };

            Ok(DetailedUsageSummary {
                total_requests,
                total_cost: format!("{total_cost:.6}"),
                total_input_tokens,
                total_output_tokens,
                total_cache_creation_tokens,
                total_cache_read_tokens,
                success_rate,
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_provider_usage_summary(
        &self,
        app: &str,
        provider_id: &str,
        days: u32,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let since = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let since_ts = since.timestamp_millis();

        self.query_usage_summary(app, Some(provider_id), Some(since_ts), None, &conn)
    }

    pub fn get_provider_usage_summary_all(
        &self,
        app: &str,
        provider_id: &str,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        self.query_usage_summary(app, Some(provider_id), None, None, &conn)
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
        let (where_clause, params) = build_usage_filters(app, None, start_ts, end_ts);
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let created_at_expr = normalized_usage_timestamp_sql("created_at");

        let sql = format!(
            "SELECT {created_at_expr} AS normalized_created_at, model, input_tokens + output_tokens, CAST(total_cost_usd AS REAL)
             FROM proxy_request_logs
             {where_clause}
             ORDER BY normalized_created_at DESC"
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

        let created_at_expr = normalized_usage_timestamp_sql("created_at");
        let sql = format!(
            "
            SELECT
                CAST(({created_at_expr} - ?1) / ?3 AS INTEGER) as bucket_idx,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as total_cache_read_tokens
            FROM proxy_request_logs
            WHERE {created_at_expr} >= ?1 AND {created_at_expr} <= ?2
            GROUP BY bucket_idx
            ORDER BY bucket_idx ASC"
        );

        let mut stmt = conn
            .prepare(&sql)
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
        let created_at_expr = normalized_usage_timestamp_sql("l.created_at");

        let sql = format!(
            "SELECT l.request_id, l.provider_id, p.name, l.app_type, l.model, l.request_model,
                    COALESCE(l.cost_multiplier, '1'),
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, {created_at_expr} AS normalized_created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}
             ORDER BY normalized_created_at DESC
             LIMIT ? OFFSET ?"
        );

        let mut data = {
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

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Database(e.to_string()))?
        };

        let mut provider_cache = HashMap::new();
        let mut pricing_cache = HashMap::new();
        for item in &mut data {
            Self::maybe_backfill_log_costs(&conn, item, &mut provider_cache, &mut pricing_cache)?;
        }

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
        let created_at_expr = normalized_usage_timestamp_sql("l.created_at");
        let sql = format!(
            "SELECT l.request_id, l.provider_id, p.name, l.app_type, l.model, l.request_model,
                    COALESCE(l.cost_multiplier, '1'),
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, {created_at_expr} AS normalized_created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?1
             LIMIT 1"
        );

        let result = conn
            .query_row(&sql, [request_id], |row| {
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
            .map_err(|e| AppError::Database(e.to_string()))?;

        match result {
            Some(mut detail) => {
                let mut provider_cache = HashMap::new();
                let mut pricing_cache = HashMap::new();
                Self::maybe_backfill_log_costs(
                    &conn,
                    &mut detail,
                    &mut provider_cache,
                    &mut pricing_cache,
                )?;
                Ok(Some(detail))
            }
            None => Ok(None),
        }
    }

    pub fn get_model_pricing(&self) -> Result<Vec<ModelPricingInfo>, AppError> {
        self.ensure_model_pricing_seeded()?;
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million
                 FROM model_pricing
                 ORDER BY display_name, model_id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ModelPricingInfo {
                    model_id: row.get(0)?,
                    display_name: row.get(1)?,
                    input_cost_per_million: row.get(2)?,
                    output_cost_per_million: row.get(3)?,
                    cache_read_cost_per_million: row.get(4)?,
                    cache_creation_cost_per_million: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn upsert_model_pricing(&self, pricing: &ModelPricingInfo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                pricing.model_id,
                pricing.display_name,
                pricing.input_cost_per_million,
                pricing.output_cost_per_million,
                pricing.cache_read_cost_per_million,
                pricing.cache_creation_cost_per_million
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
        Ok(())
    }

    pub fn delete_model_pricing(&self, model_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM model_pricing WHERE model_id = ?1", [model_id])
            .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;
        Ok(())
    }

    pub fn check_provider_limits(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        let conn = lock_conn!(self.conn);
        let (limit_daily, limit_monthly) = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider_id, app_type],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("查询 provider meta 失败: {e}")))?
            .and_then(|meta| serde_json::from_str::<Value>(&meta).ok())
            .map(|meta| {
                let daily = meta
                    .get("limitDailyUsd")
                    .and_then(|value| value.as_str())
                    .and_then(|value| value.parse::<f64>().ok());
                let monthly = meta
                    .get("limitMonthlyUsd")
                    .and_then(|value| value.as_str())
                    .and_then(|value| value.parse::<f64>().ok());
                (daily, monthly)
            })
            .unwrap_or((None, None));

        let daily_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
                 FROM proxy_request_logs
                 WHERE provider_id = ?1 AND app_type = ?2
                   AND date(datetime((CASE WHEN ABS(created_at) < 100000000000 THEN created_at * 1000 ELSE created_at END) / 1000, 'unixepoch', 'localtime')) = date('now', 'localtime')",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let monthly_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
                 FROM proxy_request_logs
                 WHERE provider_id = ?1 AND app_type = ?2
                   AND strftime('%Y-%m', datetime((CASE WHEN ABS(created_at) < 100000000000 THEN created_at * 1000 ELSE created_at END) / 1000, 'unixepoch', 'localtime')) = strftime('%Y-%m', 'now', 'localtime')",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Ok(ProviderLimitStatus {
            provider_id: provider_id.to_string(),
            daily_usage: format!("{daily_usage:.6}"),
            daily_limit: limit_daily.map(|value| format!("{value:.2}")),
            daily_exceeded: limit_daily.is_some_and(|limit| daily_usage >= limit),
            monthly_usage: format!("{monthly_usage:.6}"),
            monthly_limit: limit_monthly.map(|value| format!("{value:.2}")),
            monthly_exceeded: limit_monthly.is_some_and(|limit| monthly_usage >= limit),
        })
    }

    pub fn save_stream_check_log(
        &self,
        provider_id: &str,
        provider_name: &str,
        app_type: &str,
        result: &StreamCheckResult,
    ) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO stream_check_logs
             (provider_id, provider_name, app_type, status, success, message,
              response_time_ms, http_status, model_used, retry_count, tested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                provider_id,
                provider_name,
                app_type,
                format!("{:?}", result.status).to_lowercase(),
                result.success,
                result.message,
                result.response_time_ms.map(|value| value as i64),
                result.http_status.map(|value| value as i64),
                result.model_used,
                result.retry_count as i64,
                result.tested_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(conn.last_insert_rowid())
    }

    pub fn get_stream_check_config(&self) -> Result<StreamCheckConfig, AppError> {
        match self.get_setting("stream_check_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Message(format!("解析配置失败: {e}"))),
            None => Ok(StreamCheckConfig::default()),
        }
    }

    pub fn save_stream_check_config(&self, config: &StreamCheckConfig) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Message(format!("序列化配置失败: {e}")))?;
        self.set_setting("stream_check_config", &json)
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

#[derive(Clone)]
struct PricingInfo {
    input: rust_decimal::Decimal,
    output: rust_decimal::Decimal,
    cache_read: rust_decimal::Decimal,
    cache_creation: rust_decimal::Decimal,
}

impl Database {
    fn query_usage_summary(
        &self,
        app: &str,
        provider_id: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        conn: &Connection,
    ) -> Result<UsageSummary, AppError> {
        let (where_clause, params) = build_usage_filters(app, provider_id, start_ts, end_ts);
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

    fn maybe_backfill_log_costs(
        conn: &Connection,
        log: &mut UsageLogDetail,
        provider_cache: &mut HashMap<(String, String), rust_decimal::Decimal>,
        pricing_cache: &mut HashMap<String, PricingInfo>,
    ) -> Result<(), AppError> {
        let total_cost = rust_decimal::Decimal::from_str(&log.total_cost_usd)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let has_cost = total_cost > rust_decimal::Decimal::ZERO;
        let has_usage = log.input_tokens > 0
            || log.output_tokens > 0
            || log.cache_read_tokens > 0
            || log.cache_creation_tokens > 0;

        if has_cost || !has_usage {
            return Ok(());
        }

        let pricing = match Self::get_model_pricing_cached(conn, pricing_cache, &log.model)? {
            Some(info) => info,
            None => return Ok(()),
        };
        let multiplier = Self::get_cost_multiplier_cached(
            conn,
            provider_cache,
            &log.provider_id,
            &log.app_type,
        )?;

        let million = rust_decimal::Decimal::from(1_000_000u64);
        let billable_input_tokens =
            (log.input_tokens as u64).saturating_sub(log.cache_read_tokens as u64);
        let input_cost =
            rust_decimal::Decimal::from(billable_input_tokens) * pricing.input / million;
        let output_cost =
            rust_decimal::Decimal::from(log.output_tokens as u64) * pricing.output / million;
        let cache_read_cost = rust_decimal::Decimal::from(log.cache_read_tokens as u64)
            * pricing.cache_read
            / million;
        let cache_creation_cost = rust_decimal::Decimal::from(log.cache_creation_tokens as u64)
            * pricing.cache_creation
            / million;
        let base_total = input_cost + output_cost + cache_read_cost + cache_creation_cost;
        let total_cost = base_total * multiplier;

        log.input_cost_usd = format!("{input_cost:.6}");
        log.output_cost_usd = format!("{output_cost:.6}");
        log.cache_read_cost_usd = format!("{cache_read_cost:.6}");
        log.cache_creation_cost_usd = format!("{cache_creation_cost:.6}");
        log.total_cost_usd = format!("{total_cost:.6}");

        conn.execute(
            "UPDATE proxy_request_logs
             SET input_cost_usd = ?1,
                 output_cost_usd = ?2,
                 cache_read_cost_usd = ?3,
                 cache_creation_cost_usd = ?4,
                 total_cost_usd = ?5
             WHERE request_id = ?6",
            params![
                log.input_cost_usd,
                log.output_cost_usd,
                log.cache_read_cost_usd,
                log.cache_creation_cost_usd,
                log.total_cost_usd,
                log.request_id
            ],
        )
        .map_err(|e| AppError::Database(format!("更新请求成本失败: {e}")))?;

        Ok(())
    }

    fn get_cost_multiplier_cached(
        conn: &Connection,
        cache: &mut HashMap<(String, String), rust_decimal::Decimal>,
        provider_id: &str,
        app_type: &str,
    ) -> Result<rust_decimal::Decimal, AppError> {
        let key = (provider_id.to_string(), app_type.to_string());
        if let Some(multiplier) = cache.get(&key) {
            return Ok(*multiplier);
        }

        let meta_json: Option<String> = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("查询 provider meta 失败: {e}")))?;

        let multiplier = meta_json
            .and_then(|meta| serde_json::from_str::<Value>(&meta).ok())
            .and_then(|value| value.get("costMultiplier").cloned())
            .and_then(|value| {
                value
                    .as_str()
                    .and_then(|s| rust_decimal::Decimal::from_str(s).ok())
            })
            .unwrap_or(rust_decimal::Decimal::ONE);

        cache.insert(key, multiplier);
        Ok(multiplier)
    }

    fn get_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, PricingInfo>,
        model: &str,
    ) -> Result<Option<PricingInfo>, AppError> {
        if let Some(info) = cache.get(model) {
            return Ok(Some(info.clone()));
        }

        let row = find_model_pricing_row(conn, model)?;
        let Some((input, output, cache_read, cache_creation)) = row else {
            return Ok(None);
        };

        let pricing = PricingInfo {
            input: rust_decimal::Decimal::from_str(&input)
                .map_err(|e| AppError::Database(format!("解析输入价格失败: {e}")))?,
            output: rust_decimal::Decimal::from_str(&output)
                .map_err(|e| AppError::Database(format!("解析输出价格失败: {e}")))?,
            cache_read: rust_decimal::Decimal::from_str(&cache_read)
                .map_err(|e| AppError::Database(format!("解析缓存读取价格失败: {e}")))?,
            cache_creation: rust_decimal::Decimal::from_str(&cache_creation)
                .map_err(|e| AppError::Database(format!("解析缓存写入价格失败: {e}")))?,
        };

        cache.insert(model.to_string(), pricing.clone());
        Ok(Some(pricing))
    }
}

pub(crate) fn find_model_pricing_row(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    let cleaned = model_id
        .rsplit_once('/')
        .map_or(model_id, |(_, rest)| rest)
        .split(':')
        .next()
        .unwrap_or(model_id)
        .trim()
        .replace('@', "-");

    let exact = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing
             WHERE model_id = ?1",
            [&cleaned],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| AppError::Database(format!("查询模型定价失败: {e}")))?;

    if exact.is_none() {
        log::warn!("模型 {model_id}（清洗后: {cleaned}）未找到定价信息，成本将记录为 0");
    }

    Ok(exact)
}

fn parse_json<T: DeserializeOwned>(json: String) -> T {
    serde_json::from_str(&json).unwrap_or_else(|_| panic!("Failed to parse JSON: {}", json))
}

fn load_provider_endpoints(
    conn: &Connection,
    provider_id: &str,
    app_type: &str,
) -> Result<Vec<crate::settings::CustomEndpoint>, AppError> {
    let query_with_last_used = || -> Result<Vec<crate::settings::CustomEndpoint>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT url, added_at, last_used FROM provider_endpoints
                 WHERE provider_id = ?1 AND app_type = ?2
                 ORDER BY added_at ASC, url ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![provider_id, app_type], |row| {
                Ok(crate::settings::CustomEndpoint {
                    url: row.get(0)?,
                    added_at: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    last_used: row.get(2)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    };

    match query_with_last_used() {
        Ok(endpoints) => Ok(endpoints),
        Err(AppError::Database(message)) if message.contains("no such column: last_used") => {
            let mut stmt = conn
                .prepare(
                    "SELECT url, added_at FROM provider_endpoints
                     WHERE provider_id = ?1 AND app_type = ?2
                     ORDER BY added_at ASC, url ASC",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![provider_id, app_type], |row| {
                    Ok(crate::settings::CustomEndpoint {
                        url: row.get(0)?,
                        added_at: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        last_used: None,
                    })
                })
                .map_err(|e| AppError::Database(e.to_string()))?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Database(e.to_string()))
        }
        Err(err) => Err(err),
    }
}

fn parse_json_opt<T: DeserializeOwned>(json: Option<String>) -> Option<T> {
    json.and_then(|s| serde_json::from_str(&s).ok())
}

fn build_usage_filters(
    app: &str,
    provider_id: Option<&str>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let created_at_expr = normalized_usage_timestamp_sql("created_at");
    let mut conditions = vec!["app_type = ?".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(app.to_string())];

    if let Some(provider_id) = provider_id {
        conditions.push("provider_id = ?".to_string());
        params.push(Box::new(provider_id.to_string()));
    }

    if let Some(start_ts) = start_ts {
        conditions.push(format!("{created_at_expr} >= ?"));
        params.push(Box::new(start_ts));
    }

    if let Some(end_ts) = end_ts {
        conditions.push(format!("{created_at_expr} <= ?"));
        params.push(Box::new(end_ts));
    }

    (format!("WHERE {}", conditions.join(" AND ")), params)
}

fn build_usage_detail_filters(
    filters: &UsageLogFilters,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let created_at_expr = normalized_usage_timestamp_sql("l.created_at");
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
        conditions.push(format!("{created_at_expr} >= ?"));
        params.push(Box::new(start_date));
    }
    if let Some(end_date) = filters.end_date {
        conditions.push(format!("{created_at_expr} <= ?"));
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

fn normalize_usage_timestamp(created_at: i64) -> i64 {
    if created_at.abs() < 100_000_000_000 {
        created_at.saturating_mul(1000)
    } else {
        created_at
    }
}

fn normalized_usage_timestamp_sql(column: &str) -> String {
    format!("CASE WHEN ABS({column}) < 100000000000 THEN {column} * 1000 ELSE {column} END")
}

fn format_usage_timestamp(created_at: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(normalize_usage_timestamp(created_at))
        .map(|datetime| datetime.to_rfc3339())
        .unwrap_or_else(|| created_at.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, UniversalProvider};
    use rusqlite::params;
    use serde_json::json;
    use std::collections::HashMap;

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
    fn provider_custom_endpoints_round_trip_last_used() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut provider = Provider::with_id(
            "provider-a".to_string(),
            "Provider A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-test"
                }
            }),
            None,
        );

        let mut meta = provider.meta.take().unwrap_or_default();
        meta.custom_endpoints = HashMap::from([(
            "https://edge.example.com/v1".to_string(),
            crate::settings::CustomEndpoint {
                url: "https://edge.example.com/v1".to_string(),
                added_at: 123,
                last_used: Some(456),
            },
        )]);
        provider.meta = Some(meta);

        db.save_provider("claude", &provider)?;

        let providers = db.get_all_providers("claude")?;
        let saved = providers.get("provider-a").expect("provider should exist");
        let endpoint = saved
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_endpoints.get("https://edge.example.com/v1"))
            .expect("custom endpoint should exist");
        assert_eq!(endpoint.added_at, 123);
        assert_eq!(endpoint.last_used, Some(456));

        Ok(())
    }

    #[test]
    fn provider_custom_endpoints_legacy_schema_without_last_used_still_loads(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DROP TABLE provider_endpoints", [])?;
            conn.execute(
                "CREATE TABLE provider_endpoints (
                    provider_id TEXT NOT NULL,
                    app_type TEXT NOT NULL,
                    url TEXT NOT NULL,
                    added_at INTEGER NOT NULL DEFAULT (unixepoch())
                )",
                [],
            )?;
            conn.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category, is_current,
                    created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
                ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, ?5, NULL, NULL, NULL, NULL, ?6, 0)",
                params![
                    "legacy-provider",
                    "claude",
                    "Legacy Provider",
                    json!({
                        "env": {
                            "ANTHROPIC_BASE_URL": "https://legacy.example.com",
                            "ANTHROPIC_AUTH_TOKEN": "sk-legacy"
                        }
                    })
                    .to_string(),
                    123_i64,
                    "{}"
                ],
            )?;
            conn.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "legacy-provider",
                    "claude",
                    "https://legacy.example.com/v1",
                    456_i64
                ],
            )?;
        }

        let providers = db.get_all_providers("claude")?;
        let saved = providers
            .get("legacy-provider")
            .expect("legacy provider should exist");
        let endpoint = saved
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_endpoints.get("https://legacy.example.com/v1"))
            .expect("legacy endpoint should exist");
        assert_eq!(endpoint.added_at, 456);
        assert_eq!(endpoint.last_used, None);

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
    fn legacy_second_timestamps_are_compatible_with_usage_queries() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now_secs = chrono::Utc::now().timestamp();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-legacy-seconds",
                    "p1",
                    "claude",
                    "claude-haiku",
                    12,
                    7,
                    "0.0015",
                    100,
                    200,
                    now_secs
                ],
            )?;
        }

        let summary = db.get_usage_summary("claude", 7)?;
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_tokens, 19);
        assert!((summary.total_cost - 0.0015).abs() < f64::EPSILON);

        let logs = db.get_request_logs("claude", None, None)?;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].model, "claude-haiku");
        assert!(!logs[0].timestamp.starts_with("1970-"));

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
    fn model_pricing_is_seeded_on_database_init() -> Result<(), AppError> {
        let db = Database::memory()?;
        let pricing = db.get_model_pricing()?;
        assert!(!pricing.is_empty());
        assert!(pricing
            .iter()
            .any(|item| item.model_id == "claude-sonnet-4-5-20250929"));
        Ok(())
    }

    #[test]
    fn model_pricing_matching_normalizes_vendor_prefix_and_effort_suffix() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        let matched = find_model_pricing_row(&conn, "moonshotai/gpt-5.2-codex@low:v2")?;
        assert!(matched.is_some());
        Ok(())
    }

    #[test]
    fn request_detail_backfills_costs_from_model_pricing() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "p-cost",
                    "claude",
                    "Claude Cost",
                    "{}",
                    "{\"costMultiplier\":\"2\"}"
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, first_token_ms, duration_ms, status_code, error_message, is_streaming, cost_multiplier, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-backfill",
                    "p-cost",
                    "claude",
                    "claude-sonnet-4-5-20250929",
                    "claude-sonnet-4-5-20250929",
                    1_000_000i64,
                    500_000i64,
                    100_000i64,
                    50_000i64,
                    "0",
                    "0",
                    "0",
                    "0",
                    "0",
                    123,
                    Option::<i64>::None,
                    Option::<i64>::None,
                    200,
                    Option::<String>::None,
                    1,
                    "2",
                    1_741_000_300_000i64
                ],
            )?;
        }

        let detail = db
            .get_usage_request_detail("req-backfill")?
            .expect("detail should exist");
        assert_eq!(detail.input_cost_usd, "2.700000");
        assert_eq!(detail.output_cost_usd, "7.500000");
        assert_eq!(detail.cache_read_cost_usd, "0.030000");
        assert_eq!(detail.cache_creation_cost_usd, "0.187500");
        assert_eq!(detail.total_cost_usd, "20.835000");

        Ok(())
    }

    #[test]
    fn provider_limits_aggregate_daily_and_monthly_costs() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp_millis();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "p-limit",
                    "claude",
                    "Claude Limit",
                    "{}",
                    "{\"limitDailyUsd\":\"1.00\",\"limitMonthlyUsd\":\"5.00\"}"
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-limit-1",
                    "p-limit",
                    "claude",
                    "claude-sonnet",
                    10,
                    5,
                    "0.60",
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
                    "req-limit-2",
                    "p-limit",
                    "claude",
                    "claude-haiku",
                    10,
                    5,
                    "0.50",
                    100,
                    200,
                    now
                ],
            )?;
        }

        let status = db.check_provider_limits("p-limit", "claude")?;
        assert_eq!(status.daily_usage, "1.100000");
        assert_eq!(status.monthly_usage, "1.100000");
        assert!(status.daily_exceeded);
        assert!(!status.monthly_exceeded);

        Ok(())
    }

    #[test]
    fn stream_check_config_round_trip_through_settings() -> Result<(), AppError> {
        let db = Database::memory()?;
        let config = StreamCheckConfig {
            timeout_secs: 12,
            max_retries: 3,
            degraded_threshold_ms: 999,
            claude_model: "claude-custom".to_string(),
            codex_model: "gpt-custom".to_string(),
            gemini_model: "gemini-custom".to_string(),
            test_prompt: "ping".to_string(),
        };

        db.save_stream_check_config(&config)?;
        let saved = db.get_stream_check_config()?;
        assert_eq!(saved.timeout_secs, 12);
        assert_eq!(saved.test_prompt, "ping");

        let result = StreamCheckResult {
            status: crate::services::stream_check::HealthStatus::Operational,
            success: true,
            message: "ok".to_string(),
            response_time_ms: Some(42),
            http_status: Some(200),
            model_used: "claude-custom".to_string(),
            tested_at: 1_741_000_400,
            retry_count: 1,
        };
        let id = db.save_stream_check_log("p-limit", "Claude Limit", "claude", &result)?;
        assert!(id > 0);

        Ok(())
    }

    #[test]
    fn config_snippet_round_trip_and_clear() -> Result<(), AppError> {
        let db = Database::memory()?;
        let snippet = r#"{"env":{"HTTPS_PROXY":"http://127.0.0.1:8080"}}"#.to_string();

        db.set_config_snippet("claude", Some(snippet.clone()))?;
        assert_eq!(db.get_config_snippet("claude")?, Some(snippet));

        db.set_config_snippet("claude", None)?;
        assert_eq!(db.get_config_snippet("claude")?, None);

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
