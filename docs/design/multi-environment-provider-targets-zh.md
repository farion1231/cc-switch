# 多环境 Provider 管理设计

状态：已确认设计，尚未实施

首个交付范围：Windows 版 CC Switch 管理 Windows Codex 与一个或多个 WSL2 Codex 环境

## 1. 背景

当前 CC Switch 为每个 Application 保存一个配置目录覆盖，例如 Codex 只有一个 `codexConfigDir`。所有 live 配置读写、Provider 回填、MCP 重投影和会话扫描最终都解析到这个全局目录。

这在 Windows 与 WSL2 同时使用 Codex 时产生四类问题：

1. CC Switch 一次只能管理 Windows 或 WSL 中的一边。
2. 把完整 Windows 配置复制到 WSL 会混入 Windows 路径、命令和工作区设置。
3. Provider 切换会整体写入配置并回填共享 Provider，某个环境的本地改动可能污染另一个环境。
4. Codex 原生历史按 `model_provider` 分桶；切换后记录可能不可见，即使文件并未丢失。

本设计不增加一个特例式的“WSL Codex 目录”，而是建立通用的 Managed Target 模型。

## 2. 目标与非目标

### 2.1 第一版目标

- Windows 版 CC Switch 自动发现并管理 Windows 与多个 WSL2 用户环境。
- Provider 定义共享，但每个 Managed Target 独立记录当前 Provider 和 Target Override。
- 用户可以只切换一个环境，也可以显式多选环境进行事务式联动切换。
- Provider 切换只投影 Managed Fields，Local Fields 原样保留。
- Windows 与 WSL 的官方认证、路径和会话历史互相隔离。
- CC Switch 会话管理器只读聚合所有环境的会话，并依据 Session Provenance 辅助恢复。
- 现有单目录用户无损迁移为一个 Managed Target，升级过程不重写 live 配置。

### 2.2 第一版非目标

- 不合并或改写 Codex 原生历史分桶。
- 不承诺跨 Provider 无损续聊；`encrypted_content` 可能只能由原后端解密。
- 不同步 Windows/WSL 的完整配置目录。
- 不对多环境同步 MCP、Skills 或 Prompts。
- 不支持 WSL 代理接管、热切换或自动故障转移。
- 不实现 SSH、Dev Container 或远程主机，但接口必须允许后续增加 Adapter。
- 不由 WSL/Linux 版 CC Switch 反向管理 Windows。

## 3. 核心原则

### 3.1 Provider 与 Environment 分离

Provider 只描述模型后端，例如 API 地址、凭据、模型和协议。Environment 拥有配置路径、官方登录、工作区、MCP、沙箱和会话历史。

```text
Provider A
  ├─ base_url
  ├─ api_key
  ├─ model
  └─ wire_api

Windows Codex Target
  ├─ config directory
  ├─ current Provider A
  ├─ Windows Local Fields
  └─ optional Target Overrides

Ubuntu Codex Target
  ├─ distro + Linux user + config directory
  ├─ current Provider B
  ├─ Linux Local Fields
  └─ optional Target Overrides
```

### 3.2 未声明即本地

Application Adapter 必须显式声明它管理的字段。任何未知字段默认属于 Local Field，不得跨环境传播。这是避免新版本 Codex 增加字段后被 CC Switch 意外覆盖的安全默认值。

### 3.3 添加环境不等于接管环境

添加 Managed Target 只执行只读探测。首个可用版本允许用户选择：

- 关联到已有 Provider；
- 保持为 Unmanaged Environment。

从受管字段导入为新 Provider 属于后续增强，不是首个版本的接管前置条件。

在用户第一次主动切换前，不得重写该环境的 live 配置。

### 3.4 切换不触碰会话

普通 Provider 切换不得读取或写入：

- `sessions/`；
- `archived_sessions/`；
- Codex state SQLite；
- 会话索引与会话正文。

会话扫描是独立的只读能力。

## 4. 领域模型

### 4.1 Managed Target

建议的设备本地结构：

```rust
struct ManagedTarget {
    id: TargetId,
    app: AppType,
    name: String,
    kind: TargetKind,
    config_location: ConfigLocation,
    current_provider_id: Option<String>,
    management_state: ManagementState,
    provider_overrides: Map<ProviderId, TargetOverride>,
    last_viewed_at: Option<Timestamp>,
}

enum TargetKind {
    LocalWindows,
    Wsl { distro: String, user: String },
    // Future: LocalUnix, Ssh, DevContainer
}

enum ManagementState {
    Managed,
    Unmanaged,
    Offline,
}
```

约束：

- Target ID 稳定，改名或路径展示变化不改变身份。
- 一个规范化后的实际配置目录只能属于一个 Managed Target。
- 同一 WSL 发行版的不同 Linux 用户可以是不同 Managed Target。
- Target、Target Override、当前 Provider、会话来源账本和快照均不参与 WebDAV/S3 同步。

### 4.2 Provider 与 Target Override

Provider 继续作为共享定义存储。Target Override 只保存与共享定义不同的显式值，并在界面显示最终值来源。

生效优先级：

```text
Target Override > Provider Managed Field
```

共享 Provider 更新时，已有 Target Override 保持不变。用户可以“恢复继承”以删除覆盖。共享字段被删除时，相关覆盖标记为孤立覆盖，等待用户处理。

### 4.3 Session Provenance

建议的设备本地账本键：

```text
(target_id, application, session_id) -> origin_provider_id | unknown
```

新会话在 CC Switch 运行时自动记录。CC Switch 关闭期间创建的会话，在下次启动时根据 Target 的切换记录、创建时间和 live 配置补充；无法可靠判断时必须标记为未知，不得猜测。

旧会话第一次恢复时，用户可以选择原 Provider，CC Switch 随后记住映射。

## 5. 模块与接口

### 5.1 Target Adapter seam

每种执行环境通过同一个小接口隐藏文件系统与命令差异：

```rust
trait TargetAdapter {
    fn inspect(&self, target: &ManagedTarget) -> Result<TargetInspection>;
    fn read_live(&self, target: &ManagedTarget) -> Result<LiveDocument>;
    fn snapshot(&self, target: &ManagedTarget) -> Result<SnapshotId>;
    fn apply(&self, target: &ManagedTarget, plan: &ProjectionPlan) -> Result<ApplyReceipt>;
    fn restore(&self, target: &ManagedTarget, snapshot: &SnapshotId) -> Result<()>;
    fn running_processes(&self, target: &ManagedTarget) -> Result<ProcessSummary>;
    fn scan_sessions(&self, target: &ManagedTarget) -> Result<Vec<SessionRecord>>;
    fn resume_session(&self, target: &ManagedTarget, request: ResumeRequest) -> Result<()>;
}
```

第一版包含：

- `LocalWindowsTargetAdapter`：Rust 直接操作 Windows 文件和进程。
- `WslTargetAdapter`：通过受限的 `wsl.exe` 在指定发行版和用户内部操作。

未来可以增加 `SshTargetAdapter`，而不改变 Provider、Projection 或事务调用方。

### 5.2 Provider Projection module

Projection 分为计划与执行：

```rust
fn plan_projection(
    target: &TargetInspection,
    provider: &Provider,
    overrides: &TargetOverride,
) -> Result<ProjectionPlan>;

fn apply_transaction(
    targets: &[ManagedTarget],
    plan: &MultiTargetProjectionPlan,
) -> Result<ProjectionOutcome>;
```

`ProjectionPlan` 必须包含：

- 将修改的 Managed Fields；
- 保留的 Local Fields；
- 检测到的 Drift；
- 配置语法验证结果；
- 预计写入文件；
- 回滚快照引用；
- 不包含密钥的用户可见 diff。

### 5.3 Codex 字段所有权

第一版先通过特征测试锁定精确字段集合。原则性分类如下：

| 类别 | 示例 | 所有者 |
| --- | --- | --- |
| 后端路由 | active `model_provider`、对应 provider table、`base_url`、`wire_api` | Provider |
| 模型选择 | `model` 及明确的 Provider 模型能力设置 | Provider，可被 Target Override |
| 第三方凭据 | API Key、Provider-scoped bearer token | Provider |
| 官方登录 | ChatGPT OAuth、刷新令牌、官方 `auth.json` | Managed Target |
| 工作区与路径 | `projects`、Windows/Unix 路径、`sqlite_home` | Managed Target |
| 本地行为 | approval、sandbox、notices、hooks | Managed Target |
| 扩展 | MCP、Skills、Prompts | 第一版不参与多 Target Projection |
| 未识别字段 | 新版 Codex 增加的未知键 | Managed Target |

禁止再把完整 `config.toml` 作为共享 Provider 模板写入多个环境。现有 Provider 升级时，只提取明确的 Managed Fields；无法分类的内容保留在原 Target。

## 6. WSL 执行安全

WSL 的读写必须在发行版内部完成，而不是通过 UNC 直接写入：

- 使用 `wsl.exe -d <distro> -u <user> -- ...`；
- 发行版和用户先校验，不拼接到任意 shell 文本；
- 使用固定脚本，路径作为独立参数传递；
- 内容通过 stdin 传入；
- 在目标文件同目录创建临时文件并原子重命名；
- 写后在 WSL 内验证 TOML/JSON；
- 保留 Linux 所有者和严格权限；
- 禁止为方便访问而自动使用 root。

自动发现为主，手动路径为高级选项。首个版本按发行版默认用户发现，至少返回发行版、默认用户、真实 home、Codex 目录和在线状态。同一发行版的多 Linux 用户注册留作后续增强。

## 7. 切换流程

### 7.1 单 Target

```text
选择一个 Environment
  -> 检查在线状态与目录身份
  -> 读取 live 并检测 Drift
  -> 如有运行中 Codex，显示警告
  -> 生成并展示 ProjectionPlan
  -> 建立快照
  -> 原子应用 Managed Fields
  -> 验证结果
  -> 最后更新 Target current_provider_id
```

现有 Codex 进程不被结束。界面必须说明：现有进程可能继续使用旧 Provider，新启动或恢复的会话使用新配置。

### 7.2 多 Target

多选必须显式发生，默认只选择当前查看的 Environment。事务流程：

```text
预检所有 Target
  -> 为所有 Target 建立快照
  -> 逐个应用
  -> 全部验证成功后提交 current Provider 状态
  -> 任一失败则按相反顺序恢复已修改 Target
```

离线 Target 会在预检阶段终止事务，因此不会出现部分写入。

### 7.3 Drift

发现 Managed Field 被外部修改时暂停切换，提供：

- 保存为当前 Target Override；
- 放弃外部 Managed Field 修改并继续；
- 取消。

禁止自动把某个 Target 的 live 配置回填进共享 Provider。

### 7.4 Provider 编辑与删除

编辑 Provider 时列出正在使用它的 Environment。用户选择“仅保存定义”或“保存并应用到所选环境”；多环境立即应用仍使用事务式切换。

使用中的 Provider 禁止直接删除。用户必须先切换相关 Target，或选择替代 Provider 并在全部切换成功后删除。

## 8. 快照与恢复

- 每次实际修改前建立完整配置快照。
- 快照按 Target 隔离，保留最近 10 个。
- 用户可以固定重要快照；固定项不参与自动清理。
- 多 Target 事务所需快照在事务结束前不得清理。
- 快照使用严格文件权限，并在日志和 UI 中隐藏密钥。
- 第三方切换默认不修改官方 `auth.json`，因此不重复备份它。
- “停止管理 Environment”不删除目标配置、认证、会话或 state DB。

## 9. 会话管理

### 9.1 聚合与身份

会话列表使用复合身份：

```text
(target_id, application, session_id, source_path)
```

列表显示 Environment 标签并支持筛选。离线 Environment 可以显示缓存元数据，但不能读取正文、删除或恢复。

### 9.2 恢复

- 当前 Environment 已使用来源 Provider：直接恢复。
- 当前 Provider 不同：显示来源 Environment、来源 Provider 和当前 Provider，用户确认后只切换该 Environment，再执行恢复。
- 来源 Provider 已删除：只允许查看或重新绑定替代 Provider。
- 来源未知：让用户选择一次并记录。

这保证“尽可能回到原后端续聊”，但不声称任意 Provider 都能解密另一后端生成的推理内容。

### 9.3 不采用原生历史合并

第一版不重写 JSONL 或 state DB 的 `model_provider`。原因是原生列表统一只能解决可见性，无法解决跨后端续聊，而且会模糊真实来源。CC Switch 的聚合视图提供可见性，Session Provenance 提供恢复指导。

## 10. 用户界面

### 10.1 Codex 主页面

Provider 列表上方显示 Environment 选择器：

```text
环境：
[ Windows | Provider A ] [ Ubuntu-24.04 · m1kasa | Provider B ] [管理环境]
```

- 第一次默认 Windows；之后记住上次查看的 Environment。
- 默认单选；用户显式进入多选后才能联动切换。
- 多选按钮明确显示“切换到 Provider X（2 个环境）”。
- 离线、Unmanaged、Drift 和 Target Override 均有清晰状态。

### 10.2 设置页

`设置 -> 高级 -> 环境管理` 提供：

- 自动发现与添加；
- 编辑名称、发行版、用户和配置位置；
- 连接测试；
- Provider 关联或保持 Unmanaged；
- Target Override 管理；
- 快照查看与恢复；
- 停止管理。

### 10.3 托盘

Codex 托盘菜单改为两级结构，并且只切换单个 Environment：

```text
Codex
  Windows
    Provider A
    Provider B
  Ubuntu-24.04
    Provider A
    Provider B
```

多 Target 联动只在主界面完成。

## 11. 兼容与迁移

首次加载新结构时：

1. 读取现有 `codexConfigDir` 与有效当前 Provider。
2. 默认 Windows 路径转换为 `Windows Codex` Managed Target。
3. `\\wsl$` 或 `\\wsl.localhost` 路径解析为对应 WSL Managed Target。
4. 保留目录、当前 Provider 和 live 内容，不执行 Projection。
5. 标记迁移完成，保证幂等。
6. 显示一次添加其他 Environment 的引导。

Provider 导入导出继续只处理共享定义，不包含 Target、本地路径、Target Override、当前状态、会话来源、快照或官方认证。

## 12. 分阶段实施计划

### 阶段 0：特征测试与 seam 准备

- 为现有 Codex 目录解析、live 读写、Provider 切换、认证保留和会话扫描补充特征测试。
- 将隐式全局 `get_codex_config_dir()` 调用逐步收束到显式上下文，但保持行为不变。
- 建立 Codex Managed Field 分类测试，未知字段必须保留。
- 建立“切换不修改会话目录/state DB”的哈希回归测试。

### 阶段 1：设备本地 Managed Target

- 增加 Target 存储、规范化目录身份和旧设置幂等迁移。
- 实现 Windows Adapter 与 Target inspection。
- 让当前 Provider 从全局单值迁移为 Target 级状态，同时保留旧接口兼容层。
- 加入快照轮换与恢复。

### 阶段 2：Codex Projection

- 拆分共享 Managed Fields 与 Target Local Fields。
- 实现 ProjectionPlan、无密钥 diff、Drift 检测和单 Target 原子应用。
- 移除多 Target 路径中的自动共享 Provider 回填。
- 实现 Provider 编辑应用范围和使用中删除保护。

### 阶段 3：WSL Adapter 与多 Target 事务

- 自动发现 WSL 发行版、用户、home、Codex 目录和在线状态。
- 实现 WSL 内部的安全读写、快照、进程探测和配置验证。
- 实现多 Target 预检、执行、逆序回滚和状态提交。
- 覆盖离线、权限失败、语法失败和回滚失败测试。

### 阶段 4：界面与托盘

- 主页面 Environment 选择器与显式多选。
- 设置页 Environment 管理、关联/导入/Unmanaged 流程。
- Target Override 来源展示与恢复继承。
- 托盘“Environment -> Provider”菜单。

### 阶段 5：会话聚合与来源恢复

- SessionMeta 增加 Target 身份与来源状态。
- 多 Target 并行只读扫描、缓存和复合键去重。
- 建立 Session Provenance 账本及旧会话人工绑定。
- 实现 Windows/WSL 环境感知恢复和 Provider 切换确认。

### 后续阶段

- Target-aware 本地代理：Target 路由命名空间、独立 Provider/熔断状态、WSL 可达性和非回环鉴权。
- Claude、Gemini 等 Application Adapter。
- SSH Adapter：远程认证、SFTP/命令执行、远程锁与回滚。
- MCP、Skills、Prompts 的多 Target 所有权与投影模型。

## 13. 验收矩阵

| # | 验收项 | 自动化重点 |
| --- | --- | --- |
| 1 | 发现并添加多个 WSL 用户 Environment | 发行版/用户解析、重复目录拒绝 |
| 2 | 添加时不修改现有配置 | 添加前后文件哈希一致 |
| 3 | Windows 与 WSL 使用不同 Provider | Target current 状态隔离 |
| 4 | 显式多选联动切换 | 两边最终投影一致 |
| 5 | 任一 Target 失败全部回滚 | 故障注入、逆序恢复 |
| 6 | Local Fields 保持不变 | 结构化 diff + 未知键保留 |
| 7 | 官方认证隔离 | 两边 auth 哈希与所有权 |
| 8 | 切换不修改会话与 state DB | 目录树与 DB 哈希回归 |
| 9 | 会话聚合显示 Environment | 复合键、筛选、重复 ID |
| 10 | 来源未知不猜测 | unknown 状态与人工绑定 |
| 11 | 离线 WSL 不影响 Windows | 单选成功、多选预检失败 |
| 12 | 每 Target 最近 10 个快照 | 轮换、固定、事务保留 |
| 13 | 托盘按 Environment 切换 | 菜单作用域测试 |
| 14 | 未实现能力不被误开放 | WSL proxy/SSH feature gating |
| 15 | 单目录升级无损 | 幂等迁移、live 零写入 |

## 14. 实施门槛

进入业务实现前必须先满足：

- 本文与 ADR 经维护者确认；
- 阶段 0 的特征测试能够在未改行为的代码上通过；
- Codex Managed Field 白名单完成逐项审查；
- 失败与回滚日志不泄露 API Key、OAuth 数据或完整 URL 查询参数；
- Windows 与至少一个真实 WSL2 发行版具有手工集成测试清单。

## 15. 当前实现进度（2026-07-21）

已完成的首个可用竖切：

- 设备本地 Managed Target 注册表、旧单目录幂等迁移、规范化目录去重；
- Windows 与 WSL 只读检查，WSL 发行版/默认用户/home/Codex 目录发现；
- 设置页 Environment 注册、Provider 关联、显式首次启用管理；
- Codex Managed Field 明确白名单投影，未知字段和路径类字段默认归 Target；
- Windows 与 WSL 单 Target 独立切换，Target current Provider 与旧全局 current 隔离；
- WSL 内部同目录临时文件 + `mv` 原子替换，状态提交失败时精确恢复原始字节；
- WSL 写入保留原文件权限（新文件使用 `0600`），替换后通过 WSL 读回并校验完整字节；
- 内联 model catalog 在 Windows/WSL 内按安全顺序写入或删除，并与 `config.toml` 一起快照回滚；
- 切换不写 `auth.json`、sessions、archived_sessions 或 state DB；
- WSL 命令只使用独立 argv 和 stdin，不启动 shell，也不使用 UNC 直接写入。
- 已用 Windows 原生 `stable-x86_64-pc-windows-msvc` 对当前工作区完成 `cargo check --lib`；并从真实 Windows/WSL 互操作链路发现 `Ubuntu-24.04`、默认用户、HOME 与 `.codex` 目录。

尚未完成、界面也不应暗示已经支持：

- 多 Target 一次联动切换及逆序回滚；
- 持久化快照轮换、Drift 检测、Target Override 编辑；
- WSL 进程探测与占用提示；
- 会话聚合 UI、Session Provenance 与跨环境恢复；
- Target-aware proxy、托盘菜单、SSH 和其他 Application Adapter；
- 同一 WSL 发行版的多用户发现/注册，以及从现有环境直接导入新 Provider；
