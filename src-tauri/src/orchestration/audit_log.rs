use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::orchestration::classifier::TaskProfile;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// JSONL-format audit logger for orchestration events.
///
/// Each call to `log` appends one JSON line to the configured file path.
/// The parent directory is created on first write if it does not exist.
pub struct AuditLogger {
    log_path: PathBuf,
}

/// A single audit log entry serialized as one JSON line.
#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub event_type: AuditEventType,
    pub request_id: String,
    pub details: Value,
}

/// Types of events that can be recorded in the audit log.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    RequestReceived,
    TaskClassified,
    StrategySelected,
    ModelCalled,
    QualityVerified,
    Escalation,
    ResponseReturned,
    Error,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl AuditLogger {
    pub fn new(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    /// Append one JSON line to the audit log file.
    ///
    /// Creates the parent directory if it does not exist.  Opens the file in
    /// append mode so that concurrent or sequential writes are safe.
    pub fn log(
        &self,
        event_type: AuditEventType,
        request_id: &str,
        details: Value,
    ) -> Result<(), String> {
        let entry = AuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            event_type,
            request_id: request_id.to_string(),
            details,
        };

        let json_line = serde_json::to_string(&entry)
            .map_err(|e| format!("failed to serialize audit entry: {}", e))?;

        // Ensure parent directory exists.
        if let Some(parent) = self.log_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create audit log directory: {}", e))?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| format!("failed to open audit log file: {}", e))?;

        writeln!(file, "{}", json_line)
            .map_err(|e| format!("failed to write audit log entry: {}", e))?;

        Ok(())
    }

    /// Convenience: log that a request was received and classified.
    pub fn log_request(&self, classification: &TaskProfile, request_id: &str) {
        let details = serde_json::json!({
            "task_type": format!("{:?}", classification.task_type),
            "complexity": classification.complexity,
            "risk": format!("{:?}", classification.risk),
            "verifiability": classification.verifiability,
            "has_image": classification.has_image,
            "need_code": classification.need_code,
        });

        // Best-effort: log both events.  Errors are logged, not propagated.
        if let Err(e) = self.log(AuditEventType::RequestReceived, request_id, details.clone()) {
            log::warn!("[AuditLogger] {}", e);
        }
        if let Err(e) = self.log(AuditEventType::TaskClassified, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }

    /// Convenience: log the selected strategy.
    pub fn log_strategy(&self, strategy_name: &str, request_id: &str) {
        let details = serde_json::json!({
            "strategy": strategy_name,
        });
        if let Err(e) = self.log(AuditEventType::StrategySelected, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }

    /// Convenience: log a model call with latency and cost.
    pub fn log_model_call(&self, model: &str, latency_ms: u64, cost_usd: f64, request_id: &str) {
        let details = serde_json::json!({
            "model": model,
            "latency_ms": latency_ms,
            "cost_usd": cost_usd,
        });
        if let Err(e) = self.log(AuditEventType::ModelCalled, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }

    /// Convenience: log a quality verification result.
    pub fn log_quality(&self, score: f64, passed: bool, request_id: &str) {
        let details = serde_json::json!({
            "score": score,
            "passed": passed,
        });
        if let Err(e) = self.log(AuditEventType::QualityVerified, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }

    /// Convenience: log an escalation event.
    pub fn log_escalation(&self, from: &str, to: &str, reason: &str, request_id: &str) {
        let details = serde_json::json!({
            "from_model": from,
            "to_model": to,
            "reason": reason,
        });
        if let Err(e) = self.log(AuditEventType::Escalation, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }

    /// Convenience: log the final response returned to the caller.
    pub fn log_response(
        &self,
        model: &str,
        total_latency_ms: u64,
        total_cost_usd: f64,
        request_id: &str,
    ) {
        let details = serde_json::json!({
            "model": model,
            "total_latency_ms": total_latency_ms,
            "total_cost_usd": total_cost_usd,
        });
        if let Err(e) = self.log(AuditEventType::ResponseReturned, request_id, details) {
            log::warn!("[AuditLogger] {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskType};
    use std::fs;
    use tempfile::TempDir;

    fn make_logger(dir: &TempDir) -> AuditLogger {
        let path = dir.path().join("audit.jsonl");
        AuditLogger::new(path)
    }

    fn read_lines(path: &PathBuf) -> Vec<String> {
        let content = fs::read_to_string(path).expect("read audit file");
        content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    }

    // ---- Log entry written as valid JSON line ----

    #[test]
    fn log_entry_is_valid_json_line() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);

        logger
            .log(
                AuditEventType::RequestReceived,
                "req-1",
                serde_json::json!({"key": "value"}),
            )
            .unwrap();

        let lines = read_lines(&logger.log_path);
        assert_eq!(lines.len(), 1, "Should have exactly one line");

        let parsed: serde_json::Value =
            serde_json::from_str(&lines[0]).expect("Line should be valid JSON");
        assert_eq!(parsed["event_type"], "request_received");
        assert_eq!(parsed["request_id"], "req-1");
        assert_eq!(parsed["details"]["key"], "value");
        assert!(
            parsed["timestamp"].as_str().is_some(),
            "timestamp should be a string",
        );
    }

    // ---- Multiple entries are appended (not overwritten) ----

    #[test]
    fn multiple_entries_appended() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);

        logger
            .log(
                AuditEventType::RequestReceived,
                "req-1",
                serde_json::json!({"n": 1}),
            )
            .unwrap();
        logger
            .log(
                AuditEventType::StrategySelected,
                "req-1",
                serde_json::json!({"n": 2}),
            )
            .unwrap();
        logger
            .log(
                AuditEventType::ResponseReturned,
                "req-1",
                serde_json::json!({"n": 3}),
            )
            .unwrap();

        let lines = read_lines(&logger.log_path);
        assert_eq!(lines.len(), 3, "Should have three lines");

        let first: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        let third: serde_json::Value = serde_json::from_str(&lines[2]).unwrap();

        assert_eq!(first["event_type"], "request_received");
        assert_eq!(second["event_type"], "strategy_selected");
        assert_eq!(third["event_type"], "response_returned");
    }

    // ---- Convenience methods produce correct event_type ----

    #[test]
    fn convenience_method_event_types() {
        let dir = TempDir::new().unwrap();
        let logger = make_logger(&dir);

        let profile = TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.5,
            risk: RiskLevel::Low,
            verifiability: 0.8,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };

        logger.log_request(&profile, "req-c");
        logger.log_strategy("react", "req-c");
        logger.log_model_call("sonnet", 1200, 0.003, "req-c");
        logger.log_quality(0.85, true, "req-c");
        logger.log_escalation("haiku", "sonnet", "low score", "req-c");
        logger.log_response("sonnet", 2400, 0.006, "req-c");

        let lines = read_lines(&logger.log_path);
        assert_eq!(
            lines.len(),
            7,
            "Should have 7 lines (request=2 + 5 convenience)"
        );

        let types: Vec<String> = lines
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["event_type"].as_str().unwrap().to_string()
            })
            .collect();

        assert_eq!(types[0], "request_received");
        assert_eq!(types[1], "task_classified");
        assert_eq!(types[2], "strategy_selected");
        assert_eq!(types[3], "model_called");
        assert_eq!(types[4], "quality_verified");
        assert_eq!(types[5], "escalation");
        assert_eq!(types[6], "response_returned");
    }

    // ---- Directory created if missing ----

    #[test]
    fn directory_created_if_missing() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("deep").join("nested").join("dir");
        let path = nested.join("audit.jsonl");

        let logger = AuditLogger::new(path);
        logger
            .log(
                AuditEventType::Error,
                "req-d",
                serde_json::json!({"msg": "test"}),
            )
            .unwrap();

        assert!(nested.exists(), "Parent directory should be created");
        let lines = read_lines(&logger.log_path);
        assert_eq!(lines.len(), 1);
    }

    // ---- AuditEntry serializes correctly ----

    #[test]
    fn audit_entry_serializes_correctly() {
        let entry = AuditEntry {
            timestamp: "2026-01-15T12:00:00+00:00".to_string(),
            event_type: AuditEventType::ModelCalled,
            request_id: "req-ser".to_string(),
            details: serde_json::json!({"model": "opus", "latency_ms": 500}),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["timestamp"], "2026-01-15T12:00:00+00:00");
        assert_eq!(parsed["event_type"], "model_called");
        assert_eq!(parsed["request_id"], "req-ser");
        assert_eq!(parsed["details"]["model"], "opus");
        assert_eq!(parsed["details"]["latency_ms"], 500);
    }

    // ---- All event types serialize with snake_case ----

    #[test]
    fn all_event_types_snake_case() {
        let cases = vec![
            (AuditEventType::RequestReceived, "request_received"),
            (AuditEventType::TaskClassified, "task_classified"),
            (AuditEventType::StrategySelected, "strategy_selected"),
            (AuditEventType::ModelCalled, "model_called"),
            (AuditEventType::QualityVerified, "quality_verified"),
            (AuditEventType::Escalation, "escalation"),
            (AuditEventType::ResponseReturned, "response_returned"),
            (AuditEventType::Error, "error"),
        ];

        for (event_type, expected) in cases {
            let json = serde_json::to_string(&event_type).unwrap();
            assert_eq!(
                json.trim_matches('"'),
                expected,
                "{:?} should serialize as {}",
                event_type,
                expected,
            );
        }
    }
}
