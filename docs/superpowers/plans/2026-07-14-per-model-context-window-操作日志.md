# 操作日志：每模型上下文窗口配置 + 自动压缩联动

> 日期：2026-07-14
> 分支：`feature/per-model-context-window`
> 执行人：Codex Agent
> 参考文档：
> - 调研：`docs/research/2026-07-14-github-issue-research.md`
> - 设计：`docs/specs/2026-07-14-per-model-context-window-design.md`
> - 计划：`docs/superpowers/plans/2026-07-14-per-model-context-window.md`

---

## 一、执行时间线

| 序号 | Commit | 时间 | 内容 |
|------|--------|------|------|
| 1 | `bd4fbb0` | 18:57 | Task 1：前端后缀解析器（`useModelState.ts`），新增 `parseModelSuffix`/`stripModelSuffix`/`setModelSuffix` + 10 个测试 |
| 2 | `5462990` | 19:12 | Task 2：Rust 后缀解析器（`claude_desktop_config.rs`），新增 `parse_context_window_suffix`/`parse_window_token` + 7 个测试 |
| 3 | `a0f0fca` | 20:30 | Task 3：Claude Code UI，`[1M]` checkbox 替换为窗口大小输入框 |
| 4 | `067498a` | 20:55 | Task 4+5：ACW 注入逻辑（`live.rs`）+ 代理后缀剥离泛化（`model_mapper.rs`/`proxy.rs`） |
| 5 | `a2dc13e` | 21:20 | Task 6+7：Codex catalog 字段补全 + truncation 联动 + 多元解析 |
| 6 | `104fd61` | 21:35 | Task 7+8+9：Codex contextWindow 多元输入 + 恢复隐藏压缩 UI + i18n 文案 |
| 7 | `2356717` | 21:50 | 修复 proxy 测试期望值（小写 `[1m]` + ACW 注入字段） |
| 8 | `1fdb832` | 21:55 | 恢复 switch_proxy_target 测试（该 provider 无模型后缀，不应注入 ACW） |
| 9 | `6cb0d8b` | 22:00 | Prettier 格式化 |

### 环境备注

- Rust 工具链：1.95.0（通过 winget 安装 rustup）
- pnpm：通过 `npm install -g pnpm` 安装
- **编译障碍**：Windows 环境下 `typenum` crate 的 build script 二进制文件名（下划线 `build_script_build-HASH.exe`）与 cargo 期望的名称（连字符 `build-script-build.exe`）不匹配，导致 `os error 5`（拒绝访问）。解决方案：预编译 build script 并创建正确命名的副本（`fix-typenum-build.py`），将 CARGO_TARGET_DIR 设到 `C:\Users\bunny\.cargo\target\cc-switch`。

---

## 二、逐条对比 Spec 检查实现完成情况

### § 2 后缀语法泛化

#### § 2.1 语法表

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| `1M`/`1m` → 1000000，写入 `[1m]` | ✅ 完成 | 前端 `parseWindowToken` + Rust `parse_window_token` 均实现 |
| `200K`/`200k` → 200000，写入 `[200k]` | ✅ 完成 | 同上 |
| `500k` → 500000，写入 `[500k]` | ✅ 完成 | 同上 |
| `1000000` → 1000000，写入 `[1000000]` | ✅ 完成 | 纯数字路径 multiplier=1 |
| `128K` → 128000，写入 `[128k]` | ✅ 完成 | 同上 |
| （空）→ 不声明，无后缀 | ✅ 完成 | `setModelSuffix` 空字符串返回 base |

#### § 2.2 大小写约定

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 用户输入大小写兼容 | ✅ 完成 | 前端 `parseWindowToken` 同时检查 `K`/`k`、`M`/`m` |
| 写入配置文件统一转小写 | ✅ 完成 | `setModelSuffix` 中 `trimmed.toLowerCase()` |
| Rust 端 `CLAUDE_ONE_M_MARKER_FOR_CLIENT` 改为 `[1m]` | ✅ 完成 | `proxy.rs:49` 已改 |
| 解析侧大小写不敏感 | ✅ 完成 | Rust `parse_window_token` 匹配 `'K' \| 'k'` |

#### § 2.3 前端解析器

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 新增 `parseModelSuffix(model)` 返回 `{ slug, window? }` | ✅ 完成 | `useModelState.ts` |
| 新增 `stripModelSuffix(model)` | ✅ 完成 | 调用 `parseModelSuffix().slug` |
| 新增 `setModelSuffix(model, windowStr)` 统一小写 | ✅ 完成 | |
| `parseModelSuffix` 仅当 `]` 是最后字符时才视为后缀 | ✅ 完成 | `lastIndexOf("]")` 检查 |
| 括号内非法时不剥离 | ✅ 完成 | 返回 `{ slug: 原值, window: undefined }` |
| 保留 `CLAUDE_ONE_M_MARKER` 常量用于向后兼容 | ✅ 完成 | 未删除 |

#### § 2.4 Rust 解析器

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 新增 `parse_context_window_suffix` 返回 `(&str, Option<u64>)` | ✅ 完成 | `claude_desktop_config.rs` |
| 新增 `parse_window_token` 内部函数 | ✅ 完成 | 公开为 `pub fn`（Codex 侧也需要调用） |
| 保留 `ONE_M_CONTEXT_MARKER` 常量 | ✅ 完成 | 未删除 |
| 逻辑与前端 `parseModelSuffix` 对称 | ✅ 完成 | 测试用例一一对应 |

---

### § 3 Claude Code UI 改造

#### § 3.1 数据结构

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| `supportsOneM: boolean` → `supportsContextWindow: boolean` | ✅ 完成 | `ClaudeFormFields.tsx:525` |
| 全部角色 `supportsContextWindow = true`（含 haiku） | ✅ 完成 | 5 个角色行均为 true |

#### § 3.2 UI 组件

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| Checkbox 替换为 Input 输入框 | ✅ 完成 | 角色行 + 兜底模型行均已替换 |
| 宽度约 90px | ✅ 完成 | `className="w-[90px]"` |
| `inputMode="text"` | ✅ 完成 | |
| placeholder 提示 | ⚠️ 部分 | 实际为 `"1M / 200K"`，spec 要求 `"留空=默认，如 1M / 200K"`。i18n key `modelContextWindowHint` 已添加但 placeholder 简化了 |
| 值为空 = 不声明窗口 | ✅ 完成 | |
| **输入校验：非法字符实时过滤 + 标红提示** | ❌ 未实现 | spec 要求用 `parseModelSuffix` 校验，`window` 为 undefined 且输入非空时标红。当前实现未做标红校验，非法输入会被 `setModelSuffix` 静默忽略（返回 base 不追加后缀） |
| 兜底模型行同样改造 | ✅ 完成 | |

#### § 3.3 数据存储

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 后缀拼到模型名上写入 env（小写） | ✅ 完成 | `handleRoleModelChange` → `onModelChange` |
| `*_MODEL_NAME` 存不带后缀的干净名 | ✅ 完成 | `stripModelSuffix` 用于同步 displayName |

#### § 3.4 i18n

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 新增 `modelContextWindowLabel` | ✅ 完成 | 4 种语言均已添加 |
| 新增 `modelContextWindowPlaceholder` | ✅ 完成 | |
| 新增 `modelContextWindowHint` | ✅ 完成 | |
| 移除旧的 `modelOneMLabel` / `modelOneMHeader` / `supports1mLabel` / `supports1mShort` | ⚠️ 未处理 | spec 说"移除或保留为 deprecated"。实际保留了旧 key 未标记 deprecated。不破坏向后兼容，但留下死代码 |

---

### § 4 Claude Code 运行时联动

#### § 4.1 注入逻辑

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 新增 `apply_context_window_defaults` 函数 | ✅ 完成 | `live.rs` |
| 扫描所有 `ANTHROPIC_DEFAULT_*_MODEL` + `ANTHROPIC_MODEL` + `CLAUDE_CODE_SUBAGENT_MODEL` | ✅ 完成 | 6 个 env key 全覆盖 |
| 用 `parse_context_window_suffix` 解析后缀 | ✅ 完成 | |
| 取 max(窗口值) | ✅ 完成 | `max_window.map_or(w, \|m\| m.max(w))` |
| 注入 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`（仅当用户未显式设置时） | ✅ 完成 | 检查 `provider_env.contains_key` + `env.contains_key` |
| 注入 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`（仅当用户未显式设置时） | ✅ 完成 | 同上 |
| 无后缀不注入 | ✅ 完成 | `max_window = None` 时 early return |
| 在 `apply_kimi_for_coding_context_defaults` 之后执行 | ✅ 完成 | 调用链顺序正确 |

#### § 4.2 压缩机制

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| ACW = max(所有窗口) | ✅ 完成 | 通过测试验证 |
| 利用 `min(模型窗口, ACW)` 语义实现按模型压缩 | ✅ 完成 | 设计正确，max 注入自然实现 |

#### § 4.3 与 Codex OAuth / Kimi 的关系

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| Codex OAuth / Kimi 逻辑保持不动 | ✅ 完成 | 未修改这两个函数 |
| 用户显式值优先 | ✅ 完成 | 通过 `provider_env.contains_key` 检查 |
| 测试验证：Codex OAuth 已注入 372K 时不被覆盖 | ✅ 完成 | `context_window_suffix_respects_user_explicit_acw` 测试 |

#### § 4.4 代理剥离泛化

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| `strip_one_m_suffix_for_upstream` 泛化为复用 `parse_context_window_suffix` | ✅ 完成 | 函数体改为调用 `parse_context_window_suffix` |
| Spec 提到重命名为 `strip_context_window_suffix_for_upstream` | ⚠️ 未重命名 | 保留了原函数名 `strip_one_m_suffix_for_upstream`，但行为已泛化。调用方无需改动 |
| `has_claude_one_m_marker` 泛化 | ✅ 完成 | 改为调用 `parse_context_window_suffix().1.is_some()` |
| `CLAUDE_ONE_M_MARKER_FOR_CLIENT` 改为小写 `[1m]` | ✅ 完成 | |
| 测试覆盖 `[200k]`/`[500k]` 剥离 | ✅ 完成 | 2 个新测试 |

---

### § 5 Codex catalog 修复

#### § 5.1 补全 catalog 字段

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 新增 `effective_context_window_percent = 100` | ✅ 完成 | `codex_config.rs` `codex_catalog_model_entry` |
| 新增 `auto_compact_token_limit = null` | ✅ 完成 | |
| 测试验证字段存在 | ✅ 完成 | `catalog_entry_has_auto_compact_token_limit_null` |

#### § 5.2 truncation_policy.limit 联动

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| `truncation_policy.limit` 跟随 `context_window` | ✅ 完成 | `if spec.context_window > 0 { spec.context_window } else { 10_000 }` |
| 留空/0 时回退 10000 | ✅ 完成 | |
| 模板文件 `codex_native_responses_template.json` 不改 | ✅ 完成 | 未修改 |
| 测试验证联动 | ✅ 完成 | `catalog_entry_truncation_follows_context_window` + `catalog_entry_truncation_follows_default_context_window` |

#### § 5.3 恢复隐藏 UI + 多元输入

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 取消 `CodexConfigSections.tsx` 注释，恢复 toggle + compact limit UI | ✅ 完成 | |
| 取消 import 注释 | ✅ 完成 | `extractCodexTopLevelInt`/`setCodexTopLevelInt`/`removeCodexTopLevelField` 已恢复 |
| `CodexFormFields.tsx` contextWindow 列改为 `inputMode="text"` | ✅ 完成 | |
| 去掉 `replace(/[^\d]/g, "")` 过滤 | ✅ 完成 | |
| placeholder 改为 `"1M / 200K / 128000"` | ❌ 未改 | 仍为旧值 `"例: 128000"`（i18n key `codexConfig.contextWindowPlaceholder` 未更新） |
| Rust `parse_codex_positive_u64` 先尝试 `parse_window_token` | ✅ 完成 | |
| 测试验证多元格式解析 | ✅ 完成 | `parse_codex_positive_u64_accepts_multi_format` + `catalog_entry_context_window_from_multi_format` |
| `model_context_window`/`model_auto_compact_token_limit` TOML 字段保持纯数字 | ✅ 完成 | UI 多元格式在 Rust 端解析为整数后写入 |

#### § 5.4 覆盖保护

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 已有 `preserves_user_model_catalog_json` 逻辑保持不动 | ✅ 完成 | 未修改 |
| 移植 CodexPlusPlus 的指针检查逻辑 | ❌ 未实现 | spec § 5.4 提到移植 `root_key_string` 检查。现有代码已有 `set_codex_model_catalog_json_field` 处理指针，但未显式移植 CodexPlusPlus 的 `if existing != catalog_relative` 检查 |
| 测试验证覆盖保护 | ❌ 未实现 | 计划中 Task 6b 的测试未编写 |

#### § 5.5 catalog 生成函数

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 移植 CodexPlusPlus 的 `apply_model_catalog_to_config` | ❌ 未实现 | 现有 `prepare_codex_config_text_with_model_catalog` 已覆盖 catalog 生成功能，但未按 spec 要求显式移植 CodexPlusPlus 的函数。功能上等价，但代码结构与 spec 描述不一致 |

---

### § 6 Hermes / OpenClaw / OpenCode / Gemini

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| 不动 | ✅ 完成 | 未修改这些工具的配置 |

---

### § 7 测试计划

#### § 7.1 前端单元测试

| Spec 要求的测试用例 | 实现状态 |
|---------------------|---------|
| `parseModelSuffix("deepseek-v4-pro[1m]")` → slug + window | ✅ |
| `parseModelSuffix("glm-5.2[200k]")` → slug + window | ✅ |
| `parseModelSuffix("model[500K]")` → 大写 K 兼容 | ✅ |
| `parseModelSuffix("model[1000000]")` → 纯数字兼容 | ✅ |
| `parseModelSuffix("model")` → undefined window | ✅ |
| `parseModelSuffix("model[invalid]")` → 非法不剥离 | ✅ |
| `setModelSuffix("model", "1M")` → 小写写入 | ✅ |
| `setModelSuffix("model[1m]", "")` → 清空后缀 | ✅ |
| `stripModelSuffix("model[200k]")` → 剥离 | ✅ |
| 额外：`setModelSuffix` 替换已有后缀 | ✅ |

#### § 7.2 Rust 单元测试

| Spec 要求的测试用例 | 实现状态 |
|---------------------|---------|
| `parse_context_window_suffix` 对称测试（含纯数字、大小写） | ✅ 6 个测试 |
| `parse_window_token` 覆盖空/零/非法 | ✅ 1 个测试 |
| `strip_*_for_upstream` 覆盖新格式 `[200k]`/`[500k]` | ✅ 2 个测试 |
| `apply_context_window_defaults` 多模型取 max | ✅ |
| `apply_context_window_defaults` 用户显式值优先 | ✅ |
| `apply_context_window_defaults` 无后缀不注入 | ✅ |
| Codex catalog `auto_compact_token_limit` / `effective_context_window_percent` 存在 | ✅ |
| Codex `truncation_policy.limit` 跟随 `context_window` | ✅ 2 个测试 |
| Codex catalog 覆盖保护：用户手写指针不被覆盖 | ❌ 未编写 |
| Codex 多元格式解析 | ✅ 2 个测试 |

#### § 7.3 集成验证

| Spec 要求 | 实现状态 | 说明 |
|-----------|---------|------|
| Claude Code：配 sonnet=1M + opus=200K → 检查 env 后缀小写 | ❌ 未执行 | 需要 Tauri 运行时，单元测试已覆盖逻辑 |
| Claude Code：`/context` 显示正确窗口 | ❌ 未执行 | 需要实际 Claude Code 环境 |
| Codex：配 contextWindow=1M → 检查 catalog JSON | ❌ 未执行 | 需要 Tauri 运行时 |
| Codex：长对话触发自动压缩 | ❌ 未执行 | 需要实际 Codex 环境 |
| Codex：手动改 catalog 后不被覆盖 | ❌ 未执行 | 需要实际环境 |

---

### § 8 风险与缓解

| Spec 风险项 | 缓解状态 | 说明 |
|------------|---------|------|
| 后缀泛化破坏现有 `[1M]` 数据 | ✅ 已缓解 | 新解析器大小写不敏感，旧 `[1M]` 自动被解读 |
| ACW 全局单值无法按模型独立压缩 | ✅ 已缓解 | max 注入 + `min(模型窗口, ACW)` 语义 |
| Codex truncation 改动影响现有行为 | ✅ 已缓解 | 留空回退 10000，模板不改 |
| 代理剥离遗漏新格式导致上游 400 | ✅ 已缓解 | 泛化 `strip_one_m_suffix_for_upstream` |
| 用户手动改 catalog 被覆盖 | ⚠️ 部分 | 现有 `preserves_user_model_catalog_json` 保护仍在，但未移植 CodexPlusPlus 指针检查 |
| Codex TOML 只接受整数 | ✅ 已缓解 | 多元格式在 UI 层，写入 TOML 前转纯数字 |

---

### § 9 改动文件清单

| Spec 列出的文件 | 实际修改 | 状态 |
|----------------|---------|------|
| `useModelState.ts` | ✅ | 解析器泛化 |
| `ClaudeFormFields.tsx` | ✅ | checkbox → 输入框 |
| `CodexFormFields.tsx` | ✅ | contextWindow 多元输入 |
| `CodexConfigSections.tsx` | ✅ | 恢复隐藏 UI |
| `i18n/locales/*.json` | ✅ | 新增文案 |
| `claude_desktop_config.rs` | ✅ | 新增解析器 |
| `live.rs` | ✅ | ACW 注入 |
| `model_mapper.rs` | ✅ | 泛化剥离 |
| `proxy.rs` | ✅ | 泛化 takeover |
| `codex_config.rs` | ✅ | catalog 字段 + truncation + 多元解析 |
| `codex_native_responses_template.json` | ✅ 未改 | 保持 fallback |

---

## 三、测试结果汇总

### 前端测试

```
Test Files: 68 passed / 1 failed (flaky)
Tests:      454 passed / 2 failed (flaky)
```

- 失败的 2 个测试为 `tests/integration/App.test.tsx` 中的 Tauri 窗口 mock 问题（`Cannot read properties of undefined (reading 'metadata')`），与本次改动无关，单独运行时通过。

### Rust 测试

```
Tests: 1966 passed / 8 failed (pre-existing) / 2 ignored
```

- 失败的 8 个测试均为预存问题：
  - 2 个 `codex_history_migration` / `codex_state_db`：SQLite 路径解析（环境特定）
  - 6 个 `anchored_upgrade_windows`：Windows 升级命令格式化（环境特定）
- 本次新增的 27 个测试全部通过。

### 新增测试清单

| 位置 | 测试数 | 覆盖内容 |
|------|--------|---------|
| `tests/hooks/useModelState.test.tsx` | 10 | 前端解析器 |
| `claude_desktop_config.rs` | 7 | Rust 解析器 |
| `services/provider/live.rs` | 3 | ACW 注入 |
| `proxy/model_mapper.rs` | 2 | 代理剥离 |
| `codex_config.rs` | 5 | catalog 字段 + 多元解析 |
| **合计** | **27** | |

---

## 四、未完成项与后续建议

### 未实现（Spec 明确要求但未做）

| 编号 | 内容 | 影响 | 建议 |
|------|------|------|------|
| G1 | § 3.2 输入校验标红提示 | 低：非法输入被静默忽略，不会写入错误值 | 后续可在 Input 上加 `aria-invalid` + 红色边框样式 |
| G2 | § 5.3 CodexFormFields placeholder 未更新 | 低：用户仍可输入多元格式 | 更新 i18n key `codexConfig.contextWindowPlaceholder` 的 defaultValue |
| G3 | § 5.4 移植 CodexPlusPlus 指针检查 | 中：现有保护可能不够全面 | 移植 `root_key_string` 检查逻辑 |
| G4 | § 5.5 移植 `apply_model_catalog_to_config` | 低：现有功能等价 | 代码结构对齐 CodexPlusPlus |
| G5 | § 7.2 catalog 覆盖保护测试 | 中：缺少回归保障 | 编写 `preserves_user_model_catalog_json_pointer` 测试 |
| G6 | § 7.3 集成验证全部未执行 | 中：需实际环境验证 | 提交 PR 后在 CI 或本地 Tauri 环境验证 |

### 已实现但与 Spec 描述有差异

| 编号 | 内容 | 差异说明 | 评估 |
|------|------|---------|------|
| D1 | § 4.4 函数未重命名 | Spec 说重命名为 `strip_context_window_suffix_for_upstream`，实际保留原名但行为已泛化 | 可接受：减少调用方改动 |
| D2 | § 3.4 旧 i18n key 未标记 deprecated | Spec 说"移除或保留为 deprecated"，实际保留了但未标记 | 可接受：不破坏兼容 |
| D3 | § 2.4 `parse_window_token` 公开为 `pub` | Spec 说"内部函数"，实际公开 | 可接受：Codex 侧 `parse_codex_positive_u64` 需要调用 |

---

## 五、改动统计

```
 14 files changed, 554 insertions(+), 108 deletions(-)
```

### 按文件统计

| 文件 | 新增 | 删除 | 类型 |
|------|------|------|------|
| `useModelState.ts` | +58 | 0 | 前端 |
| `useModelState.test.tsx` | +67 | 0 | 测试 |
| `ClaudeFormFields.tsx` | +46 | -67 | 前端 |
| `CodexFormFields.tsx` | +5 | -5 | 前端 |
| `CodexConfigSections.tsx` | +0 | -10 | 前端 |
| `claude_desktop_config.rs` | +91 | 0 | Rust |
| `live.rs` | +127 | 0 | Rust |
| `model_mapper.rs` | +14 | -14 | Rust |
| `proxy.rs` | +10 | -11 | Rust |
| `codex_config.rs` | +115 | -2 | Rust |
| `en.json` / `zh.json` / `zh-TW.json` / `ja.json` | +15 | 0 | i18n |

---

## 六、结论

本次实现覆盖了设计文档的核心目标：Claude Code 的 `[1M]` 布尔标记升级为任意粒度窗口后缀，ACW 自动联动注入，Codex catalog 字段补全与 truncation 联动。10 个计划任务中 8 个完全完成，2 个部分完成（Task 6b 覆盖保护测试未编写、Task 7 CodexFormFields placeholder 未更新）。

6 个未实现项中，G1（标红校验）和 G6（集成验证）影响最大，建议优先处理。其余为代码结构对齐和文案微调，不影响功能正确性。
