use crate::orchestration::executor::ExecutionResult;
use crate::orchestration::history::{ModelCall, OrchestrationRecord, QualityScore};
use crate::orchestration::{HistoryStore, TaskProfile};

/// Trace ledger that persists orchestration executions onto the HistoryStore.
///
/// Wraps a HistoryStore and converts ExecutionResult + profile metadata into
/// an OrchestrationRecord row. PII constraint is preserved because the
/// underlying HistoryStore hashes/truncates raw prompts.
pub struct TraceLedger {
    store: HistoryStore,
}

/// A single observable step within an orchestration run.
///
/// This is the lightweight, transport-agnostic description that callers pass
/// in when they have granular per-step telemetry. When `steps` is empty the
/// ledger falls back to a single ModelCall derived from the ExecutionResult.
#[derive(Debug, Clone)]
pub struct TraceStep {
    pub step_type: String,
    pub model_key: String,
    pub provider: String,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub status: String,
    pub score: Option<f64>,
    pub error_kind: Option<String>,
}

impl TraceLedger {
    /// Open (or create) the trace ledger backed by a SQLite database at `path`.
    pub fn new(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            store: HistoryStore::new(path)?,
        })
    }

    /// Borrow the underlying HistoryStore for read queries (stats, lookups).
    pub fn store(&self) -> &HistoryStore {
        &self.store
    }

    /// Persist an orchestration execution as an OrchestrationRecord row.
    ///
    /// When `steps` is empty, a single ModelCall is synthesized from the
    /// aggregate ExecutionResult. When steps are provided, each step becomes
    /// its own ModelCall entry, preserving per-model telemetry.
    pub fn record_execution(
        &self,
        profile: &TaskProfile,
        raw_prompt: &str,
        result: &ExecutionResult,
        steps: Vec<TraceStep>,
    ) -> Result<String, String> {
        let mut record = OrchestrationRecord::new(
            &format!("{:?}", profile.task_type).to_ascii_lowercase(),
            profile.complexity,
            &format!("{:?}", profile.risk).to_ascii_lowercase(),
            raw_prompt,
            &result.strategy,
        );

        record.models_called = if steps.is_empty() {
            vec![ModelCall {
                model_key: result.model_used.clone(),
                provider: "unknown".to_string(),
                latency_ms: result.total_latency_ms,
                cost_usd: 0.0,
                quality_score: result.judge_score.unwrap_or(0.0),
                was_selected: true,
            }]
        } else {
            steps
                .iter()
                .map(|step| ModelCall {
                    model_key: step.model_key.clone(),
                    provider: step.provider.clone(),
                    latency_ms: step.latency_ms,
                    cost_usd: 0.0,
                    quality_score: step.score.unwrap_or(0.0),
                    was_selected: step.status == "selected",
                })
                .collect()
        };

        record.quality_scores = vec![QualityScore {
            tool_name: "judge".to_string(),
            score: result.judge_score.unwrap_or(0.0),
        }];
        record.final_quality = result.judge_score.unwrap_or(0.0);
        record.passed = result.verified;
        record.total_latency_ms = result.total_latency_ms;
        record.total_input_tokens = result.total_input_tokens;
        record.total_output_tokens = result.total_output_tokens;
        record.escalation_count = result.cascade_attempts.saturating_sub(1);

        let id = record.id.clone();
        self.store.record(&record)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskProfile, TaskType};
    use tempfile::TempDir;

    fn profile() -> TaskProfile {
        TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.8,
            risk: RiskLevel::High,
            verifiability: 0.9,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        }
    }

    #[test]
    fn trace_ledger_records_execution_result() {
        let dir = TempDir::new().unwrap();
        let ledger = TraceLedger::new(&dir.path().join("trace.db")).unwrap();
        let result = ExecutionResult {
            content: "answer".to_string(),
            model_used: "frontier".to_string(),
            strategy: "debate".to_string(),
            total_latency_ms: 1234,
            total_input_tokens: 100,
            total_output_tokens: 200,
            cascade_attempts: 3,
            verified: true,
            judge_score: Some(0.88),
        };

        let id = ledger
            .record_execution(&profile(), "raw prompt", &result, Vec::new())
            .unwrap();
        let stored = ledger.store().get_by_id(&id).unwrap().unwrap();

        assert_eq!(stored.strategy_used, "debate");
        assert_eq!(stored.final_quality, 0.88);
        assert!(stored.passed);
        assert_eq!(stored.total_latency_ms, 1234);
    }

    #[test]
    fn trace_ledger_synthesizes_single_model_call_when_no_steps() {
        let dir = TempDir::new().unwrap();
        let ledger = TraceLedger::new(&dir.path().join("trace.db")).unwrap();
        let result = ExecutionResult {
            content: "answer".to_string(),
            model_used: "frontier".to_string(),
            strategy: "route".to_string(),
            total_latency_ms: 500,
            total_input_tokens: 10,
            total_output_tokens: 20,
            cascade_attempts: 1,
            verified: false,
            judge_score: None,
        };

        let id = ledger
            .record_execution(&profile(), "raw prompt", &result, Vec::new())
            .unwrap();
        let stored = ledger.store().get_by_id(&id).unwrap().unwrap();

        assert_eq!(stored.models_called.len(), 1);
        assert_eq!(stored.models_called[0].model_key, "frontier");
        assert!(stored.models_called[0].was_selected);
        // judge_score None -> 0.0 default
        assert_eq!(stored.models_called[0].quality_score, 0.0);
        assert_eq!(stored.final_quality, 0.0);
        assert!(!stored.passed);
    }

    #[test]
    fn trace_ledger_records_each_step_as_model_call() {
        let dir = TempDir::new().unwrap();
        let ledger = TraceLedger::new(&dir.path().join("trace.db")).unwrap();
        let result = ExecutionResult {
            content: "final".to_string(),
            model_used: "judge-model".to_string(),
            strategy: "moa".to_string(),
            total_latency_ms: 2000,
            total_input_tokens: 300,
            total_output_tokens: 400,
            cascade_attempts: 3,
            verified: true,
            judge_score: Some(0.92),
        };

        let steps = vec![
            TraceStep {
                step_type: "propose".to_string(),
                model_key: "model-a".to_string(),
                provider: "openai".to_string(),
                latency_ms: 600,
                input_tokens: 100,
                output_tokens: 150,
                status: "candidate".to_string(),
                score: Some(0.75),
                error_kind: None,
            },
            TraceStep {
                step_type: "propose".to_string(),
                model_key: "model-b".to_string(),
                provider: "anthropic".to_string(),
                latency_ms: 700,
                input_tokens: 100,
                output_tokens: 160,
                status: "selected".to_string(),
                score: Some(0.91),
                error_kind: None,
            },
        ];

        let id = ledger
            .record_execution(&profile(), "raw prompt", &result, steps)
            .unwrap();
        let stored = ledger.store().get_by_id(&id).unwrap().unwrap();

        assert_eq!(stored.models_called.len(), 2);
        assert_eq!(stored.models_called[0].model_key, "model-a");
        assert!(!stored.models_called[0].was_selected); // status "candidate"
        assert_eq!(stored.models_called[0].quality_score, 0.75);
        assert_eq!(stored.models_called[1].model_key, "model-b");
        assert!(stored.models_called[1].was_selected); // status "selected"
        assert_eq!(stored.models_called[1].provider, "anthropic");
        // escalation_count = cascade_attempts - 1
        assert_eq!(stored.escalation_count, 2);
    }

    #[test]
    fn trace_ledger_redacts_prompt_via_history_store() {
        let dir = TempDir::new().unwrap();
        let ledger = TraceLedger::new(&dir.path().join("trace.db")).unwrap();
        let result = ExecutionResult {
            content: "out".to_string(),
            model_used: "m".to_string(),
            strategy: "route".to_string(),
            total_latency_ms: 1,
            total_input_tokens: 1,
            total_output_tokens: 1,
            cascade_attempts: 1,
            verified: true,
            judge_score: Some(0.5),
        };

        // Pass a prompt containing an API-key-like token; it must be redacted.
        let raw_prompt = "Use sk-abc123def456ghi789jkl012mno345 for the request";
        let id = ledger
            .record_execution(&profile(), raw_prompt, &result, Vec::new())
            .unwrap();
        let stored = ledger.store().get_by_id(&id).unwrap().unwrap();

        assert!(
            !stored.prompt_summary.contains("sk-abc123def456ghi789jkl012mno345"),
            "prompt_summary must not contain raw secret, got: {}",
            stored.prompt_summary
        );
        assert!(stored.prompt_summary.contains("[REDACTED]"));
    }
}
