# Kimi Code 接入

Kimi Code 是独立应用，应用 ID 为 `kimi`。接入面向当前的 [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code)，使用 `~/.kimi-code`；已归档的 Python Kimi CLI 使用 `~/.kimi`，其数据迁移由 Kimi Code 的 `kimi migrate` 完成。

## 使用方式

1. 在「应用」页安装 Kimi Code，或先按[官方说明](https://moonshotai.github.io/kimi-code/en/guides/getting-started)安装。CC Switch 使用官方独立程序安装器，自带运行时，不依赖系统 Node 版本；已有客户端升级使用 `kimi upgrade --yes`，静态兜底重跑官方安装器。手动使用 npm 包 `@moonshot-ai/kimi-code` 时需满足其 Node 要求，2.3.0 要求 Node ≥22.19.0，Node 22.12.0 会因缺少 `createZstdDecompress` 无法启动。
2. 在侧栏选择 **Kimi Code**。已有原生供应商和模型会自动出现在列表中。
3. 添加供应商：和 Pi 等应用一样，从「模型厂商」「第三方平台」「云服务商」「本地」分组中选择具体供应商，或使用各组共用的「自定义配置」。预设保留厂商的套餐、地区版本、模型参数、请求头和获取 API Key 的链接，并转换为 Kimi 原生字段。「模型厂商」中的 **Kimi** 包含开放平台、编程套餐及国内／海外版本，替代此前独立的 Kimi Code、Moonshot 和通用第三方条目。每个模型必须填写模型 ID 和正整数的上下文长度。
4. 点击「添加」将供应商及模型写入 Kimi 配置。多个供应商可以同时启用。在 Kimi Code 内使用 `/model` 选择默认模型；没有默认模型时，首次添加会选择该供应商的第一个模型。
5. 「移除」会从原生配置移除供应商和所属模型，并保留 CC Switch 中的档案。主模型或子代理模型池仍引用的模型不能移除，需要先在 Kimi Code 中调整选择。

## 配置与能力

| 能力 | 原生位置／行为 |
| --- | --- |
| 供应商和模型 | `~/.kimi-code/config.toml` 的 `providers`、`models` 表 |
| 协议 | `kimi`、`openai`、`openai_responses`、`anthropic`、`google-genai`、`vertexai` |
| 模型能力 | 提供图片输入、思考、始终思考、工具调用、按需加载工具的配置；音视频等其他能力由 Kimi 原生管理，已有字段原样保留 |
| 供应商目录 | 复用已维护的 Pi 供应商信息并转换为 Kimi 原生协议和模型字段；AWS Bedrock 的专有签名协议不兼容，不提供该预设 |
| MCP | `~/.kimi-code/mcp.json`，支持 stdio、HTTP、SSE；SSE 使用原生的 `transport = "sse"` 字段 |
| Skills | `~/.kimi-code/skills/`，通过统一 Skills 页面管理 |
| 全局提示词 | `~/.kimi-code/AGENTS.md`，通过统一提示词页面管理 |
| 会话 | `~/.kimi-code/sessions/<workDirKey>/<sessionId>/state.json` 和 `agents/main/wire.jsonl` |
| 恢复会话 | `kimi --session <id>`，保留原工作目录和数据目录 |
| 用量 | 从已提交的 assistant 消息导入输入／输出和缓存 token，按 `step.end.messageId` 关联 `llm.request` 中的实际模型；使用项目价格表估算费用 |

设置环境变量 `KIMI_CODE_HOME` 后，上述位置全部跟随该目录。CC Switch 进程需要继承这个环境变量。

Kimi 使用原生多供应商配置，因此此接入采用共存模式。当前不提供 CC Switch 的本地路由或聚合模式。官方 OAuth 登录和凭据刷新仍由 Kimi Code 管理；CC Switch 保留配置中的 OAuth 引用，不写入或替换凭据文件。

Ollama、LM Studio 和 vLLM 预设使用各自的本地 OpenAI 兼容地址，并预填 `local` 占位 Key，供默认不校验凭据的服务使用；服务器已开启鉴权时应替换为实际 Key。模型 ID 和上下文长度需要与本机已加载的模型一致。Microsoft Foundry 的资源名占位符需要改为自己的资源名。

## 数据保留规则

- 编辑供应商只修改对应的供应商节点和所属模型，保留其他供应商、全局设置、服务配置及未修改表的 TOML 注释。
- 原生模型键全局唯一。发现同名键属于另一个供应商时，拒绝覆盖。复制供应商时会生成新的模型别名，并先保存为未启用档案。
- 原生配置在外部变化后，刷新列表会更新同 ID 档案；原生节点被删除时，档案仍保留，状态变为未启用。
- CC Switch 不提供音视频能力开关，也不在预设中主动声明这些能力。原生模型及其 `overrides` 中的音视频、输出能力和未知能力字段在导入、编辑及保存时原样保留，由 Kimi 解释；图片输入和语言模型能力仍可在界面配置。
- Kimi 原生 Skills 目录可能包含用户自行安装的同名 Skill。重新同步、存储迁移及卸载未对 Kimi 启用的受管 Skill 时保留这些目录；显式启用、停用或更新时，只操作能确认为 CC Switch 部署的副本。
- 无法解析的 TOML／MCP JSON 会报错并保留原文件。供应商和 MCP 的原生写入与数据库保存之间发生失败时，恢复写入前的文件。
- 会话阅读优先使用原生日志中的已提交消息，避免与执行过程记录重复；分支切换后仅展示活动分支。会话删除在 Kimi Code 中完成，以保持会话索引、锁和历史记录一致。
- 用量使用日志中当时的模型，不从当前供应商推断历史归属。缺少模型时记为 `unknown`；缺少价格时费用为 `0`，并不表示免费。持久去重记录防止重复导入已汇总清理的请求或复制到分支中的历史请求。
- 首字读取 `step.end` 的原生计时，耗时优先使用首字等待与流式输出时长之和，排除后续工具执行；旧日志缺少原生计时时仅在起止时间可靠时估算耗时，已完成的内容块不能用于推断首字。速度沿用会话日志口径：输出 token ÷ 总耗时（含首字等待），输出至少 200 token、耗时至少 1 秒才显示，并带估算标记。
- 更新后首次同步会重新检查未变化的 Kimi 日志，修正仍保留明细的模型、费用和计时。请求 ID 和持久去重记录保持稳定，已汇总清理的历史不会重复导入。

原生格式依据：[配置文件](https://moonshotai.github.io/kimi-code/en/configuration/config-files)、[数据位置](https://moonshotai.github.io/kimi-code/en/configuration/data-locations)、[MCP](https://moonshotai.github.io/kimi-code/en/customization/mcp)。
