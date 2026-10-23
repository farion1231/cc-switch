# TRAE CLI 接入

TRAE CLI 是独立应用，应用 ID 为 `trae`，命令行工具为 `traecli`。接入面向字节跳动官方的 [TRAE CLI 2.0](https://docs.trae.cn/cli_about-trae-code-cli-2)（`traecli`，Codex 派生，配置为 TOML），不是 TRAE IDE / SOLO 桌面版（模型配置在服务端），也不是开源的 [bytedance/trae-agent](https://github.com/bytedance/trae-agent)（项目内 YAML 配置）。

实测版本：traecli 0.211.1（macOS arm64）。

## 使用方式

1. 在「应用」页安装 TRAE CLI，或手动运行官方安装器 `sh -c "$(curl -fsSL https://trae.cn/trae-cli/install_v2.sh)"`（Windows：`irm https://trae.cn/trae-cli/install_v2.ps1 | iex`）。安装器把独立程序放进 `~/.local/bin`，不依赖 Node；升级使用 `traecli update`，失败时重跑官方安装器。最新版本读取官方 CDN 的 `v2/public/stable/latest.json`。
2. 在侧栏选择 **TRAE CLI**。`traecli.toml` 中已有的 `[model_providers.*]` 会自动出现在列表中。
3. 「添加供应商」选择预设或自定义配置，填写 API Key（或 API Key 环境变量）、Responses API 地址和默认模型；「可选模型」可从接口拉取模型列表后勾选。预设复用 Codex 预设里原生支持 Responses API 的供应商；只支持 Chat Completions 的预设不提供。「TRAE 账号登录」预设对应 traecli 自带的账号登录。
4. 卡片上的「添加」只把供应商写入 `traecli.toml` 的 `[model_providers.<标识>]`，不改当前选择，多个供应商可以同时存在。「更多 → 设为默认」写入顶层 `model_provider`、`model` 和模型目录，当前供应商标为「默认」。从 `traecli.toml` 导入、尚未设置模型的供应商需先编辑设置模型。切换到「TRAE 账号登录」会移除这些键，traecli 回到账号提供的模型。
5. 「移除」从 `traecli.toml` 删除对应的 `[model_providers.<标识>]`，保留 CC Switch 中的档案；正在使用的供应商需先切换到其他供应商才能移除或删除。

## 配置与能力

| 能力 | 原生位置／行为 |
| --- | --- |
| 供应商 | `~/.trae/traecli.toml` 的 `[model_providers.<标识>]`（`name`、`base_url`、`wire_api`、`experimental_bearer_token` 或 `env_key`，其余字段原样保留） |
| 当前供应商 | 顶层 `model_provider` + `model`；两者都不存在时为 TRAE 账号登录 |
| 模型列表 | traecli 不会向自定义供应商的 `/v1/models` 拉取模型，`traecli models` 和 `/model` 只读 Codex 格式的模型目录。当前供应商由 CCS 写入 `~/.trae/cc-switch-model-catalog.json`（默认模型 + 可选模型，复用 Codex 原生 Responses 目录生成逻辑），并以**绝对路径**写入 `model_catalog_json`（相对路径按当前工作目录解析，会找不到文件）。`model_catalog_json` 指向用户自己的目录时不改动 |
| 协议 | 仅 `wire_api = "responses"`。traecli 0.211.1 对 `wire_api = "chat"` 直接报错拒绝加载 |
| MCP | 同一文件的 `[mcp_servers.<id>]`，支持 stdio（`command`/`args`/`env`/`cwd`）和 streamable HTTP（`url`/`http_headers`）；不支持 SSE |
| Skills | `~/.trae/skills/`，通过统一 Skills 页面管理；未经 CC Switch 安装的同名目录不会被覆盖或删除 |
| 全局提示词 | `~/.trae/AGENTS.md`，traecli 每次会话把它作为 `# AGENTS.md instructions` 注入 |
| 会话 | `~/.trae/cli/sessions/YYYY/MM/DD/rollout-*.jsonl`；消息来自 `history_mutation` 记录，注入的上下文（AGENTS.md、环境信息、system-reminder）标记为注入内容 |
| 恢复会话 | `traecli resume <id>`；删除会话请在终端运行 `traecli delete <id>`，以同步更新 traecli 的会话索引 |
| 用量 | 从 `token_usage_record` 按 `response_id` 导入，输入 token 扣除缓存读取后按 fresh 语义入库，模型取对应 turn 的 `turn_context.model` |

设置环境变量 `TRAE_HOME` 后，上述位置全部跟随该目录（traecli 自身也遵循该变量）。CC Switch 进程需要继承这个环境变量。

保留的供应商标识：`openai`、`oss`、`ollama`、`lmstudio` 是 traecli 内置供应商，`trae-official` 是 CC Switch 的账号登录卡片，均不能用作自定义标识。

TRAE CLI 使用原生多供应商配置，因此此接入采用共存模式，不提供 CC Switch 的本地路由或聚合模式。需要接入只支持 Chat Completions 的供应商时，暂不支持通过本地路由转换。

## 与 TRAE 桌面版的关系

TRAE 桌面版（国际版 Trae、国内版 Trae CN）和 TRAE CLI 不是一套配置。国际版桌面版和 CLI 都使用 `~/.trae` 目录，但读取的文件不同，互不读取对方的配置；国内版桌面版使用独立的 `~/.trae-cn`。

| | TRAE 桌面版（Trae / Trae CN） | TRAE CLI（`traecli`） |
| --- | --- | --- |
| 配置目录 | 国际版 `~/.trae`，国内版 `~/.trae-cn` | `~/.trae`（或 `$TRAE_HOME`） |
| 模型／供应商 | 在应用「设置 → 模型」中配置，随账号保存在服务端 | `~/.trae/traecli.toml` 的 `[model_providers.*]` |
| 编辑器设置 | `~/Library/Application Support/Trae[ CN]/User/settings.json` | — |
| MCP | 应用内配置，或项目内的 `.trae/mcp.json` | `traecli.toml` 的 `[mcp_servers.*]` |
| Skills | `builtin_skills/`、`skill-config.json` | `~/.trae/skills/` |
| 会话 | 桌面版自己的存储 | `~/.trae/cli/sessions/` |

在 Trae 3.6.4 和 Trae CN 3.4.4 的全部资源中搜索 `traecli.toml`、`model_providers`、`TRAE_HOME`，均无引用。桌面版的模型配置随账号保存在服务端、本地没有对应文件，因此 CC Switch 只管理 `traecli`，在 CCS 中配置的供应商不会出现在桌面版的模型列表里。

## 数据保留规则

- 编辑、切换、移除只修改对应的 `[model_providers.<标识>]` 表以及顶层 `model_provider` / `model`，保留其余设置（权限、项目信任、MCP、插件等）和 TOML 注释。
- 卡片切换时以 `traecli.toml` 中已有的供应商表为准，不用 CC Switch 的档案覆盖用户在 traecli 中的修改；只有模型取自 CC Switch。
- `traecli.toml` 解析失败时拒绝写入，不覆盖原文件。配置文件、模型目录与数据库提交一起完成，任一步失败都恢复这两个文件。

## 验证方式

在隔离目录中实测（不触碰真实 `~/.trae`）：

1. 用本地 Responses SSE 模拟服务作为 `base_url`，CC Switch 写入的配置能被 `traecli exec` 加载，请求带 `Authorization: Bearer <experimental_bearer_token>` 发往 `<base_url>/responses`；`env_key` 同样生效。
2. `$TRAE_HOME/AGENTS.md` 的内容出现在请求的 `# AGENTS.md instructions` 中。
3. 含工具调用的会话可在会话页查看，用量按每次响应导入。
