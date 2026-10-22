# Kimi Code 接入

Kimi Code 是独立应用，应用 ID 为 `kimi`。接入面向当前的 [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code)，使用 `~/.kimi-code`；已归档的 Python Kimi CLI 使用 `~/.kimi`，其数据迁移由 Kimi Code 的 `kimi migrate` 完成。

## 使用方式

1. 在「应用」页安装 Kimi Code，或先按[官方说明](https://moonshotai.github.io/kimi-code/en/guides/getting-started)安装。CC Switch 使用官方独立程序安装器，自带运行时，不依赖系统 Node 版本；已有客户端升级使用 `kimi upgrade --yes`，静态兜底重跑官方安装器。手动使用 npm 包 `@moonshot-ai/kimi-code` 时需满足其 Node 要求，2.3.0 要求 Node ≥22.19.0，Node 22.12.0 会因缺少 `createZstdDecompress` 无法启动。
2. 在侧栏选择 **Kimi Code**。已有原生供应商和模型会自动出现在列表中。
3. 使用编程套餐时，可直接在「添加供应商」的「账号登录」分组中选择 **Kimi Code 官方登录**，完成授权后自动添加模型。使用 API Key 时，和 Pi 等应用一样，从「模型厂商」「第三方平台」「云服务商」「本地」分组中选择具体供应商，或使用各组共用的「自定义配置」。预设保留厂商的套餐、地区版本、模型参数、请求头和获取 API Key 的链接，并转换为 Kimi 原生字段。「模型厂商」中的 **Kimi** 包含开放平台、编程套餐及国内／海外版本，替代此前独立的 Kimi Code、Moonshot 和通用第三方条目。每个模型必须填写模型 ID 和正整数的上下文长度。
4. 点击「添加」将供应商及模型写入 Kimi 配置。多个供应商可以同时启用。供应商卡片上的「切换」会选择该供应商的模型，当前供应商以蓝色「使用中」标识。已有当前模型属于目标供应商时保留该模型，否则选择目标供应商的第一个原生模型；需要切换到其他具体模型时，可在 Kimi Code 内使用 `/model`。没有默认模型时，首次添加会选择该供应商的第一个模型。
5. 「移除」位于卡片的「更多」菜单，会从原生配置移除供应商和所属模型，并保留 CC Switch 中的档案。主模型或子代理模型池仍引用的模型不能移除，需要先在 Kimi Code 中调整选择。

## 配置与能力

| 能力 | 原生位置／行为 |
| --- | --- |
| 供应商和模型 | `~/.kimi-code/config.toml` 的 `providers`、`models` 表 |
| 官方 OAuth | 设备码授权，凭据保存到 `~/.kimi-code/credentials/`，由 Kimi Code 自动刷新 |
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

Kimi 使用原生多供应商配置，因此此接入采用共存模式。当前不提供 CC Switch 的本地路由或聚合模式。

Ollama、LM Studio 和 vLLM 预设使用各自的本地 OpenAI 兼容地址，并预填 `local` 占位 Key，供默认不校验凭据的服务使用；服务器已开启鉴权时应替换为实际 Key。模型 ID 和上下文长度需要与本机已加载的模型一致。Microsoft Foundry 的资源名占位符需要改为自己的资源名。

## 官方 OAuth 登录

官方账号供应商默认显示为 **Kimi Official**，使用 Kimi 图标并带有「官方」徽标。已有自动导入的条目会在刷新列表时补齐名称、图标和官方分类；自定义名称和图标保留。原生供应商 ID 仍为 `managed:kimi-code`，以兼容 Kimi Code 的 OAuth 模型刷新及现有模型引用。

点击「添加供应商」，在「账号登录」分组中选择 **Kimi Code 官方登录**，进入独立的账号登录页。选择国内版或海外版后点击预设条右侧的「登录 Kimi」，在浏览器中输入授权码并确认。若浏览器没有自动打开，可使用页面上的授权链接和复制授权码按钮。登录成功后，点击「使用此账号」选中官方模型并返回供应商列表（单纯登录仍保留此前的默认模型）；此流程无需填写 API Key、模型或 JSON 配置，也不显示普通供应商表单的「添加」按钮。

授权成功后，CC Switch 从官方 `/models` 获取当前账号的模型和能力，添加原生 `managed:kimi-code` 供应商、`kimi-code/<模型 ID>` 模型，以及缺失的联网搜索和抓取服务。已有的默认模型、用户模型别名、模型 overrides、自定义服务及其他供应商配置会保留。配置写入失败时不会替换旧登录；凭据保存失败时恢复原配置，可点击「重试」继续完成当前授权。

凭据按官方格式保存在 `~/.kimi-code/credentials/`，仅后端读写，不进入供应商档案或前端响应。国内版使用 `kimi-code.json`；海外版及自定义环境使用官方规则生成的独立凭据槽位。`KIMI_CODE_HOME`、`KIMI_CODE_OAUTH_HOST`（兼容 `KIMI_OAUTH_HOST`）和 `KIMI_CODE_BASE_URL` 会被尊重。授权后的令牌刷新仍由 Kimi Code 管理，CC Switch 不与客户端并行轮换 refresh token。

账号登录页会读取 Kimi Code 已有的文件登录状态，支持刷新、取消、重新登录和退出登录。退出登录会移除当前区域／环境的本机凭据，并让 Kimi Code 退出该登录；保留供应商、模型与模型选择。切换区域重新登录只更新目标区域的凭据，其他区域的凭据文件保留。

协议及存储格式依据：[官方 OAuth 实现](https://github.com/MoonshotAI/kimi-code/tree/44cdf8f4039e2ff20d3fe8a4e7868bd90fd786e1/packages/oauth/src)。

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
