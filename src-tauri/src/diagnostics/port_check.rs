use crate::diagnostics::report::DiagnosticCheck;
use serde_json::json;
use std::net::TcpListener;

pub fn check_ports() -> Vec<DiagnosticCheck> {
    let mut available = Vec::new();
    let mut occupied = Vec::new();
    for port in 15722..=15799 {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                available.push(port);
            }
            Err(_) => occupied.push(port),
        }
    }

    if available.is_empty() {
        vec![DiagnosticCheck::error(
            "agent_gateway_ports",
            "Agent Gateway port pool",
            "No ports are available in 15722-15799.",
            "Stop stale local services using the Agent Gateway port range.",
        )
        .with_details(json!({ "occupied": occupied }))]
    } else {
        vec![DiagnosticCheck::ok(
            "agent_gateway_ports",
            "Agent Gateway port pool",
            format!("{} ports are available", available.len()),
        )
        .with_details(json!({
            "availableSample": available.into_iter().take(8).collect::<Vec<_>>(),
            "occupied": occupied
        }))]
    }
}
