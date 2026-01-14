//! CLI module for headless provider switching
//!
//! Provides command-line interface for managing providers without launching the GUI.

mod args;
mod commands;
mod interactive;

pub use args::{Cli, CliCommand};
pub use commands::run_cli;
