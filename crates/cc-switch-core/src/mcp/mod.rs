//! MCP configuration handling module
//!
//! This module provides utilities for reading/writing MCP configurations
//! for different AI coding clients.

pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;
pub mod validation;

pub use validation::validate_mcp_config;
