# GitHub Copilot CLI 接入

GitHub Copilot CLI 是独立应用，应用 ID 为 `copilot`（兼容 `copilot-cli`、`github-copilot`），对应 npm 包 `@github/copilot`、命令 `copilot`。数据目录为 `~/.copilot`，设置 `COPILOT_HOME` 后全部跟随该目录（CC Switch 进程需要继承这个环境变量）。

以下格式均以本机 Copilot CLI 1.0.103 的 `copilot help providers`、`copilot help environment`、`copilot help config` 和隔离 `COPILOT_HOME` 实测为准。

## 使用方式

1. 在「应用」页安装 GitHub Copilot CLI（`npm i -g @github/copilot@latest`；Homebrew cask `copilot-cli` 的安装也能识别和卸载）。
2. 在侧栏选择 **Copilot CLI**，点击「添加供应商」：
   - **GitHub Copilot（官方）**：用 Copilot CLI 自己的 GitHub 登录。可选模型（以 `--model` 传入，只对这次会话生效）；可以绑定授权中心里的 GitHub 账号，打开终端时经 `COPILOT_GITHUB_TOKEN` 只给这一个进程使用该账号，卡片同时显示该账号的 Premium 额度。
   - **自带模型（BYOK）**：从「模型厂商」「第三方平台」「云服务商」「本地」中选预设（复用 Pi 的供应商目录，换算为 Copilot 的协议字段），或「自定义配置」。选择接口格式（OpenAI Chat Completions、OpenAI Responses、Anthropic Messages、Azure OpenAI 的 Chat Completions / Responses，一项同时决定 `COPILOT_PROVIDER_TYPE` 和 `COPILOT_PROVIDER_WIRE_API`），填写地址、Key 和模型；「获取模型」从地址的 `/models` 读取可用模型。
3. 卡片只有「打开终端」，没有「启用」。从卡片打开的终端按这张卡启动 `copilot`。

## 为什么没有「启用」

结论：Copilot CLI 只能用启动形式，不能像 Kimi、OpenCode 那样以添加（共存）形式写进原生配置。依据（Copilot CLI 1.0.103）：

- `copilot help config` 列出的 `settings.json` 设置里没有任何供应商、地址或 Key 字段；`copilot help providers` 只给出 `COPILOT_PROVIDER_*` 环境变量。
- 实测在 `settings.json` 写 `"env": { "COPILOT_PROVIDER_BASE_URL": … }` 无效：只靠它启动时 CLI 仍提示需要设置 `COPILOT_PROVIDER_BASE_URL`。
- CLI 内置的「Provider discovery」只发现扩展、Foundry Local 等运行时提供的模型，没有用户可写的持久化 BYOK 配置；SDK / ACP 会话的 `provider` 参数也只对单个会话有效。

Copilot CLI 的自带模型只能经进程环境变量 `COPILOT_PROVIDER_*` 配置，没有对应的配置文件字段；写进 shell 启动文件会影响所有终端，也会让 Key 常驻磁盘。所以与 Antigravity 相同：

- 卡片里存的就是这些环境变量：`{ "env": { "COPILOT_PROVIDER_BASE_URL": …, "COPILOT_PROVIDER_TYPE": …, "COPILOT_PROVIDER_WIRE_API": …, "COPILOT_PROVIDER_API_KEY": …, "COPILOT_MODEL": … } }`。其他 `COPILOT_PROVIDER_*`（`BEARER_TOKEN`、`MODEL_ID`、`WIRE_MODEL`、`MAX_PROMPT_TOKENS`、`MAX_OUTPUT_TOKENS`、`HEADERS`、`TRANSPORT`、`AZURE_API_VERSION`）可在环境变量 JSON 里填写。
- 「打开终端」：macOS / Linux 的启动脚本先读入一个 `0600` 的临时文件，读完立即删除，再经 `env` 把变量交给这一个 copilot 进程；Windows 写进 `.bat` 的 `setlocal` 段。Key 不出现在命令行参数里，也不写进任何常驻文件。卡上没填的同名变量会被 `env -u` 清掉，避免与用户 rc 里 export 的值混用；官方卡会清掉全部 `COPILOT_PROVIDER_*`，防止 rc 里的 BYOK 变量把它悄悄变成自带模型。
- `COPILOT_PROVIDER_HEADERS` 的多行值在传给进程前转成字面量 `\n` 分隔（Copilot CLI 同样认这种写法），以便 Windows 批处理也能承载。
- 卡片可选「推理强度」（`settingsConfig.reasoningEffort`：none / minimal / low / medium / high / xhigh / max），「打开终端」时作为 `--reasoning-effort` 传入；Copilot CLI 没有对应的环境变量或设置项。Responses 接口下 Copilot 默认在请求里带 `reasoning.summary`，不认这个字段的中转会返回 400（如 `unknown field "summary"`）；选 `none` 时不带这个字段（实测本地中转 + DeepSeek 可正常回复）。
- `~/.copilot/settings.json` 不被供应商功能改写；直接运行 `copilot` 永远是 Copilot 自己的登录和 `/model` 选择。

保存前校验：值必须是字符串、不能含控制字符（`HEADERS` 允许换行）；有凭据或选项时必须有地址；自带模型必须有模型（`COPILOT_MODEL`、`COPILOT_PROVIDER_MODEL_ID` 或 `COPILOT_PROVIDER_WIRE_MODEL`）；协议、接口、传输取值与 token 上限必须是 Copilot CLI 认的值。

已有 `settings.json` 带 `model` 时，首次进入会导入为一张 `default` 官方卡；只有界面设置的 `settings.json` 不导入。

## 配置与能力

| 能力       | 原生位置 / 行为                                                                                                                                                                                                                                                                                                   |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 供应商     | 仅「打开终端」时经进程环境传入，不落盘                                                                                                                                                                                                                                                                            |
| 官方账号   | Copilot CLI 自己的登录；或绑定授权中心 GitHub 账号，经 `COPILOT_GITHUB_TOKEN` / `COPILOT_GH_HOST` 传入                                                                                                                                                                                                            |
| 官方额度   | 卡片显示 GitHub Copilot 的 Premium 额度，在卡片「更多 → 配置用量查询」里开关（只有 Copilot 额度一种模板）。绑了授权中心账号的卡默认打开并查该账号；没绑的卡默认关闭，打开后读 `~/.copilot/config.json` 的 `lastLoggedInUser`，按用户名和域名在授权中心找对应账号来查；找不到或 Copilot CLI 未登录时卡片上提示原因 |
| MCP        | `~/.copilot/mcp-config.json`：stdio 写为 `"type": "local"`，另有 `http`、`sse`；新条目带 `"tools": ["*"]`，已有条目的 `tools`、`timeout` 保留。停用时移除条目；导入时 `settings.json` 的 `disabledMcpServers` 记为未启用                                                                                          |
| Skills     | `~/.copilot/skills/`，与 Kimi 相同的原生目录保护（同名外部目录不覆盖、不删除）                                                                                                                                                                                                                                    |
| 全局提示词 | `~/.copilot/copilot-instructions.md`（`copilot instruction list` 中的 Personal instructions）                                                                                                                                                                                                                     |
| 会话       | `~/.copilot/session-state/<id>/events.jsonl` 与 `workspace.yaml`；旧版本没有 `workspace.yaml` 时退回 `session.start`                                                                                                                                                                                              |
| 恢复会话   | `copilot --resume=<id>`                                                                                                                                                                                                                                                                                           |
| 用量       | 导入 `session.usage_record`（新版本才写出）；按项目价格表估算费用                                                                                                                                                                                                                                                 |
| 连通性检查 | 自带模型卡检查 `COPILOT_PROVIDER_BASE_URL`；官方卡不提供                                                                                                                                                                                                                                                          |
| 深链接     | `resource=provider&app=copilot`，`apiFormat` 支持 `openai_chat`、`openai_responses`、`anthropic`；`config` 可为 `{env:{…}}` 或扁平 `COPILOT_*`，只取地址、Key、模型；导入后只保存，不启用。MCP 用 `apps=copilot`                                                                                                  |

## 会话与 Token 语义

- 主会话只展示 `user.message`、`assistant.message`（含 `reasoningText`、`toolRequests`）、`tool.execution_complete` 和压缩摘要；带 `parentToolCallId` 的子代理内部消息不进主会话，`report_intent` 意图标签不作为工具调用显示。
- `session.usage_record` 在对应回复之前写出，会话里挂到下一条回复上。`inputTokens` 包含缓存命中：新输入取 `copilotUsage.tokenDetails` 的 `input`，没有时用 `inputTokens − cacheReadTokens − cacheWriteTokens`。耗时、首字时间取 `duration`、`timeToFirstTokenMs`。
- 用量按 `copilot:<会话>:<事件 ID>` 去重，去重账本与明细同事务提交；未变化的文件按大小和修改时间跳过，写到一半的尾行下次再读。旧版本只有汇总、没有逐次记录的会话不导入，不做估算。
- Copilot 订阅按 Premium 请求 / AI Credits 计费，表中费用是按 token 价格表的估算；缺少价格时为 0，不代表免费。
- 会话删除在 Copilot CLI 中完成，以保持 `session-store.db` 与会话目录一致。

## 验证

- 单元与集成测试：`cargo test --lib copilot`、`cargo test --test copilot_commands`、`pnpm vitest run tests/components/CopilotProviderForm.test.tsx src/config/copilotProviderPresets.test.ts`。
- 真实 CLI：在临时 `COPILOT_HOME` 中用 `copilot mcp add/disable` 核对 MCP 原生格式；用本地 OpenAI 兼容端点以 BYOK 环境变量运行 `copilot -p`，确认请求成功，并用生成的会话目录运行 `CCS_COPILOT_SMOKE_HOME=<目录> cargo test --lib real_copilot_home -- --ignored` 核对会话和用量解析。
