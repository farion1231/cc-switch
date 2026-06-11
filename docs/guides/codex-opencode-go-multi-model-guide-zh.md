# 在 Codex 中使用 OpenCode Go 订阅(多模型)本地路由配置指南

> 适用版本:CC Switch 3.16.x 附近。本文根据实践整理,补全官方预设里缺失的 **OpenCode Go × Codex** 适配方法。示例使用去敏数据,请勿泄露真实 API Key。最后更新:2026-06-11。

## 为什么需要这篇指南

[OpenCode Go](https://opencode.ai/docs/zen/go) 是一项低成本订阅,通过统一的 OpenAI 兼容端点 `https://opencode.ai/zen/go/v1` 提供 GLM、Kimi、Qwen、DeepSeek、MiniMax、MiMo 等十余个开源模型。

但把它接到 **Codex** 上有两个障碍:

1. 新版 Codex(CLI / 桌面)只面向 **OpenAI Responses API**(`/responses`),而 OpenCode Go 暴露的是 **Chat Completions**(`/chat/completions`)和 **Anthropic Messages**(`/messages`)两种端点,少数模型还有特殊的协议行为。直接把 Go 的端点填进 Codex,通常会 404 / 400 或流式无法解析。
2. CC Switch 官方预设里 **没有** OpenCode Go 的 Codex 适配,GitHub 与社区也缺少教程。

本文给出一套**通用、可复用**的方法:用 CC Switch 的本地路由,让 Codex 正常使用 OpenCode Go 的全部模型,并支持一键在多个模型间切换。

## 原理

CC Switch 让 Codex 始终连接本机路由(默认 `127.0.0.1:15721`),Codex 仍以 Responses 协议发送请求;路由层根据 provider 的 `apiFormat` 把请求改写为目标格式,再把响应转换回 Responses 形态返回给 Codex。

**OpenCode Go 的模型分两条上游通路**(见 [OpenCode Go 文档](https://opencode.ai/docs/go/))：

| 通路 | 端点 | 模型 |
|---|---|---|
| Chat Completions | `/zen/go/v1/chat/completions` | DeepSeek、GLM、Kimi、MiMo |
| Messages（Anthropic 风格） | `/zen/go/v1/messages` | Qwen 3.6/3.7、MiniMax |

> **重要**：两路模型必须用不同的 `apiFormat` 来配置，不可混用。错误搭配会导致请求发到错误的上游端点 —— 详见下方配置步骤。

Chat 通路模型的链路：
`Codex (responses) → CC Switch 本地路由 → opencode.ai/zen/go/v1/chat/completions → 转回 responses`。
关键开关：provider 的 **API 格式 = OpenAI Chat Completions（需开启路由）**（底层 `meta.apiFormat = "openai_chat"`）。

Messages 通路模型的链路：
`Codex (responses) → CC Switch 本地路由 → opencode.ai/zen/go/v1/messages → 转回 responses`。
关键开关：provider 的 **API 格式 = Anthropic Messages（需开启路由）**（底层 `meta.apiFormat = "anthropic_messages"`）。

## 准备工作

- 已安装 CC Switch,且 Codex 至少运行过一次(`~/.codex/config.toml` 已存在)。
- 一个 OpenCode Go 的 API Key(在 opencode.ai 订阅 Go 后于控制台获取)。
- 确认要使用的模型属于哪条通路（Chat Completions 还是 Messages），见下方模型对照表。

## 配置步骤

### 1a. 本地路由与 Codex 接管（Chat / Messages 共用）

设置 → **路由** → 展开 **本地路由**:

1. 打开**路由总开关**(默认 `127.0.0.1:15721`)。
2. 在**路由启用**中打开 **Codex**。

### 1b. Chat 通路模型（DeepSeek / GLM / Kimi / MiMo）

**推荐起步方式**——这类模型占 Go 订阅里的大多数，社区实测也最成熟。

CC Switch → 顶部切到 **Codex** 标签 → 右上角 **+** → 选择 **自定义(Custom)**,填写：

- **base_url**:`https://opencode.ai/zen/go/v1`
- **API Key**:你的 OpenCode Go Key
- **API 格式 / apiFormat**:**OpenAI Chat Completions(需开启路由)**
- **默认模型**:填**准确的模型 ID**(见下方对照表,例如 `deepseek-v4-flash`)
- 可点 **Fetch Models(拉取模型)**,CC Switch 会调用 `/v1/models` 自动列出全部可用模型

对应到 `config.toml` 的形态:

```toml
model_provider = "custom"
model = "deepseek-v4-flash"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers]
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://opencode.ai/zen/go/v1"
```

auth 由 CC Switch 在转发时注入,Codex 的 `auth.json` 中是占位符 `PROXY_MANAGED`,无需把真实 Key 暴露给 Codex。

### 1c. Messages 通路模型（Qwen 3.6/3.7 / MiniMax）

对于走 `/zen/go/v1/messages` 的模型，不能用 `openai_chat`，需改为 **Anthropic Messages** 格式：

CC Switch → Codex 标签 → **自定义供应商**,填写：

- **base_url**:`https://opencode.ai/zen/go/v1`
- **API Key**:你的 OpenCode Go Key
- **API 格式 / apiFormat**:**Anthropic Messages（需开启路由）**
- **默认模型**:填模型 ID（例如 `qwen3.7-plus`）

对应 `config.toml` 的差异：

```toml
[model_providers]
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = false           # Anthropic 格式不需要 OpenAI auth
base_url = "https://opencode.ai/zen/go/v1"

[...meta]
apiFormat = "anthropic_messages"       # 关键：不是 openai_chat
```

> ⚠️ **MiniMax 特别说明**：MiniMax M2/M3 在 `/messages` 通路下仍然有 thinking.type 限制（仅接受 `adaptive` 或 `disabled`），交错的 thinking 支持在各厂商间的表现也不一致。在 Codex agent 场景下建议优先使用 DeepSeek / GLM / Kimi，MiniMax 更适合一次性或读图任务。详见下方「坑 5」。
>
> ⚠️ **Qwen 特别说明**：Qwen 3.6/3.7 虽然支持视觉，但在 `/messages` 通路下多模态行为可能与 `/chat/completions` 通路不同。涉及视觉任务时建议先用 curl 验证端点行为。

### 1d. 同时使用两路模型

由于 Go 所有模型共用 `base_url`,你可以为不同通路各建一个供应商（如"Go-Chat"和"Go-Messages"），在 CC Switch 界面里一键切换。

### 2. 启用供应商并重启 Codex

回到 Codex 供应商列表,点该供应商的**启用**。然后**重启 Codex 会话**(`config.toml`、`model_catalog_json` 通常需要新进程才会刷新)。

## 模型 ID 对照表(关键:必须用 ID,不能用显示名)

| 显示名 | 模型 ID(填这个) | 端点通路 | 上下文 | 多模态输入 |
|---|---|---|---|---|
| DeepSeek V4 Flash | `deepseek-v4-flash` | Chat Completions | 1M | 纯文本 |
| DeepSeek V4 Pro | `deepseek-v4-pro` | Chat Completions | 1M | 纯文本 |
| GLM-5.1 | `glm-5.1` | Chat Completions | 200k | 纯文本 |
| GLM-5 | `glm-5` | Chat Completions | 200k | 纯文本 |
| Kimi K2.6 | `kimi-k2.6` | Chat Completions | 262k | 图 + 视频 |
| Kimi K2.5 | `kimi-k2.5` | Chat Completions | 262k | 图 + 视频 |
| MiMo V2.5 | `mimo-v2.5` | Chat Completions | 1M | 图 + 音 + 视频 |
| MiMo V2.5 Pro | `mimo-v2.5-pro` | Chat Completions | 1M | 纯文本 |
| Qwen3.7 Plus | `qwen3.7-plus` | Messages | 1M | 图 + 视频 |
| Qwen3.7 Max | `qwen3.7-max` | Messages | 1M | 纯文本 |
| Qwen3.6 Plus | `qwen3.6-plus` | Messages | 1M | 图 + 视频 |
| MiniMax M3 | `minimax-m3` | Messages | 512k | 图 + 视频 |
| MiniMax M2.7 / M2.5 | `minimax-m2.7` / `minimax-m2.5` | Messages | 204k | 纯文本 |

> 完整、最新列表见 `https://opencode.ai/zen/go/v1/models`;模型规格(上下文/模态/价格)可参考 models.dev 的 `opencode-go` provider。
> **端点通路信息以 OpenCode 官方文档为准**：[opencode.ai/docs/go](https://opencode.ai/docs/go/)。模型通路可能随上游调整而变化。

## 我们踩过的坑与解决办法

### 坑 1:模型必须用 ID,不能用显示名
把默认模型填成 `Qwen3.7 Plus`(带空格的显示名)会被上游拒绝:`Model not supported`。必须用 `qwen3.7-plus`。CC Switch 的"显示名"只影响界面,**实际请求用的是模型 ID**。

### 坑 2:上下文窗口默认只有 128k,需手动放开
CC Switch 生成的模型目录默认把每个模型的 `context_window` 设为 128000,导致即使 DeepSeek/Qwen 是 1M,Codex 也只当 128k 用。

解决:用添加 Codex 供应商时的 **"Enable 1M Context Window"** 开关,或在 `config.toml` 手动加(按模型真实上限设置):

```toml
model_context_window = 1000000
model_auto_compact_token_limit = 900000
```

### 坑 3:多模型管理与切换时机
OpenCode Go 所有模型共用一个端点,因此可以**为每个常用模型各建一个供应商**(或一个供应商挂多模型目录),用 CC Switch 一键切换。

注意:CC Switch **只在你于界面里点击切换供应商时**才会重新生成 `config.toml` 和 `cc-switch-model-catalog.json`;**单纯重启 CC Switch 不会刷新**这两个文件。所以改完配置后,务必在界面里点一下目标供应商,再新开 Codex 对话。

### 坑 4:端点通路不能混用 —— Chat 模型走 Chat, Messages 模型走 Messages
Go 的模型分 `/chat/completions` 和 `/messages` 两条上游通路。如果把 Messages 通路的模型（Qwen 3.6/3.7、MiniMax）配成 `openai_chat`，CC Switch 会把请求路由到 `/chat/completions`，而这些模型在 `/chat/completions` 上不可用，会报 404 或 "Model not supported"。

反之，Chat 通路模型走 `anthropic_messages` 同样不可用。**必须按模型对照表中的"端点通路"列选择对应的 apiFormat**。

### 坑 5:MiniMax(M2/M3)的 thinking.type 限制
- MiniMax 只接受 `thinking.type` 为 `adaptive` 或 `disabled`;而 CC Switch 的 chat/messages 通路默认发 `enabled`,导致 `HTTP 400: invalid params, invalid thinking.type "enabled"`。
- 临时规避:把该供应商的 thinking 关掉(`meta.codexChatReasoning.supportsThinking=false`、`supportsEffort=false`,并移除 `model_reasoning_effort`)。
- **但**:MiniMax 官方说明 M2/M3 依赖**交错思考(interleaved thinking)**才能可靠地做多步 agent;关掉 thinking 后会出现"**回一句就停、需要不停手动让它继续**"的现象。
- 结论:MiniMax 系列在 Codex 的 agent 场景下**不适合长 agent 循环**;需要连续工具调用的 agent 任务,建议用 **DeepSeek / GLM / Kimi**。MiniMax 更适合一次性/读图场景。

### 坑 6:对话级模型固定
改了默认模型后,**已有对话仍使用其创建时的模型**(并保留其上下文)。切模型后请**新开一个 Codex 对话**,否则可能出现"模型不匹配 / context window exceeds limit"等报错。

## 推荐的供应商分层

为兼顾成本、上下文与能力,建议建立多个供应商按需切换:

| 供应商名 | 通路 | 默认模型 | 定位 |
|---|---|---|---|
| Go Chat - DS Flash（主力） | Chat Completions | `deepseek-v4-flash` | 默认主力:1M 上下文、最便宜 |
| Go Chat - DS Pro | Chat Completions | `deepseek-v4-pro` | 强 agent（质量更高，成本约 9×） |
| Go Chat - GLM 5.1 | Chat Completions | `glm-5.1` | 难规划 / 审查（注意仅 200k） |
| Go Chat - Kimi（视觉） | Chat Completions | `kimi-k2.6` | 需要视觉 + 长上下文时 |
| Go Messages - Qwen（视觉） | Messages | `qwen3.7-plus` | Qwen 走 Messages 通路；视觉场景可用 |

成本权衡:Flash 与 MiMo V2.5 的缓存读价格极低($0.0028/1M),在 agent 这种反复重发上下文的场景下最划算;V4 Pro / GLM-5.1 更强但额度消耗快很多。

## 验证

```bash
# 1) Chat 通路模型测试（替换 KEY）
curl -s https://opencode.ai/zen/go/v1/chat/completions \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-flash","messages":[{"role":"user","content":"reply only: 42"}],"max_tokens":50}'

# 2) Messages 通路模型测试（替换 KEY）
curl -s https://opencode.ai/zen/go/v1/messages \
  -H "x-api-key: $KEY" -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"qwen3.7-plus","max_tokens":50,"messages":[{"role":"user","content":"reply only: 42"}]}'

# 3) 在 Codex 新对话发一个简单问题,观察 CC Switch 路由面板请求数 +1
```

## 常见报错速查

| 报错 | 原因 | 解决 |
|---|---|---|
| `404 /chat/completions`（Messages 通路模型） | 把 Messages 通路模型配成了 `openai_chat` | 改用 `anthropic_messages` 格式（参见配置步骤 1c） |
| `404 /responses` | 直接把 Go 端点填给 Codex,未开路由 | 开启 Codex 接管,base_url 指向 `127.0.0.1:15721/v1` |
| `Model ... is not supported` | 用了显示名而非模型 ID | 改用准确的模型 ID |
| `Model ... is not supported`（已用 ID 仍报错） | 端点通路不匹配 | 检查模型属于哪个通路,切换 apiFormat |
| `invalid thinking.type "enabled"` | MiniMax 不接受 enabled | 关闭该供应商 thinking,或改用其它模型 |
| `context window exceeds limit` | 在旧对话/超长上下文里切到小窗口模型 | 新开对话;按模型设 `model_context_window` |
| 回一句就停、需手动继续 | MiniMax 关闭 thinking 后失去交错思考 | agent 任务改用 DeepSeek/GLM/Kimi |

---

*本指南由社区实践整理,欢迎补充 English 版本与更多模型的实测数据。端点通路信息基于 OpenCode 官方文档与社区实测,如有变更请提交 PR 更新。*
