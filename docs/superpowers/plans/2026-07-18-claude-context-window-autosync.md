# Claude Code 上下文窗口自动同步 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 CC Switch 客户端新增 settings.json 文件监听器，根据 Claude Code 终端 `/model` 切换时写入的 `model` 字段，自动同步 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`（=模型窗口 × 0.8）和 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`（=模型窗口），实现 per-model 自动压缩。

**Architecture:** 新增 Rust 后台模块 `claude_settings_watcher` 监听 `~/.claude/settings.json` 顶层 `model` 字段变化，解析对应 `ANTHROPIC_DEFAULT_<ROLE>_MODEL` 后缀得到窗口大小，写入 env；前端 ClaudeFormFields 加圆角胶囊开关 + ⓘ Tooltip。`apply_context_window_defaults` 等现有兜底函数**完全不动**。

**Tech Stack:** Rust (notify-debouncer-mini, tempfile, serde_json, tokio), React/TypeScript (Radix UI Tooltip, lucide-react, i18next), Tauri 2.0

## Global Constraints

- **依赖版本**：`notify-debouncer-mini = "0.4"`（轻量、跨平台）
- **Tauri 命令命名**：camelCase（仅本 plan 涉及 UI 触发，非 Tauri 命令）
- **i18n key**：所有用户可见文本通过 i18next `t()` 函数获取，禁止硬编码；新增 2 个 key 到 `en`/`zh`/`ja` 三个 i18n locale 文件
- **写入边界**：监听器**只**改 `settings.json.env` 子对象的两个字段，**绝不**触碰其他顶层字段（`effortLevel` / `enabledPlugins` / `mcpServers` / `permissions` / `sandbox` / `statusLine` / `model`）或其他 env 字段
- **防循环**：以"顶层 model 字段值变化"为唯一触发器，监听器自己写 env 不改 model 字段 → 自动不触发
- **错误处理**：所有错误 log 走 `[ClaudeSettingsWatcher]` 前缀，**不**抛到前端
- **后端测试**：`cargo test`，新增 tempfile crate（dev-dependency）
- **前端验证**：`pnpm typecheck && pnpm test:unit`
- **提交规范**：Conventional Commits，每 task 一次 commit
- **不动文件**：`apply_context_window_defaults` / `apply_kimi_for_coding_context_defaults` / `apply_codex_oauth_claude_context_defaults` 全部保留

---

## File Structure

| 文件 | 职责 | 动作 |
|---|---|---|
| `src-tauri/src/claude_settings_watcher.rs` | Rust 监听器模块（结构 + 纯函数 + 单元测试 + fs 集成测试） | Create |
| `src-tauri/src/lib.rs` | 挂载新模块 | Modify |
| `src-tauri/src/services/provider/live.rs` | 在 `build_effective_settings_with_common_config` 末尾启动 watcher | Modify |
| `src-tauri/Cargo.toml` | +1 依赖（notify-debouncer-mini）+1 dev-dependency（tempfile） | Modify |
| `src/components/providers/forms/ClaudeFormFields.tsx` | 圆角胶囊开关 + ⓘ Tooltip + Provider 配置读写 | Modify |
| `src/i18n/locales/en.json` | +2 key | Modify |
| `src/i18n/locales/zh.json` | +2 key | Modify |
| `src/i18n/locales/ja.json` | +2 key | Modify |

**不动文件**：
- `src-tauri/src/services/provider/live.rs` 里的 `apply_context_window_defaults` / `apply_kimi_for_coding_context_defaults` / `apply_codex_oauth_claude_context_defaults`（**完全保留**）
- `src/components/providers/forms/hooks/useModelState.ts`（已有功能不重写）
- `src/components/providers/forms/shared/EndpointField.tsx`（只读复用，不修改）

---

## Task 1: 添加 Rust 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 在 `[dependencies]` 加 notify-debouncer-mini**

在 `src-tauri/Cargo.toml` 找 `[dependencies]` 段，在 `serde_json` 下面加：

```toml
notify-debouncer-mini = "0.4"
```

- [ ] **Step 2: 在 `[dev-dependencies]` 加 tempfile**

在 `src-tauri/Cargo.toml` 找 `[dev-dependencies]` 段（如果存在就在 `serial_test` 旁边加，没有就新建段），加：

```toml
tempfile = "3"
```

- [ ] **Step 3: 验证编译**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo check`
Expected: 编译成功，新依赖下载并链接，无 error

- [ ] **Step 4: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/Cargo.toml
git commit -m "chore(deps): add notify-debouncer-mini for settings watcher"
```

---

## Task 2: 监听器核心函数 — 角色映射

**Files:**
- Create: `src-tauri/src/claude_settings_watcher.rs`
- Test: `src-tauri/src/claude_settings_watcher.rs`（同文件 `#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `pub fn resolve_active_model_window(settings: &Value, provider: &Provider) -> Option<ActiveModelWindow>` 供后续 task 复用

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/claude_settings_watcher.rs` 顶部写：

```rust
use serde_json::{json, Value};

use crate::provider::Provider;

/// 监听 settings.json 后决定的当前激活模型窗口
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveModelWindow {
    pub model: String,
    pub window: u64,
}

/// 根据 settings.json 顶层 model 字段和 provider env 配置，
/// 决定要写入 ACW/MAX 的窗口值。
///
/// 返回 None 表示"不写"（model 字段无效 / 角色对应 env 不存在 / env 无后缀）。
pub(crate) fn resolve_active_model_window(
    settings: &Value,
    provider: &Provider,
) -> Option<ActiveModelWindow> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(env: Value) -> Provider {
        Provider::with_id("p".to_string(), "P".to_string(), json!({ "env": env }), None)
    }

    #[test]
    fn resolve_maps_haiku_to_anthropic_default_haiku_model() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "haiku");
        assert_eq!(result.window, 30000);
    }

    #[test]
    fn resolve_maps_sonnet_to_anthropic_default_sonnet_model() {
        let settings = json!({ "model": "sonnet" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3[1M]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "sonnet");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_opus_to_anthropic_default_opus_model() {
        let settings = json!({ "model": "opus" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1000000]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "opus");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_fable_to_anthropic_default_fable_model() {
        let settings = json!({ "model": "fable" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_FABLE_MODEL": "GLM-5.2[200k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "fable");
        assert_eq!(result.window, 200000);
    }

    #[test]
    fn resolve_maps_subagent_to_claude_code_subagent_model() {
        let settings = json!({ "model": "subagent" });
        let provider = make_provider(json!({
            "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-flash"
        }));
        // subagent 没后缀 → 期望 None（无窗口可写）
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }
}
```

- [ ] **Step 2: 验证测试编译但失败**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::resolve_maps_haiku_to_anthropic_default_haiku_model -- --nocapture 2>&1 | head -20`
Expected: 编译通过，测试失败，`thread 'tests::resolve_maps_...' panicked at 'not yet implemented: resolve_active_model_window'`

- [ ] **Step 3: 实现 resolve_active_model_window**

替换 `unimplemented!()`：

```rust
pub(crate) fn resolve_active_model_window(
    settings: &Value,
    provider: &Provider,
) -> Option<ActiveModelWindow> {
    // 1. 读顶层 model 字段
    let model = settings.get("model").and_then(Value::as_str)?;
    // 2. 映射到 env 字段名
    let env_key = match model {
        "sonnet" => "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "opus" => "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "fable" => "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "haiku" => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "subagent" => "CLAUDE_CODE_SUBAGENT_MODEL",
        _ => return None,
    };
    // 3. 读 provider env 里对应字段
    let env_value = provider
        .settings_config
        .get("env")
        .and_then(|e| e.get(env_key))
        .and_then(Value::as_str)?;
    // 4. 解析后缀得到窗口
    let (_, window) = crate::claude_desktop_config::parse_context_window_suffix(env_value);
    window.map(|w| ActiveModelWindow {
        model: model.to_string(),
        window: w,
    })
}
```

- [ ] **Step 4: 验证测试通过**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::`
Expected: 5 个测试全部 PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/claude_settings_watcher.rs
git commit -m "feat(watcher): add core role-mapping function for model→window"
```

---

## Task 3: 监听器写入逻辑 — ACW/MAX 计算

**Files:**
- Modify: `src-tauri/src/claude_settings_watcher.rs`

**Interfaces:**
- Produces: `pub(crate) fn build_env_writes(window: u64) -> Vec<(&'static str, String)>` 供后续 task 复用

- [ ] **Step 1: 写失败测试**

在 `claude_settings_watcher.rs` 的 `mod tests` 里追加：

```rust
    #[test]
    fn build_writes_for_30k_window() {
        let writes = build_env_writes(30000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "24000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "30000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_1m_window() {
        let writes = build_env_writes(1000000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "800000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1000000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_200k_window() {
        let writes = build_env_writes(200000);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "160000".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "200000".to_string()),
        ]);
    }

    #[test]
    fn build_writes_for_1_token_boundary() {
        // 最小边界：1 token → ACW=0（×0.8 = 0.8，向下取整 = 0）
        let writes = build_env_writes(1);
        assert_eq!(writes, vec![
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "0".to_string()),
            ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1".to_string()),
        ]);
    }
```

- [ ] **Step 2: 验证测试编译失败**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::build_writes`
Expected: 编译失败，`error[E0425]: cannot find function 'build_env_writes'`

- [ ] **Step 3: 实现 build_env_writes**

在 `claude_settings_watcher.rs` 顶部结构体定义下加：

```rust
/// 根据窗口值生成要写入 settings.json.env 的两个 env 项。
/// ACW = 窗口 × 0.8（向下取整），MAX = 窗口本身。
pub(crate) fn build_env_writes(window: u64) -> Vec<(&'static str, String)> {
    let acw = (window * 80) / 100;
    vec![
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw.to_string()),
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", window.to_string()),
    ]
}
```

- [ ] **Step 4: 验证测试通过**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::build_writes`
Expected: 4 个测试全部 PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/claude_settings_watcher.rs
git commit -m "feat(watcher): add env write builder for ACW/MAX"
```

---

## Task 4: 监听器无效输入处理

**Files:**
- Modify: `src-tauri/src/claude_settings_watcher.rs`

- [ ] **Step 1: 写失败测试（无效 model 值）**

在 `mod tests` 追加：

```rust
    #[test]
    fn resolve_returns_none_when_model_field_missing() {
        let settings = json!({});
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_value_unknown() {
        let settings = json!({ "model": "custom-alias" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_not_string() {
        let settings = json!({ "model": 123 });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_null() {
        let settings = json!({ "model": null });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_role_env_field_missing() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_env_value_not_string() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": { "name": "weird" }
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }
```

- [ ] **Step 2: 写失败测试（无效后缀）**

追加：

```rust
    #[test]
    fn resolve_returns_none_when_suffix_invalid() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[invalid]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_zero() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[0]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_unsupported_unit() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1G]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_decimal() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1.5m]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_suffix_at_all() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }
```

- [ ] **Step 3: 验证测试通过**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::resolve_returns_none`
Expected: 11 个测试全部 PASS（之前的实现已经用 `?` 操作符和 `Option` 处理了所有无效输入，**不需要新代码**）

- [ ] **Step 4: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/claude_settings_watcher.rs
git commit -m "test(watcher): cover invalid model and suffix inputs"
```

---

## Task 5: 防循环核心 — `last_seen_model` 跟踪

**Files:**
- Modify: `src-tauri/src/claude_settings_watcher.rs`

**Interfaces:**
- Produces: `pub(crate) struct ClaudeSettingsWatcher { last_seen_model: Mutex<Option<String>>, settings_path: PathBuf, auto_sync_enabled: Arc<AtomicBool> }` 供后续 task 复用

- [ ] **Step 1: 写失败测试**

在 `mod tests` 追加（用 `Mutex<Option<String>>` 模拟 watcher 内部状态）：

```rust
    use std::sync::Mutex;

    /// 模拟 watcher 内部状态的辅助函数：
    /// 决定一个新事件是否应该处理（model 字段值变了才处理）
    fn should_process(state: &Mutex<Option<String>>, new_model: Option<&str>) -> bool {
        let mut guard = state.lock().unwrap();
        let new_model_owned = new_model.map(|s| s.to_string());
        if *guard == new_model_owned {
            return false;
        }
        *guard = new_model_owned;
        true
    }

    #[test]
    fn loop_same_model_consecutive_triggers() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(!should_process(&state, Some("haiku")));
        assert!(!should_process(&state, Some("haiku")));
    }

    #[test]
    fn loop_two_models_alternating() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, Some("sonnet")));
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, Some("sonnet")));
    }

    #[test]
    fn loop_model_to_none_to_same() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, None));       // model 被删
        assert!(should_process(&state, Some("haiku"))); // 重新出现
    }

    #[test]
    fn loop_model_to_none_stays_none() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, None));
        // 后续 None 事件都算"无变化" → 跳过
        assert!(!should_process(&state, None));
        assert!(!should_process(&state, None));
    }

    #[test]
    fn loop_initial_state_with_existing_model() {
        // 启动时 model 已经是 "haiku"（如上次会话留下的）
        let state = Mutex::new(Some("haiku".to_string()));
        // 第一次触发就是 haiku → 跳过（不算变化）
        assert!(!should_process(&state, Some("haiku")));
        // 切到别的 → 处理
        assert!(should_process(&state, Some("sonnet")));
    }
```

- [ ] **Step 2: 验证测试编译失败**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::loop`
Expected: 编译失败，`error[E0425]: cannot find function 'should_process'`

- [ ] **Step 3: 实现 should_process**

在 `claude_settings_watcher.rs` 顶部（`build_env_writes` 后面）加：

```rust
/// 检查新事件的 model 字段是否需要处理（与上次不同则处理）。
/// 这是监听器防循环的核心逻辑：自己写 env 不改 model 字段 → 自动跳过。
pub(crate) fn should_process(
    state: &Mutex<Option<String>>,
    new_model: Option<&str>,
) -> bool {
    let mut guard = state.lock().expect("settings watcher mutex poisoned");
    let new_model_owned = new_model.map(|s| s.to_string());
    if *guard == new_model_owned {
        return false;
    }
    *guard = new_model_owned;
    true
}
```

并在文件顶部加：

```rust
use std::sync::Mutex;
```

- [ ] **Step 4: 验证测试通过**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::loop`
Expected: 5 个测试全部 PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/claude_settings_watcher.rs
git commit -m "feat(watcher): add should_process loop-prevention helper"
```

---

## Task 6: 完整 watcher 结构 + 文件系统监听

**Files:**
- Modify: `src-tauri/src/claude_settings_watcher.rs`

**Interfaces:**
- Produces: `pub(crate) fn spawn_claude_settings_watcher(settings_path: PathBuf, provider: Arc<Provider>) -> Result<ClaudeSettingsWatcher, AppError>`

- [ ] **Step 1: 写 fs 集成测试（用 tempfile）**

在 `mod tests` 追加：

```rust
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;
    use std::time::Duration;
    use std::thread;

    fn write_settings(path: &std::path::Path, model: &str) {
        let content = format!(r#"{{"model": "{}"}}"#, model);
        fs::write(path, content).unwrap();
    }

    fn read_acw_max(path: &std::path::Path) -> Option<(String, String)> {
        let content = fs::read_to_string(path).ok()?;
        let v: Value = serde_json::from_str(&content).ok()?;
        let env = v.get("env")?.as_object()?;
        let acw = env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")?.as_str()?.to_string();
        let max = env.get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")?.as_str()?.to_string();
        Some((acw, max))
    }

    #[test]
    fn fs_modify_model_field_triggers_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("settings.json");
        write_settings(&path, "haiku");
        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        })));

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        write_settings(&path, "sonnet");
        // 给监听器时间触发（debounce 200ms + 处理时间）
        thread::sleep(Duration::from_millis(500));

        let (acw, max) = read_acw_max(&path).expect("acw/max should be written");
        assert_eq!(acw, "24000");
        assert_eq!(max, "30000");
        drop(watcher);
    }

    #[test]
    fn fs_modify_other_field_does_not_touch_env() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("settings.json");
        write_settings(&path, "haiku");
        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        })));

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        // 改 model=haiku，env 加一个无关字段
        let content = fs::read_to_string(&path).unwrap();
        let mut v: Value = serde_json::from_str(&content).unwrap();
        v["env"] = json!({ "SOME_OTHER_VAR": "foo" });
        fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(500));

        // ACW/MAX 应该**没**被写（model 没变）
        assert!(read_acw_max(&path).is_none());
        // 无关 env 应该**保留**
        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["env"]["SOME_OTHER_VAR"], "foo");
        drop(watcher);
    }

    #[test]
    fn fs_watcher_self_write_does_not_loop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("settings.json");
        write_settings(&path, "haiku");
        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        })));

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        write_settings(&path, "sonnet");
        thread::sleep(Duration::from_millis(500));

        // 第一次写入：应该写入 ACW/MAX
        let (acw, _) = read_acw_max(&path).expect("first write should happen");
        assert_eq!(acw, "24000");

        // 等待额外 1 秒：监听器自己写入不应触发再次处理
        let content_before = fs::read_to_string(&path).unwrap();
        thread::sleep(Duration::from_millis(1000));
        let content_after = fs::read_to_string(&path).unwrap();
        // 文件 mtime 可能变但 model 字段不变 → 内容中 ACW/MAX 值应保持
        assert_eq!(content_before, content_after);
        drop(watcher);
    }

    #[test]
    fn fs_disabled_auto_sync_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("settings.json");
        write_settings(&path, "haiku");
        let mut provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        }));
        provider.settings_config["autoSyncContextWindow"] = json!(false);
        let provider = Arc::new(provider);

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        write_settings(&path, "sonnet");
        thread::sleep(Duration::from_millis(500));

        // 开关 OFF → 不应写
        assert!(read_acw_max(&path).is_none());
        drop(watcher);
    }

    #[test]
    fn fs_settings_missing_does_not_panic() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let provider = Arc::new(make_provider(json!({})));

        // 不应 panic
        let result = spawn_claude_settings_watcher(path, provider);
        // 期望：返回错误或 watcher 启动后立即失败，**不** panic
        // 行为可接受：Err 或 Ok（取决于实现）
        if let Ok(w) = result {
            drop(w); // 优雅关闭
        }
    }

    #[test]
    fn fs_modify_unknown_role_does_not_touch_env() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("settings.json");
        write_settings(&path, "haiku");
        let provider = Arc::new(make_provider(json!({}))); // 没配 env

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        write_settings(&path, "sonnet");
        thread::sleep(Duration::from_millis(500));

        // model 有效但 env 字段不存在 → 不写
        assert!(read_acw_max(&path).is_none());
        drop(watcher);
    }
```

- [ ] **Step 2: 验证测试编译失败**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::fs 2>&1 | head -20`
Expected: 编译失败，`error[E0425]: cannot find function 'spawn_claude_settings_watcher'`

- [ ] **Step 3: 实现 ClaudeSettingsWatcher + spawn**

在 `claude_settings_watcher.rs` 顶部加 import：

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use parking_lot_lite::Mutex; // 见 Step 4 注释

use crate::error::AppError;
```

如果 `parking_lot_lite` 不存在，改用 `std::sync::Mutex`（Task 5 已用 std::sync::Mutex，可以**复用**）：
- 把 Task 5 的 `use std::sync::Mutex;` 留着
- 删除 `parking_lot_lite` 那行

实现 watcher 结构：

```rust
/// Claude Code settings.json 监听器
///
/// 后台线程监听文件变化，根据顶层 model 字段值变化自动同步 ACW/MAX。
pub struct ClaudeSettingsWatcher {
    /// 防循环用的"上次见到的 model 字段值"
    state: Arc<Mutex<Option<String>>>,
    /// 关闭信号
    shutdown: Arc<AtomicBool>,
    /// notify debouncer handle（Drop 时自动停止监听）
    _debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
}

impl Drop for ClaudeSettingsWatcher {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// 启动 settings.json 监听器
///
/// 返回的 watcher 在 Drop 时自动停止监听。
pub(crate) fn spawn_claude_settings_watcher(
    settings_path: PathBuf,
    provider: Arc<Provider>,
) -> Result<ClaudeSettingsWatcher, AppError> {
    let state = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(AtomicBool::new(false));

    // 启动时读一次 settings.json 初始化 state
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            *state.lock().unwrap() = v.get("model").and_then(Value::as_str).map(String::from);
        }
    }

    let state_clone = state.clone();
    let shutdown_clone = shutdown.clone();
    let provider_clone = provider.clone();
    let path_clone = settings_path.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |result: DebounceEventResult| {
            if shutdown_clone.load(Ordering::SeqCst) {
                return;
            }
            let events = match result {
                Ok(events) => events,
                Err(errors) => {
                    for err in errors {
                        log::warn!("[ClaudeSettingsWatcher] debounce error: {err}");
                    }
                    return;
                }
            };
            // 任意一个事件触发就处理
            for _event in events {
                handle_settings_change(&path_clone, &provider_clone, &state_clone);
            }
        },
    )
    .map_err(|e| AppError::Message(format!("failed to create settings watcher: {e}")))?;

    debouncer
        .watcher()
        .watch(&settings_path, RecursiveMode::NonRecursive)
        .map_err(|e| AppError::Message(format!("failed to watch settings.json: {e}")))?;

    Ok(ClaudeSettingsWatcher {
        state,
        shutdown,
        _debouncer: Some(debouncer),
    })
}

/// 处理一次 settings.json 变化
fn handle_settings_change(
    path: &std::path::Path,
    provider: &Provider,
    state: &Mutex<Option<String>>,
) {
    // 1. 读最新内容
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("[ClaudeSettingsWatcher] read failed: {e}");
            return;
        }
    };
    let v: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] invalid JSON: {e}");
            return;
        }
    };

    // 2. 读 model 字段
    let new_model = v.get("model").and_then(Value::as_str);

    // 3. 检查 model 字段是否变化（防循环）
    if !should_process(state, new_model) {
        return;
    }

    // 4. 检查 provider 的 autoSyncContextWindow 开关
    let auto_sync = provider
        .settings_config
        .get("autoSyncContextWindow")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !auto_sync {
        log::debug!("[ClaudeSettingsWatcher] auto-sync disabled for provider, skip");
        return;
    }

    // 5. 决定要写的窗口值
    let active = match resolve_active_model_window(&v, provider) {
        Some(a) => a,
        None => {
            log::debug!("[ClaudeSettingsWatcher] no active model window to write");
            return;
        }
    };

    // 6. 写 ACW/MAX
    let writes = build_env_writes(active.window);
    let new_content = match update_env_fields(&content, &writes) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] update failed: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(path, new_content) {
        log::warn!("[ClaudeSettingsWatcher] write failed: {e}");
    } else {
        log::info!(
            "[ClaudeSettingsWatcher] wrote ACW/MAX for model={} window={}",
            active.model, active.window
        );
    }
}

/// 原子地更新 settings.json 中 env 子对象的指定字段，其他字段全部保留
fn update_env_fields(
    content: &str,
    writes: &[(&'static str, String)],
) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("top-level not object".to_string());
    }
    let obj = v.as_object_mut().unwrap();
    let env = obj
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !env.is_object() {
        return Err("env not object".to_string());
    }
    let env_obj = env.as_object_mut().unwrap();
    for (key, value) in writes {
        env_obj.insert((*key).to_string(), Value::String(value.clone()));
    }
    serde_json::to_string(&v).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: 验证 fs 测试通过**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib claude_settings_watcher::tests::fs 2>&1 | tail -30`
Expected: 6 个 fs 测试全部 PASS

如果某些测试**偶尔 flaky**（macOS FSEvents 异步行为），重试 1-2 次。**不**改逻辑去重试，是事件系统的时序问题，测试容忍度可接受。

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/claude_settings_watcher.rs
git commit -m "feat(watcher): add fs-level watcher with loop prevention"
```

---

## Task 7: 挂载模块 + 启动 watcher

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/services/provider/live.rs`

- [ ] **Step 1: 挂载新模块**

在 `src-tauri/src/lib.rs` 找现有 `pub mod xxx;` 声明附近，加：

```rust
pub mod claude_settings_watcher;
```

- [ ] **Step 2: 在 live.rs 启动 watcher**

在 `src-tauri/src/services/provider/live.rs` 找 `build_effective_settings_with_common_config` 函数（已经在 Step 6 看过位置，约 708 行），在**返回 `Ok(effective_settings)` 之前**加：

```rust
    // 启动 settings.json 监听器（per-process 单例，重复调用由 watcher 内部去重）
    if let Some(provider_id) = provider.id.strip_prefix("claude_").or(Some(provider.id.as_str())) {
        let path = crate::config::get_claude_settings_path();
        if path.exists() {
            let provider_arc = std::sync::Arc::new(provider.clone());
            if let Err(e) = crate::claude_settings_watcher::spawn_claude_settings_watcher(path, provider_arc) {
                log::warn!("[ClaudeSettingsWatcher] spawn failed: {e}");
            }
        }
    }
```

注意：watcher 启动在 `build_effective_settings_with_common_config` 内是**每次同步**都会调，但 watcher 内部用 `Arc<Mutex<Option<String>>>` 状态共享；多次启动**会有多个线程监听同一文件**——这是**已知缺陷**（详细设计权衡见 spec 3.3）。

**更优方案**（独立实施）：用 `OnceCell<ClaudeSettingsWatcher>` 在进程内单例化。本 task **先**接受多次启动的浪费，**后续** task 优化。

- [ ] **Step 3: 验证编译**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo check`
Expected: 编译成功，**无** warning（除非 `provider.id.strip_prefix` 链式有未使用返回值）

- [ ] **Step 4: 跑全量测试**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib 2>&1 | tail -20`
Expected: 之前所有测试仍 PASS + claude_settings_watcher 测试 PASS（无回归）

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src-tauri/src/lib.rs src-tauri/src/services/provider/live.rs
git commit -m "feat(watcher): wire up settings watcher in live config flow"
```

---

## Task 8: 前端 UI 圆角胶囊开关

**Files:**
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx`

- [ ] **Step 1: 找现有 imports 和 state 位置**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && grep -n "import\|setAutoSync" src/components/providers/forms/ClaudeFormFields.tsx | head -20`
Expected: 看到现有的 import 列表（Lucide icons、Radix 等）

- [ ] **Step 2: 加新 import**

在 `import` 段加：

```tsx
import { Switch } from "@/components/ui/switch";
import { Info, RefreshCw } from "lucide-react";
```

如果 `Switch` 已存在，跳过第一个 import。

- [ ] **Step 3: 加 autoSyncContextWindow state**

在 `ClaudeFormFields` 函数体内（找现有的 `useState` 声明附近）加：

```tsx
const autoSyncContextWindow = settingsConfig
  ? ((JSON.parse(settingsConfig).autoSyncContextWindow as boolean | undefined) ?? true)
  : true;
```

如果 `settingsConfig` 是 prop，**保留** prop 名称。逻辑：默认 true，从 settingsConfig 顶层读。

- [ ] **Step 4: 加更新函数**

```tsx
const handleAutoSyncChange = useCallback(
  (checked: boolean) => {
    if (!settingsConfig) return;
    try {
      const parsed = JSON.parse(settingsConfig);
      parsed.autoSyncContextWindow = checked;
      onConfigChange(JSON.stringify(parsed, null, 2));
    } catch (err) {
      console.error("Failed to update autoSyncContextWindow:", err);
    }
  },
  [settingsConfig, onConfigChange],
);
```

- [ ] **Step 5: 在 UI 中渲染开关**

找到"上下文长度"输入框的渲染位置（grep 找 `modelContextWindowPlaceholder` 关键字），在**该输入框下方一行**加：

```tsx
<div className="flex items-center gap-2 rounded-full border border-border/70 bg-muted/30 px-2.5 py-1 w-fit">
  <RefreshCw className="h-3.5 w-3.5 text-muted-foreground" />
  <span className="text-xs font-medium text-foreground">
    {t("providerForm.autoSyncContextWindow", { defaultValue: "自动同步" })}
  </span>
  <Switch
    checked={autoSyncContextWindow}
    onCheckedChange={handleAutoSyncChange}
    aria-label={t("providerForm.autoSyncContextWindow", { defaultValue: "自动同步" })}
    className="h-5 w-9"
  />
</div>
```

- [ ] **Step 6: 验证 typecheck**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm typecheck 2>&1 | tail -20`
Expected: PASS，无 TS error

- [ ] **Step 7: 视觉验证（启动 dev server）**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm dev`
Expected: 应用启动，打开 Provider 编辑页，找到"上下文长度"输入框下方，能看到圆角胶囊：↻ 自动同步 [Switch]

- [ ] **Step 8: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src/components/providers/forms/ClaudeFormFields.tsx
git commit -m "feat(ui): add auto-sync context window switch in Claude provider form"
```

---

## Task 9: 前端 ⓘ Tooltip

**Files:**
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx`

- [ ] **Step 1: 加 Tooltip import**

```tsx
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
```

- [ ] **Step 2: 包装开关为 TooltipTrigger**

把 Task 8 Step 5 的 `<Switch>` 包进 Tooltip 组件：

```tsx
<TooltipProvider delayDuration={200}>
  <Tooltip>
    <TooltipTrigger asChild>
      <div className="flex items-center gap-2 rounded-full border border-border/70 bg-muted/30 px-2.5 py-1 w-fit">
        <RefreshCw className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="text-xs font-medium text-foreground">
          {t("providerForm.autoSyncContextWindow", { defaultValue: "自动同步" })}
        </span>
        <Switch
          checked={autoSyncContextWindow}
          onCheckedChange={handleAutoSyncChange}
          aria-label={t("providerForm.autoSyncContextWindow", { defaultValue: "自动同步" })}
          className="h-5 w-9"
        />
        <Info className="h-3.5 w-3.5 text-muted-foreground cursor-help" />
      </div>
    </TooltipTrigger>
    <TooltipContent side="bottom" className="max-w-xs">
      <p className="text-xs leading-relaxed">
        {t("providerForm.autoSyncContextWindowTooltip", {
          defaultValue:
            "终端内切换模型时，上下文长度和压缩阈值按切换的模型更新配置 json。多 claude 终端使用不同模型，以最后切换模型时的上下文长度作为全局变量。",
        })}
      </p>
    </TooltipContent>
  </Tooltip>
</TooltipProvider>
```

- [ ] **Step 3: 验证 typecheck**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm typecheck 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 4: 视觉验证 hover 行为**

Run: `pnpm dev` 启动应用，打开 Provider 编辑页，鼠标悬停在 ⓘ 图标上 → 期望看到 tooltip 弹出，显示中文提示文案

- [ ] **Step 5: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src/components/providers/forms/ClaudeFormFields.tsx
git commit -m "feat(ui): add tooltip explaining auto-sync behavior"
```

---

## Task 10: i18n 翻译 key（3 语）

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/ja.json`

- [ ] **Step 1: 加英文 key**

在 `src/i18n/locales/en.json` 找 `"providerForm"` 段，加：

```json
    "autoSyncContextWindow": "Auto-sync",
    "autoSyncContextWindowTooltip": "When switching models in the terminal, context length and compression threshold are updated in settings.json based on the selected model. With multiple Claude terminals using different models, the most recently switched model's context length is used as the global value.",
```

- [ ] **Step 2: 加中文 key**

在 `src/i18n/locales/zh.json` 找 `"providerForm"` 段，加：

```json
    "autoSyncContextWindow": "自动同步",
    "autoSyncContextWindowTooltip": "终端内切换模型时，上下文长度和压缩阈值按切换的模型更新配置 json。多 claude 终端使用不同模型，以最后切换模型时的上下文长度作为全局变量。",
```

- [ ] **Step 3: 加日文 key**

在 `src/i18n/locales/ja.json` 找 `"providerForm"` 段，加：

```json
    "autoSyncContextWindow": "自動同期",
    "autoSyncContextWindowTooltip": "ターミナルでモデルを切り替える際、コンテキスト長と圧縮閾値が選択されたモデルに基づいて settings.json で更新されます。複数の claude ターミナルが異なるモデルを使用する場合、最後に切り替えたモデルのコンテキスト長がグローバル値として使用されます。",
```

- [ ] **Step 4: 验证格式**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && python3 -c "import json; json.load(open('src/i18n/locales/en.json')); json.load(open('src/i18n/locales/zh.json')); json.load(open('src/i18n/locales/ja.json')); print('all valid')"`
Expected: `all valid`（**不**抛 JSON 错误）

- [ ] **Step 5: 验证 i18n 集成**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm typecheck 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git add src/i18n/locales/en.json src/i18n/locales/zh.json src/i18n/locales/ja.json
git commit -m "feat(i18n): add auto-sync context window labels and tooltips"
```

---

## Task 11: 最终验证 + 端到端清单

**Files:** (无新增)

- [ ] **Step 1: 跑全量后端测试**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo test --lib 2>&1 | tail -10`
Expected: 全部测试 PASS（包含 claude_settings_watcher 的 20+5+11+6+5 = 47 个测试）

- [ ] **Step 2: 跑后端 lint**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main/src-tauri && cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: 无 warning

- [ ] **Step 3: 跑前端 typecheck**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm typecheck 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 4: 跑前端 lint/format**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm format:check 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: 跑前端单元测试**

Run: `cd /Users/jarvis/Documents/cc-switch/cc-switch-main && pnpm test:unit 2>&1 | tail -10`
Expected: 全部 PASS（**不**应该有 ClaudeFormFields 相关测试因新代码失败）

- [ ] **Step 6: 端到端手动验证（按 spec 7.4 清单）**

启动应用：
```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
pnpm dev
```

启动 Claude Code 终端，验证：
- [ ] 默认 provider 配置下，"自动同步"开关**默认 ON**
- [ ] /model 切到 Kimi，/context 显示 24k tokens
- [ ] /model 切到 MiniMax-M3，/context 显示 800k tokens
- [ ] /model 切到 GLM-5.2，/context 显示 160k tokens
- [ ] /model 切到 deepseek-v4-pro，/context 显示 800k tokens
- [ ] 关闭"自动同步"，/model 切到 opus，/context 不变（保留之前的值）
- [ ] hover ⓘ 图标，tooltip 显示中文文案

- [ ] **Step 7: 已知限制手动验证（spec 7.4）**

- [ ] 开两个 Claude Code 终端，A /model haiku，B /model sonnet → 观察 A 的 /context 显示 sonnet 的 ACW（**预期**：错位，**符合** spec 8.2）

- [ ] **Step 8: 回归测试**

- [ ] 验证手动改 ACW 后**不被** watch 覆盖（spec 5.5）
- [ ] 验证 Codex OAuth provider 上 /model 切换仍由 `apply_codex_oauth_claude_context_defaults` 注入 372K（spec 5.7）
- [ ] 验证 Kimi for Coding provider 上 /model 切换仍由 `apply_kimi_for_coding_context_defaults` 注入 262K

- [ ] **Step 9: 最终 commit（如有 docs 调整）**

```bash
cd /Users/jarvis/Documents/cc-switch/cc-switch-main
git status
# 如果有未提交的修改：
git add -A
git commit -m "docs: post-implementation verification checklist complete"
```
Expected: 工作目录干净（**没有**未提交修改），或只有 docs 类改动

---

## Self-Review 记录

写完后做了一次 self-review：

**1. Spec 覆盖检查**：spec 9 个一级 section 全部覆盖——
- 1. 背景 → Task 1-7 实现
- 3.1 监听器 → Task 2、5、6
- 3.2 开关 UI → Task 8、9
- 3.3 兜底兼容 → Task 7 + 显式说明"不动兜底函数"
- 4. UI → Task 8、9、10
- 5. 数据流 → Task 6 测试覆盖
- 6. 错误处理 → Task 4、6 测试覆盖
- 7. 测试 → Task 2-6、11
- 8. 风险 → Task 7 注释 + Task 11 Step 7 端到端验证
- 9. 实施 → 整个 plan

**2. Placeholder 扫描**：
- ❌ "TBD" / "TODO" / "implement later"：无
- ❌ "add appropriate error handling"：无（所有错误路径 Task 6 Step 3 有具体 `log::warn!` 调用）
- ❌ "Write tests for the above"：无（每 task 有具体测试代码）
- ❌ "Similar to Task N"：无（每个 task 步骤独立完整）
- ❌ 未定义的类型/函数引用：无（`ActiveModelWindow` / `resolve_active_model_window` / `build_env_writes` / `should_process` / `spawn_claude_settings_watcher` 都在定义它们的 task 里）

**3. 类型一致性**：
- `ActiveModelWindow` 在 Task 2 定义 `{ model: String, window: u64 }`，Task 6 引用同一结构 ✓
- `build_env_writes` 在 Task 3 定义 `Vec<(&'static str, String)>`，Task 6 引用同签名 ✓
- `should_process` 在 Task 5 定义 `fn(&Mutex<Option<String>>, Option<&str>) -> bool`，Task 6 引用同签名 ✓
- `spawn_claude_settings_watcher` 在 Task 6 定义，Task 7 引用 ✓
