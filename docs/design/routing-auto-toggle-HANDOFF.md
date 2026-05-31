# Routing Auto-Toggle — 交接文档（HANDOFF）

> 给接手的 agent：本机**没有可用的 JS 工具链**（只有 Adobe 自带 node.exe，无 npm/pnpm，`node_modules` 未安装），所以所有 TypeScript 的 `vitest` / `typecheck` **从未真正执行过**。你接手的第一件事就是装好工具链、跑验证。Rust 侧可正常 `cargo test`。

## 0. 必读
- 设计与决策全文：`docs/design/routing-auto-toggle.md`（Q1–Q11 决策、§4 状态机、§5 修改点、§6 任务拆分与验收、§7 冒烟、§7.5 Review 流程）。本文件只讲**当前进度 + 如何接手**。

## 1. 分支与基线
- 分支：`feat/routing-auto-toggle`，从 `main`（`8f83fa20`）切出。
- 已推送到 `origin/feat/routing-auto-toggle`。
- commit 序（origin/main 之上）：
  - `0d1e214e` docs：review 流程
  - `f88921f1` docs：设计提案
  - `35eb7752` feat：**wave 1**（本次交接的代码）

## 2. 已完成（wave 1）

| 文件 | 内容 | 验证状态 |
|---|---|---|
| `src-tauri/src/settings.rs` | `AppSettings` 加 `auto_enable_for_needs_routing` / `auto_disable_for_no_routing`（camelCase serde、`#[serde(default)]`、默认 false）+ 3 个单测 | ✅ **已亲验**：`cargo test --manifest-path src-tauri/Cargo.toml --lib settings` → 21 passed |
| `src/utils/providerRouting.ts` | 纯函数 `getProxyRequirement(provider, appId)` | ⚠️ **未验**（无工具链）。代码走读 + 已对照 `useProviderActions.ts` 源 |
| `src/utils/__tests__/providerRouting.test.ts` | 23 个 case | ⚠️ **从未运行** |
| `src/utils/switchDecision.ts` | 纯函数 `decideSwitchAction`（§4 状态机） | ⚠️ **未验** |
| `src/utils/__tests__/switchDecision.test.ts` | 全真值表 | ⚠️ **从未运行** |
| `src/components/ConfirmDialog.tsx` | 加可选 `checkbox?` prop（向后兼容） | ⚠️ **未验** typecheck/RTL |
| `tests/components/ConfirmDialog.test.tsx` | 2 个 RTL case | ⚠️ **从未运行** |

### wave 1 的一个关键设计点（已落地）
`getProxyRequirement` 的 Copilot 判定**故意做成 `ProviderCard` 现有徽章的超集**：同时认 `meta.providerType === "github_copilot"` 和 `meta.usage_script.templateType === "github_copilot"`。原因：T5 会把徽章统一到这个函数，若只认 `providerType`，仅靠 templateType 标记的 Copilot provider 会丢徽章、逃出 guard。对应测试已加。

### `reason` 字段约定
`getProxyRequirement().reason` 返回**稳定 i18n key**（如 `"notifications.proxyReasonOpenAIChat"`，见 `PROXY_REASON_KEYS` 常量），**不返回译文**——保持纯函数、不依赖 `t()`。调用方负责翻译。

## 3. 接手第一步：装工具链 + 跑验证（必须先做）

```bash
# 1. 装 Node（若无）。然后在仓库根：
corepack enable pnpm
pnpm install --frozen-lockfile

# 2. 跑 wave 1 的 TS 验证（这是我没能跑的部分）
pnpm typecheck
pnpm vitest run src/utils/__tests__/providerRouting.test.ts \
                src/utils/__tests__/switchDecision.test.ts \
                tests/components/ConfirmDialog.test.tsx

# 3. Rust（应已 green，复跑确认）
cargo test --manifest-path src-tauri/Cargo.toml --lib settings
```

**如果 typecheck/vitest 报错**：优先怀疑我走读时没发现的类型问题（如测试里 `as Partial<Provider>` 的写法、`ProviderMeta` 字段名、`@/` alias 解析）。这些都是「从未编译过」的代码，出错很正常，先修绿 wave 1 再往下。

## 4. 剩余任务（未开始）—— 见设计文档 §6

依赖序：**T5(依赖T2)‖T7(依赖T1) → T6(依赖T2/T3/T4) → T8**

- **T5** `ProviderCard.tsx`：把三处「需要路由」徽章块合并为一处，改用 `getProxyRequirement(provider,appId).required`；删除 `codexNeedsRouting` useMemo 及随之失效的 import（`extractCodexWireApi`/`isCodexChatWireApi`——删前 grep 确认无他用；`extractCodexBaseUrl`/`extractCodexExperimentalBearerToken` 要**保留**，被 extractApiUrl/isOfficialProvider 用）。两处「不支持路由」官方徽章**不动**。
- **T6** `ProviderList.tsx` + `ProviderActions.tsx`（核心 guard）：
  - **STEP 0（重要，防安全死锁）**：把 `isOfficialProvider` 从 `ProviderCard.tsx`（约 L69）**抽到 `src/utils/providerRouting.ts` 导出**，逐字复制现逻辑，ProviderCard 改 import。guard / card / actions 必须共用同一个 official 判定（broad 版，含空 base url 等），否则官方 provider 可能绕过 confirmDisable → 封号风险。
  - **STEP 1（Q9）**：`ProviderActions` 加 prop `autoDisableForNoRouting`，按钮 disable 判定改为 `isOfficialBlockedByProxy && !autoDisableForNoRouting`。
  - **STEP 2**：把 `settings?.autoDisableForNoRouting` 从 `ProviderList`（已读 settings query）穿 `ProviderCard → ProviderActions`。
  - **STEP 3（Q7/Q8）**：照搬 streamCheck 范式（`showStreamCheckConfirm`/`pendingTestProvider`/`handleStreamCheckConfirm`），加 `handleSwitchWithGuard`：用 `decideSwitchAction` 分流 → `direct`(autoEnable 静默路径仍要先 `await setTakeoverForApp(app,true)`) / `confirmEnable` / `confirmDisable` / `hardBlock`(no-op，按钮本应已禁用)。链式严格 await，**第 1 步 `set_takeover_for_app` 失败 → toast.error + 中止不切换**。「记住」勾选**点确认即写 settings**（独立于第 1 步成败）。
  - **STEP 4**：把卡片的 `onSwitch` 改成 `handleSwitchWithGuard`（注意经 `SortableProviderCard` 的 prop 透传链）。
  - **STEP 5**：两个 `ConfirmDialog`（复用 wave1 加的 checkbox prop）：enable 用 `variant="info"`，disable 用 `variant="destructive"`，文案见设计文档 §3-Q8/Q9 与 T6 任务描述。
  - **backstop 注意**：`useProviderActions.switchProvider`（约 L220）自带官方硬阻断 + return。confirmDisable 路径会先把 takeover 关掉再调 onSwitch，届时 `isProxyTakeover` 已 false，硬阻断不会触发——保留它作为兜底即可，**不要删**。
  - mutation：`useSetProxyTakeoverForApp()`（`src/lib/query/proxy.ts`），`mutateAsync({ appType, enabled })`。
- **T7** 设置面板：`Settings` 类型 / `src/lib/schemas/settings.ts` zod / `ProxyPanel.tsx`（在 `[1] enableLocalProxy` ToggleRow 之后加两个**始终可见**的 ToggleRow）/ `ProxyTabContent.tsx`（经 `onAutoSave` 透传）。**逐处镜像 `enableLocalProxy` 的写法**（grep 它所有出现点）。
- **T8** i18n：`zh.json / en.json / ja.json / zh-TW.json`，补齐 T6/T7 里所有 inline `defaultValue` 用到的 key（两对话框文案、「记住」、两开关 label/desc、失败 toast `notifications.routingEnableFailed`/`routingDisableFailed`、徽章 key）。

## 5. 测试硬性线（§3-Q11，Review 打回标准）
- **L1** `getProxyRequirement` + **L2** `decideSwitchAction` + 后端 AppSettings 默认值 Rust 单测 = **硬性**（必须有且真过）。
- **L3** 组件交互测试 = 尽力而为，不作打回条件。

## 6. Review 流程（§7.5）
每波实现完成 → 先派**独立 Review SubAgent** 审数据流/控制流 + 代码品味出独立报告 → 再由 lead 基于报告 + 复核做最终 Review（卡：①测试是否交付、②是否真覆盖验收、③是否触碰禁区）。

## 7. 禁区（§8 明确不做）
- 不碰「代理服务」全局总开关（`stop_with_restore` / `onToggleProxy`）；只用 per-app `set_takeover_for_app`。
- 不覆盖托盘切换（后端旁路 `tray.rs:handle_provider_click`，原生菜单弹不了对话框）。
- 不新建 DB 列/表（settings 走现有 KV / AppSettings serde）。
- 反向仅覆盖 `official`；无 `*_confirmed` 第三态（方案 X 单一状态位）。
- OpenCode/OpenClaw/Hermes 不参与。

## 8. 人工冒烟（待工具链就绪后，§7）
1. 正向首弹+记住：关 Codex 路由 → 启用 DeepSeek → 弹窗 → 勾记住确认 → 再启用另一需路由 provider 不再弹。
2. 反向 opt-in：开「自动关闭」→ 路由开下点官方 → 从「已拦截」变可点 → 弹窗 → 确认关+切。
3. 默认安全：全新状态，官方在路由模式仍「已拦截」。
4. 回归：删除 Provider 弹窗照常（ConfirmDialog 未传 checkbox）。
5. 持久化：开关改动后重启 app 保持。
6. 徽章扩面：Claude+Copilot / 完整URL provider 新显示「需要路由」。
