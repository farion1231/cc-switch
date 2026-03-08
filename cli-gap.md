# CLI 与 GUI 功能差距核对

## 范围与方法

这份文档基于对以下两条链路的逐项核对：

- GUI 侧：`src/App.tsx`、`src/config/appConfig.tsx`、`src/components/**`、`src/hooks/**`、`src/lib/api/**`
- Tauri 后端侧：`src-tauri/src/commands/**`
- CLI 侧：`crates/cc-switch-cli/src/cli.rs`、`crates/cc-switch-cli/src/handlers/**`

核对原则：

- 以已经出现在 GUI 中、且有真实后端支撑的功能为准
- 区分 `已基本对齐`、`部分对齐`、`CLI 缺失`
- 对 GUI 中仅占位、尚未真正交付的页面，不计入 CLI 差距

## 总结

当前 CLI 已经覆盖了项目的核心数据面：

- Provider 的基础 CRUD / switch
- Universal Provider 的基础 CRUD / sync
- MCP 的基础 CRUD / import / enable-disable
- Prompt 的基础 CRUD / enable / import
- Skill 的基础 list / search / install / uninstall / enable-disable
- Proxy 的基础运行面与大部分策略配置
- Usage 的基础 summary / logs / export
- Config 的原始 get / set / show / path
- OpenClaw 的基础专属配置读写
- Import / export / deeplink import

但如果以 GUI 当前已交付的能力为基准，CLI 仍然存在三类明显差距：

1. `完全缺域`
   - Sessions
   - WebDAV Sync
   - 全局出站代理

2. `同域但明显偏薄`
   - Provider
   - Proxy / Failover
   - Usage
   - Skill
   - Settings
   - Deeplink
   - OMO / OMO Slim

3. `偏运维 / 信息型能力未暴露`
   - Tool Versions
   - Auto Launch
   - Portable Mode
   - Updater / Release Notes
   - Rectifier / Log Config

如果后续目标是让 CLI 在进入 Tauri 迁移前承担完整的后端验证入口，那么最主要的剩余缺口已经收敛到 `Provider 高级能力`、`Sessions / WebDAV`、`OMO 专属工作流` 和少量宿主机动作能力。

## 补充：这些差距里，哪些其实是 core 也还没收口

这次核对后，可以把差距再拆成三类：

### A. 主要只是 CLI 没接，core 基本已经有能力

这些域大多不需要先补 core，优先把 CLI 接上即可：

- Provider 高级能力
  - custom endpoints
  - remove from live config
  - import OpenCode / OpenClaw live providers
  - sort order
  - usage script query/test
  - common config snippet
- Usage 高级能力
  - trends
  - provider stats
  - model stats
  - request detail
  - model pricing CRUD
  - provider limits
- Skill 高级能力
  - repo 管理
  - unmanaged import
  - zip install
- WebDAV Sync
- OMO / OMO Slim

### B. 原来需要先补 core，现在已补齐统一服务面

这几块之前不是单纯“CLI 少个命令”，而是 core 侧没有统一 service。现在已经补到 `cc-switch-core`：

- Sessions
  - 已新增 `SessionService`
  - 统一承接会话扫描、消息读取、resume command 生成

- Global Outbound Proxy 的统一能力面
  - 已新增 `GlobalProxyService`
  - 已收口 `get/set/test/scan/status/apply-persisted`

- Claude plugin integration / skip onboarding
  - 已新增 `ClaudePluginService`
  - 已收口 plugin config 读写、状态判断、onboarding skip 开关

- Settings 的高层结构化服务
  - 已新增 `SettingsService` / `HostService`
  - 已覆盖 GUI 当前最关键的结构化设置保存、WebDAV merge、Claude plugin/onboarding 副作用同步、log/rectifier 配置、host preferences 读写

所以从现在开始，这四块已经不再是“先补 core”的阻塞项，后续重点转回 CLI 和再后面的 Tauri 适配。

### C. 更适合留在壳层，不一定要硬塞进 core

这些能力和宿主机、桌面环境、文件对话框、系统集成绑定得很深，更适合作为 Tauri/CLI 壳层能力调用：

- 文件对话框
  - open file
  - save file
  - pick directory
- 打开目录 / 打开外链
- 拉起终端
- auto launch
- portable mode 检查
- tool versions 探测
- updater / download-and-install

结论就是：

- 如果只是为了补齐 CLI 与 GUI 的大部分业务差距，core 现在已经够用了，先补 CLI 即可。
- 如果目标是后面把 GUI/Tauri 也进一步迁到 core，那么最早那批 core 阻塞项已经补齐，后续主要工作会转成命令面接入和宿主机壳层能力收口。

## 总览矩阵

| 功能域 | GUI 状态 | CLI 状态 | 结论 | 说明 |
| --- | --- | --- | --- | --- |
| Provider 基础管理 | 完整 | 已有 | 部分对齐 | 基础 CRUD 有，GUI 高级能力缺失较多 |
| Universal Provider | 完整 | 已有 | 基本对齐 | CLI 已补 edit/save-and-sync，剩余主要是 UI 体验差距 |
| MCP | 完整 | 已有 | 基本对齐 | GUI 体验更完整，但核心能力差距较小 |
| Prompt | 完整 | 已有 | 基本对齐 | CLI 已补 current-live-file-content，剩余主要是编辑体验差距 |
| Skill 已安装管理 | 完整 | 已有 | 基本对齐 | CLI 已补 unmanaged import / zip-install，剩余主要是交互体验 |
| Skill 发现页 / Repo 管理 | 完整 | 已有 | 部分对齐 | CLI 已补 repo 管理，剩余主要是筛选/刷新/文档入口体验 |
| Proxy 基础生命周期 | 完整 | 已有 | 基本对齐 | start/stop/status/takeover 已覆盖 |
| Proxy 高级配置 | 完整 | 已有 | 部分对齐 | CLI 已补 app/global config、auto-failover、provider-health、pricing source，剩余主要是全局出站代理和更强观测 |
| Usage 基础查看 | 完整 | 已有 | 部分对齐 | summary/logs/export 有，分析能力不足 |
| Usage 高级分析 / 定价 | 完整 | 已有 | 部分对齐 | CLI 已补 trends / stats / pricing / limits，仍缺更完整过滤与图表体验 |
| Settings 基础配置 | 完整 | 很薄 | CLI 缺失较多 | CLI 更像原始 KV 入口 |
| Import / Export | 完整 | 已有 | 基本对齐 | GUI 多文件选择和回执体验 |
| Deeplink 预解析 / 合并预览 | 完整 | 已有 | 部分对齐 | CLI 已补 parse/merge/preview，剩余主要是确认式交互体验 |
| 数据库备份 | 完整 | 已有 | 基本对齐 | CLI 已补 create/list/restore/rename/delete，剩余主要是自动备份设置入口 |
| WebDAV Sync | 完整 | 缺失 | CLI 缺失 | GUI 支持测试/保存/上传/下载/远端信息 |
| 环境冲突处理 | 完整 | 已有 | 部分对齐 | CLI 已补 check/delete/restore，剩余主要是多选与来源级交互 |
| Sessions | 完整 | 缺失 | CLI 缺失 | GUI 可浏览会话与消息、拉起 resume |
| Workspace / Daily Memory | 完整 | 已有 | 部分对齐 | CLI 已补读写与搜索，仍缺 open-dir 这类宿主机动作 |
| OMO / OMO Slim | 完整 | 缺失 | CLI 缺失 | GUI 有本地配置读取、导入与停用当前配置 |
| OpenClaw Env / Tools / Agents | 完整 | 已有 | 基本对齐 | CLI 已补 env/tools/agents-defaults/default-model/model-catalog，剩余主要是表单体验 |
| Stream Check / Model Test | 完整 | 已有 | 部分对齐 | CLI 已补单测/批量测/配置，剩余主要是日志浏览与模型测试体验 |
| 关于 / 更新 / 工具信息 | 完整 | 缺失 | CLI 缺失 | 多为信息型或运维型功能 |
| Agents 页面 | 占位 | 无 | 不计入差距 | GUI 当前只是 Coming Soon |

## 逐域差距

### 1. Provider

#### CLI 已有

- `provider list/show/add/edit/delete/switch`
- `provider duplicate/sort-order/read-live/import-live/remove-from-live`
- `provider endpoint list/add/remove/mark-used/speedtest`
- `provider common-config-snippet get/set/extract`
- `provider usage-script show/save/test/query`
- `provider stream-check run/run-all/config`
- `provider usage`
- `provider universal list/show/add/edit/save-and-sync/sync/delete`

#### GUI 已有但 CLI 缺失或明显偏薄

- Provider 卡片层动作
  - 打开官网
  - Claude 专属的打开 provider 终端
  - OMO / OMO Slim 的禁用当前 provider 动作

- Provider 数据来源管理
  - 更细粒度的 live config 编辑 / 校验辅助

- Provider 高级配置
  - endpoint 自动选择策略
  - provider 测试配置
  - pricingConfig、proxyConfig、testConfig 等高级表单能力

- Provider 运行健康
  - 更细的表格化结果展示

#### 结论

CLI 的 provider 目前更像“最小可用配置入口”，而 GUI 已经承载了 provider 的完整运营面。这个域是目前 CLI 与 GUI 差距最大的核心域之一。

### 2. Universal Provider

#### CLI 已有

- `provider universal list/show/add/edit/save-and-sync/sync/delete`

#### GUI 已有但 CLI 缺口

- GUI 的表单体验仍然更顺手
- GUI 仍有更细的视觉反馈与表单校验

#### 结论

这个域现在已经基本对齐，CLI 剩下的差距主要是体验层，而不是能力层。

### 3. MCP

#### CLI 已有

- `mcp list/show/add/edit/delete/enable/disable/import`
- `mcp validate/docs-link`

#### GUI 相比 CLI 的优势

- 更完整的表单体验
- 更直接的 app 级启停管理体验
- 统一列表与当前 app 视图联动

#### 结论

MCP 现在已经是高度对齐的域。CLI 剩下更多是 GUI 的表单体验和列表联动，而不是能力缺口。

### 4. Prompt

#### CLI 已有

- `prompt list/show/add/edit/delete/enable/import`
- `prompt current-live-file-content`

#### GUI 相比 CLI 的优势

- 富文本式编辑体验更好
- 深链导入后的交互刷新更自然
- 启用态与表单状态反馈更直观

#### 结论

Prompt 的功能面已经基本够用，CLI 剩下主要是 GUI 那套编辑体验和状态反馈更顺手。

### 5. Skill

#### CLI 已有

- `skill list/search/install/uninstall/enable/disable`
- `skill unmanaged scan/import`
- `skill repo list/add/remove`
- `skill zip-install`

#### GUI 已有但 CLI 缺失或明显偏薄

- 已安装技能管理
  - 打开技能文档 URL

- Skills Discovery 页
  - 按 repo / 安装状态 / 关键字筛选
  - 刷新远程索引
  - 在发现页中直接安装到当前 app

#### 结论

CLI 已经补到“使用面 + 来源管理面”，这个域现在主要还差筛选、文档入口和发现页体验，不再是核心阻塞项。

### 6. Proxy / Failover / Circuit

#### CLI 已有

- `proxy start/stop/status`
- `proxy config show/set`
- `proxy global-config show/set`
- `proxy app-config show/set`
- `proxy auto-failover show/enable/disable`
- `proxy available-providers`
- `proxy provider-health`
- `proxy default-cost-multiplier get/set`
- `proxy pricing-model-source get/set`
- `proxy takeover status/enable/disable`
- `proxy failover queue/add/remove/switch`
- `proxy circuit show/reset/stats/config show/set`

#### GUI 已有但 CLI 缺失或明显偏薄

- 全局代理配置
  - 全局出站代理 URL 设置
  - 代理测试
  - 本机代理扫描
  - 账号密码拆分输入与保存

- 自动故障转移
  - GUI 侧整合在统一设置面板里，交互更完整
  - 一些更细的可视化反馈仍然只在 GUI 里更顺手

- 定价相关的 proxy 配置
  - CLI 已补默认成本倍率和 pricing model source
  - GUI 仍有更好的表单反馈和联动展示

- Circuit / 健康观测
  - CLI 已补 provider-health 和 circuit stats
  - GUI 仍有更细的 health 观测与可视化

- 代理功能开关与设置整合
  - GUI 可在 Settings 中统一开启本地代理功能、切换 takeover、编辑全局代理配置

#### 结论

CLI 现在已经覆盖了 proxy 的运行面和大部分策略配置，剩下主要是 `global outbound proxy`、统一设置入口，以及更强的可视化观测体验。

### 7. Usage

#### CLI 已有

- `usage summary`
- `usage logs`
- `usage export`
- `usage trends`
- `usage provider-stats`
- `usage model-stats`
- `usage request-detail`
- `usage model-pricing list/update/delete`
- `usage provider-limits check`
- `provider usage`

#### GUI 已有但 CLI 缺失

- 更丰富的过滤维度与图表化展示

#### 结论

CLI 已经补到 usage 的主要分析面和定价运营面，剩下主要是更细的过滤体验、分页视图和 GUI 图表化展示。

### 8. Settings / Config

#### CLI 已有

- `config show/get/set/path`
- `export/import/import-deeplink`

#### GUI 已有但 CLI 缺失或明显偏薄

- 目录与路径管理
  - app config dir override
  - 各 app 配置目录浏览与重置
  - 打开配置目录

- 常规设置
  - 主题 / 语言
  - 开机启动 / 静默启动
  - 托盘 / 窗口 / 可见性
  - preferred terminal
  - skill sync method
  - Claude plugin integration
  - skip Claude onboarding

- 系统设置
  - auto launch
  - portable mode 检查
  - rectifier config
  - log config

- 同步与修复
  - sync current providers live

#### 结论

CLI 的 config 现在更偏“原始设置读写器”，而 GUI 已经把设置做成了结构化管理面。

### 9. Import / Export / Deeplink

#### CLI 已有

- `export`
- `import`
- `deeplink parse/merge/preview`
- `import-deeplink`

#### GUI 相比 CLI 的额外能力

- 文件对话框
- 导入状态与警告回执展示
- 备份 ID 展示
- import 前确认弹窗与资源级预览
- 深链导入后的页面刷新与上下文承接

#### 结论

CLI 现在已经覆盖了解析、merge、预览和最终导入，剩余差距主要是 GUI 的确认式交互和导入后的页面反馈。

### 10. 数据库备份

#### GUI 已有

- 手动创建数据库备份
- 查看备份列表
- 恢复备份
- 恢复前自动创建 safety backup
- 重命名备份
- 删除备份
- 调整自动备份间隔
- 调整保留数量

#### CLI 状态

- `backup create/list/restore/rename/delete`
- 仍未补自动备份设置入口

#### 结论

CLI 现在已经覆盖数据库备份的核心操作，剩余差距主要是自动备份相关设置入口，而不是恢复链路本身。

### 11. WebDAV Sync

#### GUI 已有

- 保存 WebDAV 配置
- 测试连通性
- 上传本地快照
- 下载远端快照
- 拉取远端快照信息
- autoSync 开关
- provider preset
- 密码保留策略

#### CLI 状态

- 完全没有对应命令

#### 结论

这是一个明确的 GUI-only 功能域，且不是纯展示，而是真实的数据同步能力。

### 12. 环境变量冲突处理

#### GUI 已有

- 检查指定 app 的环境变量冲突
- 展示冲突来源与取值
- 多选冲突项
- 删除选中的冲突变量
- 先备份再删除
- 从备份恢复

#### CLI 状态

- `env check --app <claude|codex|gemini>`
- `env delete --app ... --yes`
- `env restore <backup-path>`
- 当前更偏“按 app 一键处理 shell-file 冲突”，还没有 GUI 那种多选交互

#### 结论

CLI 现在已经能完成检查、备份删除和恢复闭环，剩余主要差距是更细粒度的冲突选择与来源级交互。

### 13. Sessions

#### GUI 已有

- 扫描会话列表
- 按 provider / 关键字过滤
- 查看消息历史
- 用户消息目录
- 复制 resume 命令
- 直接拉起 terminal resume

#### CLI 状态

- 完全没有对应命令

#### 结论

这是 GUI 已经提供的完整工作流能力，但 CLI 目前没有任何会话管理入口。

### 14. Workspace / Daily Memory

#### GUI 已有

- 读取 OpenClaw workspace 白名单文件
- 写入 workspace 文件
- 列出 daily memory 文件
- 读取 memory 文件内容
- 创建或更新 memory 文件
- 打开 workspace / memory 目录

#### CLI 状态

- `workspace read/write`
- `workspace memory list/read/write/search/delete`
- 仍未补 `open-dir`

#### 结论

这个域对 OpenClaw 使用链路已经开始可用，剩余差距主要是目录打开这类宿主机动作，而不是内容读写能力。

### 15. OMO / OMO Slim

#### GUI 已有

- 读取本地 OMO 配置文件
- 读取本地 OMO Slim 配置文件
- 在 Provider 表单中导入本地 OMO / OMO Slim 配置
- 识别当前启用的 OMO / OMO Slim provider
- 停用当前 OMO / OMO Slim 配置
- OMO / OMO Slim 专属模型映射编辑体验

#### CLI 状态

- 没有任何 OMO / OMO Slim 专属命令

#### 结论

虽然 OMO / OMO Slim 被放在 Provider 工作流里，但它们实际上已经形成一组独立的 GUI 能力，CLI 目前完全没有覆盖。

### 16. OpenClaw 专属管理

#### GUI 已有

- Env 配置读写
- Tools 权限配置读写
- Agents Defaults 读写
- Default Model 读写
- Model Catalog 读写

#### CLI 状态

- `openclaw env get/set`
- `openclaw tools get/set`
- `openclaw agents-defaults get/set`
- `openclaw default-model get/set`
- `openclaw model-catalog get/set`

#### 结论

这个域的核心能力现在已经打通，剩余差距主要是 GUI 侧更细的表单体验和引导，而不是缺命令。

### 17. Stream Check / Model Test

#### GUI 已有

- 单 provider stream check
- 批量 stream check
- 配置 stream check 参数
- 保存检查日志

#### CLI 状态

- `provider stream-check run/run-all/config`
- 仍未补独立日志浏览和更专门的 model test 入口

#### 结论

这个域已经能在 CLI 里跑通核心验证链路，剩余差距主要在结果浏览和更完整的专项测试体验。

### 18. About / 更新 / 工具版本 / 系统信息

#### GUI 已有

- 检查更新
- 下载更新与重启
- 打开 release notes
- 展示 app version
- 展示工具版本
- 针对 WSL 工具探测配置 shell / shell flag
- Windows / WSL 环境信息
- portable mode 状态
- 一键安装命令展示

#### CLI 状态

- 没有对应命令

#### 结论

这部分更多是信息型与运维型能力，不一定要在 CLI 全量复刻，但如果目标是让 CLI 成为完整的运维入口，这一层仍是缺口。

### 19. Agents 页面

#### GUI 状态

- 当前页面仅为 `Coming Soon`

#### CLI 状态

- 无对应命令

#### 结论

这不计入真实差距，因为 GUI 本身还没有交付实际功能。

## 差距优先级建议

### P0：如果目标是让 CLI 成为完整后端验证入口，优先补

- Provider 高级能力
  - import/remove live config
  - duplicate / sort order
  - stream check

- Proxy 高级配置
  - global outbound proxy
  - auto-failover config
  - provider health / circuit stats
  - pricing source / multiplier

- Deeplink 补齐预处理链路
  - parse
  - merge
  - preview / confirm

- OMO / OMO Slim
  - read local
  - disable current
  - dedicated management commands

### P1：重要但不一定阻塞 Tauri 迁移

- Sessions
- Workspace / Daily Memory
- WebDAV Sync
- sync current providers live
- Claude plugin / onboarding / startup behavior

### P2：偏运维和体验增强

- 关于 / 更新 / 工具版本 / portable mode
- open website / docs / release notes 一类辅助动作
- GUI 级表单体验和多步交互反馈

## 结论

如果只看“最基础的配置与切换”，CLI 已经不算弱。

但如果以 GUI 当前真实交付的完整功能面为标准，CLI 还不能算“全功能镜像入口”。它目前仍然偏向：

- 基础配置与基础运维入口
- 核心数据面的脚手架
- 自动化与批处理友好的最小接口

而 GUI 额外承载了：

- 高级配置
- 运行态观测
- 发现与导入
- OpenClaw 专属工作流
- 同步 / 备份 / 恢复 / 环境修复

因此，CLI 与 GUI 的差距不是“零星缺几个命令”，而是还缺几整个功能层级。后续如果要收敛差距，建议不要按单命令补，而要按以下功能层来补：

1. Provider / Proxy / Usage 高级运维层
2. Skills / OpenClaw / Sessions / Workspace 工作流层
3. Backup / WebDAV / Env Conflict 维护层
4. About / Tooling / Updater 信息层

## 附录：本次核对的代表性入口

### GUI 代表性页面

- `src/App.tsx`
- `src/components/providers/ProviderList.tsx`
- `src/components/providers/forms/ProviderForm.tsx`
- `src/components/UsageScriptModal.tsx`
- `src/components/settings/SettingsPage.tsx`
- `src/components/settings/AboutSection.tsx`
- `src/components/settings/WindowSettings.tsx`
- `src/components/settings/GlobalProxySettings.tsx`
- `src/components/settings/BackupListSection.tsx`
- `src/components/settings/WebdavSyncSection.tsx`
- `src/components/usage/UsageDashboard.tsx`
- `src/components/skills/UnifiedSkillsPanel.tsx`
- `src/components/mcp/UnifiedMcpPanel.tsx`
- `src/components/prompts/PromptPanel.tsx`
- `src/components/sessions/SessionManagerPage.tsx`
- `src/components/workspace/WorkspaceFilesPanel.tsx`
- `src/components/openclaw/EnvPanel.tsx`
- `src/components/openclaw/ToolsPanel.tsx`
- `src/components/openclaw/AgentsDefaultsPanel.tsx`
- `src/components/universal/UniversalProviderPanel.tsx`
- `src/components/env/EnvWarningBanner.tsx`
- `src/components/DeepLinkImportDialog.tsx`

### Tauri 代表性命令面

- `src-tauri/src/commands/settings.rs`
- `src-tauri/src/commands/provider.rs`
- `src-tauri/src/commands/proxy.rs`
- `src-tauri/src/commands/failover.rs`
- `src-tauri/src/commands/global_proxy.rs`
- `src-tauri/src/commands/usage.rs`
- `src-tauri/src/commands/stream_check.rs`
- `src-tauri/src/commands/deeplink.rs`
- `src-tauri/src/commands/import_export.rs`
- `src-tauri/src/commands/webdav_sync.rs`
- `src-tauri/src/commands/session_manager.rs`
- `src-tauri/src/commands/workspace.rs`
- `src-tauri/src/commands/omo.rs`
- `src-tauri/src/commands/openclaw.rs`
- `src-tauri/src/commands/env.rs`
- `src-tauri/src/commands/misc.rs`

### CLI 代表性入口

- `crates/cc-switch-cli/src/cli.rs`
- `crates/cc-switch-cli/src/handlers/provider.rs`
- `crates/cc-switch-cli/src/handlers/proxy.rs`
- `crates/cc-switch-cli/src/handlers/usage.rs`
- `crates/cc-switch-cli/src/handlers/mcp.rs`
- `crates/cc-switch-cli/src/handlers/prompt.rs`
- `crates/cc-switch-cli/src/handlers/skill.rs`
- `crates/cc-switch-cli/src/handlers/config.rs`
- `crates/cc-switch-cli/src/handlers/import_export.rs`
