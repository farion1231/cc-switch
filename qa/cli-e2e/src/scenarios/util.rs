use anyhow::{Context, Result};
use std::net::TcpListener;

use crate::mock::MockServer;
use crate::sandbox::Sandbox;

pub fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

pub fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind random port")?;
    let port = listener
        .local_addr()
        .context("failed to inspect random port")?
        .port();
    drop(listener);
    Ok(port)
}

pub async fn finalize(
    sandbox: &Sandbox,
    success: bool,
    mock_server: Option<&MockServer>,
) -> Result<()> {
    let mock_requests = if let Some(server) = mock_server {
        Some(server.requests_json().await?)
    } else {
        None
    };
    sandbox.finalize(success, mock_requests)
}
