use super::json_file::JsonMcpFile;
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::Value;
use std::sync::Mutex;

static FILE: JsonMcpFile = JsonMcpFile {
    label: "MCode",
    import_label: "MiniMax Code",
    app: AppType::Mcode,
    path: || crate::mcode_config::data_dir().join("mcp.json"),
    transport_key: false,
    lock: Mutex::new(()),
};

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    FILE.sync(id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    FILE.sync_and_commit(id, spec, commit)
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    FILE.import(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::McpServer;
    use serde_json::json;
    use std::fs;
    #[test]
    fn database_failures_restore_mcode_mcp_and_allow_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let db = crate::database::Database::memory().unwrap();
        let server: McpServer = serde_json::from_value(json!({
            "id":"managed", "name":"Managed", "server":{"command":"node"},
            "apps":{"mcode":true}
        }))
        .unwrap();
        db.save_mcp_server(&server).unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        let spec = json!({"command":"updated"});
        // A rejected insert must also remove a newly created native file.
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || FILE.sync_file(&path, "managed", Some(&spec)),
            || db.save_mcp_server(&server),
        )
        .is_err());
        assert!(!path.exists());
        let original = b"{ \"mcpServers\": {\"managed\": {\"command\": \"node\", \"timeout\": 5000}}, \"version\": 7 }";
        fs::write(&path, original).unwrap();
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || FILE.sync_file(&path, "managed", None),
            || db.update_mcp_server_app_enabled(
                "managed",
                &crate::app_config::AppType::Mcode,
                false
            ),
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(db.get_all_mcp_servers().unwrap()["managed"].apps.mcode);
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || FILE.sync_file(&path, "managed", None),
            || db.delete_mcp_server("managed"),
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=OFF")
            .unwrap();
        crate::mcode_config::write_and_commit(
            &path,
            || FILE.sync_file(&path, "managed", None),
            || db.delete_mcp_server("managed"),
        )
        .unwrap();
        assert!(db.get_all_mcp_servers().unwrap().is_empty());
        assert!(FILE.read(&path).unwrap()["mcpServers"]
            .get("managed")
            .is_none());
    }

    #[test]
    fn mcode_mcp_preserves_unmanaged_entries_and_transport_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = json!({"version":7,"mcpServers":{"other":{"command":"node"},"edit":{"command":"old","args":["old"],"timeout":5000}}});
        fs::write(&path, original.to_string()).unwrap();
        FILE.sync_file(
            &path,
            "edit",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
        )
        .unwrap();
        let written = FILE.read(&path).unwrap();
        assert_eq!(written["version"], 7);
        assert_eq!(
            written["mcpServers"]["other"],
            original["mcpServers"]["other"]
        );
        assert_eq!(written["mcpServers"]["edit"]["timeout"], 5000);
        assert_eq!(written["mcpServers"]["edit"]["type"], "http");
        assert_eq!(written["mcpServers"]["edit"]["enabled"], true);
        assert!(written["mcpServers"]["edit"].get("command").is_none());
        FILE.sync_file(&path, "edit", None).unwrap();
        assert!(FILE.read(&path).unwrap()["mcpServers"]
            .get("edit")
            .is_none());
        fs::write(&path, "broken json").unwrap();
        assert!(FILE
            .sync_file(&path, "new", Some(&json!({"command":"node"})))
            .is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken json");
    }
}
