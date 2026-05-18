use crate::agent_gateway::launcher_security::build_process_marker_query;
use crate::error::AppError;
use std::process::Command;

pub fn taskkill_pid_tree(pid: u32, force: bool) -> Result<(), AppError> {
    let mut command = Command::new("taskkill");
    command.arg("/PID").arg(pid.to_string()).arg("/T");
    if force {
        command.arg("/F");
    }
    let output = command
        .output()
        .map_err(|e| AppError::Message(format!("LAUNCH_STRATEGY_FAILED: taskkill failed: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "LAUNCH_STRATEGY_FAILED: taskkill exited with status {:?}",
            output.status.code()
        )))
    }
}

pub fn find_processes_by_agent_title(agent_id: &str) -> Result<Vec<u32>, AppError> {
    let query = build_process_marker_query(agent_id)?;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &query])
        .output()
        .map_err(|e| {
            AppError::Message(format!("LAUNCH_STRATEGY_FAILED: process query failed: {e}"))
        })?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect())
}
