# Claude Code 上下文窗口自动同步设计

> 日期：2026-07-18
> 分支：待定
> 状态：待用户审阅
> 前置参考：[docs/research/2026-07-14-github-issue-research.md](../research/2026-07-14-github-issue-research.md)、[docs/specs/2026-07-14-per-model-context-window-design.md](../specs/2026-07-14-per-model-context-window-design.md)

## 1. 背景与目标

### 1.1 问题

cc-switch 当前为 Claude Code 注入的 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`（ACW）和 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`（MAX）采用"取所有模型后缀窗口的 max"策略，导致：

- 用户在终端 `/model` 切到小窗口模型（如 Kimi 30K）时，/context 仍按 max（1M）显示容量和压缩窗口
- 小窗口模型**永远不会**触发自动压缩（窗口被抬到 1M 后，30K 用量远不到 80%）
- 大窗口模型（如 MiniMax 1M）的实际压缩触发点也偏离用户期望的"窗口 × 80%"

### 1.2 目标

让 Claude Code 终端 `/model` 切换角色时，**自动**同步 `ACW = 当前角色模型窗口 × 80%`、`MAX = 当前角色模型窗口`。`min(ACW, MAX)` 逻辑（实测在 Claude Code 客户端存在）保证 /context 显示正确的 per-model 容量。

### 1.3 非目标

- 不在 CC Switch 代理层重写 /context 响应
- 不修复 `MAX_CONTEXT_TOKENS` 在某些 Claude Code 版本被忽略的问题（实测当前版本有效）
- 不让 `model name` 后缀（如 `[30k]`、`[200k]`）影响 /context 容量（实测后缀不参与容量计算）
- 不修改 `apply_context_window_defaults` / `apply_kimi_for_coding_context_defaults` / `apply_codex_oauth_claude_context_defaults`（保留作为兜底）
- 不解决多 Claude Code 终端 + 不同角色并发使用时的 ACW 错位（这是 Claude Code 客户端 `model` 字段全局共享的架构限制，**不**是 CC Switch 能改的，详见 8.2）

## 2. 核心机制

### 2.1 关键发现（基于实测）

通过用户在 Claude Code 终端的对照实验：

| 实验 | ACW | MAX | 模型 | /context 显示 | 推论 |
|---|---|---|---|---|---|
| A | 200000 | 300000 | Kimi[30k] | 200k tokens | min(200K, 300K) = 200K |
| B | 200000 | 300000 | GLM[200k] | 200k tokens | min(200K, 300K) = 200K |
| C | 200000 | 150000 | GLM[200k] | 150k tokens | min(200K, 150K) = 150K |

**结论**：
1. **实际容量 = min(ACW, MAX)** —— Claude Code 客户端内部做 min 计算
2. **ACW 和 MAX 都是有效 env**，都参与容量计算
3. **model name 后缀不影响容量** —— Kimi[30k] 和 GLM[200k] 同样 env 下显示一致
4. **改 env 需要重启 Claude Code 客户端** -- Claude Code 进程启动时读 settings.json 的 env 注入到进程环境，运行中切换模型不会重读。实测（v2.1.214）：同一会话内 `/model` 切换后 `/context` 仍显示旧窗口值；退出后 `claude --resume` 恢复会话（新进程）才读到新值。早期"立即生效"的结论已修正。

### 2.2 触发器

监听 `~/.claude/settings.json` 顶层 `model` 字段值变化。Claude Code 终端 `/model` 切换角色时**直接写入**该字段（实测），因此它是"用户当前激活角色"的最权威信号。

### 2.3 防循环设计

**所有可能的 settings.json 写源审计**：

| 写源 | 是否改 `model` 字段 | 是否触发处理 |
|---|---|---|
| Claude Code 终端 /model 切换 | 会 | 期望触发 |
| Claude Code 终端改 effortLevel / enabledPlugins 等 | 不会 | 自动跳过 |
| CC Switch `write_live_with_common_config` | 不会 | 自动跳过 |
| 监听器自身写 ACW/MAX | 不会 | 自动跳过 |
| 用户手动编辑（不改 model） | 不会 | 自动跳过 |
| 第三方工具 / 编辑器 | 通常不会 | 自动跳过 |

**核心防循环机制**：以"model 字段值变化"为唯一触发器——所有不涉及 model 字段的写自动被跳过，无需 hash 比对、标志位、debounce 黑科技。

**附加安全网**：200ms debouncer（`notify-debouncer-mini` 自带）平滑短时间多次事件。

### 2.4 写入边界

监听器**只**修改 `env` 子对象里的两个字段：
- `CLAUDE_CODE_AUTO_COMPACT_WINDOW`
- `CLAUDE_CODE_MAX_CONTEXT_TOKENS`

**绝不**触碰：
- settings.json 的其他顶层字段（`effortLevel` / `enabledPlugins` / `mcpServers` / `permissions` / `sandbox` / `statusLine` / `model` 本身）
- `env` 子对象里的其他字段（特别是 `ANTHROPIC_DEFAULT_*_MODEL` / `ANTHROPIC_MODEL` / `CLAUDE_CODE_SUBAGENT_MODEL` 等用户配置项）

## 3. 功能描述

### 3.1 settings.json 监听器

- CC Switch 客户端启动时**后台线程**启动一个监听器，监听 `~/.claude/settings.json` 文件修改事件
- 监听器记录**当前顶层 `model` 字段值**作为 `last_seen_model`
- 每次文件事件触发（200ms debounce 合并后）：
  1. 读最新 settings.json 的顶层 `model` 字段
  2. 跟 `last_seen_model` 比对：
     - 相同 → 跳过（这次事件不是 /model 切换触发的）
     - 不同 → 更新 `last_seen_model = current_model`，进入处理流程
- 处理流程：
  1. 查 provider 的 `autoSyncContextWindow` 开关状态（OFF 则跳过，仍更新 `last_seen_model`）
  2. 把 `model` 字段值映射到对应 env 字段名：
     - `"sonnet"` → `ANTHROPIC_DEFAULT_SONNET_MODEL`
     - `"opus"` → `ANTHROPIC_DEFAULT_OPUS_MODEL`
     - `"fable"` → `ANTHROPIC_DEFAULT_FABLE_MODEL`
     - `"haiku"` → `ANTHROPIC_DEFAULT_HAIKU_MODEL`
     - `"subagent"` → `CLAUDE_CODE_SUBAGENT_MODEL`
  3. 读取该 env 字段值，调用 `parse_context_window_suffix` 解析末尾上下文窗口后缀
  4. 解析成功：写 `CLAUDE_CODE_AUTO_COMPACT_WINDOW = 窗口 × 0.8`（字符串）和 `CLAUDE_CODE_MAX_CONTEXT_TOKENS = 窗口`（字符串）
  5. 解析失败（无后缀 / 字段不存在 / 字段不是字符串）：不写任何 env

### 3.2 自动同步开关 UI

- 位置：CC Switch 客户端 Provider 编辑页（Claude 配置区），"上下文长度"输入框**下方**一行
- 视觉风格：复用 [`EndpointField`](/Users/jarvis/Documents/cc-switch/cc-switch-main/src/components/providers/forms/shared/EndpointField.tsx) "完整 URL"开关的**圆角胶囊样式**（`rounded-full border border-border/70 bg-muted/30 px-2.5 py-1` + icon + 文字 + Switch）
- 默认状态：ON
- 持久化：保存到 provider `settings_config` 新字段 `autoSyncContextWindow: boolean`，默认 `true`
- 行为：ON 监听器按 3.1 处理；OFF 监听器跳过该 provider
- **帮助提示**：圆角胶囊**右侧**加一个 ⓘ 图标（lucide `Info`，尺寸 `h-3.5 w-3.5 text-muted-foreground`），hover 触发 Radix Tooltip 显示详细说明。**不**使用"错位"等术语，**不**暴露"多终端"作为 bug，而是描述为"按最后切换的模型为准"的规则

### 3.3 与现有兜底逻辑的关系

`apply_context_window_defaults` / `apply_kimi_for_coding_context_defaults` / `apply_codex_oauth_claude_context_defaults` **全部保留**，跟监听器**天然互补**：

- **Codex OAuth 场景**（model = "gpt-5.6" 无后缀）：监听器 parse 返回 None 跳过 → `apply_codex_oauth_claude_context_defaults` 注入 372K
- **Kimi for Coding 场景**（model = "kimi-for-coding" 无后缀）：监听器跳过 → `apply_kimi_for_coding_context_defaults` 注入 262K
- **用户开了监听器后切模型**：监听器写一次 ACW/MAX → 下次 `apply_context_window_defaults` 看到 `env.contains_key` 为 true（现有"用户显式值优先"规则）→ 自动跳过
- **用户没装/没开 CC Switch**：监听器不启动 → `apply_context_window_defaults` 跑 max 兜底，行为跟现状一致

## 4. UI 设计

### 4.1 开关位置

Claude Form Fields 的"上下文长度"输入框（90px 宽）下方一行，复用说明文字位置。

### 4.2 视觉风格

**圆角胶囊**（跟 EndpointField "完整 URL" 开关同款）：
- `rounded-full border border-border/70 bg-muted/30 px-2.5 py-1`
- icon：`RefreshCw`（lucide）
- 文字：
  - 中文：自动同步
  - 英文：Auto-sync
  - 日文：自動同期
- `Switch`（受控组件，className `h-5 w-9` 跟 EndpointField 一致）

**帮助图标**（紧贴圆角胶囊右侧）：
- icon：`Info`（lucide `h-3.5 w-3.5 text-muted-foreground`）
- hover 行为：触发 Radix Tooltip（用项目现有的 `Tooltip` / `TooltipTrigger` / `TooltipContent` 组件）
- 点击行为：无（**不**打开弹窗，避免复杂）
- 鼠标移开：tooltip 自动消失

### 4.3 i18n key

- `providerForm.autoSyncContextWindow` = "自动同步" / "Auto-sync" / "自動同期"
- `providerForm.autoSyncContextWindowTooltip`（hover 时显示）：

**中文**：
> 终端内切换模型时，上下文长度和压缩阈值按切换的模型更新配置 json。多 claude 终端使用不同模型，以最后切换模型时的上下文长度作为全局变量。

**英文**：
> Each model switch updates ACW/MAX to the most recent model's window. After switching to 1M, sessions use 1M even if the model name shows 30K. With multiple terminals on different models, the most recent switch wins.

**日文**：
> ターミナルでモデルを切り替えるたび、ACW/MAX が最後の切替モデルに応じて更新されます。1M に切替後、30K モデルでも 1M コンテキストを使用します。複数ターミナルで異なるモデルを使う場合、最後の切替が優先されます。

## 5. 数据流

### 5.1 场景 A：用户首次 /model 切换

```
t=0     用户在 Claude Code 终端 /model 选 Kimi-K2.7-Code[30k]
t=10ms  Claude Code 写 settings.json 顶层 model = "haiku"
t=220ms 监听器回调（200ms debounce）
t=221ms 读 model = "haiku" ≠ last_seen_model None → 更新
t=222ms 查 ANTHROPIC_DEFAULT_HAIKU_MODEL = "Kimi-K2.7-Code[30k]"
t=223ms parse → 30000
t=224ms 写 env.ACW = "24000", env.MAX = "30000"
t=420ms 触发再次（监听器自己写）→ model 没变 → 跳过
t=500ms 用户 /context → Claude Code 读 ACW/MAX → 显示 24k tokens
```

### 5.2 场景 B：用户切回 sonnet

```
t=0     用户 /model 选 MiniMax-M3[1M]
t=220ms 监听器回调
t=221ms model = "sonnet" ≠ last_seen_model "haiku" → 更新
t=222ms 查 ANTHROPIC_DEFAULT_SONNET_MODEL = "MiniMax-M3[1M]"
t=223ms parse → 1000000
t=224ms 写 ACW = "800000", MAX = "1000000"
t=420ms 触发 → model 没变 → 跳过
t=500ms /context → 显示 800k tokens
```

### 5.3 场景 C：用户改其他字段（不触发）

```
t=0     Claude Code 改 effortLevel "xhigh" → "max"
t=200ms 监听器回调
t=201ms model = "sonnet" == last_seen_model "sonnet" → 跳过
```

### 5.4 场景 D：用户关掉开关后切模型

```
t=0     用户在 CC Switch 客户端关掉"自动同步" → 写 provider.autoSyncContextWindow = false
t=100ms 用户 /model 切到 opus
t=300ms 监听器回调
t=301ms model = "opus" ≠ last_seen_model "sonnet" → 更新
t=302ms 查 autoSyncContextWindow = false → 跳过（不写 env）
        （用户之前手动设的 ACW/MAX 保留）
```

### 5.5 场景 E：用户手动改 ACW

```
t=0     settings.json: env.ACW = "24000", model = "haiku"
t=10ms  用户用编辑器改 env.ACW = "18000"（不动 model 字段）
t=210ms 监听器回调
t=211ms model = "haiku" == last_seen_model "haiku" → 跳过
        （用户手动值被尊重，不被 watch 覆盖）
```

### 5.6 场景 F：CC Switch 自己 live 同步

```
t=0     用户在 CC Switch 客户端切换 provider → write_live_with_common_config
t=10ms  该流程写 env.ANTHROPIC_DEFAULT_HAIKU_MODEL = "Kimi-K2.7-Code[30k]"
        （不动 model 字段）
t=210ms 监听器回调
t=211ms model 字段没变 → 跳过
        （不互相干扰）
```

### 5.7 场景 G：Codex OAuth / Kimi 无后缀场景

```
t=0     用户在 Codex OAuth provider 上 /model 切到 haiku
        settings.json 顶层 model = "haiku"
        provider.env.ANTHROPIC_DEFAULT_HAIKU_MODEL = "gpt-5.6"（无后缀）
t=220ms 监听器回调
t=221ms model = "haiku" ≠ last_seen_model → 更新
t=222ms parse_context_window_suffix("gpt-5.6") → None → 跳过（不写 ACW/MAX）
t=300ms apply_codex_oauth_claude_context_defaults 跑 → 注入 ACW=372000, MAX=372000
        （保留原有兜底逻辑，watch 不干预）
```

## 6. 错误处理

监听器全程在后台线程跑，**不会**阻塞 Tauri 主线程或前端。所有错误遵循"静默失败 + 写日志"原则。

### 6.1 启动阶段

| 场景 | 处理 |
|---|---|
| settings.json 文件不存在 | 监听器**不**启动，log warn，退出。**不**重试。 |
| 文件存在但无读权限 | 同上 |
| notify 库初始化失败 | log error，退出。**不**抛到前端 |

启动失败**不影响** CC Switch 其他功能，watcher 退化到无 watch 现状（依赖兜底逻辑）。

### 6.2 事件处理阶段

每次事件触发**任何**步骤出错都**整体跳过**这次事件，**不**影响后续事件。

| 场景 | 处理 |
|---|---|
| 文件读失败（锁住 / 权限） | 跳过，log debug |
| 文件内容不是合法 JSON | 跳过，log warn |
| 文件内容不是 Object | 跳过，log debug |
| 顶层 `model` 不是字符串 | 跳过 |
| `model` 值不是五个角色之一 | 跳过（合法情况，**不**算错误） |
| 角色对应 env 字段不存在 | 跳过 |
| env 字段值不是字符串 | 跳过 |
| env 字段值无上下文窗口后缀 | 跳过 |
| `autoSyncContextWindow` 开关 OFF | 跳过（仍更新 last_seen_model） |
| 后台写文件失败 | 跳过，log warn |
| `last_seen_model` 跟当前一致 | 直接 return（正常跳过） |

### 6.3 关闭 / 退出

Tauri 应用退出时优雅关闭 watcher（释放 fs handle，停止后台线程）。关闭失败不阻塞退出。

### 6.4 日志格式

统一前缀 `[ClaudeSettingsWatcher]`：

```
[ClaudeSettingsWatcher] settings.json not found, watcher disabled
[ClaudeSettingsWatcher] watcher init failed: <err>
[ClaudeSettingsWatcher] settings.json invalid JSON
[ClaudeSettingsWatcher] unknown model field value: "<value>"
[ClaudeSettingsWatcher] missing env field ANTHROPIC_DEFAULT_<ROLE>_MODEL
[ClaudeSettingsWatcher] model <name> has no context window suffix
[ClaudeSettingsWatcher] auto-sync disabled for provider <id>, skip
[ClaudeSettingsWatcher] failed to write settings.json: <err>
```

## 7. 测试

### 7.1 单元测试

**位置**：`src-tauri/src/claude_settings_watcher.rs` 同目录 `#[cfg(test)] mod tests`

**核心解析与决策测试**（20 个用例）：

覆盖 5 个角色映射、6 种无效 model 值、6 种无效 env 值、4 种无效后缀、4 种窗口 × 0.8 计算。

### 7.2 防循环测试（5 个用例）

| # | 场景 | 期望 |
|---|---|---|
| 21 | 同一 model 值连续触发 3 次 | 第 2、3 次直接跳过 |
| 22 | model 在两值间反复切换 | 每次都进处理函数 |
| 23 | model → None → 同值 | 1、3 次处理；2 次进函数但跳过 |
| 24 | 监听器自己写入触发 | 跳过（model 没变） |
| 25 | 监听器写入后任何字段变化 | 同上，只看 model 字段 |

### 7.3 文件系统集成测试（6 个用例）

用 `tempfile` crate 创建临时目录，真实 fs 事件：

| # | 场景 | 期望 |
|---|---|---|
| 26 | 启动 watcher，外部程序修改 model 字段 | 回调触发，ACW/MAX 被改 |
| 27 | 启动 watcher，外部程序只改 effortLevel | 回调触发但**不**改 ACW/MAX |
| 28 | 连续 5 次快速修改 | debouncer 合并，最多 1 次进处理 |
| 29 | settings.json 不存在时启动 | 不启动，log warn，**不** panic |
| 30 | 启动后删除 settings.json | 优雅处理，**不**自动重启 |
| 31 | 启动后文件被替换 | 处理新文件 |

### 7.4 端到端手动验证清单

12 步覆盖 5 个角色切换 + 重启 CC Switch 客户端；3 步回归覆盖场景 E（手动改 ACW）、场景 F（CC Switch 改 env）、场景 G（Codex OAuth 兜底）。

**已知限制手动验证（1 步，文档化用）**：
- 开两个 Claude Code 终端，A /model haiku，B /model sonnet → 观察 A 的 /context 显示 sonnet 的 ACW（**预期**：错位，**符合** 8.2 描述）

### 7.5 不测试的东西

- 真实 Claude Code 客户端的 `/model` 行为（黑盒）
- 文件锁的跨平台行为
- 通知线程关闭时的极端 race condition
- 多终端不同角色并发的 per-model 隔离（**已知限制**，见 8.2）

## 8. 风险与缓解

### 8.1 文件锁跨平台差异

**风险**：macOS / Windows / Linux 对 fs 事件通知的实现不同，notify crate 抽象层可能有边缘 case。

**缓解**：7.3 的集成测试覆盖 6 个真实 fs 场景；启动失败优雅降级。

### 8.2 多 Claude Code 终端并发（已知架构限制）

**限制（不是 bug，是 Claude Code 客户端架构）**：

`~/.claude/settings.json` 是**单文件全局共享**，顶层 `model` 字段被所有 Claude Code 终端读写。每个终端的"实际当前模型"在**进程内存**里，**不**持久化到 settings.json。

因此**任何**基于 settings.json.model 的机制（包括本 spec 的 watch）都只能看到**最后一个写 model 字段的终端的角色**。错位场景示例：

```
终端 A 启动，/model haiku  → settings.json.model = "haiku"
终端 A 实际在 haiku 跟用户对话
终端 B 启动，/model sonnet → settings.json.model = "sonnet"  ← 覆盖
CC Switch watch 触发 → 写 ACW = 800K（sonnet 窗口）
终端 A 实际还在 haiku（进程内状态没变），但 /context 显示 800K
→ 终端 A 看到的容量跟自己实际模型不匹配
```

**适用范围**：
- **单终端使用**（绝大多数场景）→ watch 工作完美
- **多终端 + 所有终端用同一角色**（如都 Sonnet）→ watch 工作正常
- **多终端 + 不同角色同时使用** → watch 永远跟"最后 /model 那个终端"同步，**不**保证跟其他终端的实际模型匹配

**缓解**：spec 不试图解决此限制（需要 Claude Code 客户端改为 per-terminal 的 model 字段才能根治，**不**是 CC Switch 能改的）。在 spec 范围外说明：

- 用户文档建议"使用 watch 时避免多终端 + 不同角色"
- 真正需要多终端 per-model 隔离的用户，可关闭 watch 开关（autoSyncContextWindow = false），让每个终端保留自己的 env

**为什么不检测多终端**：CC Switch 客户端**无法可靠检测**有多少个 Claude Code 进程在跑（osascript / pgrep 都不可靠），为此引入检测机制成本高、收益小，**不**进 spec 范围。

### 8.3 settings.json 体积膨胀

**风险**：监听器每次切换都重写整个文件，频繁切换可能增加磁盘 IO。

**缓解**：只动 env 子对象两个字段；监听器本身有 debouncer；用户主动切换频率有限。

### 8.4 用户误关开关

**风险**：用户关了"自动同步"，per-model 失效，但用户不知道为什么。

**缓解**：UI 提示文案说清楚"切换模型时自动调整上下文窗口"；开关默认 ON；可观察性靠 Claude Code 终端 /context 验证。

## 9. 实施清单

### 9.1 新增文件

- `src-tauri/src/claude_settings_watcher.rs`（监听器主体 + 单元测试 + 集成测试）

### 9.2 修改文件

- `src-tauri/Cargo.toml`（+1 依赖：`notify-debouncer-mini = "0.4"`）
- `src-tauri/src/lib.rs`（+1 行 `pub mod claude_settings_watcher;`）
- `src-tauri/src/services/provider/live.rs`（在 `build_effective_settings_with_common_config` 末尾**新增** watcher 启动，**不动** `apply_context_window_defaults`）
- `src/components/providers/forms/ClaudeFormFields.tsx`（+1 圆角胶囊 UI + Provider 配置读写）
- `src/i18n/locales/en.json`（+2 key）
- `src/i18n/locales/zh.json`（+2 key）
- `src/i18n/locales/ja.json`（+2 key）

### 9.3 不动文件

- `src-tauri/src/services/provider/live.rs` 里的 `apply_context_window_defaults` / `apply_kimi_for_coding_context_defaults` / `apply_codex_oauth_claude_context_defaults`（**全部保留**）
- `src/components/providers/forms/hooks/useModelState.ts`（已有功能不重写）
- 其他无关文件

### 9.4 新增依赖

- `notify-debouncer-mini = "0.4"`（Rust crate，跨平台文件监听）

### 9.5 移除/废弃

无。

## 10. 后续增强（不进本次实施）

- CC Switch 客户端 UI 提供"当前激活模型"可视化（避免用户在终端 /model 后忘记切回）
- 监听器响应速度优化（事件触发延迟、写入延迟测量）
- 支持多个 settings.json（worktree / 多用户场景）
- 把 `autoSyncContextWindow` 开关同步到 settings.json 顶层（让 Claude Code 客户端自己也能感知）
