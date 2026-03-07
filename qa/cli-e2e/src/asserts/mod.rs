use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::Path;

use crate::sandbox::CommandOutput;

pub fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        bail!("{}", message.into())
    }
}

pub fn stdout_json(output: &CommandOutput) -> Result<Value> {
    serde_json::from_str(output.stdout.trim())
        .with_context(|| format!("stdout is not valid JSON:\n{}", output.stdout))
}

pub fn read_json(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON file {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("invalid JSON in {}", path.display()))
}

pub fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read text file {}", path.display()))
}

pub fn assert_contains(haystack: &str, needle: &str, context: &str) -> Result<()> {
    ensure(
        haystack.contains(needle),
        format!("{context}: expected to find '{needle}'"),
    )
}

pub fn assert_not_contains(haystack: &str, needle: &str, context: &str) -> Result<()> {
    ensure(
        !haystack.contains(needle),
        format!("{context}: did not expect to find '{needle}'"),
    )
}
