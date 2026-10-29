# DeepSeek Harness 接入

DeepSeek Harness 的应用 ID 为 `dsh`，命令为 `dsh`，npm 包为 `@deepseek-ai/dsh`。接入参考 Qoder CLI 和 Kimi Code 的原生配置管理方式：供应商共存，直接连接原生客户端，不启用 CC Switch 本地路由。

## 安装、升级与卸载

在「应用」页找到 **DeepSeek Harness**，可以检查版本、安装、升级、诊断多处安装、预览并卸载指定位置的安装。

- 安装：`npm i -g @deepseek-ai/dsh@latest --engine-strict`。
- 升级：沿用现有工具管理机制，优先定位实际安装位置及包管理器；npm 升级保留 `--engine-strict`。
- npm 手动卸载：`npm uninstall -g @deepseek-ai/dsh`。自动卸载核对包名和实际 npm prefix，只移除所选安装。
- 配置、凭据、Skills 和会话数据保留。无法确认归属的安装沿用现有手动处理流程。

安装和升级保留 npm 的 `--engine-strict`，由 npm 校验发布包及依赖的运行时要求并报告兼容性错误。

安装后在终端运行 `dsh web` 打开官方 Web 界面。

## 供应商与模型

在侧栏选择 DeepSeek Harness，添加供应商时可以选择已有的厂商、第三方平台、云服务商、本地服务预设，也可手动配置。预设复用已维护的 Pi 厂商信息，并转成 DeepSeek Harness 的原生协议、模型、请求头和兼容参数；仅提供原生适配器支持的三种协议：

- OpenAI Chat Completions：`openai-completions`。
- OpenAI Responses：`openai-responses`。
- Anthropic Messages：`anthropic-messages`。

添加的供应商写入 `web`、`desktop`，以及已初始化的 `headless`、`acp` profile 的 `llm-pi-ai.config.providers`。Web 或 Desktop 缺少 `cordis.patch.yml` 时会提前创建；客户端首次启动时保留已有补丁并补齐其他文件（[上游 `initProfile` 实现](https://github.com/deepseek-ai/deepseek-harness/blob/f3803c96974ad4a94195fb97e5c637e9c2a3afe3/packages/boot/app-boot/src/profile.ts#L223-L249)）。`sdk`、`sdk-minimal` 供嵌入开发使用，不写。添加、编辑、移除对所有目标 profile 一起做。多个供应商可以同时添加；「移除」只从原生配置移除，保留 CC Switch 中可再次添加的档案；删除档案同时移除其原生节点。供应商 key 不覆盖同名节点。已有配置在列表刷新时导入，原生节点被其他程序移除后，档案变为未添加。

列表和「已添加」状态读取**所有**已初始化 profile，`web`、`desktop` 优先：只在某个 profile 里存在的供应商（例如在 DeepSeek Harness 的 headless 界面里原生添加的）也会显示出来，不会出现「列表里没有、添加时却说 key 已占用」的死角。每个 profile 里的同名节点内容必须与优先显示的那份一致：如果用户在 DeepSeek Harness 原生界面里改了其中一份（换端点、换模型、换凭据引用），CC Switch 编辑或移除时会拒绝写入并指出是哪个 profile 不一致，而不是用优先显示的那份静默覆盖。MCP 条目比较实际存在的副本：优先显示的 profile 缺少某条 MCP 时，使用其他 profile 中已有的条目作为比较基准；缺少副本不会阻止编辑或删除，内容确实不同才报错。

启动 CC Switch 或打开 DeepSeek Harness 供应商页时，会检查首次遇到的目标 profile。尚无受管供应商的 profile 会补入 `liveConfigManaged` 为真的供应商，尚无受管 MCP 的 profile 会补入已为 DSH 启用的服务器；现有原生配置保留。完成记录保存在 `$DSH_HOME/.cc-switch-profile-sync.json`，和本次配置写入一起提交、失败一起回滚。以后用户在原生客户端删除条目，即使删空，也不会因重启或刷新页面反复补回。

供应商的模型出现在 DeepSeek Harness 的模型选择器中。默认模型和 `sdk` 类 profile 通过 DeepSeek Harness 原生界面管理。CC Switch 的「添加」不会更改原生默认模型。

`$DSH_HOME/cordis.patch.yml`（home 级补丁）在所有 profile 之后生效。如果其中有 `llm-pi-ai` 的 `config`，写进任何 profile 的供应商都会被盖住，此时添加、编辑直接报错「供应商被 home 级补丁覆盖，请先在该文件里移除」。仍可移除 profile 中的条目或删除 CC Switch 档案。

API Key 写入原生 `.credentials.yaml` 的 `refs`，profile 仅保存 `apiKeyEnv` 引用。只有登录 `records`、尚未建立 `refs` 或 `refs` 为 null 的原生凭据文件也可读取，首次写入 Key 时再创建 `refs` 并保留登录记录。编辑已有供应商且 Key 不变时保留其原生引用；改用新 Key 时使用 CC Switch 为该供应商生成的独立引用。移除供应商保留凭据，避免影响可能复用同一引用的配置。CC Switch 的供应商档案仍沿用项目现有的本机保存和同步机制。

## 数据位置

| 能力 | 原生位置与行为 |
| --- | --- |
| 数据目录 | `$DSH_HOME`，未设置时为 `~/.dsh` |
| 供应商、模型 | `profiles/{web,desktop}/cordis.patch.yml`，以及已有的 `headless`、`acp` 补丁中的 `llm-pi-ai` 插件 override |
| API Key | `.credentials.yaml` 的 `refs`；保留其他 refs 与 records |
| MCP | 同一组 profile 的 `insert` 列表中每个服务器一个 `@deepseek-ai/dsh-mcp-client` 插件条目 |
| DeepSeek 账号（OAuth） | `.credentials.yaml` 的 `records["deepseek-account-platform/default"]`，Web 与 Desktop 共用 |
| Skills | `skills/<目录>/SKILL.md` |
| 全局提示词 | `AGENTS.md` |
| 会话 | `sessions/<项目>/<会话>/session.vN.jsonl` 或 `.jsonl.zstd` |

`DSH_HOME` 需要由 CC Switch 进程继承。Web/CLI、Desktop 共用 `$DSH_HOME` 下的凭据、会话、Skills 和 `AGENTS.md`，只有 profile 各自独立。

## DeepSeek Harness Desktop

Desktop 不单独作为一个应用：它和 Web 共用凭据、会话、Skills 和 `AGENTS.md`，供应商和 MCP 也总是同时写进 `web` 和 `desktop` 两个 profile，所以侧栏和「应用」页只有一个 DeepSeek Harness，它的供应商、MCP、Skills、提示词同时作用于 Web 和 Desktop。

## DeepSeek 账号（OAuth）

授权中心有 DeepSeek Harness 一组：可以用浏览器登录 DeepSeek 账号，流程与 DeepSeek Harness Web 相同（PKCE S256，`auth-api/v0/dsh/auth_init`、`auth_exchange`，本机 `127.0.0.1` 回调，取消时 `auth_cancel`），登录写进 Web 与 Desktop 共用的 `.credentials.yaml`，运行中的 DeepSeek Harness 会自动重新读取。也可以在 DeepSeek Harness 里登录，CC Switch 读取同一份记录，显示账号、充值余额和赠金余额，退出登录时两端同时退出（先删本地记录，再尽力通知平台）。查询账号和远程退出前都会核对凭据的 `issuer`；账号令牌只发往 `https://platform.deepseek.com`，请求不跟随重定向。其他 issuer 的凭据仍可在本地退出，不会向它发送请求。

## 会话与用量

Web 和 Desktop 共用 `$DSH_HOME/sessions`，会话里没有记录是哪一端创建的，因此会话页和用量页把两者合并为一个 DeepSeek Harness 来源。用量导入器（沿用 [PR #8771](https://github.com/farion1231/cc-switch/pull/8771)）读取 `assistant/message` 的 token 用量；走 DeepSeek 账号路由（`source.provider` 为 `deepseek-account`）的请求单独记为「DeepSeek 账号（OAuth）」，其余记为会话日志。

用量导入按字节游标增量读取：游标记下已处理到的文件位置，每次同步只读追加的那一段、只解压新增的 zstd 帧，不再整份重读重解压（长会话每次同步的代价与新增量成正比，而不是与会话总长度成正比）。未完成请求的 `step/start` 时间与游标一起保存到数据库，`assistant/message` 即使落在下轮同步或应用重启后，也能计算耗时和首字时间；旧版游标会重读一次以恢复上下文，并沿用去重账本。末尾写了一半的帧不会被跨过，游标停在最后一个完整帧，等下一轮补全；日志被重写或变短时从头重扫，去重账本保证不会重复入库。

MCP 支持 stdio 和 Streamable HTTP，SSE 会提示改用 HTTP。新增使用 DSH 的 `insert` 补丁创建插件，读取、修改和移除支持同一插入列表中的多个插件；移除最后一个补丁时写回 `[]` 并保留文件头注释。修改保留原生超时、重连等扩展字段；更换传输类型时移除旧的启动或连接参数。发生数据库保存失败时恢复原生文件。

Skills 复用现有应用同步、导入、更新和迁移机制，并沿用 Kimi 的副本归属保护：不会在普通重新同步或卸载未启用的技能时删除用户自行安装的同名目录。

会话页支持阅读正文、思考、工具调用、工具结果及模型与 token 元数据。阅读器保留原始对话和完整工具输出，跳过只修改模型历史的 `surfaceOp: { op: 'replace', ... }` 替换副本，因此自动压缩摘要和工具输出裁剪不会覆盖用户看到的记录。压缩日志按完整的独立 zstd 帧读取，尚未写完的最后一帧或 JSONL 行留待下次刷新；同一会话目录只选择最高版本的日志。恢复和删除在 DeepSeek Harness 原生界面完成。

## 配置保留与支持边界

原生写入使用原子替换，并遵循 DeepSeek Harness 的 `<文件>.lock` 独占文件锁与 PID 记录。供应商、凭据、MCP 写入与数据库提交之间失败时恢复写入前的内容。

修改保留其他顶层补丁的原始文本（含注释与 `!!js`），并保留目标插件中的其他供应商、配置项及未编辑的扩展字段。被修改的顶层补丁（含其 `insert` 列表）及凭据文档会重新序列化，其中的 YAML 注释和排版不保证保留。无法解析的 YAML、重复插件 ID、无法安全定位的 MCP 条目会报错并保留原文件。供应商本身含动态 YAML 时请在原生配置中编辑。

原生写入在一次修改中同时提交所有目标 profile 与凭据文件：每个文件各自持有 DSH 原生的 `<文件>.lock`，先算出全部新内容再写盘，任一文件或数据库提交失败时全部还原。改写后的 profile 写盘前重新解析一次，删除条目会让别处的别名失去锚点时拒绝写入。PR #8771 的合并注意事项见 [兼容性分析](deepseek-harness-pr8771-compatibility.md)。

残留的 `<文件>.lock`（上一个写入进程崩溃留下）不会被当成永久占用：锁里记录的 PID 已经不存在的直接接管，两个平台再用修改时间兜底；写入方还活着时仍然按冲突处理，不会两个进程同时改一个文件。

`headless` profile 的默认组合把 `credentials` 插件关掉了（`dsh --profile headless --dump-config` 里是 `disabled: true`），所以 `apiKeyEnv` 引用在 headless 里解析不到，用 CC Switch 添加的供应商在 headless 里会报 `MISSING_CREDENTIAL`；`web`、`acp` 和 Desktop 正常。需要在 headless 里用同一个 Key 时，把 `CC_SWITCH_DSH_<供应商>` 导出到环境变量，或在那个 profile 里启用 `credentials` 插件。

## 验证记录

在 Windows 本机完成以下检查：

- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo fmt --check`、`cargo clippy --lib -- -D warnings` 通过。
- `cargo test --lib dsh -- --test-threads=1`：21 项通过（其中 1 项为名称匹配到的既有 WebSocket 测试）；覆盖出厂 `[]` 模板首次添加供应商和 MCP、`insert` 条目的读写与移除、空 profile 往返、登录凭据缺少 `refs`、原生工具结果及错误状态，以及保留压缩前原始对话和完整工具输出。
- `cargo test --test dsh_commands`：2 项通过，覆盖完整供应商操作、原生配置保留、MCP 写入失败时的文件与数据库一致性。
- `cargo test --test mcp_commands -- --test-threads=1`：36 项通过，默认 MCP 同步目标包含 `dsh`。
- 将实际 MCP 新增、修改和移除流程生成的补丁交给 DSH 发布包中的 `applyEntryPatches`：三阶段均无警告，新增后确实出现在插件列表中，修改和移除结果符合预期。
- DeepSeek 预设 6 项前端测试通过；使用 `@deepseek-ai/dsh-llm-pi-ai@0.2.1-rc.2` 发布包中的推理档位和兼容字段校验函数，验证全部 113 个预设、387 个模型通过。
- 前端全量首轮共 3269 项，3263 项通过、6 项失败。更新新增应用涉及的来源数量、隐藏应用 fixture、Skills 导入字段和图标文字定位断言后，涉及的 80 项用例分别复测全部通过；其中应用集成测试单独运行通过。
- Rust 全量库测试首轮 3944 项通过、11 项忽略，1 项既有测试 `proxy::error::tests::describe_reqwest_error_keeps_cause_and_drops_url` 在本机失败：它要求连接被拒绝，环境返回了 HTTP 502；单独重跑仍失败。该测试及其实现未被本次修改。
- 与 PR #8771 解决冲突后的隔离合并树：前端类型检查通过，用量面板和预设 6 项测试通过，PR 的用量导入 9 项测试通过。


在 macOS（Apple silicon）上用真实安装的 DSH 复验了与 profile 读写、用量导入相关的行为：

- `npm i -g --prefix <dir>` 安装 `@deepseek-ai/dsh@0.2.1-rc.2`，用 `dsh web/headless/acp` 真机生成 `web`、`headless`、`acp` 三个 profile，`desktop` 按 Electron 模板等价物铺好。
- 多 profile 读取与冲突保护：只在 `headless` 里存在的原生供应商会出现在列表里；添加同名 key、以及编辑/移除与优先 profile 不一致的副本时都报错并指名 profile，原生节点一个字节不变（`dsh --profile headless --dump-config` 确认 DSH 侧仍能应用该节点）。
- 用量导入：用本地 OpenAI 兼容 mock 端点跑出 25 回合真实会话（`session.v4.jsonl.zstd`，111 帧 / 200646 B / 解压后 605382 B），`inputTokens=334`、`outputTokens=56`、`cacheReadTokens=900`、`latency≈695ms`、`first_token≈290ms` 与写入端一致。
- 增量读取：同一份真实内容按 1×/4×/16× 复制帧后，首次全量 19/72/281 ms，此后每次追加一帧的同步为 198/431/514 µs，与日志总长无关（修复前每次都是 21/74/298 ms 的全量重扫）。

## 原生格式依据

2026-10-24 核对官方源码 `22ddee3792402c3e70ec5f86a63c3e3e895a2f60`：

- [模型配置](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/docs/user/guide/providers.md)。
- [pi-ai 适配器](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/llm/llm-pi-ai/README.md)。
- [MCP](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/mcp/mcp-client/README.md)。
- [Skills](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/skill/skill-filesystem/README.md)。
- [全局指令](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/context/agent-instructions/README.md)。
- [会话阅读与模型历史的区别](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/core/session/src/surface.ts#L78-L93)。
- [凭据存储](https://github.com/deepseek-ai/deepseek-harness/blob/22ddee3792402c3e70ec5f86a63c3e3e895a2f60/packages/credentials/credentials-local/README.md)。
