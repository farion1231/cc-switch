//! Structured settings command handlers

use anyhow::anyhow;
use serde_json::json;

use crate::cli::{
    SettingsCommands, SettingsOnboardingCommands, SettingsStartupCommands,
    SettingsToggleCommands, SettingsValueCommands, SettingsVisibleAppsCommands,
};
use crate::output::Printer;
use cc_switch_core::settings::{self, VisibleApps};
use cc_switch_core::{
    AppSettings, AppState, ClaudePluginService, HostService, SettingsSaveResult, SettingsService,
};

pub async fn handle(
    cmd: SettingsCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        SettingsCommands::Show => {
            printer.print_value(&SettingsService::get_settings()?)?;
            Ok(())
        }
        SettingsCommands::Language { cmd } => handle_language(cmd, printer),
        SettingsCommands::VisibleApps { cmd } => handle_visible_apps(cmd, printer),
        SettingsCommands::Terminal { cmd } => handle_terminal(cmd, printer),
        SettingsCommands::Startup { cmd } => handle_startup(cmd, printer),
        SettingsCommands::Plugin { cmd } => handle_plugin(cmd, state, printer),
        SettingsCommands::Onboarding { cmd } => handle_onboarding(cmd, state, printer),
    }
}

fn handle_language(cmd: SettingsValueCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SettingsValueCommands::Get => {
            printer.print_value(&json!({
                "language": settings::get_settings().language,
            }))?;
        }
        SettingsValueCommands::Set { value } => {
            let mut current = settings::get_settings();
            current.language = Some(value);
            settings::update_settings(current.clone())?;
            printer.print_value(&json!({
                "language": current.language,
            }))?;
        }
        SettingsValueCommands::Clear => {
            let mut current = settings::get_settings();
            current.language = None;
            settings::update_settings(current)?;
            printer.print_value(&json!({ "language": null }))?;
        }
    }

    Ok(())
}

fn handle_visible_apps(
    cmd: SettingsVisibleAppsCommands,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        SettingsVisibleAppsCommands::Get => {
            let current = settings::get_settings();
            printer.print_value(&json!({
                "configured": current.visible_apps.is_some(),
                "visibleApps": current.visible_apps.unwrap_or_default(),
            }))?;
        }
        SettingsVisibleAppsCommands::Set {
            claude,
            codex,
            gemini,
            opencode,
            openclaw,
        } => {
            if [claude, codex, gemini, opencode, openclaw]
                .into_iter()
                .all(|value| value.is_none())
            {
                return Err(anyhow!(
                    "settings visible-apps set requires at least one app flag"
                ));
            }

            let mut value = settings::get_settings().visible_apps.unwrap_or_default();
            if let Some(claude) = claude {
                value.claude = claude;
            }
            if let Some(codex) = codex {
                value.codex = codex;
            }
            if let Some(gemini) = gemini {
                value.gemini = gemini;
            }
            if let Some(opencode) = opencode {
                value.opencode = opencode;
            }
            if let Some(openclaw) = openclaw {
                value.openclaw = openclaw;
            }

            HostService::set_visible_apps(Some(value.clone()))?;
            printer.print_value(&json!({
                "configured": true,
                "visibleApps": value,
            }))?;
        }
        SettingsVisibleAppsCommands::Clear => {
            HostService::set_visible_apps(None)?;
            printer.print_value(&json!({
                "configured": false,
                "visibleApps": VisibleApps::default(),
            }))?;
        }
    }

    Ok(())
}

fn handle_terminal(cmd: SettingsValueCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SettingsValueCommands::Get => {
            printer.print_value(&json!({
                "preferredTerminal": settings::get_settings().preferred_terminal,
            }))?;
        }
        SettingsValueCommands::Set { value } => {
            HostService::set_preferred_terminal(Some(&value))?;
            printer.print_value(&json!({
                "preferredTerminal": settings::get_settings().preferred_terminal,
            }))?;
        }
        SettingsValueCommands::Clear => {
            HostService::set_preferred_terminal(None)?;
            printer.print_value(&json!({ "preferredTerminal": null }))?;
        }
    }

    Ok(())
}

fn handle_startup(cmd: SettingsStartupCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SettingsStartupCommands::Show => {
            printer.print_value(&startup_payload(&settings::get_settings()))?;
        }
        SettingsStartupCommands::Set {
            show_in_tray,
            minimize_to_tray_on_close,
            launch_on_startup,
            silent_startup,
        } => {
            if [
                show_in_tray,
                minimize_to_tray_on_close,
                launch_on_startup,
                silent_startup,
            ]
            .into_iter()
            .all(|value| value.is_none())
            {
                return Err(anyhow!(
                    "settings startup set requires at least one startup flag"
                ));
            }

            let mut current = settings::get_settings();
            if let Some(show_in_tray) = show_in_tray {
                current.show_in_tray = show_in_tray;
            }
            if let Some(minimize_to_tray_on_close) = minimize_to_tray_on_close {
                current.minimize_to_tray_on_close = minimize_to_tray_on_close;
            }
            if let Some(launch_on_startup) = launch_on_startup {
                current.launch_on_startup = launch_on_startup;
            }
            if let Some(silent_startup) = silent_startup {
                current.silent_startup = silent_startup;
            }

            settings::update_settings(current.clone())?;
            printer.print_value(&startup_payload(&current))?;
        }
    }

    Ok(())
}

fn handle_plugin(
    cmd: SettingsToggleCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        SettingsToggleCommands::Status => {
            printer.print_value(&plugin_status_payload()?)?;
        }
        SettingsToggleCommands::Enable => {
            let result = save_merged_settings(state, |settings| {
                settings.enable_claude_plugin_integration = true;
            })?;
            printer.print_value(&plugin_result_payload(result)?)?;
        }
        SettingsToggleCommands::Disable => {
            let result = save_merged_settings(state, |settings| {
                settings.enable_claude_plugin_integration = false;
            })?;
            printer.print_value(&plugin_result_payload(result)?)?;
        }
    }

    Ok(())
}

fn handle_onboarding(
    cmd: SettingsOnboardingCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        SettingsOnboardingCommands::Status => {
            printer.print_value(&onboarding_status_payload()?)?;
        }
        SettingsOnboardingCommands::Skip => {
            let result = save_merged_settings(state, |settings| {
                settings.skip_claude_onboarding = true;
            })?;
            printer.print_value(&onboarding_result_payload(result)?)?;
        }
        SettingsOnboardingCommands::Clear => {
            let result = save_merged_settings(state, |settings| {
                settings.skip_claude_onboarding = false;
            })?;
            printer.print_value(&onboarding_result_payload(result)?)?;
        }
    }

    Ok(())
}

fn save_merged_settings(
    state: &AppState,
    mutate: impl FnOnce(&mut AppSettings),
) -> anyhow::Result<SettingsSaveResult> {
    let mut next = settings::get_settings();
    mutate(&mut next);
    Ok(SettingsService::save_settings(state, next)?)
}

fn startup_payload(settings: &AppSettings) -> serde_json::Value {
    json!({
        "showInTray": settings.show_in_tray,
        "minimizeToTrayOnClose": settings.minimize_to_tray_on_close,
        "launchOnStartup": settings.launch_on_startup,
        "silentStartup": settings.silent_startup,
    })
}

fn plugin_status_payload() -> anyhow::Result<serde_json::Value> {
    let current = settings::get_settings();
    let status = ClaudePluginService::get_status()?;
    Ok(json!({
        "enabledInSettings": current.enable_claude_plugin_integration,
        "applied": ClaudePluginService::is_applied()?,
        "exists": status.exists,
        "path": status.path,
    }))
}

fn plugin_result_payload(result: SettingsSaveResult) -> anyhow::Result<serde_json::Value> {
    Ok(json!({
        "enabledInSettings": result.settings.enable_claude_plugin_integration,
        "applied": ClaudePluginService::is_applied()?,
        "warnings": result.warnings,
        "claudePluginSynced": result.claude_plugin_synced,
    }))
}

fn onboarding_status_payload() -> anyhow::Result<serde_json::Value> {
    let current = settings::get_settings();
    Ok(json!({
        "skipInSettings": current.skip_claude_onboarding,
        "applied": ClaudePluginService::is_onboarding_skip_applied()?,
    }))
}

fn onboarding_result_payload(result: SettingsSaveResult) -> anyhow::Result<serde_json::Value> {
    Ok(json!({
        "skipInSettings": result.settings.skip_claude_onboarding,
        "applied": ClaudePluginService::is_onboarding_skip_applied()?,
        "warnings": result.warnings,
        "claudeOnboardingSynced": result.claude_onboarding_synced,
    }))
}
