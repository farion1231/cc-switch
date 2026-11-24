//! Unified Multi-Agent Remote Control (LAN 远控) driver registry, run manager,
//! session/transcript resolver, input validator, secret redactor, and tolerant
//! NDJSON/plain-text stream parser.
//!
//! Supported CLI agents (10 verified official CLIs):
//! `claude`, `codex`, `gemini`, `grokbuild`, `opencode`, `agy`, `copilot`,
//! `kimi`, `pi`, `omp`.
//!
//! Security invariants:
//! - Strictly invokes each CLI binary directly via `tokio::process::Command`
//!   with explicit argv arrays — NEVER invokes a shell (`sh -c` / `cmd /c`).
//! - NEVER passes `--dangerously-skip-permissions`.
//! - Clients NEVER supply raw file paths or `--gemini_dir`; `sessionId` is
//!   validated and resolved server-side against scanned sessions.
//! - Private Antigravity API-key credentials (`GEMINI_API_KEY`,
//!   `GOOGLE_GEMINI_BASE_URL`) are injected strictly via child `.env(...)`
//!   and redacted from all emitted text/stderr/errors.

use crate::agy_config;
use crate::services::provider::agy_direct;
use crate::session_manager::{self, SessionMessage, SessionMeta};
use crate::store::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, oneshot, watch, Mutex};
use uuid::Uuid;

pub const MAX_CONCURRENT_RUNS: usize = 3;
pub const MAX_RETAINED_RUNS: usize = 20;
pub const MAX_EVENTS_PER_RUN: usize = 2000;
pub const MAX_PROMPT_BYTES: usize = 32_768;

const BROADCAST_CAPACITY: usize = 256;
const MAX_STREAM_LINE_BYTES: usize = 512 * 1024;
const MAX_TOOL_TEXT_CHARS: usize = 2000;

/// Canonical list of CLI agents supported by Unified Multi-Agent Remote Control.
pub const SUPPORTED_REMOTE_AGENTS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "agy",
    "copilot",
    "kimi",
    "pi",
    "omp",
];

#[derive(Debug, Clone, Copy)]
pub struct AgentDriverSpec {
    pub id: &'static str,
    pub binary: &'static str,
    pub supports_resume: bool,
    pub supports_model: bool,
    pub supports_effort: bool,
    pub supports_mode: bool,
    pub supports_sandbox: bool,
    pub modes: &'static [&'static str],
    pub efforts: &'static [&'static str],
}

const AGENT_DRIVERS: &[AgentDriverSpec] = &[
    AgentDriverSpec {
        id: "claude",
        binary: "claude",
        supports_resume: true,
        supports_model: true,
        supports_effort: true,
        supports_mode: true,
        supports_sandbox: false,
        modes: &["default", "plan", "acceptEdits", "dontAsk"],
        efforts: &["low", "medium", "high", "max"],
    },
    AgentDriverSpec {
        id: "codex",
        binary: "codex",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: true,
        supports_sandbox: false,
        modes: &["read-only", "workspace-write"],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "gemini",
        binary: "gemini",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: true,
        modes: &[],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "grokbuild",
        binary: "grok",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: false,
        modes: &[],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "opencode",
        binary: "opencode",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: true,
        supports_sandbox: false,
        modes: &["default", "thinking"],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "agy",
        binary: "agy",
        supports_resume: true,
        supports_model: true,
        supports_effort: true,
        supports_mode: true,
        supports_sandbox: true,
        modes: &["plan", "accept-edits", "auto"],
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    AgentDriverSpec {
        id: "copilot",
        binary: "copilot",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: false,
        modes: &[],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "kimi",
        binary: "kimi",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: false,
        modes: &[],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "pi",
        binary: "pi",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: false,
        modes: &[],
        efforts: &[],
    },
    AgentDriverSpec {
        id: "omp",
        binary: "omp",
        supports_resume: true,
        supports_model: true,
        supports_effort: false,
        supports_mode: false,
        supports_sandbox: false,
        modes: &[],
        efforts: &[],
    },
];

pub fn find_driver(app: &str) -> Option<&'static AgentDriverSpec> {
    AGENT_DRIVERS.iter().find(|d| d.id == app)
}

pub fn is_supported_remote_agent(app: &str) -> bool {
    find_driver(app).is_some()
}

/// Filter and deduplicate an arbitrary list of app IDs to supported remote agents
/// in canonical CC Switch order.
pub fn sanitize_remote_control_apps(apps: &[String]) -> Vec<String> {
    let requested: BTreeSet<&str> = apps.iter().map(|s| s.trim()).collect();
    SUPPORTED_REMOTE_AGENTS
        .iter()
        .filter(|id| requested.contains(**id))
        .map(|id| (*id).to_string())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRunStatus {
    Running,
    Completed,
    Error,
    Cancelled,
}

impl RemoteRunStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEventKind {
    Started,
    Init,
    Thought,
    Text,
    ToolCall,
    ToolResult,
    Stderr,
    Completed,
    Error,
    Cancelled,
}

impl RemoteEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Init => "init",
            Self::Thought => "thought",
            Self::Text => "text",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Stderr => "stderr",
            Self::Completed => "completed",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Error | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStreamEvent {
    pub seq: u64,
    pub ts: i64,
    pub kind: RemoteEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRunSnapshot {
    pub run_id: String,
    pub app: String,
    pub status: RemoteRunStatus,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub sandbox: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRemoteRunRequest {
    pub app: String,
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub sandbox: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentInfo {
    pub id: String,
    pub installed: bool,
    pub enabled: bool,
    pub supports_resume: bool,
    pub supports_model: bool,
    pub supports_effort: bool,
    pub supports_mode: bool,
    pub supports_sandbox: bool,
    pub modes: Vec<String>,
    pub efforts: Vec<String>,
    pub active_run_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentsResponse {
    pub agents: Vec<RemoteAgentInfo>,
    pub default_cwd: String,
    pub recent_workspaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeRemoteSession {
    pub app: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    pub resumable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionsResponse {
    pub sessions: Vec<SafeRemoteSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionMessagesResponse {
    pub session: SafeRemoteSession,
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRunsResponse {
    pub active_run_count: usize,
    pub runs: Vec<RemoteRunSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRunDetailResponse {
    pub run: RemoteRunSnapshot,
    pub events: Vec<RemoteStreamEvent>,
}

#[derive(Debug)]
pub enum RemoteAgentError {
    BadRequest(String),
    Forbidden(String),
    NotFound(String),
    TooManyRequests(String),
    Unavailable(String),
    Internal(String),
}

struct RunEntry {
    snapshot: RemoteRunSnapshot,
    events: VecDeque<RemoteStreamEvent>,
    tx: broadcast::Sender<RemoteStreamEvent>,
    cancel_tx: Option<oneshot::Sender<()>>,
    done_tx: watch::Sender<bool>,
    done_rx: watch::Receiver<bool>,
    streamed_text: bool,
}

#[derive(Default)]
struct RunStore {
    runs: HashMap<String, RunEntry>,
    order: VecDeque<String>,
}

impl RunStore {
    fn active_run_count(&self) -> usize {
        self.runs
            .values()
            .filter(|entry| entry.snapshot.status == RemoteRunStatus::Running)
            .count()
    }

    fn active_run_count_for_app(&self, app: &str) -> usize {
        self.runs
            .values()
            .filter(|entry| {
                entry.snapshot.status == RemoteRunStatus::Running && entry.snapshot.app == app
            })
            .count()
    }

    fn active_run_for_session(&self, app: &str, session_id: &str) -> Option<String> {
        self.order.iter().rev().find_map(|run_id| {
            let entry = self.runs.get(run_id)?;
            if entry.snapshot.status != RemoteRunStatus::Running || entry.snapshot.app != app {
                return None;
            }
            let matches = entry.snapshot.session_id.as_deref() == Some(session_id)
                || entry.snapshot.conversation_id.as_deref() == Some(session_id);
            matches.then(|| run_id.clone())
        })
    }

    fn evict_if_needed(&mut self) {
        while self.order.len() >= MAX_RETAINED_RUNS {
            let evict_idx = self
                .order
                .iter()
                .position(|id| {
                    self.runs
                        .get(id)
                        .is_none_or(|e| e.snapshot.status.is_terminal())
                })
                .unwrap_or(0);
            if let Some(evicted_id) = self.order.remove(evict_idx) {
                self.runs.remove(&evicted_id);
            } else {
                break;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_event(
        &mut self,
        run_id: &str,
        kind: RemoteEventKind,
        text: Option<String>,
        tool_name: Option<String>,
        tool_id: Option<String>,
        is_error: Option<bool>,
        session_id: Option<String>,
        exit_code: Option<i32>,
    ) -> Option<RemoteStreamEvent> {
        let entry = self.runs.get_mut(run_id)?;
        let now = Utc::now().timestamp_millis();
        let next_seq = entry.snapshot.last_seq + 1;

        if let Some(sid) = session_id.as_ref().filter(|s| !s.is_empty()) {
            entry.snapshot.conversation_id = Some(sid.clone());
        }
        if kind == RemoteEventKind::Text && text.as_deref().is_some_and(|t| !t.is_empty()) {
            entry.streamed_text = true;
        }

        let event = RemoteStreamEvent {
            seq: next_seq,
            ts: now,
            kind,
            text,
            tool_name,
            tool_id,
            is_error,
            session_id,
            exit_code,
        };

        entry.snapshot.last_seq = next_seq;
        entry.snapshot.updated_at = now;
        if entry.events.len() >= MAX_EVENTS_PER_RUN {
            entry.events.pop_front();
        }
        entry.events.push_back(event.clone());
        let _ = entry.tx.send(event.clone());
        Some(event)
    }

    fn finish_run(
        &mut self,
        run_id: &str,
        status: RemoteRunStatus,
        exit_code: Option<i32>,
        error: Option<String>,
    ) {
        let terminal_kind = match status {
            RemoteRunStatus::Completed => RemoteEventKind::Completed,
            RemoteRunStatus::Error => RemoteEventKind::Error,
            RemoteRunStatus::Cancelled => RemoteEventKind::Cancelled,
            RemoteRunStatus::Running => return,
        };

        let Some(entry) = self.runs.get_mut(run_id) else {
            return;
        };
        if entry.snapshot.status.is_terminal() {
            return;
        }
        let now = Utc::now().timestamp_millis();
        entry.snapshot.status = status;
        entry.snapshot.exit_code = exit_code;
        entry.snapshot.error = error.clone();
        entry.snapshot.finished_at = Some(now);
        entry.snapshot.updated_at = now;
        entry.cancel_tx = None;
        let conv_id = entry.snapshot.conversation_id.clone();
        let done_tx = entry.done_tx.clone();

        let _ = self.push_event(
            run_id,
            terminal_kind,
            error,
            None,
            None,
            if status == RemoteRunStatus::Error {
                Some(true)
            } else {
                None
            },
            conv_id,
            exit_code,
        );
        let _ = done_tx.send(true);
    }
}

pub struct AgentRunManager {
    store: Arc<Mutex<RunStore>>,
    bin_overrides: HashMap<String, PathBuf>,
}

impl Default for AgentRunManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRunManager {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(RunStore::default())),
            bin_overrides: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_bin_overrides(bin_overrides: HashMap<String, PathBuf>) -> Self {
        Self {
            store: Arc::new(Mutex::new(RunStore::default())),
            bin_overrides,
        }
    }

    async fn resolve_bin(&self, driver: &'static AgentDriverSpec) -> Option<PathBuf> {
        if let Some(override_bin) = self.bin_overrides.get(driver.id) {
            return override_bin.is_file().then(|| override_bin.clone());
        }
        let app_id = driver.id;
        let binary = driver.binary;
        tokio::task::spawn_blocking(move || resolve_agent_binary(app_id, binary))
            .await
            .ok()
            .flatten()
    }

    pub async fn installed_agent_ids(&self) -> Vec<String> {
        let overrides = self.bin_overrides.clone();
        tokio::task::spawn_blocking(move || {
            AGENT_DRIVERS
                .iter()
                .filter(|d| {
                    if let Some(p) = overrides.get(d.id) {
                        p.is_file()
                    } else {
                        resolve_agent_binary(d.id, d.binary).is_some()
                    }
                })
                .map(|d| d.id.to_string())
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    pub async fn list_agents(&self, enabled_apps: &[String]) -> RemoteAgentsResponse {
        let enabled_set: BTreeSet<&str> = enabled_apps.iter().map(String::as_str).collect();
        let installed_list = self.installed_agent_ids().await;
        let installed_set: BTreeSet<&str> = installed_list.iter().map(String::as_str).collect();

        let store = self.store.lock().await;
        let agents: Vec<RemoteAgentInfo> = AGENT_DRIVERS
            .iter()
            .map(|d| RemoteAgentInfo {
                id: d.id.to_string(),
                installed: installed_set.contains(d.id),
                enabled: enabled_set.contains(d.id),
                supports_resume: d.supports_resume,
                supports_model: d.supports_model,
                supports_effort: d.supports_effort,
                supports_mode: d.supports_mode,
                supports_sandbox: d.supports_sandbox,
                modes: d.modes.iter().map(|s| (*s).to_string()).collect(),
                efforts: d.efforts.iter().map(|s| (*s).to_string()).collect(),
                active_run_count: store.active_run_count_for_app(d.id),
            })
            .collect();
        drop(store);

        let default_cwd = default_working_dir().to_string_lossy().to_string();
        let recent_workspaces = tokio::task::spawn_blocking(|| {
            let sessions = scan_supported_sessions();
            collect_recent_workspaces(&sessions, 12)
        })
        .await
        .unwrap_or_default();

        RemoteAgentsResponse {
            agents,
            default_cwd,
            recent_workspaces,
        }
    }

    pub async fn list_sessions(
        &self,
        app_filter: Option<&str>,
        limit: usize,
        query: Option<&str>,
    ) -> Result<RemoteSessionsResponse, RemoteAgentError> {
        let filter_owned = match app_filter.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("all") => None,
            Some(id) => {
                if !is_supported_remote_agent(id) {
                    return Err(RemoteAgentError::BadRequest(format!(
                        "Unsupported remote agent '{id}'"
                    )));
                }
                Some(id.to_string())
            }
        };
        let query_owned = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| q.to_lowercase());
        let limit = limit.clamp(1, 200);

        let scanned = tokio::task::spawn_blocking(scan_supported_sessions)
            .await
            .unwrap_or_default();

        let store = self.store.lock().await;
        let mut sessions: Vec<SafeRemoteSession> = scanned
            .into_iter()
            .filter(|meta| {
                if let Some(ref f) = filter_owned {
                    if meta.provider_id != *f {
                        return false;
                    }
                }
                let Some(ref q) = query_owned else {
                    return true;
                };
                meta.session_id.to_lowercase().contains(q)
                    || meta.provider_id.to_lowercase().contains(q)
                    || meta
                        .title
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(q))
                    || meta
                        .summary
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(q))
                    || meta
                        .project_dir
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(q))
            })
            .map(|meta| to_safe_session(meta, &store))
            .collect();

        sessions.sort_by(|a, b| {
            let ta = a.last_active_at.or(a.created_at).unwrap_or(0);
            let tb = b.last_active_at.or(b.created_at).unwrap_or(0);
            tb.cmp(&ta)
        });
        sessions.truncate(limit);

        Ok(RemoteSessionsResponse { sessions })
    }

    pub async fn get_session_messages(
        &self,
        app: &str,
        session_id: &str,
    ) -> Result<RemoteSessionMessagesResponse, RemoteAgentError> {
        let Some(driver) = find_driver(app.trim()) else {
            return Err(RemoteAgentError::BadRequest(format!(
                "Unsupported remote agent '{app}'"
            )));
        };
        let clean_id = validate_session_id(session_id)?;
        let app_id = driver.id.to_string();
        let id_for_scan = clean_id.clone();

        let found = tokio::task::spawn_blocking(move || {
            let sessions = scan_supported_sessions();
            let meta = sessions
                .into_iter()
                .find(|s| s.provider_id == app_id && s.session_id == id_for_scan)?;
            let messages = match meta.source_path.as_deref() {
                Some(path) => session_manager::load_messages(&meta.provider_id, path),
                None => Ok(Vec::new()),
            };
            Some((meta, messages))
        })
        .await
        .map_err(|e| RemoteAgentError::Internal(format!("Failed to load session: {e}")))?;

        let Some((meta, messages_res)) = found else {
            return Err(RemoteAgentError::NotFound(format!(
                "Session '{clean_id}' not found for agent '{}'",
                driver.id
            )));
        };

        let messages = messages_res.map_err(RemoteAgentError::Internal)?;
        let store = self.store.lock().await;
        let session = to_safe_session(meta, &store);
        Ok(RemoteSessionMessagesResponse { session, messages })
    }

    pub async fn list_runs(&self, app_filter: Option<&str>) -> RemoteRunsResponse {
        let filter = app_filter
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "all");
        let store = self.store.lock().await;
        let runs: Vec<RemoteRunSnapshot> = store
            .order
            .iter()
            .rev()
            .filter_map(|id| store.runs.get(id).map(|e| e.snapshot.clone()))
            .filter(|snap| filter.is_none_or(|f| snap.app == f))
            .collect();
        let active_run_count = match filter {
            Some(f) => store.active_run_count_for_app(f),
            None => store.active_run_count(),
        };
        RemoteRunsResponse {
            active_run_count,
            runs,
        }
    }

    pub async fn get_run(
        &self,
        run_id: &str,
        after_seq: u64,
    ) -> Result<RemoteRunDetailResponse, RemoteAgentError> {
        let store = self.store.lock().await;
        let Some(entry) = store.runs.get(run_id) else {
            return Err(RemoteAgentError::NotFound(format!(
                "Run '{run_id}' not found"
            )));
        };
        let events = entry
            .events
            .iter()
            .filter(|ev| ev.seq > after_seq)
            .cloned()
            .collect();
        Ok(RemoteRunDetailResponse {
            run: entry.snapshot.clone(),
            events,
        })
    }

    pub async fn get_run_app(&self, run_id: &str) -> Result<String, RemoteAgentError> {
        let store = self.store.lock().await;
        store
            .runs
            .get(run_id)
            .map(|e| e.snapshot.app.clone())
            .ok_or_else(|| RemoteAgentError::NotFound(format!("Run '{run_id}' not found")))
    }

    pub async fn subscribe_run(
        &self,
        run_id: &str,
        after_seq: u64,
    ) -> Result<
        (
            Vec<RemoteStreamEvent>,
            bool,
            broadcast::Receiver<RemoteStreamEvent>,
        ),
        RemoteAgentError,
    > {
        let store = self.store.lock().await;
        let Some(entry) = store.runs.get(run_id) else {
            return Err(RemoteAgentError::NotFound(format!(
                "Run '{run_id}' not found"
            )));
        };
        // Subscribe BEFORE snapshotting backlog so no events are lost.
        let rx = entry.tx.subscribe();
        let replay = entry
            .events
            .iter()
            .filter(|ev| ev.seq > after_seq)
            .cloned()
            .collect();
        let is_terminal = entry.snapshot.status.is_terminal();
        Ok((replay, is_terminal, rx))
    }

    pub async fn replay_since(&self, run_id: &str, after_seq: u64) -> Vec<RemoteStreamEvent> {
        let store = self.store.lock().await;
        store
            .runs
            .get(run_id)
            .map(|entry| {
                entry
                    .events
                    .iter()
                    .filter(|ev| ev.seq > after_seq)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn start_run(
        &self,
        app_state: &AppState,
        enabled_apps: &[String],
        req: StartRemoteRunRequest,
    ) -> Result<RemoteRunSnapshot, RemoteAgentError> {
        // 1. Validate driver & per-agent permission gate.
        let app_trimmed = req.app.trim();
        let Some(driver) = find_driver(app_trimmed) else {
            return Err(RemoteAgentError::BadRequest(format!(
                "Unsupported remote agent '{app_trimmed}'"
            )));
        };
        if !enabled_apps.iter().any(|id| id == driver.id) {
            return Err(RemoteAgentError::Forbidden(format!(
                "Remote control is disabled for '{}'. Enable it in desktop Settings → Network.",
                driver.id
            )));
        }

        // 2. Strict input validation per driver.
        let prompt = validate_prompt(&req.prompt)?;
        let model = validate_optional_model(req.model.as_deref())?;
        let effort = validate_optional_effort(driver, req.effort.as_deref())?;
        let mode = validate_optional_mode(driver, req.mode.as_deref())?;
        let requested_session_id = match req.session_id.as_deref().map(str::trim) {
            Some(raw) if !raw.is_empty() => Some(validate_session_id(raw)?),
            _ => None,
        };

        // 3. Resolve real CLI binary.
        let bin = self.resolve_bin(driver).await.ok_or_else(|| {
            RemoteAgentError::Unavailable(format!(
                "CLI binary '{}' for agent '{}' not found on this machine",
                driver.binary, driver.id
            ))
        })?;

        // 4. Resolve optional session server-side (NEVER trust client paths or --gemini_dir).
        let mut resumed_meta: Option<SessionMeta> = None;
        let mut resume_target: Option<ResumeTarget> = None;
        let mut private_env: Vec<(&'static str, Option<String>)> = Vec::new();
        let mut secrets_to_redact: Vec<String> = Vec::new();
        let mut resolved_conv_id: Option<String> = None;

        if let Some(ref sid) = requested_session_id {
            let app_id = driver.id.to_string();
            let sid_clone = sid.clone();
            let meta = tokio::task::spawn_blocking(move || {
                scan_supported_sessions()
                    .into_iter()
                    .find(|s| s.provider_id == app_id && s.session_id == sid_clone)
            })
            .await
            .map_err(|e| RemoteAgentError::Internal(format!("Session lookup failed: {e}")))?
            .ok_or_else(|| {
                RemoteAgentError::NotFound(format!(
                    "Session '{sid}' not found for agent '{}'",
                    driver.id
                ))
            })?;

            if !is_session_resumable(driver.id, &meta) {
                return Err(RemoteAgentError::BadRequest(format!(
                    "Session '{sid}' cannot be resumed via '{}' CLI",
                    driver.id
                )));
            }

            if driver.id == "agy" {
                let resume_cmd = meta.resume_command.clone().unwrap_or_default();
                let db = app_state.db.clone();
                let key_target = tokio::task::spawn_blocking(move || {
                    let target = agy_direct::key_resume_target(&db, &resume_cmd)?;
                    match target {
                        Some((provider, conv_id)) => {
                            let launch = agy_direct::terminal_launch(&provider)?;
                            Ok::<_, crate::error::AppError>(Some((launch, conv_id)))
                        }
                        None => Ok(None),
                    }
                })
                .await
                .map_err(|e| RemoteAgentError::Internal(format!("Key profile lookup failed: {e}")))?
                .map_err(|e| RemoteAgentError::Internal(e.to_string()))?;

                if let Some((launch, conv_id)) = key_target {
                    for (k, v) in &launch.env {
                        let key_upper = k.to_ascii_uppercase();
                        if key_upper.contains("KEY")
                            || key_upper.contains("TOKEN")
                            || key_upper.contains("SECRET")
                        {
                            if let Some(secret) = v.as_ref().filter(|s| !s.trim().is_empty()) {
                                secrets_to_redact.push(secret.clone());
                            }
                        }
                    }
                    private_env = launch.env;
                    resolved_conv_id = Some(conv_id.clone());
                    resume_target = Some(ResumeTarget {
                        session_id: conv_id,
                        source_path: meta.source_path.clone(),
                        agy_gemini_dir: launch.gemini_dir,
                    });
                } else {
                    resolved_conv_id = Some(sid.clone());
                    resume_target = Some(ResumeTarget {
                        session_id: sid.clone(),
                        source_path: meta.source_path.clone(),
                        agy_gemini_dir: None,
                    });
                }
            } else {
                resolved_conv_id = Some(sid.clone());
                resume_target = Some(ResumeTarget {
                    session_id: sid.clone(),
                    source_path: meta.source_path.clone(),
                    agy_gemini_dir: None,
                });
            }

            resumed_meta = Some(meta);
        }

        // 5. Resolve working directory.
        let cwd = resolve_working_dir(
            req.cwd.as_deref(),
            resumed_meta.as_ref().and_then(|m| m.project_dir.as_deref()),
        )?;
        let cwd_str = cwd.to_string_lossy().to_string();

        // 6. Build driver argv explicitly (zero shell, never --dangerously-skip-permissions).
        let args = build_driver_args(
            driver,
            &DriverRunParams {
                prompt: &prompt,
                resume: resume_target.as_ref(),
                model: model.as_deref(),
                effort: effort.as_deref(),
                mode: mode.as_deref(),
                sandbox: req.sandbox,
            },
        )?;

        // 7. Enforce concurrency cap and register run entry atomically.
        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = watch::channel(false);

        let initial_snapshot = RemoteRunSnapshot {
            run_id: run_id.clone(),
            app: driver.id.to_string(),
            status: RemoteRunStatus::Running,
            prompt: prompt.clone(),
            session_id: requested_session_id.clone(),
            conversation_id: resolved_conv_id.clone(),
            cwd: cwd_str,
            model: model.clone(),
            effort: effort.clone(),
            mode: mode.clone(),
            sandbox: req.sandbox,
            created_at: now,
            updated_at: now,
            finished_at: None,
            exit_code: None,
            error: None,
            last_seq: 0,
        };

        {
            let mut store = self.store.lock().await;
            if store.active_run_count() >= MAX_CONCURRENT_RUNS {
                return Err(RemoteAgentError::TooManyRequests(format!(
                    "Maximum concurrent remote runs ({MAX_CONCURRENT_RUNS}) reached"
                )));
            }
            store.evict_if_needed();
            store.runs.insert(
                run_id.clone(),
                RunEntry {
                    snapshot: initial_snapshot,
                    events: VecDeque::new(),
                    tx,
                    cancel_tx: Some(cancel_tx),
                    done_tx,
                    done_rx,
                    streamed_text: false,
                },
            );
            store.order.push_back(run_id.clone());
            store.push_event(
                &run_id,
                RemoteEventKind::Started,
                None,
                None,
                None,
                None,
                resolved_conv_id.clone(),
                None,
            );
        }

        // 8. Spawn child process directly without shell.
        let mut command = agy_config::background_command(&bin);
        command
            .kill_on_drop(true)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(&args);

        for (key, val) in &private_env {
            match val {
                Some(v) => {
                    command.env(key, v);
                }
                None => {
                    command.env_remove(key);
                }
            }
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let msg = redact_secrets(
                    &format!("Failed to start '{}' CLI: {err}", driver.id),
                    &secrets_to_redact,
                );
                let mut store = self.store.lock().await;
                store.finish_run(&run_id, RemoteRunStatus::Error, None, Some(msg.clone()));
                return Err(RemoteAgentError::Internal(msg));
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let store_ref = self.store.clone();
        let run_id_task = run_id.clone();
        let secrets = Arc::new(secrets_to_redact);
        let driver_id = driver.id;

        tokio::spawn(async move {
            let mut stdout_handle = stdout.map(|out| {
                let store = store_ref.clone();
                let rid = run_id_task.clone();
                let secs = secrets.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(out);
                    let mut buf = Vec::new();
                    while let Ok(Some(raw_line)) = read_bounded_line(&mut reader, &mut buf).await {
                        let trimmed = raw_line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let parsed = parse_stream_line(trimmed);
                        let mut guard = store.lock().await;
                        for ev in parsed {
                            if ev.from_result_summary {
                                let already_streamed =
                                    guard.runs.get(&rid).is_some_and(|e| e.streamed_text);
                                if already_streamed && ev.kind == RemoteEventKind::Text {
                                    if let Some(sid) = ev.session_id {
                                        if let Some(entry) = guard.runs.get_mut(&rid) {
                                            entry.snapshot.conversation_id = Some(sid);
                                        }
                                    }
                                    continue;
                                }
                            }
                            let text = ev.text.map(|t| redact_secrets(&t, &secs));
                            let tool_name = ev.tool_name.map(|t| redact_secrets(&t, &secs));
                            let tool_id = ev.tool_id.map(|t| redact_secrets(&t, &secs));
                            guard.push_event(
                                &rid,
                                ev.kind,
                                text,
                                tool_name,
                                tool_id,
                                ev.is_error,
                                ev.session_id,
                                None,
                            );
                        }
                    }
                })
            });

            let mut stderr_handle = stderr.map(|err_pipe| {
                let store = store_ref.clone();
                let rid = run_id_task.clone();
                let secs = secrets.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(err_pipe);
                    let mut buf = Vec::new();
                    while let Ok(Some(raw_line)) = read_bounded_line(&mut reader, &mut buf).await {
                        let trimmed = raw_line.trim_end();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let clean = redact_secrets(trimmed, &secs);
                        let mut guard = store.lock().await;
                        guard.push_event(
                            &rid,
                            RemoteEventKind::Stderr,
                            Some(clean),
                            None,
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                })
            });

            enum Outcome {
                Exited(std::io::Result<std::process::ExitStatus>),
                Cancelled,
            }

            let outcome = tokio::select! {
                status = child.wait() => Outcome::Exited(status),
                _ = cancel_rx => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Outcome::Cancelled
                }
            };

            if let Some(h) = stdout_handle.as_mut() {
                if tokio::time::timeout(Duration::from_secs(2), &mut *h)
                    .await
                    .is_err()
                {
                    h.abort();
                }
            }
            if let Some(h) = stderr_handle.as_mut() {
                if tokio::time::timeout(Duration::from_secs(2), &mut *h)
                    .await
                    .is_err()
                {
                    h.abort();
                }
            }

            let mut guard = store_ref.lock().await;
            match outcome {
                Outcome::Cancelled => {
                    guard.finish_run(&run_id_task, RemoteRunStatus::Cancelled, None, None);
                }
                Outcome::Exited(Ok(status)) => {
                    let code = status.code();
                    if status.success() {
                        guard.finish_run(&run_id_task, RemoteRunStatus::Completed, code, None);
                    } else {
                        let err_msg = redact_secrets(
                            &format!(
                                "Agent '{driver_id}' exited with status {}",
                                code.map(|c| c.to_string())
                                    .unwrap_or_else(|| "signal".to_string())
                            ),
                            &secrets,
                        );
                        guard.finish_run(&run_id_task, RemoteRunStatus::Error, code, Some(err_msg));
                    }
                }
                Outcome::Exited(Err(err)) => {
                    let err_msg = redact_secrets(&format!("Process wait error: {err}"), &secrets);
                    guard.finish_run(&run_id_task, RemoteRunStatus::Error, None, Some(err_msg));
                }
            }
        });

        let snapshot = {
            let store = self.store.lock().await;
            store
                .runs
                .get(&run_id)
                .map(|e| e.snapshot.clone())
                .ok_or_else(|| RemoteAgentError::Internal("Run vanished immediately".into()))?
        };
        Ok(snapshot)
    }

    pub async fn cancel_run(&self, run_id: &str) -> Result<RemoteRunSnapshot, RemoteAgentError> {
        let mut done_rx = {
            let mut store = self.store.lock().await;
            let Some(entry) = store.runs.get_mut(run_id) else {
                return Err(RemoteAgentError::NotFound(format!(
                    "Run '{run_id}' not found"
                )));
            };
            if entry.snapshot.status.is_terminal() {
                return Ok(entry.snapshot.clone());
            }
            if let Some(tx) = entry.cancel_tx.take() {
                let _ = tx.send(());
            }
            entry.done_rx.clone()
        };

        let _ = tokio::time::timeout(Duration::from_secs(2), done_rx.wait_for(|&done| done)).await;

        let store = self.store.lock().await;
        store
            .runs
            .get(run_id)
            .map(|e| e.snapshot.clone())
            .ok_or_else(|| RemoteAgentError::NotFound(format!("Run '{run_id}' not found")))
    }

    /// Kill all active agent child processes cleanly during shutdown/stop.
    pub async fn cancel_all(&self) {
        let mut receivers: Vec<watch::Receiver<bool>> = {
            let mut store = self.store.lock().await;
            let mut out = Vec::new();
            for entry in store.runs.values_mut() {
                if !entry.snapshot.status.is_terminal() {
                    if let Some(tx) = entry.cancel_tx.take() {
                        let _ = tx.send(());
                    }
                    out.push(entry.done_rx.clone());
                }
            }
            out
        };

        if receivers.is_empty() {
            return;
        }

        let wait_all = async {
            for rx in &mut receivers {
                let _ = rx.wait_for(|&done| done).await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(2), wait_all).await;
    }
}

/// Read a single line from an async buffered reader with a strict byte cap
/// (`MAX_STREAM_LINE_BYTES`) and lossy UTF-8 conversion so invalid UTF-8 bytes
/// never abort the stream and giant lines without `\n` cannot spike memory.
async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<Option<String>> {
    buf.clear();
    let mut saw_any = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if !saw_any {
                return Ok(None);
            }
            break;
        }
        saw_any = true;
        if let Some(nl_pos) = available.iter().position(|&b| b == b'\n') {
            let chunk = &available[..nl_pos];
            let remaining = MAX_STREAM_LINE_BYTES.saturating_sub(buf.len());
            if remaining > 0 {
                buf.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            reader.consume(nl_pos + 1);
            break;
        } else {
            let remaining = MAX_STREAM_LINE_BYTES.saturating_sub(buf.len());
            if remaining > 0 {
                buf.extend_from_slice(&available[..available.len().min(remaining)]);
            }
            let len = available.len();
            reader.consume(len);
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(buf).into_owned()))
}

// ============================================================
// Driver Argv Construction (Pure & Exhaustively Unit-Tested)
// ============================================================

#[derive(Debug, Clone)]
pub struct ResumeTarget {
    pub session_id: String,
    pub source_path: Option<String>,
    pub agy_gemini_dir: Option<PathBuf>,
}

pub struct DriverRunParams<'a> {
    pub prompt: &'a str,
    pub resume: Option<&'a ResumeTarget>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub sandbox: bool,
}

pub fn build_driver_args(
    driver: &AgentDriverSpec,
    params: &DriverRunParams<'_>,
) -> Result<Vec<String>, RemoteAgentError> {
    let mut args = Vec::new();
    match driver.id {
        "claude" => {
            // `--print` is a boolean flag in Claude Code; emit all options first,
            // then `--`, then the positional prompt so leading-`-` prompts can
            // never be parsed as CLI flags (e.g. `--dangerously-skip-permissions`).
            args.push("--print".into());
            args.push("--output-format".into());
            args.push("stream-json".into());
            args.push("--verbose".into());
            if let Some(r) = params.resume {
                args.push("--resume".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            if let Some(e) = params.effort {
                args.push("--effort".into());
                args.push(e.into());
            }
            if let Some(mode) = params.mode {
                args.push("--permission-mode".into());
                args.push(mode.into());
            }
            args.push("--".into());
            args.push(params.prompt.into());
        }
        "codex" => {
            if let Some(r) = params.resume {
                args.push("exec".into());
                args.push("resume".into());
                args.push("--json".into());
                if let Some(m) = params.model {
                    args.push("--model".into());
                    args.push(m.into());
                }
                args.push("--".into());
                args.push(r.session_id.clone());
                args.push(params.prompt.into());
            } else {
                args.push("exec".into());
                args.push("--json".into());
                if let Some(m) = params.model {
                    args.push("--model".into());
                    args.push(m.into());
                }
                if let Some(mode) = params.mode {
                    args.push("--sandbox".into());
                    args.push(mode.into());
                }
                args.push("--".into());
                args.push(params.prompt.into());
            }
        }
        "gemini" => {
            args.push(format!("--prompt={}", params.prompt));
            args.push("--output-format".into());
            args.push("stream-json".into());
            if let Some(r) = params.resume {
                args.push("--resume".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            if params.sandbox {
                args.push("--sandbox".into());
            }
        }
        "grokbuild" => {
            args.push(format!("--prompt={}", params.prompt));
            args.push("--output-format".into());
            args.push("json".into());
            if let Some(r) = params.resume {
                args.push("--resume".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
        }
        "opencode" => {
            args.push("run".into());
            args.push("--format".into());
            args.push("json".into());
            if let Some(r) = params.resume {
                args.push("--session".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            if params.mode == Some("thinking") {
                args.push("--thinking".into());
            }
            args.push("--".into());
            args.push(params.prompt.into());
        }
        "agy" => {
            if let Some(dir) = params.resume.and_then(|r| r.agy_gemini_dir.as_ref()) {
                args.push(format!("--gemini_dir={}", dir.display()));
            }
            args.push(format!("--print={}", params.prompt));
            args.push("--output-format".into());
            args.push("stream-json".into());
            if let Some(r) = params.resume {
                args.push("--conversation".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            if let Some(e) = params.effort {
                args.push("--effort".into());
                args.push(e.into());
            }
            let effective_mode = params.mode.unwrap_or("accept-edits");
            args.push("--mode".into());
            args.push(effective_mode.into());
            if params.sandbox {
                args.push("--sandbox".into());
            }
        }
        "copilot" => {
            args.push(format!("--prompt={}", params.prompt));
            if let Some(r) = params.resume {
                args.push(format!("--resume={}", r.session_id));
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
        }
        "kimi" => {
            args.push("--print".into());
            args.push("--output-format".into());
            args.push("stream-json".into());
            if let Some(r) = params.resume {
                args.push("--session".into());
                args.push(r.session_id.clone());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            args.push(format!("--prompt={}", params.prompt));
        }
        "pi" | "omp" => {
            args.push("--print".into());
            args.push("--mode".into());
            args.push("json".into());
            if let Some(r) = params.resume {
                let source = r.source_path.as_deref().ok_or_else(|| {
                    RemoteAgentError::BadRequest(format!(
                        "Session '{}' is missing a local transcript path required by '{}'",
                        r.session_id, driver.id
                    ))
                })?;
                args.push("--session".into());
                args.push(source.to_string());
            }
            if let Some(m) = params.model {
                args.push("--model".into());
                args.push(m.into());
            }
            args.push("--".into());
            args.push(params.prompt.into());
        }
        other => {
            return Err(RemoteAgentError::BadRequest(format!(
                "Unsupported remote agent '{other}'"
            )));
        }
    }
    Ok(args)
}

// ============================================================
// Binary Resolution & Session Scanning Helpers
// ============================================================

fn resolve_agent_binary(app_id: &str, binary_name: &str) -> Option<PathBuf> {
    if app_id == "agy" {
        return agy_config::real_agy_bin();
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let home = crate::config::get_home_dir();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path_var)
        .filter(|d| !d.as_os_str().is_empty())
        .collect();
    dirs.push(home.join(".local").join("bin"));
    dirs.push(home.join(".opencode").join("bin"));
    dirs.push(home.join(".cargo").join("bin"));
    dirs.push(home.join(".bun").join("bin"));
    if !cfg!(windows) {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }

    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{binary_name}.exe"),
            format!("{binary_name}.cmd"),
            binary_name.to_string(),
        ]
    } else {
        vec![binary_name.to_string()]
    };

    for dir in dirs {
        for name in &names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn scan_supported_sessions() -> Vec<SessionMeta> {
    session_manager::scan_sessions()
        .into_iter()
        .filter(|meta| is_supported_remote_agent(&meta.provider_id))
        .collect()
}

fn is_session_resumable(app: &str, meta: &SessionMeta) -> bool {
    match app {
        "agy" => meta.resume_command.is_some(),
        "pi" | "omp" => meta.source_path.as_deref().is_some_and(|p| !p.is_empty()),
        _ => true,
    }
}

fn to_safe_session(meta: SessionMeta, store: &RunStore) -> SafeRemoteSession {
    let resumable = is_session_resumable(&meta.provider_id, &meta);
    let active_run_id = store.active_run_for_session(&meta.provider_id, &meta.session_id);
    SafeRemoteSession {
        app: meta.provider_id,
        session_id: meta.session_id,
        title: meta.title,
        summary: meta.summary,
        project_dir: meta.project_dir,
        created_at: meta.created_at,
        last_active_at: meta.last_active_at,
        resumable,
        active_run_id,
    }
}

fn collect_recent_workspaces(sessions: &[SessionMeta], max: usize) -> Vec<String> {
    let mut sorted: Vec<&SessionMeta> = sessions.iter().collect();
    sorted.sort_by(|a, b| {
        let ta = a.last_active_at.or(a.created_at).unwrap_or(0);
        let tb = b.last_active_at.or(b.created_at).unwrap_or(0);
        tb.cmp(&ta)
    });

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for meta in sorted {
        let Some(dir) = meta.project_dir.as_deref().map(str::trim) else {
            continue;
        };
        if dir.is_empty() || !Path::new(dir).is_dir() {
            continue;
        }
        if seen.insert(dir.to_string()) {
            out.push(dir.to_string());
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

// ============================================================
// Validation & Path Helpers
// ============================================================

fn validate_prompt(raw: &str) -> Result<String, RemoteAgentError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RemoteAgentError::BadRequest(
            "Prompt must not be empty".into(),
        ));
    }
    if trimmed.len() > MAX_PROMPT_BYTES {
        return Err(RemoteAgentError::BadRequest(format!(
            "Prompt exceeds maximum length of {MAX_PROMPT_BYTES} bytes"
        )));
    }
    if trimmed.contains('\0') {
        return Err(RemoteAgentError::BadRequest(
            "Prompt must not contain NUL bytes".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_session_id(raw: &str) -> Result<String, RemoteAgentError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(RemoteAgentError::BadRequest(
            "sessionId must be between 1 and 128 characters".into(),
        ));
    }
    if trimmed.starts_with('-') || trimmed == "." || trimmed == ".." {
        return Err(RemoteAgentError::BadRequest(
            "sessionId contains invalid leading character".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(RemoteAgentError::BadRequest(
            "sessionId may only contain A-Z, a-z, 0-9, '.', '_', and '-'".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_optional_model(raw: Option<&str>) -> Result<Option<String>, RemoteAgentError> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if trimmed.len() > 128 {
        return Err(RemoteAgentError::BadRequest(
            "model must be at most 128 characters".into(),
        ));
    }
    if trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err(RemoteAgentError::BadRequest(
            "model must not start with '-' or contain control characters".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

fn validate_optional_effort(
    driver: &AgentDriverSpec,
    raw: Option<&str>,
) -> Result<Option<String>, RemoteAgentError> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if !driver.supports_effort || !driver.efforts.contains(&trimmed) {
        return Err(RemoteAgentError::BadRequest(format!(
            "Invalid effort '{trimmed}' for agent '{}'",
            driver.id
        )));
    }
    Ok(Some(trimmed.to_string()))
}

fn validate_optional_mode(
    driver: &AgentDriverSpec,
    raw: Option<&str>,
) -> Result<Option<String>, RemoteAgentError> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if !driver.supports_mode || !driver.modes.contains(&trimmed) {
        return Err(RemoteAgentError::BadRequest(format!(
            "Invalid mode '{trimmed}' for agent '{}'",
            driver.id
        )));
    }
    Ok(Some(trimmed.to_string()))
}

fn default_working_dir() -> PathBuf {
    let home = crate::config::get_home_dir();
    if home.is_dir() {
        return home;
    }
    std::env::current_dir().unwrap_or(home)
}

fn resolve_working_dir(
    requested: Option<&str>,
    session_project_dir: Option<&str>,
) -> Result<PathBuf, RemoteAgentError> {
    if let Some(raw) = requested.map(str::trim).filter(|s| !s.is_empty()) {
        if raw.chars().any(char::is_control) {
            return Err(RemoteAgentError::BadRequest(
                "cwd must not contain control characters".into(),
            ));
        }
        let path = PathBuf::from(raw);
        if !path.exists() || !path.is_dir() {
            return Err(RemoteAgentError::BadRequest(format!(
                "Working directory does not exist or is not a directory: {raw}"
            )));
        }
        return Ok(path);
    }

    if let Some(proj) = session_project_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(proj);
        if path.is_dir() {
            return Ok(path);
        }
    }

    let fallback = default_working_dir();
    if fallback.is_dir() {
        Ok(fallback)
    } else {
        Err(RemoteAgentError::BadRequest(
            "Unable to resolve a valid working directory".into(),
        ))
    }
}

// ============================================================
// Secret Redaction
// ============================================================

pub fn redact_secrets(input: &str, secrets: &[String]) -> String {
    let mut out = input.to_string();
    for secret in secrets {
        if secret.len() >= 4 && out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "[REDACTED]");
        }
    }
    const PREFIX: &str = "GEMINI_API_KEY=";
    let mut cursor = 0;
    while cursor < out.len() {
        let Some(rel) = out[cursor..].find(PREFIX) else {
            break;
        };
        let start = cursor + rel + PREFIX.len();
        let rest = &out[start..];
        let end_rel = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | ',' | ';' | '}'))
            .unwrap_or(rest.len());
        if end_rel > 0 && &rest[..end_rel] != "[REDACTED]" {
            out.replace_range(start..start + end_rel, "[REDACTED]");
            cursor = start + "[REDACTED]".len();
        } else {
            cursor = start + end_rel;
        }
    }
    out
}

// ============================================================
// Tolerant Unified NDJSON / Plain-Text Stream Parser
// ============================================================

#[derive(Debug, Clone)]
struct ParsedStreamEvent {
    kind: RemoteEventKind,
    text: Option<String>,
    tool_name: Option<String>,
    tool_id: Option<String>,
    is_error: Option<bool>,
    session_id: Option<String>,
    from_result_summary: bool,
}

impl ParsedStreamEvent {
    fn simple(kind: RemoteEventKind, text: Option<String>) -> Self {
        Self {
            kind,
            text,
            tool_name: None,
            tool_id: None,
            is_error: None,
            session_id: None,
            from_result_summary: false,
        }
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn extract_session_id(v: &Value) -> Option<String> {
    for key in [
        "conversation_id",
        "conversationId",
        "session_id",
        "sessionId",
        "sessionID",
        "thread_id",
        "threadId",
    ] {
        if let Some(s) = v.get(key).and_then(Value::as_str).map(str::trim) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    // Pi / OMP `{ "type": "session", "id": "..." }`
    if v.get("type").and_then(Value::as_str) == Some("session") {
        if let Some(s) = v.get("id").and_then(Value::as_str).map(str::trim) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    for nested_key in [
        "init",
        "result",
        "data",
        "metadata",
        "step_update",
        "stepUpdate",
        "part",
        "event",
    ] {
        if let Some(nested) = v.get(nested_key) {
            if nested.is_object() {
                if let Some(found) = extract_session_id(nested) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn value_to_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(_) | Value::Array(_) => serde_json::to_string(v)
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| truncate_chars(&s, MAX_TOOL_TEXT_CHARS)),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn extract_text_field(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(val) = v.get(*key) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            } else if let Some(obj) = val.as_object() {
                for inner in ["text", "content", "message", "delta", "thinking"] {
                    if let Some(s) = obj.get(inner).and_then(Value::as_str) {
                        if !s.is_empty() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_content_blocks(blocks: &[Value], sid: Option<&str>) -> Vec<ParsedStreamEvent> {
    let mut out = Vec::new();
    for block in blocks {
        let btype = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        match btype.as_str() {
            "thinking" | "thought" | "reasoning" => {
                if let Some(text) =
                    extract_text_field(block, &["thinking", "thought", "text", "content"])
                {
                    let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Thought, Some(text));
                    ev.session_id = sid.map(str::to_string);
                    out.push(ev);
                }
            }
            "tool_use" | "tool_call" => {
                let tool_name = extract_text_field(block, &["name", "tool_name", "toolName"]);
                let tool_id = extract_text_field(block, &["id", "tool_id", "call_id"]);
                let input_text = block
                    .get("input")
                    .or_else(|| block.get("arguments"))
                    .or_else(|| block.get("args"))
                    .or_else(|| block.get("parameters"))
                    .and_then(value_to_text);
                out.push(ParsedStreamEvent {
                    kind: RemoteEventKind::ToolCall,
                    text: input_text,
                    tool_name,
                    tool_id,
                    is_error: None,
                    session_id: sid.map(str::to_string),
                    from_result_summary: false,
                });
            }
            "tool_result" => {
                let tool_id =
                    extract_text_field(block, &["tool_use_id", "call_id", "tool_id", "id"]);
                let tool_name = extract_text_field(block, &["name", "tool_name", "toolName"]);
                let is_error = block
                    .get("is_error")
                    .or_else(|| block.get("isError"))
                    .and_then(Value::as_bool);
                let text = block
                    .get("content")
                    .or_else(|| block.get("output"))
                    .or_else(|| block.get("result"))
                    .or_else(|| block.get("text"))
                    .and_then(value_to_text);
                out.push(ParsedStreamEvent {
                    kind: RemoteEventKind::ToolResult,
                    text,
                    tool_name,
                    tool_id,
                    is_error,
                    session_id: sid.map(str::to_string),
                    from_result_summary: false,
                });
            }
            _ => {
                if let Some(text) = extract_text_field(block, &["text", "content", "delta"]) {
                    let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Text, Some(text));
                    ev.session_id = sid.map(str::to_string);
                    out.push(ev);
                }
            }
        }
    }
    out
}

fn parse_step_update_object(step: &Value, sid: Option<&str>) -> Vec<ParsedStreamEvent> {
    let mut out = Vec::new();

    if let Some(thought) = extract_text_field(step, &["thought", "thinking", "reasoning"]) {
        let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Thought, Some(thought));
        ev.session_id = sid.map(str::to_string);
        out.push(ev);
    }

    if let Some(tc) = step
        .get("tool_call")
        .or_else(|| step.get("toolCall"))
        .or_else(|| step.get("tool_use"))
    {
        let tool_name = extract_text_field(tc, &["name", "tool_name", "toolName"]);
        let tool_id = extract_text_field(tc, &["id", "call_id", "tool_id"]);
        let text = tc
            .get("input")
            .or_else(|| tc.get("arguments"))
            .or_else(|| tc.get("args"))
            .or_else(|| tc.get("command"))
            .and_then(value_to_text);
        out.push(ParsedStreamEvent {
            kind: RemoteEventKind::ToolCall,
            text,
            tool_name,
            tool_id,
            is_error: None,
            session_id: sid.map(str::to_string),
            from_result_summary: false,
        });
    }

    if let Some(tr) = step.get("tool_result").or_else(|| step.get("toolResult")) {
        let tool_name = extract_text_field(tr, &["name", "tool_name", "toolName"]);
        let tool_id = extract_text_field(tr, &["tool_use_id", "call_id", "tool_id", "id"]);
        let is_error = tr
            .get("is_error")
            .or_else(|| tr.get("isError"))
            .and_then(Value::as_bool);
        let text = tr
            .get("output")
            .or_else(|| tr.get("content"))
            .or_else(|| tr.get("result"))
            .or_else(|| tr.get("text"))
            .and_then(value_to_text);
        out.push(ParsedStreamEvent {
            kind: RemoteEventKind::ToolResult,
            text,
            tool_name,
            tool_id,
            is_error,
            session_id: sid.map(str::to_string),
            from_result_summary: false,
        });
    }

    if let Some(blocks) = step
        .get("content")
        .and_then(Value::as_array)
        .or_else(|| step.pointer("/message/content").and_then(Value::as_array))
    {
        out.extend(parse_content_blocks(blocks, sid));
    } else if let Some(text) =
        extract_text_field(step, &["text", "delta", "response", "content", "message"])
    {
        let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Text, Some(text));
        ev.session_id = sid.map(str::to_string);
        out.push(ev);
    }

    out
}

fn parse_codex_item_event(
    kind_tag: &str,
    item: &Value,
    sid: Option<&str>,
) -> Vec<ParsedStreamEvent> {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let item_id = extract_text_field(item, &["id", "call_id"]);
    match item_type.as_str() {
        "reasoning" => {
            if let Some(text) = extract_text_field(item, &["text", "summary", "content"]) {
                let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Thought, Some(text));
                ev.session_id = sid.map(str::to_string);
                return vec![ev];
            }
            Vec::new()
        }
        "agent_message" | "assistant_message" => {
            if let Some(text) = extract_text_field(item, &["text", "content"]) {
                let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Text, Some(text));
                ev.session_id = sid.map(str::to_string);
                return vec![ev];
            }
            Vec::new()
        }
        "command_execution" => {
            if kind_tag == "item.started" {
                let cmd = extract_text_field(item, &["command"]);
                return vec![ParsedStreamEvent {
                    kind: RemoteEventKind::ToolCall,
                    text: cmd,
                    tool_name: Some("shell".into()),
                    tool_id: item_id,
                    is_error: None,
                    session_id: sid.map(str::to_string),
                    from_result_summary: false,
                }];
            }
            let exit_code = item.get("exit_code").and_then(Value::as_i64);
            let output = item
                .get("aggregated_output")
                .or_else(|| item.get("output"))
                .and_then(value_to_text);
            vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolResult,
                text: output,
                tool_name: Some("shell".into()),
                tool_id: item_id,
                is_error: exit_code.map(|c| c != 0),
                session_id: sid.map(str::to_string),
                from_result_summary: false,
            }]
        }
        "mcp_tool_call" | "function_call" => {
            let tool_name = extract_text_field(item, &["tool", "name"]);
            if kind_tag == "item.started" {
                let text = item
                    .get("arguments")
                    .or_else(|| item.get("input"))
                    .and_then(value_to_text);
                return vec![ParsedStreamEvent {
                    kind: RemoteEventKind::ToolCall,
                    text,
                    tool_name,
                    tool_id: item_id,
                    is_error: None,
                    session_id: sid.map(str::to_string),
                    from_result_summary: false,
                }];
            }
            let text = item
                .get("result")
                .or_else(|| item.get("output"))
                .or_else(|| item.get("error"))
                .and_then(value_to_text);
            let is_error = item.get("error").is_some_and(|v| !v.is_null());
            vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolResult,
                text,
                tool_name,
                tool_id: item_id,
                is_error: Some(is_error),
                session_id: sid.map(str::to_string),
                from_result_summary: false,
            }]
        }
        "file_change" => {
            let text = item.get("changes").and_then(value_to_text);
            vec![ParsedStreamEvent {
                kind: if kind_tag == "item.started" {
                    RemoteEventKind::ToolCall
                } else {
                    RemoteEventKind::ToolResult
                },
                text,
                tool_name: Some("file_change".into()),
                tool_id: item_id,
                is_error: None,
                session_id: sid.map(str::to_string),
                from_result_summary: false,
            }]
        }
        _ => Vec::new(),
    }
}

fn parse_stream_line(line: &str) -> Vec<ParsedStreamEvent> {
    let Ok(val) = serde_json::from_str::<Value>(line) else {
        return vec![ParsedStreamEvent::simple(
            RemoteEventKind::Text,
            Some(line.to_string()),
        )];
    };

    let Some(obj) = val.as_object() else {
        return vec![ParsedStreamEvent::simple(
            RemoteEventKind::Text,
            Some(line.to_string()),
        )];
    };

    let sid = extract_session_id(&val);
    let kind_tag = val
        .get("type")
        .or_else(|| val.get("event"))
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());

    // 1. Init / Session started events across AGY, Claude (`system` subtype `init`),
    //    Codex (`thread.started`), Gemini (`init`), Pi/OMP (`session`).
    let is_claude_init = kind_tag.as_deref() == Some("system")
        && val.get("subtype").and_then(Value::as_str) == Some("init");
    if is_claude_init
        || matches!(
            kind_tag.as_deref(),
            Some("init" | "thread.started" | "session" | "step_start")
        )
        || obj.contains_key("init")
    {
        let model = val
            .get("model")
            .or_else(|| val.pointer("/init/model"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Init, model);
        ev.session_id = sid;
        return vec![ev];
    }

    // 2. AGY `step_update` keyed shape.
    if matches!(kind_tag.as_deref(), Some("step_update" | "stepupdate"))
        || obj.contains_key("step_update")
        || obj.contains_key("stepUpdate")
    {
        let inner = val
            .get("step_update")
            .or_else(|| val.get("stepUpdate"))
            .unwrap_or(&val);
        return parse_step_update_object(inner, sid.as_deref());
    }

    // 3. Codex `item.*` & `turn.*` events.
    if let Some(tag) = kind_tag.as_deref() {
        if tag.starts_with("item.") {
            if let Some(item) = val.get("item") {
                return parse_codex_item_event(tag, item, sid.as_deref());
            }
        }
        if tag == "turn.failed" {
            let msg = extract_text_field(&val, &["error", "message"])
                .or_else(|| Some("Codex turn failed".into()));
            let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Error, msg);
            ev.is_error = Some(true);
            ev.session_id = sid;
            return vec![ev];
        }
        if tag == "turn.completed" || tag == "turn.started" || tag == "step_finish" {
            if sid.is_some() {
                let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Init, None);
                ev.session_id = sid;
                return vec![ev];
            }
            return Vec::new();
        }
    }

    // 4. Pi / OMP `message_update`, `tool_execution_start`, `tool_execution_end`.
    match kind_tag.as_deref() {
        Some("message_update") => {
            if let Some(ame) = val.get("assistantMessageEvent") {
                let sub = ame.get("type").and_then(Value::as_str).unwrap_or_default();
                if sub == "thinking_delta" {
                    if let Some(delta) = ame.get("delta").and_then(Value::as_str) {
                        let mut ev = ParsedStreamEvent::simple(
                            RemoteEventKind::Thought,
                            Some(delta.to_string()),
                        );
                        ev.session_id = sid;
                        return vec![ev];
                    }
                } else if sub == "text_delta" {
                    if let Some(delta) = ame.get("delta").and_then(Value::as_str) {
                        let mut ev = ParsedStreamEvent::simple(
                            RemoteEventKind::Text,
                            Some(delta.to_string()),
                        );
                        ev.session_id = sid;
                        return vec![ev];
                    }
                }
            }
            return Vec::new();
        }
        Some("tool_execution_start") => {
            let tool_name = extract_text_field(&val, &["toolName", "tool_name", "name"]);
            let tool_id = extract_text_field(&val, &["toolCallId", "tool_call_id", "id"]);
            let text = val
                .get("args")
                .or_else(|| val.get("input"))
                .and_then(value_to_text);
            return vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolCall,
                text,
                tool_name,
                tool_id,
                is_error: None,
                session_id: sid,
                from_result_summary: false,
            }];
        }
        Some("tool_execution_end") => {
            let tool_name = extract_text_field(&val, &["toolName", "tool_name", "name"]);
            let tool_id = extract_text_field(&val, &["toolCallId", "tool_call_id", "id"]);
            let is_error = val
                .get("isError")
                .or_else(|| val.get("is_error"))
                .and_then(Value::as_bool);
            let text = val
                .get("result")
                .or_else(|| val.get("output"))
                .and_then(value_to_text);
            return vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolResult,
                text,
                tool_name,
                tool_id,
                is_error,
                session_id: sid,
                from_result_summary: false,
            }];
        }
        _ => {}
    }

    // 5. Claude `stream_event` wrapper (`{ "type": "stream_event", "event": { ... } }`).
    if kind_tag.as_deref() == Some("stream_event") {
        if let Some(inner) = val.get("event") {
            if let Ok(inner_str) = serde_json::to_string(inner) {
                let mut events = parse_stream_line(&inner_str);
                for ev in &mut events {
                    if ev.session_id.is_none() {
                        ev.session_id = sid.clone();
                    }
                }
                return events;
            }
        }
        return Vec::new();
    }

    // 6. OpenCode `part`-wrapped JSON events (`text`, `reasoning`, `tool_use`).
    if let Some(part) = val.get("part").and_then(Value::as_object) {
        if kind_tag.as_deref() == Some("tool_use") {
            let tool_name = part.get("tool").and_then(Value::as_str).map(str::to_string);
            let tool_id = part
                .get("callID")
                .or_else(|| part.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut out = Vec::new();
            if let Some(state) = part.get("state") {
                let input_text = state.get("input").and_then(value_to_text);
                out.push(ParsedStreamEvent {
                    kind: RemoteEventKind::ToolCall,
                    text: input_text,
                    tool_name: tool_name.clone(),
                    tool_id: tool_id.clone(),
                    is_error: None,
                    session_id: sid.clone(),
                    from_result_summary: false,
                });
                if let Some(output_text) = state
                    .get("output")
                    .or_else(|| state.get("error"))
                    .and_then(value_to_text)
                {
                    let is_err = state.get("status").and_then(Value::as_str) == Some("error")
                        || state.get("error").is_some();
                    out.push(ParsedStreamEvent {
                        kind: RemoteEventKind::ToolResult,
                        text: Some(output_text),
                        tool_name,
                        tool_id,
                        is_error: Some(is_err),
                        session_id: sid,
                        from_result_summary: false,
                    });
                }
            }
            return out;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                let kind = if matches!(
                    kind_tag.as_deref(),
                    Some("reasoning" | "thought" | "thinking")
                ) {
                    RemoteEventKind::Thought
                } else {
                    RemoteEventKind::Text
                };
                let mut ev = ParsedStreamEvent::simple(kind, Some(text.to_string()));
                ev.session_id = sid;
                return vec![ev];
            }
        }
    }

    // 7. Result events (`type == "result"` or `{ "result": ... }`).
    if kind_tag.as_deref() == Some("result") || obj.contains_key("result") {
        let inner = val.get("result").filter(|v| v.is_object()).unwrap_or(&val);
        let is_error = inner
            .get("is_error")
            .or_else(|| inner.get("isError"))
            .or_else(|| val.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || val.get("status").and_then(Value::as_str) == Some("error");
        let error_text =
            extract_text_field(inner, &["error"]).or_else(|| extract_text_field(&val, &["error"]));
        if is_error || error_text.is_some() {
            let mut ev = ParsedStreamEvent::simple(
                RemoteEventKind::Error,
                error_text
                    .or_else(|| extract_text_field(inner, &["message", "text", "result"]))
                    .or_else(|| Some("Agent run reported an error".into())),
            );
            ev.is_error = Some(true);
            ev.session_id = sid;
            return vec![ev];
        }

        let summary_text = val
            .get("result")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| extract_text_field(inner, &["text", "response", "output", "message"]));
        if let Some(text) = summary_text {
            let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Text, Some(text));
            ev.session_id = sid;
            ev.from_result_summary = true;
            return vec![ev];
        }
        if sid.is_some() {
            let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Init, None);
            ev.session_id = sid;
            return vec![ev];
        }
        return Vec::new();
    }

    // 8. Standard `type`-keyed streaming shapes across Claude/Gemini/Kimi/AGY.
    match kind_tag.as_deref() {
        Some("thought" | "thinking" | "reasoning") => {
            let text =
                extract_text_field(&val, &["thought", "thinking", "text", "delta", "content"]);
            let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Thought, text);
            ev.session_id = sid;
            vec![ev]
        }
        Some("tool_use" | "tool_call") => {
            let tool_name = extract_text_field(&val, &["name", "tool_name", "toolName"]);
            let tool_id = extract_text_field(&val, &["id", "tool_id", "call_id"]);
            let text = val
                .get("input")
                .or_else(|| val.get("parameters"))
                .or_else(|| val.get("arguments"))
                .or_else(|| val.get("args"))
                .or_else(|| val.get("text"))
                .and_then(value_to_text);
            vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolCall,
                text,
                tool_name,
                tool_id,
                is_error: None,
                session_id: sid,
                from_result_summary: false,
            }]
        }
        Some("tool_result") => {
            let tool_name = extract_text_field(&val, &["name", "tool_name", "toolName"]);
            let tool_id = extract_text_field(&val, &["tool_use_id", "call_id", "tool_id", "id"]);
            let is_error = val
                .get("is_error")
                .or_else(|| val.get("isError"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    val.get("status")
                        .and_then(Value::as_str)
                        .map(|s| s == "error")
                });
            let text = val
                .get("content")
                .or_else(|| val.get("output"))
                .or_else(|| val.get("result"))
                .or_else(|| val.get("text"))
                .and_then(value_to_text);
            vec![ParsedStreamEvent {
                kind: RemoteEventKind::ToolResult,
                text,
                tool_name,
                tool_id,
                is_error,
                session_id: sid,
                from_result_summary: false,
            }]
        }
        Some("error") => {
            let text = extract_text_field(&val, &["error", "message", "text"])
                .or_else(|| Some("Agent error".into()));
            let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Error, text);
            ev.is_error = Some(true);
            ev.session_id = sid;
            vec![ev]
        }
        Some("user") => {
            // Claude user frames may carry `tool_result` blocks; ignore plain user prompt echoes.
            if let Some(blocks) = val.pointer("/message/content").and_then(Value::as_array) {
                return parse_content_blocks(blocks, sid.as_deref())
                    .into_iter()
                    .filter(|ev| ev.kind == RemoteEventKind::ToolResult)
                    .collect();
            }
            Vec::new()
        }
        Some("assistant" | "delta" | "text" | "message" | "content_block_delta") => {
            if val.get("role").and_then(Value::as_str) == Some("user") {
                return Vec::new();
            }
            if let Some(blocks) = val
                .pointer("/message/content")
                .and_then(Value::as_array)
                .or_else(|| val.get("content").and_then(Value::as_array))
            {
                return parse_content_blocks(blocks, sid.as_deref());
            }
            if let Some(delta_obj) = val.get("delta").and_then(Value::as_object) {
                let dtype = delta_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if dtype.contains("thinking") {
                    if let Some(t) = delta_obj
                        .get("thinking")
                        .or_else(|| delta_obj.get("text"))
                        .and_then(Value::as_str)
                    {
                        let mut ev = ParsedStreamEvent::simple(
                            RemoteEventKind::Thought,
                            Some(t.to_string()),
                        );
                        ev.session_id = sid;
                        return vec![ev];
                    }
                }
            }
            if let Some(text) = extract_text_field(&val, &["delta", "text", "content", "message"]) {
                let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Text, Some(text));
                ev.session_id = sid;
                return vec![ev];
            }
            Vec::new()
        }
        _ => {
            let step_parsed = parse_step_update_object(&val, sid.as_deref());
            if !step_parsed.is_empty() {
                return step_parsed;
            }
            if sid.is_some() {
                let mut ev = ParsedStreamEvent::simple(RemoteEventKind::Init, None);
                ev.session_id = sid;
                return vec![ev];
            }
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_forbidden_flags(args: &[String]) {
        for arg in args {
            assert_ne!(
                arg, "--dangerously-skip-permissions",
                "driver must never emit --dangerously-skip-permissions"
            );
            assert_ne!(arg, "sh");
            assert_ne!(arg, "cmd");
            assert_ne!(arg, "-c");
            assert_ne!(arg, "/c");
        }
    }

    #[test]
    fn sanitize_remote_control_apps_filters_unknown_and_orders_canonically() {
        let input = vec![
            "omp".into(),
            "unknown".into(),
            "agy".into(),
            "claude".into(),
            "agy".into(),
        ];
        assert_eq!(
            sanitize_remote_control_apps(&input),
            vec!["claude".to_string(), "agy".to_string(), "omp".to_string()]
        );
    }

    #[test]
    fn build_driver_args_matches_verified_flags_for_all_10_agents() {
        let resume = ResumeTarget {
            session_id: "sess-123".into(),
            source_path: Some("/tmp/transcript.jsonl".into()),
            agy_gemini_dir: Some(PathBuf::from("/home/user/.cc-switch/agy/key-1")),
        };

        // 1. claude
        let claude = find_driver("claude").unwrap();
        let args = build_driver_args(
            claude,
            &DriverRunParams {
                prompt: "fix bug",
                resume: Some(&resume),
                model: Some("claude-sonnet-4-5"),
                effort: Some("high"),
                mode: Some("acceptEdits"),
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&args);
        assert_eq!(
            args,
            vec![
                "--print",
                "--output-format",
                "stream-json",
                "--verbose",
                "--resume",
                "sess-123",
                "--model",
                "claude-sonnet-4-5",
                "--effort",
                "high",
                "--permission-mode",
                "acceptEdits",
                "--",
                "fix bug",
            ]
        );

        // 2. codex (new vs resume)
        let codex = find_driver("codex").unwrap();
        let codex_new = build_driver_args(
            codex,
            &DriverRunParams {
                prompt: "add tests",
                resume: None,
                model: Some("gpt-5.3-codex"),
                effort: None,
                mode: Some("workspace-write"),
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&codex_new);
        assert_eq!(
            codex_new,
            vec![
                "exec",
                "--json",
                "--model",
                "gpt-5.3-codex",
                "--sandbox",
                "workspace-write",
                "--",
                "add tests",
            ]
        );
        let codex_resume = build_driver_args(
            codex,
            &DriverRunParams {
                prompt: "continue",
                resume: Some(&resume),
                model: Some("gpt-5.3-codex"),
                effort: None,
                mode: Some("read-only"),
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&codex_resume);
        assert_eq!(
            codex_resume,
            vec![
                "exec",
                "resume",
                "--json",
                "--model",
                "gpt-5.3-codex",
                "--",
                "sess-123",
                "continue",
            ]
        );

        // 3. gemini
        let gemini = find_driver("gemini").unwrap();
        let gemini_args = build_driver_args(
            gemini,
            &DriverRunParams {
                prompt: "explain",
                resume: Some(&resume),
                model: Some("gemini-2.5-pro"),
                effort: None,
                mode: None,
                sandbox: true,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&gemini_args);
        assert_eq!(
            gemini_args,
            vec![
                "--prompt=explain",
                "--output-format",
                "stream-json",
                "--resume",
                "sess-123",
                "--model",
                "gemini-2.5-pro",
                "--sandbox",
            ]
        );

        // 4. grokbuild
        let grok = find_driver("grokbuild").unwrap();
        let grok_args = build_driver_args(
            grok,
            &DriverRunParams {
                prompt: "hello",
                resume: Some(&resume),
                model: Some("grok-4"),
                effort: None,
                mode: None,
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&grok_args);
        assert_eq!(
            grok_args,
            vec![
                "--prompt=hello",
                "--output-format",
                "json",
                "--resume",
                "sess-123",
                "--model",
                "grok-4",
            ]
        );

        // 5. opencode
        let opencode = find_driver("opencode").unwrap();
        let opencode_args = build_driver_args(
            opencode,
            &DriverRunParams {
                prompt: "refactor",
                resume: Some(&resume),
                model: Some("anthropic/claude-sonnet-4-5"),
                effort: None,
                mode: Some("thinking"),
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&opencode_args);
        assert_eq!(
            opencode_args,
            vec![
                "run",
                "--format",
                "json",
                "--session",
                "sess-123",
                "--model",
                "anthropic/claude-sonnet-4-5",
                "--thinking",
                "--",
                "refactor",
            ]
        );

        // 6. agy
        let agy = find_driver("agy").unwrap();
        let agy_args = build_driver_args(
            agy,
            &DriverRunParams {
                prompt: "ship it",
                resume: Some(&resume),
                model: Some("Gemini 2.5 Pro"),
                effort: Some("xhigh"),
                mode: Some("plan"),
                sandbox: true,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&agy_args);
        assert_eq!(
            agy_args,
            vec![
                "--gemini_dir=/home/user/.cc-switch/agy/key-1",
                "--print=ship it",
                "--output-format",
                "stream-json",
                "--conversation",
                "sess-123",
                "--model",
                "Gemini 2.5 Pro",
                "--effort",
                "xhigh",
                "--mode",
                "plan",
                "--sandbox",
            ]
        );

        // 7. copilot
        let copilot = find_driver("copilot").unwrap();
        let copilot_args = build_driver_args(
            copilot,
            &DriverRunParams {
                prompt: "status",
                resume: Some(&resume),
                model: Some("gpt-5"),
                effort: None,
                mode: None,
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&copilot_args);
        assert_eq!(
            copilot_args,
            vec!["--prompt=status", "--resume=sess-123", "--model", "gpt-5"]
        );

        // 8. kimi
        let kimi = find_driver("kimi").unwrap();
        let kimi_args = build_driver_args(
            kimi,
            &DriverRunParams {
                prompt: "summarize",
                resume: Some(&resume),
                model: Some("kimi-k2"),
                effort: None,
                mode: None,
                sandbox: false,
            },
        )
        .unwrap();
        assert_no_forbidden_flags(&kimi_args);
        assert_eq!(
            kimi_args,
            vec![
                "--print",
                "--output-format",
                "stream-json",
                "--session",
                "sess-123",
                "--model",
                "kimi-k2",
                "--prompt=summarize",
            ]
        );

        // 9. pi & 10. omp
        for id in ["pi", "omp"] {
            let drv = find_driver(id).unwrap();
            let drv_args = build_driver_args(
                drv,
                &DriverRunParams {
                    prompt: "build",
                    resume: Some(&resume),
                    model: Some("sonnet"),
                    effort: None,
                    mode: None,
                    sandbox: false,
                },
            )
            .unwrap();
            assert_no_forbidden_flags(&drv_args);
            assert_eq!(
                drv_args,
                vec![
                    "--print",
                    "--mode",
                    "json",
                    "--session",
                    "/tmp/transcript.jsonl",
                    "--model",
                    "sonnet",
                    "--",
                    "build",
                ]
            );
        }
    }

    #[test]
    fn leading_dash_prompts_cannot_inject_cli_flags_in_any_driver() {
        let resume = ResumeTarget {
            session_id: "sess-1".into(),
            source_path: Some("/tmp/s.jsonl".into()),
            agy_gemini_dir: None,
        };
        for malicious_prompt in ["--dangerously-skip-permissions", "- fix bug", "--yolo"] {
            for driver in AGENT_DRIVERS {
                for resume_opt in [None, Some(&resume)] {
                    let args = build_driver_args(
                        driver,
                        &DriverRunParams {
                            prompt: malicious_prompt,
                            resume: resume_opt,
                            model: None,
                            effort: None,
                            mode: None,
                            sandbox: false,
                        },
                    )
                    .unwrap();

                    // Split argv into options-before-`--` and positionals-after-`--`.
                    let dd_pos = args.iter().position(|a| a == "--");
                    let option_slice = match dd_pos {
                        Some(idx) => &args[..idx],
                        None => &args[..],
                    };
                    assert!(
                        !option_slice.iter().any(|a| a == malicious_prompt),
                        "driver '{}' emitted '{}' as a standalone option before `--`: {:?}",
                        driver.id,
                        malicious_prompt,
                        args
                    );
                    if let Some(idx) = dd_pos {
                        assert!(
                            args[idx + 1..].iter().any(|a| a == malicious_prompt),
                            "driver '{}' lost positional prompt after `--`: {:?}",
                            driver.id,
                            args
                        );
                    } else {
                        assert!(
                            args.iter()
                                .any(|a| a == &format!("--print={malicious_prompt}")
                                    || a == &format!("--prompt={malicious_prompt}")),
                            "driver '{}' did not bind prompt via `=`: {:?}",
                            driver.id,
                            args
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn stream_parser_normalizes_multi_agent_ndjson_and_plain_lines() {
        // Claude assistant block with thinking + tool_use + text
        let claude_events = parse_stream_line(
            r#"{"type":"assistant","session_id":"cl-1","message":{"content":[{"type":"thinking","thinking":"plan"},{"type":"tool_use","id":"t1","name":"Bash","input":{"cmd":"ls"}},{"type":"text","text":"All done"}]}}"#,
        );
        assert_eq!(claude_events.len(), 3);
        assert_eq!(claude_events[0].kind, RemoteEventKind::Thought);
        assert_eq!(claude_events[1].kind, RemoteEventKind::ToolCall);
        assert_eq!(claude_events[1].tool_name.as_deref(), Some("Bash"));
        assert_eq!(claude_events[2].kind, RemoteEventKind::Text);
        assert_eq!(claude_events[2].session_id.as_deref(), Some("cl-1"));

        // Codex thread.started + item.started/completed
        let codex_init = parse_stream_line(r#"{"type":"thread.started","thread_id":"th-99"}"#);
        assert_eq!(codex_init[0].kind, RemoteEventKind::Init);
        assert_eq!(codex_init[0].session_id.as_deref(), Some("th-99"));

        let codex_cmd = parse_stream_line(
            r#"{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"cargo test"}}"#,
        );
        assert_eq!(codex_cmd[0].kind, RemoteEventKind::ToolCall);
        assert_eq!(codex_cmd[0].tool_name.as_deref(), Some("shell"));

        let codex_msg = parse_stream_line(
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"Tests pass"}}"#,
        );
        assert_eq!(codex_msg[0].kind, RemoteEventKind::Text);
        assert_eq!(codex_msg[0].text.as_deref(), Some("Tests pass"));

        // OpenCode part-wrapped JSON
        let oc = parse_stream_line(
            r#"{"type":"text","sessionID":"ses_abc","part":{"type":"text","text":"Hello from OpenCode"}}"#,
        );
        assert_eq!(oc[0].kind, RemoteEventKind::Text);
        assert_eq!(oc[0].session_id.as_deref(), Some("ses_abc"));

        // Pi/OMP session + message_update
        let pi_sess = parse_stream_line(r#"{"type":"session","id":"pi-sess-7"}"#);
        assert_eq!(pi_sess[0].kind, RemoteEventKind::Init);
        assert_eq!(pi_sess[0].session_id.as_deref(), Some("pi-sess-7"));

        let pi_delta = parse_stream_line(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"chunk"}}"#,
        );
        assert_eq!(pi_delta[0].kind, RemoteEventKind::Text);
        assert_eq!(pi_delta[0].text.as_deref(), Some("chunk"));

        // Plain-text fallback (e.g. Copilot)
        let plain = parse_stream_line("Plain text response line");
        assert_eq!(plain[0].kind, RemoteEventKind::Text);
        assert_eq!(plain[0].text.as_deref(), Some("Plain text response line"));
    }

    #[test]
    fn redact_secrets_masks_explicit_keys_and_all_inline_env_occurrences() {
        let secrets = vec!["my-super-secret-key".to_string()];
        let cleaned = redact_secrets(
            "Error using my-super-secret-key, GEMINI_API_KEY=first-secret and GEMINI_API_KEY=second-secret here",
            &secrets,
        );
        assert!(!cleaned.contains("my-super-secret-key"));
        assert!(!cleaned.contains("first-secret"));
        assert!(!cleaned.contains("second-secret"));
        assert_eq!(
            cleaned,
            "Error using [REDACTED], GEMINI_API_KEY=[REDACTED] and GEMINI_API_KEY=[REDACTED] here"
        );
    }

    #[tokio::test]
    async fn read_bounded_line_survives_invalid_utf8_and_caps_huge_lines() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"line1-\xff-\xfe-ok\n");
        raw.extend(std::iter::repeat_n(b'a', MAX_STREAM_LINE_BYTES + 1024));
        raw.push(b'\n');
        raw.extend_from_slice(b"after-huge\n");

        let mut reader = BufReader::new(std::io::Cursor::new(raw));
        let mut buf = Vec::new();

        let l1 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert!(l1.starts_with("line1-"));
        assert!(l1.ends_with("-ok"));

        let l2 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(l2.len(), MAX_STREAM_LINE_BYTES);

        let l3 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(l3, "after-huge");

        assert!(read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .is_none());
    }
}
