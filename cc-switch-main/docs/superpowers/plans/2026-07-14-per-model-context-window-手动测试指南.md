# 手动测试指南：每模型上下文窗口 + 自动压缩联动

> 用途：在真实 Tauri 运行时 + 真实 Claude Code / Codex 环境下验证本特性的端到端行为。单元测试已覆盖逻辑，本指南补齐 spec §7.3 列出的集成验证项。
> 前置分支：`feature/per-model-context-window`
> 参考文档：
> - 设计：`docs/specs/2026-07-14-per-model-context-window-design.md`
> - 操作日志：`docs/superpowers/plans/2026-07-14-per-model-context-window-操作日志.md`

## 0. 环境准备

1. 构建 dev 版：在 `cc-switch-main/` 执行 `pnpm dev`（Tauri 热重载）。
2. 准备一个 Claude Code 可用的 provider（如 DeepSeek 或 Anthropic 官方）。
3. 准备一个 Codex 可用的 provider（指向 Codex CLI）。
4. 备份并清空 Codex 相关文件，保证从干净状态开始：
   ```bash
   cp ~/.codex/config.toml ~/.codex/config.toml.bak 2>/dev/null
   rm -f ~/.codex/cc-switch-model-catalog.json
   ```
5. 关键观测点：
   - Claude Code：cc-switch 写入的 env（在 provider 的 settings 配置里），关注 `ANTHROPIC_DEFAULT_*_MODEL`、`CLAUDE_CODE_AUTO_COMPACT_WINDOW`（ACW）、`CLAUDE_CODE_MAX_CONTEXT_TOKENS`。
   - Codex：`~/.codex/config.toml` 的 `model_catalog_json` 字段 + `~/.codex/cc-switch-model-catalog.json` 文件内容。

---

## 1. Claude Code：后缀写入与窗口联动

**目标（spec §7.3-1）**：配 sonnet=1M + opus=200K，检查后缀写入小写、`/context` 显示正确窗口。

### 步骤

1. 在 cc-switch 的 Claude Code provider 表单，把 Sonnet 模型填成 `deepseek-v4-pro`，上下文窗口输入框填 `1M`。
2. Opus 模型填成 `glm-5.2`，窗口输入框填 `200K`。
3. 保存并切换到该 provider。
4. 打开生成的 settings 配置（cc-switch 的"实时配置"面板或对应文件），核对：
   - `ANTHROPIC_DEFAULT_SONNET_MODEL` = `deepseek-v4-pro[1m]`（**小写后缀**）
   - `ANTHROPIC_DEFAULT_OPUS_MODEL` = `glm-5.2[200k]`（**小写后缀**）
   - `CLAUDE_CODE_AUTO_COMPACT_WINDOW` = `1000000`（取 max，1M）
   - `CLAUDE_CODE_MAX_CONTEXT_TOKENS` = `1000000`
5. 打开 Claude Code，执行 `/context`，确认显示的上下文窗口为 1M（对 sonnet）。

### 预期

- 后缀统一小写 `[1m]` / `[200k]`，不能出现大写 `[1M]`（issue #3679）。
- ACW 注入值 = max(1M, 200K) = 1M。
- 用户若手动设过 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`，cc-switch **不覆盖**（显式值优先）。

### 失败信号

- 后缀大写、ACW 缺失、ACW 被 200K 而非 1M 覆盖、`/context` 仍显示 200K。

---

## 2. Claude Code：按模型自动压缩

**目标（spec §7.3-2）**：验证 `min(模型窗口, ACW)` 语义下，每个模型按自己窗口的 ~80% 压缩。

### 步骤

1. 承接步骤 1 的配置（sonnet=1M, opus=200K）。
2. 在 Claude Code 用 sonnet 模型进行长对话，持续累积上下文。
3. 观察上下文百分比，当接近 ~80%（约 800K tokens）时应触发自动压缩。
4. 切到 opus 模型重复，应在 ~160K（200K 的 80%）触发压缩。

### 预期

- sonnet 在 ~800K 压缩，opus 在 ~160K 压缩，证明按模型独立压缩而非全局一刀切。

### 失败信号

- 两个模型都在同一绝对值压缩；或超过 100% 不压缩（issue #4832/#5110 同类症状）。

---

## 3. Codex：catalog 生成正确

**目标（spec §7.3-3）**：配 contextWindow=1M，检查 catalog JSON 的 `context_window`、`auto_compact_token_limit`、`effective_context_window_percent`、`truncation_policy.limit`。

### 步骤

1. 在 cc-switch 的 Codex provider，把某模型 `contextWindow` 列填 `1M`（多元格式）。
2. 保存并切换。
3. 查看 `~/.codex/config.toml`，确认 `model_catalog_json = "cc-switch-model-catalog.json"`。
4. 查看 `~/.codex/cc-switch-model-catalog.json`，找到该模型条目，核对：
   - `context_window` = `1000000`
   - `effective_context_window_percent` = `100`
   - `auto_compact_token_limit` = `null`
   - `truncation_policy.limit` = `1000000`（跟随 context_window，不再是 10000）

### 预期

- 多元格式 `1M` 被解析为 `1000000` 写入 TOML。
- catalog 四个字段齐全且联动正确。

### 失败信号

- `truncation_policy.limit` 仍是硬编码 `10000`（issue #4832 未修）。
- `auto_compact_token_limit` 字段缺失。
- `1M` 没被解析、contextWindow 为空或 0。

---

## 4. Codex：长对话触发自动压缩

**目标（spec §7.3-4）**：验证补全字段后压缩能被触发。

### 步骤

1. 承接步骤 3 配置。
2. 在 Codex 用该模型进行长对话。
3. 观察是否在接近 context_window 时自动压缩，而不是持续累积直到 400/422 错误。

### 预期

- 接近窗口时触发压缩，上下文不无限增长。

### 失败信号

- HTTP 400/422 input exceeds context window（issue #4508/#5110 症状）。
- 上下文只增不减直到崩溃（issue #4051 症状）。

---

## 5. Codex：覆盖保护（本次 G3/G5 重点）

**目标（spec §7.3-5 + §5.4）**：用户手动改 catalog 指针后，cc-switch 重新生成 catalog 时不覆盖该指针。

### 步骤

1. 承接步骤 3，确认 `config.toml` 里有 `model_catalog_json = "cc-switch-model-catalog.json"`。
2. **手动**把 `config.toml` 里的指针改成自定义路径（模拟用户手写）：
   ```bash
   # 用编辑器把 config.toml 里这一行：
   #   model_catalog_json = "cc-switch-model-catalog.json"
   # 改成：
   #   model_catalog_json = "/Users/<你>/.codex/my-custom-catalog.json"
   # 并放一个真实 catalog 文件到该路径
   cp ~/.codex/cc-switch-model-catalog.json ~/.codex/my-custom-catalog.json
   ```
3. 回到 cc-switch，对 Codex provider 做任意一处改动（如改一个模型名）再保存切换，触发 catalog 重新生成。
4. 再次查看 `~/.codex/config.toml`。

### 预期

- `model_catalog_json` **仍为** `/Users/<你>/.codex/my-custom-catalog.json`，**不被** cc-switch 覆盖回 `cc-switch-model-catalog.json`。
- cc-switch 只会刷新指向自己文件名（`cc-switch-model-catalog.json`）的指针；用户手写的不同路径指针保持原样。

### 失败信号

- 指针被覆盖回 `cc-switch-model-catalog.json`（这就是本次 G3 修复要堵的漏洞）。
- 用户手写的 catalog 文件内容被改写。

---

## 6. 回归对照（可选）

对照操作日志"二、逐条对比 Spec"中的状态表，重点关注本次补的两个点：

- **G3**：Some 分支保护——已在单元测试 `set_catalog_json_some_preserves_user_owned_catalog` / `set_catalog_json_some_preserves_user_owned_relative_catalog` / `set_catalog_json_some_overwrites_cc_switch_owned_pointer` 覆盖；手动测试步骤 5 复现真实场景。
- **G5**：上述三个测试为回归保障，防止未来误改回无条件覆盖。

## 7. 测试后清理

```bash
# 还原 config.toml
mv ~/.codex/config.toml.bak ~/.codex/config.toml
# 删除测试用的自定义 catalog
rm -f ~/.codex/my-custom-catalog.json
# cc-switch 自己的 catalog 可保留或删除
rm -f ~/.codex/cc-switch-model-catalog.json
```

## 结果记录模板

| 步骤 | 通过 | 备注 |
|------|------|------|
| 1 后缀写入 + ACW 联动 | ☐ | |
| 2 按模型压缩 | ☐ | |
| 3 Codex catalog 字段 | ☐ | |
| 4 Codex 自动压缩 | ☐ | |
| 5 覆盖保护（G3/G5） | ☐ | |
