use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{ZedRemoteError, codex_global_state_path, resolve_ssh_target_from_global_state};

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn ordered_remote_projects_from_global_state(state: &Value) -> Vec<Value> {
    let projects = state
        .get("remote-projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|project| project.as_object().is_some())
        .collect::<Vec<_>>();
    let project_order = state
        .get("project-order")
        .and_then(Value::as_array)
        .map(|order| {
            order
                .iter()
                .map(|item| string_value(Some(item)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut ordered = Vec::new();
    for project_id in project_order {
        if let Some(project) = projects
            .iter()
            .find(|project| string_value(project.get("id")) == project_id)
        {
            ordered.push(project.clone());
        }
    }
    let ordered_ids = ordered
        .iter()
        .map(|project| string_value(project.get("id")))
        .collect::<std::collections::HashSet<_>>();
    ordered.extend(
        projects
            .into_iter()
            .filter(|project| !ordered_ids.contains(&string_value(project.get("id")))),
    );
    ordered
}

fn workspace_path_from_hint(hint: Option<&Value>) -> String {
    match hint {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Object(object)) => {
            for key in [
                "remotePath",
                "remoteWorkspaceRoot",
                "workspaceRoot",
                "path",
                "cwd",
            ] {
                let value = string_value(object.get(key));
                if !value.is_empty() {
                    return value;
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn normalize_thread_id(thread_id: &str) -> String {
    thread_id
        .trim()
        .strip_prefix("local:")
        .unwrap_or_else(|| thread_id.trim())
        .to_string()
}

pub fn workspace_root_from_sqlite(thread_id: &str, state_path: Option<&Path>) -> String {
    let thread_id = normalize_thread_id(thread_id);
    if thread_id.is_empty() {
        return String::new();
    }
    if let Some(path) = state_path {
        return workspace_root_from_sqlite_path(path, &thread_id);
    }
    for path in crate::codex_sqlite::codex_session_db_paths_from_home(
        &crate::codex_sqlite::default_codex_home_dir(),
    ) {
        let cwd = workspace_root_from_sqlite_path(&path, &thread_id);
        if !cwd.is_empty() {
            return cwd;
        }
    }
    String::new()
}

fn workspace_root_from_sqlite_path(path: &Path, thread_id: &str) -> String {
    if !path.is_file() {
        return String::new();
    }
    Connection::open(path)
        .and_then(|db| {
            db.query_row(
                "SELECT cwd FROM threads WHERE id = ?1 LIMIT 1",
                [thread_id],
                |row| row.get::<_, String>(0),
            )
        })
        .ok()
        .map(|cwd| cwd.trim().to_string())
        .unwrap_or_default()
}

fn host_id_from_hint(hint: Option<&Value>) -> String {
    match hint.and_then(Value::as_object) {
        Some(object) => string_value(object.get("hostId"))
            .or_else_nonempty(|| string_value(object.get("remoteHostId"))),
        None => String::new(),
    }
}

fn thread_workspace_hint<'a>(state: &'a Value, thread_id: &str) -> Option<&'a Value> {
    if thread_id.is_empty() {
        return None;
    }
    let bare_thread_id = normalize_thread_id(thread_id);
    state
        .get("thread-workspace-root-hints")
        .and_then(Value::as_object)
        .and_then(|hints| {
            hints
                .get(thread_id)
                .or_else(|| hints.get(&bare_thread_id))
                .or_else(|| hints.get(&format!("local:{bare_thread_id}")))
        })
}

fn project_path_matches(remote_path: &str, project_path: &str) -> bool {
    let project_path = project_path.trim_end_matches('/');
    !project_path.is_empty()
        && (remote_path == project_path
            || remote_path
                .strip_prefix(project_path)
                .is_some_and(|suffix| suffix.starts_with('/')))
}

fn host_id_for_remote_path(state: &Value, preferred_host_id: &str, remote_path: &str) -> String {
    if !preferred_host_id.is_empty() {
        return preferred_host_id.to_string();
    }
    ordered_remote_projects_from_global_state(state)
        .into_iter()
        .find_map(|project| {
            let project_path = string_value(project.get("remotePath"));
            if project_path_matches(remote_path, &project_path) {
                Some(string_value(project.get("hostId")))
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn open_request_for_remote_path(
    state: &Value,
    host_id: &str,
    remote_path: &str,
) -> Result<Value, ZedRemoteError> {
    if !remote_path.starts_with('/') {
        return Err(ZedRemoteError::Validation(
            "Cannot determine remote workspace or file for Zed",
        ));
    }
    if host_id.is_empty() {
        return Err(ZedRemoteError::Validation("Remote host id is required"));
    }
    let target = resolve_ssh_target_from_global_state(state, host_id)?;
    Ok(json!({
        "hostId": host_id,
        "ssh": { "user": target.user, "host": target.host, "port": target.port },
        "path": remote_path,
    }))
}

pub fn fallback_open_request_from_global_state_with_context(
    state: &Value,
    host_id: &str,
    thread_id: &str,
    workspace_root: &str,
    remote_project_id: &str,
) -> Result<Value, ZedRemoteError> {
    let hint = thread_workspace_hint(state, thread_id);
    let selected_host_id = host_id
        .trim()
        .to_string()
        .or_else_nonempty(|| host_id_from_hint(hint))
        .or_else_nonempty(|| string_value(state.get("selected-remote-host-id")));
    let hinted_path = workspace_root
        .trim()
        .to_string()
        .or_else_nonempty(|| workspace_path_from_hint(hint))
        .or_else_nonempty(|| workspace_root_from_sqlite(thread_id, None));
    if hinted_path.starts_with('/') {
        let resolved_host_id = host_id_for_remote_path(state, &selected_host_id, &hinted_path);
        return open_request_for_remote_path(state, &resolved_host_id, &hinted_path);
    }

    let requested_project_id = remote_project_id.trim();
    if !requested_project_id.is_empty() {
        if requested_project_id.starts_with('/') {
            return open_request_for_remote_path(state, &selected_host_id, requested_project_id);
        }
        for project in ordered_remote_projects_from_global_state(state) {
            if string_value(project.get("id")) != requested_project_id {
                continue;
            }
            let project_host_id = string_value(project.get("hostId"));
            if !selected_host_id.is_empty() && project_host_id != selected_host_id {
                continue;
            }
            return open_request_for_remote_path(
                state,
                &project_host_id,
                &string_value(project.get("remotePath")),
            );
        }
    }

    let selected_project = ordered_remote_projects_from_global_state(state)
        .into_iter()
        .find(|project| {
            let project_host_id = string_value(project.get("hostId"));
            let remote_path = string_value(project.get("remotePath"));
            (selected_host_id.is_empty() || project_host_id == selected_host_id)
                && remote_path.starts_with('/')
        })
        .ok_or(ZedRemoteError::Validation(
            "Cannot determine remote workspace or file for Zed",
        ))?;
    let host_id =
        selected_host_id.or_else_nonempty(|| string_value(selected_project.get("hostId")));
    if host_id.is_empty() {
        return Err(ZedRemoteError::Validation("Remote host id is required"));
    }
    open_request_for_remote_path(
        state,
        &host_id,
        &string_value(selected_project.get("remotePath")),
    )
}

pub fn fallback_open_request_response(payload: &Value) -> Value {
    let host_id = string_value(payload.get("hostId"));
    let thread_id = string_value(payload.get("threadId"))
        .or_else_nonempty(|| string_value(payload.get("sessionId")))
        .or_else_nonempty(|| string_value(payload.get("session_id")));
    let workspace_root = string_value(payload.get("remoteWorkspaceRoot"))
        .or_else_nonempty(|| string_value(payload.get("workspaceRoot")))
        .or_else_nonempty(|| string_value(payload.get("cwd")))
        .or_else_nonempty(|| string_value(payload.get("path")));
    let remote_project_id = string_value(payload.get("remoteProjectId"))
        .or_else_nonempty(|| string_value(payload.get("projectId")));
    let path = codex_global_state_path();
    let result = fs::read_to_string(path)
        .map_err(ZedRemoteError::StateRead)
        .and_then(|data| serde_json::from_str::<Value>(&data).map_err(ZedRemoteError::StateParse))
        .and_then(|state| {
            fallback_open_request_from_global_state_with_context(
                &state,
                &host_id,
                &thread_id,
                &workspace_root,
                &remote_project_id,
            )
        });
    match result {
        Ok(request) => json!({"status": "ok", "request": request}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

trait NonEmptyStringExt {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String;
}

impl NonEmptyStringExt for String {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String,
    {
        if self.is_empty() { fallback() } else { self }
    }
}
