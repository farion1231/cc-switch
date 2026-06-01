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
        return Err(format!("base_url must use HTTPS, got '{}'", parsed.scheme()));
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
        return Err(format!("base_url points to internal/private address: '{}'", host));
    }

    Ok(())
}

/// Validate that `api_key_env` matches known-safe patterns.
/// Only allows env var names that look like API key variables.
pub fn validate_api_key_env(name: &str) -> Result<(), String> {
    let upper = name.to_uppercase();
    let allowed_suffixes = [
        "API_KEY", "API_SECRET", "TOKEN", "ACCESS_TOKEN",
        "SECRET_KEY", "SECRET", "KEY",
    ];
    let is_allowed = allowed_suffixes.iter().any(|suffix| upper.ends_with(suffix))
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
    /// Validate all model configs for security. Returns first error found.
    pub fn validate(&self) -> Result<(), String> {
        for (name, model) in &self.models {
            if let Err(e) = model.validate() {
                return Err(format!("model '{}': {}", name, e));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyDef {
    pub description: String,
    #[serde(default)]
    pub when: StrategyCondition,
    pub action: StrategyAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyCondition {
    pub complexity: Option<(f64, f64)>,
    pub risk: Option<Vec<String>>,
    pub task_type: Option<Vec<String>>,
    pub has_image: Option<bool>,
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
}

fn default_true() -> bool {
    true
}

fn default_threshold() -> f64 {
    0.65
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
                description: "Direct route to best model".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.0, 0.4)),
                    risk: Some(vec!["low".to_string()]),
                    ..Default::default()
                },
                action: StrategyAction::Route {
                    use_model: "cheap_coder".to_string(),
                    verify: false,
                },
            },
        );
        strategies.insert(
            "cascade".to_string(),
            StrategyDef {
                description: "Cheap first, verify, escalate".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.4, 0.7)),
                    risk: Some(vec!["medium".to_string(), "high".to_string()]),
                    ..Default::default()
                },
                action: StrategyAction::Cascade {
                    models: vec![
                        "cheap_coder".to_string(),
                        "frontier".to_string(),
                    ],
                    verify_each: true,
                    escalate_on_fail: true,
                    quality_threshold: 0.65,
                },
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
    fn default_config_has_models() {
        let config = OrchestrationConfig::default();
        assert!(!config.models.is_empty());
        assert!(config.models.contains_key("cheap_coder"));
        assert!(config.models.contains_key("frontier"));
    }

    #[test]
    fn default_config_has_strategies() {
        let config = OrchestrationConfig::default();
        assert!(config.strategies.contains_key("route"));
        assert!(config.strategies.contains_key("cascade"));
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
            StrategyAction::Cascade { models, quality_threshold, .. } => {
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
}
