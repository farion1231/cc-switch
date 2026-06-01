# Orchestration MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the v2 MVP orchestration path: configurable ROUTE decisions, non-streaming CASCADE execution, deterministic quality checks, a working proxy-panel switch, and focused tests.

**Architecture:** Keep the existing CC-Switch proxy path as the default. ROUTE preserves the existing streaming passthrough path and records the selected strategy, while CASCADE runs a non-streaming orchestration executor before provider selection and returns a client-shaped response only when orchestration succeeds. Configuration stays YAML-backed, with the UI switch reading and writing the same runtime strategies file used by `OrchestrationEngine`.

**Tech Stack:** Rust/Tauri 2, Axum, Tokio, serde/serde_yaml, reqwest, React, TanStack Query, Vitest, MSW.

---

## Scope Check

The v2 design document covers several independent systems: ROUTE/CASCADE, DEBATE/MoA, historical learning, bias mitigation, dynamic workflow assembly, human gates, React Flow editing, and server/team features. This plan deliberately covers only the first working slice: ROUTE/CASCADE orchestration with a runtime switch and tests. The other systems need separate plans because each can ship and be tested independently.

Not in this plan:

- DEBATE, MoA, CROSS-MODAL execution
- PredictiveRouter, StatsEngine, ThresholdOptimizer
- HumanGate and model health checks
- React Flow strategy editor
- Real project command execution such as `cargo test` or `eslint` from generated code

## Manual Review Gate

Status after manual review on 2026-06-01: **NOT READY FOR BLIND EXECUTION**. The architecture direction is sound for a ROUTE/CASCADE MVP, but this plan has drifted from the current worktree. A weaker implementation model must resolve the P1 items below before executing Task 1 through Task 10 as written.

P1 blockers:

- The plan says to create several files that already exist, especially `src-tauri/src/orchestration/quality_gate.rs` and `src-tauri/src/commands/orchestration.rs`. Do not overwrite those files with the older snippets in this plan; adapt the tasks to the current code instead.
- Command names are inconsistent. Current code registers `orchestration_status`, `orchestration_reload`, and `orchestration_toggle`, while this plan later introduces `get_orchestration_status` and `set_orchestration_enabled`. Pick one command surface and update Rust, React hooks, MSW handlers, and tests consistently.
- `orchestration_toggle` currently does not persist the enabled state to YAML; it mostly reloads and returns the old state. The runtime switch is not shippable until the command writes the runtime config, validates it, reloads the engine, and returns `{ enabled, configPath }`.
- `StrategyLoader::default_strategies_path()` is referenced by `src-tauri/src/lib.rs` and `src-tauri/src/services/proxy.rs`, but the loader implementation shown in the current worktree does not define it. The first backend task must add a single runtime path helper and remove all duplicate path logic.
- The default config references `mid_coder`, and `configs/strategies.yaml` references `mid_coder`, `mid_executor_code`, `cheap_executor_code`, and `frontier_planner`, but the orchestration config loader does not currently normalize or validate all referenced model keys. CASCADE can fail late at request time unless model references are validated on load.
- `StrategyExecutor` still uses `quick_quality_check()` instead of the existing `QualityGate`. Replace the executor logic without replacing the existing `quality_gate.rs` implementation, and make the empty-tool behavior explicit: production configs must not silently pass because no verification tool was configured.
- `ModelCaller` still has provider correctness gaps: `max_tokens` is hard-coded, known providers ignore `base_url`, OpenAI-compatible providers still need bearer auth, and usage extraction needs OpenAI `prompt_tokens` / `completion_tokens` fallbacks.
- OpenAI Responses requests hit the same OpenAI orchestration helper as chat completions, but Responses commonly uses `input`, not `messages`. Normalize `input` before executing CASCADE.

Required correction before handoff:

1. Add a short "Plan Rebase" patch that updates Task 1 through Task 9 to modify existing files rather than recreate them.
2. Run `pnpm.cmd typecheck` on Windows PowerShell because plain `pnpm typecheck` may be blocked by execution policy.
3. Do not claim Rust verification passed until MSVC Build Tools are installed and `cargo test orchestration --lib` reaches Rust compilation instead of failing at missing `link.exe`.

## Review Hardening Notes

This plan has been reviewed for execution by weaker coding models. The implementation should stay strictly sequential: complete one task, run that task's verification command, commit only the intended files, then move to the next task. Do not batch Task 1 through Task 9 in one edit because several later steps rely on APIs introduced by earlier steps.

Important compile and test traps to avoid:

- `StrategyExecutor` still contains out-of-scope `execute_debate`; when changing `caller` to `Arc<dyn ModelBackend>`, the trait must include a default `call_prompt` method so existing debate code still compiles.
- `ModelCaller::build_url` must return `base_url` first when one is configured; changing only the function visibility is not enough for the custom URL test.
- OpenAI-compatible usage fields are named `prompt_tokens` and `completion_tokens`; keep Anthropic `input_tokens` and `output_tokens` support, but add fallback extraction for OpenAI Chat usage.
- OpenAI Responses requests often use `input` rather than `messages`; the proxy orchestration helper must normalize both shapes before calling CASCADE.
- The orchestration switch is hidden unless `useProxyStatus()` reports `running: true`; the component test must override the MSW `get_proxy_status` response to make the switch visible.
- Add an explicit `aria-label` to the orchestration switch so the test can find it by role without depending on localized or mojibake text.
- Keep ROUTE as passthrough in this MVP. Do not add model rewriting, request mutation, React Flow editing, or usage-ledger integration unless a later plan explicitly adds them.

## Implementation Cautions

本节是给后续较弱模型执行时看的硬性注意事项。实现时优先遵守这里，再看每个 Task 的细节。

### 执行顺序

- 必须按 `Task 1 -> Task 10` 顺序执行，不要跳任务、合并任务、提前实现后续 Task。
- 每个 Task 完成后先运行该 Task 指定的验证命令；验证通过后再进入下一 Task。
- 每个 Task 只改该 Task 的 `Files:` 列表里的文件。验证失败需要改其它文件时，先确认该文件确实由当前失败直接要求。
- 每个 Task 的 commit 只包含当前 Task 的相关文件，不要 `git add -A`。
- 工作区已有很多未跟踪或已修改文件，开发时不要还原、删除、格式化无关文件。

### 范围边界

- 本 MVP 只交付 ROUTE/CASCADE、确定性质量门、运行时开关和测试。
- 不实现 DEBATE、MoA、CROSS-MODAL、PredictiveRouter、StatsEngine、ThresholdOptimizer、HumanGate、模型健康检查、React Flow 策略编辑器。
- `ROUTE` 在本计划中仍保持 passthrough，不要改请求体模型名，不要接管流式响应。
- `CASCADE` 只接管非流式请求；流式请求必须继续走现有 proxy passthrough。
- 不要把使用量写入现有 usage ledger；本计划只在响应 JSON 里返回 `omniagent` metadata。

### 后端注意事项

- `OrchestrationConfig::default()` 当前 cascade 引用了 `mid_coder`，但默认 `models` 里缺少它；Task 1 必须补齐。
- YAML 加载后必须 `normalize_with_defaults()` 再 `validate()`，否则运行时文件缺模型时会晚到请求阶段才失败。
- `StrategyLoader::new()` 不要继续静默 `unwrap_or_default()`；失败时要 log warning，并返回规范化后的默认配置。
- `runtime_strategies_path()` 必须使用 `crate::config::get_app_config_dir()`，不要继续手写 `dirs::config_dir()`，否则和应用配置目录覆盖逻辑不一致。
- `StrategyExecutor` 改成 `Arc<dyn ModelBackend>` 后，trait 必须包含默认 `call_prompt()`，否则现有 `execute_debate()` 编译会失败。
- 不要删除 `execute_debate()`；它虽不在本 MVP 范围内，但当前代码仍引用其 helper/test。
- 替换 CASCADE 质量逻辑后必须删除旧的 `quick_quality_check()`，避免两个质量实现并存。
- CASCADE 所有模型都没过质量门但至少有成功响应时，应返回最后一个成功响应，`verified=false`，不要直接报错。
- 只有所有模型调用都失败或没有任何成功响应时，CASCADE 才返回错误并 fallback 到 passthrough。

### ModelCaller 注意事项

- `build_url()` 必须优先使用非空 `base_url`，再按 provider 选择默认 URL。
- Anthropic 使用 `x-api-key` 和 `anthropic-version`；OpenAI、DeepSeek、Qwen 和其它 OpenAI-compatible provider 使用 `Authorization: Bearer ...`。
- 请求体 `max_tokens` 必须来自 `ModelConfig.max_tokens`，不要保留硬编码 `16384`。
- usage 解析必须同时支持 Anthropic 的 `input_tokens/output_tokens` 和 OpenAI Chat 的 `prompt_tokens/completion_tokens`。
- `auth_headers()`、`build_url()`、`extract_usage()` 保持 `pub(crate)`，方便单元测试但不扩大公共 API。

### Proxy Handler 注意事项

- `try_execute_orchestration()` 必须在创建 `RequestContext` 和 `forward_with_retry()` 之前调用；否则已经进入 provider passthrough。
- Orchestration 失败时只 log warning 并返回 `None` 让原 proxy 继续处理，不要把 CASCADE 内部错误直接返回给客户端。
- Claude `/v1/messages` 响应用 Anthropic Messages 形状。
- OpenAI `/chat/completions` 响应用 Chat Completions 形状。
- OpenAI `/responses` 响应用 Responses 形状。
- `messages` 输入可以直接传给 executor；Responses API 的 `input` 必须归一化成 messages 后再传给 executor。
- `tools` 只从请求顶层 `tools` 复制；不要在本 MVP 里改工具 schema。

### Runtime Command 注意事项

- `get_orchestration_status` 和 `set_orchestration_enabled` 必须注册到 `commands/mod.rs` 和 `tauri::generate_handler!`。
- `set_orchestration_enabled` 写 YAML 前要创建父目录。
- 如果 runtime YAML 无效，命令可以回退到默认配置，但写入前必须 normalize 和 validate。
- 切换开关后，如果 proxy server 正在运行，必须调用 `reload_orchestration_config()` 热加载；如果未运行，返回成功即可。
- `ProxyServer::state()` 只返回 `&ProxyState`，不要暴露可变引用。

### 前端注意事项

- `OrchestrationStatus` 前端字段是 `enabled` 和 `configPath`，对应 Rust `#[serde(rename_all = "camelCase")]` 的 `config_path`。
- `ProxyPanel` 里必须使用 `useOrchestrationStatus()` 和 `useSetOrchestrationEnabled()`，不要继续硬编码 `checked={false}`。
- Switch 必须加 `aria-label`，测试通过 accessible role 查找，不依赖中文文本。
- mutation pending 时禁用 Switch，避免重复写 YAML。
- 保存成功后 invalidate `["orchestrationStatus"]`。
- 当前组件的编排开关只在 proxy running 时显示；测试必须让 MSW 的 `get_proxy_status` 返回 `running: true`。

### 测试注意事项

- Rust 单元测试先写失败测试，再实现代码，不要直接跳到实现。
- `cargo test orchestration::model_caller --lib` 需要覆盖 URL、auth header、OpenAI usage、Anthropic usage。
- `cargo test orchestration::executor --lib` 不能访问真实网络，必须使用 fake backend。
- `ProxyPanel.orchestration.test.tsx` 必须通过 MSW 覆盖 `get_proxy_status`，否则开关不会渲染。
- 前端测试里如需 Tauri invoke，继续使用 `tests/msw/tauriMocks.ts`，不要手写新的 Tauri mock 体系。
- 最终验证必须跑 `cargo fmt --check`、`pnpm format:check`、Rust 相关测试、Vitest 相关测试和 `pnpm typecheck`。

### 失败处理

- 如果某个测试失败，先修当前 Task 相关文件；不要顺手重构周边模块。
- 如果编译错误来自现有非 MVP 代码，例如 `execute_debate()`，只做保持兼容的最小改动。
- 如果某 provider 真实 API 失败，不要把真实网络调用放进单元测试；用 fake backend 或 helper 函数测试确定性逻辑。
- 如果格式化命令改动大量无关文件，回退格式化改动，只格式化本计划触及文件。
- 如果发现计划和代码不一致，先更新计划或在当前 Task 增加最小兼容步骤，再继续实现。

## Current Code Map

- `src-tauri/src/orchestration/config.rs` defines YAML structs but does not validate referenced models or supply all default model keys.
- `src-tauri/src/orchestration/loader.rs` loads a file once and falls back to defaults if the file is absent.
- `src-tauri/src/orchestration/classifier.rs` already contains a rules-based `TaskClassifier`.
- `src-tauri/src/orchestration/selector.rs` already maps `TaskProfile` to `StrategyAction`.
- `src-tauri/src/orchestration/engine.rs` can decide ROUTE/CASCADE but currently owns no runtime executor in `ProxyServer`.
- `src-tauri/src/orchestration/executor.rs` contains a first-pass CASCADE executor, but it directly owns `ModelCaller` and has a weak `quick_quality_check`.
- `src-tauri/src/orchestration/model_caller.rs` calls model APIs directly, but OpenAI-compatible auth is currently wrong because it always sends `x-api-key`.
- `src-tauri/src/proxy/handlers.rs` calls `state.orchestration.decide(&body).await` and discards the decision.
- `src/components/proxy/ProxyPanel.tsx` renders an orchestration switch, but it is hard-coded to `checked={false}` and only shows a toast.
- `configs/strategies.yaml` has strategy actions but no `models:` block.

## File Structure

### Rust Backend

- Modify: `src-tauri/src/orchestration/config.rs`
  - Normalize default model entries into YAML-loaded configs.
  - Validate strategy model references and thresholds.
- Modify: `src-tauri/src/orchestration/loader.rs`
  - Expose the runtime config path helper.
  - Reload validated configs only.
- Create: `src-tauri/src/orchestration/quality_gate.rs`
  - Deterministic MVP quality checks for non-empty output, balanced code fences/brackets, JSON shape, and weak-answer phrases.
- Modify: `src-tauri/src/orchestration/executor.rs`
  - Replace `quick_quality_check` with `QualityGate`.
  - Add a mockable model backend trait so CASCADE can be unit-tested without network calls.
  - Return the strongest final attempt when all attempts fail quality but at least one model returned content.
- Modify: `src-tauri/src/orchestration/model_caller.rs`
  - Send provider-correct auth headers for Anthropic and OpenAI-compatible APIs.
  - Use configured `max_tokens`.
- Create: `src-tauri/src/orchestration/response_adapter.rs`
  - Convert `ExecutionResult` into Anthropic Messages JSON, OpenAI Chat Completions JSON, and OpenAI Responses JSON.
- Modify: `src-tauri/src/orchestration/mod.rs`
  - Export `quality_gate` and `response_adapter`.
- Modify: `src-tauri/src/orchestration/engine.rs`
  - Add `decide_with_profile`.
  - Execute CASCADE by creating a config-snapshot executor from the validated YAML.
- Modify: `src-tauri/src/proxy/server.rs`
  - Use the shared runtime strategies path from the loader.
- Modify: `src-tauri/src/proxy/handlers.rs`
  - Route non-streaming CASCADE requests through the orchestration executor.
  - Keep ROUTE and streaming requests on existing passthrough.
- Create: `src-tauri/src/commands/orchestration.rs`
  - Add commands for reading/toggling orchestration status.
- Modify: `src-tauri/src/commands/mod.rs`
  - Re-export orchestration commands.
- Modify: `src-tauri/src/lib.rs`
  - Register orchestration commands in `generate_handler!`.
- Modify: `configs/strategies.yaml`
  - Add the `models:` block used by the existing strategy actions.

### Frontend

- Modify: `src/types/proxy.ts`
  - Add `OrchestrationStatus`.
- Modify: `src/lib/api/proxy.ts`
  - Add `getOrchestrationStatus` and `setOrchestrationEnabled`.
- Modify: `src/lib/query/proxy.ts`
  - Add query/mutation hooks for the orchestration status.
- Modify: `src/components/proxy/ProxyPanel.tsx`
  - Replace the hard-coded switch with the real hook.
- Modify: `tests/msw/handlers.ts`
  - Add MSW handlers for the new Tauri commands.
- Create: `tests/components/ProxyPanel.orchestration.test.tsx`
  - Cover enabled, disabled, pending, and mutation states.

## Task 1: Validate And Normalize Orchestration Config

**Files:**
- Modify: `src-tauri/src/orchestration/config.rs`
- Modify: `configs/strategies.yaml`

- [ ] **Step 1: Add failing config tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `src-tauri/src/orchestration/config.rs`. If that module does not exist yet, create it at the bottom of the file.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_config_adds_default_model_keys() {
        let mut config = OrchestrationConfig {
            enabled: true,
            models: HashMap::new(),
            strategies: OrchestrationConfig::default().strategies,
        };

        config.normalize_with_defaults();

        assert!(config.models.contains_key("cheap_coder"));
        assert!(config.models.contains_key("mid_coder"));
        assert!(config.models.contains_key("frontier"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unknown_route_model() {
        let mut config = OrchestrationConfig::default();
        config.strategies.insert(
            "broken".to_string(),
            StrategyDef {
                description: "Broken route".to_string(),
                when: StrategyCondition::default(),
                action: StrategyAction::Route {
                    use_model: "missing_model".to_string(),
                    verify: false,
                },
            },
        );

        let err = config.validate().expect_err("unknown model should fail");
        assert!(err.contains("missing_model"));
    }

    #[test]
    fn validate_rejects_empty_cascade_models() {
        let mut config = OrchestrationConfig::default();
        config.strategies.insert(
            "broken".to_string(),
            StrategyDef {
                description: "Broken cascade".to_string(),
                when: StrategyCondition::default(),
                action: StrategyAction::Cascade {
                    models: vec![],
                    verify_each: true,
                    escalate_on_fail: true,
                    quality_threshold: 0.65,
                },
            },
        );

        let err = config.validate().expect_err("empty cascade should fail");
        assert!(err.contains("at least one model"));
    }

    #[test]
    fn validate_rejects_out_of_range_threshold() {
        let mut config = OrchestrationConfig::default();
        config.strategies.insert(
            "broken".to_string(),
            StrategyDef {
                description: "Broken threshold".to_string(),
                when: StrategyCondition::default(),
                action: StrategyAction::Cascade {
                    models: vec!["cheap_coder".to_string()],
                    verify_each: true,
                    escalate_on_fail: true,
                    quality_threshold: 1.25,
                },
            },
        );

        let err = config.validate().expect_err("threshold should fail");
        assert!(err.contains("quality_threshold"));
    }
}
```

- [ ] **Step 2: Run the config tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test orchestration::config --lib
```

Expected: FAIL because `normalize_with_defaults` and `validate` do not exist.

- [ ] **Step 3: Add default `mid_coder`, normalization, and validation**

In `src-tauri/src/orchestration/config.rs`, add this implementation below `impl Default for OrchestrationConfig`:

```rust
impl OrchestrationConfig {
    pub fn normalize_with_defaults(&mut self) {
        let defaults = OrchestrationConfig::default();
        for (key, model) in defaults.models {
            self.models.entry(key).or_insert(model);
        }
        if self.strategies.is_empty() {
            self.strategies = defaults.strategies;
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (strategy_name, strategy) in &self.strategies {
            match &strategy.action {
                StrategyAction::Route { use_model, .. } => {
                    self.require_model(strategy_name, use_model)?;
                }
                StrategyAction::Cascade {
                    models,
                    quality_threshold,
                    ..
                } => {
                    if models.is_empty() {
                        return Err(format!(
                            "strategy '{strategy_name}' cascade must contain at least one model"
                        ));
                    }
                    if !(0.0..=1.0).contains(quality_threshold) {
                        return Err(format!(
                            "strategy '{strategy_name}' quality_threshold must be between 0.0 and 1.0"
                        ));
                    }
                    for model_key in models {
                        self.require_model(strategy_name, model_key)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn require_model(&self, strategy_name: &str, model_key: &str) -> Result<(), String> {
        if self.models.contains_key(model_key) {
            Ok(())
        } else {
            Err(format!(
                "strategy '{strategy_name}' references unknown model '{model_key}'"
            ))
        }
    }
}
```

Also add `mid_coder` in `Default::default()` after `cheap_coder`:

```rust
models.insert(
    "mid_coder".to_string(),
    ModelConfig {
        provider: "openai".to_string(),
        model: "gpt-5-mini".to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        base_url: None,
        max_tokens: 16384,
    },
);
```

- [ ] **Step 4: Update bundled YAML with model definitions**

In `configs/strategies.yaml`, insert this block between `enabled: false` and `strategies:`:

```yaml
models:
  cheap_coder:
    provider: deepseek
    model: deepseek-chat
    api_key_env: DEEPSEEK_API_KEY
    max_tokens: 16384

  mid_coder:
    provider: openai
    model: gpt-5-mini
    api_key_env: OPENAI_API_KEY
    max_tokens: 16384

  frontier:
    provider: anthropic
    model: claude-sonnet-4-20250514
    api_key_env: ANTHROPIC_API_KEY
    max_tokens: 16384
```

- [ ] **Step 5: Run config tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::config --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/config.rs configs/strategies.yaml
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: validate orchestration strategy config"
```

## Task 2: Load Runtime Strategy Files Safely

**Files:**
- Modify: `src-tauri/src/orchestration/loader.rs`
- Modify: `src-tauri/src/proxy/server.rs`

- [ ] **Step 1: Add failing loader tests**

Append these tests to `src-tauri/src/orchestration/loader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_missing_file_uses_valid_normalized_defaults() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("strategies.yaml");

        let config = StrategyLoader::load_from_file(&path).unwrap();

        assert!(!config.enabled);
        assert!(config.models.contains_key("mid_coder"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn load_invalid_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("strategies.yaml");
        std::fs::write(
            &path,
            r#"
enabled: true
strategies:
  broken:
    description: broken
    action:
      type: route
      use_model: missing_model
"#,
        )
        .unwrap();

        let err = StrategyLoader::load_from_file(&path).expect_err("invalid config should fail");
        assert!(err.to_string().contains("missing_model"));
    }
}
```

- [ ] **Step 2: Run loader tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test orchestration::loader --lib
```

Expected: FAIL because loader does not normalize or validate configs.

- [ ] **Step 3: Normalize and validate inside `load_from_file`**

Replace the final parse block in `StrategyLoader::load_from_file` with:

```rust
let content = std::fs::read_to_string(path)?;
let mut config: OrchestrationConfig = serde_yaml::from_str(&content)?;
config.normalize_with_defaults();
config.validate().map_err(anyhow::Error::msg)?;
log::info!(
    "[Orchestration] Loaded {} strategies from {:?}",
    config.strategies.len(),
    path
);
Ok(config)
```

For the missing-file branch, normalize and validate the default config before returning:

```rust
let mut config = OrchestrationConfig::default();
config.normalize_with_defaults();
config.validate().map_err(anyhow::Error::msg)?;
return Ok(config);
```

Also replace the current `StrategyLoader::new` body so invalid runtime YAML does not disappear through `unwrap_or_default()` without a log:

```rust
pub fn new(path: PathBuf) -> Self {
    let config = match Self::load_from_file(&path) {
        Ok(config) => config,
        Err(error) => {
            log::warn!(
                "[Orchestration] Failed to load strategy config from {:?}, using defaults: {}",
                path,
                error
            );
            let mut fallback = OrchestrationConfig::default();
            fallback.normalize_with_defaults();
            fallback
        }
    };
    Self {
        config: Arc::new(RwLock::new(config)),
        path,
    }
}
```

- [ ] **Step 4: Add one runtime path helper**

Add this function to `src-tauri/src/orchestration/loader.rs`:

```rust
pub fn runtime_strategies_path() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("omniagent")
        .join("strategies.yaml")
}
```

In `src-tauri/src/proxy/server.rs`, replace the manual `dirs::config_dir()` block in `ProxyServer::new` with:

```rust
let strategies_path = crate::orchestration::loader::runtime_strategies_path();
let orchestration = Arc::new(OrchestrationEngine::new(strategies_path));
```

- [ ] **Step 5: Run loader tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::loader --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/loader.rs src-tauri/src/proxy/server.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: normalize orchestration strategy loading"
```

## Task 3: Add Deterministic QualityGate

**Files:**
- Create: `src-tauri/src/orchestration/quality_gate.rs`
- Modify: `src-tauri/src/orchestration/mod.rs`

- [ ] **Step 1: Create the failing quality gate tests first**

Create `src-tauri/src/orchestration/quality_gate.rs` with this test-only skeleton:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_answer_fails() {
        let result = QualityGate::default().verify("");
        assert!(!result.passed);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn balanced_code_answer_passes_default_threshold() {
        let answer = "```rust\nfn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n```\nBecause the function returns the sum.";
        let result = QualityGate::default().verify(answer);
        assert!(result.passed);
        assert!(result.score >= 0.65, "score was {}", result.score);
    }

    #[test]
    fn unclosed_code_fence_is_penalized() {
        let answer = "```rust\nfn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let result = QualityGate::default().verify(answer);
        assert!(!result.passed);
        assert!(result.reasons.iter().any(|reason| reason.contains("code fence")));
    }

    #[test]
    fn weak_uncertain_answer_is_penalized() {
        let answer = "I am not sure. This might work, but it could be wrong.";
        let result = QualityGate::default().verify(answer);
        assert!(!result.passed);
        assert!(result.reasons.iter().any(|reason| reason.contains("uncertain")));
    }
}
```

- [ ] **Step 2: Register the module and run tests**

Add this line to `src-tauri/src/orchestration/mod.rs`:

```rust
pub mod quality_gate;
```

Run:

```bash
cd src-tauri
cargo test orchestration::quality_gate --lib
```

Expected: FAIL because `QualityGate` is not defined.

- [ ] **Step 3: Implement the deterministic quality gate**

Insert this implementation above the tests in `quality_gate.rs`:

```rust
#[derive(Debug, Clone)]
pub struct QualityGate {
    threshold: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityResult {
    pub passed: bool,
    pub score: f64,
    pub reasons: Vec<String>,
}

impl Default for QualityGate {
    fn default() -> Self {
        Self { threshold: 0.65 }
    }
}

impl QualityGate {
    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    pub fn verify(&self, content: &str) -> QualityResult {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return QualityResult {
                passed: false,
                score: 0.0,
                reasons: vec!["empty answer".to_string()],
            };
        }

        let mut score: f64 = 0.45;
        let mut reasons = Vec::new();

        let word_count = trimmed.split_whitespace().count();
        if word_count >= 20 {
            score += 0.10;
        } else {
            reasons.push("answer is very short".to_string());
        }

        if word_count >= 80 {
            score += 0.05;
        }

        if has_balanced_code_fences(trimmed) {
            score += 0.10;
        } else {
            score -= 0.25;
            reasons.push("unbalanced code fence".to_string());
        }

        if has_balanced_delimiters(trimmed) {
            score += 0.10;
        } else {
            score -= 0.20;
            reasons.push("unbalanced brackets".to_string());
        }

        let lower = trimmed.to_lowercase();
        if lower.contains("because") || lower.contains("therefore") || lower.contains("reason") {
            score += 0.05;
        }

        if lower.contains("i don't know")
            || lower.contains("i am not sure")
            || lower.contains("i'm not sure")
            || lower.contains("might work")
        {
            score -= 0.25;
            reasons.push("uncertain answer".to_string());
        }

        let score = score.clamp(0.0, 1.0);
        QualityResult {
            passed: score >= self.threshold,
            score,
            reasons,
        }
    }
}

fn has_balanced_code_fences(content: &str) -> bool {
    content.matches("```").count() % 2 == 0
}

fn has_balanced_delimiters(content: &str) -> bool {
    let mut stack = Vec::new();
    for ch in content.chars() {
        match ch {
            '(' | '[' | '{' => stack.push(ch),
            ')' => {
                if stack.pop() != Some('(') {
                    return false;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return false;
                }
            }
            '}' => {
                if stack.pop() != Some('{') {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}
```

- [ ] **Step 4: Run tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::quality_gate --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/quality_gate.rs src-tauri/src/orchestration/mod.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: add deterministic orchestration quality gate"
```

## Task 4: Make CASCADE Executor Testable And Quality-Gated

**Files:**
- Modify: `src-tauri/src/orchestration/executor.rs`
- Modify: `src-tauri/src/orchestration/model_caller.rs`

- [ ] **Step 1: Add failing executor tests with a fake backend**

Replace the existing `mod tests` in `src-tauri/src/orchestration/executor.rs` with tests that include the previous score/prompt tests plus these CASCADE tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::model_caller::TokenUsage;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FakeBackend {
        calls: Arc<Mutex<Vec<String>>>,
        responses: Arc<Mutex<Vec<Result<ModelResponse, String>>>>,
    }

    impl FakeBackend {
        fn new(responses: Vec<Result<ModelResponse, String>>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses)),
            }
        }
    }

    impl ModelBackend for FakeBackend {
        fn call<'a>(
            &'a self,
            model_key: &'a str,
            _messages: Vec<Value>,
            _tools: Option<Vec<Value>>,
            _temperature: Option<f64>,
        ) -> Pin<Box<dyn Future<Output = Result<ModelResponse, String>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(model_key.to_string());
                self.responses.lock().unwrap().remove(0)
            })
        }
    }

    fn response(content: &str, model: &str) -> ModelResponse {
        ModelResponse {
            content: content.to_string(),
            model: model.to_string(),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
            latency_ms: 1,
        }
    }

    #[tokio::test]
    async fn cascade_stops_on_first_quality_pass() {
        let backend = FakeBackend::new(vec![
            Ok(response("I am not sure. This might work.", "cheap")),
            Ok(response(
                "```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```\nBecause this returns the sum.",
                "mid",
            )),
            Ok(response("unused", "frontier")),
        ]);
        let calls = backend.calls.clone();
        let executor = StrategyExecutor::with_backend(Arc::new(backend));
        let decision = OrchestrationDecision::Cascade {
            models: vec![
                "cheap_coder".to_string(),
                "mid_coder".to_string(),
                "frontier".to_string(),
            ],
            quality_threshold: 0.65,
        };

        let result = executor.execute(&decision, vec![], None).await.unwrap();

        assert_eq!(result.model_used, "mid");
        assert_eq!(result.cascade_attempts, 2);
        assert!(result.verified);
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &["cheap_coder".to_string(), "mid_coder".to_string()]
        );
    }

    #[tokio::test]
    async fn cascade_returns_best_available_response_when_quality_never_passes() {
        let backend = FakeBackend::new(vec![
            Ok(response("bad", "cheap")),
            Ok(response("still weak but not empty", "mid")),
        ]);
        let executor = StrategyExecutor::with_backend(Arc::new(backend));
        let decision = OrchestrationDecision::Cascade {
            models: vec!["cheap_coder".to_string(), "mid_coder".to_string()],
            quality_threshold: 0.95,
        };

        let result = executor.execute(&decision, vec![], None).await.unwrap();

        assert_eq!(result.model_used, "mid");
        assert_eq!(result.cascade_attempts, 2);
        assert!(!result.verified);
        assert!(result.judge_score.is_some());
    }

    #[test]
    fn extract_score_from_valid_judge_response() {
        let content = "SCORE: 0.85\nANSWER:\nThe best approach is...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(0.85));
    }

    #[test]
    fn extract_score_clamps_to_range() {
        let content = "SCORE: 1.5\nANSWER:\n...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(1.0));

        let content = "SCORE: -0.2\nANSWER:\n...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(0.0));
    }

    #[test]
    fn extract_score_missing_returns_none() {
        let content = "No score line here, just a regular response.";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), None);
    }
}
```

- [ ] **Step 2: Run executor tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test orchestration::executor --lib
```

Expected: FAIL because `ModelBackend`, `with_backend`, and quality-gated fallback behavior do not exist.

- [ ] **Step 3: Add the model backend trait**

At the top of `executor.rs`, add:

```rust
use crate::orchestration::quality_gate::QualityGate;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub trait ModelBackend: Send + Sync {
    fn call<'a>(
        &'a self,
        model_key: &'a str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
        temperature: Option<f64>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelResponse, String>> + Send + 'a>>;

    fn call_prompt<'a>(
        &'a self,
        model_key: &'a str,
        _system: &'a str,
        user_prompt: &'a str,
        temperature: Option<f64>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelResponse, String>> + Send + 'a>> {
        let messages = vec![json!({
            "role": "user",
            "content": user_prompt,
        })];
        self.call(model_key, messages, None, temperature)
    }
}
```

In `model_caller.rs`, implement the trait:

```rust
impl crate::orchestration::executor::ModelBackend for ModelCaller {
    fn call<'a>(
        &'a self,
        model_key: &'a str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
        temperature: Option<f64>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ModelResponse, String>> + Send + 'a>> {
        Box::pin(async move { self.call(model_key, messages, tools, temperature).await })
    }
}
```

The default `call_prompt` is required because `execute_debate` is still present in `executor.rs` even though DEBATE is out of this MVP scope. Do not delete `execute_debate` in this task.

- [ ] **Step 4: Update `StrategyExecutor` construction**

Replace only the `caller` field and constructor block in `executor.rs`; keep the existing `execute`, `execute_route`, `execute_cascade`, and `execute_debate` methods below it:

```rust
pub struct StrategyExecutor {
    caller: Arc<dyn ModelBackend>,
}

impl StrategyExecutor {
    pub fn new(models: HashMap<String, ModelConfig>) -> Self {
        Self {
            caller: Arc::new(ModelCaller::new(models)),
        }
    }

    pub fn with_backend(caller: Arc<dyn ModelBackend>) -> Self {
        Self { caller }
    }
}
```

- [ ] **Step 5: Replace CASCADE quality logic**

Inside `execute_cascade`, replace `quick_quality_check` usage with `QualityGate::with_threshold(quality_threshold)`. Keep track of the last successful response:

```rust
let gate = QualityGate::with_threshold(quality_threshold);
let mut last_successful: Option<(ModelResponse, f64)> = None;
```

For each successful response:

```rust
let quality = gate.verify(&resp.content);
let score = quality.score;
last_successful = Some((resp.clone(), score));

if quality.passed {
    log::info!(
        "[Cascade] Model '{}' passed quality check (score={:.2} >= {:.2})",
        model_key,
        score,
        quality_threshold
    );
    return Ok(ExecutionResult {
        content: resp.content,
        model_used: resp.model,
        strategy: "cascade".to_string(),
        total_latency_ms: start.elapsed().as_millis() as u64,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        cascade_attempts: attempts,
        verified: true,
        judge_score: Some(score),
    });
}
```

After the loop, before returning `Err`, add:

```rust
if let Some((resp, score)) = last_successful {
    return Ok(ExecutionResult {
        content: resp.content,
        model_used: resp.model,
        strategy: "cascade".to_string(),
        total_latency_ms: start.elapsed().as_millis() as u64,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        cascade_attempts: attempts,
        verified: false,
        judge_score: Some(score),
    });
}
```

Delete the old `quick_quality_check` method after this replacement. It should not remain as a second, unused quality implementation.

- [ ] **Step 6: Run executor tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::executor --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/executor.rs src-tauri/src/orchestration/model_caller.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: quality gate cascade orchestration"
```

## Task 5: Fix Provider-Specific ModelCaller Requests

**Files:**
- Modify: `src-tauri/src/orchestration/model_caller.rs`

- [ ] **Step 1: Add failing URL, auth-header, and usage tests**

Append these tests to `model_caller.rs`:

```rust
#[test]
fn build_url_uses_custom_base_url() {
    let config = ModelConfig {
        provider: "openai".to_string(),
        model: "custom-model".to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        base_url: Some("https://relay.example/v1/chat/completions".to_string()),
        max_tokens: 4096,
    };

    assert_eq!(
        ModelCaller::build_url(&config),
        "https://relay.example/v1/chat/completions"
    );
}

#[test]
fn auth_headers_for_openai_compatible_use_bearer() {
    let config = ModelConfig {
        provider: "openai".to_string(),
        model: "gpt-5-mini".to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        base_url: None,
        max_tokens: 4096,
    };

    let headers = ModelCaller::auth_headers(&config, "sk-test");

    assert_eq!(headers, vec![("Authorization".to_string(), "Bearer sk-test".to_string())]);
}

#[test]
fn auth_headers_for_anthropic_use_x_api_key_and_version() {
    let config = ModelConfig {
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        api_key_env: "ANTHROPIC_API_KEY".to_string(),
        base_url: None,
        max_tokens: 4096,
    };

    let headers = ModelCaller::auth_headers(&config, "sk-ant-test");

    assert!(headers.contains(&("x-api-key".to_string(), "sk-ant-test".to_string())));
    assert!(headers.contains(&(
        "anthropic-version".to_string(),
        "2023-06-01".to_string()
    )));
}

#[test]
fn extract_usage_accepts_openai_chat_usage_names() {
    let body = json!({
        "usage": {
            "prompt_tokens": 11,
            "completion_tokens": 7
        }
    });

    let usage = ModelCaller::extract_usage(&body);

    assert_eq!(usage.input_tokens, 11);
    assert_eq!(usage.output_tokens, 7);
}

#[test]
fn extract_usage_accepts_anthropic_usage_names() {
    let body = json!({
        "usage": {
            "input_tokens": 13,
            "output_tokens": 5
        }
    });

    let usage = ModelCaller::extract_usage(&body);

    assert_eq!(usage.input_tokens, 13);
    assert_eq!(usage.output_tokens, 5);
}
```

- [ ] **Step 2: Run model caller tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test orchestration::model_caller --lib
```

Expected: FAIL because `auth_headers` and `extract_usage` are missing, `build_url` is private, and the current `build_url` ignores `base_url` for known providers.

- [ ] **Step 3: Expose deterministic request helpers**

Replace `build_url` with a version that honors explicit `base_url` before provider defaults:

```rust
pub(crate) fn build_url(config: &ModelConfig) -> String {
    if let Some(base_url) = config.base_url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
        return base_url.to_string();
    }

    match config.provider.as_str() {
        "anthropic" => "https://api.anthropic.com/v1/messages".to_string(),
        "openai" => "https://api.openai.com/v1/chat/completions".to_string(),
        "deepseek" => "https://api.deepseek.com/v1/chat/completions".to_string(),
        "qwen" => "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string(),
        _ => "https://api.example.com/v1/chat/completions".to_string(),
    }
}
```

Add:

```rust
pub(crate) fn auth_headers(config: &ModelConfig, api_key: &str) -> Vec<(String, String)> {
    match config.provider.as_str() {
        "anthropic" => vec![
            ("x-api-key".to_string(), api_key.to_string()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
        ],
        _ => vec![(
            "Authorization".to_string(),
            format!("Bearer {api_key}"),
        )],
    }
}
```

In `call`, replace the fixed header chain with:

```rust
let mut request = self
    .client
    .post(&url)
    .header("Content-Type", "application/json");
for (name, value) in Self::auth_headers(config, &api_key) {
    request = request.header(name.as_str(), value.as_str());
}
let resp = request
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("HTTP error calling '{}': {}", model_key, e))?;
```

Also use configured `max_tokens`:

```rust
"max_tokens": config.max_tokens,
```

Add a deterministic usage helper and replace the inline `TokenUsage { ... }` construction in `call` with `let usage = Self::extract_usage(&resp_body);`:

```rust
pub(crate) fn extract_usage(resp_body: &Value) -> TokenUsage {
    let usage = resp_body.get("usage");
    TokenUsage {
        input_tokens: usage
            .and_then(|u| u.get("input_tokens"))
            .or_else(|| usage.and_then(|u| u.get("prompt_tokens")))
            .and_then(|t| t.as_u64())
            .unwrap_or(0),
        output_tokens: usage
            .and_then(|u| u.get("output_tokens"))
            .or_else(|| usage.and_then(|u| u.get("completion_tokens")))
            .and_then(|t| t.as_u64())
            .unwrap_or(0),
    }
}
```

- [ ] **Step 4: Run model caller tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::model_caller --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/model_caller.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix: use provider-specific orchestration auth headers"
```

## Task 6: Add Response Adapters For Orchestrated CASCADE

**Files:**
- Create: `src-tauri/src/orchestration/response_adapter.rs`
- Modify: `src-tauri/src/orchestration/mod.rs`

- [ ] **Step 1: Create failing response adapter tests**

Create `src-tauri/src/orchestration/response_adapter.rs` with:

```rust
use crate::orchestration::executor::ExecutionResult;
use serde_json::{json, Value};

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> ExecutionResult {
        ExecutionResult {
            content: "final answer".to_string(),
            model_used: "gpt-5-mini".to_string(),
            strategy: "cascade".to_string(),
            total_latency_ms: 123,
            total_input_tokens: 10,
            total_output_tokens: 20,
            cascade_attempts: 2,
            verified: true,
            judge_score: Some(0.72),
        }
    }

    #[test]
    fn anthropic_response_contains_text_and_usage() {
        let value = anthropic_messages_response(&result(), "claude-request");

        assert_eq!(value["type"], "message");
        assert_eq!(value["model"], "gpt-5-mini");
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][0]["text"], "final answer");
        assert_eq!(value["usage"]["input_tokens"], 10);
        assert_eq!(value["usage"]["output_tokens"], 20);
    }

    #[test]
    fn openai_chat_response_contains_choice() {
        let value = openai_chat_response(&result(), "gpt-request");

        assert_eq!(value["object"], "chat.completion");
        assert_eq!(value["model"], "gpt-5-mini");
        assert_eq!(value["choices"][0]["message"]["content"], "final answer");
    }

    #[test]
    fn openai_responses_response_contains_output_text() {
        let value = openai_responses_response(&result(), "gpt-request");

        assert_eq!(value["object"], "response");
        assert_eq!(value["model"], "gpt-5-mini");
        assert_eq!(value["output"][0]["content"][0]["text"], "final answer");
    }
}
```

- [ ] **Step 2: Register module and run tests**

Add to `src-tauri/src/orchestration/mod.rs`:

```rust
pub mod response_adapter;
```

Run:

```bash
cd src-tauri
cargo test orchestration::response_adapter --lib
```

Expected: FAIL because the adapter functions do not exist.

- [ ] **Step 3: Implement response adapter functions**

Add these functions above the tests in `response_adapter.rs`:

```rust
pub fn anthropic_messages_response(result: &ExecutionResult, request_model: &str) -> Value {
    json!({
        "id": format!("msg_omni_{}", uuid::Uuid::new_v4()),
        "type": "message",
        "role": "assistant",
        "model": result.model_used,
        "content": [{
            "type": "text",
            "text": result.content
        }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": result.total_input_tokens,
            "output_tokens": result.total_output_tokens
        },
        "omniagent": {
            "request_model": request_model,
            "strategy": result.strategy,
            "cascade_attempts": result.cascade_attempts,
            "verified": result.verified,
            "judge_score": result.judge_score,
            "latency_ms": result.total_latency_ms
        }
    })
}

pub fn openai_chat_response(result: &ExecutionResult, request_model: &str) -> Value {
    json!({
        "id": format!("chatcmpl-omni-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": result.model_used,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": result.content
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": result.total_input_tokens,
            "completion_tokens": result.total_output_tokens,
            "total_tokens": result.total_input_tokens + result.total_output_tokens
        },
        "omniagent": {
            "request_model": request_model,
            "strategy": result.strategy,
            "cascade_attempts": result.cascade_attempts,
            "verified": result.verified,
            "judge_score": result.judge_score,
            "latency_ms": result.total_latency_ms
        }
    })
}

pub fn openai_responses_response(result: &ExecutionResult, request_model: &str) -> Value {
    json!({
        "id": format!("resp_omni_{}", uuid::Uuid::new_v4()),
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": "completed",
        "model": result.model_used,
        "output": [{
            "id": format!("msg_{}", uuid::Uuid::new_v4()),
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": result.content
            }]
        }],
        "usage": {
            "input_tokens": result.total_input_tokens,
            "output_tokens": result.total_output_tokens,
            "total_tokens": result.total_input_tokens + result.total_output_tokens
        },
        "omniagent": {
            "request_model": request_model,
            "strategy": result.strategy,
            "cascade_attempts": result.cascade_attempts,
            "verified": result.verified,
            "judge_score": result.judge_score,
            "latency_ms": result.total_latency_ms
        }
    })
}
```

- [ ] **Step 4: Run tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration::response_adapter --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/response_adapter.rs src-tauri/src/orchestration/mod.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: adapt orchestration results to client responses"
```

## Task 7: Execute CASCADE From Proxy Handlers

**Files:**
- Modify: `src-tauri/src/orchestration/engine.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`

- [ ] **Step 1: Add engine tests for config-snapshot execution**

Add this test to `src-tauri/src/orchestration/engine.rs`:

```rust
#[tokio::test]
async fn enabled_cascade_decision_keeps_model_chain_snapshot() {
    let yaml = r#"
enabled: true
models:
  cheap_coder:
    provider: deepseek
    model: deepseek-chat
    api_key_env: DEEPSEEK_API_KEY
  mid_coder:
    provider: openai
    model: gpt-5-mini
    api_key_env: OPENAI_API_KEY
strategies:
  cascade:
    description: "Cascade"
    when:
      complexity: [0, 1]
      risk: ["medium"]
    action:
      type: cascade
      models: [cheap_coder, mid_coder]
      quality_threshold: 0.65
"#;
    let (engine, _dir) = create_engine_with_yaml(yaml);
    let body = json!({
        "messages": [{"role": "user", "content": "fix this bug with code"}],
        "tools": [{"name": "bash", "type": "function"}],
        "model": "claude-sonnet"
    });

    let decision = engine.decide(&body).await;

    match decision {
        OrchestrationDecision::Cascade { models, quality_threshold } => {
            assert_eq!(models, vec!["cheap_coder", "mid_coder"]);
            assert_eq!(quality_threshold, 0.65);
        }
        other => panic!("Expected cascade, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run engine tests**

Run:

```bash
cd src-tauri
cargo test orchestration::engine --lib
```

Expected: PASS after previous tasks. If it fails because classifier complexity is too low, change the test input only by increasing content length with repeated code text until complexity enters the configured range.

- [ ] **Step 3: Add `execute_with_current_config` to engine**

In `engine.rs`, add:

```rust
pub async fn execute_with_current_config(
    &self,
    decision: &OrchestrationDecision,
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
) -> Result<ExecutionResult, String> {
    let config = self.loader.get_config().await;
    let executor = StrategyExecutor::new(config.models);
    executor.execute(decision, messages, tools).await
}
```

Update `decide_and_execute` to call `execute_with_current_config` instead of `execute`.

- [ ] **Step 4: Add handler helper for CASCADE-only execution**

In `src-tauri/src/proxy/handlers.rs`, add imports:

```rust
use crate::orchestration::engine::OrchestrationDecision;
use crate::orchestration::response_adapter::{
    anthropic_messages_response, openai_chat_response, openai_responses_response,
};
```

Add this helper near the top of the file after `get_status`:

```rust
fn extract_messages_for_orchestration(body: &Value) -> Vec<Value> {
    if let Some(messages) = body.get("messages").and_then(|value| value.as_array()) {
        return messages.clone();
    }

    match body.get("input") {
        Some(Value::String(text)) => vec![json!({
            "role": "user",
            "content": text,
        })],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                let role = item
                    .get("role")
                    .and_then(|value| value.as_str())
                    .unwrap_or("user");
                let content = item
                    .get("content")
                    .cloned()
                    .unwrap_or_else(|| Value::String(item.to_string()));
                Some(json!({
                    "role": role,
                    "content": content,
                }))
            })
            .collect(),
        Some(other) => vec![json!({
            "role": "user",
            "content": other,
        })],
        None => Vec::new(),
    }
}

async fn try_execute_orchestration(
    state: &ProxyState,
    body: &Value,
    response_shape: OrchestratedResponseShape,
) -> Option<Result<axum::response::Response, ProxyError>> {
    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    if is_stream {
        return None;
    }

    let decision = state.orchestration.decide(body).await;
    if !matches!(decision, OrchestrationDecision::Cascade { .. }) {
        return None;
    }

    let messages = extract_messages_for_orchestration(body);
    let tools = body.get("tools").and_then(|value| value.as_array()).cloned();

    let result = match state
        .orchestration
        .execute_with_current_config(&decision, messages, tools)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            log::warn!("[Orchestration] CASCADE failed, falling back to passthrough: {error}");
            return None;
        }
    };

    let request_model = body
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let payload = match response_shape {
        OrchestratedResponseShape::AnthropicMessages => {
            anthropic_messages_response(&result, request_model)
        }
        OrchestratedResponseShape::OpenAiChat => openai_chat_response(&result, request_model),
        OrchestratedResponseShape::OpenAiResponses => {
            openai_responses_response(&result, request_model)
        }
    };

    let body = match serde_json::to_vec(&payload) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Some(Err(ProxyError::Internal(format!(
                "Failed to serialize orchestrated response: {error}"
            ))));
        }
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    Some(Ok((headers, axum::body::Body::from(body)).into_response()))
}

#[derive(Debug, Clone, Copy)]
enum OrchestratedResponseShape {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
}
```

- [ ] **Step 5: Call the helper from Claude and Codex handlers**

In `handle_messages_for_app`, after parsing `body` and before creating `RequestContext`, replace the discarded decision line with:

```rust
if let Some(response) =
    try_execute_orchestration(&state, &body, OrchestratedResponseShape::AnthropicMessages).await
{
    return response;
}
```

In `handle_chat_completions`, after parsing `body` and before creating `RequestContext`, add:

```rust
if let Some(response) =
    try_execute_orchestration(&state, &body, OrchestratedResponseShape::OpenAiChat).await
{
    return response;
}
```

In `handle_responses`, after parsing `body` and before creating `RequestContext`, add:

```rust
if let Some(response) =
    try_execute_orchestration(&state, &body, OrchestratedResponseShape::OpenAiResponses).await
{
    return response;
}
```

- [ ] **Step 6: Run focused Rust tests and commit**

Run:

```bash
cd src-tauri
cargo test orchestration --lib
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/orchestration/engine.rs src-tauri/src/proxy/handlers.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: execute cascade orchestration in proxy handlers"
```

## Task 8: Add Runtime Orchestration Commands

**Files:**
- Create: `src-tauri/src/commands/orchestration.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/services/proxy.rs`
- Modify: `src-tauri/src/proxy/server.rs`

- [ ] **Step 1: Add command implementation**

Create `src-tauri/src/commands/orchestration.rs`:

```rust
use crate::orchestration::loader::{runtime_strategies_path, StrategyLoader};
use crate::orchestration::OrchestrationConfig;
use crate::AppState;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationStatus {
    pub enabled: bool,
    pub config_path: String,
}

#[tauri::command]
pub async fn get_orchestration_status() -> Result<OrchestrationStatus, String> {
    let path = runtime_strategies_path();
    let config = StrategyLoader::load_from_file(&path).map_err(|error| error.to_string())?;
    Ok(OrchestrationStatus {
        enabled: config.enabled,
        config_path: path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn set_orchestration_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<OrchestrationStatus, String> {
    let path = runtime_strategies_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut config = StrategyLoader::load_from_file(&path).unwrap_or_else(|_| {
        let mut fallback = OrchestrationConfig::default();
        fallback.normalize_with_defaults();
        fallback
    });
    config.enabled = enabled;
    config.normalize_with_defaults();
    config.validate()?;

    let content = serde_yaml::to_string(&config).map_err(|error| error.to_string())?;
    std::fs::write(&path, content).map_err(|error| error.to_string())?;

    state.proxy_service.reload_orchestration_config().await?;

    Ok(OrchestrationStatus {
        enabled,
        config_path: path.to_string_lossy().to_string(),
    })
}
```

- [ ] **Step 2: Add proxy service reload method**

In `src-tauri/src/services/proxy.rs`, add this method inside `impl ProxyService`:

```rust
pub async fn reload_orchestration_config(&self) -> Result<(), String> {
    let server_guard = self.server.read().await;
    if let Some(server) = server_guard.as_ref() {
        server
            .state()
            .orchestration
            .reload_config()
            .await
            .map_err(|error| format!("reload orchestration config failed: {error}"))?;
    }
    Ok(())
}
```

If `ProxyServer::state()` does not exist, add this method inside `impl ProxyServer` in `src-tauri/src/proxy/server.rs`:

```rust
pub fn state(&self) -> &ProxyState {
    &self.state
}
```

- [ ] **Step 3: Re-export and register commands**

In `src-tauri/src/commands/mod.rs`, add:

```rust
mod orchestration;
pub use orchestration::*;
```

In `src-tauri/src/lib.rs`, add these to `tauri::generate_handler!` near other proxy commands:

```rust
commands::get_orchestration_status,
commands::set_orchestration_enabled,
```

- [ ] **Step 4: Run Rust command build checks**

Run:

```bash
cd src-tauri
cargo test orchestration --lib
cargo test proxy_commands --test proxy_commands
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src-tauri/src/commands/orchestration.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/src/services/proxy.rs src-tauri/src/proxy/server.rs
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: add orchestration runtime commands"
```

## Task 9: Wire The ProxyPanel Orchestration Switch

**Files:**
- Modify: `src/types/proxy.ts`
- Modify: `src/lib/api/proxy.ts`
- Modify: `src/lib/query/proxy.ts`
- Modify: `src/components/proxy/ProxyPanel.tsx`
- Modify: `tests/msw/handlers.ts`
- Create: `tests/components/ProxyPanel.orchestration.test.tsx`

- [ ] **Step 1: Add frontend API types and calls**

In `src/types/proxy.ts`, add:

```ts
export interface OrchestrationStatus {
  enabled: boolean;
  configPath: string;
}
```

In `src/lib/api/proxy.ts`, import the new type:

```ts
  OrchestrationStatus,
```

Add these methods inside `proxyApi`:

```ts
  async getOrchestrationStatus(): Promise<OrchestrationStatus> {
    return invoke("get_orchestration_status");
  },

  async setOrchestrationEnabled(enabled: boolean): Promise<OrchestrationStatus> {
    return invoke("set_orchestration_enabled", { enabled });
  },
```

- [ ] **Step 2: Add query hooks**

In `src/lib/query/proxy.ts`, add:

```ts
export function useOrchestrationStatus() {
  return useQuery({
    queryKey: ["orchestrationStatus"],
    queryFn: () => proxyApi.getOrchestrationStatus(),
  });
}

export function useSetOrchestrationEnabled() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (enabled: boolean) => proxyApi.setOrchestrationEnabled(enabled),
    onSuccess: () => {
      toast.success(t("proxy.orchestration.saved", { defaultValue: "Orchestration setting saved" }), {
        closeButton: true,
      });
      queryClient.invalidateQueries({ queryKey: ["orchestrationStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.orchestration.saveFailed", {
          defaultValue: "Failed to update orchestration: {{error}}",
          error: error.message,
        }),
      );
    },
  });
}
```

- [ ] **Step 3: Use hooks in `ProxyPanel`**

Change this import in `src/components/proxy/ProxyPanel.tsx`:

```ts
  useUpdateGlobalProxyConfig,
  useOrchestrationStatus,
  useSetOrchestrationEnabled,
} from "@/lib/query/proxy";
```

Inside `ProxyPanel`, after `updateGlobalConfig`, add:

```ts
const { data: orchestrationStatus } = useOrchestrationStatus();
const setOrchestrationEnabled = useSetOrchestrationEnabled();
```

Replace the hard-coded switch with:

```tsx
<Switch
  aria-label={t("proxy.orchestration.title", {
    defaultValue: "Orchestration",
  })}
  checked={orchestrationStatus?.enabled ?? false}
  onCheckedChange={(checked) => setOrchestrationEnabled.mutate(checked)}
  disabled={setOrchestrationEnabled.isPending}
/>
```

Replace the description paragraph with one that shows the YAML path only when loaded:

```tsx
<p className="text-xs text-muted-foreground">
  {orchestrationStatus?.configPath
    ? t("proxy.orchestration.path", {
        defaultValue: "YAML: {{path}}",
        path: orchestrationStatus.configPath,
      })
    : t("proxy.orchestration.description", {
        defaultValue: "Multi-model strategy routing and quality checks",
      })}
</p>
```

- [ ] **Step 4: Add MSW command handlers**

In `tests/msw/handlers.ts`, add module state near the other local state:

```ts
let orchestrationStatus = {
  enabled: false,
  configPath: "/mock/omniagent/strategies.yaml",
};
```

Add handlers before the proxy status handler:

```ts
http.post(`${TAURI_ENDPOINT}/get_orchestration_status`, () =>
  success(orchestrationStatus),
),

http.post(`${TAURI_ENDPOINT}/set_orchestration_enabled`, async ({ request }) => {
  const { enabled } = await withJson<{ enabled: boolean }>(request);
  orchestrationStatus = {
    ...orchestrationStatus,
    enabled,
  };
  return success(orchestrationStatus);
}),
```

- [ ] **Step 5: Add component test**

Create `tests/components/ProxyPanel.orchestration.test.tsx`:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import "@/tests/msw/tauriMocks";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { server } from "@/tests/msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <ProxyPanel
        enableLocalProxy
        onEnableLocalProxyChange={vi.fn()}
        onToggleProxy={vi.fn()}
        isProxyPending={false}
      />
    </QueryClientProvider>,
  );
}

describe("ProxyPanel orchestration switch", () => {
  it("loads orchestration status and toggles it through Tauri commands", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
        HttpResponse.json({
          running: true,
          address: "127.0.0.1",
          port: 15721,
          active_connections: 0,
          total_requests: 0,
          success_requests: 0,
          failed_requests: 0,
          success_rate: 0,
          uptime_seconds: 1,
          current_provider: null,
          current_provider_id: null,
          last_request_at: null,
          last_error: null,
          failover_count: 0,
          active_targets: [],
        }),
      ),
    );
    renderPanel();

    const switchElement = await screen.findByRole("switch", {
      name: /orchestration/i,
    });

    expect(switchElement).not.toBeChecked();
    await user.click(switchElement);

    await waitFor(() => {
      expect(switchElement).toBeChecked();
    });
    expect(await screen.findByText(/strategies\.yaml/)).toBeInTheDocument();
  });
});
```

The `aria-label` is required. Do not rely on nearby text for the accessible name because the current source contains localized text and some terminals render it as mojibake.

- [ ] **Step 6: Run frontend test and commit**

Run:

```bash
pnpm vitest tests/components/ProxyPanel.orchestration.test.tsx --run
```

Expected: PASS.

Commit:

```bash
git -c safe.directory=D:/14-OneAgentSwithc add src/types/proxy.ts src/lib/api/proxy.ts src/lib/query/proxy.ts src/components/proxy/ProxyPanel.tsx tests/msw/handlers.ts tests/components/ProxyPanel.orchestration.test.tsx
git -c safe.directory=D:/14-OneAgentSwithc commit -m "feat: wire orchestration proxy panel toggle"
```

## Task 10: Final Verification

**Files:**
- No code changes expected unless a verification command exposes a real failure.

- [ ] **Step 1: Run formatting checks**

Run:

```bash
cd src-tauri
cargo fmt --check
cd ..
pnpm format:check
```

Expected: PASS. If either fails, run the matching formatter (`cargo fmt` or `pnpm format`), inspect the diff, and commit only formatting changes for files touched by this plan.

- [ ] **Step 2: Run Rust orchestration tests**

Run:

```bash
cd src-tauri
cargo test orchestration --lib
```

Expected: PASS.

- [ ] **Step 3: Run affected command and proxy tests**

Run:

```bash
cd src-tauri
cargo test proxy_commands --test proxy_commands
cargo test proxy::handlers --lib
```

Expected: PASS.

- [ ] **Step 4: Run frontend affected tests**

Run:

```bash
pnpm vitest tests/components/ProxyPanel.orchestration.test.tsx tests/hooks/useProxyStatus.test.tsx --run
```

Expected: PASS.

- [ ] **Step 5: Run type checks**

Run:

```bash
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 6: Commit any verification fixes**

If the verification steps required code changes, commit only those files:

```bash
git -c safe.directory=D:/14-OneAgentSwithc status --short
git -c safe.directory=D:/14-OneAgentSwithc add <changed-files>
git -c safe.directory=D:/14-OneAgentSwithc commit -m "fix: stabilize orchestration mvp tests"
```

Expected: no commit is needed if all prior tasks passed cleanly.

## Manual QA

- [ ] Start the app with `pnpm tauri dev`.
- [ ] Open Settings -> Proxy.
- [ ] Start the proxy service.
- [ ] Toggle Orchestration on.
- [ ] Confirm the switch remains on after closing and reopening the panel.
- [ ] Confirm the YAML file shown in the panel exists on disk.
- [ ] Send a streaming Claude request and confirm it still streams through passthrough.
- [ ] Send a non-streaming coding request with `configs/strategies.yaml` enabled and valid API env vars. Confirm CASCADE returns one complete JSON response with an `omniagent` metadata object.
- [ ] Disable Orchestration and confirm the next request uses the original passthrough path.

## Self-Review

Manual review correction: this section reflects the intended post-implementation state. It is not a statement that the current worktree already satisfies these bullets.

### Spec Coverage

- Provider management and existing proxy passthrough remain untouched except for a CASCADE preflight branch.
- Strategy YAML is still the source of truth.
- ROUTE preserves streaming UX by leaving streaming and direct-route requests on the existing passthrough path.
- CASCADE gets deterministic quality checking and model escalation.
- UI switch becomes functional and writes the same YAML file the backend reads.
- Advanced v2 features are excluded because they are independent follow-up systems.

### Placeholder Scan

The plan contains exact file paths, concrete code blocks, commands, and expected results. It contains no `TBD`, no `TODO`, and no "fill in details" steps.

### Type Consistency

Manual review correction: the bullets below describe the intended target state after the plan is rebased, not the current worktree state.

- `OrchestrationStatus` should use `enabled` and `configPath` on the frontend, matching Rust `#[serde(rename_all = "camelCase")]`.
- `ExecutionResult` fields used by response adapters should match the existing executor struct.
- `StrategyExecutor::with_backend` and `ModelBackend` still need to be introduced or the tests must be rewritten around the existing executor shape.
- `QualityGate::verify` must be wired into the executor before `judge_score` can be treated as the real quality signal.

## Execution Handoff

Plan saved to `docs/superpowers/plans/2026-06-01-orchestration-mvp.md`, but manual review found P1 drift from the current worktree. Do not execute the task list blindly. First rebase the plan as described in `Manual Review Gate`, then choose one execution option:

1. Subagent-Driven (recommended) - dispatch a fresh subagent per task, review between tasks, fast iteration.
2. Inline Execution - execute tasks in this session using executing-plans, batch execution with checkpoints.

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | Manual fallback | Scope & strategy | 1 | Qualified | Long-term goal is feasible only on bounded, verifiable workflows; universal replacement of frontier models is explicitly out of scope. |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | Not run | No external model review was available in this session. |
| Eng Review | Manual fallback for `/plan-eng-review` | Architecture & tests | 1 | **Blocked** | Plan/code drift found: existing files would be overwritten, command names diverge, toggle does not persist YAML, loader path helper is referenced but absent, config validation is missing, executor bypasses `QualityGate`, `ModelCaller` provider handling is incomplete, and Responses `input` is not normalized. |
| Design Review | Manual fallback | UI/UX gaps | 1 | Issues open | ProxyPanel switch is still hard-coded to `checked={false}` and only shows a toast; it needs real status/mutation hooks and accessible test coverage. |
| DX Review | Manual fallback | Developer experience gaps | 1 | Issues open | Windows verification needs `pnpm.cmd`; Rust verification is blocked until MSVC `link.exe` is installed. The handoff must tell weaker models not to claim cargo success from this machine yet. |

- **UNRESOLVED:** 8 P1 blockers listed in `Manual Review Gate`.
- **VERDICT:** NOT CLEARED for blind weak-model execution. First rebase the plan against the current worktree, then execute task-by-task.
