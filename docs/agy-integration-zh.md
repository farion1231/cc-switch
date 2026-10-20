# Antigravity CLI（agy）接入 CC Switch

## 范围与现状

`agy` 是独立应用 ID，复用 CCS 的供应商、Skills、MCP、会话和用量页面。它与 Gemini CLI 的数据库标记、供应商选择和文件路径分别管理。此次在已有未完成实现上直接补齐；不迁移或覆盖 Gemini CLI 的配置。

| 功能       | 实现                                                           | 数据来源 / 写入目标                                       |
| ---------- | -------------------------------------------------------------- | --------------------------------------------------------- |
| 供应商切换 | 官方登录、Gemini API Key、端点、模型；编辑当前供应商后重新应用 | `~/.gemini/antigravity-cli/settings.json`、`env`          |
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
    "model": { "name": "your-model-id" }
  }
}
```

这是 CCS 的存储结构，并不原样写入 agy。投影仅修改原生 `settings.json` 的 `modelProvider` 与 `model` 两项，保留主题、受信工作区和其他配置。API Key 模式设置 `modelProvider: "gemini"`；官方登录移除该字段，并清除启动器中的 API Key 和自定义端点，不接管 agy 自身的登录令牌。

官方身份由分类或明确的官方预设标记决定；供应商名称含 Google 不会自动删除其 API Key。第三方供应商须兼容 Gemini 协议，不能将任意 OpenAI/Anthropic 端点直接当作 Gemini 使用。

agy 从进程环境读 Key 和端点，不读取 Gemini CLI 的 `.env`。CCS 因此写入权限为 `0600` 的专用 `env`，并生成 `~/.gemini/antigravity-cli/bin/agy` 启动器。启动器先清除继承的旧值，再加载当前配置，最后调用真正的 CLI：

- 凭据和路径进行 shell 引号转义，不拼接执行凭据内容。
- 手动传入 `--model` 优先于 CCS 默认模型。
- `mcp`、`models`、`plugin` 等子命令不追加会话模型参数。
- 同名但没有 CCS 标记的启动器不会被覆盖。
- 自动 PATH 设置支持 macOS/Linux 的 zsh、bash、POSIX sh；Windows 和其他 shell 的供应商启动器接入会明确报错，不伪装成已生效。
- 真实 `agy` 的定位顺序：PATH（排除启动器自身及任何带 CCS 标记的脚本）、`~/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin`、`~/.gemini/antigravity-cli/bin`，最后用登录 shell 执行一次 `command -v agy`（限时 2 秒）。从 Dock / Finder 启动 CCS 时 PATH 只有系统目录，所以后几步不可省。安装时仍找不到则不会写入虚构路径：启动器在运行时按终端的 PATH 再查找（同样跳过自身，避免递归），仍找不到时输出 `command not found` 并以 127 退出；此时本机本来就没有 agy，不会遮蔽任何已有命令。`get_agy_models` 使用同一解析，找不到时提示先安装 agy。
- 启动器与 PATH 片段只在当前供应商带 Key 或自定义端点（第三方）时安装。切换到官方登录模式、或删除供应商后已没有第三方当前供应商时，CCS 会移除自己安装的启动器（含 CCS 标记）和 `.zshenv`、`.bashrc`、`.bash_profile`、`.bash_login`、`.profile` 中带标记的 PATH 片段；无标记的同名脚本和用户的其他内容原样保留，只含我们片段的文件会随之删除。
- PATH 只追加带标记的片段，保留现有启动配置。切换后需要新开终端或重启 agy；已经运行的进程不会热切换。

Key 文件、模型配置和当前供应商指针共用现有 `AppWrite` 操作日志。写入前验证 JSON；发布中断后使用已有恢复机制补完文件及指针。启动器安装是幂等的基础设施准备，发生在配置事务前；不包含某一家供应商的 Key。损坏配置不会按空文件重建。

新增供应商遵循 CCS 原有生命周期，前端不再无条件追加一次切换。原生 `settings.json` 与 `env` 均不存在时返回配置缺失，启动导入不会生成虚构的 default/current，也不会使随后的全量同步创建 agy 启动器或修改 shell 文件。agy 首次启动自己会写出不含供应商信息的 `settings.json`，这样的配置也不会被导入：只有带 Key、端点或模型的配置才会作为 `default` 导入。全量同步（配置导入、云同步）对没有真实内容的当前供应商不执行任何写入，也不会安装启动器、修改 shell 文件，Windows 上也不会再报告 `provider/agy` 失败（Windows 仅在确有第三方供应商需要启动器时才提示不支持）。官方模型按钮读取当前本机 agy 配置；目标供应商尚未启用或尚未登录时仍可手工填入模型。

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

新增测试覆盖迁移与新建数据库、独立开关、MCP 格式与失败回滚、Skills 原生文件保护及失败恢复、启动器转义和环境清理、空配置不导入且全量同步不接管启动器/shell、官方模式与删除后的启动器及 PATH 片段清理、真实 agy 的多位置解析与运行时回退、连通性检查、配置事务中断恢复、会话路径验证、JSONL 重写/残缺尾部、原生 Token 语义和持久去重。

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

1. CCS 后端查询模型，配置投影及当前供应商指针生效，生成的启动器实际调用 `gemini-3.8-flash`。
2. Skill 经 CCS 导入、禁用、重新启用后，被原生 agy 加载并返回预设标记 `CCS_AGY_SKILL_OK`。
3. 原生 agy 识别 CCS 写出的 MCP 配置并完成本地测试服务的 `initialize` 与 `tools/list`；此测试验证连接和工具发现，不覆盖任意第三方 MCP 业务工具的执行。
4. CCS 会话预览包含实际回答，导入完成响应的用量明细，重复导入不新增记录。

无界面 `--print` 模式不会弹出权限确认。Skill 若需要 `read_file`，必须有对应读取许可，否则 CLI 可能退出为成功、JSON 的 response 却为空并带有 `denied_actions`。测试仅为临时 Skill 的实际路径及 SSOT 路径授予精确读取许可，保留正常运行时的用户权限决策。

模型 ID 必须精确匹配所用接口。实测 Gemini API 接受 `gemini-3.8-flash`，而 `gemini-3-8-flash` 返回 404；不应通过全局替换连字符“修复”任意供应商模型名。原生 `agy models` 的档位别名与 Google API 模型列表也并不完全相同，表单按登录模式选择对应来源。

## 深链接导入

统一入口是 `ccswitch://v1/import`，agy 使用独立的 `app=agy` / `apps=agy`：

- **供应商**：`resource=provider&app=agy`，支持 `name`、`apiKey`、`endpoint`、`model`、`config` 与 `apiFormat=gemini_native`。不支持 OpenAI/Anthropic 协议。确认框关闭「导入后立即切换」时只保存数据库，不创建原生配置、启动器或修改 shell；开启时走正常供应商切换。Windows 仍受供应商启动器的既有限制。
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

原生登录模式应使用 `agy models` 输出第二列的显示名（例如 `Gemini 3.8 Flash (Low)`），模型列表会填入该值。第一列 CLI 别名适用于命令行 `--model`；直接把别名保存到原生 `settings.json.model` 可能静默回退默认模型。API 模式继续使用接口模型 ID。

### zsh 启动与恢复

API 模式会在 `.zshenv`、`.zprofile`、`.zshrc` 以及已有的 `.zlogin` 末尾维护带标记的 PATH 片段，使登录或交互终端中的后续 PATH 重设不会绕过启动器。重复应用不会重复追加。切回登录或释放启动器时仅移除当前包装目录的完整片段，保留其他目录的同名标记和原文件内容，包括末尾无换行的文件。
