use crate::agent_gateway::launcher_security::build_agent_window_title;
use crate::agent_gateway::listener::{start_agent_listener, stop_agent_listener};
use crate::agent_gateway::models::{
    AgentCommandError, AgentInstance, AgentLaunchMode, AgentLog, AgentRuntimeKind, AgentStatus,
    LaunchAgentRequest, LaunchStrategy, ProviderRuntimeSnapshot, ProviderSnapshotRequest,
    RestartAgentRequest, RunProfile, RunProfileKind,
};
use crate::agent_gateway::port_registry::PortRegistry;
use crate::agent_gateway::process_tracker::{find_processes_by_agent_title, taskkill_pid_tree};
use crate::agent_gateway::runtime_snapshot::{
    build_provider_runtime_snapshot, resolve_provider_for_snapshot, validate_snapshot_launchable,
};
use crate::agent_gateway::service::{AgentGatewayService, CleanupReport};
use crate::agent_gateway::wt_launcher::{
    launch_with_strategy, prepare_launch, strategy_available, PreparedLaunch,
};
use crate::store::AppState;
use chrono::Utc;
use tauri::State;
use uuid::Uuid;

type CommandResult<T> = Result<T, AgentCommandError>;

#[tauri::command]
pub async fn agent_gateway_launch_agent(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: LaunchAgentRequest,
) -> CommandResult<AgentInstance> {
    state
        .db
        .verify_agent_gateway_schema_ready()
        .map_err(db_error)?;

    if req.runtime != AgentRuntimeKind::ClaudeCode {
        return Err(AgentCommandError::new(
            "RUNTIME_UNSUPPORTED",
            "Only ClaudeCode runtime can launch in the Agent Gateway MVP.",
            "Use ClaudeCode for now; other runtimes are reserved for a later release.",
        ));
    }

    let requested_session_id = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let run_profile_id = req.run_profile_id.clone().unwrap_or_else(|| {
        if requested_session_id.is_some() {
            "resume".to_string()
        } else {
            "safe".to_string()
        }
    });
    if run_profile_id == "custom" {
        return Err(AgentCommandError::new(
            "CUSTOM_PROFILE_DISABLED",
            "Custom RunProfile execution is disabled by default.",
            "Enable allow_custom_profiles in a future advanced setting and pass strict validation.",
        ));
    }

    let provider = state
        .db
        .verify_agent_gateway_schema_ready()
        .map_err(db_error)
        .and_then(|_| {
            resolve_provider_for_snapshot(
                &state.db,
                &ProviderSnapshotRequest {
                    provider_id: Some(req.provider_id.clone()),
                    provider_mode: req.provider_mode.clone(),
                },
            )
        })?;
    let snapshot = build_provider_runtime_snapshot(&provider);
    validate_snapshot_launchable(&snapshot)?;

    let registry = PortRegistry::new(&state.db);
    let port = registry.allocate_port().map_err(|e| {
        AgentCommandError::new(
            "PORT_POOL_EXHAUSTED",
            "No available Agent Gateway port was found in 15722-15799.",
            "Close stale agents or free one of the reserved Agent Gateway ports.",
        )
        .with_details(e.to_string())
    })?;

    let agent_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let launch_mode = if requested_session_id.is_some() {
        AgentLaunchMode::Resume
    } else {
        AgentLaunchMode::New
    };
    let title = build_agent_window_title(
        &agent_id,
        Some(&provider.name),
        req.cwd.as_ref().map(|s| std::path::Path::new(s)),
    )
    .map_err(|e| {
        AgentCommandError::new(
            "POWERSHELL_ARG_REJECTED",
            "The generated Agent window title failed launcher validation.",
            "Retry the launch. If the issue persists, export diagnostics.",
        )
        .with_details(e.to_string())
    })?;
    let mut agent = AgentInstance {
        id: agent_id.clone(),
        name: req.name.clone(),
        runtime: AgentRuntimeKind::ClaudeCode,
        provider_id: snapshot.provider_id.clone(),
        provider_name: Some(provider.name.clone()),
        model: sanitize_display_model(
            req.upstream_provider_model
                .as_deref()
                .or(snapshot.default_upstream_model.as_deref())
                .or(req.model.as_deref()),
        ),
        launch_mode,
        run_profile_id,
        port,
        cwd: req.cwd.clone(),
        pid: None,
        window_title: Some(title),
        session_id: requested_session_id.clone(),
        status: AgentStatus::Launching,
        created_at: now.clone(),
        started_at: Some(now),
        stopped_at: None,
        last_error: None,
        deleted_at: None,
    };

    state.db.save_agent_instance(&agent).map_err(db_error)?;
    state
        .db
        .save_agent_provider_snapshot(&agent_id, &snapshot)
        .map_err(db_error)?;
    registry
        .bind_provider(
            port,
            &agent_id,
            &snapshot.provider_id,
            AgentRuntimeKind::ClaudeCode,
        )
        .map_err(db_error)?;

    if let Err(error) = start_agent_listener(
        state.db.clone(),
        &agent_id,
        provider.clone(),
        port,
        Some(app),
    )
    .await
    {
        let _ = registry.release_port(port);
        agent.status = AgentStatus::Failed;
        agent.last_error = Some(error.to_string());
        state.db.save_agent_instance(&agent).map_err(db_error)?;
        return Err(AgentCommandError::new(
            "AGENT_LISTENER_FAILED",
            "Agent Gateway could not start the local Anthropic listener.",
            "Free the assigned port and retry. Existing CC Switch pages remain usable.",
        )
        .with_details(error.to_string()));
    }

    append_log(
        &state,
        &agent_id,
        "info",
        "launch_requested",
        Some("Agent launch requested in native compatibility mode"),
        None,
    )?;

    let strategies = requested_strategies(req.launch_strategy.clone());
    let mut last_error = None;
    for strategy in strategies {
        if !strategy_available(&strategy) {
            last_error = Some(format!("{strategy:?} is unavailable"));
            continue;
        }
        let prepared = prepare_launch(
            &req,
            &agent_id,
            port,
            strategy.clone(),
            &provider.name,
            snapshot
                .default_upstream_model
                .as_deref()
                .unwrap_or("unknown"),
            Some(&provider.settings_config),
        )
        .map_err(|e| {
            AgentCommandError::new(
                "POWERSHELL_ARG_REJECTED",
                "The Agent launch request failed launcher validation.",
                "Remove shell control characters from cwd, args, env, title, or session id.",
            )
            .with_details(e.to_string())
        })?;
        match launch_prepared(&prepared) {
            Ok(pid) => {
                agent.pid = Some(pid);
                agent.status = AgentStatus::Running;
                state.db.save_agent_instance(&agent).map_err(db_error)?;
                append_log(
                    &state,
                    &agent_id,
                    "info",
                    "launch_started",
                    Some(&format!("Started with strategy {:?}", prepared.strategy)),
                    Some(&serde_json::to_string(&prepared.preview()).unwrap_or_default()),
                )?;
                // Spawn background task to auto-capture the Claude Code session ID
                let capture_db = state.db.clone();
                let capture_agent_id = agent_id.clone();
                let capture_cwd = agent.cwd.clone();
                tokio::spawn(async move {
                    // Retry loop: Claude Code takes variable time to write its
                    // session JSONL file depending on project size and disk speed.
                    let delays = [5, 10, 20, 30];
                    for &secs in &delays {
                        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                        if let Some(sid) = capture_session_id_from_jsonl(capture_cwd.as_deref()) {
                            if let Ok(Some(mut agent)) =
                                capture_db.get_agent_instance(&capture_agent_id)
                            {
                                agent.session_id = Some(sid.clone());
                                let _ = capture_db.save_agent_instance(&agent);
                                log::info!("[Agent] Auto-captured session_id: {sid}");
                            }
                            return;
                        }
                    }
                    log::warn!("[Agent] Failed to capture session_id after retries");
                });
                return Ok(agent);
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
    }

    let details = last_error.unwrap_or_else(|| "No launch strategy succeeded".to_string());
    agent.status = AgentStatus::Failed;
    agent.last_error = Some(details.clone());
    state.db.save_agent_instance(&agent).map_err(db_error)?;
    let _ = stop_agent_listener(&agent_id).await;
    let _ = registry.release_port(port);
    Err(AgentCommandError::new(
        "LAUNCH_STRATEGY_FAILED",
        "All Agent launch strategies failed.",
        "Install Windows Terminal or PowerShell, confirm Claude Code is on PATH, then retry.",
    )
    .with_details(details))
}

#[tauri::command]
pub async fn agent_gateway_stop_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> CommandResult<()> {
    stop_or_kill(state, agent_id, false).await
}

#[tauri::command]
pub async fn agent_gateway_kill_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> CommandResult<()> {
    stop_or_kill(state, agent_id, true).await
}

#[tauri::command]
pub async fn agent_gateway_delete_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> CommandResult<()> {
    state
        .db
        .verify_agent_gateway_schema_ready()
        .map_err(db_error)?;

    let agent = state
        .db
        .get_agent_instance(&agent_id)
        .map_err(db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "AGENT_NOT_FOUND",
                "The requested agent does not exist.",
                "Refresh the Agent Gateway list and try again.",
            )
        })?;

    if !is_deletable_agent_status(agent.status) {
        return Err(AgentCommandError::new(
            "AGENT_DELETE_REJECTED",
            "Running or launching agents cannot be deleted.",
            "Stop or kill the agent first. Delete only hides the Agent Gateway record and never removes Claude sessions.",
        )
        .with_details(format!("agent_id={agent_id}; status={}", agent.status)));
    }

    state
        .db
        .soft_delete_agent_instance(&agent_id)
        .map_err(db_error)?;
    append_log(
        &state,
        &agent_id,
        "info",
        "agent_deleted",
        Some("Agent Gateway record soft deleted; Claude sessions and user config were not touched"),
        None,
    )?;
    Ok(())
}

fn is_deletable_agent_status(status: AgentStatus) -> bool {
    status.is_terminal()
}

fn resolve_restart_session_id(agent: &AgentInstance) -> Option<String> {
    agent
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| capture_session_id_from_jsonl(agent.cwd.as_deref()))
}

#[tauri::command]
pub async fn agent_gateway_restart_agent(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    agent_id: String,
    req: RestartAgentRequest,
) -> CommandResult<AgentInstance> {
    state
        .db
        .verify_agent_gateway_schema_ready()
        .map_err(db_error)?;

    let mut agent = state
        .db
        .get_agent_instance(&agent_id)
        .map_err(db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "AGENT_NOT_FOUND",
                "The requested agent does not exist.",
                "Refresh the Agent Gateway list and try again.",
            )
        })?;

    let provider = state
        .db
        .get_provider_by_id(&agent.provider_id, "claude")
        .map_err(db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "PROVIDER_NOT_FOUND",
                "The selected Claude provider does not exist.",
                "Choose an existing Claude provider before restarting the agent.",
            )
        })?;
    let snapshot = build_provider_runtime_snapshot(&provider);
    validate_snapshot_launchable(&snapshot)?;

    if let Some(pid) = agent.pid {
        let _ = taskkill_pid_tree(pid, true);
    }
    for pid in find_processes_by_agent_title(&agent_id).unwrap_or_default() {
        let _ = taskkill_pid_tree(pid, true);
    }
    let _ = stop_agent_listener(&agent_id).await;
    let registry = PortRegistry::new(&state.db);
    let _ = registry.release_port(agent.port);

    let port = registry.allocate_port().map_err(|e| {
        AgentCommandError::new(
            "PORT_POOL_EXHAUSTED",
            "No available Agent Gateway port was found in 15722-15799.",
            "Close stale agents or free one of the reserved Agent Gateway ports.",
        )
        .with_details(e.to_string())
    })?;

    let restart_session_id = resolve_restart_session_id(&agent).ok_or_else(|| {
        AgentCommandError::new(
            "SESSION_ID_NOT_FOUND",
            "This agent cannot be resumed because its Claude Code session id was not found.",
            "Open the session picker or run the agent until Claude Code writes a session JSONL, then try resume again.",
        )
        .with_details(format!(
            "agent_id={}; cwd={}",
            agent.id,
            agent.cwd.as_deref().unwrap_or("<none>")
        ))
    })?;
    let restart_run_profile_id = "resume".to_string();

    let launch_req = LaunchAgentRequest {
        name: agent.name.clone(),
        runtime: AgentRuntimeKind::ClaudeCode,
        provider_id: agent.provider_id.clone(),
        model: agent.model.clone(),
        provider_mode: None,
        claude_entry_model: None,
        upstream_provider_model: agent.model.clone(),
        run_profile_id: Some(restart_run_profile_id.clone()),
        cwd: agent.cwd.clone(),
        session_id: Some(restart_session_id.clone()),
        launch_strategy: req.launch_strategy.clone(),
        permission_mode: req.permission_mode.clone(),
    };

    let now = Utc::now().to_rfc3339();
    agent.port = port;
    agent.pid = None;
    agent.launch_mode = AgentLaunchMode::Resume;
    agent.run_profile_id = restart_run_profile_id;
    agent.session_id = Some(restart_session_id);
    agent.status = AgentStatus::Launching;
    agent.started_at = Some(now);
    agent.stopped_at = None;
    agent.last_error = None;
    state.db.save_agent_instance(&agent).map_err(db_error)?;
    registry
        .bind_provider(
            port,
            &agent_id,
            &agent.provider_id,
            AgentRuntimeKind::ClaudeCode,
        )
        .map_err(db_error)?;

    if let Err(error) = start_agent_listener(
        state.db.clone(),
        &agent_id,
        provider.clone(),
        port,
        Some(app),
    )
    .await
    {
        let _ = registry.release_port(port);
        agent.status = AgentStatus::Failed;
        agent.last_error = Some(error.to_string());
        state.db.save_agent_instance(&agent).map_err(db_error)?;
        return Err(AgentCommandError::new(
            "AGENT_LISTENER_FAILED",
            "Agent Gateway could not restart the local Anthropic listener.",
            "Free the assigned port and retry. Existing CC Switch pages remain usable.",
        )
        .with_details(error.to_string()));
    }

    let strategies = requested_strategies(launch_req.launch_strategy.clone());
    let mut last_error = None;
    for strategy in strategies {
        if !strategy_available(&strategy) {
            last_error = Some(format!("{strategy:?} is unavailable"));
            continue;
        }
        let prepared = prepare_launch(
            &launch_req,
            &agent_id,
            port,
            strategy.clone(),
            &provider.name,
            snapshot
                .default_upstream_model
                .as_deref()
                .unwrap_or("unknown"),
            Some(&provider.settings_config),
        )
        .map_err(|e| {
            AgentCommandError::new(
                "POWERSHELL_ARG_REJECTED",
                "The Agent restart request failed launcher validation.",
                "Remove shell control characters from cwd, args, env, title, or session id.",
            )
            .with_details(e.to_string())
        })?;
        match launch_prepared(&prepared) {
            Ok(pid) => {
                agent.pid = Some(pid);
                agent.status = AgentStatus::Running;
                state.db.save_agent_instance(&agent).map_err(db_error)?;
                append_log(
                    &state,
                    &agent_id,
                    "info",
                    "agent_restarted",
                    Some(&format!("Restarted with strategy {:?}", prepared.strategy)),
                    Some(&serde_json::to_string(&prepared.preview()).unwrap_or_default()),
                )?;
                return Ok(agent);
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    let details = last_error.unwrap_or_else(|| "No launch strategy succeeded".to_string());
    agent.status = AgentStatus::Failed;
    agent.last_error = Some(details.clone());
    state.db.save_agent_instance(&agent).map_err(db_error)?;
    let _ = stop_agent_listener(&agent_id).await;
    let _ = registry.release_port(port);
    Err(AgentCommandError::new(
        "LAUNCH_STRATEGY_FAILED",
        "All Agent restart strategies failed.",
        "Use Windows Terminal or PowerShell for interactive Claude Code sessions.",
    )
    .with_details(details))
}

#[tauri::command]
pub async fn agent_gateway_list_agents(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AgentInstance>> {
    state.db.list_agent_instances().map_err(db_error)
}

#[tauri::command]
pub async fn agent_gateway_get_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> CommandResult<AgentInstance> {
    state
        .db
        .get_agent_instance(&agent_id)
        .map_err(db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "AGENT_NOT_FOUND",
                "The requested agent does not exist.",
                "Refresh the Agent Gateway list and try again.",
            )
        })
}

#[tauri::command]
pub async fn agent_gateway_sync_status(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AgentInstance>> {
    AgentGatewayService::new(&state.db)
        .cleanup_stale()
        .map_err(db_error)?;
    state.db.list_agent_instances().map_err(db_error)
}

#[tauri::command]
pub async fn agent_gateway_get_logs(
    state: State<'_, AppState>,
    agent_id: String,
    limit: usize,
) -> CommandResult<Vec<AgentLog>> {
    state
        .db
        .list_agent_logs(&agent_id, limit.clamp(1, 500))
        .map_err(db_error)
}

#[tauri::command]
pub async fn agent_gateway_list_run_profiles() -> CommandResult<Vec<RunProfile>> {
    let now = Utc::now().to_rfc3339();
    Ok(vec![
        RunProfile {
            id: "safe".to_string(),
            name: "Safe".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            kind: RunProfileKind::Safe,
            args: Vec::new(),
            env: Vec::new(),
            allow_custom_profiles: false,
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        RunProfile {
            id: "resume".to_string(),
            name: "Resume".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            kind: RunProfileKind::Resume,
            args: vec!["--resume".to_string()],
            env: Vec::new(),
            allow_custom_profiles: false,
            created_at: now.clone(),
            updated_at: now,
        },
    ])
}

#[tauri::command]
pub async fn agent_gateway_save_run_profile(_profile: RunProfile) -> CommandResult<()> {
    Err(AgentCommandError::new(
        "CUSTOM_PROFILE_DISABLED",
        "Saving custom RunProfiles is disabled in the MVP.",
        "Use the built-in safe or resume profiles.",
    ))
}

#[tauri::command]
pub async fn agent_gateway_delete_run_profile(_profile_id: String) -> CommandResult<()> {
    Err(AgentCommandError::new(
        "CUSTOM_PROFILE_DISABLED",
        "Deleting built-in RunProfiles is not supported.",
        "Use the built-in safe or resume profiles.",
    ))
}

#[tauri::command]
pub async fn agent_gateway_cleanup_stale(
    state: State<'_, AppState>,
) -> CommandResult<CleanupReport> {
    AgentGatewayService::new(&state.db)
        .cleanup_stale()
        .map_err(db_error)
}

#[tauri::command]
pub async fn agent_gateway_preview_provider_snapshot(
    state: State<'_, AppState>,
    req: ProviderSnapshotRequest,
) -> CommandResult<ProviderRuntimeSnapshot> {
    state
        .db
        .verify_agent_gateway_schema_ready()
        .map_err(db_error)?;
    let provider = resolve_provider_for_snapshot(&state.db, &req)?;
    Ok(build_provider_runtime_snapshot(&provider))
}

async fn stop_or_kill(
    state: State<'_, AppState>,
    agent_id: String,
    force: bool,
) -> CommandResult<()> {
    let agent = state
        .db
        .get_agent_instance(&agent_id)
        .map_err(db_error)?
        .ok_or_else(|| {
            AgentCommandError::new(
                "AGENT_NOT_FOUND",
                "The requested agent does not exist.",
                "Refresh the Agent Gateway list and try again.",
            )
        })?;
    if let Some(pid) = agent.pid {
        let _ = taskkill_pid_tree(pid, force);
    }
    for pid in find_processes_by_agent_title(&agent_id).unwrap_or_default() {
        let _ = taskkill_pid_tree(pid, force);
    }
    let _ = stop_agent_listener(&agent_id).await;
    state
        .db
        .update_agent_status(
            &agent_id,
            if force {
                AgentStatus::Killed
            } else {
                AgentStatus::Stopped
            },
            None,
        )
        .map_err(db_error)?;
    let _ = PortRegistry::new(&state.db).release_port(agent.port);
    append_log(
        &state,
        &agent_id,
        "info",
        if force {
            "agent_killed"
        } else {
            "agent_stopped"
        },
        None,
        None,
    )?;
    Ok(())
}

fn launch_prepared(prepared: &PreparedLaunch) -> Result<u32, crate::AppError> {
    launch_with_strategy(prepared)
}

fn requested_strategies(preferred: Option<LaunchStrategy>) -> Vec<LaunchStrategy> {
    match preferred {
        Some(LaunchStrategy::WindowsTerminal) => vec![LaunchStrategy::WindowsTerminal],
        Some(LaunchStrategy::PowerShellWindow) => vec![LaunchStrategy::PowerShellWindow],
        Some(LaunchStrategy::BackgroundProcess) => vec![LaunchStrategy::BackgroundProcess],
        None => vec![
            LaunchStrategy::WindowsTerminal,
            LaunchStrategy::PowerShellWindow,
        ],
    }
}

fn sanitize_display_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn append_log(
    state: &State<'_, AppState>,
    agent_id: &str,
    level: &str,
    event: &str,
    message: Option<&str>,
    payload_json: Option<&str>,
) -> CommandResult<()> {
    state
        .db
        .append_agent_log(&AgentLog {
            id: Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            level: level.to_string(),
            event: event.to_string(),
            message: message.map(str::to_string),
            payload_json: payload_json.map(str::to_string),
            created_at: Utc::now().to_rfc3339(),
        })
        .map_err(db_error)
}

fn db_error(error: crate::AppError) -> AgentCommandError {
    AgentCommandError::new(
        "DB_MIGRATION_FAILED",
        "Agent Gateway database operation failed.",
        "Restart the app. If the issue persists, export diagnostics.",
    )
    .with_details(error.to_string())
}

/// Scan ~/.claude/projects/ JSONL files to find the most recent Claude Code
/// session and extract its session ID. Runs after an agent starts so the
/// agent can later be resumed via --resume without requiring manual input.
fn capture_session_id_from_jsonl(cwd: Option<&str>) -> Option<String> {
    use std::io::{BufRead, BufReader};
    use std::time::SystemTime;

    let projects_dir = crate::config::get_claude_config_dir().join("projects");
    let target_cwd = cwd.map(normalize_session_cwd);

    let mut files: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();
    collect_session_jsonl_files(&projects_dir, &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0)); // newest first

    let mut fallback: Option<String> = None;
    for (_, path) in files.iter().take(50) {
        if let Ok(file) = std::fs::File::open(path) {
            let reader = BufReader::new(file);
            let mut session_id: Option<String> = None;
            let mut cwd_matches = target_cwd.is_none();

            for line in reader.lines().flatten() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if session_id.is_none() {
                        session_id = val
                            .get("sessionId")
                            .or_else(|| val.get("session_id"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|sid| !sid.is_empty())
                            .map(str::to_string);
                    }

                    if let Some(target) = target_cwd.as_deref() {
                        if val
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(normalize_session_cwd)
                            .as_deref()
                            == Some(target)
                        {
                            cwd_matches = true;
                        }
                    }

                    if session_id.is_some() && cwd_matches {
                        break;
                    }
                }
            }

            if session_id.is_none() {
                session_id = infer_session_id_from_filename(path);
            }

            if let Some(sid) = session_id {
                if cwd_matches {
                    log::debug!("[Agent] Found session_id={sid} in {}", path.display());
                    return Some(sid);
                }
                fallback.get_or_insert(sid);
            }
        }
    }

    fallback
}

fn collect_session_jsonl_files(
    root: &std::path::Path,
    files: &mut Vec<(std::time::SystemTime, std::path::PathBuf)>,
) {
    if !root.exists() {
        return;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_jsonl_files(&path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("agent-"))
        {
            continue;
        }
        let modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        files.push((modified, path));
    }
}

fn normalize_session_cwd(cwd: &str) -> String {
    let path = std::path::Path::new(cwd);
    let normalized: std::path::PathBuf = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    normalized.to_string_lossy().replace('\\', "/")
}

fn infer_session_id_from_filename(path: &std::path::Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        capture_session_id_from_jsonl, is_deletable_agent_status, resolve_restart_session_id,
    };
    use crate::agent_gateway::models::{
        AgentInstance, AgentLaunchMode, AgentRuntimeKind, AgentStatus,
    };
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn agent_with_session(cwd: Option<String>, session_id: Option<String>) -> AgentInstance {
        AgentInstance {
            id: "agent-1".to_string(),
            name: "Agent".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            provider_id: "provider-1".to_string(),
            provider_name: Some("Provider".to_string()),
            model: Some("model-1".to_string()),
            launch_mode: AgentLaunchMode::New,
            run_profile_id: "safe".to_string(),
            port: 15722,
            cwd,
            pid: None,
            window_title: Some("CCSA:agent-1".to_string()),
            session_id,
            status: AgentStatus::Stopped,
            created_at: "2026-05-17T00:00:00Z".to_string(),
            started_at: None,
            stopped_at: None,
            last_error: None,
            deleted_at: None,
        }
    }

    #[test]
    fn delete_rejects_active_agent_statuses() {
        for status in [
            AgentStatus::Created,
            AgentStatus::Launching,
            AgentStatus::Running,
            AgentStatus::Stopping,
        ] {
            assert!(!is_deletable_agent_status(status), "{status}");
        }
    }

    #[test]
    fn delete_allows_terminal_agent_statuses() {
        for status in [
            AgentStatus::Stopped,
            AgentStatus::Failed,
            AgentStatus::Exited,
            AgentStatus::Killed,
        ] {
            assert!(is_deletable_agent_status(status), "{status}");
        }
    }

    #[test]
    #[serial]
    fn captures_session_id_from_nested_claude_project_jsonl() {
        let home = TempHome::new();
        let project = home
            .dir
            .path()
            .join(".claude")
            .join("projects")
            .join("nested")
            .join("project");
        std::fs::create_dir_all(&project).expect("create project dir");
        let cwd = home.dir.path().join("work").join("repo");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        std::fs::write(
            project.join("session-file.jsonl"),
            format!(
                "{{\"sessionId\":\"session-nested\",\"cwd\":\"{}\",\"timestamp\":\"2026-05-16T10:00:00Z\"}}\n",
                cwd.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("write jsonl");

        assert_eq!(
            capture_session_id_from_jsonl(Some(&cwd.to_string_lossy())),
            Some("session-nested".to_string())
        );
    }

    #[test]
    #[serial]
    fn captures_session_id_prefers_matching_cwd() {
        let home = TempHome::new();
        let projects = home.dir.path().join(".claude").join("projects");
        let target_cwd = home.dir.path().join("target");
        let other_cwd = home.dir.path().join("other");
        std::fs::create_dir_all(&target_cwd).expect("target cwd");
        std::fs::create_dir_all(&other_cwd).expect("other cwd");
        std::fs::create_dir_all(projects.join("a")).expect("project a");
        std::fs::create_dir_all(projects.join("b")).expect("project b");
        std::fs::write(
            projects.join("a").join("newer-other.jsonl"),
            format!(
                "{{\"sessionId\":\"session-other\",\"cwd\":\"{}\"}}\n",
                other_cwd.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("write other");
        std::fs::write(
            projects.join("b").join("matching-target.jsonl"),
            format!(
                "{{\"sessionId\":\"session-target\",\"cwd\":\"{}\"}}\n",
                target_cwd.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("write target");

        assert_eq!(
            capture_session_id_from_jsonl(Some(&target_cwd.to_string_lossy())),
            Some("session-target".to_string())
        );
    }

    #[test]
    fn restart_session_resolution_prefers_stored_session_id() {
        let agent = agent_with_session(
            Some("C:\\work\\repo".to_string()),
            Some("  stored-session  ".to_string()),
        );
        assert_eq!(
            resolve_restart_session_id(&agent),
            Some("stored-session".to_string())
        );
    }

    #[test]
    #[serial]
    fn restart_session_resolution_falls_back_to_matching_claude_jsonl() {
        let home = TempHome::new();
        let projects = home
            .dir
            .path()
            .join(".claude")
            .join("projects")
            .join("repo");
        let cwd = home.dir.path().join("work").join("repo");
        std::fs::create_dir_all(&projects).expect("projects");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            projects.join("session-file.jsonl"),
            format!(
                "{{\"sessionId\":\"session-from-jsonl\",\"cwd\":\"{}\"}}\n",
                cwd.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("write jsonl");

        let agent = agent_with_session(Some(cwd.to_string_lossy().to_string()), None);
        assert_eq!(
            resolve_restart_session_id(&agent),
            Some("session-from-jsonl".to_string())
        );
    }

    #[test]
    #[serial]
    fn restart_session_resolution_returns_none_instead_of_creating_new_session() {
        let _home = TempHome::new();
        let agent = agent_with_session(Some("C:\\missing\\repo".to_string()), None);
        assert_eq!(resolve_restart_session_id(&agent), None);
    }
}
