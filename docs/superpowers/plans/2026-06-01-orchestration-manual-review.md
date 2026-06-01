# Manual Review: Orchestration MVP And Small-Model Scaling Plan

Date: 2026-06-01
Mode: manual fallback, because `$autoplan` and `/plan-eng-review` require `AskUserQuestion`, which is unavailable in this environment.

## Executive Verdict

The strategic direction is feasible for **bounded, verifiable engineering workflows**: use cheap model candidates, deterministic checks, rank/repair/fallback, and measure against a strong-model baseline.

It is **not feasible as a universal replacement for frontier models across all open-ended tasks**. The design document now states this explicitly. The practical target should be:

- Match or beat a strong single-model baseline on selected verifiable tasks.
- Lower `cost_per_success`.
- Detect failures before returning them to the user.
- Fall back to a stronger model or human gate when verification is weak.

The current MVP implementation plan is **not ready for blind execution by weaker models**. It has drifted from the current worktree and must be rebased first.

## P1 Blockers

1. **Plan/code drift: files already exist**

The plan still says to create files that already exist:

- `src-tauri/src/orchestration/quality_gate.rs`
- `src-tauri/src/commands/orchestration.rs`
- many `src-tauri/src/orchestration/*` modules

Risk: a weaker model may overwrite richer existing code with older snippets from the plan.

Required fix: change the plan language from "Create" to "Modify existing file" where applicable, and tell implementers to preserve current APIs unless a task explicitly changes them.

2. **Command surface is inconsistent**

Current code uses:

- `orchestration_status`
- `orchestration_reload`
- `orchestration_toggle`

The plan later introduces:

- `get_orchestration_status`
- `set_orchestration_enabled`

Risk: frontend, MSW, and Rust command registration can split into two incompatible APIs.

Required fix: pick one command surface. Recommended for least churn: keep the existing command names, but change `orchestration_status` to return `{ enabled, configPath }` and change `orchestration_toggle` to persist YAML.

3. **Toggle command does not toggle**

`src-tauri/src/commands/orchestration.rs` reloads config and returns the current enabled state; it does not write the new value to the runtime YAML.

Required fix:

- Load runtime strategies YAML.
- Normalize defaults.
- Set `enabled`.
- Validate.
- Write YAML.
- Reload the shared engine.
- Return `{ enabled, configPath }`.

4. **Runtime config path helper is referenced but absent**

`src-tauri/src/lib.rs` and `src-tauri/src/services/proxy.rs` call `StrategyLoader::default_strategies_path()`, but the current loader file does not define that function.

Required fix: add one canonical helper, preferably `runtime_strategies_path()`, backed by `crate::config::get_app_config_dir()`. Either expose it as a free function or update all callers consistently.

5. **Model reference validation is missing**

The default config references `mid_coder`, but the default `models` map only defines `cheap_coder` and `frontier`. `configs/strategies.yaml` also references `mid_executor_code`, `cheap_executor_code`, and `frontier_planner`, while those live in a separate `configs/models.yaml` shape that `OrchestrationConfig` does not read.

Risk: CASCADE may fail only after a live request reaches a missing model key.

Required fix:

- Decide whether `configs/strategies.yaml` owns `models:` or imports from `configs/models.yaml`.
- Implement `normalize_with_defaults()`.
- Implement `validate()`.
- Validate every `Route.use_model` and every `Cascade.models[]` at load time.

6. **Executor bypasses the existing QualityGate**

`StrategyExecutor` still uses `quick_quality_check()` while `quality_gate.rs` already exists.

Risk: the MVP quality gate in the plan and the actual executor disagree, so tests can pass while production uses a weaker heuristic.

Required fix:

- Add a mockable backend trait without removing `execute_debate`.
- Replace `quick_quality_check()` with `QualityGate`.
- Keep the last successful response when all candidates fail quality, but mark `verified=false`.
- Return an error only when all model calls fail.

7. **Empty verifier behavior is unsafe**

The design doc says empty verification tools must be rejected. Current `QualityGate` tests say empty tools "vacuously pass".

Risk: a production config with no verification can pass every answer.

Required fix: for production config, reject empty verifier lists during validation. If `QualityGate::new(vec![], ...)` remains useful for internal tests, do not let runtime YAML create that state.

8. **ModelCaller is not provider-correct yet**

Current issues:

- `max_tokens` is hard-coded to `16384`.
- known providers ignore configured `base_url`.
- all providers get `x-api-key` and `anthropic-version`.
- OpenAI-compatible usage fields need `prompt_tokens` and `completion_tokens` fallback.

Required fix: implement and test `auth_headers`, `build_url`, and `extract_usage` as explicit helpers.

9. **OpenAI Responses `input` is not normalized**

`handle_responses` currently uses the OpenAI orchestration helper, but that helper reads only `messages`.

Risk: a Responses API request can execute CASCADE with an empty prompt.

Required fix: add `extract_messages_for_orchestration()` that supports:

- Chat `messages`
- Responses string `input`
- Responses array `input`

## P2 Issues

1. **Top-level design claim was too strong**

The design document previously said quality would exceed any single frontier model. It now says the target is limited to verifiable tasks with fallback for unverifiable/high-risk tasks.

2. **Research evidence needs verification**

Several local research filenames look like placeholder arXiv IDs. Do not cite them as proof until each paper is verified against its actual source and result.

3. **MVP verification cannot prove the long-term goal**

The current MVP does not run real project tests from generated code, does not implement ranker, does not maintain benchmark telemetry, and does not have fallback economics yet. It can prove routing/CASCADE mechanics, not the "small models exceed strong model" thesis.

4. **Manual QA is necessary**

Automated tests should cover config, loader, executor, model caller, handlers, and the ProxyPanel switch. Manual QA still needs:

- streaming passthrough remains unchanged;
- non-streaming CASCADE returns the correct API shape;
- toggle persists across app restart;
- invalid YAML recovers safely;
- missing API keys fall back without breaking proxy passthrough.

## Corrected Implementation Order

1. Install/verify MSVC Build Tools so `cargo test orchestration --lib` can compile on Windows.
2. Rebase the plan against current files and remove "create existing file" instructions.
3. Fix runtime config path and loader normalization/validation.
4. Fix command surface and persistence.
5. Replace executor quality heuristic with the existing `QualityGate`.
6. Fix `ModelCaller` provider handling.
7. Add response adapters or unify the existing inline response builders.
8. Normalize OpenAI Responses `input`.
9. Wire ProxyPanel to the real command surface.
10. Add MSW and component tests.
11. Run final verification.

## Verification Notes

Commands attempted:

```powershell
cargo test orchestration --lib
```

Result: blocked before Rust code compilation because MSVC `link.exe` is missing.

```powershell
pnpm.cmd typecheck
```

Result: passed.

Plain `pnpm typecheck` failed in PowerShell because `pnpm.ps1` is blocked by execution policy. Use `pnpm.cmd` on this machine.

## Handoff Rule For Weaker Models

Do not start from Task 1 as written. First update the plan so it matches the current worktree, then execute one task at a time. If a task says "create file" and the file already exists, stop and convert that task into a minimal edit of the existing file.
