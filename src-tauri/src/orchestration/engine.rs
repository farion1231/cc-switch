use crate::orchestration::classifier::{TaskClassifier, TaskProfile};
use crate::orchestration::config::StrategyAction;
use crate::orchestration::executor::{ExecutionResult, StrategyExecutor};
use crate::orchestration::health_checker::ModelHealthChecker;
use crate::orchestration::loader::StrategyLoader;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct OrchestrationEngine {
    loader: StrategyLoader,
    executor: Option<StrategyExecutor>,
    health_checker: Mutex<Option<ModelHealthChecker>>,
}

#[derive(Debug)]
pub enum OrchestrationDecision {
    Passthrough,
    Route {
        model: String,
    },
    Cascade {
        models: Vec<String>,
        quality_threshold: f64,
    },
    Debate {
        debaters: Vec<String>,
        judge: String,
        quality_threshold: f64,
        max_rounds: u32,
        critique: bool,
        revision: bool,
    },
    MoA {
        proposers: Vec<String>,
        aggregator: String,
        quality_threshold: f64,
    },
}

impl OrchestrationEngine {
    pub fn new(config_path: PathBuf) -> Self {
        let loader = StrategyLoader::new(config_path);
        Self {
            loader,
            executor: None,
            health_checker: Mutex::new(None),
        }
    }

    pub fn with_executor(config_path: PathBuf, executor: StrategyExecutor) -> Self {
        let loader = StrategyLoader::new(config_path);
        Self {
            loader,
            executor: Some(executor),
            health_checker: Mutex::new(None),
        }
    }

    /// Enable health-aware routing. Call this after construction with a list of known model keys.
    pub fn enable_health_checks(&self, model_keys: &[String]) {
        *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) = Some(ModelHealthChecker::new(model_keys));
    }

    /// Report the result of a model invocation so the health checker can react.
    pub fn report_model_result(&self, model_key: &str, success: bool, latency_ms: u64) {
        if let Some(ref mut hc) = *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) {
            hc.update_health(model_key, success, latency_ms);
        }
    }

    pub async fn decide(&self, body: &Value) -> OrchestrationDecision {
        let config = self.loader.get_config().await;

        if !config.enabled {
            return OrchestrationDecision::Passthrough;
        }

        let profile = TaskClassifier::classify(body);

        let Some((strategy_name, action)) =
            crate::orchestration::selector::StrategySelector::select(&profile, &config)
        else {
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

        let health_filter = |models: Vec<String>| -> Vec<String> {
            if let Some(ref hc) = *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) {
                let filtered: Vec<String> = models
                    .into_iter()
                    .filter(|m| {
                        let ok = hc.is_available(m);
                        if !ok {
                            log::warn!("[Orchestration] Skipping unhealthy model: {}", m);
                        }
                        ok
                    })
                    .collect();
                if filtered.is_empty() {
                    log::warn!(
                        "[Orchestration] All models unhealthy for strategy '{}', passthrough",
                        strategy_name
                    );
                }
                filtered
            } else {
                models
            }
        };

        match action {
            StrategyAction::Route { use_model, .. } => {
                let ok = if let Some(ref hc) = *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) {
                    hc.is_available(&use_model)
                } else {
                    true
                };
                if !ok {
                    log::warn!(
                        "[Orchestration] ROUTE model '{}' is unhealthy, passthrough",
                        use_model
                    );
                    return OrchestrationDecision::Passthrough;
                }
                log::info!("[Orchestration] ROUTE → {}", use_model);
                OrchestrationDecision::Route { model: use_model }
            }
            StrategyAction::Cascade {
                models,
                quality_threshold,
                ..
            } => {
                let healthy = health_filter(models);
                if healthy.is_empty() {
                    return OrchestrationDecision::Passthrough;
                }
                log::info!(
                    "[Orchestration] CASCADE → {} (threshold={})",
                    healthy.join(" → "),
                    quality_threshold
                );
                OrchestrationDecision::Cascade {
                    models: healthy,
                    quality_threshold,
                }
            }
            StrategyAction::Debate {
                debaters,
                judge,
                quality_threshold,
                max_rounds,
                critique,
                revision,
            } => {
                let healthy_debaters = health_filter(debaters);
                if healthy_debaters.len() < 2 {
                    log::warn!(
                        "[Orchestration] DEBATE needs >=2 healthy debaters, only {} available, passthrough",
                        healthy_debaters.len()
                    );
                    return OrchestrationDecision::Passthrough;
                }
                let judge_ok = if let Some(ref hc) = *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) {
                    hc.is_available(&judge)
                } else {
                    true
                };
                if !judge_ok {
                    log::warn!("[Orchestration] DEBATE judge '{}' is unhealthy, passthrough", judge);
                    return OrchestrationDecision::Passthrough;
                }
                log::info!(
                    "[Orchestration] DEBATE — debaters=[{}], judge={}, threshold={:.2}",
                    healthy_debaters.join(", "),
                    judge,
                    quality_threshold
                );
                OrchestrationDecision::Debate {
                    debaters: healthy_debaters,
                    judge,
                    quality_threshold,
                    max_rounds,
                    critique,
                    revision,
                }
            }
            StrategyAction::MoA {
                proposers,
                aggregator,
                quality_threshold,
                ..
            } => {
                let healthy_proposers = health_filter(proposers);
                if healthy_proposers.len() < 2 {
                    log::warn!(
                        "[Orchestration] MoA needs >=2 healthy proposers, only {} available, passthrough",
                        healthy_proposers.len()
                    );
                    return OrchestrationDecision::Passthrough;
                }
                let agg_ok = if let Some(ref hc) = *self.health_checker.lock().unwrap_or_else(|e| e.into_inner()) {
                    hc.is_available(&aggregator)
                } else {
                    true
                };
                if !agg_ok {
                    log::warn!(
                        "[Orchestration] MoA aggregator '{}' is unhealthy, passthrough",
                        aggregator
                    );
                    return OrchestrationDecision::Passthrough;
                }
                log::info!(
                    "[Orchestration] MoA — proposers=[{}], aggregator={}",
                    healthy_proposers.join(", "),
                    aggregator
                );
                OrchestrationDecision::MoA {
                    proposers: healthy_proposers,
                    aggregator,
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
            Ok(result) => OrchestrationOutcome::Executed { decision, result },
            Err(e) => OrchestrationOutcome::Fallback {
                reason: e,
                decision,
            },
        }
    }

    pub async fn get_config(&self) -> crate::orchestration::config::OrchestrationConfig {
        self.loader.get_config().await
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

    pub async fn persist_enabled(&self, enabled: bool) -> Result<(), String> {
        self.loader.persist_enabled(enabled)
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
models:
  cheap_coder:
    provider: deepseek
    model: deepseek-chat
    api_key_env: DEEPSEEK_API_KEY
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

    #[tokio::test]
    async fn debate_decision_carries_quality_threshold() {
        let yaml = r#"
enabled: true
models:
  a: { provider: deepseek, model: deepseek-chat, api_key_env: DEEPSEEK_API_KEY }
  b: { provider: qwen, model: qwen-plus, api_key_env: QWEN_API_KEY }
  judge: { provider: anthropic, model: claude-sonnet-4-20250514, api_key_env: ANTHROPIC_API_KEY }
strategies:
  debate:
    priority: 10
    description: "Debate"
    when:
      complexity: [0.0, 1.0]
    action:
      type: debate
      debaters: [a, b]
      judge: judge
      quality_threshold: 0.83
"#;
        let (engine, _dir) = create_engine_with_yaml(yaml);
        let body = json!({"messages": [{"role": "user", "content": "design a compiler"}]});
        let decision = engine.decide(&body).await;

        match decision {
            OrchestrationDecision::Debate { quality_threshold, .. } => {
                assert!((quality_threshold - 0.83).abs() < f64::EPSILON);
            }
            other => panic!("expected Debate, got {other:?}"),
        }
    }
}
