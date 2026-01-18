//! CLI module for headless provider switching
//!
//! Provides command-line interface for managing providers without launching the GUI.

mod args;
mod commands;
mod crud;
mod interactive;
mod tui;

pub use args::{Cli, CliCommand, CmdAction};
pub use commands::run_cli;
