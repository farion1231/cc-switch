# TRAE CLI 接入

TRAE CLI 是独立应用，应用 ID 为 `trae`，命令行工具为 `traecli`。接入面向字节跳动官方的 [TRAE CLI 2.0](https://docs.trae.cn/cli_about-trae-code-cli-2)（`traecli`，Codex 派生，配置为 TOML），不是 TRAE IDE / SOLO 桌面版（模型配置在服务端），也不是开源的 [bytedance/trae-agent](https://github.com/bytedance/trae-agent)（项目内 YAML 配置）。

实测版本：traecli 0.211.1（macOS arm64）。

## 使用方式

1. 在「应用」页安装 TRAE CLI，或手动运行官方安装器 `sh -c "$(curl -fsSL https://trae.cn/trae-cli/install_v2.sh)"`（Windows：`irm https://trae.cn/trae-cli/install_v2.ps1 | iex`）。安装器把独立程序放进 `~/.local/bin`，不依赖 Node；升级使用 `traecli update`，失败时重跑官方安装器。最新版本读取官方 CDN 的 `v2/public/stable/latest.json`。
2. 在侧栏选择 **TRAE CLI**。`traecli.toml` 中已有的 `[model_providers.*]` 会自动出现在列表中。
3. 「添加供应商」选择预设或自定义配置，填写 Responses API 地址、API Key（或 API Key 环境变量）和模型。预设复用 Codex 预设里原生支持 Responses API 的供应商；只支持 Chat Completions 的预设不提供。「TRAE 账号登录」预设对应 traecli 自带的账号登录。
4. 点击「添加」把供应商写入 `traecli.toml`，多个供应商可以同时存在。卡片上的「切换」写入顶层 `model_provider` 和 `model`，当前供应商标为「使用中」。切换到「TRAE 账号登录」会移除这两个键，traecli 回到账号提供的模型。
5. 「移除」从 `traecli.toml` 删除对应的 `[model_providers.<标识>]`，保留 CC Switch 中的档案；正在使用的供应商需先切换到其他供应商才能移除或删除。

## 配置与能力

| 能力 | 原生位置／行为 |
| --- | --- |
| 供应商 | `~/.trae/traecli.toml` 的 `[model_providers.<标识>]`（`name`、`base_url`、`wire_api`、`experimental_bearer_token` 或 `env_key`，其余字段原样保留） |
| 当前供应商 | 顶层 `model_provider` + `model`；两者都不存在时为 TRAE 账号登录 |
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

## 数据保留规则

- 编辑、切换、移除只修改对应的 `[model_providers.<标识>]` 表以及顶层 `model_provider` / `model`，保留其余设置（权限、项目信任、MCP、插件等）和 TOML 注释。
- 卡片切换时以 `traecli.toml` 中已有的供应商表为准，不用 CC Switch 的档案覆盖用户在 traecli 中的修改；只有模型取自 CC Switch。
- `traecli.toml` 解析失败时拒绝写入，不覆盖原文件。写文件与数据库提交一起完成，任一步失败都恢复原文件。

## 验证方式

在隔离目录中实测（不触碰真实 `~/.trae`）：

1. 用本地 Responses SSE 模拟服务作为 `base_url`，CC Switch 写入的配置能被 `traecli exec` 加载，请求带 `Authorization: Bearer <experimental_bearer_token>` 发往 `<base_url>/responses`；`env_key` 同样生效。
2. `$TRAE_HOME/AGENTS.md` 的内容出现在请求的 `# AGENTS.md instructions` 中。
3. 含工具调用的会话可在会话页查看，用量按每次响应导入。
