## Context

**合并时间点记录（不归档，供下次增量合并参考）**：
- **第一轮合并**：2026-07-27，`878c26f3` → `934a2d03`，7 个提交，自动合并无冲突
- **第二轮合并**：2026-07-29，`934a2d03` → `87b0e3fb`，17 个提交，涉及安全修复/deeplink/models.dev/CI
- **下次增量合并**：从 `87b0e3fb` 开始 fetch github/main 新提交，再次执行 `/opsx-apply`

---

### 第一轮合并（已完成）

当前 `guoddt/main` 分支（本地 `main`，用于添加 CodeFree-O 功能的 PR 分支）已完成 CodeFree-O 全栈集成，但落后于上游 `github/main` 分支 7 个提交。合并基点为 `878c26f3`，上游新增提交包括 presets 升级、Claude Opus 5 pricing、AICoding partner 恢复、Cloudflare R2 release mirror 等。第一轮自动合并无冲突。

### 第二轮合并（进行中）

第二轮合并范围：`934a2d03` → `87b0e3fb`，17 个提交，71 文件变更，+7470/-728 行。上游提交清单：

1. `87b0e3fb` - fix(test): pin zip extraction temp dir
2. `b33d300d` - docs(security): threat model
3. `245d180c` - docs(user-manual): deeplink usageEnabled
4. `cfa90f39` - fix(deeplink): import usage scripts
5. `19bf236e` - fix(deeplink): URL-safe Base64 decode
6. `a443eae9` - fix(deeplink): MCP args/env risk
7. `6dbb944b` - feat(deeplink): risk classification
8. `cd17912f` - fix(config): Object.prototype
9. `134bdc0e` - docs(sessions): renderer trust boundary
10. `35486afd` - fix(sessions): POSIX single-quote escaping
11. `c98913df` - fix(database): cross-file SQL statements
12. `993077c6` - chore(presets): AIGoCode domain
13. `12b972a6` - feat(usage): models.dev pricing sync
14. `ff3bc242` - fix(Security): zip-slip + credential leaks + panic paths
15. `ccda04bf` - chore(presets): AICodeMirror/AICoding domains
16. `2b2f2cfa` - ci: Cloudflare R2 mirror
17. `708b3879` - ci: sha256 download manifest

**重点冲突区域**：
- `services/skill.rs`（+1583 行）：zip-slip 修复 + skill 安装加固，codefree skills 逻辑可能冲突
- `services/provider/mod.rs`（+713 行）：provider 安全修复
- `mcp/codex.rs`（+175 行）：MCP 安全修复
- `commands/usage.rs`（+127 行）：models.dev 定价同步
- `i18n/locales/*.json`（+52 行/语言）：新增翻译 key
- `tauri.conf.json`：updater 配置

当前分支的 codefree 集成涉及 5 个能力域：session-usage、skills-mcp、frontend-ui、version-check、session-manager。合并时需确保这些能力不被上游变更破坏。

## Goals / Non-Goals

**Goals:**
- 将 `github/main` 的 17 个新提交（第二轮）合并到当前分支
- 解决所有合并冲突，同时保留 codefree 功能和上游新功能
- 确保合并后 codefree 的 5 个能力域功能完整
- 确保上游新功能（安全修复、deeplink 风险分类、models.dev 定价同步、CI mirror）正常工作
- 在版本号中添加考生姓名标识：`3.18.0` → `3.18.0-郭孔泉`
- 通过全部质量门禁：cargo fmt --check / cargo clippy / pnpm typecheck / pnpm format:check
- 重新打包 exe 验证

**Non-Goals:**
- 不修改 codefree 的功能逻辑（仅解决冲突）
- 不修改上游新功能的实现
- 不新增 codefree 功能或能力
- 不处理 OpenSpec specs 目录的冲突（openspec/changes/archive 和 openspec/specs 目录的删除是上游预期行为，因为上游不包含 codefree specs）
- 不归档本变更（仅标记合并时间点，供下次增量合并参考）

## Decisions

### 决策 1：合并策略 - 使用 `git merge` 而非 `git rebase`

**选择**：`git merge github/main`（在当前 `main` 分支即 `guoddt/main` 上执行）
**理由**：当前分支已有大量 codefree 提交且已推送 PR，rebase 会改变所有提交 hash，导致 PR 历史混乱。merge 保留完整历史，生成一个合并提交，便于回溯。
**备选**：`git rebase github/main` - 会导致 PR #5167 的所有提交 hash 变化，不利于 review。

### 决策 2：OpenSpec specs 目录冲突处理 - 保留 codefree specs

**选择**：合并冲突中保留 `openspec/specs/codefree-*/spec.md` 和 `openspec/changes/archive/2026-07-09-codefree-o-integration/` 目录。
**理由**：上游不包含 codefree 相关 specs，合并时 git 会尝试删除这些文件（因为上游没有）。这些文件是 codefree 集成的契约文档，必须保留。
**备选**：删除 specs 目录 - 会导致 codefree 集成失去规格文档支撑。

### 决策 3：schema.rs 冲突解决 - 以 github/main 为主，codefree 变更追加在新版本之后

**选择**：schema.rs 冲突时，以 `github/main` 分支代码为主体基础，codefree 的迁移变更作为新增部分追加在 github/main 的最新版本之后。
**具体规则**：
- 如果上游引入了新的 `SCHEMA_VERSION`（如 v17），则 codefree 的迁移链调整为：上游最新版本 → 新增 v(上游+1) 迁移添加 codefree 的 `enabled_codefree` 列等变更
- 如果上游未引入新版本（仍为 v16 或更低），则保持 codefree 现有的 v15→v16 迁移不变
- **绝对禁止**将 codefree 的迁移版本号插入到上游已有版本之间
- **绝对禁止**修改上游已有的迁移函数内容
**理由**：上游是主干，codefree 是分支新增功能。迁移链必须遵循"主干优先，分支追加"原则，确保上游迁移逻辑不被破坏，codefree 变更作为增量叠加。
**备选**：将 codefree 迁移插入上游版本之间 - 会导致上游迁移逻辑混乱，破坏版本顺序。

### 决策 4：i18n 合并策略 - 手动合并保留双方 key

**选择**：i18n 文件（en/zh/zh-TW/ja）手动合并，保留 codefree 相关 key 和上游新增 key。
**理由**：i18n JSON 文件容易产生合并冲突，自动合并可能丢失 key。手动合并确保双方 key 都存在。
**备选**：使用 `git merge -X ours` - 会丢失上游新增的翻译 key。

### 决策 5：presets 文件采用上游版本

**选择**：`src/config/*ProviderPresets.ts` 文件采用上游版本（theirs），codefree 不涉及 presets。
**理由**：codefree 不使用 provider presets（不支持代理），上游的 presets 升级（Opus 5、GPT-5.6 Sol 等）应完整保留。
**备选**：手动合并 - 不必要，codefree 不修改 presets 文件。

### 决策 6：前端表单组件冲突 - 逐文件评估

**选择**：`src/components/providers/forms/*.tsx` 文件逐个评估，保留 codefree 的条件分支（如有）和上游的组件调整。
**理由**：上游可能调整了表单组件的通用逻辑，codefree 可能在某些组件中添加了条件分支。需逐文件确认。

### 决策 7：全局冲突解决原则 - 以 github/main 为主，codefree 作为新增追加

**选择**：所有冲突文件统一采用"github/main 为主，codefree 追加"策略。
**具体规则**：
- 冲突代码块中，优先保留 `github/main` 分支的代码内容
- codefree 相关的代码作为新增部分，追加在 github/main 代码之后
- 对于结构体/枚举定义：github/main 的字段在前，codefree 新增字段在后
- 对于函数定义：github/main 的函数保留原位，codefree 新增函数追加在文件末尾或相应模块末尾
- 对于配置/常量：github/main 的值为基础，codefree 新增的常量/配置追加在后
- 对于 i18n JSON：github/main 的 key 为基础，codefree 新增 key 追加在同一层级末尾
**理由**：上游是主干，codefree 是分支新增功能。保持主干代码的原始结构和顺序，codefree 作为增量叠加，便于未来上游同步和 codefree 维护。
**备选**：混合排列 - 会导致代码结构混乱，难以区分主干和分支变更。

### 决策 8：第二轮版本号标识 - 添加考生姓名

**选择**：在 `package.json` / `tauri.conf.json` / `Cargo.toml` 三处将版本号从 `3.18.0` 修改为 `3.18.0-郭孔泉`。
**理由**：考题要求考生在版本号中标识姓名，便于验收时确认交付物归属。
**备选**：仅在 package.json 修改 - 不一致，三处版本号应保持同步。

### 决策 9：第二轮 skill.rs 冲突处理 - 保留上游安全加固，codefree skills 逻辑追加

**选择**：`services/skill.rs`（+1583 行）冲突时，完整保留上游的 zip-slip 修复和 skill 安装加固逻辑，codefree 相关的 skills 逻辑作为新增追加。
**理由**：zip-slip 是安全漏洞，必须完整修复。codefree skills 逻辑是新增功能，不能破坏上游安全修复。
**备选**：部分保留 - 安全修复必须完整，不能部分保留。

### 决策 10：第二轮 tauri.conf.json 冲突处理 - 保留上游 updater 配置 + codefree 修改

**选择**：`tauri.conf.json` 冲突时，保留上游新增的 updater 配置，同时保留 codefree 对该文件的修改（如有），版本号统一改为 `3.18.0-郭孔泉`。
**理由**：updater 配置是 CI R2 mirror 功能的基础，必须保留。版本号修改是考题要求。
**备选**：仅保留上游 - 会丢失 codefree 修改。

## Risks / Trade-offs

- **[风险] schema.rs 合并冲突可能导致迁移链断裂** → 缓解：合并后运行 `cargo test` 验证所有迁移测试通过
- **[风险] i18n key 遗漏导致翻译缺失** → 缓解：合并后用 `pnpm typecheck` 和手动检查确认所有 key 存在
- **[风险] 上游表单组件调整与 codefree 条件分支冲突** → 缓解：逐文件 review，确保 codefree 分支逻辑保留
- **[风险] CI release mirror 配置可能与本地构建冲突** → 缓解：CI 配置不影响本地构建，仅影响 GitHub Actions
- **[风险] 测试文件冲突导致测试失败** → 缓解：合并后运行全部测试套件验证
- **[权衡] merge 策略生成合并提交，PR 历史不如 rebase 干净** → 可接受，保留完整历史更有利于回溯

## Migration Plan

1. **合并前准备**：确保工作区干净，无未提交变更
2. **执行合并**：`git merge github/main`
3. **解决冲突**：按上述决策逐文件解决
4. **验证**：运行全部质量门禁（cargo clippy、cargo test、pnpm typecheck、pnpm test:unit、pnpm format:check）
5. **打包验证**：`npx tauri build` 生成 exe
6. **提交合并**：用户手动提交合并结果
7. **回滚策略**：如合并失败，`git merge --abort` 回到合并前状态

## Open Questions

- 上游 schema.rs 的 11 行新增具体内容是什么？是否影响 v15→v16 迁移？（需在合并时确认）
- 上游 `src/types/omo.ts` 的类型调整是否与 codefree 的 AppType 定义冲突？（需在合并时确认）
