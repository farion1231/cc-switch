use crate::agent_gateway::models::AgentRuntimeKind;
use crate::database::Database;
use crate::error::AppError;
use chrono::Utc;
use std::collections::HashSet;
use std::net::TcpListener;

pub const AGENT_PORT_START: u16 = 15722;
pub const AGENT_PORT_END: u16 = 15799;

#[derive(Debug, Clone)]
pub struct PortBinding {
    pub port: u16,
    pub agent_id: String,
    pub provider_id: String,
    pub runtime: AgentRuntimeKind,
    pub created_at: String,
}

pub struct PortRegistry<'a> {
    db: &'a Database,
}

impl<'a> PortRegistry<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn allocate_port(&self) -> Result<u16, AppError> {
        let used = self.db.list_agent_port_numbers()?;
        for port in AGENT_PORT_START..=AGENT_PORT_END {
            if used.contains(&port) {
                continue;
            }
            if is_port_available(port) {
                return Ok(port);
            }
        }
        Err(AppError::Message(
            "PORT_POOL_EXHAUSTED: no free Agent Gateway ports in 15722-15799".to_string(),
        ))
    }

    pub fn bind_provider(
        &self,
        port: u16,
        agent_id: &str,
        provider_id: &str,
        runtime: AgentRuntimeKind,
    ) -> Result<PortBinding, AppError> {
        validate_agent_port(port)?;
        let binding = PortBinding {
            port,
            agent_id: agent_id.to_string(),
            provider_id: provider_id.to_string(),
            runtime,
            created_at: Utc::now().to_rfc3339(),
        };
        self.db.save_agent_port_binding(&binding)?;
        Ok(binding)
    }

    pub fn release_port(&self, port: u16) -> Result<(), AppError> {
        validate_agent_port(port)?;
        self.db.delete_agent_port_binding(port)
    }

    pub fn get_provider_by_port(&self, port: u16) -> Result<Option<String>, AppError> {
        validate_agent_port(port)?;
        self.db.get_provider_id_for_agent_port(port)
    }
}

pub fn validate_agent_port(port: u16) -> Result<(), AppError> {
    if !(AGENT_PORT_START..=AGENT_PORT_END).contains(&port) {
        return Err(AppError::Message(format!(
            "PORT_OUT_OF_RANGE: agent port {port} is outside 15722-15799"
        )));
    }
    Ok(())
}

fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub(crate) fn port_set(values: impl IntoIterator<Item = u16>) -> HashSet<u16> {
    values.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{validate_agent_port, PortRegistry, AGENT_PORT_START};
    use crate::agent_gateway::models::AgentRuntimeKind;
    use crate::database::Database;

    #[test]
    fn port_registry_allocates_and_releases_port() {
        let db = Database::memory().expect("memory db");
        let registry = PortRegistry::new(&db);

        let port = registry.allocate_port().expect("allocate");
        assert!((15722..=15799).contains(&port));

        registry
            .bind_provider(port, "agent-1", "provider-1", AgentRuntimeKind::ClaudeCode)
            .expect("bind");
        assert_eq!(
            registry
                .get_provider_by_port(port)
                .expect("lookup")
                .as_deref(),
            Some("provider-1")
        );

        registry.release_port(port).expect("release");
        assert!(registry
            .get_provider_by_port(port)
            .expect("lookup")
            .is_none());
    }

    #[test]
    fn port_registry_rejects_out_of_range_ports() {
        assert!(validate_agent_port(AGENT_PORT_START - 1).is_err());
        assert!(validate_agent_port(15800).is_err());
    }
}
