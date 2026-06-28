use rusqlite::{Connection, Result as SqliteResult};
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    /// Absolute path to the shared SQLite file. Used to lazily build a full
    /// desktop [`crate::store::AppState`] for features that need the migrated
    /// schema, DAOs and services (skills, claude-desktop, etc.).
    db_path: String,
    /// Lazily-constructed desktop application state. Shares the same on-disk
    /// database file as `db` (a second connection), so advanced features reuse
    /// the exact desktop logic instead of being reimplemented here.
    desktop_state: Mutex<Option<Arc<crate::store::AppState>>>,
}

impl AppState {
    pub fn new(db_path: &str) -> SqliteResult<Self> {
        let mut conn = Connection::open(db_path)?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        Self::init_schema(&mut conn)?;
        // Repair legacy single-column PK on `providers` so it matches the
        // desktop schema's composite key. The desktop `provider_endpoints`
        // table (created on the same file via `desktop()`) declares
        // `FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type)`,
        // which SQLite cannot resolve unless `providers` has a UNIQUE key on
        // exactly `(id, app_type)` — otherwise every DELETE raises
        // "foreign key mismatch". This also makes per-app IDs unique instead of
        // global (so e.g. each app can have a "default" provider).
        Self::migrate_providers_primary_key(&conn)?;
        Self::migrate_prompts_to_per_app(&conn)?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            db_path: db_path.to_string(),
            desktop_state: Mutex::new(None),
        })
    }

    /// Rebuild `providers` with a composite `(id, app_type)` primary key when an
    /// older database still has the single-column `id` PK.
    ///
    /// Idempotent: a no-op once the composite key is in place. Runs with foreign
    /// keys temporarily disabled because the file may already contain the
    /// desktop `provider_endpoints` table that references `providers`.
    fn migrate_providers_primary_key(conn: &Connection) -> SqliteResult<()> {
        // Collect the primary-key column set from PRAGMA table_info.
        let mut pk_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(providers)")?;
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?; // column name
                let pk: i64 = row.get(5)?; // pk position (0 = not part of pk)
                Ok((name, pk))
            })?;
            let mut cols: Vec<(String, i64)> = Vec::new();
            for r in rows {
                let (name, pk) = r?;
                if pk > 0 {
                    cols.push((name, pk));
                }
            }
            cols.sort_by_key(|(_, pk)| *pk);
            cols.into_iter().map(|(name, _)| name).collect()
        };
        pk_cols.sort();

        // Already composite (id, app_type) → nothing to do.
        if pk_cols == ["app_type", "id"] {
            return Ok(());
        }

        log::info!(
            "Rebuilding web `providers` table to composite (id, app_type) primary key (was: {pk_cols:?})"
        );

        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = OFF;
            BEGIN;
            CREATE TABLE providers_new (
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                is_partner BOOLEAN,
                meta TEXT,
                icon TEXT,
                icon_color TEXT,
                in_failover_queue BOOLEAN,
                app_type TEXT NOT NULL DEFAULT 'claude',
                is_current BOOLEAN DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );
            INSERT OR IGNORE INTO providers_new
                (id, name, settings_config, website_url, category, created_at,
                 sort_index, notes, is_partner, meta, icon, icon_color,
                 in_failover_queue, app_type, is_current)
            SELECT id, name, settings_config, website_url, category, created_at,
                   sort_index, notes, is_partner, meta, icon, icon_color,
                   in_failover_queue, COALESCE(app_type, 'claude'), is_current
            FROM providers;
            DROP TABLE providers;
            ALTER TABLE providers_new RENAME TO providers;
            COMMIT;
            PRAGMA foreign_keys = ON;
            "#,
        )?;

        Ok(())
    }

    /// Rebuild `prompts` with app_type/description/enabled columns when an older
    /// web database still has the single-column `id` primary key and `is_active`.
    fn migrate_prompts_to_per_app(conn: &Connection) -> SqliteResult<()> {
        // Collect primary-key columns to detect the legacy single-column PK.
        let mut pk_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(prompts)")?;
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                let pk: i64 = row.get(5)?;
                Ok((name, pk))
            })?;
            let mut cols: Vec<(String, i64)> = Vec::new();
            for r in rows {
                let (name, pk) = r?;
                if pk > 0 {
                    cols.push((name, pk));
                }
            }
            cols.sort_by_key(|(_, pk)| *pk);
            cols.into_iter().map(|(name, _)| name).collect()
        };
        pk_cols.sort();

        if pk_cols == ["app_type", "id"] {
            // Already has the composite key; ensure new columns exist for legacy web DBs.
            let _ = conn.execute("ALTER TABLE prompts ADD COLUMN description TEXT", []);
            let _ = conn.execute(
                "ALTER TABLE prompts ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT 1",
                [],
            );
            return Ok(());
        }

        log::info!(
            "Rebuilding web `prompts` table to composite (id, app_type) primary key (was: {pk_cols:?})"
        );

        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = OFF;
            BEGIN;
            CREATE TABLE prompts_new (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL DEFAULT 'claude',
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                description TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                PRIMARY KEY (id, app_type)
            );
            INSERT INTO prompts_new
                (id, app_type, name, content, description, enabled, created_at, updated_at)
            SELECT
                id,
                'claude' AS app_type,
                name,
                content,
                NULL AS description,
                COALESCE(is_active, 0) AS enabled,
                created_at,
                updated_at
            FROM prompts;
            DROP TABLE prompts;
            ALTER TABLE prompts_new RENAME TO prompts;
            COMMIT;
            PRAGMA foreign_keys = ON;
            "#,
        )?;

        Ok(())
    }

    fn init_schema(conn: &mut Connection) -> SqliteResult<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS providers (
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                is_partner BOOLEAN,
                meta TEXT,
                icon TEXT,
                icon_color TEXT,
                in_failover_queue BOOLEAN,
                app_type TEXT NOT NULL DEFAULT 'claude',
                is_current BOOLEAN DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                server_config TEXT NOT NULL,
                description TEXT,
                homepage TEXT,
                docs TEXT,
                tags TEXT,
                enabled_claude BOOLEAN DEFAULT 0,
                enabled_codex BOOLEAN DEFAULT 0,
                enabled_gemini BOOLEAN DEFAULT 0,
                enabled_opencode BOOLEAN DEFAULT 0,
                enabled_openclaw BOOLEAN DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS prompts (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL DEFAULT 'claude',
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                description TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                PRIMARY KEY (id, app_type)
            );

            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                installed_at INTEGER,
                updated_at INTEGER,
                source TEXT,
                version TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                provider_id TEXT,
                title TEXT,
                summary TEXT,
                project_dir TEXT,
                created_at INTEGER,
                last_active_at INTEGER,
                source_path TEXT,
                resume_command TEXT
            );

            CREATE TABLE IF NOT EXISTS session_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                messages TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS proxy_settings (
                id TEXT PRIMARY KEY,
                enabled BOOLEAN DEFAULT 0,
                config TEXT
            );
            "#,
        )?;

        Ok(())
    }

    pub fn with_db<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Connection) -> T,
    {
        let db = self.db.lock().unwrap();
        f(&db)
    }

    pub fn with_db_mut<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut Connection) -> T,
    {
        let mut db = self.db.lock().unwrap();
        f(&mut db)
    }

    /// Lazily build (and cache) a full desktop [`crate::store::AppState`] backed
    /// by the same SQLite file as this web state.
    ///
    /// This opens a second connection and runs the desktop schema migrations via
    /// [`crate::database::Database::init_at_path`]. In the embedded Tauri web
    /// server the file is already migrated by the desktop app, so this is a
    /// no-op migration-wise; it simply unlocks the desktop DAOs and services for
    /// feature parity (skills, claude-desktop, live imports, etc.).
    pub fn desktop(&self) -> Result<Arc<crate::store::AppState>, String> {
        let mut guard = self
            .desktop_state
            .lock()
            .map_err(|e| format!("desktop state lock poisoned: {e}"))?;

        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }

        let db = crate::database::Database::init_at_path(std::path::Path::new(&self.db_path))
            .map_err(|e| e.to_string())?;
        let state = Arc::new(crate::store::AppState::new(Arc::new(db)));
        *guard = Some(state.clone());
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn legacy_prompts_table_gets_app_type_and_enabled_columns() {
        let db_path = temp_dir().join(format!(
            "cc-switch-prompt-migration-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        // Simulate an old web database created before this fix.
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE prompts (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    content TEXT NOT NULL,
                    is_active BOOLEAN DEFAULT 0,
                    created_at INTEGER,
                    updated_at INTEGER
                );
                INSERT INTO prompts (id, name, content, is_active, created_at, updated_at)
                VALUES ('old-1', 'Old', 'content', 1, 1, 2);
                "#,
            )
            .unwrap();
        }

        // Re-open via AppState::new, which must migrate the table.
        let state = AppState::new(db_path.to_str().unwrap()).unwrap();
        let db = state.db.lock().unwrap();
        let mut stmt = db
            .prepare("SELECT id, app_type, name, content, description, enabled, created_at, updated_at FROM prompts")
            .unwrap();
        let rows: Vec<_> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0).unwrap(),
                    row.get::<_, String>(1).unwrap(),
                    row.get::<_, String>(2).unwrap(),
                    row.get::<_, String>(3).unwrap(),
                    row.get::<_, Option<String>>(4).unwrap(),
                    row.get::<_, bool>(5).unwrap(),
                    row.get::<_, Option<i64>>(6).unwrap(),
                    row.get::<_, Option<i64>>(7).unwrap(),
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            (
                "old-1".to_string(),
                "claude".to_string(),
                "Old".to_string(),
                "content".to_string(),
                None,
                true,
                Some(1),
                Some(2),
            )
        );

        drop(stmt);
        drop(db);
        let _ = std::fs::remove_file(&db_path);
    }
}
