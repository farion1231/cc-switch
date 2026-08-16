//! MCP 服务器数据访问对象
//!
//! 提供 MCP 服务器的 CRUD 操作。

use crate::app_config::{AppType, McpApps, McpServer};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension, Row};

const MCP_SERVER_SELECT: &str =
    "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild, enabled_opencode, enabled_hermes FROM mcp_servers";

fn row_to_mcp_server(row: &Row<'_>) -> rusqlite::Result<(String, McpServer)> {
    let id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let server_config_str: String = row.get(2)?;
    let description: Option<String> = row.get(3)?;
    let homepage: Option<String> = row.get(4)?;
    let docs: Option<String> = row.get(5)?;
    let tags_str: String = row.get(6)?;
    let enabled_claude: bool = row.get(7)?;
    let enabled_codex: bool = row.get(8)?;
    let enabled_gemini: bool = row.get(9)?;
    let enabled_grokbuild: bool = row.get(10)?;
    let enabled_opencode: bool = row.get(11)?;
    let enabled_hermes: bool = row.get(12)?;

    let server = serde_json::from_str(&server_config_str).unwrap_or_default();
    let tags = serde_json::from_str(&tags_str).unwrap_or_default();

    Ok((
        id.clone(),
        McpServer {
            id,
            name,
            server,
            apps: McpApps {
                claude: enabled_claude,
                codex: enabled_codex,
                gemini: enabled_gemini,
                grokbuild: enabled_grokbuild,
                opencode: enabled_opencode,
                hermes: enabled_hermes,
            },
            description,
            homepage,
            docs,
            tags,
        },
    ))
}

impl Database {
    /// 获取所有 MCP 服务器
    pub fn get_all_mcp_servers(&self) -> Result<IndexMap<String, McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!("{MCP_SERVER_SELECT} ORDER BY name ASC, id ASC"))
            .map_err(|e| AppError::Database(e.to_string()))?;

        let server_iter = stmt
            .query_map([], row_to_mcp_server)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut servers = IndexMap::new();
        for server_res in server_iter {
            let (id, server) = server_res.map_err(|e| AppError::Database(e.to_string()))?;
            servers.insert(id, server);
        }
        Ok(servers)
    }

    /// Atomically update one application's flag and return the authoritative row.
    ///
    /// The update and read share the same connection lock, so concurrent toggles
    /// for different applications cannot overwrite one another through a stale
    /// whole-row snapshot.
    pub fn update_mcp_server_app_enabled(
        &self,
        id: &str,
        app: &AppType,
        enabled: bool,
    ) -> Result<Option<McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let column = match app {
            AppType::Claude => Some("enabled_claude"),
            AppType::Codex => Some("enabled_codex"),
            AppType::Gemini => Some("enabled_gemini"),
            AppType::GrokBuild => Some("enabled_grokbuild"),
            AppType::OpenCode => Some("enabled_opencode"),
            AppType::Hermes => Some("enabled_hermes"),
            // These applications intentionally have no MCP flag in the SSOT.
            AppType::ClaudeDesktop | AppType::OpenClaw | AppType::Pi => None,
        };

        if let Some(column) = column {
            // `column` comes exclusively from the fixed allow-list above.
            let sql = format!("UPDATE mcp_servers SET {column} = ?1 WHERE id = ?2");
            let affected = conn
                .execute(&sql, params![enabled, id])
                .map_err(|e| AppError::Database(e.to_string()))?;
            if affected == 0 {
                return Ok(None);
            }
        }

        conn.query_row(
            &format!("{MCP_SERVER_SELECT} WHERE id = ?1"),
            params![id],
            |row| row_to_mcp_server(row).map(|(_, server)| server),
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 保存 MCP 服务器
    pub fn save_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild, enabled_opencode, enabled_hermes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                server.id,
                server.name,
                serde_json::to_string(&server.server).map_err(|e| AppError::Database(format!(
                    "Failed to serialize server config: {e}"
                )))?,
                server.description,
                server.homepage,
                server.docs,
                serde_json::to_string(&server.tags)
                    .map_err(|e| AppError::Database(format!("Failed to serialize tags: {e}")))?,
                server.apps.claude,
                server.apps.codex,
                server.apps.gemini,
                server.apps.grokbuild,
                server.apps.opencode,
                server.apps.hermes,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn test_server() -> McpServer {
        McpServer {
            id: "shared-server".to_string(),
            name: "Shared Server".to_string(),
            server: json!({ "command": "echo", "args": ["hello"] }),
            apps: McpApps {
                gemini: true,
                ..McpApps::default()
            },
            description: Some("description".to_string()),
            homepage: Some("https://example.com".to_string()),
            docs: None,
            tags: vec!["shared".to_string()],
        }
    }

    #[test]
    fn app_flag_updates_preserve_other_flags_and_return_authoritative_row() {
        let db = Database::memory().expect("create memory db");
        db.save_mcp_server(&test_server()).expect("seed server");

        let after_claude = db
            .update_mcp_server_app_enabled("shared-server", &AppType::Claude, true)
            .expect("enable Claude")
            .expect("server exists");
        assert!(after_claude.apps.claude);
        assert!(after_claude.apps.gemini);

        let after_codex = db
            .update_mcp_server_app_enabled("shared-server", &AppType::Codex, true)
            .expect("enable Codex")
            .expect("server exists");
        assert!(after_codex.apps.claude);
        assert!(after_codex.apps.codex);
        assert!(after_codex.apps.gemini);
        assert_eq!(after_codex.description.as_deref(), Some("description"));
        assert_eq!(after_codex.tags, vec!["shared"]);

        let stored = db
            .get_all_mcp_servers()
            .expect("read servers")
            .shift_remove("shared-server")
            .expect("stored server");
        assert_eq!(stored.apps, after_codex.apps);
    }

    #[test]
    fn concurrent_app_flag_updates_do_not_lose_each_other() {
        let db = Arc::new(Database::memory().expect("create memory db"));
        db.save_mcp_server(&test_server()).expect("seed server");
        let barrier = Arc::new(Barrier::new(3));

        let handles = [AppType::Claude, AppType::Codex].map(|app| {
            let db = Arc::clone(&db);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                db.update_mcp_server_app_enabled("shared-server", &app, true)
                    .expect("update app flag")
                    .expect("server exists");
            })
        });

        barrier.wait();
        for handle in handles {
            handle.join().expect("join app toggle");
        }

        let stored = db
            .get_all_mcp_servers()
            .expect("read servers")
            .shift_remove("shared-server")
            .expect("stored server");
        assert!(stored.apps.claude);
        assert!(stored.apps.codex);
        assert!(stored.apps.gemini);
    }

    #[test]
    fn app_flag_update_does_not_insert_a_missing_server() {
        let db = Database::memory().expect("create memory db");

        let updated = db
            .update_mcp_server_app_enabled("missing", &AppType::Claude, true)
            .expect("update missing server");

        assert!(updated.is_none());
        assert!(db.get_all_mcp_servers().expect("read servers").is_empty());
    }

    #[test]
    fn unsupported_mcp_apps_keep_the_existing_noop_semantics() {
        let db = Database::memory().expect("create memory db");
        let original = test_server();
        db.save_mcp_server(&original).expect("seed server");

        for app in [AppType::ClaudeDesktop, AppType::OpenClaw, AppType::Pi] {
            let returned = db
                .update_mcp_server_app_enabled("shared-server", &app, true)
                .expect("toggle unsupported app")
                .expect("server exists");
            assert_eq!(returned.apps, original.apps);
        }
    }
}
