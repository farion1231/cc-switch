//! DagExecutor — YAML-driven directed acyclic graph workflow execution.
//!
//! Workflows define a sequence of steps, each calling a model or aggregating.
//! Steps can depend on previous outputs, enabling multi-stage pipelines.

use crate::orchestration::model_caller::{ModelCaller, ModelResponse};
use crate::orchestration::workflow_lifecycle::{WorkflowLifecycle, WorkflowState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A single step in a DAG workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagStep {
    /// Unique step identifier.
    pub id: String,
    /// Model key to call, or "aggregate" for combining results.
    pub model: String,
    /// System prompt override (optional).
    pub system: Option<String>,
    /// User prompt template. `{step_N}` is replaced with previous step output.
    pub prompt: String,
    /// Dependencies: step IDs that must complete before this step runs.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Temperature override.
    #[serde(default = "default_temp")]
    pub temperature: f64,
}

fn default_temp() -> f64 {
    0.7
}

/// A DAG workflow definition loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagWorkflow {
    pub name: String,
    pub description: String,
    pub steps: Vec<DagStep>,
}

/// Result of executing a single DAG step.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub model: String,
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
}

/// Result of executing a full DAG workflow.
#[derive(Debug, Clone)]
pub struct DagResult {
    pub workflow_name: String,
    pub step_results: Vec<StepResult>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_latency_ms: u64,
    pub lifecycle: WorkflowLifecycle,
}

/// Executes DAG workflows by calling models through ModelCaller.
pub struct DagExecutor {
    caller: ModelCaller,
}

impl DagExecutor {
    pub fn new(caller: ModelCaller) -> Self {
        Self { caller }
    }

    /// Execute a DAG workflow. Steps are executed in dependency order.
    pub async fn execute(
        &self,
        workflow: &DagWorkflow,
        initial_input: &str,
        now_ms: u64,
    ) -> Result<DagResult, String> {
        let mut lifecycle = WorkflowLifecycle::new(now_ms);
        lifecycle.start_classifying(now_ms).ok();

        // Resolve execution order (topological sort by dependencies)
        let ordered = Self::topological_sort(&workflow.steps)?;

        lifecycle.start_executing(now_ms).ok();

        let mut outputs: HashMap<String, StepResult> = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut total_latency = 0u64;
        let start = std::time::Instant::now();

        for step in &ordered {
            // Build the prompt: substitute {step_X} placeholders
            let mut prompt = step.prompt.clone();
            prompt = prompt.replace("{input}", initial_input);
            for dep in &step.depends_on {
                if let Some(prev) = outputs.get(dep) {
                    prompt = prompt.replace(&format!("{{{}}}", dep), &prev.content);
                }
            }

            let system = step.system.clone().unwrap_or_default();
            let messages = if system.is_empty() {
                vec![serde_json::json!({"role": "user", "content": prompt})]
            } else {
                vec![
                    serde_json::json!({"role": "system", "content": system}),
                    serde_json::json!({"role": "user", "content": prompt}),
                ]
            };

            let resp = self
                .caller
                .call(&step.model, messages, None, Some(step.temperature))
                .await
                .map_err(|e| format!("Step '{}' failed: {}", step.id, e))?;

            total_input += resp.usage.input_tokens;
            total_output += resp.usage.output_tokens;
            total_latency += resp.latency_ms;

            let result = StepResult {
                step_id: step.id.clone(),
                model: resp.model.clone(),
                content: resp.content.clone(),
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
                latency_ms: resp.latency_ms,
            };
            outputs.insert(step.id.clone(), result);
        }

        lifecycle.start_verifying(now_ms).ok();
        lifecycle.complete(now_ms).ok();

        Ok(DagResult {
            workflow_name: workflow.name.clone(),
            step_results: outputs.into_values().collect(),
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            total_latency_ms: start.elapsed().as_millis() as u64,
            lifecycle,
        })
    }

    /// Simple topological sort: steps without dependencies first,
    /// then steps whose deps are satisfied.
    fn topological_sort(steps: &[DagStep]) -> Result<Vec<DagStep>, String> {
        let mut remaining: Vec<DagStep> = steps.to_vec();
        let mut sorted: Vec<DagStep> = Vec::new();
        let mut satisfied: Vec<String> = Vec::new();
        let mut last_len = remaining.len() + 1;

        while !remaining.is_empty() {
            if remaining.len() == last_len {
                return Err(format!(
                    "Circular dependency detected in steps: {}",
                    remaining
                        .iter()
                        .map(|s| s.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            last_len = remaining.len();

            let mut next_batch: Vec<DagStep> = Vec::new();
            remaining.retain(|step| {
                if step
                    .depends_on
                    .iter()
                    .all(|d| satisfied.contains(d))
                {
                    next_batch.push(step.clone());
                    false
                } else {
                    true
                }
            });
            for step in &next_batch {
                satisfied.push(step.id.clone());
            }
            sorted.extend(next_batch);
        }
        Ok(sorted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_sort_simple_chain() {
        let steps = vec![
            DagStep {
                id: "a".into(),
                model: "deepseek".into(),
                system: None,
                prompt: "step a".into(),
                depends_on: vec![],
                temperature: 0.7,
            },
            DagStep {
                id: "b".into(),
                model: "deepseek".into(),
                system: None,
                prompt: "step b with {a}".into(),
                depends_on: vec!["a".into()],
                temperature: 0.7,
            },
        ];
        let sorted = DagExecutor::topological_sort(&steps).unwrap();
        assert_eq!(sorted[0].id, "a");
        assert_eq!(sorted[1].id, "b");
    }

    #[test]
    fn topological_sort_detects_cycle() {
        let steps = vec![
            DagStep {
                id: "a".into(),
                model: "x".into(),
                system: None,
                prompt: "a".into(),
                depends_on: vec!["b".into()],
                temperature: 0.7,
            },
            DagStep {
                id: "b".into(),
                model: "x".into(),
                system: None,
                prompt: "b".into(),
                depends_on: vec!["a".into()],
                temperature: 0.7,
            },
        ];
        assert!(DagExecutor::topological_sort(&steps).is_err());
    }

    #[test]
    fn prompt_substitution_simple() {
        let mut prompt = "Review: {step_1} and {step_2}".to_string();
        let dep_outputs: HashMap<String, String> = [
            ("step_1".into(), "output A".into()),
            ("step_2".into(), "output B".into()),
        ]
        .into_iter()
        .collect();
        for (k, v) in &dep_outputs {
            prompt = prompt.replace(&format!("{{{}}}", k), v);
        }
        assert_eq!(prompt, "Review: output A and output B");
    }
}
