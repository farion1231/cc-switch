# 在 Codex 中接入自建 LLM 网关（llm-gateway-go）：保留官方登录，多模型并存

> 适用版本：CC Switch v3.20.4 及以上。本文来自 [2026-09-28 端到端验证日志](https://github.com/farion1231/cc-switch/.../verification/README.md)（CODEX_HOME = `~/.codex`，真实实例）。截图暂缺。

## 这篇攻略解决什么问题

很多团队内部部署了 OpenAI 兼容网关（基于 [llm-gateway-go](https://github.com/kaixuan/llm-gateway-go) 等），把多家上游模型（Anthropic、GLM、Kimi、MiniMax、DeepSeek 等）统一成 OpenAI 协议对外暴露。他们想用 Codex 直接接入这个网关，**同时保留** ChatGPT 官方登录态（官方 App / 远程操作 / 官方插件都依赖它），不让切换到网关供应商时把 ChatGPT OAuth token 抹掉。

CC Switch v3.20.1 起已经把第三方 API Key 永远只写进 `config.toml`、`auth.json` 留给官方登录；本文重点是如何在这样的网关（一个 base_url，多个模型）下：

1. 同时挂多个非 OpenAI 模型，并让 codex 在模型列表里都可见、可选。
2. 切回 OpenAI 官方档仍然能用，不被副作用波及。
3. 不误删通用配置（approval_policy、sandbox_mode、agents 等）。

## 先看结论

推荐流程：

1. 升级到 CC Switch v3.20.4 或更新版本（不要低于这个版，否则 `[model_providers.custom]` 的几项不会被 cc-switch 反提取）。
2. 在 Codex 标签下选择"开轩 LLM 网关"或"本地 LLM 网关 (8782)"预设（如果看不到，请在 `设置 → 自定义预设` 处添加源 `custom`，或在 PR 中向 `src/config/codexProviderPresets.ts` 追加）。
3. 填入对应网关的 API Key，保存并启用。CC Switch 会把 Key 写进 `config.toml` 而不是 `auth.json`，不会动你的官方 ChatGPT OAuth 缓存。
4. 启动一次 Codex，让它拉取 `~/.codex/cc-switch-model-catalog.json` 并把模型挂到 TUI 候选列表。
5. 在 cc-switch 的供应商列表里，"OpenAI Official" / `default` 与"开轩 LLM 网关" / "本地 LLM 网关 (8782)" 是**并列独立**的入口，UI 上点哪个就切到哪个，互不破坏对方状态。

## 架构一览

```
┌──────────────────────────────────────────────────────────┐
│ CC Switch 维护 4 个独立的 codex 供应商：                  │
│  • default          → ChatGPT OAuth 登录（官方档）       │
│  • OpenAI Official  → API key 占位                       │
│  • 开轩 LLM 网关     → https://llm.kxpms.cn/v1（多模型）  │
│  • 本地 LLM 网关(8782)→ http://localhost:8782/v1（多模型） │
│                                                          │
│ UI 切档：DB 标记 is_current → live config 重写           │
│      model_provider / model / model_catalog_json / auth  │
└──────────────────────────────────────────────────────────┘
```

## 端到端验证（2026-09-28）

判据：`codex exec --json` 事件流，必须出现 `item.completed(type=agent_message)` 且其文本精确等于 `TOKEN-<model>`，同时存在 `turn.completed`。

| 网关 | 模型 | 结果 |
|---|---|---|
| kxpms | claude-opus-5 | **PASS** |
| kxpms | claude-opus-4-8 | **PASS** |
| 8782 | claude-opus-5 | **PASS** |
| 8782 | claude-sonnet-5 | **PASS** |

**已知限制（网关侧，非配置问题）**：

- `glm-5.2` / `kimi-k3` 在采集窗口被限流：8782 侧 6 分钟内 264 次 `rate_limit_exceeded`；kxpms 侧长尾 140s。
- `claude-sonnet-5` / `claude-opus-5-5` 在 kxpms 上保持连接不吐字（curl 100s 零输出），是上游凭据问题。
- `deepseek-v4-pro` / `grok-4.7` → 503 provider_unavailable，`qwen*.max` / `gemini-3.x` → 503 no_candidate。

`[profiles.kx-*]` 这种写法**不被 codex 0.158 接受**——它已经迁到 `~/.codex/<name>.config.toml` 独立配置。如果你的 config.toml 里还有遗留的 `[profiles.xx]`，带 `--profile xx` 会直接报：

```
Error loading config.toml: --profile `xx` cannot be used while
config.toml contains legacy `profile = "xx"` or `[profiles.xx]`
```

我们项目里的 `codexProviderPresets.gateways.test.ts` 已把这条契约写死。

## 准备工作

1. CC Switch v3.20.4+。
2. Codex CLI 0.158+（`codex --version`，老版本的 `model_catalog_json` 路径可能没有 v2 表）。
3. 一个自建网关 URL + API Key。**不要在论坛/工单里粘贴真实 Key**。
4. 网关暴露 OpenAI Responses API（`/v1/responses`），不要只暴露 Chat Completions。

## 第一步：先登录 ChatGPT 官方账号

切到 `OpenAI Official` 档（已绑定的官方 OAuth 在 `default` 供应商里），启动 Codex 走一遍官方登录（Free 套餐就行）。这个登录态保存在 `auth.json`，后续切到网关档不会被擦。

## 第二步：添加/选择网关供应商

切到 `Codex` 标签页，选 "开轩 LLM 网关" / "本地 LLM 网关 (8782)" 预设。填 API Key → 保存。**这一动作只写 `config.toml`，不会动 `auth.json`**。

## 第三步：启动 Codex 拉取模型目录

`codex exec --skip-git-repo-check -m claude-opus-5 "ping"` 跑一次，让 Codex 解析并缓存 `~/.codex/cc-switch-model-catalog.json`。目录里的 `slug` 即 `--model` 可选值。

## 第四步：切回 OpenAI 官方档验证不破坏

在 cc-switch UI 点 `OpenAI Official` 或 `default` → `codex exec -m gpt-5.6-sol ...` 应该正常走 ChatGPT 后端（实测 4 个官方档模型都收到认证响应，失败原因是 Plus 用量限制而非配置）。点回网关档继续用 claude-opus-5。

## 排障清单

| 现象 | 排查点 |
|---|---|
| 切档后 ChatGPT 登录态丢失 | 你的 cc-switch 版本 < v3.20.1，第三方 Key 会写进 `auth.json`；升级即可 |
| `[profiles.xx]` legacy 警告 | 把 `[profiles.xx]` 迁到 `~/.codex/xx.config.toml`；见 codex 0.158 release notes |
| 网关档 `--model glm-5.2` 503 | 网关上游 glia 凭据失效或限流；用 `curl -m 10 -X POST <base>/v1/responses` 直接验 |
| 端到端测只有 `Reconnecting 1/5` 5 次重试 | 不要在 `[model_providers.custom]` 下塞 `request_max_retries=2`（实测 0.158 不消费该键，会被认为是虚假配置） |

## 工程契约（开发者视角）

写一个新的自建网关供应商到 `codexProviderPresets.ts` 时：

- `apiFormat: "openai_responses"`（直连，不要让 cc-switch 路由再做 Responses→Chat 转换）。
- `modelCatalog` 里**每个**模型显式给 `contextWindow` / `inputModalities` / `reasoningLevels`（不要让后端猜）。
- 默认 `model = "<catalog 第一行 model>"`。
- 不写 `[profiles.kx-*]` 之类的配置（0.158 不接受）。
- 不写 `request_max_retries` / `stream_max_retries` / `stream_idle_timeout_ms`（0.158 在 `[model_providers.custom]` 层不消费这三键，是虚假功能）。

回归测试：`src/config/codexProviderPresets.gateways.test.ts`（静态断言）+ `src-tauri/src/codex_config.rs::tests::self_hosted_gateway_presets_round_trip_through_catalog_pipeline`（真目录写盘 + 反解 round-trip）。