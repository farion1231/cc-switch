# 设计：每模型上下文窗口配置 + 自动压缩联动

> 日期：2026-07-14
> 分支：`feature/per-model-context-window`
> 状态：待审阅
> 参考：`docs/research/2026-07-14-github-issue-research.md`

## 1. 背景与目标

### 1.1 问题

cc-switch 对模型上下文窗口的支持存在三个痛点：

1. **Claude Code 只有 1M 一档**：`[1M]` 后缀是布尔标记，无法声明 200K / 256K / 500K 等其他窗口大小
2. **后缀与压缩割裂**：勾选"声明支持 1M"只声明能力，不设置 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`，导致超过 200K 显示 100% 但不压缩
3. **Codex catalog 字段缺失**：`auto_compact_token_limit` 和 `effective_context_window_percent` 完全缺失；`truncation_policy.limit` 硬编码 10000，不跟随用户配置（issue #4832 / #5110）

### 1.2 目标

- Claude Code：checkbox → 窗口大小输入框，支持任意粒度后缀，后缀与压缩阈值自动联动
- Codex：补全 catalog 字段，`truncation_policy.limit` 跟随 `context_window` 联动，恢复隐藏的压缩配置 UI，catalog 的 `contextWindow` 列支持多元输入
- Hermes / OpenClaw：已有窗口字段，不动
- OpenCode / Gemini：工具无此机制，跳过

### 1.3 非目标

- 不在代理层重新实现压缩逻辑（#4765 的 ConversationIR 方案已被关闭，太重）
- 不自动从 `/v1/models` 解析窗口值（#5199，后续增强）
- 不修复 stream_check 的 beta header 问题（#2117，后续增强）

## 2. 后缀语法泛化

### 2.1 语法

复用 `model_list` 后缀语法（与 CodexPlusPlus 一致），扩展到 Claude Code 的 `ANTHROPIC_DEFAULT_*_MODEL` 字段：

| 输入 | 解析为 token 数 | 写入 env 的后缀 |
|------|----------------|----------------|
| `1M` / `1m` | 1000000 | `[1m]` |
| `200K` / `200k` | 200000 | `[200k]` |
| `500k` | 500000 | `[500k]` |
| `1000000` | 1000000 | `[1000000]` |
| `128K` | 128000 | `[128k]` |
| （空） | 不声明 | 无后缀 |

单位规则：`K/k` = 1000，`M/m` = 1000000，纯数字直接用（如 `1000000` → 1000000）。

### 2.2 大小写约定

参考 issue #3679：Claude Code 官方文档明确要求小写 `[1m]`。

- **用户输入**：大小写兼容（`1M`、`1m`、`200K`、`200k` 均可）
- **写入配置文件**：统一转小写（`[1m]`、`[200k]`、`[128k]`）
- **解析侧**：大小写不敏感（已有 `.toLowerCase()` / `eq_ignore_ascii_case`，保持不动）

### 2.3 前端解析器（`useModelState.ts`）

替换现有的三个布尔函数为基于解析的函数：

```
现有：
  hasClaudeOneMMarker(model): boolean
  stripClaudeOneMMarker(model): string
  setClaudeOneMMarker(model, enabled): string

改为：
  parseModelSuffix(model): { slug: string, window?: number }
  stripModelSuffix(model): string
  setModelSuffix(model, windowStr: string): string  // windowStr 如 "1M"/"200K"/""
```

`parseModelSuffix` 逻辑（移植 CodexPlusPlus `model_suffix.rs` 的 `parse_model_suffix` + `parse_window_token`）：
1. 取末尾 `[...]`（仅当 `]` 是最后一个字符时才视为后缀）
2. 解析括号内 token：末尾 `K/k` ×1000，`M/m` ×1000000，纯数字 ×1
3. 返回 `{ slug: 去后缀的模型名, window?: 解析值 }`
4. 无后缀或括号内非法（非数字+单位）→ `{ slug: 原值, window: undefined }`，不剥离括号

`setModelSuffix` 写入时统一小写：`setModelSuffix("model", "1M")` → `"model[1m]"`。

### 2.4 Rust 解析器（`claude_desktop_config.rs`）

保留 `ONE_M_CONTEXT_MARKER` 常量用于向后兼容，新增通用函数：

```rust
/// 解析模型名末尾的上下文窗口后缀，返回 (slug, Option<u64>)
/// 逻辑与前端 parseModelSuffix 对称，移植自 CodexPlusPlus model_suffix.rs
pub fn parse_context_window_suffix(model: &str) -> (&str, Option<u64>)
```

`parse_window_token` 内部函数（移植自 CodexPlusPlus）：
```rust
fn parse_window_token(token: &str) -> Option<u64> {
    let token = token.trim();
    if token.is_empty() { return None; }
    let (num_part, multiplier) = match token.chars().last() {
        Some('K' | 'k') => (&token[..token.len() - 1], 1_000u64),
        Some('M' | 'm') => (&token[..token.len() - 1], 1_000_000u64),
        Some(_) => (token, 1u64),
        None => return None,
    };
    num_part.trim().parse::<u64>().ok()
        .map(|value| value * multiplier)
        .filter(|value| *value > 0)
}
```

## 3. Claude Code UI 改造

### 3.1 数据结构

`ClaudeFormFields.tsx` 的 `ModelRoleRow`：

```
现有：
  supportsOneM: boolean  // haiku=false, 其余=true

改为：
  supportsContextWindow: boolean  // 全部=true（haiku 也能配窗口）
```

### 3.2 UI 组件

每个角色行的 `[1M]` checkbox 替换为窗口大小输入框：

- 宽度约 90px，`inputMode="text"`（允许字母 K/M）
- placeholder：`1M`（灰色提示）
- 值为空 = 不声明窗口
- 输入校验：允许数字 + `K`/`k`/`M`/`m` + 纯数字，非法字符实时过滤（标红提示）
  - 合法：`1M`、`200K`、`500k`、`128k`、`1000000`、`1000000`（纯数字）
  - 非法：`1.5M`（小数）、`abc`、`1G`（不支持的单位）
  - 校验逻辑：用 `parseModelSuffix` 尝试解析，`window` 为 `undefined` 且输入非空 → 标红

兜底模型行同样改造。

### 3.3 数据存储

存储方式不变——后缀拼到模型名上写入 env（小写）：

```
ANTHROPIC_DEFAULT_SONNET_MODEL = "deepseek-v4-pro[1m]"
ANTHROPIC_DEFAULT_SONNET_MODEL_NAME = "deepseek-v4-pro"
```

`*_MODEL_NAME` 存不带后缀的干净名（已有逻辑）。

### 3.4 i18n

新增/修改文案：

| key | 中文 |
|-----|------|
| `providerForm.modelContextWindowLabel` | 上下文窗口 |
| `providerForm.modelContextWindowPlaceholder` | 留空=默认，如 1M / 200K |
| `providerForm.modelContextWindowHint` | 声明模型上下文窗口大小，留空使用 Claude Code 默认 200K |

移除旧的 `modelOneMLabel` / `modelOneMHeader` / `supports1mLabel` / `supports1mShort`（或保留为 deprecated）。

## 4. Claude Code 运行时联动（live.rs）

### 4.1 注入逻辑

在 `build_effective_settings_with_common_config` 流程中新增一步 `apply_context_window_defaults`：

1. 扫描 `env` 中所有 `ANTHROPIC_DEFAULT_*_MODEL` 和 `ANTHROPIC_MODEL` 的值
2. 用 `parse_context_window_suffix` 解析每个值的后缀
3. 取所有解析出的窗口值的 **max**
4. 如果 max 存在且 > 0：
   - 注入 `CLAUDE_CODE_MAX_CONTEXT_TOKENS = max`（仅当用户未显式设置时）
   - 注入 `CLAUDE_CODE_AUTO_COMPACT_WINDOW = max`（仅当用户未显式设置时）
5. 如果没有任何后缀：不注入（保持现有行为）

### 4.2 压缩机制

Claude Code 的 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`（ACW）语义：`压缩窗口 = min(模型窗口, ACW值)`，实际压缩触发点在压缩窗口的 ~80%。

设 `ACW = max(所有窗口)`：
- 1M 模型：`min(1M, 1M) = 1M` → 触发 ~800K ✓
- 200K 模型：`min(200K, 1M) = 200K` → 触发 ~160K ✓
- 无后缀模型：`min(200K默认, 1M) = 200K` → 触发 ~160K ✓

每个模型自动按自己窗口的 ~80% 压缩，无需单独配压缩阈值。

### 4.3 与现有 Codex OAuth / Kimi 逻辑的关系

现有 `apply_codex_oauth_claude_context_defaults` 和 `apply_kimi_for_coding_context_defaults` 逻辑保持不动。新的 `apply_context_window_defaults` 在它们之后执行，但**用户显式值优先**——如果 Codex OAuth 已注入 372K，且用户没手动配后缀，则不会被覆盖。

### 4.4 代理剥离泛化（model_mapper.rs）

`strip_one_m_suffix_for_upstream` 泛化为 `strip_context_window_suffix_for_upstream`：

- 现有：只剥离 `[1m]` / `[1M]`
- 改后：剥离任意 `[数字+单位]` 后缀（`[200k]`、`[500k]`、`[1m]` 等），复用 `parse_context_window_suffix`

参考 issue #5177：后缀泄漏到上游会导致 400 错误。

`services/proxy.rs` 的 `has_claude_one_m_marker` / `strip_claude_one_m_marker` / `CLAUDE_ONE_M_MARKER_FOR_CLIENT` 同步泛化。代理在 live takeover 模式下给客户端模型名追加后缀时，也要用新格式（小写）。

## 5. Codex catalog 修复（codex_config.rs）

### 5.1 补全 catalog 字段

`codex_catalog_model_entry` 函数（当前 `:452`）补两个字段（移植自 CodexPlusPlus `build_model_catalog_json`）：

```rust
// 默认 95 会让 1M 显示为 950K，显式写 100 以显示真实窗口
entry_obj.insert("effective_context_window_percent".to_string(), json!(100));
// null = 让 Codex 按内置比例自动计算压缩点（非 null 会固定压缩阈值）
entry_obj.insert("auto_compact_token_limit".to_string(), Value::Null);
```

### 5.2 truncation_policy.limit 联动

`codex_native_responses_template.json` 中硬编码的 `truncation_policy.limit = 10000`（issue #4832）改为动态生成。

在 `codex_catalog_model_entry` 生成 entry 时，覆盖模板的 `truncation_policy`：

```rust
// truncation_policy.limit 跟随 context_window（issue #4832/#5110）
// 留空/0 时回退 10000（保持兼容）
let truncation_limit = if spec.context_window > 0 { spec.context_window } else { 10_000 };
entry_obj.insert("truncation_policy".to_string(), json!({
    "mode": "bytes",
    "limit": truncation_limit
}));
```

模板文件 `codex_native_responses_template.json` 本身不改（保持 10000 作为 fallback），在 Rust 生成时动态覆盖。

### 5.3 恢复隐藏 UI + 多元输入

`CodexConfigSections.tsx:1` 的注释取消，恢复 `model_context_window` / `model_auto_compact_token_limit` 两个输入框。

**关键改动：catalog 表格的 `contextWindow` 列也支持多元输入**（与 Claude Code 一致）：

现有（`CodexFormFields.tsx:940`）：
```tsx
<Input type="number" min={1} inputMode="numeric"
  value={row.contextWindow ?? ""}
  onChange={...replace(/[^\d]/g, "")}  // 只允许纯数字
/>
```

改为：
```tsx
<Input inputMode="text"
  value={row.contextWindow ?? ""}
  onChange={...}  // 允许数字 + K/k/M/m，用 parseModelSuffix 校验
  placeholder="1M / 200K / 128000"
/>
```

Rust 端 `parse_codex_positive_u64`（`:425`）泛化：先尝试 `parse_window_token` 解析多元格式，回退纯数字解析。`codex_catalog_model_specs`（`:578`）读取 `contextWindow` 时用新解析器。

`model_context_window` / `model_auto_compact_token_limit` 两个 TOML 字段保持纯数字（Codex 的 TOML 只接受整数），UI 输入多元格式后在保存时转成纯数字写入。

### 5.4 覆盖保护

已有的 `preserves_user_model_catalog_json` 逻辑保持不动（issue #5110 问题4）。用户手动改的 catalog 不被覆盖。移植自 CodexPlusPlus `apply_model_catalog_to_config` 的检查逻辑：

```rust
// 用户已手写 model_catalog_json 指针时不覆盖
if let Some(existing) = root_key_string(config_text, "model_catalog_json") {
    if existing != catalog_relative {
        return Ok(config_text.to_string());
    }
}
```

### 5.5 catalog 生成函数

移植 CodexPlusPlus 的 `apply_model_catalog_to_config`（`relay_config.rs:1367`）到 cc-switch 的 `codex_config.rs`，适配 cc-switch 的数据结构（`CodexCatalogModelSpec` 而非 `ModelCatalogEntry`）：

- 从 `modelCatalog.models` 收集所有条目
- 用 `parse_context_window_suffix` 解析每个模型的窗口后缀
- 有后缀条目写窗口值，无后缀回退 `model_context_window` 或 272000
- 生成 catalog JSON 文件 + 写入 `model_catalog_json` 指针

## 6. Hermes / OpenClaw / OpenCode / Gemini

| 工具 | 窗口字段 | 压缩机制 | 改动 |
|------|---------|---------|------|
| Hermes | `context_length`（已有 UI） | 无 | 不动 |
| OpenClaw | `context_window`（已有 UI） | 无 | 不动 |
| OpenCode | 无 | 无 | 跳过 |
| Gemini | 无 | 无 | 跳过 |

## 7. 测试计划

### 7.1 前端单元测试（useModelState.test.tsx）

- `parseModelSuffix("deepseek-v4-pro[1m]")` → `{ slug: "deepseek-v4-pro", window: 1000000 }`
- `parseModelSuffix("glm-5.2[200k]")` → `{ slug: "glm-5.2", window: 200000 }`
- `parseModelSuffix("model[500K]")` → `{ slug: "model", window: 500000 }`（大写 K 兼容）
- `parseModelSuffix("model[1000000]")` → `{ slug: "model", window: 1000000 }`（纯数字兼容）
- `parseModelSuffix("model")` → `{ slug: "model", window: undefined }`
- `parseModelSuffix("model[invalid]")` → `{ slug: "model[invalid]", window: undefined }`（非法后缀不剥离）
- `setModelSuffix("model", "1M")` → `"model[1m]"`（小写写入）
- `setModelSuffix("model[1m]", "")` → `"model"`（清空后缀）
- `stripModelSuffix("model[200k]")` → `"model"`

### 7.2 Rust 单元测试

- `parse_context_window_suffix` 对称测试（含纯数字、大小写）
- `parse_window_token` 覆盖：`1M`/`1m`/`200K`/`200k`/`1000000`/空/非法
- `strip_context_window_suffix_for_upstream` 覆盖新格式
- `apply_context_window_defaults` 注入逻辑：
  - 多模型取 max
  - 用户显式值优先
  - 无后缀不注入
- Codex catalog 生成：`auto_compact_token_limit` / `effective_context_window_percent` 字段存在
- Codex `truncation_policy.limit` 跟随 `context_window`
- Codex catalog 覆盖保护：用户手写指针不被覆盖

### 7.3 集成验证

- Claude Code：配 sonnet=1M + opus=200K → 检查 `settings.json` 的 env（后缀小写）
- Claude Code：`/context` 显示正确窗口
- Codex：配 deepseek-v4-pro contextWindow=1M → 检查 catalog JSON（context_window=1000000）
- Codex：长对话触发自动压缩
- Codex：手动改 catalog 后不被覆盖

## 8. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 后缀泛化破坏现有 `[1M]` 数据 | 新解析器兼容 `[1M]`（大小写不敏感），旧数据自动被新格式解读 |
| ACW 全局单值无法按模型独立压缩 | 利用 Claude Code 的 `min(模型窗口, ACW)` 语义，设 ACW=max 自然实现按模型压缩 |
| Codex truncation 改动影响现有行为 | 留空 = 不改写，保持现有 10000 回退；模板文件不改 |
| 代理剥离遗漏新格式导致上游 400 | 泛化 `strip_context_window_suffix_for_upstream`，复用 `parse_context_window_suffix` |
| 用户手动改 catalog 被覆盖 | 已有 `preserves_user_model_catalog_json` 保护 + 移植 CodexPlusPlus 的指针检查 |
| Codex TOML 只接受整数 | 多元格式（1M/200K）只在 UI 输入层，写入 TOML 前转成纯数字 |

## 9. 改动文件清单

### 前端
- `src/components/providers/forms/hooks/useModelState.ts` — 后缀解析器泛化
- `src/components/providers/forms/ClaudeFormFields.tsx` — checkbox → 输入框
- `src/components/providers/forms/CodexFormFields.tsx` — contextWindow 列多元输入
- `src/components/providers/forms/CodexConfigSections.tsx` — 恢复隐藏 UI
- `src/i18n/locales/*.json` — 新增文案

### Rust 后端
- `src-tauri/src/claude_desktop_config.rs` — 新增 `parse_context_window_suffix` + `parse_window_token`
- `src-tauri/src/services/provider/live.rs` — 新增 `apply_context_window_defaults`
- `src-tauri/src/proxy/model_mapper.rs` — 泛化 `strip_*_suffix_for_upstream`
- `src-tauri/src/services/proxy.rs` — 泛化 takeover 后缀处理
- `src-tauri/src/codex_config.rs` — 补 catalog 字段 + truncation 联动 + 多元解析 + catalog 生成
- `src-tauri/src/resources/codex_native_responses_template.json` — 不改（Rust 动态覆盖）

### 测试
- `tests/hooks/useModelState.test.tsx`
- `src-tauri/src/services/provider/live.rs`（内联测试）
- `src-tauri/src/proxy/model_mapper.rs`（内联测试）
- `src-tauri/src/codex_config.rs`（内联测试）
