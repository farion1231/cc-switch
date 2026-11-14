# AtomCode 适配 PR 评审意见逐条核验报告

- **核验对象**：分支 `feat/atomcode-provider`
  - **主体核验快照**：HEAD `7b5b5771`（§2–§10 的判定与证据以此为准）
  - **最后一次源码变更**：`5e6d6156`（2026-11-14 08:59，`fix(atomcode): sync kimi-specific hardcodes and harden i18n consistency`；触及 App.tsx / mutations.ts / errorUtils.ts / AtomCodeProviderForm.tsx 源码 + 5 核心语言 i18n + PR 模板 + 新增 i18n 回归测试）
  - **增量修订复核**：`5843fe16`（QA 对 `8941b0ae` 独立回归核验；判定统计按该代码状态更新）
  - **当前报告版本**：同步至 `5e6d6156` + 未提交工作区改动（2026-11-15 00:15 独立复核增补 §12）；工作区另有 `docs/atomcode-integration.md` 重写（v5.2.7 源码依据落地）、`atomcode_config.rs` 仅文档注释，均未入库
- **对照基线**：`upstream/main` = `bd487e83`（含 GitHub Copilot CLI 支持 `#8753`）
- **核验日期**：初版 2026-11-13 20:24；增量修订 21:05；计数勘误 21:41；版本同步 2026-11-14 00:34；`5e6d6156` 同步 2026-11-14 22:06；**独立复核（§12）2026-11-15 00:15**；**§0 评审意见原文收录 2026-11-15 00:29**
- **独立复核方式**：以当前工作区（HEAD `5e6d6156` + 3 个未提交文件）为准只读复核；对评审 #12 另建临时探针测试实跑、随即删除（`git status --short` 首尾一致，仍为原有 3 个文件）；**未修改任何源码**，本次只增补本报告（详见 §12）
- **核验方式**：主体核验为只读。源码逐行核对 + `git diff upstream/main...HEAD` + 前端/后端实测 + 联网核实 AtomCode 官方文档；核验阶段未修改任何源码（`git status --short` 首尾均为空）。报告由 `5843fe16` 入库、`52108a48` 收录 §11 与勘误、`5e6d6156` 附带微调，修订提交均**只编辑本报告文件**；当前工作区另有本报告未提交改动（见 §11.8）
- **协作方式**：三路并行核验后交叉复核 —— 代码事实（9 条）、架构一致性 / Skills·MCP / 出处 / SSOT、i18n / 截图 / 检查清单 / 测试，主理人汇总并对关键结论独立复验；增量修订由 QA 对 `8941b0ae` 独立回归核验后汇编
- **关联文档**：[2026-11-11 代码审查](./2026-11-11-code-review-211aff9a.md)、`../atomcode-fixes-summary.md`、`../atomcode-integration.md`、`../atomcode-provider-integration-zh.md`
- **核验输入**：评审意见原文由用户于 2026-11-15 提供，逐字收录于 **§0**；此前报告所述「仓库三份文档中无该评审原文」指仓库内文档，该空白现已消除

> **修订说明（重要）**：本报告主体是对 `7b5b5771` 的快照核验。其后分支依次为 `8941b0ae`（源码修复：补 i18n、移除未接线类型声称）、`5843fe16`（报告入库）、`52108a48`（§11 增量修订与计数勘误入库）、`5e6d6156`（源码修复：同步 kimi 专属硬编码、加固 i18n 一致性、PR 模板诚实化）。**`5e6d6156` 是 `8941b0ae` 之后又一次源码变更**，使 §1 判定统计与 §2 中 #7 的判定相对 `52108a48` 发生变化（见 §11.8）；工作区另有未提交改动（`docs/atomcode-integration.md` 重写、`atomcode_config.rs` 文档注释、本报告自身），均未入库。需区分三个时点：**判定结论原始快照 `7b5b5771`、源码状态 `5e6d6156`、报告版本同步至 `5e6d6156` + 工作区**。§1 判定统计与 §2 中 #2 / #4 / #7 的判定见 §11.3 与 §11.8 的更新口径；§3–§10 保留原始快照证据并在受影响结论处标注「已修订，见 §11」。完整增量差异见 **§11**；2026-11-15 的只读独立复核与 #12 失焦实测见 **§12**；评审意见原文见 **§0**。

---

## 0. 核验输入：评审意见原文

> **来源**：用户于 2026-11-15 00:15 提供的 PR 评审意见全文，为本报告 12 个判定项的输入。此前报告所述「仓库三份文档中无该评审原文」（原 §4.4、§9 P1#8）指**仓库内**文档，现由本节收录后该空白消除。以下逐字转录，编号与 §2 逐条结论表一一对应；#3 原文为一句，报告按「截图 / 检查清单」拆为 3a、3b；#5–#8 原文为同段连写，此处按语义拆行以便对照；**#12 句尾省略号为用户原文所有，未作补全**。

| # | 评审意见原文 |
|---|---|
| 1 | 需要新增 CLI 类型的支持，请参考项目内已有的实现示例，而不是仅指定几个供应商，前端实现不要自做一套，而是参考已有的应用设计，确保一致性 |
| 2 | 多语言支持缺失，请补全 |
| 3a / 3b | 缺少截图示例，测试检查清单默认全勾选仍保留"如修改了rust代码"的提示，不可信 |
| 4 | 需要考虑已有的 Skills / MCP 支持 |
| 5 | App.tsx 一行都没改。 |
| 6 | ProviderList.tsx 判断"在不在配置里"时，AtomCode 走默认分支直接返回 true。 |
| 7 | "设为默认"和"从配置移除"的白名单都没有 atomcode。 |
| 8 | 后端取当前供应商的函数对它返回空。 |
| 9 | 编辑当前账号时，删掉或改名默认模型，会让 AtomCode 的默认模型指向不存在的条目。 |
| 10 | 官方文档只写了 [providers.*] 加 default_provider，接口类型只有 openai/claude/ollama 三种（配置文件文档）。PR 读写的 provider_accounts 格式只在第三方集成里出现过（magpie #736、dsh-connect-atomcode）。所以按官方文档配置的用户点"导入"什么都拿不到。deepseek/gemini/openai_responses 这几种类型和 ATOMCODE_HOME 环境变量也查不到出处。应请贡献者说明适配的 AtomCode 版本，并给出源码依据。 |
| 11 | 更新路径先写库的问题会在下次启动时被配置覆盖 |
| 12 | 模型键输入框失焦... 你检查一下 |

**转录口径**：`[providers.*]` 等方括号内容按原文保留（表格中未构成 Markdown 链接）；`"如修改了rust代码"` 的大小写与空格照录原文，报告正文与 §6.3 按模板实际文案 `（如修改了 Rust 代码）` 引用，二者差异不作更正。逐条判定的当前状态见 §1 / §2，硬证据见 §3–§7，2026-11-15 独立复核见 §12。

---

## 1. 结论摘要

> **评审意见仍未全部满足。** 代码状态 `5e6d6156` + 当前工作区下，13 个判定项中闭环 **6 项**、部分满足 **3 项**、未满足 **4 项**。演进：初版（`7b5b5771`）为 ✅4 / ⚠️5 / ❌4；`8941b0ae` 使「多语言」由 ⚠️ 升 ✅（→ ✅5 / ⚠️4 / ❌4）；`5e6d6156` 使「白名单 kimi 硬编码同步」由 ⚠️ 升 ✅（→ ✅6 / ⚠️3 / ❌4）。**2026-11-15 独立复核（§12）以只读实测坐实 #12「模型键输入框失焦」仍可复现；评审意见全文已由用户提供并收录于 §0（#12 句尾仍为省略号），原句截断不再构成判定空白；判定等级与统计均不变。**

| 判定 | 数量（代码状态 `5e6d6156`） | 条目 |
|---|---|---|
| ✅ 已满足 | 6 | App.tsx 未改、`isInConfig` 默认分支、默认模型删改保护、更新路径写序、多语言补全、**白名单 kimi 硬编码同步（`5e6d6156`）** |
| ⚠️ 部分满足 | 3 | CLI 参考既有实现、`current_provider`、模型键失焦 |
| ❌ 未满足 | 4 | 截图、检查清单、Skills / MCP、官方文档格式·导入·版本出处 |

**可合并的核心判断**：

1. 上一轮 `docs/atomcode-fixes-summary.md` 声称"已修复"的**代码类**问题，绝大多数属实且有回归测试支撑；
2. 但**能力类**（Skills / MCP / 工具管理 / deeplink）、**交付物类**（截图）、**依据类**（适配版本、配置格式源码出处、官方格式导入）基本没有实质推进，部分被文档明确标注为"未接线 / 留待合并前补齐"；
3. 核验中**新发现一个本分支引入的用户可见缺陷**：`apps.atomcode` 在 7 个语言文件中全部缺失，界面会渲染裸 i18n key —— **该缺陷已于 `8941b0ae` 修复**（见 §11）；
4. `docs/atomcode-fixes-summary.md` 自身存在多处与源码不符的表述（详见 §7），PR 描述中"全部勾选"的检查清单因此缺乏可信基础；
5. `8941b0ae` 采取的是「移除前端类型声称」而非接线后端，消除了"前端可选、后端必拒"的矛盾，但 **Skills / MCP 能力整体仍未落地**，故按严格口径 #4 仍判 ❌（见 §11.3）。

---

## 2. 逐条结论表

| # | 评审意见 | 判定 | 一句话依据 |
|---|---|:--:|---|
| 1 | 新增 CLI 类型应参考项目内既有实现，而非只指定几个供应商；前端不要自做一套，确保一致性 | ⚠️ | 枚举 / 表单 / 呈现层 / 图标 / IPC 注册复用了既有扩展点；但工具安装卸载、目录设置、MCP/Skills、deeplink 缺口明显，预设清单为自建 517 行，`AppsPage` 用 `Partial<Record>` 容忍缺失代替接入 |
| 2 | 多语言支持缺失，请补全 | ✅ **（增量修订，原 ⚠️）** | 5 个核心语言 `atomcode.*` key 齐全（`atomcode.*` 22 + `apps.atomcode` = 23，`8941b0ae` 起；`5e6d6156` 后 5 核心语言达 24 个、ko/ru 23 个靠 `fallbackLng: "en"`，见 §7.1 口径说明），ko/ru 靠 `fallbackLng: "en"`（AGENTS.md 明确豁免）；`8941b0ae` 补齐 7 语言 `apps.atomcode`、把 `sharedProviders` 接入 `presentation.ts` 的 atomcode 分支、`empty.*` 接入 `ProviderEmptyState` 的 `emptyCopyNs`，裸 key 与 3 个死 key 全部消除并经 QA 实测（见 §11.2）；`5e6d6156` 另新增 `tests/i18n/appIdsI18n.test.ts` 防回归。剩余仅硬编码英文 preset label，属专有名词 / 既有惯例，低优先级 |
| 3a | 缺少截图示例 | ❌ | 分支 0 张图片、`docs/images` 无 atomcode 目录、PR 模板截图表为空，中文集成文档自承"留待合并前补齐" |
| 3b | 检查清单默认全勾选仍保留"如修改了 rust 代码"的提示，不可信 | ❌ | 核心判据指向 PR 描述正文（仓库内不可验证）。**修订**：`5e6d6156` 改了 `.github/pull_request_template.md`——新增「逐项真实勾选、不适用写 N/A + 理由」提示、列出 clippy 全 workspace 口径（`-D warnings`，勿只跑 `--lib`）、补 `pnpm test:unit` / `cargo fmt --check` / `cargo test`；但 `(if Rust code changed)` 条件提示**仍保留**（见 §6.3、§11.8）。`5843fe16`/`52108a48` 时期模板与基线一致、4 项 `- [ ]` 的结论对那时成立 |
| 4 | 需要考虑已有的 Skills / MCP 支持 | ❌ **（判定不变，依据已重写）** | `8941b0ae` 落实了 §9 P0-2 的「选项二」：删除 `McpApps.atomcode?` / `SkillApps.atomcode?` / `AppType` 的 atomcode、`deeplink.ts` 移除可选 app，并加 `skillAppEnabled()` 安全索引 → **"前端可选、后端必拒"的矛盾已消除**。但后端 `app_config.rs`、`dao/mcp.rs`、`services/mcp.rs`、`deeplink/provider.rs` 仍全部 `false` / `None` / `skip` / `unsupported`，`services/skill.rs:752` 仍半接线，且**无新增 schema 列 / 迁移 / 测试 / 明确不支持决策记录**。**仍证伪（`5e6d6156`）**：新增 `tests/i18n/appIdsI18n.test.ts` 显式断言 `skillAppEnabled({}, "atomcode") === false` 并注释「atomcode is not wired yet」，与 §6.1 后端恒 false 一致 → 严格口径维持 ❌（见 §11.3） |
| 5 | `App.tsx` 一行都没改 | ✅ | 相对基线 +33/-10（43 行），切换 / 添加 / 移除 / 复制四处 atomcode 分支全部在列 |
| 6 | `ProviderList.tsx` 判断"在不在配置里"时 AtomCode 走默认分支直接返回 `true` | ✅ | 改为穷举 switch，atomcode 与 kimi/dsh/mcode 同走 `meta.liveConfigManaged`，`default` 返回 `false` |
| 7 | "设为默认"和"从配置移除"的白名单都没有 atomcode | ✅ **（5e6d6156，原 ⚠️）** | 核心动作白名单已补齐并复用共存型逻辑；`5e6d6156` 把 kimi 专属硬编码同步到 atomcode：mutations.ts add/update/delete 三处接入 `translateAtomcodeProviderMutationError`、onSettled 重读由 `kimi` 扩为 `kimi \|\| atomcode`（`["providers", appId]`）、App.tsx 的 env 冲突跳过列表加入 `atomcode`、AtomCodeProviderForm 的 JSON 编辑器路径补 `setKeyConflict(false)`（见 §4.2、§4.4、§11.8） |
| 8 | 后端取当前供应商的函数对它返回空 | ⚠️ | `current_provider()` 现返回 account ID 而非模型键；但 `default_provider` 缺失 / 无 `models` 表时仍返回空串，且与 `default_model` 语义混用 |
| 9 | 编辑当前账号时删掉或改名默认模型，会让默认模型指向不存在的条目 | ✅ | `apply_provider` 校验新 `models` 仍包含 `default_model`，删除与改名一并拒绝；测试真实有效 |
| 10 | 官方文档只写 `[providers.*]` + `default_provider`、类型仅 openai/claude/ollama 三种；按官方文档配置点"导入"什么都拿不到；deepseek/gemini/openai_responses 与 `ATOMCODE_HOME` 查不到出处；请贡献者说明适配版本并给源码依据 | ❌ | 导入链只解析 `[provider_accounts.*]`，文档自列 Known Limitation；PR 文档引用非官方地址与占位域名、版本号查无出处、未给源码依据。**其中 2 处评审细节需修正，见 §5** |
| 11 | 更新路径先写库，会在下次启动时被配置覆盖 | ✅ | update 路径严格"先写 `config.toml`、成功后才提交库"，注释点明防回填覆盖，另有 4 条写序回归测试。⚠️ 架构层启动时 live→db 单向覆盖窗口仍在（项目级既有设计，非本分支引入） |
| 12 | 模型键输入框失焦…（原文见 §0 #12，句尾省略号为用户原文所有） | ⚠️ **（已坐实，原 ⚠️ / ❓）** | 稳定 `rowIds` 已修复"改键导致行重挂载失焦"，`update()` 统一清 `keyConflict` 修掉 P3 残留提示；`5e6d6156` 已补 JSON 编辑路径 `setKeyConflict(false)`（`AtomCodeProviderForm.tsx:604`），原「JSON 路径未清」子项闭环。**2026-11-15 只读探针实测坐实**：合法改键后失焦值保持（`m2new`），但改键与已有键冲突时输入 `m1` 在失焦后**被静默打回 `m2`**、含非法字符 `m1 中文` 在失焦后才被吞成 `m1`（§4.4、§12.2）。原句截断不再影响判定，具体现象已可复现 |

---

## 3. 已满足项的硬证据

### 3.1 `App.tsx` 已实质修改（评审 #5）

`git diff upstream/main...HEAD -- src/App.tsx` → `1 file changed, 33 insertions(+), 10 deletions(-)`：

```diff
-    } else if (app === "mcode" || app === "dsh" || app === "kimi") {
+      app === "mcode" || app === "dsh" || app === "kimi" || app === "atomcode"
...
-                : activeApp === "kimi"
-                  ? handleAddKimiProvider(provider)
+                : activeApp === "kimi" || activeApp === "atomcode"
+                  ? handleAddToLiveConfig(provider, activeApp)
...
-              activeApp === "omp"
+              activeApp === "omp" || activeApp === "atomcode"
...
-                : activeApp === "hermes" || activeApp === "kimi"
+                : activeApp === "hermes" || activeApp === "kimi" || activeApp === "atomcode"
```

当前文件真实位置：`src/App.tsx:1685-1691`（onSwitch → `handleAddToLiveConfig`）、`:1692-1704`（onRemoveFromConfig 白名单，`:1701` 为 atomcode）、`:1711-1720`（onSetAsDefault，`:1717` 走 `switchProvider`）、`:1300-1338`（复制时按全局唯一模型 key 重写 + `addToLive = false`）。

> 时间线：初始提交 `45372ee3` 的 `--stat` 中确实没有 `src/App.tsx`，`211aff9a` 起补齐——该评审意见针对早期提交，**当前 HEAD 已不成立**。

### 3.2 `isInConfig` 已穷举且默认返回 false（评审 #6）

`src/components/providers/ProviderList.tsx:185-215`：

```ts
switch (appId) {
  case "kimi": case "dsh": case "mcode": case "atomcode":
    return provider.meta?.liveConfigManaged === true;
  case "opencode": ... case "hermes": ... case "pi": case "omp": ...
  case "claude": case "claude-desktop": case "codex": case "gemini":
  case "grokbuild": case "copilot": case "agy":
    return true;
  default:
    return false;
}
```

显式列出 16 个 AppId，与 `src-tauri/src/app_config.rs:617` 的 allowed 列表一致。`78ed827e` 的 diff 显示 `default: return true` → 显式 case + `return false`。

### 3.3 白名单主体已补齐（评审 #7 的 ✅ 部分）

`src/components/providers/presentation.ts:726-784` 的 `buildAdditiveSections` 共用 Kimi 分支：

```ts
if (app === "kimi" || app === "atomcode") {
  ...
  if (!live) { buttons.push({ key: "add", ... }); }
  else {
    if (!current) menuItems.push({ key: "setDefault", ... });   // :753-759
    buttons.push({ key: "remove", disabledReason: current ? ... }); // :760-767
  }
}
```

其他在列的扩展点：`src/config/appConfig.tsx:143-166`、`src-tauri/src/app_config.rs:538-550`、`src-tauri/src/services/provider/mod.rs:162-165`（per-app 锁）、`:6640-6651`、`src/components/providers/forms/ProviderForm.tsx:341`。

### 3.4 默认模型删改保护（评审 #9）

`src-tauri/src/atomcode_config.rs:305-324`：

```rust
// Editing the current account must not delete or rename the default model.
if let Some(default_key) = document.get("default_model").and_then(Value::as_str) {
    if document["models"].as_object().unwrap().get(default_key)
        .and_then(|model| model.get("account"))
        .and_then(Value::as_str) == Some(id)
        && !config["models"].as_object().unwrap().contains_key(default_key)
    {
        return Err(AppError::InvalidInput(format!(
            "Cannot delete or rename the default model '{}' while it is selected", default_key)));
    }
}
```

测试 `src-tauri/src/atomcode_config.rs:725-755` `cannot_delete_or_rename_default_model` 三段断言：删掉 `default_model` → `is_err()` 且含 `"default model"`；改名为 `acct-key-renamed` → 同样 err（改名 = 删旧键 + 加新键，被 `!contains_key` 拦住）；编辑非默认账号并改名 → `.unwrap()` 成功。实测 `cargo test --lib atomcode_` 中该例 `ok`。

### 3.5 更新路径写序（评审 #11）

追踪链（**update/edit 路径，不只是 add/switch**）：`mod.rs:6464 update()` → `:6594 is_additive_mode()` → `:6640-6651` → `:5802-5819`：

```rust
if app_type == &AppType::AtomCode {
    // AtomCode 同 Kimi：先写 config.toml，成功后才提交库，live 写失败时库不动，
    // 否则下次启动的回填会用旧 live 配置覆盖刚保存的编辑。
    return if write_live {
        crate::atomcode_config::save_and_commit(&provider.id,
            Some(provider.settings_config.clone()), create,
            || { state.db.save_provider(app_type.as_str(), provider)?; Ok(true) })
    } else { state.db.save_provider(app_type.as_str(), provider)?; Ok(true) };
}
```

`src-tauri/src/atomcode_config.rs:386-409` → `src-tauri/src/mcode_config.rs:30-57`：`write()?` 先执行，`commit()` 后执行；`commit()` 失败还会回滚 live。不存在"先 `db.save_provider` 再写 live"的 atomcode 路径。回归测试：`mod.rs:1299-1345`（live 写失败 db 保持旧值）、`:1406-1449`（更新成功 / create / `write_live=false`）。

---

## 4. 部分满足项的残留缺口

### 4.1 CLI 接入一致性（评审 #1）

以 `bd487e83`（Copilot CLI）为项目内既有范例逐点对比，下表 18 个扩展点中 **8 项复用、6 项半接、4 项缺失**（deeplink 一行已按 `8941b0ae` 由 ❌ 调整为 ⚠️）：

| 扩展点 | 既有范例 | AtomCode @ HEAD | 判定 |
|---|---|---|:--:|
| AppType 枚举 / as_str / FromStr / all() / is_additive_mode | `app_config.rs` | `:504/525/546/581/608` | ✅ |
| `settings.rs` VisibleApps + current_provider 映射 | +10 | `:62/84/108/1511/1537` | ✅ |
| tray / AppGlyph / 用量标签 / RequestLogTable / UsageScriptModal | 均改 | 均改 | ✅ |
| 后端 config 模块（live 读写 + 原子写） | `copilot_config.rs` | `atomcode_config.rs`（763 行） | ✅ |
| `generate_handler!` 注册 + `src/lib/api/` 包装 + camelCase | +2 命令 | `lib.rs:1717/1946`、`model-fetch.ts`、`providers.ts` | ✅ |
| 专用 commands 模块 | `commands/copilot.rs` 等 | 仅复用 `model_fetch.rs` / `provider.rs` | ⚠️ |
| `appConfig.tsx` MCP_APP_IDS | 含 copilot | `Exclude<AppId, ... \| "atomcode">`（`:191-196`） | ⚠️ |
| 表单与预设目录 | 复用 Pi 的供应商目录 | `ProviderForm.tsx:339` 分发复用；`atomcodeProviderPresets.ts` 517 行自建，`presetGroups.ts` / `ProviderPresetSelector.tsx` 无 atomcode | ⚠️ |
| `presentation.ts` buildAdditiveSections | 既有 kimi 分支 | 复用（+5 行） | ✅ |
| ProviderList / App.tsx 白名单 | 既有 kimi 分支 | 已接 | ✅ |
| 图标 / i18n / 测试 | 均有 | 均有 | ✅ |
| 目录设置 | 后端 3 处已加 | 前端 `DirectorySettings.tsx:10-14` Exclude，实际仅靠 `ATOMCODE_HOME` | ⚠️ |
| **工具安装/卸载管理** | `TOOL_NAMES` / `TOOL_APP_IDS` / 安装脚本 / `tool_uninstall.rs` | **全无**；`AppsPage.tsx:112-115` 改 `Partial<Record<AppId, Row>>` + filter 容忍缺失（提交 `71137177` 自承） | ❌ |
| **MCP / Skills 后端** | schema 列 + 迁移 + `mcp/copilot.rs` + dao + sync | 全部 `false` / `None` / skip（见 §6.1） | ❌ |
| prompt files | 后端 + 前端各改 | 后端加 `AGENTS.md` 但注释"管理页暂不接入"，`promptUtils.tsx` 未加 | ⚠️ |
| **deeplink** | `build_settings` + `merge_config` + preview + dialog | 后端返回 `atomcode.deeplink_unsupported`；**已修订**：`8941b0ae` 从 `deeplink.ts` 的可选 app 中移除 atomcode，前后端一致，但能力仍未接线 | ⚠️（原 ❌） |
| 会话管理 / 用量 / 打开终端 / 额度卡 | 均有 | 未接（共存型未必全需） | ❌ |
| 文档 | README + 带实测依据的 integration-zh | 3 份 docs，无可验证版本/源码依据 | ❌ |

另有夹带无关改动：`src/components/usage/UsageTrendChart.tsx:221-238`、`src/hooks/useDragSort.ts:13-25`（locale 抽取），影响 diff 纯净度。

### 4.2 白名单的 kimi 专属硬编码（评审 #7 的 ⚠️ 部分，5e6d6156 已闭环）

~~`src/lib/query/mutations.ts:240-248` 与 `:323-331` 的 onSettled 仅 `kimi`~~ → **已修订（`5e6d6156`）**：`useAddProviderMutation` / `useUpdateProviderMutation` / `useDeleteProviderMutation` 三处把 atomcode 接入 `translateAtomcodeProviderMutationError`（`errorUtils.ts` 新增），且 onSettled 重读由 `appId === "kimi"` 扩为 `appId === "kimi" || appId === "atomcode"`（失效键从硬编码 `["providers", "kimi"]` 改为 `["providers", appId]`）；`App.tsx:876` 的 env 冲突跳过列表（`mcode|dsh|kimi|atomcode|agy|copilot`）补入 `atomcode`。AtomCode 的「默认模型正在被使用」后端报错经该翻译映射为 `atomcode.error.defaultModelInUse`（见 §3.4 删改保护、§11.8）。原 ⚠️ 三项子项全部闭环，#7 升 ✅。

### 4.3 `current_provider()` 仍可能为空且语义混用（评审 #8）

`src-tauri/src/services/provider/mod.rs:5743-5749` 已对 AtomCode 分流；`src-tauri/src/atomcode_config.rs:80-96`：

```rust
let default_model_key = match document.get("default_provider").and_then(Value::as_str) {
    Some(key) if !key.is_empty() => key,
    _ => return Ok(String::new()),
};
let account_id = document.get("models")
    .and_then(|models| models.get(default_model_key))
    .and_then(|model| model.get("account"))
    .and_then(Value::as_str)
    .unwrap_or_default();
Ok(account_id.to_string())
```

残留：① `default_provider` 缺失/空 → 空串；② `models` 无该键或无 `account` → 空串（官方 `[providers.*]` 格式无 `models` 表时**必空**）；③ 注释把 `default_provider` 当"模型键"，而官方文档把它描述为 `[providers.*]` 的 key —— **已修订（§11.7）：AtomCode 源码证实两者都对**，`default_provider` 是 legacy 字段（`[providers.*]` 键或新 schema 的 selection id），本分支写的模型键正是新 schema 的 selection id，双写行为与 AtomCode CLI 一致；④ `src-tauri/src/settings.rs:1514-1519` `get_current_provider` 对 AtomCode 返回 `None`、`:1540-1545` setter 是 no-op。

### 4.4 模型键输入与 keyConflict（评审 #12）

- **已修**：`AtomCodeProviderForm.tsx:234-243` 改键时 `rowIds` 跟着键走，`key={rowIds[key] ?? key}`（`:539`）保证行不重建、焦点不丢；`ImeSafeInput`（`:543-573`）处理中文输入法。
- **已修（P3）**：`:172-178` 的 `update()` 统一 `setKeyConflict(false)`，`removeModel` / `choosePreset` / `updateModel` 成功路径都走它。
- ~~**未修**~~ → **已修（`5e6d6156`）**：`:600-610` 的 `JsonEditor.onChange` 现于解析成功后 `setKeyConflict(false)`（`AtomCodeProviderForm.tsx:604`），通过 JSON 编辑解决冲突后"该模型键已存在"提示会被清除。提交 `d6d55d57` 信息声称「via **JSON editor**」在 `5e6d6156` 前源码不支持，`5e6d6156` 后该声称已落地（§11.8）。
- **已坐实的失焦丢值（2026-11-15 只读实测，见 §12.2）**：`src/components/ui/ime-safe-input.tsx:67-83` 的 `onBlur` 会把内部 `draft` 无条件重置为外部 `value`；当 `updateModel` 因重名提前 `return`（`AtomCodeProviderForm.tsx:225-232`）时父级 `value={key}` 未变，失焦后用户输入被丢弃。另发现同页不一致：模型键输入框（`:554-562`）**未传 `normalize`**，而供应商键输入框（`:406-415`）传了 `normalizeKey`；`onValueChange` 内的手动 `normalizeKey(next)` 只作用于父级 config，`ImeSafeInput` 的 draft 仍保留非法字符，直到失焦才被吞 —— 这也呼应评审 #1「前端不要自做一套」。

  | 场景 | 输入后 | 失焦后 | 判定 |
  |---|---|---|:--:|
  | A. `m2` 改成已存在的 `m1`（同时出现"该模型键已存在"） | `m1` | **`m2`** | 输入被静默吞掉 |
  | B. `m2` 改成合法新键 `m2new` | `m2new` | `m2new` | 正常，稳定 `rowIds` 生效 |
  | C. 输入 `m1 中文` | `m1 中文` | `m1` | 非法字符拖到失焦才被吞 |

- ~~**❓ 无法确认**~~ → **已坐实**：评审意见全文已由用户提供并收录于 **§0**（2026-11-15），此前「仓库三份文档中无该评审原文」的空白已消除；#12 原句仍以省略号收尾，但只读探针实际渲染输入框后已复现两类失焦丢值（A / C），足以确认评审指向的现象成立；#12 由 `⚠️ / ❓` 收敛为 `⚠️`，统计不变。原句补全不再是判定前提，仅影响修复时是否还覆盖其他未测场景。

### 4.5 SSOT 写序的架构层残留（评审 #11 的 ⚠️ 部分）

具体 bug 已修，但启动回填仍是 **live → db 单向覆盖、无冲突判定**：

- `src-tauri/src/lib.rs:1020-1028` 注释：「…也让外部修改的 live 文件能在重启后同步到数据库」；`:1064-1070` 无条件调用 `import_atomcode_providers_from_live`
- `src-tauri/src/services/provider/live.rs:1598-1612`：`if existing.settings_config != config { provider.settings_config = config; state.db.save_provider(...) }`，无时间戳/哈希/来源判定
- `live.rs:906-917` `sync_current_to_live` 跳过 AtomCode → db→live 方向无 reconcile
- `mod.rs:5815-5817` 允许 `write_live=false` 的 db-only 写入；测试 `atomcode_db_only_update_leaves_live_untouched` 只断言"live 不变"，**未断言重启后 db 不被回退**
- `atomcode_config.rs:479-484` `switch_and_commit` 用 live 投影整体替换 db 卡片 `settings_config`，可能丢失其他字段
- `AGENTS.md` 声明"db 为 SSOT"，与"启动时以 live 为准"存在张力；但 `live.rs` 中 OpenCode / OpenClaw / Hermes 同为覆盖式实现 → **属项目级既有 additive 设计权衡，非本分支新引入**

---

## 5. 联网核实：评审意见 #10 中需修正的两处细节

评审 #10 的**主体成立**（按官方文档配置导入为 0、`type` 仅三种、PR 未给版本与源码依据），但以下两点需按核实结果修正措辞，避免据此误伤贡献者。

**核实来源（2026-11-13）**：官方文档 `https://atomcode.atomgit.com/docs/zh/configuration.html` / `/docs/en/configuration.html`；真实仓库 `atomgit.com/atomgit_atomcode/atomcode`（GitHub 镜像 `github.com/atomgit-atomcode/atomcode`）。

**(1) `ATOMCODE_HOME` 并非"查不到出处"** —— 官方配置文档明确写 `init_prompt_file` 相对路径基于 `$ATOMCODE_HOME` 解析；官方 README 也使用 `$ATOMCODE_HOME/commands/`、`$ATOMCODE_HOME/config.toml`；公开源码解析文章给出 `crates/atomcode-config/src/distribution.rs` 的 `HOME_ENV = "ATOMCODE_HOME"` / `HOME_DIR_NAME = ".atomcode"`。本分支 `atomcode_config.rs:19-24` 的 `data_dir()` 实现与之吻合。**问题只在 PR 文档 `docs/atomcode-integration.md:43` 未附出处**，而非环境变量本身无据。

**(2) `provider_accounts` 并非"纯第三方杜撰"** —— 官方文档 v5.0.7 段落写明：推荐 `/provider`「先添加供应商账号，再在该账号下添加一个或多个模型；供应商共用协议、Base URL 和 API Key；模型分别保存名称、上下文窗口等参数」，并在 `retry_max_attempts` 说明中提到应写在 `[models."..."]` 模型配置中。即 **account/model 结构有官方依据**。但官方文档顶层字段表只有 `default_provider` / `default_workdir` / `providers` / `vision_preprocessor_provider` / `init_prompt_file`，**没有** `provider_accounts` / `models` / `default_model` 的定义，也未给出 `[provider_accounts.*]` 表名。

**核实后仍成立的部分**：

- 官方 `[providers.xxx].type` 明确「目前支持 `openai`、`claude`、`ollama`」→ 评审对类型数量的描述准确；`deepseek` / `gemini` / `openai_responses` 在官方 `type` 语义下确无出处，本分支把它们作为"预设 id / 历史 wire 名"使用，文档 `docs/atomcode-integration.md:31-39` 的类型清单与代码 `PROVIDER_IDS`（`atomcode_config.rs:102-127`）不一致。
- 官方 `default_provider` = `[providers.*]` 中定义的 key；本分支 `current_provider()` 注释称其为"模型键"并用 `models[default_provider].account` 反查，`select_provider_model`（`:443-444`）还把 `default_model` 与 `default_provider` 写成同一个模型键 → 当时判定**存在语义冲突，需要源码级依据解释**。**已修订（§11.7）：拿到 AtomCode 源码后证实无冲突**——`default_provider` 是 legacy 字段、`default_model` 为 canonical，CLI 程序化切换同样把两字段写成同一个 selection id（`main.rs:3215-3219`），本分支的双写与之一致。
- `docs/atomcode-integration.md:5` 声称基于 **AtomCode v0.11.0**，公开检索查无出处，与官方 v4.x/v5.x 版本序列明显冲突；`:7` 指向 `github.com/atomcode-ai/atomcode/blob/main/docs/configuration.md`（非官方文档地址，实测不可达）；`:9-10` 的 `github.com/some-repo/magpie/pull/736` 与 `github.com/example/dsh-connect-atomcode` 为**占位域名**。真实第三方项目为 `github.com/eghrhegpe/dsh-connect-atomcode`、`aqiu817/magpie-atomcode`（未找到 magpie PR #736）。
- `:88-94` Known Limitations 第 1 条自承「按官方文档配置的用户导入为 0」；`:111-116` Future Improvements 第 1、2 条自承「尚未记录确切版本/commit」「官方 `[providers.*]` 格式导入未实现」。
- 导入链证据：`src-tauri/src/atomcode_config.rs:61-73` `get_providers()` 只 `document.get("provider_accounts")`；全仓 atomcode 相关 Rust 代码 grep `get("providers")` 无命中；`read()`（`:30-48`）只校验 `provider_accounts` / `models` 两个键。前端 `ProviderList.tsx:531-538` 会显示"导入"按钮，官方格式配置下返回 `Ok(0)`。

> **局限**：未登录克隆 AtomCode 官方仓库逐行核对 `provider_preset.rs` 的 `PRESETS`；WebSearch 可能漏检历史 / 内测标签，因此对 v0.11.0 只表述为"公开可检索范围内查无出处、高度可疑"，不臆断其绝对不存在。

---

## 6. 未满足项的硬证据

### 6.1 Skills / MCP 未接线（评审 #4）

| 位置 | 代码 / 事实 |
|---|---|
| `src-tauri/src/app_config.rs:53-57` | `// AtomCode MCP sync is not wired yet (needs a schema column).` → `AppType::AtomCode => false` |
| `:76-78` | `McpApps` setter 对 AtomCode 是空分支 `{}` |
| `:194-195,216-219` | `SkillApps::get` → `false`；setter → `{}  // AtomCode skills are not wired yet` |
| `src-tauri/src/database/dao/mcp.rs:104-108` | `AppType::AtomCode \| ClaudeDesktop \| OpenClaw => None`（无 `enabled_atomcode` 列） |
| `src-tauri/src/services/mcp.rs:398-405,462-470` | sync / remove 两处 `log::debug!("AtomCode MCP sync is not wired yet, skipping")` |
| `src-tauri/src/services/skill.rs:685-686` | 外部目录保护 `AppType::AtomCode => {}`；`:752` 却给出 `data_dir().join("skills")` → 目录映射给了、开关恒 false |
| `src-tauri/src/prompt_files.rs:44-48,62-66` | 已映射 `AGENTS.md`，但注释「管理页暂不接入」；前端 `promptUtils.tsx` 无 atomcode |
| `src-tauri/src/deeplink/provider.rs:408-417` | 明确返回 `atomcode.deeplink_unsupported`（**未变**）；~~`src/lib/api/deeplink.ts` 把 atomcode 加入可选 app~~ → **已修订**：`8941b0ae` 已移除，前后端不再矛盾 |
| `src/types.ts:855-860`、`src/lib/api/skills.ts:17-42` | ~~前端类型新增 `McpApps.atomcode?` / `SkillApps.atomcode?` → 声称支持，后端恒 false~~ → **已修订**：`8941b0ae` 删除这两处类型声称与 `AppType` 中的 atomcode，新增 `skillAppEnabled()` helper（`skills.ts:42`）做安全索引；`SkillsPage` / `UnifiedSkillsPanel` 共 10 处调用点改走 helper。**后端仍恒 false** |
| `docs/atomcode-provider-integration-zh.md:16-17` | 表格自承「MCP 未接线：需要数据库 schema 新列…静默跳过」「Skills 未接线」（**未变**） |

`git grep` 确认以下文件在 HEAD 中完全不含 atomcode：`deepLinkConfigPreview.ts`、`promptUtils.tsx`、`DeepLinkImportDialog.tsx`、`UnifiedMcpPanel.tsx`、`SkillImportDialog.tsx`、`src-tauri/src/deeplink/mcp.rs`、`src-tauri/src/database/dao/skills.rs`、`presetGroups.ts`、`ProviderPresetSelector.tsx`。

对照 `bd487e83` 的既有正确做法：`database/schema.rs` 新增 `enabled_copilot` 列 + `24 => 25` 迁移 + 2 个测试、新增 `mcp/copilot.rs`(519 行)、`services/mcp.rs` +37、`services/skill.rs` +142、`deeplink/provider.rs` +72，且文档用 `copilot help providers/environment/config` 实测逐项给出原生格式依据。`AGENTS.md` 禁止随意改 `SCHEMA_VERSION` 是合理约束，但该提交演示了"与维护者协作加列 + 迁移 + 测试"的解法；本分支初版选择了"前端类型加了、后端恒 false"，**`8941b0ae` 已把前者改为"前端类型也移除"，使声称与实现一致，但能力缺口本身不变**——这正是 #4 维持 ❌ 的理由。

### 6.2 截图（评审 #3a）

- `.github/pull_request_template.md:17-23`：Screenshots `| Before | After |` 两格全空
- `git diff --name-status upstream/main...HEAD | grep -iE '\.(png|jpe?g|gif|webp|svg)$'` → 无输出；全仓 193 张位图无 atomcode 命名；`docs/images/` 20+ 子目录无 atomcode；`assets/screenshots/` 仅 add/main 的 en/ja/zh
- 4 份 atomcode 文档与 `docs/release-notes/` 均无 `![]()` / `<img>` 配图
- 仓库内自认未完成：`docs/atomcode-provider-integration-zh.md:41`「…以及界面截图，留待合并前补齐」；`docs/atomcode-provider-pr-audit-zh.md:110`「『截图』章节仍是占位文字」、`:192` 未勾选的 `- [ ] **M3**：…补真实截图`
- **PR 描述层面的截图在仓库内无法验证**：PR body 不是 git 内容，需 `gh pr view` / 人工看 PR 页面，本次未执行 → ❓

### 6.3 检查清单可信度（评审 #3b）

`.github/pull_request_template.md:25-30`：

```md
- [ ] `pnpm typecheck` passes ...
- [ ] `pnpm format:check` passes ...
- [ ] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [ ] Updated i18n files if user-facing text changed ...
```

- 4 项全部 `- [ ]`（**未勾选**）；条件提示仍在。`CONTRIBUTING.md:458` 也有同类条件句
- 本分支在 `52108a48` 之前是否改过模板：当时 `git log upstream/main..HEAD -- .github/pull_request_template.md` 为空、`git rev-parse HEAD:... == upstream/main:... == 784a91dfab3d652a4590808b9adb3795221fac0b` → **那时完全未改**（§11.5 原结论对 `52108a48` 成立）。
- **修订（`5e6d6156`）**：模板现已改——`<!- 逐项真实勾选、不适用写 N/A + 理由 ->` 提示、`cargo clippy -- -D warnings` 标注「全 workspace，勿只跑 `--lib`」、新增 `pnpm test:unit` / `cargo fmt --check` / `cargo test` 三项；但 `(if Rust code changed) / 如修改了 Rust 代码` 条件提示**仍保留**（与 `CONTRIBUTING.md:458` 同类条件句）。即 P1 #6 的「补齐 test 命令 + 写明 clippy 口径」子项已落地，但条件句设计未移除（§9 P1 #6、§11.8）。
- 本分支 Rust 改动：`git diff --name-only upstream/main...HEAD -- src-tauri/ | wc -l` = **19**（含新增 `atomcode_config.rs`、改 `lib.rs` / `services/provider/mod.rs` / `tests/mcp_commands.rs`）→ 条件句客观上「适用」
- **意见指向**：「默认全勾选」只可能发生在 PR 描述正文（GitHub 把模板 `- [ ]` 渲染成可点复选框），仓库内不存在任何已勾选清单 → 该意见指向 PR 描述文本，**仓库内无法验证 (❓)**；仓库可验证的只有「模板空勾选 + 条件提示 + 本分支未改」
- **可信度结论**：模板 `- [ ]` 本身自洽（不预置勾选、要求逐项确认），真正的信息失真发生在 PR 描述层，改模板无法追溯修正该 PR。`5e6d6156` 前模板有两个削弱可信度的设计缺陷：① `clippy` 带条件句，把 19 个 Rust 文件改动下的必跑项变成作者自判「可跳过」，与 `AGENTS.md` 的后端全套不一致；② 清单只有 4 项，缺 `pnpm test:unit`、`pnpm build:renderer`、`cargo test`、`cargo fmt --check`。`5e6d6156` 已消除缺陷 ②（补齐 `pnpm test:unit` / `cargo test` / `cargo fmt --check`，并写明 clippy 全 workspace 口径），但缺陷 ① 的条件句**仍保留**（即「如修改了 Rust 代码」的提示文字未删除）。另有口径差异：`cargo clippy -- -D warnings` 通过，但 `cargo clippy --lib --tests -- -D warnings` 失败（见 §8）→ 若勾选 clippy 必须写明运行范围（模板现已注明）

---

## 7. i18n 矩阵与文档可信度

### 7.1 新增 key × 7 语言

`git diff upstream/main...HEAD -- src/i18n/locales/`：7 个文件全部修改。口径按**新增叶子 key** 计（含结构行的行数另有 28，不混用）：主体快照（`7b5b5771`）为 `atomcode.*` **22 个**（基线为 0）→ `8941b0ae` 补 `apps.atomcode` 后为 **23 个**（22 + 1）→ `5e6d6156` 又增 `atomcode.error.defaultModelInUse` 使 **5 核心语言达 24 个**（ko/ru 为 23 个，靠 `fallbackLng: "en"`），详见下方口径说明。主体快照时核心 5 语言均覆盖这 22 个（`22/22`）；ko / ru 仅 `form.modelKeyDuplicate` 1 个，其余走 `fallbackLng: "en"`（`src/i18n/index.ts:109`）。

| 新增 key（`atomcode.*` 22，主体快照） | zh | zh-TW | en | ja | tr | ko | ru |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| `empty.title`、`empty.description`、`sharedProviders` | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ⤵en | ⤵en |
| `form.providerKeyHint`、`providerKeyInvalid`、`providerType` | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ⤵en | ⤵en |
| `form.baseUrl`、`baseUrlHint`、`apiKey`、`apiKeyPlaceholder`、`apiKeyHint` | ✅×5 | ✅×5 | ✅×5 | ✅×5 | ✅×5 | ⤵en | ⤵en |
| `form.models`、`addModel`、`noModels`、`modelKey`、`modelKeyPlaceholder`、`modelId`、`modelName`、`contextWindow`、`contextWindowPlaceholder`、`removeModel` | ✅×10 | ✅×10 | ✅×10 | ✅×10 | ✅×10 | ⤵en | ⤵en |
| **`form.modelKeyDuplicate`** | ✅ | ✅ | ✅ | ✅ | ✅ | **✅** | **✅** |
| **`apps.atomcode`（增量修订，`8941b0ae` 新增）** | **✅** | **✅** | **✅** | **✅** | **✅** | **✅** | **✅** |

> 口径说明：主体快照（`7b5b5771`）按 `git diff -U0 | grep -c '^+'` 计为 28 行（含 `"atomcode": {`、`"form": {`、`},` 等 6 行结构行），按扁平化叶子 key 计为 `atomcode.*` **22** 个，两者不矛盾。**递增链**：`atomcode.*` 22（`7b5b5771`）→ +`apps.atomcode` = **23**（`8941b0ae` 起，均 7 语言齐全，ko/ru 的 `atomcode.form.*` 仍靠 fallback）→ +`atomcode.error.defaultModelInUse`（5 核心语言）使 **5 核心语言达 24 个、ko/ru 23 个靠 fallback**（`5e6d6156` 起）；`5e6d6156` 同期还补了基线即缺失的 `apps.kimi` / `apps.dsh`（属通用 `apps.<id>`，不计入 atomcode 专属叶子 key 增量，见 §11.8）。

**符合约定的部分**：`AGENTS.md` 明确「ko / ru 为社区翻译，允许缺失并走 `fallbackLng: "en"`」，ko/ru 唯一新增的 `modelKeyDuplicate` 恰是 `docs/atomcode-provider-pr-audit-zh.md:118-121 (M5)` 要求补的那条 → M5 已修复。`ru.json` 整体仅 2556 个 key（en 为 4556），本就长期不完整。静态扫描 28 个改动的 `src/**/*.ts(x)` 提取 415 个不同 `t("字面量")` key，**0 个未在 `en.json` 定义**；新增行中无硬编码中文用户可见文案。

**2026-11-15 精确口径复核（§12）**：以 `en.json` 的 `atomcode.*` 扁平化叶子 key 为基准共 **23 个**，逐语言比对为 `zh / zh-TW / en / ja / tr = 23/23`、`ko = 1/23`、`ru = 1/23`（各缺 **22** 个，含 `empty.*`、`form.*`、`error.defaultModelInUse`）；7 语言的 `apps.atomcode` 则全部存在。因此 #2 的 ✅ 是**按 `AGENTS.md`「ko / ru 可 fallback」的项目约定**成立；若严格按评审字面「多语言支持缺失，请补全」，ko / ru 界面仍显示英文，字面诉求未完全满足。该口径差异不改变判定，但补全成本仅 44 条译文，属合并前可低成本闭环项（§12.4）。

**⚠️ 新发现的本分支引入缺陷（主体快照 `7b5b5771`；第 1、2 项已由 `8941b0ae` 修复，见 §11.2）**：

1. ~~**`apps.atomcode` 在 7 个语言文件中全部缺失**~~ → **已修复**。原状态：`en.json` 的 `apps` 段仅 15 个 key；动态调用点 `presentation.ts:968` → `t("providerCard.section.addedHelp", { app: t(\`apps.${app}\`) })` 与 `ProviderList.tsx:499` → `t("provider.liveConfigReadFailed", { app: t(\`apps.${appId}\`) })`；实测 `t("apps.atomcode") === "apps.atomcode"` → 裸 key 泄漏。`8941b0ae` 补齐 7 语言后，QA 实测均返回 `"AtomCode"`。
2. ~~**3 个死 key**~~ → **已修复**。原状态：`atomcode.sharedProviders`、`empty.title`、`empty.description` 定义后从未引用（`presentation.ts:966` 只对 dsh 特判；`ProviderEmptyState.tsx:25-31` 的 `emptyCopyNs` 不含 atomcode），22 个 key 中仅 19 个真正被使用。现调用链：`sharedProviders` ← `presentation.ts:967-968` ← `buildAdditiveSections` ← `ProviderList.tsx:393`；`empty.*` ← `ProviderEmptyState.tsx:30-31,48,53` ← `ProviderList.tsx:522-528`。
3. **硬编码英文（低风险 / 有既有惯例，未变）**：`atomcodeProviderPresets.ts` 的协议与厂商显示名在 `AtomCodeProviderForm.tsx:453` 直接 `{type.label}` 渲染；但 `copilotProviderPresets.ts:48-67` 等基线文件同样如此，属既有惯例。此项是 #2 升 ✅ 后唯一的残留，属专有名词、不足以维持 ⚠️。
4. `AGENTS.md` 被 `.gitignore:11` 忽略、`git ls-files AGENTS.md` 为空 → 它是本机 agent 指南，**"ko/ru 允许 fallback"的约定无法被 CI 或 PR 评审者从仓库引用**（未变）。
5. ~~**新增残留风险（QA 建议，P1）**~~ → **已闭环（`5e6d6156`）**：`apps.<id>` 动态模板 key 无法被静态扫描 / TypeScript 拦截的风险，已由新增 `tests/i18n/appIdsI18n.test.ts` 覆盖——断言所有 `APP_IDS` 在 en + 5 核心语言都有 `apps.<id>` 且 `t()` 不返回 key 本身，并断言 `skillAppEnabled({}, "atomcode") === false`。该测试同时把基线即缺失的 `apps.kimi` / `apps.dsh` 补进了 5 核心语言（见 §11.8）。

### 7.2 文档中"声称已修复"但源码不支持的条目

| # | 位置 | 声称 | 实际 |
|---|---|---|---|
| 1 | `atomcode-fixes-summary.md:63-73` | App.tsx「Already correct – no changes needed」+ 行号 1677/1690/1706/1324 | 这四处**正是本分支新增的 atomcode 白名单**；当前行号为 1685/1701/1717/1324；1324 现为模型 key 重写，`addToLive: false` 在 1335-1338 |
| 2 | `:39-61` | Update Path Ordering「Already correct」+ `atomcode_config.rs:285-318`、`mod.rs:7142-7150` | `211aff9a` 提交信息明确写 "Provider edits **now** write the live file before the database commit…"，说明是本次修复；实际位置 `376-384` / `7146-7156` |
| 3 | `:19` | 默认模型校验在 `atomcode_config.rs:225-256` | 实际在 `:305-324`（225-256 是 `write()` 尾部）；所声称的测试属实 |
| 4 | `:85-92` | ProviderCard Actions Whitelist「Not needed – already handled by presentation.ts」 | presentation.ts 确实处理，但 §4.2 的 kimi 专属硬编码未同步 |
| 5 | 提交 `d6d55d57` 信息 | "Clear keyConflict warning in **all** config update paths…via **JSON editor**" | `AtomCodeProviderForm.tsx:600-610` 的 `JsonEditor.onChange` 不经过 `update()`。**已修订（`5e6d6156`，已入库）**：JSON 路径已补 `setKeyConflict(false)`（§4.4），该声称现与源码一致，去向见 §11.8 |
| 6 | `:5-11` | isInConfig「**now** correctly checks `meta.liveConfigManaged`」 | `211aff9a` 时 atomcode 已有专属 case；`78ed827e` 实际改的是 `default: true→false` |
| 7 | `:23-35` | Documentation「✅ Fixed – Supported AtomCode version (v0.11.0)」 | 只新增了一份自证文档，链接不可达、版本查无出处、截图未补，见 §5。**已修订（未提交工作区）**：`docs/atomcode-integration.md` 重写为 v5.2.7 / `0c0512143` / 官方地址 / 6 处源码行号（§11.7），但尚未入库 |
| 8 | `docs/atomcode-integration.md` | `:5/:92` 声称基于 v0.11.0 | `:113` Future Improvements 却写「尚未记录确切版本/commit」；`:33-39` 类型清单与 `PROVIDER_IDS` 不符；`:43` 的 `ATOMCODE_HOME` 无出处。**已修订（未提交工作区）**：v0.11.0、占位链接、类型清单、Known Limitations 1 / Future Improvements 1–2 均已重写处置（§11.7、§11.8 第 5 点），尚未入库 |
| 9 | `docs/reviews/2026-11-11-code-review-211aff9a.md:55` | 「i18n 七语言同步未发现问题」 | **已修订**：与 `apps.atomcode` 裸 key 的冲突随 `8941b0ae` 消失（该审查只扫静态 key，漏了动态 `t(\`apps.${app}\`)` 的历史事实仍成立）；但 ko/ru 仍缺 21 个 `atomcode.form.*`，严格按"七语言同步"字面口径该结论仍不完全成立 |
| 10 | `docs/atomcode-integration.md:90/:114` | Known Limitation / Future Improvement | **诚实记录**，与源码一致，恰是评审 #10「导入为 0」的直接证据 |

---

## 8. 实测结果

### ✅ 已验证（只读，记录退出码）

| 命令 | 退出码 | 关键输出 |
|---|:--:|---|
| `pnpm typecheck` | 0 | `tsc --noEmit` 无输出 |
| `pnpm format:check` | 0 | `All matched files use Prettier code style!`（仅覆盖 `src/**`，不含 `tests/`） |
| `pnpm exec vitest run src/config/atomcodeProviderPresets.test.ts` | 0 | **11 passed** |
| `pnpm exec vitest run tests/components/AtomCodeProviderForm.test.tsx` | 0 | **4 passed** |
| `pnpm exec vitest run tests/components/ProviderList.test.tsx` | 0 | **39 passed**（MSW 未匹配请求 warning，不影响） |
| 3 个 atomcode 前端文件合并跑 | 1 | 15 passed，但 `Vitest caught 1 unhandled error: EPERM …\Temp\…\web\40b9f1…`（baseline-browser-mapping 缓存写被拒）→ 断言全绿、进程码被环境噪音拉红；主理人独立复现同现象 |
| `pnpm test:unit`（全量） | 1 | **2 failed / 3598 passed**；293 files passed / 2 failed；6 个 EPERM。失败例为 `PiProviderForm.test.tsx` 与 `tests/integration/App.test.tsx` 的 5000ms 超时，**与 atomcode 无关**；单独复跑均通过（3358ms / 4541ms） |
| `CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test --lib atomcode_ -- --test-threads=1` | 0 | **11 passed; 0 failed; 4534 filtered out**，含 `cannot_delete_or_rename_default_model ... ok` |
| `cargo fmt --check` | 0 | 无差异 |
| `cargo clippy -- -D warnings`（AGENTS.md 标准口径） | 0 | `Finished dev profile` |
| `cargo clippy --lib --tests -- -D warnings`（更严） | 101 | 4 errors：`dsh_config.rs:2275`、`settings.rs:1711/1719` → **既有测试代码 lint，非本分支引入** |
| node + i18next 实跑 `t("apps.atomcode")`（主体快照 `7b5b5771`） | — | 返回 `"apps.atomcode"` → 裸 key 泄漏实证 |
| `git status --short`（首/尾各一次） | — | 均为空 |

**增量修订复核（QA 于 HEAD `5843fe16` 独立回归；其后 `52108a48` 为纯文档、`5e6d6156` 为源码提交但未触碰被实测文件，结论对 `5e6d6156` 仍有效，见 §11.4）**：

| 命令 | 退出码 | 关键输出 |
|---|:--:|---|
| `pnpm typecheck` | 0 | `tsc --noEmit` 无输出 |
| `pnpm format:check` | 0 | `All matched files use Prettier code style!` |
| `pnpm exec vitest run src/config/atomcodeProviderPresets.test.ts` | 0 | **11 passed** |
| `pnpm exec vitest run tests/components/AtomCodeProviderForm.test.tsx` | 0 | **4 passed** |
| `pnpm exec vitest run tests/components/ProviderList.test.tsx` | 0 | **39 passed**（三个文件合计 54 passed） |
| 三文件合并跑 | 1 | 断言全绿，但 2 个 `EPERM …\Temp\…\web\40b9f1…` unhandled error 拉红进程码，与主体快照同因的环境噪音 |
| node + i18next 实跑 `t("apps.atomcode")` × 7 语言 | — | 7 语言均返回 `"AtomCode"`，无一返回裸 key |
| `git show --name-only 8941b0ae \| grep -iE 'test\|spec'` | — | **无新增/修改测试**；`5843fe16` 仅新增 1 份报告 md，`52108a48` 仅修订该报告 md |

> 增量复核边界（QA 按约定）：**未跑**全量 `pnpm test:unit`、任何 cargo 命令、`pnpm build:renderer`；`ProviderEmptyState` 组件无对应测试文件；PR 描述正文与真机 `/model` 验证仍未执行。故「全量 `pnpm test:unit` 通过」依然不能宣称已验证。

**新增/修改测试文件共 10 个（+1254 行）**：`tests/components/AtomCodeProviderForm.test.tsx`(A, 156)、`src/config/atomcodeProviderPresets.test.ts`(A, 205)、`tests/components/ProviderList.test.tsx`(M, +75)、`src-tauri/src/atomcode_config.rs`(A, 763，含 7 个 `#[test]`)、`src-tauri/tests/mcp_commands.rs`(M, +2)、`tests/msw/state.ts`(M, +4)、`AppsPage.test.tsx` / `SessionManagerPage.test.tsx` / `UsageDashboard.test.tsx`(M)、**`tests/i18n/appIdsI18n.test.ts`(A, 45 行／经 `describe.each`×`it.each` 展开为 82 个用例，`5e6d6156`)**；另有 `services/provider/mod.rs` 内 4 个写序测试。

### ❓ 未验证（如实标注）

- `pnpm build:renderer`：未执行（会写 `dist/`）
- 全量 `cargo test`（约 4545 例）：未执行，只跑 `--lib atomcode_` 子集 11 例
- `pnpm test:unit` 的 2 例失败**未在 `upstream/main` 基线上对照复跑**（切工作树会破坏只读约束）；"环境抖动 / 分支既有"是基于"单独复跑通过 + 与 atomcode 无关"的推断，不是基线对照结论
- PR 描述正文（全勾选清单、截图链接、手动验证声明）、`gh pr view` 及任何 GitHub 网络操作：未执行
- 真机 AtomCode `/model` 列表与 `default_model` 生效：未执行（需真实客户端）
- **结论：「全量 `pnpm test:unit` 通过」这条清单项在本环境不能宣称已验证通过**

---

## 9. 待办清单（按优先级）

### P0 — 用户可见 / 声称与实现矛盾 / 评审主体

1. ✅ **已完成（`8941b0ae`，`5e6d6156` 加固）**：补 `apps.atomcode` 到 7 个语言文件；`presentation.ts:967-968` 新增 atomcode 分支走 `t("atomcode.sharedProviders")`；`ProviderEmptyState.tsx:30-31` 的 `emptyCopyNs` 加入 atomcode 使 `empty.*` 生效。QA 实测 7 语言 `t("apps.atomcode")` 均返回 `"AtomCode"`。`5e6d6156` 进一步补 `apps.kimi` / `apps.dsh` 到 5 核心语言，并新增 `tests/i18n/appIdsI18n.test.ts` 覆盖所有 `APP_IDS` 的 `apps.<id>` 一致性断言（**原「遗留 P1」已闭环**，见 §7.1 第 5 点、§11.8）。
2. ⚠️ **选项二已落实、选项一未做（`8941b0ae`）**：已移除 `src/types.ts` 的 `McpApps.atomcode?`、`src/lib/api/skills.ts` 的 `SkillApps.atomcode?` / `AppType` 中的 atomcode、`src/lib/api/deeplink.ts` 的可选 app，并新增 `skillAppEnabled()` helper 覆盖 10 处调用点 → 矛盾消除。**但能力仍未接线**：需与维护者协作新建 issue 明确记录「AtomCode 暂不支持 Skills / MCP / deeplink 导入」的决策与后续计划，或按 `bd487e83` 模式补 `enabled_atomcode` 列 + 迁移 + 测试全量接线。此项未完成前 #4 维持 ❌。
3. **导入与配置格式依据**：实现官方 `[providers.*]` 格式导入，或在 UI 明确提示「仅支持 v5.x `/provider` 生成的账号格式」，并删除/兑现 Known Limitation 第 1 条。
4. **由贡献者给出可验证凭据**（而非再写一份自证文档）：确切版本号 + commit SHA + 仓库 URL，修正/删除 v0.11.0；把 404 链接换成 `https://atomcode.atomgit.com/docs/zh/configuration.html` 并标注文档版本；替换 `some-repo` / `example` 占位链接；对 `[provider_accounts.*]` / `[models.*]` 字段规范、`default_provider` 与 `default_model` 的语义及优先级（含本分支把两者写成同一模型键与官方定义的冲突）给出源码依据；逐项标注 `deepseek` / `gemini` / `openai_responses` 等 id 的来源与归一化理由；为 `ATOMCODE_HOME` 补 `distribution.rs` 的源码依据。
5. **补交付物**：真实截图（PR 模板表 + `docs/images/`），附 `/provider` 实际写出的 config.toml 样例与真机 `/model` 验证结果。

### P1 — 一致性与清单可信度

6. 把 PR 描述检查清单改为逐项真实勾选；模板去掉 `(if Rust code changed)` 或改为「必填 / N/A + 理由」，补齐 `pnpm test:unit`、`pnpm build:renderer`、`cargo test`、`cargo fmt --check`，并写明 clippy 命令口径。**部分已做（`5e6d6156`）**：模板新增逐项真实勾选提示、补 `pnpm test:unit` / `cargo fmt --check` / `cargo test`、写明 clippy 全 workspace 口径；但 `(if Rust code changed)` 条件句仍保留（§6.3）。
7. CLI 一致性：接工具安装/卸载（`TOOL_NAMES` / `TOOL_APP_IDS` / `tool_uninstall.rs`），恢复 `AppsPage.tsx` 的 `Record<AppId, Row>` 完整性；目录设置接入既有 override-dir 体系或明确"仅支持 `ATOMCODE_HOME`"并写入文档；预设清单复用共享供应商目录或补可验证出处并接 `presetGroups.ts` / `ProviderPresetSelector.tsx`。
8. ~~同步 kimi 专属硬编码（`mutations.ts:243-248/326-331`、错误翻译、`App.tsx:874-880` env 跳过列表）；JSON 编辑路径补 `setKeyConflict(false)`~~ → **已闭环（`5e6d6156`）**：kimi 专属硬编码（mutations onSettled、错误翻译、App.tsx env 跳过列表）已同步 atomcode，JSON 编辑路径已补 `setKeyConflict(false)`（见 §4.2、§4.4、§11.8）。**仍待处理**：统一 `current_provider()` 空值与 `default_provider`/`default_model` 语义（#8 残留）；#12 已由只读实测坐实、评审原文已收录 §0、原句补全不再是判定前提，但修复 A / C 两类失焦丢值仍待处理（§4.4、§12.2）。
9. 去掉 `docs/atomcode-fixes-summary.md` 中「Already correct」「Documentation ✅ Fixed」等自证表述，按 §7.2 校正行号与描述。

### P2 — 架构与测试稳定性

10. 给 `import_atomcode_providers_from_live` 增加方向/冲突判定（时间戳、内容哈希或"已有 id 不覆盖、只新增"）；补一条「`write_live=false` 写 db 后模拟重启，断言 db 不被回退」的回归测试；`switch_and_commit`（`atomcode_config.rs:479-484`）改为字段合并而非整体替换；统一 `AGENTS.md` 的 SSOT 声明与实际启动行为。
11. 修复 `docs/reviews` P2 #2：用 Drop guard 自动清除 `crash_at(Some("atomcode:write"))`，避免断言 panic 时污染同 crate 后续测试；`atomcode_config.rs:639-657` 的 `set_var("ATOMCODE_HOME")` 同样缺 guard。
12. 清理与 AtomCode 无关的夹带改动（`UsageTrendChart.tsx`、`useDragSort.ts`）；决定 `prompt_files.rs` 的"接入"或"不接入"并同步 `promptUtils.tsx`。

---

## 10. 核验范围与局限

- 全程只读：主体核验阶段 `git status --short` 首尾均为空，未写入 / 编辑 / 提交任何**源码**文件；报告文件由 `5843fe16` 入库、`52108a48` 收录 §11 与计数勘误（`5843fe16`/`52108a48` 各自**只改动本报告一个文件**），`5e6d6156` 在 12 文件源码提交中附带本报告微调。当前工作区另有本报告未提交改动（见 §11.8）。
- 基线 `upstream/main` = `bd487e83`。主体快照 `7b5b5771` 时 `upstream/main..HEAD` 共 15 个提交、65 个改动文件（+3534/-68）；`5843fe16` 时为 17 个提交、68 个文件（+3996/-96）；`52108a48` 时为 18 个提交、68 个文件（+4080/-96）；**当前 HEAD `5e6d6156` 实测为 19 个提交、71 个文件（+4217/-108）**（`git rev-list --count` / `git diff --shortstat`）。`5e6d6156` 改动 12 个文件（+156/-31），含 4 个源码文件 + 5 核心语言 i18n + PR 模板 + 新增 i18n 测试 + 本报告，是 `8941b0ae` 之后又一次源码变更。`7b5b5771..HEAD` 的 name-status 仍为 15 个条目（1 新增文档 + 14 修改源码/语言文件），报告相对基线始终是纯新增文件。对照 `git show bd487e83`（Copilot CLI，105 文件 +4753/-179）。
- AGENTS.md 为被 gitignore 的本机指南，不在仓库内，其约定无法被 CI / PR 评审者引用；本报告按其内容作为判断依据并已标注。
- 联网核实仅覆盖公开可检索范围，未登录克隆 AtomCode 官方仓库逐行核对 `provider_preset.rs`；对版本号、第三方链接的结论已标注来源与局限，不作超出可检索范围的推断。
- PR 描述正文、GitHub PR 页面、真机 AtomCode 客户端均不在本次核验范围内。**评审意见原文**此前亦不在仓库内，§0 依据用户 2026-11-15 消息逐字转录，未经 PR 页面二次校对；#12 句尾省略号为用户原文所有，未作补全或推测。
- 增量修订复核按约定**有意收窄边界**：未跑全量 `pnpm test:unit`、任何 cargo 命令、`pnpm build:renderer`；结论不与主体快照的实测结果混用。
- §12 的独立复核以上述只读口径为准，**唯一例外**为临时探针 `tests/components/__probe.atomcode-blur.test.tsx`（实跑后已删除，见 §12.1）；§12 与本次 §0 收录均**未修改任何源码**，工作区仍为原有 3 个未提交文件。

---

## 11. 后续提交状态（截至 `5e6d6156` + 未提交工作区）

### 11.1 提交时间线

| 提交 | 时间 | 内容 | 影响范围 |
|---|---|---|---|
| `7b5b5771` | 11-11 21:10 | `fix(atomcode): fall back to default protocol for empty provider string` | 报告主体核验快照 |
| `8941b0ae` | 11-13 20:37 | `fix(atomcode): add missing apps.atomcode i18n and drop unwired skills/MCP/deeplink types` | 源码变更（i18n/类型）：14 文件，+52/-34；修复报告 §9 P0-1，落实 P0-2 的选项二 |
| `5843fe16` | 11-13 20:44 | `docs(atomcode): add PR review verification report` | 416 行报告入库，**仅 1 个文件**，无源码改动 |
| `52108a48` | 11-13 23:56 | `docs(atomcode): revise PR review verification report with §11 incremental findings` | 报告 416 → 500 行（+115/-31）；收录 §11 增量修订、#2 升 ✅、`14 → 10` 计数勘误、剩余缺口与 P1 建议，**仅 1 个文件**，无源码改动 |
| `5e6d6156` | 11-14 08:59 | `fix(atomcode): sync kimi-specific hardcodes and harden i18n consistency` | **最后一次源码变更**：12 文件（+156/-31）；同步 kimi 硬编码到 atomcode（报错翻译 / onSettled 重读 / env 跳过列表）、JSON 编辑器补 `setKeyConflict(false)`、补 `apps.kimi`/`apps.dsh` 到 5 核心语言、新增 `tests/i18n/appIdsI18n.test.ts`、PR 模板诚实化、本报告微调 |

**时点区分**：`8941b0ae` 与 `5e6d6156` 均为源码变更，其间 `5843fe16`、`52108a48` 为纯文档提交（`git show --name-only` 各只返回本报告文件）。因此 §1 判定统计与 §2 判定**对应代码状态 `5e6d6156`**（`8941b0ae` 之后又一次源码变更使 #7 由 ⚠️ 升 ✅），报告版本同步至 `5e6d6156` + 未提交工作区。

截至当前 HEAD `5e6d6156`，**工作区不干净**（`git status --short` 显示 3 个未提交文件：`docs/atomcode-integration.md` 重写、`src-tauri/src/atomcode_config.rs` 仅文档注释、本报告自身）。`git diff --name-status 7b5b5771..HEAD` 共 15 个条目（1 个新增文档 + 14 个修改源码/语言文件）；`5e6d6156` 已在其中补入 PR 模板、Rust 后端、i18n、测试文件，即 §11.5 所列「PR 模板未改 / kimi 硬编码 / JSON keyConflict / apps.<id> 断言」等缺口自 `5e6d6156` 起已闭环；剩余缺口见 §11.8。

### 11.2 `8941b0ae` 修了什么（对应评审 #2）

| 改动 | 位置 | 效果 |
|---|---|---|
| 7 个语言文件各加 `apps.atomcode: "AtomCode"` | `zh/zh-TW/en/ja/tr/ko/ru.json`（`apps` 段，en 等约 `:1183`、ru 约 `:853`） | QA 用 i18next 实测 7 语言均返回 `"AtomCode"`，裸 key 泄漏消除 |
| `presentation.ts:967-968` 新增 atomcode 分支 | `app === "atomcode" ? t("atomcode.sharedProviders")` | 调用链 `buildAdditiveSections()`(`:715`) ← `ProviderList.tsx:393` → `added` 区块 help.body；`atomcode.sharedProviders` 由死 key 变为生效 |
| `ProviderEmptyState.tsx:30-31` 的 `emptyCopyNs` 加入 atomcode | → `:48` `t(\`${emptyCopyNs}.empty.title\`)` / `:53` `…description\`)` ← `ProviderList.tsx:522-528` | `atomcode.empty.*` 两个死 key 生效 |
| 删除三处前端类型声称 | `src/types.ts`（`McpApps.atomcode?`）、`src/lib/api/skills.ts`（`AppType` / `SkillApps.atomcode?`）、`src/lib/api/deeplink.ts`（`DeepLinkImportRequest.app` 的 atomcode） | 消除「前端可选、后端必拒」矛盾 |
| 新增 `skillAppEnabled()` helper | `skills.ts:42`：`Boolean((apps as Partial<Record<AppId, boolean \| undefined>>)[app])` | 共 **10 处** `skill.apps[app]` 改走它（`SkillsPage.tsx:277` 1 处 + `UnifiedSkillsPanel.tsx:317/326/506/518/1147/1156/1290/1811/1939` 9 处）；能兜住 `statusFilter.slice(4) as AppId` 的任意字符串，缺失键返回 `false` |

> **计数勘误（21:41，由用户指出）**：本报告初版在 §6.1、§9 P0-2、§11.2 三处写作「14 处调用点」，系笔误，现统一更正为 **10 处**。用户推测「14」可能误计了 import 行或 lint/报错总数；从数字对应关系看更可能是与 `8941b0ae` 的改动文件数（14 files changed）混淆 —— 但无论成因，精确的调用点计数只有 10，且任何计数口径都得不到 14：调用 10 → 含 2 处 import 为 12 → 再含定义为 13；被替换的直接索引行数也是 10。精确依据：`grep -rn 'skillAppEnabled(' src/ | grep -vc 'export const'` = **10**；`git show --format='' 8941b0ae | grep -E '^-\s*.*skill\.apps\[app\]' | wc -l` = **10**（被替换的直接索引行数，与调用点一一对应）；`skillAppEnabled` 全仓共出现 13 次 = 10 次调用 + 2 次 import（`SkillsPage.tsx:34`、`UnifiedSkillsPanel.tsx:55`）+ 1 次定义（`skills.ts:42`）。此数字仅是改动规模描述，**不影响任何判定结论**（#2 ✅、#4 ❌ 及 ✅5/⚠️4/❌4 统计均不变）。

**QA 类型安全核验**：全仓 `grep -rn "\.apps\[" src/` 剩余 **9 处**全部是 `server.apps[app]`（MCP 侧），`skill.apps[` 为 0。其中 `UnifiedMcpPanel.tsx` 占 8 处（`:326/433/486/599/818/821/869/980`）、`src/hooks/useMcpConnectivity.ts:20` 占 1 处。它们因 `McpAppId = Exclude<AppId, "claude-desktop" \| "openclaw" \| "atomcode">`（`appConfig.tsx:190-193`）、`MCP_APP_IDS: McpAppId[]`（`:199-214`，13 项）与 `isMcpAppId` 守卫而天然安全，**无需**再加 helper。全仓 grep 已删除类型 × atomcode 仅命中 `skills.ts:41` 的说明性注释；`src/types.ts:327` 的 `atomcode: boolean` 属 `VisibleApps`（`:312`），是应用可见性配置、合法保留。`typecheck` 退出码 0 佐证。

### 11.3 判定升降与口径说明

| 判定项 | 主体快照 `7b5b5771` | 代码状态 `8941b0ae` | 代码状态 `5e6d6156`（当前） | 理由 |
|---|:--:|:--:|:--:|---|
| #2 多语言 | ⚠️ | **✅** | ✅（不变） | `8941b0ae` 修复 7 语言缺 `apps.atomcode` 与 3 个死 key；`5e6d6156` 补 `apps.kimi`/`apps.dsh` 并新增 `tests/i18n/appIdsI18n.test.ts` 防回归，进一步加固 |
| #4 Skills / MCP | ❌ | **❌（不变）** | ❌（不变） | 始终走「移除前端类型声称」的诚实性修复；`5e6d6156` 新增测试显式断言 `skillAppEnabled({}, "atomcode") === false`（注释「not wired yet」），与后端恒 false 互证 |
| #7 白名单 | ⚠️ | ⚠️ | **✅** | `8941b0ae` 仅补齐核心动作白名单，残留 kimi 专属硬编码；`5e6d6156` 把报错翻译 / onSettled 重读 / env 跳过列表一并同步到 atomcode，并补 JSON 编辑器 `setKeyConflict(false)` → 原 ⚠️ 子项全部闭环 |
| #1 CLI 一致性 | ⚠️ | ⚠️（不变） | ⚠️（不变） | 工具安装/卸载全无、`AppsPage` 用 `Partial<Record>` 容忍缺失、目录设置未接等缺口仍在；仅 §4.1 的 deeplink 子项由 ❌ 调为 ⚠️ |
| 其余 9 项 | — | 不变 | 不变 | `8941b0ae` / `5e6d6156` 的文件不涉及图片、截图、Skills/MCP 后端、legacy 导入、启动回填等其余判定项代码 |

**统计变化**：✅4 / ⚠️5 / ❌4 →（`8941b0ae`）✅5 / ⚠️4 / ❌4（#2 升 ✅）→（`5e6d6156`）**✅6 / ⚠️3 / ❌4**（#7 升 ✅）。QA 宽松口径备选（若「矛盾消除即算部分满足」则 #4 升 ⚠️，统计 ✅5 / ⚠️5 / ❌3）对 `8941b0ae` 适用；`5e6d6156` 后若同样放宽则 #4 升 ⚠️ → ✅6 / ⚠️4 / ❌3。**本报告采用严格口径**（评审诉求是能力或明确处置，而非仅消除自相矛盾）。

### 11.4 独立回归验证（QA，只读，约 2 分钟）

| 命令 | 退出码 | 结果 |
|---|:--:|---|
| `pnpm typecheck` | 0 | 无输出 |
| `pnpm format:check` | 0 | `All matched files use Prettier code style!` |
| `pnpm exec vitest run src/config/atomcodeProviderPresets.test.ts` | 0 | 11 passed |
| `pnpm exec vitest run tests/components/AtomCodeProviderForm.test.tsx` | 0 | 4 passed |
| `pnpm exec vitest run tests/components/ProviderList.test.tsx` | 0 | 39 passed（合计 54 passed） |
| 三文件合并跑 | 1 | 断言全绿，2 个 `EPERM …\Temp\…\web\40b9f1…` unhandled error 拉红进程码（baseline-browser-mapping 缓存写被拒），与主体快照同因的环境噪音 |

**未跑**（按约定边界）：全量 `pnpm test:unit`、所有 cargo 命令、`pnpm build:renderer`、PR 描述正文 / `gh pr view`、真机 `/model` 验证。`8941b0ae` **未新增/修改任何测试**；`ProviderEmptyState` 组件在仓库内也没有对应测试文件。

**`5e6d6156` + 未提交工作区的补充实测（2026-11-14）**：提交前在该内容状态下重跑 `pnpm typecheck`（0）、`pnpm format:check`（0）、`vitest run` AtomCodeProviderForm(4) + ProviderList(39) + **新增 `tests/i18n/appIdsI18n.test.ts`(82 用例，首跑曾抓到 10 个真实失败即 `apps.kimi`/`apps.dsh` 基线缺失，补齐后全绿）** → 合计 **125 passed / 0 failed**；`atomcode_config.rs` 注释改动后重跑 `CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test --lib atomcode_ -- --test-threads=1` → **11 passed / 0 failed**（编译警告仅既有 agy dead_code）。首跑 `cargo test` 时 `-j 2` 误置于 `--` 之后被测试二进制拒绝（exit 101，仅参数解析失败），修正参数位置后通过。

### 11.5 仍未处理的缺口（截至 `5e6d6156` + 未提交工作区）

**已由 `5e6d6156` 闭环（自 `8941b0ae` 起的变化）**：
- PR 模板 `.github/pull_request_template.md` 已被修改（诚实勾选提示 + clippy 全 workspace 口径 + `pnpm test:unit` / `cargo fmt --check` / `cargo test`），不再「完全未改」；但 `(if Rust code changed)` 条件句仍保留；
- kimi 专属硬编码同步（报错翻译 / onSettled 重读 / env 跳过列表）、JSON 编辑路径 `setKeyConflict(false)`、覆盖所有 `APP_IDS` 的 `apps.<id>` 一致性断言（新增 `tests/i18n/appIdsI18n.test.ts`）—— #7 升 ✅ 的支撑，详见 §11.8。

**仍未处理（与 `8941b0ae` 一致，未因 `5e6d6156` 改变）**：
- 截图 0 张、`docs/images/` 无 atomcode 目录、PR 模板截图表为空（#3a）；
- `src-tauri/src/atomcode_config.rs` 的 `get_providers()` 仍只解析 `[provider_accounts.*]`，官方 legacy `[providers.*]` 格式导入仍为 0（#10）；
- CLI 工具安装/卸载管理、`AppsPage.tsx` 的 `Partial<Record>` 容忍缺失、目录设置接入（`ATOMCODE_HOME` 之外）、启动回填的 SSOT 冲突窗口（#1 / #11）；
- Skills / MCP / deeplink 后端仍 `false` / `None` / `skip`（#4）；
- `current_provider()` 空值残留与 `default_provider`/`default_model` 语义统一（#8）；
- **文档依据类**：`docs/atomcode-integration.md` 的 `v0.11.0` / `some-repo` / `example` 占位链接**已由未提交工作区重写处置**（v5.2.7 / `0c0512143` / 官方文档地址 / 6 处源码行号，见 §11.7、§11.8），但**尚未入库**；`atomcode_config.rs` 仅补文档注释（未提交）。

> 即 §11.5 原列的 6 项中，第 4 项（PR 模板）、第 5 项中的 kimi 硬编码 / JSON keyConflict / apps.<id> 断言已被 `5e6d6156` 消除；第 1 项（文档依据）转入未提交工作区处置；第 2、3、6 项（legacy 导入 / 截图 / CLI 工具·目录·启动回填 / Skills·MCP）维持原状。

### 11.6 版本同步记录（2026-11-14 00:34）

本次更新由 `52108a48` 之后的「更新一下报告」触发，**只调整报告的时点标注与统计，未新增任何核验或判定**：

1. 头部元信息区分三个时点：主体快照 `7b5b5771`、最后一次源码变更 `8941b0ae`、当前报告版本 `52108a48`；
2. §11 标题、§11.1 时间线补入 `5843fe16`（报告入库）与 `52108a48`（§11 修订入库）两条**纯文档提交**，并说明二者各只改动本报告一个文件；
3. §1、§10、§11.3、§11.5 的「截至」表述统一改为「代码状态 `8941b0ae` / 报告版本 `52108a48`」，明确判定结论未因文档提交改变；
4. §10 分支统计更新为 **18 个提交、68 文件（+4080/-96）**，并说明 +84 行增量全部来自报告文件自身（416 → 500 行）、删除行数不变；
5. §7.1、§8、§11.4 的实测时点标注补上「结论对 `52108a48` 仍有效」。

> 行数口径：§10 的 `+4080/-96` 与「416 → 500 行」均指 **已提交的 HEAD `52108a48`**。本次版本同步把报告由 500 行扩至 **519 行**（+38/-19），属于**未提交的工作区修改**，不计入上述 HEAD 统计；提交后 `git diff --shortstat upstream/main...HEAD` 的 insertions 会再增加，删除行数仍不变（该报告相对基线始终是纯新增文件）。

**未变内容**：所有判定（✅5 / ⚠️4 / ❌4）、#2/#4 的理由、§3–§9 的硬证据、§11.2 的 `14 → 10` 计数勘误、§11.5 的缺口清单。原因是 `8941b0ae` 之后无源码改动，重新核验不会得到不同结论；本次也未重跑任何测试命令。

### 11.7 语义冲突指控修正（2026-11-14，基于 AtomCode 本地源码）

贡献者提供 AtomCode 本地源码（`E:\project\atomcode`，workspace v5.2.7，commit `0c0512143`，2026-11-13），据此修正本报告 **§4.3 ③ / §5「核实后仍成立的部分」** 中的「语义冲突，需要源码级依据解释」表述：

| 字段 | AtomCode 源码事实 | 出处 |
|---|---|---|
| `default_provider` | **legacy 字段**：一个 `[providers.*]` 键，被 `default_model` 取代（design §14.1）；legacy 配置经投影后也可承载新 schema 的 selection id | `crates/atomcode-config/src/config/mod.rs:633-637` |
| `default_model` | **canonical 选择**（新 schema）：`[models.<id>]` 的 selection id | `mod.rs:661-665` |
| 解析顺序 | `effective_model_selection()` = `default_model` 优先，回退 `default_provider` | `mod.rs:1448-1457` |
| 程序化切换 | CLI 把**两个字段写成同一个 selection id**（「`default_model` is canonical; sync the legacy field too」） | `crates/atomcode-cli/src/main.rs:3215-3219` |

**结论修正**：本分支 `select_provider_model`（`atomcode_config.rs:443-444`）的双写与 `current_provider()` 的 `models[default_provider].account` 反查**与 AtomCode 自身行为一致，语义冲突不成立**。`default_provider` 并非账号 id —— 账号经模型键的 `account` 字段到达，这与 AtomCode 推荐的 selection id 格式（`<account>/<model>`，`mod.rs:657-660`）相容。已同步落地：

1. `atomcode_config.rs:75-88` 注释补 legacy 双写出处；
2. `docs/atomcode-integration.md` 重写版本章节（v5.2.7 / `0c0512143` / 官方文档地址 / 6 处源码行号引用）、新增 *Selection Semantics* 节、替换占位链接、按 `provider_preset.rs` 修正类型清单、Known Limitations 1 与 Future Improvements 1–2 改为「legacy 格式由 AtomCode 自身迁移（`mod.rs:1459`）」的明确处置。

> 上述两处改动均位于**未提交工作区，未入库**（详见 §11.8 第 5 点）；`5e6d6156` 的提交信息虽提及「Verification report: sync timeline metadata」，但其 `--stat` 仅含 12 个文件、不含 `docs/atomcode-integration.md` 重写与 `atomcode_config.rs` 注释。

**判定影响**：评审 #10 的「语义混用需源码解释」子项已闭环（本节即源码依据）；#10 的其余缺口（legacy `[providers.*]` 格式导入、官方文档未写 `[provider_accounts.*]` 表名）仍以 Known Limitations 形式如实记录。#8 中 `current_provider()` 空值残留（①②④）与本次修正无关，仍然成立。

### 11.8 同步至 `5e6d6156` + 未提交工作区（2026-11-14 22:06）

本次「更新一下报告」触发，把核验结论同步到 HEAD `5e6d6156` 与未提交工作区。本次（22:06）同步本身**未重跑**全量实测命令，但 §11.4 已记录 11-14 上午在 `5e6d6156` + 工作区状态下的**补充实测**——`typecheck`（0）/ `format:check`（0）/ `vitest run` 合计 **125 passed**（含新测试首跑抓到 `apps.kimi` / `apps.dsh` 基线缺失的 10 个真实失败，补齐后全绿）、`cargo test --lib atomcode_` **11 passed**；以及首跑 `cargo test` 时 `-j 2` 误置于 `--` 之后被测试二进制拒绝的一次 **exit 101（仅参数解析失败）**，修正参数位置后通过——故 §8 退出码结论对 `5e6d6156` + 工作区仍有效。

1. **时点与统计**：头部元信息区分「原始快照 `7b5b5771` / 源码状态 `5e6d6156` / 报告版本 `5e6d6156` + 工作区」；§1、§2、§11.3 判定统计由 ✅5 / ⚠️4 / ❌4 更新为 **✅6 / ⚠️3 / ❌4**（#7 由 ⚠️ 升 ✅）；§10 分支统计更新为 **19 提交、71 文件（+4217/-108）**。
2. **#7 升 ✅ 的依据**：`5e6d6156` 把 kimi 专属硬编码同步到 atomcode——`errorUtils.ts` 新增 `translateAtomcodeProviderMutationError`（映射「默认模型正在被使用」为 `atomcode.error.defaultModelInUse`）、`mutations.ts` 的 add/update/delete 三处接入该翻译、onSettled 重读由 `kimi` 扩为 `kimi || atomcode`（失效键 `["providers", appId]`）、`App.tsx:876` 的 env 冲突跳过列表加入 `atomcode`、`AtomCodeProviderForm.tsx:604` 的 JSON 编辑器路径补 `setKeyConflict(false)`（见 §4.2、§4.4）。
3. **i18n 加固**：`apps.kimi` / `apps.dsh` 补进 5 核心语言（zh/zh-TW/en/ja/tr），新增 `atomcode.error.defaultModelInUse`；新增 `tests/i18n/appIdsI18n.test.ts` 断言所有 `APP_IDS` 在 en + 5 核心语言都有 `apps.<id>` 且非裸 key，并断言 `skillAppEnabled({}, "atomcode") === false`（§7.1 第 5 点、§11.5 原 P1 建议已闭环）。
4. **PR 模板诚实化**：`.github/pull_request_template.md` 新增「逐项真实勾选、不适用写 N/A + 理由」提示、clippy 全 workspace 口径（`-D warnings`，勿只跑 `--lib`）、补 `pnpm test:unit` / `cargo fmt --check` / `cargo test`；但 `(if Rust code changed)` 条件句仍保留（§6.3、§9 P1 #6）。
5. **未提交工作区（未入库）**：`docs/atomcode-integration.md` 重写（v5.2.7 / commit `0c0512143` / 官方文档地址 / 6 处源码行号、新增 *Selection Semantics* 节、替换占位链接、Known Limitations 1 与 Future Improvements 1–2 改为「legacy 格式由 AtomCode 自身迁移（`mod.rs:1459`）」），`src-tauri/src/atomcode_config.rs` 仅补文档注释（无行为变更），本报告自身改动。
6. **判定影响**：#2 / #4 / #8 / #10 / #3a 等其余 9 项判定不变；#4 因新测试「not wired yet」断言而进一步证伪；#10 的依据类缺口（v0.11.0 / 占位链接）已由工作区重写处置但未入库，legacy `[providers.*]` 导入实现仍缺。

**未变内容**：§3–§9 的硬证据、§11.2 的 `14 → 10` 计数勘误、§11.4 的回归实测（11-14 上午在 `5e6d6156` + 工作区状态下所跑，非本次 22:06 同步所跑）、§11.7 的语义修正结论均保持有效；本次（22:06）同步未重跑 `pnpm test:unit` / 任何 cargo 命令 / `pnpm build:renderer`。

---

## 12. 独立复核：只读源码核对 + 失焦实测（2026-11-15 00:15）

**触发**：用户重发评审意见全文并要求「检查一下」，重点为原句被截断的 #12。本次以**当前工作区**（HEAD `5e6d6156` + 3 个未提交文件：`docs/atomcode-integration.md`、`src-tauri/src/atomcode_config.rs`、本报告）为准，独立复核 §1–§11 的判定是否仍成立，并坐实 #12。

### 12.1 方式与边界

- 以只读为主：`git status --short` 首尾均为原有 3 个 `M` 文件，**未修改任何源码、i18n、PR 模板、Rust 或测试文件**；本次改动仅为本报告。
- 唯一临时写入为探针 `tests/components/__probe.atomcode-blur.test.tsx`，实跑后**已删除**；探针用 `@testing-library/react` 真实渲染表单、对模型键输入框执行 `fireEvent.change` + `fireEvent.blur` 并读取 DOM `value`。
- 命令 `pnpm exec vitest run tests/components/__probe.atomcode-blur.test.tsx --reporter=verbose`：退出码 **0**，`1 passed / 3 passed`（Vitest 2.1.9，11.27s）。首跑在前台超时收到 `SIGTERM`（非测试失败），重跑并重定向日志后正常完成，输出见 §12.2。
- 未跑：全量 `pnpm test:unit` / `cargo test` / `pnpm build:renderer` / `gh pr view` / 真机 AtomCode `/model` 验证；未对 `upstream/main` 做基线对照。

### 12.2 #12 实测结果（坐实）

| 场景 | 输入后 `value` | 失焦后 `value` | 附带的界面反馈 |
|---|---|---|---|
| A. `m2` 改成已存在的 `m1` | `"m1"` | **`"m2"`** | 出现 `atomcode.form.modelKeyDuplicate` 提示，输入被静默打回旧键 |
| B. `m2` 改成合法新键 `m2new` | `"m2new"` | `"m2new"` | 无；稳定 `rowIds` 生效，确认「每输入一字符失焦」已修复 |
| C. 输入 `"m1 中文"` | `"m1 中文"` | `"m1"` | 无；非法字符拖到失焦才被吞 |

**根因（两处配合，较 §4.4 原描述更精确）**：

1. `AtomCodeProviderForm.tsx:225-232` 重名冲突时 `setKeyConflict(true); return`，父级 `value={key}` 不变；`ime-safe-input.tsx:67-83` 的 `onBlur` 无条件 `setDraft(value)`，把内部 draft 打回旧键 → **A**。
2. 模型键输入框（`:554-562`）**未传 `normalize`**，而供应商键输入框（`:406-415`）传了 `normalizeKey`；`onValueChange` 内的手动 `normalizeKey(next)` 只作用于父级 config，`ImeSafeInput` 的 draft 仍保留非法字符 → **C**。同页两处不一致，亦呼应评审 #1「前端不要自做一套，确保一致性」。

**判定影响**：#12 由 `⚠️ / ❓` 收敛为 **`⚠️`（已坐实）**——原句截断不再构成判定空白，评审指向的现象已可复现；但「每字符失焦」确已修复，故仍属部分满足。§1 统计 **✅6 / ⚠️3 / ❌4 不变**，§2 表格与 §4.4 已同步。

### 12.3 抽查复核（与既有判定一致，无回归）

| 抽查项 | 当前工作区事实 | 判定 |
|---|---|:--:|
| #5 / #6 | `App.tsx` 9 处 atomcode 分支（`:879/1132/1154/1308/1328/1339/1692/1705/1721`）；`ProviderList.tsx:187-215` 穷举 switch，atomcode 与 kimi/dsh/mcode 同走 `liveConfigManaged`，`default → false` | ✅ 不变 |
| #9 / #11 | `atomcode_config.rs:312-326` 默认模型删改校验；`:382` `save_and_commit` 先写 live、成功后提交 db | ✅ 不变 |
| #8 | `settings.rs:1519` 对 AtomCode 返回 `None`、`:1545` setter 为空分支 | ⚠️ 不变 |
| #2 | 5 核心语言 `atomcode.*` 23/23、7 语言 `apps.atomcode` 齐全；ko / ru 各 1/23（精确缺口见 §7.1） | ✅（按项目约定）不变 |
| #3a | 分支 diff 无图片文件，`docs/images` / `assets/screenshots` 无 atomcode 命名，PR 模板截图表仍为空 | ❌ 不变 |
| #3b | 模板已补 `pnpm test:unit` / `cargo fmt --check` / `cargo test`、N/A 提示与 clippy 全 workspace 口径；但 `:32-36` 的「（如修改了 Rust 代码 / 前端代码）」条件句仍在，且由原 1 处扩为 **4 处** | ❌ 不变 |
| #4 | `app_config.rs:57/197/219` 恒 `false`，无 `enabled_atomcode` 列 / 迁移 / 测试 | ❌ 不变 |
| #10 | `get_providers()`（`:64`）仍只解析 `provider_accounts`；`docs/atomcode-integration.md` 已重写为 v5.2.7 / `0c0512143` / 官方地址 / 6 处源码行号，但**仍未入库**；`atomcode_config.rs` 工作区改动仅为 `:75-88` 文档注释 | ❌ 不变（依据类部分推进但未入库） |

### 12.4 结论与可选下一步

- §1–§11 的判定在 `5e6d6156` + 当前工作区下**全部仍成立**，未发现回归或与源码冲突的新事实。
- **仓库内可低成本闭环、但需用户决策**：① #12 的 A / C 两类失焦丢值（纯前端逻辑 + 回归测试）；② ko / ru 各缺 22 条 `atomcode.*` 译文（严格按评审 #2 字面口径，项目约定下可豁免）；③ PR 模板 4 处条件句；④ 把 `docs/atomcode-integration.md` 重写与 `atomcode_config.rs` 注释**入库**。
- **需外部输入、仓库内无法独立解决**：截图（需真机）、Skills / MCP 接线（需 `enabled_atomcode` 列 + 迁移，AGENTS.md 要求改 schema 时 @ 维护者）、官方 `[providers.*]` 格式导入的产品决策、贡献者对适配版本 / 源码依据的正式确认、PR 描述正文「默认全勾选」的更正。
- 本次按用户指示**只更新本报告，未改动任何源码**；探针文件已删除，`git status --short` 首尾均为原有 3 个 `M` 文件。用户随后（00:29）要求「更新一下报告」，据此补录 §0 评审意见原文并同步 §1 / §2 / §4.4 / §9 / §10 的相关表述，**仍为纯文档更新、判定与统计不变**，本轮明确不修 A / C 两类失焦丢值。

---

*报告由软件团队三路并行核验（代码事实 / 架构一致性 / QA）后汇总，主理人对关键结论另行交叉复验；§11 增量修订由 QA 对 `8941b0ae` 独立回归核验后汇编；§11.6 为截至 `52108a48` 的版本同步记录；§11.7 为基于 AtomCode v5.2.7 本地源码的语义修正；§11.8 为同步至 `5e6d6156` + 未提交工作区的版本记录；§12 为 2026-11-15 的只读独立复核与 #12 失焦实测；§0 为用户提供的评审意见原文，作为全部判定的核验输入。*
