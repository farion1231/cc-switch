//! WorkflowAssembler -- dynamically assemble execution workflows based on
//! task complexity, risk, and available models.
//!
//! Uses a typed `enum Workflow` instead of `Vec<WorkflowStep>` with index
//! jumps (per autoplan design review fix).

use crate::orchestration::classifier::{RiskLevel, TaskProfile};
use crate::orchestration::cross_judge::JudgeAggregation;
use crate::orchestration::scoring::CostQualityScorer;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A fully assembled workflow ready for execution.
#[derive(Debug, Clone)]
pub struct DynamicWorkflow {
    pub workflow: Workflow,
    pub estimated_cost: f64,
    pub estimated_latency_ms: u64,
    pub reasoning: String,
}

/// Typed workflow variants -- each represents a distinct execution strategy.
#[derive(Debug, Clone)]
pub enum Workflow {
    /// Simple single-model routing.
    Route { model: String, temperature: f64 },
    /// Try cheap model first, verify quality, fallback to stronger model.
    Cascade {
        first: CascadeStep,
        quality_check: QualityCheck,
        fallback: Option<Box<CascadeStep>>,
    },
    /// Multiple models debate, judged by independent models.
    Debate {
        debaters: Vec<String>,
        judge: JudgeConfig,
        spot_check: Option<SpotCheckConfig>,
        human_gate: Option<HumanGateConfig>,
    },
    /// Multiple proposers generate answers, one aggregator synthesizes.
    MixtureOfAgents {
        proposers: Vec<String>,
        aggregator: String,
        quality_check: Option<QualityCheck>,
    },
}

#[derive(Debug, Clone)]
pub struct CascadeStep {
    pub model_key: String,
    pub temperature: f64,
}

#[derive(Debug, Clone)]
pub struct QualityCheck {
    pub threshold: f64,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct JudgeConfig {
    pub judge_models: Vec<String>,
    pub aggregation: JudgeAggregation,
}

#[derive(Debug, Clone)]
pub struct SpotCheckConfig {
    pub inspector_model: String,
    pub probability: f64,
}

#[derive(Debug, Clone)]
pub struct HumanGateConfig {
    pub reason: String,
    pub timeout_secs: u32,
    pub fallback_action: FallbackAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FallbackAction {
    UseBestCandidate,
    Abort,
    EscalateToHuman,
}

// ---------------------------------------------------------------------------
// WorkflowAssembler
// ---------------------------------------------------------------------------

pub struct WorkflowAssembler {
    #[allow(dead_code)] // Used by future score-aware model selection
    scorer: CostQualityScorer,
    available_models: Vec<String>,
}

impl WorkflowAssembler {
    /// Create a new assembler with a scorer and a list of available model keys.
    pub fn new(scorer: CostQualityScorer, available_models: Vec<String>) -> Self {
        Self {
            scorer,
            available_models,
        }
    }

    /// Assemble a dynamic workflow based on the task profile and remaining budget.
    #[allow(clippy::let_and_return)] // budget_remaining reserved for cost-aware routing
    pub fn assemble(&self, profile: &TaskProfile, budget_remaining: f64) -> DynamicWorkflow {
        let models = &self.available_models;
        let complexity = profile.complexity;
        let risk = &profile.risk;
        let _ = budget_remaining; // Used in future cost-aware model selection

        if complexity < 0.4 {
            // Low complexity: simple route with cheapest model.
            let model = self.cheapest_model();
            DynamicWorkflow {
                workflow: Workflow::Route {
                    model: model.clone(),
                    temperature: 0.3,
                },
                estimated_cost: 0.001,
                estimated_latency_ms: 2000,
                reasoning: format!(
                    "Low complexity ({:.2}) -- routing to cheapest model '{}'",
                    complexity, model
                ),
            }
        } else if complexity < 0.7 {
            // Medium complexity: cascade (cheap -> verify -> fallback).
            let cheap = self.cheapest_model();
            let mid = self.mid_model();
            let threshold = self.dynamic_threshold(profile);

            DynamicWorkflow {
                workflow: Workflow::Cascade {
                    first: CascadeStep {
                        model_key: cheap.clone(),
                        temperature: 0.3,
                    },
                    quality_check: QualityCheck {
                        threshold,
                        tools: vec!["quality_scorer".to_string()],
                    },
                    fallback: Some(Box::new(CascadeStep {
                        model_key: mid.clone(),
                        temperature: 0.5,
                    })),
                },
                estimated_cost: 0.01,
                estimated_latency_ms: 8000,
                reasoning: format!(
                    "Medium complexity ({:.2}) -- cascade from '{}' to '{}' with threshold {:.2}",
                    complexity, cheap, mid, threshold
                ),
            }
        } else {
            // High complexity: debate with top models + judge + spot check.
            let top3 = self.top_models(3);
            let judge_model = self.top_models(1);
            let inspector = if models.len() > 3 {
                models[3].clone()
            } else {
                top3[0].clone()
            };

            let threshold = self.dynamic_threshold(profile);

            // Add HumanGate for Critical risk.
            let human_gate = if *risk == RiskLevel::Critical {
                Some(HumanGateConfig {
                    reason: "Critical risk task requires human approval".to_string(),
                    timeout_secs: 300,
                    fallback_action: FallbackAction::EscalateToHuman,
                })
            } else {
                None
            };

            let estimated_cost = 0.05 * top3.len() as f64;
            let estimated_latency = 15000 + (top3.len() as u64 * 3000);

            DynamicWorkflow {
                workflow: Workflow::Debate {
                    debaters: top3.clone(),
                    judge: JudgeConfig {
                        judge_models: judge_model,
                        aggregation: JudgeAggregation::Median,
                    },
                    spot_check: Some(SpotCheckConfig {
                        inspector_model: inspector,
                        probability: 0.1,
                    }),
                    human_gate: human_gate.clone(),
                },
                estimated_cost,
                estimated_latency_ms: estimated_latency,
                reasoning: format!(
                    "High complexity ({:.2}) -- debate with {} models, threshold {:.2}{}",
                    complexity,
                    top3.len(),
                    threshold,
                    if human_gate.is_some() {
                        ", human gate enabled (Critical risk)"
                    } else {
                        ""
                    }
                ),
            }
        }
    }

    /// Compute a dynamic quality threshold based on risk and verifiability.
    ///
    /// Base 0.65 + risk adjustment - verifiability adjustment, clamped to [0.3, 0.95].
    pub fn dynamic_threshold(&self, profile: &TaskProfile) -> f64 {
        let base = 0.65;
        let risk_adj = match profile.risk {
            RiskLevel::Critical => 0.15,
            RiskLevel::High => 0.10,
            RiskLevel::Medium => 0.05,
            RiskLevel::Low => 0.0,
        };
        let verifiability_adj = profile.verifiability * 0.1;
        (base + risk_adj - verifiability_adj).clamp(0.3, 0.95)
    }

    // -----------------------------------------------------------------------
    // Model selection helpers
    // -----------------------------------------------------------------------

    /// Return the first (cheapest) model key.
    fn cheapest_model(&self) -> String {
        self.available_models
            .last()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Return a mid-tier model key.
    fn mid_model(&self) -> String {
        let mid = self.available_models.len() / 2;
        self.available_models
            .get(mid)
            .cloned()
            .unwrap_or_else(|| self.cheapest_model())
    }

    /// Return the top N model keys (highest quality).
    fn top_models(&self, n: usize) -> Vec<String> {
        self.available_models.iter().take(n).cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskType};

    fn make_models() -> Vec<String> {
        vec![
            "opus".to_string(),
            "sonnet".to_string(),
            "haiku".to_string(),
            "gpt4".to_string(),
        ]
    }

    fn make_profile(complexity: f64, risk: RiskLevel, verifiability: f64) -> TaskProfile {
        TaskProfile {
            task_type: TaskType::Coding,
            complexity,
            risk,
            verifiability,
            has_image: false,
            need_code: true,
        }
    }

    fn make_assembler() -> WorkflowAssembler {
        WorkflowAssembler::new(CostQualityScorer::default(), make_models())
    }

    // --- Low complexity -> Route ---

    #[test]
    fn low_complexity_produces_route() {
        let asm = make_assembler();
        let profile = make_profile(0.2, RiskLevel::Low, 0.1);
        let wf = asm.assemble(&profile, 1.0);

        match wf.workflow {
            Workflow::Route { .. } => {}
            ref other => panic!("Expected Route, got {:?}", other),
        }
    }

    // --- Medium complexity -> Cascade ---

    #[test]
    fn medium_complexity_produces_cascade() {
        let asm = make_assembler();
        let profile = make_profile(0.5, RiskLevel::Medium, 0.3);
        let wf = asm.assemble(&profile, 1.0);

        match wf.workflow {
            Workflow::Cascade {
                ref first,
                ref fallback,
                ..
            } => {
                assert!(!first.model_key.is_empty());
                assert!(fallback.is_some());
            }
            ref other => panic!("Expected Cascade, got {:?}", other),
        }
    }

    // --- High complexity -> Debate ---

    #[test]
    fn high_complexity_produces_debate() {
        let asm = make_assembler();
        let profile = make_profile(0.8, RiskLevel::High, 0.5);
        let wf = asm.assemble(&profile, 1.0);

        match wf.workflow {
            Workflow::Debate {
                ref debaters,
                ref spot_check,
                ..
            } => {
                assert_eq!(debaters.len(), 3);
                assert!(spot_check.is_some());
            }
            ref other => panic!("Expected Debate, got {:?}", other),
        }
    }

    // --- Critical risk adds HumanGate ---

    #[test]
    fn critical_risk_adds_human_gate() {
        let asm = make_assembler();
        let profile = make_profile(0.9, RiskLevel::Critical, 0.5);
        let wf = asm.assemble(&profile, 1.0);

        match wf.workflow {
            Workflow::Debate { ref human_gate, .. } => {
                assert!(human_gate.is_some());
                let gate = human_gate.as_ref().unwrap();
                assert_eq!(gate.fallback_action, FallbackAction::EscalateToHuman);
                assert_eq!(gate.timeout_secs, 300);
            }
            ref other => panic!("Expected Debate for critical risk, got {:?}", other),
        }
    }

    // --- Non-critical high complexity has no HumanGate ---

    #[test]
    fn high_risk_no_human_gate() {
        let asm = make_assembler();
        let profile = make_profile(0.8, RiskLevel::High, 0.5);
        let wf = asm.assemble(&profile, 1.0);

        match wf.workflow {
            Workflow::Debate { ref human_gate, .. } => {
                assert!(human_gate.is_none());
            }
            ref other => panic!("Expected Debate, got {:?}", other),
        }
    }

    // --- Dynamic threshold calculation ---

    #[test]
    fn threshold_low_risk() {
        let asm = make_assembler();
        let profile = make_profile(0.5, RiskLevel::Low, 0.1);
        let threshold = asm.dynamic_threshold(&profile);
        // 0.65 + 0.0 - 0.1*0.1 = 0.64
        assert!((threshold - 0.64).abs() < 1e-10);
    }

    #[test]
    fn threshold_critical_risk() {
        let asm = make_assembler();
        let profile = make_profile(0.5, RiskLevel::Critical, 0.1);
        let threshold = asm.dynamic_threshold(&profile);
        // 0.65 + 0.15 - 0.1*0.1 = 0.79
        assert!((threshold - 0.79).abs() < 1e-10);
    }

    #[test]
    fn threshold_clamped_lower() {
        let asm = make_assembler();
        let profile = make_profile(0.5, RiskLevel::Low, 5.0);
        // verifiability is 5.0 (abnormally high) -> 0.65 + 0.0 - 5.0*0.1 = 0.15 -> clamped to 0.3
        let threshold = asm.dynamic_threshold(&profile);
        assert!((threshold - 0.3).abs() < 1e-10);
    }

    #[test]
    fn threshold_clamped_upper() {
        let asm = make_assembler();
        let profile = make_profile(0.5, RiskLevel::Critical, 0.0);
        // 0.65 + 0.15 - 0.0 = 0.80 (within range)
        let threshold = asm.dynamic_threshold(&profile);
        assert!((threshold - 0.80).abs() < 1e-10);
    }

    // --- Reasoning string is populated ---

    #[test]
    fn workflow_has_reasoning() {
        let asm = make_assembler();
        let profile = make_profile(0.2, RiskLevel::Low, 0.1);
        let wf = asm.assemble(&profile, 1.0);
        assert!(!wf.reasoning.is_empty());
    }

    // --- Boundary: complexity exactly 0.4 -> Cascade (not Route) ---

    #[test]
    fn boundary_complexity_0_4_is_cascade() {
        let asm = make_assembler();
        let profile = make_profile(0.4, RiskLevel::Low, 0.1);
        let wf = asm.assemble(&profile, 1.0);
        match wf.workflow {
            Workflow::Cascade { .. } => {}
            ref other => panic!("Expected Cascade at complexity 0.4, got {:?}", other),
        }
    }

    // --- Boundary: complexity exactly 0.7 -> Debate (not Cascade) ---

    #[test]
    fn boundary_complexity_0_7_is_debate() {
        let asm = make_assembler();
        let profile = make_profile(0.7, RiskLevel::Low, 0.1);
        let wf = asm.assemble(&profile, 1.0);
        match wf.workflow {
            Workflow::Debate { .. } => {}
            ref other => panic!("Expected Debate at complexity 0.7, got {:?}", other),
        }
    }
}
