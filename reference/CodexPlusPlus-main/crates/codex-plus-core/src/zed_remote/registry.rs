use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    SshTarget, ZedRemoteError, build_zed_remote_url, codex_global_state_path,
    fallback_open_request_from_global_state_with_context, resolve_ssh_target_from_global_state,
    target_from_payload,
};

const REGISTRY_FILE: &str = "zed_remote_projects.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteProject {
    pub id: String,
    pub label: String,
    pub host_id: String,
    pub ssh: SshTarget,
    pub path: String,
    pub url: String,
    pub source: ZedRemoteProjectSource,
    pub last_opened_at_ms: Option<i64>,
    pub is_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZedRemoteProjectSource {
    CurrentThread,
    CodexRemoteProject,
    ThreadWorkspaceHint,
    SqliteThreadCwd,
    Recent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZedRemoteProjectRegistry {
    #[serde(default)]
    projects: Vec<ZedRemoteProject>,
}

pub fn list_zed_remote_projects_response(payload: &Value) -> Value {
    let state = match fs::read_to_string(codex_global_state_path()) {
        Ok(data) => match serde_json::from_str::<Value>(&data) {
            Ok(state) => Some(state),
            Err(error) => {
                return json!({"status": "failed", "message": ZedRemoteError::StateParse(error).to_string()});
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return json!({"status": "failed", "message": ZedRemoteError::StateRead(error).to_string()});
        }
    };
    let result = list_zed_remote_projects_from_optional_state(
        state.as_ref(),
        payload,
        Some(&default_zed_remote_project_registry_path()),
        Some(&crate::codex_sqlite::codex_session_db_path()),
    );
    match result {
        Ok(projects) => json!({
            "status": "ok",
            "projects": projects,
        }),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

pub fn list_zed_remote_projects_from_state(
    state: &Value,
    payload: &Value,
    registry_path: Option<&Path>,
    sqlite_state_path: Option<&Path>,
) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    list_zed_remote_projects_from_optional_state(
        Some(state),
        payload,
        registry_path,
        sqlite_state_path,
    )
}

fn list_zed_remote_projects_from_optional_state(
    state: Option<&Value>,
    payload: &Value,
    registry_path: Option<&Path>,
    sqlite_state_path: Option<&Path>,
) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    let mut projects = Vec::new();
    if let Some(state) = state {
        collect_current_project(state, payload, &mut projects);
        collect_codex_remote_projects(state, &mut projects);
        collect_thread_workspace_hints(state, &mut projects);
        collect_sqlite_thread_cwds(state, sqlite_state_path, &mut projects);
    }
    let registry_path = registry_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_zed_remote_project_registry_path);
    for mut project in read_registry_projects(&registry_path)? {
        project.source = ZedRemoteProjectSource::Recent;
        project.is_current = false;
        push_project(&mut projects, project);
    }
    Ok(projects)
}

pub fn remember_zed_remote_project_response(payload: &Value) -> Value {
    match remember_zed_remote_project(payload, None, None) {
        Ok(project) => json!({"status": "ok", "project": project}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

pub(super) fn remember_zed_remote_project(
    payload: &Value,
    resolved_target: Option<&SshTarget>,
    resolved_url: Option<&str>,
) -> Result<ZedRemoteProject, ZedRemoteError> {
    let target = resolved_target
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| target_from_payload(payload))?;
    let path = string_value(payload.get("path"));
    let url = resolved_url
        .map(ToString::to_string)
        .map(Ok)
        .unwrap_or_else(|| build_zed_remote_url(&target, &path))?;
    let host_id = string_value(payload.get("hostId"));
    let label = string_value(payload.get("label"));
    let project = project_from_parts(
        &host_id,
        target,
        &path,
        &url,
        label,
        ZedRemoteProjectSource::Recent,
        Some(now_ms()),
        false,
    );
    let registry_path = default_zed_remote_project_registry_path();
    let mut projects = read_registry_projects(&registry_path)?;
    push_project(&mut projects, project.clone());
    projects.sort_by(|left, right| {
        right
            .last_opened_at_ms
            .unwrap_or_default()
            .cmp(&left.last_opened_at_ms.unwrap_or_default())
    });
    projects.truncate(100);
    write_registry_projects(&registry_path, &projects)?;
    Ok(project)
}

pub fn forget_zed_remote_project_response(payload: &Value) -> Value {
    match forget_zed_remote_project(payload) {
        Ok(removed) => json!({"status": "ok", "removed": removed}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

fn forget_zed_remote_project(payload: &Value) -> Result<usize, ZedRemoteError> {
    let explicit_id = string_value(payload.get("id"));
    let target_id = if explicit_id.is_empty() {
        let target = target_from_payload(payload)?;
        let path = string_value(payload.get("path"));
        project_id(&target, &path)
    } else {
        explicit_id
    };
    let registry_path = default_zed_remote_project_registry_path();
    let mut projects = read_registry_projects(&registry_path)?;
    let before = projects.len();
    projects.retain(|project| project.id != target_id);
    let removed = before.saturating_sub(projects.len());
    write_registry_projects(&registry_path, &projects)?;
    Ok(removed)
}

fn collect_current_project(state: &Value, payload: &Value, projects: &mut Vec<ZedRemoteProject>) {
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
    if host_id.is_empty()
        && thread_id.is_empty()
        && workspace_root.is_empty()
        && remote_project_id.is_empty()
        && string_value(state.get("selected-remote-host-id")).is_empty()
    {
        return;
    }
    let Ok(request) = fallback_open_request_from_global_state_with_context(
        state,
        &host_id,
        &thread_id,
        &workspace_root,
        &remote_project_id,
    ) else {
        return;
    };
    let Ok(target) = target_from_payload(&request) else {
        return;
    };
    let path = string_value(request.get("path"));
    let Ok(url) = build_zed_remote_url(&target, &path) else {
        return;
    };
    let project = project_from_parts(
        &string_value(request.get("hostId")),
        target,
        &path,
        &url,
        String::new(),
        ZedRemoteProjectSource::CurrentThread,
        None,
        true,
    );
    push_project(projects, project);
}

fn collect_codex_remote_projects(state: &Value, projects: &mut Vec<ZedRemoteProject>) {
    for project in ordered_remote_projects_from_global_state(state) {
        let Some(object) = project.as_object() else {
            continue;
        };
        let host_id = string_value(object.get("hostId"));
        let path = string_value(object.get("remotePath"));
        if !path.starts_with('/') || host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &path) else {
            continue;
        };
        let label =
            string_value(object.get("label")).or_else_nonempty(|| string_value(object.get("name")));
        let project = project_from_parts(
            &host_id,
            target,
            &path,
            &url,
            label,
            ZedRemoteProjectSource::CodexRemoteProject,
            None,
            false,
        );
        push_project(projects, project);
    }
}

fn collect_thread_workspace_hints(state: &Value, projects: &mut Vec<ZedRemoteProject>) {
    let Some(hints) = state
        .get("thread-workspace-root-hints")
        .and_then(Value::as_object)
    else {
        return;
    };
    for hint in hints.values() {
        let path = workspace_path_from_hint(Some(hint));
        if !path.starts_with('/') {
            continue;
        }
        let hinted_host_id = host_id_from_hint(Some(hint));
        let host_id = host_id_for_remote_path(state, &hinted_host_id, &path);
        if host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &path) else {
            continue;
        };
        let project = project_from_parts(
            &host_id,
            target,
            &path,
            &url,
            String::new(),
            ZedRemoteProjectSource::ThreadWorkspaceHint,
            None,
            false,
        );
        push_project(projects, project);
    }
}

fn collect_sqlite_thread_cwds(
    state: &Value,
    sqlite_state_path: Option<&Path>,
    projects: &mut Vec<ZedRemoteProject>,
) {
    let paths = sqlite_state_path
        .map(|path| vec![path.to_path_buf()])
        .unwrap_or_else(|| {
            crate::codex_sqlite::codex_session_db_paths_from_home(
                &crate::codex_sqlite::default_codex_home_dir(),
            )
        });
    let mut cwds = Vec::new();
    for path in paths {
        if let Ok(mut items) = sqlite_thread_cwds(&path) {
            cwds.append(&mut items);
        }
    }
    for cwd in cwds {
        if !cwd.starts_with('/') {
            continue;
        }
        let host_id = host_id_for_remote_path(state, "", &cwd);
        if host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &cwd) else {
            continue;
        };
        let project = project_from_parts(
            &host_id,
            target,
            &cwd,
            &url,
            String::new(),
            ZedRemoteProjectSource::SqliteThreadCwd,
            None,
            false,
        );
        push_project(projects, project);
    }
}

fn read_registry_projects(path: &Path) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ZedRemoteError::RegistryRead(error)),
    };
    if data.trim().is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(projects) = serde_json::from_str::<Vec<ZedRemoteProject>>(&data) {
        return Ok(projects);
    }
    serde_json::from_str::<ZedRemoteProjectRegistry>(&data)
        .map(|registry| registry.projects)
        .map_err(ZedRemoteError::RegistryParse)
}

fn write_registry_projects(
    path: &Path,
    projects: &[ZedRemoteProject],
) -> Result<(), ZedRemoteError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ZedRemoteError::RegistryWrite)?;
    }
    let registry = ZedRemoteProjectRegistry {
        projects: projects.to_vec(),
    };
    let data = serde_json::to_vec_pretty(&registry).map_err(ZedRemoteError::RegistryParse)?;
    fs::write(path, data).map_err(ZedRemoteError::RegistryWrite)
}

fn sqlite_thread_cwds(path: &Path) -> anyhow::Result<Vec<String>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let db = Connection::open(path)?;
    let mut statement = db
        .prepare("SELECT DISTINCT cwd FROM threads WHERE cwd IS NOT NULL AND cwd != '' LIMIT 80")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut cwds = Vec::new();
    for cwd in rows.flatten() {
        let cwd = cwd.trim().to_string();
        if !cwd.is_empty() {
            cwds.push(cwd);
        }
    }
    Ok(cwds)
}

fn push_project(projects: &mut Vec<ZedRemoteProject>, project: ZedRemoteProject) {
    if let Some(existing) = projects
        .iter_mut()
        .find(|existing| existing.id == project.id)
    {
        if source_priority(project.source) < source_priority(existing.source) {
            existing.source = project.source;
            existing.label = project.label;
            existing.host_id = project.host_id;
        }
        existing.last_opened_at_ms = match (existing.last_opened_at_ms, project.last_opened_at_ms) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        existing.is_current |= project.is_current;
        return;
    }
    projects.push(project);
}

fn source_priority(source: ZedRemoteProjectSource) -> u8 {
    match source {
        ZedRemoteProjectSource::CurrentThread => 0,
        ZedRemoteProjectSource::CodexRemoteProject => 1,
        ZedRemoteProjectSource::ThreadWorkspaceHint => 2,
        ZedRemoteProjectSource::SqliteThreadCwd => 3,
        ZedRemoteProjectSource::Recent => 4,
    }
}

fn project_from_parts(
    host_id: &str,
    target: SshTarget,
    path: &str,
    url: &str,
    label: String,
    source: ZedRemoteProjectSource,
    last_opened_at_ms: Option<i64>,
    is_current: bool,
) -> ZedRemoteProject {
    let label = label.or_else_nonempty(|| label_from_path(path));
    ZedRemoteProject {
        id: project_id(&target, path),
        label,
        host_id: host_id.trim().to_string(),
        ssh: target,
        path: path.trim().to_string(),
        url: url.to_string(),
        source,
        last_opened_at_ms,
        is_current,
    }
}

fn project_id(target: &SshTarget, path: &str) -> String {
    let composite = format!(
        "{}|{}|{}|{}",
        target.user.trim(),
        target.host.trim(),
        target.port.map(|port| port.to_string()).unwrap_or_default(),
        path.trim()
    );
    format!("zed-remote-project:{:016x}", stable_hash(&composite))
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn label_from_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .to_string()
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

fn host_id_from_hint(hint: Option<&Value>) -> String {
    match hint.and_then(Value::as_object) {
        Some(object) => string_value(object.get("hostId"))
            .or_else_nonempty(|| string_value(object.get("remoteHostId"))),
        None => String::new(),
    }
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
        .or_else(|| string_value(state.get("selected-remote-host-id")).into_nonempty())
        .unwrap_or_default()
}

fn default_zed_remote_project_registry_path() -> PathBuf {
    crate::paths::default_app_state_dir().join(REGISTRY_FILE)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

trait NonEmptyStringExt {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String;

    fn into_nonempty(self) -> Option<String>;
}

impl NonEmptyStringExt for String {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String,
    {
        if self.is_empty() { fallback() } else { self }
    }

    fn into_nonempty(self) -> Option<String> {
        if self.is_empty() { None } else { Some(self) }
    }
}
