use crate::orchestration::classifier::{TaskClassifier, TaskProfile};
use crate::orchestration::config::StrategyAction;
use crate::orchestration::executor::{ExecutionResult, StrategyExecutor};
use crate::orchestration::loader::StrategyLoader;
use serde_json::Value;
use std::path::PathBuf;

pub struct OrchestrationEngine {
    loader: StrategyLoader,
    executor: Option<StrategyExecutor>,
}

#[derive(Debug)]
pub enum OrchestrationDecision {
    Passthrough,
    Route { model: String },
    Cascade { models: Vec<String>, quality_threshold: f64 },
}

impl OrchestrationEngine {
    pub fn new(config_path: PathBuf) -> Self {
        let loader = StrategyLoader::new(config_path);
        Self {
            loader,
            executor: None,
        }
    }

    pub fn with_executor(config_path: PathBuf, executor: StrategyExecutor) -> Self {
        let loader = StrategyLoader::new(config_path);
        Self {
            loader,
            executor: Some(executor),
        }
    }

    pub async fn decide(&self, body: &Value) -> OrchestrationDecision {
        let config = self.loader.get_config().await;

        if !config.enabled {
            return OrchestrationDecision::Passthrough;
        }

        let profile = TaskClassifier::classify(body);

        let Some((strategy_name, action)) = crate::orchestration::selector::StrategySelector::select(&profile, &config) else {
            log::info!(
                "[Orchestration] No strategy matched for task_type={:?} complexity={:.2} risk={:?}, passthrough",
                profile.task_type,
                profile.complexity,
                profile.risk
            );
            return OrchestrationDecision::Passthrough;
        };

        log::info!(
            "[Orchestration] Strategy '{}' selected for task_type={:?} complexity={:.2} risk={:?}",
            strategy_name,
            profile.task_type,
            profile.complexity,
            profile.risk
        );

        match action {
            StrategyAction::Route { use_model, .. } => {
                log::info!("[Orchestration] ROUTE → {}", use_model);
                OrchestrationDecision::Route { model: use_model }
            }
            StrategyAction::Cascade {
                models,
                quality_threshold,
                ..
            } => {
                log::info!(
                    "[Orchestration] CASCADE → {} (threshold={})",
                    models.join(" → "),
                    quality_threshold
                );
                OrchestrationDecision::Cascade {
                    models,
                    quality_threshold,
                }
            }
        }
    }

    pub async fn execute(
        &self,
        decision: &OrchestrationDecision,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        match &self.executor {
            Some(exec) => exec.execute(decision, messages, tools).await,
            None => Err("StrategyExecutor not initialized".to_string()),
        }
    }

    pub async fn decide_and_execute(
        &self,
        body: &Value,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> OrchestrationOutcome {
        let decision = self.decide(body).await;

        if matches!(decision, OrchestrationDecision::Passthrough) {
            return OrchestrationOutcome::Passthrough;
        }

        match self.execute(&decision, messages, tools).await {
            Ok(result) => OrchestrationOutcome::Executed {
                decision,
                result,
            },
            Err(e) => OrchestrationOutcome::Fallback {
                reason: e,
                decision,
            },
        }
    }

    pub async fn reload_config(&self) -> Result<(), String> {
        self.loader.reload().await
    }

    pub async fn is_enabled(&self) -> bool {
        self.loader.get_config().await.enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.loader.set_enabled(enabled);
    }
}

#[derive(Debug)]
pub enum OrchestrationOutcome {
    Passthrough,
    Executed {
        decision: OrchestrationDecision,
        result: ExecutionResult,
    },
    Fallback {
        reason: String,
        decision: OrchestrationDecision,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn create_engine_with_yaml(yaml_content: &str) -> (OrchestrationEngine, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("strategies.yaml");
        std::fs::write(&path, yaml_content).unwrap();
        (OrchestrationEngine::new(path), dir)
    }

    #[tokio::test]
    async fn disabled_passthrough() {
        let (engine, _dir) = create_engine_with_yaml("enabled: false\nstrategies: {}\n");
        let body = json!({"messages": [{"role": "user", "content": "hello"}]});
        let decision = engine.decide(&body).await;
        assert!(matches!(decision, OrchestrationDecision::Passthrough));
    }

    #[tokio::test]
    async fn route_simple_task() {
        let yaml = r#"
enabled: true
strategies:
  route:
    description: "Direct route"
    when:
      complexity: [0, 0.4]
      risk: ["low"]
    action:
      type: route
      use_model: cheap_coder
      verify: false
"#;
        let (engine, _dir) = create_engine_with_yaml(yaml);
        let body = json!({
            "messages": [{"role": "user", "content": "what is 2+2?"}],
            "model": "claude-opus-4"
        });
        let decision = engine.decide(&body).await;
        match decision {
            OrchestrationDecision::Route { model } => assert_eq!(model, "cheap_coder"),
            other => panic!("Expected Route, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_without_executor_returns_error() {
        let (engine, _dir) = create_engine_with_yaml("enabled: true\nstrategies: {}\n");
        let decision = OrchestrationDecision::Route {
            model: "test".to_string(),
        };
        let result = engine.execute(&decision, vec![], None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }
}
