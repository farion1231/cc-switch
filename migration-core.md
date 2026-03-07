# `cc-switch-core` 迁移清单

审查时间：2026-03-06

关联文档：
- [cli-review.md](/Users/eric8810/Code/cc-switch/cli-review.md)

## 目标

把 `cc-switch-core` 收敛成真正的共享后端，让：

- `src-tauri` 主要负责桌面壳层和 Tauri 命令暴露。
- `crates/cc-switch-cli` 主要负责命令解析、输出和测试。
- 领域逻辑、数据库逻辑、Live 配置读写、导入导出、deeplink、proxy/failover/usage 等核心能力尽量只在 `cc-switch-core` 保留一份。

## 完成标准

当下面四件事同时成立时，才算这轮迁移真正完成：

- tauri command 层不再承载领域级业务逻辑，只做参数适配和平台桥接。
- CLI 不再直接碰底层 DB，也不再补一套 tauri 同款逻辑。
- 同一个后端动作在 GUI 和 CLI 下使用同一份 core 实现。
- core 的能力面至少覆盖 tauri 当前已经对外暴露的主要后端能力。

## 迁移原则

- 只要某段逻辑决定“数据、文件、状态如何变化”，优先下沉到 core。
- 只要某段逻辑明显依赖 `AppHandle`、窗口、托盘、文件对话框、打开目录、桌面生命周期，就留在 tauri。
- 先补齐领域模型和后端契约，再做 CLI 收口；不要在 CLI 里继续长出第二套后端。
- 迁移过程中允许 tauri 暂时调用“旧逻辑 + 新 core”，但每个域必须有明确的收口终点。

## 状态标记

- `P0`：最先做，直接影响 core 是否能成为统一后端。
- `P1`：紧随其后，决定 CLI 是否能接通主要功能。
- `P2`：建议后续纳入 core，但不阻塞第一波 CLI 对接。
- `Stay`：明确留在 tauri 壳层，不迁到 core。

## 迁移总表

| 域 | tauri 主要来源 | core 建议落点 | 决策 | 优先级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| App 模型与配置适配 | `src-tauri/src/app_config.rs` `src-tauri/src/openclaw_config.rs` `src-tauri/src/opencode_config.rs` `src-tauri/src/codex_config.rs` `src-tauri/src/gemini_config.rs` | `crates/cc-switch-core/src/app_config.rs` + 新增 `config/*` 适配模块 | Move | P0 | 先统一 `AppType`、app 能力矩阵和各 app Live 配置读写接口。 |
| Provider 主链路 | `src-tauri/src/services/provider/mod.rs` `src-tauri/src/provider.rs` `src-tauri/src/provider_defaults.rs` | `crates/cc-switch-core/src/services/provider.rs` `crates/cc-switch-core/src/provider.rs` | Move | P0 | 切换、Live 同步、默认配置导入、公共配置抽取、自定义 endpoint、usage script 都应落到 core。 |
| Speedtest / endpoint latency | `src-tauri/src/services/speedtest.rs` | `crates/cc-switch-core/src/services/provider.rs` 或新增 `services/speedtest.rs` | Move | P1 | 已有 `EndpointLatency` 类型导出，建议一起收口。 |
| MCP | `src-tauri/src/services/mcp.rs` `src-tauri/src/mcp/*` `src-tauri/src/commands/mcp.rs` | `crates/cc-switch-core/src/services/mcp.rs` `crates/cc-switch-core/src/mcp/*` | Move | P1 | 把真实同步、多 app 导入和 Live 清理迁入 core。 |
| Prompt | `src-tauri/src/services/prompt.rs` `src-tauri/src/prompt.rs` `src-tauri/src/prompt_files.rs` | `crates/cc-switch-core/src/services/prompt.rs` `crates/cc-switch-core/src/prompt.rs` | Move | P1 | prompt 文件同步、当前文件导入、首次导入都不应留在 tauri。 |
| Skill | `src-tauri/src/services/skill.rs` `src-tauri/src/commands/skill.rs` | `crates/cc-switch-core/src/services/skill.rs` + 可能新增 `skill/*` 支撑模块 | Move | P1 | repo/ZIP 安装、同步到 app 目录、SSOT 迁移都应在 core。 |
| Proxy / Failover / Circuit | `src-tauri/src/services/proxy.rs` `src-tauri/src/proxy/*` `src-tauri/src/commands/proxy.rs` `src-tauri/src/commands/failover.rs` | `crates/cc-switch-core/src/proxy/*` `crates/cc-switch-core/src/services/proxy.rs` | Move | P0 | 这是当前缺口最大的域，也是 CLI 假成功的主要来源。 |
| Usage / Model Pricing | `src-tauri/src/services/usage_stats.rs` `src-tauri/src/commands/usage.rs` `src-tauri/src/usage_script.rs` | 新增 `crates/cc-switch-core/src/services/usage.rs`，必要时拆分 `usage_script.rs` | Move | P0 | summary/trends/stats/logs/detail/pricing/limit 检查应收敛到 core。 |
| Stream Check | `src-tauri/src/services/stream_check.rs` `src-tauri/src/commands/stream_check.rs` | 新增 `crates/cc-switch-core/src/services/stream_check.rs` | Move | P1 | 这是纯后端健康检查逻辑，CLI 未来也可能需要。 |
| Global Proxy | `src-tauri/src/commands/global_proxy.rs` `src-tauri/src/proxy/http_client.rs` | `crates/cc-switch-core/src/services/proxy.rs` 或新增 `services/global_proxy.rs` | Move | P2 | 属于后端配置和连接测试，应该和 proxy 主链路一起收口。 |
| Config / Settings / Import-Export | `src-tauri/src/commands/config.rs` `src-tauri/src/commands/settings.rs` `src-tauri/src/commands/import_export.rs` `src-tauri/src/services/config.rs` | `crates/cc-switch-core/src/services/config.rs` | Move | P1 | 业务级 merge、sync current providers live、导入导出校验应都在 core。 |
| Deeplink | `src-tauri/src/deeplink/*` `src-tauri/src/commands/deeplink.rs` | `crates/cc-switch-core/src/services/config.rs` 或新增 `crates/cc-switch-core/src/deeplink/*` | Move | P1 | unified import 和 parse/merge 不应只在 tauri。 |
| OpenClaw / Omo 专属后端 | `src-tauri/src/commands/openclaw.rs` `src-tauri/src/commands/omo.rs` `src-tauri/src/services/omo.rs` | `crates/cc-switch-core/src/app_config.rs` + 新增 `config/openclaw.rs` / `services/omo.rs` | Move | P1 | 这些本质上是 app 专属后端，不应长期停留在 tauri 命令层。 |
| Workspace 文件读写 | `src-tauri/src/commands/workspace.rs` | 新增 `crates/cc-switch-core/src/services/workspace.rs` | Move | P2 | 读写和搜索属于后端能力；打开目录、外部打开动作仍留 tauri。 |
| WebDAV Sync | `src-tauri/src/services/webdav.rs` `src-tauri/src/services/webdav_sync.rs` `src-tauri/src/services/webdav_auto_sync.rs` `src-tauri/src/commands/webdav_sync.rs` | 新增 `crates/cc-switch-core/src/services/webdav.rs` / `services/webdav_sync.rs` | Move | P2 | 同步逻辑可下沉，自动任务调度和事件发射留 tauri。 |
| Env Checker / Env Manager | `src-tauri/src/services/env_checker.rs` `src-tauri/src/services/env_manager.rs` `src-tauri/src/commands/env.rs` | 新增 `crates/cc-switch-core/src/services/env.rs` | Move | P2 | 这是系统配置文件/环境变量管理，属于后端工具能力。 |
| Session 扫描与消息读取 | `src-tauri/src/session_manager/*` `src-tauri/src/commands/session_manager.rs` | 新增 `crates/cc-switch-core/src/services/session_manager.rs` | Move | P2 | 扫描和解析可迁；终端拉起动作不迁。 |
| 终端拉起 / 外部打开 / 更新页 / 对话框 / 托盘 / 重启 | `src-tauri/src/commands/misc.rs` `src-tauri/src/commands/config.rs` `src-tauri/src/commands/import_export.rs` `src-tauri/src/tray.rs` `src-tauri/src/commands/settings.rs` | 无 | Stay | Stay | 明显是桌面壳层能力。 |
| Claude 插件 / onboarding 写入 | `src-tauri/src/commands/plugin.rs` `src-tauri/src/claude_plugin.rs` `src-tauri/src/claude_mcp.rs` | 暂不设 core 落点，待 CLI 需求明确后再定 | Evaluate | P2 | 如果未来 CLI 也要管这类本地文件能力，可再下沉；当前先不阻塞主链路。 |
| Misc 初始化状态 / 一次性提示 | `src-tauri/src/init_status.rs` `src-tauri/src/commands/misc.rs` | 无 | Stay | Stay | 属于 UI 生命周期和桌面提示。 |

## 推荐实施顺序

新的顺序采用：

1. 先完整迁移和实现 core。
2. 再让 CLI 在 core 之上完整跑通。
3. 最后再切 tauri 到 core，并清理 tauri 内的重复逻辑。

这样做的原因是：

- CLI 比 tauri 更轻，适合先当作 core 的“第一个完整消费者”。
- 如果 tauri 和 core 同时改，很容易出现“GUI 兼容旧逻辑、CLI 兼容新逻辑”的双线状态。
- 先用 CLI 验证 core，能更快暴露 API 缺口、错误语义不一致、输出不稳定这些问题。

### Stage 0：盘点与契约冻结

目标：

- 先把迁移边界定死，避免后面边迁边补。

Checklist：

- [ ] 冻结“哪些能力必须进 core、哪些能力必须留在 tauri”的边界。
- [ ] 为每个迁移域指定 core 落点文件，避免同类逻辑分散新增。
- [ ] 列出 tauri 当前对外暴露的 command 清单，并标记其最终归属。
- [ ] 梳理 core 需要新增的公共类型与错误语义。
- [ ] 统一 `AppType` 和 app 能力矩阵作为全局前置依赖。

完成标准：

- 有一份不会再频繁改方向的模块迁移图。
- 新需求不再默认加进 tauri，而是先判断是否该进 core。

### Stage 1：完整迁移并实现 core

目标：

- 先让 `cc-switch-core` 真正具备完整后端能力，再谈 CLI 和 tauri 的切换。

Checklist：

- [ ] 把 `OpenClaw`、OpenCode additive mode、Omo 相关 app 语义补进 core。
- [ ] 把 Provider 的完整 switch flow、Live backfill、Live 同步、默认配置导入、读取 Live settings 下沉到 core。
- [ ] 把 `remove_from_live_config`、custom endpoint、speedtest、usage script 测试能力下沉到 core。
- [ ] 把 MCP 的真实 `sync_all_enabled`、多 app 导入、删除后的 Live 清理迁入 core。
- [ ] 把 Prompt 的真实文件同步、当前文件导入、首次导入迁入 core。
- [ ] 把 Skill 的 repo 安装、ZIP 安装、扫描、同步到 app 目录、SSOT 迁移迁入 core。
- [ ] 把 tauri proxy service 中的 start/stop/status/switch/takeover/recover/circuit/failover 逻辑迁入 core。
- [ ] 补齐 core 中仍然是 stub 的 proxy/failover 路径，尤其是 `switch_proxy_target`。
- [ ] 新建 core usage service，迁入 usage summary/trends/provider stats/model stats/request logs/detail/model pricing/limit 检查。
- [ ] 把 Stream Check、Global Proxy、Workspace 文件读写、Env Checker / Env Manager 纳入 core 规划并实现。
- [ ] 把 WebDAV 同步核心逻辑迁入 core，把 auto-sync worker 与事件发射继续留在 tauri。
- [ ] 把 Deeplink parse / merge / unified import、settings merge、`sync_current_to_live`、导入导出校验统一收口到 core。
- [ ] 为迁入 core 的每个域补齐 core 级测试。

完成标准：

- core 的能力面覆盖 tauri 当前主要后端能力。
- 除明显桌面壳层能力外，tauri 不再是唯一后端实现来源。

#### Stage 1 ext：代码 review 结论与补测清单

当前结论：

- `Stage 1` 仍然不能被标记为“已完成”，但 core 已经不再只有 provider 骨架，而是具备了一条能被 CLI 和 tauri 共用的后端主链。
- `cc-switch-core` 当前已经覆盖的主线是：`AppType/OpenClaw/OpenCode -> 文件型 settings -> app config adapter -> provider live read/write/import/sync -> MCP live sync/import -> Prompt 文件同步 -> Skill SSOT/导入/ZIP 安装 -> OMO 独占配置 -> usage 聚合查询 -> usage script / model_pricing / provider limits -> stream-check service -> proxy schema/DAO 契约`。
- 当前最大的剩余阻塞，已经进一步收缩到真正的 runtime 和少数外围域：`Proxy runtime 与 takeover/failover/recover / deeplink merge / env & workspace & webdav 等系统域`。

本轮已完成：

- 已补齐 `OpenClaw` 到 core 的 app model。
  - `AppType`、`McpApps`、`SkillApps` 都已经纳入 `openclaw`。
  - additive mode 语义已经扩展到 `OpenCode + OpenClaw`。
- 已把 settings 基础能力迁到 core。
  - core `settings.rs` 现在具备文件型设备设置缓存、`openclaw_config_dir`、`current_provider_openclaw`、override dir 解析、`get_current_provider()`、`set_current_provider()`、`get_effective_current_provider()`。
- 已补齐 provider live 所需的 config adapter。
  - core 已新增 `codex_config`、`gemini_config`、`opencode_config`、`openclaw_config`。
  - `OpenClaw` JSON5 配置读写、`OpenCode` provider 片段读写、Gemini `.env/settings.json` 适配都已经能在 core 内独立完成。
- 已把 provider 主链的核心 live 能力迁进 core。
  - `ProviderService` 已支持基础版的 `add/update/delete/switch`。
  - 已支持 `read_live_settings()`、`sync_current_to_live()`、`import_default_config()`、`import_opencode_providers_from_live()`、`import_openclaw_providers_from_live()`。
  - 已支持 custom endpoint 的基础 CRUD 和 last-used 更新时间回写。
- 已把 `OMO / OMO Slim` 的独占配置链路迁进 core。
  - core 已新增 `services/omo.rs`，包含 `STANDARD / SLIM` 变体、JSONC 清理、配置文件写入、插件同步、从本地文件导入等能力。
  - `ProviderService` 已接通 `omo / omo-slim` 的 add/update/delete/switch/remove_from_live_config 分支，不再把它们当普通 additive provider 处理。
- 已把 MCP 从 stub 补成真实行为。
  - core `mcp/validation.rs` 现在有统一的 server spec 校验和 `extract_server_spec()`。
  - `mcp/claude.rs`、`mcp/codex.rs`、`mcp/gemini.rs`、`mcp/opencode.rs` 已支持真实 live config 读写、单项同步、删除和导入。
  - `McpService` 已支持 `upsert_server()`、`toggle_app()`、`delete_server()`、`sync_all_enabled()`、`import_from_claude/codex/gemini/opencode()`。
- 已把 usage 聚合查询往 core 继续下沉。
  - 新增 `services/usage.rs`。
  - DAO 已支持 `usage_trends`、`provider_stats`、`model_stats`、`paginated logs`、`request detail`。
- 已把 provider usage script 链路迁回 core。
  - 新增 `usage_script.rs`，已支持脚本执行、模板变量替换、返回值校验、基础 SSRF / 同源 / HTTPS 防护。
  - `ProviderService` 已接入 `query_usage()`、`test_usage_script()`、`validate_usage_script()`，CLI 和 tauri 后续都可以直接走 core。
- 已把 pricing / provider limits / stream-check 服务层补齐到 core。
  - `model_pricing` 现在不只是建表，core 启库时会自动 seed 默认定价数据。
  - DAO / `UsageService` 已支持 `get/update/delete model pricing`、`check_provider_limits()`、请求详情成本回填。
  - 新增 `services/stream_check.rs`，已支持配置读写、单 provider 检查、批量检查、日志落库，以及 Claude / Codex / Gemini 的真实流式探测。
- 已把 Skill 从“只改 DB”补成真实文件链。
  - core `services/skill.rs` 现在已经支持 `SSOT (~/.cc-switch/skills)`、`sync_to_app_dir()`、`remove_from_app()`、`sync_to_app()`。
  - 已支持 `scan_unmanaged()`、`import_from_apps()`、`install_from_zip()`、`migrate_skills_to_ssot()`。
  - 已补 `UnmanagedSkill`、`SkillApps::from_labels()`、`skill_repos` schema/DAO，以及 `provider/live::sync_current_to_live()` 对 skill 同步的接入。
- 已补一版独立的 endpoint speedtest 能力。
  - 新增 `services/speedtest.rs`。
  - 已具备基础 URL 校验、超时归一化和并发测速骨架。
- 已补齐这条链路依赖的数据库契约。
  - schema 里新增了 `provider_endpoints` 表。
  - `mcp_servers` / `skills` 已补 `enabled_openclaw` 列，并加了向前兼容的加列逻辑。
  - 这轮继续补了 `skill_repos`、`model_pricing`、`stream_check_logs`、`proxy_live_backup`、`proxy_config.default_cost_multiplier`、`proxy_config.pricing_model_source` 这些表和列。
  - DAO 已对齐 provider endpoint 读写、OpenClaw settings 字段、OpenClaw export 范围，以及 skill repo / failover queue / live backup 的基础读写。
- 已把 proxy 的一部分“假成功”语义收掉。
  - `switch_proxy_target()` 现在不再只是切 DB current，还会同步设备级 current provider。
  - 当存在 live backup 时，`switch_proxy_target()` 会同步更新 backup 内容，作为后续真正 `stop_with_restore` / `recover` 的基础。
- 当前 `cargo test -p cc-switch-core` 与 `cargo test -p cc-switch-cli` 已通过。
  - core 当前测试结果是 `50 passed`。
  - 新增测试已覆盖到 `MCP + OMO + usage detail + model_pricing seed/match/backfill + provider limits + stream-check config/log + usage script validation + skill filesystem + failover queue + live backup`。

代码 review 结论：

- provider 主链已经能跑，但还没有完全达到 tauri 等价。
  - usage script 的执行与测试链路已经迁回 core。
  - 普通 provider 的 remove/import/sync 回归测试仍不够完整。
- Prompt 已经不是纯 DB stub，但 Stage 1 还不能算收尾。
  - Prompt 已具备文件启用、当前文件读取、首次导入基础能力。
  - 但禁用、覆盖、跨 app 回归测试还需要补齐。
- Skill 已经不再是主阻塞，但还没有完全达到 tauri 全量等价。
  - 真实文件同步、扫描导入、ZIP 安装、SSOT 迁移已经迁回 core。
  - 剩余差距主要是 repo discover/install 的完整远程生态、默认 repo 初始化后的 CLI/tauri 接入，以及更完整的回归测试。
- Proxy / Failover / Usage 仍是 Stage 1 后半段的主阻塞。
  - `services/proxy.rs` 仍然不是 tauri 那种可运行 runtime。
  - 当前只补到了 schema/DAO、`switch_proxy_target()` 的 backup 回写，以及 usage / pricing / stream-check 的服务层。
  - 真正还没迁完的是 `start/stop/status/takeover/recover/failover` 运行态。
- core runtime boundary 仍未完成。
  - 当前 `AppState` 仍然只有 `db`。
  - 如果要承接 tauri 的 proxy / failover / usage runtime，后面必须把纯后端运行态正式抽到 core。

建议的代码实现顺序：

- 先把 proxy runtime 主链真正下沉。
  - `start / stop / status / takeover / recover / failover` 现在是 Stage 1 的第一阻塞项。
- 再收 deeplink merge。
  - provider deeplink 里的 `usageScript`、meta merge、导入语义需要从 tauri 收到 core。
- 最后处理外围系统域。
  - `env / workspace / webdav` 这些不是 provider 主链阻塞，但会影响 Stage 1 是否能被定义成“后端能力完整迁入 core”。

必须补的测试：

- App model / settings
  - [x] `OpenClaw` 的 `AppType::from_str()`、`as_str()`、`is_additive_mode()` 测试。
  - [x] settings 对 `openclaw_config_dir`、`current_provider_openclaw`、override dir 读取的测试。
  - [x] `get_effective_current_provider()` 在“本地设置优先、DB 回退、无效 ID 自动清理”场景下的测试。
- Config adapter
  - [x] `codex` / `gemini` / `opencode` / `openclaw` 配置文件读写测试。
  - [x] additive mode app 的 provider 片段写入和移除测试已经覆盖到 `OpenCode MCP` 与 `OpenClaw provider` 的基础路径。
  - [ ] `OpenCode` 普通 provider 的删除与重入测试还要补。
  - [x] OMO / OMO Slim 配置拼装基础测试已补。
  - [ ] OMO / OMO Slim 删除、互斥切换、从本地导入测试还要补。
- Provider
  - [ ] add / update / delete / switch 在普通模式和 additive mode 下的行为测试还要继续补。
  - [x] `switch()` 的 OMO 独占链路基础测试已补。
  - [ ] `sync_current_to_live()`、`import_default_config()`、`read_live_settings()` 的回归测试还要补。
  - [ ] `remove_from_live_config()`、custom endpoint CRUD 回归测试还要补。
  - [x] usage script 校验基础测试已补。
  - [ ] usage script 实际联网执行链路测试仍可继续补。
  - [x] OpenClaw / OpenCode 导入 live providers 的测试已具备基础覆盖，仍需补删除与重入场景。
- MCP
  - [x] `sync_all_enabled()` 真实写入 live 配置的测试已补基础覆盖。
  - [ ] `import_from_claude` / `codex` / `gemini` / `opencode` 的完整导入测试还要补。
  - [ ] toggle / delete 后对 live 配置的清理测试还要补。
- Prompt
  - [x] prompt 启用、读取当前文件、首次导入测试已经具备基础覆盖。
  - [ ] prompt 禁用、覆盖、跨 app 回归测试还要补。
- Skill
  - [x] ZIP 安装、同步到 app 目录、扫描导入、SSOT 迁移基础测试已补。
  - [ ] repo discover/install、默认 repo 初始化、冲突目录名与重复安装测试还要补。
- Proxy / Failover / Usage
  - [x] usage summary / logs / trends / provider stats / model stats / request detail 基础测试已补到 DAO 层。
  - [x] schema 迁移后 `model_pricing` seed 测试。
  - [x] failover queue 增删改查基础测试已补。
  - [x] `switch_proxy_target()` 更新 live backup 的基础测试已补。
  - [ ] proxy start / stop / status / takeover / recover 测试。
  - [x] model pricing 匹配和计费回填测试。
  - [x] provider limits / pricing 基础测试已补。
  - [ ] usage script 实际执行 / speedtest 更完整测试还要补。
- 外围后端域
  - [x] stream check 配置、日志基础测试已补。
  - [ ] stream check provider 级真实请求测试还要补。
  - [ ] global proxy 配置校验、保存、连通性测试。
  - [ ] workspace 文件读写与搜索测试。
  - [ ] env checker / env manager 备份、删除、恢复测试。
  - [ ] WebDAV 基础连通性、上传、下载、冲突与 post-sync 测试。

Stage 1 退出前必须看到的信号：

- `cargo test -p cc-switch-core` 持续为绿，并且新增域不会再回退已迁入的 provider 基础链。
- core 可以独立完成 provider live 操作、MCP 同步、OMO 切换和 usage 查询，而不依赖 tauri command 层兜底。
- core 可以独立完成 usage script、model pricing、provider limits、stream-check，而不依赖 tauri command 层兜底。
- skill 的安装/扫描/同步不再只能依赖 tauri。
- proxy 切换不再只是改 DB current，而能承接 takeover / backup / recover 语义。
- 进入 Stage 2 时，CLI 不需要再自带任何“临时补丁逻辑”来绕过 core 缺口。

### Stage 2：让 CLI 基于 core 完整实现

目标：

- 先用 CLI 把 core 跑透，确认 core 真能独立作为完整后端存在。

Checklist：

- [ ] 所有 CLI handler 统一走 core service，不再直接打底层 DB。
- [ ] 把所有 `todo!()` 命令改成真实实现，或在极少数未完成功能上明确返回 unsupported。
- [ ] 清掉所有占位成功输出，尤其是 proxy / failover / prompt / mcp / skill。
- [ ] 统一 `app` 参数解析与错误语义，不再让 proxy 类命令直接吃裸字符串。
- [ ] 所有输出统一走 `Printer`，让 `--format`、`--quiet`、`--verbose` 真正全局生效。
- [ ] 增加 CLI 行为测试，覆盖“参数 -> core -> 输出”的完整链路。
- [ ] 用 CLI 逐域验证 Provider、MCP、Prompt、Skill、Proxy、Usage、Config、Deeplink 是否都能只靠 core 跑通。

完成标准：

- CLI 只剩下参数解析、确认交互、统一输出。
- CLI 成为 core 的第一个完整消费者，可以独立验证 core 后端契约是否稳定。

### Stage 3：切换 tauri 到 core

目标：

- 当 core 已被 CLI 跑通后，再让 tauri 切到 core，并清掉 tauri 内的重复后端逻辑。

Checklist：

- [ ] 把 tauri command 改成优先只调用 core service。
- [ ] 删除迁移后已经没有必要保留的 tauri 领域逻辑重复实现。
- [ ] 对仍必须留在 tauri 的能力，加注释说明“为什么不能进 core”。
- [ ] 保留 `AppHandle`、事件发射、窗口/托盘、文件对话框、打开目录、重启应用等壳层逻辑在 tauri。
- [ ] 给 tauri command 层增加回归测试，确认它只负责桥接，不改业务语义。

完成标准：

- tauri 真正退化成桌面壳层。
- GUI 和 CLI 对同一后端动作都使用同一份 core 实现。

## 建议新增的 core 模块

为避免迁移后继续把逻辑堆进现有几个大文件，建议预留这些落点：

- [x] `crates/cc-switch-core/src/services/usage.rs`
- [x] `crates/cc-switch-core/src/services/stream_check.rs`
- [ ] `crates/cc-switch-core/src/services/webdav.rs`
- [ ] `crates/cc-switch-core/src/services/env.rs`
- [ ] `crates/cc-switch-core/src/services/workspace.rs`
- [ ] `crates/cc-switch-core/src/services/session_manager.rs`
- [ ] `crates/cc-switch-core/src/config/openclaw.rs`
- [ ] `crates/cc-switch-core/src/config/opencode.rs`
- [ ] `crates/cc-switch-core/src/config/codex.rs`
- [ ] `crates/cc-switch-core/src/config/gemini.rs`

说明：

- 如果不想新增太多 service 文件，也至少要给 `provider / proxy / config` 继续拆子模块。
- 迁移时优先把“读写外部 app 配置文件”的逻辑从 tauri 挪到 core，不要先改 command 层。

## 高风险注意点

- Proxy / Failover / Usage 必须优先迁，不然 CLI 继续扩功能时很容易新增假成功路径。
- `AppType` 和 app 配置适配必须先统一，不然后面每个域都要反复返工。
- Prompt / MCP / Skill 的“数据库状态”和“真实文件状态”必须同时收口，否则会继续出现“DB 成功但环境没变”。
- tauri 中依赖 `AppHandle` 的逻辑不要硬搬进 core，应该拆成“core 逻辑 + tauri 事件桥”。

## 备注

- 对 `Workspace / WebDAV / Env / Session / Claude Plugin` 这些外围域，本清单里的归类有一部分是根据现有模块职责做的推断，不是逐行实现审计。
- 真正开工前，建议为每个域再做一次 15 到 30 分钟的源码核对，确认是否有隐藏依赖、全局状态或平台限定逻辑。
