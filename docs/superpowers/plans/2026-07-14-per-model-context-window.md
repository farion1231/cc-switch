# 每模型上下文窗口 + 自动压缩 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 cc-switch 的 Claude Code `[1M]` 布尔后缀升级为任意粒度上下文窗口后缀（`[1m]`/`[200k]`/`[500k]`），联动注入 `CLAUDE_CODE_AUTO_COMPACT_WINDOW` 实现自动压缩；同时补全 Codex catalog 的 `auto_compact_token_limit`/`effective_context_window_percent`/`truncation_policy.limit` 字段并恢复隐藏 UI。

**Architecture:** 前端 `useModelState.ts` 新增通用后缀解析器（移植自 CodexPlusPlus `model_suffix.rs`），替换布尔 `hasClaudeOneMMarker`；Rust 端 `claude_desktop_config.rs` 新增对称的 `parse_context_window_suffix`；`live.rs` 新增 `apply_context_window_defaults` 扫描所有模型后缀取 max 注入 ACW；`model_mapper.rs` 泛化代理剥离；`codex_config.rs` 补 catalog 字段 + truncation 联动 + 多元解析。

**Tech Stack:** TypeScript/React (Vitest), Rust (cargo test), Tauri, TOML, JSON

---

## File Structure

| 文件 | 职责 | 动作 |
|------|------|------|
| `src/components/providers/forms/hooks/useModelState.ts` | 前端后缀解析器 | Modify |
| `tests/hooks/useModelState.test.tsx` | 前端解析器测试 | Modify |
| `src-tauri/src/claude_desktop_config.rs` | Rust 后缀解析器 | Modify |
| `src-tauri/src/services/provider/live.rs` | ACW 注入逻辑 | Modify |
| `src-tauri/src/proxy/model_mapper.rs` | 代理后缀剥离 | Modify |
| `src-tauri/src/services/proxy.rs` | takeover 后缀处理 | Modify |
| `src-tauri/src/codex_config.rs` | Codex catalog 生成 | Modify |
| `src/components/providers/forms/ClaudeFormFields.tsx` | Claude UI checkbox→input | Modify |
| `src/components/providers/forms/CodexFormFields.tsx` | Codex contextWindow 多元输入 | Modify |
| `src/components/providers/forms/CodexConfigSections.tsx` | 恢复隐藏压缩 UI | Modify |
| `src/i18n/locales/zh.json` 等 | 文案 | Modify |

---

### Task 1: 前端后缀解析器

**Files:**
- Modify: `src/components/providers/forms/hooks/useModelState.ts:17-35`
- Test: `tests/hooks/useModelState.test.tsx`

- [ ] **Step 1: Write the failing tests**

在 `tests/hooks/useModelState.test.tsx` 末尾新增测试块：

```tsx
import { parseModelSuffix, stripModelSuffix, setModelSuffix } from "@/components/providers/forms/hooks/useModelState";

describe("parseModelSuffix", () => {
  it("parses [1m] suffix", () => {
    expect(parseModelSuffix("deepseek-v4-pro[1m]")).toEqual({
      slug: "deepseek-v4-pro",
      window: 1000000,
    });
  });

  it("parses [200k] suffix", () => {
    expect(parseModelSuffix("glm-5.2[200k]")).toEqual({
      slug: "glm-5.2",
      window: 200000,
    });
  });

  it("parses uppercase [500K]", () => {
    expect(parseModelSuffix("model[500K]")).toEqual({
      slug: "model",
      window: 500000,
    });
  });

  it("parses pure number [1000000]", () => {
    expect(parseModelSuffix("model[1000000]")).toEqual({
      slug: "model",
      window: 1000000,
    });
  });

  it("returns undefined window for no suffix", () => {
    expect(parseModelSuffix("model")).toEqual({
      slug: "model",
      window: undefined,
    });
  });

  it("does not strip invalid suffix", () => {
    expect(parseModelSuffix("model[invalid]")).toEqual({
      slug: "model[invalid]",
      window: undefined,
    });
  });
});

describe("setModelSuffix", () => {
  it("appends lowercase suffix", () => {
    expect(setModelSuffix("model", "1M")).toBe("model[1m]");
  });

  it("clears suffix when empty", () => {
    expect(setModelSuffix("model[1m]", "")).toBe("model");
  });

  it("replaces existing suffix", () => {
    expect(setModelSuffix("model[1m]", "200K")).toBe("model[200k]");
  });
});

describe("stripModelSuffix", () => {
  it("strips [200k]", () => {
    expect(stripModelSuffix("model[200k]")).toBe("model");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm vitest run tests/hooks/useModelState.test.tsx`
Expected: FAIL — `parseModelSuffix` is not exported

- [ ] **Step 3: Implement the parser**

在 `src/components/providers/forms/hooks/useModelState.ts` 中，保留 `CLAUDE_ONE_M_MARKER` 常量（向后兼容），在现有三个函数之后新增：

```typescript
export interface ModelSuffixResult {
  slug: string;
  window?: number;
}

function parseWindowToken(token: string): number | undefined {
  const trimmed = token.trim();
  if (!trimmed) return undefined;
  const last = trimmed[trimmed.length - 1];
  let numPart: string;
  let multiplier: number;
  if (last === "K" || last === "k") {
    numPart = trimmed.slice(0, -1);
    multiplier = 1000;
  } else if (last === "M" || last === "m") {
    numPart = trimmed.slice(0, -1);
    multiplier = 1000000;
  } else {
    numPart = trimmed;
    multiplier = 1;
  }
  const value = Number.parseInt(numPart.trim(), 10);
  if (Number.isNaN(value) || value <= 0) return undefined;
  return value * multiplier;
}

export function parseModelSuffix(model: string): ModelSuffixResult {
  const trimmed = model.trim();
  const close = trimmed.lastIndexOf("]");
  if (close !== trimmed.length - 1) {
    return { slug: model, window: undefined };
  }
  const open = trimmed.lastIndexOf("[", close);
  if (open <= 0) return { slug: model, window: undefined };
  const slug = trimmed.slice(0, open).trim();
  if (!slug) return { slug: model, window: undefined };
  const window = parseWindowToken(trimmed.slice(open + 1, close));
  if (window === undefined) return { slug: model, window: undefined };
  return { slug, window };
}

export function stripModelSuffix(model: string): string {
  return parseModelSuffix(model).slug;
}

export function setModelSuffix(model: string, windowStr: string): string {
  const base = stripModelSuffix(model).trim();
  if (!base) return "";
  const trimmed = windowStr.trim();
  if (!trimmed) return base;
  const window = parseWindowToken(trimmed);
  if (window === undefined) return base;
  // 统一小写写入
  return `${base}[${trimmed.toLowerCase()}]`;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm vitest run tests/hooks/useModelState.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/components/providers/forms/hooks/useModelState.ts tests/hooks/useModelState.test.tsx
git commit -m "feat(claude): add generalized model suffix parser with K/M support"
```

---

### Task 2: Rust 后缀解析器

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs` (在 `ONE_M_CONTEXT_MARKER` 常量定义之后)
- Test: 同文件内联 `#[cfg(test)]` 模块

- [ ] **Step 1: Write the failing tests**

在 `claude_desktop_config.rs` 的 `#[cfg(test)]` 模块末尾新增：

```rust
#[test]
fn parse_context_window_suffix_1m() {
    let (slug, window) = parse_context_window_suffix("deepseek-v4-pro[1m]");
    assert_eq!(slug, "deepseek-v4-pro");
    assert_eq!(window, Some(1000000));
}

#[test]
fn parse_context_window_suffix_200k() {
    let (slug, window) = parse_context_window_suffix("glm-5.2[200k]");
    assert_eq!(slug, "glm-5.2");
    assert_eq!(window, Some(200000));
}

#[test]
fn parse_context_window_suffix_uppercase() {
    let (slug, window) = parse_context_window_suffix("model[500K]");
    assert_eq!(slug, "model");
    assert_eq!(window, Some(500000));
}

#[test]
fn parse_context_window_suffix_pure_number() {
    let (slug, window) = parse_context_window_suffix("model[1000000]");
    assert_eq!(slug, "model");
    assert_eq!(window, Some(1000000));
}

#[test]
fn parse_context_window_suffix_no_suffix() {
    let (slug, window) = parse_context_window_suffix("model");
    assert_eq!(slug, "model");
    assert_eq!(window, None);
}

#[test]
fn parse_context_window_suffix_invalid() {
    let (slug, window) = parse_context_window_suffix("model[invalid]");
    assert_eq!(slug, "model[invalid]");
    assert_eq!(window, None);
}

#[test]
fn parse_window_token_handles_empty_and_zero() {
    assert_eq!(parse_window_token(""), None);
    assert_eq!(parse_window_token("0"), None);
    assert_eq!(parse_window_token("0K"), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test parse_context_window_suffix --lib`
Expected: FAIL — `parse_context_window_suffix` not found

- [ ] **Step 3: Implement the parser**

在 `claude_desktop_config.rs` 的 `ONE_M_CONTEXT_MARKER` 常量定义之后新增：

```rust
/// 解析窗口 token，如 "1M" / "200K" / "1000000"。非法或 0 返回 None。
pub fn parse_window_token(token: &str) -> Option<u64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (num_part, multiplier) = match token.chars().last() {
        Some('K' | 'k') => (&token[..token.len() - 1], 1_000u64),
        Some('M' | 'm') => (&token[..token.len() - 1], 1_000_000u64),
        Some(_) => (token, 1u64),
        None => return None,
    };
    num_part
        .trim()
        .parse::<u64>()
        .ok()
        .map(|value| value * multiplier)
        .filter(|value| *value > 0)
}

/// 解析模型名末尾的上下文窗口后缀，返回 (slug, Option<u64>)。
/// 仅当 ] 是最后一个字符时才视为后缀；括号内非法时不剥离。
pub fn parse_context_window_suffix(model: &str) -> (&str, Option<u64>) {
    let trimmed = model.trim();
    if let Some(close) = trimmed.rfind(']') {
        if close == trimmed.len() - 1 {
            if let Some(open) = trimmed[..close].rfind('[') {
                if open > 0 {
                    let slug = trimmed[..open].trim();
                    let inner = trimmed[open + 1..close].trim();
                    if !slug.is_empty() {
                        if let Some(window) = parse_window_token(inner) {
                            return (slug, Some(window));
                        }
                    }
                }
            }
        }
    }
    (model, None)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test parse_context_window_suffix parse_window_token --lib`
Expected: PASS (7 tests)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_desktop_config.rs
git commit -m "feat(core): add Rust parse_context_window_suffix parser"
```

---

### Task 3: Claude Code UI — checkbox → 窗口输入框

**Files:**
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx:519-600,915-1017`

- [ ] **Step 1: Update ModelRoleRow type and rows**

在 `ClaudeFormFields.tsx:527`，将 `supportsOneM: boolean` 改为 `supportsContextWindow: boolean`。
在 `modelRoleRows` 数组（`:530-581`）中，所有 `supportsOneM: false` / `supportsOneM: true` 改为 `supportsContextWindow: true`（全部 true，包括 haiku）。

- [ ] **Step 2: Replace checkbox with input**

在 `:965-977`，替换 checkbox 块：

```tsx
{row.supportsContextWindow && (
  <Input
    inputMode="text"
    className="w-[90px] text-center font-mono text-sm"
    value={parseModelSuffix(row.model).window ? row.model.slice(row.model.lastIndexOf("[")) : ""}
    onChange={(event) => {
      const windowStr = event.currentTarget.value;
      handleRoleModelChange(row, setModelSuffix(row.model, windowStr));
    }}
    placeholder={t("providerForm.modelContextWindowPlaceholder", {
      defaultValue: "1M / 200K",
    })}
    aria-label={t("providerForm.modelContextWindowLabel", {
      defaultValue: "上下文窗口",
    })}
  />
)}
```

在 `:1001-1016`，替换兜底模型的 checkbox 块为同样的 Input。

- [ ] **Step 3: Update imports**

在文件顶部 import 中添加 `parseModelSuffix`, `setModelSuffix`（替换或补充 `hasClaudeOneMMarker`, `setClaudeOneMMarker`）。

- [ ] **Step 4: Update handleRoleModelChange and fallback logic**

在 `:583-599`，`handleRoleModelChange` 中的 `row.supportsOneM` 改为 `row.supportsContextWindow`。`handleRoleOneMChange` 删除（不再需要），或重命名为 `handleRoleWindowChange` 保留空壳。

在 `:250` 和 `:918`，`fallbackUsesOneM` / `usesOneM` 逻辑改用 `parseModelSuffix(...).window !== undefined` 判断。

- [ ] **Step 5: Run typecheck**

Run: `pnpm typecheck`
Expected: PASS — 无类型错误

- [ ] **Step 6: Run existing tests**

Run: `pnpm vitest run tests/components/ClaudeFormFields.test.tsx`
Expected: PASS（如有失败需适配测试中的 `[1M]` 断言为新格式）

- [ ] **Step 7: Commit**

```bash
git add src/components/providers/forms/ClaudeFormFields.tsx
git commit -m "feat(claude): replace 1M checkbox with context window input"
```

---

### Task 4: Claude Code ACW 注入逻辑

**Files:**
- Modify: `src-tauri/src/services/provider/live.rs` (在 `apply_kimi_for_coding_context_defaults` 之后)

- [ ] **Step 1: Write the failing tests**

在 `live.rs` 的 `#[cfg(test)]` 模块末尾新增：

```rust
#[test]
fn context_window_suffix_injects_acw_from_max() {
    let db = Database::memory().expect("create memory db");
    let provider = Provider::with_id(
        "test-suffix".to_string(),
        "Test".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1m]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5.2[200k]"
            }
        }),
        None,
    );

    let effective =
        build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
    assert_eq!(
        effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
        json!("1000000")
    );
    assert_eq!(
        effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
        json!("1000000")
    );
}

#[test]
fn context_window_suffix_no_inject_without_suffix() {
    let db = Database::memory().expect("create memory db");
    let provider = Provider::with_id(
        "test-no-suffix".to_string(),
        "Test".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro"
            }
        }),
        None,
    );

    let effective =
        build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
    assert!(effective["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
    assert!(effective["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none());
}

#[test]
fn context_window_suffix_respects_user_explicit_acw() {
    let db = Database::memory().expect("create memory db");
    let provider = Provider::with_id(
        "test-explicit".to_string(),
        "Test".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1m]",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "500000"
            }
        }),
        None,
    );

    let effective =
        build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
    assert_eq!(
        effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
        json!("500000")
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test context_window_suffix --lib`
Expected: FAIL — 注入逻辑不存在

- [ ] **Step 3: Implement apply_context_window_defaults**

在 `live.rs` 的 `apply_kimi_for_coding_context_defaults` 函数之后新增：

```rust
/// 扫描 env 中所有 ANTHROPIC_DEFAULT_*_MODEL / ANTHROPIC_MODEL 的后缀，
/// 取 max(窗口值) 注入 CLAUDE_CODE_MAX_CONTEXT_TOKENS 和 CLAUDE_CODE_AUTO_COMPACT_WINDOW。
/// 用户显式值优先；无后缀不注入。
fn apply_context_window_defaults(settings: &mut Value, provider: &Provider) {
    let model_env_keys = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ];

    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);

    // 从 provider 配置中收集所有窗口值
    let mut max_window: Option<u64> = None;
    if let Some(env) = provider_env {
        for key in model_env_keys {
            if let Some(model) = env.get(key).and_then(Value::as_str) {
                let (_, window) = crate::claude_desktop_config::parse_context_window_suffix(model);
                if let Some(w) = window {
                    max_window = Some(max_window.map_or(w, |m| m.max(w)));
                }
            }
        }
    }

    let Some(max_window) = max_window else {
        return;
    };

    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };

    let max_str = max_window.to_string();
    for key in ["CLAUDE_CODE_MAX_CONTEXT_TOKENS", "CLAUDE_CODE_AUTO_COMPACT_WINDOW"] {
        // 仅当用户未显式设置时注入
        let user_has_explicit = provider_env
            .is_some_and(|e| e.contains_key(key));
        if !user_has_explicit && !env.contains_key(key) {
            env.insert(key.to_string(), Value::String(max_str.clone()));
        }
    }
}
```

然后在 `build_effective_settings_with_common_config` 的调用链中，在 `apply_kimi_for_coding_context_defaults` 调用之后加：

```rust
apply_context_window_defaults(&mut settings, provider);
```

找到 `build_effective_settings_with_common_config` 中调用 `apply_codex_oauth_claude_context_defaults` 和 `apply_kimi_for_coding_context_defaults` 的位置，在其后添加上述调用。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test context_window_suffix --lib`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/provider/live.rs
git commit -m "feat(claude): inject ACW from max model context window suffix"
```

---

### Task 5: 代理后缀剥离泛化

**Files:**
- Modify: `src-tauri/src/proxy/model_mapper.rs:149-170`
- Modify: `src-tauri/src/services/proxy.rs:314-325`

- [ ] **Step 1: Write the failing tests**

在 `model_mapper.rs` 的 `#[cfg(test)]` 模块末尾新增：

```rust
#[test]
fn strips_200k_suffix_before_upstream() {
    let body = json!({"model": "glm-5.2[200k]"});
    let result = strip_one_m_suffix_for_upstream_from_body(body);
    assert_eq!(result["model"], "glm-5.2");
}

#[test]
fn strips_500k_suffix_before_upstream() {
    let body = json!({"model": "model[500k]"});
    let result = strip_one_m_suffix_for_upstream_from_body(body);
    assert_eq!(result["model"], "model");
}

#[test]
fn keeps_model_without_suffix() {
    let body = json!({"model": "deepseek-v4-pro"});
    let result = strip_one_m_suffix_for_upstream_from_body(body);
    assert_eq!(result["model"], "deepseek-v4-pro");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test strips_200k strips_500k --lib`
Expected: FAIL（现有 `strip_one_m_suffix_for_upstream` 只认 `[1m]`）

- [ ] **Step 3: Generalize the strip function**

在 `model_mapper.rs:149`，将 `strip_one_m_suffix_for_upstream` 改为复用 `parse_context_window_suffix`：

```rust
pub fn strip_one_m_suffix_for_upstream(model: &str) -> &str {
    let (slug, window) = crate::claude_desktop_config::parse_context_window_suffix(model);
    if window.is_some() {
        slug
    } else {
        model
    }
}
```

`strip_one_m_suffix_for_upstream_from_body` 不需要改（它调用上面的函数）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test strips_200k strips_500k strips_one_m --lib`
Expected: PASS（新旧测试都过）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/proxy/model_mapper.rs
git commit -m "feat(proxy): generalize suffix stripping for K/M context windows"
```

---

### Task 6: Codex catalog 字段补全 + truncation 联动

**Files:**
- Modify: `src-tauri/src/codex_config.rs:452-521`

- [ ] **Step 1: Write the failing tests**

在 `codex_config.rs` 的 `#[cfg(test)]` 模块末尾新增：

```rust
#[test]
fn catalog_entry_has_auto_compact_token_limit_null() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-pro", "contextWindow": "1000000" }
            ]
        }
    });
    let specs = codex_catalog_model_specs(&settings, "");
    assert_eq!(specs.len(), 1);
    // 生成 catalog entry 并检查字段
    let template = load_codex_model_catalog_template();
    let entry = codex_catalog_model_entry(&template, &specs[0], 0, CodexCatalogToolProfile::ProxyChat);
    assert_eq!(entry["effective_context_window_percent"], json!(100));
    assert_eq!(entry["auto_compact_token_limit"], json!(null));
}

#[test]
fn catalog_entry_truncation_follows_context_window() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-pro", "contextWindow": "1000000" }
            ]
        }
    });
    let specs = codex_catalog_model_specs(&settings, "");
    let template = load_codex_model_catalog_template();
    let entry = codex_catalog_model_entry(&template, &specs[0], 0, CodexCatalogToolProfile::ProxyChat);
    assert_eq!(entry["truncation_policy"]["limit"], json!(1000000));
}

#[test]
fn catalog_entry_truncation_fallback_10000_when_no_window() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-pro" }
            ]
        }
    });
    let config = r#"model_context_window = 0"#;
    let specs = codex_catalog_model_specs(&settings, config);
    let template = load_codex_model_catalog_template();
    let entry = codex_catalog_model_entry(&template, &specs[0], 0, CodexCatalogToolProfile::ProxyChat);
    assert_eq!(entry["truncation_policy"]["limit"], json!(10000));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test catalog_entry_truncation catalog_entry_has_auto_compact --lib`
Expected: FAIL — 字段不存在 / truncation 仍为 10000

- [ ] **Step 3: Add fields to codex_catalog_model_entry**

在 `codex_config.rs:466-467`（`context_window` / `max_context_window` insert 之后）新增：

```rust
entry_obj.insert("effective_context_window_percent".to_string(), json!(100));
entry_obj.insert("auto_compact_token_limit".to_string(), Value::Null);

// truncation_policy.limit 跟随 context_window（issue #4832/#5110）
let truncation_limit = if spec.context_window > 0 { spec.context_window } else { 10_000 };
entry_obj.insert("truncation_policy".to_string(), json!({
    "mode": "bytes",
    "limit": truncation_limit
}));
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test catalog_entry_truncation catalog_entry_has_auto_compact --lib`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/codex_config.rs
git commit -m "feat(codex): add auto_compact_token_limit + effective_percent + truncation linkage"
```

---


### Task 6b: Codex catalog ??????

**Files:**
- Test: `src-tauri/src/codex_config.rs` (????)

Spec ?5.4??????? catalog ??????????????? Task ????

- [ ] **Step 1: Write verification test**

? `codex_config.rs` ? `#[cfg(test)]` ???????

```rust
#[test]
fn preserves_user_model_catalog_json_pointer() {
    let settings = json!({
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-pro", "contextWindow": "1M" }
            ]
        }
    });
    let config = r#"model = "deepseek-v4-pro"
model_catalog_json = "/my/custom/catalog.json"
"#;
    let result = prepare_codex_config_text_with_model_catalog(&settings, config, "test-id");
    assert!(result.unwrap().contains("/my/custom/catalog.json"));
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test preserves_user_model_catalog --lib`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/codex_config.rs
git commit -m "test(codex): verify user-written catalog pointer is preserved"
```

---

### Task 7: Codex contextWindow 多元输入

**Files:**
- Modify: `src/components/providers/forms/CodexFormFields.tsx:940-962`
- Modify: `src-tauri/src/codex_config.rs:425-431` (`parse_codex_positive_u64`)

- [ ] **Step 1: Generalize Rust parser to accept multi-format**

在 `codex_config.rs:425`，将 `parse_codex_positive_u64` 改为先尝试 `parse_window_token`：

```rust
fn parse_codex_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n.as_u64().filter(|v| *v > 0),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            // 先尝试多元格式（1M / 200K）
            if let Some(w) = crate::claude_desktop_config::parse_window_token(trimmed) {
                return Some(w);
            }
            // 回退纯数字
            trimmed.parse::<u64>().ok().filter(|v| *v > 0)
        }
        _ => None,
    }
}
```

- [ ] **Step 2: Update Codex UI input**

在 `CodexFormFields.tsx:940`，将 `type="number"` 输入改为多元：

```tsx
<Input
  inputMode="text"
  className="w-[100px] text-center font-mono text-sm"
  value={row.contextWindow ?? ""}
  onChange={(event) =>
    handleUpdateCatalogRow(index, {
      contextWindow: event.currentTarget.value,
    })
  }
  placeholder={t("codexConfig.contextWindowPlaceholder", {
    defaultValue: "1M / 200K / 128000",
  })}
  aria-label={t("codexConfig.catalogColumnContext", {
    defaultValue: "上下文窗口",
  })}
/>
```

去掉 `replace(/[^\d]/g, "")` 过滤，允许字母 K/M。

- [ ] **Step 3: Run Rust tests**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS（现有测试不应被破坏，`parse_window_token` 兼容纯数字）

- [ ] **Step 4: Run typecheck**

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/codex_config.rs src/components/providers/forms/CodexFormFields.tsx
git commit -m "feat(codex): support multi-format contextWindow input (1M/200K/number)"
```

---

### Task 8: 恢复 Codex 隐藏的压缩 UI

**Files:**
- Modify: `src/components/providers/forms/CodexConfigSections.tsx:1-6,195-360`

- [ ] **Step 1: Uncomment the import and toggle code**

在 `CodexConfigSections.tsx:1`，删除注释说明，取消 `extractCodexTopLevelInt`, `setCodexTopLevelInt`, `removeCodexTopLevelField` 的 import 注释。

在 `:195-280`，取消 `toggleStates` / `handleContextWindowToggle` / `handleCompactLimitChange` 的注释。

- [ ] **Step 2: Uncomment the JSX**

在文件中找到被注释的 `model_context_window` toggle 和 `model_auto_compact_token_limit` input JSX，取消注释。

- [ ] **Step 3: Run typecheck**

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 4: Run existing tests**

Run: `pnpm vitest run`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/components/providers/forms/CodexConfigSections.tsx
git commit -m "feat(codex): restore hidden context window + compact limit UI"
```

---

### Task 9: i18n 文案更新

**Files:**
- Modify: `src/i18n/locales/zh.json`, `zh-TW.json`, `en.json`, `ja.json`

- [ ] **Step 1: Add new keys**

在 `zh.json` 的 `providerForm` 对象中添加：

```json
"modelContextWindowLabel": "上下文窗口",
"modelContextWindowPlaceholder": "1M / 200K",
"modelContextWindowHint": "声明模型上下文窗口大小，留空使用默认 200K"
```

在 `en.json` 中：

```json
"modelContextWindowLabel": "Context Window",
"modelContextWindowPlaceholder": "1M / 200K",
"modelContextWindowHint": "Declare model context window size; leave empty for default 200K"
```

`zh-TW.json` 和 `ja.json` 按对应语言添加。

- [ ] **Step 2: Run typecheck and format**

Run: `pnpm typecheck && pnpm format`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/i18n/locales/*.json
git commit -m "feat(i18n): add context window label translations"
```

---

### Task 10: 全量测试 + 格式化

- [ ] **Step 1: Run full Rust test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: ALL PASS

- [ ] **Step 2: Run full frontend test suite**

Run: `pnpm vitest run`
Expected: ALL PASS

- [ ] **Step 3: Run typecheck**

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 4: Run format check**

Run: `pnpm format:check`
Expected: PASS（如失败运行 `pnpm format`）

- [ ] **Step 5: Final commit if any formatting changes**

```bash
git add -A
git commit -m "chore: format and finalize"
```
