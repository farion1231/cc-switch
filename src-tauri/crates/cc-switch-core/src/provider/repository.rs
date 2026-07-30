use std::collections::HashMap;

use indexmap::IndexMap;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

use crate::{CoreError, HeadlessState};

use super::{ProviderRecord, ProviderSortUpdate};

pub(crate) fn list(
    state: &HeadlessState,
    app: &str,
) -> Result<IndexMap<String, ProviderRecord>, CoreError> {
    // Provider 主表和 endpoint 子表分开存储；读取时重建前端兼容的 meta.customEndpoints。
    validate_app(app)?;
    let connection = state.connection()?;
    let mut statement = connection.prepare(
        "SELECT id, name, settings_config, website_url, category, created_at,
                sort_index, notes, icon, icon_color, meta, in_failover_queue
         FROM providers WHERE app_type = ?1
         ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC",
    )?;
    let rows = statement.query_map([app], provider_from_row)?;
    let mut providers = IndexMap::new();
    for provider in rows {
        let mut provider = provider?;
        merge_endpoints(&connection, app, &mut provider)?;
        providers.insert(provider.id.clone(), provider);
    }
    Ok(providers)
}

pub(crate) fn current(state: &HeadlessState, app: &str) -> Result<String, CoreError> {
    // 桌面协议用空字符串表示尚无当前项；数据库内部仍使用零行而不是哨兵记录。
    validate_app(app)?;
    let connection = state.connection()?;
    Ok(connection
        .query_row(
            "SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
            [app],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_default())
}

pub(crate) fn add(
    state: &HeadlessState,
    app: &str,
    mut provider: ProviderRecord,
) -> Result<bool, CoreError> {
    // 首项判定、主表写入与 endpoint 写入必须处于同一事务，避免出现半初始化 Provider。
    validate_provider(app, &provider)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let current = current_in_connection(&transaction, app)?;
    provider.created_at.get_or_insert_with(unix_timestamp);
    if provider.sort_index.is_none() {
        provider.sort_index = Some(next_sort_index(&transaction, app)?);
    }
    insert_provider(&transaction, app, &provider, current.is_none())?;
    replace_endpoints(&transaction, app, &provider)?;
    transaction.commit()?;
    Ok(true)
}

pub(crate) fn update(
    state: &HeadlessState,
    app: &str,
    original_id: &str,
    mut provider: ProviderRecord,
) -> Result<bool, CoreError> {
    validate_provider(app, &provider)?;
    if original_id != provider.id {
        return Err(CoreError::ProviderIdChangeUnsupported);
    }
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let existing = get_provider(&transaction, app, original_id)?
        .ok_or_else(|| CoreError::ProviderNotFound(original_id.to_string()))?;
    provider.created_at = provider.created_at.or(existing.created_at);
    provider.sort_index = provider.sort_index.or(existing.sort_index);
    let changed = update_provider(&transaction, app, &provider)?;
    replace_endpoints_if_present(&transaction, app, &provider)?;
    transaction.commit()?;
    Ok(changed == 1)
}

pub(crate) fn delete(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
    // 当前项删除会让数据库与 live 配置失去对应关系，因此必须由调用方先完成切换。
    validate_app(app)?;
    let connection = state.connection()?;
    if current_in_connection(&connection, app)?.as_deref() == Some(id) {
        return Err(CoreError::CurrentProviderDeletion);
    }
    let changed = connection.execute(
        "DELETE FROM providers WHERE app_type = ?1 AND id = ?2",
        params![app, id],
    )?;
    if changed != 1 {
        return Err(CoreError::ProviderNotFound(id.to_string()));
    }
    Ok(())
}

pub(crate) fn switch(
    state: &HeadlessState,
    app: &str,
    id: &str,
) -> Result<ProviderRecord, CoreError> {
    // CASE 更新在单个事务中清除旧当前项并选中新项，保证每个应用最多一个 current。
    validate_app(app)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    let provider = get_provider(&transaction, app, id)?
        .ok_or_else(|| CoreError::ProviderNotFound(id.to_string()))?;
    set_current(&transaction, app, id)?;
    transaction.commit()?;
    Ok(provider)
}

pub(crate) fn update_sort_order(
    state: &HeadlessState,
    app: &str,
    updates: &[ProviderSortUpdate],
) -> Result<(), CoreError> {
    // 排序批次必须全部成功；任一 ID 不存在时事务回滚，不能留下部分新顺序。
    validate_app(app)?;
    let mut connection = state.connection()?;
    let transaction = connection.transaction()?;
    for update in updates {
        let changed = transaction.execute(
            "UPDATE providers SET sort_index = ?3 WHERE app_type = ?1 AND id = ?2",
            params![app, update.id, update.sort_index],
        )?;
        if changed != 1 {
            return Err(CoreError::ProviderNotFound(update.id.clone()));
        }
    }
    transaction.commit()?;
    Ok(())
}

fn validate_app(app: &str) -> Result<(), CoreError> {
    // 列表与桌面 AppType 保持同步；新增应用时必须同时补 live writer 与兼容测试。
    match app {
        "claude" | "claude-desktop" | "codex" | "gemini" | "grokbuild" | "opencode"
        | "openclaw" | "hermes" => Ok(()),
        _ => Err(CoreError::UnsupportedApp(app.to_string())),
    }
}

fn validate_provider(app: &str, provider: &ProviderRecord) -> Result<(), CoreError> {
    // settingsConfig 必须保持对象形态，各应用 writer 才能按键合并并保留无关配置。
    validate_app(app)?;
    if provider.id.trim().is_empty() || provider.name.trim().is_empty() {
        return Err(CoreError::InvalidProvider(
            "供应商 id 和 name 不能为空".to_string(),
        ));
    }
    if !provider.settings_config.is_object() {
        return Err(CoreError::InvalidProvider(
            "settingsConfig 必须是 JSON 对象".to_string(),
        ));
    }
    Ok(())
}

fn insert_provider(
    transaction: &Transaction<'_>,
    app: &str,
    provider: &ProviderRecord,
    is_current: bool,
) -> Result<(), CoreError> {
    // meta 中的 endpoint 在写主表前剥离，随后由同一事务写入规范化子表。
    transaction.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category, created_at,
            sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            provider.id,
            app,
            provider.name,
            serde_json::to_string(&provider.settings_config)?,
            provider.website_url,
            provider.category,
            provider.created_at,
            provider.sort_index.map(|value| value as i64),
            provider.notes,
            provider.icon,
            provider.icon_color,
            meta_for_storage(provider)?,
            is_current,
            provider.in_failover_queue,
        ],
    )?;
    Ok(())
}

fn update_provider(
    connection: &Connection,
    app: &str,
    provider: &ProviderRecord,
) -> Result<usize, CoreError> {
    // is_current 不由普通编辑覆盖；只有 switch 事务有权改变当前项状态。
    Ok(connection.execute(
        "UPDATE providers SET
            name = ?3, settings_config = ?4, website_url = ?5, category = ?6,
            created_at = ?7, sort_index = ?8, notes = ?9, icon = ?10,
            icon_color = ?11, meta = ?12, in_failover_queue = ?13
         WHERE app_type = ?1 AND id = ?2",
        params![
            app,
            provider.id,
            provider.name,
            serde_json::to_string(&provider.settings_config)?,
            provider.website_url,
            provider.category,
            provider.created_at,
            provider.sort_index.map(|value| value as i64),
            provider.notes,
            provider.icon,
            provider.icon_color,
            meta_for_storage(provider)?,
            provider.in_failover_queue,
        ],
    )?)
}

fn provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderRecord> {
    // JSON 解析失败应作为数据库内容损坏返回，不能像桌面旧路径一样静默降级为 null。
    let settings: String = row.get(2)?;
    let meta: String = row.get(10)?;
    Ok(ProviderRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        settings_config: serde_json::from_str(&settings).map_err(json_from_sql_error)?,
        website_url: row.get(3)?,
        category: row.get(4)?,
        created_at: row.get(5)?,
        sort_index: row.get::<_, Option<i64>>(6)?.map(|value| value as usize),
        notes: row.get(7)?,
        icon: row.get(8)?,
        icon_color: row.get(9)?,
        meta: Some(serde_json::from_str(&meta).map_err(json_from_sql_error)?),
        in_failover_queue: row.get(11)?,
    })
}

fn merge_endpoints(
    connection: &Connection,
    app: &str,
    provider: &mut ProviderRecord,
) -> Result<(), CoreError> {
    // added_at 为空兼容旧桌面数据，并按既有 DTO 约定对外投影为 0。
    let mut statement = connection.prepare(
        "SELECT url, added_at FROM provider_endpoints
         WHERE provider_id = ?1 AND app_type = ?2
         ORDER BY added_at ASC, url ASC",
    )?;
    let rows = statement.query_map(params![provider.id, app], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;
    let mut endpoints = Map::new();
    for row in rows {
        let (url, added_at) = row?;
        endpoints.insert(
            url.clone(),
            json!({ "url": url, "addedAt": added_at.unwrap_or_default() }),
        );
    }
    if endpoints.is_empty() {
        return Ok(());
    }
    let meta = provider
        .meta
        .get_or_insert_with(|| Value::Object(Map::new()));
    let object = meta
        .as_object_mut()
        .ok_or_else(|| CoreError::InvalidProvider("meta 必须是 JSON 对象".to_string()))?;
    object.insert("customEndpoints".to_string(), Value::Object(endpoints));
    Ok(())
}

fn replace_endpoints(
    transaction: &Transaction<'_>,
    app: &str,
    provider: &ProviderRecord,
) -> Result<(), CoreError> {
    // 先删后插位于调用方事务内，显式空对象可用于清空全部 endpoint。
    transaction.execute(
        "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
        params![provider.id, app],
    )?;
    for (url, added_at) in custom_endpoints(provider)? {
        transaction.execute(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![provider.id, app, url, added_at],
        )?;
    }
    Ok(())
}

fn replace_endpoints_if_present(
    transaction: &Transaction<'_>,
    app: &str,
    provider: &ProviderRecord,
) -> Result<(), CoreError> {
    // 更新 payload 省略 customEndpoints 表示“不修改子资源”；只有显式携带时才执行替换。
    // 这样旧客户端或局部编辑不会误删远端主机已经维护的 endpoint。
    let has_endpoint_payload = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.get("customEndpoints"))
        .is_some();
    if has_endpoint_payload {
        replace_endpoints(transaction, app, provider)?;
    }
    Ok(())
}

fn custom_endpoints(provider: &ProviderRecord) -> Result<HashMap<String, i64>, CoreError> {
    // URL 是子表自然键；payload 中的对象值只读取 addedAt，其余字段由 DTO 重建。
    let Some(meta) = provider.meta.as_ref() else {
        return Ok(HashMap::new());
    };
    let Some(endpoints) = meta.get("customEndpoints") else {
        return Ok(HashMap::new());
    };
    let object = endpoints.as_object().ok_or_else(|| {
        CoreError::InvalidProvider("customEndpoints 必须是 JSON 对象".to_string())
    })?;
    Ok(object
        .iter()
        .map(|(url, endpoint)| {
            (
                url.clone(),
                endpoint
                    .get("addedAt")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            )
        })
        .collect())
}

fn meta_for_storage(provider: &ProviderRecord) -> Result<String, CoreError> {
    // customEndpoints 属于规范化子表，主表 meta 只保存其余开放字段，避免双份数据漂移。
    let mut meta = provider
        .meta
        .clone()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let object = meta
        .as_object_mut()
        .ok_or_else(|| CoreError::InvalidProvider("meta 必须是 JSON 对象".to_string()))?;
    object.remove("customEndpoints");
    Ok(serde_json::to_string(&meta)?)
}

fn get_provider(
    connection: &Connection,
    app: &str,
    id: &str,
) -> Result<Option<ProviderRecord>, CoreError> {
    // 内部单项读取不合并 endpoint；需要完整 DTO 的公开 list 路径会统一执行合并。
    connection
        .query_row(
            "SELECT id, name, settings_config, website_url, category, created_at,
                    sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE app_type = ?1 AND id = ?2",
            params![app, id],
            provider_from_row,
        )
        .optional()
        .map_err(CoreError::Database)
}

fn current_in_connection(connection: &Connection, app: &str) -> Result<Option<String>, CoreError> {
    // LIMIT 1 兼容历史异常库；所有新切换事务仍会清理同应用的其他 current 标记。
    Ok(connection
        .query_row(
            "SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
            [app],
            |row| row.get(0),
        )
        .optional()?)
}

fn set_current(transaction: &Transaction<'_>, app: &str, id: &str) -> Result<(), CoreError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM providers WHERE app_type = ?1 AND id = ?2)",
        params![app, id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(CoreError::ProviderNotFound(id.to_string()));
    }
    transaction.execute(
        "UPDATE providers SET is_current = CASE WHEN id = ?2 THEN 1 ELSE 0 END
         WHERE app_type = ?1",
        params![app, id],
    )?;
    Ok(())
}

fn next_sort_index(transaction: &Transaction<'_>, app: &str) -> Result<usize, CoreError> {
    // 空集合从 0 开始；NULL 排序项不参与 MAX，保持桌面端的追加语义。
    let next = transaction.query_row(
        "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM providers WHERE app_type = ?1",
        [app],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(next as usize)
}

fn json_from_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn unix_timestamp() -> i64 {
    // 系统时间异常时沿用既有 0 兜底，避免新增操作因时钟回拨而完全失败。
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
