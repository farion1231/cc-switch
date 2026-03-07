# Core / Tauri / CLI 迁移差异审查报告

审查范围：
- `crates/cc-switch-core`
- `src-tauri/src/commands`
- `src-tauri/src/services`
- `crates/cc-switch-cli`

审查时间：2026-03-06

审查方式：静态代码审查，只做 review，不改业务代码。

## 审查目标

本次不把问题仅仅看成“CLI 还有很多命令没写完”，而是把目标架构先摆正：

- `cc-switch-core` 应该成为共享后端，承接领域模型、数据库访问、Live 配置读写、导入导出、deeplink、proxy/failover/usage 等核心能力。
- `src-tauri` 应该收敛为桌面壳层，负责 Tauri command 暴露、`AppHandle`、窗口事件、托盘、文件对话框、打开目录、重启应用等平台相关能力。
- `crates/cc-switch-cli` 应该只是一个薄适配层，主要负责参数解析、调用 core、统一输出、测试命令行为。

## 总体结论

当前真正卡住 CLI 的根因，不是 handler 写得少，而是 `cc-switch-core` 还没有成为 `src-tauri` 后端能力的超集。

- Provider 只对齐了一部分基础 CRUD，复杂切换、Live 同步、默认配置导入、自定义 endpoint、usage 脚本等能力仍主要留在 `src-tauri`。
- MCP、Prompt、Skill、Proxy、Failover、Usage、Deeplink、Import/Export、Settings 这些域都存在明显的“tauri richer / core thinner”现象。
- CLI 里现在的 `todo!()`、占位成功输出、直接打数据库、绕过统一输出层，很多都只是这种分层缺口的表层症状。
- 如果继续直接补 CLI，而不先把后端契约收敛到 core，最后只会得到两套逻辑：一套在 `src-tauri`，一套在 CLI，后续维护成本会继续升高。

## 主要问题

### Critical

#### 1. `cc-switch-core` 还不是 `src-tauri` 后端能力的超集

目前 core 与 tauri 的关系更像“简化版后端”和“完整版后端”，而不是“同一后端的不同适配层”。

直接证据：

- Provider 在 core 只有基础能力：`crates/cc-switch-core/src/services/provider.rs:25`、`:33`、`:44`、`:88`、`:123`、`:146`。
- Provider 在 tauri 额外承载了 Live 配置移除、复杂切换、Live 同步、公共配置提取、默认配置导入、读取 Live 设置、自定义 endpoint、usage 查询与脚本测试：`src-tauri/src/services/provider/mod.rs:357`、`:434`、`:597`、`:605`、`:820`、`:825`、`:830`、`:887`、`:897`。
- MCP 在 core 只有基础 CRUD、Claude 导入和一个空的全量同步：`crates/cc-switch-core/src/services/mcp.rs:14`、`:24`、`:50`、`:96`。
- MCP 在 tauri 已有多 app 导入和真实同步：`src-tauri/src/services/mcp.rs:165`、`:224`、`:262`、`:300`、`:338`。
- Prompt 在 core 只有 DB CRUD + enable/disable + 目录导入：`crates/cc-switch-core/src/services/prompt.rs:15`、`:25`、`:35`、`:57`。
- Prompt 在 tauri 已带 Live prompt 文件同步、当前文件读取、首次启动导入：`src-tauri/src/services/prompt.rs:28`、`:73`、`:146`、`:175`、`:187`。
- Skill 在 core 基本只有安装记录 CRUD 和 app 开关：`crates/cc-switch-core/src/services/skill.rs:13`、`:23`、`:28`、`:33`。
- Skill 在 tauri 已经覆盖 repo 管理、远程安装、ZIP 安装、扫描、同步到 app 目录、SSOT 迁移：`src-tauri/src/services/skill.rs:418`、`:458`、`:1477`、`:1667`、`:1704`、`:1814`。

影响：

- CLI 现在无法真正只依赖 core，因为很多“可用后端能力”实际上只存在于 tauri。
- 一旦继续在 CLI 里补功能，很容易把 tauri 里的领域逻辑又复制一遍。
- 未来想让 core 成为 tauri 后端时，迁移成本会比现在更高。

#### 2. Proxy / Failover / Usage 的迁移缺口最大，而且已经反向污染了 CLI 语义

这块不是“功能少一点”，而是已经出现了 core stub 和 CLI 假成功。

直接证据：

- Core proxy service 目前只暴露了很薄的一层：`crates/cc-switch-core/src/services/proxy.rs:70`、`:78`、`:83`、`:97`、`:106`、`:143`、`:148`、`:158`。
- `switch_proxy_target` 的 DAO 还是空实现：`crates/cc-switch-core/src/database/dao.rs:668`。
- Tauri proxy service 已经有真实的启动、接管、恢复、热切换、状态查询、配置更新、熔断配置更新：`src-tauri/src/services/proxy.rs:84`、`:134`、`:193`、`:660`、`:1566`、`:1774`、`:1883`。
- Tauri failover commands 已经有队列和自动切换行为：`src-tauri/src/commands/failover.rs:151`。
- Tauri usage backend 已经有 summary、trends、provider stats、model stats、request logs、request detail、provider limit 检查：`src-tauri/src/services/usage_stats.rs:120`、`:188`、`:298`、`:346`、`:386`、`:514`、`:578`。
- CLI proxy handler 里已经出现大量“显示成功但并未真正落库/生效”的路径：`crates/cc-switch-cli/src/handlers/proxy.rs:30`、`:35`、`:60`、`:95`、`:98`、`:101`、`:105`、`:119`、`:153`。

影响：

- 这是当前最危险的分层问题，因为 CLI 会把后端未实现伪装成“操作已完成”。
- 如果未来 CLI 继续先接 tauri 缺失能力，而 core 还没补齐，这类假成功会越来越多。
- Proxy/failover/usage 这条链路应该优先迁移，否则 CLI 很难可信。

### High

#### 3. MCP / Prompt / Skill 的关键“文件同步能力”仍被锁在 tauri

这些域的问题不只是 CRUD 缺失，而是“真正影响外部 app 的动作”还留在 tauri。

MCP：

- core 只有 DB 保存、删除、toggle 和 Claude 导入，`sync_all_enabled` 还是空实现：`crates/cc-switch-core/src/services/mcp.rs:24`、`:34`、`:50`、`:96`。
- tauri 已有真实 `sync_all_enabled`，并支持 Codex、Gemini、OpenCode 导入：`src-tauri/src/services/mcp.rs:165`、`:262`、`:300`、`:338`。

Prompt：

- core 的 `enable` 只是数据库层面的启用/禁用：`crates/cc-switch-core/src/services/prompt.rs:35`、`:46`。
- tauri 的 `enable_prompt` 会落到真实 prompt 文件，还带当前文件内容读取和首次导入：`src-tauri/src/services/prompt.rs:73`、`:146`、`:175`、`:187`。

Skill：

- core 只有安装记录与 app toggle：`crates/cc-switch-core/src/services/skill.rs:23`、`:28`、`:33`。
- tauri 的 skill service 才真正负责 repo 下载、目录同步、ZIP 解包、SSOT 迁移：`src-tauri/src/services/skill.rs:418`、`:1477`、`:1598`、`:1814`。

影响：

- 如果这些同步动作不先下沉到 core，CLI 就只能做“改数据库但不改真实环境”的半成品。
- 这类半成品命令最容易让用户误判系统状态。

#### 4. AppType 与应用能力矩阵没有对齐，core 还不是 tauri 的完整领域模型

Core 和 Tauri 连应用集合都还没有统一。

直接证据：

- Core `AppType` 只有 `Claude / Codex / Gemini / OpenCode`：`crates/cc-switch-core/src/app_config.rs:13`。
- Tauri `AppType` 已经包含 `OpenClaw`，并且有成套 OpenClaw / Omo 相关逻辑：`src-tauri/src/app_config.rs:295`、`:300`、`:317`、`:345`，`src-tauri/src/commands/openclaw.rs:11`，`src-tauri/src/commands/omo.rs:8`。

影响：

- 只要 core 的 app 模型不是 tauri 的超集，CLI 就不可能天然复用 tauri 后端能力。
- 这也会影响 provider、prompt、config、deeplink、workspace 等多个域的统一设计。

#### 5. Deeplink / Import-Export / Settings 的后端契约仍然分裂在两边

Core 已经开始承接一部分配置能力，但和 tauri 还没有对齐成一个完整契约。

直接证据：

- Core config service 目前只有 settings CRUD、全量 JSON 导入导出、简单 deeplink import：`crates/cc-switch-core/src/services/config.rs:14`、`:19`、`:24`、`:29`、`:46`、`:112`。
- Core deeplink 只识别 `provider / mcp / skill`，没有 prompt：`crates/cc-switch-core/src/services/config.rs:125` 到 `:129`。
- Tauri unified deeplink 已支持 `provider / prompt / mcp / skill`：`src-tauri/src/commands/deeplink.rs:52` 到 `:87`。
- Tauri 还额外承接了设置合并、应用重启、App config dir override、rectifier config、sync current providers live、文件对话框等：`src-tauri/src/commands/settings.rs:17`、`:23`、`:32`、`:43`、`:129`，`src-tauri/src/commands/import_export.rs:63`，`src-tauri/src/commands/config.rs:80`、`:93`、`:152`。

影响：

- CLI 的 `import/export/import-deeplink` 现在只能对接 core 的简化能力，和桌面端能力天然不一致。
- 后续如果要做到“CLI 和 GUI 用同一套后端”，这块必须先在 core 收敛统一的数据与导入语义。

### Medium

#### 6. CLI 当前接的是“部分 core + 直接 DB + 占位语义”，还不是一个干净的后端适配层

这不是最上游的问题，但它决定了迁移落地时的工作量。

直接证据：

- 很多命令仍然直接 `todo!()`：`crates/cc-switch-cli/src/handlers/provider.rs:91`、`:103`、`:113`、`:134`、`:153`、`:160`，`crates/cc-switch-cli/src/handlers/mcp.rs:74`、`:84`、`:93`，`crates/cc-switch-cli/src/handlers/prompt.rs:58`、`:68`、`:78`，`crates/cc-switch-cli/src/handlers/skill.rs:25`、`:34`、`:43`、`:53`。
- CLI 有些地方直接打数据库而不是统一走 service，例如 proxy failover switch：`crates/cc-switch-cli/src/handlers/proxy.rs:104`。
- CLI 全局声明了 `--format / --quiet / --verbose`，但 dispatch 只把 `format` 传给 `Printer`：`crates/cc-switch-cli/src/cli.rs:13` 到 `:23`，`crates/cc-switch-cli/src/handlers/mod.rs:16` 到 `:18`。
- 多个 handler 仍然直接 `println!` / `eprintln!`，绕过统一输出：`crates/cc-switch-cli/src/handlers/provider.rs:124`、`:157`，`crates/cc-switch-cli/src/handlers/mcp.rs:106`、`:112`，`crates/cc-switch-cli/src/handlers/prompt.rs:89`、`:96`，`crates/cc-switch-cli/src/handlers/config.rs:29`、`:40`、`:45`，`crates/cc-switch-cli/src/handlers/import_export.rs:14`、`:27`、`:33`、`:36`，`crates/cc-switch-cli/src/handlers/usage.rs:47`。

影响：

- 就算 core 补齐后端能力，CLI 这层也还需要一轮“收口”才能真正稳定。
- 但这轮工作应当放在 core 契约稳定之后做，不然很容易返工。

## 迁移边界建议

### 应该下沉到 `cc-switch-core` 的能力

- 领域模型与 app 能力矩阵，包括 `OpenClaw` 以及相关 provider / prompt / config 语义。
- Provider 的完整业务逻辑，包括 Live 切换、公共配置抽取、默认配置导入、读取 Live 设置、自定义 endpoint、usage 查询与 usage 脚本测试。
- MCP 的真实同步、删除后的 Live 清理、从多 app 导入。
- Prompt 的真实文件同步、启用/禁用语义、当前文件导入、首次导入。
- Skill 的 repo/ZIP 安装、扫描、同步到 app 目录、SSOT 迁移。
- Proxy server 的核心生命周期、接管、恢复、热切换、熔断配置与 failover 队列。
- Usage 统计和模型定价相关数据库能力。
- Deeplink 解析、合并、统一导入。
- 配置导入导出、当前 provider 同步到 Live、与外部 app 配置的业务级交互。

### 应该继续留在 `src-tauri` 的能力

- `tauri::command` 暴露本身。
- `AppHandle`、事件发射、窗口更新、托盘菜单刷新。
- 文件对话框、打开目录、打开外链、应用重启。
- 明显依赖桌面运行时的集成功能，例如 UI 通知、桌面生命周期钩子、自动启动的壳层接入。

结论很简单：

- “会决定数据、文件、状态如何变化”的逻辑，尽量放到 core。
- “只和桌面外壳交互”的逻辑，留在 tauri。

## 分域差异清单

### Provider

Core 现状：

- 已有基础 provider CRUD、current、switch、universal provider 基础操作：`crates/cc-switch-core/src/services/provider.rs:25` 到 `:174`。

Tauri 现状：

- 已有更完整的切换流、Live backfill、Live 配置移除、同步当前 provider 到 Live、默认配置导入、Live settings 读取、自定义 endpoints、usage 查询：`src-tauri/src/services/provider/mod.rs:357`、`:434`、`:597`、`:605`、`:820`、`:825`、`:830`、`:887`、`:897`。

应该搬运到 core：

- `SwitchResult` 语义和完整切换流。
- additive mode app 的 remove-from-live-config。
- sync-current-to-live 与公共配置抽取。
- import-default-config 与 read-live-settings。
- custom endpoints 与 provider usage script。

CLI 最终只应负责：

- 参数解析和交互确认。
- 调 core provider service。
- 用统一输出层渲染结果。

### MCP

Core 现状：

- 只有基础 CRUD、toggle、Claude 导入；`sync_all_enabled` 为空实现：`crates/cc-switch-core/src/services/mcp.rs:14` 到 `:96`。

Tauri 现状：

- 已有真实同步和多 app 导入：`src-tauri/src/services/mcp.rs:165`、`:224`、`:262`、`:300`、`:338`。

应该搬运到 core：

- `sync_all_enabled` 的真实落地。
- `import_from_codex`、`import_from_gemini`、`import_from_opencode`。
- 删除/禁用时对 Live 配置的真实清理。

CLI 最终只应负责：

- `list/show/add/edit/delete/toggle/import` 的参数和输出适配。
- 不再自己假设“toggle 成功就代表 Live 已同步”。

### Prompt

Core 现状：

- 只有 DB CRUD、enable/disable、批量目录导入：`crates/cc-switch-core/src/services/prompt.rs:15` 到 `:57`。

Tauri 现状：

- 已有 `upsert_prompt`、`enable_prompt`、当前文件读取、首次启动导入：`src-tauri/src/services/prompt.rs:28`、`:73`、`:146`、`:175`、`:187`。

应该搬运到 core：

- prompt 文件真实写入与备份恢复。
- “启用 prompt” 的完整语义，而不是只改 DB 标记。
- 从当前 live prompt 文件导入。

CLI 最终只应负责：

- `add/edit/delete/enable/import/show` 的命令适配。
- 统一显示当前启用状态和来源文件信息。

### Skill

Core 现状：

- 只有安装记录 CRUD 和 app toggle：`crates/cc-switch-core/src/services/skill.rs:13` 到 `:33`。

Tauri 现状：

- 已有 repo 管理、远程安装、ZIP 安装、扫描、同步到 app 目录、SSOT 迁移：`src-tauri/src/services/skill.rs:418`、`:1477`、`:1667`、`:1704`、`:1814`。

应该搬运到 core：

- skill repo store 与安装来源管理。
- repo/ZIP 安装与卸载。
- 与 app 目录之间的同步。
- 未托管 skill 扫描与 SSOT 迁移。

CLI 最终只应负责：

- `search/install/uninstall/toggle/list` 的薄适配。
- 不再自己决定安装细节。

### Proxy / Failover / Usage

Core 现状：

- proxy 只有基础接口，状态与切换逻辑仍不完整：`crates/cc-switch-core/src/services/proxy.rs:70` 到 `:158`。
- DAO 的 `switch_proxy_target` 为空：`crates/cc-switch-core/src/database/dao.rs:668`。
- usage 只有 summary/logs/export 级别能力，且挂在 proxy service 附近：`crates/cc-switch-core/src/services/proxy.rs:143`、`:148`、`:158`。

Tauri 现状：

- proxy 已经有真实生命周期、接管恢复、热切换、配置更新、熔断配置更新：`src-tauri/src/services/proxy.rs:84`、`:134`、`:660`、`:1566`、`:1774`、`:1883`。
- usage 已有 summary、trends、provider stats、model stats、request logs、request detail、provider limits：`src-tauri/src/services/usage_stats.rs:120`、`:188`、`:298`、`:346`、`:386`、`:514`、`:578`。

应该搬运到 core：

- failover queue 和 auto-failover 逻辑。
- proxy server start/stop/status/switch 的核心逻辑。
- circuit breaker 配置与状态。
- usage stats 与 model pricing 的数据库和业务层。
- provider limit 检查与 request detail 查询。

CLI 最终只应负责：

- start/stop/status/config/takeover/failover/circuit/usage 的命令入口。
- 不再打印任何未实现的“成功”提示。

### Config / Settings / Import-Export / Deeplink

Core 现状：

- 目前只有 settings 基础 CRUD、全量 JSON 导入导出、简化 deeplink：`crates/cc-switch-core/src/services/config.rs:14` 到 `:112`。

Tauri 现状：

- 已有 settings merge、restart、config dir override、rectifier config、sync current providers live、deeplink parse/merge/unified import、文件对话框：`src-tauri/src/commands/settings.rs:17`、`:23`、`:32`、`:43`、`:129`，`src-tauri/src/commands/import_export.rs:63`，`src-tauri/src/commands/deeplink.rs:10`、`:18`、`:46`。

应该搬运到 core：

- deeplink parse + merge + unified import。
- sync current providers to live。
- 导入导出的业务语义和数据校验。
- settings 的业务级 merge 规则。

应该留在 tauri：

- restart app。
- 打开目录和文件对话框。
- 任何依赖 `AppHandle` 的 UI 行为。

CLI 最终只应负责：

- `config`、`export`、`import`、`import-deeplink` 的参数与输出。
- 不承担桌面端专属能力。

## CLI 层最终需要做的收尾工作

当前 CLI 的很多问题确实存在，但它们应该放在“core 契约补齐之后”统一收尾。

需要收的尾主要有：

- 移除所有 `todo!()` 路径，或在 core 能力补齐前把命令显式标成 unsupported，而不是 panic。
- 停止直接访问低层 DB，统一改成走 core service。
- 所有成功/警告/错误输出统一进 `Printer`，让 `--format`、`--quiet`、`--verbose` 真正生效。
- 给 `app` 参数做统一解析，不让 proxy 相关命令继续吃原始字符串。
- 用行为测试覆盖“命令 -> core -> 输出”的完整链路，而不只是 `clap` 定义自检。

## 推荐迁移顺序

1. 先统一 `AppType` 和 app 能力矩阵，把 `OpenClaw`、OpenCode additive mode、Omo 等模型收进 core。
2. 再迁移 Provider，因为它是 MCP、Prompt、Proxy、Import/Export 的基础依赖。
3. 接着迁移 MCP / Prompt / Skill，把所有“会改真实 app 文件”的逻辑从 tauri 下沉到 core。
4. 然后迁移 Proxy / Failover / Usage，这是当前风险最高的一组。
5. 最后统一 Config / Deeplink / Import-Export 契约。
6. 当 core 成为稳定后端后，再收口 CLI，清掉 `todo!()`、假成功输出和输出层分裂问题。

## One-line TODO

- [ ] 把 `src-tauri` 中所有领域级后端能力按域梳理成迁移清单，明确哪些必须下沉到 `cc-switch-core`，哪些必须留在 tauri 壳层。
- [ ] 先统一 `AppType` 与应用能力矩阵，让 core 至少覆盖 tauri 当前支持的 app 集合和关键模式。
- [ ] 将 Provider 的完整切换流、Live 同步、默认配置导入、Live 设置读取、自定义 endpoint 和 usage 脚本能力下沉到 core。
- [ ] 将 MCP 的真实 `sync_all_enabled`、多 app 导入、删除后 Live 清理能力下沉到 core。
- [ ] 将 Prompt 的真实文件同步、启用语义、当前文件导入、首次导入能力下沉到 core。
- [ ] 将 Skill 的 repo 安装、ZIP 安装、扫描、同步到 app 目录、SSOT 迁移能力下沉到 core。
- [ ] 将 Proxy / Failover / Circuit Breaker / Usage / Model Pricing 的核心逻辑与数据访问从 tauri 收敛到 core。
- [ ] 补齐 core 中仍然是 stub 的路径，尤其是 `switch_proxy_target` 和任何只返回占位状态的 proxy 相关实现。
- [ ] 统一 Deeplink 的 parse / merge / unified import 契约，让 core 覆盖 `provider / prompt / mcp / skill` 全部资源类型。
- [ ] 将 settings merge、sync current providers live、导入导出校验等业务规则收敛到 core。
- [ ] 明确把 `AppHandle`、文件对话框、打开目录、托盘、重启应用、窗口事件这些桌面壳层能力留在 tauri。
- [ ] 在 core 能力补齐后，再让 CLI 改成纯适配层，统一走 core service，不再直接碰底层 DB。
- [ ] 清理 CLI 中所有 `todo!()` 命令和占位成功输出，在后端未实现前返回明确的 unsupported/未实现错误。
- [ ] 收口 CLI 输出层，确保 `--format`、`--quiet`、`--verbose` 对所有命令都一致生效。
- [ ] 为 CLI 增加行为测试，验证“参数解析 -> core 调用 -> 输出语义”整条链路，而不是只测 `clap` 冲突。
