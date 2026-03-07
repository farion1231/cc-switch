//! Hidden JSON-over-stdio session for CLI E2E harness.

use crate::cli::{Cli, Commands};
use cc_switch_core::AppState;
use clap::Parser;
use gag::BufferRedirect;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SessionRequest {
    Run { args: Vec<String> },
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    ok: bool,
    stdout: String,
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub async fn run(state: AppState) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<SessionRequest>(&line) {
            Ok(request) => request,
            Err(err) => {
                write_response(
                    &mut stdout,
                    SessionResponse {
                        ok: false,
                        stdout: String::new(),
                        stderr: String::new(),
                        error: Some(format!("Invalid session request JSON: {err}")),
                    },
                )?;
                continue;
            }
        };

        match request {
            SessionRequest::Shutdown => {
                write_response(
                    &mut stdout,
                    SessionResponse {
                        ok: true,
                        stdout: String::new(),
                        stderr: String::new(),
                        error: None,
                    },
                )?;
                break;
            }
            SessionRequest::Run { args } => {
                let response = run_single_command(args, state.clone()).await;
                write_response(&mut stdout, response)?;
            }
        }
    }

    Ok(())
}

async fn run_single_command(args: Vec<String>, state: AppState) -> SessionResponse {
    let parsed = match Cli::try_parse_from(
        std::iter::once("cc-switch".to_string()).chain(args.iter().cloned()),
    ) {
        Ok(cli) => cli,
        Err(err) => {
            return SessionResponse {
                ok: false,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(err.to_string()),
            };
        }
    };

    if matches!(parsed.command, Commands::E2eSession) {
        return SessionResponse {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("Nested __e2e-session is not allowed".to_string()),
        };
    }

    match capture_dispatch(parsed, state).await {
        Ok(response) => response,
        Err(err) => SessionResponse {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("Failed to capture CLI session output: {err}")),
        },
    }
}

async fn capture_dispatch(cli: Cli, state: AppState) -> anyhow::Result<SessionResponse> {
    let mut stdout_redirect = BufferRedirect::stdout()?;
    let mut stderr_redirect = BufferRedirect::stderr()?;

    let result = crate::handlers::dispatch(cli, state).await;

    io::stdout().flush()?;
    io::stderr().flush()?;

    let mut stdout = String::new();
    stdout_redirect.read_to_string(&mut stdout)?;

    let mut stderr = String::new();
    stderr_redirect.read_to_string(&mut stderr)?;

    Ok(match result {
        Ok(()) => SessionResponse {
            ok: true,
            stdout,
            stderr,
            error: None,
        },
        Err(err) => SessionResponse {
            ok: false,
            stdout,
            stderr,
            error: Some(err.to_string()),
        },
    })
}

fn write_response(stdout: &mut impl Write, response: SessionResponse) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *stdout, &response)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
