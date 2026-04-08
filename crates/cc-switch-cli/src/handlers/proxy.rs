//! Proxy command handlers

use anyhow::{anyhow, Error};

use crate::cli::{
    ProxyAppConfigCommands, ProxyAutoFailoverCommands, ProxyCircuitCommands,
    ProxyCircuitConfigCommands, ProxyCommands, ProxyConfigCommands,
    ProxyDefaultCostMultiplierCommands, ProxyFailoverCommands, ProxyGlobalConfigCommands,
    ProxyPricingModelSourceCommands, ProxyTakeoverCommands,
};
use crate::handlers::common::parse_proxy_app_type;
use crate::output::Printer;
use cc_switch_core::{
    AppProxyConfig, AppState, CircuitBreakerConfig, GlobalProxyConfig, ProviderService,
    ProviderSortUpdate,
};
use serde_json::json;

pub async fn handle(cmd: ProxyCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        ProxyCommands::Start { port, host } => handle_start(port, &host, state, printer).await,
        ProxyCommands::Stop => handle_stop(state, printer).await,
        ProxyCommands::Status => handle_status(state, printer).await,
        ProxyCommands::Config(cmd) => handle_config(cmd, state, printer).await,
        ProxyCommands::GlobalConfig(cmd) => handle_global_config(cmd, state, printer).await,
        ProxyCommands::AppConfig(cmd) => handle_app_config(cmd, state, printer).await,
        ProxyCommands::AutoFailover(cmd) => handle_auto_failover(cmd, state, printer).await,
        ProxyCommands::AvailableProviders { app } => {
            handle_available_providers(&app, state, printer).await
        }
        ProxyCommands::ProviderHealth { id, app } => {
            handle_provider_health(&id, &app, state, printer).await
        }
        ProxyCommands::DefaultCostMultiplier(cmd) => {
            handle_default_cost_multiplier(cmd, state, printer).await
        }
        ProxyCommands::PricingModelSource(cmd) => {
            handle_pricing_model_source(cmd, state, printer).await
        }
        ProxyCommands::Takeover(cmd) => handle_takeover(cmd, state, printer).await,
        ProxyCommands::Failover(cmd) => handle_failover(cmd, state, printer).await,
        ProxyCommands::Circuit(cmd) => handle_circuit(cmd, state, printer).await,
    }
}

async fn handle_start(
    port: u16,
    host: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let mut config = state.proxy_service.get_config().await.map_err(Error::msg)?;

    let mut changed = false;
    if config.listen_port != port {
        config.listen_port = port;
        changed = true;
    }
    if config.listen_address != host {
        config.listen_address = host.to_string();
        changed = true;
    }

    if changed {
        state
            .proxy_service
            .update_config(&config)
            .await
            .map_err(Error::msg)?;
    }

    let info = state.proxy_service.start().await.map_err(Error::msg)?;
    printer.success(format!(
        "✓ Proxy server started at {}:{}",
        info.address, info.port
    ));
    Ok(())
}

async fn handle_stop(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match state.proxy_service.stop_with_restore().await {
        Ok(()) => {
            printer.success("✓ Proxy server stopped");
            Ok(())
        }
        Err(err) if err.contains("未运行") || err.contains("not running") => {
            printer.print_text("Proxy server is not running.")?;
            Ok(())
        }
        Err(err) => Err(anyhow!(err)),
    }
}

async fn handle_status(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let status = state.proxy_service.get_status().await.map_err(Error::msg)?;
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
            let config = state.proxy_service.get_config().await.map_err(Error::msg)?;
            printer.print_proxy_config(&config)?;
        }
        ProxyConfigCommands::Set {
            port,
            host,
            log_enabled,
        } => {
            let mut config = state.proxy_service.get_config().await.map_err(Error::msg)?;

            if let Some(port) = port {
                config.listen_port = port;
            }
            if let Some(host) = host {
                config.listen_address = host;
            }
            if let Some(log_enabled) = log_enabled {
                config.enable_logging = log_enabled;
            }

            state
                .proxy_service
                .update_config(&config)
                .await
                .map_err(Error::msg)?;
            printer.print_proxy_config(&config)?;
        }
    }
    Ok(())
}

async fn handle_global_config(
    cmd: ProxyGlobalConfigCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyGlobalConfigCommands::Show => {
            let config = state
                .proxy_service
                .get_global_proxy_config()
                .await
                .map_err(Error::msg)?;
            printer.print_value(&config)?;
        }
        ProxyGlobalConfigCommands::Set {
            proxy_enabled,
            host,
            port,
            log_enabled,
        } => {
            let mut config: GlobalProxyConfig = state
                .proxy_service
                .get_global_proxy_config()
                .await
                .map_err(Error::msg)?;

            if let Some(proxy_enabled) = proxy_enabled {
                config.proxy_enabled = proxy_enabled;
            }
            if let Some(host) = host {
                config.listen_address = host;
            }
            if let Some(port) = port {
                config.listen_port = port;
            }
            if let Some(log_enabled) = log_enabled {
                config.enable_logging = log_enabled;
            }

            state
                .proxy_service
                .update_global_proxy_config(config.clone())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&config)?;
        }
    }

    Ok(())
}

async fn handle_app_config(
    cmd: ProxyAppConfigCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyAppConfigCommands::Show { app } => {
            let app = parse_proxy_app_type(&app)?;
            let config = state
                .proxy_service
                .get_app_proxy_config(app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&config)?;
        }
        ProxyAppConfigCommands::Set {
            app,
            enabled,
            auto_failover_enabled,
            max_retries,
            streaming_first_byte_timeout,
            streaming_idle_timeout,
            non_streaming_timeout,
            circuit_failure_threshold,
            circuit_success_threshold,
            circuit_timeout_seconds,
            circuit_error_rate_threshold,
            circuit_min_requests,
        } => {
            let app = parse_proxy_app_type(&app)?;
            let mut config: AppProxyConfig = state
                .proxy_service
                .get_app_proxy_config(app.as_str())
                .await
                .map_err(Error::msg)?;

            if let Some(enabled) = enabled {
                config.enabled = enabled;
            }
            if let Some(auto_failover_enabled) = auto_failover_enabled {
                config.auto_failover_enabled = auto_failover_enabled;
            }
            if let Some(max_retries) = max_retries {
                config.max_retries = max_retries;
            }
            if let Some(streaming_first_byte_timeout) = streaming_first_byte_timeout {
                config.streaming_first_byte_timeout = streaming_first_byte_timeout;
            }
            if let Some(streaming_idle_timeout) = streaming_idle_timeout {
                config.streaming_idle_timeout = streaming_idle_timeout;
            }
            if let Some(non_streaming_timeout) = non_streaming_timeout {
                config.non_streaming_timeout = non_streaming_timeout;
            }
            if let Some(circuit_failure_threshold) = circuit_failure_threshold {
                config.circuit_failure_threshold = circuit_failure_threshold;
            }
            if let Some(circuit_success_threshold) = circuit_success_threshold {
                config.circuit_success_threshold = circuit_success_threshold;
            }
            if let Some(circuit_timeout_seconds) = circuit_timeout_seconds {
                config.circuit_timeout_seconds = circuit_timeout_seconds;
            }
            if let Some(circuit_error_rate_threshold) = circuit_error_rate_threshold {
                config.circuit_error_rate_threshold = circuit_error_rate_threshold;
            }
            if let Some(circuit_min_requests) = circuit_min_requests {
                config.circuit_min_requests = circuit_min_requests;
            }

            state
                .proxy_service
                .update_app_proxy_config(config.clone())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&config)?;
        }
    }

    Ok(())
}

async fn handle_auto_failover(
    cmd: ProxyAutoFailoverCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyAutoFailoverCommands::Show { app } => {
            let app = parse_proxy_app_type(&app)?;
            let enabled = state
                .proxy_service
                .get_auto_failover_enabled(app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "enabled": enabled,
            }))?;
        }
        ProxyAutoFailoverCommands::Enable { app } => {
            let app = parse_proxy_app_type(&app)?;
            let active_provider_id = state
                .proxy_service
                .set_auto_failover_enabled(app.as_str(), true)
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "enabled": true,
                "activeProviderId": active_provider_id,
            }))?;
        }
        ProxyAutoFailoverCommands::Disable { app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .set_auto_failover_enabled(app.as_str(), false)
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "enabled": false,
            }))?;
        }
    }

    Ok(())
}

async fn handle_available_providers(
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app = parse_proxy_app_type(app)?;
    let providers = state
        .proxy_service
        .get_available_providers_for_failover(app.as_str())
        .await
        .map_err(Error::msg)?;
    let providers = providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect();
    printer.print_providers(&providers)?;
    Ok(())
}

async fn handle_provider_health(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app = parse_proxy_app_type(app)?;
    let health = state
        .proxy_service
        .get_provider_health(id, app.as_str())
        .await
        .map_err(Error::msg)?;
    printer.print_provider_health(&health)?;
    Ok(())
}

async fn handle_default_cost_multiplier(
    cmd: ProxyDefaultCostMultiplierCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyDefaultCostMultiplierCommands::Get { app } => {
            let app = parse_proxy_app_type(&app)?;
            let value = state
                .proxy_service
                .get_default_cost_multiplier(app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "value": value,
            }))?;
        }
        ProxyDefaultCostMultiplierCommands::Set { app, value } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .set_default_cost_multiplier(app.as_str(), &value)
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "value": value,
            }))?;
        }
    }

    Ok(())
}

async fn handle_pricing_model_source(
    cmd: ProxyPricingModelSourceCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyPricingModelSourceCommands::Get { app } => {
            let app = parse_proxy_app_type(&app)?;
            let value = state
                .proxy_service
                .get_pricing_model_source(app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "value": value,
            }))?;
        }
        ProxyPricingModelSourceCommands::Set { app, value } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .set_pricing_model_source(app.as_str(), &value)
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "value": value,
            }))?;
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
            let status = state
                .proxy_service
                .get_takeover_status()
                .await
                .map_err(Error::msg)?;
            printer.print_takeover_status(&status)?;
        }
        ProxyTakeoverCommands::Enable { app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .set_takeover_for_app(app.as_str(), true)
                .await
                .map_err(Error::msg)?;
            printer.success(format!("✓ Enabled takeover for {}", app.as_str()));
        }
        ProxyTakeoverCommands::Disable { app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .set_takeover_for_app(app.as_str(), false)
                .await
                .map_err(Error::msg)?;
            printer.success(format!("✓ Disabled takeover for {}", app.as_str()));
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
            let app = parse_proxy_app_type(&app)?;
            let queue = state
                .proxy_service
                .get_failover_queue(app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_failover_queue(&queue)?;
        }
        ProxyFailoverCommands::Add { id, app, priority } => {
            let app = parse_proxy_app_type(&app)?;
            if let Some(priority) = priority {
                let sort_index = usize::try_from(priority)
                    .map_err(|_| anyhow!("Failover priority must be zero or greater"))?;
                ProviderService::update_sort_order(
                    state,
                    app.clone(),
                    vec![ProviderSortUpdate {
                        id: id.clone(),
                        sort_index,
                    }],
                )
                .map_err(Error::msg)?;
            }
            state
                .proxy_service
                .add_to_failover_queue(app.as_str(), &id)
                .await
                .map_err(Error::msg)?;
            printer.success(format!(
                "✓ Added provider '{}' to failover queue for {}",
                id,
                app.as_str()
            ));
        }
        ProxyFailoverCommands::Remove { id, app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .remove_from_failover_queue(app.as_str(), &id)
                .await
                .map_err(Error::msg)?;
            printer.success(format!(
                "✓ Removed provider '{}' from failover queue for {}",
                id,
                app.as_str()
            ));
        }
        ProxyFailoverCommands::Switch { id, app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .switch_proxy_target(app.as_str(), &id)
                .await
                .map_err(Error::msg)?;
            printer.success(format!(
                "✓ Switched to provider '{}' for {}",
                id,
                app.as_str()
            ));
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
            handle_provider_health(&id, &app, state, printer).await?;
        }
        ProxyCircuitCommands::Reset { id, app } => {
            let app = parse_proxy_app_type(&app)?;
            state
                .proxy_service
                .reset_provider_circuit(&id, app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.success(format!(
                "✓ Reset circuit breaker for provider '{}' in {}",
                id,
                app.as_str()
            ));
        }
        ProxyCircuitCommands::Stats { id, app } => {
            let app = parse_proxy_app_type(&app)?;
            let stats = state
                .proxy_service
                .get_circuit_breaker_stats(&id, app.as_str())
                .await
                .map_err(Error::msg)?;
            printer.print_value(&json!({
                "app": app.as_str(),
                "providerId": id,
                "stats": stats,
            }))?;
        }
        ProxyCircuitCommands::Config(cmd) => {
            handle_circuit_config(cmd, state, printer).await?;
        }
    }
    Ok(())
}

async fn handle_circuit_config(
    cmd: ProxyCircuitConfigCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProxyCircuitConfigCommands::Show => {
            let config = state
                .proxy_service
                .get_circuit_breaker_config()
                .await
                .map_err(Error::msg)?;
            printer.print_circuit_breaker_config(&config)?;
        }
        ProxyCircuitConfigCommands::Set {
            failure_threshold,
            recovery_timeout,
            half_open_requests,
        } => {
            if half_open_requests.is_some() {
                return Err(anyhow!(
                    "`--half-open-requests` is not supported by the current core circuit breaker model"
                ));
            }

            let mut config: CircuitBreakerConfig = state
                .proxy_service
                .get_circuit_breaker_config()
                .await
                .map_err(Error::msg)?;
            if let Some(failure_threshold) = failure_threshold {
                config.failure_threshold = failure_threshold;
            }
            if let Some(recovery_timeout) = recovery_timeout {
                config.timeout_seconds = recovery_timeout;
            }

            state
                .proxy_service
                .save_circuit_breaker_config(config.clone())
                .await
                .map_err(Error::msg)?;
            printer.print_circuit_breaker_config(&config)?;
        }
    }
    Ok(())
}
