use crate::orchestration::classifier::{RiskLevel, TaskProfile, TaskType};
use crate::orchestration::config::{OrchestrationConfig, StrategyAction};

#[derive(Debug, Clone)]
pub struct SelectionDecision {
    pub strategy_name: String,
    pub action: StrategyAction,
    pub score: f64,
    pub priority: i32,
    pub rejected: Vec<RejectedStrategy>,
}

#[derive(Debug, Clone)]
pub struct RejectedStrategy {
    pub strategy_name: String,
    pub reason: String,
}

pub struct StrategySelector;

impl StrategySelector {
    pub fn select(
        profile: &TaskProfile,
        config: &OrchestrationConfig,
    ) -> Option<(String, StrategyAction)> {
        Self::select_detailed(profile, config)
            .map(|decision| (decision.strategy_name, decision.action))
    }

    pub fn select_detailed(
        profile: &TaskProfile,
        config: &OrchestrationConfig,
    ) -> Option<SelectionDecision> {
        if !profile.eligible_for_orchestration {
            return None;
        }

        // Format-sensitive requests (strict JSON, schema-validated, code with exact
        // syntax requirements) must NOT be routed through Debate/MoA/Cascade, because
        // those strategies wrap model output in SCORE/REASONING/BEST/ANSWER scaffolding
        // that breaks downstream parsers. Short-circuit to Passthrough so the request
        // reaches the upstream provider untouched.
        if profile.requires_exact_format {
            log::info!(
                "[Orchestration] requires_exact_format=true; skipping orchestration to preserve output format"
            );
            return None;
        }

        let mut candidates: Vec<(String, StrategyAction, f64, i32)> = Vec::new();
        let mut rejected = Vec::new();

        let mut names: Vec<&String> = config.strategies.keys().collect();
        names.sort();

        for name in names {
            let def = &config.strategies[name];
            let score = Self::match_score(profile, &def.when);
            // C5 fix: require minimum 0.5 score for expensive-strategy eligibility.
            // Without this, a partial-match (e.g. 0.33 from risk=critical alone) plus
            // priority sort lets trivial prompts force MoA/Debate — denial-of-wallet.
            let is_expensive = matches!(
                def.action,
                StrategyAction::Cascade { .. }
                    | StrategyAction::Debate { .. }
                    | StrategyAction::MoA { .. }
            );
            let min_threshold = if is_expensive { 0.5 } else { 0.0 };
            if score <= min_threshold {
                rejected.push(RejectedStrategy {
                    strategy_name: name.clone(),
                    reason: if is_expensive && score <= 0.5 {
                        "expensive_strategy_below_min_score".to_string()
                    } else {
                        "condition_score_zero".to_string()
                    },
                });
                continue;
            }

            candidates.push((name.clone(), def.action.clone(), score, def.priority));
        }

        candidates.sort_by(|a, b| {
            b.3.cmp(&a.3)
                .then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.0.cmp(&b.0))
        });

        candidates.into_iter().next().map(
            |(strategy_name, action, score, priority)| SelectionDecision {
                strategy_name,
                action,
                score,
                priority,
                rejected,
            },
        )
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

        if let Some(has_audio) = &condition.has_audio {
            total_weight += 1.0;
            if &profile.has_audio == has_audio {
                score += 1.0;
            }
        }

        if let Some(has_tools) = &condition.has_tools {
            total_weight += 1.0;
            if &profile.has_tools == has_tools {
                score += 1.0;
            }
        }

        if let Some(is_streaming) = &condition.is_streaming {
            total_weight += 1.0;
            if &profile.is_streaming == is_streaming {
                score += 1.0;
            }
        }

        if let Some(modalities) = &condition.modalities {
            total_weight += 1.0;
            let profile_modalities = profile_modalities(profile);
            if modalities
                .iter()
                .all(|required| profile_modalities.iter().any(|actual| actual == required))
            {
                score += 1.0;
            }
        }

        if total_weight == 0.0 {
            return 0.5;
        }
        score / total_weight
    }
}

fn profile_modalities(profile: &TaskProfile) -> Vec<&'static str> {
    let mut modalities = Vec::new();
    if !profile.has_image && !profile.has_audio {
        modalities.push("text");
    }
    if profile.has_image {
        modalities.push("image");
    }
    if profile.has_audio {
        modalities.push("audio");
    }
    modalities
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
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "route");
    }

    #[test]
    fn select_cascade_for_complex_task() {
        // complexity=0.5 / risk=Medium matches the `cascade` condition on both
        // complexity ([0.4, 0.7]) and risk (medium/high). The `debate` condition
        // ([0.7, 0.9]) misses on complexity, so cascade has the strictly higher
        // match score. Under the deterministic sort (priority desc, score desc,
        // name asc) cascade wins here even though debate has higher priority.
        let profile = TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.5,
            risk: RiskLevel::Medium,
            verifiability: 0.9,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
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
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };
        let mut config = OrchestrationConfig::default();
        config.strategies.clear();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_none());
    }

    #[test]
    fn selection_uses_priority_when_scores_tie() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  low_priority:
    priority: 10
    description: "low"
    when:
      complexity: [0.0, 1.0]
    action:
      type: route
      use_model: a
  high_priority:
    priority: 90
    description: "high"
    when:
      complexity: [0.0, 1.0]
    action:
      type: route
      use_model: b
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.5,
            risk: RiskLevel::Low,
            verifiability: 0.1,
            has_image: false,
            need_code: false,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };

        let decision = StrategySelector::select_detailed(&profile, &config).unwrap();
        assert_eq!(decision.strategy_name, "high_priority");
        assert_eq!(decision.priority, 90);
    }

    #[test]
    fn selection_uses_name_when_priority_and_score_tie() {
        let yaml = r#"
enabled: true
models: {}
strategies:
  beta:
    priority: 10
    description: "beta"
    when: {}
    action:
      type: route
      use_model: b
  alpha:
    priority: 10
    description: "alpha"
    when: {}
    action:
      type: route
      use_model: a
"#;
        let config: OrchestrationConfig = serde_yaml::from_str(yaml).unwrap();
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.5,
            risk: RiskLevel::Low,
            verifiability: 0.1,
            has_image: false,
            need_code: false,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };

        let decision = StrategySelector::select_detailed(&profile, &config).unwrap();
        assert_eq!(decision.strategy_name, "alpha");
    }

    #[test]
    fn selector_rejects_ineligible_profile() {
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.5,
            risk: RiskLevel::Low,
            verifiability: 0.1,
            has_image: false,
            need_code: false,
            has_audio: false,
            has_tools: true,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: false,
            ineligibility_reason: Some("streaming_or_tools_or_audio".to_string()),
        };
        let config = OrchestrationConfig::default();

        assert!(StrategySelector::select_detailed(&profile, &config).is_none());
    }

    #[test]
    fn requires_exact_format_short_circuits_to_passthrough() {
        // C3 fix: JSON-only / schema-validated requests must NOT be routed through
        // Debate/MoA/Cascade because the scaffolding (SCORE/REASONING/BEST/ANSWER)
        // corrupts strict-JSON output. Selector returns None so engine passes through.
        let profile = TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.5,
            risk: RiskLevel::Medium,
            verifiability: 0.9,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: true,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        assert!(
            result.is_none(),
            "requires_exact_format=true must short-circuit to Passthrough"
        );
    }

    #[test]
    fn expensive_strategies_require_minimum_half_match_score() {
        // C5 fix: a trivial prompt that only trips risk=critical (0.5 partial match
        // from risk alone against MoA's complexity+risk condition) must NOT reach
        // MoA. Without the threshold, priority 90 lets it beat cheaper strategies.
        let profile = TaskProfile {
            task_type: TaskType::Chat,
            complexity: 0.1, // MoA condition is [0.9, 1.0] — misses
            risk: RiskLevel::Critical, // MoA condition includes "critical" — matches
            verifiability: 0.1,
            has_image: false,
            need_code: false,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        if let Some((name, _)) = result {
            assert_ne!(
                name, "moa",
                "MoA must not be reachable with 0.5 partial-match score (cost-amplification vector)"
            );
        }
    }

    #[test]
    fn expensive_strategy_at_full_match_still_eligible() {
        // Sanity: the new threshold doesn't break the legitimate path.
        // MoA at full match (complexity + risk both match) = 1.0 score → eligible.
        let profile = TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.95, // matches MoA [0.9, 1.0]
            risk: RiskLevel::Critical, // matches MoA ["critical"]
            verifiability: 0.9,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        };
        let config = default_config();
        let result = StrategySelector::select(&profile, &config);
        assert!(result.is_some(), "full match must remain eligible");
        let (name, action) = result.unwrap();
        assert_eq!(name, "moa");
        assert!(matches!(action, StrategyAction::MoA { .. }));
    }
}
