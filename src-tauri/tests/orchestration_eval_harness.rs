use ec_switch_lib::orchestration::{EvalResult, MiniEvalHarness};

#[test]
fn default_eval_cases_cover_release_strategies() {
    let cases = MiniEvalHarness::default_cases();
    let strategies: std::collections::HashSet<_> =
        cases.iter().map(|case| case.expected_strategy.as_str()).collect();

    assert!(strategies.contains("route"));
    assert!(strategies.contains("cascade"));
    assert!(strategies.contains("debate"));
    assert!(strategies.contains("moa"));
}

#[test]
fn eval_summary_counts_pass_rate_and_quality() {
    let results = vec![
        EvalResult {
            case_id: "a".to_string(),
            strategy_used: "route".to_string(),
            quality_score: 0.8,
            passed: true,
            latency_ms: 100,
            input_tokens: 10,
            output_tokens: 20,
        },
        EvalResult {
            case_id: "b".to_string(),
            strategy_used: "debate".to_string(),
            quality_score: 0.4,
            passed: false,
            latency_ms: 200,
            input_tokens: 30,
            output_tokens: 40,
        },
    ];

    let summary = MiniEvalHarness::summarize(&results);

    assert_eq!(summary.total, 2);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.pass_rate, 0.5);
    assert!((summary.avg_quality - 0.6).abs() < f64::EPSILON);
}
