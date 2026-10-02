# VPS 自动托管 Skill：执行计划与新会话交接

日期：2026-09-29。关联 [Issue #7739](https://github.com/farion1231/cc-switch/issues/7739)。

**2026-10-02 新授权：** 用户要求按调查结论制定计划并实现客户端密码自动复用，替代第 17–18 节当时的搁置决定。新阶段计划及验证记录见 [VPS 客户端密码复用](vps-client-password-reuse.md)；保留系统凭据库，新增一次性本机 SSH 执行入口，不新增 MCP/Agent/常驻服务。本轮不包含提交、推送或生产服务器连接授权。

**用途：在新对话中直接执行本计划，不再重新讨论已确认的产品方向。阶段 A–F 已实现，G 的自动化复验已推进至完整 Rust 库/集成目标；三个既有 Windows 1314 测试已通过夹具修复并定向验证，原提交前上游合并与全量复验见第 19 节；PR 提交后的 CI/评审修复与最新上游整合见第 20 节。用户已反馈页面和真实服务器 SSH 测试通过，客户端端到端及其余认证/平台验收未完成。认证扩展见第 15 节，帮助退出/表单分组修复见第 16 节，最新用户实测反馈与下一步见第 17 节。** 产品要求以 [最终方案](../proposals/vps-skill-management.md) 为准；旧 Issue 草稿和早期聊天中的双开关/可见自动 Skill/自定义客户端注册方案均不再采用。

## 0. 开发启动条件与边界

### 已完成

- 工作分支：`feat/vps-skill-management`。
- 基线提交：`846de29c13ac4d65f164db8c15dd5fd58e29f972`，来自当时最新 `upstream/main`。
- `origin` 为用户 fork，`upstream` 为官方仓库；没有创建 commit、推送或 PR。
- Node 22.20.0、pnpm 10.12.3、Rust/Cargo 1.95.0、rustfmt/clippy、Windows C++ Build Tools/SDK、WebView2、系统 SSH 已核对。
- `pnpm install --frozen-lockfile` 完成，依赖/锁文件未修改。
- TypeScript、前端格式、前端构建、Rust 格式与 Clippy 已通过。
- 最近完整前端低并发测试：146 文件、1659 测试通过。
- Issue 已发布，方案已确认到可实施粒度；维护者范围意见与最终合入仍不能假定。

### 已知基线问题，不得隐瞒或混为新功能回归

最近 Rust 库测试：2890 通过、4 失败、10 忽略；因库测试失败，后续集成测试未执行。单编译任务可以完成编译，原来的页面文件不足已通过降低并发规避，没有修改系统设置。

| 原有失败                                                                                | 已观察到的结果                                 |
| --------------------------------------------------------------------------------------- | ---------------------------------------------- |
| `codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir`              | Windows error 1314：无创建符号链接所需权限     |
| `services::session_usage_grokbuild::tests::symlink_cycle_does_not_cause_stack_overflow` | 同上                                           |
| `services::model_pricing::tests::reloads_manual_file_edits_and_deletion_tombstones`     | 预期 1、实际 0；单独运行仍失败                 |
| `services::skill::tests::migrate_storage_safely_leaves_an_existing_pi_ssot_alias`       | 全量运行计数失败；单独运行目标 SKILL.md 不存在 |

这四项没有被修复，也没有新增 skip/放宽断言。Windows Developer Mode 未启用；不能擅自改注册表、提权或重装工具链。处理权限需要用户确认，另两项需独立定位。

**可以开始本地功能开发**：工具链、分支、依赖、设计和可比较基线已就绪。**不能宣称整体测试全绿或直接提交 PR**。基线问题单独跟踪，新功能与受影响测试必须通过；PR 前处理/清楚报告剩余基线，并验证尚未执行的集成测试。

尚需真实验证的资源：测试 VPS/VM、macOS 环境、各客户端安装及模型调用授权。当前仅检测到 Claude Code/Codex，未找到 OpenCode。纯实现和本地 mock/临时目录测试无需先提供生产服务器或私钥。

## 1. 新会话先读什么

按顺序：

1. 本文件。
2. `docs/proposals/vps-skill-management.md`。
3. `CONTRIBUTING.md`，尤其 AI 辅助贡献、隔离测试、四语言文案要求。
4. `.cc-switch/contribution-prep/preflight.md`（本机检查记录；该目录被 Git 忽略）。
5. 根据当前阶段读取下面列出的代码，而不是从头遍历整个仓库。

开始时执行 `git status --short --branch`、`git log -1 --oneline`、`git diff --stat`，保护用户新改动。不要重建分支、重置 main、自动拉取并合并新提交。当前准备文档未提交，新会话应保留并更新它们。

仅在用户明确要求后 commit、push、发布评论或创建 PR；实施计划本身不包含对外发布授权。不要修改既有全局 Agent 权限以通过测试。

## 2. 不可偏离的交互

- VPS 在 **Skills、提示词、会话管理、MCP 所在功能导航组** 中，与它们平级。
- 当前代码的这组按钮在供应商页页头工具组，不是左侧应用选择或设置页；沿用该结构。
- 默认分支现有顺序为 Skills → 提示词 → 会话 → MCP。把 VPS 放在同组末尾，不重排原项；在分支外放一个共同入口，避免 Hermes/OpenClaw 等分支重复实现或漏掉全局页。
- `VpsPanel` 是全局主机页，不把当前 `activeApp` 自动视为接入客户端。
- VPS 页的客户端选择自动完成 Skill 配置，**没有第二个 Skill 开关，没有额外常驻 Skill 状态行**。
- 自动 Skill 在普通 Skill 管理中隐藏；普通手动 Skill 正常显示。
- 同名/同目录手动导入与自动实例冲突时拒绝覆盖并给出明确原因，不静默接管。
- 成功无需额外操作；失败或确需重载时才提示。pending 控件和错误提示不是新增常驻状态模块。

## 3. 代码落点

行号只用于定位当前基线，执行时以符号为准。

| 范围              | 现有位置 / 预计新增位置                                                                                                                   |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 页面路由与记忆    | `src/App.tsx`：`View`（约 116）、`VALID_VIEWS`/`getInitialView`（150–173）、`renderContent`、页头标题/动作                                |
| 同栏入口          | `src/App.tsx` 页头 `currentView === "providers"` 的功能按钮组（约 1601–1766）；新增全局 VPS 项，不复用 `sharedFeatureApp` 的 Desktop 映射 |
| 客户端范围        | `src/config/appConfig.tsx` 的 `SKILLS_APP_IDS`；`src-tauri/src/app_config.rs` 的 `SkillApps`                                              |
| VPS 前端          | 预计 `src/components/vps/VpsPanel.tsx`、必要的主机表单/行组件、`src/hooks/useVps.ts`、`src/lib/api/vps.ts`；命名遵循实现时邻近代码        |
| VPS 后端          | 预计 `src-tauri/src/commands/vps.rs`、`src-tauri/src/services/vps.rs`（确有需要再拆子模块）；注册到现有 mod 与 `lib.rs`                   |
| Skill 来源        | `src-tauri/src/app_config.rs::InstalledSkill`；`src/types.ts` 对应类型；`src-tauri/src/database/schema.rs`、`dao/skills.rs`               |
| Skill 生命周期    | `src-tauri/src/services/skill.rs`：`persist_and_sync_new_skill`、`toggle_app`、`sync_to_app_dir`、扫描/导入/卸载/迁移/更新                |
| Skill 公共接口/UI | `src-tauri/src/commands/skill.rs`、`src/lib/api/skills.ts`、`src/hooks/useSkills.ts`、`UnifiedSkillsPanel.tsx` 及实际消费列表/计数的入口  |
| 同步/备份         | `src-tauri/src/database/backup.rs`、`src-tauri/src/services/sync_protocol.rs` 与其 Skill ZIP 打包流程                                     |
| 文案              | `src/i18n/locales/{zh,zh-TW,en,ja}.json`                                                                                                  |

不新增客户端注册表，不修改供应商切换模型，不实现独立 MCP 服务。原有代码注释可能仍写“三应用/四应用”，客户端实际集合应以类型/能力列表为准。

## 4. 按阶段实施

### 阶段 A：托管来源与管理权（先写测试）

1. 在现有 Skill 记录上设计最小、持久化的 VPS 托管来源标记；例如内部 owner 字段。具体字段名可遵循项目命名，但不创建第二张完整 Skill 表或第二套存储。
2. 如需数据库列，使用现有迁移机制；旧记录默认普通用户管理，序列化/DAO/测试构造同步更新。
3. 只有内部 VPS 注册路径可以设置该标记；不能信任导入 Skill 的 frontmatter 或外部输入自报内部来源。
4. 普通 Skill 展示/搜索/计数/批量操作排除托管项，但内部生命周期仍能枚举；不要全局过滤数据库记录导致同步失去对象。
5. 普通启停、卸载、导入、恢复入口对托管记录执行来源/所有权保护；内部 VPS 操作使用明确的内部路径，不向前端开放任意绕过保护参数。
6. 同 ID/目录冲突明确报错，不重复导入；没有所有权证据不按名称接管已有目录。

验收：旧 Skill 行为不变；托管记录重启后仍可识别；显示隐藏与内部管理分离；导入不能伪造托管身份。

### 阶段 B：主机数据、目录和聚合规则

1. 新增本机 VPS 数据结构/存储，复用 `SkillApps` 及路径/原子写入基础。
2. 主机字段校验包括控制字符、选项前缀、SSH config 换行与路径注入；自动生成稳定 ID/别名。
3. 从主数据生成独立 `ssh_config` 和各应用目录，不整体覆盖用户 `~/.ssh/config`。
4. 纯函数计算每个应用是否仍被任一主机使用，以及更新前后的安装/更新/清理差异。
5. 多文件生成幂等；写入失败返回可诊断结果并能安全重试，不把数据已保存当作客户端已接入。

验收：零/一/多主机、交错应用绑定、删除一台但仍有引用、最后解绑、无效/损坏文件、写入失败/重试均有临时目录测试。

### 阶段 C：复用 Skill 服务做自动部署

1. 提供一份 `cc-switch-vps` 通用模板，内部注册为 VPS 托管来源，不伪造仓库地址。
2. 按应用生成确定的目录引用；共享模板不含特定应用路径，不在符号链接目标上修改共享源。
3. 复用现有目录解析、复制/链接、登记和移除能力，仅增加该生成来源需要的投影。
4. 新增绑定自动安装；仍有引用只更新；最后解绑才清理该应用的托管部署。
5. Pi 的文件启用语义、Pi/MiniMax Code 的目录/哈希判断沿用原契约，适配生成内容后保持一致。
6. 同一投影适用于首次安装、启动恢复、重同步、迁移和更新；不只修首次安装路径。
7. 并发操作使用现有锁策略/必要的串行协调，避免主机保存和 Skill 清理互相覆盖。

验收：从 VPS 选择到客户端可读部署是一个可验证流程；没有第二开关；失败不标成功；其他主机/客户端/用户文件不受损。

### 阶段 D：扫描、导入、备份和恢复

1. 自动扫描不能把已托管部署再登记为普通 Skill，尤其检查 Pi 原生扫描路径。
2. 托管实例不走普通远端仓库更新检查；需要模板更新时仍走受控生成流程。
3. 排除/隔离带本机引用的目录与投影，明确数据库导出和 Skill ZIP 的处理，而不是只保护 `servers.json`。
4. 恢复后不能变成普通可见 Skill；在本机主数据存在时受控重建，不因远端备份缺失主机数据而猜测绑定。
5. 只清理拥有且内容符合预期的部署，手工修改/同名目录/别名重叠须报告冲突。

验收：重启、重复扫描、用户手动导入、迁移、云同步/备份恢复不产生重复、可见的自动项或错误客户端引用。

### 阶段 E：SSH 测试连接

1. 直接以参数数组调用实际环境里的 OpenSSH；不要引入 SSH 库/远端服务来替代已经存在的 SSH。
2. 固定非修改性探测，明确超时、取消和退出状态；异步处理避免阻塞 UI。
3. 使用主机指纹验证，首次确认与指纹变化区分；不得用 `StrictHostKeyChecking=no` 绕过。
4. 不收集/记录私钥内容，不在 argv/env/日志传密码；首版使用密钥引用或 SSH agent。
5. 测试进程执行与参数构造时用可控替身/测试接缝，不让单元测试连接公网或操作真实服务器。

验收：SSH 缺失、认证失败、主机指纹变化、超时/取消、空格/Unicode 路径均可测试；成功只代表该次探测，不宣称共享长期会话。

### 阶段 F：VPS 页面与导航

1. `View`、`VALID_VIEWS` 增加 `vps`，补页面分发、标题、返回/新增动作、视图持久化和滚动行为。
2. 在当前功能按钮组末尾增加 VPS 图标按钮（优先现有 lucide `Server`）；跨 Hermes/OpenClaw 等分支使用共同入口，不重复整套导航。
3. 页面包含主机列表、添加/编辑/删除、测试连接、客户端选择；复用现有组件和主题，不新建监控卡片、聊天框或视觉体系。
4. 客户端选择只使用已有 Skill 支持集合。VPS 为全局页，不把当前 Provider 的 activeApp 当作自动授权。
5. 选择客户端触发自动配置，成功只体现选择结果；不添加“Skill 已配置”常驻状态或 Skill 页第二开关。
6. 失败即时提示，pending 防重复提交；首次接入说明与确需重载提示可按需出现。
7. 自动 Skill 不出现在普通 Skills 列表中；普通手动 Skill 展示和控制照常。
8. 补齐四语言键、可访问名称、键盘操作和现有导航 busy/未保存状态保护。

验收：入口确实与 Skills/提示词/会话/MCP 同栏；恢复 `vps` 视图有效；不同当前应用下仍为同一主机页；无新增双开关/常驻标签。

### 阶段 G：集成验证与提交材料

1. 跑新增/受影响测试，比较既有基线，不更新无关快照。
2. Windows 实际启动 Tauri，并使用隔离数据或经用户确认的配置；不要未确认就让开发版接管日常供应商/Skills。
3. 使用可恢复的测试 VPS/VM 和普通测试账户，明确授权后做实际连接；不要求用户把私钥内容贴入对话。
4. 在声明兼容的客户端中分别验证发现目录、执行 SSH、修改/取消绑定；模型调用可能计费，先获得相应授权。
5. macOS 真实验证由可用设备/测试者完成。未测就记录未测，不以 CI 编译代替。
6. 准备脱敏截图、文档、实际测试表，PR 关联 #7739；发布动作等待用户明确授权。

## 5. 必须覆盖的测试矩阵

| 场景                                 | 必须成立                                               |
| ------------------------------------ | ------------------------------------------------------ |
| 两台主机共享一个客户端，删除其中一台 | Skill 保留，仅目录少一台                               |
| 最后一台主机解绑                     | 仅清理对应客户端的自动部署                             |
| 同一客户端重复选择、快速点击         | 不重复登记、不交错覆盖                                 |
| 写目录成功但部署失败                 | 不报告成功，重试可恢复                                 |
| 自动 Skill 被扫描/导入               | 不变为普通项、不重复                                   |
| 用户导入相同 ID/目录                 | 提示由 VPS 托管，不覆盖                                |
| 普通 Skill 恰好使用相似名称          | 不按名称误隐藏/接管                                    |
| 重启、全量同步、存储迁移、恢复       | 保持来源、投影和应用引用                               |
| Pi/MiniMax Code 原生检测             | 与生成内容/哈希一致，不重写原有启用语义                |
| 用户修改文件或目标为异常链接         | 不破坏用户/外部文件                                    |
| 新客户端由既有 Skill 能力扩展        | VPS 不依赖单独三客户端名单                             |
| 当前选择 Desktop/OpenClaw            | 全局 VPS 入口不混淆；它们不出现在页内 Skill 客户端选项 |
| 初次 SSH/指纹变化/认证失败           | 验证与错误清楚，无不安全绕过                           |
| 普通 Skill 页面                      | 无自动 VPS 项，无第二个控制入口                        |

## 6. 执行命令与资源约束

本机是 16 GiB 级 Windows 环境。不要同时跑完整前端测试和大规模 Rust 编译。推荐低并发，不能通过提高超时、跳过失败或修改断言伪造通过。

```bash
pnpm typecheck
pnpm format:check
pnpm test:unit --no-file-parallelism --maxWorkers=1 --minWorkers=1
pnpm build:renderer

cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --jobs 1 --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Windows Git Bash 隔离后端测试：

```bash
export CC_SWITCH_TEST_HOME="$(cygpath -m "$(mktemp -d)")"
cargo test --locked --jobs 1 --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

macOS/Linux 使用对应的原生临时目录路径，无需 `cygpath`。只设置 `CC_SWITCH_TEST_HOME`，不要改 `HOME` 导致 Cargo/rustup 缓存失效。

本机 Rust 已安装但旧进程可能没有最新 PATH：优先新开终端或读取 `.cc-switch/contribution-prep/dev-env.sh`，不要再安装一份。第一次后端构建前须有 `dist/`，可先运行前端构建。

需要验证单个原有 Rust 失败时，使用 `cargo test --lib <完整测试名> -- --exact --test-threads=1`；它与功能测试分开记录。不要用全局管理员权限运行 Agent 来规避测试权限。

## 7. 阶段交付与停止条件

每个阶段结束更新执行状态：改了哪些文件、哪些测试实际运行、哪些失败是基线/新回归、下一步是什么。不要把写了代码等同于实现已验证。

遇到以下情况暂停相关步骤并向用户说明，不自行扩大范围：

- 需要修改 Windows 系统设置、提权、实际服务器写入或发布外部内容。
- 同名 Skill/非托管文件冲突或用户已有改动。
- 现有同步格式无法安全排除本机引用，需要超出本需求的架构改造。
- 新增失败无法与已记录基线区分。

不得宣称准备期已完成真实 SSH、桌面交互、macOS 或所有客户端的验证。测试基线未全绿的问题必须在最终结果中保留。

## 8. 新对话启动提示词

```text
请在当前仓库 feat/vps-skill-management 分支继续实现 Issue #7739。
先读 docs/plans/vps-skill-management-execution.md、docs/proposals/vps-skill-management.md 和 CONTRIBUTING.md，并查看 .cc-switch/contribution-prep/preflight.md 的本机基线。
按执行计划分阶段实现，不重新设计已确认方向：VPS 与 Skills/提示词/会话/MCP 同栏；在 VPS 页选客户端自动托管一个通用 Skill；自动 Skill 在普通 Skill 页隐藏；不设第二开关或常驻 Skill 状态说明；复用现有 Skill 管理和系统 SSH，不建 MCP/Agent/自定义客户端框架。
先检查工作区并保留现有改动。记录并区分原有的 4 个 Rust 基线失败，新增与受影响测试必须实际验证。允许对完成且验证的独立阶段创建本地 commit，提交前检查差异并记录测试结果；未经进一步明确授权，不 push、不发布评论或创建 PR，不重写已有历史、不覆盖用户改动、不修改系统权限或连接/写入真实 VPS。
先核对第 10 节的阶段 B 结果及第 9 节的隔离修复/剩余权限失败，不要把准备期基线当作当前测试结果。
```

## 9. 实际执行状态（2026-09-29）

**阶段 A：来源与管理权基础已实现，新增用例通过；受影响集成测试仍有 1 项权限失败，整体未全绿。阶段 B–G 尚未开始。** 未创建 VPS 主机数据、内部注册/投影或页面入口。

### 已改代码

- `app_config.rs`、`database/{mod,schema,dao/skills}.rs`：`InstalledSkill.managed_by` / `managedBy`，schema v19→v20，旧记录默认为普通来源；DAO 不过滤内部记录，元数据更新不改管理权。
- `services/skill.rs`、`commands/skill.rs`：普通列表隐藏托管项；启停、卸载、仓库/ZIP 安装、应用目录导入、Skill 备份恢复在写文件前拒绝托管来源或同 ID/目录冲突。导入批次预检；异步下载后重新加锁检查。远端仓库更新排除托管项，扫描保留对完整记录的识别。
- `services/profile.rs`：快照/应用项目排除托管项，避免形成第二控制源。
- `src/lib/api/skills.ts`（实际 TS 类型落点）、`useSkills.ts`、`UnifiedSkillsPanel.tsx`、错误解析器及四语言文案：普通数据消费/缓存过滤，冲突提示指向 VPS 页面，不新增开关或状态栏。
- 新增来源、恢复、冲突、防伪造、持久化和前端测试；旧测试构造补默认来源。没有更改已有断言或 skip。

内部 VPS 注册仍须在阶段 C 接入，当前没有生产入口创建托管记录。旧 `migrate_skills_to_ssot` 会清表重建、数据库导入/云同步与本机内容隔离仍须按阶段 C/D 适配；不能提前安装真实托管实例并宣称生命周期已经安全。

### 已实际执行

| 检查 | 当前结果 |
| --- | --- |
| 新 Rust 来源/管理权测试 | 实现前 1 通过 / 9 失败；实现后 10 全通过 |
| 测试目录隔离回归测试 | 实现前失败，修复后通过；只使用两个临时目录 |
| 前端 Skill 测试 | 最终 5 文件、66 通过，含 7 项新增 |
| Profile 集成测试 | 最终 8 通过，含 1 项新增 |
| Skill 集成测试 | 最终 6 通过、1 失败（创建 symlink 时 Windows 1314） |
| Rust 全库 | 隔离修复后：2903 通过、2 失败、10 原有忽略 |
| TypeScript / 前端格式 / 前端构建 | 最终复核通过 |
| Rust 格式 / 单 job Clippy `-D warnings` | 最终复核通过 |

隔离修复前首次全库为 2896 通过、8 失败、10 忽略：原先第 0 节的 4 项失败均在，另出现 3 项 `services::model_pricing::tests` 失败（`batch_update_and_delete_are_persisted_to_local_file`、`creates_local_file_with_auto_sync_disabled_by_default`、`empty_override_file_does_not_roll_back_builtin_pricing_repairs`），以及 `services::skill::tests::mcode_import_checks_native_copies_and_deploys_selected_skills` 失败。暂停后 MCode 单项复跑通过；定价组复跑 3 通过、4 失败。保留这些中间证据，没有把新增差异混入原 4 项基线。

经用户确认修复测试目录回退后，重新完整运行库测试：以上 3 项定价、MCode 导入、原基线定价重载和 Pi 存储迁移均通过，未修改它们的断言或业务逻辑。**仅剩原基线的 Codex/Grok Build 两项 symlink 权限失败（1314）**。

新增执行的集成失败为 `skill_sync::sync_to_app_removes_disabled_and_orphaned_ssot_symlinks`：在测试 helper 创建链接时报 Windows 1314，未进入被测同步逻辑；不能计入原先的 4 项库测试基线。

### 暂停、获准修复与安全边界

发现既有 Windows 测试隔离缺口：`config.rs::get_app_config_dir()` 在 `CC_SWITCH_TEST_HOME` 临时目录缺少数据库时，仍可能回退到 `HOME/.cc-switch`。本机只读检查确认回退数据库存在。新测试显式创建临时数据库哨兵并断言路径；但原定价/部分 Skill 测试没有，因此前述全库测试无法保证完全隔离，也不能声称没有访问真实配置。尚未审计具体影响，不自动清理或回滚用户目录。

按第 7 节停止条件暂停并询问后，用户明确选择“修复隔离再验证”。已在 `config.rs::get_app_config_dir()` 中为有效 `CC_SWITCH_TEST_HOME` 提前返回测试配置目录，阻止旧 HOME 回退；未设置该变量时的生产行为不变。新增测试用两个临时目录复现失败，再验证修复，并在 panic 时恢复测试进程内的环境变量。没有修改系统环境或全局权限。

当前新增测试均通过，剩余两项库测试和一项既有集成测试都受 Windows 1314 限制，需具备符号链接权限的环境验证；没有新增 skip、放宽断言或改系统设置。下一步按阶段 B 实现主机数据、目录生成与聚合规则；阶段 C 注册托管实例前必须完成其受控生命周期路径。

**未 commit、push、发布评论/PR、修改系统权限、启动桌面应用或连接 VPS。** 修复前可能访问真实配置的历史风险仍保留，不能以之后通过的测试倒推之前绝无影响；未自动清理或回滚用户目录。

详细输出保存在被忽略的 `.cc-switch/contribution-prep/vps-stage-a-*.log`；各日志及复核统计见该目录 `preflight.md` 的最新执行记录。本次未重跑完整前端套件，也未运行其余 Rust 集成目标；真实 SSH、macOS、桌面和客户端端到端验证仍未执行。

### Git 授权更新

用户在继续阶段 B 时明确允许：在当前功能分支上，对完成且经过验证的独立阶段创建本地 commit，提交前检查差异并记录测试结果。因此可单独提交已验证的阶段 A，阶段 B 在完成验证后再提交。未经进一步明确授权，不 push、不创建 PR、不发布评论，不重写已有提交历史或覆盖用户改动。前文“未 commit”等描述为相应检查时点的历史记录，不覆盖这项新授权。

## 10. 阶段 B：主机数据与目录生成（2026-09-29）

阶段 A 已独立提交为 `99548764`；此提交不含阶段 B 代码。阶段 B 的改动范围仅为 `services/vps.rs`、`services/vps/tests.rs`、模块注册/库导出和执行文档，不改供应商模型、Skill 部署或前端导航。

### 实现与后续接口契约

- `VpsServer` 保存稳定 UUID、名称、用途、主机、端口、用户名、密钥路径引用和现有 `SkillApps`。别名从 UUID 派生，重命名不改变身份。构造一次记录，保存失败时复用该 ID 重试，避免重试创建新主机。
- 校验控制字符、SSH 选项/换行注入、DNS/IP、端口及密钥路径。密钥引用须为绝对路径，不接受父目录穿越、SSH `%`/环境变量展开或引号注入；不读取私钥内容。未指定密钥时生成 agent 模式的 `IdentityFile none`。
- `VpsService` 使用现有配置目录与原子私密文件写入，读写 `<data-dir>/vps/servers.json`。独立 `ssh_config` 包含严格主机密钥验证及非交互认证约束；不会覆盖 `~/.ssh/config`，也不会启动 SSH。
- `clients/<app-id>.json` 仅包含分配给该客户端的主机 ID、名称、用途、稳定 SSH 别名和独立配置路径，不包含密钥路径或凭据。遍历既有 `AppType` / `SkillApps` 能力，不维护另一份客户端名单。
- 纯函数 `required_apps`、`plan_client_changes` 计算引用聚合及 Install/Update/Remove 差异；两台主机共用客户端时删除其中一台只更新，最后解绑才清理客户端目录。主机仍保留时，即使没有客户端绑定也保留 SSH 主机条目。
- `servers.json.generatedFiles` 是生成内容的归属/恢复哈希元数据，不是另一个绑定来源。先检查目标归属，再原子保存主数据及前后哈希，逐文件写入/清理，最后收敛哈希。中断后可重新打开并 `reconcile()`；错误明确区分“主数据已保存”和“文件生成尚未完成”。
- 相同内容不重复写入；进程内锁串行协调不同服务实例；写入前比对快照，拒绝覆盖用户已改动的文件。拒绝非普通文件、符号链接及 Windows reparse/junction 路径，仅清理有哈希归属证据的生成文件，保留目录内其他文件。

**阶段 C 必须遵守：** `load()` 返回主数据中的期望绑定，不保证目录或 Skill 已成功部署；`plan_client_changes` 只是期望态差异。重试/启动恢复时须先完成目录生成，并对照实际 Skill 登记/部署收敛，即使期望态没变化也不能跳过首次失败的部署。不能把阶段 B 的保存成功单独当作客户端接入成功。当前没有 VPS Tauri 命令、VPS 页面、托管 Skill 注册或真实 SSH 操作。

### 验证与交付

阶段 B 的基础实现及本机可运行测试完成，按授权独立本地提交；不包含阶段 C–G。

| 检查 | 实际结果 |
| --- | --- |
| 首批测试红绿验证 | 实现前 3 通过、16 失败；实现后 19 全通过 |
| 扩展后的 VPS 测试 | 26 全通过，包含临时目录、恢复/幂等、并发保存、Windows junction 拒绝测试 |
| Rust 全库 | 2929 通过、2 失败、10 原有忽略 |
| Profile 集成 | 8 通过 |
| Skill 集成 | 6 通过、1 失败 |
| Rust 格式 / Clippy `-D warnings` | 通过，单 job |
| TypeScript / 前端格式 | 通过；本阶段没有修改前端代码 |

库测试仅剩原基线的 `codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir` 与 `services::session_usage_grokbuild::tests::symlink_cycle_does_not_cause_stack_overflow`，均为 Windows 1314。Skill 集成仍为 `sync_to_app_removes_disabled_and_orphaned_ssot_symlinks` 在创建链接的测试 helper 处报 1314。与阶段 A 结束时相同，没有新增失败或忽略项；原基线的定价重载与 Pi 迁移继续通过。

后端命令设置隔离 `CC_SWITCH_TEST_HOME`、单编译 job、单测试线程。VPS 用例另外逐项使用私有临时根目录；故障注入只在测试编译下启用。Windows junction 用例只在临时目录创建/移除链接，不修改系统权限；Unix symlink 用例尚未在本机执行。未运行真实 SSH、macOS、桌面或客户端模型任务，未验证真实 OpenSSH 对生成配置的加载；这些仍属于后续验收。没有重新运行前端完整测试/构建或其他 Rust 集成目标。

日志位于 `.cc-switch/contribution-prep/`：`vps-stage-b-red.log`、`vps-stage-b-green.log`、`vps-stage-b-focused.log`、`vps-stage-b-clippy.log`、`vps-stage-b-rust-lib.log`、`vps-stage-b-integration.log`。

下一步是阶段 C：通用托管 Skill 模板、按应用生成部署及受控生命周期。在接通生产注册/部署前，继续落实阶段 A 记录的迁移与所有权边界，随后按阶段 D 处理扫描、备份和恢复。仍不 push、创建 PR、发布评论、重写提交历史或操作真实 VPS。

## 11. 阶段 C：受控 Skill 部署（2026-09-30）

用户已同意并行实施：主线程负责 C/D，隔离 worktree 的子 agent 分别负责 E/F；重型验证串行安排。C 的本地提交不包含 E/F 代码，也暂不注册 VPS IPC，以便先完成 D 的恢复/同步边界后再接通页面。

### 已实现

- 一份通用 `services/skill/vps-template.md`，内部身份 `internal:vps`、目录 `cc-switch-vps`、来源 `managedBy=vps`，无伪造仓库地址。
- 各客户端生成独立 `vps/skill-projections/<app>/SKILL.md`，引用对应本机目录；共享模板不含应用路径。生成源在本机 VPS 目录，不把本机引用放进普通 Skill SSOT。
- 从既有 `sync_to_app_dir` 提取共用物化步骤，生成 Skill 复用原有 Auto/Symlink/Copy 和完整目录哈希，不另造安装器。Pi 不新增持久化启用标志，继续由实际文件存在体现启用。
- `vps/skill-state.json` 仅记录应用部署路径与已知内容哈希，是中断恢复/归属凭据，不是另一个用户绑定源。首次拒绝接管同名目录；源或部署被手改时拒绝覆盖/删除；应用目录重叠在写主数据前报错。
- 目录写入与部署完成后才返回成功。失败保留可诊断、可重试的状态；最后解绑清理对应部署，其他主机仍引用时保留。稳定 ID 及单一 Skill 登记在重试中不重复创建。
- 主机数据与部署使用 `Skill state -> VPS document -> DB` 的锁顺序；全量 Skill 重同步与存储迁移调用同一投影规则，普通 Skill 仍独立同步，即使某个 VPS 部署发生冲突也不破坏普通文件。

### 实际验证与隔离事件

- 首批流程测试：实现前 1 通过 / 5 失败，实现后 6 通过。
- 扩展复核曾出现 40 通过 / 1 失败：新测试 fixture 只隔离 home，却没有隔离 Hermes 的 Windows `%LOCALAPPDATA%` 默认路径；首次成功测试曾在那里生成一个引用临时目录的测试 Skill。此项是新测试隔离遗漏，不是原有 Rust 基线失败。
- 发现后立即暂停后端验证，只读核查；用户明确授权精确清理。再次核验文件内容、类型和目录清单后，仅移除该生成的 `SKILL.md` 与空 `cc-switch-vps` 目录，保留 Hermes 其他文件及父目录。未连接 VPS、未读取私钥、未修改系统权限。
- 修正新 fixture：显式设置临时 `HERMES_HOME`、`MINIMAX_DATA_DIR`、`MAVIS_DATA_DIR` 与 Pi 测试路径，写入前断言所有客户端根目录均在私有临时 home 内；不修改 Hermes 生产路径规则。后端命令也隔离本进程的应用数据目录变量，不改系统环境或 HOME/Cargo 缓存。
- 最终定向验证：**VPS 41 项通过（含阶段 B 的 26 项与 C 的 15 项）；既有 Skill 服务 80 通过、1 项原有忽略；Clippy `-D warnings` 通过。** 没有修改原有断言/超时或新增 skip。Windows 原生 symlink 受权限限制，Unix symlink 用例未在本机执行；Windows 目标路径规范化由纯测试验证。

日志：`vps-stage-c-red.log`、`vps-stage-c-green.log`、`vps-stage-c-focused.log`（保留隔离失败证据）、`vps-stage-c-isolated.log`、`vps-stage-c-skill-regression.log`、`vps-stage-c-clippy.log`。本阶段尚未重新运行 Rust 全库/集成或真实客户端；原两项库测试及一项集成的 1314 限制仍按前述记录跟踪。

D 必须继续完成：SQL 导出排除托管行、导入/恢复保留本机管理权且不接受远端自报来源；扫描/迁移/ZIP 排除生成副本；恢复后按本机主数据重建。完成这些边界前，不把已交付的前端接到生产 VPS 命令。

## 12. 阶段 D：扫描、导出与恢复隔离（2026-09-30）

C 已提交为 `5a63c500`。D 补齐本机生成来源边界：

- SQL 导出在查询层排除 `managed_by` 非空的记录，不修改源数据库；普通用户 Skill 照常导出。
- SQL 导入和原始数据库备份恢复先丢弃外部自报的托管行，再保留当前本机托管记录。外部普通记录与本机托管 ID/目录冲突时中止，不接管；导入触发器如果改写本机管理权，也会在替换主库前被检查阻断。
- 本机原始数据库安全备份仍保留完整镜像及来源标记；恢复时不把镜像中的应用状态当作另一设备有效的 VPS 绑定，部署仍从本机主数据/归属凭据重建。
- 自动扫描、应用目录导入、ZIP 安装、Skill 备份恢复、远端更新和旧 SSOT 迁移拒绝把生成模板当作普通 Skill。模板标识只用于拒绝生成产物的手动导入，不会据此授予托管身份。旧迁移仅清空普通行，保留本机托管行。
- Skill ZIP 排除本机 VPS 数据根、已登记部署路径及被复制/改名的生成模板目录。恢复在替换 SSOT 前拒绝生成产物、托管目录冲突及本机路径重叠；数据库导入冲突时沿用既有快照回滚，保留普通用户 Skill 文件。
- 启动和导入后执行同一受控重建入口，不根据远端备份猜测主机绑定。

实际验证：首批新增 8 项先全部失败，实现后通过；补充完整快照往返、另一临时设备恢复、文件回滚和触发器保护后，**VPS 相关 52 项全部通过（含 D 新增 11 项）**。备份模块首次回归 38 通过/2 原有忽略，新增触发器用例另在 VPS 定向集中通过；归档回归 2 通过；Skill 服务 80 通过/1 原有忽略；同步协议 23 通过；Clippy `-D warnings` 通过。未增加 skip、未修改原有断言或超时。

日志：`vps-stage-d-red.log`、`vps-stage-d-green.log`、`vps-stage-d-final.log`、`vps-stage-d-backup-regression.log`、`vps-stage-d-archive-regression.log`、`vps-stage-d-skill-regression.log`、`vps-stage-d-sync-regression.log`、`vps-stage-d-clippy-final.log`。测试均使用显式隔离的本机路径，未访问真实 VPS；完整库/集成复验留到 E/F 接线后统一执行，原有 1314 限制不能据此标为已解决。

## 13. 阶段 E：系统 SSH 测试（2026-09-30）

D 已提交为 `f1ab3a92`。E 由独立 worktree 子 agent 编写，主线程审阅并整合；未进行真实 SSH 连接或操作 VPS。首次整合发现 Tauri 异步 State 命令必须返回 `Result`，已按既有命令模式修正，前端接收的 JSON 契约不变。

- 使用系统 `ssh` / `ssh-keyscan` 参数数组；探测固定执行 `true`，只接受密钥路径或 agent，不读取私钥内容、不传递密码，不启动用户配置中的代理命令、转发或复用连接。
- 首次扫描只取得未验证公钥，必须由用户独立核对指纹后显式确认。服务端确认 token 绑定目标与公钥，五分钟有效、一次性使用；取消会撤销待确认 token，指纹变化不自动覆盖。
- 专用 `vps/ssh-trust.json` / `vps/known_hosts` 保存受控本机信任，并检查归属与中断写入恢复。共享配置和临时探测配置均使用稳定 `HostKeyAlias`；非默认端口不改变 known_hosts 中的 alias 键。
- 单次测试总期限 20 秒，支持取消、进程回收和有界输出。原生 runner 在 blocking worker 中运行，不阻塞 UI；区分缺少工具、认证失败、密钥变化、超时和取消。成功只表示本次固定探测成功。
- 注册三个 SSH 命令和 `VpsSshState`；主机 CRUD 与前端接线将在 F 中完成，不增加远端服务或后台 Agent。

验证：**VPS 相关 72 项全部通过**（此前 52 + Windows 可运行的 SSH 19 + 共享信任配置契约 1）。其中包括三个 Windows 本地 `cmd.exe` 替身进程的退出状态、超时与取消/回收测试；其余网络流程使用注入替身，没有连接公网。Clippy `-D warnings` 通过。Unix symlink 分支与真实 OpenSSH/客户端端到端尚未验证。

日志：`vps-stage-e-tests.log`（保留首次编译错误）、`vps-stage-e-recheck.log`、`vps-stage-e-clippy.log`。

另以本机 OpenSSH 9.9p2 执行了 `-G -F <临时配置>` 的离线解析 smoke check：人工构造的等价配置覆盖空格/Unicode 配置及密钥引用路径、端口 2222、稳定 alias 和专用信任文件，验证通过；没有建立连接或读取真实私钥。该检查不等于真实 VPS/客户端端到端验收。

## 14. 阶段 F 接线与 G 自动化复验（2026-09-30，会话恢复后）

从会话 `f29127ac-aa35-4fc6-9ebb-5b41e74c9c7d` 的本地记录恢复；开始时 HEAD 为 E 的 `59a83da1`，F 的前端/IPC 及尚未接入的 SSH shutdown 留在工作区。保留原有改动，本轮未新增 commit、push、评论或 PR。

### F 的实现与收尾

- `App.tsx` 将 VPS 放在原功能按钮组末尾，跨 Hermes/OpenClaw 等分支共用入口；补齐视图恢复、标题、返回、新增、滚动以及 busy/对话框导航保护。全局页不以当前供应商应用自动授予客户端绑定。
- `VpsPanel` / `VpsServerForm`、`useVps`、`lib/api/vps.ts` 与三个主机 IPC 接通。列表、增删改、SSH 测试、显式指纹确认、取消和既有 Skill 客户端选择形成同一流程；保存等待目录及托管部署收敛，不把单独保存主数据当作成功。
- 新增失败保留 UUID 与草稿，绑定失败不乐观勾选；未保存草稿有丢弃确认。没有新增第二个 Skill 开关或常驻 Skill 状态行。
- SSH shutdown 接入普通退出清理、Tauri 重启和直接进程重启；取消本进程活动探测、撤销待确认 token、阻止排队探测在退出时启动，并有界等待回收。同步重启路径不调用窗口 API，不清理外部客户端 SSH 会话。新增两个状态级测试均通过；没有执行真实桌面退出/重启验收。
- 只读核对发现并用六个失败用例复现：SSH 固定英文 `message` 绕过四语言，以及确认失败后复用失效 token。已修正为本地化状态提示；确认失败时撤销旧请求、保留草稿、提示用户显式重新测试，不自动发起连接。过期与临时信任写入失败均重新进入测试/核验流程，无新增后台恢复器或 IPC 类型。

### 实际测试结果

| 检查 | 本轮结果 |
| --- | --- |
| 首次完整前端 | 151 文件、1694 通过 |
| 新增文案/确认恢复回归 | 实现前 6 失败；修复后 VPS 组件/API/hook/语言定向 4 文件、29 通过 |
| 最终完整前端 | **151 文件、1700 通过**；低并发，未提高超时或更新快照 |
| VPS Rust 定向 | **74 通过**（之前 72 + shutdown 2） |
| Rust 全库 | **2977 通过、2 失败、10 原有忽略** |
| 全部 Rust 集成目标 | **174 通过、1 失败**；15 个目标均执行（含 0 测试的 support），使用 `--test '*' --no-fail-fast` |
| TypeScript / 前端格式 / renderer 构建 | 最终复核通过；保留既有包体/浏览器数据时效警告 |
| Rust 格式 / 单 job Clippy `-D warnings` | 最终复核通过 |

剩余三项均为已记录的 Windows `1314`：库测试 `codex_config::tests::resolve_catalog_rejects_symlink_escaping_config_dir`、`services::session_usage_grokbuild::tests::symlink_cycle_does_not_cause_stack_overflow`，及集成 `skill_sync::sync_to_app_removes_disabled_and_orphaned_ssot_symlinks`。没有增加 ignore、放宽断言或更改系统权限。首次执行的其余集成目标全部通过；不能因此把整体标为全绿。

日志位于 `.cc-switch/contribution-prep/`：`vps-stage-f-resumed-{renderer,frontend-full,rust-focused,clippy,rust-lib}.log`、`vps-stage-f-resumed-ui-{red,green}.log`、`vps-stage-f-final-{frontend,renderer,clippy}.log`、`vps-stage-g-rust-integrations.log`、`vps-stage-g-rust-lib-final.log`。

### 测试隔离范围与后续命令

本轮后端命令使用临时 `CC_SWITCH_TEST_HOME` 及进程级 Hermes/MiniMax/Mavis/AppData 覆盖，不改变启动 Cargo 的 shell 的 HOME。集成 support 自身固定使用 `temp_dir()/cc-switch-test-home`，会删除重建该目录并在测试子进程内改 HOME；因此此次全部集成目标另外使用新建的私有 `TEMP/TMP/TMPDIR`，并将 MiniMax/Mavis 覆盖对齐该 fixture 的 `.minimax`。清除 `CC_SWITCH_UPDATE_GOLDEN`，不更新 golden 源文件。

只读核查还发现：既有工具路径搜索单测会枚举真实 home/Known Folder 下的工具安装目录，环境覆盖并不能阻止这些目录元数据访问。Codex 模型目录在缺缓存时存在调用本机 CLI 的路径，未证明先前库测试实际进入该分支或读取了真实配置；后续命令补临时 `CODEX_HOME` 并清除 `CODEX_SQLITE_HOME`，同时隔离 XDG 路径。不以隔离变量或测试通过声称“完全没有访问任何真实 home 路径”；没有读取/清理真实用户配置来做事后核查。

### 尚未执行及下一步

1. Windows Tauri 真实窗口、退出/重启、主机页与 Skill 部署联动：需隔离数据与客户端目录后进行，未启动开发版接管日常配置。
2. 真实 VPS/VM 的 SSH、指纹确认/变化、取消与凭据访问：未连接，需要测试资源及明确授权；不要在对话中传私钥内容。
3. 声明支持的客户端发现/重载/执行行为与 macOS：未做端到端测试；客户端能力列表只表示配置目标，不代表实测兼容。
4. 在具备符号链接权限的环境复验三个失败；不擅自开启 Developer Mode 或提权。
5. 外部发布仍需单独授权；阶段 F 当前保持未提交，不能凭自动化通过宣称 PR 已准备就绪。

## 15. 桌面反馈修复与本机认证扩展（2026-09-30 至 2026-10-01）

用户已确认日常版恢复正常；数据库恢复与授权详情见本机 `vps-daily-db-recovery-20260930.md`。本轮只修改代码、使用前端模拟数据和隔离测试，不启动 Tauri，不读取日常数据库或真实凭据库。

### 新确认的范围

用户要求客户端选择与 MCP/供应商页一致，补密码及 SSH 用户证书，修复添加主机页顶部遮挡和返回失效；随后明确“密码保存在本地就行，SSH agent 可以不做”。采用本机系统凭据库，不使用普通 JSON 明文保存，不新增 SSH agent 独立选项；旧记录缺省认证保持兼容。原方案中“不保存密码”的范围已由本轮要求更新，见 proposal 第 8 节。

### 已完成的前端修复

- 原表单使用 z-40 的居中 Dialog，而主页面页头为 z-50，主页面返回又因 editor 状态被禁用。改为 MCP 使用的 `FullScreenPanel`，独立页头返回、可滚动内容及固定底部操作；未保存草稿仍须确认放弃。
- 表单客户端改为带名称的复选框，仍遍历 `SKILLS_APP_IDS`；列表保留与 MCP 相同的 `AppToggleGroup`。指纹/丢弃确认框高于全屏表单，帮助框高于主页面页头。
- 认证表单提供密码、私钥文件、SSH 用户证书；证书需要对应私钥。密码单独保存在编辑器临时状态，通过独立 IPC 参数传递，不混入主机元数据；留空保留已有密码，切换认证方式清空草稿密码。保存后清理 mutation 状态，不进入主机查询缓存。
- 新增凭据不可用、缺少密码与目标变化的四语言提示；界面明确本机密码库用于 CC Switch 测试，客户端 SSH 仍需用户终端交互，不把密码交给模型。

### 当前验证证据

- UI 返回/布局/客户端选择：先 3 项失败，修复后相关 24 项通过。
- 认证前端：先 4 项失败；修复后有 1 项测试环境缺失 `scrollIntoView`，按既有测试模式补可恢复 polyfill，不修改组件业务逻辑或放宽断言；随后 5 文件、37 项全部通过。
- 完整前端：151 文件、1707 项通过。之后补充目标变化提示与系统凭据漫游文案，最终 TypeScript、格式、4 文件/36 项定向测试及 renderer 构建再次通过。
- 浏览器纯前端模拟页（无 Tauri/SSH）：实际验证空白表单返回、脏表单取消/确认放弃、客户端勾选；1000×650 浅色和 900×600 日语深色下无横向溢出，顶部无遮挡、底部按钮可见。截图在本机 `vps-form-fixed-light.png` / `vps-form-fixed-dark-ja.png`。浏览器连接 Vite HMR WebSocket 超时，HTTP 加载及上述交互正常；不把模拟页当作原生或真实 SSH 验收。临时 Vite 子进程已按 PID/命令/创建时间精确回收，没有结束用户进程。

### 已整合的认证后端

- `VpsServer` 可选 `authMethod` / `certificateFile`，旧记录缺省保持原行为。新密码通过 `save_vps_server` / `test_vps_connection` 的独立参数传递，不加入主机 JSON。证书与私钥仅做路径/注入校验，实际配对由 OpenSSH 判断。
- 系统凭据库按规范化 VPS 数据目录、主机 UUID 和不可变 revision UUID 区分。`servers.json` 只保存活动 revision 指针及待清理回执。新密码先写独立凭据项，再原子切换指针；主数据提交前失败不覆盖旧密码，生成/部署失败后保留已提交的新密码，清理失败可重试且拒绝删除活动 revision。没有再升级 SQLite schema。
- 固定探测通过一次性 loopback/ASKPASS 通道向 OpenSSH 提供密码。辅助进程在 `main()` 最早分流，不初始化应用、数据库或日志；密码不经命令参数、环境变量或临时文件传递。首次确认前不读取密码；host/port 与已确认目标不同时返回 `targetChanged`，不沿用密码或静默换 pin。
- 异步取消/超时可结束 UI 请求；不可强杀的 OS 凭据读取仍占原活动名额，迟到返回后不能启动 SSH。主线程额外复现了“取消后 OS 读取仍持有全局 VPS 锁”问题（两个测试红灯），利用不可变 revision 在读取 OS 凭据前释放状态锁，修复后通过。退出/重启的 shutdown 字段、方法及生产入口 gate 已与新实现三方合并保留。
- 引入 `keyring = 3.6.3` 与 `zeroize`，锁文件仅增加相关依赖，不升级既有包版本。Windows keyring 使用 `CRED_PERSIST_ENTERPRISE`，因此只能保证 CC Switch 不写明文、不自行同步；OS 漫游由系统策略控制，已在四语言文案和方案中说明。

最终后端验证：**VPS 96 项通过（新增 22 项）；既有 Skill 服务 80 通过、1 项原有忽略；完整目标编译检查、Rust 格式、Clippy `-D warnings` 通过。** 新凭据测试使用注入的内存 store，原生凭据入口在单元测试构建中明确拒绝访问。所有后端测试使用新临时 home 和客户端路径覆盖。本轮未重跑完整 Rust 库和全部集成目标，前三项 Windows 1314 基线未解决，不沿用旧统计冒充本轮全量结果。

### 验收边界及日志

- 代码已可重新进行隔离桌面验收，但必须完全退出旧开发进程，再使用本机 `vps-desktop-acceptance.sh` 重新构建启动；不要只热更新前端或直接运行 `pnpm dev` 访问日常库。
- 未执行真实系统凭据库读写、真实密码 SSH、证书登录或新版原生辅助进程联动。实际 OS/SSH 和 macOS/Linux 行为不能由 mock 测试替代；加密私钥的口令交互、多因素登录没有实现。
- 本机密码库仅用于 CC Switch 的固定连接测试，独立客户端仍通过系统 SSH 交互输入密码；模板已禁止模型读取已存密码或要求用户在聊天中发送密码。
- 本轮没有新增 commit、push、PR、修改系统权限、再次打开日常数据库，或访问真实凭据库。

本机日志：`vps-ui-feedback-{red,green}.log`、`vps-auth-ui-{red,green,green-recheck}.log`、`vps-feedback-frontend-{full,final-focused}.log`、`vps-feedback-renderer-final.log`、`vps-auth-integrated-check.log`、`vps-auth-rust-focused.log`、`vps-auth-vault-lock-red.log`、`vps-feedback-rust-final.log`、`vps-feedback-skill-regression.log`、`vps-feedback-clippy-final.log`。

## 16. 会话恢复：帮助退出与连接表单分组（2026-10-01）

从会话 `cc7a4323-18d3-4a00-af3d-ea06c49d9fcc` 恢复。该会话第 15 节之后又收到三项反馈：其他页面/Skills 图标是否被修改、VPS 客户端接入说明无法退出、连接表单参考 Termius。上轮仅完成定位和参考资料读取，尚未实现这次反馈，不能将第 15 节当作全部收尾。

### 本轮修改范围

- 核实 `846de29c`（上游基线 `Fix/skills new icon (#7727)`）将两个 Skills 导航图标从 Wrench 改为自定义 SkillsIcon。本轮不回退该图标，不修改其他页面或共享组件；此前 VPS 功能已有的 App/后端/Skill 接线仍保留。
- `VpsPanel` 帮助框缺少显式关闭控件，而共享 Dialog 默认禁止点击遮罩关闭，背景导航又被 overlay 状态禁用。新增底部“关闭”按钮、关闭后的触发按钮焦点恢复和可滚动正文；保留 Esc 行为，不改变其他弹窗的全局规则。
- `VpsServerForm` 参考 [Termius 连接文档](https://docs.termius.com/organize-and-connect-to-hosts/connecting-to-a-server) 的主机/凭据分组，整理为主机信息、SSH 认证、客户端三个具名 fieldset。名称/用途、用户名/认证方式在宽窗口并排，地址/端口相邻，按认证方式显示相关字段。保持既有认证、保存、草稿保护、客户端能力集合与密码隔离契约；不新增终端、密钥导入或认证后端。
- 四语言仅更新 VPS 命名空间的分组标题和客户端说明。新增三项组件回归，未放宽旧断言或改共享测试环境。

### 验证结果与边界

- 新回归先得到 **3 项失败**：缺少关闭按钮、Esc 关闭后焦点未恢复、表单缺少分组；修复后 VPS 组件/API/hook/语言 **4 文件、39 项通过**。
- 完整前端 **151 文件、1710 项通过**。随后根据截图去除分组标题重复间距，最终 TypeScript、前端格式、39 项定向测试和 renderer 构建再次通过。
- Playwright 使用真实 VPS 组件/CSS 与既有内存 API 替身，拦截入口，验证未加载生产 `App`：四语言帮助按钮/Esc/焦点/导航恢复、空表单返回、脏草稿取消放弃、客户端勾选与模拟保存、900×600 日语深色证书表单滚动和底部操作均通过。无页面脚本错误或横向溢出；原生桌面和 SSH 不在本次验收范围。
- 首次浏览器失败来自被忽略预览壳的 `pt-20` 不在 Tailwind 扫描范围，固定页头挡住了帮助入口；仅将本机预览壳改为显式 padding，未改产品布局。快速重新打开后立即发送 Esc 的自动化曾出现时序失败；最终脚本等待现有弹窗入场动画结束后再发键，没有增加固定 sleep 或修改共享 Dialog。
- 临时 Vite 以直接子进程启动并按自有进程句柄回收，结束后确认 3000 端口空闲。未启动 Tauri、访问日常数据库/系统凭据库、连接 VPS、修改系统权限或新增 commit/push/PR。本轮没有改 Rust，也未重跑 Rust；既有三项 Windows 1314 及真实认证/客户端/macOS 验收仍待处理。

本机日志：`vps-help-form-resumed-{red,green,frontend-full}.log`、`vps-help-form-final-{focused,build}.log`、`vps-help-form-browser-verified.log`；中间失败保留在 `vps-help-form-browser{,-recheck,-final}.log`。截图：`vps-help-fixed-{zh,ja}.png`、`vps-form-grouped-light.png`、`vps-form-grouped-dark-ja.png`、`vps-form-grouped-clients-dark-ja.png`。浏览器脚本 `vps-help-form-browser.py` 与预览入口均留在被忽略的 `.cc-switch/contribution-prep/`，不加入应用包。

## 17. 用户实测反馈与下一步（2026-10-01）

用户反馈：“页面没问题了，我也使用了真机服务器，ssh测试同样没问题”。据此记录 **页面交互验收通过、用户执行的真实服务器 SSH 测试通过**。这是用户报告，不是本轮 Agent 重新执行；未提供具体认证方式、凭据保存后重启、主机指纹变化等测试细节，不泛化为全部认证方式或平台已验证，也不记录服务器地址或凭据。

下一步优先验证一个常用客户端的完整接入：VPS 选择客户端后自动部署一次、普通 Skill 页保持隐藏、客户端新会话读取正确目录、经用户授权对明确目标执行最小只读 SSH 命令。随后验证修改目录、共享主机引用、最后解绑清理及重启恢复。客户端必须读取与隔离 CC Switch 相同的测试目录；当前桌面验收脚本将各客户端目录指向私有 acceptance home，日常启动的客户端不会自动采用这些覆盖。不要为方便验证而改回日常目录。

本机保存的密码仍只供 CC Switch 连接测试，独立客户端需终端交互输入；不向模型提供密码。用户随后询问过通过本机启动入口复用已保存密码的可行性，并明确选择 **暂时搁置客户端密码自动复用**；不要将这一讨论当作新增实现授权。付费模型调用、真实服务器执行、系统权限变更及外部发布仍需各自明确授权。客户端端到端通过后，再做代码审阅、最终自动化复验及提交材料；既有三项 Windows 1314、未测认证方式及 macOS 等仍须分别完成或如实列为未测。本次仅更新执行记录，未运行客户端、连接服务器或创建提交。

### 同日：用户配置后的 Skill 产物只读核验

按用户请求检查本机隔离验收目录 `.cc-switch/contribution-prep/vps-desktop-data`，未接触日常配置。当前共 2 条主机记录，其中 1 条启用 Claude，其他客户端未启用。Claude 投影 `home/.cc-switch/vps/skill-projections/claude/SKILL.md` 和部署 `home/.claude/skills/cc-switch-vps/SKILL.md` 均存在，内容逐字节一致；专属目录 `home/.cc-switch/vps/clients/claude.json` 仅包含该绑定主机，ID 一致。隔离数据库以 SQLite 只读模式查询，确认 `internal:vps`、`managed_by=vps` 及 Claude-only 启用状态。检查范围内无缺失或多余客户端部署，路径均位于隔离 home 内。

结论：**用户选择客户端后的 Skill 生成、部署和目录引用核验通过**。仅比对投影与部署文件内容，没有解释数据库/恢复状态的内部哈希算法；未验证真实客户端加载或执行。普通客户端启动不会自动使用隔离验收目录。本次没有启动应用/客户端/SSH、运行测试、读取密码或私钥内容、访问系统凭据库，也没有更改验收数据；仓库仅补充此记录。

用户随后取消全部客户端选择，再次只读核验同一隔离验收目录：2 条主机记录仍保留，启用客户端绑定为空；此前 Claude 的 `cc-switch-vps` 部署目录及 `SKILL.md`、Claude 投影目录及 `SKILL.md`、`clients/claude.json` 均不存在；`skill-projections/` 与 `clients/` 为空，`skill-state.json` 的 `deployments` 为 0，隔离数据库中 `internal:vps` / `managed_by=vps` 的 Skill 记录为 0。**本次最后解绑后的部署、目录与托管登记清理核验通过**。未手动删除任何文件，未连接 SSH 或访问凭据库。

## 18. 功能验收收口与 PR 准备（2026-10-01）

用户确认本轮功能验收可以通过，转入 PR 准备；不再扩展客户端密码复用。该确认覆盖前述页面、用户真实 SSH 测试、Skill 产物和最后解绑清理，不追溯补齐没有执行的客户端命令、认证组合或其他平台验证。PR 必须如实区分这些边界与既有 Windows 1314 失败。

- 已执行 `git fetch origin main` 和 `git fetch upstream main`。`origin/main` 与 `upstream/main` 均为 `7c0d0fc6`，从功能起点 `846de29c` 新增 11 个上游提交；功能 HEAD 仍为 `59a83da1`、已有 5 个阶段提交。本地 `main` 仍为旧的 `1ee2fdc3`，fetch 不等于已合并功能分支。
- 上游与功能的改动交集为 `src/App.tsx`、`src-tauri/src/lib.rs`、四语言文件和 `tests/integration/App.test.tsx`。没有竞争性的 CC Switch schema 迁移；上游仍为 v19，新增的 OpenCode V2 SQLite 兼容是客户端会话数据库，不是本应用数据库。此为路径/历史分析，尚未执行合并或证明无冲突。
- 建议先审阅并保存当前未提交的 VPS 界面/认证/测试工作，再将最新 `upstream/main` 合并入功能分支，保留已有历史；之后在合并结果上重新检查、回归并整理 PR。当前尚未 merge/rebase、新增 commit、push 或创建 PR。
- 用户授权配置 GitHub CLI。官方便携版 `2.102.0` 已校验发布页 SHA-256，安装于被忽略的 `.cc-switch/contribution-prep/tools/gh/bin/gh.exe`，不改系统 PATH。现有环境认证识别账号为 `Furina-he`；只读确认上游 #7739 为该账号创建、无评论，该功能分支尚无 PR。
- 用户选定新 Issue 标题为 **“新增 VPS 管理模块，支持客户端接入”**。首次使用环境令牌修改时，GitHub 返回 `Resource not accessible by personal access token (updateIssue)`，当时线上标题未变。随后用户通过官方 CLI 浏览器流程完成 `Furina-he` 登录；主线程在单次命令中移除 `GH_TOKEN` / `GITHUB_TOKEN` 的优先级覆盖，重新核验账号后成功修改，并再次读取确认 [#7739](https://github.com/farion1231/cc-switch/issues/7739) 标题为用户选定文本。**标题更新已完成，正文及 Issue 开关状态未修改。** 后续 GitHub 写操作应使用此次已授权登录，不让原受限环境令牌覆盖；不在聊天或日志中输出令牌。发布 PR 与推送仍需其各自明确授权。

## 19. Windows 1314 修复与提交前审阅（2026-10-01）

用户要求修复三项 Windows 1314 后准备 PR。先在新的隔离测试 home 逐项复现，三项均在 fixture 创建目录符号链接处失败，尚未进入被测逻辑。只读检查确认 Developer Mode 关闭、当前进程未提权。

### 目录链接夹具修复

三项测试均使用已存在目录的绝对目标，分别验证 canonicalize 后越界、循环链接跳过和 SSOT 链接清理。Rust 1.95 对 Windows junction 的 `is_symlink`、`read_link`、`remove_dir` 语义已通过独立临时目录探针确认。因此新增仅在测试构建中使用的共享 helper：优先原生目录 symlink，仅错误 1314 时回退真实 junction，仍断言链接类型和真实目标。保留所有原有业务断言；不适用于文件、悬空或相对 symlink，也不声称本机已覆盖 Windows 原生 symlink tag。

- Codex、Grok 两项原失败分别通过；共享夹具的特殊路径/删除保留目标及不覆盖已有路径两项通过；完整 `skill_sync` 集成目标 9 项通过（含该 helper 的两项测试）。
- 不改业务代码、不增加 skip 或 ignore、不改系统权限；目录路径经子进程环境变量传入 `cmd.exe /D /V:OFF`，避免空格、Unicode、`&`、`%` 和 `!` 的命令展开问题，已有对应真实文件系统测试。
- 已单独提交 **`f77e3794` — `test(windows): support unprivileged directory link fixtures`**；该提交不包含之前未提交的 VPS 页面/认证扩展。

### 提交前审阅发现及修复

- 部分保存/删除在主数据已提交后失败时，旧前端缓存可能再次整体回写。新增 3 项先红灯回归，再让失败 mutation 等待 VPS query 重新 reconcile；若重读仍失败，现有 query 错误状态阻断旧列表操作。成功保存仍按原规则刷新缓存，草稿和原有断言不变。前端定向 4 文件、42 项通过。
- 已保存密码的读取原先只绑定主机 UUID，可能把当前 revision 用于旧草稿的目标。新增失败回归后，将 host/port/user/authMethod 与持久化记录一起在锁内校验并保留不可变 revision 快照，然后仍在锁外读凭据库；地址、端口、用户或认证方式变化拒绝自动取密码，名称/用途/客户端变化不受影响。补齐四语言提示。
- 原生 loopback 探针确认 Windows `accept()` 后的 socket 继承 nonblocking：设置 read timeout 后仍立即 WouldBlock。ASKPASS 分片 token 回归先以连接中止失败，修复为接受后显式恢复 blocking，再使用原有超时；没有加长超时或更换 SSH 协议。
- 增强的 `cargo clippy --all-targets -D warnings` 首次发现本分支 VPS 测试 3 项不必要 clone 及上游既有 `transform_codex_chat.rs:4498` 的 `op_ref`。VPS 三项已改用 `std::slice::from_ref`；上游那项在 `upstream/main` 原样存在，未改无关断言，也未添加 lint allow。项目 CI 实际采用不带 `--all-targets` 的 Clippy 命令，已通过；额外全目标检查不能冒充全绿。

上述认证与 ASKPASS 验证只用内存凭据库、固定过程替身或 loopback，不访问真实 VPS 密码/私钥或连接服务器。中间认证回归曾因测试在切换到非密码方式时仍提供密码而失败，已改为合法测试输入，未放宽生产验证或期望状态。最终 VPS 后端定向 **99 项通过**（含新增目标绑定/显示字段兼容和 ASKPASS 分片回归），CI 的 Clippy 命令、Rust 格式、前端 TypeScript/格式均通过。合并上游后的最终全量结果和 PR 材料准备状态将在本节继续更新。

本机证据：`windows-1314-{codex,grok,skill}-before.log`、`windows-1314-{codex,grok,helper,skill}-after.log`、`windows-1314-clippy-all-targets.log`、`vps-pr-reconciliation-{red,green}.log`、`vps-pr-credential-target-red.log`、`vps-pr-askpass-fragment-red.log`、`vps-pr-review-rust-{green,final}.log`、`vps-pr-review-clippy.log`。后端统一用本机 `run-isolated-rust.sh` 设置独立临时 home、TEMP 和各客户端/AppData 路径，不改变 Cargo/rustup 的 HOME。

### 上游合并与最终门禁

- 当前功能分支新增独立本地提交：`f77e3794`（Windows 目录链接夹具）、`b5908616`（主机 IPC/本机认证）、`3105c2c7`（全局 VPS 页面及前端一致性）。原有五个阶段提交保留，没有 rebase 或改写历史。
- 合并前再次 fetch 发现上游新增 `9be1ef7e` 定价数据提交，实际合入 12 个上游提交。自动合并无文本冲突；逐字节确认已审阅的 VPS 核心代码与四语言 VPS 文案没有被覆盖。完整验证后创建合并提交 **`4d2e51c9`**。本地 `main` 未重置，未推送任何分支。
- 第一次合并后完整 Rust 为 3056 通过 / 5 失败 / 10 原有忽略；所有集成目标通过。5 项 OpenCode 会话失败来自本机启动脚本固定 `OPENCODE_DB` 覆盖测试自身的临时 XDG 库，首项 canonicalize 失败后导致后续锁中毒。仅在被忽略的启动脚本中移除这一覆盖，保留临时 `XDG_DATA_HOME`；未改上游代码、断言或忽略项。OpenCode 组重跑 13 项全通过，再次执行完整套件。

| 合并后检查 | 最终结果 |
| --- | --- |
| Rust 库 | **3061 通过、0 失败、10 原有忽略** |
| Rust 全部集成目标 | **177 通过、0 失败**；15 个目标全部执行（含空 support） |
| 完整前端 | **153 文件、1751 通过**；单 worker |
| TypeScript / 前端格式 / Rust 格式 | 通过 |
| CI 标准 Clippy `-D warnings` | 通过 |
| renderer 构建 | 通过，既有包体/动态导入警告保留 |

完整前端仅改写了一个快照的换行格式；与 index 规范化内容逐字节比较相同后恢复 checkout 格式，不更新快照内容。前后端大套件串行运行，未提高超时、放宽断言或改系统权限。目录链接原生 Windows tag、macOS/Linux 原生认证和真实客户端执行等未测边界仍按前述记录保留，不以 Windows 本地全绿倒推跨平台验收完成。

PR 文案及脱敏截图说明已准备在被忽略的 `.cc-switch/contribution-prep/pr-draft.md`，标题为 `feat(vps): 新增 VPS 管理模块，支持客户端接入`，关联 #7739。已确认验收主机数据、本机工具和截图未进入 Git；截图是组件/内存数据预览，不宣称原生 SSH 证据。**尚未 push、创建 PR 或新增 Issue 评论，发布动作等待单独明确授权。**

最终额外 `cargo clippy --all-targets -- -D warnings` 也已复核：仅余上游既有 `transform_codex_chat.rs:4498` 的一项 `op_ref`，没有增加 allow 或修改该无关测试。该增强检查仍失败，不能与通过的项目 CI 标准 Clippy 命令混为一谈；证据为 `vps-pr-merged-clippy-all-targets.log`。

最终日志：`vps-pr-merged-opencode-recheck.log`、`vps-pr-merged-rust-final.log`、`vps-pr-merged-clippy-final.log`、`vps-pr-merged-typecheck.log`、`vps-pr-merged-format.log`、`vps-pr-merged-frontend-final.log`、`vps-pr-merged-renderer-final.log`。失败的首轮完整 Rust 输出保留为 `vps-pr-merged-rust-full.log`，不覆盖或隐去。

## 20. PR 评审修复与上游更新（2026-10-02）

[PR #7804](https://github.com/farion1231/cc-switch/pull/7804) 已为非草稿、未合并。用户明确授权合入最新上游、复现并修复评审问题，完成本地验证后提交、推送并检查新 CI；不自动合并 PR，也不改回草稿。

### 上游及既有 CI 失败

- 本轮拉取时 `origin/main` 为 `67d1daa1`，最新 `upstream/main` 为 `b9e96202`；已无文本冲突合入，并在验证后创建独立合并提交 `3f250aa8`。相对上次合并新增26项上游提交，包括 Codex/Stack、定价模型元数据及供应商预设等现有上游变更，不作为 VPS 新功能修改。
- [原 CI 运行](https://github.com/farion1231/cc-switch/actions/runs/36895406133) 的 Ubuntu/Windows 停在 Clippy：Codex 钥匙串两个变体仅在 macOS 构造，触发非 macOS 的 dead_code。该运行 checkout 的模拟合并不含上游 `67d1daa1`；本轮直接带入已有修复，不另写同类补丁。原运行的后续测试未执行，不能写成测试通过。

### 托管链接路径检查

[自动评审评论](https://github.com/farion1231/cc-switch/pull/7804#issuecomment-5944338768)指出：Auto/Symlink 部署后，保留绑定再次读取或保存时，VPS 的路径重叠检查跟随合法末级链接进入自己的投影目录，误判与本机数据重叠。

- 原实现上先添加真实目录链接回归，得到23通过/2失败，失败正是读取/保存与目录迁移误报重叠。没有跳过断言或只用字符串模拟链接。
- 仅在 `services/skill/vps.rs` 区分部署目录项位置与末级链接目标。原有 receipt、目标、内容哈希及用户文件归属校验保留，普通 Skill 的通用 `paths_overlap` 未修改。
- 路径解析从父目录向上找到最近存在的祖先，再规范化并拼回缺失后缀；既有但无法解析的祖先返回错误，不当作无重叠。不能只解析直属父目录：新增的祖先别名/缺失子目录反例，在初版修复上实际得到25通过/1失败（本应拒绝却保存成功），随后完善解析。
- VPS 本机根和 SSOT 仍不能与部署位置重叠；SSOT 额外比较完整解析后的目录目标。迁移期间指向同一投影的旧/新受控链接按各自目录项比较，不误认为是同一部署位置。
- 新增6项回归覆盖重载/二次保存、目录迁移及双 receipt 重试、未登记/改向链接拒绝、普通及链接 SSOT 重叠、词法及祖先别名造成的嵌套。扩展原 Unix 用例，分别实际使用 Auto/Symlink 首次部署后再读取/保存和清理。
- Windows 本机使用既有测试 helper 的真实目录链接（无符号链接权限时回退 junction）并保留已登记的部署回执；这是链接目录项与生命周期验证，不声称本机执行了有权限的原生 symlink 创建。Unix 原生首轮部署由后续平台 CI 验证。

### 本地验证与发布状态

- 完善修复后 `services::vps::` 定向 **99通过，0失败**，包含新增6项回归；使用隔离 home、TEMP、AppData及客户端目录，未访问系统 VPS 凭据、日常数据库或真实服务器。
- 合并工作区与最终修复共同完成全量验证：Rust 库 **3201通过、0失败、10项原有忽略**；全部15个集成目标执行，**178通过、0失败**；前端 **161文件、1835通过**。TypeScript、前端格式、Rust格式、CI标准Clippy和renderer构建均通过。前端仍有既有测试警告，构建仍有动态导入/包体警告，没有为消除警告修改无关功能。
- 本轮临时设置 `CARGO_INCREMENTAL=0`，保留已有依赖构建缓存，不立即重建之前清理的大量增量缓存；不修改仓库构建配置、系统权限或页面文件。前后端大任务串行执行，未增加超时、新增ignore或放宽测试断言。前端测试仅重写一个既有snapshot的换行；与index规范化内容逐字节一致后恢复原checkout格式，没有更新快照内容。
- 修复的独立静态复核未发现新增阻断项。上述结果是Windows本地验证，不能替代新head的远端平台CI；额外all-targets Clippy本轮未重跑，第19节的历史结果仍只代表当时检查。

日志位于被忽略的 `.cc-switch/contribution-prep/`：`vps-pr-followup-red.log`、`vps-pr-followup-parent-alias-red.log`、`vps-pr-followup-green.log` 和 `vps-pr-followup-final-*.log`。本机串行检查脚本为 `validate-vps-pr-followup.sh`。新 CI 必须对应随后推送的 head，不能沿用原 PR 检查或本机成功来声称平台 CI 已通过。

## 21. 客户端密码自动复用（2026-10-02）

用户在调查 `mcp-ssh-manager` 后明确授权按“保留系统凭据库＋一次性本机 SSH 入口”制定计划、实现并验证，更新第 17–18 节的搁置决定。完整计划、失败证据和最终验证记录见 [VPS 客户端密码复用](vps-client-password-reuse.md)。

- 已新增 `vps exec` 早期 CLI 分流和独立执行 runner，复用现有系统 SSH/ASKPASS。先检查主机、当前客户端绑定、确认 pin 与不可变密码 revision，再在凭据读取后重检快照；没有新增 MCP、Agent、常驻服务、密码明文文件或用户开关。
- 客户端目录 v2 提供执行器路径和固定参数，托管 Skill 自动调用入口；旧目录及安装路径通过已有回执升级/刷新。四语言文案、CLI 诊断和现有生命周期回归同步更新。
- 已通过实际 Windows GUI-subsystem 二进制、临时 SESSION 凭据和 loopback SSH 协议夹具验证：Windows OpenSSH 9.5p1、Git OpenSSH 9.9p2 的密码复用、二进制标准流、退出码、指纹/绑定/缺密码拒绝、取消/超时及空格/Unicode 目录；以服务端握手标识确认实际 SSH 实现。没有连接生产 VPS 或访问日常凭据。
- 实测并修复 SSH 内层选项终止符缺失，以及 Windows OpenSSH 窄字符环境导致 Unicode ASKPASS 路径失败；不是仅凭单元测试宣称原生功能完成。所有测试自建凭据已删除。
- 最终 VPS 定向 **120 通过**；Rust 库 **3222 通过、0 失败、10 原有忽略**，全部 16 个集成目标 **181 通过**；前端 **161 文件、1855 通过**。TypeScript、前后端格式、CI 标准 Clippy 和 renderer 构建均通过。既有构建/测试警告保留，不运行或冒称额外 all-targets Clippy。
- 前端测试仅重写既有 golden snapshot 的换行；确认与 index 规范化内容一致后恢复原 checkout 格式，没有更新快照内容。

本轮仍是**未提交、未推送的本地工作区**，没有替换日常安装版或启动其 GUI。真实客户端模型会话、生产服务器、macOS/Linux 原生认证、签名安装包和超出单命令入口的交互认证仍未验收；不得将本次 Windows 本机证据扩大到这些范围。

同日补充：用户明确现有主机为虚构测试信息，只验收密码配置/流程，不进行真实连接。已启动隔离 Tauri 窗口并通过真实 UI 验证保存密码、空框编辑与留空保留、密码替换及旧凭据清理、客户端目录/Skill 生成、无指纹时本地拒绝、最后解绑及删除清理，全部通过。只操作自有临时主机，原有两条记录及密码引用保持不变，所有临时凭据已删除；没有调用模型或连接测试地址。详细证据见新计划末尾和本机 `vps-client-ui-password-result.json`。

## 22. 密码复用提交、上游整合与 fork 推送（2026-10-02）

用户已确认可以提交并推送到 fork，同时要求处理主线更新，并询问是否继续使用原 Issue。该授权更新第 21 节及密码复用计划的未提交状态；不授权合并上游 PR、修改系统权限或连接生产服务器。

- 发布前核验：本地功能分支和远端功能分支均为 `62adf70b`，工作区只有本轮密码复用实现、测试与文档。fork 为 `Furina-he/cc-switch`，GitHub CLI 登录身份与 fork 一致。
- fetch 后 fork 的 `origin/main` 为 `b9e96202`，已经包含在功能分支中；官方 `upstream/main` 新增一个 `4e46e6b6`（Codex MCP 不再写入 `type`）。该上游提交只改 MCP Rust 模块及对应测试/快照，与本轮 VPS 文件无直接交集；合入功能分支，不改 fork 的 main。
- 原 [PR #7804](https://github.com/farion1231/cc-switch/pull/7804) 仍为 open、非草稿，源分支仍是当前功能分支，正文使用 `Refs #7739`。推送同一源分支自动更新原 PR，无需新开 Issue/PR；当前是引用关系，不宣称合并会自动关闭 Issue。
- 先保存已验证的功能提交，再合入上游并复验，最后非强制推送。后续结果追加于本节，不把此前门禁冒充合并后或远端 CI 结果。
