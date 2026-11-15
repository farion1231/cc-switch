# AtomCode 适配分支 · 代码问题修复状态

- **生成时间**：2026-11-15 23:47（GMT+8）；**状态更新**：2026-11-16 00:50（见下）
- **代码状态**：生成时 HEAD `fdc674f6`（2026-11-15 23:02）
- **⚠️ 2026-11-16 状态更新**：HEAD 已前进至 **`6f91d55b`**（2026-11-16 00:24），本文两大「未修」口径已变化——**§2 的 #11 失焦丢值已修复闭环**（`keepDraftOnBlur` + `normalize` + 红框绑定，含 2 个回归测试，实测 6/6 + 全量 3826 全绿），**§3 功能缺口 #1 的 legacy `[providers.*]` 导入已闭环**（只读投影，不写盘，新 schema 优先）。新发现 1 个 P3：`cargo fmt --check` 3 行超宽在本提交引入——**已由 `15efee15`（2026-11-16 00:55，纯格式）修复闭环**，fmt ✅ + 15/15 tests 实测。详见主报告 **§18**。正文保留生成时快照，不回写
- **方式**：只读核对（`git status` / `git log` / 源码定位 / 历史实测数据），未改任何源码
- **相关文档**：判定口径以 [`2026-11-13-atomcode-pr-review-verification-zh.md`](./2026-11-13-atomcode-pr-review-verification-zh.md) **§17** 为准；未闭环事项详见 [`2026-11-15-atomcode-outstanding-issues-zh.md`](./2026-11-15-atomcode-outstanding-issues-zh.md)。本文只回答「代码问题都改了吗」，按**已修 / 未修 / 非代码**重新组织
- **一句话结论**：**不能说都改了** —— 被评审点出、本分支引入的代码 bug 只剩 #11 一个没修；但另有 3 项「要写代码却没写」的功能缺口与一批既有工程债

---

## 0. 结论速览

| 层次 | 数量 | 说明 |
|---|:--:|---|
| 已修（本分支引入 / 评审指出的代码 bug） | 15 类 | 见 §1，均带回归测试或实测证据 |
| **未修的用户可见 bug** | **1** | #11 模型键输入框失焦丢值，见 §2 |
| 要写代码但没写的功能缺口 | 3 | #9 官方格式导入、#4 Skills / MCP 接线、#1 预设一致性，见 §3 |
| 既有工程债（非本分支引入） | P2 5 类 + P3 若干 | 见 §4 |
| 非代码问题（不在本文范围） | 4 类 | 截图、PR 描述勾选、贡献者说明、真机验证，见 §5 |

---

## 1. 已修的代码问题

### 1.1 评审直接指出的主体问题

| 评审项 | 修复位置 | 验证 |
|---|---|---|
| #5 `App.tsx` 一行没改 | `App.tsx` 已有 9 处 atomcode 分支（`:879/1132/1154/1308/1328/1339/1692/1705/1721`），覆盖切换 / 添加 / 移除 / 复制 / 目录设置 | 相对基线 +33/-10 |
| #6 isInConfig 默认返回 `true` | `ProviderList.tsx:187-215` 改为穷举 switch；atomcode 与 kimi / dsh / mcode 同走 `meta.liveConfigManaged`，`default → false` | 前端测试 |
| #7 设为默认 / 从配置移除白名单 | 核心动作白名单补齐；`5e6d6156` 把 kimi 专属硬编码同步到 atomcode（`mutations.ts` 三处错误翻译、onSettled 重读、`App.tsx` env 冲突跳过列表） | 回归测试 |
| #8 后端取当前供应商语义 | `atomcode_config.rs:73-84` 改为 `["default_model","default_provider"].find_map(...)`，`default_model` 优先、回退 legacy，解析顺序对齐 AtomCode `effective_model_selection()` | 4 个回归测试；**注**：未设置选择时仍返回空串，属合理降级 |
| #9 删改默认模型导致悬空 | `atomcode_config.rs:312-326` `apply_provider` 校验新 `models` 仍含 `default_model`，删除与改名一并拒绝 | `cannot_delete_or_rename_default_model` 测试 |
| #10 更新路径先写库 | `atomcode_config.rs:382` `save_and_commit` 严格「先写 `config.toml`、成功后才提交 db」 | 4 条写序回归测试；架构层启动 live→db 单向覆盖窗口为项目级既有设计，非本分支引入 |
| #11 失焦（部分） | `AtomCodeProviderForm.tsx:234-243` 用稳定 `rowIds` 消除「每输入一字符失焦」；`:172-178` `update()` 统一清 `keyConflict`；`:604` 补 JSON 编辑路径清冲突 | 前端测试 `keeps the model key input mounted when the key is renamed` |

### 1.2 后续增量修复（§12–§17 逐轮复核催生）

| 问题 | 修复 | 提交 |
|---|---|---|
| 工具安装 / 卸载 / 升级、目录设置、`AppsPage` 的 `Partial<Record>` 容忍缺失 | `VALID_TOOLS` 14→15，各生命周期表接入 atomcode；`atomcode_config_dir` 覆盖全链路；`AppsPage.tsx:112-116` 恢复 `Record<AppId, Row>` | `5395992b` |
| 官方脚本安装无法自动卸载 | `tool_uninstall.rs:434-440` 补官方安装后缀 + 正例 / 强负例；`misc.rs` 补探测路径 | `c16ca796` |
| 并行 env 竞态、`dead_code` 告警 | `atomcode_config.rs:662-678` `AtomcodeHomeGuard(Option<OsString>)` 保存并恢复原值；两个测试加 `#[serial]` | `214ba557` |
| 后缀过宽误删、缺 Windows 门控、i18n 缺口 | 后缀收紧为 `/appdata/local/...`；`misc.rs:2763` 加 `#[cfg(target_os = "windows")]`；`tr.json` 补 `browsePlaceholderDsh`、`ko/ru` 补 2 个 settings key | `214ba557` |
| MSYS / Git-Bash 官方目标被误删（P2 回归） | 加回 `/.local/bin/atomcode.exe`、注释标明 `install.sh` 的 MSYS 分支、补正例 | `fdc674f6` |
| 5 条后缀 5/5 正例 + 4 类强负例 | `tool_uninstall.rs:1068-1121` | `fdc674f6` 后经两路复核确认 |
| 验证链可信度 | 提交说明首次给出全量 `pnpm test:unit` / `cargo test` / `pnpm build:renderer` 结果并如实标注 flaky | `fdc674f6` |

> 实测（QA 独立）：默认并行 `CC_SWITCH_TEST_HOME cargo test --lib atomcode_` **2/2 次 `14 passed; 0 failed`**；`cargo test --lib --no-run -q` 仅剩 1 条既有告警 `agy_account/keychain.rs:74`，本提交新增 0 条；路径归一化模拟 MSYS 目标命中、`/opt` 与 `/usr/local/bin/atomcode.exe` 不命中。

---

## 2. 唯一未修的用户可见 bug：#11 模型键输入框失焦丢值

### 2.1 现象（§12.2 只读探针实测，至今可复现）

| 场景 | 输入后 | 失焦后 | 判定 |
|---|---|---|:--:|
| A. 把 `m2` 改成已存在的 `m1`（同时出现「该模型键已存在」） | `m1` | **`m2`** | 输入被静默打回旧键 |
| B. 改成合法新键 `m2new` | `m2new` | `m2new` | 正常，稳定 `rowIds` 生效 |
| C. 输入 `m1 中文` | `m1 中文` | `m1` | 非法字符拖到失焦才被吞 |

### 2.2 三处根因（2026-11-15 23:46 grep 复核，代码原样还在）

1. `src/components/ui/ime-safe-input.tsx:79-80` — `onBlur` 的 else 分支无条件 `externalValueRef.current = value; setDraft(value)`，把内部 draft 重置回父级值。
2. `src/components/providers/forms/AtomCodeProviderForm.tsx:225-232` — 重名冲突时 `setKeyConflict(true); return`，父级 `value={key}` 不变，于是 1 的重置对象就是旧键。
3. 同页两处不一致：模型键输入框 `:554-562` **未传 `normalize`**，只在 `onValueChange` 里手动 `normalizeKey(next)`；供应商键输入框 `:406-415` 传了 `normalize={normalizeKey}`。前者使 `ImeSafeInput` 的 draft 保留非法字符，直到失焦才被吞（场景 C），也呼应评审 #1「前端不要自做一套」。

### 2.3 建议修复方向

- 给模型键输入框传 `normalize={normalizeKey}`，让 draft 与父级 value 在输入时即保持一致（消除 C，并与供应商键输入框统一）。
- 冲突分支不要静默回滚：可保留用户输入并把冲突提示绑定到该行输入框（`aria-invalid` / 红框），或让 `ImeSafeInput` 支持「拒绝回滚」的 blur 行为；核心是**反馈即时**，而不是等失焦才跳回旧键。
- 补 A / B / C 三场景回归测试（现有 `AtomCodeProviderForm.test.tsx` 已有 B 的 DOM 节点稳定性用例，可扩展示例）。

> 这是**唯一一个纯前端、不受任何外部约束、能独立修掉的用户可见 bug**。

---

## 3. 要写代码但没写的功能缺口（被外部约束挡住）

| 项 | 代码现状 | 卡点 |
|---|---|---|
| #9 官方 `[providers.*]` 格式导入 | `atomcode_config.rs:68` 仍只 `get("provider_accounts")`；按官方文档配置的用户点「导入」拿不到任何账号 | 属代码功能缺失而非纯文档问题；需「支持哪一版配置格式」的产品决策，以及贡献者给出适配版本与源码依据 |
| #4 Skills / MCP 接线 | `app_config.rs:57`、`:197` 恒 `false`，`:219` 空分支且注释 "not wired yet"；`services/mcp.rs`、`services/skill.rs`、`deeplink/provider.rs` 仍 skip / 空分支 / unsupported | 需新增 `enabled_atomcode` 列 + 迁移；AGENTS.md 明确「不要改 `SCHEMA_VERSION`，此类 PR 会被自动关闭，需要时 @ 维护者」 |
| #1 预设一致性 | `presetGroups.ts` 中 atomcode 命中数 0；`atomcodeProviderPresets.ts` 仍为自建 517 行清单 | 未接入共享预设目录；MCP / Skills / deeplink 后端亦未接线 |

---

## 4. 既有工程债（非本分支引入，也未修）

**P2**

| 项 | 位置 |
|---|---|
| 4 个既有 clippy error，`--all-targets -- -D warnings` 仍 EXIT≠0 | `agy_account/keychain.rs:74`、`dsh_config.rs:2275`、`settings.rs:1730/1738`（均经 `git blame` 确认非本分支） |
| 全量检查客观仍非全绿 | `pnpm test:unit` 4/1 flaky、`cargo test` 5/2 flaky，集中在 `tests/integration/App.test.tsx` 超时 |
| settings override 未隔离 | `atomcode_config.rs:21` 优先读全局 `settings.atomcode_config_dir` 单例，guard 只管 `ATOMCODE_HOME` |
| `LOCALAPPDATA` 重定向漏判 | `tool_uninstall.rs:438` 硬编码 `/appdata/local/atomcode/atomcode.exe`，建议改用 `dirs::data_local_dir()` 精确比对父目录 |

**P3（节选）**：真机安装 / 卸载端到端验证始终缺位；`misc.rs:2763` 的 `#[cfg]` 异平台编译未验证；`misc.rs:11938` 的 Windows 分支测试未覆盖 `local_data.join("AtomCode")`；MSYS 正例未用真实 Windows 盘符路径；`tests/i18n/appIdsI18n.test.ts` 未覆盖 3 个 settings key；`AppsPage.test.tsx` 为间接守卫；`useDirectorySettings.test.tsx` 未覆盖目录变更 invalidate 链路。

---

## 5. 非代码问题（不在本文范围）

截图示例（0 张）、PR 描述正文默认全勾选与模板 `:32-36` 条件句、贡献者对适配版本 / 源码依据的正式说明、真机安装 / 卸载验证。详见未闭环清单文档 §1 / §3 / §5。

---

## 6. 结论与下一步

**精确说法**：「被评审点出来的代码 bug」只剩 #11 一个；但「代码层面的事」远没做完 —— §3 的 3 个功能 / 一致性缺口 + §4 的一批既有债。因此 **§17 的「建议有条件合并」仍成立**：合并阻塞已解除，剩余项应按性质分别转 PR 描述动作、独立 issue 或维护者决策。

**建议顺序**：

1. 修 #11（纯前端、小改动 + 回归测试）—— 唯一能立即消除的用户可见 bug。
2. PR 描述正文按模板逐项真实勾选、对 flaky 与 4 个既有 clippy error 写 N/A + 理由 —— #3b 唯一闭环路径。
3. #9 / #4 转独立 issue，分别向贡献者与维护者索取输入；#1 预设目录接入排期。
4. §4 的既有债按 P2 → P3 排期，不阻塞本分支。

---

## 7. 边界

本文为只读盘点，未修改任何源码；证据来自 2026-11-15 23:46–23:47 的工作区核对与核验报告 §12–§17 的历史实测。全量测试数字为提交说明自述（本轮未独立复跑）、真机验证与 PR 描述正文均未覆盖，详见未闭环清单文档 §3。判定与门槛状态以核验报告 **§17** 为权威口径。
