# Pi 原生契约与实现边界

> 适用版本：`earendil-works/pi@ab366ebe94cacd419d986be454f12b1b9913aaca`
> 最后核对：2026-08-04

这份文档记录 CC Switch 当前实际消费的 Pi 原生契约。它不是 Pi 配置格式的完整镜像，也不承诺代理、OAuth 或所有兼容字段。实现和测试只覆盖前端已经提供的能力。

## 验证来源

供应商字段、继承顺序和配置值解析曾直接执行 pinned Pi 的 TypeScript 入口。固定来源如下：

| 来源 | SHA-256 |
| --- | --- |
| `packages/coding-agent/src/core/model-config.ts` | `62141770d675ad6357a72e07354355f0eda29281c0e5be1b48d2360f341c7360` |
| `packages/coding-agent/src/core/provider-composer.ts` | `17308a4179b330526eabf6c917fa13e9dbd9ece90d1555b870e87d39b5b60d9d` |
| `packages/coding-agent/src/core/resolve-config-value.ts` | `0f53dad47fe5d5d8837c022b7951ccd3bd5a9b577bd662f0986272110e83bcc7` |

资源和会话行为曾通过 pinned Pi 的 `DefaultResourceLoader`、`loadSkills`、`loadPromptTemplates` 和 `SessionManager` 实际运行确认。大型生成器与快照没有保留在仓库中，因为其中大部分覆盖了当前产品不提供的 composer、传输和网关能力。下表及对应单元测试是现行、可维护的最小契约。

## 当前消费的契约

| 资源 | CC Switch 行为 | 状态来源 |
| --- | --- | --- |
| `models.json` | 管理 `providers` 中的自定义 API Key 供应商；精确新增、替换和移除 | 文件中的实际条目 |
| `settings.json` | 只读 `defaultProvider`、`defaultModel`、`sessionDir` | Pi 原生设置 |
| `auth.json` | 不读、不写、不刷新 | Pi `/login` |
| `AGENTS.md` | 提示库中与文件内容精确匹配的项视为正在使用 | 文件存在及内容 |
| `SYSTEM.md`、`APPEND_SYSTEM.md` | 直接编辑固定原生文件；不存在即未配置 | 文件存在 |
| `prompts/*.md` | 管理顶层斜杠命令模板；空模板是有效原生文件 | 文件存在 |
| `skills/<目录>` | 目录存在即被 Pi 发现 | 原生 Skills 目录 |
| Sessions JSONL | 读取 pinned Pi 的会话头、树分支、消息和会话名称 | 原生会话文件 |

### 供应商

结构化表单只验证并编辑常用字段：

- 供应商级 `name`、`baseUrl`、`apiKey`、`api`、`headers`
- 模型级 `id`、`name`、`reasoning`、`input`、`contextWindow`、`maxTokens`

已有配置中的其他字段原样保留。CC Switch 不组合 Pi 内置供应商，不接管含 `oauth` 的供应商，也不解析或执行 `apiKey`、Header 中的环境变量和命令表达式。请求由 Pi 自己发出。

Pi 的当前供应商和模型只读。启用供应商只把条目加入 `models.json`，不会写 `defaultProvider` 或 `defaultModel`。当前项的移除、删除和当前模型改名会被拒绝。

### 并发与外部修改

写入前比较 CC Switch 已保存内容与原生文件中的实际内容。两者不一致时失败关闭，要求重新读取；不会用数据库中的旧标志覆盖外部修改。

`models.json`、系统提示文件和模板使用原子写入。进程内写操作串行化；跨进程变化通过精确内容或 revision 比较发现。当前不实现跨进程文件锁，因为收益不足以覆盖平台差异和维护成本。

### Sessions

全局会话页只枚举绝对 `sessionDir`、`~` 路径或 Pi 默认目录。相对 `sessionDir` 依赖启动 Pi 时的项目工作目录，CC Switch 没有可靠上下文，因此明确显示“需要项目上下文”，不会猜测目录。

会话解析只消费 UI 所需字段。未知条目被忽略；删除前会验证文件仍在已解析的 Pi 会话根目录中，并核对会话 ID。

## 明确不做

- Pi OAuth 登录、令牌保存和刷新
- 默认供应商或默认模型写入
- 路由、网关、代理、故障转移和请求头合成
- Pi 内置供应商目录的复制或覆盖
- 完整 `compat`、`modelOverrides`、费用和 thinking 映射编辑器
- 相对会话目录的全局猜测

升级 pinned Pi 时，应重新核对上述三个源码哈希，并运行 Pi 供应商、提示词、Skills 和 Sessions 契约测试。没有进入产品界面的上游字段不应仅为了“覆盖完整”而扩展后端。
