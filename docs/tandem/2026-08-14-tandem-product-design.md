# Tandem 产品设计

状态：已完成产品发现并经用户逐节确认
日期：2026-08-14

## 1. 决策摘要

Tandem 是常驻菜单栏的 macOS Code Agent 任务控制台。它把人的任务与 Cursor Agent CLI、Codex CLI、Claude Code CLI 的原生 session 建立可读关系，让工作在重启后找得到、在原 Agent 中接着做、切换 Agent 时接得上。

首个可日用版本的承诺是：

> 找得到，接着做，换 Agent 也接得上。

完整愿景是：

> 一个任务，无论由哪个 Agent 执行，都能被找到、继续、接力、比较并由人验收。

价值优先级是：

1. 找回和理解大量历史及当前 session；
2. 在不同 Agent 之间可靠接力；
3. 使用共同标准让多个 Agent 独立执行、比较并融合结果。

首版交付前两项。独立对比、逐项比较和融合 Run 属于第二阶段。

## 2. 用户与问题

首版面向与产品创建者相似的个人重度用户：

- 在 macOS 上维护多个 Git 项目；
- 在 iTerm2 中同时运行大量 Cursor、Codex 和 Claude 会话；
- 简单任务偏好快速直接交互，复杂任务可能使用托管执行；
- 使用多个 provider、模型和 API key；
- 希望不同 Agent 使用相同 Rules、Skills、Persona 和门禁；
- 经常因 session ID 不可读、窗口过多和电脑重启而难以找回工作；
- 会将任务从一个 Agent 交给另一个 Agent，未来还会并行比较多个结果。

用户丢失的不是终端窗口，而是人的任务与机器 session 之间的可读关系。原生 CLI 继续拥有对话和原生 resume；Tandem 管理 Task、Run、session、项目、执行基线和人的决定之间的关系。

历史整理是一次性的 onboarding 价值。持续价值是新任务从开始就有可读身份，重启后能回到正确 session，切换 Agent 时无需翻找和复制上下文。

## 3. 产品原则

1. Task 是人的主对象，Run 是一条执行路线，Native Session 是外部执行记录。
2. Tandem 管理关系和编排，不替代 Agent CLI、iTerm2 或 Git。
3. 同一 Agent 继续时只使用原生 resume；只有切换 Agent 时才生成 Handoff。
4. 人工协作 Run 只显示可靠事实，不持续读取会话推断阶段、进度或下一步。
5. 简单任务不承担复杂任务的确认成本。
6. Agent 退出、测试通过或代码落地都不能自动完成 Task；只有用户确认才能完成。
7. 一个 Agent Preset 是一次 Run 的完整工作方式，运行后不静默切换。
8. `.tandem/` 是受管项目 Rules、Skills 和门禁的共享源；原生 CLI 文件是兼容入口或投射产物。
9. 原生 CLI 拥有完整 transcript；Tandem 默认不复制完整会话。
10. 本地优先，明文 secret 只进入 macOS Keychain。
11. Tandem 最终吸收 `dev-rules`、`twin` 和 CC-Switch 的相关职责，成为唯一产品和事实源。
12. 未知状态明确显示未知，不能用模型自述或推断伪装成事实。

## 4. 产品形态

### 4.1 菜单栏

Tandem 常驻菜单栏，但保持安静：

- 图标只表达是否存在需要用户介入的事项；
- 点击后优先显示等待输入、启动失败、恢复失败、待确认接力和待验收；
- 下方只显示少量最近活动 Task，提供打开 CLI、继续和打开控制台；
- 不把全部运行数量和持续活动做成制造压力的监控面板。

### 4.2 完整控制台

默认首页采用行动台账，而不是项目或 Agent 列表：

1. 需要你处理；
2. 待验收；
3. 正在推进；
4. 最近可继续。

一级入口为：

- 任务；
- 待认领；
- 项目；
- Agent Presets；
- 配置。

项目工作台是二级视图，展示一个项目的 Task、sessions、Preset、`.tandem/` 和三端投射状态。配置中心吸收 CC-Switch 的 provider、模型、Profile、Skills、连通性和凭据管理能力。

### 4.3 iTerm2

真实 Cursor、Codex 和 Claude CLI 始终在 iTerm2 中运行。Tandem 可以创建 tab、设置 Workspace、启动或 resume CLI、保存可重新定位引用并跳回原 tab，但不内嵌终端、不复制完整 transcript、不承诺复原完整窗口布局。

## 5. 首版范围

### 5.1 包含

- macOS 菜单栏与桌面控制台；
- iTerm2 深度集成；
- Cursor Agent CLI、Codex CLI、Claude Code CLI 的深度支持；
- CC-Switch fork 的配置、Profile、Skills、session 与托盘能力；
- 历史 session 全量本地索引、搜索和渐进式整理；
- 候选 Task 聚合和用户认领；
- 从 Tandem 快速启动新任务；
- 手动启动 session 的分级发现与认领；
- Task、Run、Native Session、Workspace 关系；
- 同 Agent 原生 resume；
- Agent Preset 和 `.tandem/` 项目配置；
- 三端确定性投射、检查和自动修复；
- 切换 Agent 时的轻量 Handoff；
- 人工协作与可选托管执行；
- 用户确认接力成功和 Task 完成。

### 5.2 不包含

- 完整 transcript 归档或 Tandem 内会话阅读器；
- 对人工 CLI 会话持续生成进度摘要；
- 所有任务强制只读研究或完整 Task Contract；
- 任意 CLI 的公开通用 Adapter；
- 完整 iTerm2 窗口、tab 和 pane 布局复原；
- 独立 worktree 对比、逐项结果比较和融合 Run；
- 账户、团队任务、云同步和远程控制；
- Tandem 自行理解并修改代码、解决冲突或执行 Git 集成；
- 自动完成 Task；
- 模型 token 转售。

## 6. 核心对象

### Project

一个本地 Git 仓库及其 Tandem 项目配置。可共享配置位于 `.tandem/`，本机配置位于 Tandem 本地数据中。

### Task

用户要完成的一件事，是行动台账、恢复、接力和验收的主对象。Task 标题由用户拥有；机器生成标题在确认前只是建议。

### Run

一个 Agent 对 Task 的一条执行路线。Run 记录 Agent、人工协作或托管模式、Workspace、Native Sessions、Agent Preset 快照和 Execution Profile 快照。

同一 Agent 的原生 resume 不创建新 Run。用户切换 Agent 时创建接力 Run。第二阶段的独立对比创建隔离 Run。

### Native Session

Cursor、Codex 或 Claude 原生拥有的 session。Tandem 保存身份、来源、项目、时间、resume 能力和 Run 归属，不拥有完整 transcript。

### Workspace

Run 使用的真实代码现场。首版接力沿用源 Run 的 Workspace。第二阶段的独立尝试和多方融合使用迁入 Tandem 的 `wts` 能力创建隔离 worktree。

### Agent Preset

某次 Run 获得的 Persona、Instructions、Rules、Skills、Hooks、机械门禁、权限和执行能力组合。首版提供一个从 `dev-rules` 迁入的默认开发 Preset，以及只读研究、代码评审、托管执行等职责实质不同的专用 Preset。

内置 Preset 只读。自定义 Preset 从复制内置版本开始。每个 Run 锁定并记录实际版本。

### Execution Profile

Agent、provider、模型、认证方式和本机启动配置。Profile 不包含可导出的明文 secret。

### Handoff

用户明确切换 Agent 时生成的一次轻量交接。它包含源 Agent 提炼的目标、进展、关键决定、约束和下一步，以及 Tandem 补充的 Workspace、branch、diff、未提交改动和验证事实。

### Evidence

用于验证接力或 Task 状态的事实，例如正确 Workspace、投射结果、Handoff 投递、命令结果、Git 状态、测试和用户确认。

## 7. 历史与发现

### 7.1 首次扫描

1. 扫描三种 CLI 的历史 sessions；
2. 建立完整、可搜索的本地索引；
3. 根据项目、目标、时间、Workspace 和改动提出候选 Task；
4. 只优先展示近期、仍可 resume、关联有效项目或可能未完成的高价值候选；
5. 用户可以认领、改名、合并、拆分、忽略或批量收纳；
6. 未确认候选不成为正式 Task。

历史整理不要求清空收件箱。需要旧任务时能搜索、找到并现场认领，即为成功。

### 7.2 标题

Tandem 可以读取必要会话内容生成短标题建议。建议必须标记为机器生成；用户确认或修改后才成为正式 Task 名称。后续扫描不得覆盖用户标题。

### 7.3 手动启动的 session

项目有三种发现策略：

- 默认：只发现 Agent、项目、时间和 session 引用，认领后才读取内容；
- 受管：自动理解并提出 Task 归属，但不自动创建正式 Task；
- 敏感：完全关闭发现。

发现、读取内容和成为正式 Task 是三个独立步骤。

## 8. 创建、继续与接力

### 8.1 创建

Tandem 采用混合入口。推荐从 Tandem 启动以获得完整关系和投射验证，同时允许手动 session 后续认领。

确认成本按意图分级：

- 快速单 Agent 任务：原始指令直接启动；
- 切换 Agent：确认精简 Handoff；
- 第二阶段对比任务：确认共同目标、约束和比较标准；
- 高风险任务：按 Preset 门禁升级为完整计划或任务契约。

启动受管 Run 前，Tandem 检查并自动修复三端 Preset 与项目配置投射。修复失败时阻止启动并展示具体差异。

### 8.2 同 Agent 继续

- 原 iTerm2 tab 存在时跳回原位置；
- tab 不存在或系统重启后，在正确 Workspace 的新 tab 中调用 CLI 原生 resume；
- 重启后只列出未完成 Task，由用户选择恢复哪些；
- 不生成 Handoff，不持续 checkpoint，不重建全部终端布局；
- 原生 resume 失败时明确失败，不自动切换 Agent。

### 8.3 切换 Agent

1. 用户明确选择“交给另一个 Agent”；
2. 源 Agent 通过统一 Skill 生成精简 Handoff；
3. Tandem 补充确定性的 Git 与 Workspace 事实；
4. 目标 Agent 沿用当前 Workspace，并获得相同 Preset 与项目配置；
5. Tandem 验证正确 Workspace、Preset 和 Handoff 已送达；
6. 目标 Agent 明确确认目标、现状、约束和下一步；
7. 目标 Agent 完成第一个可验证动作；
8. 用户确认接力成功。

源 Agent 不可用时，Tandem 可以基于可得材料降级生成 Handoff，但必须标明来源和缺失信息。

### 8.4 人工协作与托管执行

快速任务默认由用户在原生 CLI 中协作推进。人工协作 Run 只显示进程、session、Workspace、最后活动、结构化等待信号和 resume 能力等可靠事实。

复杂任务可以选择托管执行，复用迁入的 `twin` supervisor/runtime。托管模式可以有更丰富的结构化状态，但必须与人工协作模式视觉区分。两种模式都属于同一 Task/Run 模型，最终都由用户确认完成。

### 8.5 完成

Agent 退出、测试通过、commit、merge 或 PR 都只能让 Task 进入待验收。只有用户明确确认后，Task 才完成。归档与 Workspace/session 清理是独立操作。

## 9. Preset 与项目配置

### 9.1 分层

```text
Tandem Host
  Task / Run / Session / iTerm2 / Git / Credentials / Profiles

Agent Preset
  Persona / Rules / Skills / Hooks / Gates / Permissions / Managed execution

Project .tandem/
  Project rules / skills / gates / preset additions
```

Host 能力跨 Run 共享。Preset 决定某次 Run 的 Agent 工作方式。项目层补充项目特有内容。

### 9.2 `.tandem/`

仓库内 `.tandem/` 保存可共享、可版本控制的 Rules、Skills、门禁和项目级 Preset 配置。本机路径、provider、模型、API key、session 和个人覆盖不进入仓库。

仓库提交轻量 `AGENTS.md`、`CLAUDE.md` 等兼容入口，使没有 Tandem 的原生 CLI 也能找到 `.tandem/` 内容。重复副本、Skills 链接和机器配置由 Tandem 生成并忽略。

### 9.3 迁移

长期目标是 Tandem 拥有统一项目配置。迁移期：

1. 发现已有 `AGENTS.md`、`CLAUDE.md`、`.cursor/rules` 和项目 Skills；
2. 展示三个 CLI 当前实际获得的差异；
3. 区分 `dev-rules` 来源、项目特有内容和手工漂移；
4. 用户确认后导入 `.tandem/`；
5. 生成并验证三端投射；
6. 只有验证通过后，原生文件才转为受管产物。

外部修改生成产物时不反向吸收，Tandem 展示漂移并从 `.tandem/` 重新生成。

## 10. 实现与迁移策略

### 10.1 CC-Switch fork

Tandem fork CC-Switch，并基于 fork 独立演进、选择性合并 upstream：

- 保留 Tauri 2、React、Rust、SQLite 和系统托盘；
- 复用 provider/Profile、Skills、连通性、配置原子写入、session parser 和终端启动；
- Tandem 领域代码放入独立模块；
- 尽量减少对 upstream 热点的不必要修改；
- 每次 upstream 合并运行配置、数据库、session 和三 CLI 回归；
- Tandem 成为唯一写入者后，CC-Switch 不再独立运行和演进。

### 10.2 `dev-rules` 与 `twin`

`dev-rules` 的 Rules、Skills、Persona、Hooks、门禁和投射机制迁入 Tandem 默认 Preset 与项目配置。现有 `twin` 的 supervisor/runtime 迁为可选托管执行能力。迁移完成后，旧仓库只保留限期兼容或迁移入口，不再拥有独立事实源。

### 10.3 本地架构

```text
macOS menu bar + desktop console
              |
      Application Services
              |
       Tandem Domain Core
              |
 +------------+------------+------------------+
 |            |            |                  |
Agent      iTerm2      Workspace        Preset Projection
Adapters   Adapter      Adapter          + Config Core
 |                                            |
Cursor / Codex / Claude                 CC-Switch fork
```

Domain Core 独占 Task、Run、NativeSessionRef、Handoff 和用户验收。Adapter 只处理外部系统能力，不能宣告 Task 完成。

## 11. Adapter 契约

Cursor、Codex 和 Claude 首版都必须实现并通过：

- `detect`：检测 CLI、版本和可用性；
- `scan`：扫描历史 session 元数据；
- `read`：按用户策略读取必要内容；
- `launch`：在正确 Workspace 启动；
- `bind`：绑定真实 session；
- `resume`：原生恢复同一 session；
- `project`：投射并验证 Preset 与项目配置；
- `handoff`：向目标 Agent 可靠注入 Handoff；
- `receipt`：记录实际 Agent、模型、Profile、Preset 和能力结果。

首版不公开“能启动即支持”的通用 Adapter。三种 CLI 的每项承诺都必须真实验证。

## 12. 数据与安全

- Task、Run、索引、标题、Handoff 和验收存于本地数据库；
- 完整 transcript 留在原生 CLI；
- 默认不上传代码、目标、diff、transcript 或 Evidence；
- 明文 API key 存入 macOS Keychain；
- 官方 OAuth 由原生 CLI 管理；
- `.tandem/`、数据库、日志、通知和 Handoff 不得包含明文 secret；
- Run 保存 Preset、项目配置和 Execution Profile 的不可变引用或快照；
- 敏感项目可关闭发现；
- push、PR、merge、发布等动作由 Agent 按 Preset 门禁执行，Tandem 不自行修改代码。

## 13. 失败语义

- 单个 CLI 或损坏 session 扫描失败不能阻断其他索引；
- 标题或聚合失败时保留原始 session 元数据；
- 投射失败先自动修复，仍失败则阻止受管 Run 启动；
- 启动失败保留 Task，不创建虚假活跃 session；
- 绑定失败允许重试或手工认领；
- 原生 resume 失败不自动转为 Handoff；
- Handoff 失败保留源 Run，目标 Run 不标记接力成功；
- 托管失败退回可理解的人工协作状态；
- 每个非终态失败都必须有具体原因和下一步。

## 14. 验证策略

### Domain

- 一个 Native Session 只能属于一个 Run；
- 一个 Run 只能属于一个 Task；
- 原生 resume 不创建接力 Run；
- 切换 Agent 必须创建 Handoff 和新 Run；
- Agent 退出不能完成 Task；
- 只有用户确认才能完成 Task；
- 运行中的 Run 不静默更换 Preset 或 Execution Profile。

### Adapter

三个真实 CLI 分别通过扫描、启动、绑定、resume、Preset 投射和 Handoff 契约测试。发布前分别运行真实最小冒烟闭环。

### Migration

验证 CC-Switch 数据导入、`dev-rules` 到 `.tandem/` 的迁移、三端一致投射、漂移修复、数据库升级和 secret 扫描。

### UI e2e

使用真实 Tandem UI 和 Playwright 覆盖首次扫描、认领历史、创建任务、打开 iTerm2、应用或 Mac 重启后的原生 resume、切换 Agent、确认接力和用户完成。

## 15. 交付阶段

### 阶段 1：首个可日用版本

- Fork CC-Switch 并建立 upstream 同步机制；
- 菜单栏、行动台账和 Domain Core；
- 三 CLI 历史索引、候选 Task 和搜索；
- 新任务启动、Task/Run/session 绑定和 iTerm2；
- 原生 resume；
- 默认 Preset、`.tandem/` 和三端投射；
- 轻量 Handoff 与用户确认接力；
- 可选托管执行；
- 用户确认完成。

### 阶段 2：比较与融合

- “接着做、对比做、帮我评审”三种用户意图；
- 对比 Run 使用迁入的 `wts` 创建独立 worktree；
- 先按共同标准判断可用性，再展示方案、证据、风险、成本和人工介入；
- 不生成统一总分；
- 用户选择主方案与融合要求；
- 简单采用沿用主方案 Workspace，多方融合创建独立 worktree；
- 启动融合 Agent 推进最终结果，Tandem 不自行合并代码。

## 16. 成功指标

北极星指标：

> 重要 Code Agent Task 经 Tandem 建立可读关系，并最终由用户确认完成的比例。

首版连续两周 dogfood 必须同时满足：

- 至少 80% 的新 Code Agent 任务从 Tandem 启动或被认领；
- 重启后用户首先通过 Tandem 恢复重要工作；
- 任一近期重要任务可在 30 秒内找到并原生 resume；
- 没有任务因不可读 session ID 被放弃；
- 完成多次真实跨 Agent 接力，无需手工翻找和复制原始指令；
- Tandem 没有显著增加简单任务的启动步骤。

## 17. 产品与开源边界

首版是个人本地产品，不做账户、团队 Task 或云同步。`.tandem/` 可经 Git 与协作者共享，未安装 Tandem 的成员仍能使用兼容入口。

产品成熟后考虑开源，但当前不预设开源范围或商业分层。从第一天保持模块边界、数据可导出性，以及 CC-Switch、`dev-rules`、Skills 和其他来源的许可证与修改记录。是否开源、采用何种许可证和商业模式，在真实产品价值与用户需求得到验证后决定。

## 18. 尚待技术验证

以下问题不改变已确认产品设计，但会影响实施计划：

- Cursor、Codex、Claude 当前版本的历史格式、session 绑定和原生 resume 细节；
- CC-Switch fork 中 upstream 友好的 Tandem 模块边界；
- CC-Switch 数据迁移、单一写入者切换和 Keychain 迁移；
- iTerm2 稳定 tab 引用、跳转和重启后的新现场创建；
- `.tandem/` 标准格式、兼容入口和三端投射矩阵；
- 从 `dev-rules` 与 `twin` 迁移哪些实现、删除哪些重复领域概念；
- 轻量 Handoff v2 schema 与投递确认协议；
- 本地数据库备份、恢复、保留和彻底删除；
- 签名、公证、自动更新与商业分发。
