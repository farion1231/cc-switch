use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use indexmap::IndexMap;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

type OmoProviderRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<usize>,
    Option<String>,
    String,
);

/// 规范化 endpoint / base_url：trim 并去掉末尾 `/`
fn normalize_endpoint_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 从 settings_config 按 app_type 提取当前 base_url（已规范化）
fn extract_settings_base_url(app_type: &str, settings: &serde_json::Value) -> Option<String> {
    let raw = match app_type {
        "claude" | "claude-desktop" => settings
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "gemini" => settings
            .pointer("/env/GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "codex" => settings
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(crate::codex_config::extract_codex_base_url),
        "opencode" => settings
            .pointer("/options/baseURL")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "openclaw" => settings
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "hermes" => settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    };
    raw.as_deref().and_then(normalize_endpoint_url)
}

/// 同步 `provider_endpoints`：
/// 1. 读取 DB 已有端点（更新时保留测速面板 API 新增的端点）
/// 2. 合并入参 `meta.custom_endpoints`
/// 3. base_url 变更时移除旧地址并写入新地址
/// 4. DELETE + reinsert
fn resync_provider_endpoints_in_tx(
    tx: &rusqlite::Transaction<'_>,
    app_type: &str,
    provider_id: &str,
    meta_endpoints: HashMap<String, crate::settings::CustomEndpoint>,
    old_settings: Option<&serde_json::Value>,
    new_settings: &serde_json::Value,
) -> Result<(), AppError> {
    // url -> added_at
    let mut final_map: HashMap<String, i64> = HashMap::new();

    // 1) DB 已有端点
    {
        let mut stmt = tx
            .prepare(
                "SELECT url, added_at FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![provider_id, app_type], |row| {
                let url: String = row.get(0)?;
                let added_at: Option<i64> = row.get(1)?;
                Ok((url, added_at.unwrap_or(0)))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        for row in rows {
            let (url, added_at) = row.map_err(|e| AppError::Database(e.to_string()))?;
            if let Some(normalized) = normalize_endpoint_url(&url) {
                final_map.entry(normalized).or_insert(added_at);
            }
        }
    }

    // 2) 合并 meta.custom_endpoints
    for (url, endpoint) in meta_endpoints {
        if let Some(normalized) = normalize_endpoint_url(&url) {
            final_map.entry(normalized).or_insert(endpoint.added_at);
        }
    }

    // 3) base_url 变更迁移
    let old_base = old_settings.and_then(|s| extract_settings_base_url(app_type, s));
    let new_base = extract_settings_base_url(app_type, new_settings);
    let now_ms = chrono::Utc::now().timestamp_millis();

    if old_base.as_deref() != new_base.as_deref() {
        if let Some(old) = old_base.as_ref() {
            final_map.remove(old);
        }
        if let Some(new_url) = new_base.as_ref() {
            final_map.entry(new_url.clone()).or_insert(now_ms);
        }
    } else if let Some(new_url) = new_base.as_ref() {
        // base_url 未变：仍确保其存在于端点表
        final_map.entry(new_url.clone()).or_insert(now_ms);
    }

    // 4) DELETE + reinsert
    tx.execute(
        "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
        params![provider_id, app_type],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    for (url, added_at) in final_map {
        tx.execute(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![provider_id, app_type, url, added_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    }

    Ok(())
}

impl Database {
    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
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
                let in_failover_queue: bool = row.get(11)?;

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
                        in_failover_queue,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = IndexMap::new();
        for provider_res in provider_iter {
            let (id, mut provider) = provider_res.map_err(|e| AppError::Database(e.to_string()))?;
            provider.id = id.clone();

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

    pub fn get_provider_by_id(
        &self,
        id: &str,
        app_type: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
            |row| {
                let name: String = row.get(0)?;
                let settings_config_str: String = row.get(1)?;
                let website_url: Option<String> = row.get(2)?;
                let category: Option<String> = row.get(3)?;
                let created_at: Option<i64> = row.get(4)?;
                let sort_index: Option<usize> = row.get(5)?;
                let notes: Option<String> = row.get(6)?;
                let icon: Option<String> = row.get(7)?;
                let icon_color: Option<String> = row.get(8)?;
                let meta_str: String = row.get(9)?;
                let in_failover_queue: bool = row.get(10)?;

                let settings_config = serde_json::from_str(&settings_config_str).unwrap_or(serde_json::Value::Null);
                let meta: ProviderMeta = serde_json::from_str(&meta_str).unwrap_or_default();

                Ok(Provider {
                    id: id.to_string(),
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
                    in_failover_queue,
                })
            },
        );

        match result {
            Ok(provider) => Ok(Some(provider)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 保存供应商（新增或更新）
    ///
    /// 更新时同步 `provider_endpoints`：
    /// - 合并 DB 已有端点与入参 `meta.custom_endpoints`
    /// - 当 `settings_config` 中的 base_url 变更时，移除旧 base_url 并写入新 base_url
    /// - 对该 `provider_id + app_type` 做 DELETE + reinsert，保证与 settings 一致
    pub fn save_provider(&self, app_type: &str, provider: &Provider) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut meta_clone = provider.meta.clone().unwrap_or_default();
        let endpoints = std::mem::take(&mut meta_clone.custom_endpoints);

        let existing: Option<(bool, bool, String)> = tx
            .query_row(
                "SELECT is_current, in_failover_queue, settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider.id, app_type],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let is_update = existing.is_some();
        let (is_current, in_failover_queue, old_settings_str) = existing.unwrap_or((
            false,
            provider.in_failover_queue,
            String::new(),
        ));

        if is_update {
            tx.execute(
                "UPDATE providers SET
                    name = ?1,
                    settings_config = ?2,
                    website_url = ?3,
                    category = ?4,
                    created_at = ?5,
                    sort_index = ?6,
                    notes = ?7,
                    icon = ?8,
                    icon_color = ?9,
                    meta = ?10,
                    is_current = ?11,
                    in_failover_queue = ?12
                WHERE id = ?13 AND app_type = ?14",
                params![
                    provider.name,
                    serde_json::to_string(&provider.settings_config).map_err(|e| {
                        AppError::Database(format!("Failed to serialize settings_config: {e}"))
                    })?,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    provider.sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    serde_json::to_string(&meta_clone).map_err(|e| AppError::Database(format!(
                        "Failed to serialize meta: {e}"
                    )))?,
                    is_current,
                    in_failover_queue,
                    provider.id,
                    app_type,
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

            let old_settings: serde_json::Value = serde_json::from_str(&old_settings_str)
                .unwrap_or(serde_json::Value::Null);
            resync_provider_endpoints_in_tx(
                &tx,
                app_type,
                &provider.id,
                endpoints,
                Some(&old_settings),
                &provider.settings_config,
            )?;
        } else {
            tx.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category,
                    created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    provider.id,
                    app_type,
                    provider.name,
                    serde_json::to_string(&provider.settings_config)
                        .map_err(|e| AppError::Database(format!("Failed to serialize settings_config: {e}")))?,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    provider.sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    serde_json::to_string(&meta_clone)
                        .map_err(|e| AppError::Database(format!("Failed to serialize meta: {e}")))?,
                    is_current,
                    in_failover_queue,
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

            // 新建：写入 meta 中的端点，并确保当前 base_url 在列表中
            resync_provider_endpoints_in_tx(
                &tx,
                app_type,
                &provider.id,
                endpoints,
                None,
                &provider.settings_config,
            )?;
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

        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
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
        // OMO ↔ OMO Slim mutually exclusive: deactivate the opposite category
        let opposite = match category {
            "omo" => Some("omo-slim"),
            "omo-slim" => Some("omo"),
            _ => None,
        };
        if let Some(opp) = opposite {
            tx.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
                params![app_type, opp],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        let updated = tx
            .execute(
                "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2 AND category = ?3",
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
        let row_data: Result<OmoProviderRow, rusqlite::Error> = conn.query_row(
            "SELECT id, name, settings_config, category, created_at, sort_index, notes, meta
             FROM providers
             WHERE app_type = ?1 AND category = ?2 AND is_current = 1
             LIMIT 1",
            params![app_type, category],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        );

        let (id, name, settings_config_str, _row_category, created_at, sort_index, notes, meta_str) =
            match row_data {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(AppError::Database(e.to_string())),
            };

        let settings_config = serde_json::from_str(&settings_config_str).map_err(|e| {
            AppError::Database(format!(
                "Failed to parse {category} provider settings_config (provider_id={id}): {e}"
            ))
        })?;
        let meta: crate::provider::ProviderMeta = if meta_str.trim().is_empty() {
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
            category: Some(category.to_string()),
            created_at,
            sort_index,
            notes,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }))
    }

    /// 判断 providers 表是否为空（全 app_type 一起算）。
    ///
    /// 用于区分"全新安装"和"升级用户"：在启动流程 import/seed 之前调用。
    /// 使用 `EXISTS` 短路查询，比 `COUNT(*)` 在将来表变大时更高效。
    pub fn is_providers_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM providers)", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(!exists)
    }

    /// 仅获取指定 app 下所有 provider 的 id 集合。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询。
    /// 用于只需要做存在性检查的场景（如 additive 模式的 live 同步去重）。
    pub fn get_provider_ids(&self, app_type: &str) -> Result<HashSet<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![app_type], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(ids)
    }

    /// 判断指定 app 下是否已存在任意 provider。
    ///
    /// 启动阶段的 live import 需要使用这个更严格的判断：
    /// 只要该 app 已经有任何 provider（包括官方 seed），就不应再自动导入 `default`。
    pub fn has_any_provider_for_app(&self, app_type: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE app_type = ?1)",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(exists)
    }

    /// 判断指定 app 下是否存在非官方种子的供应商。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询、首条命中即返回。
    /// 用于 `import_default_config` 决定是否跳过 live 导入。
    pub fn has_non_official_seed_provider(&self, app_type: &str) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::is_official_seed_id;
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let id: String = row.get(0).map_err(|e| AppError::Database(e.to_string()))?;
            if !is_official_seed_id(&id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 计算指定 app 下一个可用的 sort_index（追加到末尾）。
    fn next_sort_index_for_app(&self, app_type: &str) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(sort_index) FROM providers WHERE app_type = ?1",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(max.map(|v| (v + 1) as usize).unwrap_or(0))
    }

    /// 启动时调用：补齐缺失的官方预设供应商（Claude / Codex / Gemini）。
    ///
    /// 使用 settings flag `official_providers_seeded` 保证每个数据库只执行一次：
    /// - 全新用户：seed 三条官方预设
    /// - 老用户升级：同样会触发一次（flag 不存在），追加到末尾，不影响已有排序
    /// - 用户删除 seed 后：不再重建（flag 已为 true），尊重用户意图
    ///
    /// 与 `Database::save_provider` 的 UPSERT 语义配合，即使被意外重复调用
    /// 也不会覆盖用户当前激活的供应商（is_current 字段会被保留）。
    pub fn init_default_official_providers(&self) -> Result<usize, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        if self
            .get_bool_flag("official_providers_seeded")
            .unwrap_or(false)
        {
            return Ok(0);
        }

        let mut inserted = 0_usize;
        let now_ms = chrono::Utc::now().timestamp_millis();

        for seed in OFFICIAL_SEEDS {
            let app_type_str = seed.app_type.as_str();

            // 若该 id 已存在（极端情况：用户曾手动用过同 id），跳过
            if self.get_provider_by_id(seed.id, app_type_str)?.is_some() {
                continue;
            }

            let next_sort_index = self.next_sort_index_for_app(app_type_str)?;

            let settings_config: serde_json::Value =
                serde_json::from_str(seed.settings_config_json).map_err(|e| {
                    AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
                })?;

            let mut provider = Provider::with_id(
                seed.id.to_string(),
                seed.name.to_string(),
                settings_config,
                Some(seed.website_url.to_string()),
            );
            provider.category = Some("official".to_string());
            provider.icon = Some(seed.icon.to_string());
            provider.icon_color = Some(seed.icon_color.to_string());
            provider.sort_index = Some(next_sort_index);
            provider.created_at = Some(now_ms);

            self.save_provider(app_type_str, &provider)?;
            inserted += 1;
            log::info!(
                "✓ Seeded official provider: {} ({})",
                seed.name,
                app_type_str
            );
        }

        // 即使 inserted=0（例如用户手动创建过同 id）也设置 flag 防止反复检查
        self.set_setting("official_providers_seeded", "true")?;

        Ok(inserted)
    }

    /// 按 id 兜底插入单条 official seed（仅当目标表中该 id 不存在时插入）。
    ///
    /// 与 `init_default_official_providers` 不同：
    /// - 不触碰 `official_providers_seeded` 全局 flag，是 on-demand 修复
    /// - 只处理一条 seed，由调用方决定 id + app_type
    /// - 已存在则尊重用户自定义，不覆盖
    ///
    /// 返回 Ok(true) 表示插入了新行，Ok(false) 表示已存在被跳过。
    pub fn ensure_official_seed_by_id(
        &self,
        seed_id: &str,
        app_type: crate::app_config::AppType,
    ) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        let seed = OFFICIAL_SEEDS
            .iter()
            .find(|s| s.id == seed_id && s.app_type == app_type)
            .ok_or_else(|| {
                AppError::Database(format!(
                    "unknown official seed: id={seed_id}, app_type={}",
                    app_type.as_str()
                ))
            })?;

        let app_type_str = seed.app_type.as_str();

        if self.get_provider_by_id(seed_id, app_type_str)?.is_some() {
            return Ok(false);
        }

        let settings_config: serde_json::Value = serde_json::from_str(seed.settings_config_json)
            .map_err(|e| {
                AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
            })?;

        let next_sort_index = self.next_sort_index_for_app(app_type_str)?;
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut provider = Provider::with_id(
            seed.id.to_string(),
            seed.name.to_string(),
            settings_config,
            Some(seed.website_url.to_string()),
        );
        provider.category = Some("official".to_string());
        provider.icon = Some(seed.icon.to_string());
        provider.icon_color = Some(seed.icon_color.to_string());
        provider.sort_index = Some(next_sort_index);
        provider.created_at = Some(now_ms);

        self.save_provider(app_type_str, &provider)?;

        Ok(true)
    }
}

#[cfg(test)]
mod ensure_official_seed_tests {
    use crate::app_config::AppType;
    use crate::database::{Database, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID};

    #[test]
    fn ensure_inserts_when_missing() {
        let db = Database::memory().expect("memory db");
        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(inserted, "should insert when missing");

        let provider = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("provider exists after ensure");

        assert_eq!(provider.id, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID);
        assert_eq!(provider.name, "Claude Desktop Official");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.icon.as_deref(), Some("anthropic"));
        assert_eq!(provider.icon_color.as_deref(), Some("#D4915D"));
    }

    #[test]
    fn ensure_skips_when_present_and_preserves_customization() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");

        let mut renamed = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("seed present");
        renamed.name = "My Custom Backup".to_string();
        db.save_provider(AppType::ClaudeDesktop.as_str(), &renamed)
            .expect("save customization");

        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(!inserted, "should skip when present");

        let after = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("still present");
        assert_eq!(
            after.name, "My Custom Backup",
            "customization must not be overwritten"
        );
    }

    #[test]
    fn ensure_rejects_unknown_seed() {
        let db = Database::memory().expect("memory db");
        let result = db.ensure_official_seed_by_id("nonexistent-id", AppType::ClaudeDesktop);
        assert!(result.is_err(), "unknown seed id should be Err");
    }

    #[test]
    fn ensure_rejects_seed_app_type_mismatch() {
        let db = Database::memory().expect("memory db");
        let result =
            db.ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::Claude);
        assert!(result.is_err(), "(id, app_type) mismatch should be Err");
    }
}

#[cfg(test)]
mod save_provider_endpoints_sync_tests {
    use super::{extract_settings_base_url, normalize_endpoint_url};
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use crate::settings::CustomEndpoint;
    use serde_json::json;
    use std::collections::HashMap;

    fn endpoint_urls(db: &Database, provider_id: &str, app_type: &str) -> Vec<String> {
        let conn = db.conn.lock().expect("lock");
        let mut stmt = conn
            .prepare(
                "SELECT url FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 ORDER BY url",
            )
            .expect("prepare");
        let rows = stmt
            .query_map(rusqlite::params![provider_id, app_type], |row| {
                row.get::<_, String>(0)
            })
            .expect("query");
        rows.map(|r| r.expect("row")).collect()
    }

    fn codex_provider(id: &str, base_url: &str, endpoints: &[&str]) -> Provider {
        let mut custom_endpoints = HashMap::new();
        let now = 1_700_000_000_000_i64;
        for url in endpoints {
            custom_endpoints.insert(
                (*url).to_string(),
                CustomEndpoint {
                    url: (*url).to_string(),
                    added_at: now,
                    last_used: None,
                },
            );
        }
        Provider {
            id: id.to_string(),
            name: "test-codex".to_string(),
            settings_config: json!({
                "auth": { "OPENAI_API_KEY": "sk-test" },
                "config": format!("model_provider = \"custom\"\nbase_url = \"{base_url}\"\n")
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(now),
            sort_index: Some(0),
            notes: None,
            meta: Some(ProviderMeta {
                custom_endpoints,
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn normalize_endpoint_url_trims_slash() {
        assert_eq!(
            normalize_endpoint_url(" https://example.com/v1/ "),
            Some("https://example.com/v1".to_string())
        );
        assert_eq!(normalize_endpoint_url("   "), None);
    }

    #[test]
    fn extract_codex_and_claude_base_url() {
        let codex = json!({
            "config": "base_url = \"http://127.0.0.1:3012/v1/\"\n"
        });
        assert_eq!(
            extract_settings_base_url("codex", &codex).as_deref(),
            Some("http://127.0.0.1:3012/v1")
        );

        let claude = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://api.example.com/" }
        });
        assert_eq!(
            extract_settings_base_url("claude", &claude).as_deref(),
            Some("https://api.example.com")
        );
    }

    /// #5099: 创建后更新 base_url，provider_endpoints 应替换为新地址
    #[test]
    fn update_replaces_base_url_endpoint() {
        let db = Database::memory().expect("memory db");
        let old_url = "http://127.0.0.1:3012/v1";
        let new_url = "http://192.168.31.135:3010/v1";

        let provider = codex_provider("p1", old_url, &[old_url]);
        db.save_provider("codex", &provider).expect("create");
        assert_eq!(endpoint_urls(&db, "p1", "codex"), vec![old_url.to_string()]);

        // 模拟编辑保存：settings 已是新 base_url，meta 仍可能携带旧 endpoints
        let mut updated = codex_provider("p1", new_url, &[old_url]);
        updated.name = "renamed".to_string();
        db.save_provider("codex", &updated).expect("update");

        let urls = endpoint_urls(&db, "p1", "codex");
        assert!(
            urls.contains(&new_url.to_string()),
            "new base_url must be present: {urls:?}"
        );
        assert!(
            !urls.contains(&old_url.to_string()),
            "old base_url must be removed: {urls:?}"
        );
    }

    /// 更新 base_url 时保留其他自定义端点
    #[test]
    fn update_base_url_keeps_other_custom_endpoints() {
        let db = Database::memory().expect("memory db");
        let old_url = "http://127.0.0.1:3012/v1";
        let backup = "http://backup.example.com/v1";
        let new_url = "http://192.168.31.135:3010/v1";

        let provider = codex_provider("p2", old_url, &[old_url, backup]);
        db.save_provider("codex", &provider).expect("create");

        let updated = codex_provider("p2", new_url, &[old_url, backup]);
        db.save_provider("codex", &updated).expect("update");

        let urls = endpoint_urls(&db, "p2", "codex");
        assert!(urls.contains(&new_url.to_string()), "{urls:?}");
        assert!(urls.contains(&backup.to_string()), "{urls:?}");
        assert!(!urls.contains(&old_url.to_string()), "{urls:?}");
    }

    /// 更新时 meta 未带 endpoints，仍应根据 settings base_url 同步
    #[test]
    fn update_syncs_base_url_when_meta_endpoints_empty() {
        let db = Database::memory().expect("memory db");
        let old_url = "http://old.example.com/v1";
        let new_url = "http://new.example.com/v1";

        let provider = codex_provider("p3", old_url, &[old_url]);
        db.save_provider("codex", &provider).expect("create");

        // 不带 custom_endpoints（编辑表单常见路径）
        let mut updated = codex_provider("p3", new_url, &[]);
        updated.meta = Some(ProviderMeta::default());
        db.save_provider("codex", &updated).expect("update");

        let urls = endpoint_urls(&db, "p3", "codex");
        assert_eq!(urls, vec![new_url.to_string()]);
    }

    /// 经 API 新增的端点在仅改 base_url 的更新中应被保留
    #[test]
    fn update_preserves_api_added_endpoints() {
        let db = Database::memory().expect("memory db");
        let old_url = "http://old.example.com/v1";
        let new_url = "http://new.example.com/v1";
        let api_added = "http://api-added.example.com/v1";

        let provider = codex_provider("p4", old_url, &[old_url]);
        db.save_provider("codex", &provider).expect("create");
        db.add_custom_endpoint("codex", "p4", api_added)
            .expect("api add");

        // meta 只有旧 base（表单打开时的快照），不含 API 新增
        let updated = codex_provider("p4", new_url, &[old_url]);
        db.save_provider("codex", &updated).expect("update");

        let urls = endpoint_urls(&db, "p4", "codex");
        assert!(urls.contains(&new_url.to_string()), "{urls:?}");
        assert!(urls.contains(&api_added.to_string()), "{urls:?}");
        assert!(!urls.contains(&old_url.to_string()), "{urls:?}");
    }

    /// Claude 供应商同样同步 ANTHROPIC_BASE_URL
    #[test]
    fn update_syncs_claude_base_url_endpoint() {
        let db = Database::memory().expect("memory db");
        let old_url = "https://old.claude.example";
        let new_url = "https://new.claude.example";
        let now = 1_700_000_000_000_i64;

        let mut custom_endpoints = HashMap::new();
        custom_endpoints.insert(
            old_url.to_string(),
            CustomEndpoint {
                url: old_url.to_string(),
                added_at: now,
                last_used: None,
            },
        );

        let provider = Provider {
            id: "claude-1".to_string(),
            name: "claude".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                    "ANTHROPIC_BASE_URL": old_url
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(now),
            sort_index: Some(0),
            notes: None,
            meta: Some(ProviderMeta {
                custom_endpoints,
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        db.save_provider("claude", &provider).expect("create");

        let mut updated = provider.clone();
        updated.settings_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
                "ANTHROPIC_BASE_URL": new_url
            }
        });
        db.save_provider("claude", &updated).expect("update");

        let urls = endpoint_urls(&db, "claude-1", "claude");
        assert_eq!(urls, vec![new_url.to_string()]);
    }
}
