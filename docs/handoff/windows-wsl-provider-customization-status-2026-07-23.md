# CC Switch Windows + WSL2 Provider 二开交接说明

> 快照日期：2026-07-23（Asia/Shanghai）  
> 仓库：`M1kasali/cc-switch`，上游：`farion1231/cc-switch`  
> 当前分支：`feat/multi-environment-provider-targets`  
> 当前 HEAD：`3a528179 fix(codex): normalize managed provider routes`  
> 上游基线：`upstream/main` = `606e7bbe`（tag `v3.18.0`）

## 1. 先读结论

这次二开的短期目标，是让 Windows 版 CC Switch 可以在不复制完整配置目录、不混入跨平台路径、也不删除原有历史的前提下，分别管理：

- Windows Codex：`C:\Users\<user>\.codex`
- 一个或多个 WSL2 Codex：例如 `/home/<user>/.codex`

目前已经形成可日常试用的第一版：Windows 与 WSL2 可以分别选择和切换 Provider；每个环境保留自己的路径、认证、MCP、策略和历史；Codex Provider 在每个环境内使用稳定的 `custom` 运行时路由，以保持原生历史列表统一；旧会话标签可以按 Target 显式迁移并从备份精确恢复。

不过，**当前工作区不是可以直接发布或直接提交 PR 的干净状态**：

- 分支上已有 7 个二开提交；
- 另有 23 个已跟踪文件的未提交修改和 5 个未跟踪文件；
- GitNexus 对相对 `upstream/main` 的整体差异判定为 `CRITICAL`（38 个文件、334 个已索引符号、52 条受影响执行流）；
- 未提交增量本身也被判定为 `CRITICAL`（23 个文件、58 个已索引符号、19 条受影响执行流）；
- 风险评级主要来自启动、设置保存、Provider 切换、WSL 命令和配置投影等中心路径被修改，不代表已经确认存在安全漏洞，但必须先审查再提交。

用户已明确决定：**暂不提交 PR，等功能完善且审查无误后再发布。**

## 2. 产品语义

### 2.1 领域对象

- **Provider**：可复用的模型后端定义，包含路由、凭据、模型和协议字段。
- **Environment**：拥有独立配置、认证和历史的运行环境，例如 Windows 或某个 WSL 用户。
- **Managed Target**：CC Switch 对“一个 Application 在一个 Environment 中的安装”的本地注册记录。
- **Projection**：把 Provider 管理的字段投影进 Target，同时保留 Target 本地字段；不是复制整个配置目录。
- **Runtime Route / Session Bucket**：Codex 原生使用的路由和历史分类键。当前统一使用 `custom`，它不是 CC Switch Provider ID。

完整术语见仓库根目录的 [`CONTEXT.md`](../../CONTEXT.md)。

### 2.2 核心原则

1. Provider 与 Environment 分离。
2. 未明确列入白名单的 Codex 字段默认属于 Target，不跨环境传播。
3. 注册或关联 Target 只做只读探测；首次实际写入必须由用户显式“启用管理”。
4. 普通 Provider 切换不复制 Windows/WSL 配置，不写 `auth.json`，也不移动会话。
5. Windows 与 WSL 历史永远是两个独立 Target 的历史，不做跨 Target 合并。
6. 每个 Target 内沿用上游 CC Switch 的统一历史语义：CC Switch 管理的 Codex Provider 共享稳定的 `custom` 桶。
7. “历史可见”不等于“跨后端无损续聊”；旧会话中的 `encrypted_content` 可能无法被另一个后端解密。

架构决策见：

- [`ADR-0001：Model Provider Management as Environment Targets`](../adr/0001-model-provider-management-as-environment-targets.md)
- [`ADR-0002：Preserve Upstream Codex Unified History per Target`](../adr/0002-preserve-upstream-codex-unified-history-per-target.md)

## 3. 已实现内容

### 3.1 Managed Target 模型与兼容迁移

- 增加设备本地的 Managed Target 注册表。
- 旧单目录设置会幂等迁移为 Windows 或 WSL Target，不在迁移时重写 live 配置。
- Target 具有稳定 ID、Application、名称、Target Kind、配置位置、管理状态和当前 Provider。
- Windows 与不同 WSL Target 的当前 Provider 状态彼此隔离。
- Provider 被 Target 使用时增加删除保护。

主要文件：

- `src-tauri/src/settings.rs`
- `src/types.ts`
- `src/hooks/useManagedTargetSelection.ts`
- `src/components/providers/ManagedTargetSelector.tsx`

### 3.2 Windows / WSL2 探测与注册

- Windows Target 直接检查 Codex 配置目录和关键文件。
- 通过 `wsl.exe --list --quiet` 等只读命令发现 WSL 发行版。
- 读取默认 Linux 用户、HOME 和 `.codex` 路径。
- 设置页支持发现、注册、检查、关联 Provider 和显式首次启用管理。
- `docker-desktop` 等没有 Codex 目录的环境只显示为未发现，不自动接管。
- WSL 命令使用独立 argv，不拼接用户输入到 shell 文本。

主要文件：

- `src-tauri/src/target_inspection.rs`
- `src-tauri/src/commands/settings.rs`
- `src/components/settings/EnvironmentTargetsPanel.tsx`
- `src/lib/api/settings.ts`

### 3.3 单 Target Provider 切换

- Codex 主界面增加“当前运行环境”选择器。
- 切换只作用于当前选中的 Windows 或 WSL Target。
- 其他 Application 继续走原有单环境切换路径。
- WSL 首次启用时先完成配置投影，成功后才把 Target 标记为 Managed。
- 状态提交失败时恢复 live 配置的原始字节。
- WSL 写入在发行版内部进行：同目录临时文件、原子替换、写后校验。
- WSL 新文件权限为 `0600`；已有文件替换时保留权限。
- `config.toml` 与内联 model catalog 一起快照和回滚。
- Windows/WSL 子进程使用 `CREATE_NO_WINDOW`，避免用户操作时弹出命令行窗口。

主要调用链：

```text
Provider UI
  -> useSwitchProviderMutation(appId, targetId)
  -> switchManagedTargetProvider
  -> ProviderService::switch_managed_target
  -> Windows 原子写入，或 WslTargetAdapter::apply_provider_config
  -> 成功后提交 target.current_provider_id
```

主要文件：

- `src/lib/query/mutations.ts`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/codex_config.rs`
- `src-tauri/src/target_inspection.rs`

### 3.4 Codex 字段级投影

当前 Adapter 只管理明确的路由/模型白名单，典型字段包括：

- active `model_provider`
- active `[model_providers.<id>]` 路由表
- `model` 与明确的模型能力字段
- `base_url`、`wire_api` 等 Provider 路由字段
- Provider-scoped bearer token

以下内容保持 Target 本地所有权：

- Windows/Unix 路径和 `projects`
- approval / sandbox / notices / hooks
- MCP、Skills、Prompts
- 官方 `auth.json`
- `sessions/`、`archived_sessions/` 和 state SQLite
- 未识别字段

第三方 Provider 和启用统一历史后的官方 Provider，都投影为 Codex 的稳定 `custom` 路由。真实渠道身份仍由 CC Switch 的 Provider ID 和 Target 状态保存，不编码到 Codex `model_provider`。

### 3.5 统一 Codex 历史

当前实现分两层：

1. **上游兼容层**：保留上游已有的 `unify_codex_session_history`、官方 `custom` 注入、迁移标记和恢复能力。
2. **Target-aware 显式迁移层**：为指定 Windows 或 WSL Target 迁移全部非 `custom` 旧标签，并建立独立备份账本。

显式迁移规则：

- 只支持 Codex Target。
- 迁移前要求该 Target 当前 live `model_provider = "custom"`；否则返回 `live_not_unified`，避免迁移后历史立即被当前路由隐藏。
- 扫描该 Target 的 `sessions/**/*.jsonl`、`archived_sessions/**/*.jsonl`。
- 同步更新有效 `state_5.sqlite` 中 `threads.model_provider`。
- 兼容 `sqlite_home` 和 `CODEX_SQLITE_HOME` 指向的状态库。
- 修改前备份 JSONL 原始内容和 SQLite。
- JSONL 使用同目录临时文件、`fsync` 和原子替换；写入前检查文件是否被并发修改。
- SQLite 使用 Backup API、`BEGIN IMMEDIATE` 和事务。
- Restore 只恢复账本中有明确原始 Provider、并且当前仍为 `custom` 的会话，不猜测新会话来源。
- 操作幂等；已经归一时返回 `already_unified`，没有可恢复内容时返回 `nothing_to_restore`。

Windows 备份根目录：

```text
C:\Users\<user>\.cc-switch\backups\codex-target-history-unify-v1\<target-id>\<generation>
```

WSL 备份根目录：

```text
~/.cc-switch/backups/codex-target-history-unify-v1/<target-id>/<generation>
```

主要文件：

- `src-tauri/src/codex_history_migration.rs`
- `src-tauri/src/target_history_migration.rs`（当前未跟踪）
- `src-tauri/tests/target_history_migration.rs`（当前未跟踪）
- `src/components/settings/EnvironmentTargetsPanel.tsx`
- `tests/components/EnvironmentTargetsPanel.test.tsx`（当前未跟踪）
- `docs/research/codex-unified-provider-history.md`（当前未跟踪）

### 3.6 用户体验修复

- Provider 切换开始后，选中的“启用”按钮立即进入禁用/加载状态，避免 WSL 操作看起来没有响应或被重复点击。
- 历史迁移确认后立即关闭确认框，后台继续执行；所有迁移/恢复按钮在 pending 期间禁用。
- WSL Provider 写入和历史迁移使用隐藏子进程，不再弹出黑色命令行窗口。
- WSL 原子写入的哈希/路径参数问题已经修复，避免空路径传给 `stat` 或 `sha256sum`。

## 4. 当前机器上的数据状态

这一节只描述本次人工验收机器，不应成为通用逻辑或测试夹具。

### 4.1 Windows 历史迁移

Windows Target 已由用户显式完成迁移。备份账本位于：

```text
C:\Users\M1kasa\.cc-switch\backups\codex-target-history-unify-v1\
```

已观察到的主要迁移代际：

- 来源标签：`cc-switch-official`、`openai`、`openai_http`
- 修改 JSONL：4 个
- 修改 SQLite 行：42 行

另有一次紧接着发生的幂等/重复操作代际，没有新增有效迁移计数。

### 4.2 WSL 历史迁移

WSL Target `Ubuntu-24.04 / m1kasa / /home/m1kasa/.codex` 已由用户显式完成迁移。备份账本位于：

```text
/home/m1kasa/.cc-switch/backups/codex-target-history-unify-v1/
```

已观察到两个迁移代际：

- `20260723_004957...`：来源 `openai`，修改 49 个 JSONL、49 行 SQLite。
- `20260723_005017...`：来源 `openai`，修改 45 个 JSONL、0 行 SQLite。

第二次操作发生于旧 UI 确认框未及时关闭期间，是 RC4 修复“立即关闭确认框/禁止重复触发”的直接背景。数据并未丢失，但审查者应重点检查重复迁移、并发写入和账本合并语义。

### 4.3 操作限制

- 不要为了测试再次对真实 Windows/WSL 历史执行迁移。
- 不要对真实历史点击 Restore，除非用户明确要求回退。
- 自动化测试必须使用临时目录和临时 SQLite。
- 迁移/恢复前应关闭该 Target 中的 Codex CLI、VS Code Codex 插件和其他可能写会话的进程。
- 备份包含会话内容，属于敏感本地数据，不能提交到 Git、上传到 PR 或粘贴到日志。

## 5. 已完成的验证

### 5.1 自动化与静态检查

在 RC4 构建前，本次开发已通过：

- Rust 全量 all-target 测试。
- 5 个 Target-aware 历史迁移/恢复集成测试：Windows 迁移、live 非 custom 拒绝、Windows 精确恢复、WSL 迁移、WSL 幂等恢复。
- WSL 隐藏进程 flag 单元测试。
- 前端单元测试：84 个测试文件、532 个测试。
- `pnpm typecheck`。
- `cargo clippy --all-targets -- -D warnings`。
- `cargo fmt` / Prettier。
- Windows 原生 MSVC Release 构建。
- `git diff --check` 当前无空白错误。

建议后续 Agent 不依赖上述历史结果，修改后重新运行：

```bash
pnpm typecheck
pnpm test:unit
pnpm format:check
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Windows Release 构建还应在 Windows `stable-x86_64-pc-windows-msvc` 工具链上重新执行。

### 5.2 人工验收

已在真实 Windows + WSL2 环境验证：

- 发现 `Ubuntu-24.04` 默认用户、HOME 和 `.codex`。
- 注册 WSL Target 并启用 Provider 管理。
- Windows 与 WSL 分别切换 Provider，互不覆盖。
- WSL Provider 切换后，CLI `codex exec` 请求成功。
- WSL 的 VS Code Codex 插件重启后读取当前 PinAI 配置。
- 一个旧 WSL 会话恢复后继续发送的新请求走当前 PinAI `/responses`，没有静默回到官方 Provider。
- Windows 与 WSL 会话文件数量在切换前后保持；迁移只改变历史标签与 SQLite 对应字段。
- WSL 命令行窗口弹出问题在 RC4 中修复。

Provider 兼容性观察：PinAI 没有实现 Codex 请求的 `/models?client_version=...` 接口，因此日志中会出现 404；但 `/responses` 请求可以成功。这是 Provider API 覆盖度问题，不等同于 Provider 切换失败。

## 6. 当前可运行产物

RC4 单文件 Windows GUI 产物：

```text
C:\Users\M1kasa\Downloads\CC-Switch-3.18.0-unified-history-rc4.exe
```

SHA-256：

```text
CB4336F7F000D971309F72465834F3297247F3A8FC023807183E88DE1FE47DCB
```

文件大小：33,649,664 bytes。已确认是 `PE32+ executable (GUI) x86-64`。

注意：

- RC4 是候选测试版，不是正式安装包。
- 它来自包含未提交修改的工作区，不能仅从当前 HEAD `3a528179` 重现。
- RC3 及更早调试产物已经过期，不应继续使用。
- 日常使用前应完全退出旧版 CC Switch，包括托盘进程，避免单实例机制打开旧进程。
- 正式发布前需要决定自定义版本的安装目录、应用身份、更新源和版本号，避免上游自动更新覆盖定制版。

## 7. 已知缺口与风险

### 7.1 P0：提交前必须处理

1. **工作区未提交且风险范围大**  
   不得直接 `git add -A && git commit`。先逐文件审查当前 23 个修改文件和 5 个未跟踪文件。

2. **`pnpm-workspace.yaml` 有可疑占位内容**  
   当前未提交差异包含：

   ```yaml
   allowBuilds:
     esbuild: set this to true or false
     msw: set this to true or false
   ```

   这看起来是 pnpm 自动提示或误写，不是已确认设计，提交前必须删除或改成合法配置。

3. **设计文档存在已过时段落**  
   `docs/design/multi-environment-provider-targets-zh.md` 的“第一版非目标”和“9.3 不采用原生历史合并”仍描述“不改写历史”。该设计已经被 ADR-0002 和当前实现取代，需要统一文档，避免后续 Agent沿错误方向开发。

4. **历史迁移涉及真实数据**  
   必须重点审查并发修改检测、SQLite WAL/SHM、外置 `sqlite_home`、重复迁移、部分失败回滚和恢复账本优先级。真实数据只能做只读核验。

5. **上游与本地 `main` 不一致**  
   本地 `main` 当前为 `08710d51`，上游基线为 `606e7bbe`。审查和差异统计应使用 `upstream/main`，不要误用本地 `main`。

6. **没有准备提交 PR**  
   用户已经撤回“立即提 PR”的计划。审查 Agent 不应自动提交、推送或创建 PR。

### 7.2 功能缺口

- CC Switch 自带 Session Manager 尚未 Target-aware，目前仍主要扫描 Windows Codex Home。
- Session Manager 需要增加 Target 选择、Target-aware query key、正文读取、删除范围和恢复命令。
- 尚未实现多 Target 一次联动切换和逆序回滚。
- 尚未实现持久化快照轮换、Drift 处理和 Target Override 编辑。
- 尚未实现运行中 Codex 进程探测与明确重启提示。
- 尚未实现 Target-aware 本地代理/路由接管；多环境切换时暂不支持 Codex 代理 takeover。
- 托盘菜单尚未按 Environment 分层。
- 尚未支持同一 WSL 发行版的多个用户、SSH、远程服务器或 Dev Container。
- 尚未把相同模型扩展到 Claude Code、Gemini 等其他 Application。
- 尚未生成正式 NSIS/MSI 安装包，也没有定制版本的独立更新通道。

### 7.3 Codex 自身边界

- `custom` 统一的是历史分类标签，不保存真实 Provider 来源。
- 统一后，Codex 原生历史无法区分 PinAI、My Codex 或 OpenAI Official；未来如需来源展示，应建立 CC Switch 自己的 provenance 账本。
- 不同后端产生的 `encrypted_content` 可能不能跨 Provider 继续；失败时应切回原 Provider 或新建会话。
- VS Code Codex 插件首页能看到本地任务计数，但点击 `View all` 可能默认进入 `Cloud tasks`，显示空列表。这是插件筛选/导航问题，不是 CC Switch 删除历史；WSL 本地 JSONL 和 `state_5.sqlite` 已确认仍存在。
- Codex 日志曾出现 `state db discrepancy ... falling_back` 警告；当前能回退到文件系统并打开会话，但应在后续审查中确认是否由迁移时间、路径索引或 Codex 自身缓存造成。

## 8. 关键文件地图

| 文件 | 作用 | 当前状态 |
| --- | --- | --- |
| `CONTEXT.md` | 领域语言与实现边界 | 已提交后又有未提交更新 |
| `docs/design/multi-environment-provider-targets-zh.md` | 最初完整设计 | 已提交；部分历史策略已过时 |
| `docs/adr/0001-...md` | Managed Target 架构决策 | 已提交后又有未提交更新 |
| `docs/adr/0002-...md` | 每 Target 保留上游统一历史 | 未跟踪 |
| `docs/research/codex-unified-provider-history.md` | 上游/GitHub 方案调研 | 未跟踪 |
| `src-tauri/src/settings.rs` | Target 数据模型、持久化、兼容迁移 | 已提交 |
| `src-tauri/src/target_inspection.rs` | Windows/WSL 探测与 WSL 安全读写 | 已提交后又有未提交修复 |
| `src-tauri/src/services/provider/mod.rs` | Target-aware Provider 切换事务 | 已提交 |
| `src-tauri/src/services/provider/live.rs` | Provider live 投影 | 已提交后又有未提交调整 |
| `src-tauri/src/codex_config.rs` | Codex 字段投影与稳定 `custom` 路由 | 已提交后又有未提交调整 |
| `src-tauri/src/codex_history_migration.rs` | 上游兼容迁移、Target 迁移公共底层 | 大量未提交修改，重点审查 |
| `src-tauri/src/target_history_migration.rs` | Target-aware Windows/WSL 显式迁移器 | 未跟踪，重点审查 |
| `src-tauri/src/commands/settings.rs` | Tauri Target/迁移命令 | 已提交后又有未提交命令 |
| `src/components/providers/ManagedTargetSelector.tsx` | 主页面 Target 选择器 | 已提交 |
| `src/components/settings/EnvironmentTargetsPanel.tsx` | 设置页环境管理与历史操作 | 已提交后又有未提交功能 |
| `src/components/providers/ProviderActions.tsx` | 切换 pending UI | 未提交修改 |
| `src/lib/api/settings.ts` | 前端 Target/迁移 API | 已提交后又有未提交 API |

## 9. 当前分支提交

从上游 v3.18.0 之后，当前分支已有：

```text
1c729fd3 docs: define managed provider targets
112373f3 feat(codex): manage providers per environment target
4009de62 feat(ui): select and manage provider targets
0e128c35 chore: satisfy Rust lint checks
a0fb58fd docs: align first slice with implemented scope
d559099e fix(wsl): hide managed target subprocess windows
3a528179 fix(codex): normalize managed provider routes
```

这些提交不包含全部 RC4 源码；历史归一、显式 Target 迁移和部分 UX 修复仍在工作区中。

## 10. 建议的审查顺序

后续 Agent 建议按以下顺序工作：

1. 阅读本文件、`CONTEXT.md`、ADR-0001、ADR-0002。
2. 执行 `git status --short --branch`，确认没有把用户后续改动当成既有内容。
3. 以 `upstream/main` 为基线审查，而不是本地 `main`。
4. 先审查并清理 `pnpm-workspace.yaml` 可疑占位改动。
5. 审查 `codex_config.rs` 和 `services/provider/live.rs` 的稳定 `custom` 投影，确认官方与第三方路由不会串线。
6. 审查 `target_inspection.rs` 的 WSL argv、stdin、原子写、权限、校验和 `CREATE_NO_WINDOW`。
7. 审查 `codex_history_migration.rs` 与 `target_history_migration.rs` 的备份、迁移、恢复、并发和失败语义。
8. 审查 Tauri commands、前端 API、确认框和 pending 状态是否一一对应。
9. 先运行 Targeted Tests，再运行全量 Rust/前端检查。
10. 在临时 Codex Home 上做 Windows/WSL 故障注入；不要动真实历史。
11. 运行 GitNexus `detect_changes(scope="compare", base_ref="upstream/main")`，逐条检查受影响执行流。
12. 只有审查和真实烟测全部通过后，才拆分合理提交；不要自动推送或创建 PR。

## 11. 后续开发优先级

### P0：形成可正式发布的基础版

- 完成当前未提交代码审查和清理。
- 更新过时设计文档。
- 补齐历史迁移的锁、并发和失败恢复审计。
- 对 Windows/WSL 各完成 Official -> PinAI -> Official -> PinAI 往返测试。
- 验证切换前后配置 Local Fields、auth、sessions 和 state DB 的预期差异。
- 生成独立版本号和正式安装包，处理自动更新隔离。

### P1：会话管理

- 让 CC Switch Session Manager Target-aware。
- Windows/WSL 只读聚合，显示 Environment 标签。
- 建立 `(target_id, session_id) -> provider provenance` 本地账本；未知来源不猜测。
- 恢复会话时通过来源 Target 执行，并明确跨 Provider 密文风险。

### P2：扩展环境与应用

- Target-aware proxy。
- Claude Code、Gemini 等 Application Adapter。
- SSH/远程服务器 Target、远程锁、SFTP/命令执行和回滚。
- MCP、Skills、Prompts 的 Target 所有权与可选投影。

## 12. 给接手 Agent 的约束

- 遵守仓库 `AGENTS.md`：修改任何函数/类/方法前先运行 GitNexus upstream impact，并向用户报告 blast radius；HIGH/CRITICAL 必须先警告。
- 提交前必须运行 GitNexus `detect_changes()`。
- 不覆盖、不删除用户工作区中的既有修改。
- 不自动迁移或恢复真实 Codex 历史。
- 不输出、记录或提交 `auth.json`、API Key、bearer token、完整会话正文或备份内容。
- 不执行 `git reset --hard`、`git checkout --` 等破坏性回滚。
- 用户未授权前，不提交、不推送、不创建 PR、不安装正式版。

建议接手提示词：

```text
先阅读 AGENTS.md、docs/handoff/windows-wsl-provider-customization-status-2026-07-23.md、
CONTEXT.md、ADR-0001 和 ADR-0002。只做审查，不修改真实 Codex 历史，不提交或创建 PR。
以 upstream/main 为基线，先核对当前 dirty worktree，再审查稳定 custom 投影、WSL 原子写和
Target-aware 历史迁移/恢复。修改任何符号前必须先运行 GitNexus impact。
```

