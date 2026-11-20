use super::json_file::JsonMcpFile;
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::Value;
use std::sync::Mutex;

static FILE: JsonMcpFile = JsonMcpFile {
    label: "Kimi Code",
    import_label: "Kimi Code",
    app: AppType::Kimi,
    path: || crate::kimi_config::data_dir().join("mcp.json"),
    transport_key: true,
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
    use serde_json::json;
    use std::fs;

    #[test]
    fn kimi_mcp_converts_transports_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, json!({"custom":"keep","mcpServers":{"server":{"url":"https://old.example.com","enabled":false,"deferred":true,"toolTimeoutMs":1234},"other":{"command":"node","args":["server.js"]}}}).to_string()).unwrap();
        FILE.sync_file(
            &path,
            "server",
            Some(
                &json!({"type":"sse","url":"https://example.com/sse","headers":{"X-Test":"keep"}}),
            ),
        )
        .unwrap();
        let value = FILE.read(&path).unwrap();
        assert_eq!(value["mcpServers"]["server"]["transport"], "sse");
        assert_eq!(value["mcpServers"]["server"]["enabled"], true);
        assert_eq!(value["mcpServers"]["server"]["deferred"], true);
        assert_eq!(value["mcpServers"]["server"]["toolTimeoutMs"], 1234);
        assert_eq!(
            FILE.unified_spec(&value["mcpServers"]["server"])["type"],
            "sse"
        );
        FILE.sync_file(
            &path,
            "server",
            Some(&json!({"type":"stdio","command":"node","args":["new.js"]})),
        )
        .unwrap();
        let value = FILE.read(&path).unwrap();
        assert!(value["mcpServers"]["server"].get("url").is_none());
        assert!(value["mcpServers"]["server"].get("transport").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
        assert_eq!(value["custom"], "keep");
    }

    #[test]
    fn kimi_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(FILE
            .sync_file(&path, "test", Some(&json!({"command":"node"})))
            .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
