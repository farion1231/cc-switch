# DSH Full Parity Implementation Plan

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将 DeepSeek Harness 应用面补齐到 Pi/OpenCode 水平：隐藏不支持的 MCP/Skills 入口、修复测速与 live-remove 缺陷、提供 live 状态查询与默认模型命令、新增 zstd 会话历史浏览、补齐目录覆盖/i18n/预设/提示词自动导入。

**架构：** 后端全部复用现有 per-app 模式（Pi 为模板）：新 `commands/deepseek_harness.rs` + `services/dsh_state.rs` + `session_manager/providers/deepseek_harness.rs`；前端新 `api/dsh.ts` + `query/dsh.ts`，其余为对既有文件的定点修改。规格：`docs/superpowers/specs/2026-09-11-dsh-full-parity-design.md`。

**技术栈：** Rust (Tauri)、serde_yaml/serde_json、zstd 0.13（已在依赖中）、React + TanStack Query、Vitest、serial_test + tempfile（测试）。

---

## 文件结构

**新建：**
- `src-tauri/src/commands/deepseek_harness.rs` — DSH 专属 Tauri 命令
- `src-tauri/src/services/dsh_state.rs` — 只读 live 状态服务（仿 `services/pi_state.rs`）
- `src-tauri/src/session_manager/providers/deepseek_harness.rs` — zstd JSONL 会话扫描/加载/删除
- `src/lib/api/dsh.ts` — 前端 invoke 封装（仿 `api/pi.ts`）
- `src/lib/query/dsh.ts` — TanStack Query hooks（仿 `query/pi.ts`）

**修改：**
- `src-tauri/src/services/stream_check.rs:175-197` — `resolve_base_url` 加 DSH 分支
- `src-tauri/src/services/provider/mod.rs:5083-5085` — remove_from_live_config 改走 source-aware
- `src-tauri/src/services/provider/deepseek_harness.rs:202-214` — `remove_native_provider` 提为 `pub(super)` 并新增 `remove_from_live`
- `src-tauri/src/commands/mod.rs` — 注册新模块
- `src-tauri/src/lib.rs`（handler 列表）— 注册新命令
- `src-tauri/src/session_manager/providers/mod.rs` 与 `session_manager/mod.rs:7,58-97,109-119,166-182,202-213` — 接线
- `src-tauri/src/settings.rs` — `dsh_config_dir` 字段 + `get_dsh_override_dir`
- `src-tauri/src/deepseek_harness_config.rs:82-87` — `get_dsh_home` 支持覆盖
- `src-tauri/src/app_config.rs` — PromptRoot 增加 dsh 字段 + 自动导入循环纳入 DSH
- `src/App.tsx`（约 230-247、311-321、1160-1166）— 隐藏 MCP/Skills、会话支持、onSetAsDefault
- `src/components/providers/ProviderList.tsx`（约 122-135、143-150、205-225）— dsh live 状态
- `src/components/sessions/SessionManagerPage.tsx:85-94` — ProviderFilter 加 dsh
- `src/hooks/useDirectorySettings.ts`、`src/components/settings/DirectorySettings.tsx`、`src/types.ts:426`、`src/hooks/useSettingsForm.ts:128,196` — dsh 目录覆盖
- `src/i18n/locales/en.json`、`zh.json` — `dsh.*` 节
- `src/config/deepseekHarnessProviderPresets.ts` — 新增通用预设

**测试：**
- Rust 测试内联于各新模块（`#[cfg(test)]`，`serial_test` + `tempfile`，仿 `deepseek_harness_config.rs:607+` 的 `with_temp_home`）
- 前端测试：`src/lib/query/__tests__/dsh.test.tsx`（新）、`src/components/sessions/__tests__/`（若已有同目录测试则跟随）

---

## 任务 1：隐藏 DSH 的 MCP / Skills 悬空入口

**文件：**
- 修改：`src/App.tsx`（视图回退 useEffect 约 229-247 行；`hasSkillsSupport`/`hasMcpSupport` 约 311-321 行）
- 测试：复用现有前端套件（该文件无单测，靠 typecheck + 全量 Vitest 防回归）

- [ ] **步骤 1：修改视图回退**

`src/App.tsx` 的回退 useEffect 当前为：

```ts
    if (currentView === "mcp" && sharedFeatureApp === "pi") {
      setCurrentView("providers");
      return;
    }
```

改为：

```ts
    if (
      currentView === "mcp" &&
      (sharedFeatureApp === "pi" || sharedFeatureApp === "deepseek-harness")
    ) {
      setCurrentView("providers");
      return;
    }
    if (
      currentView === "skills" &&
      sharedFeatureApp === "deepseek-harness"
    ) {
      setCurrentView("providers");
      return;
    }
```

- [ ] **步骤 2：修改能力开关**

```ts
  const hasSkillsSupport =
    sharedFeatureApp !== "openclaw" && sharedFeatureApp !== "deepseek-harness";
```

```ts
  const hasMcpSupport =
    sharedFeatureApp !== "pi" && sharedFeatureApp !== "deepseek-harness";
```

- [ ] **步骤 3：验证**

运行：`pnpm exec tsc --noEmit && pnpm vitest run src/App 2>/dev/null; pnpm vitest run --reporter=dot 2>&1 | tail -5`
预期：typecheck 通过；全量 Vitest 无新增失败（基线：132 文件 / 978 测试通过）。

- [ ] **步骤 4：Commit**

```bash
git add src/App.tsx
git commit -m "fix(dsh): hide unsupported MCP and Skills entries"
```

---

## 任务 2：测速后端补 DSH 分支

**文件：**
- 修改：`src-tauri/src/services/stream_check.rs:175-197`
- 测试：同文件 `#[cfg(test)]` 模块（若无则在文件末尾新建）

- [ ] **步骤 1：编写失败的测试**

在 `stream_check.rs` 测试模块中加：

```rust
    #[test]
    fn resolves_deepseek_harness_base_url_from_settings_config() {
        let provider = crate::provider::Provider {
            id: "k3".to_string(),
            name: "K3".to_string(),
            settings_config: serde_json::json!({
                "baseURL": "https://api.example.com/v1"
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
        };
        let url = StreamCheckService::resolve_base_url(&AppType::DeepSeekHarness, &provider)
            .expect("dsh base url");
        assert_eq!(url, "https://api.example.com/v1");
    }

    #[test]
    fn rejects_deepseek_harness_without_base_url() {
        let provider = crate::provider::Provider {
            id: "k3".to_string(),
            name: "K3".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
        };
        assert!(
            StreamCheckService::resolve_base_url(&AppType::DeepSeekHarness, &provider).is_err()
        );
    }
```

注：若 `Provider` 字段与上面不完全一致，按 `src-tauri/src/provider.rs` 中实际结构体调整（先 `grep -n "pub struct Provider" -A 20 src-tauri/src/provider.rs` 确认字段）。

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test --manifest-path src-tauri/Cargo.toml stream_check --lib`
预期：FAIL，`resolve_base_url` 对 DSH 走 `_` 分支报 "does not support proxy adapters"。

- [ ] **步骤 3：实现**

`resolve_base_url` 的 match 中、`AppType::Pi` 分支后加：

```rust
            AppType::DeepSeekHarness => {
                let base_url = provider
                    .settings_config
                    .get("baseURL")
                    .or_else(|| provider.settings_config.get("baseUrl"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        AppError::Message(
                            "DeepSeek Harness provider has no baseURL".to_string(),
                        )
                    })?;
                Ok(base_url.to_string())
            }
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test --manifest-path src-tauri/Cargo.toml stream_check --lib`
预期：PASS。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/stream_check.rs
git commit -m "fix(dsh): resolve provider baseURL for reachability checks"
```

---

## 任务 3：source-aware 的 remove_from_live_config

**文件：**
- 修改：`src-tauri/src/services/provider/deepseek_harness.rs`（`remove_native_provider` 提权 + 新 `remove_from_live`）
- 修改：`src-tauri/src/services/provider/mod.rs`（DSH 分支，当前 `:5083-5085`）
- 测试：`src-tauri/src/services/provider/deepseek_harness.rs` 的 `#[cfg(test)]`

- [ ] **步骤 1：编写失败的测试**

在 `services/provider/deepseek_harness.rs` 测试模块中加（复用该文件已有的 temp-home 模式）：

```rust
    #[test]
    #[serial]
    fn remove_from_live_only_clears_the_targeted_route() {
        crate::deepseek_harness_config::test_support::with_temp_home_if_available(|_| {});
        // 若 deepseek_harness_config 未导出 test helper，则用与其 tests 相同的
        // DSH_HOME env + tempfile 模式内联实现。
    }
```

实际测试（直接在 deepseek_harness.rs tests 内写，环境准备代码参照 `deepseek_harness_config.rs:613-624` 的 `with_temp_home`）：

```rust
    #[test]
    #[serial]
    fn remove_from_live_only_clears_the_targeted_route() {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::fs::write(
                directory.path().join("settings.yaml"),
                "llm-deepseek:\n  baseURL: \"https://api.deepseek.com\"\n  apiKeyEnv: DEEPSEEK_API_KEY\nllm-pi-ai:\n  providers:\n    k3:\n      baseURL: \"https://k3.example.com\"\n",
            )
            .unwrap();
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));
            // DB 中的镜像行带 meta.providerType = dsh_pi_ai
            let mut provider = crate::provider::Provider::new(
                "k3".to_string(),
                "K3".to_string(),
                serde_json::json!({"baseURL": "https://k3.example.com"}),
            );
            let mut meta = crate::provider::ProviderMeta::default();
            meta.provider_type = Some("dsh_pi_ai".to_string());
            provider.meta = Some(meta);
            state
                .db
                .save_provider("deepseek-harness", &provider)
                .unwrap();

            remove_from_live(&state, "k3").unwrap();

            let settings =
                std::fs::read_to_string(directory.path().join("settings.yaml")).unwrap();
            assert!(settings.contains("llm-deepseek:"), "official route preserved");
            assert!(!settings.contains("k3:"), "pi-ai route removed");
        }));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }
```

注：`Provider::new` / `ProviderMeta::default()` 的确切构造签名先查 `src-tauri/src/provider.rs`；若不同按实际签名调整。

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib`
预期：编译错误 `remove_from_live` 不存在。

- [ ] **步骤 3：实现**

`services/provider/deepseek_harness.rs` 中：

```rust
pub(super) fn remove_from_live(state: &AppState, id: &str) -> Result<(), AppError> {
    let provider = state
        .db
        .get_provider_by_id(id, AppType::DeepSeekHarness.as_str())?
        .ok_or_else(|| AppError::NotFound(format!("Provider {id} not found")))?;
    remove_native_provider(&provider)
}
```

并把 `fn remove_native_provider` 改为 `pub(super) fn remove_native_provider`。

`services/provider/mod.rs` 的 DSH 分支改为：

```rust
            AppType::DeepSeekHarness => {
                deepseek_harness::remove_from_live(state, id)?;
            }
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib`
预期：全部 PASS（基线 14 + 新增 1）。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/provider/deepseek_harness.rs src-tauri/src/services/provider/mod.rs
git commit -m "fix(dsh): remove only the targeted native route from live config"
```

---

## 任务 4：get_dsh_current_state 命令（后端）

**文件：**
- 创建：`src-tauri/src/services/dsh_state.rs`
- 创建：`src-tauri/src/commands/deepseek_harness.rs`
- 修改：`src-tauri/src/services/mod.rs`（加 `pub(crate) mod dsh_state;`，先 grep 确认该文件现有 mod 列表风格）
- 修改：`src-tauri/src/commands/mod.rs`（`mod deepseek_harness;` + `pub(crate) use deepseek_harness::*;`，仿第 20 行 pi 的写法）
- 修改：`src-tauri/src/lib.rs`（handler 列表 `import_deepseek_harness_providers_from_live` 附近）

- [ ] **步骤 1：编写失败的测试**

`src-tauri/src/services/dsh_state.rs` 全文（含测试）：

```rust
//! Read-only DeepSeek Harness native state for advisory UI.

use crate::error::AppError;
use crate::store::AppState;
use serde::Serialize;

const DSH_APP: &str = "deepseek-harness";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DshCurrentState {
    pub provider_ids: Vec<String>,
    pub current_provider_id: Option<String>,
    pub current_model: Option<String>,
}

pub(crate) struct DshStateService;

impl DshStateService {
    pub(crate) fn current(state: &AppState) -> Result<DshCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(DSH_APP));
        let native = crate::deepseek_harness_config::read_native_state()?;
        Ok(DshCurrentState {
            provider_ids: native.providers.keys().cloned().collect(),
            current_provider_id: native.current_provider,
            current_model: native.current_model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::Arc;

    fn with_temp_home(test: impl FnOnce(&std::path::Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    #[test]
    #[serial]
    fn exposes_native_providers_and_current_selection() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-deepseek:\n  baseURL: \"https://api.deepseek.com\"\n  apiKeyEnv: DEEPSEEK_API_KEY\nllm-pi-ai:\n  providers:\n    k3:\n      baseURL: \"https://k3.example.com\"\nagent-default-model:\n  provider: k3\n  model: k3\n",
            )
            .unwrap();
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));
            let current = DshStateService::current(&state).expect("state");
            assert!(current.provider_ids.contains(&"deepseek-official".to_string()));
            assert!(current.provider_ids.contains(&"k3".to_string()));
            assert_eq!(current.current_provider_id.as_deref(), Some("k3"));
            assert_eq!(current.current_model.as_deref(), Some("k3"));
        });
    }

    #[test]
    #[serial]
    fn empty_settings_yield_empty_state() {
        with_temp_home(|_home| {
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));
            let current = DshStateService::current(&state).expect("state");
            assert!(current.provider_ids.is_empty());
            assert_eq!(current.current_provider_id, None);
        });
    }
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test --manifest-path src-tauri/Cargo.toml dsh_state --lib`
预期：编译错误（模块未注册）。

- [ ] **步骤 3：注册模块并实现命令**

`src-tauri/src/commands/deepseek_harness.rs` 全文：

```rust
use crate::services::dsh_state::{DshCurrentState, DshStateService};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_dsh_current_state(state: State<'_, AppState>) -> Result<DshCurrentState, String> {
    DshStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_dsh_current_model(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] providerId: String,
    #[allow(non_snake_case)] modelId: String,
) -> Result<bool, String> {
    crate::services::provider::deepseek_harness::set_current_model(
        state.inner(),
        &providerId,
        &modelId,
    )
    .map_err(|error| error.to_string())
}
```

`commands/mod.rs`：在 `mod pi;`（第 20 行附近）后加 `mod deepseek_harness;`（保持字母序，放在 `mod deeplink;` 之后），并在 `pub(crate) use pi::*;` 附近加 `pub(crate) use deepseek_harness::*;`。

`lib.rs` handler 列表中 `commands::import_deepseek_harness_providers_from_live,` 之后加：

```rust
            commands::get_dsh_current_state,
            commands::set_dsh_current_model,
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test --manifest-path src-tauri/Cargo.toml dsh_state --lib && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -3`
预期：测试 PASS；编译通过（`set_current_model` 在任务 5 实现前会报未定义——因此本任务先注释掉 `set_dsh_current_model` 命令体中的调用、直接 `Ok(true)` 占位是**禁止**的；正确顺序：本任务只加 `get_dsh_current_state`，`set_dsh_current_model` 整个命令放到任务 5 一起加）。

修订后的命令文件（任务 4 版本）只含 `get_dsh_current_state`；任务 5 再追加 `set_dsh_current_model`。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/dsh_state.rs src-tauri/src/services/mod.rs src-tauri/src/commands/deepseek_harness.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(dsh): expose native current state to the UI"
```

---

## 任务 5：set_dsh_current_model 命令（后端写路径）

**文件：**
- 修改：`src-tauri/src/services/provider/deepseek_harness.rs` — 新增 `pub(super) fn set_current_model`
- 修改：`src-tauri/src/commands/deepseek_harness.rs` — 追加命令（代码见任务 4 步骤 3 的完整版本）
- 修改：`src-tauri/src/lib.rs` — handler 加 `commands::set_dsh_current_model,`
- 测试：`services/provider/deepseek_harness.rs` 的 `#[cfg(test)]`

- [ ] **步骤 1：编写失败的测试**

```rust
    #[test]
    #[serial]
    fn set_current_model_validates_membership_and_updates_state() {
        // temp-home 模式同任务 3 测试
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::fs::write(
                directory.path().join("settings.yaml"),
                "llm-pi-ai:\n  providers:\n    k3:\n      baseURL: \"https://k3.example.com\"\n      models:\n        - id: k3\n        - id: k3-turbo\n",
            )
            .unwrap();
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));

            set_current_model(&state, "k3", "k3-turbo").unwrap();
            let native = crate::deepseek_harness_config::read_native_state().unwrap();
            assert_eq!(native.current_provider.as_deref(), Some("k3"));
            assert_eq!(native.current_model.as_deref(), Some("k3-turbo"));

            assert!(set_current_model(&state, "k3", "no-such-model").is_err());
            assert!(set_current_model(&state, "ghost", "k3").is_err());
        }));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }
```

- [ ] **步骤 2：运行验证失败** — `cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib`，预期编译错误。

- [ ] **步骤 3：实现**

`services/provider/deepseek_harness.rs` 加：

```rust
pub(super) fn set_current_model(
    state: &AppState,
    provider_id: &str,
    model_id: &str,
) -> Result<(), AppError> {
    let native = crate::deepseek_harness_config::read_native_state()?;
    let provider = native.providers.get(provider_id).ok_or_else(|| {
        AppError::NotFound(format!("DSH provider {provider_id} not found in native config"))
    })?;
    let models = provider.config.get("models").and_then(|value| value.as_array());
    if let Some(models) = models {
        let known = models.iter().any(|model| {
            model.get("id").and_then(|id| id.as_str()) == Some(model_id)
                || model.as_str() == Some(model_id)
        });
        if !known {
            return Err(AppError::InvalidInput(format!(
                "Model {model_id} is not configured for DSH provider {provider_id}"
            )));
        }
    }
    crate::deepseek_harness_config::set_current_model(provider_id, model_id)?;
    // 刷新 DB 镜像的 current 标记与 dsh_current_model
    let _ = sync_native_locked(state);
    Ok(())
}
```

注：`sync_native_locked` 是该文件已有的同步函数名（`:221` 附近，先 grep 确认实际名称）；若其为私有 fn 直接调用即可（同模块）。`AppError::NotFound` 是否存在先 `grep -n "NotFound" src-tauri/src/error.rs` 确认，没有则用 `AppError::Message`。

- [ ] **步骤 4：运行验证通过** — 同步骤 2 命令，预期 PASS。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/provider/deepseek_harness.rs src-tauri/src/commands/deepseek_harness.rs src-tauri/src/lib.rs
git commit -m "feat(dsh): set native default model without switching provider"
```

---

## 任务 6：前端 dsh api/query 模块 + ProviderList live 状态

**文件：**
- 创建：`src/lib/api/dsh.ts`
- 创建：`src/lib/query/dsh.ts`
- 创建：`src/lib/query/__tests__/dsh.test.tsx`（先 `ls src/lib/query/__tests__/` 确认目录与既有测试风格，仿 `pi` 对应测试；若无 pi 测试则仿任一 query 测试）
- 修改：`src/components/providers/ProviderList.tsx`（约 122-135、205-225 行）
- 修改：`src/App.tsx`（`onSetAsDefault` 约 1160-1166 行 + dsh 缓存失效）

- [ ] **步骤 1：编写失败的测试**

`src/lib/query/__tests__/dsh.test.tsx`：

```tsx
import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useDshCurrentState, dshKeys } from "@/lib/query/dsh";

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("useDshCurrentState", () => {
  beforeEach(() => invokeMock.mockReset());

  it("maps the native state payload", async () => {
    invokeMock.mockResolvedValue({
      providerIds: ["deepseek-official", "k3"],
      currentProviderId: "k3",
      currentModel: "k3",
    });
    const { result } = renderHook(() => useDshCurrentState(true), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invokeMock).toHaveBeenCalledWith("get_dsh_current_state");
    expect(result.current.data?.providerIds).toContain("k3");
    expect(result.current.data?.currentProviderId).toBe("k3");
  });

  it("is disabled without the flag", () => {
    const { result } = renderHook(() => useDshCurrentState(false), { wrapper });
    expect(result.current.fetchStatus).toBe("idle");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **步骤 2：运行验证失败**

运行：`pnpm vitest run src/lib/query/__tests__/dsh.test.tsx`
预期：模块不存在报错。

- [ ] **步骤 3：实现 api/query 模块**

`src/lib/api/dsh.ts`：

```ts
import { invoke } from "@tauri-apps/api/core";

export interface DshCurrentState {
  providerIds: string[];
  currentProviderId: string | null;
  currentModel: string | null;
}

export const dshApi = {
  async getCurrentState(): Promise<DshCurrentState> {
    return await invoke("get_dsh_current_state");
  },

  async setCurrentModel(
    providerId: string,
    modelId: string,
  ): Promise<boolean> {
    return await invoke("set_dsh_current_model", { providerId, modelId });
  },
};
```

`src/lib/query/dsh.ts`：

```ts
import { useQuery, type QueryClient } from "@tanstack/react-query";
import { dshApi } from "@/lib/api/dsh";

export const dshKeys = {
  all: ["dsh"] as const,
  currentState: ["dsh", "currentState"] as const,
};

export const invalidateDshProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: dshKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "deepseek-harness"] }),
  ]);
};

export function useDshCurrentState(enabled = true) {
  return useQuery({
    queryKey: dshKeys.currentState,
    queryFn: () => dshApi.getCurrentState(),
    enabled,
  });
}
```

- [ ] **步骤 4：ProviderList 接线**

`ProviderList.tsx` 顶部 import 加 `import { useDshCurrentState } from "@/lib/query/dsh";`，在 `useHermesModelConfig` 调用后加：

```ts
  const { data: dshCurrentState } = useDshCurrentState(
    appId === "deepseek-harness",
  );
```

`isProviderInConfig` 中 `return true;` 之前加：

```ts
      if (appId === "deepseek-harness") {
        return dshCurrentState?.providerIds.includes(providerId) ?? false;
      }
```

并把 `dshCurrentState` 加入该 `useCallback` 依赖数组。

- [ ] **步骤 5：App.tsx 接 onSetAsDefault 与缓存失效**

`App.tsx` 中 `onSetAsDefault` 处改为：

```ts
                      onSetAsDefault={
                        activeApp === "openclaw"
                          ? setAsDefaultModel
                          : activeApp === "hermes"
                            ? switchProvider
                            : activeApp === "deepseek-harness"
                              ? handleSetDshDefaultModel
                              : undefined
                      }
```

并在 `handleEnablePiProvider` 附近新增：

```ts
  const handleSetDshDefaultModel = async (provider: Provider, modelId?: string) => {
    const config = provider.settingsConfig as {
      models?: Array<{ id?: string } | string>;
    };
    const firstModel = config.models?.[0];
    const resolved =
      modelId ??
      (typeof firstModel === "string" ? firstModel : firstModel?.id);
    if (!resolved) {
      toast.error(
        t("dsh.provider.noModels", {
          defaultValue: "该供应商没有配置模型",
        }),
      );
      return;
    }
    try {
      await dshApi.setCurrentModel(provider.id, resolved);
      await invalidateDshProviderCaches(queryClient);
      toast.success(
        t("dsh.provider.defaultSet", { defaultValue: "已设为 DSH 默认模型" }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("dsh.provider.defaultFailed", {
          defaultValue: "设置 DSH 默认模型失败",
        }),
        { description: extractErrorMessage(error), closeButton: true },
      );
    }
  };
```

import 加 `import { dshApi } from "@/lib/api/dsh";` 与 `import { invalidateDshProviderCaches } from "@/lib/query/dsh";`。

另外 remove 流程的失效逻辑（约 731-760 行 pi 分支后）加：

```ts
      if (activeApp === "deepseek-harness") {
        await invalidateDshProviderCaches(queryClient);
      }
```

- [ ] **步骤 6：运行验证通过**

运行：`pnpm vitest run src/lib/query/__tests__/dsh.test.tsx && pnpm exec tsc --noEmit`
预期：PASS + typecheck 通过。

- [ ] **步骤 7：Commit**

```bash
git add src/lib/api/dsh.ts src/lib/query/dsh.ts src/lib/query/__tests__/dsh.test.tsx src/components/providers/ProviderList.tsx src/App.tsx
git commit -m "feat(dsh): wire live state and default-model actions into the provider page"
```

---

## 任务 7：DSH 会话提供者（后端核心）

**文件：**
- 创建：`src-tauri/src/session_manager/providers/deepseek_harness.rs`
- 测试：同文件 `#[cfg(test)]`

DSH 会话格式（实测自 DSH 2.0.5）：
- 路径：`$DSH_HOME/sessions/<编码后的cwd>/<session-id>/session.jsonl.zstd`
- zstd 压缩的 JSONL，每行一个事件：`{"type": "...", "seq"/"seq0": n, "time"/"time0": ms, "data": {...}}`
- 首行 header：`{"type":"session","version":0,"id":"session-...","createdAt":<ms>,"cwd":"/abs/path",...}`
- 标题：`{"type":"session/title","data":{"title":"..."}}`
- 用户消息：`{"type":"user/message","time":<ms>,"data":{"role":"user","content":[{"type":"text","text":"..."}]}}`
- 助手消息：`{"type":"assistant/message","time":<ms>,"data":{"message":{"role":"assistant","content":[{"type":"reasoning"|"text","text":"..."}]}}}`
- `text-chunks`/`reasoning-chunks` 是流式增量，忽略（`assistant/message` 有完整内容）

- [ ] **步骤 1：编写失败的测试**

新文件全文（含实现与测试，先写测试再取消实现注释也行；按 TDD 先只放测试 + 空函数签名使编译失败可跳过——直接按下面完整文件实现，先运行测试确认 RED 的方式是：先写测试和 `todo!()` 实现）：

实现文件 `src-tauri/src/session_manager/providers/deepseek_harness.rs`：

```rust
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, path_basename, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "deepseek-harness";
const SESSION_FILE_NAME: &str = "session.jsonl.zstd";
/// Compressed size guard; decompressed content is bounded separately.
const MAX_SESSION_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 500_000;

pub fn session_roots() -> Vec<PathBuf> {
    vec![crate::deepseek_harness_config::get_dsh_home().join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = crate::deepseek_harness_config::get_dsh_home().join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for project in entries.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(session_dirs) = fs::read_dir(&project_path) else {
            continue;
        };
        for session_dir in session_dirs.flatten() {
            let file = session_dir.path().join(SESSION_FILE_NAME);
            if !file.is_file() {
                continue;
            }
            match parse_session(&file) {
                Ok(meta) => sessions.push(meta),
                Err(error) => {
                    log::debug!("Skipping invalid DSH session {}: {error}", file.display())
                }
            }
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let events = read_events(path)?;
    let mut messages = Vec::new();
    for value in &events {
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "user".to_string(),
                        content,
                        ts: event_time(value),
                    });
                }
            }
            Some("assistant/message") => {
                let content = value
                    .pointer("/data/message/content")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter(|item| {
                                item.get("type").and_then(Value::as_str) == Some("text")
                            })
                            .filter_map(|item| {
                                item.get("text").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "assistant".to_string(),
                        content,
                        ts: event_time(value),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if session_id.contains(['/', '\\']) || session_id == "." || session_id == ".." {
        return Err("Invalid DSH session ID".to_string());
    }
    if path.file_name().and_then(|n| n.to_str()) != Some(SESSION_FILE_NAME) {
        return Err(format!(
            "DSH session source must be a {SESSION_FILE_NAME} file: {}",
            path.display()
        ));
    }
    let meta = parse_session(path)?;
    if meta.session_id != session_id {
        return Err(format!(
            "DSH session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }
    let dir = path
        .parent()
        .ok_or_else(|| "DSH session has no parent directory".to_string())?;
    if !dir.starts_with(root) {
        return Err("DSH session is outside the sessions root".to_string());
    }
    fs::remove_dir_all(dir)
        .map_err(|error| format!("Failed to delete DSH session {}: {error}", dir.display()))?;
    Ok(true)
}

fn event_time(value: &Value) -> Option<i64> {
    value
        .get("time")
        .or_else(|| value.get("time0"))
        .and_then(Value::as_i64)
}

fn read_events(path: &Path) -> Result<Vec<Value>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to stat DSH session {}: {error}", path.display()))?;
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(format!(
            "DSH session exceeds the {MAX_SESSION_BYTES}-byte safety limit"
        ));
    }
    let file =
        File::open(path).map_err(|error| format!("Failed to open DSH session: {error}"))?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .map_err(|error| format!("Failed to decompress DSH session: {error}"))?;
    let reader = BufReader::new(decoder);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read DSH session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if events.len() >= MAX_EVENTS {
            return Err(format!("DSH session exceeds the {MAX_EVENTS}-event safety limit"));
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            events.push(value);
        }
    }
    Ok(events)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve DSH session {}: {error}", path.display()))?;
    let events = read_events(&source)?;
    let header = events
        .iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session"))
        .ok_or_else(|| "DSH session has no header".to_string())?;
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH session header has no id".to_string())?
        .to_string();
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = header.get("createdAt").and_then(Value::as_i64);

    let mut title = events
        .iter()
        .rev()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session/title"))
        .and_then(|value| value.pointer("/data/title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut first_user: Option<String> = None;
    let mut last_text: Option<String> = None;
    let mut last_active_at = created_at;
    for value in &events {
        if let Some(time) = event_time(value) {
            last_active_at = Some(last_active_at.map_or(time, |current| current.max(time)));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if first_user.is_none() && !content.trim().is_empty() {
                    first_user = Some(content.clone());
                }
                if !content.trim().is_empty() {
                    last_text = Some(content);
                }
            }
            Some("assistant/message") => {
                if let Some(items) =
                    value.pointer("/data/message/content").and_then(Value::as_array)
                {
                    let text = items
                        .iter()
                        .filter(|item| {
                            item.get("type").and_then(Value::as_str) == Some("text")
                        })
                        .filter_map(|item| {
                            item.get("text").and_then(Value::as_str).map(str::to_string)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.trim().is_empty() {
                        last_text = Some(text);
                    }
                }
            }
            _ => {}
        }
    }
    if title.is_none() {
        title = first_user
            .as_deref()
            .map(|message| truncate_summary(message, TITLE_MAX_CHARS))
            .filter(|message| !message.is_empty())
            .or_else(|| cwd.as_deref().and_then(path_basename));
    }
    let summary = last_text
        .as_deref()
        .map(|message| truncate_summary(message, 160))
        .filter(|message| !message.is_empty());
    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: id,
        title,
        summary,
        project_dir: cwd.filter(|value| !value.trim().is_empty()),
        created_at,
        last_active_at,
        source_path: source.to_str().map(str::to_string),
        resume_command: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn write_session(home: &Path, project: &str, id: &str, events: &[Value]) -> PathBuf {
        let dir = home.join("sessions").join(project).join(id);
        fs::create_dir_all(&dir).unwrap();
        let payload = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let compressed = zstd::stream::encode_all(payload.as_bytes(), 3).unwrap();
        let file = dir.join(SESSION_FILE_NAME);
        fs::write(&file, compressed).unwrap();
        file
    }

    fn fixture_events() -> Vec<Value> {
        vec![
            serde_json::json!({"type":"session","version":0,"id":"session-abc","createdAt":1787104521333_i64,"cwd":"/Users/test/project"}),
            serde_json::json!({"type":"user/message","seq":8,"time":1787220655606_i64,"data":{"role":"user","content":[{"type":"text","text":"hello dsh"}]}}),
            serde_json::json!({"type":"session/title","seq":11,"time":1787220655607_i64,"data":{"title":"Greeting"}}),
            serde_json::json!({"type":"text-chunks","seq0":69,"time0":1787220662295_i64,"data":{"turn":1,"step":1,"index":1,"texts":["stream"," delta"]}}),
            serde_json::json!({"type":"assistant/message","seq":352,"time":1787220668317_i64,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"reasoning","text":"thinking"},{"type":"text","text":"hi there"}]}}}),
        ]
    }

    fn with_temp_home(test: impl FnOnce(&Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    #[test]
    #[serial]
    fn scan_reads_zstd_sessions_with_titles() {
        with_temp_home(|home| {
            let file = write_session(home, "--Users-test-project--", "session-abc", &fixture_events());
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
            let meta = &sessions[0];
            assert_eq!(meta.provider_id, "deepseek-harness");
            assert_eq!(meta.session_id, "session-abc");
            assert_eq!(meta.title.as_deref(), Some("Greeting"));
            assert_eq!(meta.project_dir.as_deref(), Some("/Users/test/project"));
            assert_eq!(meta.created_at, Some(1787104521333));
            assert_eq!(meta.last_active_at, Some(1787220668317));
            assert_eq!(
                meta.source_path.as_deref(),
                file.canonicalize().unwrap().to_str()
            );
        });
    }

    #[test]
    #[serial]
    fn load_messages_skips_stream_deltas_and_reasoning() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let messages = load_messages(&file).unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].role, "user");
            assert_eq!(messages[0].content, "hello dsh");
            assert_eq!(messages[0].ts, Some(1787220655606));
            assert_eq!(messages[1].role, "assistant");
            assert_eq!(messages[1].content, "hi there");
        });
    }

    #[test]
    #[serial]
    fn corrupt_sessions_are_skipped_not_fatal() {
        with_temp_home(|home| {
            let dir = home.join("sessions").join("proj").join("session-bad");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(SESSION_FILE_NAME), b"not zstd at all").unwrap();
            write_session(home, "proj", "session-abc", &fixture_events());
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
        });
    }

    #[test]
    #[serial]
    fn delete_removes_the_session_directory() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let root = home.join("sessions");
            let deleted = delete_session(&root, &file, "session-abc").unwrap();
            assert!(deleted);
            assert!(!file.exists());
            assert!(!file.parent().unwrap().exists());
            assert!(delete_session(&root, &file, "wrong-id").is_err());
        });
    }
}
```

注：`Provider`/序列化字段若与 `SessionMeta` 实际结构不符，以 `session_manager/mod.rs:11-28` 为准（已核对：`provider_id, session_id, title, summary, project_dir, created_at, last_active_at, source_path, resume_command`，均 camelCase 序列化）。

- [ ] **步骤 2：运行验证失败**

运行：`cargo test --manifest-path src-tauri/Cargo.toml session_manager::providers::deepseek_harness --lib`
预期：编译错误（模块未在 `providers/mod.rs` 注册）。

- [ ] **步骤 3：注册模块**

`src-tauri/src/session_manager/providers/mod.rs` 在 `pub mod codex;` 后加：

```rust
pub mod deepseek_harness;
```

- [ ] **步骤 4：运行验证通过**

运行：同步骤 2 命令。
预期：4 个测试 PASS。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/session_manager/providers/deepseek_harness.rs src-tauri/src/session_manager/providers/mod.rs
git commit -m "feat(dsh): scan and parse zstd session history"
```

---

## 任务 8：session_manager 接线

**文件：**
- 修改：`src-tauri/src/session_manager/mod.rs:7,58-97,109-119,166-182,202-213`

- [ ] **步骤 1：实现**

第 7 行 use 加 `deepseek_harness`：

```rust
use providers::{claude, codex, deepseek_harness, gemini, grokbuild, hermes, openclaw, opencode, pi};
```

`scan_sessions`：tuple 扩为 9 元组，加 `let h9 = s.spawn(deepseek_harness::scan_sessions);` 与 `h9.join().unwrap_or_default()`，并 `sessions.extend(r9);`。

`load_messages` match 加：

```rust
        "deepseek-harness" => deepseek_harness::load_messages(path),
```

`delete_session_with_roots` match 加：

```rust
                "deepseek-harness" => {
                    deepseek_harness::delete_session(&validated_root, &validated_source, session_id)
                }
```

`provider_roots` match 加：

```rust
        "deepseek-harness" => deepseek_harness::session_roots(),
```

- [ ] **步骤 2：验证**

运行：`cargo test --manifest-path src-tauri/Cargo.toml session_manager --lib && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -3`
预期：PASS + 编译通过。

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/session_manager/mod.rs
git commit -m "feat(dsh): wire session provider into the session manager"
```

---

## 任务 9：会话前端接线

**文件：**
- 修改：`src/App.tsx`（`hasSessionSupport` 约 312-320 行 + sessions 回退 useEffect 约 234-247 行）
- 修改：`src/components/sessions/SessionManagerPage.tsx:85-94` 及筛选 UI（约 1160-1185 行的 SelectItem 列表）

- [ ] **步骤 1：App.tsx**

`hasSessionSupport` 末尾加一行：

```ts
    sharedFeatureApp === "pi" ||
    sharedFeatureApp === "deepseek-harness";
```

sessions 回退 useEffect 的排除列表加 `sharedFeatureApp !== "deepseek-harness" &&`。

- [ ] **步骤 2：SessionManagerPage**

`ProviderFilter` 联合类型加 `| "deepseek-harness"`；筛选 Select 中 `value="pi"` 的 SelectItem 后加：

```tsx
                            <SelectItem value="deepseek-harness">
                              <div className="flex items-center gap-2">
                                <ProviderIcon
                                  icon="deepseek"
                                  name="deepseek-harness"
                                  size={14}
                                />
                                <span>{t("apps.deepseek-harness")}</span>
                              </div>
                            </SelectItem>
```

注：`ProviderIcon` 的 dsh 图标名以 `src/config/appConfig.tsx:205-219` 的 `APP_ICON_MAP` 实际值为准；`t("apps.deepseek-harness")` key 已存在（`en.json:917`）。若页面其它位置还有 `providerId === "pi"` 式的 per-app 展示分支，逐一检查是否需补 dsh。

- [ ] **步骤 3：验证**

运行：`pnpm exec tsc --noEmit && pnpm vitest run src/components/sessions --reporter=dot 2>&1 | tail -5`
预期：PASS。

- [ ] **步骤 4：Commit**

```bash
git add src/App.tsx src/components/sessions/SessionManagerPage.tsx
git commit -m "feat(dsh): surface DSH sessions in the session manager"
```

---

## 任务 10：`~/.dsh` 目录覆盖设置

**文件：**
- 修改：`src-tauri/src/settings.rs`（`dsh_config_dir` 字段，仿 `:444` 的 `pi_config_dir`；默认 None 处 `:561`；sanitize 段 `:643-644`；新增 `get_dsh_override_dir` 仿 `:976-981`）
- 修改：`src-tauri/src/deepseek_harness_config.rs:82-87`（`get_dsh_home` 优先级）
- 修改：`src/types.ts:426` 附近、`src/hooks/useSettingsForm.ts:128,196`、`src/hooks/useDirectorySettings.ts`、`src/components/settings/DirectorySettings.tsx`
- 测试：`settings.rs`/`deepseek_harness_config.rs` 的 Rust 测试

- [ ] **步骤 1：编写失败的测试**

`deepseek_harness_config.rs` 测试模块加：

```rust
    #[test]
    #[serial]
    fn dsh_home_prefers_settings_override_then_env_then_default() {
        // settings 覆盖
        let override_dir = tempfile::tempdir().unwrap();
        crate::settings::update_settings(crate::settings::Settings {
            dsh_config_dir: Some(override_dir.path().to_string_lossy().to_string()),
            ..Default::default()
        })
        .unwrap();
        let env_dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", env_dir.path());
        let resolved = get_dsh_home();
        assert_eq!(resolved, override_dir.path().canonicalize().unwrap());
        // 清除覆盖后回落到 env
        crate::settings::update_settings(crate::settings::Settings {
            dsh_config_dir: None,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(get_dsh_home(), env_dir.path());
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
    }
```

注：`crate::settings::update_settings` 的确切名称先 `grep -n "pub fn update_settings\|pub fn save_settings" src-tauri/src/settings.rs` 确认；`Settings` 是否 `Default` 也先确认（`:554-561` 附近有手工 default，则可能有 `impl Default`）。覆盖路径用 `resolve_override_path` 处理 `~`，断言时比较 `resolve_override_path(raw)` 的返回值而非直接 canonicalize，以避免 macOS `/var` vs `/private/var` 差异。

- [ ] **步骤 2：运行验证失败**

运行：`cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib`
预期：编译错误（`dsh_config_dir` 字段不存在）。

- [ ] **步骤 3：后端实现**

`settings.rs`：
- `Settings` 结构体在 `pi_config_dir`（`:444`）后加：
  ```rust
    /// DeepSeek Harness 配置目录覆盖（~/.dsh）
    #[serde(rename = "dshConfigDir", default, skip_serializing_if = "Option::is_none")]
    pub dsh_config_dir: Option<String>,
  ```
  注：先确认该结构体是否已有全局 `#[serde(rename_all = "camelCase")]`——若有则不需要字段级 rename，与 `pi_config_dir` 保持完全一致的标注风格。
- 默认值处（`:561` 附近）加 `dsh_config_dir: None,`
- sanitize 段（`:643-644` 附近）仿 pi 加：
  ```rust
        self.dsh_config_dir = self
            .dsh_config_dir
            .take()
            .and_then(|value| sanitize_config_dir(value));
  ```
  （函数名以 pi 段实际调用的 sanitize helper 为准）
- `get_pi_override_dir` 后加：
  ```rust
pub fn get_dsh_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .dsh_config_dir
        .as_ref()
        .map(|path| resolve_override_path(path))
}
  ```

`deepseek_harness_config.rs` 的 `get_dsh_home` 改为：

```rust
pub fn get_dsh_home() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_dsh_override_dir() {
        return override_dir;
    }
    std::env::var_os("DSH_HOME")
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
}
```

- [ ] **步骤 4：前端实现**

`src/types.ts` 在 `piConfigDir?: string;`（`:426`）后加 `dshConfigDir?: string;`。

`src/hooks/useSettingsForm.ts` 两处（`:128`、`:196`）各加一行 `dshConfigDir: sanitizeDir(data.dshConfigDir),`（`serverData` 版本同理）。

`src/hooks/useDirectorySettings.ts`：
- `DirectoryAppId` 改为 `Exclude<AppId, "claude-desktop">`
- `AppDirectoryKey` 加 `| "dsh"`
- `ResolvedDirectories` 加 `dsh: string;`
- `APP_DIRECTORY_META` 加 `dsh: { key: "dsh", defaultFolder: ".dsh" },`（键名用 `"deepseek-harness"` 作为 DirectoryAppId 成员：
  ```ts
  "deepseek-harness": { key: "dsh", defaultFolder: ".dsh" },
  ```）
- `DIRECTORY_KEY_TO_SETTINGS_FIELD` 加 `dsh: "dshConfigDir",`
- 两个 `resolvedDirs` 初始对象与 `defaultsRef` 初始对象、load 的 Promise.all（加 `settingsApi.getConfigDir("deepseek-harness")` 与 `computeDefaultConfigDir("deepseek-harness")`）、`setResolvedDirs`、`resetAllDirectories` 全部补 `dsh` 字段

`src/components/settings/DirectorySettings.tsx`：
- 第 9 行 `Exclude<AppId, "claude-desktop" | "deepseek-harness">` 改为 `Exclude<AppId, "claude-desktop">`
- props 加 `dshDir?: string;` 与解构
- pi 行（`:178-185`）后加：
  ```tsx
          <DirectoryRow
            label={t("settings.dshConfigDir")}
            description={t("settings.dshConfigDirDescription")}
            value={dshDir}
            resolvedValue={resolvedDirs.dsh}
            onChange={(val) => onDirectoryChange("deepseek-harness", val)}
            onBrowse={() => onBrowseDirectory("deepseek-harness")}
            onReset={() => onResetDirectory("deepseek-harness")}
          />
  ```
  注：行组件的实际名称/props 以该文件 pi 行实际写法为准（上面是按 grep 结果推断，实现时照抄 pi 行再改字段）。

- [ ] **步骤 5：i18n keys**（en.json / zh.json 的 `settings` 节，仿 `piConfigDir` 现有 key）

en.json：
```json
    "dshConfigDir": "DSH config directory",
    "dshConfigDirDescription": "Location of DSH settings.yaml and .credentials.yaml (default ~/.dsh)",
```
zh.json：
```json
    "dshConfigDir": "DSH 配置目录",
    "dshConfigDirDescription": "DSH settings.yaml 与 .credentials.yaml 所在目录（默认 ~/.dsh）",
```

- [ ] **步骤 6：验证**

运行：`cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib && pnpm exec tsc --noEmit && pnpm vitest run src/hooks src/components/settings --reporter=dot 2>&1 | tail -5`
预期：全部 PASS。

- [ ] **步骤 7：Commit**

```bash
git add src-tauri/src/settings.rs src-tauri/src/deepseek_harness_config.rs src/types.ts src/hooks/useSettingsForm.ts src/hooks/useDirectorySettings.ts src/components/settings/DirectorySettings.tsx src/i18n/locales/en.json src/i18n/locales/zh.json
git commit -m "feat(dsh): allow overriding the ~/.dsh directory from settings"
```

---

## 任务 11：dsh i18n 节

**文件：**
- 修改：`src/i18n/locales/en.json`、`src/i18n/locales/zh.json`（在 `"pi": { ... }` 节后加 `"dsh"` 节）

- [ ] **步骤 1：en.json**

```json
  "dsh": {
    "provider": {
      "enabled": "Enabled in DSH",
      "enableFailed": "Could not enable this provider in DSH",
      "removed": "Removed from DSH",
      "noModels": "This provider has no configured models",
      "defaultSet": "Set as the DSH default model",
      "defaultFailed": "Could not set the DSH default model"
    }
  },
```

- [ ] **步骤 2：zh.json**

```json
  "dsh": {
    "provider": {
      "enabled": "已在 DSH 中启用",
      "enableFailed": "无法在 DSH 中启用此供应商",
      "removed": "已从 DSH 移除",
      "noModels": "该供应商没有配置模型",
      "defaultSet": "已设为 DSH 默认模型",
      "defaultFailed": "设置 DSH 默认模型失败"
    }
  },
```

- [ ] **步骤 3：验证 + Commit**

运行：`pnpm exec tsc --noEmit && node -e "JSON.parse(require('fs').readFileSync('src/i18n/locales/en.json'));JSON.parse(require('fs').readFileSync('src/i18n/locales/zh.json'))"`
预期：JSON 合法、typecheck 通过。

```bash
git add src/i18n/locales/en.json src/i18n/locales/zh.json
git commit -m "feat(dsh): add dedicated i18n strings"
```

---

## 任务 12：预设扩展

**文件：**
- 修改：`src/config/deepseekHarnessProviderPresets.ts:44-55`

- [ ] **步骤 1：实现**

`deepseekHarnessProviderPresets` 数组中官方预设后加：

```ts
  {
    id: "dsh-openai-compatible",
    name: "OpenAI Compatible",
    websiteUrl: "https://platform.openai.com",
    settingsConfig: {
      apiKey: "",
      baseURL: "https://api.openai.com/v1",
      profile: "desktop",
      models: [],
    },
    category: "custom",
    icon: "openai",
  },
```

注：`category` 合法值与 `icon` 合法值先对照其它预设文件（`claudeProviderPresets.ts` 的 `ProviderCategory` 定义与 `appConfig.tsx` 图标映射）确认。

- [ ] **步骤 2：验证 + Commit**

运行：`pnpm exec tsc --noEmit && pnpm vitest run src/config --reporter=dot 2>&1 | tail -3`

```bash
git add src/config/deepseekHarnessProviderPresets.ts
git commit -m "feat(dsh): add an OpenAI-compatible provider preset"
```

---

## 任务 13：提示词首启自动导入纳入 DSH

**文件：**
- 修改：`src-tauri/src/app_config.rs`（`PromptRoot` 结构体 `:352-374`、`default_with_auto_import` 循环 `:737-743`、`maybe_auto_import_prompts_for_existing_config` 的空检查 `:760-767` 与循环 `:775-783`、`auto_import_prompt_if_exists` 的 match `:848-857`）
- 测试：`app_config.rs` 的 `#[cfg(test)]`

- [ ] **步骤 1：编写失败的测试**

```rust
    #[test]
    #[serial]
    fn first_run_imports_dsh_agents_md() {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        std::fs::write(directory.path().join("AGENTS.md"), "dsh rules").unwrap();
        let mut config = MultiAppConfig::default();
        let imported =
            MultiAppConfig::auto_import_prompt_if_exists(&mut config, AppType::DeepSeekHarness)
                .unwrap();
        assert!(imported);
        assert!(config
            .prompts
            .deepseek_harness
            .prompts
            .values()
            .any(|prompt| prompt.content.contains("dsh rules")));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
    }
```

注：`MultiAppConfig` 的实际类型名与 `Prompt.content` 字段名先 grep 确认（`grep -n "struct MultiAppConfig\|pub struct Prompt" src-tauri/src/app_config.rs src-tauri/src/prompt.rs`）。

- [ ] **步骤 2：运行验证失败** — `cargo test --manifest-path src-tauri/Cargo.toml app_config --lib`，预期编译错误（无 `deepseek_harness` 字段）。

- [ ] **步骤 3：实现**

`PromptRoot` 在 `hermes` 字段后加：

```rust
    #[serde(rename = "deepseek-harness", alias = "deepseekHarness", default)]
    pub deepseek_harness: PromptConfig,
```

三处循环/检查补 DSH：
- `default_with_auto_import` 加一行 `Self::auto_import_prompt_if_exists(&mut config, AppType::DeepSeekHarness)?;`
- 空检查加 `|| !self.prompts.deepseek_harness.prompts.is_empty()`
- for 循环数组加 `AppType::DeepSeekHarness`
- match（`:848-857`）加 `AppType::DeepSeekHarness => &mut config.prompts.deepseek_harness.prompts,`

- [ ] **步骤 4：运行验证通过** — 同步骤 2 命令。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/app_config.rs
git commit -m "feat(dsh): auto-import DSH AGENTS.md prompts on first run"
```

---

## 任务 14：全量验证与记录

**文件：**
- 修改：`docs/architecture/dsh-native.md`（补会话历史/目录覆盖/命令清单；注明无 token 用量数据源）
- 修改：`docs/agent-memory/handoff.md`
- 修改：`docs/agent-memory/operations/2026-09.md`

- [ ] **步骤 1：聚焦验证**

运行：`pnpm test:dsh`
预期：全部通过（Rust DSH 测试数从 14 增长；前端 37+ 增长）。

- [ ] **步骤 2：全量门禁**

运行：`DSH_FULL_TESTS=1 pnpm test:dsh && pnpm exec tsc --noEmit && cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
预期：全绿。

- [ ] **步骤 3：真实环境冒烟**（人工确认点）

对本机真实 `~/.dsh`（先备份：`cp ~/.dsh/settings.yaml ~/.dsh/settings.yaml.backup-ccswitch-$(date +%Y%m%d-%H%M%S)`）启动应用，确认：DSH 页无 MCP/Skills 按钮、测速可用、会话页列出 DSH 会话、默认模型设置生效、目录设置出现 DSH 行。

- [ ] **步骤 4：更新文档并 Commit**

`docs/architecture/dsh-native.md` 的 Data Flow 与 Ownership Boundaries 补：
- 会话：`~/.dsh/sessions/<encoded-cwd>/<id>/session.jsonl.zstd` 只读浏览 + 删除；无 token 用量字段，用量统计明确不做
- `get_dsh_current_state` / `set_dsh_current_model` 命令
- 目录覆盖优先级：CC Switch 设置 > `$DSH_HOME` > `~/.dsh`

handoff.md 更新 Implemented/Verification/Next Step；operations 追加本轮条目。

```bash
git add docs/
git commit -m "docs(dsh): record full-parity iteration"
```

---

## 自检结果

- 规格覆盖：A(悬空入口)=任务1；A(测速)=任务2；D(remove)=任务3；C(live状态)=任务4/6；C(默认模型)=任务5/6；B(会话)=任务7/8/9；E(目录)=任务10；E(i18n)=任务11；E(预设)=任务12；E(prompt导入)=任务13。用量统计在规格中已明确排除。✓
- 占位符：任务 3/5/13 中的"先 grep 确认"是对**签名适配**的显式指引而非占位符，所附代码为完整目标代码。✓
- 类型一致性：`DshCurrentState{providerIds,currentProviderId,currentModel}` 在 Rust(camelCase serde)/api/dsh.ts/query/dsh.ts/ProviderList 四处一致；`dshApi.setCurrentModel(providerId, modelId)` 与命令参数 `providerId/modelId` 一致。✓
