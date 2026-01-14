//! CLI argument definitions using clap

use clap::{Parser, Subcommand};

/// CC-Switch: All-in-One Assistant for Claude Code, Codex & Gemini CLI
#[derive(Parser, Debug)]
#[command(name = "cc-switch")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Enter CLI mode for provider management (without launching GUI)
    #[command(disable_help_subcommand = true)]
    Cmd {
        #[command(subcommand)]
        action: Option<CmdAction>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CmdAction {
    /// Show current active provider status for all tools
    Status,

    /// List all providers for a specific tool
    List {
        /// Tool type: claude, codex, or gemini
        tool: String,
    },

    /// Switch provider for a specific tool (non-interactive)
    Switch {
        /// Tool type: claude, codex, or gemini
        tool: String,
        /// Provider name to switch to
        provider: String,
    },

    /// Show detailed help with examples
    Help,
}

/// Parse tool name to AppType
pub fn parse_tool_name(name: &str) -> Option<crate::app_config::AppType> {
    match name.to_lowercase().as_str() {
        "claude" => Some(crate::app_config::AppType::Claude),
        "codex" => Some(crate::app_config::AppType::Codex),
        "gemini" => Some(crate::app_config::AppType::Gemini),
        _ => None,
    }
}
