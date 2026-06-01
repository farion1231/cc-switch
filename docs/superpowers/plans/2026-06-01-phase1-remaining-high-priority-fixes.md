# Phase 1 Remaining HIGH-Priority Fixes + Integration Tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 3 remaining HIGH-priority bugs from the /autoplan review and verify Phase 1 integration end-to-end.

**Architecture:** Tauri 2.x desktop app (Rust backend + React 18 frontend). Orchestration engine sits inside the Axum proxy — requests may be intercepted, routed through strategy execution (ROUTE/CASCADE/DEBATE), or passed through unchanged. Fixes target model_caller, proxy_service fallback logic, and debate prompt anonymity.

**Tech Stack:** Rust 1.82+ / tokio 1.x / axum 0.7 / reqwest / serde_json / React 18 / TypeScript / shadcn/ui

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src-tauri/src/orchestration/model_caller.rs` | Modify | E-Code-5: pass `system` into messages array |
| `src-tauri/src/services/proxy.rs` | Modify | E-Code-3: remove silent fallback engine creation |
| `src-tauri/src/orchestration/executor.rs` | Modify | E-Design-2: anonymize debate prompt |
| `src-tauri/src/orchestration/engine.rs` | Read | Understand `OrchestrationOutcome` variants |
| `src/components/orchestration/StrategyEditor.tsx` | Read | E-Design-1: understand mock data shape for future YAML integration |

---

## Task 1: Fix call_prompt to pass system parameter (E-Code-5)

**Context:** `call_prompt()` accepts a `system` parameter but silently discards it. When the DEBATE judge calls `call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &debate_summary, ...)`, the system prompt never reaches the model. This means judge instructions like "be impartial" are never sent.

**Files:**
- Modify: `src-tauri/src/orchestration/model_caller.rs:129-146`
- Test: `src-tauri/src/orchestration/model_caller.rs` (add tests at bottom)

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/orchestration/model_caller.rs`:

```rust
    #[test]
    fn call_prompt_builds_messages_with_system() {
        // Verify that call_prompt constructs a messages array with both
        // system and user roles when system is non-empty.
        // We can't call call_prompt directly without HTTP, so we test the
        // message construction logic by extracting it.
        let system = "You are a judge.";
        let user = "Evaluate this.";
        let messages = build_messages_for_prompt(system, user);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], system);
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], user);
    }

    #[test]
    fn call_prompt_builds_messages_without_system() {
        let messages = build_messages_for_prompt("", "Hello");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test call_prompt_builds_messages -- --nocapture`
Expected: FAIL — `build_messages_for_prompt` does not exist yet

- [ ] **Step 3: Extract helper function and fix call_prompt**

In `src-tauri/src/orchestration/model_caller.rs`, add a visible helper function right after the `impl ModelCaller` block (around line 146):

```rust
/// Build the messages array for call_prompt. Extracted for testability.
pub fn build_messages_for_prompt(system: &str, user_prompt: &str) -> Vec<Value> {
    let mut messages = Vec::with_capacity(2);
    if !system.is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    messages.push(json!({"role": "user", "content": user_prompt}));
    messages
}
```

Then replace the `call_prompt` method body (lines 129-146) with:

```rust
    pub async fn call_prompt(
        &self,
        model_key: &str,
        system: &str,
        user_prompt: &str,
        temperature: Option<f64>,
    ) -> Result<ModelResponse, String> {
        let messages = build_messages_for_prompt(system, user_prompt);
        self.call(model_key, messages, None, temperature).await
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test call_prompt_builds_messages -- --nocapture`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/orchestration/model_caller.rs
git commit -m "fix(orchestration): pass system prompt to model in call_prompt (E-Code-5)

The system parameter was accepted but silently discarded. Now builds
a proper messages array with system role when non-empty."
```

---

## Task 2: Remove silent fallback engine creation in get_orchestration_engine (E-Code-3)

**Context:** When `get_orchestration_engine()` finds no shared engine set, it silently creates an independent fallback engine by loading strategy files from disk. This bypasses the UI toggle and creates a hidden, uncontrolled engine instance. The correct behavior is to surface the problem loudly and let the caller handle it.

**Files:**
- Modify: `src-tauri/src/services/proxy.rs:84-96`
- Read: `src-tauri/src/services/proxy.rs:445` and `src-tauri/src/services/proxy.rs:2211`

- [ ] **Step 1: Write the failing test**

This refactor changes the return type, so the existing code that calls `get_orchestration_engine().await` (lines 445, 2211) must handle the `None` case. No separate test file needed — the change is verified by the compiler.

- [ ] **Step 2: Change get_orchestration_engine to return Option**

In `src-tauri/src/services/proxy.rs`, replace the `get_orchestration_engine` method (lines 84-96) with:

```rust
    async fn get_orchestration_engine(&self) -> Option<Arc<crate::orchestration::OrchestrationEngine>> {
        self.orchestration.read().await.clone()
    }
```

- [ ] **Step 3: Update caller at line 445 (start method)**

Find this block in the `start` method:

```rust
        let orchestration = self.get_orchestration_engine().await;
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle, orchestration);
```

Replace with:

```rust
        let orchestration = match self.get_orchestration_engine().await {
            Some(engine) => engine,
            None => {
                log::error!("[Orchestration] No engine set — starting proxy without orchestration. Call set_orchestration() first.");
                return Err("Orchestration engine not initialized. Restart the application.".to_string());
            }
        };
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle, orchestration);
```

- [ ] **Step 4: Update caller at line 2211 (update_config method)**

Find this block in the `update_config` method:

```rust
            let orchestration = self.get_orchestration_engine().await;
            let new_server = ProxyServer::new(new_config, self.db.clone(), app_handle, orchestration);
```

Replace with:

```rust
            let orchestration = match self.get_orchestration_engine().await {
                Some(engine) => engine,
                None => {
                    log::error!("[Orchestration] No engine set during config update — proxy restart skipped");
                    // Re-use existing server without restart
                    *self.server.write().await = Some(existing_server);
                    return Ok(());
                }
            };
            let new_server = ProxyServer::new(new_config, self.db.clone(), app_handle, orchestration);
```

Note: the `existing_server` variable is the one that was taken from `self.server.write().await` earlier in the `update_config` method (around line 2200). Verify this variable name by reading the surrounding code.

- [ ] **Step 5: Run cargo check**

Run: `cd src-tauri && cargo check 2>&1 | grep -E "^error" | head -5`
Expected: No errors related to proxy.rs or get_orchestration_engine

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/proxy.rs
git commit -m "fix(orchestration): remove silent fallback engine creation (E-Code-3)

get_orchestration_engine() now returns Option and fails loudly instead of
silently creating an independent engine that bypasses UI toggle control."
```

---

## Task 3: Anonymize debate prompt to prevent model identity leakage (E-Design-2)

**Context:** `build_debate_prompt()` in executor.rs includes the model key name in the prompt text: `--- Answer 1 (model: model_a) ---`. This leaks model identity to the judge, introducing position/name bias. The judge should evaluate purely on content quality.

**Files:**
- Modify: `src-tauri/src/orchestration/executor.rs:251-266`
- Test: `src-tauri/src/orchestration/executor.rs` existing test `build_debate_prompt_format`

- [ ] **Step 1: Write the failing test**

Add a new test in `mod tests` in `src-tauri/src/orchestration/executor.rs`:

```rust
    #[test]
    fn build_debate_prompt_hides_model_identity() {
        let responses = vec![
            (
                "secret_model_alpha".to_string(),
                ModelResponse {
                    content: "Answer A".to_string(),
                    model: "alpha-v2".to_string(),
                    usage: Default::default(),
                    latency_ms: 100,
                },
            ),
            (
                "secret_model_beta".to_string(),
                ModelResponse {
                    content: "Answer B".to_string(),
                    model: "beta-v1".to_string(),
                    usage: Default::default(),
                    latency_ms: 200,
                },
            ),
        ];
        let prompt = StrategyExecutor::build_debate_prompt(&responses);
        assert!(!prompt.contains("secret_model_alpha"), "must not leak model key");
        assert!(!prompt.contains("secret_model_beta"), "must not leak model key");
        assert!(!prompt.contains("alpha-v2"), "must not leak model name");
        assert!(!prompt.contains("beta-v1"), "must not leak model name");
        assert!(prompt.contains("Answer A"), "must include content");
        assert!(prompt.contains("Answer B"), "must include content");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test build_debate_prompt_hides_model_identity -- --nocapture`
Expected: FAIL — prompt contains "secret_model_alpha"

- [ ] **Step 3: Fix build_debate_prompt to anonymize**

In `src-tauri/src/orchestration/executor.rs`, replace the `build_debate_prompt` method (lines 251-266):

```rust
    fn build_debate_prompt(responses: &[(String, ModelResponse)]) -> String {
        let mut prompt = String::from("Multiple AI models have provided answers to the same question. Evaluate and synthesize the best response.\n\n");
        for (i, (_key, resp)) in responses.iter().enumerate() {
            prompt.push_str(&format!("--- Answer {} ---\n{}\n\n", i + 1, resp.content));
        }
        prompt.push_str(
            "Based on the above answers, provide:\n\
             1. A quality score from 0.0 to 1.0\n\
             2. Your synthesized answer combining the best parts\n\
             \n\
             Format:\n\
             SCORE: <number>\n\
             ANSWER:\n<your response>",
        );
        prompt
    }
```

- [ ] **Step 4: Update the existing test that checks for model names**

The existing test `build_debate_prompt_format` asserts `prompt.contains("model_a")` and `prompt.contains("model_b")`. Since we removed model identity, update this test:

Find:
```rust
        assert!(prompt.contains("model_a"));
        assert!(prompt.contains("model_b"));
```

Replace with:
```rust
        assert!(prompt.contains("Answer 1"));
        assert!(prompt.contains("Answer 2"));
```

- [ ] **Step 5: Run all debate tests**

Run: `cd src-tauri && cargo test build_debate_prompt -- --nocapture`
Expected: All 3 tests PASS (format, hides_model_identity, and the updated format test)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/orchestration/executor.rs
git commit -m "fix(orchestration): anonymize debate prompt to prevent judge bias (E-Design-2)

Removes model key names from judge prompt text so the judge evaluates
purely on content quality without position/name bias."
```

---

## Task 4: Verify Phase 1 end-to-end integration

**Context:** All P0 and HIGH fixes are applied. This task verifies the complete Phase 1 chain works: proxy starts, orchestration engine initializes, ROUTE strategy routes correctly, streaming passthrough works, toggle works.

**Files:**
- Read: `src-tauri/src/orchestration/config.rs` (default strategies)
- Read: `src-tauri/src/orchestration/loader.rs` (strategies.yaml path)
- Read: `src/components/orchestration/StrategyEditor.tsx` (UI mock data)

- [ ] **Step 1: Verify cargo check passes**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: `Finished dev [unoptimized + debuginfo] target(s) in ...` or only pre-existing linker warnings

If linker errors persist (VS Build Tools issue), note this as a blocker and skip to Step 5.

- [ ] **Step 2: Verify existing orchestration unit tests pass**

Run: `cd src-tauri && cargo test --lib orchestration -- --nocapture 2>&1 | tail -20`
Expected: All orchestration module tests pass (config roundtrip, selector, classifier, quality_gate, etc.)

- [ ] **Step 3: Verify strategies.yaml default config is valid**

Read `src-tauri/src/orchestration/loader.rs` to find `default_strategies_path()`. Then verify the file exists:

Run: `cat "$(grep -o 'default_strategies_path.*\"[^\"]*\"' src-tauri/src/orchestration/loader.rs | head -1 | sed 's/.*\"\(.*\)\"/\1/')" 2>/dev/null || echo "Default strategies file not found"`

If not found, check if `StrategyLoader::load_from_file` falls back to `OrchestrationConfig::default()`.

- [ ] **Step 4: Check frontend starts without errors**

Run: `cd D:/14-OneAgentSwithc && pnpm dev 2>&1 &`
Then open browser to the dev URL and verify:
- Orchestration tab/panel loads without crash
- StrategyEditor renders with mock data
- No console errors about missing components

Kill the dev server after verification.

- [ ] **Step 5: Document any remaining blockers**

Create a brief summary of what passes and what blocks. Write it as a comment in the design doc at `docs/design/omniagent-workbench-v2-enterprise-design.md` under the "已完成" section:

```
- [ ] Phase 1 集成测试: cargo check passes / unit tests pass / frontend starts
      Blocker: [list any issues]
```

- [ ] **Step 6: Commit**

```bash
git add docs/design/omniagent-workbench-v2-enterprise-design.md
git commit -m "docs: update Phase 1 integration test status"
```

---

## Self-Review Checklist

### 1. Spec Coverage

| Requirement | Task |
|-------------|------|
| E-Code-5: call_prompt passes system param | Task 1 |
| E-Code-3: remove silent fallback engine | Task 2 |
| E-Design-2: anonymize debate prompt | Task 3 |
| Phase 1 integration verification | Task 4 |

All 3 HIGH items from the review are covered. Phase 1 integration test covers the remaining verification.

### 2. Placeholder Scan

No TBDs, TODOs, or "implement later" patterns found. Every step has actual code.

### 3. Type Consistency

- `build_messages_for_prompt` returns `Vec<Value>` — matches what `call()` expects for its `messages` parameter
- `get_orchestration_engine()` returns `Option<Arc<OrchestrationEngine>>` — callers match on `Some/None`
- `build_debate_prompt` signature unchanged — still `fn build_debate_prompt(responses: &[(String, ModelResponse)]) -> String`
- Tests use `ModelResponse` with `usage: Default::default()` — consistent with `TokenUsage` struct
