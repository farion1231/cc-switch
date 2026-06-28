use axum::{
    extract::{Path, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use indexmap::IndexMap;
use rusqlite::{Connection, OptionalExtension};
use serde_json::json;
use std::sync::Arc;

use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse, McpApps, McpServer, McpServerSpec},
};

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_mcp_servers))
        .route("/", post(create_mcp_server))
        .route("/import", post(import_mcp))
        .route("/:id", get(get_mcp_server))
        .route("/:id", put(update_mcp_server))
        .route("/:id", delete(delete_mcp_server))
        .route("/:id/toggle", post(toggle_mcp_server))
}

fn row_to_mcp_server(row: &rusqlite::Row) -> rusqlite::Result<McpServer> {
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
    let enabled_opencode: bool = row.get(10)?;

    let server: McpServerSpec =
        serde_json::from_str(&server_config_str).unwrap_or_else(|_| McpServerSpec {
            server_type: None,
            command: None,
            args: None,
            env: None,
            cwd: None,
            url: None,
            headers: None,
            extra: Default::default(),
        });

    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

    Ok(McpServer {
        id,
        name,
        server,
        apps: McpApps {
            claude: enabled_claude,
            codex: enabled_codex,
            gemini: enabled_gemini,
            opencode: enabled_opencode,
            openclaw: false,
        },
        description,
        tags: Some(tags),
        homepage,
        docs,
    })
}

async fn list_mcp_servers(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<IndexMap<String, McpServer>>> {
    let result: Result<IndexMap<String, McpServer>, String> = state.with_db(|db: &Connection| {
        let mut stmt = db.prepare(
            "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
             FROM mcp_servers
             ORDER BY name ASC, id ASC",
        ).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| row_to_mcp_server(row))
            .map_err(|e| e.to_string())?;

        let mut servers = IndexMap::new();
        for row_res in rows {
            match row_res {
                Ok(server) => {
                    servers.insert(server.id.clone(), server);
                }
                Err(e) => {
                    return Err(e.to_string());
                }
            }
        }

        Ok(servers)
    });

    match result {
        Ok(servers) => Json(ApiResponse::success(servers)),
        Err(e) => Json(ApiResponse::error(format!("Database error: {}", e))),
    }
}

async fn get_mcp_server(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Option<McpServer>>> {
    let result: Option<McpServer> = state.with_db(|db: &Connection| {
        let mut stmt = db.prepare(
            "SELECT id, name, server_config, description, homepage, docs, tags, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
             FROM mcp_servers
             WHERE id = ?1",
        ).ok()?;

        stmt.query_row([&id], |row| row_to_mcp_server(row)).ok()
    });

    Json(ApiResponse::success(result))
}

async fn create_mcp_server(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(server): Json<McpServer>,
) -> Json<ApiResponse<String>> {
    let id = server.id.clone();
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        db.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &server.id,
                &server.name,
                serde_json::to_string(&server.server).map_err(|e| e.to_string())?,
                &server.description,
                &server.homepage,
                &server.docs,
                serde_json::to_string(&server.tags.unwrap_or_default())
                    .map_err(|e| e.to_string())?,
                server.apps.claude,
                server.apps.codex,
                server.apps.gemini,
                server.apps.opencode,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "mcp.created",
                json!({ "id": id }),
            );
            Json(ApiResponse::success(id))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to create mcp server: {}",
            e
        ))),
    }
}

async fn update_mcp_server(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Json(mut server): Json<McpServer>,
) -> Json<ApiResponse<bool>> {
    server.id = id.clone();

    let result: Result<(), String> = state.with_db(|db: &Connection| {
        db.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &server.id,
                &server.name,
                serde_json::to_string(&server.server).map_err(|e| e.to_string())?,
                &server.description,
                &server.homepage,
                &server.docs,
                serde_json::to_string(&server.tags.unwrap_or_default())
                    .map_err(|e| e.to_string())?,
                server.apps.claude,
                server.apps.codex,
                server.apps.gemini,
                server.apps.opencode,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "mcp.updated",
                json!({ "id": id }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to update mcp server: {}",
            e
        ))),
    }
}

async fn delete_mcp_server(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<bool>> {
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        db.execute("DELETE FROM mcp_servers WHERE id = ?1", [&id])
            .map_err(|e| e.to_string())?;
        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "mcp.deleted",
                json!({ "id": id }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to delete mcp server: {}",
            e
        ))),
    }
}

async fn toggle_mcp_server(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let app = payload.get("app").and_then(|v| v.as_str()).unwrap_or("");
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let column = match app {
        "claude" => "enabled_claude",
        "codex" => "enabled_codex",
        "gemini" => "enabled_gemini",
        "opencode" => "enabled_opencode",
        _ => "",
    };

    if column.is_empty() {
        return Json(ApiResponse::error(format!("Invalid app: {}", app)));
    }

    let result: Result<(), String> = state.with_db(|db: &Connection| {
        db.execute(
            &format!("UPDATE mcp_servers SET {} = ?1 WHERE id = ?2", column),
            rusqlite::params![enabled, &id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "mcp.toggled",
                json!({ "id": id, "app": app, "enabled": enabled }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to toggle mcp server: {}",
            e
        ))),
    }
}

async fn import_mcp(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<Option<serde_json::Value>>,
) -> Json<ApiResponse<usize>> {
    let mut imported = 0usize;

    // Did the caller hand us an explicit `servers` payload (e.g. deep-link import)?
    let has_explicit_servers = body
        .as_ref()
        .and_then(|val| val.get("servers"))
        .and_then(|v| v.as_object())
        .is_some();

    if has_explicit_servers {
        let val = body.expect("body present when has_explicit_servers");
        let servers_obj = val
            .get("servers")
            .and_then(|v| v.as_object())
            .expect("servers object present when has_explicit_servers");

        let apps_list = val
            .get("apps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut enable_claude = false;
        let mut enable_codex = false;
        let mut enable_gemini = false;
        let mut enable_opencode = false;
        for a in apps_list.iter().filter_map(|x| x.as_str()) {
            match a {
                "claude" => enable_claude = true,
                "codex" => enable_codex = true,
                "gemini" => enable_gemini = true,
                "opencode" => enable_opencode = true,
                _ => {}
            }
        }

        let res: Result<(), String> = state.with_db_mut(|db: &mut Connection| {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            for (id, spec_val) in servers_obj.iter() {
                let name = spec_val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let server_cfg_str = serde_json::to_string(spec_val).map_err(|e| e.to_string())?;
                let tags_str = "[]".to_string();

                tx.execute(
                    "INSERT OR REPLACE INTO mcp_servers (
                        id, name, server_config, description, homepage, docs, tags,
                        enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        id,
                        &name,
                        &server_cfg_str,
                        Option::<String>::None,
                        Option::<String>::None,
                        Option::<String>::None,
                        &tags_str,
                        enable_claude,
                        enable_codex,
                        enable_gemini,
                        enable_opencode,
                    ],
                )
                .map_err(|e| e.to_string())?;

                imported += 1;
            }
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        });

        if let Err(e) = res {
            return Json(ApiResponse::error(format!("Import failed: {}", e)));
        }
    } else {
        // No explicit servers → read the live app configurations on this machine,
        // mirroring the desktop `import_mcp_from_apps` command.
        match import_from_local_apps(&state) {
            Ok(count) => imported = count,
            Err(e) => return Json(ApiResponse::error(format!("Import failed: {}", e))),
        }
    }

    crate::web::handlers::ws::broadcast_event(
        &ws_state,
        "mcp.imported",
        json!({ "count": imported }),
    );

    Json(ApiResponse::success(imported))
}

/// Import MCP servers from the live app configuration files on this machine
/// (Claude / Codex / Gemini / OpenCode / Hermes), persisting them into the web
/// server's SQLite store. Returns the number of newly-added servers.
///
/// Existing servers are only updated to enable the apps they were discovered in
/// (matching desktop semantics); their other fields are left untouched.
fn import_from_local_apps(state: &AppState) -> Result<usize, String> {
    let mut temp = crate::app_config::MultiAppConfig::default();

    // Each importer reads one app's live config file. Ignore per-app failures so a
    // single missing/invalid config doesn't abort the whole import.
    let _ = crate::mcp::import_from_claude(&mut temp);
    let _ = crate::mcp::import_from_codex(&mut temp);
    let _ = crate::mcp::import_from_gemini(&mut temp);
    let _ = crate::mcp::import_from_opencode(&mut temp);
    let _ = crate::mcp::import_from_hermes(&mut temp);

    let servers = match temp.mcp.servers {
        Some(servers) if !servers.is_empty() => servers,
        _ => return Ok(0),
    };

    state.with_db_mut(|db: &mut Connection| {
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let mut new_count = 0usize;

        for (id, srv) in servers.iter() {
            let is_new = tx
                .query_row("SELECT 1 FROM mcp_servers WHERE id = ?1", [id], |_| Ok(()))
                .optional()
                .map_err(|e| e.to_string())?
                .is_none();

            if is_new {
                let server_cfg_str =
                    serde_json::to_string(&srv.server).map_err(|e| e.to_string())?;
                let tags_str = serde_json::to_string(&srv.tags).map_err(|e| e.to_string())?;

                tx.execute(
                    "INSERT INTO mcp_servers (
                        id, name, server_config, description, homepage, docs, tags,
                        enabled_claude, enabled_codex, enabled_gemini, enabled_opencode
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        &srv.id,
                        &srv.name,
                        &server_cfg_str,
                        &srv.description,
                        &srv.homepage,
                        &srv.docs,
                        &tags_str,
                        srv.apps.claude,
                        srv.apps.codex,
                        srv.apps.gemini,
                        srv.apps.opencode,
                    ],
                )
                .map_err(|e| e.to_string())?;
                new_count += 1;
            } else if srv.apps.claude || srv.apps.codex || srv.apps.gemini || srv.apps.opencode {
                // Existing server: only turn on the apps it was discovered in,
                // never disable an already-enabled app or overwrite other fields.
                tx.execute(
                    "UPDATE mcp_servers SET
                        enabled_claude = enabled_claude OR ?2,
                        enabled_codex = enabled_codex OR ?3,
                        enabled_gemini = enabled_gemini OR ?4,
                        enabled_opencode = enabled_opencode OR ?5
                     WHERE id = ?1",
                    rusqlite::params![
                        &srv.id,
                        srv.apps.claude,
                        srv.apps.codex,
                        srv.apps.gemini,
                        srv.apps.opencode,
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().map_err(|e| e.to_string())?;
        Ok(new_count)
    })
}
