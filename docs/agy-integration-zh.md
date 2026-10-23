# Antigravity CLI（agy）接入 CC Switch

## 范围与现状

`agy` 是独立应用 ID，复用 CCS 的供应商、Skills、MCP、会话和用量页面。它与 Gemini CLI 的数据库标记、供应商选择和文件路径分别管理。此次在已有未完成实现上直接补齐；不迁移或覆盖 Gemini CLI 的配置。

| 功能       | 实现                                                           | 数据来源 / 写入目标                                       |
| ---------- | -------------------------------------------------------------- | --------------------------------------------------------- |
| 供应商切换 | 官方登录切模型；第三方 Key 经「打开终端」用私有目录启动     | `~/.gemini/antigravity-cli/settings.json`、`~/.cc-switch/agy/<供应商>/` |
| 模型选择   | 手动填写、第三方 Gemini models API、当前本机 `agy models`      | `config.model.name`，兼容 `env.GEMINI_MODEL`              |
| Token 信息 | 输入、输出、缓存读写；会话消息元数据和统一用量面板             | `brain/<id>/.system_generated/logs/transcript_full.jsonl` |
| MCP        | 导入、编辑、独立启用/禁用、删除、重新同步                      | `~/.gemini/config/mcp_config.json`                        |
| Skills     | 扫描导入、安装、启用/禁用、更新、迁移存储、卸载                | `~/.gemini/config/skills/<name>/`                         |
| 会话       | 列表、摘要、工作目录、消息预览、Token、恢复命令                | 只读 SQLite 索引和 JSONL 日志                             |

本机 CLI 自带文档明确全局定制目录为 `~/.gemini/config/`。公开迁移文档存在不同历史路径，因此本次以安装版本的内置文档和隔离 HOME 实测为准。参考：[官方 MCP 说明](https://www.agy.dev/docs/cli/mcp)、[官方迁移说明](https://www.antigravity.google/docs/cli/gcli-migration/)。MCP 与 Skills 的全局目录可能被同机其他 Antigravity 产品共享，CCS 页面显示实际写入路径。

## 供应商、凭据和模型

供应商在 CCS 中继续使用兼容现有 Gemini 表单的数据结构：

```json
{
  "env": {
    "GEMINI_API_KEY": "your-key",
    "GOOGLE_GEMINI_BASE_URL": "https://your-gemini-endpoint"
  },
  "config": {
    "model": { "name": "Gemini 3.1 Pro (High)" }
  }
}
```

这是 CCS 的存储结构，并不原样写入 agy。agy 的 `settings.json` 里 `model` 只认显示名（如 `Gemini 3.8 Flash (Low)`），写模型 ID 会被静默忽略并回落到默认模型，所以两种模式都存显示名。

官方身份由分类或明确的官方预设标记决定；供应商名称含 Google 不会自动删除其 API Key。第三方供应商须兼容 Gemini 协议，不能将任意 OpenAI/Anthropic 端点直接当作 Gemini 使用。

agy 只从进程环境读 Key 和端点，不读取任何文件里的 Key，`modelProvider` 又是全局开关：写进真实 `settings.json` 后，没有带 Key 启动的 agy 会直接报错。所以两种卡分开处理：

- **官方登录卡**：切换时只改真实 `settings.json` 的 `model`，并去掉 `modelProvider`，主题、受信工作区等其他配置保留。登录凭据由 agy 自己保管（不在 `~/.gemini` 里），CCS 不读也不碰。在哪个终端运行 `agy` 都生效。
- **第三方 Key 卡**：只在卡片「打开终端」启动的 agy 里生效。启动命令是 `agy --gemini_dir=<私有目录>`，私有目录为 `~/.cc-switch/agy/<供应商 ID>/`：其中 `antigravity-cli/settings.json` 写 `modelProvider: "gemini"` 和模型（权限 `0600`，不含 Key），`config` 软链回 `~/.gemini/config`（Windows 先试符号链接，失败时用目录联接），MCP、Skills 和提示词照常共享。会话也存在私有目录里，删除供应商不会删掉它。切换到 Key 卡时，真实 `settings.json` 只去掉 `modelProvider`，模型不动，直接运行 `agy` 仍是官方登录。
- **Key 怎么传**：macOS / Linux 的启动脚本先读入一个 `0600` 的临时文件，读完立即删除，再经 `env` 把 Key 和地址交给这一个 agy 进程；卡上没填地址时用 `env -u` 清掉用户 rc 里可能 export 的同名变量，以免被 rc 覆盖或误用。Windows 写进 `.bat` 的 `setlocal` 段，agy 退出后随 `endlocal` 撤掉；含非 ASCII 的路径按 #8034 的办法经环境变量转一手。Key 不出现在命令行参数里，也不写进任何常驻文件。
- **不改 shell 启动文件，也不装包装脚本。** #8268 的测试版装过 `~/.gemini/antigravity-cli/bin/agy` 包装脚本、`env` 文件和 shell PATH 片段，新版启动时会清理：只删带 CCS 标记的脚本、`env` 文件和 PATH 片段（只剩该片段的文件整个删掉），然后按新规则把当前供应商重写一次，让 `modelProvider` 回到登录模式。没有标记的同名文件和用户的其他内容原样保留。
- 真实 `agy` 的定位顺序（`get_agy_models` 用）：PATH（跳过旧版带标记的包装脚本）、`~/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin`，最后用登录 shell 执行一次 `command -v agy`（限时 2 秒）。从 Dock / Finder 启动 CCS 时 PATH 只有系统目录，所以后几步不可省。「打开终端」按终端里的 PATH 找 agy，不用这个解析。
- 已经运行的 agy 进程不会热切换。

模型配置和当前供应商指针共用现有 `AppWrite` 操作日志。写入前验证 JSON；发布中断后使用已有恢复机制补完文件及指针。损坏配置不会按空文件重建；agy 还没写出 `settings.json` 时切到 Key 卡也不会替它创建空文件。

新增供应商遵循 CCS 原有生命周期，前端不再无条件追加一次切换。原生 `settings.json` 不存在时返回配置缺失，启动导入不会生成虚构的 default/current。agy 首次启动自己会写出不含供应商信息的 `settings.json`，这样的配置也不会被导入：只有带模型的配置才会作为 `default` 导入。全量同步（配置导入、云同步）对没有真实内容的当前供应商不执行任何写入。官方模型按钮读取当前本机 agy 配置；目标供应商尚未启用或尚未登录时仍可手工填入模型。

## 数据持久化和兼容性

数据库版本从 20 升至 21，为 `mcp_servers`、`skills` 分别增加 `enabled_agy BOOLEAN NOT NULL DEFAULT 0`。新建数据库与升级迁移均覆盖；其他应用的开关和数据不变。旧的导入 JSON 没有 `agy` 字段时默认关闭。

MCP 支持 stdio 和 Streamable HTTP，兼容导入旧 `serverUrl`，拒绝 agy 不支持的 SSE。禁用写 `disabled: true`，保留本地选项；删除仍启用于 agy 的服务器会移除对应原生条目。数据库的关闭标记不记录托管历史，因此删除未启用于 agy 的服务器时保留原生配置，即使同名禁用条目的传输字段完全相同；如需删除保留的禁用条目，应在 agy 中操作。切换传输方式时清理旧传输字段，保留非传输选项和未托管服务器。数据库提交失败会恢复原文件；重新同步不会因为数据库中的默认关闭标记而删掉未托管条目。

Skills 复用既有 SSOT、下载、链接/复制和备份机制，新增独立持久开关。对同名原生目录先核对内容和归属；外部修改不静默覆盖。更新使用暂存副本，数据库失败时恢复旧文件；显式切换也支持回滚。迁移存储时重定向托管链接，卸载前验证选中的原生副本，保留未启用的外部同名目录。

## 会话和 Token 语义

会话同时扫描 CLI 的 `~/.gemini/antigravity-cli/brain` 与 Antigravity IDE 的 `~/.gemini/antigravity/brain`（布局相同，各自带同级 `conversation_summaries.db`）；缺失的目录会被忽略，同一会话 ID 只计一次。

SQLite 以只读方式打开，仅读取 `conversation_summaries` 的标题、预览、工作目录和时间。列表优先使用索引，避免每次刷新都解析所有正文；没有索引时回退解析 JSONL。只有二进制记录、没有可读日志的会话仍可展示摘要和恢复命令，不伪造正文。

JSONL 按 `step_index` 排序，同步骤重复写入取最后一条，容忍尾部未写完的数据。当前已实测 `USER_INPUT`、`PLANNER_RESPONSE`，其他步骤保留文本供预览。路径归属经过校验，目录中的链接不能将会话读取引向根目录外。

恢复命令为 `agy --conversation=<id>`。CCS 不删除 agy 会话，因为正文、二进制存储和摘要索引需由 agy 一起维护；界面和后端均明确要求在原应用中删除。

Token 用量只导入完成的模型响应。实测使用本地模拟 Gemini SSE 响应：输入总量 100、缓存 20、回答 5、思考 7，agy 导出的结果为：

- `input_tokens = 80`：已经排除缓存，数据库明确标为 fresh input。
- `cache_read_tokens = 20`：单列展示，不从 80 再扣一次。
- `output_tokens = 12`：已经包含思考，不再额外累加 7。

该合成样例保存在 `tests/fixtures/sessions/agy.jsonl`，不含真实对话或凭据。用量按会话 ID＋步骤 ID 去重，去重账本与明细一起事务提交；旧明细被保留策略清理后也不会重复导入。已完成步骤被改写时只更新仍然保留的明细。

原生日志通常不含历史模型与真实账单，缺失模型记为 `unknown`，不从当前供应商反推历史归属。费用保留为 0，并在用量页解释“0 不代表免费”；这是可用数据的限制。官方账号 OAuth 凭据、订阅额度及计费不由此次接入托管。

## 验证

连通性检查对第三方 agy 供应商读取 `env.GOOGLE_GEMINI_BASE_URL`（沿用 Gemini 的提取规则），未设置时使用 `https://generativelanguage.googleapis.com`；官方登录供应商不提供检测目标。

新增测试覆盖迁移与新建数据库、独立开关、MCP 格式与失败回滚、Skills 原生文件保护及失败恢复、终端启动的 Key 传递（不进命令行、rc 里的同名变量不覆盖、临时文件读完即删）、Key 卡切换不写凭据也不碰 shell 文件、私有目录与共享 config 的软链、旧版包装脚本和 PATH 片段只删带标记内容、真实 agy 的多位置解析、连通性检查、配置事务中断恢复、会话路径验证、JSONL 重写/残缺尾部、原生 Token 语义和持久去重。

此外使用本机真实 agy，在临时 HOME 中验证 MCP add/disable 的原生格式、models 输出格式及本地模拟 SSE 请求。模拟端点运行在 `127.0.0.1`，凭据为固定测试值；不连接付费模型，也不修改实际登录状态。

可复查命令：

```sh
pnpm typecheck
pnpm test:unit
cargo test --manifest-path src-tauri/Cargo.toml --lib agy -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
pnpm build:renderer
```

此次不增加 agy 本地代理、故障转移或 Stack 模式；这些能力要求单独适配数据平面，不能仅通过复用 Gemini UI 宣称可用。

## 原生联网复核与回归

补充复核发现并修复两处表单问题：

- agy 只有 API Key、未覆盖端点时，模型查询应使用 `https://generativelanguage.googleapis.com`，不能要求用户额外填写默认地址。
- agy 的模型输入框需要按后端相同的优先级读取 `config.model.name`，并在编辑时同步写回；否则旧的 `config.model.name` 会覆盖刚修改的 `GEMINI_MODEL`。Gemini CLI 保持原有环境变量行为。

新增 `src-tauri/src/agy_smoke_tests.rs` 原生联网测试，默认 `ignore`，仅在明确授权调用 API 后手工运行。它要求 `CCS_AGY_SMOKE_CONFIG` 指向权限受限的临时供应商 JSON，`CCS_AGY_SMOKE_BIN` 指向真实 agy；测试本身不读取用户数据库。所有切换、文件、Skills 和会话写入均在临时 HOME 和临时工作目录中执行，结束后清理。

已使用真实 Gemini API 完成以下验证：

1. 配置投影及当前供应商指针生效；按「打开终端」的方式（私有 `--gemini_dir` 加子进程环境变量）启动的原生 agy 实际请求到配置的模型，CCS 写出的文件里都没有 Key。
2. Skill 经 CCS 导入、禁用、重新启用后，被原生 agy 加载并返回预设标记 `CCS_AGY_SKILL_OK`。
3. 原生 agy 识别 CCS 写出的 MCP 配置并完成本地测试服务的 `initialize` 与 `tools/list`；此测试验证连接和工具发现，不覆盖任意第三方 MCP 业务工具的执行。
4. CCS 会话预览包含实际回答，导入完成响应的用量明细，重复导入不新增记录。

无界面 `--print` 模式不会弹出权限确认。Skill 若需要 `read_file`，必须有对应读取许可，否则 CLI 可能退出为成功、JSON 的 response 却为空并带有 `denied_actions`。测试仅为临时 Skill 的实际路径及 SSOT 路径授予精确读取许可，保留正常运行时的用户权限决策。

模型 ID 必须精确匹配所用接口。实测 Gemini API 接受 `gemini-3.8-flash`，而 `gemini-3-8-flash` 返回 404；不应通过全局替换连字符“修复”任意供应商模型名。原生 `agy models` 的档位别名与 Google API 模型列表也并不完全相同，表单按登录模式选择对应来源。

## 深链接导入

统一入口是 `ccswitch://v1/import`，agy 使用独立的 `app=agy` / `apps=agy`：

- **供应商**：`resource=provider&app=agy`，支持 `name`、`apiKey`、`endpoint`、`model`、`config` 与 `apiFormat=gemini_native`。不支持 OpenAI/Anthropic 协议。确认框关闭「导入后立即切换」时只保存数据库，不创建原生配置；开启时走正常供应商切换。带 Key 的供应商导入后经卡片「打开终端」使用。
- **内联配置**：`config` 是 Base64 编码 JSON，可用上文 CCS 的 `{env, config:{model:{name}}}` 结构，或扁平 `GEMINI_API_KEY`、`GOOGLE_GEMINI_BASE_URL`（兼容 `GEMINI_BASE_URL`）、`GEMINI_MODEL`。模型优先级为 URL 的 `model` > `config.model.name` > `GEMINI_MODEL`。只提取密钥、地址、模型，其他环境变量与原生选项不导入；预览中的密钥打码。远程 `configUrl` 仍不支持。
- **MCP**：`resource=mcp&apps=agy`；`apps=agy,codex` 可同时指定多个应用。`config` 为 Base64 编码的 `{"mcpServers":{...}}`。同 ID 已存在时保留数据库中的服务器定义，仅合并应用开关，再走统一 MCP 同步；agy 不支持的 SSE 会报告导入失败。
- **Prompt**：`resource=prompt&app=agy`，`content` 为 Base64 Markdown，确认后保存到 agy 提示词列表；启用时使用既有 Prompt 路径解析和写入逻辑。
- **Skills**：`resource=skill&repo=owner/name&branch=main` 只添加所有应用共享的技能仓库，不下载或安装技能，也不会因附带 `app=agy` 自动启用。随后在 Skills 页面选择技能并为 agy 启用。

供应商示例（所有凭据都是占位值；链接参数需 URL 编码）：

```text
ccswitch://v1/import?resource=provider&app=agy&name=Antigravity&endpoint=https%3A%2F%2Fgenerativelanguage.googleapis.com&apiKey=YOUR_KEY&model=YOUR_MODEL&apiFormat=gemini_native&enabled=false
```

深链接供应商沿用现有 API Key 导入规则，需要密钥和端点；不通过链接导入或接管 OAuth 登录。会话预览、Token 统计和会话恢复不是导入资源，没有新增对应 `resource` 类型。


### 原生登录与 API 凭据

Key 和自定义地址均留空时保留 agy 自身登录；仅修改模型不要求 API Key。填写 Key 时可留空地址以使用 Google 默认端点；自定义地址需要非空 Key。编辑器中的设置只投影 agy 支持的字段，不会改动 Gemini CLI 全局配置。

### 原生登录模型名称

原生登录模式应使用 `agy models` 输出第二列的显示名（例如 `Gemini 3.8 Flash (Low)`），模型列表会填入该值。第一列 CLI 别名适用于命令行 `--model`；直接把别名保存到原生 `settings.json.model` 会静默回退默认模型。API Key 模式同样写显示名，而且只能选 agy 内置清单里的 Gemini 模型（如 `gemini-2.5-flash` 会被拒绝）；agy 每次还会附带一个 `gemini-3.1-flash-lite-preview` 的辅助请求，第三方地址需要支持这些模型。
