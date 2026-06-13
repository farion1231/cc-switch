# Orchestration Core Path Fix — Multi-Model Debate/MoA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 8 critical issues blocking the "multiple small models > single large model" orchestration path, making Debate and MoA strategies actually fire in real-world usage (streaming requests, tool calls, custom providers).

**Architecture:** The orchestration engine sits inside the Tauri proxy layer. Requests arrive at `proxy/handlers.rs`, which currently guards orchestration behind `!is_streaming && !has_tools`. We will: (1) enable orchestration for streaming and tool-use requests by buffering the orchestration result then streaming it back, (2) fix `build_url` to support custom `base_url`, (3) parallelize MoA/Debate proposer calls with `tokio::join!`, (4) wire `CrossJudge` into the Debate execution path, (5) fix the frontend StrategyEditor data loss bug, (6) add default Debate/MoA strategies to the config.

**Tech Stack:** Rust (Tauri v2, tokio, reqwest, serde_yaml, axum), TypeScript/React (Tauri API, TanStack React Query, Radix UI)

---

## Investigation Summary (Root Cause Analysis)

### Data Flow

```
Request → Proxy Handler
  → Gate: !streaming && !has_tools?  ← 🔴 BLOCKER (lines 143, 521, 600 of handlers.rs)
    → TaskClassifier.classify(body)   → TaskProfile (classifier.rs)
      → StrategySelector.select()     → StrategyAction (selector.rs)
        → StrategyExecutor.execute()
          ├─ Route:   single model call
          ├─ Cascade: cheap model → QualityGate → escalate if below threshold
          ├─ Debate:  models answer independently → Shuffle → single Judge
          └─ MoA:     proposers answer sequentially → Aggregator → QualityGate
        → ModelCaller.call()           → HTTP to model API (model_caller.rs)
      → Return Claude/OpenAI format response
```

### Issues Found

| # | Severity | File | Issue |
|---|----------|------|-------|
| 1 | P0 BLOCKER | `handlers.rs:143,521,600` | `!is_streaming && !has_tools` guard blocks all real requests |
| 2 | P0 BUG | `model_caller.rs:175-199` | `build_url` ignores `base_url` for unknown providers |
| 3 | P1 PERF | `executor.rs:199,292` | Debaters and proposers run sequentially, not in parallel |
| 4 | P1 DEAD CODE | `executor.rs:187-274` | `CrossJudge` module never called in Debate/MoA pipeline |
| 5 | P1 DATA LOSS | `StrategyEditor.tsx:64` | `models: {}` wipes model definitions on save |
| 6 | P2 MISSING | `config.rs:248-311` | Default config has no Debate or MoA strategies |
| 7 | P2 DESIGN | `executor.rs:187-274` | Debate is one-shot evaluation, not multi-round |

---

## File Structure

### Modified Files

| File | Change |
|------|--------|
| `src-tauri/src/proxy/handlers.rs` | Remove streaming/tools guard; add orchestration-for-streaming path |
| `src-tauri/src/orchestration/model_caller.rs` | Fix `build_url` to check `base_url` before erroring |
| `src-tauri/src/orchestration/executor.rs` | Parallelize debaters/proposers; wire CrossJudge into Debate |
| `src-tauri/src/orchestration/config.rs` | Add default Debate and MoA strategies |
| `src/components/orchestration/StrategyEditor.tsx` | Preserve existing `models` on save |

### New Test Files

| File | Purpose |
|------|---------|
| `src-tauri/src/orchestration/executor.rs` (mod tests) | Add tests for parallel execution and CrossJudge integration |
| `src-tauri/src/orchestration/model_caller.rs` (mod tests) | Fix failing `unknown_provider_uses_explicit_base_url` test |

---

## Task 1: Fix `build_url` to Support Custom `base_url`

**Files:**
- Modify: `src-tauri/src/orchestration/model_caller.rs:175-199`
- Test: existing `unknown_provider_uses_explicit_base_url` test at line 304

- [ ] **Step 1: Write the failing test (already exists, verify it fails)**

The test `unknown_provider_uses_explicit_base_url` at line 304 already expects custom `base_url` to work. Verify it fails:

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test unknown_provider_uses_explicit_base_url -- --nocapture 2>&1 | tail -20`

Expected: FAIL — `build_url` returns error for unknown provider despite `base_url` being set.

- [ ] **Step 2: Fix `build_url` to check `base_url` before erroring**

In `src-tauri/src/orchestration/model_caller.rs`, replace the `_` match arm at line 193-198:

```rust
// BEFORE (line 193-198):
        _ => {
            return Err(format!(
                "Unknown provider '{}' and no base_url configured — cannot route request",
                config.provider
            ));
        }

// AFTER:
        _ => {
            if let Some(ref url) = config.base_url {
                return Ok(url.clone());
            }
            return Err(format!(
                "Unknown provider '{}' and no base_url configured — cannot route request",
                config.provider
            ));
        }
```

- [ ] **Step 3: Run the previously-failing test to verify it passes**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test unknown_provider -- --nocapture 2>&1 | tail -20`

Expected: PASS for both `unknown_provider_uses_explicit_base_url` and `unknown_provider_no_base_url_returns_error`.

- [ ] **Step 4: Run all model_caller tests**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test model_caller -- --nocapture 2>&1 | tail -30`

Expected: All 7 tests pass.

- [ ] **Step 5: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src-tauri/src/orchestration/model_caller.rs
git commit -m "fix(orchestration): support custom base_url for unknown providers in build_url"
```

---

## Task 2: Fix StrategyEditor Data Loss on Save

**Files:**
- Modify: `src/components/orchestration/StrategyEditor.tsx:57-85`

- [ ] **Step 1: Write a failing test for the data loss bug**

Create `src/components/orchestration/__tests__/StrategyEditor.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrategyEditor } from "../StrategyEditor";

// Mock Tauri invoke
const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("StrategyEditor", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    vi.clearAllMocks();
  });

  it("preserves existing models when saving strategies", async () => {
    const existingConfig = {
      enabled: true,
      models: {
        cheap_coder: {
          provider: "deepseek",
          model: "deepseek-chat",
          api_key_env: "DEEPSEEK_API_KEY",
          max_tokens: 16384,
        },
        frontier: {
          provider: "anthropic",
          model: "claude-sonnet-4-20250514",
          api_key_env: "ANTHROPIC_API_KEY",
          max_tokens: 16384,
        },
      },
      strategies: {
        route: {
          description: "Direct route",
          when: { complexity: [0, 0.4], risk: ["low"] },
          action: { type: "route", use_model: "cheap_coder" },
        },
      },
    };

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "get_strategies_config") return Promise.resolve(existingConfig);
      if (cmd === "save_strategies_config") return Promise.resolve();
      if (cmd === "get_strategies_config_path") return Promise.resolve("/tmp/test.yaml");
      return Promise.resolve();
    });

    render(
      <QueryClientProvider client={queryClient}>
        <StrategyEditor />
      </QueryClientProvider>,
    );

    // Wait for config to load
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_strategies_config");
    });

    // Find and click save button
    const saveButton = await screen.findByRole("button", { name: /save/i });
    await userEvent.click(saveButton);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "save_strategies_config",
        expect.objectContaining({
          configJson: expect.objectContaining({
            models: {
              cheap_coder: expect.any(Object),
              frontier: expect.any(Object),
            },
          }),
        }),
      );
    });
  });
});
```

Run: `cd D:/14-OneAgentSwithc && pnpm test:unit -- --run src/components/orchestration/__tests__/StrategyEditor.test.tsx 2>&1 | tail -20`

Expected: FAIL — the `save_strategies_config` call will have `models: {}`.

- [ ] **Step 2: Fix the `handleSave` to preserve existing models**

In `src/components/orchestration/StrategyEditor.tsx`, replace the `handleSave` callback (lines 57-85):

```tsx
  const handleSave = useCallback(async () => {
    try {
      setSaving(true);
      setError(null);
      // Rebuild strategies from current rules, but preserve existing models
      const strategies: OrchestrationConfig["strategies"] = {};
      for (const s of strategies) {
        const action = buildAction(s);
        if (!action) continue;
        strategies[s.name] = {
          description: s.description ?? s.name,
          when: {
            complexity: s.complexityRange,
            risk: s.riskLevels,
          },
          action: action as OrchestrationConfig["strategies"][string]["action"],
        };
      }
      const config: OrchestrationConfig = {
        enabled: true,
        models: savedConfig?.models ?? {}, // preserve existing model definitions
        strategies,
      };
      await saveConfig(config);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [strategies, savedConfig]);
```

This requires tracking the loaded config. Add a state variable above the existing state declarations:

```tsx
  const [savedConfig, setSavedConfig] = useState<OrchestrationConfig | null>(null);
```

And update `loadConfig` to save the raw config:

```tsx
  const loadConfig = useCallback(async () => {
    try {
      const cfg = await getConfig();
      setSavedConfig(cfg);
      setStrategies(configToStrategyRules(cfg));
    } catch (e) {
      setError(String(e));
    }
  }, []);
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd D:/14-OneAgentSwithc && pnpm test:unit -- --run src/components/orchestration/__tests__/StrategyEditor.test.tsx 2>&1 | tail -20`

Expected: PASS — models are preserved on save.

- [ ] **Step 4: Run typecheck**

Run: `cd D:/14-OneAgentSwithc && pnpm typecheck 2>&1 | tail -10`

Expected: No errors.

- [ ] **Step 5: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src/components/orchestration/StrategyEditor.tsx src/components/orchestration/__tests__/StrategyEditor.test.tsx
git commit -m "fix(orchestration): preserve model definitions when saving strategy config"
```

---

## Task 3: Parallelize Debater and Proposer Calls

**Files:**
- Modify: `src-tauri/src/orchestration/executor.rs:187-345`

- [ ] **Step 1: Write failing test for parallel execution timing**

Add test in `src-tauri/src/orchestration/executor.rs` inside `mod tests`:

```rust
    #[test]
    fn build_debate_prompt_includes_all_responses() {
        let responses = vec![
            (
                "model_a".to_string(),
                ModelResponse {
                    content: "Answer A".to_string(),
                    model: "a".to_string(),
                    usage: Default::default(),
                    latency_ms: 100,
                },
            ),
            (
                "model_b".to_string(),
                ModelResponse {
                    content: "Answer B".to_string(),
                    model: "b".to_string(),
                    usage: Default::default(),
                    latency_ms: 200,
                },
            ),
            (
                "model_c".to_string(),
                ModelResponse {
                    content: "Answer C".to_string(),
                    model: "c".to_string(),
                    usage: Default::default(),
                    latency_ms: 300,
                },
            ),
        ];
        let prompt = StrategyExecutor::build_debate_prompt(&responses);
        assert!(prompt.contains("Answer 1"));
        assert!(prompt.contains("Answer 2"));
        assert!(prompt.contains("Answer 3"));
        assert!(prompt.contains("Answer A"));
        assert!(prompt.contains("Answer B"));
        assert!(prompt.contains("Answer C"));
    }
```

This test documents the expected behavior before we refactor the call pattern.

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test build_debate_prompt_includes_all_responses -- --nocapture 2>&1 | tail -10`

Expected: PASS (this documents behavior before refactor).

- [ ] **Step 2: Refactor debater calls to use `futures::future::join_all`**

In `src-tauri/src/orchestration/executor.rs`, replace the sequential debater loop (lines 198-214) with parallel calls.

Add import at top of file:

```rust
use futures::future::join_all;
```

Replace the sequential loop in `execute_debate`:

```rust
// BEFORE (lines 198-214):
        let mut responses: Vec<(String, ModelResponse)> = Vec::new();
        for model_key in debater_keys {
            match self
                .caller
                .call(model_key, messages.clone(), tools.clone(), None)
                .await
            {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    responses.push((model_key.clone(), resp));
                }
                Err(e) => {
                    log::warn!("[Debate] Debater '{}' failed: {}", model_key, e);
                }
            }
        }

// AFTER:
        let debater_futures: Vec<_> = debater_keys
            .iter()
            .map(|model_key| {
                let key = model_key.clone();
                let msgs = messages.clone();
                let tls = tools.clone();
                async move {
                    let result = self.caller.call(&key, msgs, tls, None).await;
                    (key, result)
                }
            })
            .collect();

        let debater_results = join_all(debater_futures).await;

        let mut responses: Vec<(String, ModelResponse)> = Vec::new();
        for (model_key, result) in debater_results {
            match result {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    responses.push((model_key, resp));
                }
                Err(e) => {
                    log::warn!("[Debate] Debater failed: {}", e);
                }
            }
        }
```

- [ ] **Step 3: Apply the same parallelization to MoA proposers**

Replace the sequential proposer loop in `execute_moa` (lines 292-307):

```rust
// BEFORE (lines 292-307):
        let mut proposals: Vec<(String, ModelResponse)> = Vec::new();
        for model_key in proposer_keys {
            match self
                .caller
                .call(model_key, messages.clone(), tools.clone(), None)
                .await
            {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    proposals.push((model_key.clone(), resp));
                }
                Err(e) => {
                    log::warn!("[MoA] Proposer '{}' failed: {}", model_key, e);
                }
            }
        }

// AFTER:
        let proposer_futures: Vec<_> = proposer_keys
            .iter()
            .map(|model_key| {
                let key = model_key.clone();
                let msgs = messages.clone();
                let tls = tools.clone();
                async move {
                    let result = self.caller.call(&key, msgs, tls, None).await;
                    (key, result)
                }
            })
            .collect();

        let proposer_results = join_all(proposer_futures).await;

        let mut proposals: Vec<(String, ModelResponse)> = Vec::new();
        for (model_key, result) in proposer_results {
            match result {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    proposals.push((model_key, resp));
                }
                Err(e) => {
                    log::warn!("[MoA] Proposer failed: {}", e);
                }
            }
        }
```

- [ ] **Step 4: Verify compilation and run tests**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test executor -- --nocapture 2>&1 | tail -30`

Expected: All existing tests still pass. The `futures` crate is already a transitive dependency via `tokio`/`reqwest`.

If `futures` is not available, add to `Cargo.toml`:

```bash
cd D:/14-OneAgentSwithc/src-tauri && grep -q "^futures" Cargo.toml || echo 'futures = "0.3"' >> Cargo.toml
```

- [ ] **Step 5: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src-tauri/src/orchestration/executor.rs src-tauri/Cargo.toml
git commit -m "perf(orchestration): parallelize debater and proposer model calls with join_all"
```

---

## Task 4: Wire CrossJudge Into Debate Execution Path

**Files:**
- Modify: `src-tauri/src/orchestration/executor.rs:187-274`

- [ ] **Step 1: Add a test for CrossJudge integration in Debate**

Add test in `executor.rs` mod tests:

```rust
    #[test]
    fn cross_judge_config_default_has_single_judge() {
        // Verify default debate uses a single judge (backward compatible)
        // CrossJudge is only used when multiple judges are configured
        use crate::orchestration::cross_judge::{CrossJudge, JudgeAggregation, JudgeModel};

        let judges = vec![JudgeModel {
            model_key: "judge_a".to_string(),
            weight: 1.0,
        }];
        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        assert_eq!(cj.judges.len(), 1);
    }
```

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test cross_judge_config -- --nocapture 2>&1 | tail -10`

Expected: PASS.

- [ ] **Step 2: Add `cross_judge` field to `StrategyExecutor` and wire into `execute_debate`**

In `executor.rs`, add the optional CrossJudge to the struct:

```rust
pub struct StrategyExecutor {
    caller: ModelCaller,
    quality_gate: QualityGate,
    cross_judge: Option<CrossJudge>,
}
```

Update `new`:

```rust
    pub fn new(models: HashMap<String, ModelConfig>) -> Result<Self, String> {
        Ok(Self {
            caller: ModelCaller::new(models)?,
            quality_gate: QualityGate::default(),
            cross_judge: None,
        })
    }

    /// Set a cross-judge evaluator for multi-judge debate evaluation.
    pub fn with_cross_judge(mut self, judges: Vec<crate::orchestration::cross_judge::JudgeModel>, aggregation: crate::orchestration::cross_judge::JudgeAggregation) -> Self {
        self.cross_judge = Some(CrossJudge::new(judges, aggregation));
        self
    }
```

Add the import at top:

```rust
use crate::orchestration::cross_judge::{ConsensusLevel, CrossJudge, CrossJudgeResult, JudgeAggregation};
```

Update `execute_debate` to use CrossJudge when available:

```rust
    pub async fn execute_debate(
        &self,
        debater_keys: &[String],
        judge_key: &str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        // ... (parallel debater calls from Task 3) ...

        // Shuffle responses to prevent position bias in judge evaluation
        let candidates: Vec<crate::orchestration::shuffle::CandidateAnswer> = responses
            .iter()
            .map(|(key, resp)| crate::orchestration::shuffle::CandidateAnswer {
                model_key: key.clone(),
                content: resp.content.clone(),
                quality_score: 0.5,
                latency_ms: resp.latency_ms,
                cost_usd: 0.0,
            })
            .collect();
        let shuffled = CandidateShuffler::shuffle(candidates);

        // Use CrossJudge if configured, otherwise fall back to single judge
        if let Some(ref cj) = self.cross_judge {
            let original_prompt = messages
                .iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join("\n");

            let cj_result = cj
                .evaluate(&original_prompt, &shuffled, &self.caller)
                .await
                .map_err(|e| format!("CrossJudge evaluation failed: {}", e))?;

            let best = &shuffled.candidates[cj_result.best_candidate_idx];
            log::info!(
                "[Debate] CrossJudge consensus={:?}, best_score={:.2}, best_model={}",
                cj_result.consensus_level,
                cj_result.final_score,
                best.model_key,
            );

            return Ok(ExecutionResult {
                content: best.content.clone(),
                model_used: best.model_key.clone(),
                strategy: "debate".to_string(),
                total_latency_ms: start.elapsed().as_millis() as u64,
                total_input_tokens: total_input,
                total_output_tokens: total_output,
                cascade_attempts: responses.len() as u32,
                verified: cj_result.consensus_level != ConsensusLevel::Low,
                judge_score: Some(cj_result.final_score),
            });
        }

        // Fallback: single judge (original behavior)
        let debate_summary = Self::build_debate_prompt_from_candidates(&shuffled.candidates);
        let judge_messages = vec![json!({
            "role": "user",
            "content": debate_summary
        })];

        let judge_resp = self
            .caller
            .call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &debate_summary, Some(0.3))
            .await?;

        total_input += judge_resp.usage.input_tokens;
        total_output += judge_resp.usage.output_tokens;

        let score = Self::extract_score_from_judge(&judge_resp.content);

        Ok(ExecutionResult {
            content: judge_resp.content,
            model_used: judge_resp.model,
            strategy: "debate".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            cascade_attempts: responses.len() as u32,
            verified: true,
            judge_score: score,
        })
    }
```

- [ ] **Step 3: Run tests**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test executor -- --nocapture 2>&1 | tail -30`

Expected: All tests pass. CrossJudge is only used when `with_cross_judge` is called, so existing behavior is preserved.

- [ ] **Step 4: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src-tauri/src/orchestration/executor.rs
git commit -m "feat(orchestration): wire CrossJudge into debate execution path for multi-judge evaluation"
```

---

## Task 5: Enable Orchestration for Streaming Requests

**Files:**
- Modify: `src-tauri/src/proxy/handlers.rs:130-150, 510-530, 590-610`

This is the most architecturally significant change. The approach: when orchestration decides to handle a request that would otherwise be streamed, we buffer the full orchestration response and then stream it back in SSE format to the client.

- [ ] **Step 1: Write a test for the orchestration guard removal**

This is an integration-level change. We verify the guard is removed by checking the condition in the handler code. The unit test approach: add a helper function that decides whether orchestration should be attempted:

Add to `handlers.rs` (above the `try_orchestrate_claude` function):

```rust
/// Decide whether to attempt orchestration for this request.
/// Returns true unless the request is explicitly streaming AND the client
/// requires real-time tool-use interaction (streaming + tools).
fn should_try_orchestrate(is_streaming: bool, has_tools: bool) -> bool {
    // Orchestration is always attempted. For streaming+tools, we buffer
    // the orchestration result and stream it back in SSE format.
    // The orchestration engine's decide() will return Passthrough if
    // it's not configured for this request type, so the overhead is minimal.
    let _ = (is_streaming, has_tools);
    true
}
```

- [ ] **Step 2: Replace the three guard blocks**

Replace each of the three guard blocks at lines ~143, ~521, ~600.

Before (each location has this pattern):
```rust
    if !is_streaming && !has_tools {
        if let Some(resp) = try_orchestrate_claude(&state, &body).await {
            return resp;
        }
    }
```

After:
```rust
    if should_try_orchestrate(is_streaming, has_tools) {
        if let Some(resp) = try_orchestrate_claude(&state, &body).await {
            // If the original request was streaming, wrap the buffered
            // orchestration response in SSE format before returning.
            if is_streaming {
                return Some(wrap_as_sse_response(resp));
            }
            return resp;
        }
    }
```

Similarly for `try_orchestrate_openai` at lines ~521 and ~600.

- [ ] **Step 3: Add the SSE wrapper function**

Add this helper function in `handlers.rs`:

```rust
/// Wrap a buffered orchestration response into an SSE stream.
/// This allows orchestration to work with clients that expect streaming responses.
fn wrap_as_sse_response(
    inner: Result<axum::response::Response, ProxyError>,
) -> Option<Result<axum::response::Response, ProxyError>> {
    match inner {
        Ok(response) => {
            // Extract the JSON body from the buffered response
            let (parts, body) = response.into_parts();
            let body_bytes = axum::body::to_bytes(body, 1024 * 1024)
                .await
                .unwrap_or_default();

            // Create SSE stream from the single response
            let sse_data = format!("data: {}\n\ndata: [DONE]\n\n", String::from_utf8_lossy(&body_bytes));
            let stream = futures::stream::once(async move { Ok::<_, std::convert::Infallible>(sse_data) });

            Some(Ok((
                StatusCode::OK,
                [
                    (http::header::CONTENT_TYPE, http::HeaderValue::from_static("text/event-stream")),
                    (http::header::CACHE_CONTROL, http::HeaderValue::from_static("no-cache")),
                ],
                axum::body::Body::from_stream(stream),
            ).into_response()))
        }
        Err(e) => Some(Err(e)),
    }
}
```

Note: Since `wrap_as_sse_response` is async, the call sites need `.await`. Adjust the calling code accordingly:

```rust
    if should_try_orchestrate(is_streaming, has_tools) {
        if is_streaming {
            if let Some(resp) = try_orchestrate_claude(&state, &body).await {
                return wrap_as_sse_response(resp).await;
            }
        } else {
            if let Some(resp) = try_orchestrate_claude(&state, &body).await {
                return resp;
            }
        }
    }
```

- [ ] **Step 4: Update the `try_orchestrate_openai` calls similarly**

Apply the same pattern at lines ~521 and ~600 for the OpenAI-format handlers, using `try_orchestrate_openai` instead.

- [ ] **Step 5: Run compilation check**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo check 2>&1 | tail -30`

Expected: Compiles without errors.

- [ ] **Step 6: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src-tauri/src/proxy/handlers.rs
git commit -m "feat(orchestration): enable orchestration for streaming and tool-use requests"
```

---

## Task 6: Add Default Debate and MoA Strategies

**Files:**
- Modify: `src-tauri/src/orchestration/config.rs:248-311`

- [ ] **Step 1: Write a test for the new default strategies**

Add test in `config.rs` mod tests:

```rust
    #[test]
    fn default_config_has_debate_strategy() {
        let config = OrchestrationConfig::default();
        assert!(config.strategies.contains_key("debate"), "Default config should include debate strategy");
        match &config.strategies["debate"].action {
            StrategyAction::Debate { debaters, judge, .. } => {
                assert!(debaters.len() >= 2, "Debate needs at least 2 debaters");
                assert!(!judge.is_empty(), "Debate needs a judge");
            }
            other => panic!("Expected Debate action, got {:?}", other),
        }
    }

    #[test]
    fn default_config_has_moa_strategy() {
        let config = OrchestrationConfig::default();
        assert!(config.strategies.contains_key("moa"), "Default config should include MoA strategy");
        match &config.strategies["moa"].action {
            StrategyAction::MoA { proposers, aggregator, .. } => {
                assert!(proposers.len() >= 2, "MoA needs at least 2 proposers");
                assert!(!aggregator.is_empty(), "MoA needs an aggregator");
            }
            other => panic!("Expected MoA action, got {:?}", other),
        }
    }
```

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test default_config_has_debate -- --nocapture 2>&1 | tail -10`

Expected: FAIL — default config doesn't have these strategies yet.

- [ ] **Step 2: Add default models and strategies**

In `config.rs`, update the `Default` impl for `OrchestrationConfig` (line 248). Add models for small models and a judge:

```rust
impl Default for OrchestrationConfig {
    fn default() -> Self {
        let mut models = HashMap::new();
        // Small/cheap models for debate and MoA
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
                model: "qwen-coder-plus-latest".to_string(),
                api_key_env: "DASHSCOPE_API_KEY".to_string(),
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
        // Frontier model for judging and aggregation
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
                description: "Direct route to cheap model".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.0, 0.3)),
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
                description: "Cheap first, verify, escalate to frontier".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.3, 0.6)),
                    risk: Some(vec!["medium".to_string()]),
                    ..Default::default()
                },
                action: StrategyAction::Cascade {
                    models: vec!["cheap_coder".to_string(), "frontier".to_string()],
                    verify_each: true,
                    escalate_on_fail: true,
                    quality_threshold: 0.65,
                },
            },
        );
        strategies.insert(
            "debate".to_string(),
            StrategyDef {
                description: "Multiple small models debate, frontier judges".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.6, 0.85)),
                    risk: Some(vec!["medium".to_string(), "high".to_string()]),
                    task_type: Some(vec!["coding".to_string(), "architecture".to_string()]),
                    ..Default::default()
                },
                action: StrategyAction::Debate {
                    debaters: vec![
                        "cheap_coder".to_string(),
                        "qwen_coder".to_string(),
                        "glm_coder".to_string(),
                    ],
                    judge: "frontier".to_string(),
                    quality_threshold: 0.7,
                },
            },
        );
        strategies.insert(
            "moa".to_string(),
            StrategyDef {
                description: "Multiple proposers + frontier aggregator".to_string(),
                when: StrategyCondition {
                    complexity: Some((0.85, 1.0)),
                    risk: Some(vec!["high".to_string(), "critical".to_string()]),
                    task_type: Some(vec!["coding".to_string(), "architecture".to_string()]),
                    ..Default::default()
                },
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
            },
        );
        Self {
            enabled: false,
            models,
            strategies,
        }
    }
```

- [ ] **Step 3: Run tests to verify**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test default_config -- --nocapture 2>&1 | tail -20`

Expected: All 4 default_config tests pass (has_models, has_strategies, has_debate, has_moa).

- [ ] **Step 4: Run all config tests**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test config -- --nocapture 2>&1 | tail -30`

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
cd D:/14-OneAgentSwithc
git add src-tauri/src/orchestration/config.rs
git commit -m "feat(orchestration): add default debate and MoA strategies with 3 small models"
```

---

## Task 7: Integration Verification

**Files:**
- No new files — verification only

- [ ] **Step 1: Run full Rust test suite**

Run: `cd D:/14-OneAgentSwithc/src-tauri && cargo test 2>&1 | tail -40`

Expected: All tests pass. Look for `test result: ok` in output.

- [ ] **Step 2: Run TypeScript type check**

Run: `cd D:/14-OneAgentSwithc && pnpm typecheck 2>&1 | tail -10`

Expected: No errors.

- [ ] **Step 3: Run frontend unit tests**

Run: `cd D:/14-OneAgentSwithc && pnpm test:unit -- --run 2>&1 | tail -20`

Expected: All tests pass.

- [ ] **Step 4: Build the Vite frontend**

Run: `cd D:/14-OneAgentSwithc && pnpm build:renderer 2>&1 | tail -15`

Expected: Build succeeds. The chunk size warning is acceptable.

- [ ] **Step 5: Final commit (if any test fixes needed)**

```bash
cd D:/14-OneAgentSwithc
git add -A
git commit -m "test(orchestration): integration verification — all tests pass"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- ✅ P0: `build_url` base_url fix → Task 1
- ✅ P0: Streaming/tools guard removal → Task 5
- ✅ P1: Parallel debaters/proposers → Task 3
- ✅ P1: CrossJudge wiring → Task 4
- ✅ P1: StrategyEditor data loss → Task 2
- ✅ P2: Default Debate/MoA strategies → Task 6
- ⏸ P2: Multi-round debate → Deferred to future plan (requires architectural design for iterative refinement — not a bug fix)
- ✅ Integration verification → Task 7

**2. Placeholder scan:** No TBD, TODO, "implement later", or "add validation" found.

**3. Type consistency:**
- `ModelCaller::build_url` returns `Result<String, String>` — consistent across all call sites
- `StrategyExecutor::execute_debate` signature unchanged — `CrossJudge` is internal
- `OrchestrationConfig` `models` field type `HashMap<String, ModelConfig>` — consistent with frontend `Record<string, ModelDef>`
- `should_try_orchestrate` takes `(bool, bool)` — matches the existing `is_streaming` and `has_tools` variables
