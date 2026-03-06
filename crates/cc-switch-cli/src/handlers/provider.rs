//! Provider command handlers

use crate::cli::{ProviderCommands, UniversalProviderCommands};
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn handle(
    cmd: ProviderCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ProviderCommands::List { app } => handle_list(&app, state, printer).await,
        ProviderCommands::Show { id, app } => handle_show(&id, &app, state, printer).await,
        ProviderCommands::Add {
            app,
            name,
            base_url,
            api_key,
            from_json,
        } => {
            handle_add(
                &app,
                name.as_deref(),
                base_url.as_deref(),
                api_key.as_deref(),
                from_json.as_deref(),
                state,
                printer,
            )
            .await
        }
        ProviderCommands::Edit {
            id,
            app,
            set_api_key,
            set_base_url,
            set_name,
        } => {
            handle_edit(
                &id,
                &app,
                set_api_key.as_deref(),
                set_base_url.as_deref(),
                set_name.as_deref(),
                state,
                printer,
            )
            .await
        }
        ProviderCommands::Delete { id, app, yes } => {
            handle_delete(&id, &app, yes, state, printer).await
        }
        ProviderCommands::Switch { id, app } => handle_switch(&id, &app, state, printer).await,
        ProviderCommands::Usage { id, app } => handle_usage(&id, &app, state, printer).await,
        ProviderCommands::Universal(cmd) => handle_universal(cmd, state, printer).await,
    }
}

async fn handle_list(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let providers = cc_switch_core::ProviderService::list(state, app_type)?;
    printer.print_providers(&providers)?;
    Ok(())
}

async fn handle_show(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let providers = cc_switch_core::ProviderService::list(state, app_type)?;
    let provider = providers
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", id))?;
    printer.print_provider_detail(provider)?;
    Ok(())
}

async fn handle_add(
    app: &str,
    name: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
    from_json: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement provider add")
}

async fn handle_edit(
    id: &str,
    app: &str,
    set_api_key: Option<&str>,
    set_base_url: Option<&str>,
    set_name: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement provider edit")
}

async fn handle_delete(
    id: &str,
    app: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement provider delete")
}

async fn handle_switch(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::ProviderService::switch(state, app_type, id)?;
    println!("✓ Switched to provider '{}' for {}", id, app);
    Ok(())
}

async fn handle_usage(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement provider usage query")
}

async fn handle_universal(
    cmd: UniversalProviderCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        UniversalProviderCommands::List => {
            let providers = cc_switch_core::ProviderService::list_universal(state)?;
            printer.print_universal_providers(&providers)?;
        }
        UniversalProviderCommands::Add {
            name,
            apps,
            base_url,
            api_key,
        } => {
            todo!("Implement universal provider add")
        }
        UniversalProviderCommands::Sync { id } => {
            cc_switch_core::ProviderService::sync_universal_to_apps(state, &id)?;
            println!("✓ Synced universal provider '{}' to apps", id);
        }
        UniversalProviderCommands::Delete { id, yes } => {
            todo!("Implement universal provider delete")
        }
    }
    Ok(())
}

fn parse_app_type(s: &str) -> anyhow::Result<cc_switch_core::AppType> {
    s.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid app type: {}. Valid values: claude, codex, gemini, opencode",
            s
        )
    })
}
