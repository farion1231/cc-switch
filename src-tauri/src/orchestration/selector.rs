use crate::orchestration::classifier::{RiskLevel, TaskProfile, TaskType};
use crate::orchestration::config::{OrchestrationConfig, StrategyAction};

pub struct StrategySelector;

impl StrategySelector {
    pub fn select(
        profile: &TaskProfile,
        config: &OrchestrationConfig,
    ) -> Option<(String, StrategyAction)> {
        let mut best_match: Option<(&String, f64)> = None;

        for (name, def) in &config.strategies {
            let score = Self::match_score(profile, &def.when);
            if score > 0.0 {
                match best_match {
                    Some((_, best_score)) if score <= best_score => {}
                    _ => best_match = Some((name, score)),
                }
            }
        }

        best_match.map(|(name, _)| (name.clone(), config.strategies[name].action.clone()))
    }

    fn match_score(
        profile: &TaskProfile,
        condition: &crate::orchestration::config::StrategyCondition,
    ) -> f64 {
        let mut score = 0.0;
        let mut total_weight = 0.0;

        if let Some((lo, hi)) = &condition.complexity {
            total_weight += 1.0;
            if profile.complexity >= *lo && profile.complexity <= *hi {
                score += 1.0;
            }
        }

        if let Some(risks) = &condition.risk {
            total_weight += 1.0;
            let risk_str = match &profile.risk {
                RiskLevel::Low => "low",
                RiskLevel::Medium => "medium",
                RiskLevel::High => "high",
                RiskLevel::Critical => "critical",
            };
            if risks.iter().any(|r| r == risk_str) {
                score += 1.0;
            }
        }

        if let Some(task_types) = &condition.task_type {
            total_weight += 1.0;
            let type_str = match &profile.task_type {
                TaskType::Coding => "coding",
                TaskType::Architecture => "architecture",
                TaskType::Summary => "summary",
                TaskType::Image => "image",
                TaskType::Chat => "chat",
            };
            if task_types.iter().any(|t| t == type_str) {
                score += 1.0;
            }
        }

        if let Some(has_image) = &condition.has_image {
            total_weight += 1.0;
            if &profile.has_image == has_image {
                score += 1.0;
            }
        }

        if total_weight == 0.0 {
            return 0.5;
        }
        score / total_weight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::config::OrchestrationConfig;

    fn default_config() -> OrchestrationConfig {
        OrchestrationConfig::default()
    }

    #[test]
    fn select_route_for_simple_task() {
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.2,
            risk: RiskLevel::Low,
            verifiability: 0.1,
            has_image: false,
            need_code: false,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "route");
    }

    #[test]
    fn select_cascade_for_complex_task() {
        let profile = TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.6,
            risk: RiskLevel::High,
            verifiability: 0.9,
            has_image: false,
            need_code: true,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_some());
        let (name, action) = result.unwrap();
        assert_eq!(name, "cascade");
        assert!(matches!(action, StrategyAction::Cascade { .. }));
    }

    #[test]
    fn no_match_returns_none_for_empty_config() {
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.5,
            risk: RiskLevel::Low,
            verifiability: 0.1,
            has_image: false,
            need_code: false,
        };
        let mut config = OrchestrationConfig::default();
        config.strategies.clear();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_none());
    }
}
