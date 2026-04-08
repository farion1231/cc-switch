//! OpenClaw command handlers

use std::collections::HashMap;
use std::fs;

use anyhow::Context;
use serde::de::DeserializeOwned;

use crate::cli::{OpenClawCommands, OpenClawConfigCommands};
use crate::output::Printer;
use cc_switch_core::{
    openclaw_config, OpenClawAgentsDefaults, OpenClawDefaultModel, OpenClawEnvConfig,
    OpenClawModelCatalogEntry, OpenClawToolsConfig,
};

pub async fn handle(cmd: OpenClawCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawCommands::Env { cmd } => handle_env(cmd, printer),
        OpenClawCommands::Tools { cmd } => handle_tools(cmd, printer),
        OpenClawCommands::AgentsDefaults { cmd } => handle_agents_defaults(cmd, printer),
        OpenClawCommands::DefaultModel { cmd } => handle_default_model(cmd, printer),
        OpenClawCommands::ModelCatalog { cmd } => handle_model_catalog(cmd, printer),
    }
}

fn handle_env(cmd: OpenClawConfigCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawConfigCommands::Get => printer.print_value(&openclaw_config::get_env_config()?),
        OpenClawConfigCommands::Set { file, value } => {
            let env = parse_json_input::<OpenClawEnvConfig>(
                "openclaw env set",
                file.as_deref(),
                value.as_deref(),
            )?;
            openclaw_config::set_env_config(&env)?;
            printer.print_value(&serde_json::json!({
                "saved": true,
                "section": "env",
            }))
        }
    }
}

fn handle_tools(cmd: OpenClawConfigCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawConfigCommands::Get => printer.print_value(&openclaw_config::get_tools_config()?),
        OpenClawConfigCommands::Set { file, value } => {
            let tools = parse_json_input::<OpenClawToolsConfig>(
                "openclaw tools set",
                file.as_deref(),
                value.as_deref(),
            )?;
            openclaw_config::set_tools_config(&tools)?;
            printer.print_value(&serde_json::json!({
                "saved": true,
                "section": "tools",
            }))
        }
    }
}

fn handle_agents_defaults(cmd: OpenClawConfigCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawConfigCommands::Get => {
            printer.print_value(&openclaw_config::get_agents_defaults()?)
        }
        OpenClawConfigCommands::Set { file, value } => {
            let defaults = parse_json_input::<OpenClawAgentsDefaults>(
                "openclaw agents-defaults set",
                file.as_deref(),
                value.as_deref(),
            )?;
            openclaw_config::set_agents_defaults(&defaults)?;
            printer.print_value(&serde_json::json!({
                "saved": true,
                "section": "agentsDefaults",
            }))
        }
    }
}

fn handle_default_model(cmd: OpenClawConfigCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawConfigCommands::Get => printer.print_value(&openclaw_config::get_default_model()?),
        OpenClawConfigCommands::Set { file, value } => {
            let model = parse_json_input::<OpenClawDefaultModel>(
                "openclaw default-model set",
                file.as_deref(),
                value.as_deref(),
            )?;
            openclaw_config::set_default_model(&model)?;
            printer.print_value(&serde_json::json!({
                "saved": true,
                "section": "defaultModel",
            }))
        }
    }
}

fn handle_model_catalog(cmd: OpenClawConfigCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        OpenClawConfigCommands::Get => printer.print_value(&openclaw_config::get_model_catalog()?),
        OpenClawConfigCommands::Set { file, value } => {
            let catalog = parse_json_input::<HashMap<String, OpenClawModelCatalogEntry>>(
                "openclaw model-catalog set",
                file.as_deref(),
                value.as_deref(),
            )?;
            openclaw_config::set_model_catalog(&catalog)?;
            printer.print_value(&serde_json::json!({
                "saved": true,
                "section": "modelCatalog",
            }))
        }
    }
}

fn parse_json_input<T>(label: &str, file: Option<&str>, value: Option<&str>) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let raw = match (file, value) {
        (Some(path), None) => fs::read_to_string(path)
            .with_context(|| format!("{label} failed to read file: {path}"))?,
        (None, Some(value)) => value.to_string(),
        _ => anyhow::bail!("{label} requires either --file or --value"),
    };

    serde_json::from_str(&raw).with_context(|| format!("{label} expects valid JSON"))
}
