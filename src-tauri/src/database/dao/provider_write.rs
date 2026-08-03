use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::{ProviderMeta, ProviderMutationInput};
use crate::settings::CustomEndpoint;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;
use std::collections::HashSet;

use super::providers::{StoredProviderRow, PROVIDER_SELECT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKey {
    app_type: String,
    id: String,
}

impl ProviderKey {
    pub fn new(app_type: impl Into<String>, id: impl Into<String>) -> Result<Self, AppError> {
        let app_type = app_type.into();
        let id = id.into();
        if app_type.trim().is_empty() || id.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "provider app type and id must be non-empty".to_string(),
            ));
        }
        Ok(Self { app_type, id })
    }

    pub fn app_type(&self) -> &str {
        &self.app_type
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRowUpdate {
    name: String,
    settings_config: Value,
    website_url: Option<String>,
    category: Option<String>,
    notes: Option<String>,
    meta: ProviderMeta,
    icon: Option<String>,
    icon_color: Option<String>,
}

impl ProviderRowUpdate {
    pub fn from_input(input: &ProviderMutationInput) -> Result<Self, AppError> {
        let meta = input.meta.clone().unwrap_or_default();
        if !meta.custom_endpoints.is_empty() {
            return Err(AppError::InvalidInput(
                "provider update must not contain customEndpoints; use endpoint operations"
                    .to_string(),
            ));
        }
        Ok(Self {
            name: input.name.clone(),
            settings_config: input.settings_config.clone(),
            website_url: input.website_url.clone(),
            category: input.category.clone(),
            notes: input.notes.clone(),
            meta,
            icon: input.icon.clone(),
            icon_color: input.icon_color.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRowCreate {
    content: ProviderRowUpdate,
    created_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewEndpoint {
    url: String,
    added_at: Option<i64>,
    last_used: Option<i64>,
}

impl NewEndpoint {
    pub fn new(
        url: impl Into<String>,
        added_at: Option<i64>,
        last_used: Option<i64>,
    ) -> Result<Self, AppError> {
        let url = url.into();
        if url.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "provider endpoint URL cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            url,
            added_at,
            last_used,
        })
    }

    pub fn now(url: impl Into<String>) -> Result<Self, AppError> {
        Self::new(url, Some(chrono::Utc::now().timestamp_millis()), None)
    }
}

impl TryFrom<CustomEndpoint> for NewEndpoint {
    type Error = AppError;

    fn try_from(endpoint: CustomEndpoint) -> Result<Self, Self::Error> {
        Self::new(endpoint.url, endpoint.added_at, endpoint.last_used)
    }
}

#[derive(Debug, Clone)]
pub struct NewProviderAggregate {
    key: ProviderKey,
    row: ProviderRowCreate,
    sort_index: Option<usize>,
    in_failover_queue: bool,
    initial_endpoints: Vec<NewEndpoint>,
}

impl NewProviderAggregate {
    pub fn from_input(app_type: &str, mut input: ProviderMutationInput) -> Result<Self, AppError> {
        let endpoints = input
            .meta
            .as_mut()
            .map(|meta| std::mem::take(&mut meta.custom_endpoints))
            .unwrap_or_default();
        let mut seen = HashSet::with_capacity(endpoints.len());
        let mut initial_endpoints = Vec::with_capacity(endpoints.len());
        for (key, endpoint) in endpoints {
            let normalized_key = key.trim().trim_end_matches('/').to_string();
            let normalized_url = endpoint.url.trim().trim_end_matches('/').to_string();
            if normalized_key != normalized_url {
                return Err(AppError::InvalidInput(format!(
                    "provider endpoint key '{key}' must match endpoint URL '{}'",
                    endpoint.url
                )));
            }
            if !seen.insert(normalized_url.clone()) {
                return Err(AppError::InvalidInput(format!(
                    "duplicate initial provider endpoint '{}'",
                    endpoint.url
                )));
            }
            initial_endpoints.push(NewEndpoint::new(
                normalized_url,
                endpoint.added_at,
                endpoint.last_used,
            )?);
        }
        let key = ProviderKey::new(app_type, input.id.clone())?;
        let row = ProviderRowCreate {
            content: ProviderRowUpdate::from_input(&input)?,
            created_at: input.created_at,
        };
        Ok(Self {
            key,
            row,
            sort_index: input.sort_index,
            in_failover_queue: input.in_failover_queue,
            initial_endpoints,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RenameProvider {
    source: ProviderKey,
    target_id: String,
    row: ProviderRowUpdate,
}

impl RenameProvider {
    pub fn from_input(
        source: ProviderKey,
        input: &ProviderMutationInput,
    ) -> Result<Self, AppError> {
        if !matches!(source.app_type(), "opencode" | "openclaw") {
            return Err(AppError::InvalidInput(
                "provider key changes are restricted to additive OpenCode/OpenClaw providers"
                    .to_string(),
            ));
        }
        if source.id() == input.id {
            return Err(AppError::InvalidInput(
                "provider rename requires a different target id".to_string(),
            ));
        }
        if input.id.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "provider target id must be non-empty".to_string(),
            ));
        }
        let mut row = ProviderRowUpdate::from_input(input)?;
        // A successful key change always remains DB-only.  The service owns
        // the corresponding live-file absence check, while the DAO persists
        // the durable half of that invariant.
        row.meta.live_config_managed = Some(false);
        Ok(Self {
            source,
            target_id: input.id.clone(),
            row,
        })
    }
}

fn encode_row(row: &ProviderRowUpdate) -> Result<(String, String), AppError> {
    let settings_config = serde_json::to_string(&row.settings_config).map_err(|error| {
        AppError::Database(format!("failed to serialize settings_config: {error}"))
    })?;
    let meta = serde_json::to_string(&row.meta).map_err(|error| {
        AppError::Database(format!("failed to serialize provider meta: {error}"))
    })?;
    Ok((settings_config, meta))
}

fn insert_row(
    tx: &Transaction<'_>,
    key: &ProviderKey,
    row: &ProviderRowUpdate,
    created_at: Option<i64>,
    sort_index: Option<usize>,
    is_current: bool,
    in_failover_queue: bool,
) -> Result<(), AppError> {
    let (settings_config, meta) = encode_row(row)?;
    tx.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta,
            is_current, in_failover_queue
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
         )",
        params![
            key.id,
            key.app_type,
            row.name,
            settings_config,
            row.website_url,
            row.category,
            created_at,
            sort_index,
            row.notes,
            row.icon,
            row.icon_color,
            meta,
            is_current,
            in_failover_queue,
        ],
    )
    .map_err(|error| match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.extended_code,
                rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                    | rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            ) =>
        {
            AppError::Conflict(format!(
                "provider '{}/{}' already exists",
                key.app_type, key.id
            ))
        }
        _ => AppError::Database(error.to_string()),
    })?;
    Ok(())
}

fn insert_endpoint(
    tx: &Transaction<'_>,
    key: &ProviderKey,
    endpoint: &NewEndpoint,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO provider_endpoints
            (provider_id, app_type, url, added_at, last_used)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            key.id,
            key.app_type,
            endpoint.url,
            endpoint.added_at,
            endpoint.last_used
        ],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}

/// Exact aggregate replacement is sealed inside the DAO parent module.  The
/// catalog compensation coordinator introduced with the ordered mutation
/// pipeline is the only intended caller.
#[allow(dead_code)]
// The certification contract keeps immutable creation time separate from the
// mutable row DTO and calls this sealed helper directly with the full snapshot.
#[allow(clippy::too_many_arguments)]
pub(super) fn restore_provider_aggregate_on_tx(
    tx: &Transaction<'_>,
    key: &ProviderKey,
    row: &ProviderRowUpdate,
    created_at: Option<i64>,
    sort_index: Option<usize>,
    is_current: bool,
    in_failover_queue: bool,
    endpoints: &[NewEndpoint],
) -> Result<(), AppError> {
    let updated = update_row(tx, key, row)?;
    if updated == 0 {
        insert_row(
            tx,
            key,
            row,
            created_at,
            sort_index,
            is_current,
            in_failover_queue,
        )?;
    } else {
        // Exact compensation is the only path allowed to restore immutable
        // creation time after a prior aggregate mutation.
        tx.execute(
            "UPDATE providers SET created_at = ?1 WHERE id = ?2 AND app_type = ?3",
            params![created_at, key.id, key.app_type],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }
    tx.execute(
        "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
        params![key.id, key.app_type],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    for endpoint in endpoints {
        insert_endpoint(tx, key, endpoint)?;
    }
    // State and order are maintained by their dedicated authorities.  Exact
    // compensation may restore their captured values without exposing them in
    // ProviderRowUpdate.
    tx.execute(
        "UPDATE providers
            SET sort_index = ?1, is_current = ?2, in_failover_queue = ?3
          WHERE id = ?4 AND app_type = ?5",
        params![
            sort_index,
            is_current,
            in_failover_queue,
            key.id,
            key.app_type
        ],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}

fn update_row(
    tx: &Transaction<'_>,
    key: &ProviderKey,
    row: &ProviderRowUpdate,
) -> Result<usize, AppError> {
    let (settings_config, meta) = encode_row(row)?;
    tx.execute(
        "UPDATE providers SET
            name = ?1,
            settings_config = ?2,
            website_url = ?3,
            category = ?4,
            notes = ?5,
            icon = ?6,
            icon_color = ?7,
            meta = ?8
          WHERE id = ?9 AND app_type = ?10",
        params![
            row.name,
            settings_config,
            row.website_url,
            row.category,
            row.notes,
            row.icon,
            row.icon_color,
            meta,
            key.id,
            key.app_type,
        ],
    )
    .map_err(|error| AppError::Database(error.to_string()))
}

impl Database {
    pub fn create_provider(&self, input: NewProviderAggregate) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        insert_row(
            &tx,
            &input.key,
            &input.row.content,
            input.row.created_at,
            input.sort_index,
            false,
            input.in_failover_queue,
        )?;
        for endpoint in &input.initial_endpoints {
            insert_endpoint(&tx, &input.key, endpoint)?;
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn update_provider(
        &self,
        key: &ProviderKey,
        row: &ProviderRowUpdate,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        if update_row(&tx, key, row)? != 1 {
            return Err(AppError::NotFound(format!(
                "provider '{}/{}'",
                key.app_type, key.id
            )));
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn update_provider_if_content_fingerprint(
        &self,
        key: &ProviderKey,
        expected_fingerprint: &str,
        row: &ProviderRowUpdate,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let current = tx
            .query_row(
                &format!("{PROVIDER_SELECT} WHERE id = ?1 AND app_type = ?2"),
                params![key.id, key.app_type],
                StoredProviderRow::from_row,
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?
            .ok_or_else(|| AppError::NotFound(format!("provider '{}/{}'", key.app_type, key.id)))?
            .decode(key.app_type())?;
        if current.row_content_fingerprint() != expected_fingerprint {
            return Err(AppError::Conflict(format!(
                "provider '{}/{}' changed since it was read",
                key.app_type, key.id
            )));
        }
        if update_row(&tx, key, row)? != 1 {
            return Err(AppError::NotFound(format!(
                "provider '{}/{}'",
                key.app_type, key.id
            )));
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub(crate) fn rename_db_only_additive_provider(
        &self,
        input: RenameProvider,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let source_state = tx
            .query_row(
                "SELECT sort_index, is_current, in_failover_queue, category, created_at, meta
                   FROM providers
                  WHERE id = ?1 AND app_type = ?2",
                params![input.source.id, input.source.app_type],
                |row| {
                    Ok((
                        row.get::<_, Option<usize>>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "provider '{}/{}'",
                    input.source.app_type, input.source.id
                ))
            })?;
        if matches!(source_state.3.as_deref(), Some("omo" | "omo-slim")) {
            return Err(AppError::InvalidInput(
                "OMO/OMO Slim providers cannot be renamed".to_string(),
            ));
        }
        let source_meta: ProviderMeta = if source_state.5.trim().is_empty() {
            ProviderMeta::default()
        } else {
            serde_json::from_str(&source_state.5).map_err(|error| {
                AppError::Database(format!(
                    "invalid meta for provider '{}/{}': {error}",
                    input.source.app_type, input.source.id
                ))
            })?
        };
        if source_meta.live_config_managed == Some(true) {
            return Err(AppError::Conflict(format!(
                "provider '{}/{}' became live-managed before rename",
                input.source.app_type, input.source.id
            )));
        }
        let target = ProviderKey::new(&input.source.app_type, &input.target_id)?;
        insert_row(
            &tx,
            &target,
            &input.row,
            source_state.4,
            source_state.0,
            source_state.1,
            source_state.2,
        )?;
        tx.execute(
            "INSERT INTO provider_endpoints
                (provider_id, app_type, url, added_at, last_used)
             SELECT ?1, app_type, url, added_at, last_used
               FROM provider_endpoints
              WHERE provider_id = ?2 AND app_type = ?3
              ORDER BY id",
            params![target.id, input.source.id, input.source.app_type],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        if tx
            .execute(
                "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
                params![input.source.id, input.source.app_type],
            )
            .map_err(|error| AppError::Database(error.to_string()))?
            != 1
        {
            return Err(AppError::NotFound(format!(
                "provider '{}/{}'",
                input.source.app_type, input.source.id
            )));
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn add_provider_endpoint(
        &self,
        key: &ProviderKey,
        endpoint: NewEndpoint,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        insert_endpoint(&tx, key, &endpoint)?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn remove_provider_endpoint(&self, key: &ProviderKey, url: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        if conn
            .execute(
                "DELETE FROM provider_endpoints
                  WHERE provider_id = ?1 AND app_type = ?2 AND url = ?3",
                params![key.id, key.app_type, url],
            )
            .map_err(|error| AppError::Database(error.to_string()))?
            != 1
        {
            return Err(AppError::NotFound(format!(
                "provider endpoint '{}/{}/{}'",
                key.app_type, key.id, url
            )));
        }
        Ok(())
    }

    pub fn touch_provider_endpoint(
        &self,
        key: &ProviderKey,
        url: &str,
        at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        if conn
            .execute(
                "UPDATE provider_endpoints
                    SET last_used = ?1
                  WHERE provider_id = ?2 AND app_type = ?3 AND url = ?4",
                params![at, key.id, key.app_type, url],
            )
            .map_err(|error| AppError::Database(error.to_string()))?
            != 1
        {
            return Err(AppError::NotFound(format!(
                "provider endpoint '{}/{}/{}'",
                key.app_type, key.id, url
            )));
        }
        Ok(())
    }

    pub(crate) fn update_provider_sort_index(
        &self,
        key: &ProviderKey,
        sort_index: usize,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        if conn
            .execute(
                "UPDATE providers SET sort_index = ?1 WHERE id = ?2 AND app_type = ?3",
                params![sort_index, key.id, key.app_type],
            )
            .map_err(|error| AppError::Database(error.to_string()))?
            != 1
        {
            return Err(AppError::NotFound(format!(
                "provider '{}/{}'",
                key.app_type, key.id
            )));
        }
        Ok(())
    }
}
