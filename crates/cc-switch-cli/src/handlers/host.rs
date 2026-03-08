use std::collections::HashMap;

use anyhow::{anyhow, bail};
use serde_json::json;

use crate::cli::AutoLaunchCommands;
use crate::output::Printer;
use cc_switch_core::{
    AutoLaunchService, HostService, RuntimeService, WslShellPreference,
};

const VALID_TOOLS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];

pub async fn handle_auto_launch(
    cmd: AutoLaunchCommands,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        AutoLaunchCommands::Status => {
            printer.print_value(&auto_launch_payload(AutoLaunchService::is_enabled()?)?)?;
        }
        AutoLaunchCommands::Enable => {
            AutoLaunchService::enable()?;
            HostService::set_launch_on_startup(true)?;
            printer.print_value(&auto_launch_payload(true)?)?;
        }
        AutoLaunchCommands::Disable => {
            AutoLaunchService::disable()?;
            HostService::set_launch_on_startup(false)?;
            printer.print_value(&auto_launch_payload(false)?)?;
        }
    }

    Ok(())
}

pub async fn handle_portable_mode(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&json!({
        "portableMode": RuntimeService::is_portable_mode()?,
    }))?;
    Ok(())
}

pub async fn handle_tool_versions(
    tools: Vec<String>,
    latest: bool,
    wsl_shell: Vec<String>,
    wsl_shell_flag: Vec<String>,
    printer: &Printer,
) -> anyhow::Result<()> {
    let tools = if tools.is_empty() { None } else { Some(tools) };
    let prefs = merge_wsl_preferences(wsl_shell, wsl_shell_flag)?;
    let prefs = if prefs.is_empty() { None } else { Some(prefs) };
    let versions = RuntimeService::get_tool_versions(tools, prefs, latest).await?;
    printer.print_value(&versions)?;
    Ok(())
}

pub async fn handle_about(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&RuntimeService::about()?)?;
    Ok(())
}

pub async fn handle_update_check(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&RuntimeService::check_for_updates().await?)?;
    Ok(())
}

pub async fn handle_release_notes(latest: bool, printer: &Printer) -> anyhow::Result<()> {
    let about = RuntimeService::about()?;
    let url = if latest {
        about.latest_release_url
    } else {
        about.current_release_notes_url
    };

    printer.print_value(&json!({ "url": url }))?;
    Ok(())
}

fn auto_launch_payload(enabled: bool) -> anyhow::Result<serde_json::Value> {
    let preferences = HostService::get_preferences()?;
    Ok(json!({
        "enabled": enabled,
        "launchOnStartup": preferences.launch_on_startup,
    }))
}

fn merge_wsl_preferences(
    wsl_shell: Vec<String>,
    wsl_shell_flag: Vec<String>,
) -> anyhow::Result<HashMap<String, WslShellPreference>> {
    let mut map = HashMap::new();

    for entry in wsl_shell {
        let (tool, value) = parse_tool_assignment(&entry, "wsl shell override")?;
        map.entry(tool)
            .or_insert_with(WslShellPreference::default)
            .wsl_shell = Some(value);
    }

    for entry in wsl_shell_flag {
        let (tool, value) = parse_tool_assignment(&entry, "wsl shell flag override")?;
        map.entry(tool)
            .or_insert_with(WslShellPreference::default)
            .wsl_shell_flag = Some(value);
    }

    Ok(map)
}

fn parse_tool_assignment(raw: &str, label: &str) -> anyhow::Result<(String, String)> {
    let (tool, value) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("{label} must look like <tool>=<value>"))?;
    let tool = tool.trim().to_lowercase();
    let value = value.trim().to_string();

    if !VALID_TOOLS.contains(&tool.as_str()) {
        bail!(
            "{label} uses unsupported tool '{}', expected one of: {}",
            tool,
            VALID_TOOLS.join(", ")
        );
    }

    if value.is_empty() {
        bail!("{label} value cannot be empty");
    }

    Ok((tool, value))
}

#[cfg(test)]
mod tests {
    use super::{merge_wsl_preferences, parse_tool_assignment};

    #[test]
    fn parse_tool_assignment_accepts_valid_entries() {
        let (tool, value) =
            parse_tool_assignment("claude=bash", "wsl shell override").expect("valid entry");
        assert_eq!(tool, "claude");
        assert_eq!(value, "bash");
    }

    #[test]
    fn parse_tool_assignment_rejects_invalid_entries() {
        let error =
            parse_tool_assignment("oops", "wsl shell override").expect_err("expected error");
        assert!(error.to_string().contains("<tool>=<value>"));
    }

    #[test]
    fn merge_wsl_preferences_combines_shell_and_flag() {
        let prefs = merge_wsl_preferences(
            vec!["claude=bash".to_string()],
            vec!["claude=-lc".to_string()],
        )
        .expect("valid prefs");
        let claude = prefs.get("claude").expect("claude pref");
        assert_eq!(claude.wsl_shell.as_deref(), Some("bash"));
        assert_eq!(claude.wsl_shell_flag.as_deref(), Some("-lc"));
    }
}
