# VPS 自动托管 Skill：执行计划与新会话交接

日期：2026-09-29。关联 [Issue #7739](https://github.com/farion1231/cc-switch/issues/7739)。

**用途：在新对话中直接执行本计划，不再重新讨论已确认的产品方向。阶段 A/B 基础和阶段 C 受控部署已实现并验证定向用例；D 的恢复/同步保护及 E/F 的整合仍在推进，整体仍有既有符号链接权限测试失败。最新主线程记录见第 11 节，尚未完成真实 SSH 与桌面验收。** 产品要求以 [最终方案](../proposals/vps-skill-management.md) 为准；旧 Issue 草稿和早期聊天中的双开关/可见自动 Skill/自定义客户端注册方案均不再采用。

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
