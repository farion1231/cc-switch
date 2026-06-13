use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub expected_strategy: String,
    pub min_quality: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub case_id: String,
    pub strategy_used: String,
    pub quality_score: f64,
    pub passed: bool,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub struct MiniEvalHarness {
    cases: Vec<EvalCase>,
}

impl MiniEvalHarness {
    pub fn default_cases() -> Vec<EvalCase> {
        vec![
            EvalCase {
                id: "simple_text_route".to_string(),
                name: "Simple text should route cheaply".to_string(),
                prompt: "Explain what HTTP status 404 means.".to_string(),
                expected_strategy: "route".to_string(),
                min_quality: 0.60,
            },
            EvalCase {
                id: "coding_cascade".to_string(),
                name: "Coding task should allow cascade".to_string(),
                prompt: "Write a Rust function that validates balanced parentheses.".to_string(),
                expected_strategy: "cascade".to_string(),
                min_quality: 0.70,
            },
            EvalCase {
                id: "high_risk_debate".to_string(),
                name: "High-risk architecture task should use debate".to_string(),
                prompt: "Design a payment retry system with idempotency and failure recovery.".to_string(),
                expected_strategy: "debate".to_string(),
                min_quality: 0.80,
            },
            EvalCase {
                id: "critical_moa".to_string(),
                name: "Critical complex synthesis should use MoA".to_string(),
                prompt: "Compare three database migration strategies and choose the safest rollout plan.".to_string(),
                expected_strategy: "moa".to_string(),
                min_quality: 0.82,
            },
        ]
    }

    pub fn new(cases: Vec<EvalCase>) -> Self {
        Self { cases }
    }

    pub fn cases(&self) -> &[EvalCase] {
        &self.cases
    }

    pub fn summarize(results: &[EvalResult]) -> EvalSummary {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let avg_quality = if total == 0 {
            0.0
        } else {
            results.iter().map(|r| r.quality_score).sum::<f64>() / total as f64
        };
        EvalSummary {
            total,
            passed,
            pass_rate: if total == 0 { 0.0 } else { passed as f64 / total as f64 },
            avg_quality,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total: usize,
    pub passed: usize,
    pub pass_rate: f64,
    pub avg_quality: f64,
}
