use ec_switch_lib::orchestration::quality_gate::{QualityAction, QualityDecision, QualityResult};
use ec_switch_lib::orchestration::OrchestrationEngine;
use serde_json::json;
use tempfile::TempDir;

fn write_strategy_config(yaml: &str) -> (std::path::PathBuf, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("strategies.yaml");
    std::fs::write(&path, yaml).unwrap();
    (path, dir)
}

#[tokio::test]
async fn release_core_selects_debate_and_carries_threshold() {
    let yaml = r#"
enabled: true
models:
  a: { provider: deepseek, model: deepseek-chat, api_key_env: DEEPSEEK_API_KEY }
  b: { provider: qwen, model: qwen-plus, api_key_env: QWEN_API_KEY }
  judge: { provider: anthropic, model: claude-sonnet-4-20250514, api_key_env: ANTHROPIC_API_KEY }
strategies:
  debate:
    priority: 70
    description: "Debate"
    when:
      complexity: [0.0, 1.0]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    action:
      type: debate
      debaters: [a, b]
      judge: judge
      quality_threshold: 0.81
"#;
    let (path, _dir) = write_strategy_config(yaml);
    let engine = OrchestrationEngine::new(path);
    let body = json!({
        "stream": false,
        "messages": [{"role": "user", "content": "Design a reliable queue processor with retries and idempotency."}]
    });

    let decision = engine.decide(&body).await;
    match decision {
        ec_switch_lib::orchestration::engine::OrchestrationDecision::Debate { quality_threshold, .. } => {
            assert!((quality_threshold - 0.81).abs() < f64::EPSILON);
        }
        other => panic!("expected Debate decision, got {other:?}"),
    }
}

#[test]
fn release_core_quality_decision_fallback_is_explicit() {
    // Score 0.30 vs threshold 0.80 → ratio 0.375, lands in the Fallback band
    // [0.25, 0.50) * threshold. Band-aware scoring replaced the old
    // "anything below threshold → Fallback" rule so callers can distinguish
    // near-miss (Retry), mid-miss (Escalate), low-miss (Fallback), and
    // catastrophic miss (FailVisible).
    let result = QualityResult {
        passed: false,
        score: 0.30,
        individual_scores: vec![("judge".to_string(), 0.30)],
    };
    let decision = QualityDecision::from_result(&result, 0.80);

    assert_eq!(decision.action, QualityAction::Fallback);
    assert!(decision.reason.contains("below threshold"));
}
