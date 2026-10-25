use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::config::{atomic_write, get_claude_mcp_path};
use crate::error::AppError;

const PROXY_MANAGED_KEY: &str = "PROXY_MANAGED";

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

pub fn approve_proxy_managed_key() -> Result<bool, AppError> {
    let path = get_claude_mcp_path();
    let mut root = read_json_value(&path)?;

    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

    let responses = obj
        .entry("customApiKeyResponses".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !responses.is_object() {
        *responses = serde_json::json!({});
    }

    let responses_obj = responses
        .as_object_mut()
        .expect("customApiKeyResponses normalized to object");

    let approved = responses_obj
        .entry("approved".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if !approved.is_array() {
        *approved = serde_json::json!([]);
    }

    let approved_arr = approved
        .as_array_mut()
        .expect("approved normalized to array");

    let already_approved = approved_arr
        .iter()
        .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY));

    let mut changed = false;

    if !already_approved {
        approved_arr.push(Value::String(PROXY_MANAGED_KEY.to_string()));
        changed = true;
    }

    if let Some(rejected) = responses_obj
        .get_mut("rejected")
        .and_then(|v| v.as_array_mut())
    {
        let before_len = rejected.len();
        rejected.retain(|v| v.as_str() != Some(PROXY_MANAGED_KEY));
        if rejected.len() < before_len {
            changed = true;
        }
    }

    if changed {
        write_json_value(&path, &root)?;
    }

    Ok(changed)
}

pub fn remove_proxy_managed_key() -> Result<bool, AppError> {
    let path = get_claude_mcp_path();
    if !path.exists() {
        return Ok(false);
    }

    let mut root = read_json_value(&path)?;
    let Some(responses) = root
        .get_mut("customApiKeyResponses")
        .and_then(|v| v.as_object_mut())
    else {
        return Ok(false);
    };

    let Some(approved) = responses.get_mut("approved").and_then(|v| v.as_array_mut()) else {
        return Ok(false);
    };

    let before_len = approved.len();
    approved.retain(|v| v.as_str() != Some(PROXY_MANAGED_KEY));

    if approved.len() == before_len {
        return Ok(false);
    }

    write_json_value(&path, &root)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                dir,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn read_claude_json() -> Value {
        let path = get_claude_mcp_path();
        if !path.exists() {
            return serde_json::json!({});
        }
        let content = std::fs::read_to_string(&path).expect("read claude.json");
        serde_json::from_str(&content).expect("parse claude.json")
    }

    #[test]
    #[serial]
    fn approve_on_nonexistent_file_creates_approved_with_proxy_managed() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let changed = approve_proxy_managed_key().expect("approve");
        assert!(changed, "should report changes were made");

        let root = read_claude_json();
        let approved = root
            .get("customApiKeyResponses")
            .and_then(|v| v.get("approved"))
            .and_then(|v| v.as_array())
            .expect("customApiKeyResponses.approved should exist");
        assert!(
            approved
                .iter()
                .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY)),
            "approved should contain PROXY_MANAGED"
        );
    }

    #[test]
    #[serial]
    fn approve_moves_proxy_managed_from_rejected_to_approved() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let path = get_claude_mcp_path();
        std::fs::write(
            &path,
            serde_json::json!({
                "customApiKeyResponses": {
                    "approved": [],
                    "rejected": ["PROXY_MANAGED"]
                }
            })
            .to_string(),
        )
        .expect("seed claude.json");

        let changed = approve_proxy_managed_key().expect("approve");
        assert!(changed, "should report changes were made");

        let root = read_claude_json();
        let responses = root
            .get("customApiKeyResponses")
            .expect("customApiKeyResponses should exist");

        let approved = responses
            .get("approved")
            .and_then(|v| v.as_array())
            .expect("approved should exist");
        assert!(
            approved
                .iter()
                .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY)),
            "approved should contain PROXY_MANAGED"
        );

        let rejected = responses
            .get("rejected")
            .and_then(|v| v.as_array())
            .expect("rejected should exist");
        assert!(
            !rejected
                .iter()
                .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY)),
            "rejected should NOT contain PROXY_MANAGED"
        );
    }

    #[test]
    #[serial]
    fn approve_preserves_other_fields() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let path = get_claude_mcp_path();
        std::fs::write(
            &path,
            serde_json::json!({
                "hasCompletedOnboarding": true,
                "mcpServers": {
                    "context7": {
                        "command": "npx",
                        "args": ["-y", "@upstash/context7-mcp"]
                    }
                },
                "projects": {
                    "/home/user/project": {
                        "allowedTools": ["Bash"]
                    }
                },
                "customApiKeyResponses": {
                    "approved": ["sk-existing-key"],
                    "rejected": ["sk-bad-key"]
                }
            })
            .to_string(),
        )
        .expect("seed claude.json");

        approve_proxy_managed_key().expect("approve");

        let root = read_claude_json();

        assert_eq!(
            root.get("hasCompletedOnboarding").and_then(|v| v.as_bool()),
            Some(true),
            "hasCompletedOnboarding should be preserved"
        );

        let mcp = root
            .get("mcpServers")
            .and_then(|v| v.get("context7"))
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str());
        assert_eq!(mcp, Some("npx"), "mcpServers should be preserved");

        let projects = root
            .get("projects")
            .and_then(|v| v.get("/home/user/project"))
            .and_then(|v| v.get("allowedTools"))
            .and_then(|v| v.as_array());
        assert!(projects.is_some(), "projects should be preserved");

        let approved = root
            .get("customApiKeyResponses")
            .and_then(|v| v.get("approved"))
            .and_then(|v| v.as_array())
            .expect("approved should exist");
        assert!(
            approved
                .iter()
                .any(|v| v.as_str() == Some("sk-existing-key")),
            "existing approved key should be preserved"
        );
        assert!(
            approved
                .iter()
                .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY)),
            "PROXY_MANAGED should be added to approved"
        );

        let rejected = root
            .get("customApiKeyResponses")
            .and_then(|v| v.get("rejected"))
            .and_then(|v| v.as_array())
            .expect("rejected should exist");
        assert!(
            rejected.iter().any(|v| v.as_str() == Some("sk-bad-key")),
            "existing rejected key should be preserved"
        );
    }

    #[test]
    #[serial]
    fn remove_takes_proxy_managed_out_of_approved() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let path = get_claude_mcp_path();
        std::fs::write(
            &path,
            serde_json::json!({
                "hasCompletedOnboarding": true,
                "customApiKeyResponses": {
                    "approved": ["PROXY_MANAGED", "sk-other-key"],
                    "rejected": []
                }
            })
            .to_string(),
        )
        .expect("seed claude.json");

        let changed = remove_proxy_managed_key().expect("remove");
        assert!(changed, "should report changes were made");

        let root = read_claude_json();
        let approved = root
            .get("customApiKeyResponses")
            .and_then(|v| v.get("approved"))
            .and_then(|v| v.as_array())
            .expect("approved should exist");
        assert!(
            !approved
                .iter()
                .any(|v| v.as_str() == Some(PROXY_MANAGED_KEY)),
            "approved should NOT contain PROXY_MANAGED"
        );
        assert!(
            approved.iter().any(|v| v.as_str() == Some("sk-other-key")),
            "other approved keys should be preserved"
        );
        assert_eq!(
            root.get("hasCompletedOnboarding").and_then(|v| v.as_bool()),
            Some(true),
            "hasCompletedOnboarding should be preserved"
        );
    }

    #[test]
    #[serial]
    fn approve_is_idempotent() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let first = approve_proxy_managed_key().expect("first approve");
        assert!(first, "first call should report changes");

        let second = approve_proxy_managed_key().expect("second approve");
        assert!(!second, "second call should report no changes");

        let root = read_claude_json();
        let approved = root
            .get("customApiKeyResponses")
            .and_then(|v| v.get("approved"))
            .and_then(|v| v.as_array())
            .expect("approved should exist");
        let count = approved
            .iter()
            .filter(|v| v.as_str() == Some(PROXY_MANAGED_KEY))
            .count();
        assert_eq!(count, 1, "PROXY_MANAGED should appear exactly once");
    }
}
