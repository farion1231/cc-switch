# AtomCode 适配 PR 评审意见逐条核验报告

- **核验对象**：分支 `feat/atomcode-provider`，HEAD `7b5b5771`
- **对照基线**：`upstream/main` = `bd487e83`（含 GitHub Copilot CLI 支持 `#8753`）
- **核验日期**：2026-11-13
- **核验方式**：只读。源码逐行核对 + `git diff upstream/main...HEAD` + 前端/后端实测 + 联网核实 AtomCode 官方文档；**未修改任何仓库文件**（`git status --short` 首尾均为空）
- **协作方式**：三路并行核验后交叉复核 —— 代码事实（9 条）、架构一致性 / Skills·MCP / 出处 / SSOT、i18n / 截图 / 检查清单 / 测试，主理人汇总并对关键结论独立复验
- **关联文档**：[2026-11-11 代码审查](./2026-11-11-code-review-211aff9a.md)、`../atomcode-fixes-summary.md`、`../atomcode-integration.md`、`../atomcode-provider-integration-zh.md`

---

## 1. 结论摘要

> **评审意见未全部满足。** 13 个判定项中，真正闭环的 **4 项**（全部是代码层具体缺陷），**5 项部分满足**，**4 项未满足**（其中 1 项含因原文截断无法确认的部分）。

| 判定 | 数量 | 条目 |
|---|---|---|
| ✅ 已满足 | 4 | App.tsx 未改、`isInConfig` 默认分支、默认模型删改保护、更新路径写序 |
| ⚠️ 部分满足 | 5 | CLI 参考既有实现、多语言、白名单、`current_provider`、模型键失焦 |
| ❌ 未满足 | 4 | 截图、检查清单、Skills / MCP、官方文档格式·导入·版本出处 |

**可合并的核心判断**：

1. 上一轮 `docs/atomcode-fixes-summary.md` 声称"已修复"的**代码类**问题，绝大多数属实且有回归测试支撑；
2. 但**能力类**（Skills / MCP / 工具管理 / deeplink）、**交付物类**（截图）、**依据类**（适配版本、配置格式源码出处、官方格式导入）基本没有实质推进，部分被文档明确标注为"未接线 / 留待合并前补齐"；
3. 核验中**新发现一个本分支引入的用户可见缺陷**：`apps.atomcode` 在 7 个语言文件中全部缺失，界面会渲染裸 i18n key；
4. `docs/atomcode-fixes-summary.md` 自身存在多处与源码不符的表述（详见 §7），PR 描述中"全部勾选"的检查清单因此缺乏可信基础。

---

## 2. 逐条结论表

| # | 评审意见 | 判定 | 一句话依据 |
|---|---|:--:|---|
| 1 | 新增 CLI 类型应参考项目内既有实现，而非只指定几个供应商；前端不要自做一套，确保一致性 | ⚠️ | 枚举 / 表单 / 呈现层 / 图标 / IPC 注册复用了既有扩展点；但工具安装卸载、目录设置、MCP/Skills、deeplink 缺口明显，预设清单为自建 517 行，`AppsPage` 用 `Partial<Record>` 容忍缺失代替接入 |
| 2 | 多语言支持缺失，请补全 | ⚠️ | 5 个核心语言 22/22 新增 key 齐全，ko/ru 靠 `fallbackLng: "en"`（符合约定）；但 `apps.atomcode` **7 语言全缺**，动态插值渲染裸 key；另有 3 个定义未被引用的死 key |
| 3a | 缺少截图示例 | ❌ | 分支 0 张图片、`docs/images` 无 atomcode 目录、PR 模板截图表为空，中文集成文档自承"留待合并前补齐" |
| 3b | 检查清单默认全勾选仍保留"如修改了 rust 代码"的提示，不可信 | ❌ | `.github/pull_request_template.md` 本分支 **blob 与基线完全一致（未改）**，4 项仍为 `- [ ]`，`:29` 条件提示仍在；本分支实际改了 19 个 `src-tauri/` 文件 |
| 4 | 需要考虑已有的 Skills / MCP 支持 | ❌ | 后端多处显式 `false` / `None` / `log::debug!("...not wired yet, skipping")`，中文文档自承"未接线"；前端类型却声称支持 |
| 5 | `App.tsx` 一行都没改 | ✅ | 相对基线 +33/-10（43 行），切换 / 添加 / 移除 / 复制四处 atomcode 分支全部在列 |
| 6 | `ProviderList.tsx` 判断"在不在配置里"时 AtomCode 走默认分支直接返回 `true` | ✅ | 改为穷举 switch，atomcode 与 kimi/dsh/mcode 同走 `meta.liveConfigManaged`，`default` 返回 `false` |
| 7 | "设为默认"和"从配置移除"的白名单都没有 atomcode | ⚠️ | 核心动作白名单已补齐并复用共存型逻辑；但若干 kimi 专属硬编码（失败重读、错误翻译、env 冲突跳过列表）未同步 |
| 8 | 后端取当前供应商的函数对它返回空 | ⚠️ | `current_provider()` 现返回 account ID 而非模型键；但 `default_provider` 缺失 / 无 `models` 表时仍返回空串，且与 `default_model` 语义混用 |
| 9 | 编辑当前账号时删掉或改名默认模型，会让默认模型指向不存在的条目 | ✅ | `apply_provider` 校验新 `models` 仍包含 `default_model`，删除与改名一并拒绝；测试真实有效 |
| 10 | 官方文档只写 `[providers.*]` + `default_provider`、类型仅 openai/claude/ollama 三种；按官方文档配置点"导入"什么都拿不到；deepseek/gemini/openai_responses 与 `ATOMCODE_HOME` 查不到出处；请贡献者说明适配版本并给源码依据 | ❌ | 导入链只解析 `[provider_accounts.*]`，文档自列 Known Limitation；PR 文档引用非官方地址与占位域名、版本号查无出处、未给源码依据。**其中 2 处评审细节需修正，见 §5** |
| 11 | 更新路径先写库，会在下次启动时被配置覆盖 | ✅ | update 路径严格"先写 `config.toml`、成功后才提交库"，注释点明防回填覆盖，另有 4 条写序回归测试。⚠️ 架构层启动时 live→db 单向覆盖窗口仍在（项目级既有设计，非本分支引入） |
| 12 | 模型键输入框失焦…（评审原文被截断） | ⚠️ / ❓ | 稳定 `rowIds` 已修复"改键导致行重挂载失焦"，`update()` 统一清 `keyConflict` 修掉 P3 残留提示；但 JSON 编辑路径未清 `keyConflict`，且原句截断、仓库三份文档中无该评审原文，具体现象无法完整确认 |

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

以 `bd487e83`（Copilot CLI）为项目内既有范例逐点对比，22 个扩展点中 **12 项复用、5 项半接、5 项缺失**：

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
| **deeplink** | `build_settings` + `merge_config` + preview + dialog | 后端返回 `atomcode.deeplink_unsupported`，前端 `deeplink.ts:26` 反而允许 atomcode | ❌ |
| 会话管理 / 用量 / 打开终端 / 额度卡 | 均有 | 未接（共存型未必全需） | ❌ |
| 文档 | README + 带实测依据的 integration-zh | 3 份 docs，无可验证版本/源码依据 | ❌ |

另有夹带无关改动：`src/components/usage/UsageTrendChart.tsx:221-238`、`src/hooks/useDragSort.ts:13-25`（locale 抽取），影响 diff 纯净度。

### 4.2 白名单的 kimi 专属硬编码未同步（评审 #7 的 ⚠️ 部分）

`src/lib/query/mutations.ts:240-248` 与 `:323-331`：

```ts
if (isPiFamilyApp(appId)) { ... }
else if (appId === "kimi") {
  // 默认模型可能在 Kimi Code 里改过：失败时也重读
  await queryClient.invalidateQueries({ queryKey: ["providers", "kimi"] });
}
```

AtomCode 同样有 `default_provider`，但失败时不会重读 → 默认 badge 可能不同步。此外：`mutations.ts:158/228/311` 与 `App.tsx:1150` 的错误翻译只对 kimi（AtomCode 会显示后端英文原文）；`App.tsx:874-880` 的 env 冲突跳过列表 `mcode|dsh|kimi|agy|copilot` 不含 atomcode。

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

残留：① `default_provider` 缺失/空 → 空串；② `models` 无该键或无 `account` → 空串（官方 `[providers.*]` 格式无 `models` 表时**必空**）；③ 注释把 `default_provider` 当"模型键"，而官方定义为 `[providers.*]` 的 key（见 §5）；④ `src-tauri/src/settings.rs:1514-1519` `get_current_provider` 对 AtomCode 返回 `None`、`:1540-1545` setter 是 no-op。

### 4.4 模型键输入与 keyConflict（评审 #12）

- **已修**：`AtomCodeProviderForm.tsx:234-243` 改键时 `rowIds` 跟着键走，`key={rowIds[key] ?? key}`（`:539`）保证行不重建、焦点不丢；`ImeSafeInput`（`:543-573`）处理中文输入法。
- **已修（P3）**：`:172-178` 的 `update()` 统一 `setKeyConflict(false)`，`removeModel` / `choosePreset` / `updateModel` 成功路径都走它。
- **未修**：`:600-610` 的 `JsonEditor.onChange` 不经过 `update()`、不调用 `setKeyConflict(false)` → 通过 JSON 编辑解决冲突后"该模型键已存在"提示仍在。而提交 `d6d55d57` 的信息写着 "Clear keyConflict warning in **all** config update paths ... via **JSON editor**"，源码不支持。
- **仍可复现的失焦丢值**：`src/components/ui/ime-safe-input.tsx:67-83` 的 `onBlur` 会把内部 `draft` 重置为外部 `value`；当 `updateModel` 因重名提前 `return`（`AtomCodeProviderForm.tsx:225-232`）时父级 `value={key}` 未变，失焦后用户输入被丢弃。
- **❓ 无法确认**：评审原句"模型键输入框失焦…"被截断，仓库三份文档中无该条记录，因此无法判定评审具体指向哪种现象。

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
- 官方 `default_provider` = `[providers.*]` 中定义的 key；本分支 `current_provider()` 注释称其为"模型键"并用 `models[default_provider].account` 反查，`select_provider_model`（`:443-444`）还把 `default_model` 与 `default_provider` 写成同一个模型键 → **存在语义冲突，需要源码级依据解释**。
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
| `src-tauri/src/deeplink/provider.rs:408-417` | 明确返回 `atomcode.deeplink_unsupported`；而 `src/lib/api/deeplink.ts:22-27` 把 atomcode 加入可选 app |
| `src/types.ts:855-860`、`src/lib/api/skills.ts:17-42` | 前端类型新增 `McpApps.atomcode?` / `SkillApps.atomcode?` → **声称支持，后端恒 false** |
| `docs/atomcode-provider-integration-zh.md:16-17` | 表格自承「MCP 未接线：需要数据库 schema 新列…静默跳过」「Skills 未接线」 |

`git grep` 确认以下文件在 HEAD 中完全不含 atomcode：`deepLinkConfigPreview.ts`、`promptUtils.tsx`、`DeepLinkImportDialog.tsx`、`UnifiedMcpPanel.tsx`、`SkillImportDialog.tsx`、`src-tauri/src/deeplink/mcp.rs`、`src-tauri/src/database/dao/skills.rs`、`presetGroups.ts`、`ProviderPresetSelector.tsx`。

对照 `bd487e83` 的既有正确做法：`database/schema.rs` 新增 `enabled_copilot` 列 + `24 => 25` 迁移 + 2 个测试、新增 `mcp/copilot.rs`(519 行)、`services/mcp.rs` +37、`services/skill.rs` +142、`deeplink/provider.rs` +72，且文档用 `copilot help providers/environment/config` 实测逐项给出原生格式依据。`AGENTS.md` 禁止随意改 `SCHEMA_VERSION` 是合理约束，但该提交演示了"与维护者协作加列 + 迁移 + 测试"的解法；本分支选择了"前端类型加了、后端恒 false"。

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

- 4 项全部 `- [ ]`（**未勾选**）；`:29` 条件提示仍在。`CONTRIBUTING.md:458` 也有同类条件句
- 本分支是否改过模板：`git log upstream/main..HEAD -- .github/pull_request_template.md` 为空；`git diff` 为空；`git rev-parse HEAD:... == upstream/main:... == 784a91dfab3d652a4590808b9adb3795221fac0b` → **完全未改**
- 本分支 Rust 改动：`git diff --name-only upstream/main...HEAD -- src-tauri/ | wc -l` = **19**（含新增 `atomcode_config.rs`、改 `lib.rs` / `services/provider/mod.rs` / `tests/mcp_commands.rs`）→ 条件句客观上「适用」
- **意见指向**：「默认全勾选」只可能发生在 PR 描述正文（GitHub 把模板 `- [ ]` 渲染成可点复选框），仓库内不存在任何已勾选清单 → 该意见指向 PR 描述文本，**仓库内无法验证 (❓)**；仓库可验证的只有「模板空勾选 + 条件提示 + 本分支未改」
- **可信度结论**：模板 `- [ ]` 本身自洽（不预置勾选、要求逐项确认），真正的信息失真发生在 PR 描述层，改模板无法追溯修正该 PR。但模板有两个削弱可信度的设计缺陷：① `clippy` 带条件句，把 19 个 Rust 文件改动下的必跑项变成作者自判「可跳过」，与 `AGENTS.md` 的后端全套不一致；② 清单只有 4 项，缺 `pnpm test:unit`、`pnpm build:renderer`、`cargo test`、`cargo fmt --check`。另有口径差异：`cargo clippy -- -D warnings` 通过，但 `cargo clippy --lib --tests -- -D warnings` 失败（见 §8）→ 若勾选 clippy 必须写明运行范围

---

## 7. i18n 矩阵与文档可信度

### 7.1 新增 key × 7 语言

`git diff upstream/main...HEAD -- src/i18n/locales/`：7 个文件全部修改，共新增 **22 个叶子 key**（全部在 `atomcode.*`，基线为 0）。核心 5 语言 22/22；ko / ru 仅 `form.modelKeyDuplicate` 1 个，其余 21 个走 `fallbackLng: "en"`（`src/i18n/index.ts:109`）。

| 新增 key（22） | zh | zh-TW | en | ja | tr | ko | ru |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| `empty.title`、`empty.description`、`sharedProviders` | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ⤵en | ⤵en |
| `form.providerKeyHint`、`providerKeyInvalid`、`providerType` | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ✅×3 | ⤵en | ⤵en |
| `form.baseUrl`、`baseUrlHint`、`apiKey`、`apiKeyPlaceholder`、`apiKeyHint` | ✅×5 | ✅×5 | ✅×5 | ✅×5 | ✅×5 | ⤵en | ⤵en |
| `form.models`、`addModel`、`noModels`、`modelKey`、`modelKeyPlaceholder`、`modelId`、`modelName`、`contextWindow`、`contextWindowPlaceholder`、`removeModel` | ✅×10 | ✅×10 | ✅×10 | ✅×10 | ✅×10 | ⤵en | ⤵en |
| **`form.modelKeyDuplicate`** | ✅ | ✅ | ✅ | ✅ | ✅ | **✅** | **✅** |

> 口径说明：按 `git diff -U0 | grep -c '^+'` 计为 28 行（含 `"atomcode": {`、`"form": {`、`},` 等 6 行结构行），按扁平化叶子 key 计为 22 个，两者不矛盾。

**符合约定的部分**：`AGENTS.md` 明确「ko / ru 为社区翻译，允许缺失并走 `fallbackLng: "en"`」，ko/ru 唯一新增的 `modelKeyDuplicate` 恰是 `docs/atomcode-provider-pr-audit-zh.md:118-121 (M5)` 要求补的那条 → M5 已修复。`ru.json` 整体仅 2556 个 key（en 为 4556），本就长期不完整。静态扫描 28 个改动的 `src/**/*.ts(x)` 提取 415 个不同 `t("字面量")` key，**0 个未在 `en.json` 定义**；新增行中无硬编码中文用户可见文案。

**⚠️ 新发现的本分支引入缺陷**：

1. **`apps.atomcode` 在 7 个语言文件中全部缺失**（`en.json` 的 `apps` 段仅 15 个 key）。动态调用点 `presentation.ts:968` → `t("providerCard.section.addedHelp", { app: t(\`apps.${app}\`) })` 与 `ProviderList.tsx:499` → `t("provider.liveConfigReadFailed", { app: t(\`apps.${appId}\`) })`。实测 i18next：`t("apps.atomcode") === "apps.atomcode"`，插值后用户看到 "…written into **apps.atomcode**'s config…" → **裸 key 泄漏**。
2. **3 个死 key**：`atomcode.sharedProviders`、`empty.title`、`empty.description` 定义后从未引用。`presentation.ts:966` 只对 `app === "dsh"` 特判 `t("dsh.sharedProviders")`，atomcode 落到通用分支（正是裸 key 成因）；`ProviderEmptyState.tsx:25-31` 的 `emptyCopyNs` 不含 atomcode。22 个 key 中仅 19 个真正被 `AtomCodeProviderForm.tsx` 使用。
3. **硬编码英文（低风险 / 有既有惯例）**：`atomcodeProviderPresets.ts` 的协议与厂商显示名在 `AtomCodeProviderForm.tsx:453` 直接 `{type.label}` 渲染；但 `copilotProviderPresets.ts:48-67` 等基线文件同样如此，属既有惯例。
4. `AGENTS.md` 被 `.gitignore:11` 忽略、`git ls-files AGENTS.md` 为空 → 它是本机 agent 指南，**"ko/ru 允许 fallback"的约定无法被 CI 或 PR 评审者从仓库引用**。

### 7.2 文档中"声称已修复"但源码不支持的条目

| # | 位置 | 声称 | 实际 |
|---|---|---|---|
| 1 | `atomcode-fixes-summary.md:63-73` | App.tsx「Already correct – no changes needed」+ 行号 1677/1690/1706/1324 | 这四处**正是本分支新增的 atomcode 白名单**；当前行号为 1685/1701/1717/1324；1324 现为模型 key 重写，`addToLive: false` 在 1335-1338 |
| 2 | `:39-61` | Update Path Ordering「Already correct」+ `atomcode_config.rs:285-318`、`mod.rs:7142-7150` | `211aff9a` 提交信息明确写 "Provider edits **now** write the live file before the database commit…"，说明是本次修复；实际位置 `376-384` / `7146-7156` |
| 3 | `:19` | 默认模型校验在 `atomcode_config.rs:225-256` | 实际在 `:305-324`（225-256 是 `write()` 尾部）；所声称的测试属实 |
| 4 | `:85-92` | ProviderCard Actions Whitelist「Not needed – already handled by presentation.ts」 | presentation.ts 确实处理，但 §4.2 的 kimi 专属硬编码未同步 |
| 5 | 提交 `d6d55d57` 信息 | "Clear keyConflict warning in **all** config update paths…via **JSON editor**" | `AtomCodeProviderForm.tsx:600-610` 的 `JsonEditor.onChange` 不经过 `update()` |
| 6 | `:5-11` | isInConfig「**now** correctly checks `meta.liveConfigManaged`」 | `211aff9a` 时 atomcode 已有专属 case；`78ed827e` 实际改的是 `default: true→false` |
| 7 | `:23-35` | Documentation「✅ Fixed – Supported AtomCode version (v0.11.0)」 | 只新增了一份自证文档，链接不可达、版本查无出处、截图未补，见 §5 |
| 8 | `docs/atomcode-integration.md` | `:5/:92` 声称基于 v0.11.0 | `:113` Future Improvements 却写「尚未记录确切版本/commit」；`:33-39` 类型清单与 `PROVIDER_IDS` 不符；`:43` 的 `ATOMCODE_HOME` 无出处 |
| 9 | `docs/reviews/2026-11-11-code-review-211aff9a.md:55` | 「i18n 七语言同步未发现问题」 | 与本次实测的 `apps.atomcode` 裸 key 冲突——该审查只扫静态 key，漏了动态 `t(\`apps.${app}\`)` |
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
| node + i18next 实跑 `t("apps.atomcode")` | — | 返回 `"apps.atomcode"` → 裸 key 泄漏实证 |
| `git status --short`（首/尾各一次） | — | 均为空 |

**新增/修改测试文件共 9 个（+1209 行）**：`tests/components/AtomCodeProviderForm.test.tsx`(A, 156)、`src/config/atomcodeProviderPresets.test.ts`(A, 205)、`tests/components/ProviderList.test.tsx`(M, +75)、`src-tauri/src/atomcode_config.rs`(A, 763，含 7 个 `#[test]`)、`src-tauri/tests/mcp_commands.rs`(M, +2)、`tests/msw/state.ts`(M, +4)、`AppsPage.test.tsx` / `SessionManagerPage.test.tsx` / `UsageDashboard.test.tsx`(M)；另有 `services/provider/mod.rs` 内 4 个写序测试。

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

1. **补 `apps.atomcode` 到 7 个语言文件**（至少 5 个核心语言），或让 `presentation.ts:968` / `ProviderList.tsx:499` 改用 `appConfig` 的 `label`；把 `atomcode.sharedProviders` 接进 `presentation.ts` 的 atomcode 分支；`atomcode.empty.*` 接进 `ProviderEmptyState.tsx` 的 `emptyCopyNs` 或删除冗余。
2. **Skills / MCP 二选一**：按 `bd487e83` 模式与维护者协作新增 `enabled_atomcode` 列 + 迁移 + 测试并全量接线（McpApps/SkillApps get/set、dao、services、deeplink）；或**移除** `src/types.ts` 的 `McpApps.atomcode?` 与 `src/lib/api/skills.ts` 的 `SkillApps.atomcode?`，并把 `deeplink.ts` 的 atomcode 从可选 app 移除，消除"前端可选、后端必拒"。
3. **导入与配置格式依据**：实现官方 `[providers.*]` 格式导入，或在 UI 明确提示「仅支持 v5.x `/provider` 生成的账号格式」，并删除/兑现 Known Limitation 第 1 条。
4. **由贡献者给出可验证凭据**（而非再写一份自证文档）：确切版本号 + commit SHA + 仓库 URL，修正/删除 v0.11.0；把 404 链接换成 `https://atomcode.atomgit.com/docs/zh/configuration.html` 并标注文档版本；替换 `some-repo` / `example` 占位链接；对 `[provider_accounts.*]` / `[models.*]` 字段规范、`default_provider` 与 `default_model` 的语义及优先级（含本分支把两者写成同一模型键与官方定义的冲突）给出源码依据；逐项标注 `deepseek` / `gemini` / `openai_responses` 等 id 的来源与归一化理由；为 `ATOMCODE_HOME` 补 `distribution.rs` 的源码依据。
5. **补交付物**：真实截图（PR 模板表 + `docs/images/`），附 `/provider` 实际写出的 config.toml 样例与真机 `/model` 验证结果。

### P1 — 一致性与清单可信度

6. 把 PR 描述检查清单改为逐项真实勾选；模板去掉 `(if Rust code changed)` 或改为「必填 / N/A + 理由」，补齐 `pnpm test:unit`、`pnpm build:renderer`、`cargo test`、`cargo fmt --check`，并写明 clippy 命令口径。
7. CLI 一致性：接工具安装/卸载（`TOOL_NAMES` / `TOOL_APP_IDS` / `tool_uninstall.rs`），恢复 `AppsPage.tsx` 的 `Record<AppId, Row>` 完整性；目录设置接入既有 override-dir 体系或明确"仅支持 `ATOMCODE_HOME`"并写入文档；预设清单复用共享供应商目录或补可验证出处并接 `presetGroups.ts` / `ProviderPresetSelector.tsx`。
8. 同步 kimi 专属硬编码（`mutations.ts:243-248/326-331`、错误翻译、`App.tsx:874-880` env 跳过列表）；统一 `current_provider()` 的空值与 `default_provider`/`default_model` 语义；JSON 编辑路径补 `setKeyConflict(false)`；请贡献者补全「模型键输入框失焦…」原句以便最终确认。
9. 去掉 `docs/atomcode-fixes-summary.md` 中「Already correct」「Documentation ✅ Fixed」等自证表述，按 §7.2 校正行号与描述。

### P2 — 架构与测试稳定性

10. 给 `import_atomcode_providers_from_live` 增加方向/冲突判定（时间戳、内容哈希或"已有 id 不覆盖、只新增"）；补一条「`write_live=false` 写 db 后模拟重启，断言 db 不被回退」的回归测试；`switch_and_commit`（`atomcode_config.rs:479-484`）改为字段合并而非整体替换；统一 `AGENTS.md` 的 SSOT 声明与实际启动行为。
11. 修复 `docs/reviews` P2 #2：用 Drop guard 自动清除 `crash_at(Some("atomcode:write"))`，避免断言 panic 时污染同 crate 后续测试；`atomcode_config.rs:639-657` 的 `set_var("ATOMCODE_HOME")` 同样缺 guard。
12. 清理与 AtomCode 无关的夹带改动（`UsageTrendChart.tsx`、`useDragSort.ts`）；决定 `prompt_files.rs` 的"接入"或"不接入"并同步 `promptUtils.tsx`。

---

## 10. 核验范围与局限

- 全程只读：`git status --short` 首尾均为空，未写入 / 编辑 / 提交任何仓库文件（本报告除外）。
- 基线 `upstream/main` = `bd487e83`；分支 `upstream/main..HEAD` 共 15 个提交、65 个改动文件（+3534/-68）；对照 `git show bd487e83`（Copilot CLI，105 文件 +4753/-179）。
- AGENTS.md 为被 gitignore 的本机指南，不在仓库内，其约定无法被 CI / PR 评审者引用；本报告按其内容作为判断依据并已标注。
- 联网核实仅覆盖公开可检索范围，未登录克隆 AtomCode 官方仓库逐行核对 `provider_preset.rs`；对版本号、第三方链接的结论已标注来源与局限，不作超出可检索范围的推断。
- PR 描述正文、GitHub PR 页面、真机 AtomCode 客户端均不在本次核验范围内。

---

*报告由软件团队三路并行核验（代码事实 / 架构一致性 / QA）后汇总，主理人对关键结论另行交叉复验。*
