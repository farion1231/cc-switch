//! Persistence adapter for the prototype's custom:<id> namespace.
//! Built-in AppType APIs remain closed; the storage contract awaits maintainer review.
use super::providers::{row_to_provider, PROVIDER_SELECT};
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    provider::Provider,
};
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn namespace(id: &str) -> String {
    format!("custom:{id}")
}

fn require_app(conn: &Connection, id: &str) -> Result<(), AppError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM settings, json_each(settings.value) AS profile
         WHERE settings.key = 'custom_app_profiles' AND json_extract(profile.value, '$.id') = ?1)",
        [id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(AppError::localized(
            "customApps.notFound",
            "未找到自定义应用",
            "Custom app profile was not found.",
        ));
    }
    Ok(())
}

fn missing_provider() -> AppError {
    AppError::localized(
        "customApps.providerNotFound",
        "未找到此应用的供应商",
        "Provider was not found in this profile.",
    )
}

impl Database {
    pub(crate) fn custom_provider_rows(&self, app_id: &str) -> Result<Vec<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        require_app(&conn, app_id)?;
        let mut stmt = conn.prepare(&format!(
            "{PROVIDER_SELECT} WHERE app_type = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([namespace(app_id)], row_to_provider)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn custom_provider(
        &self,
        app_id: &str,
        id: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        Ok(conn
            .query_row(
                &format!("{PROVIDER_SELECT} WHERE app_type = ?1 AND id = ?2"),
                params![namespace(app_id), id],
                row_to_provider,
            )
            .optional()?)
    }

    pub(crate) fn current_custom_provider(&self, app_id: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        Ok(conn
            .query_row(
                "SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
                [namespace(app_id)],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn save_custom_provider(
        &self,
        app_id: &str,
        row: &Provider,
        editing: bool,
        keep_key: bool,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        require_app(&tx, app_id)?;
        let ns = namespace(app_id);
        let mut config = row.settings_config.clone();
        if editing {
            let stored: String = tx
                .query_row(
                    "SELECT settings_config FROM providers WHERE app_type = ?1 AND id = ?2",
                    params![ns, row.id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(missing_provider)?;
            if keep_key {
                let stored: serde_json::Value =
                    serde_json::from_str(&stored).map_err(|e| AppError::Config(e.to_string()))?;
                config["apiKey"] = stored
                    .get("apiKey")
                    .cloned()
                    .unwrap_or(serde_json::json!(""));
            }
            tx.execute("UPDATE providers SET name = ?1, settings_config = ?2 WHERE app_type = ?3 AND id = ?4",
                params![row.name, crate::database::to_json_string(&config)?, ns, row.id])?;
        } else {
            tx.execute("INSERT INTO providers (id, app_type, name, settings_config) VALUES (?1, ?2, ?3, ?4)",
                params![row.id, ns, row.name, crate::database::to_json_string(&config)?])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn select_custom_provider_row(
        &self,
        app_id: &str,
        id: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        require_app(&tx, app_id)?;
        let ns = namespace(app_id);
        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
            [&ns],
        )?;
        if tx.execute(
            "UPDATE providers SET is_current = 1 WHERE app_type = ?1 AND id = ?2",
            params![ns, id],
        )? != 1
        {
            return Err(missing_provider()); // rollback also preserves the previous selection
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn delete_custom_provider_row(
        &self,
        app_id: &str,
        id: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        require_app(&tx, app_id)?;
        if tx.execute(
            "DELETE FROM providers WHERE app_type = ?1 AND id = ?2",
            params![namespace(app_id), id],
        )? != 1
        {
            return Err(missing_provider());
        }
        tx.commit()?;
        Ok(())
    }
}
