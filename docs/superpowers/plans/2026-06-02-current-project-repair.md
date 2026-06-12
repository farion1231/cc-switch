# Current Project Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the current Tauri/React/Rust project back to a verifiable, internally consistent state.

**Architecture:** Repair the smallest failing surfaces first: frontend test harness mocks, Rust orchestration unit behavior, strategy configuration validation, formatting, and identity/version consistency. Keep existing module boundaries intact; do not redesign orchestration beyond making the currently shipped behavior honest and testable.

**Tech Stack:** React 18, TypeScript, Vitest, MSW, TanStack Query, Tauri 2, Rust 1.85, Cargo tests, Prettier.

---

## File Structure

- `tests/msw/handlers.ts` owns mocked Tauri IPC endpoints for frontend integration tests.
- `tests/integration/App.test.tsx` owns full App integration flows and must reset browser storage between tests.
- `src-tauri/src/orchestration/health_checker.rs` owns reactive model availability and latency/error tracking.
- `src-tauri/src/orchestration/json_healer.rs` owns progressive JSON repair.
- `src-tauri/src/orchestration/quality_gate.rs` owns structural and pattern verification scoring.
- `src-tauri/src/orchestration/react_executor.rs` owns ReACT retry prompt construction.
- `src-tauri/src/orchestration/config.rs` owns orchestration config shape and validation.
- `src-tauri/src/orchestration/model_caller.rs` owns model provider endpoint resolution.
- `configs/strategies.yaml` owns the shipped editable orchestration strategies.
- `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `QUICKSTART.md`, and `docs/architecture/overview.md` own project identity and user-facing naming.
- `src/components/orchestration/*.tsx` and `src/components/proxy/ProxyPanel.tsx` require Prettier formatting.
- `ec-switch-main/src/config/universalProviderPresets.ts` is an unused duplicate tree and should be removed if `rg -n 'ec-switch-main'` still returns no references.

## Scope Check

This repair spans frontend tests, Rust tests, config, docs, and formatting. The work stays in one plan because every task is a quality-gate repair for the same release-readiness objective, and each task can be verified independently.

### Task 1: Stabilize Frontend Integration Test Harness

**Files:**
- Modify: `tests/msw/handlers.ts`
- Modify: `tests/integration/App.test.tsx`
- Test: `tests/integration/App.test.tsx`
- Test: `tests/integration/SettingsDialog.test.tsx`

- [ ] **Step 1: Add a failing MSW regression assertion**

Add this test near the MSW handler tests if there is a handler-specific test file. If there is no handler-specific test file, add the check to `tests/integration/SettingsDialog.test.tsx` before the existing `"loads default settings from MSW"` test by asserting that settings render without an unhandled skills request.

```tsx
it("loads default settings with installed skills mocked", async () => {
  renderDialog();

  await waitFor(() =>
    expect(screen.getByText("language:zh")).toBeInTheDocument(),
  );

  expect(screen.queryByTestId("loading")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run the failing frontend integration tests**

Run:

```powershell
pnpm.cmd test:unit -- tests/integration/SettingsDialog.test.tsx tests/integration/App.test.tsx
```

Expected before the fix: `SettingsDialog.test.tsx` fails to find `language:zh`, `App.test.tsx` has timeouts, and MSW logs an unhandled `POST http://tauri.local/get_installed_skills`.

- [ ] **Step 3: Mock `get_installed_skills`**

In `tests/msw/handlers.ts`, add this handler immediately after `get_skills_migration_result`:

```ts
  http.post(`${TAURI_ENDPOINT}/get_installed_skills`, () => success([])),
```

The top of the handler list should contain:

```ts
export const handlers = [
  http.post(`${TAURI_ENDPOINT}/get_migration_result`, () => success(false)),
  http.post(`${TAURI_ENDPOINT}/get_skills_migration_result`, () =>
    success(null),
  ),
  http.post(`${TAURI_ENDPOINT}/get_installed_skills`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_providers`, async ({ request }) => {
```

- [ ] **Step 4: Reset browser storage in App integration tests**

In `tests/integration/App.test.tsx`, update `beforeEach` to clear browser state that affects `getInitialApp()` and `getInitialView()`:

```tsx
  beforeEach(() => {
    resetProviderState();
    localStorage.clear();
    sessionStorage.clear();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });
```

- [ ] **Step 5: Disable query retries in App test QueryClient**

In `tests/integration/App.test.tsx`, replace `const client = new QueryClient();` inside `renderApp` with:

```tsx
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
```

- [ ] **Step 6: Run the targeted frontend tests**

Run:

```powershell
pnpm.cmd test:unit -- tests/integration/SettingsDialog.test.tsx tests/integration/App.test.tsx
```

Expected after the fix: both files pass, with no MSW warning for `get_installed_skills`.

- [ ] **Step 7: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add tests/msw/handlers.ts tests/integration/App.test.tsx tests/integration/SettingsDialog.test.tsx
git -c safe.directory=D:/14-OneAgentSwithc commit -m "test: stabilize frontend integration harness"
```

### Task 2: Fix Model Health Checker Semantics

**Files:**
- Modify: `src-tauri/src/orchestration/health_checker.rs`
- Test: `src-tauri/src/orchestration/health_checker.rs`

- [ ] **Step 1: Add explicit recovery and first-latency tests**

In `src-tauri/src/orchestration/health_checker.rs`, add these tests inside the existing `tests` module:

```rust
    #[test]
    fn first_latency_uses_observed_value_without_ema_dampening() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", true, 500);

        let health = checker.get_health("model-a").unwrap();
        assert_eq!(health.avg_latency_ms, 500);
        assert!(checker.is_available("model-a"));
    }

    #[test]
    fn single_error_records_error_without_disabling_model() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", false, 1000);

        let health = checker.get_health("model-a").unwrap();
        assert_eq!(health.consecutive_errors, 1);
        assert!(health.error_rate > 0.0);
        assert!(checker.is_available("model-a"));
    }
```

- [ ] **Step 2: Run the health checker tests to verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::health_checker
```

Expected before the fix: `success_keeps_available`, `single_error_does_not_disable`, and the new tests fail.

- [ ] **Step 3: Replace the health update and availability logic**

In `src-tauri/src/orchestration/health_checker.rs`, replace `update_health`, `is_available`, and `available_models` with:

```rust
    pub fn update_health(&mut self, model_key: &str, success: bool, latency_ms: u64) {
        let Some(entry) = self.health.get_mut(model_key) else {
            return;
        };

        let alpha = 0.3; // EMA smoothing factor after the first observation.

        let error_val = if success { 0.0 } else { 1.0 };
        entry.error_rate = alpha * error_val + (1.0 - alpha) * entry.error_rate;

        entry.avg_latency_ms = if entry.avg_latency_ms == 0 {
            latency_ms.max(1)
        } else {
            ((alpha * latency_ms as f64 + (1.0 - alpha) * entry.avg_latency_ms as f64) as u64)
                .max(1)
        };

        if success {
            entry.consecutive_errors = 0;
        } else {
            entry.consecutive_errors += 1;
        }

        if entry.consecutive_errors >= 3 {
            entry.is_available = false;
        }

        if success && !entry.is_available && entry.error_rate < 0.1 {
            entry.is_available = true;
        }

        entry.last_check_ms = Self::current_time_ms();
    }

    pub fn is_available(&self, model_key: &str) -> bool {
        self.health
            .get(model_key)
            .map(|h| h.is_available)
            .unwrap_or(false)
    }

    pub fn available_models(&self) -> Vec<String> {
        self.health
            .values()
            .filter(|h| h.is_available)
            .map(|h| h.model_key.clone())
            .collect()
    }
```

- [ ] **Step 4: Run health checker tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::health_checker
```

Expected after the fix: all health checker tests pass.

- [ ] **Step 5: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/health_checker.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix(orchestration): stabilize model health tracking"
```

### Task 3: Fix JSON Healer Bracket and Raw Newline Repair

**Files:**
- Modify: `src-tauri/src/orchestration/json_healer.rs`
- Test: `src-tauri/src/orchestration/json_healer.rs`

- [ ] **Step 1: Add focused JSON healer tests**

In `src-tauri/src/orchestration/json_healer.rs`, add these tests inside the existing `tests` module:

```rust
    #[test]
    fn close_unclosed_string_before_container() {
        assert_eq!(
            close_unclosed_brackets(r#"{"name": "abc"#),
            r#"{"name": "abc"}"#
        );
    }

    #[test]
    fn escape_raw_newlines_inside_strings() {
        let input = "{\"name\": \"line1\nline2\"}";
        assert_eq!(
            escape_raw_control_chars_in_strings(input),
            "{\"name\": \"line1\\nline2\"}"
        );
    }
```

- [ ] **Step 2: Run JSON healer tests to verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::json_healer
```

Expected before the fix: tests around `close_unclosed_brackets_basic`, missing closing JSON, and raw newlines fail.

- [ ] **Step 3: Replace bracket closing with stack-based repair**

In `src-tauri/src/orchestration/json_healer.rs`, replace `close_unclosed_brackets` with:

```rust
fn close_unclosed_brackets(raw: &str) -> String {
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape_next = false;

    for ch in raw.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            continue;
        }

        if in_string {
            continue;
        }

        match ch {
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.last() == Some(&ch) {
                    stack.pop();
                }
            }
            _ => {}
        }
    }

    let mut result = raw.to_string();
    if in_string {
        result.push('"');
    }

    while let Some(closer) = stack.pop() {
        result.push(closer);
    }

    result
}
```

- [ ] **Step 4: Add raw control character escaping inside strings**

Add this helper below `clean_control_chars`:

```rust
fn escape_raw_control_chars_in_strings(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escape_next = false;

    for ch in raw.chars() {
        if escape_next {
            escape_next = false;
            result.push(ch);
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            result.push(ch);
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }

        if in_string {
            match ch {
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                c if (c as u32) < 0x20 => result.push(' '),
                c => result.push(c),
            }
        } else {
            result.push(ch);
        }
    }

    result
}
```

- [ ] **Step 5: Use escaped level 4 input during repair**

In `heal_json`, replace the level 4 block with:

```rust
    // Level 4 -- remove control characters outside strings and escape raw
    // control characters that appear inside JSON strings.
    let level4 = clean_control_chars(raw);
    let level4_escaped = escape_raw_control_chars_in_strings(&level4);
    if let Ok(val) = serde_json::from_str::<Value>(&level4_escaped) {
        return Ok(val);
    }
    let level4_fixed = close_unclosed_brackets(&level4_escaped);
    if let Ok(val) = serde_json::from_str::<Value>(&level4_fixed) {
        return Ok(val);
    }
```

- [ ] **Step 6: Run JSON healer tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::json_healer
```

Expected after the fix: all JSON healer tests pass.

- [ ] **Step 7: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/json_healer.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix(orchestration): repair truncated json handling"
```

### Task 4: Make Structural Quality Gate Penalize Bracket Imbalance Strongly

**Files:**
- Modify: `src-tauri/src/orchestration/quality_gate.rs`
- Test: `src-tauri/src/orchestration/quality_gate.rs`

- [ ] **Step 1: Add a direct regression test for unclosed Rust braces**

In `src-tauri/src/orchestration/quality_gate.rs`, add this test inside the existing `tests` module next to `structural_check_fails_unclosed_brace`:

```rust
    #[test]
    fn structural_check_caps_score_when_any_bracket_type_is_unbalanced() {
        let code = "```rust\nfn open() {\n    let x = 1;\n```";
        let score = run_structural_check(code);

        assert!(
            score <= 0.66,
            "Bracket imbalance must cap structural score, got {}",
            score
        );
    }
```

- [ ] **Step 2: Run quality gate structural tests to verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::quality_gate::tests::structural_check_fails_unclosed_brace orchestration::quality_gate::tests::structural_check_caps_score_when_any_bracket_type_is_unbalanced
```

Expected before the fix: `structural_check_fails_unclosed_brace` fails because the averaged score is too high.

- [ ] **Step 3: Cap the final structural score by bracket score**

In `src-tauri/src/orchestration/quality_gate.rs`, replace the final line of `run_structural_check`:

```rust
    (total / count).min(1.0).max(0.0)
```

with:

```rust
    let mut final_score = (total / count).min(1.0).max(0.0);
    if bracket_score < 1.0 {
        final_score = final_score.min(bracket_score);
    }
    final_score
```

- [ ] **Step 4: Run all quality gate tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::quality_gate
```

Expected after the fix: all quality gate tests pass.

- [ ] **Step 5: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/quality_gate.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix(orchestration): cap structural score on bracket imbalance"
```

### Task 5: Keep ReACT Verification Feedback as the Last Retry Message

**Files:**
- Modify: `src-tauri/src/orchestration/react_executor.rs`
- Test: `src-tauri/src/orchestration/react_executor.rs`

- [ ] **Step 1: Add a message-order regression test**

In `src-tauri/src/orchestration/react_executor.rs`, add this test inside the existing `tests` module:

```rust
    #[test]
    fn build_messages_puts_feedback_after_unused_tool_hint() {
        let executor = ReACTExecutor::new(vec![
            "structural_check".to_string(),
            "pattern_match".to_string(),
        ]);
        let msgs = test_messages();
        let feedback = Some("Please improve".to_string());
        let used: HashSet<String> = vec!["structural_check".to_string()]
            .into_iter()
            .collect();

        let built = executor.build_messages(&msgs, &feedback, &used, 1);
        let last_content = built
            .last()
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap();

        assert!(last_content.contains("Verification Feedback"));
        assert!(last_content.contains("Please improve"));
    }
```

- [ ] **Step 2: Run ReACT message tests to verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::react_executor::tests::build_messages_iteration_one_adds_feedback orchestration::react_executor::tests::build_messages_puts_feedback_after_unused_tool_hint
```

Expected before the fix: the feedback test fails because the unused-tools hint is appended after feedback.

- [ ] **Step 3: Reorder `build_messages`**

In `src-tauri/src/orchestration/react_executor.rs`, replace the body of `build_messages` with:

```rust
        let mut msgs = original.to_vec();

        if iteration == 0 {
            return msgs;
        }

        let unused = self.unused_tools(tools_used);
        if !unused.is_empty() {
            msgs.push(json!({
                "role": "assistant",
                "content": format!(
                    "Note: the following verification tools have not been used yet: [{}]. \
                     Consider applying them for a more thorough verification.",
                    unused.join(", "),
                ),
            }));
        }

        if let Some(ref fb) = feedback {
            msgs.push(json!({
                "role": "user",
                "content": format!(
                    "[Verification Feedback]\n{}\n\nPlease improve your previous response \
                     based on this feedback. Focus on the issues identified above.",
                    fb
                ),
            }));
        }

        msgs
```

- [ ] **Step 4: Run all ReACT executor tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::react_executor
```

Expected after the fix: all ReACT executor tests pass.

- [ ] **Step 5: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/react_executor.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix(orchestration): preserve react feedback order"
```

### Task 6: Validate Strategy Model References and Remove Unsupported MiniMax Default

**Files:**
- Modify: `src-tauri/src/orchestration/config.rs`
- Modify: `src-tauri/src/orchestration/model_caller.rs`
- Modify: `configs/strategies.yaml`
- Test: `src-tauri/src/orchestration/config.rs`
- Test: `src-tauri/src/orchestration/model_caller.rs`

- [ ] **Step 1: Add config validation tests**

In `src-tauri/src/orchestration/config.rs`, add these tests inside the existing `tests` module:

```rust
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
```

- [ ] **Step 2: Add model URL fallback test**

In `src-tauri/src/orchestration/model_caller.rs`, add this test inside the existing `tests` module:

```rust
    #[test]
    fn unknown_provider_uses_explicit_base_url() {
        let config = ModelConfig {
            provider: "minimax".to_string(),
            model: "MiniMax-Text-01".to_string(),
            api_key_env: "MINIMAX_API_KEY".to_string(),
            base_url: Some("https://example.com/v1/chat/completions".to_string()),
            max_tokens: 1024,
        };

        assert_eq!(
            ModelCaller::build_url(&config),
            "https://example.com/v1/chat/completions"
        );
    }
```

- [ ] **Step 3: Run the new tests to verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::config::tests::validate_rejects_route_model_that_is_not_defined orchestration::config::tests::validate_rejects_cascade_model_that_is_not_defined orchestration::model_caller::tests::unknown_provider_uses_explicit_base_url
```

Expected before the fix: config validation accepts missing model references, and `minimax` ignores explicit `base_url` because it has a hard-coded URL.

- [ ] **Step 4: Validate strategy model references**

In `src-tauri/src/orchestration/config.rs`, replace `OrchestrationConfig::validate` with:

```rust
impl OrchestrationConfig {
    /// Validate model configs and strategy references.
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
            }
        }

        Ok(())
    }
}
```

- [ ] **Step 5: Remove MiniMax hard-coded URL**

In `src-tauri/src/orchestration/model_caller.rs`, remove this match arm:

```rust
            "minimax" => "https://api.minimax.chat/v1/text/chatcompletion_v2".to_string(),
```

This makes `provider: minimax` require an explicit `base_url`, which prevents the shipped OpenAI-compatible caller from silently using a non-compatible endpoint.

- [ ] **Step 6: Remove MiniMax from shipped default strategies**

In `configs/strategies.yaml`, make these exact edits:

```yaml
# Supported providers: anthropic, openai, deepseek, qwen, glm, kimi,
#                      doubao, yi, baichuan, spark
```

Remove this model block:

```yaml
  minimax:
    provider: minimax
    model: MiniMax-Text-01
    api_key_env: MINIMAX_API_KEY
    max_tokens: 16384
```

Change the MoA description and model list to:

```yaml
  moa:
    description: "DeepSeek + Kimi 提议，GLM 聚合"
    when:
      complexity: [0.8, 1.0]
      risk: ["critical"]
      task_type: ["coding", "architecture"]
    action:
      type: cascade
      models:
        - deepseek
        - kimi
        - glm
      verify_each: true
      escalate_on_fail: true
      quality_threshold: 0.80
```

- [ ] **Step 7: Run config and model caller tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration::config orchestration::model_caller
```

Expected after the fix: config and model caller tests pass.

- [ ] **Step 8: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/config.rs src-tauri/src/orchestration/model_caller.rs configs/strategies.yaml
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix(orchestration): validate strategy model references"
```

### Task 7: Align Project Identity and Version Metadata

**Files:**
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `QUICKSTART.md`
- Modify: `docs/architecture/overview.md`
- Check: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Confirm the public product name**

Run:

```powershell
rg -n '"productName"|"version"|# EC Switch|# OmniAgent Workbench' package.json src-tauri\Cargo.toml src-tauri\tauri.conf.json README_ZH.md QUICKSTART.md docs\architecture\overview.md
```

Expected before the fix: `EC Switch 3.15.0` in Tauri/README and `omniagent-workbench 0.1.0` in package/Cargo docs.

- [ ] **Step 2: Update `package.json` metadata**

Change the first fields in `package.json` to:

```json
{
  "name": "ec-switch",
  "version": "3.15.0",
  "description": "Desktop manager for Claude Code, Codex, Gemini CLI, OpenCode, OpenClaw, Hermes, providers, proxy, MCP, prompts, skills, and sessions",
```

- [ ] **Step 3: Update `src-tauri/Cargo.toml` metadata**

Change the first fields in `src-tauri/Cargo.toml` to:

```toml
[package]
name = "ec-switch"
version = "3.15.0"
description = "Desktop manager for Claude Code, Codex, Gemini CLI, OpenCode, OpenClaw, Hermes, providers, proxy, MCP, prompts, skills, and sessions"
authors = ["OmniAgent Contributors"]
license = "MIT"
repository = ""
edition = "2021"
rust-version = "1.85.0"
```

Leave the lib section unchanged:

```toml
[lib]
name = "ec_switch_lib"
crate-type = ["staticlib", "cdylib", "rlib"]
doctest = false
```

- [ ] **Step 4: Update Quickstart public title and executable name**

In `QUICKSTART.md`, replace:

```markdown
# OmniAgent Workbench — 快速上手指南
```

with:

```markdown
# EC Switch — 快速上手指南
```

Replace:

```markdown
./src-tauri/target/release/omniagent-workbench.exe
```

with:

```markdown
./src-tauri/target/release/ec-switch.exe
```

- [ ] **Step 5: Update architecture title and diagram label**

In `docs/architecture/overview.md`, replace:

```markdown
# OmniAgent Workbench — Architecture Overview
```

with:

```markdown
# EC Switch — Architecture Overview
```

Replace the diagram label:

```text
│  │         OmniAgent Workbench                 │                 │
```

with:

```text
│  │              EC Switch                       │                 │
```

- [ ] **Step 6: Refresh lock files after package rename**

Run:

```powershell
pnpm.cmd install --lockfile-only
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected after the fix: `pnpm-lock.yaml` and `src-tauri/Cargo.lock` reflect the package rename, and Cargo check passes.

- [ ] **Step 7: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add package.json pnpm-lock.yaml src-tauri/Cargo.toml src-tauri/Cargo.lock QUICKSTART.md docs/architecture/overview.md
git -c safe.directory=D:/14-OneAgentSwithc commit -m "chore: align project identity metadata"
```

### Task 8: Remove Unused Duplicate Source Tree

**Files:**
- Delete: `ec-switch-main/src/config/universalProviderPresets.ts`
- Delete directory if empty: `ec-switch-main/src/config`
- Delete directory if empty: `ec-switch-main/src`
- Delete directory if empty: `ec-switch-main`

- [ ] **Step 1: Prove the duplicate tree is unreferenced**

Run:

```powershell
rg -n 'ec-switch-main' .gitignore package.json pnpm-workspace.yaml tsconfig.json vite.config.ts src tests docs src-tauri
```

Expected before deletion: no matches.

- [ ] **Step 2: Delete the duplicate file**

Use PowerShell with path checks:

```powershell
$root = (Resolve-Path .).Path
$file = Join-Path $root 'ec-switch-main\src\config\universalProviderPresets.ts'
if ((Test-Path -LiteralPath $file) -and $file.StartsWith($root)) {
  Remove-Item -LiteralPath $file -Force
}
```

- [ ] **Step 3: Remove empty duplicate directories**

Use PowerShell with path checks:

```powershell
$root = (Resolve-Path .).Path
foreach ($relative in @('ec-switch-main\src\config', 'ec-switch-main\src', 'ec-switch-main')) {
  $path = Join-Path $root $relative
  if ((Test-Path -LiteralPath $path) -and $path.StartsWith($root)) {
    $children = Get-ChildItem -LiteralPath $path -Force
    if ($children.Count -eq 0) {
      Remove-Item -LiteralPath $path -Force
    }
  }
}
```

- [ ] **Step 4: Verify no duplicate tree remains**

Run:

```powershell
Test-Path .\ec-switch-main
```

Expected after deletion: `False`.

- [ ] **Step 5: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add -A ec-switch-main
git -c safe.directory=D:/14-OneAgentSwithc commit -m "chore: remove unused duplicate source tree"
```

### Task 9: Format Frontend Files

**Files:**
- Modify: `src/components/orchestration/AuditLog.tsx`
- Modify: `src/components/orchestration/FlowCanvas.tsx`
- Modify: `src/components/orchestration/ModelLeaderboard.tsx`
- Modify: `src/components/orchestration/StrategyEditor.tsx`
- Modify: `src/components/proxy/ProxyPanel.tsx`

- [ ] **Step 1: Reproduce formatting failure**

Run:

```powershell
pnpm.cmd format:check
```

Expected before the fix: Prettier reports the five files listed above.

- [ ] **Step 2: Format the reported files**

Run:

```powershell
pnpm.cmd prettier --write src/components/orchestration/AuditLog.tsx src/components/orchestration/FlowCanvas.tsx src/components/orchestration/ModelLeaderboard.tsx src/components/orchestration/StrategyEditor.tsx src/components/proxy/ProxyPanel.tsx
```

- [ ] **Step 3: Verify formatting**

Run:

```powershell
pnpm.cmd format:check
```

Expected after the fix: `All matched files use Prettier code style!`

- [ ] **Step 4: Commit**

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add src/components/orchestration/AuditLog.tsx src/components/orchestration/FlowCanvas.tsx src/components/orchestration/ModelLeaderboard.tsx src/components/orchestration/StrategyEditor.tsx src/components/proxy/ProxyPanel.tsx
git -c safe.directory=D:/14-OneAgentSwithc commit -m "style: format frontend panels"
```

### Task 10: Full Verification Pass

**Files:**
- Check only unless a failure points to a file already covered by Tasks 1-9.

- [ ] **Step 1: Run frontend typecheck**

Run:

```powershell
pnpm.cmd typecheck
```

Expected: command exits `0`.

- [ ] **Step 2: Run frontend unit and integration tests**

Run:

```powershell
pnpm.cmd test:unit
```

Expected: all test files pass. If a test times out, run that file directly with `pnpm.cmd test:unit -- path\to\file.test.tsx` and fix the component or mock that the failure names.

- [ ] **Step 3: Run renderer production build**

Run:

```powershell
pnpm.cmd build:renderer
```

Expected: Vite build exits `0`. Chunk-size warnings are acceptable for this repair if no new build error appears.

- [ ] **Step 4: Run Rust orchestration tests**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib orchestration
```

Expected: all orchestration tests pass.

- [ ] **Step 5: Run Rust full test suite with an extended timeout**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: full suite completes successfully. If Windows linking makes this exceed five minutes, capture the last active test name and rerun that test module directly.

- [ ] **Step 6: Run Rust compile check**

Run:

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: command exits `0`. Warnings from unused orchestration modules can remain only if the tests pass and the warnings do not hide a failing behavior.

- [ ] **Step 7: Run format check**

Run:

```powershell
pnpm.cmd format:check
```

Expected: command exits `0`.

- [ ] **Step 8: Inspect final git status**

Run:

```powershell
git -c safe.directory=D:/14-OneAgentSwithc status --short
```

Expected: only intentional changes from this plan are present.

- [ ] **Step 9: Commit final verification note if changes were needed after earlier commits**

If Step 8 shows additional fixes made during verification, commit them:

```powershell
git -c safe.directory=D:/14-OneAgentSwithc add -A
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix: complete project repair verification"
```

## Self-Review

**Spec coverage:** Task 1 covers frontend test failures and MSW gaps. Tasks 2-5 cover Rust orchestration failures from health checker, JSON healer, quality gate, and ReACT executor. Task 6 covers strategy/model compatibility and config validation. Task 7 covers naming/version inconsistency. Task 8 covers the unused duplicate tree. Task 9 covers formatting failures. Task 10 covers final verification.

**Placeholder scan:** The plan contains concrete file paths, snippets, commands, and expected outputs for every task.

**Type consistency:** Rust snippets use existing `ModelConfig`, `OrchestrationConfig`, `StrategyAction`, `ModelCaller`, and test module names. TypeScript snippets use existing `QueryClient`, `localStorage`, `sessionStorage`, `screen`, and `waitFor` imports already present in the test files.
