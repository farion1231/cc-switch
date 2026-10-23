# Qoder CLI 接入

在「应用」页找到 Qoder CLI，通过名称下方固定位置的「国际版／国内版」双选按钮选择版本。首次使用按界面语言选择：中文（含繁体中文）默认国内版，其他语言默认国际版。手动选择会保存在本机设置 `qoderEdition`，以后更改语言不会覆盖这一选择。侧栏、MCP、Skills 和会话筛选只显示当前所选版本；隐藏另一版本不会停止其已启用配置的同步。

两个版本分别管理安装状态和数据，切换版本不会迁移、合并或删除已有配置。安装、升级、卸载和手动命令均跟随所选版本；已经排队的任务保留提交时的目标。正在处理任一 Qoder 版本的任务时，版本选择按钮暂时不可用。

| 项目 | 国际版 | 国内版 |
| --- | --- | --- |
| 应用 ID | `qoder` | `qoder-cn` |
| CLI 启动命令 | `qoder` | `qodercn` |
| 原生独立程序 | `qodercli` | `qoderclicn` |
| npm 包（兼容已有安装） | `@qoder-ai/qodercli` | `@qodercn-ai/qoderclicn` |
| 官方安装源 | `https://qoder.com/install`、`install.ps1` | `https://static.qoder.com.cn/qoder-cli-cn/install.sh`、`install.ps1` |
| 默认数据目录 | `~/.qoder` | `~/.qoder-cn` |
| 自定义数据目录环境变量 | `QODER_CONFIG_DIR` | `QODERCN_CONFIG_DIR` |

新安装使用官方原生安装器，不依赖系统 Node.js。已有 npm 安装沿用检测到的包管理器升级和卸载；原生安装使用自身的 `update`。默认路径无法确定或无法确认安装归属时，沿用项目现有的预览和手动处理流程。

原生卸载逐个删除预览列出的 CLI 程序和历史版本文件，保留设置、认证、会话、提示词、Skills 和其他用户文件。Qoder 安装器的 `entry/` 启动入口也用于 IDE，卸载 CLI 时保留该共享入口及 PATH 设置。

| 能力 | 原生位置或行为 |
| --- | --- |
| 官方账号登录 | 在「授权中心」新增多个官方账号、逐账号重新登录、选择使用或确认删除；国际版与国内版独立管理 |
| 模型及供应商 | 在 CC Switch 的 Qoder 页面添加、编辑和删除原生自定义供应商；配置写入 `settings.json.providers`，默认模型写入 `model.name` |
| MCP | 数据目录下 `settings.json` 的 `mcpServers`；支持 stdio、HTTP、SSE |
| Skills | 数据目录下 `skills/`，通过统一 Skills 页面管理，两个版本各自启用 |
| 全局提示词 | 数据目录下 `AGENTS.md` |
| 会话 | 数据目录下 `projects/<project>/<session-id>.jsonl`；复用已有 user/assistant/tool 消息阅读组件 |
| 恢复会话 | `qoder --resume <id>` 或 `qodercn --resume <id>`，保留工作目录和自定义数据目录 |
| 删除会话 | 在 Qoder CLI 中完成，以保持会话状态和历史一致 |

MCP 写入只更新指定服务器，保留其他设置和服务器的原生扩展字段。无法解析的配置会报错并保留原文件；文件写入与数据库提交之间发生失败时恢复原文件。一次编辑涉及两个版本时，两边都成功后才保存数据库，失败时同时回滚。

CC Switch 每次写入任一版本的 `settings.json` 时，都会补充或覆盖 `privacy.usageStatisticsEnabled` 为 `false`，关闭 Qoder 用量统计上报。模型、供应商、MCP 和 Skills 的配置写入共用这一规则，同时保留其他 `privacy` 字段和 JSONC 注释。

MCP 配置支持 JSONC 注释和尾随逗号，写入时保留注释、`cwd`、超时、工具筛选与 OAuth 等原生选项；切换传输类型时清理不再适用的启动字段。用户级配置对所有项目生效，不改变 Qoder 对项目级 MCP 的信任与审批设置。Skills 同步到所选版本的用户级 `skills/<目录>/SKILL.md`，支持 Qoder 原生目录结构与符号链接；启用时同时清理该技能在用户配置中的 `skills.disabled` 和 `skillOverrides: off`，保留其他技能的禁用状态。配置修改在新会话中加载。

模型配置直接在 CC Switch 内完成，无需在 CLI 中配置。添加供应商时先显示与其他 CLI 共用的预设选择列表，支持按名称或网址搜索、厂商分类和套餐／地区版本选择；选择预设后自动填写连接信息与模型，也可选择「自定义配置」手动填写。列表只包含 Qoder 原生协议能够表达的预设。支持 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages、Bearer／API Key 认证，以及每个模型的 ID、名称、上下文长度、最大输出 Token、工具调用、图片输入和思考模式。可以指定供应商的默认模型，并选择当前默认模型；删除当前供应商会切回 Qoder 内置的自动选择。

「授权中心」以账号列表管理官方账号，可以从侧栏直接进入；添加供应商中的「账号登录」预设及官方供应商卡片的编辑入口也进入同一页面。从供应商预设进入时自动选择对应版本，直接从侧栏进入时默认跟随已保存的 Qoder 版本偏好。在授权中心选择另一个版本只切换所管理的账号，不修改应用页的版本选择。

点击「登录 Qoder」或「添加账号」发起官方浏览器授权。新增授权在独立临时目录中完成，保留原生机器标识，但不复制当前登录；因此已登录时也能继续新增账号，取消或失败不会使原账号退出。首次登录成功后保存该版本的机器标识，后续新增账号继续使用同一标识，即使尚未启用任何账号，也能在多个已保存账号间切换。新增成功后留在本页，点击某一行的「使用此账号」才切换 Qoder 原生登录并启用官方自动选模，同时刷新该版本的供应商列表。每个账号右侧提供红色垃圾桶按钮，删除前显示确认窗口；删除当前账号会同时清除该版本的本机原生登录，其他账号、供应商、MCP、Skills 和会话文件保持原样。已有原生登录会自动收录到列表，也可以点击刷新按钮重新读取。

每行的「重新登录」更新该账号原有的档案 ID，不新增重复条目。重新登录当前账号时同步更新原生登录；重新登录其他账号时保留当前登录。两种情况都保留当前模型、供应商和其他原生配置。登录期间账号已被删除或凭据发生变化时，拒绝覆盖；浏览器登录到另一个已保存的账号时，提示使用已有账号。写入账号档案失败时恢复原生凭据。授权中心返回后保留进入前的供应商页面；关闭授权中心或取消登录会清理待处理的授权流程。

浏览器授权、凭据加密和令牌续期仍使用 Qoder CLI 的官方 `login` / `status --output json` 流程，已按 1.1.76 分发包核对。CC Switch 在本机 `qoder_accounts.json` 中保存原生加密凭据快照及账号显示信息，按版本、原生配置目录和运行环境分别管理；加密凭据不写入供应商数据库，也不返回前端；界面只读取账号显示信息。切换和删除会检查原生凭据是否已被其他程序更改，并遵守 Qoder 的凭据事务锁。对应版本的 CLI 必须已安装，此流程保留 `privacy.usageStatisticsEnabled: false`。

未使用的供应商卡片提供与其他应用一致的「切换」按钮，点击后使用该供应商配置的默认模型，未指定时使用第一个模型；当前供应商显示通用的「使用中」状态。模型也可通过卡片上的菜单选择；切回 Qoder 内置自动选择的入口放在当前供应商的更多菜单中。卡片的「配置用量查询」复用现有余额、Coding Plan 与自定义查询模板，并支持自动刷新。查询使用 Qoder 原生供应商中的地址与凭据，支持 `${ENV_NAME}` 引用；查询脚本等 CC Switch 元数据保存在本机数据库，独立于 Qoder 的配置文件和另一版本。

用量页支持从两个版本各自的 `projects/` 会话日志导入请求数、输入／输出 Token、缓存 Token、供应商、模型与定价表估算费用，包含子代理请求。流式快照、重复内容块、会话分叉和已汇总的历史记录会去重；未写完的最后一行留到下一轮读取。OpenAI／Responses 的输入 Token 含缓存，Anthropic 的输入 Token 不含缓存，统计按原生协议区分口径。自定义模型的费用来自本机定价表；Qoder 官方订阅使用 Credits，不以 Token 定价换算成账单费用，官方 Plan／Credits 额度仍由 Qoder 账号提供。

原生格式已根据 Qoder CLI 1.1.76 分发包中的 `providers` 配置 schema 与模型加载器核对。保存保留 JSONC 注释、其他供应商、MCP、原生模型偏好及未编辑的扩展字段；编辑期间供应商被其他程序修改时拒绝覆盖并提示刷新。国际版和国内版的数据分别读写。新配置在 Qoder 重启后加载，自定义模型的运行权限仍取决于 Qoder 账号；CC Switch 不改写登录态或功能权限。Qoder 暂不接入本地路由。

依据 2026-10-22 核对的官方说明：[登录与认证](https://docs.qoder.com/cli/authentication)、[国际版安装与卸载](https://docs.qoder.com/cli/installation)、[国内版安装与卸载](https://docs.qoder.cn/cli/installation)、[自定义模型](https://docs.qoder.com/cli/custom-models)、[配置](https://docs.qoder.com/cli/settings)、[MCP](https://docs.qoder.com/cli/mcp-reference)、[Skills](https://docs.qoder.com/cli/Skills)、[会话](https://docs.qoder.com/cli/sessions)。
