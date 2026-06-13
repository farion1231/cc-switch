# Orchestration Debate/MoA Release Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Milestone 1 release-trustworthy orchestration core: deterministic strategy selection, Provider-integrated model resolution, enforceable Debate/MoA quality gates, visible traces, provider mocks, and a minimal eval harness.

**Architecture:** Keep the existing proxy and orchestration modules, but tighten the core path with small focused additions. Strategy selection becomes deterministic, model roles resolve through existing Provider records, Debate/MoA return structured decisions instead of decorative scores, and every advanced run writes an inspectable trace.

**Tech Stack:** Rust, Tauri, Tokio, Serde/serde_json/serde_yaml, rusqlite, reqwest, existing `ProviderService`, existing proxy/orchestration modules, Vitest only for later UI follow-up.

---

## Scope Check

This plan implements only Milestone 1 from `docs/superpowers/specs/2026-06-13-orchestration-debate-moa-release-core-design.md`.

Included:

- deterministic strategy selection;
- config fields needed for release-safe selection;
- request profile fields for streaming/tools/image/audio guards;
- ProviderModelResolver using existing Provider records;
- ModelCaller structured errors and mockable transport seam;
- enforceable QualityGate decisions;
- Debate threshold, critique, revision, judge, fallback;
- MoA ranking, filtering, aggregation, fallback;
- trace ledger built on existing `history.rs`;
- end-to-end mock tests;
- minimal eval harness;
- final verification commands.

Excluded from this plan:

- full image+text CrossModal execution;
- full audio STT/TTS execution;
- token-level real-time streaming for Debate/MoA/Cascade;
- large frontend observability dashboard.

The excluded items need separate implementation plans after Milestone 1 passes.

## Existing Files and Responsibilities

### Modify

- `src-tauri/src/orchestration/config.rs`  
  Add strategy priority, modality guards, streaming/tools guards, budgets, and fallback config. Keep serde backward compatible.

- `src-tauri/src/orchestration/classifier.rs`  
  Extend `TaskProfile` with `has_tools`, `is_streaming`, `has_audio`, `requires_exact_format`, and `eligible_for_orchestration`.

- `src-tauri/src/orchestration/selector.rs`  
  Replace `HashMap` iteration-dependent selection with deterministic `SelectionDecision`.

- `src-tauri/src/orchestration/model_caller.rs`  
  Add structured `ModelCallError`, `ModelCallTarget`, and mockable call path. Keep current public wrappers where possible.

- `src-tauri/src/orchestration/quality_gate.rs`  
  Add structured rubric parsing and `QualityDecision`.

- `src-tauri/src/orchestration/executor.rs`  
  Enforce Debate/MoA thresholds. Add critique/revision/ranking stages and visible fallback.

- `src-tauri/src/orchestration/engine.rs`  
  Carry selector details into decisions/outcomes and call trace ledger.

- `src-tauri/src/orchestration/history.rs`  
  Extend stored records with selector, step, fallback, and error fields.

- `src-tauri/src/orchestration/mod.rs`  
  Export new modules.

- `src-tauri/src/lib.rs`  
  Wire ProviderModelResolver/AppState into orchestration initialization.

- `src-tauri/src/proxy/handlers.rs`  
  Preserve passthrough for strict streaming/tools and surface orchestration fallback metadata where compatible.

- `configs/strategies.yaml`  
  Add priorities, budgets, modality constraints, fallback policies, and role names.

### Create

- `src-tauri/src/orchestration/provider_resolver.rs`  
  Resolve orchestration logical roles to concrete Provider-backed model call targets.

- `src-tauri/src/orchestration/trace_ledger.rs`  
  Convert execution facts into records and write them through `HistoryStore`.

- `src-tauri/src/orchestration/eval_harness.rs`  
  Run fixed local eval cases against mock or real model caller.

- `src-tauri/tests/orchestration_release_core.rs`  
  End-to-end release core tests with mock providers.

- `src-tauri/tests/orchestration_eval_harness.rs`  
  Minimal eval harness tests.

## Implementation Rules for GLM5.1

- Do not rewrite the proxy architecture.
- Do not remove existing tests.
- Do not commit unrelated dirty files.
- Do not enable orchestration by default for all users.
- Do not route image/audio requests into text-only Debate/MoA.
- Do not keep hardcoded provider URLs as the primary orchestration path.
- Do not treat a quality score as meaningful unless it changes control flow.
- Do not claim streaming support for Debate/MoA/Cascade unless the implementation explicitly tests that behavior.
- Commit after each task.

## Task 1: Expand Config Without Breaking Existing YAML

**Files:**
- Modify: `src-tauri/src/orchestration/config.rs`
- Modify: `configs/strategies.yaml`
- Test: existing unit tests in `src-tauri/src/orchestration/config.rs`

- [ ] **Step 1: Write failing config tests**

Add these tests inside `#[cfg(test)] mod tests` in `src-tauri/src/orchestration/config.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test orchestration::config::tests::strategy_def_deserializes_release_fields orchestration::config::tests::old_strategy_yaml_still_deserializes_with_defaults
```

Expected: FAIL because `priority`, `budgets`, `fallback`, `has_tools`, `is_streaming`, `modalities`, `max_rounds`, `critique`, and `revision` are not defined yet.

- [ ] **Step 3: Add config structs and serde defaults**

Modify `src-tauri/src/orchestration/config.rs` with these additions.

Replace `StrategyDef` with:

```rust
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
```

Replace `StrategyCondition` with:

```rust
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
```

Add these structs below `StrategyCondition`:

```rust
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
```

Update `StrategyAction::Debate`:

```rust
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
```

Add:

```rust
fn default_debate_rounds() -> u32 {
    1
}
```

Update all existing `StrategyAction::Debate` pattern matches in `config.rs` with `..` when the new fields are not used:

```rust
StrategyAction::Debate { debaters, judge, .. } => {
```

Update the default Debate action:

```rust
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
```

Update tests that destructure Debate:

```rust
StrategyAction::Debate {
    debaters,
    judge,
    quality_threshold,
    ..
} => {
```

- [ ] **Step 4: Run config tests**

Run:

```powershell
cargo test orchestration::config::tests
```

Expected: PASS.

- [ ] **Step 5: Update `configs/strategies.yaml` release fields**

Keep `enabled: false` unless product owner explicitly decides to enable. Add `priority`, `budgets`, `fallback`, and modality guards to each strategy. Use this pattern:

```yaml
strategies:
  route:
    priority: 10
    description: "Direct route for simple text requests"
    when:
      complexity: [0.0, 0.4]
      risk: ["low"]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    budgets:
      max_calls: 1
      max_latency_ms: 30000
      max_cost_usd: 0.05
    action:
      type: route
      use_model: cheap_coder
      verify: false
    fallback:
      on_provider_fail: "passthrough"

  cascade:
    priority: 40
    description: "Cheap first, verify, escalate"
    when:
      complexity: [0.4, 0.75]
      risk: ["medium", "high"]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    budgets:
      max_calls: 3
      max_latency_ms: 60000
      max_cost_usd: 0.20
    action:
      type: cascade
      models: ["cheap_coder", "frontier"]
      verify_each: true
      escalate_on_fail: true
      quality_threshold: 0.70
    fallback:
      on_quality_fail: "frontier"
      on_provider_fail: "passthrough"

  debate:
    priority: 70
    description: "Structured multi-model debate for high-risk reasoning"
    when:
      complexity: [0.70, 0.95]
      risk: ["high", "critical"]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    budgets:
      max_calls: 6
      max_latency_ms: 90000
      max_cost_usd: 0.50
    action:
      type: debate
      debaters: ["cheap_coder", "qwen_coder", "glm_coder"]
      judge: "frontier"
      max_rounds: 1
      critique: true
      revision: true
      quality_threshold: 0.80
    fallback:
      on_quality_fail: "frontier"
      on_judge_fail: "frontier"
      on_provider_fail: "passthrough"

  moa:
    priority: 90
    description: "Ranked Mixture-of-Agents for critical complex tasks"
    when:
      complexity: [0.85, 1.0]
      risk: ["critical"]
      has_tools: false
      is_streaming: false
      modalities: ["text"]
    budgets:
      max_calls: 5
      max_latency_ms: 90000
      max_cost_usd: 0.60
    action:
      type: moa
      proposers: ["cheap_coder", "qwen_coder", "glm_coder", "frontier"]
      aggregator: "frontier"
      verify_each: true
      quality_threshold: 0.82
    fallback:
      on_quality_fail: "frontier"
      on_provider_fail: "passthrough"
```

- [ ] **Step 6: Verify YAML parses**

Run:

```powershell
python -c "import yaml, pathlib; p=pathlib.Path('configs/strategies.yaml'); d=yaml.safe_load(p.read_text(encoding='utf-8')); print(sorted(d['strategies'].keys()))"
```

Expected: prints the strategy keys without an exception.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/orchestration/config.rs configs/strategies.yaml
git commit -m "feat(orchestration): extend strategy config for release core"
```

## Task 2: Expand TaskProfile and Modality Guards

**Files:**
- Modify: `src-tauri/src/orchestration/classifier.rs`
- Modify: `src-tauri/src/orchestration/selector.rs`
- Test: unit tests in both files

- [ ] **Step 1: Add failing classifier tests**

Add tests to `src-tauri/src/orchestration/classifier.rs`:

```rust
#[test]
fn classify_detects_streaming_tools_and_audio() {
    let body = serde_json::json!({
        "stream": true,
        "tools": [{"name": "shell"}],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "input_audio", "audio": {"data": "abc", "format": "wav"}},
                {"type": "text", "text": "transcribe this"}
            ]
        }]
    });

    let profile = TaskClassifier::classify(&body);

    assert!(profile.is_streaming);
    assert!(profile.has_tools);
    assert!(profile.has_audio);
    assert!(!profile.eligible_for_orchestration);
    assert_eq!(
        profile.ineligibility_reason.as_deref(),
        Some("streaming_or_tools_or_audio")
    );
}

#[test]
fn classify_text_request_is_eligible() {
    let body = serde_json::json!({
        "stream": false,
        "messages": [{"role": "user", "content": "explain merge sort"}]
    });

    let profile = TaskClassifier::classify(&body);

    assert!(!profile.is_streaming);
    assert!(!profile.has_tools);
    assert!(!profile.has_audio);
    assert!(profile.eligible_for_orchestration);
    assert_eq!(profile.ineligibility_reason, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::classifier::tests::classify_detects_streaming_tools_and_audio orchestration::classifier::tests::classify_text_request_is_eligible
```

Expected: FAIL because the new fields do not exist.

- [ ] **Step 3: Extend `TaskProfile`**

In `src-tauri/src/orchestration/classifier.rs`, add fields to `TaskProfile`:

```rust
pub has_audio: bool,
pub has_tools: bool,
pub is_streaming: bool,
pub requires_exact_format: bool,
pub eligible_for_orchestration: bool,
pub ineligibility_reason: Option<String>,
```

Update every existing test-created `TaskProfile` in `selector.rs` and `classifier.rs` to include:

```rust
has_audio: false,
has_tools: false,
is_streaming: false,
requires_exact_format: false,
eligible_for_orchestration: true,
ineligibility_reason: None,
```

- [ ] **Step 4: Add helper detection methods**

Add these helper functions inside `impl TaskClassifier`:

```rust
fn has_tools(body: &serde_json::Value) -> bool {
    body.get("tools")
        .and_then(|v| v.as_array())
        .map(|tools| !tools.is_empty())
        .unwrap_or(false)
}

fn is_streaming(body: &serde_json::Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn has_audio(body: &serde_json::Value) -> bool {
    let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
        return false;
    };

    messages.iter().any(|message| {
        let Some(content) = message.get("content") else {
            return false;
        };
        if let Some(items) = content.as_array() {
            return items.iter().any(|item| {
                matches!(
                    item.get("type").and_then(|v| v.as_str()),
                    Some("input_audio") | Some("audio") | Some("audio_url")
                ) || item.get("audio").is_some()
            });
        }
        false
    })
}

fn requires_exact_format(body: &serde_json::Value) -> bool {
    let text = Self::extract_text(body).to_ascii_lowercase();
    text.contains("return json")
        || text.contains("valid json")
        || text.contains("exact format")
        || text.contains("do not include anything else")
}

fn orchestration_eligibility(
    is_streaming: bool,
    has_tools: bool,
    has_audio: bool,
) -> (bool, Option<String>) {
    if is_streaming || has_tools || has_audio {
        return (false, Some("streaming_or_tools_or_audio".to_string()));
    }
    (true, None)
}
```

If `extract_text` is private and named differently in the current file, reuse the existing text extraction function. If no such function exists, add:

```rust
fn extract_text(body: &serde_json::Value) -> String {
    let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
        return String::new();
    };

    messages
        .iter()
        .filter_map(|message| message.get("content"))
        .flat_map(|content| {
            if let Some(s) = content.as_str() {
                vec![s.to_string()]
            } else if let Some(items) = content.as_array() {
                items
                    .iter()
                    .filter_map(|item| {
                        item.get("text")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 5: Populate fields in `classify`**

Inside `TaskClassifier::classify`, compute:

```rust
let has_tools = Self::has_tools(body);
let is_streaming = Self::is_streaming(body);
let has_audio = Self::has_audio(body);
let requires_exact_format = Self::requires_exact_format(body);
let (eligible_for_orchestration, ineligibility_reason) =
    Self::orchestration_eligibility(is_streaming, has_tools, has_audio);
```

Set those values in the returned `TaskProfile`.

- [ ] **Step 6: Run classifier and selector tests**

```powershell
cargo test orchestration::classifier::tests orchestration::selector::tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/orchestration/classifier.rs src-tauri/src/orchestration/selector.rs
git commit -m "feat(orchestration): classify streaming tools and modalities"
```

## Task 3: Deterministic StrategySelector

**Files:**
- Modify: `src-tauri/src/orchestration/selector.rs`
- Test: unit tests in `selector.rs`

- [ ] **Step 1: Add failing deterministic selection tests**

Add these tests in `src-tauri/src/orchestration/selector.rs`:

```rust
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
```

- [ ] **Step 2: Run selector tests to verify they fail**

```powershell
cargo test orchestration::selector::tests::selection_uses_priority_when_scores_tie orchestration::selector::tests::selection_uses_name_when_priority_and_score_tie orchestration::selector::tests::selector_rejects_ineligible_profile
```

Expected: FAIL because `select_detailed` and `SelectionDecision` do not exist.

- [ ] **Step 3: Add selection decision type**

In `selector.rs`, add:

```rust
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
```

- [ ] **Step 4: Replace selector implementation**

Replace `select` with this pair:

```rust
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

    let mut candidates: Vec<(String, StrategyAction, f64, i32)> = Vec::new();
    let mut rejected = Vec::new();

    let mut names: Vec<&String> = config.strategies.keys().collect();
    names.sort();

    for name in names {
        let def = &config.strategies[name];
        let score = Self::match_score(profile, &def.when);
        if score <= 0.0 {
            rejected.push(RejectedStrategy {
                strategy_name: name.clone(),
                reason: "condition_score_zero".to_string(),
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

    candidates.into_iter().next().map(|(strategy_name, action, score, priority)| {
        SelectionDecision {
            strategy_name,
            action,
            score,
            priority,
            rejected,
        }
    })
}
```

- [ ] **Step 5: Extend `match_score` for new guards**

Add after existing `has_image` scoring:

```rust
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
```

Add helper:

```rust
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
```

- [ ] **Step 6: Run selector tests**

```powershell
cargo test orchestration::selector::tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/orchestration/selector.rs
git commit -m "feat(orchestration): make strategy selection deterministic"
```

## Task 4: ProviderModelResolver

**Files:**
- Create: `src-tauri/src/orchestration/provider_resolver.rs`
- Modify: `src-tauri/src/orchestration/mod.rs`
- Modify: `src-tauri/src/orchestration/config.rs`
- Test: unit tests in `provider_resolver.rs`

- [ ] **Step 1: Create failing resolver tests**

Create `src-tauri/src/orchestration/provider_resolver.rs` with this test module and public type stubs:

```rust
use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Text,
    Vision,
    AudioInput,
    AudioOutput,
    Json,
}

#[derive(Debug, Clone)]
pub struct ResolvedModelCallTarget {
    pub role: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_type: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub capabilities: HashSet<ModelCapability>,
}

pub struct ProviderModelResolver;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use indexmap::IndexMap;
    use serde_json::json;

    fn provider(id: &str, provider_type: &str, model: &str, base_url: &str, api_key: &str) -> Provider {
        let mut p = Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": api_key,
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_MODEL": model
                }
            }),
            Some(base_url.to_string()),
        );
        p.meta = Some(ProviderMeta {
            provider_type: Some(provider_type.to_string()),
            ..ProviderMeta::default()
        });
        p
    }

    #[test]
    fn resolves_text_role_from_provider_map() {
        let mut providers = IndexMap::new();
        providers.insert(
            "claude-primary".to_string(),
            provider(
                "claude-primary",
                "anthropic",
                "claude-sonnet-4-20250514",
                "https://api.anthropic.com",
                "sk-test",
            ),
        );

        let target = ProviderModelResolver::resolve_role(
            "frontier",
            &providers,
            &[ModelCapability::Text, ModelCapability::Json],
        )
        .unwrap();

        assert_eq!(target.role, "frontier");
        assert_eq!(target.provider_id, "claude-primary");
        assert_eq!(target.provider_type, "anthropic");
        assert_eq!(target.model, "claude-sonnet-4-20250514");
        assert_eq!(target.base_url, "https://api.anthropic.com");
        assert_eq!(target.api_key, "sk-test");
        assert!(target.capabilities.contains(&ModelCapability::Text));
    }

    #[test]
    fn rejects_missing_capability() {
        let mut providers = IndexMap::new();
        providers.insert(
            "text-only".to_string(),
            provider("text-only", "openai_chat", "gpt-5-mini", "https://example.com/v1", "sk-test"),
        );

        let err = ProviderModelResolver::resolve_role(
            "vision_extractor",
            &providers,
            &[ModelCapability::Vision],
        )
        .unwrap_err();

        assert!(err.contains("capability"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::provider_resolver::tests
```

Expected: FAIL because module is not exported and `resolve_role` is not implemented.

- [ ] **Step 3: Export module**

Modify `src-tauri/src/orchestration/mod.rs`:

```rust
pub mod provider_resolver;
pub use provider_resolver::{ModelCapability, ProviderModelResolver, ResolvedModelCallTarget};
```

- [ ] **Step 4: Implement resolver**

Append this implementation to `provider_resolver.rs`:

```rust
impl ProviderModelResolver {
    pub fn resolve_role(
        role: &str,
        providers: &indexmap::IndexMap<String, Provider>,
        required: &[ModelCapability],
    ) -> Result<ResolvedModelCallTarget, String> {
        let role_lower = role.to_ascii_lowercase();
        let mut candidates: Vec<&Provider> = providers
            .values()
            .filter(|provider| !provider.in_failover_queue)
            .collect();

        candidates.sort_by(|a, b| {
            a.sort_index
                .unwrap_or(usize::MAX)
                .cmp(&b.sort_index.unwrap_or(usize::MAX))
                .then_with(|| a.id.cmp(&b.id))
        });

        for provider in candidates {
            let capabilities = infer_capabilities(provider);
            if !required.iter().all(|cap| capabilities.contains(cap)) {
                continue;
            }

            if !role_matches_provider(&role_lower, provider) {
                continue;
            }

            return build_target(role, provider, capabilities);
        }

        Err(format!(
            "no provider resolved for role '{role}' with required capabilities {:?}",
            required
        ))
    }
}

fn role_matches_provider(role: &str, provider: &Provider) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        provider.id,
        provider.name,
        provider.category.clone().unwrap_or_default(),
        provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.clone())
            .unwrap_or_default()
    )
    .to_ascii_lowercase();

    match role {
        "frontier" | "frontier_judge" | "frontier_single" => {
            haystack.contains("claude")
                || haystack.contains("openai")
                || haystack.contains("gpt")
                || haystack.contains("sonnet")
        }
        "cheap_coder" | "cheap_reasoner" => {
            haystack.contains("deepseek")
                || haystack.contains("qwen")
                || haystack.contains("glm")
                || haystack.contains("mini")
                || haystack.contains("flash")
        }
        "qwen_coder" => haystack.contains("qwen"),
        "glm_coder" => haystack.contains("glm"),
        "vision_extractor" => haystack.contains("vision") || haystack.contains("gemini"),
        _ => haystack.contains(role),
    }
}

fn infer_capabilities(provider: &Provider) -> HashSet<ModelCapability> {
    let mut caps = HashSet::new();
    caps.insert(ModelCapability::Text);

    let combined = format!(
        "{} {} {}",
        provider.id,
        provider.name,
        provider.settings_config
    )
    .to_ascii_lowercase();

    if combined.contains("json") || combined.contains("claude") || combined.contains("gpt") {
        caps.insert(ModelCapability::Json);
    }
    if combined.contains("vision")
        || combined.contains("gemini")
        || combined.contains("gpt-4o")
        || combined.contains("image")
    {
        caps.insert(ModelCapability::Vision);
    }
    if combined.contains("audio") || combined.contains("transcrib") || combined.contains("whisper") {
        caps.insert(ModelCapability::AudioInput);
    }
    if combined.contains("tts") || combined.contains("speech") {
        caps.insert(ModelCapability::AudioOutput);
    }

    caps
}

fn build_target(
    role: &str,
    provider: &Provider,
    capabilities: HashSet<ModelCapability>,
) -> Result<ResolvedModelCallTarget, String> {
    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|m| m.provider_type.clone())
        .unwrap_or_else(|| "openai_chat".to_string());
    let model = extract_model(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no model field was found",
            provider.id, role
        )
    })?;
    let base_url = extract_base_url(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no base URL was found",
            provider.id, role
        )
    })?;
    let api_key = extract_api_key(provider).ok_or_else(|| {
        format!(
            "provider '{}' matched role '{}' but no API key was found",
            provider.id, role
        )
    })?;

    Ok(ResolvedModelCallTarget {
        role: role.to_string(),
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        provider_type,
        model,
        base_url,
        api_key,
        capabilities,
    })
}

fn extract_model(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_MODEL"))
        .or_else(|| env.and_then(|v| v.get("OPENAI_MODEL")))
        .or_else(|| env.and_then(|v| v.get("MODEL")))
        .or_else(|| provider.settings_config.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_base_url(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_BASE_URL"))
        .or_else(|| env.and_then(|v| v.get("OPENAI_BASE_URL")))
        .or_else(|| env.and_then(|v| v.get("BASE_URL")))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .or_else(|| provider.settings_config.get("base_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
        .or_else(|| provider.website_url.clone())
}

fn extract_api_key(provider: &Provider) -> Option<String> {
    let env = provider.settings_config.get("env");
    env.and_then(|v| v.get("ANTHROPIC_API_KEY"))
        .or_else(|| env.and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN")))
        .or_else(|| env.and_then(|v| v.get("OPENAI_API_KEY")))
        .or_else(|| env.and_then(|v| v.get("API_KEY")))
        .or_else(|| provider.settings_config.pointer("/auth/OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
```

- [ ] **Step 5: Run resolver tests**

```powershell
cargo test orchestration::provider_resolver::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/orchestration/provider_resolver.rs src-tauri/src/orchestration/mod.rs
git commit -m "feat(orchestration): resolve model roles from providers"
```

## Task 5: Structured ModelCaller Errors and Targets

**Files:**
- Modify: `src-tauri/src/orchestration/model_caller.rs`
- Test: unit tests in `model_caller.rs`

- [ ] **Step 1: Add failing model caller parsing tests**

Add tests:

```rust
#[test]
fn model_call_error_kind_is_stable() {
    let err = ModelCallError::provider_timeout("cheap_coder", "request timed out");
    assert_eq!(err.kind, ModelCallErrorKind::ProviderTimeout);
    assert_eq!(err.model_key, "cheap_coder");
    assert!(err.message.contains("request timed out"));
}

#[test]
fn target_builds_openai_chat_url() {
    let target = ModelCallTarget {
        model_key: "frontier".to_string(),
        provider_type: "openai_chat".to_string(),
        model: "gpt-5-mini".to_string(),
        base_url: "https://example.com/v1".to_string(),
        api_key: "sk-test".to_string(),
        max_tokens: 1024,
    };

    assert_eq!(
        ModelCaller::build_target_url(&target).unwrap(),
        "https://example.com/v1/chat/completions"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::model_caller::tests::model_call_error_kind_is_stable orchestration::model_caller::tests::target_builds_openai_chat_url
```

Expected: FAIL because the new types do not exist.

- [ ] **Step 3: Add structured types**

Add near existing `ModelResponse`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCallErrorKind {
    ModelNotFound,
    ProviderAuthFailed,
    ProviderRateLimited,
    ProviderTimeout,
    ProviderHttp,
    ResponseMalformed,
}

#[derive(Debug, Clone)]
pub struct ModelCallError {
    pub kind: ModelCallErrorKind,
    pub model_key: String,
    pub message: String,
}

impl ModelCallError {
    pub fn provider_timeout(model_key: &str, message: &str) -> Self {
        Self {
            kind: ModelCallErrorKind::ProviderTimeout,
            model_key: model_key.to_string(),
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCallTarget {
    pub model_key: String,
    pub provider_type: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub max_tokens: u32,
}
```

- [ ] **Step 4: Add target URL builder**

Add inside `impl ModelCaller`:

```rust
pub fn build_target_url(target: &ModelCallTarget) -> Result<String, String> {
    let base = target.base_url.trim_end_matches('/');
    match target.provider_type.as_str() {
        "anthropic" => Ok(format!("{base}/v1/messages")),
        "openai_chat" | "openai" => {
            if base.ends_with("/chat/completions") {
                Ok(base.to_string())
            } else {
                Ok(format!("{base}/chat/completions"))
            }
        }
        other => Err(format!("unsupported provider_type '{other}'")),
    }
}
```

- [ ] **Step 5: Keep old `call` behavior, add target-based call**

Add:

```rust
pub async fn call_target(
    &self,
    target: &ModelCallTarget,
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
    temperature: Option<f64>,
) -> Result<ModelResponse, ModelCallError> {
    let mut body = json!({
        "model": target.model,
        "messages": messages,
        "max_tokens": target.max_tokens,
        "stream": false,
    });

    if let Some(t) = temperature {
        body["temperature"] = json!(t);
    }
    if let Some(ref t) = tools {
        body["tools"] = json!(t);
    }

    let start = std::time::Instant::now();
    let url = Self::build_target_url(target).map_err(|message| ModelCallError {
        kind: ModelCallErrorKind::ProviderHttp,
        model_key: target.model_key.clone(),
        message,
    })?;

    let mut req = self.client.post(&url).header("Content-Type", "application/json");
    if target.provider_type == "anthropic" {
        req = req
            .header("x-api-key", &target.api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", target.api_key));
    }

    let resp = req.json(&body).send().await.map_err(|e| {
        let message = e.to_string();
        let kind = if e.is_timeout() {
            ModelCallErrorKind::ProviderTimeout
        } else {
            ModelCallErrorKind::ProviderHttp
        };
        ModelCallError {
            kind,
            model_key: target.model_key.clone(),
            message,
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        let kind = match status.as_u16() {
            401 | 403 => ModelCallErrorKind::ProviderAuthFailed,
            429 => ModelCallErrorKind::ProviderRateLimited,
            _ => ModelCallErrorKind::ProviderHttp,
        };
        return Err(ModelCallError {
            kind,
            model_key: target.model_key.clone(),
            message: format!("status {status}: {error_body}"),
        });
    }

    let resp_body: Value = resp.json().await.map_err(|e| ModelCallError {
        kind: ModelCallErrorKind::ResponseMalformed,
        model_key: target.model_key.clone(),
        message: e.to_string(),
    })?;

    let latency_ms = start.elapsed().as_millis() as u64;
    let content = Self::extract_content(&resp_body);
    let usage = TokenUsage {
        input_tokens: resp_body
            .get("usage")
            .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
            .and_then(|t| t.as_u64())
            .unwrap_or(0),
        output_tokens: resp_body
            .get("usage")
            .and_then(|u| u.get("output_tokens").or_else(|| u.get("completion_tokens")))
            .and_then(|t| t.as_u64())
            .unwrap_or(0),
    };

    Ok(ModelResponse {
        content,
        model: target.model.clone(),
        usage,
        latency_ms,
    })
}
```

- [ ] **Step 6: Run model caller tests**

```powershell
cargo test orchestration::model_caller::tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/orchestration/model_caller.rs
git commit -m "feat(orchestration): add structured model call targets"
```

## Task 6: QualityGate Decisions That Control Flow

**Files:**
- Modify: `src-tauri/src/orchestration/quality_gate.rs`
- Test: unit tests in `quality_gate.rs`

- [ ] **Step 1: Add failing decision tests**

Add:

```rust
#[test]
fn quality_decision_accepts_score_at_threshold() {
    let result = QualityResult {
        passed: true,
        score: 0.82,
        individual_scores: vec![("rubric".to_string(), 0.82)],
    };

    let decision = QualityDecision::from_result(&result, 0.80);
    assert_eq!(decision.action, QualityAction::Accept);
    assert_eq!(decision.score, 0.82);
}

#[test]
fn quality_decision_falls_back_below_threshold() {
    let result = QualityResult {
        passed: false,
        score: 0.62,
        individual_scores: vec![("rubric".to_string(), 0.62)],
    };

    let decision = QualityDecision::from_result(&result, 0.80);
    assert_eq!(decision.action, QualityAction::Fallback);
    assert!(decision.reason.contains("below threshold"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::quality_gate::tests::quality_decision_accepts_score_at_threshold orchestration::quality_gate::tests::quality_decision_falls_back_below_threshold
```

Expected: FAIL because `QualityDecision` and `QualityAction` do not exist.

- [ ] **Step 3: Add decision types**

Add near `QualityResult`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityAction {
    Accept,
    Retry,
    Escalate,
    Fallback,
    FailVisible,
}

#[derive(Debug, Clone)]
pub struct QualityDecision {
    pub action: QualityAction,
    pub score: f64,
    pub threshold: f64,
    pub reason: String,
}

impl QualityDecision {
    pub fn from_result(result: &QualityResult, threshold: f64) -> Self {
        if result.score >= threshold && result.passed {
            return Self {
                action: QualityAction::Accept,
                score: result.score,
                threshold,
                reason: format!("score {:.2} met threshold {:.2}", result.score, threshold),
            };
        }

        Self {
            action: QualityAction::Fallback,
            score: result.score,
            threshold,
            reason: format!("score {:.2} below threshold {:.2}", result.score, threshold),
        }
    }
}
```

- [ ] **Step 4: Run quality tests**

```powershell
cargo test orchestration::quality_gate::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/orchestration/quality_gate.rs
git commit -m "feat(orchestration): add enforceable quality decisions"
```

## Task 7: Debate Threshold, Critique, Revision, and Fallback

**Files:**
- Modify: `src-tauri/src/orchestration/engine.rs`
- Modify: `src-tauri/src/orchestration/executor.rs`
- Test: unit tests in `executor.rs`

- [ ] **Step 1: Carry Debate threshold in decisions**

Add failing test in `engine.rs`:

```rust
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
```

Run:

```powershell
cargo test orchestration::engine::tests::debate_decision_carries_quality_threshold
```

Expected: FAIL because `OrchestrationDecision::Debate` lacks `quality_threshold`.

- [ ] **Step 2: Update `OrchestrationDecision::Debate`**

In `engine.rs`, change:

```rust
Debate {
    debaters: Vec<String>,
    judge: String,
    quality_threshold: f64,
    max_rounds: u32,
    critique: bool,
    revision: bool,
},
```

Update the `StrategyAction::Debate` match:

```rust
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
    OrchestrationDecision::Debate {
        debaters: healthy_debaters,
        judge,
        quality_threshold,
        max_rounds,
        critique,
        revision,
    }
}
```

Update `executor.rs` match:

```rust
OrchestrationDecision::Debate {
    debaters,
    judge,
    quality_threshold,
    max_rounds,
    critique,
    revision,
} => {
    self.execute_debate(
        debaters,
        judge,
        *quality_threshold,
        *max_rounds,
        *critique,
        *revision,
        messages,
        tools,
    )
    .await
}
```

- [ ] **Step 3: Replace Debate signature**

Change `execute_debate` signature:

```rust
pub async fn execute_debate(
    &self,
    debater_keys: &[String],
    judge_key: &str,
    quality_threshold: f64,
    max_rounds: u32,
    critique: bool,
    revision: bool,
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
) -> Result<ExecutionResult, String> {
```

- [ ] **Step 4: Add prompt builders**

Add these helpers to `executor.rs`:

```rust
fn build_debate_critique_prompt(candidates: &[(String, ModelResponse)]) -> String {
    let mut prompt = String::from(
        "Review the candidate answers. Identify factual errors, missing requirements, unsafe claims, and format violations.\n\n",
    );
    for (i, (key, resp)) in candidates.iter().enumerate() {
        prompt.push_str(&format!("Candidate {} ({})\n{}\n\n", i + 1, key, resp.content));
    }
    prompt.push_str(
        "Return concise critique bullets and end with SCORE: <0.0 to 1.0> for the strongest candidate.",
    );
    prompt
}

fn build_debate_revision_prompt(original_answer: &str, critique: &str) -> String {
    format!(
        "Revise your answer using the critique. Keep correct parts, fix errors, and preserve the user's requested format.\n\nOriginal answer:\n{original_answer}\n\nCritique:\n{critique}\n\nRevised answer:"
    )
}

fn build_debate_judge_prompt(
    candidates: &[(String, ModelResponse)],
    critique_text: Option<&str>,
) -> String {
    let mut prompt = String::from(
        "You are judging revised candidate answers. Select the best final answer and enforce the quality threshold strictly.\n\n",
    );
    if let Some(critique) = critique_text {
        prompt.push_str("Critique summary:\n");
        prompt.push_str(critique);
        prompt.push_str("\n\n");
    }
    for (i, (key, resp)) in candidates.iter().enumerate() {
        prompt.push_str(&format!("Candidate {} ({})\n{}\n\n", i + 1, key, resp.content));
    }
    prompt.push_str(
        "Return exactly:\nSCORE: <0.0 to 1.0>\nBEST: <candidate number>\nANSWER:\n<final answer>",
    );
    prompt
}
```

- [ ] **Step 5: Enforce threshold after judge**

In `execute_debate`, after judge response:

```rust
let score = Self::extract_score_from_judge(&judge_resp.content).unwrap_or(0.0);
if score < quality_threshold {
    log::warn!(
        "[Debate] judge score {:.2} below threshold {:.2}; returning fallback error",
        score,
        quality_threshold
    );
    return Err(format!(
        "quality_threshold_failed: debate score {:.2} below {:.2}",
        score, quality_threshold
    ));
}
```

Set `verified: true` only when threshold passes.

- [ ] **Step 6: Add critique and revision calls behind flags**

After initial successful `responses`:

```rust
let mut critique_text: Option<String> = None;
if critique {
    let critique_prompt = Self::build_debate_critique_prompt(&responses);
    let critique_resp = self
        .caller
        .call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &critique_prompt, Some(0.2))
        .await?;
    total_input += critique_resp.usage.input_tokens;
    total_output += critique_resp.usage.output_tokens;
    critique_text = Some(critique_resp.content);
}

let mut revised_responses = responses.clone();
if revision && max_rounds > 0 {
    let critique_for_revision = critique_text.as_deref().unwrap_or("");
    let revision_futures: Vec<_> = responses
        .iter()
        .map(|(model_key, resp)| async {
            let prompt = Self::build_debate_revision_prompt(&resp.content, critique_for_revision);
            let result = self
                .caller
                .call_prompt(model_key, "", &prompt, Some(0.2))
                .await;
            (model_key.clone(), result)
        })
        .collect();
    let revision_results = join_all(revision_futures).await;
    let mut revisions = Vec::new();
    for (model_key, result) in revision_results {
        if let Ok(resp) = result {
            total_input += resp.usage.input_tokens;
            total_output += resp.usage.output_tokens;
            revisions.push((model_key, resp));
        }
    }
    if !revisions.is_empty() {
        revised_responses = revisions;
    }
}
```

Then judge `revised_responses` instead of initial `responses`.

- [ ] **Step 7: Run Debate-related tests**

```powershell
cargo test orchestration::engine::tests::debate_decision_carries_quality_threshold orchestration::executor::tests
```

Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/orchestration/engine.rs src-tauri/src/orchestration/executor.rs
git commit -m "feat(orchestration): enforce structured debate thresholds"
```

## Task 8: MoA Ranking, Filtering, and Fallback

**Files:**
- Modify: `src-tauri/src/orchestration/executor.rs`
- Test: unit tests in `executor.rs`

- [ ] **Step 1: Add failing MoA helper tests**

Add:

```rust
#[test]
fn moa_ranking_prompt_contains_scores_and_candidates() {
    let proposals = vec![
        ("a".to_string(), ModelResponse {
            content: "weak answer".to_string(),
            model: "a-model".to_string(),
            usage: Default::default(),
            latency_ms: 10,
        }),
        ("b".to_string(), ModelResponse {
            content: "strong answer".to_string(),
            model: "b-model".to_string(),
            usage: Default::default(),
            latency_ms: 20,
        }),
    ];

    let prompt = StrategyExecutor::build_moa_ranking_prompt(&proposals);
    assert!(prompt.contains("Candidate 1"));
    assert!(prompt.contains("Candidate 2"));
    assert!(prompt.contains("SCORES_JSON"));
}

#[test]
fn parse_moa_scores_sorts_descending() {
    let content = r#"SCORES_JSON: [{"candidate":2,"score":0.9},{"candidate":1,"score":0.4}]"#;
    let scores = StrategyExecutor::parse_moa_scores(content);

    assert_eq!(scores, vec![(1, 0.9), (0, 0.4)]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::executor::tests::moa_ranking_prompt_contains_scores_and_candidates orchestration::executor::tests::parse_moa_scores_sorts_descending
```

Expected: FAIL because helpers do not exist.

- [ ] **Step 3: Add ranking helpers**

Add to `executor.rs`:

```rust
fn build_moa_ranking_prompt(proposals: &[(String, ModelResponse)]) -> String {
    let mut prompt = String::from(
        "Rank these candidate answers for correctness, completeness, instruction following, and format compliance.\n\n",
    );
    for (i, (key, resp)) in proposals.iter().enumerate() {
        prompt.push_str(&format!("Candidate {} ({})\n{}\n\n", i + 1, key, resp.content));
    }
    prompt.push_str(
        "Return exactly one line:\nSCORES_JSON: [{\"candidate\":1,\"score\":0.0},{\"candidate\":2,\"score\":0.0}]",
    );
    prompt
}

fn parse_moa_scores(content: &str) -> Vec<(usize, f64)> {
    let Some((_, json_part)) = content.split_once("SCORES_JSON:") else {
        return Vec::new();
    };
    let parsed: serde_json::Value = match serde_json::from_str(json_part.trim()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(items) = parsed.as_array() else {
        return Vec::new();
    };

    let mut scores: Vec<(usize, f64)> = items
        .iter()
        .filter_map(|item| {
            let candidate = item.get("candidate").and_then(|v| v.as_u64())?;
            let score = item.get("score").and_then(|v| v.as_f64())?;
            if candidate == 0 {
                return None;
            }
            Some(((candidate - 1) as usize, score.clamp(0.0, 1.0)))
        })
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
}
```

- [ ] **Step 4: Insert ranking before aggregation**

In `execute_moa`, before `build_moa_aggregation_prompt`, add:

```rust
let ranking_prompt = Self::build_moa_ranking_prompt(&proposals);
let ranking_resp = self
    .caller
    .call_prompt(aggregator_key, MOA_AGGREGATOR_SYSTEM, &ranking_prompt, Some(0.0))
    .await?;
total_input += ranking_resp.usage.input_tokens;
total_output += ranking_resp.usage.output_tokens;

let scores = Self::parse_moa_scores(&ranking_resp.content);
let ranked_proposals: Vec<(String, ModelResponse)> = if scores.is_empty() {
    proposals.clone()
} else {
    scores
        .into_iter()
        .filter(|(_, score)| *score >= 0.50)
        .filter_map(|(idx, _)| proposals.get(idx).cloned())
        .collect()
};

let proposals_for_aggregation = if ranked_proposals.is_empty() {
    proposals.clone()
} else {
    ranked_proposals
};
```

Then change:

```rust
let aggregation_prompt = Self::build_moa_aggregation_prompt(&proposals);
```

to:

```rust
let aggregation_prompt = Self::build_moa_aggregation_prompt(&proposals_for_aggregation);
```

- [ ] **Step 5: Enforce MoA threshold**

After `quality_result`:

```rust
if quality_result.score < quality_threshold {
    if let Some((model_key, best_resp)) = proposals_for_aggregation.first() {
        log::warn!(
            "[MoA] aggregate score {:.2} below threshold {:.2}; falling back to best proposal '{}'",
            quality_result.score,
            quality_threshold,
            model_key
        );
        return Ok(ExecutionResult {
            content: best_resp.content.clone(),
            model_used: best_resp.model.clone(),
            strategy: "moa_fallback_best_candidate".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            cascade_attempts: proposals.len() as u32,
            verified: false,
            judge_score: Some(quality_result.score),
        });
    }
}
```

- [ ] **Step 6: Run MoA tests**

```powershell
cargo test orchestration::executor::tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/orchestration/executor.rs
git commit -m "feat(orchestration): rank and gate MoA proposals"
```

## Task 9: Trace Ledger Built on HistoryStore

**Files:**
- Create: `src-tauri/src/orchestration/trace_ledger.rs`
- Modify: `src-tauri/src/orchestration/mod.rs`
- Modify: `src-tauri/src/orchestration/history.rs`
- Test: unit tests in `trace_ledger.rs`

- [ ] **Step 1: Create trace ledger with failing tests**

Create `src-tauri/src/orchestration/trace_ledger.rs`:

```rust
use crate::orchestration::executor::ExecutionResult;
use crate::orchestration::history::{ModelCall, OrchestrationRecord, QualityScore};
use crate::orchestration::{HistoryStore, TaskProfile};

pub struct TraceLedger {
    store: HistoryStore,
}

#[derive(Debug, Clone)]
pub struct TraceStep {
    pub step_type: String,
    pub model_key: String,
    pub provider: String,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub status: String,
    pub score: Option<f64>,
    pub error_kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskProfile, TaskType};
    use tempfile::TempDir;

    fn profile() -> TaskProfile {
        TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.8,
            risk: RiskLevel::High,
            verifiability: 0.9,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        }
    }

    #[test]
    fn trace_ledger_records_execution_result() {
        let dir = TempDir::new().unwrap();
        let ledger = TraceLedger::new(&dir.path().join("trace.db")).unwrap();
        let result = ExecutionResult {
            content: "answer".to_string(),
            model_used: "frontier".to_string(),
            strategy: "debate".to_string(),
            total_latency_ms: 1234,
            total_input_tokens: 100,
            total_output_tokens: 200,
            cascade_attempts: 3,
            verified: true,
            judge_score: Some(0.88),
        };

        let id = ledger
            .record_execution(&profile(), "raw prompt", &result, Vec::new())
            .unwrap();
        let stored = ledger.store().get_by_id(&id).unwrap().unwrap();

        assert_eq!(stored.strategy_used, "debate");
        assert_eq!(stored.final_quality, 0.88);
        assert!(stored.passed);
        assert_eq!(stored.total_latency_ms, 1234);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test orchestration::trace_ledger::tests
```

Expected: FAIL because module export and methods do not exist.

- [ ] **Step 3: Export trace ledger**

Modify `src-tauri/src/orchestration/mod.rs`:

```rust
pub mod trace_ledger;
pub use trace_ledger::{TraceLedger, TraceStep};
```

- [ ] **Step 4: Implement TraceLedger**

Append:

```rust
impl TraceLedger {
    pub fn new(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            store: HistoryStore::new(path)?,
        })
    }

    pub fn store(&self) -> &HistoryStore {
        &self.store
    }

    pub fn record_execution(
        &self,
        profile: &TaskProfile,
        raw_prompt: &str,
        result: &ExecutionResult,
        steps: Vec<TraceStep>,
    ) -> Result<String, String> {
        let mut record = OrchestrationRecord::new(
            &format!("{:?}", profile.task_type).to_ascii_lowercase(),
            profile.complexity,
            &format!("{:?}", profile.risk).to_ascii_lowercase(),
            raw_prompt,
            &result.strategy,
        );

        record.models_called = if steps.is_empty() {
            vec![ModelCall {
                model_key: result.model_used.clone(),
                provider: "unknown".to_string(),
                latency_ms: result.total_latency_ms,
                cost_usd: 0.0,
                quality_score: result.judge_score.unwrap_or(0.0),
                was_selected: true,
            }]
        } else {
            steps
                .iter()
                .map(|step| ModelCall {
                    model_key: step.model_key.clone(),
                    provider: step.provider.clone(),
                    latency_ms: step.latency_ms,
                    cost_usd: 0.0,
                    quality_score: step.score.unwrap_or(0.0),
                    was_selected: step.status == "selected",
                })
                .collect()
        };

        record.quality_scores = vec![QualityScore {
            tool_name: "judge".to_string(),
            score: result.judge_score.unwrap_or(0.0),
        }];
        record.final_quality = result.judge_score.unwrap_or(0.0);
        record.passed = result.verified;
        record.total_latency_ms = result.total_latency_ms;
        record.total_input_tokens = result.total_input_tokens;
        record.total_output_tokens = result.total_output_tokens;
        record.escalation_count = result.cascade_attempts.saturating_sub(1);

        let id = record.id.clone();
        self.store.record(&record)?;
        Ok(id)
    }
}
```

- [ ] **Step 5: Run trace tests**

```powershell
cargo test orchestration::trace_ledger::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/orchestration/trace_ledger.rs src-tauri/src/orchestration/mod.rs
git commit -m "feat(orchestration): add trace ledger"
```

## Task 10: Engine and Proxy Integration Guards

**Files:**
- Modify: `src-tauri/src/orchestration/engine.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: unit tests in `engine.rs` and `handlers.rs`

- [ ] **Step 1: Add streaming/tools passthrough tests**

In `engine.rs`, add:

```rust
#[tokio::test]
async fn streaming_request_passthrough_even_when_strategy_matches() {
    let yaml = r#"
enabled: true
models:
  cheap_coder:
    provider: deepseek
    model: deepseek-chat
    api_key_env: DEEPSEEK_API_KEY
strategies:
  route:
    priority: 10
    description: "Direct route"
    when:
      complexity: [0, 1]
    action:
      type: route
      use_model: cheap_coder
"#;
    let (engine, _dir) = create_engine_with_yaml(yaml);
    let body = json!({
        "stream": true,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let decision = engine.decide(&body).await;
    assert!(matches!(decision, OrchestrationDecision::Passthrough));
}
```

- [ ] **Step 2: Run test**

```powershell
cargo test orchestration::engine::tests::streaming_request_passthrough_even_when_strategy_matches
```

Expected: PASS after Task 2 and Task 3. If it fails, ensure `StrategySelector::select_detailed` rejects ineligible profiles.

- [ ] **Step 3: Ensure proxy does not force advanced orchestration for streaming/tools**

In `src-tauri/src/proxy/handlers.rs`, find the function that checks orchestration eligibility, likely near `should_try_orchestrate` or `should_use_claude_transform_streaming`. Ensure the guard is exactly:

```rust
fn should_try_orchestrate(is_streaming: bool, has_tools: bool) -> bool {
    !is_streaming && !has_tools
}
```

If the function already exists with that logic, do not change it. Add or update tests:

```rust
#[test]
fn should_not_orchestrate_streaming_or_tool_requests() {
    assert!(!should_try_orchestrate(true, false));
    assert!(!should_try_orchestrate(false, true));
    assert!(!should_try_orchestrate(true, true));
    assert!(should_try_orchestrate(false, false));
}
```

- [ ] **Step 4: Run proxy handler tests**

```powershell
cargo test proxy::handlers::tests::should_not_orchestrate_streaming_or_tool_requests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/orchestration/engine.rs src-tauri/src/proxy/handlers.rs
git commit -m "feat(orchestration): guard streaming and tool requests"
```

## Task 11: End-to-End Mock Tests

**Files:**
- Create: `src-tauri/tests/orchestration_release_core.rs`
- Modify: `src-tauri/src/orchestration/model_caller.rs` only if needed for mock seam

- [ ] **Step 1: Create integration test skeleton**

Create `src-tauri/tests/orchestration_release_core.rs`:

```rust
use ec_switch_lib::orchestration::quality_gate::{QualityAction, QualityDecision, QualityResult};
use ec_switch_lib::orchestration::selector::StrategySelector;
use ec_switch_lib::orchestration::{
    OrchestrationConfig, OrchestrationEngine,
};
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
    let result = QualityResult {
        passed: false,
        score: 0.40,
        individual_scores: vec![("judge".to_string(), 0.40)],
    };
    let decision = QualityDecision::from_result(&result, 0.80);

    assert_eq!(decision.action, QualityAction::Fallback);
    assert!(decision.reason.contains("below threshold"));
}
```

- [ ] **Step 2: Run integration test**

```powershell
cargo test --test orchestration_release_core
```

Expected: PASS after previous tasks.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/tests/orchestration_release_core.rs
git commit -m "test(orchestration): add release core integration coverage"
```

## Task 12: Minimal Eval Harness

**Files:**
- Create: `src-tauri/src/orchestration/eval_harness.rs`
- Modify: `src-tauri/src/orchestration/mod.rs`
- Create: `src-tauri/tests/orchestration_eval_harness.rs`

- [ ] **Step 1: Create eval harness module**

Create `src-tauri/src/orchestration/eval_harness.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub expected_strategy: String,
    pub min_quality: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub case_id: String,
    pub strategy_used: String,
    pub quality_score: f64,
    pub passed: bool,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub struct MiniEvalHarness {
    cases: Vec<EvalCase>,
}

impl MiniEvalHarness {
    pub fn default_cases() -> Vec<EvalCase> {
        vec![
            EvalCase {
                id: "simple_text_route".to_string(),
                name: "Simple text should route cheaply".to_string(),
                prompt: "Explain what HTTP status 404 means.".to_string(),
                expected_strategy: "route".to_string(),
                min_quality: 0.60,
            },
            EvalCase {
                id: "coding_cascade".to_string(),
                name: "Coding task should allow cascade".to_string(),
                prompt: "Write a Rust function that validates balanced parentheses.".to_string(),
                expected_strategy: "cascade".to_string(),
                min_quality: 0.70,
            },
            EvalCase {
                id: "high_risk_debate".to_string(),
                name: "High-risk architecture task should use debate".to_string(),
                prompt: "Design a payment retry system with idempotency and failure recovery.".to_string(),
                expected_strategy: "debate".to_string(),
                min_quality: 0.80,
            },
            EvalCase {
                id: "critical_moa".to_string(),
                name: "Critical complex synthesis should use MoA".to_string(),
                prompt: "Compare three database migration strategies and choose the safest rollout plan.".to_string(),
                expected_strategy: "moa".to_string(),
                min_quality: 0.82,
            },
        ]
    }

    pub fn new(cases: Vec<EvalCase>) -> Self {
        Self { cases }
    }

    pub fn cases(&self) -> &[EvalCase] {
        &self.cases
    }

    pub fn summarize(results: &[EvalResult]) -> EvalSummary {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let avg_quality = if total == 0 {
            0.0
        } else {
            results.iter().map(|r| r.quality_score).sum::<f64>() / total as f64
        };
        EvalSummary {
            total,
            passed,
            pass_rate: if total == 0 { 0.0 } else { passed as f64 / total as f64 },
            avg_quality,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total: usize,
    pub passed: usize,
    pub pass_rate: f64,
    pub avg_quality: f64,
}
```

- [ ] **Step 2: Export eval harness**

Modify `src-tauri/src/orchestration/mod.rs`:

```rust
pub mod eval_harness;
pub use eval_harness::{EvalCase, EvalResult, EvalSummary, MiniEvalHarness};
```

- [ ] **Step 3: Add tests**

Create `src-tauri/tests/orchestration_eval_harness.rs`:

```rust
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
```

- [ ] **Step 4: Run eval tests**

```powershell
cargo test --test orchestration_eval_harness
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/orchestration/eval_harness.rs src-tauri/src/orchestration/mod.rs src-tauri/tests/orchestration_eval_harness.rs
git commit -m "feat(orchestration): add minimal eval harness"
```

## Task 13: Full Verification

**Files:**
- No source edits unless verification exposes a defect in files changed by previous tasks.

- [ ] **Step 1: Run Rust orchestration tests**

```powershell
cargo test orchestration
```

Expected: PASS.

- [ ] **Step 2: Run Rust integration tests**

```powershell
cargo test --test orchestration_release_core
cargo test --test orchestration_eval_harness
```

Expected: PASS.

- [ ] **Step 3: Run full Rust test suite**

```powershell
cargo test
```

Expected: PASS.

- [ ] **Step 4: Run frontend typecheck**

Use `pnpm.cmd`, not `pnpm`, because PowerShell script execution policy can block `pnpm.ps1`.

```powershell
pnpm.cmd typecheck
```

Expected: PASS.

- [ ] **Step 5: Run frontend unit tests**

```powershell
pnpm.cmd test:unit
```

Expected: PASS.

- [ ] **Step 6: Run production renderer build**

```powershell
pnpm.cmd build:renderer
```

Expected: PASS. Existing chunk-size warnings are acceptable if no new build failure appears.

- [ ] **Step 7: Run Rust build**

```powershell
cargo build
```

Expected: PASS. New orchestration code should not add new dead-code warnings for modules that are part of the Milestone 1 path.

- [ ] **Step 8: Commit verification fixes only if needed**

If a verification failure required a fix:

```powershell
git add <exact files fixed>
git commit -m "fix(orchestration): stabilize release core verification"
```

If no fix was needed, do not create an empty commit.

## Self-Review

Spec coverage:

- Deterministic selector: Task 3.
- Provider integration: Task 4 and Task 5.
- Debate threshold and structured flow: Task 7.
- MoA ranking/filtering/threshold fallback: Task 8.
- Traceability: Task 9.
- Streaming/tools/image/audio safety guards: Task 2 and Task 10.
- End-to-end mock-style release tests: Task 11.
- Minimal eval harness: Task 12.
- Verification: Task 13.

Known scope gaps intentionally deferred to separate plans:

- Full image+text CrossModal execution.
- Full audio STT/TTS execution.
- Frontend trace dashboard.
- True token-level streaming for Debate/MoA/Cascade.

Type consistency:

- `TaskProfile` fields are used consistently across selector, engine, and tests.
- `QualityDecision`, `QualityAction`, and `QualityResult` names match across tasks.
- `ModelCallTarget`, `ModelCallError`, and `ModelCallErrorKind` names match across tasks.
- `TraceLedger`, `TraceStep`, `EvalCase`, `EvalResult`, and `MiniEvalHarness` are exported from `orchestration/mod.rs`.

Execution guardrail:

- GLM5.1 should complete tasks in order. If a later task needs a change in an earlier file, update the earlier module through the smallest compatible edit and rerun that module's tests before continuing.

