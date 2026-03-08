//! Session command handlers

use anyhow::anyhow;
use serde_json::json;

use crate::cli::SessionCommands;
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use cc_switch_core::{SessionMeta, SessionService};

pub async fn handle(cmd: SessionCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SessionCommands::List { provider, query } => {
            let provider = provider
                .as_deref()
                .map(normalize_provider)
                .transpose()?;
            let query = query.as_deref().map(str::trim).filter(|value| !value.is_empty());

            let sessions = SessionService::list_sessions()
                .into_iter()
                .filter(|session| provider.as_deref().is_none_or(|value| session.provider_id == value))
                .filter(|session| query.is_none_or(|value| matches_query(session, value)))
                .collect::<Vec<_>>();
            printer.print_value(&sessions)?;
            Ok(())
        }
        SessionCommands::Messages {
            provider,
            source_path,
        } => {
            let provider = normalize_provider(&provider)?;
            let messages = SessionService::get_session_messages(&provider, &source_path)?;
            printer.print_value(&messages)?;
            Ok(())
        }
        SessionCommands::ResumeCommand {
            session_id,
            provider,
            source_path,
        } => {
            let provider = provider
                .as_deref()
                .map(normalize_provider)
                .transpose()?;
            let session = resolve_session(&session_id, provider.as_deref(), source_path.as_deref())?;
            let resume_command = session.resume_command.clone().ok_or_else(|| {
                anyhow!(
                    "No resume command is available for provider '{}' and session '{}'",
                    session.provider_id,
                    session.session_id
                )
            })?;
            printer.print_value(&json!({
                "providerId": session.provider_id,
                "sessionId": session.session_id,
                "sourcePath": session.source_path,
                "resumeCommand": resume_command,
            }))?;
            Ok(())
        }
    }
}

fn normalize_provider(provider: &str) -> anyhow::Result<String> {
    Ok(parse_app_type(provider)?.as_str().to_string())
}

fn matches_query(session: &SessionMeta, query: &str) -> bool {
    let needle = query.to_lowercase();
    [
        Some(session.provider_id.as_str()),
        Some(session.session_id.as_str()),
        session.title.as_deref(),
        session.summary.as_deref(),
        session.project_dir.as_deref(),
        session.source_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|field| field.to_lowercase().contains(&needle))
}

fn resolve_session(
    session_id: &str,
    provider: Option<&str>,
    source_path: Option<&str>,
) -> anyhow::Result<SessionMeta> {
    let matches = SessionService::list_sessions()
        .into_iter()
        .filter(|session| session.session_id == session_id)
        .filter(|session| provider.is_none_or(|value| session.provider_id == value))
        .filter(|session| {
            source_path.is_none_or(|value| session.source_path.as_deref() == Some(value))
        })
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Err(anyhow!("Session not found: {}", session_id)),
        1 => Ok(matches.into_iter().next().expect("one session")),
        _ => Err(anyhow!(
            "Multiple sessions matched '{}'. Re-run with --provider or --source-path.",
            session_id
        )),
    }
}
