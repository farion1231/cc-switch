# Pi 原生契约与实现边界

> 验证原则：开发和验收使用当时最新发布的 Pi；CC Switch 不绑定某个 Pi 版本。

这份文档记录 CC Switch 当前实际消费的 Pi 原生契约。它不是 Pi 配置格式的完整镜像，也不承诺代理、OAuth 或所有兼容字段。实现和测试只覆盖前端已经提供的能力。

## 验证方式

供应商字段、继承顺序和配置值解析以开发时最新 Pi 的源码入口和真实 CLI 行为为准。资源与会话行为通过 Pi 的 `DefaultResourceLoader`、`loadSkills`、`loadPromptTemplates` 和 `SessionManager` 实际运行确认。仓库只保留产品真正消费的最小契约和普通测试，不维护上游源码快照、哈希或版本证据库。

## 当前消费的契约

| 资源                            | CC Switch 行为                                                | 状态来源         |
| ------------------------------- | ------------------------------------------------------------- | ---------------- |
| `models.json`                   | 管理 `providers` 中的全部显式供应商节点；精确新增、替换和移除 | 文件中的实际条目 |
| 全局 `settings.json`            | 只读 `defaultProvider`、`defaultModel`、`sessionDir`          | Pi 原生设置      |
| `auth.json`                     | 不读、不写、不刷新                                            | Pi `/login`      |
| `AGENTS.md`                     | 提示库中与文件内容精确匹配的项视为正在使用                    | 文件存在及内容   |
| `SYSTEM.md`、`APPEND_SYSTEM.md` | 直接编辑固定原生文件；不存在即未配置                          | 文件存在         |
| `prompts/*.md`                  | 管理顶层斜杠命令模板；空模板是有效原生文件                    | 文件存在         |
| `skills/<目录>`                 | 目录存在即被 Pi 发现                                          | 原生 Skills 目录 |
| Sessions JSONL                  | 读取 Pi 的会话头、树分支、消息和会话名称                      | 原生会话文件     |

### 供应商

结构化表单只验证并编辑常用字段：

- 供应商级 `name`、`baseUrl`、`apiKey`、`api`、`headers`
- 模型级 `id`、`name`、`reasoning`、`input`、`contextWindow`、`maxTokens`

已有配置中的其他字段原样保留。供应商是否可管理只取决于节点是否显式存在于 `models.json.providers`：`anthropic`、`openai`、`deepseek` 等 Pi 内置 ID，以及带有未知字段的节点，都按普通显式配置同步。CC Switch 不把 Pi 运行时合并出的内置模型复制回配置，也不解析或执行 `apiKey`、Header 中的环境变量和命令表达式。请求仍由 Pi 自己发出。

Pi 在全局设置中保存的当前供应商和模型不进入供应商列表状态。启用供应商只把条目加入 `models.json`，不会写 `defaultProvider` 或 `defaultModel`。移除或删除全局默认供应商时，前端在原有确认框内给出非阻塞提醒，后端允许继续且不改写默认项。编辑显式供应商同样不修改当前供应商或模型；失效引用和回退由 Pi 原生处理。

项目级 `.pi/settings.json` 会按启动 Pi 时的工作目录覆盖全局默认项。供应商页没有项目上下文，因此不扫描项目目录或猜测活动会话；条件提醒只读取全局默认供应商。项目级默认项继续由对应 Pi 会话和 `/model` 管理。

用量查询脚本属于 CC Switch 元数据，只更新数据库，不重写 `models.json`。

### 并发与外部修改

供应商标识是成员身份：`models.json.providers` 中存在该标识即为已启用。CC Switch 每次进入列表及应用启动时同步全部显式节点，不按内置 ID、认证字段或模型完整度过滤。原生配置发生修改后，以原生内容更新同 ID 的已保存档案；原生删除只改变启用状态。移除前保存目标节点的最新内容，数据库中的卡片和完整配置继续保留。

`models.json`、系统提示文件和模板使用原子写入。进程内写操作串行化；同一次读改写期间的跨进程变化通过 revision 比较发现。列表刷新会先把外部修改同步到数据库；保存时只替换目标 provider 节点，启用和移除只增删目标节点，其他 provider 与顶层字段保持不变。不再维护编辑快照或额外 ownership 状态。

### Sessions

全局会话页只枚举绝对 `sessionDir`、`~` 路径或 Pi 默认目录。相对 `sessionDir` 依赖启动 Pi 时的项目工作目录，CC Switch 没有可靠上下文，因此明确显示“需要项目上下文”，不会猜测目录。

会话解析只消费 UI 所需字段。未知条目被忽略；删除前会验证文件仍在已解析的 Pi 会话根目录中，并核对会话 ID。

## 明确不做

- Pi `/login`、`auth.json` 中的 OAuth/API Key 登录、令牌保存和刷新
- 默认供应商或默认模型写入
- 路由、网关、代理、故障转移和请求头合成
- Pi 运行时内置供应商与内置模型目录的复制
- 完整 `compat`、`modelOverrides` 和费用编辑器；思考档位只提供 Pi 原生 `thinkingLevelMap` 的轻量入口
- 相对会话目录的全局猜测

Pi 发布新版本时，通过正常开发和验收重新运行供应商、提示词、Skills 和 Sessions 契约测试。没有进入产品界面的上游字段不应仅为了“覆盖完整”而扩展后端。
