use crate::database::{lock_conn, Database, NewProviderAggregate};
use crate::error::AppError;
use crate::provider::{Provider, ProviderAggregate, ProviderMeta, ProviderMutationInput};
use crate::settings::CustomEndpoint;
use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension, Row};
use std::collections::{HashMap, HashSet};

pub(super) struct StoredProviderRow {
    id: String,
    name: String,
    settings_config: String,
    website_url: Option<String>,
    category: Option<String>,
    created_at: Option<i64>,
    sort_index: Option<usize>,
    notes: Option<String>,
    icon: Option<String>,
    icon_color: Option<String>,
    meta: String,
    in_failover_queue: bool,
}

impl StoredProviderRow {
    pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            settings_config: row.get(2)?,
            website_url: row.get(3)?,
            category: row.get(4)?,
            created_at: row.get(5)?,
            sort_index: row.get(6)?,
            notes: row.get(7)?,
            icon: row.get(8)?,
            icon_color: row.get(9)?,
            meta: row.get(10)?,
            in_failover_queue: row.get(11)?,
        })
    }

    pub(super) fn decode(self, app_type: &str) -> Result<Provider, AppError> {
        let (settings_config, mut meta) =
            decode_provider_json(app_type, &self.id, &self.settings_config, &self.meta)?;
        // Child rows are the sole endpoint authority. Do not expose a stale
        // legacy copy that happens to remain embedded in provider metadata.
        meta.custom_endpoints.clear();
        Ok(Provider {
            id: self.id,
            name: self.name,
            settings_config,
            website_url: self.website_url,
            category: self.category,
            created_at: self.created_at,
            sort_index: self.sort_index,
            notes: self.notes,
            meta: Some(meta),
            icon: self.icon,
            icon_color: self.icon_color,
            in_failover_queue: self.in_failover_queue,
        })
    }
}

fn decode_provider_json(
    app_type: &str,
    provider_id: &str,
    settings_config: &str,
    meta: &str,
) -> Result<(serde_json::Value, ProviderMeta), AppError> {
    let settings_config = serde_json::from_str(settings_config).map_err(|error| {
        AppError::Database(format!(
            "invalid settings_config for provider '{app_type}/{provider_id}': {error}"
        ))
    })?;
    let meta = if meta.trim().is_empty() {
        ProviderMeta::default()
    } else {
        serde_json::from_str(meta).map_err(|error| {
            AppError::Database(format!(
                "invalid meta for provider '{app_type}/{provider_id}': {error}"
            ))
        })?
    };
    Ok((settings_config, meta))
}

pub(super) const PROVIDER_SELECT: &str =
    "SELECT id, name, settings_config, website_url, category, created_at, sort_index,
            notes, icon, icon_color, meta, in_failover_queue
     FROM providers";

fn load_endpoints(
    conn: &rusqlite::Connection,
    app_type: &str,
    provider_id: Option<&str>,
) -> Result<HashMap<String, IndexMap<String, CustomEndpoint>>, AppError> {
    let mut grouped: HashMap<String, IndexMap<String, CustomEndpoint>> = HashMap::new();
    if let Some(provider_id) = provider_id {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, url, added_at, last_used
                 FROM provider_endpoints
                 WHERE app_type = ?1 AND provider_id = ?2
                 ORDER BY added_at, url, id",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map(params![app_type, provider_id], decode_endpoint_row)
            .map_err(|error| AppError::Database(error.to_string()))?;
        collect_endpoints(rows, app_type, &mut grouped)?;
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, url, added_at, last_used
                 FROM provider_endpoints
                 WHERE app_type = ?1
                 ORDER BY provider_id, added_at, url, id",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map([app_type], decode_endpoint_row)
            .map_err(|error| AppError::Database(error.to_string()))?;
        collect_endpoints(rows, app_type, &mut grouped)?;
    }
    Ok(grouped)
}

type StoredEndpoint = (String, String, CustomEndpoint);

fn decode_endpoint_row(row: &Row<'_>) -> rusqlite::Result<StoredEndpoint> {
    let provider_id: String = row.get(0)?;
    let url: String = row.get(1)?;
    Ok((
        provider_id,
        url.clone(),
        CustomEndpoint {
            url,
            added_at: row.get(2)?,
            last_used: row.get(3)?,
        },
    ))
}

fn collect_endpoints(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<StoredEndpoint>>,
    app_type: &str,
    grouped: &mut HashMap<String, IndexMap<String, CustomEndpoint>>,
) -> Result<(), AppError> {
    for row in rows {
        let (provider_id, url, endpoint) =
            row.map_err(|error| AppError::Database(error.to_string()))?;
        if grouped
            .entry(provider_id.clone())
            .or_default()
            .insert(url.clone(), endpoint)
            .is_some()
        {
            return Err(AppError::Database(format!(
                "duplicate endpoint '{url}' for provider '{app_type}/{provider_id}'"
            )));
        }
    }
    Ok(())
}

impl Database {
    pub fn get_all_provider_aggregates(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, ProviderAggregate>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!(
                "{PROVIDER_SELECT}
                 WHERE app_type = ?1
                 ORDER BY COALESCE(sort_index, 999999), created_at, id"
            ))
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map([app_type], StoredProviderRow::from_row)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut endpoints = load_endpoints(&conn, app_type, None)?;
        let mut aggregates = IndexMap::new();
        for row in rows {
            let provider = row
                .map_err(|error| AppError::Database(error.to_string()))?
                .decode(app_type)?;
            let provider_id = provider.id.clone();
            aggregates.insert(
                provider_id.clone(),
                ProviderAggregate {
                    provider,
                    endpoints: endpoints.remove(&provider_id).unwrap_or_default(),
                },
            );
        }
        Ok(aggregates)
    }

    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        Ok(self
            .get_all_provider_aggregates(app_type)?
            .into_iter()
            .map(|(id, aggregate)| (id, aggregate.into_provider()))
            .collect())
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
        Ok(self
            .get_provider_aggregate(app_type, id)?
            .map(ProviderAggregate::into_provider))
    }

    pub fn get_provider_aggregate(
        &self,
        app_type: &str,
        id: &str,
    ) -> Result<Option<ProviderAggregate>, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                &format!("{PROVIDER_SELECT} WHERE id = ?1 AND app_type = ?2"),
                params![id, app_type],
                StoredProviderRow::from_row,
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let provider = row.decode(app_type)?;
        let mut endpoints = load_endpoints(&conn, app_type, Some(id))?;
        Ok(Some(ProviderAggregate {
            provider,
            endpoints: endpoints.remove(id).unwrap_or_default(),
        }))
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
        let provider_id = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT id FROM providers
                 WHERE app_type = ?1 AND category = ?2 AND is_current = 1
                 LIMIT 1",
                params![app_type, category],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?
        };
        provider_id
            .map(|provider_id| self.get_provider_by_id(&provider_id, app_type))
            .transpose()
            .map(Option::flatten)
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
    /// 每条 seed 都先读存在性，再走严格 create；并发冲突向上传播，不会覆盖
    /// 用户已有的同名供应商或当前状态。
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

            self.create_provider(NewProviderAggregate::from_input(
                app_type_str,
                ProviderMutationInput {
                    id: seed.id.to_string(),
                    name: seed.name.to_string(),
                    settings_config,
                    website_url: Some(seed.website_url.to_string()),
                    category: Some("official".to_string()),
                    created_at: Some(now_ms),
                    sort_index: Some(next_sort_index),
                    notes: None,
                    meta: None,
                    icon: Some(seed.icon.to_string()),
                    icon_color: Some(seed.icon_color.to_string()),
                    in_failover_queue: false,
                },
            )?)?;
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

        self.create_provider(NewProviderAggregate::from_input(
            app_type_str,
            ProviderMutationInput {
                id: seed.id.to_string(),
                name: seed.name.to_string(),
                settings_config,
                website_url: Some(seed.website_url.to_string()),
                category: Some("official".to_string()),
                created_at: Some(now_ms),
                sort_index: Some(next_sort_index),
                notes: None,
                meta: None,
                icon: Some(seed.icon.to_string()),
                icon_color: Some(seed.icon_color.to_string()),
                in_failover_queue: false,
            },
        )?)?;

        Ok(true)
    }
}

#[cfg(test)]
mod ensure_official_seed_tests {
    use crate::app_config::AppType;
    use crate::database::{
        Database, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, CODEX_OFFICIAL_PROVIDER_ID,
        GROKBUILD_OFFICIAL_PROVIDER_ID,
    };

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
        db.reconcile_provider_fixture(AppType::ClaudeDesktop.as_str(), &renamed)
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
    fn ensure_recreates_codex_official_seed_after_deletion() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");
        db.delete_provider(AppType::Codex.as_str(), CODEX_OFFICIAL_PROVIDER_ID)
            .expect("delete Codex official");

        let inserted = db
            .ensure_official_seed_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
            .expect("ensure Codex official");
        assert!(inserted);
        let provider = db
            .get_provider_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex.as_str())
            .expect("query")
            .expect("Codex official restored");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.settings_config["auth"], serde_json::json!({}));
    }

    #[test]
    fn ensure_recreates_grokbuild_official_seed_after_deletion() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");
        db.delete_provider(AppType::GrokBuild.as_str(), GROKBUILD_OFFICIAL_PROVIDER_ID)
            .expect("delete Grok Build official");

        let inserted = db
            .ensure_official_seed_by_id(GROKBUILD_OFFICIAL_PROVIDER_ID, AppType::GrokBuild)
            .expect("ensure Grok Build official");
        assert!(inserted);
        let provider = db
            .get_provider_by_id(GROKBUILD_OFFICIAL_PROVIDER_ID, AppType::GrokBuild.as_str())
            .expect("query")
            .expect("Grok Build official restored");
        assert_eq!(provider.category.as_deref(), Some("official"));
        // 空 config：切换时不注入自定义模型表，Grok CLI 回落到自带 OAuth 登录
        assert_eq!(provider.settings_config["config"], serde_json::json!(""));
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
mod aggregate_tests {
    use crate::database::dao::provider_write;
    use crate::database::{
        Database, NewEndpoint, NewProviderAggregate, ProviderKey, ProviderRowUpdate,
    };
    use crate::error::AppError;
    use crate::provider::{Provider, ProviderAggregate, ProviderMeta, ProviderMutationInput};
    use crate::settings::CustomEndpoint;
    use indexmap::IndexMap;
    use serde_json::json;

    fn aggregate() -> ProviderAggregate {
        ProviderAggregate {
            provider: Provider::with_id(
                "pi-provider".into(),
                "Pi Provider".into(),
                json!({"models": [{"id": "m"}]}),
                Some("https://example.test".into()),
            ),
            endpoints: IndexMap::from([
                (
                    "https://one.test".into(),
                    CustomEndpoint {
                        url: "https://one.test".into(),
                        added_at: Some(10),
                        last_used: Some(11),
                    },
                ),
                (
                    "https://two.test".into(),
                    CustomEndpoint {
                        url: "https://two.test".into(),
                        added_at: Some(20),
                        last_used: Some(21),
                    },
                ),
            ]),
        }
    }

    fn mutation_input(provider: Provider) -> ProviderMutationInput {
        ProviderMutationInput {
            id: provider.id,
            name: provider.name,
            settings_config: provider.settings_config,
            website_url: provider.website_url,
            category: provider.category,
            created_at: provider.created_at,
            sort_index: provider.sort_index,
            notes: provider.notes,
            meta: provider.meta,
            icon: provider.icon,
            icon_color: provider.icon_color,
            in_failover_queue: provider.in_failover_queue,
        }
    }

    fn create_aggregate(db: &Database, app_type: &str) -> Result<(), AppError> {
        db.create_provider(NewProviderAggregate::from_input(
            app_type,
            mutation_input(aggregate().into_provider()),
        )?)
    }

    #[test]
    fn aggregate_single_and_all_hydration_match() -> Result<(), AppError> {
        let db = Database::memory()?;
        create_aggregate(&db, "pi")?;

        let single = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("single aggregate");
        let all = db.get_all_provider_aggregates("pi")?;
        assert_eq!(
            serde_json::to_value(&single).expect("serialize single"),
            serde_json::to_value(&all["pi-provider"]).expect("serialize all")
        );
        assert_eq!(single.endpoints["https://one.test"].last_used, Some(11));
        let legacy = db
            .get_provider_by_id("pi-provider", "pi")?
            .expect("legacy projection");
        assert_eq!(
            legacy
                .meta
                .expect("meta")
                .custom_endpoints
                .get("https://two.test")
                .and_then(|endpoint| endpoint.last_used),
            Some(21)
        );
        Ok(())
    }

    #[test]
    fn strict_create_rolls_back_and_never_upserts() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut malformed = aggregate();
        malformed.provider.id = "duplicate-payload".into();
        malformed.endpoints.insert(
            "wrong-map-key".into(),
            CustomEndpoint {
                url: "https://duplicate.test".into(),
                added_at: Some(1),
                last_used: None,
            },
        );
        assert!(
            NewProviderAggregate::from_input("pi", mutation_input(malformed.into_provider()))
                .and_then(|input| db.create_provider(input))
                .is_err()
        );
        assert!(db
            .get_provider_aggregate("pi", "duplicate-payload")?
            .is_none());

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER reject_bad_endpoint
                 BEFORE INSERT ON provider_endpoints
                 WHEN NEW.url = 'https://reject.test'
                 BEGIN SELECT RAISE(ABORT, 'injected endpoint failure'); END;",
            )?;
        }
        let mut rejected = aggregate();
        rejected.provider.id = "rejected".into();
        rejected.endpoints = IndexMap::from([(
            "https://reject.test".into(),
            CustomEndpoint {
                url: "https://reject.test".into(),
                added_at: Some(99),
                last_used: None,
            },
        )]);
        assert!(
            NewProviderAggregate::from_input("pi", mutation_input(rejected.into_provider()))
                .and_then(|input| db.create_provider(input))
                .is_err()
        );
        assert!(db.get_provider_aggregate("pi", "rejected")?.is_none());

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch("DROP TRIGGER reject_bad_endpoint;")?;
        }
        create_aggregate(&db, "pi")?;
        let original = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("baseline aggregate");
        let mut conflicting = original.clone();
        conflicting.provider.meta = Some(ProviderMeta {
            custom_endpoints: conflicting.endpoints.clone().into_iter().collect(),
            ..conflicting.provider.meta.clone().unwrap_or_default()
        });
        let mut conflicting = conflicting.into_provider();
        conflicting.name = "Must not upsert".into();
        assert!(
            NewProviderAggregate::from_input("pi", mutation_input(conflicting))
                .and_then(|input| db.create_provider(input))
                .is_err()
        );
        let after = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("unchanged aggregate");
        assert_eq!(
            serde_json::to_value(after).expect("serialize after"),
            serde_json::to_value(original).expect("serialize original")
        );
        Ok(())
    }

    #[test]
    fn stale_row_update_cannot_overwrite_endpoint_mutations() -> Result<(), AppError> {
        let db = Database::memory()?;
        create_aggregate(&db, "pi")?;
        let mut stale = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("stale aggregate");

        let key = ProviderKey::new("pi", "pi-provider")?;
        db.add_provider_endpoint(&key, NewEndpoint::now("https://three.test")?)?;
        db.remove_provider_endpoint(&key, "https://one.test")?;
        db.touch_provider_endpoint(&key, "https://two.test", 222)?;

        stale.provider.name = "Row-only edit".into();
        stale
            .provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .custom_endpoints = stale.endpoints.clone().into_iter().collect();
        stale
            .provider
            .meta
            .as_mut()
            .expect("meta")
            .custom_endpoints
            .clear();
        db.update_provider(
            &key,
            &ProviderRowUpdate::from_input(&mutation_input(stale.provider))?,
        )?;

        let after = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("provider after row-only update");
        assert_eq!(after.provider.name, "Row-only edit");
        assert_eq!(
            after.endpoints.keys().cloned().collect::<Vec<_>>(),
            vec!["https://two.test", "https://three.test"]
        );
        assert_eq!(after.endpoints["https://two.test"].last_used, Some(222));
        assert!(matches!(
            db.touch_provider_endpoint(&key, "https://missing.test", 1),
            Err(AppError::NotFound(_))
        ));
        let stored_meta: String = {
            let conn = crate::database::lock_conn!(db.conn);
            conn.query_row(
                "SELECT meta FROM providers WHERE id = 'pi-provider' AND app_type = 'pi'",
                [],
                |row| row.get(0),
            )?
        };
        let stored_meta: ProviderMeta = serde_json::from_str(&stored_meta)
            .map_err(|error| AppError::Database(error.to_string()))?;
        assert!(stored_meta.custom_endpoints.is_empty());
        Ok(())
    }

    #[test]
    fn projection_failure_compensation_restores_exact_aggregate() -> Result<(), AppError> {
        let db = Database::memory()?;
        create_aggregate(&db, "pi")?;
        let snapshot = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("rollback snapshot");

        {
            let mut conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER reject_projection
                 BEFORE INSERT ON pi_provider_projections
                 BEGIN SELECT RAISE(ABORT, 'injected projection failure'); END;",
            )?;
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM provider_endpoints
                  WHERE provider_id = 'pi-provider'
                    AND app_type = 'pi'
                    AND url = 'https://one.test'",
                [],
            )?;
            assert!(tx
                .execute(
                    "INSERT INTO pi_provider_projections
                        (provider_id, provider_key, created_at, updated_at)
                     VALUES ('pi-provider', 'native-key', 1, 1)",
                    [],
                )
                .is_err());
            let key = ProviderKey::new("pi", "pi-provider")?;
            let row = ProviderRowUpdate::from_input(&mutation_input(snapshot.provider.clone()))?;
            let endpoints = snapshot
                .endpoints
                .values()
                .cloned()
                .map(NewEndpoint::try_from)
                .collect::<Result<Vec<_>, _>>()?;
            provider_write::restore_provider_aggregate_on_tx(
                &tx,
                &key,
                &row,
                snapshot.provider.created_at,
                snapshot.provider.sort_index,
                false,
                snapshot.provider.in_failover_queue,
                &endpoints,
            )?;
            tx.commit()?;
        }

        let single = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("restored single aggregate");
        let all = db.get_all_provider_aggregates("pi")?;
        assert_eq!(
            serde_json::to_value(&single).expect("serialize single"),
            serde_json::to_value(&snapshot).expect("serialize snapshot")
        );
        assert_eq!(
            serde_json::to_value(&all["pi-provider"]).expect("serialize all"),
            serde_json::to_value(&snapshot).expect("serialize snapshot")
        );
        Ok(())
    }

    #[test]
    fn custom_endpoint_add_is_strict_for_one_logical_url() -> Result<(), AppError> {
        let db = Database::memory()?;
        create_aggregate(&db, "pi")?;
        let key = ProviderKey::new("pi", "pi-provider")?;
        db.add_provider_endpoint(&key, NewEndpoint::now("https://repeat.test")?)?;
        assert!(db
            .add_provider_endpoint(&key, NewEndpoint::now("https://repeat.test")?)
            .is_err());

        let saved = db
            .get_provider_aggregate("pi", "pi-provider")?
            .expect("aggregate");
        assert_eq!(
            saved
                .endpoints
                .keys()
                .filter(|url| url.as_str() == "https://repeat.test")
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn aggregate_read_rejects_corrupt_json_consistently() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers
                    (id, app_type, name, settings_config, meta)
                 VALUES ('corrupt', 'pi', 'Corrupt', '{', '{}')",
                [],
            )?;
        }
        assert!(db.get_provider_aggregate("pi", "corrupt").is_err());
        assert!(db.get_all_provider_aggregates("pi").is_err());
        assert!(db.get_provider_by_id("corrupt", "pi").is_err());
        assert!(db.get_all_providers("pi").is_err());
        Ok(())
    }
}
