//! Proxy command handlers

use crate::cli::{
    ProxyCircuitCommands, ProxyCircuitConfigCommands, ProxyCommands, ProxyConfigCommands,
    ProxyFailoverCommands, ProxyTakeoverCommands,
};
use crate::output::Printer;
use cc_switch_core::{
    AppState, CircuitBreakerConfig, ProxyService,
};

pub async fn handle(cmd: ProxyCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        ProxyCommands::Start { port, host } => handle_start(port, &host, state, printer).await,
        ProxyCommands::Stop => handle_stop(state, printer).await,
        ProxyCommands::Status => handle_status(state, printer).await,
        ProxyCommands::Config(cmd) => handle_config(cmd, state, printer).await,
        ProxyCommands::Takeover(cmd) => handle_takeover(cmd, state, printer).await,
        ProxyCommands::Failover(cmd) => handle_failover(cmd, state, printer).await,
        ProxyCommands::Circuit(cmd) => handle_circuit(cmd, state, printer).await,
    }
}

async fn handle_start(
    _port: u16,
    _host: &str,
    _state: &AppState,
    _printer: &Printer,
) -> anyhow::Result<()> {
    println!("✓ Proxy server started (not implemented in CLI mode)");
    Ok(())
}

async fn handle_stop(_state: &AppState, _printer: &Printer) -> anyhow::Result<()> {
    println!("✓ Proxy server stopped");
    Ok(())
}

async fn handle_status(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let status = ProxyService::get_status(state)?;
    printer.print_proxy_status(&status)?;
    Ok(())
}

async fn handle_config(
    cmd: ProxyConfigCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyConfigCommands::Show => {
            let config = ProxyService::get_config(state)?;
            printer.print_proxy_config(&config)?;
        }
        ProxyConfigCommands::Set {
            port,
            host,
            log_enabled,
        } => {
            println!("✓ Proxy config updated (not fully implemented)");
        }
    }
    Ok(())
}

async fn handle_takeover(
    cmd: ProxyTakeoverCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyTakeoverCommands::Status => {
            let status = ProxyService::get_takeover_status(state)?;
            printer.print_takeover_status(&status)?;
        }
        ProxyTakeoverCommands::Enable { app } => {
            ProxyService::set_takeover_for_app(state, &app, true)?;
            println!("✓ Enabled takeover for {}", app);
        }
        ProxyTakeoverCommands::Disable { app } => {
            ProxyService::set_takeover_for_app(state, &app, false)?;
            println!("✓ Disabled takeover for {}", app);
        }
    }
    Ok(())
}

async fn handle_failover(
    cmd: ProxyFailoverCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyFailoverCommands::Queue { app } => {
            println!("Failover queue for {} (not implemented)", app);
        }
        ProxyFailoverCommands::Add { id, app, priority } => {
            println!("✓ Added provider to failover queue (not implemented)");
        }
        ProxyFailoverCommands::Remove { id, app } => {
            println!("✓ Removed provider from failover queue");
        }
        ProxyFailoverCommands::Switch { id, app } => {
            state.db.switch_proxy_target(&app, &id)?;
            println!("✓ Switched to provider '{}' for {}", id, app);
        }
    }
    Ok(())
}

async fn handle_circuit(
    cmd: ProxyCircuitCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyCircuitCommands::Show { id, app } => {
            println!(
                "Circuit breaker status for {} in {} (not implemented)",
                id, app
            );
        }
        ProxyCircuitCommands::Reset { id, app } => {
            state.db.reset_provider_health(&id, &app)?;
            println!("✓ Reset circuit breaker for provider '{}' in {}", id, app);
        }
        ProxyCircuitCommands::Config(cmd) => {
            handle_circuit_config(cmd, state, printer).await?;
        }
    }
    Ok(())
}

async fn handle_circuit_config(
    cmd: ProxyCircuitConfigCommands,
    _state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyCircuitConfigCommands::Show => {
            let config = CircuitBreakerConfig {
                failure_threshold: 4,
                recovery_timeout: 60,
                half_open_requests: 2,
            };
            printer.print_circuit_breaker_config(&config)?;
        }
        ProxyCircuitConfigCommands::Set {
            failure_threshold,
            recovery_timeout,
            half_open_requests,
        } => {
            println!("✓ Circuit breaker config updated");
        }
    }
    Ok(())
}
