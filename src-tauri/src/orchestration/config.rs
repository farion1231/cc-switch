use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    16384
}

/// Check if host falls in the 172.16.0.0/12 private range (172.16.x.x – 172.31.x.x).
fn is_172_private(host: &str) -> bool {
    let rest = match host.strip_prefix("172.") {
        Some(r) => r,
        None => return false,
    };
    let second_octet = match rest.split('.').next() {
        Some(s) => s,
        None => return false,
    };
    match second_octet.parse::<u8>() {
        Ok(octet) => octet >= 16 && octet <= 31,
        Err(_) => false,
    }
}

/// Validate that `base_url` (if present) is not an internal/private address.
/// Rejects: loopback, link-local, private IP ranges, and non-HTTPS schemes.
pub fn validate_base_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL '{}': {}", url, e))?;

    if parsed.scheme() != "https" {
        return Err(format!(
            "base_url must use HTTPS, got '{}'",
            parsed.scheme()
        ));
    }

    let host = parsed.host_str().unwrap_or("");
    // Reject obvious internal addresses
    if host.is_empty()
        || host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host.starts_with("192.168.")
        || host.starts_with("10.")
        || is_172_private(host)
        || host.starts_with("169.254.")
        || host.ends_with(".internal")
        || host.ends_with(".local")
        || host.ends_with(".localhost")
    {
        return Err(format!(
            "base_url points to internal/private address: '{}'",
            host
        ));
    }

    Ok(())
}

/// Validate that `api_key_env` matches known-safe patterns.
/// Only allows env var names that look like API key variables.
pub fn validate_api_key_env(name: &str) -> Result<(), String> {
    let upper = name.to_uppercase();
    let allowed_suffixes = [
        "API_KEY",
        "API_SECRET",
        "TOKEN",
        "ACCESS_TOKEN",
        "SECRET_KEY",
        "SECRET",
        "KEY",
    ];
    let is_allowed = allowed_suffixes
        .iter()
        .any(|suffix| upper.ends_with(suffix))
        || upper.contains("API_KEY")
        || upper.contains("API_SECRET");

    if !is_allowed {
        return Err(format!(
            "api_key_env '{}' is not allowed. Must end with one of: API_KEY, API_SECRET, TOKEN, SECRET_KEY, KEY",
            name
        ));
    }

    Ok(())
}

impl ModelConfig {
    /// Validate all fields, returning errors for security violations.
    pub fn validate(&self) -> Result<(), String> {
        validate_api_key_env(&self.api_key_env)?;
        if let Some(ref url) = self.base_url {
            validate_base_url(url)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    pub enabled: bool,
    #[serde(default)]
    pub models: HashMap<String, ModelConfig>,
    pub strategies: HashMap<String, StrategyDef>,
}

impl OrchestrationConfig {
    /// Validate model configs and strategy references. Returns first error found.
    pub fn validate(&self) -> Result<(), String> {
        for (name, model) in &self.models {
            if let Err(e) = model.validate() {
                return Err(format!("model '{}': {}", name, e));
            }
        }

        for (strategy_name, strategy) in &self.strategies {
            match &strategy.action {
                StrategyAction::Route { use_model, .. } => {
                    if !self.models.contains_key(use_model) {
                        return Err(format!(
                            "strategy '{}' references undefined model '{}'",
                            strategy_name, use_model
                        ));
                    }
                }
                StrategyAction::Cascade { models, .. } => {
                    for model_key in models {
                        if !self.models.contains_key(model_key) {
                            return Err(format!(
                                "strategy '{}' references undefined model '{}'",
                                strategy_name, model_key
                            ));
                        }
                    }
                }
                StrategyAction::Debate {
                    debaters, judge, ..
                } => {
                    for model_key in debaters {
                        if !self.models.contains_key(model_key) {
                            return Err(format!(
                                "strategy '{}' references undefined debater '{}'",
                                strategy_name, model_key
                            ));
                        }
                    }
                    if !self.models.contains_key(judge) {
                        return Err(format!(
                            "strategy '{}' references undefined judge '{}'",
                            strategy_name, judge
                        ));
                    }
                }
                StrategyAction::MoA {
                    proposers, aggregator, ..
                } => {
                    for model_key in proposers {
                        if !self.models.contains_key(model_key) {
                            return Err(format!(
                                "strategy '{}' references undefined proposer '{}'",
                                strategy_name, model_key
                            ));
                        }
                    }
                    if !self.models.contains_key(aggregator) {
                        return Err(format!(
                            "strategy '{}' references undefined aggregator '{}'",
                            strategy_name, aggregator
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyDef {
    #[serde(default)]
    pub priority: i32,
    pub description: String,
    #[serde(default)]
    pub when: StrategyCondition,
    #[serde(default)]
    pub budgets: StrategyBudgets,
    pub action: StrategyAction,
    #[serde(default)]
    pub fallback: FallbackPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyCondition {
    pub complexity: Option<(f64, f64)>,
    pub risk: Option<Vec<String>>,
    pub task_type: Option<Vec<String>>,
    pub has_image: Option<bool>,
    pub has_audio: Option<bool>,
    pub has_tools: Option<bool>,
    pub is_streaming: Option<bool>,
    pub modalities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyBudgets {
    pub max_calls: Option<u32>,
    pub max_latency_ms: Option<u64>,
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FallbackPolicy {
    pub on_quality_fail: Option<String>,
    pub on_judge_fail: Option<String>,
    pub on_provider_fail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyAction {
    Route {
        use_model: String,
        #[serde(default)]
        verify: bool,
    },
    Cascade {
        models: Vec<String>,
        #[serde(default = "default_true")]
        verify_each: bool,
        #[serde(default = "default_true")]
        escalate_on_fail: bool,
        #[serde(default = "default_threshold")]
        quality_threshold: f64,
    },
    Debate {
        debaters: Vec<String>,
        judge: String,
        #[serde(default = "default_debate_rounds")]
        max_rounds: u32,
        #[serde(default = "default_true")]
        critique: bool,
        #[serde(default = "default_true")]
        revision: bool,
        #[serde(default = "default_threshold")]
        quality_threshold: f64,
    },
    #[serde(rename = "moa")]
    MoA {
        proposers: Vec<String>,
        aggregator: String,
        #[serde(default = "default_true")]
        verify_each: bool,
        #[serde(default = "default_threshold")]
        quality_threshold: f64,
    },
}

fn default_true() -> bool {
    true
}

fn default_threshold() -> f64 {
    0.65
}

fn default_debate_rounds() -> u32 {
    1
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        let mut models = HashMap::new();
        models.insert(
            "cheap_coder".to_string(),
            ModelConfig {
                provider: "deepseek".to_string(),
                model: "deepseek-chat".to_string(),
                api_key_env: "DEEPSEEK_API_KEY".to_string(),
                base_url: None,
                max_tokens: 16384,
            },
        );
        models.insert(
            "qwen_coder".to_string(),
            ModelConfig {
                provider: "qwen".to_string(),
                model: "qwen-plus".to_string(),
                api_key_env: "QWEN_API_KEY".to_string(),
                base_url: None,
                max_tokens: 16384,
            },
        );
        models.insert(
            "glm_coder".to_string(),
            ModelConfig {
                provider: "glm".to_string(),
                model: "glm-4-flash".to_string(),
                api_key_env: "GLM_API_KEY".to_string(),
                base_url: None,
                max_tokens: 16384,
            },
        );
        models.insert(
            "frontier".to_string(),
            ModelConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
                base_url: None,
                max_tokens: 16384,
            },
        );

        let mut strategies = HashMap::new();
        strategies.insert(
            "route".to_string(),
            StrategyDef {
                priority: 10,
                description: "Direct route to best model".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.0, 0.4)),
                    risk: Some(vec!["low".to_string()]),
                    ..Default::default()
                },
                budgets: StrategyBudgets::default(),
                action: StrategyAction::Route {
                    use_model: "cheap_coder".to_string(),
                    verify: false,
                },
                fallback: FallbackPolicy::default(),
            },
        );
        strategies.insert(
            "cascade".to_string(),
            StrategyDef {
                priority: 40,
                description: "Cheap first, verify, escalate".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.4, 0.7)),
                    risk: Some(vec!["medium".to_string(), "high".to_string()]),
                    ..Default::default()
                },
                budgets: StrategyBudgets::default(),
                action: StrategyAction::Cascade {
                    models: vec!["cheap_coder".to_string(), "frontier".to_string()],
                    verify_each: true,
                    escalate_on_fail: true,
                    quality_threshold: 0.65,
                },
                fallback: FallbackPolicy::default(),
            },
        );
        strategies.insert(
            "debate".to_string(),
            StrategyDef {
                priority: 70,
                description: "Multi-model debate with judge arbitration".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.7, 0.9)),
                    risk: Some(vec!["high".to_string(), "critical".to_string()]),
                    ..Default::default()
                },
                budgets: StrategyBudgets::default(),
                action: StrategyAction::Debate {
                    debaters: vec![
                        "cheap_coder".to_string(),
                        "qwen_coder".to_string(),
                        "glm_coder".to_string(),
                    ],
                    judge: "frontier".to_string(),
                    max_rounds: 1,
                    critique: true,
                    revision: true,
                    quality_threshold: 0.7,
                },
                fallback: FallbackPolicy::default(),
            },
        );
        strategies.insert(
            "moa".to_string(),
            StrategyDef {
                priority: 90,
                description: "Mixture of Agents — propose then aggregate".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.9, 1.0)),
                    risk: Some(vec!["critical".to_string()]),
                    ..Default::default()
                },
                budgets: StrategyBudgets::default(),
                action: StrategyAction::MoA {
                    proposers: vec![
                        "cheap_coder".to_string(),
                        "qwen_coder".to_string(),
                        "glm_coder".to_string(),
                        "frontier".to_string(),
                    ],
                    aggregator: "frontier".to_string(),
                    verify_each: true,
                    quality_threshold: 0.75,
                },
                fallback: FallbackPolicy::default(),
            },
        );
        Self {
            enabled: false,
            models,
            strategies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_def_deserializes_release_fields() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  debate_high:
    priority: 80
    description: "Structured debate"
    when:
      complexity: [0.7, 1.0]
      risk: ["high", "critical"]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    budgets:
      max_calls: 6
      max_latency_ms: 60000
      max_cost_usd: 0.50
    action:
      type: debate
      debaters: ["cheap_reasoner", "mid_reasoner"]
      judge: "frontier_judge"
      max_rounds: 1
      critique: true
      revision: true
      quality_threshold: 0.8
    fallback:
      on_quality_fail: "frontier_single"
      on_judge_fail: "backup_judge"
      on_provider_fail: "passthrough"
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let strategy = config.strategies.get("debate_high").unwrap();

        assert_eq!(strategy.priority, 80);
        assert_eq!(strategy.when.has_tools, Some(false));
        assert_eq!(strategy.when.is_streaming, Some(false));
        assert_eq!(strategy.when.modalities, Some(vec!["text".to_string()]));
        assert_eq!(strategy.budgets.max_calls, Some(6));
        assert_eq!(strategy.budgets.max_latency_ms, Some(60000));
        assert_eq!(strategy.budgets.max_cost_usd, Some(0.50));
        assert_eq!(strategy.fallback.on_quality_fail.as_deref(), Some("frontier_single"));

        match &strategy.action {
            StrategyAction::Debate {
                max_rounds,
                critique,
                revision,
                quality_threshold,
                ..
            } => {
                assert_eq!(*max_rounds, 1);
                assert!(*critique);
                assert!(*revision);
                assert!((*quality_threshold - 0.8).abs() < f64::EPSILON);
            }
            other => panic!("expected Debate action, got {other:?}"),
        }
    }

    #[test]
    fn old_strategy_yaml_still_deserializes_with_defaults() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  route:
    description: "Old route shape"
    when: {}
    action:
      type: route
      use_model: cheap_coder
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let strategy = config.strategies.get("route").unwrap();

        assert_eq!(strategy.priority, 0);
        assert_eq!(strategy.budgets.max_calls, None);
        assert_eq!(strategy.fallback.on_quality_fail, None);
    }

    #[test]
    fn default_config_has_models() {
        let config = OrchestrationConfig::default();
        assert!(!config.models.is_empty());
        assert!(config.models.contains_key("cheap_coder"));
        assert!(config.models.contains_key("qwen_coder"));
        assert!(config.models.contains_key("glm_coder"));
        assert!(config.models.contains_key("frontier"));
    }

    #[test]
    fn default_config_has_strategies() {
        let config = OrchestrationConfig::default();
        assert!(config.strategies.contains_key("route"));
        assert!(config.strategies.contains_key("cascade"));
        assert!(config.strategies.contains_key("debate"));
        assert!(config.strategies.contains_key("moa"));
    }

    #[test]
    fn model_config_fields() {
        let config = OrchestrationConfig::default();
        let cheap = &config.models["cheap_coder"];
        assert_eq!(cheap.provider, "deepseek");
        assert_eq!(cheap.model, "deepseek-chat");
        assert_eq!(cheap.api_key_env, "DEEPSEEK_API_KEY");
    }

    #[test]
    fn strategy_action_route_serde() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  test:
    description: "Test"
    when: {}
    action:
      type: route
      use_model: my_model
      verify: true
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.strategies["test"].action {
            StrategyAction::Route { use_model, verify } => {
                assert_eq!(use_model, "my_model");
                assert!(*verify);
            }
            other => panic!("Expected Route, got {:?}", other),
        }
    }

    #[test]
    fn strategy_action_cascade_serde() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  test:
    description: "Test"
    when: {}
    action:
      type: cascade
      models: ["a", "b"]
      quality_threshold: 0.8
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.strategies["test"].action {
            StrategyAction::Cascade {
                models,
                quality_threshold,
                ..
            } => {
                assert_eq!(models, &vec!["a".to_string(), "b".to_string()]);
                assert!((*quality_threshold - 0.8).abs() < 0.001);
            }
            other => panic!("Expected Cascade, got {:?}", other),
        }
    }

    #[test]
    fn config_roundtrip() {
        let original = OrchestrationConfig::default();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let parsed: OrchestrationConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.enabled, original.enabled);
        assert_eq!(parsed.models.len(), original.models.len());
        assert_eq!(parsed.strategies.len(), original.strategies.len());
    }

    #[test]
    fn validate_rejects_route_model_that_is_not_defined() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  bad:
    description: "Bad route"
    when: {}
    action:
      type: route
      use_model: missing_model
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("missing_model"));
    }

    #[test]
    fn validate_rejects_cascade_model_that_is_not_defined() {
        let yaml = r#"
enabled: true
models:
  present:
    provider: deepseek
    model: deepseek-chat
    api_key_env: DEEPSEEK_API_KEY
strategies:
  bad:
    description: "Bad cascade"
    when: {}
    action:
      type: cascade
      models: [present, missing_model]
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("missing_model"));
    }

    #[test]
    fn default_config_has_debate_strategy() {
        let config = OrchestrationConfig::default();
        assert!(
            config.strategies.contains_key("debate"),
            "Default config should include debate strategy"
        );
        match &config.strategies["debate"].action {
            StrategyAction::Debate {
                debaters,
                judge,
                quality_threshold,
                ..
            } => {
                assert!(
                    debaters.len() >= 2,
                    "Debate needs at least 2 debaters"
                );
                assert!(!judge.is_empty(), "Debate needs a judge");
                assert!(
                    (*quality_threshold - 0.7).abs() < 0.001,
                    "Debate quality threshold should be 0.7"
                );
            }
            other => panic!("Expected Debate action, got {:?}", other),
        }
    }

    #[test]
    fn default_config_has_moa_strategy() {
        let config = OrchestrationConfig::default();
        assert!(
            config.strategies.contains_key("moa"),
            "Default config should include MoA strategy"
        );
        match &config.strategies["moa"].action {
            StrategyAction::MoA {
                proposers,
                aggregator,
                verify_each,
                quality_threshold,
            } => {
                assert!(
                    proposers.len() >= 2,
                    "MoA needs at least 2 proposers"
                );
                assert!(
                    !aggregator.is_empty(),
                    "MoA needs an aggregator"
                );
                assert!(
                    *verify_each,
                    "MoA should verify each proposer"
                );
                assert!(
                    (*quality_threshold - 0.75).abs() < 0.001,
                    "MoA quality threshold should be 0.75"
                );
            }
            other => panic!("Expected MoA action, got {:?}", other),
        }
    }
}
