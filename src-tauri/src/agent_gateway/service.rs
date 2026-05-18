use crate::agent_gateway::models::AgentStatus;
use crate::database::Database;
use crate::error::AppError;
use chrono::{DateTime, Duration, Utc};

pub struct AgentGatewayService<'a> {
    db: &'a Database,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub failed_launching: usize,
    pub zombie_killed: usize,
    pub released_ports: usize,
}

impl<'a> AgentGatewayService<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn is_process_running(pid: u32) -> bool {
        #[cfg(windows)]
        {
            // On Windows, query specific PID via tasklist (fast, single lookup)
            std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let out = String::from_utf8_lossy(&o.stdout);
                        Some(out.contains(&pid.to_string()))
                    } else {
                        None
                    }
                })
                .unwrap_or(true) // on error, assume alive (avoid false cleanup)
        }
        #[cfg(not(windows))]
        {
            // Unix: send signal 0 to check existence
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(true)
        }
    }

    pub fn cleanup_stale(&self) -> Result<CleanupReport, AppError> {
        let agents = self.db.list_agent_instances()?;
        let now = Utc::now();
        let mut report = CleanupReport::default();

        for agent in agents {
            match agent.status {
                AgentStatus::Launching
                    if is_older_than(
                        agent.started_at.as_deref().unwrap_or(&agent.created_at),
                        now,
                        30,
                    ) =>
                {
                    self.db.update_agent_status(
                        &agent.id,
                        AgentStatus::Failed,
                        Some("Launching exceeded 30 seconds without a running process"),
                    )?;
                    report.failed_launching += 1;
                    let _ = self.db.delete_agent_port_binding(agent.port);
                    report.released_ports += 1;
                }
                AgentStatus::Running => {
                    if let Some(pid) = agent.pid {
                        if !Self::is_process_running(pid) {
                            self.db.update_agent_status(
                                &agent.id,
                                AgentStatus::Exited,
                                Some("Process no longer running (zombie)"),
                            )?;
                            report.zombie_killed += 1;
                            let _ = self.db.delete_agent_port_binding(agent.port);
                            report.released_ports += 1;
                        }
                    }
                }
                AgentStatus::Exited
                | AgentStatus::Killed
                | AgentStatus::Stopped
                | AgentStatus::Failed => {
                    if self.db.delete_agent_port_binding(agent.port).is_ok() {
                        report.released_ports += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(report)
    }
}

fn is_older_than(timestamp: &str, now: DateTime<Utc>, seconds: i64) -> bool {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| now.signed_duration_since(dt.with_timezone(&Utc)) > Duration::seconds(seconds))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::AgentGatewayService;
    use crate::agent_gateway::models::{
        AgentInstance, AgentLaunchMode, AgentRuntimeKind, AgentStatus,
    };
    use crate::agent_gateway::port_registry::PortRegistry;
    use crate::database::Database;
    use chrono::{Duration, Utc};

    #[test]
    fn cleanup_stale_marks_old_launching_agents_failed() {
        let db = Database::memory().expect("memory db");
        let created_at = (Utc::now() - Duration::seconds(45)).to_rfc3339();
        let agent = AgentInstance {
            id: "agent-1".to_string(),
            name: "Agent 1".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            provider_id: "provider-1".to_string(),
            provider_name: Some("Provider 1".to_string()),
            model: None,
            launch_mode: AgentLaunchMode::New,
            run_profile_id: "safe".to_string(),
            port: 15722,
            cwd: None,
            pid: None,
            window_title: None,
            session_id: None,
            status: AgentStatus::Launching,
            created_at: created_at.clone(),
            started_at: Some(created_at),
            stopped_at: None,
            last_error: None,
            deleted_at: None,
        };
        db.save_agent_instance(&agent).expect("save agent");
        PortRegistry::new(&db)
            .bind_provider(15722, "agent-1", "provider-1", AgentRuntimeKind::ClaudeCode)
            .expect("bind port");

        let report = AgentGatewayService::new(&db)
            .cleanup_stale()
            .expect("cleanup");

        assert_eq!(report.failed_launching, 1);
        assert_eq!(report.released_ports, 1);
        let agents = db.list_agent_instances().expect("agents");
        assert_eq!(agents[0].status, AgentStatus::Failed);
    }
}
