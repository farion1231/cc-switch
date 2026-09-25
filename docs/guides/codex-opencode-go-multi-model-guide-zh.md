# 在 Codex 中使用 OpenCode Go 订阅(多模型)本地路由配置指南

> 适用版本:CC Switch 3.16.x 附近。本文根据实践整理,补全官方预设里缺失的 **OpenCode Go × Codex** 适配方法。示例使用去敏数据,请勿泄露真实 API Key。最后更新:2026-06-11。

## 为什么需要这篇指南

[OpenCode Go](https://opencode.ai/docs/zen/go) 是一项低成本订阅,通过统一的 OpenAI 兼容端点 `https://opencode.ai/zen/go/v1` 提供 GLM、Kimi、Qwen、DeepSeek、MiniMax、MiMo 等十余个开源模型。

但把它接到 **Codex** 上有两个障碍:

1. 新版 Codex(CLI / 桌面)只面向 **OpenAI Responses API**(`/responses`),而 OpenCode Go 暴露的是 **Chat Completions**(`/chat/completions`),少数模型还走 Anthropic `/messages`。直接把 Go 的端点填进 Codex,通常会 404 / 400 或流式无法解析。
2. CC Switch 官方预设里 **没有** OpenCode Go 的 Codex 适配,GitHub 与社区也缺少教程。

本文给出一套**通用、可复用**的方法:用 CC Switch 的本地路由,让 Codex 正常使用 OpenCode Go 的全部模型,并支持一键在多个模型间切换。

## 原理

CC Switch 让 Codex 始终连接本机路由(默认 `127.0.0.1:15721`),Codex 仍以 Responses 协议发送请求;路由层根据 provider 的 `apiFormat` 把请求改写为 Chat Completions 发给 OpenCode Go,再把响应转换回 Responses 形态返回给 Codex。

链路:`Codex (responses) → CC Switch 本地路由 → opencode.ai/zen/go/v1/chat/completions → 转回 responses`。

关键开关:provider 的 **API 格式 = OpenAI Chat Completions(需开启路由)**(底层 `meta.apiFormat = "openai_chat"`)。

> **关于模型端点**：OpenCode Go 的文档页按推荐 SDK 列出了 `/chat/completions` 和 `/messages` 两列,但这反映的是**推荐使用的 AI SDK 包**,而非排他的端点。经直接 curl 实测（见下方自检脚本），除 `qwen3.7-max` 外本指南列出的全部模型在 `/chat/completions` 端点均可正常使用，`openai_chat` 就是这些模型的正确配置。`qwen3.7-max` 在 chat 端点返回 `not supported for format oa-compat`，不在本指南覆盖范围；若未来 cc-switch 的 Codex 通路确认支持 Responses→Messages 转换，可补写 messages 通路配置。

## 准备工作

- 已安装 CC Switch,且 Codex 至少运行过一次(`~/.codex/config.toml` 已存在)。
- 一个 OpenCode Go 的 API Key(在 opencode.ai 订阅 Go 后于控制台获取)。

## 配置步骤

### 1. 在 CC Switch 添加 Codex 供应商

CC Switch → 顶部切到 **Codex** 标签 → 右上角 **+** → 选择 **自定义(Custom)**,填写:

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

### 2. 开启本地路由并接管 Codex

设置 → **路由** → 展开 **本地路由**:

1. 打开**路由总开关**(默认 `127.0.0.1:15721`)。
2. 在**路由启用**中打开 **Codex**。

### 3. 启用供应商并重启 Codex

回到 Codex 供应商列表,点该供应商的**启用**。然后**重启 Codex 会话**(`config.toml`、`model_catalog_json` 通常需要新进程才会刷新)。

## 模型 ID 对照表(关键:必须用 ID,不能用显示名)

| 显示名 | 模型 ID(填这个) | 上下文 | 多模态输入 | `/chat/completions` |
|---|---|---|---|---|
| DeepSeek V4 Flash | `deepseek-v4-flash` | 1M | 纯文本 | ✅ 实测可用 |
| DeepSeek V4 Pro | `deepseek-v4-pro` | 1M | 纯文本 | ✅ 实测可用 |
| Qwen3.7 Plus | `qwen3.7-plus` | 1M | 图 + 视频 | ✅ 实测可用 |
| Qwen3.7 Max | `qwen3.7-max` | 1M | 纯文本 | ❌ `oa-compat` 不支持 |
| Qwen3.6 Plus | `qwen3.6-plus` | 1M | 图 + 视频 | ✅ 实测可用 |
| GLM-5.1 | `glm-5.1` | 200k | 纯文本 | ✅ 实测可用 |
| GLM-5 | `glm-5` | 200k | 纯文本 | ✅ 实测可用 |
| Kimi K2.6 | `kimi-k2.6` | 262k | 图 + 视频 | ✅ 实测可用 |
| Kimi K2.5 | `kimi-k2.5` | 262k | 图 + 视频 | ✅ 实测可用 |
| MiniMax M3 | `minimax-m3` | 512k | 图 + 视频 | ✅ 实测可用 |
| MiniMax M2.7 / M2.5 | `minimax-m2.7` / `minimax-m2.5` | 204k | 纯文本 | ✅ 实测可用 |
| MiMo V2.5 | `mimo-v2.5` | 1M | 图 + 音 + 视频 | ✅ 实测可用 |
| MiMo V2.5 Pro | `mimo-v2.5-pro` | 1M | 纯文本 | ✅ 实测可用 |

> 完整、最新列表见 `https://opencode.ai/zen/go/v1/models`;模型规格(上下文/模态/价格)可参考 models.dev 的 `opencode-go` provider。
>
> **关于 `qwen3.7-max`**：该模型在 `/chat/completions` 端点返回 `Model qwen3.7-max is not supported for format oa-compat`，当前 cc-switch 的 Codex 通路仅支持 chat 转换,因此 `qwen3.7-max` 不在本指南覆盖范围。OpenCode Go 文档中 `/messages` 列对应推荐 SDK 包,不代表 `/chat/completions` 必定不可用——上表已对每个模型做了直接 curl 验证,标有 ✅ 的均可放心用 `openai_chat` 配置。
>
> **自检脚本**：不确定某个模型是否能走 chat 通路时,直接用 curl 打一下:
> ```bash
> curl -s https://opencode.ai/zen/go/v1/chat/completions \
>   -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
>   -d '{"model":"<模型ID>","messages":[{"role":"user","content":"hi"}],"max_tokens":1}'
> # 若返回 "not supported for format oa-compat",该模型不能走 openai_chat
> ```

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

### 坑 4:MiniMax(M2/M3)的 thinking.type 限制
- MiniMax 只接受 `thinking.type` 为 `adaptive` 或 `disabled`;而 CC Switch 的 chat 通路默认发 `enabled`,导致 `HTTP 400: invalid params, invalid thinking.type "enabled"`。
- 临时规避:把该供应商的 thinking 关掉(`meta.codexChatReasoning.supportsThinking=false`、`supportsEffort=false`,并移除 `model_reasoning_effort`)。
- **但**:MiniMax 官方说明 M2/M3 依赖**交错思考(interleaved thinking)**才能可靠地做多步 agent;关掉 thinking 后会出现"**回一句就停、需要不停手动让它继续**"的现象。
- 结论:MiniMax 系列在 Codex 的 agent 场景下**不适合长 agent 循环**;需要连续工具调用的 agent 任务,建议用 **DeepSeek / GLM / Kimi**。MiniMax 更适合一次性/读图场景。

### 坑 5:对话级模型固定
改了默认模型后,**已有对话仍使用其创建时的模型**(并保留其上下文)。切模型后请**新开一个 Codex 对话**,否则可能出现"模型不匹配 / context window exceeds limit"等报错。

## 推荐的供应商分层

为兼顾成本、上下文与能力,建议建立多个供应商按需切换:

| 供应商 | 模型 | 定位 |
|---|---|---|
| DeepSeek V4 Flash(主力) | `deepseek-v4-flash` | 默认主力:1M 上下文、最便宜、额度最多 |
| DeepSeek V4 Pro | `deepseek-v4-pro` | 更强的 agent(质量更高、成本约 9×) |
| Qwen3.7 Plus(视觉) | `qwen3.7-plus` | 涉及图片时切换读取 |
| Kimi K2.6(视觉) | `kimi-k2.6` | 视觉 + 长上下文(262k) |
| GLM-5.1(高端) | `glm-5.1` | 难规划 / 审查(注意仅 200k 上下文) |

成本权衡:Flash 与 MiMo V2.5 的缓存读价格极低($0.0028/1M),在 agent 这种反复重发上下文的场景下最划算;V4 Pro / GLM-5.1 更强但额度消耗快很多。

## 验证

```bash
# 1) 逐模型自检——直接测 /chat/completions 端点(替换 KEY 和模型 ID)
curl -s https://opencode.ai/zen/go/v1/chat/completions \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-flash","messages":[{"role":"user","content":"reply only: 42"}],"max_tokens":50}'
# 若返回正常 JSON 则可用;若返回 "not supported for format oa-compat" 则该模型不能走 openai_chat

# 2) 在 Codex 新对话发一个简单问题,观察 CC Switch 路由面板请求数 +1
```

## 常见报错速查

| 报错 | 原因 | 解决 |
|---|---|---|
| `404 /responses` | 直接把 Go 端点填给 Codex,未开路由 | 开启 Codex 接管,base_url 指向 `127.0.0.1:15721/v1` |
| `Model ... is not supported` | 用了显示名而非模型 ID | 改用准确的模型 ID |
| `not supported for format oa-compat` | 该模型不被 `/chat/completions` 接受(如 `qwen3.7-max`) | 换用 chat 通路支持的模型;或等待 cc-switch 支持 Codex→Messages 转换 |
| `invalid thinking.type "enabled"` | MiniMax 不接受 enabled | 关闭该供应商 thinking,或改用 DeepSeek/GLM/Kimi |
| `context window exceeds limit` | 在旧对话/超长上下文里切到小窗口模型 | 新开对话;按模型设 `model_context_window` |
| 回一句就停、需手动继续 | MiniMax 关闭 thinking 后失去交错思考 | agent 任务改用 DeepSeek/GLM/Kimi |

---

*本指南由社区实践整理,欢迎补充 English 版本与更多模型的实测数据。模型 chat 通路可用性经直接 curl 验证(2026-06-11),如有变更请提交 PR 更新。*
