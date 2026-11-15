# AtomCode 适配分支 · 未闭环事项清单

- **生成时间**：2026-11-15 23:33（GMT+8）；**状态更新**：2026-11-16 00:50（见下）
- **代码状态**：生成时 HEAD `fdc674f6`（2026-11-15 23:02）
- **⚠️ 2026-11-16 状态更新**：HEAD 已前进至 **`6f91d55b`**。#12（含 #11 失焦丢值）已由该提交修复闭环并附回归测试，评审 12 条中**代码级问题全部解决**；新发现 1 个 P3：`cargo fmt --check` 3 行超宽（`atomcode_config.rs:790/:797/:802`）——**已由 `15efee15`（2026-11-16 00:55，纯格式）修复闭环**。详见主报告 **§18**。正文保留生成时快照，不回写
- **来源**：对 [`2026-11-13-atomcode-pr-review-verification-zh.md`](./2026-11-13-atomcode-pr-review-verification-zh.md) **§17 最终放行复核**的衍生盘点。**判定口径以该报告 §17 为准**，本文只回答「还有什么没修」，不另立判定
- **方式**：只读核对（`git status` / `git log` / 源码定位 / i18n JSON 统计），未改任何源码
- **一句话结论**：代码层面的三个阻塞（§16.4 的 MSYS 回归、构建门禁违规、并行 env 竞态）**已全部闭环**；剩余为「评审意见未闭环 6 项 + 明确工程债 + 需外部输入」，**无遗漏的新回归**

---

## 1. 评审意见仍未闭环（统计 `⚠️2 / ❌4`）

| # | 判定 | 未闭环内容 | 当前证据（2026-11-15 23:33 核对） |
|---|:--:|---|---|
| 3a | ❌ | 缺少截图示例 | 分支至今 0 张图片；`docs/images` 无 atomcode 目录；PR 模板「修改前 / 修改后」截图表为空 |
| 3b | ❌ | 检查清单默认全勾选、条件提示不可信 | `.github/pull_request_template.md:32-36` 条件句**全部仍在**；PR 描述正文默认全勾选（仓库内不可验证）。提交说明已如实标注 flaky，只把「证据加重」缓解为「部分缓解」，判定未变 |
| 4 | ❌ | 需要考虑已有的 Skills / MCP 支持 | `app_config.rs:57`、`:197` 恒 `false`，`:219` 空分支且注释仍写 "not wired yet"；`services/mcp.rs:401-403,465-467` 仍 skip、`services/skill.rs:685` 空分支、`deeplink/provider.rs:413` unsupported。接线需 `enabled_atomcode` 列 + 迁移，AGENTS.md 要求改 schema 时 @ 维护者 |
| 10 | ❌ | 官方配置格式导入 / 版本与源码出处 | `atomcode_config.rs:68` 仍只 `get("provider_accounts")`；按官方 `[providers.*]` 配置点「导入」拿不到任何账号；贡献者对适配版本、配置格式源码依据的正式说明仍缺（`de4296a6` 已把 v5.2.7 / commit / 6 处源码行号写入集成文档，但不等于 PR 里的正式确认） |
| 12 | ⚠️ | 模型键输入框失焦丢值 | `ime-safe-input.tsx:67-83`（尤其 `:79-80` 无条件 `setDraft(value)`）+ `AtomCodeProviderForm.tsx:230` 重名冲突时 `setKeyConflict(true); return`；且模型键输入框 `:558` 未传 `normalize`、供应商键输入框 `:410` 传了 `normalizeKey`，同页两处不一致。A（冲突改键失焦被回滚）/ C（非法字符失焦才被吞）两类现象原样可复现 |
| 1 | ⚠️ | CLI 接入一致性 / 前端不要自做一套 | `presetGroups.ts` 中 atomcode 命中数 0；`atomcodeProviderPresets.ts` 仍为自建 517 行预设清单；MCP / Skills / deeplink 后端未接线。工具安装·卸载·升级、目录设置、`Record<AppId, Row>` 断言已补齐，但上述三项残留未动 |

> 已闭环的 7 项：#2 多语言（按 AGENTS.md 口径）、#5 App.tsx、#6 isInConfig、#7 白名单、#8 current_provider、#9 默认模型删改保护、#11 更新路径写序。

---

## 2. 工程债（本轮复核发现，明确未修）

### 2.1 P2 — 建议合并前或紧随其后处理

| 项 | 位置 / 现状 | 影响 |
|---|---|---|
| settings override 未隔离 | `atomcode_config.rs:21` 优先读全局 `settings.atomcode_config_dir` 单例，`AtomcodeHomeGuard` 只管 `ATOMCODE_HOME` | 本机 settings 含该 override 时测试会读真实目录；按 AGENTS.md 必须用 `CC_SWITCH_TEST_HOME` 跑，属理论残留。彻底方案：复用 `agy_config.rs:610-632` 的 `HomeGuard` 或 `app_config.rs:1217-1262` 的 `TempHome` |
| `LOCALAPPDATA` 重定向漏判 | `tool_uninstall.rs:438` 硬编码 `/appdata/local/atomcode/atomcode.exe` | 组策略 / 用户把 `LOCALAPPDATA` 重定向到不含该段的路径时，官方 `install.ps1` 安装会回退到「无法自动卸载」。建议改用 `dirs::data_local_dir()` 精确比对父目录 |
| 4 个既有 clippy error | `agy_account/keychain.rs:74`、`dsh_config.rs:2275`、`settings.rs:1730/1738`（均非本分支引入，经 `git blame` 确认） | `cargo clippy --all-targets -- -D warnings` 仍 EXIT≠0；本次改了 Rust，模板 `:34` 必填却未标注 |
| 全量检查客观仍非全绿 | `pnpm test:unit` 4/1 flaky、`cargo test` 5/2 flaky，集中在 `tests/integration/App.test.tsx` 超时 | 已如实标注且与 atomcode 无关，但模板措辞为 "passes"，严格只能写 N/A + 理由 |
| 门槛 ③ 形式项 | N/A + 理由写在 commit message，而非 PR 描述正文；模板 `:30/:31/:34/:36` 未逐项标注 | §17.3 判为 ⚠️ 部分（由 ❌ 升一级），与 #3b 维持 ❌ 不矛盾（载体不同） |

### 2.2 P3 — 可排期

- 真机安装 / 卸载端到端验证**始终缺位**：MSYS 修复目前只有静态 + 单测 + 路径归一化模拟证据。
- `misc.rs:2763` 的 `#[cfg(target_os = "windows")]` 加在 `if let` 语句上，Linux / macOS 下语法合法性未实测。
- `misc.rs:11938` 的 `build_tool_search_paths_includes_standalone_installer_dirs` 只断言 codex / claude / gemini，**未覆盖 `local_data.join("AtomCode")`**。
- MSYS 单测正例在 POSIX `tempfile::tempdir()` 下构造，未用含反斜杠的真实 Windows 盘符路径。
- 自定义 `ATOMCODE_PREFIX` 排除项、`/.local/bin/atomcode.exe` 的「错放目录」专门负例仍缺。
- `tests/i18n/appIdsI18n.test.ts` 只校验 5 核心语言的 `apps.<id>`，未覆盖 3 个 settings key。
- `AppsPage.test.tsx` 为间接守卫（`ROW_BY_APP` 未导出、不校验 Row 值）；`useDirectorySettings.test.tsx` 未覆盖目录变更 `invalidate(["providers","atomcode"])` 链路。
- `std::env::set_var` 在 Rust 2024 将变 `unsafe`（当前 `Cargo.toml:8` 为 edition 2021），可借 guard 重构一次收敛。

### 2.3 按约定豁免（不算缺陷）

ko / ru 的 `atomcode.*` 各 **1/23**（缺 22 个，唯一 key 为 `form.modelKeyDuplicate`）；`ru.json` 的 `browsePlaceholderDsh` 已由 `214ba557` 补齐。AGENTS.md 与 PR 模板均允许 ko / ru 走 `fallbackLng: "en"`，5 核心语言 zh / zh-TW / en / ja / tr 均 23/23。

---

## 3. 复核本身的盲点（这些地方可能还藏着问题）

1. **全量测试未独立复跑**：`pnpm test:unit` / `cargo test` / `pnpm build:renderer` / `cargo clippy --all-targets` 的数字来自提交说明，仅与 §14.4 的历史实测（同样在 `App.test.tsx` 波动失败）交叉印证，未由本轮 QA 重新执行。
2. **未做真机 Git Bash 安装 / 卸载**：`~/.local/bin/atomcode.exe` 的恢复未经端到端验证。
3. **官方脚本取证不完整**：本轮联网只重取了 `install.sh`；`install.ps1` 依据沿用 §13.3 历史取证，`/.atomcode/bin/atomcode` 只有 FAQ 弱来源（已在代码注释中如实标注）。
4. **未验证 PR 描述正文**（GitHub 渲染的复选框，仓库内不可验证）——这正是 #3b 的核心判据。
5. 未验证 `LOCALAPPDATA` 重定向、异平台 `#[cfg]` 编译，未对 `upstream/main` 做基线对照。

---

## 4. 报告口径勘误（建议修正，主报告当前未改）

§17.6 / §17.7 写「模板 `:32-36` **四处**条件句」，与实际不符：grep 显示 `:32-36` **5 行都带条件句** —— `:32` `pnpm test:unit`（如修改了前端代码）、`:33` `cargo fmt --check`、`:34` `cargo clippy`、`:35` `cargo test`（以上四条为「如修改了 Rust 代码」）、`:36` i18n「如修改了用户可见文本」。

建议改为「**5 处（:32-36）**」或「**4 条代码条件句 + 1 条 i18n 条件句**」。原表述大概只数了 `:32-35`，却把范围写成 `:32-36`，属范围与计数不一致，不影响 #3b 的 ❌ 判定。

---

## 5. 唯一能改变判定的低成本动作

其余未闭环项要么需要真机 / 维护者 / 贡献者输入，要么是明确的工程债；**只有 #3b 可以用几乎零成本改变判定**：

1. PR 描述正文按模板逐项真实勾选、**去掉默认全勾选**；
2. 对 `App.test.tsx` 的 flaky 与 4 个既有 clippy error 写明 **N/A + 理由**；
3. 提交说明补 `cargo clippy --all-targets -- -D warnings` 的 4 个既有 error 说明，并补齐模板 `:30/:31/:34/:36` 四项的通过或 N/A 标注。

完成这三条后，§14.7 门槛 ③ 可由 ⚠️ 升 ✅、#3b 具备闭环条件（其余 5 项仍需外部输入，建议转独立 issue，不再阻塞本分支）。

---

## 6. 边界

本文为**只读盘点**，未修改任何源码 / 配置；证据来自 2026-11-15 23:33 的工作区核对与核验报告 §12–§17。判定与门槛状态以 [`2026-11-13-atomcode-pr-review-verification-zh.md`](./2026-11-13-atomcode-pr-review-verification-zh.md) **§17** 为权威口径；本文列出的 P2 / P3 与 §17.6、§17.7 一致，仅按「未修复」视角重新组织并补入口径勘误。
