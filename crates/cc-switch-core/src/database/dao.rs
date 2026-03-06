//! Database DAO (Data Access Object) methods

use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

use crate::app_config::{InstalledSkill, McpApps, McpServer, SkillApps};
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::services::proxy::{ProxyConfig, ProxyTakeoverStatus, RequestLog, UsageSummary};
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
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
             FROM providers WHERE app_type = ? ORDER BY sort_index, created_at"
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
                    meta: parse_json_opt(row.get(10)?),
                    in_failover_queue: row.get(12)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = IndexMap::new();
        for p in providers {
            map.insert(p.id.clone(), p);
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
        let conn = lock_conn!(self.conn);
        let meta_json = to_json_string(&provider.meta)?;
        let config_json = to_json_string(&provider.settings_config)?;

        conn.execute(
            "INSERT OR REPLACE INTO providers (id, app_type, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue)
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
            "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
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
                "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
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
            "INSERT OR REPLACE INTO mcp_servers (id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
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
                    },
                    installed_at: row.get(12)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(skills)
    }

    pub fn get_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn
            .query_row(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
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
                        },
                        installed_at: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result)
    }

    pub fn save_skill(&self, skill: &InstalledSkill) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills (id, name, description, directory, repo_owner, repo_name, repo_branch, readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                "currentProviderClaude" => settings.current_provider_claude = Some(value),
                "currentProviderCodex" => settings.current_provider_codex = Some(value),
                "currentProviderGemini" => settings.current_provider_gemini = Some(value),
                "currentProviderOpenCode" => settings.current_provider_opencode = Some(value),
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
            "UPDATE proxy_config SET proxy_enabled = ? WHERE app_type = ?",
            params![enabled, app],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_proxy_takeover_status(&self) -> Result<ProxyTakeoverStatus, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT app_type, proxy_enabled FROM proxy_config")
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
        Ok(())
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

    pub fn get_usage_summary(&self, app: &str, days: u32) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let since = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let since_ts = since.timestamp_millis();

        let total_requests: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE created_at >= ?",
                [since_ts],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_tokens: u64 = conn
            .query_row(
                "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM proxy_request_logs WHERE created_at >= ?",
                [since_ts],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_cost: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) FROM proxy_request_logs WHERE created_at >= ?",
                [since_ts],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Ok(UsageSummary {
            total_requests,
            total_tokens,
            total_cost,
            requests_by_model: HashMap::new(),
        })
    }

    pub fn get_request_logs(
        &self,
        app: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<RequestLog>, AppError> {
        Ok(vec![])
    }

    // ========== Export Methods ==========

    pub fn export_all_providers(&self) -> Result<serde_json::Value, AppError> {
        let mut result = serde_json::Map::new();
        for app in ["claude", "codex", "gemini", "opencode"] {
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
        for app in ["claude", "codex", "gemini", "opencode"] {
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
