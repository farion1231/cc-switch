use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub id: String,
    pub label: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub suggestion: Option<String>,
    pub details: Option<Value>,
}

impl DiagnosticCheck {
    pub fn ok(id: &str, label: &str, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Ok,
            message: message.into(),
            suggestion: None,
            details: None,
        }
    }

    pub fn warning(
        id: &str,
        label: &str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Warning,
            message: message.into(),
            suggestion: Some(suggestion.into()),
            details: None,
        }
    }

    pub fn error(
        id: &str,
        label: &str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Error,
            message: message.into(),
            suggestion: Some(suggestion.into()),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(redact_value(&details));
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub generated_at: String,
    pub checks: Vec<DiagnosticCheck>,
    pub summary: DiagnosticSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSummary {
    pub ok: usize,
    pub warnings: usize,
    pub errors: usize,
    pub degraded_agent_gateway: bool,
}

impl DiagnosticReport {
    pub fn new(checks: Vec<DiagnosticCheck>) -> Self {
        let ok = checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Ok)
            .count();
        let warnings = checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Warning)
            .count();
        let errors = checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Error)
            .count();
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            checks,
            summary: DiagnosticSummary {
                ok,
                warnings,
                errors,
                degraded_agent_gateway: errors > 0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticError {
    pub code: String,
    pub message: String,
    pub suggestion: String,
    pub details: Option<String>,
}

impl DiagnosticError {
    pub fn new(code: &str, message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            suggestion: suggestion.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(redact_text(&details.into()));
        self
    }
}

pub fn export_zip(
    output_dir: &Path,
    report: &DiagnosticReport,
    extra_files: BTreeMap<&str, String>,
) -> Result<PathBuf, DiagnosticError> {
    std::fs::create_dir_all(output_dir).map_err(|e| {
        DiagnosticError::new(
            "DIAGNOSTIC_EXPORT_FAILED",
            "Unable to create diagnostic export directory.",
            "Choose a writable application data directory and retry.",
        )
        .with_details(e.to_string())
    })?;
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = output_dir.join(format!("cc-switch-agent-diagnostic-{timestamp}.zip"));
    let file = File::create(&path).map_err(|e| {
        DiagnosticError::new(
            "DIAGNOSTIC_EXPORT_FAILED",
            "Unable to create diagnostic report zip.",
            "Check write permissions and retry.",
        )
        .with_details(e.to_string())
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let diagnostics = serde_json::to_string_pretty(&redact_value(&json!(report))).map_err(|e| {
        DiagnosticError::new(
            "DIAGNOSTIC_EXPORT_FAILED",
            "Unable to serialize diagnostics.",
            "Retry the export.",
        )
        .with_details(e.to_string())
    })?;
    zip.start_file("diagnostics.json", options)
        .map_err(zip_error)?;
    zip.write_all(diagnostics.as_bytes()).map_err(io_error)?;

    for (name, content) in extra_files {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(redact_text(&content).as_bytes())
            .map_err(io_error)?;
    }

    zip.finish().map_err(zip_error)?;
    Ok(path)
}

pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if is_secret_key(key) {
                        (key.clone(), Value::String("[redacted]".to_string()))
                    } else {
                        (key.clone(), redact_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

pub fn redact_text(value: &str) -> String {
    let mut redacted = value.to_string();
    for marker in [
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "cookie",
        "session",
        "token",
        "secret",
    ] {
        redacted = redacted.replace(marker, "[redacted]");
        redacted = redacted.replace(&marker.to_ascii_uppercase(), "[redacted]");
    }
    redacted
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("session")
        || lower.contains("secret")
}

fn io_error(error: std::io::Error) -> DiagnosticError {
    DiagnosticError::new(
        "DIAGNOSTIC_EXPORT_FAILED",
        "Unable to write diagnostic report.",
        "Check write permissions and retry.",
    )
    .with_details(error.to_string())
}

fn zip_error(error: zip::result::ZipError) -> DiagnosticError {
    DiagnosticError::new(
        "DIAGNOSTIC_EXPORT_FAILED",
        "Unable to package diagnostic report.",
        "Retry the export.",
    )
    .with_details(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_nested_secret_values() {
        let value = json!({ "headers": { "Authorization": "Bearer secret" } });
        assert_eq!(
            redact_value(&value)["headers"]["Authorization"],
            "[redacted]"
        );
    }
}
